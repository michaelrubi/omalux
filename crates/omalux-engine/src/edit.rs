//! A frame's edit while its shoot is open: its settings, and the steps to
//! undo and redo. The history lives in memory for the session; what's
//! saved is the settings, in the sidecar ([`crate::sidecar`]).
//!
//! Each frame has its own steps, but a paste reaches several frames at
//! once and is one step to undo: its step on each frame carries the
//! paste's number, so whoever undoes it on one can undo it on the rest.

use lightcraft_develop::{DevelopSettings, WbMode, controls};
use lightcraft_pipeline::SourceInfo;
use lightcraft_raster::Rgb32f;

/// One frame's settings and how they got there.
#[derive(Clone, Debug, PartialEq)]
pub struct History {
    /// A fresh raw's settings, white balance as shot: what a frame opens
    /// with before it's edited, and what Reset goes back to.
    fresh: DevelopSettings,
    settings: DevelopSettings,
    /// The settings before the change under way (a drag), while one is.
    before: Option<DevelopSettings>,
    undo: Vec<Step>,
    redo: Vec<Step>,
}

/// The settings a step goes back (or forward) to, and the paste it was
/// part of, if it was.
type Step = (DevelopSettings, Option<u64>);

/// A white balance as shot shows the frame's own Kelvin and tint, whatever
/// frame it was pasted from, and whatever stood in for them before the
/// raw was decoded.
fn own_as_shot(settings: &mut DevelopSettings, fresh: &DevelopSettings) {
    if settings.wb.mode == WbMode::AsShot {
        (settings.wb.temp, settings.wb.tint) = (fresh.wb.temp, fresh.wb.tint);
    }
}

impl History {
    /// A frame opened: with the edit its sidecar held, or as a fresh raw.
    pub fn new(fresh: DevelopSettings, saved: Option<DevelopSettings>) -> Self {
        let mut settings = saved.unwrap_or_else(|| fresh.clone());
        own_as_shot(&mut settings, &fresh);
        Self { fresh, settings, before: None, undo: Vec::new(), redo: Vec::new() }
    }

    /// A frame pasted onto before its raw has been decoded: what its white
    /// balance was as shot isn't known until [`Self::decoded`], which
    /// changes nothing about how it renders (as shot is read from the
    /// raw), only the numbers shown.
    pub fn unseen(saved: Option<DevelopSettings>) -> Self {
        let wb = DevelopSettings::default().wb;
        Self::new(DevelopSettings::for_raw(wb.temp, wb.tint), saved)
    }

    /// The raw of a frame begun [`Self::unseen`] has been decoded: these
    /// are its fresh settings.
    pub fn decoded(&mut self, fresh: DevelopSettings) {
        let steps = self.undo.iter_mut().chain(&mut self.redo).map(|(settings, _)| settings);
        for settings in steps.chain(&mut self.before).chain([&mut self.settings]) {
            own_as_shot(settings, &fresh);
        }
        self.fresh = fresh;
    }

    pub fn settings(&self) -> &DevelopSettings {
        &self.settings
    }

    pub fn fresh(&self) -> &DevelopSettings {
        &self.fresh
    }

    /// Whether the frame has been changed from a fresh raw's settings.
    pub fn edited(&self) -> bool {
        self.settings != self.fresh
    }

    /// Change the settings. Every change until [`Self::commit`] is one step
    /// to undo, so a drag's many small ones are undone together.
    pub fn change(&mut self, change: impl FnOnce(&mut DevelopSettings)) {
        if self.before.is_none() {
            self.before = Some(self.settings.clone());
        }
        change(&mut self.settings);
    }

    /// Whether a change is under way: a drag that hasn't ended.
    pub fn changing(&self) -> bool {
        self.before.is_some()
    }

    /// End the change under way. True if it left the settings different
    /// from what they were before it: a step was made, to save and to undo.
    pub fn commit(&mut self) -> bool {
        match self.before.take() {
            Some(before) if before != self.settings => {
                self.undo.push((before, None));
                self.redo.clear();
                true
            }
            _ => false,
        }
    }

    /// Paste `pasted` over the settings, as a step of paste number
    /// `paste`. It's a step even if it changed nothing here, so the frame
    /// a paste was made from can undo it for the frames it reached. True
    /// if it changed the settings.
    pub fn paste(&mut self, paste: u64, pasted: DevelopSettings) -> bool {
        self.commit();
        let before = std::mem::replace(&mut self.settings, pasted);
        let changed = before != self.settings;
        self.undo.push((before, Some(paste)));
        self.redo.clear();
        changed
    }

    /// The paste the next undo would take back, if it's one.
    pub fn undoes(&self) -> Option<u64> {
        if self.before.as_ref().is_some_and(|before| *before != self.settings) {
            return None;
        }
        self.undo.last().and_then(|(_, paste)| *paste)
    }

    /// The paste the next redo would make again, if it's one.
    pub fn redoes(&self) -> Option<u64> {
        if self.before.as_ref().is_some_and(|before| *before != self.settings) {
            return None;
        }
        self.redo.last().and_then(|(_, paste)| *paste)
    }

    /// Change the settings in one step.
    pub fn set(&mut self, change: impl FnOnce(&mut DevelopSettings)) -> bool {
        self.commit();
        self.change(change);
        self.commit()
    }

    /// Take back the last step. False if there was none.
    pub fn undo(&mut self) -> bool {
        self.commit();
        let Some((was, paste)) = self.undo.pop() else { return false };
        self.redo.push((std::mem::replace(&mut self.settings, was), paste));
        true
    }

    pub fn redo(&mut self) -> bool {
        self.commit();
        let Some((next, paste)) = self.redo.pop() else { return false };
        self.undo.push((std::mem::replace(&mut self.settings, next), paste));
        true
    }

    /// Back to a fresh raw's settings, as a step that can be undone. False
    /// if the frame had no edit.
    pub fn reset(&mut self) -> bool {
        let fresh = self.fresh.clone();
        self.set(|settings| *settings = fresh)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
}

/// Write one control by its id (`light.exposure`, `wb.temp`…), clamped to
/// its range. Temp and Tint make the white balance Custom, starting from
/// the as-shot values and on the scale the raw is edited on (Kelvin, for
/// the ILCE-7M3). As Lightcraft's `develop.set` does it
/// (`engine/src/cmd/develop.rs` at `701f78e`). False for an id that isn't
/// a control.
pub fn set_control(settings: &mut DevelopSettings, info: &SourceInfo, id: &str, value: f64) -> bool {
    let white_balance = id == "wb.temp" || id == "wb.tint";
    if white_balance {
        lightcraft_pipeline::local::normalize_wb(settings, info);
        if settings.wb.mode == WbMode::AsShot {
            (settings.wb.temp, settings.wb.tint) = (info.as_shot_temp, info.as_shot_tint);
        }
    }
    let known = controls::set(settings, id, value);
    if known && white_balance {
        settings.wb.mode = WbMode::Custom;
    }
    known
}

/// White balance as the camera set it.
pub fn as_shot(settings: &mut DevelopSettings, info: &SourceInfo) {
    lightcraft_pipeline::local::normalize_wb(settings, info);
    (settings.wb.mode, settings.wb.temp, settings.wb.tint) = (WbMode::AsShot, info.as_shot_temp, info.as_shot_tint);
}

/// The white balance that makes the source's pixels round a point
/// neutral: Lightroom's White Balance Selector. `at` is the point as a
/// share of the frame's width and height. As Lightcraft's `develop.wbPick`
/// does it (`engine/src/cmd/develop.rs` at `701f78e`): the mean of five
/// pixels by five, through its auto white balance.
pub fn pick_white_balance(settings: &mut DevelopSettings, info: &SourceInfo, source: &Rgb32f, at: [f64; 2]) {
    // Turned as the frame is shown, which for a frame that hasn't been
    // turned is as it is.
    let turned;
    let oriented = if settings.orientation == Default::default() {
        source
    } else {
        turned = source.oriented(settings.orientation);
        &turned
    };
    let centre = |share: f64, size: usize| (share.clamp(0.0, 1.0) * size as f64) as isize;
    let (x, y) = (centre(at[0], oriented.width), centre(at[1], oriented.height));
    let mut mean = [0.0f32; 3];
    for dy in -2..=2 {
        for dx in -2..=2 {
            let pixel = oriented.get_clamped(x + dx, y + dy);
            for (sum, channel) in mean.iter_mut().zip(pixel) {
                *sum += channel / 25.0;
            }
        }
    }
    let (temp, tint) = lightcraft_pipeline::auto::auto_wb(&Rgb32f::filled(4, 4, mean), info);
    lightcraft_pipeline::local::normalize_wb(settings, info);
    (settings.wb.mode, settings.wb.temp, settings.wb.tint) = (WbMode::Custom, temp, tint);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_selector_makes_what_was_clicked_neutral() {
        let info = SourceInfo::default();
        // A grey frame with a warm cast in its right half.
        let mut source = Rgb32f::filled(40, 20, [0.2, 0.2, 0.2]);
        for y in 0..20 {
            for x in 20..40 {
                source.data[y * 40 + x] = [0.3, 0.2, 0.1];
            }
        }
        let mut neutral = DevelopSettings::default();
        pick_white_balance(&mut neutral, &info, &source, [0.25, 0.5]);
        let mut warm = DevelopSettings::default();
        pick_white_balance(&mut warm, &info, &source, [0.75, 0.5]);
        assert_eq!(neutral.wb.mode, WbMode::Custom);
        assert!((neutral.wb.temp - 6500.0).abs() < 100.0 && neutral.wb.tint.abs() < 2.0, "grey is neutral already: {:?}", neutral.wb);
        assert!(warm.wb.temp < neutral.wb.temp - 500.0, "a warm cast is cooled: {:?}", warm.wb);
    }

    #[test]
    fn temp_and_tint_make_the_white_balance_custom_from_as_shot() {
        let info = SourceInfo { raw: true, as_shot_temp: 5200.0, as_shot_tint: 3.0, ..SourceInfo::default() };
        let mut s = DevelopSettings::default();
        assert!(set_control(&mut s, &info, "wb.tint", 10.0));
        assert_eq!((s.wb.mode, s.wb.temp, s.wb.tint, s.wb.scale), (WbMode::Custom, 5200.0, 10.0, lightcraft_develop::WbScale::Kelvin));
        assert!(set_control(&mut s, &info, "light.exposure", 9.0) && s.light.exposure == 5.0, "clamped to its range");
        assert!(!set_control(&mut s, &info, "light.nothing", 1.0));
        as_shot(&mut s, &info);
        assert_eq!((s.wb.mode, s.wb.temp, s.wb.tint), (WbMode::AsShot, 5200.0, 3.0));
    }

    fn history() -> History {
        History::new(DevelopSettings::for_raw(5200.0, 3.0), None)
    }

    #[test]
    fn a_drag_is_one_step_and_undo_and_redo_walk_the_steps() {
        let mut h = history();
        assert!(!h.edited() && !h.can_undo());
        for exposure in [0.1, 0.2, 0.3] {
            h.change(|s| s.light.exposure = exposure);
            assert!(h.changing());
        }
        assert!(h.commit() && !h.changing());
        assert!(h.set(|s| s.light.contrast = 20.0));
        assert_eq!((h.settings().light.exposure, h.settings().light.contrast), (0.3, 20.0));

        assert!(h.undo());
        assert_eq!((h.settings().light.exposure, h.settings().light.contrast), (0.3, 0.0));
        assert!(h.undo() && !h.edited(), "the whole drag at once");
        assert!(!h.undo());
        assert!(h.redo() && h.redo() && !h.redo());
        assert_eq!((h.settings().light.exposure, h.settings().light.contrast), (0.3, 20.0));

        // A new step forgets what was undone.
        h.undo();
        h.set(|s| s.light.shadows = 10.0);
        assert!(!h.can_redo());
    }

    #[test]
    fn a_change_that_changes_nothing_is_no_step() {
        let mut h = history();
        h.change(|s| s.light.exposure = 0.5);
        h.change(|s| s.light.exposure = 0.0);
        assert!(!h.commit() && !h.can_undo());
        assert!(!h.set(|s| s.light.exposure = 0.0));
        assert!(!h.reset(), "nothing to reset");
    }

    #[test]
    fn reset_goes_back_to_a_fresh_raw_and_can_be_undone() {
        let mut saved = DevelopSettings::for_raw(5200.0, 3.0);
        saved.light.exposure = 1.0;
        let mut h = History::new(DevelopSettings::for_raw(5200.0, 3.0), Some(saved.clone()));
        assert!(h.edited() && !h.can_undo(), "the saved edit is where the session starts");
        assert!(h.reset());
        assert_eq!(h.settings(), h.fresh());
        assert!(h.undo());
        assert_eq!(h.settings(), &saved);
    }

    #[test]
    fn undoing_in_the_middle_of_a_drag_ends_it_first() {
        let mut h = history();
        h.change(|s| s.light.exposure = 0.5);
        assert!(h.undo() && !h.edited());
        assert!(h.redo());
        assert_eq!(h.settings().light.exposure, 0.5);
    }

    #[test]
    fn a_paste_is_a_step_that_knows_its_paste_even_where_it_changed_nothing() {
        let mut h = history();
        h.set(|s| s.light.exposure = 0.5);
        assert_eq!(h.undoes(), None);
        let mut pasted = h.settings().clone();
        pasted.light.contrast = 20.0;
        assert!(h.paste(7, pasted.clone()));
        assert_eq!((h.undoes(), h.redoes()), (Some(7), None));
        assert!(h.undo());
        assert_eq!((h.settings().light.contrast, h.undoes(), h.redoes()), (0.0, None, Some(7)));
        assert!(h.redo());
        assert_eq!((h.settings(), h.undoes()), (&pasted, Some(7)));

        // Onto the frame it was copied from: nothing changes, and it's a step all the same.
        assert!(!h.paste(8, pasted.clone()));
        assert_eq!(h.undoes(), Some(8));
        assert!(h.undo());
        assert_eq!((h.settings(), h.undoes()), (&pasted, Some(7)));

        // An edit since is what's undone first.
        h.set(|s| s.light.shadows = 10.0);
        assert_eq!((h.undoes(), h.redoes()), (None, None));
        h.change(|s| s.light.shadows = 20.0);
        assert_eq!(h.undoes(), None, "so is a drag under way");
    }

    #[test]
    fn a_frame_pasted_onto_unseen_gets_its_own_as_shot_once_decoded() {
        let mut h = History::unseen(None);
        assert!(!h.edited());
        let mut pasted = h.settings().clone();
        pasted.light.exposure = 1.0;
        assert!(h.paste(1, pasted));
        h.decoded(DevelopSettings::for_raw(5200.0, 3.0));
        assert_eq!((h.settings().wb.temp, h.settings().wb.tint, h.settings().light.exposure), (5200.0, 3.0, 1.0));
        assert!(h.undo());
        assert!(!h.edited(), "what it was before the paste is a fresh raw");
        assert_eq!(h.settings(), &DevelopSettings::for_raw(5200.0, 3.0));

        // A custom white balance is its own, and stays.
        let mut custom = DevelopSettings::for_raw(0.0, 0.0);
        (custom.wb.mode, custom.wb.temp) = (WbMode::Custom, 4000.0);
        let mut h = History::unseen(Some(custom.clone()));
        h.decoded(DevelopSettings::for_raw(5200.0, 3.0));
        assert_eq!(h.settings(), &custom);

        // An edit saved as shot with another frame's numbers opens with its own.
        let h = History::new(DevelopSettings::for_raw(5200.0, 3.0), Some(DevelopSettings::for_raw(6100.0, -2.0)));
        assert!(!h.edited());
    }
}

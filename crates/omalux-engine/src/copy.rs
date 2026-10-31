//! Copy and paste: the groups of settings a copy can take, and what was
//! copied. What's chosen at copy time is what's pasted; there's no second
//! choice at paste time (see "Copy and paste" in DESIGN.md).
//!
//! The groups are Lightroom's Copy Settings checklist, cut down to the
//! panels OmaLux has; a milestone that adds a panel adds its group. The
//! partial settings and their merge are Lightcraft's
//! (`lightcraft_develop::presets`), whose own groups are cut differently
//! (its Light takes its own HDR with it, its Detail both sharpening and
//! noise reduction). A frame's quarter turns are in no group: they're the way
//! up it is, which another frame doesn't want, as in Lightroom.

use lightcraft_develop::{DevelopSettings, WbMode, apply_partial};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// A group of settings copied together: a panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Group {
    WhiteBalance,
    /// Camera match or Flat.
    Profile,
    Light,
    Hdr,
    Presence,
    Sharpening,
    NoiseReduction,
    LensCorrections,
    /// Upright and the Transform sliders.
    Transform,
    Crop,
    /// Sensor dust's heal circles ([`crate::dust`]).
    SpotRemoval,
}

/// Part of `DevelopSettings`' JSON: a top-level key, and the fields under
/// it, or none for all of them.
type Part = (&'static str, &'static [&'static str]);

impl Group {
    /// Every group, in the panels' order: what Ctrl+C copies.
    pub const ALL: &[Group] = &[
        Group::WhiteBalance,
        Group::Profile,
        Group::Light,
        Group::Hdr,
        Group::Presence,
        Group::Sharpening,
        Group::NoiseReduction,
        Group::LensCorrections,
        Group::Transform,
        Group::Crop,
        Group::SpotRemoval,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Group::WhiteBalance => "White Balance",
            Group::Profile => "Profile",
            Group::Light => "Light",
            Group::Hdr => "HDR",
            Group::Presence => "Presence",
            Group::Sharpening => "Sharpening",
            Group::NoiseReduction => "Noise Reduction",
            Group::LensCorrections => "Lens Corrections",
            Group::Transform => "Transform",
            Group::Crop => "Crop",
            Group::SpotRemoval => "Spot Removal",
        }
    }

    /// The parts of `DevelopSettings`' JSON that are the group's.
    fn parts(self) -> &'static [Part] {
        match self {
            Group::WhiteBalance => &[("wb", &[])],
            Group::Profile => &[("profile", &[])],
            Group::Light => &[("light", &["exposure", "contrast", "highlights", "shadows", "whites", "blacks"])],
            Group::Hdr => &[("light", &["hdr", "hdr_highlights", "hdr_shadows"])],
            Group::Presence => &[("effects", &[]), ("color", &[])],
            Group::Sharpening => &[("detail", &["sharpen_amount", "sharpen_radius", "sharpen_detail", "sharpen_masking"])],
            Group::NoiseReduction => {
                &[("detail", &["nr_luminance", "nr_detail", "nr_contrast", "nr_color", "nr_color_detail", "nr_color_smoothness"])]
            }
            Group::LensCorrections => &[("optics", &[])],
            Group::Transform => &[("geometry", &[])],
            Group::Crop => &[("crop", &[])],
            Group::SpotRemoval => &[("spots", &[])],
        }
    }
}

/// Settings copied from a frame: the groups chosen, as they stood.
#[derive(Clone, Debug, PartialEq)]
pub struct Copied {
    groups: Vec<Group>,
    partial: Value,
    /// The way up of the frame it was copied from, where its spots are.
    way: crate::dust::Way,
}

impl Copied {
    /// Copy `groups` of `settings`.
    pub fn new(settings: &DevelopSettings, groups: &[Group]) -> Self {
        let full = settings.to_json();
        let mut partial = Map::new();
        for (key, fields) in groups.iter().flat_map(|group| group.parts()) {
            let Some(value) = full.get(*key) else { continue };
            if fields.is_empty() {
                partial.insert((*key).to_owned(), value.clone());
                continue;
            }
            // Some of its fields: two groups can share a key.
            let Value::Object(part) = partial.entry(*key).or_insert_with(|| Value::Object(Map::new())) else { continue };
            for field in *fields {
                // A field that isn't written is one left out at 0 (HDR):
                // copied as that, so pasting it takes the other frame's away.
                part.insert((*field).to_owned(), value.get(*field).cloned().unwrap_or_else(|| 0.0.into()));
            }
        }
        Self { groups: groups.to_vec(), partial: Value::Object(partial), way: crate::dust::Way { turned: settings.orientation, ..Default::default() } }
    }

    /// The frame it's copied from was held `held` (its raw's Exif
    /// orientation): its spots are carried to where its dust is on each
    /// frame they're pasted onto ([`Self::paste_held`]).
    pub fn held(mut self, held: lightcraft_geom::Orientation) -> Self {
        self.way.held = held;
        self
    }

    pub fn groups(&self) -> &[Group] {
        &self.groups
    }

    /// `onto` with what was copied. `fresh` is the frame's own fresh
    /// settings: a white balance copied as shot is pasted as the frame's
    /// own as shot, not the Kelvin the other frame was shot at.
    pub fn paste(&self, onto: &DevelopSettings, fresh: &DevelopSettings) -> DevelopSettings {
        self.paste_held(onto, fresh, None)
    }

    /// [`Self::paste`] onto a frame held `held`: spots copied are carried
    /// to where the same dust is on it.
    pub fn paste_held(&self, onto: &DevelopSettings, fresh: &DevelopSettings, held: Option<lightcraft_geom::Orientation>) -> DevelopSettings {
        let mut pasted = apply_partial(onto, &self.partial, 1.0);
        if let Some(held) = held.filter(|_| self.groups.contains(&Group::SpotRemoval)) {
            pasted.spots = crate::dust::carry(&pasted.spots, self.way, crate::dust::Way { held, turned: pasted.orientation });
        }
        if pasted.wb.mode == WbMode::AsShot {
            (pasted.wb.temp, pasted.wb.tint) = (fresh.wb.temp, fresh.wb.tint);
        }
        // An automatic Upright's transform was found on the frame it was
        // copied from: this one's is its own ([`crate::edit::upright`]).
        if self.groups.contains(&Group::Transform) && crate::edit::upright_is_auto(pasted.geometry.upright) {
            pasted.geometry.upright_transform = None;
        }
        pasted
    }
}

/// What a change to one frame's settings touched, for Auto Sync to make
/// the same change on others: each setting that changed (a value of
/// `DevelopSettings`' JSON, or a list whole), at the value it was changed
/// to, as Lightroom's Auto Sync makes it. A frame's quarter turns are its
/// own, and never in it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Changes(Vec<(Vec<String>, Option<Value>)>);

/// The leaves of `after` that differ from `before`'s, at `path`; `None` for
/// one `after` doesn't have (left out at its default, as HDR is).
fn diff(path: &mut Vec<String>, before: Option<&Value>, after: Option<&Value>, out: &mut Vec<(Vec<String>, Option<Value>)>) {
    match (before, after) {
        (Some(Value::Object(b)), Some(Value::Object(a))) => {
            let keys: std::collections::BTreeSet<&String> = b.keys().chain(a.keys()).collect();
            for key in keys {
                path.push(key.clone());
                diff(path, b.get(key), a.get(key), out);
                path.pop();
            }
        }
        (b, a) if b != a => out.push((path.clone(), a.cloned())),
        _ => {}
    }
}

/// What changed from `before` to `after`.
pub fn changes(before: &DevelopSettings, after: &DevelopSettings) -> Changes {
    let mut out = Vec::new();
    diff(&mut Vec::new(), Some(&before.to_json()), Some(&after.to_json()), &mut out);
    out.retain(|(path, _)| !matches!(path.first().map(String::as_str), Some("orientation" | "version" | "process")));
    Changes(out)
}

impl Changes {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Whether it changed anything under `key` (a top-level one, as
    /// `spots`).
    pub fn touches(&self, key: &str) -> bool {
        self.0.iter().any(|(path, _)| path.first().is_some_and(|first| first == key))
    }

    /// `onto` with the same changes made. `fresh` is the frame's own fresh
    /// settings: a white balance changed to as shot is the frame's own,
    /// and an automatic Upright's transform is left to be found on it.
    pub fn apply(&self, onto: &DevelopSettings, fresh: &DevelopSettings) -> DevelopSettings {
        let mut json = onto.to_json();
        for (path, value) in &self.0 {
            let Some((last, parents)) = path.split_last() else { continue };
            let mut at = &mut json;
            for key in parents {
                let Value::Object(map) = at else { break };
                at = map.entry(key.clone()).or_insert_with(|| Value::Object(Map::new()));
            }
            if let Value::Object(map) = at {
                match value {
                    Some(value) => _ = map.insert(last.clone(), value.clone()),
                    None => _ = map.remove(last),
                }
            }
        }
        let mut changed = DevelopSettings::from_json(&json).unwrap_or_else(|_| onto.clone());
        if changed.wb.mode == WbMode::AsShot {
            (changed.wb.temp, changed.wb.tint) = (fresh.wb.temp, fresh.wb.tint);
        }
        let transform = self.0.iter().any(|(path, _)| path.iter().map(String::as_str).eq(["geometry", "upright_transform"]));
        if transform && crate::edit::upright_is_auto(changed.geometry.upright) {
            changed.geometry.upright_transform = None;
        }
        changed
    }
}

/// `settings` with `groups` switched off, as a panel's switch shows it:
/// each group as a fresh raw would have it with nothing applied (no
/// sharpening, no lens corrections), but the white balance as shot.
pub fn switched_off(settings: &DevelopSettings, fresh: &DevelopSettings, groups: &[Group]) -> DevelopSettings {
    let off = DevelopSettings { wb: fresh.wb, ..DevelopSettings::default() };
    Copied::new(&off, groups).paste(settings, fresh)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lightcraft_develop::WbScale;

    fn edited() -> DevelopSettings {
        let mut s = DevelopSettings::for_raw(5200.0, 3.0);
        (s.wb.mode, s.wb.temp, s.wb.tint, s.wb.scale) = (WbMode::Custom, 4800.0, 12.0, WbScale::Kelvin);
        (s.light.exposure, s.light.shadows) = (0.7, 30.0);
        s.color.vibrance = 15.0;
        s
    }

    #[test]
    fn everything_copied_is_everything_pasted() {
        let fresh = DevelopSettings::for_raw(6100.0, -2.0);
        let pasted = Copied::new(&edited(), Group::ALL).paste(&fresh, &fresh);
        assert_eq!(pasted, edited(), "two fresh raws differ only in their white balance as shot");
    }

    #[test]
    fn only_the_groups_chosen_are_pasted() {
        let fresh = DevelopSettings::for_raw(6100.0, -2.0);
        let mut onto = fresh.clone();
        (onto.light.exposure, onto.light.contrast, onto.color.saturation) = (-1.0, 20.0, -10.0);
        let copied = Copied::new(&edited(), &[Group::Light]);
        assert_eq!(copied.groups(), [Group::Light]);
        let pasted = copied.paste(&onto, &fresh);
        assert_eq!((pasted.light.exposure, pasted.light.shadows, pasted.light.contrast), (0.7, 30.0, 0.0), "the whole group");
        assert_eq!((pasted.wb, pasted.color), (onto.wb, onto.color), "and nothing else");

        let nothing = Copied::new(&edited(), &[]);
        assert_eq!(nothing.paste(&onto, &fresh), onto);
    }

    #[test]
    fn presence_takes_texture_clarity_and_dehaze_with_vibrance_and_saturation() {
        let fresh = DevelopSettings::for_raw(6100.0, -2.0);
        let mut from = fresh.clone();
        (from.effects.texture, from.effects.clarity, from.effects.dehaze, from.color.vibrance) = (10.0, 20.0, 30.0, 15.0);
        let pasted = Copied::new(&from, &[Group::Presence]).paste(&fresh, &fresh);
        assert_eq!((pasted.effects, pasted.color), (from.effects, from.color));
        let pasted = Copied::new(&from, &[Group::Light]).paste(&fresh, &fresh);
        assert_eq!(pasted.effects, fresh.effects, "nor in any other group");
        // A frame with none pasted takes them away.
        let pasted = Copied::new(&fresh, &[Group::Presence]).paste(&from, &fresh);
        assert_eq!(pasted.effects, fresh.effects);
        assert_eq!(switched_off(&from, &fresh, &[Group::Presence]).effects, Default::default());
    }

    #[test]
    fn transform_takes_upright_and_its_sliders_but_not_another_frames_analysis() {
        use lightcraft_develop::Upright;
        let fresh = DevelopSettings::for_raw(6100.0, -2.0);
        let mut from = fresh.clone();
        (from.geometry.upright, from.geometry.vertical, from.geometry.constrain_crop) = (Upright::Vertical, -20.0, true);
        from.geometry.upright_transform = Some(lightcraft_geom::Homography::IDENTITY);
        let pasted = Copied::new(&from, &[Group::Transform]).paste(&fresh, &fresh);
        assert_eq!((pasted.geometry.upright, pasted.geometry.vertical, pasted.geometry.constrain_crop), (Upright::Vertical, -20.0, true));
        assert_eq!(pasted.geometry.upright_transform, None, "found again on the frame it's pasted onto");
        // Guided takes its lines, which are where the frame's uprights are.
        from.geometry.upright = Upright::Guided;
        from.geometry.guides = vec![(lightcraft_geom::Point::new(0.1, 0.1), lightcraft_geom::Point::new(0.1, 0.9))];
        let pasted = Copied::new(&from, &[Group::Transform]).paste(&fresh, &fresh);
        assert_eq!(pasted.geometry.guides, from.geometry.guides);
        assert_eq!(Copied::new(&from, &[Group::Crop]).paste(&fresh, &fresh).geometry, fresh.geometry, "nor in another group");
        assert_eq!(switched_off(&from, &fresh, &[Group::Transform]).geometry, Default::default());
    }

    #[test]
    fn a_change_is_made_on_another_frame_as_the_values_touched_and_nothing_else() {
        use lightcraft_develop::Upright;
        let fresh = DevelopSettings::for_raw(5200.0, 3.0);
        let mut before = fresh.clone();
        (before.light.exposure, before.light.contrast) = (0.5, 10.0);
        let mut other_fresh = DevelopSettings::for_raw(6100.0, -2.0);
        other_fresh.light.contrast = -30.0;
        let other = other_fresh.clone();

        // Exposure moved to 0.7: 0.7 on the other frame, its own contrast kept.
        let mut after = before.clone();
        after.light.exposure = 0.7;
        let changed = changes(&before, &after);
        assert!(!changed.is_empty());
        let made = changed.apply(&other, &other_fresh);
        assert_eq!((made.light.exposure, made.light.contrast), (0.7, -30.0));
        assert_eq!(changes(&other, &made), Changes(vec![(vec!["light".into(), "exposure".into()], Some(0.7.into()))]), "and nothing else");

        // HDR on, and off again: it's left out at its default.
        let mut hdr = before.clone();
        hdr.light.hdr = 40.0;
        let on = changes(&before, &hdr).apply(&other, &other_fresh);
        assert_eq!(on.light.hdr, 40.0);
        assert_eq!(changes(&hdr, &before).apply(&on, &other_fresh).light.hdr, 0.0);

        // As shot is each frame's own; a turn is no change; an automatic Upright is found again.
        let mut custom = before.clone();
        (custom.wb.mode, custom.wb.temp) = (WbMode::Custom, 7000.0);
        let mut as_shot = custom.clone();
        (as_shot.wb.mode, as_shot.wb.temp, as_shot.wb.tint) = (WbMode::AsShot, 5200.0, 3.0);
        let made = changes(&custom, &as_shot).apply(&changes(&before, &custom).apply(&other, &other_fresh), &other_fresh);
        assert_eq!((made.wb.mode, made.wb.temp, made.wb.tint), (WbMode::AsShot, 6100.0, -2.0));
        let mut turned = before.clone();
        turned.orientation = turned.orientation.rotated(true);
        assert!(changes(&before, &turned).is_empty());
        let mut upright = before.clone();
        (upright.geometry.upright, upright.geometry.upright_transform) = (Upright::Auto, Some(lightcraft_geom::Homography::IDENTITY));
        let made = changes(&before, &upright).apply(&other, &other_fresh);
        assert_eq!((made.geometry.upright, made.geometry.upright_transform), (Upright::Auto, None));
    }

    #[test]
    fn spots_pasted_onto_a_frame_held_another_way_are_where_its_dust_is() {
        use lightcraft_geom::{Orientation, Point};
        let fresh = DevelopSettings::for_raw(5200.0, 3.0);
        let mut from = fresh.clone();
        from.spots = vec![crate::dust::heal(Point::new(0.2, 0.1), 0.01)];
        let copied = Copied::new(&from, &[Group::SpotRemoval]).held(Orientation::Normal);
        let level = copied.paste_held(&fresh, &fresh, Some(Orientation::Normal));
        assert_eq!(level.spots, from.spots);
        let portrait = copied.paste_held(&fresh, &fresh, Some(Orientation::Rotate90));
        let at = portrait.spots[0].points[0];
        assert!((at.x - 0.9).abs() < 1e-9 && (at.y - 0.2).abs() < 1e-9, "{at:?}");
        assert_eq!(Copied::new(&from, &[Group::Crop]).paste_held(&fresh, &fresh, Some(Orientation::Rotate90)).spots, fresh.spots, "nor in another group");
        assert!(changes(&fresh, &from).touches("spots") && !changes(&fresh, &from).touches("crop"));
    }

    #[test]
    fn a_panel_switched_off_is_as_a_fresh_raw_with_nothing_applied() {
        let fresh = crate::look::fresh(5200.0, 3.0);
        let mut settings = fresh.clone();
        (settings.wb.temp, settings.wb.mode) = (7000.0, WbMode::Custom);
        (settings.light.exposure, settings.light.hdr, settings.light.hdr_shadows) = (0.7, 40.0, 20.0);
        settings.color.vibrance = 30.0;
        let off = switched_off(&settings, &fresh, &[Group::WhiteBalance, Group::Hdr, Group::Sharpening, Group::LensCorrections]);
        assert_eq!((off.wb.temp, off.wb.mode), (5200.0, WbMode::AsShot));
        assert_eq!((off.light.exposure, off.light.hdr, off.light.hdr_shadows), (0.7, 0.0, 0.0));
        assert_eq!((off.detail.sharpen_amount, off.optics.lens_profile, off.color.vibrance), (0.0, false, 30.0));
        assert_eq!(switched_off(&settings, &fresh, &[]), settings);
    }

    #[test]
        fn the_profile_and_hdr_are_groups_of_their_own() {
        let fresh = crate::look::fresh(6100.0, -2.0);
        let mut from = fresh.clone();
        crate::look::Profile::Flat.set(&mut from);
        (from.light.hdr, from.light.exposure) = (40.0, 0.7);
        let pasted = Copied::new(&from, &[Group::Profile]).paste(&fresh, &fresh);
        assert_eq!((crate::look::Profile::of(&pasted), pasted.light), (crate::look::Profile::Flat, fresh.light));
        let pasted = Copied::new(&from, &[Group::Hdr]).paste(&fresh, &fresh);
        assert_eq!((pasted.light.hdr, pasted.light.exposure, pasted.profile.clone()), (40.0, 0.0, fresh.profile.clone()));
        let pasted = Copied::new(&from, &[Group::Light]).paste(&fresh, &fresh);
        assert_eq!((pasted.light.hdr, pasted.light.exposure), (0.0, 0.7), "Light without HDR");
        // HDR at 0 is copied as 0, though it isn't written at 0.
        let pasted = Copied::new(&fresh, &[Group::Hdr]).paste(&from, &fresh);
        assert_eq!((pasted.light.hdr, pasted.light.exposure), (0.0, 0.7));
        let pasted = Copied::new(&from, Group::ALL).paste(&fresh, &fresh);
        assert_eq!(pasted, from);
    }

    #[test]
    fn sharpening_and_noise_reduction_are_groups_of_their_own() {
        let fresh = DevelopSettings::for_raw(6100.0, -2.0);
        let mut from = edited();
        (from.detail.sharpen_amount, from.detail.sharpen_masking, from.detail.nr_luminance, from.detail.nr_color) = (80.0, 30.0, 20.0, 40.0);
        let pasted = Copied::new(&from, &[Group::Sharpening]).paste(&fresh, &fresh);
        assert_eq!((pasted.detail.sharpen_amount, pasted.detail.sharpen_masking), (80.0, 30.0));
        assert_eq!((pasted.detail.nr_luminance, pasted.detail.nr_color), (fresh.detail.nr_luminance, fresh.detail.nr_color), "not the noise reduction");
        let pasted = Copied::new(&from, &[Group::NoiseReduction]).paste(&fresh, &fresh);
        assert_eq!((pasted.detail.nr_luminance, pasted.detail.nr_color, pasted.detail.sharpen_amount), (20.0, 40.0, fresh.detail.sharpen_amount));
        let pasted = Copied::new(&from, &[Group::Sharpening, Group::NoiseReduction]).paste(&fresh, &fresh);
        assert_eq!(pasted.detail, from.detail, "both are all of Detail");
    }

    #[test]
    fn lens_corrections_and_the_crop_are_pasted_and_a_quarter_turn_never_is() {
        let fresh = DevelopSettings::for_raw(6100.0, -2.0);
        let mut from = edited();
        (from.optics.remove_ca, from.optics.defringe_purple_amount) = (true, 6.0);
        crate::crop::set_aspect(&mut from, (6000.0, 4000.0), crate::crop::Aspect::Ratio(1, 1));
        crate::crop::straighten(&mut from, (6000.0, 4000.0), 3.0);
        crate::crop::turn(&mut from, true);
        let pasted = Copied::new(&from, &[Group::LensCorrections]).paste(&fresh, &fresh);
        assert_eq!((pasted.optics, pasted.crop), (from.optics, fresh.crop));
        let pasted = Copied::new(&from, Group::ALL).paste(&fresh, &fresh);
        assert_eq!((pasted.crop, pasted.orientation), (from.crop, fresh.orientation));

        // No crop copied is no crop pasted: the frame's own is cleared.
        let pasted = Copied::new(&fresh, &[Group::Crop]).paste(&from, &fresh);
        assert_eq!(pasted.crop, fresh.crop);
    }

    #[test]
    fn as_shot_is_pasted_as_the_frames_own() {
        let fresh = DevelopSettings::for_raw(6100.0, -2.0);
        let mut onto = fresh.clone();
        (onto.wb.mode, onto.wb.temp, onto.wb.tint, onto.wb.scale) = (WbMode::Custom, 4000.0, 20.0, WbScale::Kelvin);
        let copied = Copied::new(&DevelopSettings::for_raw(5200.0, 3.0), &[Group::WhiteBalance]);
        let pasted = copied.paste(&onto, &fresh);
        assert_eq!((pasted.wb.mode, pasted.wb.temp, pasted.wb.tint), (WbMode::AsShot, 6100.0, -2.0));

        // A custom white balance is pasted as it is, Kelvin and all.
        let pasted = Copied::new(&edited(), &[Group::WhiteBalance]).paste(&fresh, &fresh);
        assert_eq!(pasted.wb, edited().wb);
    }
}

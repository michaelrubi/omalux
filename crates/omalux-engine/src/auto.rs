//! Auto: white balance, exposure, tone and level, each a button, and any
//! one slider by Shift+double-click, as in Lightroom. Nothing here runs
//! on its own (see "Auto is a button" in DESIGN.md).
//!
//! White balance, exposure and level are Lightcraft's
//! (`lightcraft_pipeline::auto` and `upright`), as are the values a single
//! slider gets, all but two: Lightcraft's Auto leaves Whites and Blacks at
//! 0, so Auto tone is OmaLux's own. It's what DESIGN.md asks of it, and
//! no guess at Lightroom's: Whites brought down from 0, and Blacks up,
//! as far as it takes for nothing to clip that needn't, found by
//! rendering the frame small and counting. Highlights the sensor itself
//! ran out on, and shadows that are black wherever Blacks is, can't be
//! helped and are left; a frame where nothing else clips gets 0 and 0.
//! Neither is pushed the other way to fill the histogram: on Lightcraft's
//! pipeline Whites at +100 and Blacks at -100 move the ends very little
//! (measured on Michael's frames), so that would be large numbers for
//! nothing.

use std::sync::Arc;

use lightcraft_develop::{DevelopSettings, WbMode};
use lightcraft_pipeline::{RenderRequest, SourceInfo, StageCache};
use lightcraft_raster::Rgb32f;

use crate::look::{self, Camera};
use crate::{crop, edit};

/// Something to set automatically.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Auto {
    /// Temp and Tint, from the frame's own greys.
    WhiteBalance,
    Exposure,
    /// Whites and Blacks, so nothing clips that needn't.
    Tone,
    /// The straighten angle that levels the horizon, or makes the
    /// uprights plumb.
    Level,
    /// One slider, by its control's id.
    Control(&'static str),
}

/// The long edge Auto tone renders the frame at to count what clips.
const PROXY: usize = 256;
/// The share of the frame that may clip: specks don't move the sliders.
const SPECKS: f64 = 0.0005;

/// Set `what` in `settings` from the frame itself. False if there was
/// nothing to go by (no horizon to level, a slider with no automatic
/// value), and the settings are as they were.
///
/// The frame is looked at as it's rendered: with what its profile and
/// HDR put under the sliders ([`look::effective`]).
pub fn apply(settings: &mut DevelopSettings, info: &SourceInfo, camera: &Camera, source: &Arc<Rgb32f>, what: Auto) -> bool {
    let (rendered, facts) = look::effective(settings, info, camera);
    let (mut rendered, facts) = (rendered.into_owned(), &facts.into_owned());
    match what {
        Auto::WhiteBalance => {
            let (temp, tint) = lightcraft_pipeline::auto::auto_wb(source, info);
            lightcraft_pipeline::local::normalize_wb(settings, info);
            (settings.wb.mode, settings.wb.temp, settings.wb.tint) = (WbMode::Auto, temp, tint);
        }
        Auto::Exposure => settings.light.exposure = lightcraft_pipeline::auto::auto_tone(source, facts, &rendered).exposure,
        Auto::Tone => {
            settings.light.whites = whites(&rendered, facts, source);
            rendered.light.whites = settings.light.whites;
            settings.light.blacks = blacks(&rendered, facts, source);
        }
        Auto::Level => {
            let Some(angle) = lightcraft_pipeline::upright::level_degrees(source, info, settings) else { return false };
            crop::straighten(settings, crop::dims((source.width, source.height), settings), angle);
        }
        Auto::Control(id) => {
            let tone = || lightcraft_pipeline::auto::auto_tone(source, facts, &rendered);
            let value = match id {
                "wb.temp" => lightcraft_pipeline::auto::auto_wb(source, info).0,
                "wb.tint" => lightcraft_pipeline::auto::auto_wb(source, info).1,
                "light.exposure" => tone().exposure,
                "light.contrast" => tone().contrast,
                "light.highlights" => tone().highlights,
                "light.shadows" => tone().shadows,
                "light.whites" => whites(&rendered, facts, source),
                "light.blacks" => blacks(&rendered, facts, source),
                "color.vibrance" => tone().vibrance,
                "color.saturation" => tone().saturation,
                _ => return false,
            };
            return edit::set_control(settings, info, id, value);
        }
    }
    true
}

/// The frame small, to render with Whites and Blacks tried at one value
/// after another: whole and unturned, so its pixels are the render's.
struct Trials<'a> {
    settings: DevelopSettings,
    info: &'a SourceInfo,
    small: Arc<Rgb32f>,
    /// The pixels the sensor ran out on.
    saturated: Vec<bool>,
    stages: StageCache,
}

impl<'a> Trials<'a> {
    fn new(settings: &DevelopSettings, info: &'a SourceInfo, source: &Rgb32f) -> Self {
        use lightcraft_raster::resample::{Filter, fit};
        let small = fit(source, PROXY, PROXY, Filter::Box);
        // Where the sensor ran out, every channel stops at the same value:
        // more than specks of the frame at exactly its brightest is that,
        // and no highlight a slider could bring back.
        let brightest = |p: &[f32; 3]| p[0].max(p[1]).max(p[2]);
        let top = source.data.iter().map(brightest).filter(|v| v.is_finite()).fold(0.0, f32::max);
        let at_top = source.data.iter().filter(|p| brightest(p) >= top * 0.9999).count();
        let ran_out = top > 0.0 && at_top as f64 > source.data.len() as f64 * SPECKS;
        let saturated = small.data.iter().map(|p| ran_out && brightest(p) >= top * 0.9).collect();
        let whole = DevelopSettings { crop: Default::default(), orientation: Default::default(), geometry: Default::default(), ..settings.clone() };
        Self { settings: whole, info, small: Arc::new(small), saturated, stages: StageCache::default() }
    }

    /// Each pixel's brightest and darkest channel, as developed.
    fn render(&mut self, whites: f64, blacks: f64) -> Vec<(u8, u8)> {
        (self.settings.light.whites, self.settings.light.blacks) = (whites, blacks);
        let request = RenderRequest::fit(self.small.width, self.small.height);
        let rendered = lightcraft_pipeline::render_cached(&self.small, self.info, &self.settings, &request, &self.stages);
        rendered.image.data.iter().map(|p| (p[0].max(p[1]).max(p[2]), p[0].min(p[1]).min(p[2]))).collect()
    }
}

/// The value from 0 towards `far` nearest 0 at which no more than specks
/// of the frame are `clipped`, to a whole number; `far` itself if even
/// that isn't enough. Pixels that are `lost` can't be helped, nor can
/// those clipped even at `far`, and neither count.
fn limit(far: f64, lost: &[bool], mut clipped: impl FnMut(f64) -> Vec<bool>) -> f64 {
    let hopeless = clipped(far);
    let allowed = (hopeless.len() as f64 * SPECKS) as usize;
    let mut fits = |value: f64| {
        let clipped = clipped(value);
        let needless = clipped.iter().enumerate().filter(|(i, clipped)| **clipped && !hopeless[*i] && !lost.get(*i).is_some_and(|lost| *lost));
        needless.count() <= allowed
    };
    if fits(0.0) {
        return 0.0;
    }
    // Clipping only grows on the way back to 0.
    let (mut safe, mut over) = (far, 0.0);
    for _ in 0..8 {
        let middle = (safe + over) / 2.0;
        if fits(middle) { safe = middle } else { over = middle }
    }
    if far < 0.0 { safe.floor() } else { safe.ceil() }
}

/// Whites, brought down from 0 until no highlight clips that needn't.
fn whites(settings: &DevelopSettings, info: &SourceInfo, source: &Rgb32f) -> f64 {
    let mut trials = Trials::new(settings, info, source);
    let (blacks, saturated) = (settings.light.blacks, std::mem::take(&mut trials.saturated));
    limit(-100.0, &saturated, |whites| trials.render(whites, blacks).iter().map(|(brightest, _)| *brightest == u8::MAX).collect())
}

/// Blacks, brought up from 0 until no shadow is crushed that needn't be.
fn blacks(settings: &DevelopSettings, info: &SourceInfo, source: &Rgb32f) -> f64 {
    let mut trials = Trials::new(settings, info, source);
    let whites = settings.light.whites;
    limit(100.0, &[], |blacks| trials.render(whites, blacks).iter().map(|(_, darkest)| *darkest == 0).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A frame that runs from `dark` at the top left to `bright` at the
    /// bottom right, no two of its diagonals alike.
    fn ramp(dark: f32, bright: f32) -> Arc<Rgb32f> {
        let (w, h) = (240, 160);
        Arc::new(Rgb32f::from_fn(w, h, |x, y| [dark + (bright - dark) * (x * h + y * w) as f32 / (2 * w * h) as f32; 3]))
    }

    /// The shares of a frame's pixels that are blown and that are crushed,
    /// as developed.
    fn clipping(settings: &DevelopSettings, info: &SourceInfo, source: &Arc<Rgb32f>) -> (f64, f64) {
        let rendered = lightcraft_pipeline::render(source, info, settings, &RenderRequest::fit(PROXY, PROXY));
        let pixels = &rendered.image.data;
        let share = |of: &dyn Fn(&[u8; 4]) -> bool| pixels.iter().filter(|p| of(p)).count() as f64 / pixels.len() as f64;
        (share(&|p| p[0] == 255), share(&|p| p[0] == 0))
    }

    #[test]
    fn auto_tone_leaves_a_frame_alone_where_nothing_clips() {
        let info = SourceInfo::default();
        let source = ramp(0.05, 0.35);
        let mut s = DevelopSettings::default();
        assert_eq!(clipping(&s, &info, &source), (0.0, 0.0));
        (s.light.whites, s.light.blacks) = (30.0, -20.0);
        assert!(apply(&mut s, &info, &Camera::default(), &source, Auto::Tone));
        assert_eq!((s.light.whites, s.light.blacks), (0.0, 0.0), "not pushed out to fill the histogram, and what was set is set again");
    }

    #[test]
    fn auto_tone_brings_whites_down_until_nothing_clips_that_neednt() {
        let info = SourceInfo::default();
        // Bright enough that the end of the ramp is blown as developed,
        // some of it past anything Whites can do.
        let source = ramp(0.0, 1.6);
        let mut s = DevelopSettings::default();
        let (blown, _) = clipping(&s, &info, &source);
        let hopeless = clipping(&DevelopSettings { light: lightcraft_develop::Light { whites: -100.0, ..Default::default() }, ..s.clone() }, &info, &source).0;
        assert!(blown > hopeless + 0.02, "more blown than need be to start with: {blown}, {hopeless}");
        assert!(apply(&mut s, &info, &Camera::default(), &source, Auto::Tone));
        assert!(s.light.whites < 0.0 && s.light.whites.fract() == 0.0, "whites {}", s.light.whites);
        let (now_blown, _) = clipping(&s, &info, &source);
        assert!(now_blown <= hopeless + 2.0 * SPECKS, "{now_blown} of {blown}, {hopeless} past help");

        // And no further than it takes: a patch just over white comes
        // just under it.
        let mut patched = (*ramp(0.0, 0.5)).clone();
        for y in 20..60 {
            for x in 20..80 {
                patched.data[y * patched.width + x] = [1.02 + (x + y) as f32 * 1e-4; 3];
            }
        }
        let patched = Arc::new(patched);
        let mut s = DevelopSettings::default();
        assert!(clipping(&s, &info, &patched).0 > 0.03, "the patch is blown as developed");
        assert!(apply(&mut s, &info, &Camera::default(), &patched, Auto::Tone));
        assert!(s.light.whites < 0.0 && s.light.whites > -100.0, "whites {}", s.light.whites);
        assert!(clipping(&s, &info, &patched).0 <= 2.0 * SPECKS);
        let mut less = s.clone();
        less.light.whites += 3.0;
        assert!(clipping(&less, &info, &patched).0 > 0.01, "three more and it clips");
    }

    #[test]
    fn auto_tone_leaves_what_the_sensor_ran_out_on() {
        let info = SourceInfo::default();
        let ramp = ramp(0.0, 1.6);
        let mut plain = DevelopSettings::default();
        apply(&mut plain, &info, &Camera::default(), &ramp, Auto::Tone);
        // The same frame with a window in it, blown out in the raw: every
        // pixel of it at the one value the sensor stops at.
        let mut window = (*ramp).clone();
        for y in 20..60 {
            for x in 20..80 {
                window.data[y * window.width + x] = [4.0; 3];
            }
        }
        let window = Arc::new(window);
        let mut s = DevelopSettings::default();
        assert!(apply(&mut s, &info, &Camera::default(), &window, Auto::Tone));
        assert_eq!(s.light.whites, plain.light.whites, "the window doesn't pull Whites down any further");
        assert!(clipping(&s, &info, &window).0 > 0.03, "and stays white");
    }

    #[test]
    fn auto_tone_brings_blacks_up_for_shadows_that_can_be_opened() {
        let info = SourceInfo::default();
        // Dark, but not black: Blacks pulled down crushes its start.
        let source = ramp(0.0005, 0.3);
        let mut s = DevelopSettings::default();
        s.light.blacks = -100.0;
        let crushed = clipping(&s, &info, &source).1;
        let mut auto = s.clone();
        assert!(apply(&mut auto, &info, &Camera::default(), &source, Auto::Tone));
        assert!(auto.light.blacks >= 0.0, "blacks {}", auto.light.blacks);
        assert!(clipping(&auto, &info, &source).1 <= crushed);
        // Black in the raw is black wherever Blacks is: it's left.
        let mut black = Rgb32f::new(64, 48);
        black.data.fill([0.0; 3]);
        let mut s = DevelopSettings::default();
        assert!(apply(&mut s, &info, &Camera::default(), &Arc::new(black), Auto::Tone));
        assert_eq!((s.light.whites, s.light.blacks), (0.0, 0.0));
    }

    #[test]
    fn auto_exposure_brightens_a_dark_frame_and_only_that() {
        let info = SourceInfo::default();
        let source = ramp(0.001, 0.02);
        let mut s = DevelopSettings::default();
        assert!(apply(&mut s, &info, &Camera::default(), &source, Auto::Exposure));
        assert!(s.light.exposure > 0.5, "{}", s.light.exposure);
        let expected = DevelopSettings { light: lightcraft_develop::Light { exposure: s.light.exposure, ..Default::default() }, ..DevelopSettings::default() };
        assert_eq!(s, expected);
    }

    #[test]
    fn auto_white_balance_cools_a_warm_frame() {
        let info = SourceInfo::default();
        let mut warm = Rgb32f::new(64, 48);
        warm.data.fill([0.3, 0.2, 0.1]);
        let mut s = DevelopSettings::default();
        assert!(apply(&mut s, &info, &Camera::default(), &Arc::new(warm), Auto::WhiteBalance));
        assert_eq!(s.wb.mode, WbMode::Auto);
        assert!(s.wb.temp < 6000.0, "{:?}", s.wb);
    }

    #[test]
    fn one_slider_gets_its_own_automatic_value_and_the_rest_stay() {
        let info = SourceInfo::default();
        let source = ramp(0.05, 0.35);
        let mut s = DevelopSettings::default();
        assert!(apply(&mut s, &info, &Camera::default(), &source, Auto::Control("light.shadows")));
        assert!(s.light.shadows > 0.0);
        let expected = DevelopSettings { light: lightcraft_develop::Light { shadows: s.light.shadows, ..Default::default() }, ..DevelopSettings::default() };
        assert_eq!(s, expected);

        // Whites alone, on a frame that clips.
        let bright = ramp(0.0, 1.6);
        let mut both = DevelopSettings::default();
        apply(&mut both, &info, &Camera::default(), &bright, Auto::Tone);
        s.light.blacks = 15.0;
        assert!(apply(&mut s, &info, &Camera::default(), &bright, Auto::Control("light.whites")));
        assert!(s.light.whites < 0.0 && s.light.blacks == 15.0 && both.light.blacks == 0.0, "whites {}", s.light.whites);

        assert!(apply(&mut s, &info, &Camera::default(), &source, Auto::Control("wb.tint")));
        assert_eq!(s.wb.mode, WbMode::Custom, "a slider set by hand or by Auto is a custom white balance");

        let before = s.clone();
        assert!(!apply(&mut s, &info, &Camera::default(), &source, Auto::Control("detail.sharpenAmount")), "no automatic value");
        assert_eq!(s, before);
    }

    #[test]
    fn auto_level_straightens_a_tilted_horizon_or_says_theres_none() {
        let info = SourceInfo::default();
        // Sky over water, the horizon and the swell behind it dropping 4°
        // to the right.
        let (w, h) = (900, 600);
        let slope = 4f32.to_radians().tan();
        let tilted = Rgb32f::from_fn(w, h, |x, y| {
            let below = y as f32 - (h as f32 / 2.0 + (x as f32 - w as f32 / 2.0) * slope);
            if below < 0.0 {
                [0.6, 0.7, 0.9]
            } else if (below / 30.0) as i32 % 2 == 0 {
                [0.05, 0.1, 0.2]
            } else {
                [0.1, 0.2, 0.3]
            }
        });
        let mut s = DevelopSettings::default();
        assert!(apply(&mut s, &info, &Camera::default(), &Arc::new(tilted), Auto::Level));
        assert!((s.crop.geometry.angle + 4.0).abs() < 0.5, "{}", s.crop.geometry.angle);
        assert!(s.crop.geometry.is_within_image(w as f64, h as f64));

        let mut plain = Rgb32f::new(w, h);
        plain.data.fill([0.2; 3]);
        let mut s = DevelopSettings::default();
        assert!(!apply(&mut s, &info, &Camera::default(), &Arc::new(plain), Auto::Level), "nothing to go by");
        assert_eq!(s, DevelopSettings::default());
    }
}

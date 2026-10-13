//! Auto: white balance, exposure, tone and level, each a button, and any
//! one slider by Shift+double-click, as in Lightroom. Nothing here runs
//! on its own (see "Auto is a button" in DESIGN.md).
//!
//! White balance, exposure and level are Lightcraft's
//! (`lightcraft_pipeline::auto` and `upright`), as are the values a single
//! slider gets, all but two: Lightcraft's Auto leaves Whites and Blacks at
//! 0, so Auto tone is OmaLux's own. It takes Lightroom's behaviour, not
//! its code: Whites as high and Blacks as low as they go before anything
//! clips that needn't, found by rendering the frame small and counting.

use std::sync::Arc;

use lightcraft_develop::{DevelopSettings, WbMode};
use lightcraft_pipeline::{RenderRequest, SourceInfo, StageCache};
use lightcraft_raster::Rgb32f;

use crate::{crop, edit};

/// Something to set automatically.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Auto {
    /// Temp and Tint, from the frame's own greys.
    WhiteBalance,
    Exposure,
    /// Whites and Blacks.
    Tone,
    /// The straighten angle that levels the horizon, or makes the
    /// uprights plumb.
    Level,
    /// One slider, by its control's id.
    Control(&'static str),
}

/// The long edge Auto tone renders the frame at to count what clips.
const PROXY: usize = 256;
/// The share of the frame that may clip: specks don't hold the rest back.
const SPECKS: f64 = 0.0005;

/// Set `what` in `settings` from the frame itself. False if there was
/// nothing to go by (no horizon to level, a slider with no automatic
/// value), and the settings are as they were.
pub fn apply(settings: &mut DevelopSettings, info: &SourceInfo, source: &Arc<Rgb32f>, what: Auto) -> bool {
    match what {
        Auto::WhiteBalance => {
            let (temp, tint) = lightcraft_pipeline::auto::auto_wb(source, info);
            lightcraft_pipeline::local::normalize_wb(settings, info);
            (settings.wb.mode, settings.wb.temp, settings.wb.tint) = (WbMode::Auto, temp, tint);
        }
        Auto::Exposure => settings.light.exposure = lightcraft_pipeline::auto::auto_tone(source, info, settings).exposure,
        Auto::Tone => {
            settings.light.whites = whites(settings, info, source);
            settings.light.blacks = blacks(settings, info, source);
        }
        Auto::Level => {
            let Some(angle) = lightcraft_pipeline::upright::level_degrees(source, info, settings) else { return false };
            crop::straighten(settings, crop::dims((source.width, source.height), settings), angle);
        }
        Auto::Control(id) => {
            let tone = || lightcraft_pipeline::auto::auto_tone(source, info, settings);
            let value = match id {
                "wb.temp" => lightcraft_pipeline::auto::auto_wb(source, info).0,
                "wb.tint" => lightcraft_pipeline::auto::auto_wb(source, info).1,
                "light.exposure" => tone().exposure,
                "light.contrast" => tone().contrast,
                "light.highlights" => tone().highlights,
                "light.shadows" => tone().shadows,
                "light.whites" => whites(settings, info, source),
                "light.blacks" => blacks(settings, info, source),
                "color.vibrance" => tone().vibrance,
                "color.saturation" => tone().saturation,
                _ => return false,
            };
            return edit::set_control(settings, info, id, value);
        }
    }
    true
}

/// Renders of the frame, small, with Whites and Blacks tried at one value
/// after another: the stages before them are kept between renders.
struct Trials<'a> {
    settings: DevelopSettings,
    info: &'a SourceInfo,
    source: &'a Arc<Rgb32f>,
    stages: StageCache,
}

impl Trials<'_> {
    /// Each pixel's brightest and darkest channel.
    fn render(&mut self, whites: f64, blacks: f64) -> Vec<(u8, u8)> {
        (self.settings.light.whites, self.settings.light.blacks) = (whites, blacks);
        let rendered = lightcraft_pipeline::render_cached(self.source, self.info, &self.settings, &RenderRequest::fit(PROXY, PROXY), &self.stages);
        rendered.image.data.iter().map(|p| (p[0].max(p[1]).max(p[2]), p[0].min(p[1]).min(p[2]))).collect()
    }
}

/// The value between -100 and 100 nearest `towards` at which no more than
/// specks of the frame are `clipped`, to a whole number. Pixels clipped
/// even at the far end can't be helped by this slider, and don't count.
fn limit(towards: f64, mut clipped: impl FnMut(f64) -> Vec<bool>) -> f64 {
    let lost = clipped(-towards);
    let allowed = (lost.len() as f64 * SPECKS) as usize;
    let mut fits = |value: f64| clipped(value).iter().zip(&lost).filter(|(clipped, lost)| **clipped && !**lost).count() <= allowed;
    if fits(towards) {
        return towards;
    }
    // Clipping only grows on the way to `towards`.
    let (mut safe, mut over) = (-towards, towards);
    for _ in 0..8 {
        let middle = (safe + over) / 2.0;
        if fits(middle) { safe = middle } else { over = middle }
    }
    if towards > 0.0 { safe.floor() } else { safe.ceil() }
}

/// Whites as high as it goes before highlights clip that needn't.
fn whites(settings: &DevelopSettings, info: &SourceInfo, source: &Arc<Rgb32f>) -> f64 {
    let mut trials = Trials { settings: settings.clone(), info, source, stages: StageCache::default() };
    let blacks = settings.light.blacks;
    limit(100.0, |whites| trials.render(whites, blacks).iter().map(|(brightest, _)| *brightest == u8::MAX).collect())
}

/// Blacks as low as it goes before shadows are crushed that needn't be.
fn blacks(settings: &DevelopSettings, info: &SourceInfo, source: &Arc<Rgb32f>) -> f64 {
    let mut trials = Trials { settings: settings.clone(), info, source, stages: StageCache::default() };
    let whites = settings.light.whites;
    limit(-100.0, |blacks| trials.render(whites, blacks).iter().map(|(_, darkest)| *darkest == 0).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A frame whose rows run from `dark` on the left to `bright` on the right.
    fn ramp(dark: f32, bright: f32) -> Arc<Rgb32f> {
        let (w, h) = (240, 160);
        let mut image = Rgb32f::new(w, h);
        for y in 0..h {
            for x in 0..w {
                image.data[y * w + x] = [dark + (bright - dark) * x as f32 / (w - 1) as f32; 3];
            }
        }
        Arc::new(image)
    }

    /// The shares of a frame's pixels that are blown and that are crushed,
    /// and its brightest and darkest values, as developed.
    fn clipping(settings: &DevelopSettings, info: &SourceInfo, source: &Arc<Rgb32f>) -> (f64, f64, u8, u8) {
        let rendered = lightcraft_pipeline::render(source, info, settings, &RenderRequest::fit(PROXY, PROXY));
        let pixels = &rendered.image.data;
        let share = |of: &dyn Fn(&[u8; 4]) -> bool| pixels.iter().filter(|p| of(p)).count() as f64 / pixels.len() as f64;
        let brightest = pixels.iter().map(|p| p[0]).max().unwrap_or(0);
        let darkest = pixels.iter().map(|p| p[0]).min().unwrap_or(0);
        (share(&|p| p[0] == 255), share(&|p| p[0] == 0), brightest, darkest)
    }

    #[test]
    fn auto_tone_opens_a_flat_frame_out_to_black_and_white_and_no_further() {
        let info = SourceInfo::default();
        let source = ramp(0.05, 0.35);
        let mut s = DevelopSettings::default();
        let (.., brightest, darkest) = clipping(&s, &info, &source);
        assert!(brightest < 240 && darkest > 15, "flat to start with: {darkest} to {brightest}");

        assert!(apply(&mut s, &info, &source, Auto::Tone));
        assert!(s.light.whites > 0.0 && s.light.blacks < 0.0, "whites {} blacks {}", s.light.whites, s.light.blacks);
        let (blown, crushed, now_brightest, now_darkest) = clipping(&s, &info, &source);
        assert!(blown <= 0.01 && crushed <= 0.01, "blown {blown}, crushed {crushed}");
        assert!(now_brightest > brightest && now_darkest < darkest, "{now_darkest} to {now_brightest}");
        assert_eq!((s.light.whites.fract(), s.light.blacks.fract()), (0.0, 0.0), "whole numbers, as the sliders step");
    }

    #[test]
    fn auto_tone_pulls_back_what_clips_and_leaves_what_cant_be_helped() {
        let info = SourceInfo::default();
        // Bright enough that the top of the ramp is blown as developed.
        let source = ramp(0.0, 1.6);
        let mut s = DevelopSettings::default();
        let (blown, ..) = clipping(&s, &info, &source);
        assert!(blown > 0.02, "blown to start with: {blown}");
        assert!(apply(&mut s, &info, &source, Auto::Tone));
        assert!(s.light.whites < 0.0, "whites {}", s.light.whites);
        let (now_blown, ..) = clipping(&s, &info, &source);
        assert!(now_blown < blown, "{now_blown} of {blown}");
    }

    #[test]
    fn auto_exposure_brightens_a_dark_frame_and_only_that() {
        let info = SourceInfo::default();
        let source = ramp(0.001, 0.02);
        let mut s = DevelopSettings::default();
        assert!(apply(&mut s, &info, &source, Auto::Exposure));
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
        assert!(apply(&mut s, &info, &Arc::new(warm), Auto::WhiteBalance));
        assert_eq!(s.wb.mode, WbMode::Auto);
        assert!(s.wb.temp < 6000.0, "{:?}", s.wb);
    }

    #[test]
    fn one_slider_gets_its_own_automatic_value_and_the_rest_stay() {
        let info = SourceInfo::default();
        let source = ramp(0.05, 0.35);
        let mut s = DevelopSettings::default();
        assert!(apply(&mut s, &info, &source, Auto::Control("light.shadows")));
        assert!(s.light.shadows > 0.0);
        let expected = DevelopSettings { light: lightcraft_develop::Light { shadows: s.light.shadows, ..Default::default() }, ..DevelopSettings::default() };
        assert_eq!(s, expected);

        assert!(apply(&mut s, &info, &source, Auto::Control("light.whites")));
        let mut both = DevelopSettings::default();
        apply(&mut both, &info, &source, Auto::Tone);
        assert!(s.light.whites > 0.0 && s.light.blacks == 0.0 && both.light.blacks < 0.0);

        assert!(apply(&mut s, &info, &source, Auto::Control("wb.tint")));
        assert_eq!(s.wb.mode, WbMode::Custom, "a slider set by hand or by Auto is a custom white balance");

        let before = s.clone();
        assert!(!apply(&mut s, &info, &source, Auto::Control("detail.sharpenAmount")), "no automatic value");
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
        assert!(apply(&mut s, &info, &Arc::new(tilted), Auto::Level));
        assert!((s.crop.geometry.angle + 4.0).abs() < 0.5, "{}", s.crop.geometry.angle);
        assert!(s.crop.geometry.is_within_image(w as f64, h as f64));

        let mut plain = Rgb32f::new(w, h);
        plain.data.fill([0.2; 3]);
        let mut s = DevelopSettings::default();
        assert!(!apply(&mut s, &info, &Arc::new(plain), Auto::Level), "nothing to go by");
        assert_eq!(s, DevelopSettings::default());
    }
}

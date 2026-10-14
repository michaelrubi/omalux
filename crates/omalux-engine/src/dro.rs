//! Sony's Dynamic Range Optimizer, put back: how much it lifted a frame's
//! shadows (see "DRO is matched" in docs/DESIGN.md).
//!
//! DRO brightens the darker parts of the camera's JPEG, locally, and the
//! raw doesn't have it. The camera's maker note says whether it was on
//! and at what setting, but not what Auto chose, so the lift is measured
//! from the frame's own JPEG, which is what the camera did: the raw is
//! developed small with the camera's tone curve without DRO, with Shadows
//! at one value after another on Lightcraft's local tone mapping, and the
//! value that comes closest to the JPEG's lightness is the frame's lift.
//! Camera match renders it under the Shadows slider
//! ([`crate::look::effective`]).
//!
//! The fixed levels (Lv1 to Lv5) are measured the same way for now: none
//! of Michael's shoots has a frame shot at one, so there's nothing to pool
//! a strength per level from yet.

use std::sync::Arc;

use lightcraft_develop::DevelopSettings;
use lightcraft_pipeline::tone::CameraTone;
use lightcraft_pipeline::{RenderRequest, SourceInfo, StageCache};
use lightcraft_raster::Rgb32f;
use lightcraft_raw::RawImage;
use lightcraft_raw::color::CameraTransform;

use crate::camera_preview::{CameraLook, HueSat, PROFILE_PROXY};
use crate::compare::lab;

/// The most Shadows that's tried: twice the slider's end.
const MOST: f64 = 200.0;
/// The steps the first pass tries Shadows at.
const COARSE: f64 = 40.0;
/// The share of each side, in the middle, that's compared: the camera's
/// JPEG has its shading correction in it, which brightens the corners as
/// a lift would.
const MIDDLE: f64 = 0.7;
/// A lift has to bring the render at least this much closer to the JPEG
/// (as a share of the squared error with none) to be one.
const WORTH: f64 = 0.03;

/// DRO's lift on a frame, in the Shadows slider's units, from its raw
/// and the JPEG in it. `transform` and `look` are the colour the frame is
/// developed with. `None` if the two can't be compared (no usable JPEG).
pub(crate) fn measure(raw: &RawImage, bytes: &[u8], transform: &CameraTransform, look: &CameraLook) -> Option<f64> {
    let (mut scene, reference, clipped) = crate::camera_preview::proxies(raw, bytes, transform, PROFILE_PROXY)?;
    // The proxy is through the camera's generic colour: the look goes on top, as the decode applies it.
    let (matrix, hue_sat) = (look.matrix.to_f32(), look.hue_sat.as_ref().and_then(HueSat::new));
    scene.map_in_place(|p| {
        let rgb = std::array::from_fn(|i| matrix[i][0] * p[0] + matrix[i][1] * p[1] + matrix[i][2] * p[2]);
        hue_sat.as_ref().map_or(rgb, |h| h.apply(rgb)).map(|v: f32| v.max(0.0))
    });
    Some(lift(scene, &reference, &clipped, &look.tone))
}

/// The Shadows value at which `scene`, developed with `tone`, comes
/// closest in lightness to `reference` (the camera's JPEG at the same
/// size, linear Rec. 2020), from 0 to [`MOST`]; 0 if none helps. Pixels
/// the sensor `clipped` on aren't compared.
fn lift(scene: Rgb32f, reference: &Rgb32f, clipped: &[bool], tone: &CameraTone) -> f64 {
    let (w, h) = (scene.width, scene.height);
    if (reference.width, reference.height) != (w, h) || clipped.len() != w * h || w == 0 || h == 0 {
        return 0.0;
    }
    let lightness = |linear: f32| lab([linear_to_srgb8(linear); 3])[0];
    let inside = |i: usize, n: usize| ((i as f64 + 0.5) / n as f64 - 0.5).abs() <= MIDDLE / 2.0;
    // What the JPEG shows, where it's worth comparing.
    let wanted: Vec<Option<f64>> = reference
        .data
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let y = lightcraft_color::luminance_2020(*p);
            (inside(i % w, w) && inside(i / w, h) && !clipped[i] && y.is_finite()).then(|| lightness(y))
        })
        .collect();
    let compared = wanted.iter().flatten().count();
    if compared == 0 {
        return 0.0;
    }
    let (scene, stages) = (Arc::new(scene), StageCache::default());
    let info = SourceInfo { raw: true, camera_tone: Some(*tone), ..SourceInfo::default() };
    let mut settings = DevelopSettings::default();
    // The mean squared difference in L* from the JPEG with Shadows at `shadows`.
    let mut error = |shadows: f64| {
        settings.light.shadows = shadows;
        let rendered = lightcraft_pipeline::render_cached(&scene, &info, &settings, &RenderRequest::fit(w, h), &stages).image;
        if (rendered.width, rendered.height) != (w, h) {
            return f64::INFINITY;
        }
        let squares = rendered.data.iter().zip(&wanted).filter_map(|(p, wanted)| {
            let y = 0.2126729 * srgb8_to_linear(p[0]) + 0.7151522 * srgb8_to_linear(p[1]) + 0.0721750 * srgb8_to_linear(p[2]);
            wanted.map(|wanted| (lightness(y as f32) - wanted).powi(2))
        });
        squares.sum::<f64>() / compared as f64
    };
    let none = error(0.0);
    // Coarsely first, then closer in round the best: the error has one low point, lifting too little on one
    // side of it and too much on the other.
    let steps = (MOST / COARSE) as usize;
    let (mut best, mut least) = (0.0, none);
    for step in 1..=steps {
        let shadows = step as f64 * COARSE;
        let e = error(shadows);
        if e < least {
            (best, least) = (shadows, e);
        }
    }
    let mut reach = COARSE / 2.0;
    while reach >= 2.5 {
        for shadows in [best - reach, best + reach] {
            if (0.0..=MOST).contains(&shadows) {
                let e = error(shadows);
                if e < least {
                    (best, least) = (shadows, e);
                }
            }
        }
        reach /= 2.0;
    }
    if least.is_finite() && least < none * (1.0 - WORTH) { best.round() } else { 0.0 }
}

fn linear_to_srgb8(v: f32) -> u8 {
    (lightcraft_color::transfer::linear_to_srgb(v.clamp(0.0, 1.0)) * 255.0).round() as u8
}

fn srgb8_to_linear(v: u8) -> f64 {
    f64::from(lightcraft_color::transfer::srgb_to_linear(f32::from(v) / 255.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scene with a dark half and a bright half, and enough in each for
    /// the local tone mapping to tell them apart.
    fn scene() -> Rgb32f {
        Rgb32f::from_fn(96, 64, |x, y| {
            let ripple = 1.0 + 0.2 * ((x as f32 * 0.9).sin() * (y as f32 * 0.7).cos());
            [if x < 48 { 0.02 } else { 0.4 } * ripple; 3]
        })
    }

    fn tone() -> CameraTone {
        crate::camera_preview::sony_dro_off_tone().unwrap()
    }

    /// What the camera's JPEG of `scene` would be, lifted by `shadows`: linear Rec. 2020 (grey is grey in both).
    fn jpeg(scene: &Rgb32f, shadows: f64) -> Rgb32f {
        let info = SourceInfo { raw: true, camera_tone: Some(tone()), ..SourceInfo::default() };
        let mut settings = DevelopSettings::default();
        settings.light.shadows = shadows;
        let rendered = lightcraft_pipeline::render(scene, &info, &settings, &RenderRequest::fit(scene.width, scene.height)).image;
        Rgb32f::from_fn(scene.width, scene.height, |x, y| rendered.get(x, y)[..3].iter().map(|v| srgb8_to_linear(*v) as f32).collect::<Vec<_>>().try_into().unwrap())
    }

    #[test]
    fn the_lift_the_camera_gave_is_the_lift_measured() {
        let scene = scene();
        let clipped = vec![false; scene.data.len()];
        for shadows in [0.0, 30.0, 85.0, 150.0] {
            let measured = lift(scene.clone(), &jpeg(&scene, shadows), &clipped, &tone());
            assert!((measured - shadows).abs() <= 3.0, "{shadows} measured as {measured}");
        }
    }

    #[test]
    fn a_jpeg_darker_than_the_raw_is_no_lift_and_nothing_to_compare_is_none() {
        let scene = scene();
        let clipped = vec![false; scene.data.len()];
        assert_eq!(lift(scene.clone(), &jpeg(&scene, -60.0), &clipped, &tone()), 0.0);
        // Every pixel clipped, or a JPEG of another size: nothing to go by.
        assert_eq!(lift(scene.clone(), &jpeg(&scene, 85.0), &vec![true; scene.data.len()], &tone()), 0.0);
        assert_eq!(lift(scene.clone(), &Rgb32f::new(10, 10), &clipped, &tone()), 0.0);
        // NaNs in the JPEG are left out, not compared.
        let mut holed = jpeg(&scene, 85.0);
        holed.data[40 * 96 + 20] = [f32::NAN; 3];
        assert!((lift(scene.clone(), &holed, &clipped, &tone()) - 85.0).abs() <= 3.0);
    }
}

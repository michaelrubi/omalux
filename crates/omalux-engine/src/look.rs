//! The look a frame starts with, and what's rendered under the sliders
//! (see "The look" and "HDR" in docs/DESIGN.md).
//!
//! - **The profile** is a setting, as Lightroom's is: Camera match (the
//!   camera's own JPEG, from a profile fixed for the camera and its
//!   Creative Style) or Flat. It's kept in Lightcraft's `profile.id`,
//!   which its pipeline ignores for ids it doesn't know. An edit made
//!   before M4 has Lightcraft's default there, and keeps the look it was
//!   made with: the one fitted to the frame's own JPEG ([`Profile::Fitted`]).
//! - **What the camera did** to its JPEG that the raw doesn't have
//!   ([`Camera`]): the Dynamic Range Optimizer's shadow lift. Camera match
//!   puts it back under the Shadows slider, which stays at 0.
//! - **HDR** is Highlights down and Shadows up together, by one amount, on
//!   the pipeline's local tone mapping. Its own Highlights and Shadows give
//!   either end more of it, or less, than the amount, darker to the left
//!   and brighter to the right as Light's are.
//!
//! None of it is a stage of its own: [`effective`] gives the settings and
//! the source facts the pipeline is handed, as Lightcraft's own profiles
//! do (`lightcraft_pipeline::profiles::effective`), so the CPU and the GPU
//! render it alike. Everything that renders a frame goes through it.

use std::borrow::Cow;

use lightcraft_develop::DevelopSettings;
use lightcraft_pipeline::SourceInfo;

use crate::sony::Dro;

/// `profile.id` for Camera match.
const CAMERA: &str = "omalux.camera";
/// `profile.id` for Flat.
const FLAT: &str = "omalux.flat";

/// What HDR at 100 takes from Highlights and gives to Shadows, in their
/// units. Past the sliders' own ends on purpose: the pipeline doesn't stop
/// at them, and the slider is for what they can't reach apart.
const HDR_HIGHLIGHTS: f64 = 1.0;
const HDR_SHADOWS: f64 = 1.0;

/// The starting look.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Profile {
    /// Fitted to the frame's own JPEG, as every frame was before M4
    /// (process version 1): what an edit made then keeps.
    Fitted,
    /// The camera's JPEG: the profile fixed for the camera and its Creative
    /// Style, with DRO's shadow lift.
    Camera,
    /// Camera match's colour without its tone curve or DRO's lift: low
    /// contrast, all the highlight and shadow room there is.
    Flat,
}

impl Profile {
    /// The profile `settings` ask for. An id that isn't OmaLux's is an
    /// edit from before there were any.
    pub fn of(settings: &DevelopSettings) -> Profile {
        match settings.profile.id.as_str() {
            CAMERA => Profile::Camera,
            FLAT => Profile::Flat,
            _ => Profile::Fitted,
        }
    }

    pub fn set(self, settings: &mut DevelopSettings) {
        settings.profile.id = match self {
            Profile::Fitted => lightcraft_develop::Profile::default().id,
            Profile::Camera => CAMERA.into(),
            Profile::Flat => FLAT.into(),
        };
    }

    /// What the Flat key goes to from here: Flat, and from Flat back to
    /// Camera match.
    pub fn toggled(self) -> Profile {
        if self == Profile::Flat { Profile::Camera } else { Profile::Flat }
    }

    /// Whether the raw is developed with the look fitted to its own JPEG.
    /// Camera match and Flat share a development: they differ in what
    /// [`effective`] does, so going from one to the other decodes nothing.
    pub fn fitted(self) -> bool {
        self == Profile::Fitted
    }

    /// The profile's name, as Lightroom names its own: "Camera Standard"
    /// for a frame shot in Standard.
    pub fn label(self, camera: &Camera) -> String {
        match self {
            Profile::Fitted => "Camera (fitted)".into(),
            Profile::Camera => format!("Camera {}", camera.style.as_deref().unwrap_or("match")),
            Profile::Flat => "Flat".into(),
        }
    }
}

/// What a frame's camera did to its own JPEG, as far as Camera match
/// follows it, and the lens it was shot through.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Camera {
    /// The Creative Style, as the camera wrote it ("Standard", "Vivid").
    pub style: Option<String>,
    pub dro: Option<Dro>,
    /// DRO's shadow lift on this frame, in the Shadows slider's units (100
    /// is the slider's end): what Camera match adds under the slider. 0
    /// for a frame shot with DRO off.
    pub lift: f64,
    /// The frame was developed with a profile fixed for its camera and
    /// style. False where there's none for them, or under
    /// [`Profile::Fitted`]: its look was fitted to its own JPEG.
    pub fixed: bool,
    /// The lens a profile correction was found for, as the Lens
    /// Corrections panel names it ([`crate::lens_profiles::Found`]).
    pub lens: Option<String>,
}

/// A fresh raw's settings, white balance as shot: Lightcraft's raw
/// defaults under Camera match, with the lens profile's corrections on.
pub fn fresh(as_shot_temp: f64, as_shot_tint: f64) -> DevelopSettings {
    let mut settings = DevelopSettings::for_raw(as_shot_temp, as_shot_tint);
    Profile::Camera.set(&mut settings);
    settings.optics.lens_profile = true;
    settings
}

/// Whether the lens profile `info` has corrects lateral chromatic
/// aberration (its colour planes are warped apart), under `settings`.
fn profile_corrects_ca(settings: &DevelopSettings, info: &SourceInfo) -> bool {
    fn apart<T: PartialEq>(planes: &[T; 3]) -> bool {
        planes[0] != planes[1] || planes[1] != planes[2]
    }
    settings.optics.lens_profile
        && settings.optics.profile_distortion > 0.0
        && info.lens.is_some_and(|lens| lens.warp.is_some_and(|warp| apart(&warp.planes)) || lens.stages.iter().flatten().any(|stage| apart(&stage.planes)))
}

/// What the pipeline is handed to render `settings`: the profile's part
/// and HDR folded in under the sliders, which show what they did; and
/// Remove Chromatic Aberration's estimate left out where the lens profile
/// corrects the lateral CA itself, as M5 has it (the estimate is made on
/// the uncorrected picture, so the two would add up).
pub fn effective<'a>(settings: &'a DevelopSettings, info: &'a SourceInfo, camera: &Camera) -> (Cow<'a, DevelopSettings>, Cow<'a, SourceInfo>) {
    let profile = Profile::of(settings);
    let lift = if profile == Profile::Camera { camera.lift } else { 0.0 };
    let hdr = |side: f64| (settings.light.hdr + side).clamp(0.0, 100.0);
    // Both read as brightness, as Light's sliders do: HDR's Highlights to
    // the left holds the highlights back more, its Shadows to the right
    // lifts the shadows more.
    let (highlights, shadows) = (hdr(-settings.light.hdr_highlights), hdr(settings.light.hdr_shadows));
    let ca = settings.optics.remove_ca && profile_corrects_ca(settings, info);
    let settings = if lift == 0.0 && highlights == 0.0 && shadows == 0.0 && !ca {
        Cow::Borrowed(settings)
    } else {
        let mut folded = settings.clone();
        folded.light.shadows += lift + shadows * HDR_SHADOWS;
        folded.light.highlights -= highlights * HDR_HIGHLIGHTS;
        (folded.light.hdr, folded.light.hdr_highlights, folded.light.hdr_shadows) = (0.0, 0.0, 0.0);
        folded.optics.remove_ca &= !ca;
        Cow::Owned(folded)
    };
    // Flat keeps the pipeline's own curve for raws, whose shoulder never
    // clips, in place of the camera's.
    let info = if profile == Profile::Flat && info.camera_tone.is_some() {
        Cow::Owned(SourceInfo { camera_tone: None, ..info.clone() })
    } else {
        Cow::Borrowed(info)
    };
    (settings, info)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dro_frame() -> (SourceInfo, Camera) {
        let knots = std::array::from_fn(|i| {
            let x = (i as f32 + 1.0) / 32.0;
            [x, 0.95 * x.sqrt()]
        });
        let tone = lightcraft_pipeline::tone::CameraTone::new(knots);
        assert!(tone.is_some());
        let info = SourceInfo { raw: true, camera_tone: tone, ..SourceInfo::default() };
        (info, Camera { style: Some("Standard".into()), dro: Some(Dro::Auto), lift: 30.0, fixed: true, lens: None })
    }

    #[test]
    fn a_fresh_raw_is_camera_match_and_an_edit_from_before_keeps_its_fit() {
        let fresh = fresh(5200.0, 3.0);
        assert_eq!(Profile::of(&fresh), Profile::Camera);
        assert_eq!((fresh.wb.temp, fresh.detail.sharpen_amount), (5200.0, 40.0));
        let before = DevelopSettings::for_raw(5200.0, 3.0);
        assert_eq!(Profile::of(&before), Profile::Fitted);
        for profile in [Profile::Fitted, Profile::Camera, Profile::Flat] {
            let mut settings = before.clone();
            profile.set(&mut settings);
            assert_eq!(Profile::of(&settings), profile);
            assert_eq!(profile.fitted(), profile == Profile::Fitted);
        }
        // Lightcraft's pipeline does nothing of its own with OmaLux's ids.
        assert_eq!(*lightcraft_pipeline::profiles::effective(&fresh), fresh);
    }

    #[test]
    fn camera_match_puts_the_lift_under_the_slider_and_the_others_dont() {
        let (info, camera) = dro_frame();
        let mut settings = fresh(5200.0, 3.0);
        settings.light.shadows = 10.0;
        let (folded, facts) = effective(&settings, &info, &camera);
        assert_eq!(folded.light.shadows, 40.0);
        assert!(facts.camera_tone.is_some());
        assert_eq!(settings.light.shadows, 10.0, "the slider shows what it did");

        Profile::Flat.set(&mut settings);
        let (folded, facts) = effective(&settings, &info, &camera);
        assert_eq!(folded.light.shadows, 10.0);
        assert!(facts.camera_tone.is_none(), "Flat leaves the camera's curve out");

        Profile::Fitted.set(&mut settings);
        let (folded, facts) = effective(&settings, &info, &camera);
        assert!(matches!(folded, Cow::Borrowed(_)) && matches!(facts, Cow::Borrowed(_)), "an edit from before renders as it did");
    }

    #[test]
    fn hdr_moves_highlights_and_shadows_together_past_their_ends() {
        let (info, _) = dro_frame();
        let mut settings = fresh(5200.0, 3.0);
        (settings.light.highlights, settings.light.shadows, settings.light.hdr) = (-80.0, 60.0, 50.0);
        let (folded, _) = effective(&settings, &info, &Camera::default());
        assert_eq!((folded.light.highlights, folded.light.shadows, folded.light.hdr), (-130.0, 110.0, 0.0));
        settings.light.hdr = 0.0;
        assert!(matches!(effective(&settings, &info, &Camera::default()).0, Cow::Borrowed(_)));
    }

    #[test]
    fn hdrs_highlights_and_shadows_move_its_ends_apart_within_0_and_100() {
        let (info, _) = dro_frame();
        let mut settings = fresh(5200.0, 3.0);
        let folded = |settings: &DevelopSettings| {
            let folded = effective(settings, &info, &Camera::default()).0;
            (folded.light.highlights, folded.light.shadows, folded.light.hdr_highlights, folded.light.hdr_shadows)
        };
        // Left darkens and right brightens, as Light's Highlights and Shadows.
        (settings.light.hdr, settings.light.hdr_highlights, settings.light.hdr_shadows) = (50.0, -30.0, -20.0);
        assert_eq!(folded(&settings), (-80.0, 30.0, 0.0, 0.0));
        settings.light.hdr_highlights = 30.0;
        assert_eq!(folded(&settings).0, -20.0);
        (settings.light.hdr_highlights, settings.light.hdr_shadows) = (-80.0, -90.0);
        assert_eq!(folded(&settings), (-100.0, 0.0, 0.0, 0.0), "each end between none and all of it");
        // Either end on its own, with no amount.
        (settings.light.hdr, settings.light.hdr_highlights, settings.light.hdr_shadows) = (0.0, -25.0, 40.0);
        assert_eq!(folded(&settings), (-25.0, 40.0, 0.0, 0.0));
        (settings.light.hdr_highlights, settings.light.hdr_shadows) = (25.0, -40.0);
        assert!(matches!(effective(&settings, &info, &Camera::default()).0, Cow::Borrowed(_)), "nothing below none");
    }

    #[test]
    fn a_lens_profile_with_lateral_ca_takes_over_from_the_estimate() {
        use lightcraft_develop::{EmbeddedLens, EmbeddedWarp};
        let plane = [1.0, -0.01, 0.0, 0.0, 0.0, 0.0];
        let warp = |red: f64| EmbeddedWarp { planes: [[red, -0.01, 0.0, 0.0, 0.0, 0.0], plane, plane], center: lightcraft_geom::Point::new(0.5, 0.5), radius: 0.6 };
        let lens = |red| SourceInfo { lens: Some(EmbeddedLens { warp: Some(warp(red)), ..EmbeddedLens::default() }), ..SourceInfo::default() };
        let mut settings = fresh(5200.0, 3.0);
        assert!(settings.optics.lens_profile, "a fresh raw has its profile's corrections on");
        settings.optics.remove_ca = true;
        let removed = |settings: &DevelopSettings, info: &SourceInfo| effective(settings, info, &Camera::default()).0.optics.remove_ca;
        assert!(!removed(&settings, &lens(1.001)), "the profile's lateral CA, not the estimate's");
        assert!(settings.optics.remove_ca, "the checkbox shows what was asked");
        assert!(removed(&settings, &lens(1.0)), "a profile without lateral CA leaves the estimate");
        assert!(removed(&settings, &SourceInfo::default()), "no profile, the estimate");
        settings.optics.lens_profile = false;
        assert!(removed(&settings, &lens(1.001)), "the profile off, the estimate");
    }

    #[test]
    fn the_profiles_are_named_as_lightrooms() {
        let (_, camera) = dro_frame();
        assert_eq!(Profile::Camera.label(&camera), "Camera Standard");
        assert_eq!(Profile::Camera.label(&Camera::default()), "Camera match");
        assert_eq!(Profile::Flat.label(&camera), "Flat");
        assert_eq!([Profile::Camera, Profile::Flat, Profile::Fitted].map(Profile::toggled), [Profile::Flat, Profile::Camera, Profile::Flat]);
    }
}

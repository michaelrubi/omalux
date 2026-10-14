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
    Crop,
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
        Group::Crop,
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
            Group::Crop => "Crop",
        }
    }

    /// The parts of `DevelopSettings`' JSON that are the group's.
    fn parts(self) -> &'static [Part] {
        match self {
            Group::WhiteBalance => &[("wb", &[])],
            Group::Profile => &[("profile", &[])],
            Group::Light => &[("light", &["exposure", "contrast", "highlights", "shadows", "whites", "blacks"])],
            Group::Hdr => &[("light", &["hdr"])],
            Group::Presence => &[("color", &[])],
            Group::Sharpening => &[("detail", &["sharpen_amount", "sharpen_radius", "sharpen_detail", "sharpen_masking"])],
            Group::NoiseReduction => {
                &[("detail", &["nr_luminance", "nr_detail", "nr_contrast", "nr_color", "nr_color_detail", "nr_color_smoothness"])]
            }
            Group::LensCorrections => &[("optics", &[])],
            Group::Crop => &[("crop", &[])],
        }
    }
}

/// Settings copied from a frame: the groups chosen, as they stood.
#[derive(Clone, Debug, PartialEq)]
pub struct Copied {
    groups: Vec<Group>,
    partial: Value,
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
        Self { groups: groups.to_vec(), partial: Value::Object(partial) }
    }

    pub fn groups(&self) -> &[Group] {
        &self.groups
    }

    /// `onto` with what was copied. `fresh` is the frame's own fresh
    /// settings: a white balance copied as shot is pasted as the frame's
    /// own as shot, not the Kelvin the other frame was shot at.
    pub fn paste(&self, onto: &DevelopSettings, fresh: &DevelopSettings) -> DevelopSettings {
        let mut pasted = apply_partial(onto, &self.partial, 1.0);
        if pasted.wb.mode == WbMode::AsShot {
            (pasted.wb.temp, pasted.wb.tint) = (fresh.wb.temp, fresh.wb.tint);
        }
        pasted
    }
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

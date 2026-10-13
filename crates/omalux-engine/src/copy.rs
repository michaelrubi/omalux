//! Copy and paste: the groups of settings a copy can take, and what was
//! copied. What's chosen at copy time is what's pasted; there's no second
//! choice at paste time (see "Copy and paste" in DESIGN.md).
//!
//! The groups are Lightroom's Copy Settings checklist, cut down to the
//! panels OmaLux has; a milestone that adds a panel adds its group. The
//! partial settings and their merge are Lightcraft's
//! (`lightcraft_develop::presets`), whose own groups are cut differently
//! (its Light takes HDR with it, its Detail both sharpening and noise
//! reduction).

use lightcraft_develop::{DevelopSettings, WbMode, apply_partial};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// A group of settings copied together: a panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Group {
    WhiteBalance,
    Light,
    Presence,
}

impl Group {
    /// Every group, in the panels' order: what Ctrl+C copies.
    pub const ALL: &[Group] = &[Group::WhiteBalance, Group::Light, Group::Presence];

    pub fn label(self) -> &'static str {
        match self {
            Group::WhiteBalance => "White Balance",
            Group::Light => "Light",
            Group::Presence => "Presence",
        }
    }

    /// The top-level keys of `DevelopSettings`' JSON that are the group's.
    fn keys(self) -> &'static [&'static str] {
        match self {
            Group::WhiteBalance => &["wb"],
            Group::Light => &["light"],
            Group::Presence => &["color"],
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
        for key in groups.iter().flat_map(|group| group.keys()) {
            if let Some(value) = full.get(*key) {
                partial.insert((*key).to_owned(), value.clone());
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

//! The Crop & Straighten tool (R), as Lightroom has it: the frame is
//! shown whole with the crop drawn over it, to drag by its handles or
//! turn from outside; its settings are in the panel
//! ([`crate::crop_panel`]). What each gesture does to the crop is the
//! engine's (`omalux_engine::crop`).

use std::path::{Path, PathBuf};

use lightcraft_develop::{Crop, DevelopSettings};
use omalux_engine::crop::Orientation;

/// The tool, while it's open.
#[derive(Default)]
pub struct Tool {
    /// The frame it's on, and that frame's crop and turn when the tool
    /// came to it: what Esc goes back to.
    was: Option<(PathBuf, Crop, Orientation)>,
}

impl Tool {
    /// The tool is on `path`, whose settings are `settings`: remembered
    /// the first time, for [`Self::was`].
    pub fn on(&mut self, path: &Path, settings: &DevelopSettings) {
        if self.was.as_ref().is_none_or(|(on, ..)| on != path) {
            self.was = Some((path.to_path_buf(), settings.crop, settings.orientation));
        }
    }

    /// The crop and turn `path` had when the tool came to it.
    pub fn was(&self, path: &Path) -> Option<(Crop, Orientation)> {
        self.was.as_ref().filter(|(on, ..)| on == path).map(|(_, crop, orientation)| (*crop, *orientation))
    }
}

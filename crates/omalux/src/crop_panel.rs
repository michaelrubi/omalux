//! The Crop & Straighten tool's settings, in the develop panel under the
//! histogram while the tool is open, as Lightroom's tool drawer is: the
//! aspect and its lock, the angle, Auto, the quarter turns, Reset and
//! Close.

use egui::Ui;
use lightcraft_develop::DevelopSettings;
use omalux_engine::crop::Aspect;

use crate::slider::SliderOut;
use crate::theme::Theme;

/// What was done in the crop panel this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Out {
    /// An aspect was chosen from the menu.
    pub aspect: Option<Aspect>,
    /// The lock was clicked: locked, the crop keeps its shape when it's
    /// dragged.
    pub lock: bool,
    /// Swap the aspect, landscape to portrait and back (Lightroom's X).
    pub swap: bool,
    /// What happened to the Angle slider.
    pub angle: SliderOut,
    /// Auto was clicked: level the frame.
    pub level: bool,
    /// A quarter turn was asked for: clockwise if true.
    pub turn: Option<bool>,
    pub reset: bool,
    /// Close was clicked.
    pub done: bool,
}

/// The crop panel showing `settings`' crop of a frame `dims` in size as
/// it's shown. `locked` is whether the crop keeps its shape when dragged.
pub fn show(_ui: &mut Ui, _theme: &Theme, _settings: &DevelopSettings, _dims: (f64, f64), _locked: bool) -> Out {
    // M3's crop panel branch draws it.
    Out::default()
}

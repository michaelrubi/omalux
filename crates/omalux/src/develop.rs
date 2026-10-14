//! The develop panel's sliders, in Lightroom's order under Lightroom's
//! names: the Profile, White Balance, Light, HDR, Presence, Detail, Lens
//! Corrections. Which controls there are, their ranges and what they do
//! are Lightcraft's (`lightcraft_develop::controls`), all but HDR, which
//! is OmaLux's (`omalux_engine::look`).

use egui::{Align2, FontId, Sense, Ui, vec2};
use lightcraft_develop::controls::{self, ControlSpec};
use lightcraft_develop::{DevelopSettings, WbMode};
use omalux_engine::auto::Auto;
use omalux_engine::look::{Camera, Profile};

use crate::commands::Command;
use crate::hotkeys::{self, format_shortcut};
use crate::slider::{SliderOut, slider};
use crate::theme::Theme;

/// A row of a panel.
enum Row {
    /// A slider, by its control's id, under Lightcraft's label.
    Slider(&'static str),
    /// A slider under Lightroom's name, where Lightcraft's label is
    /// another (its Amount is "Sharpening", and two of its Detail
    /// controls are both "Detail").
    Named(&'static str, &'static str),
    /// A heading inside a panel, as Lightroom's Detail has.
    Sub(&'static str),
    /// The Remove Chromatic Aberration checkbox.
    RemoveCa,
    /// The Enable Profile Corrections checkbox, and the lens the profile
    /// is for.
    ProfileCorrections,
}

use Row::{Named, ProfileCorrections, RemoveCa, Slider, Sub};

/// The panels and their rows, top to bottom: the order `,` and `.` step
/// through the sliders in. Lightcraft's other noise-reduction controls
/// aren't shown. Lightroom draws each of Defringe's hue ranges as one
/// two-thumbed slider, which neither Lightcraft nor OmaLux has: they're
/// two sliders each here, under Lightcraft's labels.
const PANELS: &[(&str, &[Row])] = &[
    // Where Lightroom's Basic panel has it: above the white balance.
    ("Profile", &[]),
    ("White Balance", &[Slider("wb.temp"), Slider("wb.tint")]),
    (
        "Light",
        &[
            Slider("light.exposure"),
            Slider("light.contrast"),
            Slider("light.highlights"),
            Slider("light.shadows"),
            Slider("light.whites"),
            Slider("light.blacks"),
        ],
    ),
    ("HDR", &[Named("light.hdr", "Amount")]),
    ("Presence", &[Slider("color.vibrance"), Slider("color.saturation")]),
    (
        "Detail",
        &[
            Sub("Sharpening"),
            Named("detail.sharpenAmount", "Amount"),
            Slider("detail.sharpenRadius"),
            Slider("detail.sharpenDetail"),
            Slider("detail.sharpenMasking"),
            Sub("Noise Reduction"),
            Named("detail.nrLuminance", "Luminance"),
            Named("detail.nrColor", "Color"),
        ],
    ),
    (
        "Lens Corrections",
        &[
            RemoveCa,
            ProfileCorrections,
            Sub("Amount"),
            Named("optics.profileDistortion", "Distortion"),
            Named("optics.profileVignetting", "Vignetting"),
            Sub("Defringe"),
            Slider("optics.defringePurple"),
            Slider("optics.defringePurpleHueLo"),
            Slider("optics.defringePurpleHueHi"),
            Slider("optics.defringeGreen"),
            Slider("optics.defringeGreenHueLo"),
            Slider("optics.defringeGreenHueHi"),
        ],
    ),
];

impl Row {
    /// The row's control as it's shown, if it's a slider.
    fn spec(&self) -> Option<ControlSpec> {
        match self {
            Slider(id) => controls::find(id).copied(),
            Named(id, label) => controls::find(id).map(|spec| ControlSpec { label, ..*spec }),
            Sub(_) | RemoveCa | ProfileCorrections => None,
        }
    }
}

/// Every slider's control, in order, as it's shown.
pub fn sliders() -> impl Iterator<Item = ControlSpec> {
    PANELS.iter().flat_map(|(_, rows)| rows.iter()).filter_map(Row::spec)
}

/// What was done in the panel this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PanelOut {
    /// The slider touched, by its place in [`sliders`], and what happened
    /// to it.
    pub slider: Option<(usize, SliderOut)>,
    /// The profile was clicked: the one to go to.
    pub profile: Option<Profile>,
    /// As Shot was clicked.
    pub as_shot: bool,
    /// An Auto button was clicked.
    pub auto: Option<Auto>,
    /// Remove Chromatic Aberration was ticked, or unticked.
    pub remove_ca: Option<bool>,
    /// Enable Profile Corrections was ticked, or unticked.
    pub lens_profile: Option<bool>,
}

/// A panel's title, and the row it's in, for what goes at its right.
pub fn heading(ui: &mut Ui, theme: &Theme, title: &str) -> egui::Rect {
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::hover());
    ui.painter().text(row.left_center() + vec2(4.0, 2.0), Align2::LEFT_CENTER, title, FontId::proportional(13.5), theme.dark_foreground);
    row
}

/// A heading inside a panel: smaller and dimmer than the panel's own.
fn sub_heading(ui: &mut Ui, theme: &Theme, title: &str) {
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 20.0), Sense::hover());
    ui.painter().text(row.left_center() + vec2(4.0, 1.0), Align2::LEFT_CENTER, title, FontId::proportional(11.5), theme.muted);
}

/// What a command's button says when it's hovered: its name and its key.
fn tip(command: Command) -> String {
    let key = hotkeys::current().command(command).map(|s| format!(" ({})", format_shortcut(&s))).unwrap_or_default();
    format!("{}{key}", command.label())
}

/// A word to click at the right of a heading's row, ending at `right`,
/// which it moves left of itself for the next. `clickable` or not, it
/// takes its room.
fn word(ui: &mut Ui, right: &mut egui::Pos2, text: &str, colour: egui::Color32, clickable: Option<&str>) -> bool {
    let galley = ui.painter().layout_no_wrap(text.to_owned(), FontId::proportional(12.5), colour);
    let rect = Align2::RIGHT_CENTER.anchor_size(*right, galley.size());
    right.x = rect.left() - 10.0;
    ui.painter().galley(rect.min, galley, colour);
    let Some(hover) = clickable else { return false };
    ui.interact(rect.expand(3.0), ui.id().with(text), Sense::click()).on_hover_text(hover).clicked()
}

/// The sliders showing `settings`. `fresh` is what a double-click resets
/// each to; `selected` is the slider the keys act on; `camera` is what
/// the frame's camera did, which names its profile.
pub fn show(ui: &mut Ui, theme: &Theme, settings: &DevelopSettings, fresh: &DevelopSettings, selected: usize, camera: &Camera) -> PanelOut {
    let mut out = PanelOut::default();
    let mut index = 0;
    for (title, rows) in PANELS {
        let row = heading(ui, theme, title);
        let mut right = row.right_center() + vec2(-4.0, 2.0);
        match *title {
            "Profile" => {
                // Lightroom's "Profile: Camera Standard", with a click
                // where it has a menu: there are two to choose from.
                let profile = Profile::of(settings);
                if word(ui, &mut right, &profile.label(camera), theme.foreground, Some(&tip(Command::Flat))) {
                    out.profile = Some(profile.toggled());
                }
            }
            "White Balance" => {
                // Lightroom's "WB: As Shot": what the white balance is, and
                // a click to put it back; and Auto beside it.
                out.as_shot = match settings.wb.mode {
                    WbMode::AsShot => word(ui, &mut right, "As Shot", theme.dark_foreground, None),
                    WbMode::Auto => word(ui, &mut right, "Auto", theme.foreground, Some("Back to As Shot")),
                    _ => word(ui, &mut right, "Custom", theme.foreground, Some("Back to As Shot")),
                };
                if settings.wb.mode != WbMode::Auto && word(ui, &mut right, "Auto", theme.foreground, Some(&tip(Command::AutoWhiteBalance))) {
                    out.auto = Some(Auto::WhiteBalance);
                }
            }
            "Light" => {
                if word(ui, &mut right, "Auto Tone", theme.foreground, Some(&tip(Command::AutoTone))) {
                    out.auto = Some(Auto::Tone);
                }
                if word(ui, &mut right, "Auto Exposure", theme.foreground, Some(&tip(Command::AutoExposure))) {
                    out.auto = Some(Auto::Exposure);
                }
            }
            _ => {}
        }
        for row in *rows {
            match row {
                Sub(title) => sub_heading(ui, theme, title),
                RemoveCa => {
                    let mut on = settings.optics.remove_ca;
                    if ui.checkbox(&mut on, "Remove Chromatic Aberration").changed() {
                        out.remove_ca = Some(on);
                    }
                }
                ProfileCorrections => {
                    let mut on = settings.optics.lens_profile;
                    if ui.checkbox(&mut on, "Enable Profile Corrections").changed() {
                        out.lens_profile = Some(on);
                    }
                    // Lightroom's Lens Profile: which lens it found, or that
                    // it found none (and the checkbox does nothing).
                    let lens = camera.lens.as_deref().unwrap_or("No profile for this lens");
                    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 20.0), Sense::hover());
                    ui.painter().text(row.left_center() + vec2(4.0, 1.0), Align2::LEFT_CENTER, lens, FontId::proportional(11.5), theme.muted);
                }
                Slider(id) | Named(id, _) => {
                    let Some(spec) = row.spec() else { continue };
                    let value = controls::get(settings, id).unwrap_or(spec.default);
                    let reset_to = controls::get(fresh, id).unwrap_or(spec.default);
                    let slid = slider(ui, theme, &spec, value, reset_to, index == selected);
                    if slid != SliderOut::default() {
                        out.slider = Some((index, slid));
                    }
                    index += 1;
                }
            }
        }
        ui.add_space(6.0);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sliders_are_lightrooms_in_lightrooms_order() {
        let labels: Vec<&str> = sliders().map(|spec| spec.label).collect();
        assert_eq!(labels, [
            "Temp", "Tint",
            "Exposure", "Contrast", "Highlights", "Shadows", "Whites", "Blacks",
            "Amount",
            "Vibrance", "Saturation",
            "Amount", "Radius", "Detail", "Masking", "Luminance", "Color",
            "Distortion", "Vignetting",
            "Purple Amount", "Purple Hue Low", "Purple Hue High", "Green Amount", "Green Hue Low", "Green Hue High",
        ]);
    }
}

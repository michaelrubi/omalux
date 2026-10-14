//! The develop panel's sliders, in Lightroom's order under Lightroom's
//! names: White Balance, Light, Presence, Detail, Lens Corrections. Which
//! controls there are, their ranges and what they do are Lightcraft's
//! (`lightcraft_develop::controls`).

use egui::{Align2, FontId, Sense, Ui, pos2, vec2};
use lightcraft_develop::controls::{self, ControlSpec};
use lightcraft_develop::{DevelopSettings, WbMode};
use omalux_engine::auto::Auto;

use crate::commands::Command;
use crate::hotkeys::{self, format_shortcut};
use crate::slider::{SliderOut, slider};
use crate::theme::Theme;

/// The panels and their controls, top to bottom: the order `,` and `.`
/// step through.
const PANELS: &[(&str, &[&str])] = &[
    ("White Balance", &["wb.temp", "wb.tint"]),
    ("Light", &["light.exposure", "light.contrast", "light.highlights", "light.shadows", "light.whites", "light.blacks"]),
    ("Presence", &["color.vibrance", "color.saturation"]),
    ("Detail", &[
        "detail.sharpenAmount",
        "detail.sharpenRadius",
        "detail.sharpenDetail",
        "detail.sharpenMasking",
        "detail.nrLuminance",
        "detail.nrColor",
    ]),
    ("Lens Corrections", &[
        "optics.defringePurple",
        "optics.defringePurpleHueLo",
        "optics.defringePurpleHueHi",
        "optics.defringeGreen",
        "optics.defringeGreenHueLo",
        "optics.defringeGreenHueHi",
    ]),
];

/// Every slider's control, in order.
pub fn sliders() -> impl Iterator<Item = &'static ControlSpec> {
    PANELS.iter().flat_map(|(_, ids)| ids.iter()).filter_map(|id| controls::find(id))
}

/// What was done in the panel this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PanelOut {
    /// The slider touched, by its place in [`sliders`], and what happened
    /// to it.
    pub slider: Option<(usize, SliderOut)>,
    /// As Shot was clicked.
    pub as_shot: bool,
    /// An Auto button was clicked.
    pub auto: Option<Auto>,
    /// Remove Chromatic Aberration was ticked, or unticked.
    pub remove_ca: Option<bool>,
}

/// A panel's title, and the row it's in, for what goes at its right.
pub fn heading(ui: &mut Ui, theme: &Theme, title: &str) -> egui::Rect {
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::hover());
    ui.painter().text(row.left_center() + vec2(4.0, 2.0), Align2::LEFT_CENTER, title, FontId::proportional(13.5), theme.dark_foreground);
    row
}

fn sub_heading(ui: &mut Ui, theme: &Theme, title: &str) {
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 20.0), Sense::hover());
    ui.painter().text(row.left_center() + vec2(4.0, 1.0), Align2::LEFT_CENTER, title, FontId::proportional(11.5), theme.muted);
}

fn action_tip(command: Command) -> String {
    let how = hotkeys::current().command(command).map(|s| format!(" ({})", format_shortcut(&s))).unwrap_or_default();
    format!("{}{how}", command.label())
}

fn header_button(ui: &mut Ui, right: &mut f32, y: f32, theme: &Theme, id_str: &str, text: &str, tip: &str) -> bool {
    let colour = theme.foreground;
    let galley = ui.painter().layout_no_wrap(text.to_owned(), FontId::proportional(12.5), colour);
    let rect = Align2::RIGHT_CENTER.anchor_size(pos2(*right, y), galley.size());
    *right = rect.min.x - 8.0;
    let response = ui.interact(rect.expand(3.0), ui.id().with(id_str), Sense::click());
    ui.painter().galley(rect.min, galley, colour);
    response.on_hover_text(tip).clicked()
}

fn label_override(id: &str) -> Option<&'static str> {
    match id {
        "detail.sharpenAmount" => Some("Amount"),
        "detail.nrLuminance" => Some("Luminance"),
        "detail.nrColor" => Some("Color"),
        _ => None,
    }
}

/// The sliders showing `settings`. `fresh` is what a double-click resets
/// each to; `selected` is the slider the keys act on.
pub fn show(ui: &mut Ui, theme: &Theme, settings: &DevelopSettings, fresh: &DevelopSettings, selected: usize) -> PanelOut {
    let mut out = PanelOut::default();
    let mut index = 0;
    for (title, ids) in PANELS {
        let row = heading(ui, theme, title);
        let mut right = row.right() - 4.0;
        let y = row.center().y + 2.0;
        if *title == "White Balance" {
            // Lightroom's "WB: As Shot": what the white balance is, and a
            // click to put it back.
            let as_shot = settings.wb.mode == WbMode::AsShot;
            let (text, colour) = match settings.wb.mode {
                WbMode::AsShot => ("As Shot", theme.dark_foreground),
                WbMode::Auto => ("Auto", theme.foreground),
                _ => ("Custom", theme.foreground),
            };
            let galley = ui.painter().layout_no_wrap(text.to_owned(), FontId::proportional(12.5), colour);
            let rect = Align2::RIGHT_CENTER.anchor_size(pos2(right, y), galley.size());
            right = rect.min.x - 8.0;
            let response = ui.interact(rect.expand(3.0), ui.id().with("as shot"), if as_shot { Sense::hover() } else { Sense::click() });
            ui.painter().galley(rect.min, galley, colour);
            if !as_shot && response.on_hover_text("Back to As Shot").clicked() {
                out.as_shot = true;
            }
            if !matches!(settings.wb.mode, WbMode::Auto)
                && header_button(ui, &mut right, y, theme, "wb auto", "Auto", &action_tip(Command::AutoWhiteBalance))
            {
                out.auto = Some(Auto::WhiteBalance);
            }
        } else if *title == "Light" {
            if header_button(ui, &mut right, y, theme, "auto tone", "Auto Tone", &action_tip(Command::AutoTone)) {
                out.auto = Some(Auto::Tone);
            }
            if header_button(ui, &mut right, y, theme, "auto exposure", "Auto Exposure", &action_tip(Command::AutoExposure)) {
                out.auto = Some(Auto::Exposure);
            }
        } else if *title == "Lens Corrections" {
            let mut remove_ca = settings.optics.remove_ca;
            let response = ui.checkbox(&mut remove_ca, "Remove Chromatic Aberration");
            if response.changed() || response.clicked() {
                out.remove_ca = Some(remove_ca);
            }
        }
        for id in *ids {
            if *id == "detail.sharpenAmount" {
                sub_heading(ui, theme, "Sharpening");
            } else if *id == "detail.nrLuminance" {
                sub_heading(ui, theme, "Noise Reduction");
            } else if *id == "optics.defringePurple" {
                // Lightroom draws each hue range as one two-thumbed slider, which
                // neither Lightcraft nor OmaLux has.
                sub_heading(ui, theme, "Defringe");
            }
            let Some(spec) = controls::find(id) else { continue };
            let mut spec = *spec;
            if let Some(label) = label_override(spec.id) {
                spec.label = label;
            }
            let value = controls::get(settings, id).unwrap_or(spec.default);
            let reset_to = controls::get(fresh, id).unwrap_or(spec.default);
            let slid = slider(ui, theme, &spec, value, reset_to, index == selected);
            if slid != SliderOut::default() {
                out.slider = Some((index, slid));
            }
            index += 1;
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
            "Temp", "Tint", "Exposure", "Contrast", "Highlights", "Shadows", "Whites", "Blacks", "Vibrance", "Saturation",
            "Sharpening", "Radius", "Detail", "Masking", "Noise Reduction", "Color Noise Reduction",
            "Purple Amount", "Purple Hue Low", "Purple Hue High", "Green Amount", "Green Hue Low", "Green Hue High",
        ]);
    }
}

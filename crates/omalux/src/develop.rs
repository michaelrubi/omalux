//! The develop panel's sliders, in Lightroom's order under Lightroom's
//! names: White Balance, Light, Presence. Which controls there are, their
//! ranges and what they do are Lightcraft's (`lightcraft_develop::controls`).

use egui::{Align2, FontId, Sense, Ui, vec2};
use lightcraft_develop::controls::{self, ControlSpec};
use lightcraft_develop::{DevelopSettings, WbMode};
use omalux_engine::auto::Auto;

use crate::slider::{SliderOut, slider};
use crate::theme::Theme;

/// The panels and their controls, top to bottom: the order `,` and `.`
/// step through.
const PANELS: &[(&str, &[&str])] = &[
    ("White Balance", &["wb.temp", "wb.tint"]),
    ("Light", &["light.exposure", "light.contrast", "light.highlights", "light.shadows", "light.whites", "light.blacks"]),
    ("Presence", &["color.vibrance", "color.saturation"]),
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

/// The sliders showing `settings`. `fresh` is what a double-click resets
/// each to; `selected` is the slider the keys act on.
pub fn show(ui: &mut Ui, theme: &Theme, settings: &DevelopSettings, fresh: &DevelopSettings, selected: usize) -> PanelOut {
    let mut out = PanelOut::default();
    let mut index = 0;
    for (title, ids) in PANELS {
        let row = heading(ui, theme, title);
        if *title == "White Balance" {
            // Lightroom's "WB: As Shot": what the white balance is, and a
            // click to put it back.
            let as_shot = settings.wb.mode == WbMode::AsShot;
            let (text, colour) = if as_shot { ("As Shot", theme.dark_foreground) } else { ("Custom", theme.foreground) };
            let galley = ui.painter().layout_no_wrap(text.to_owned(), FontId::proportional(12.5), colour);
            let rect = Align2::RIGHT_CENTER.anchor_size(row.right_center() + vec2(-4.0, 2.0), galley.size());
            let response = ui.interact(rect.expand(3.0), ui.id().with("as shot"), Sense::click());
            ui.painter().galley(rect.min, galley, colour);
            if !as_shot {
                out.as_shot = response.on_hover_text("Back to As Shot").clicked();
            }
        }
        for id in *ids {
            let Some(spec) = controls::find(id) else { continue };
            let value = controls::get(settings, id).unwrap_or(spec.default);
            let reset_to = controls::get(fresh, id).unwrap_or(spec.default);
            let slid = slider(ui, theme, spec, value, reset_to, index == selected);
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
        assert_eq!(labels, ["Temp", "Tint", "Exposure", "Contrast", "Highlights", "Shadows", "Whites", "Blacks", "Vibrance", "Saturation"]);
    }
}

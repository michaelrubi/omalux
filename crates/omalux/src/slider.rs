//! Lightroom's slider: a name and a value over a thin track with a ring
//! for a thumb. Drag the track or click it to set (Shift drags finely),
//! double-click the name or the thumb to reset (with Shift, to set it
//! automatically), and with the pointer over the selected slider the
//! wheel nudges it.
//!
//! Lifted from Lightcraft's `slider` (`ui-egui/src/widgets.rs` at
//! `701f78e`, MIT OR Apache-2.0) and restyled to Omarchy's theme, with
//! its typed values (click the number to type one) and without its
//! automation ids. One difference from it, for Lightroom's sake: Temp's
//! track is spaced by ratio, not by Kelvin, so daylight isn't squeezed
//! into the track's first tenth.

use egui::{
    Align2, Color32, CursorIcon, FontId, Key, Layout, Rect, Sense, Stroke, Ui, UiBuilder, pos2,
    text::{CCursor, CCursorRange},
    text_edit::TextEditState,
    vec2,
};
use lightcraft_develop::{ControlSpec, Track};

use crate::theme::Theme;

pub const ROW_HEIGHT: f32 = 40.0;
const RING: f32 = 6.0;
const VALUE_WIDTH: f32 = 64.0;

/// What happened to a slider this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SliderOut {
    /// The value it was moved to.
    pub value: Option<f64>,
    /// A drag began, or ended: a drag's changes are one step to undo.
    pub drag_started: bool,
    pub drag_stopped: bool,
    /// It was clicked or dragged: it's the selected slider now.
    pub touched: bool,
    /// Shift+double-click: set it automatically.
    pub auto: bool,
}

fn hex(s: &str) -> Color32 {
    Color32::from_hex(s).unwrap_or(Color32::GRAY)
}

fn lerp(a: Color32, b: Color32, t: f32) -> Color32 {
    let f = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t.clamp(0.0, 1.0)).round() as u8;
    Color32::from_rgb(f(a.r(), b.r()), f(a.g(), b.g()), f(a.b(), b.b()))
}

/// The colours along a track that has them: Temp's blue to yellow, Tint's
/// green to magenta, a control's own gradient. Lightcraft's.
fn track_stops(track: &Track) -> Option<Vec<Color32>> {
    let stops = |hexes: &[&str]| hexes.iter().map(|h| hex(h)).collect();
    match track {
        Track::Temp => Some(stops(&["#193961", "#47566a", "#60666d", "#70705b", "#737339", "#75742a"])),
        Track::Tint => Some(stops(&["#246123", "#2c552c", "#394739", "#4a3d4c", "#6b336b", "#872c87"])),
        Track::Gradient { from, to } => Some(vec![hex(from), hex(to)]),
        _ => None,
    }
}

/// Where along the track (0 to 1) a value sits. Temp by ratio: each
/// doubling of the Kelvin takes the same length of track.
fn position(spec: &ControlSpec, value: f64) -> f64 {
    let at = match spec.track {
        Track::Temp => (value / spec.min).ln() / (spec.max / spec.min).ln(),
        _ => (value - spec.min) / (spec.max - spec.min).max(1e-9),
    };
    at.clamp(0.0, 1.0)
}

/// The value at a place along the track, on the control's steps.
fn value_at(spec: &ControlSpec, at: f64) -> f64 {
    let at = at.clamp(0.0, 1.0);
    let value = match spec.track {
        Track::Temp => spec.min * (spec.max / spec.min).powf(at),
        _ => spec.min + at * (spec.max - spec.min),
    };
    stepped(spec, value)
}

fn stepped(spec: &ControlSpec, value: f64) -> f64 {
    let step = spec.step.max(1e-9);
    ((value / step).round() * step).clamp(spec.min, spec.max)
}

/// `value` moved by `steps` nudges of the keyboard or the wheel: one is
/// about 1/200 of the range, a whole number of the control's steps
/// (Exposure 0.05, most sliders 1, Temp 50 K). Lightcraft's.
pub fn nudged(spec: &ControlSpec, value: f64, steps: f64) -> f64 {
    let step = spec.step.max(1e-9);
    let unit = if spec.id == "wb.temp" { 50.0 } else { ((spec.max - spec.min) / 200.0 / step).round().max(1.0) * step };
    stepped(spec, value + unit * steps)
}

/// Wheel movement as nudges, keeping what's left of a notch for the next
/// frame so a trackpad's small movements add up. Lightcraft's.
fn wheel_nudges(remainder: &mut f32, delta: f32) -> f64 {
    const POINTS_PER_STEP: f32 = 40.0;
    let total = (*remainder + delta).clamp(-400.0, 400.0);
    let steps = (total / POINTS_PER_STEP).trunc();
    *remainder = total - steps * POINTS_PER_STEP;
    f64::from(steps)
}

/// A value as shown beside the slider's name: Kelvin plain, the rest
/// signed, and zero just "0".
pub fn shown_value(spec: &ControlSpec, value: f64) -> String {
    let shown = if spec.id == "wb.temp" { format!("{value:.0}") } else { spec.format(value) };
    if shown.trim_start_matches(['+', '-']).chars().all(|c| c == '0' || c == '.') { "0".to_owned() } else { shown }
}

/// What a value typed into a slider asks for: a number such as `0.5`,
/// `+12`, `-1,5` (comma or point) or `5600 K`, on the control's steps and
/// clamped to its range. `None` when it isn't a finite number, so the
/// slider keeps its value. Lightcraft's.
pub fn typed_value(spec: &ControlSpec, text: &str) -> Option<f64> {
    let t = text.trim().replace(',', ".");
    let t = t.trim_end_matches(|c: char| c == '%' || c == 'K' || c == 'k' || c.is_whitespace());
    let v: f64 = t.parse().ok().filter(|v: &f64| v.is_finite())?;
    Some(stepped(spec, v))
}

/// Select all the text in the field, so typing replaces it. Lifted from
/// Lightcraft's `text_field::select_all` (`ui-egui/src/text_field.rs` at `701f78e`).
fn select_all(ctx: &egui::Context, id: egui::Id, text: &str) {
    let mut state = TextEditState::load(ctx, id).unwrap_or_default();
    let end = CCursor::new(text.chars().count());
    state.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(0), end)));
    state.store(ctx, id);
}

/// A slider row. `reset_to` is what a double-click sets (the control's
/// default, or the as-shot value for white balance); `selected` marks the
/// slider the keys act on.
pub fn slider(ui: &mut Ui, theme: &Theme, spec: &ControlSpec, value: f64, reset_to: f64, selected: bool) -> SliderOut {
    let width = ui.available_width();
    let (row, _) = ui.allocate_exact_size(vec2(width, ROW_HEIGHT), Sense::hover());
    let label = Rect::from_min_size(pos2(row.left() + 4.0, row.top() + 2.0), vec2(width - 8.0, 18.0));
    let track = Rect::from_min_max(pos2(row.left() + 4.0 + RING, row.top() + 20.0), pos2(row.right() - 4.0 - RING, row.top() + 36.0));
    let id = ui.id().with(spec.id);
    let response = ui.interact(track.expand2(vec2(RING, 2.0)), id, Sense::click_and_drag());
    let value_rect = Rect::from_min_max(pos2(label.right() - VALUE_WIDTH, label.top()), label.max);
    let name_rect = Rect::from_min_max(label.min, pos2(label.right() - VALUE_WIDTH, label.max.y));
    // Where each slider and its value are, for the tests.
    #[cfg(test)]
    ui.data_mut(|d| {
        d.insert_temp(egui::Id::new(spec.id), track);
        d.insert_temp(egui::Id::new((spec.id, "value")), value_rect);
    });
    let label_response = ui.interact(name_rect, id.with("label"), Sense::click());
    let typing_id = id.with("typing");
    let field_id = id.with("field");
    let mut typing: Option<String> = ui.data(|m| m.get_temp(typing_id));
    let is_typing = typing.is_some();
    let value_response = ui.interact(value_rect, id.with("value"), if is_typing { Sense::hover() } else { Sense::click() });
    let opened = typing.is_none() && value_response.clicked();
    if opened {
        let text = shown_value(spec, value).trim_start_matches('+').to_string();
        ui.memory_mut(|m| m.request_focus(field_id));
        typing = Some(text);
    }
    let was_typing = is_typing || opened;

    let mut out = SliderOut {
        touched: response.clicked() || response.drag_started() || label_response.clicked() || value_response.clicked(),
        ..SliderOut::default()
    };
    let at_pointer = |x: f32| value_at(spec, f64::from((x - track.left()) / track.width()));
    let mut shown = value;
    let font = FontId::proportional(12.5);
    let text_color = if selected { theme.accent } else { theme.foreground };

    if let Some(mut text) = typing.take() {
        let mut child = ui.new_child(UiBuilder::new().max_rect(value_rect).layout(Layout::right_to_left(egui::Align::Center)));
        let edit = egui::TextEdit::singleline(&mut text)
            .id(field_id)
            .font(font.clone())
            .text_color(text_color)
            .horizontal_align(egui::Align::RIGHT)
            .desired_width(value_rect.width())
            .margin(egui::Margin::ZERO)
            .frame(egui::Frame::NONE);
        let edit_response = child.add(edit);
        if opened {
            edit_response.request_focus();
            select_all(ui.ctx(), field_id, &text);
        }
        let escape = ui.input(|i| i.key_pressed(Key::Escape));
        let enter = ui.input(|i| i.key_pressed(Key::Enter));
        if !opened && (edit_response.lost_focus() || escape || enter) {
            if escape {
                ui.memory_mut(|m| m.surrender_focus(field_id));
            } else if let Some(nv) = typed_value(spec, &text)
                && (nv - value).abs() > 1e-12
            {
                out.value = Some(nv);
                out.drag_started = true;
                out.drag_stopped = true;
                out.touched = true;
                shown = nv;
            }
            ui.memory_mut(|m| m.surrender_focus(field_id));
            ui.data_mut(|m| m.remove::<String>(typing_id));
        } else {
            ui.data_mut(|m| m.insert_temp(typing_id, text));
        }
    } else if response.double_clicked() || label_response.double_clicked() {
        // With Shift, Lightroom sets it automatically; without, it's put back.
        out = if ui.input(|i| i.modifiers.shift) {
            SliderOut { touched: true, auto: true, ..SliderOut::default() }
        } else {
            SliderOut { value: Some(reset_to), drag_started: true, drag_stopped: true, touched: true, auto: false }
        };
    } else if (response.dragged() || response.drag_started())
        && let Some(pointer) = response.interact_pointer_pos()
    {
        out.drag_started = response.drag_started();
        // Shift: a tenth of the pointer's movement.
        let to = if ui.input(|i| i.modifiers.shift) {
            let moved = f64::from(ui.input(|i| i.pointer.delta().x) / track.width()) * 0.1;
            value_at(spec, position(spec, value) + moved)
        } else {
            at_pointer(pointer.x)
        };
        out.value = Some(to);
    } else if response.clicked()
        && let Some(pointer) = response.interact_pointer_pos()
    {
        out = SliderOut { value: Some(at_pointer(pointer.x)), drag_started: true, drag_stopped: true, touched: true, auto: false };
    } else if selected && !was_typing && ui.rect_contains_pointer(row) {
        // The wheel nudges the selected slider only, so over any other it
        // still scrolls the panel.
        let delta = ui.input_mut(|i| std::mem::take(&mut i.smooth_scroll_delta.y));
        if delta != 0.0 {
            let wheel = id.with("wheel");
            let mut remainder = ui.data(|d| d.get_temp::<f32>(wheel)).unwrap_or(0.0);
            let steps = wheel_nudges(&mut remainder, delta);
            ui.data_mut(|d| d.insert_temp(wheel, remainder));
            if steps != 0.0 {
                out = SliderOut { value: Some(nudged(spec, value, steps)), drag_started: true, drag_stopped: true, touched: false, auto: false };
            }
        }
    }
    out.drag_stopped |= response.drag_stopped();
    if out.value.is_some_and(|to| (to - value).abs() < 1e-12) {
        out.value = None;
    }
    if let Some(to) = out.value {
        shown = to;
    }

    let painter = ui.painter();
    painter.text(label.left_center(), Align2::LEFT_CENTER, spec.label, font.clone(), text_color);
    if !was_typing {
        painter.text(label.right_center(), Align2::RIGHT_CENTER, shown_value(spec, shown), font, text_color);
        if value_response.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::Text);
        }
    }
    let y = track.center().y;
    match track_stops(&spec.track) {
        Some(stops) => {
            let last = stops.len() - 1;
            const STEPS: usize = 48;
            for i in 0..STEPS {
                let (from, to) = (i as f32 / STEPS as f32, (i + 1) as f32 / STEPS as f32);
                let along = from * last as f32;
                let colour = lerp(stops[(along as usize).min(last)], stops[(along as usize + 1).min(last)], along.fract());
                let segment = Rect::from_min_max(pos2(track.left() + track.width() * from, y - 1.5), pos2(track.left() + track.width() * to + 0.5, y + 1.5));
                painter.rect_filled(segment, 0.0, colour);
            }
        }
        None => drop(painter.rect_filled(Rect::from_min_max(pos2(track.left(), y - 1.0), pos2(track.right(), y + 1.0)), 1.0, theme.muted)),
    }
    let thumb = pos2(track.left() + position(spec, shown) as f32 * track.width(), y);
    let hovered = response.hovered() || response.dragged();
    painter.circle_filled(thumb, RING, theme.dark_background);
    painter.circle_stroke(thumb, RING, Stroke::new(2.0, if hovered || selected { theme.accent } else { theme.foreground }));
    if hovered {
        ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use lightcraft_develop::controls::find;

    #[test]
    fn nudges_are_whole_steps_about_a_two_hundredth_of_the_range() {
        let exposure = find("light.exposure").unwrap();
        assert!((nudged(exposure, 0.0, 1.0) - 0.05).abs() < 1e-9);
        assert!((nudged(exposure, 0.0, -5.0) + 0.25).abs() < 1e-9, "Shift: five at once");
        assert_eq!(nudged(exposure, 4.99, 1.0), 5.0, "stops at the end");
        assert_eq!(nudged(find("light.contrast").unwrap(), 10.0, 1.0), 11.0);
        assert_eq!(nudged(find("wb.temp").unwrap(), 5200.0, -1.0), 5150.0);
    }

    #[test]
    fn temps_track_is_spaced_by_ratio_and_the_rest_evenly() {
        let temp = find("wb.temp").unwrap();
        assert_eq!((position(temp, 2000.0), position(temp, 50000.0)), (0.0, 1.0));
        assert!((position(temp, 10000.0) - 0.5).abs() < 1e-9, "the middle of 2000 and 50000 by ratio");
        assert_eq!(value_at(temp, 0.5), 10000.0);
        assert_eq!(value_at(temp, position(temp, 5200.0)), 5200.0);
        let contrast = find("light.contrast").unwrap();
        assert_eq!((position(contrast, 0.0), value_at(contrast, 0.75), value_at(contrast, 2.0)), (0.5, 50.0, 100.0));
    }

    #[test]
    fn values_are_shown_as_lightroom_shows_them() {
        let exposure = find("light.exposure").unwrap();
        assert_eq!(shown_value(exposure, 0.0), "0");
        assert_eq!(shown_value(exposure, -0.001), "0");
        assert_eq!(shown_value(exposure, 0.5), "+0.50");
        assert_eq!(shown_value(find("light.shadows").unwrap(), -30.0), "-30");
        assert_eq!(shown_value(find("wb.temp").unwrap(), 5200.0), "5200");
    }

    #[test]
    fn the_wheel_adds_up_small_movements() {
        let mut remainder = 0.0;
        assert_eq!(wheel_nudges(&mut remainder, 30.0), 0.0);
        assert_eq!(wheel_nudges(&mut remainder, 30.0), 1.0);
        assert_eq!(remainder, 20.0);
        assert_eq!(wheel_nudges(&mut remainder, -100.0), -2.0);
    }

    #[test]
    fn typed_values_are_stepped_and_clamped() {
        let exposure = find("light.exposure").unwrap();
        let contrast = find("light.contrast").unwrap();
        let temp = find("wb.temp").unwrap();
        assert_eq!(typed_value(exposure, "1.5"), Some(1.5));
        assert_eq!(typed_value(exposure, " +0,25 "), Some(0.25));
        assert_eq!(typed_value(exposure, "-12"), Some(exposure.min), "clamped");
        assert_eq!(typed_value(contrast, "33.4"), Some(33.0), "on the control's steps");
        assert_eq!(typed_value(contrast, "-20 %"), Some(-20.0));
        assert_eq!(typed_value(temp, "5600 K"), Some(5600.0));
        for bad in ["", "abc", "1.2.3", "NaN", "inf", "--1"] {
            assert_eq!(typed_value(contrast, bad), None, "{bad:?}");
        }
    }
}

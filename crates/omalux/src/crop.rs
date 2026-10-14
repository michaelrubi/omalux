//! The Crop & Straighten tool (R), as Lightroom has it: the frame is
//! shown whole with the crop drawn over it, to drag by its corners and
//! edges, move from inside, or turn from outside; with Ctrl held, a line
//! drawn along the horizon straightens by it, and a double-click inside
//! puts the tool away. Its settings are in the panel
//! ([`crate::crop_panel`]). What each gesture does to the crop is the
//! engine's (`omalux_engine::crop`).
//!
//! The frame is drawn turned by the angle it's straightened by, so the
//! crop stays upright and the picture turns under it, following the
//! pointer, as in Lightroom. Lightcraft's crop tool, whose gestures these
//! are (`crop_overlay` in `ui-egui/src/panels/detail.rs` at `701f78e`),
//! leaves the picture as it is and turns the crop over it instead.

use std::path::{Path, PathBuf};

use egui::{Align2, Color32, CursorIcon, FontId, Pos2, Rect, Sense, Stroke, TextureHandle, Ui, pos2, vec2};
use lightcraft_develop::{Crop, DevelopSettings};
use omalux_engine::crop::{CropGeometry, CropHandle, Orientation, Point};

use crate::theme::Theme;

/// How near a handle the pointer has to be to take it, in points.
const REACH: f32 = 12.0;
/// The handles, as `CropHandle::from_index` numbers them: the corners
/// clockwise from the top left, then the edges from the top.
const HANDLES: usize = 8;

/// What the pointer has hold of.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Gesture {
    /// A handle, or the whole crop: the crop as it was when the drag
    /// began, and where it began, in the straightened frame.
    Handle { handle: CropHandle, start: CropGeometry, from: Point },
    /// The frame, from outside the crop, to turn it: the angle it had,
    /// and the pointer's bearing from the middle of the frame.
    Turn { angle: f64, bearing: f32 },
    /// A line to straighten along, from here.
    Line { from: Pos2 },
}

/// What a gesture asks of the crop this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Act {
    /// `omalux_engine::crop::drag`'s arguments.
    Drag { start: CropGeometry, handle: CropHandle, from: Point, to: Point, keep_ratio: bool },
    /// Straighten by this angle.
    Turn(f64),
    /// Straighten along this line, in the straightened frame.
    Level { from: Point, to: Point },
}

/// What happened on the frame this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Out {
    pub act: Option<Act>,
    /// A drag ended: what it changed is one step.
    pub stopped: bool,
    /// A double-click inside the crop: put the tool away.
    pub done: bool,
}

/// The frame the crop is drawn over.
pub struct Frame<'a> {
    /// The room it has.
    pub area: Rect,
    /// Its render, whole and uncropped, once there is one.
    pub whole: Option<&'a TextureHandle>,
    /// Its size as it's shown.
    pub dims: (f64, f64),
}

/// The tool, while it's open.
#[derive(Default)]
pub struct Tool {
    /// The frame it's on, and that frame's crop and turn when the tool
    /// came to it: what Esc goes back to.
    was: Option<(PathBuf, Crop, Orientation)>,
    gesture: Option<Gesture>,
}

/// The largest rectangle of `size`'s shape centred in `area`.
fn fit(size: egui::Vec2, area: Rect) -> Rect {
    let scale = (area.width() / size.x).min(area.height() / size.y);
    Rect::from_center_size(area.center(), size * scale)
}

/// Where each handle of a crop drawn at `on` is.
fn handles(on: Rect) -> [Pos2; HANDLES] {
    let (c, m) = (on.center(), on);
    [m.left_top(), m.right_top(), m.right_bottom(), m.left_bottom(), pos2(c.x, m.top()), pos2(m.right(), c.y), pos2(c.x, m.bottom()), pos2(m.left(), c.y)]
}

/// What a press at `at` takes hold of, on a crop drawn at `on`: a handle
/// within reach, else the whole crop from inside it, else nothing (the
/// frame, to turn).
fn grabbed(on: Rect, at: Pos2) -> Option<CropHandle> {
    let nearest = handles(on).into_iter().enumerate().min_by(|a, b| a.1.distance(at).total_cmp(&b.1.distance(at)));
    match nearest {
        Some((index, handle)) if handle.distance(at) < REACH => CropHandle::from_index(index as u8),
        _ => on.contains(at).then_some(CropHandle::Move),
    }
}

impl Tool {
    /// The tool is on `path`, whose settings are `settings`: remembered
    /// the first time, for [`Self::was`].
    pub fn on(&mut self, path: &Path, settings: &DevelopSettings) {
        if self.was.as_ref().is_none_or(|(on, ..)| on != path) {
            self.was = Some((path.to_path_buf(), settings.crop, settings.orientation));
            self.gesture = None;
        }
    }

    /// The crop and turn `path` had when the tool came to it.
    pub fn was(&self, path: &Path) -> Option<(Crop, Orientation)> {
        self.was.as_ref().filter(|(on, ..)| on == path).map(|(_, crop, orientation)| (*crop, *orientation))
    }

    /// Draw `frame` with `settings`' crop over it, and take the mouse.
    /// `locked` is whether the crop keeps its shape when a handle is
    /// dragged (Shift holds it for one drag).
    pub fn show(&mut self, ui: &mut Ui, theme: &Theme, frame: Frame, settings: &DevelopSettings, locked: bool) -> Out {
        let Frame { area, whole, dims } = frame;
        let mut out = Out::default();
        let geometry = settings.crop.geometry;
        // Where the frame would be unturned: the straightened frame the
        // crop lives in is laid over it, and the picture turned in it.
        let frame = fit(vec2(dims.0 as f32, dims.1 as f32), area);
        if !frame.is_positive() {
            return out;
        }
        let painter = ui.painter_at(area);
        if let Some(texture) = whole {
            let mut picture = egui::Mesh::with_texture(texture.id());
            picture.add_rect_with_uv(frame, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
            picture.rotate(egui::emath::Rot2::from_angle(geometry.angle.to_radians() as f32), frame.center());
            painter.add(picture);
        }
        let on_screen = |point: Point| frame.min + frame.size() * vec2(point.x as f32, point.y as f32);
        let in_frame = |at: Pos2| Point::new(f64::from((at.x - frame.left()) / frame.width()), f64::from((at.y - frame.top()) / frame.height()));
        let rect = geometry.rect;
        let on = Rect::from_min_max(on_screen(Point::new(rect.x0, rect.y0)), on_screen(Point::new(rect.x1, rect.y1)));
        #[cfg(test)]
        ui.data_mut(|d| d.insert_temp(egui::Id::new("crop box"), on));

        let response = ui.interact(area, ui.id().with("crop tool"), Sense::click_and_drag());
        let (ctrl, shift, down) = ui.input(|i| (i.modifiers.command, i.modifiers.shift, i.pointer.primary_down()));
        // A drag cut short (the window lost the mouse) is over.
        if self.gesture.is_some() && !down && !response.drag_stopped() {
            self.gesture = None;
            out.stopped = true;
        }
        if response.double_clicked() && response.interact_pointer_pos().is_some_and(|at| grabbed(on, at) == Some(CropHandle::Move)) {
            out.done = true;
            return out;
        }
        if response.drag_started()
            && let Some(at) = ui.input(|i| i.pointer.press_origin()).or(response.interact_pointer_pos())
        {
            self.gesture = Some(match grabbed(on, at) {
                _ if ctrl => Gesture::Line { from: at },
                Some(handle) => Gesture::Handle { handle, start: geometry, from: in_frame(at) },
                None => Gesture::Turn { angle: geometry.angle, bearing: (at - frame.center()).angle() },
            });
        }
        let pointer = response.interact_pointer_pos().or(response.hover_pos());
        if let (Some(gesture), Some(at)) = (self.gesture, pointer) {
            match gesture {
                Gesture::Handle { handle, start, from } if response.dragged() => {
                    out.act = Some(Act::Drag { start, handle, from, to: in_frame(at), keep_ratio: locked || shift });
                }
                Gesture::Turn { angle, bearing } if response.dragged() => {
                    // The picture follows the pointer round, the short way.
                    let turned = egui::emath::normalized_angle((at - frame.center()).angle() - bearing);
                    out.act = Some(Act::Turn(angle + f64::from(turned.to_degrees())));
                }
                Gesture::Line { from } if response.drag_stopped() => out.act = Some(Act::Level { from: in_frame(from), to: in_frame(at) }),
                _ => {}
            }
        }
        if response.drag_stopped() {
            self.gesture = None;
            out.stopped = true;
        }

        // Dim what the crop leaves out, the pasteboard with it.
        let dim = Color32::from_black_alpha(150);
        for outside in [
            Rect::from_min_max(area.min, pos2(area.right(), on.top())),
            Rect::from_min_max(pos2(area.left(), on.bottom()), area.max),
            Rect::from_min_max(pos2(area.left(), on.top()), on.left_bottom()),
            Rect::from_min_max(on.right_top(), pos2(area.right(), on.bottom())),
        ] {
            painter.rect_filled(outside, 0.0, dim);
        }
        // Thirds; a finer grid to line things up by while the frame is turned.
        let turning = matches!(self.gesture, Some(Gesture::Turn { .. }));
        let lines = if turning { 12 } else { 3 };
        let guide = Stroke::new(1.0, Color32::from_white_alpha(if turning { 60 } else { 110 }));
        for i in 1..lines {
            let t = i as f32 / lines as f32;
            painter.vline(on.left() + on.width() * t, on.y_range(), guide);
            painter.hline(on.x_range(), on.top() + on.height() * t, guide);
        }
        painter.rect_stroke(on, 0.0, Stroke::new(1.0, Color32::WHITE), egui::StrokeKind::Outside);
        for (index, handle) in handles(on).into_iter().enumerate() {
            let length = if index < 4 { 14.0 } else { 10.0 };
            painter.rect_filled(Rect::from_center_size(handle, vec2(length, 3.0)), 0.0, Color32::WHITE);
            painter.rect_filled(Rect::from_center_size(handle, vec2(3.0, length)), 0.0, Color32::WHITE);
        }
        match (self.gesture, pointer) {
            (Some(Gesture::Line { from }), Some(to)) => {
                painter.line_segment([from, to], Stroke::new(3.0, Color32::from_black_alpha(140)));
                painter.line_segment([from, to], Stroke::new(1.5, Color32::WHITE));
            }
            (Some(Gesture::Turn { .. }), Some(at)) => {
                // The angle beside the pointer, as it'll be once this frame's turn is taken.
                let angle = match out.act {
                    Some(Act::Turn(angle)) => (angle.clamp(-45.0, 45.0) * 100.0).round() / 100.0,
                    _ => geometry.angle,
                };
                let text = format!("{angle:+.2}°");
                let font = FontId::proportional(12.5);
                let galley = painter.layout_no_wrap(text, font, theme.foreground);
                let label = Align2::LEFT_BOTTOM.anchor_size(at + vec2(14.0, -10.0), galley.size()).expand2(vec2(5.0, 2.0));
                painter.rect_filled(label, 2.0, theme.darker_background);
                painter.galley(label.min + vec2(5.0, 2.0), galley, theme.foreground);
            }
            _ => {}
        }

        if let Some(at) = response.hover_pos().filter(|_| response.hovered() || self.gesture.is_some()) {
            let held = match self.gesture {
                Some(Gesture::Handle { handle, .. }) => Some(Some(handle)),
                Some(Gesture::Turn { .. }) => Some(None),
                Some(Gesture::Line { .. }) => None,
                None if ctrl => None,
                None => Some(grabbed(on, at)),
            };
            ui.ctx().set_cursor_icon(match held {
                None => CursorIcon::Crosshair,
                Some(Some(CropHandle::TopLeft | CropHandle::BottomRight)) => CursorIcon::ResizeNwSe,
                Some(Some(CropHandle::TopRight | CropHandle::BottomLeft)) => CursorIcon::ResizeNeSw,
                Some(Some(CropHandle::Top | CropHandle::Bottom)) => CursorIcon::ResizeVertical,
                Some(Some(CropHandle::Left | CropHandle::Right)) => CursorIcon::ResizeHorizontal,
                Some(Some(CropHandle::Move)) => CursorIcon::Move,
                Some(None) if down => CursorIcon::Grabbing,
                Some(None) => CursorIcon::Grab,
            });
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_press_takes_the_nearest_handle_then_the_crop_then_the_frame() {
        let on = Rect::from_min_max(pos2(100.0, 100.0), pos2(400.0, 300.0));
        assert_eq!(grabbed(on, pos2(103.0, 96.0)), Some(CropHandle::TopLeft), "from just outside it too");
        assert_eq!(grabbed(on, pos2(398.0, 295.0)), Some(CropHandle::BottomRight));
        assert_eq!(grabbed(on, pos2(250.0, 104.0)), Some(CropHandle::Top));
        assert_eq!(grabbed(on, pos2(395.0, 200.0)), Some(CropHandle::Right));
        assert_eq!(grabbed(on, pos2(250.0, 200.0)), Some(CropHandle::Move));
        assert_eq!(grabbed(on, pos2(250.0, 50.0)), None, "outside: the frame is turned");
        // A crop too small for its handles to be told apart still gives the nearest.
        let small = Rect::from_min_max(pos2(100.0, 100.0), pos2(106.0, 106.0));
        assert_eq!(grabbed(small, pos2(107.0, 107.0)), Some(CropHandle::BottomRight));
    }
}

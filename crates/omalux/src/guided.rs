//! Guided Upright's tool (Shift+T), as Lightroom has it: lines drawn on
//! the frame along what should be upright or level, up to four, which
//! Upright then makes so. A drag draws a line; a click takes hold of one,
//! and Delete takes the one held, or else the last, away. Return puts the
//! tool away, Esc puts it away with the lines as they were.
//!
//! The lines are where they are in the frame through its lens corrections
//! and before any perspective (Lightcraft's `Geometry::guides`), so the
//! tool shows the frame that way while it's open, and the transform they
//! make once it's put away. Lightroom shows the transform as the lines go
//! in; this shows the picture they were drawn on, which holds still.

use std::path::{Path, PathBuf};

use egui::{Color32, CursorIcon, Pos2, Rect, Sense, Stroke, TextureHandle, Ui, pos2, vec2};
use lightcraft_develop::{DevelopSettings, Geometry};
use omalux_engine::crop::Point;

use crate::theme::Theme;

/// The most lines Upright takes, as in Lightroom.
pub const MAX_GUIDES: usize = 4;
/// How near a line the pointer has to be to take it, in points.
const REACH: f32 = 8.0;
/// A drag shorter than this, in points, is a click, not a line.
const SHORTEST: f32 = 12.0;

/// What happened on the frame this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Out {
    /// A line was drawn, from and to, as shares of the frame.
    pub added: Option<(Point, Point)>,
}

/// The tool, while it's open.
#[derive(Default)]
pub struct Tool {
    /// The frame it's on, and that frame's geometry when the tool came to
    /// it: what Esc goes back to.
    was: Option<(PathBuf, Geometry)>,
    /// A line being drawn, from here.
    drawing: Option<Pos2>,
    /// The line held, by its place in the frame's guides.
    pub held: Option<usize>,
}

/// The largest rectangle of `size`'s shape centred in `area`.
fn fit(size: egui::Vec2, area: Rect) -> Rect {
    let scale = (area.width() / size.x).min(area.height() / size.y);
    Rect::from_center_size(area.center(), size * scale)
}

/// How far `at` is from the segment `a`–`b`.
fn distance(at: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let t = if ab.length_sq() > 0.0 { ((at - a).dot(ab) / ab.length_sq()).clamp(0.0, 1.0) } else { 0.0 };
    at.distance(a + ab * t)
}

impl Tool {
    /// The tool is on `path`, whose settings are `settings`: remembered
    /// the first time, for [`Self::was`].
    pub fn on(&mut self, path: &Path, settings: &DevelopSettings) {
        if self.was.as_ref().is_none_or(|(on, _)| on != path) {
            self.was = Some((path.to_path_buf(), settings.geometry.clone()));
            (self.drawing, self.held) = (None, None);
        }
    }

    /// The geometry `path` had when the tool came to it.
    pub fn was(&self, path: &Path) -> Option<Geometry> {
        self.was.as_ref().filter(|(on, _)| on == path).map(|(_, geometry)| geometry.clone())
    }

    /// Draw the frame, `dims` in size as it's shown, in `area`, with
    /// `guides` over it, and take the mouse. `whole` is its render through
    /// its lens corrections alone, once there is one.
    pub fn show(&mut self, ui: &mut Ui, theme: &Theme, area: Rect, whole: Option<&TextureHandle>, dims: (f64, f64), guides: &[(Point, Point)]) -> Out {
        let mut out = Out::default();
        let frame = fit(vec2(dims.0 as f32, dims.1 as f32), area);
        if !frame.is_positive() {
            return out;
        }
        let painter = ui.painter_at(area);
        if let Some(texture) = whole {
            painter.image(texture.id(), frame, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        }
        let on_screen = |p: Point| frame.min + frame.size() * vec2(p.x as f32, p.y as f32);
        let in_frame = |at: Pos2| {
            let at = frame.clamp(at);
            Point::new(f64::from((at.x - frame.left()) / frame.width()), f64::from((at.y - frame.top()) / frame.height()))
        };
        #[cfg(test)]
        ui.data_mut(|d| d.insert_temp(egui::Id::new("guided frame"), frame));

        let response = ui.interact(area, ui.id().with("guided upright"), Sense::click_and_drag());
        let near = |at: Pos2| {
            let nearest = guides.iter().enumerate().map(|(i, (a, b))| (i, distance(at, on_screen(*a), on_screen(*b)))).min_by(|a, b| a.1.total_cmp(&b.1));
            nearest.filter(|(_, d)| *d < REACH).map(|(i, _)| i)
        };
        if response.clicked() {
            self.held = response.interact_pointer_pos().and_then(near);
        }
        if response.drag_started() {
            self.drawing = ui.input(|i| i.pointer.press_origin()).or(response.interact_pointer_pos()).filter(|at| frame.contains(*at));
        }
        let pointer = response.interact_pointer_pos().or(response.hover_pos());
        if response.drag_stopped()
            && let (Some(from), Some(to)) = (self.drawing.take(), pointer)
            && from.distance(to) >= SHORTEST
            && guides.len() < MAX_GUIDES
        {
            out.added = Some((in_frame(from), in_frame(to)));
            self.held = None;
        }
        if !ui.input(|i| i.pointer.primary_down()) && !response.drag_stopped() {
            self.drawing = None;
        }

        let line = |painter: &egui::Painter, a: Pos2, b: Pos2, colour: Color32| {
            painter.line_segment([a, b], Stroke::new(3.0, Color32::from_black_alpha(140)));
            painter.line_segment([a, b], Stroke::new(1.5, colour));
            for end in [a, b] {
                painter.circle_filled(end, 3.5, colour);
            }
        };
        for (i, (a, b)) in guides.iter().enumerate() {
            let colour = if self.held == Some(i) { theme.accent } else { Color32::WHITE };
            line(&painter, on_screen(*a), on_screen(*b), colour);
        }
        if let (Some(from), Some(to)) = (self.drawing, pointer) {
            let colour = if guides.len() < MAX_GUIDES { Color32::WHITE } else { theme.red };
            line(&painter, from, frame.clamp(to), colour);
        }
        if response.hovered() {
            ui.ctx().set_cursor_icon(if response.hover_pos().and_then(near).is_some() { CursorIcon::PointingHand } else { CursorIcon::Crosshair });
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_distance_to_a_line_is_to_its_nearest_point() {
        let (a, b) = (pos2(0.0, 0.0), pos2(10.0, 0.0));
        assert_eq!(distance(pos2(5.0, 3.0), a, b), 3.0);
        assert_eq!(distance(pos2(14.0, 3.0), a, b), 5.0, "past its end, to the end");
        assert_eq!(distance(pos2(3.0, 4.0), a, a), 5.0, "a line of no length is a point");
    }
}

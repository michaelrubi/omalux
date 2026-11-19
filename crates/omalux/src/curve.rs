//! Tone Curve's graph, as Lightroom's: the parametric curve, its regions
//! dragged up and down in the graph and its three splits under it, or a
//! point curve for RGB, Red, Green or Blue. A click adds a point, a drag
//! moves one (or adds one and moves it), a double-click takes one away;
//! the right-click menu resets the channel or every curve. The frame's
//! histogram is drawn behind it.
//!
//! Lifted from Lightcraft's `curve_editor` (`ui-egui/src/panels/edit.rs`
//! at `0da998f2`, MIT OR Apache-2.0), restyled to Omarchy's theme, with
//! its edits handed back rather than run, and the parametric curve drawn
//! as the pipeline builds it (`lightcraft_pipeline::finish::curve_luts`).

use egui::{Align2, Color32, CursorIcon, FontId, Mesh, Pos2, Rect, Sense, Stroke, Ui, pos2, vec2};
use egui::epaint::Vertex;
use lightcraft_color::spline::MonotoneCurve;
use lightcraft_develop::ToneCurve;
use lightcraft_raster::Histogram;
use omalux_engine::crop::Point;

use crate::theme::Theme;

/// Which curve the graph shows: Lightroom's channel buttons.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Channel {
    #[default]
    Parametric,
    Rgb,
    Red,
    Green,
    Blue,
}

impl Channel {
    pub const ALL: [Channel; 5] = [Channel::Parametric, Channel::Rgb, Channel::Red, Channel::Green, Channel::Blue];

    pub fn label(self) -> &'static str {
        match self {
            Channel::Parametric => "Parametric Curve",
            Channel::Rgb => "Point Curve: RGB",
            Channel::Red => "Point Curve: Red",
            Channel::Green => "Point Curve: Green",
            Channel::Blue => "Point Curve: Blue",
        }
    }

    fn colour(self) -> Color32 {
        match self {
            Channel::Parametric | Channel::Rgb => Color32::from_gray(225),
            Channel::Red => Color32::from_rgb(0xdd, 0x33, 0x33),
            Channel::Green => Color32::from_rgb(0x33, 0xbb, 0x55),
            Channel::Blue => Color32::from_rgb(0x33, 0x77, 0xee),
        }
    }
}

/// The parametric curve's four regions, dark to light, and the three
/// splits between them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Region {
    Shadows,
    Darks,
    Lights,
    Highlights,
}

/// A change to the tone curve.
#[derive(Clone, Debug, PartialEq)]
pub enum Change {
    /// A point curve's points, in order.
    Points(Channel, Vec<Point>),
    /// A region of the parametric curve, to this amount (-100 to 100).
    Region(Region, f64),
    /// One of the three splits (0 to 2, dark to light), to this percent.
    Split(usize, f64),
    /// A channel straight again; the parametric curve's splits go back
    /// too.
    Reset(Channel),
    /// Every curve straight again.
    ResetAll,
}

impl Change {
    pub fn apply(&self, curve: &mut ToneCurve) {
        match self {
            Change::Points(channel, points) => {
                // The straight line between the ends alone is no curve: kept
                // empty, as a fresh raw's is. A point on it is an anchor, kept.
                let points = if points.len() <= 2 && ToneCurve::point_curve_is_identity(points) { Vec::new() } else { points.clone() };
                match channel {
                    Channel::Parametric => {}
                    Channel::Rgb => curve.master = points,
                    Channel::Red => curve.red = points,
                    Channel::Green => curve.green = points,
                    Channel::Blue => curve.blue = points,
                }
            }
            Change::Region(region, amount) => {
                let amount = amount.clamp(-100.0, 100.0).round();
                match region {
                    Region::Shadows => curve.shadows = amount,
                    Region::Darks => curve.darks = amount,
                    Region::Lights => curve.lights = amount,
                    Region::Highlights => curve.highlights = amount,
                }
            }
            Change::Split(i, at) => set_split(curve, *i, *at),
            Change::Reset(Channel::Parametric) => {
                let straight = ToneCurve::default();
                (curve.shadows, curve.darks, curve.lights, curve.highlights) = (0.0, 0.0, 0.0, 0.0);
                (curve.split_shadows, curve.split_mid, curve.split_highlights) = (straight.split_shadows, straight.split_mid, straight.split_highlights);
            }
            Change::Reset(channel) => Change::Points(*channel, Vec::new()).apply(curve),
            Change::ResetAll => {
                let refine = curve.refine_saturation;
                *curve = ToneCurve { refine_saturation: refine, ..ToneCurve::default() };
            }
        }
    }
}

/// The splits' ranges, as Lightcraft's controls have them, and the room
/// kept between two.
const SPLIT_RANGES: [(f64, f64); 3] = [(10.0, 70.0), (20.0, 80.0), (30.0, 90.0)];
const SPLIT_GAP: f64 = 5.0;

fn splits(curve: &ToneCurve) -> [f64; 3] {
    [curve.split_shadows, curve.split_mid, curve.split_highlights]
}

/// Move split `i` to `at` percent, within its range and kept apart from
/// its neighbours, as Lightroom's can't pass each other.
fn set_split(curve: &mut ToneCurve, i: usize, at: f64) {
    let Some(&(lo, hi)) = SPLIT_RANGES.get(i) else { return };
    let now = splits(curve);
    let lo = if i > 0 { lo.max(now[i - 1] + SPLIT_GAP) } else { lo };
    let hi = if i < 2 { hi.min(now[i + 1] - SPLIT_GAP) } else { hi };
    let at = if lo <= hi { at.clamp(lo, hi).round() } else { now[i] };
    match i {
        0 => curve.split_shadows = at,
        1 => curve.split_mid = at,
        _ => curve.split_highlights = at,
    }
}

/// The parametric curve at `x` (0 to 1), as the pipeline builds it: each
/// region's amount over a raised-cosine window, kept monotone.
pub fn parametric(curve: &ToneCurve, samples: usize) -> Vec<f64> {
    let [s1, s2, s3] = splits(curve).map(|s| s / 100.0);
    let regions = [(0.0, s1, curve.shadows), (s1, s2, curve.darks), (s2, s3, curve.lights), (s3, 1.0, curve.highlights)];
    let mut ys = Vec::with_capacity(samples + 1);
    let mut floor = 0.0_f64;
    for i in 0..=samples {
        let x = i as f64 / samples.max(1) as f64;
        let mut d = 0.0;
        for (a, b, amount) in regions {
            let (centre, half) = ((a + b) / 2.0, (b - a) * 0.75 + 0.05);
            let t = ((x - centre) / half).clamp(-1.0, 1.0);
            d += amount / 100.0 * 0.22 * (0.5 + 0.5 * (t * std::f64::consts::PI).cos());
        }
        floor = floor.max((x + d * 4.0 * x * (1.0 - x)).clamp(0.0, 1.0));
        ys.push(floor);
    }
    ys
}

/// The region of the parametric curve at `x` (0 to 1).
fn region_at(curve: &ToneCurve, x: f64) -> Region {
    let [s1, s2, s3] = splits(curve).map(|s| s / 100.0);
    if x < s1 {
        Region::Shadows
    } else if x < s2 {
        Region::Darks
    } else if x < s3 {
        Region::Lights
    } else {
        Region::Highlights
    }
}

fn region_amount(curve: &ToneCurve, region: Region) -> f64 {
    match region {
        Region::Shadows => curve.shadows,
        Region::Darks => curve.darks,
        Region::Lights => curve.lights,
        Region::Highlights => curve.highlights,
    }
}

/// A channel's points, in order and finite, the straight line for none.
pub fn points(curve: &ToneCurve, channel: Channel) -> Vec<Point> {
    let points = match channel {
        Channel::Parametric | Channel::Rgb => &curve.master,
        Channel::Red => &curve.red,
        Channel::Green => &curve.green,
        Channel::Blue => &curve.blue,
    };
    let mut points: Vec<Point> = points.iter().copied().filter(|p| p.x.is_finite() && p.y.is_finite()).collect();
    points.sort_by(|a, b| a.x.total_cmp(&b.x));
    if points.len() < 2 { vec![Point::new(0.0, 0.0), Point::new(1.0, 1.0)] } else { points }
}

/// The closest two points' `x` can be.
const POINT_GAP: f64 = 0.01;

/// `points` with one more at (`x`, `y`), and where it went.
pub fn inserted(points: &[Point], x: f64, y: f64) -> (Vec<Point>, usize) {
    let mut points = points.to_vec();
    let i = points.iter().position(|p| p.x > x).unwrap_or(points.len());
    points.insert(i, Point::new(x.clamp(0.0, 1.0), y.clamp(0.0, 1.0)));
    (points, i)
}

/// `points` with point `i` at (`x`, `y`), kept between its neighbours.
pub fn moved(points: &[Point], i: usize, x: f64, y: f64) -> Vec<Point> {
    let mut points = points.to_vec();
    let n = points.len();
    if i >= n {
        return points;
    }
    let lo = if i == 0 { 0.0 } else { points[i - 1].x + POINT_GAP };
    let hi = if i + 1 == n { 1.0 } else { points[i + 1].x - POINT_GAP };
    // Neighbours too close to leave room (points from a file) keep it midway.
    let x = if lo <= hi { x.clamp(lo, hi) } else { (lo + hi) / 2.0 };
    points[i] = Point::new(x.clamp(0.0, 1.0), y.clamp(0.0, 1.0));
    points
}

/// `points` without point `i`; the two ends stay.
pub fn removed(points: &[Point], i: usize) -> Option<Vec<Point>> {
    (i > 0 && i + 1 < points.len()).then(|| {
        let mut points = points.to_vec();
        points.remove(i);
        points
    })
}

/// What was done in the graph this frame. A drag's changes are one step
/// to undo; a click's is one on its own.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Out {
    pub change: Option<Change>,
    pub drag_started: bool,
    pub drag_stopped: bool,
}

/// What a drag in the graph holds on to.
#[derive(Clone, Copy, Debug)]
enum Held {
    /// A point curve's point, by its place.
    Point(usize),
    /// A parametric region, and its amount when the drag began.
    Region(Region, f64),
    /// A split, by its place.
    Split(usize),
}

const NEAR: f32 = 10.0;
const SPLIT_ROW: f32 = 14.0;

/// The channel buttons, the graph and (parametric) its splits.
pub fn show(ui: &mut Ui, theme: &Theme, curve: &ToneCurve, channel: &mut Channel, histogram: Option<&Histogram>) -> Out {
    let mut out = Out::default();
    channel_buttons(ui, theme, channel, &mut out);
    let channel = *channel;
    let width = ui.available_width();
    let side = (width - 16.0).clamp(80.0, 260.0);
    let height = side + if channel == Channel::Parametric { SPLIT_ROW } else { 0.0 } + 8.0;
    let (outer, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    let graph = Rect::from_min_size(pos2(outer.center().x - side / 2.0, outer.top() + 4.0), vec2(side, side));
    let id = ui.id().with(("curve", channel as u8));
    let response = ui.interact(graph, id, Sense::click_and_drag());
    // Where it is, for the headless tests.
    ui.data_mut(|d| d.insert_temp(egui::Id::new("tone curve"), graph));
    response.context_menu(|ui| {
        let name = if channel == Channel::Parametric { "Reset the Parametric Curve" } else { "Reset the Channel" };
        if ui.button(name).clicked() {
            out.change = Some(Change::Reset(channel));
        }
        if ui.button("Reset All Curves").clicked() {
            out.change = Some(Change::ResetAll);
        }
    });

    let painter = ui.painter();
    painter.rect_filled(graph, 2.0, theme.darker_background);
    if let Some(histogram) = histogram {
        draw_histogram(painter, graph, histogram, channel);
    }
    let grid = Stroke::new(1.0, theme.dark_background);
    for i in 1..4 {
        let f = i as f32 / 4.0;
        painter.line_segment([pos2(graph.left() + graph.width() * f, graph.top()), pos2(graph.left() + graph.width() * f, graph.bottom())], grid);
        painter.line_segment([pos2(graph.left(), graph.top() + graph.height() * f), pos2(graph.right(), graph.top() + graph.height() * f)], grid);
    }
    painter.line_segment([graph.left_bottom(), graph.right_top()], Stroke::new(1.0, theme.muted.gamma_multiply(0.5)));
    let to_screen = |x: f64, y: f64| pos2(graph.left() + x as f32 * graph.width(), graph.bottom() - y as f32 * graph.height());
    let from_screen = |p: Pos2| {
        (f64::from(((p.x - graph.left()) / graph.width()).clamp(0.0, 1.0)), f64::from(((graph.bottom() - p.y) / graph.height()).clamp(0.0, 1.0)))
    };
    let held_id = id.with("held");
    let mut held: Option<Held> = ui.data(|d| d.get_temp::<Option<Held>>(held_id)).flatten();

    if channel == Channel::Parametric {
        let ys = parametric(curve, 128);
        let line: Vec<Pos2> = ys.iter().enumerate().map(|(i, y)| to_screen(i as f64 / 128.0, *y)).collect();
        // The region under the pointer, or the one being dragged, lit.
        let lit = match held {
            Some(Held::Region(region, _)) => Some(region),
            _ => response.hover_pos().map(|p| region_at(curve, from_screen(p).0)),
        };
        if let Some(region) = lit {
            let [s1, s2, s3] = splits(curve).map(|s| s / 100.0);
            let (a, b) = match region {
                Region::Shadows => (0.0, s1),
                Region::Darks => (s1, s2),
                Region::Lights => (s2, s3),
                Region::Highlights => (s3, 1.0),
            };
            let band = Rect::from_x_y_ranges(to_screen(a, 0.0).x..=to_screen(b, 0.0).x, graph.y_range());
            painter.rect_filled(band, 0.0, theme.foreground.gamma_multiply(0.05));
            let name = match region {
                Region::Shadows => "Shadows",
                Region::Darks => "Darks",
                Region::Lights => "Lights",
                Region::Highlights => "Highlights",
            };
            let text = format!("{name} {:+.0}", region_amount(curve, region));
            painter.text(graph.left_top() + vec2(6.0, 4.0), Align2::LEFT_TOP, text, FontId::proportional(11.0), theme.muted);
        }
        painter.add(egui::Shape::line(line, Stroke::new(2.0, channel.colour())));
        // The splits, as triangles under the graph.
        let row = Rect::from_min_size(graph.left_bottom() + vec2(0.0, 2.0), vec2(graph.width(), SPLIT_ROW));
        let split_response = ui.interact(row, id.with("splits"), Sense::drag());
        for (i, at) in splits(curve).into_iter().enumerate() {
            let x = to_screen(at / 100.0, 0.0).x;
            let lit = matches!(held, Some(Held::Split(j)) if j == i);
            let colour = if lit { theme.accent } else { theme.muted };
            let tip = [pos2(x, row.top() + 2.0), pos2(x - 5.0, row.bottom() - 2.0), pos2(x + 5.0, row.bottom() - 2.0)];
            painter.add(egui::Shape::convex_polygon(tip.to_vec(), colour, Stroke::NONE));
        }
        if split_response.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
        }
        if split_response.drag_started()
            && let Some(p) = ui.input(|i| i.pointer.press_origin())
        {
            let nearest = splits(curve).into_iter().enumerate().min_by(|a, b| {
                let d = |at: f64| (to_screen(at / 100.0, 0.0).x - p.x).abs();
                d(a.1).total_cmp(&d(b.1))
            });
            if let Some((i, _)) = nearest {
                held = Some(Held::Split(i));
                out.drag_started = true;
            }
        }
        if let (Some(Held::Split(i)), Some(p)) = (held, split_response.interact_pointer_pos())
            && split_response.dragged()
        {
            out.change = Some(Change::Split(i, from_screen(p).0 * 100.0));
        }
        if split_response.drag_stopped() {
            held = None;
            out.drag_stopped = true;
        }
        // A region dragged up or down: the graph's height is its range.
        if response.drag_started()
            && let Some(p) = ui.input(|i| i.pointer.press_origin())
        {
            let region = region_at(curve, from_screen(p).0);
            held = Some(Held::Region(region, region_amount(curve, region)));
            out.drag_started = true;
        }
        if let Some(Held::Region(region, start)) = held
            && response.dragged()
            && let (Some(origin), Some(p)) = (ui.input(|i| i.pointer.press_origin()), response.interact_pointer_pos())
        {
            let moved = f64::from((origin.y - p.y) / graph.height()) * 200.0;
            out.change = Some(Change::Region(region, start + moved));
        }
        if response.drag_stopped() {
            held = None;
            out.drag_stopped = true;
        }
        if response.hovered() || matches!(held, Some(Held::Region(..))) {
            ui.ctx().set_cursor_icon(CursorIcon::ResizeVertical);
        }
        ui.data_mut(|d| d.insert_temp(held_id, held));
        return out;
    }

    let pts = points(curve, channel);
    let spline = MonotoneCurve::new(&pts.iter().map(|p| (p.x, p.y)).collect::<Vec<_>>());
    let colour = channel.colour();
    let line: Vec<Pos2> = (0..=128).map(|i| f64::from(i) / 128.0).map(|x| to_screen(x, spline.eval(x).clamp(0.0, 1.0))).collect();
    painter.add(egui::Shape::line(line, Stroke::new(2.0, colour)));
    let nearest = |q: Pos2| pts.iter().enumerate().map(|(i, p)| (i, to_screen(p.x, p.y).distance(q))).min_by(|a, b| a.1.total_cmp(&b.1)).filter(|(_, d)| *d < NEAR).map(|(i, _)| i);
    let dragging = match held {
        Some(Held::Point(i)) => Some(i),
        _ => None,
    };
    let hovered = response.hover_pos().and_then(nearest);
    if dragging.is_some() {
        ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
    } else if hovered.is_some() {
        ui.ctx().set_cursor_icon(CursorIcon::Grab);
    }
    for (i, p) in pts.iter().enumerate() {
        let big = dragging == Some(i) || (dragging.is_none() && hovered == Some(i));
        painter.circle_filled(to_screen(p.x, p.y), if big { 5.5 } else { 4.0 }, colour);
    }
    // Input and output of the point being dragged, 0 to 255 as Lightroom's.
    if let Some(p) = dragging.and_then(|i| pts.get(i)) {
        let text = format!("{:.0} / {:.0}", p.x * 255.0, p.y * 255.0);
        painter.text(graph.left_top() + vec2(6.0, 4.0), Align2::LEFT_TOP, text, FontId::proportional(11.0), theme.muted);
    }
    if response.double_clicked() {
        if let Some(fewer) = response.interact_pointer_pos().and_then(nearest).and_then(|i| removed(&pts, i)) {
            out.change = Some(Change::Points(channel, fewer));
        }
    } else if response.drag_started()
        && let Some(q) = ui.input(|i| i.pointer.press_origin())
    {
        // The point under the press, or a new one there, moved to where
        // the pointer is now.
        out.drag_started = true;
        let (pts, i) = match nearest(q) {
            Some(i) => (pts.clone(), i),
            None => {
                let (x, y) = from_screen(q);
                inserted(&pts, x, y)
            }
        };
        held = Some(Held::Point(i));
        let (x, y) = from_screen(response.interact_pointer_pos().unwrap_or(q));
        out.change = Some(Change::Points(channel, moved(&pts, i, x, y)));
    } else if let (Some(i), Some(q)) = (dragging, response.interact_pointer_pos())
        && response.dragged()
    {
        let (x, y) = from_screen(q);
        out.change = Some(Change::Points(channel, moved(&pts, i, x, y)));
    } else if response.clicked()
        && let Some(q) = response.interact_pointer_pos()
        && nearest(q).is_none()
    {
        let (x, y) = from_screen(q);
        out.change = Some(Change::Points(channel, inserted(&pts, x, y).0));
    }
    if response.drag_stopped() {
        held = None;
        out.drag_stopped = true;
    }
    ui.data_mut(|d| d.insert_temp(held_id, held));
    out
}

/// Lightroom's buttons over the graph: the parametric curve, then the
/// point curve's channels. A double-click resets the one clicked.
fn channel_buttons(ui: &mut Ui, theme: &Theme, channel: &mut Channel, out: &mut Out) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        for choice in Channel::ALL {
            let (rect, response) = ui.allocate_exact_size(vec2(24.0, 22.0), Sense::click());
            let on = *channel == choice;
            let painter = ui.painter();
            let c = rect.center();
            match choice {
                // A curve, for the parametric one.
                Channel::Parametric => {
                    let colour = if on { theme.foreground } else { theme.muted };
                    let s: Vec<Pos2> = (0..=12).map(|i| i as f32 / 12.0).map(|t| c + vec2((t - 0.5) * 14.0, 6.0 - 12.0 * (t * t * (3.0 - 2.0 * t)))).collect();
                    painter.add(egui::Shape::line(s, Stroke::new(1.5, colour)));
                }
                _ => {
                    painter.circle_stroke(c, 7.0, Stroke::new(2.0, choice.colour()));
                    if on {
                        painter.circle_filled(c, 3.5, choice.colour());
                    }
                }
            }
            if on {
                painter.line_segment([rect.left_bottom() + vec2(4.0, 0.0), rect.right_bottom() - vec2(4.0, 0.0)], Stroke::new(2.0, theme.accent));
            }
            let response = response.on_hover_text(format!("{} (double-click to reset it)", choice.label()));
            if response.double_clicked() {
                out.change = Some(Change::Reset(choice));
            }
            if response.clicked() {
                *channel = choice;
            }
        }
    });
}

/// The frame's histogram behind the graph, for the channel shown: a
/// peak-normalised, square-rooted profile, as Lightcraft draws it.
fn draw_histogram(painter: &egui::Painter, graph: Rect, histogram: &Histogram, channel: Channel) {
    let (bins, colour) = match channel {
        Channel::Red => (&histogram.r, Channel::Red.colour()),
        Channel::Green => (&histogram.g, Channel::Green.colour()),
        Channel::Blue => (&histogram.b, Channel::Blue.colour()),
        Channel::Parametric | Channel::Rgb => (&histogram.luma, Color32::from_gray(130)),
    };
    let colour = colour.gamma_multiply(0.27);
    // An HDR histogram's bins above SDR white aren't on the curve.
    let n = histogram.hdr.map_or(256, |hdr| hdr.from.clamp(2, 256)).min(bins.len());
    if n < 2 {
        return;
    }
    let samples: Vec<f32> = (0..256)
        .map(|i| {
            let j = i * (n - 1) / 255;
            let (start, end) = (j.saturating_sub(2), (j + 3).min(n));
            bins[start..end].iter().map(|&v| v as f32).sum::<f32>() / (end - start) as f32
        })
        .collect();
    let peak = samples.iter().copied().fold(1.0_f32, f32::max);
    let mut mesh = Mesh::default();
    for (i, v) in samples.into_iter().enumerate() {
        let x = graph.left() + graph.width() * i as f32 / 255.0;
        let top = pos2(x, graph.bottom() - graph.height() * (v / peak).sqrt());
        mesh.vertices.push(Vertex { pos: top, uv: Pos2::ZERO, color: colour });
        mesh.vertices.push(Vertex { pos: pos2(x, graph.bottom()), uv: Pos2::ZERO, color: colour });
        if i > 0 {
            let k = i as u32 * 2;
            mesh.indices.extend([k - 2, k - 1, k, k - 1, k + 1, k]);
        }
    }
    painter.add(mesh);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_straight_curve_is_no_curve() {
        let mut curve = ToneCurve::default();
        Change::Points(Channel::Rgb, vec![Point::new(0.0, 0.0), Point::new(1.0, 1.0)]).apply(&mut curve);
        assert!(curve.master.is_empty(), "kept as a fresh raw's is");
        Change::Points(Channel::Rgb, vec![Point::new(0.0, 0.0), Point::new(0.5, 0.5), Point::new(1.0, 1.0)]).apply(&mut curve);
        assert_eq!(curve.master.len(), 3, "an anchor on the line is kept");
        curve.master.clear();
        Change::Points(Channel::Red, vec![Point::new(0.0, 0.1), Point::new(1.0, 1.0)]).apply(&mut curve);
        assert_eq!(curve.red.len(), 2);
        assert!(curve.master.is_empty() && curve.green.is_empty());
        assert_eq!(parametric(&ToneCurve::default(), 4), [0.0, 0.25, 0.5, 0.75, 1.0], "the parametric one at 0 is the straight line");
    }

    #[test]
    fn points_are_added_moved_between_their_neighbours_and_taken_away_but_the_ends_stay() {
        let straight = points(&ToneCurve::default(), Channel::Rgb);
        let (three, i) = inserted(&straight, 0.4, 0.6);
        assert_eq!((three.len(), i), (3, 1));
        let moved = moved(&three, 1, 1.5, -1.0);
        assert_eq!(moved[1], Point::new(1.0 - POINT_GAP, 0.0), "it can't pass the end, or leave the graph");
        assert_eq!(removed(&moved, 1).map(|p| p.len()), Some(2));
        assert_eq!(removed(&moved, 0), None);
        assert_eq!(removed(&moved, 2), None);
    }

    #[test]
    fn splits_keep_their_order_and_range() {
        let mut curve = ToneCurve::default();
        Change::Split(0, 90.0).apply(&mut curve);
        assert_eq!(curve.split_shadows, 50.0 - SPLIT_GAP, "short of the next split");
        Change::Split(2, 0.0).apply(&mut curve);
        assert_eq!(curve.split_highlights, 50.0 + SPLIT_GAP);
        Change::Split(1, 99.0).apply(&mut curve);
        assert_eq!(curve.split_mid, curve.split_highlights - SPLIT_GAP);
        Change::Reset(Channel::Parametric).apply(&mut curve);
        assert_eq!(splits(&curve), [25.0, 50.0, 75.0]);
    }

    #[test]
    fn a_region_is_dragged_within_its_range_and_reset_all_keeps_refine_saturation() {
        let mut curve = ToneCurve { refine_saturation: 40.0, ..ToneCurve::default() };
        Change::Region(Region::Lights, 140.0).apply(&mut curve);
        assert_eq!(curve.lights, 100.0);
        assert!(parametric(&curve, 64).windows(2).all(|w| w[1] >= w[0]), "kept monotone");
        assert!(parametric(&curve, 4)[2] > 0.5, "lights lifts the middle");
        Change::Points(Channel::Blue, vec![Point::new(0.0, 0.2), Point::new(1.0, 1.0)]).apply(&mut curve);
        Change::ResetAll.apply(&mut curve);
        assert_eq!(curve, ToneCurve { refine_saturation: 40.0, ..ToneCurve::default() });
    }
}

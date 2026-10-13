//! Crop & Straighten: what the crop tool does to a frame's settings.
//!
//! The crop is Lightcraft's (`lightcraft_geom::CropGeometry`): a rectangle
//! in the frame turned by the straighten angle, as shares of the frame's
//! width and height, never reaching outside the picture (Lightroom's
//! Constrain to Image). What each gesture does to it is ported from
//! Lightcraft's `crop.*` commands (`engine/src/cmd/develop.rs` at
//! `701f78e`), with one difference: a 90° turn takes the crop round with
//! the picture, as Lightroom's does, where Lightcraft's leaves the
//! rectangle where it was on screen.
//!
//! Every function takes the frame's size as it's shown ([`dims`]); only
//! its shape matters.

use lightcraft_develop::DevelopSettings;
use lightcraft_geom::{Affine, Rect, crop_fit_angle, drag_crop};

pub use lightcraft_geom::{CropGeometry, CropHandle, Orientation, Point};

/// Lightroom's aspect list under Original, each with its ratio. The crop
/// lies the way the frame does (4 x 5 on a landscape frame is 5 wide by 4
/// high) until it's swapped ([`swap_aspect`]).
pub const RATIOS: &[(&str, (u32, u32))] = &[
    ("1 x 1", (1, 1)),
    ("4 x 5 / 8 x 10", (4, 5)),
    ("8.5 x 11", (85, 110)),
    ("5 x 7", (5, 7)),
    ("2 x 3 / 4 x 6", (2, 3)),
    ("4 x 3", (4, 3)),
    ("16 x 9", (16, 9)),
    ("16 x 10", (16, 10)),
];

/// A shape to crop to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Aspect {
    /// The frame's own.
    Original,
    /// Width to height, either way round.
    Ratio(u32, u32),
}

/// The frame's size as it's shown: the source's, turned as the settings
/// turn it.
pub fn dims(source: (usize, usize), settings: &DevelopSettings) -> (f64, f64) {
    let (w, h) = (source.0.max(1) as f64, source.1.max(1) as f64);
    if settings.orientation.swaps_axes() { (h, w) } else { (w, h) }
}

/// Crop to the largest rectangle of `aspect` that fits at the angle the
/// frame is straightened by.
pub fn set_aspect(settings: &mut DevelopSettings, (w, h): (f64, f64), aspect: Aspect) {
    let (aw, ah) = match aspect {
        Aspect::Original => (w, h),
        Aspect::Ratio(aw, ah) => (f64::from(aw.max(1)), f64::from(ah.max(1))),
    };
    settings.crop.aspect = match aspect {
        Aspect::Original => None,
        Aspect::Ratio(aw, ah) => Some((aw.max(1), ah.max(1))),
    };
    // The crop lies the way the frame does.
    let ratio = if (w >= h) == (aw >= ah) { aw / ah } else { ah / aw };
    settings.crop.geometry = crop_fit_angle(w, h, settings.crop.geometry.angle, Some(ratio));
}

/// The shape last cropped to, for the aspect menu to show: `None` once the
/// crop has been dragged to a shape of its own (Lightroom's Custom).
pub fn aspect(settings: &DevelopSettings, (w, h): (f64, f64)) -> Option<Aspect> {
    let rect = settings.crop.geometry.rect_px(w, h);
    let shape = rect.width() / rect.height();
    let is = |ratio: f64| (shape / ratio - 1.0).abs() < 0.005 || (shape * ratio - 1.0).abs() < 0.005;
    match settings.crop.aspect {
        Some((aw, ah)) if is(f64::from(aw) / f64::from(ah.max(1))) => Some(Aspect::Ratio(aw, ah)),
        None if (shape / (w / h) - 1.0).abs() < 0.005 => Some(Aspect::Original),
        _ => None,
    }
}

/// Lightroom's X: the crop turned on its side, landscape to portrait and
/// back, as large as fits.
pub fn swap_aspect(settings: &mut DevelopSettings, (w, h): (f64, f64)) {
    let rect = settings.crop.geometry.rect_px(w, h);
    let ratio = Some(rect.height() / rect.width()).filter(|a| a.is_finite() && *a > 0.0);
    settings.crop.aspect = settings.crop.aspect.map(|(aw, ah)| (ah, aw));
    settings.crop.geometry = crop_fit_angle(w, h, settings.crop.geometry.angle, ratio);
}

/// Straighten by `angle` degrees, clockwise: the crop is kept if it still
/// fits inside the turned picture, else it's the largest of its shape that
/// does.
pub fn straighten(settings: &mut DevelopSettings, (w, h): (f64, f64), angle: f64) {
    let angle = if angle.is_finite() { angle.clamp(-45.0, 45.0) } else { 0.0 };
    settings.crop.geometry = settings.crop.geometry.with_angle((angle * 100.0).round() / 100.0, w, h);
}

/// Straighten so the line from `from` to `to` is level, or plumb if it's
/// nearer upright: Lightroom's Straighten tool. The points are shares of
/// the frame's width and height as it's shown uncropped and unturned. A
/// line too short to mean anything (under a hundredth of the frame)
/// changes nothing.
pub fn straighten_along(settings: &mut DevelopSettings, (w, h): (f64, f64), from: [f64; 2], to: [f64; 2]) {
    let (dx, dy) = ((to[0] - from[0]) * w, (to[1] - from[1]) * h);
    let length = dx.hypot(dy);
    if length.is_nan() || length <= 0.01 * w.max(h) {
        return;
    }
    // To the nearer of level and plumb.
    let mut degrees = dy.atan2(dx).to_degrees();
    while degrees > 45.0 {
        degrees -= 90.0;
    }
    while degrees < -45.0 {
        degrees += 90.0;
    }
    straighten(settings, (w, h), -degrees);
}

/// A point of the frame as it's shown uncropped and unturned (shares of
/// its width and height) in the straightened frame the crop rectangle
/// lives in: where a pointer is, for [`drag`]. Lightcraft's `to_straight`
/// (`ui-egui/src/panels/detail.rs` at `701f78e`).
pub fn straightened(at: [f64; 2], angle: f64, (w, h): (f64, f64)) -> Point {
    let turned = Affine::rotate_about(angle.to_radians(), Point::new(w / 2.0, h / 2.0)).apply(Point::new(at[0] * w, at[1] * h));
    Point::new(turned.x / w, turned.y / h)
}

/// The crop rectangle's corners (top left, top right, bottom right, bottom
/// left) on the frame as it's shown uncropped and unturned, as shares of
/// its width and height: where to draw it.
pub fn corners(settings: &DevelopSettings, (w, h): (f64, f64)) -> [[f64; 2]; 4] {
    let geometry = settings.crop.geometry;
    let back = Affine::rotate_about(-geometry.angle.to_radians(), Point::new(w / 2.0, h / 2.0));
    geometry.rect_px(w, h).corners().map(|corner| {
        let at = back.apply(corner);
        [at.x / w, at.y / h]
    })
}

/// Drag `handle` of the crop `start` (as it was when the drag began) from
/// `from` to `to`, both in the straightened frame ([`straightened`]). It
/// stops at the picture's edge, and with `keep_ratio` keeps the shape it
/// had. The rules are Lightcraft's `drag_crop`.
pub fn drag(settings: &mut DevelopSettings, (w, h): (f64, f64), start: CropGeometry, handle: CropHandle, from: Point, to: Point, keep_ratio: bool) {
    let ratio = keep_ratio.then(|| start.rect_px(w, h).aspect()).filter(|a| a.is_finite() && *a > 0.0);
    let dragged = drag_crop(start, handle, from, to, w, h, ratio);
    // A shape of its own now, unless it was held.
    if !keep_ratio && dragged.rect != start.rect && handle != CropHandle::Move {
        settings.crop.aspect = None;
    }
    settings.crop.geometry = dragged;
}

/// Turn the frame a quarter turn, and its crop with it.
pub fn turn(settings: &mut DevelopSettings, clockwise: bool) {
    settings.orientation = settings.orientation.rotated(clockwise);
    let Rect { x0, y0, x1, y1 } = settings.crop.geometry.rect;
    settings.crop.geometry.rect = if clockwise { Rect::new(1.0 - y1, x0, 1.0 - y0, x1) } else { Rect::new(y0, 1.0 - x1, y1, 1.0 - x0) };
}

/// Back to the whole frame, unstraightened. Its turns stay: they're the
/// way up it is, not a crop.
pub fn reset(settings: &mut DevelopSettings) {
    settings.crop = Default::default();
}

/// A crop that reaches outside the picture (one pasted from a frame of
/// another shape, say) moved and shrunk until it doesn't.
pub fn constrain(settings: &mut DevelopSettings, (w, h): (f64, f64)) {
    settings.crop.geometry = settings.crop.geometry.constrained(w, h);
}

#[cfg(test)]
mod tests {
    use super::*;

    const LANDSCAPE: (f64, f64) = (6000.0, 4000.0);

    fn shape(settings: &DevelopSettings, dims: (f64, f64)) -> f64 {
        let rect = settings.crop.geometry.rect_px(dims.0, dims.1);
        rect.width() / rect.height()
    }

    fn near(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    #[test]
    fn an_aspect_lies_the_way_the_frame_does_and_x_swaps_it() {
        let mut s = DevelopSettings::default();
        assert_eq!(aspect(&s, LANDSCAPE), Some(Aspect::Original));
        set_aspect(&mut s, LANDSCAPE, Aspect::Ratio(4, 5));
        assert!(near(shape(&s, LANDSCAPE), 5.0 / 4.0), "landscape on a landscape frame");
        assert!(near(s.crop.geometry.rect_px(6000.0, 4000.0).height(), 4000.0), "as large as fits");
        assert_eq!(aspect(&s, LANDSCAPE), Some(Aspect::Ratio(4, 5)));

        swap_aspect(&mut s, LANDSCAPE);
        assert!(near(shape(&s, LANDSCAPE), 4.0 / 5.0));
        assert_eq!(aspect(&s, LANDSCAPE), Some(Aspect::Ratio(5, 4)));

        let portrait = (4000.0, 6000.0);
        set_aspect(&mut s, portrait, Aspect::Ratio(16, 9));
        assert!(near(shape(&s, portrait), 9.0 / 16.0), "portrait on a portrait frame");

        set_aspect(&mut s, LANDSCAPE, Aspect::Original);
        assert!(s.crop.geometry.is_identity() && s.crop.aspect.is_none());
    }

    #[test]
    fn straightening_keeps_the_crop_inside_the_picture_and_its_shape() {
        let mut s = DevelopSettings::default();
        straighten(&mut s, LANDSCAPE, 5.0);
        assert_eq!(s.crop.geometry.angle, 5.0);
        assert!(s.crop.geometry.is_within_image(6000.0, 4000.0));
        assert!(near(shape(&s, LANDSCAPE), 1.5), "the frame's own shape");
        assert!(s.crop.geometry.rect.width() < 1.0, "smaller, to fit");
        straighten(&mut s, LANDSCAPE, 90.0);
        assert_eq!(s.crop.geometry.angle, 45.0, "no further than Lightroom's 45°");
        straighten(&mut s, LANDSCAPE, f64::NAN);
        assert_eq!(s.crop.geometry.angle, 0.0);
    }

    #[test]
    fn a_line_drawn_along_the_horizon_levels_it() {
        let mut s = DevelopSettings::default();
        // A horizon that drops 6° to the right.
        let drop = 6f64.to_radians().tan() * 0.5 * 6000.0 / 4000.0;
        straighten_along(&mut s, LANDSCAPE, [0.25, 0.5], [0.75, 0.5 + drop]);
        assert!((s.crop.geometry.angle + 6.0).abs() < 0.01, "{}", s.crop.geometry.angle);

        // A wall that leans 3° is made plumb, not laid flat.
        let mut s = DevelopSettings::default();
        let lean = 3f64.to_radians().tan() * 0.5 * 4000.0 / 6000.0;
        straighten_along(&mut s, LANDSCAPE, [0.5, 0.75], [0.5 + lean, 0.25]);
        assert!((s.crop.geometry.angle.abs() - 3.0).abs() < 0.01, "{}", s.crop.geometry.angle);

        // A click isn't a line.
        let mut s = DevelopSettings::default();
        straighten_along(&mut s, LANDSCAPE, [0.5, 0.5], [0.501, 0.505]);
        assert_eq!(s.crop.geometry.angle, 0.0);
    }

    #[test]
    fn where_the_crop_is_drawn_is_where_a_pointer_finds_it() {
        let mut s = DevelopSettings::default();
        set_aspect(&mut s, LANDSCAPE, Aspect::Ratio(1, 1));
        straighten(&mut s, LANDSCAPE, 10.0);
        let rect = s.crop.geometry.rect;
        let drawn = corners(&s, LANDSCAPE);
        let found = drawn.map(|corner| straightened(corner, 10.0, LANDSCAPE));
        for (found, corner) in found.iter().zip([(rect.x0, rect.y0), (rect.x1, rect.y0), (rect.x1, rect.y1), (rect.x0, rect.y1)]) {
            assert!(near(found.x, corner.0) && near(found.y, corner.1), "{found:?} {corner:?}");
        }
        // Unturned, it's drawn where it is.
        let mut s = DevelopSettings::default();
        s.crop.geometry.rect = Rect::new(0.1, 0.2, 0.6, 0.9);
        assert_eq!(corners(&s, LANDSCAPE), [[0.1, 0.2], [0.6, 0.2], [0.6, 0.9], [0.1, 0.9]]);
    }

    #[test]
    fn a_drag_moves_its_handle_stops_at_the_edge_and_keeps_the_shape_when_asked() {
        let mut s = DevelopSettings::default();
        let start = s.crop.geometry;
        drag(&mut s, LANDSCAPE, start, CropHandle::BottomRight, Point::new(1.0, 1.0), Point::new(0.5, 0.75), false);
        assert_eq!(s.crop.geometry.rect, Rect::new(0.0, 0.0, 0.5, 0.75));
        assert_eq!(aspect(&s, LANDSCAPE), None, "a shape of its own");

        let mut s = DevelopSettings::default();
        drag(&mut s, LANDSCAPE, start, CropHandle::BottomRight, Point::new(1.0, 1.0), Point::new(0.5, 0.9), true);
        assert!(near(shape(&s, LANDSCAPE), 1.5), "held to the shape it had");
        assert_eq!(aspect(&s, LANDSCAPE), Some(Aspect::Original));

        // Past the edge, it stops there.
        let mut s = DevelopSettings::default();
        s.crop.geometry.rect = Rect::new(0.2, 0.2, 0.6, 0.6);
        let start = s.crop.geometry;
        drag(&mut s, LANDSCAPE, start, CropHandle::Move, Point::new(0.4, 0.4), Point::new(3.0, 0.4), false);
        assert!(near(s.crop.geometry.rect.x1, 1.0) && near(s.crop.geometry.rect.width(), 0.4), "{:?}", s.crop.geometry.rect);
    }

    #[test]
    fn a_quarter_turn_takes_the_crop_round_with_the_picture() {
        let mut s = DevelopSettings::default();
        // The left half of a landscape frame.
        s.crop.geometry.rect = Rect::new(0.0, 0.0, 0.5, 1.0);
        let was = s.clone();
        turn(&mut s, true);
        assert_eq!(s.crop.geometry.rect, Rect::new(0.0, 0.0, 1.0, 0.5), "turned clockwise, the left is the top");
        assert_eq!(dims((6000, 4000), &s), (4000.0, 6000.0));
        turn(&mut s, false);
        assert_eq!(s, was, "and back");
        for _ in 0..4 {
            turn(&mut s, true);
        }
        assert_eq!(s, was, "four turns are none");

        reset(&mut s);
        assert!(s.crop.geometry.is_identity());
        turn(&mut s, false);
        reset(&mut s);
        assert_ne!(s.orientation, was.orientation, "a reset keeps the way up it is");
    }

    #[test]
    fn a_crop_from_a_frame_of_another_shape_is_brought_inside() {
        // A strip down the left of a landscape frame, straightened.
        let mut s = DevelopSettings::default();
        s.crop.geometry = CropGeometry { rect: Rect::new(0.02, 0.3, 0.12, 0.7), angle: 8.0 };
        assert!(s.crop.geometry.is_within_image(6000.0, 4000.0));
        let portrait = (4000.0, 6000.0);
        assert!(!s.crop.geometry.is_within_image(portrait.0, portrait.1));
        constrain(&mut s, portrait);
        assert!(s.crop.geometry.is_within_image(portrait.0, portrait.1));
        assert_eq!(s.crop.geometry.angle, 8.0);
    }
}

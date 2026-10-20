//! Sensor dust: Lightroom's Spot Removal cut down to heal circles, for the
//! dust that sits in the same place on every frame of a shoot (healing
//! anything else is Omapix's).
//!
//! Lightcraft keeps a spot where it is in the frame as it's shown before
//! its lens corrections, crop and transform: the raw turned the way the
//! camera was held and then by the frame's quarter turns. Dust is where it
//! is on the sensor, so a spot carried to another frame goes back to the
//! sensor through this frame's ways up and out through the other's
//! ([`carry`]). Finding dust is Lightcraft's `dust::detect`, run on the
//! frame's source ([`find`]).

use lightcraft_develop::{Spot, SpotMode};
use lightcraft_geom::{Orientation, Point};
use lightcraft_raster::{Rgb32f, Rgba8};

/// A new spot's size, as a share of the frame's long edge: a speck of
/// dust at f/11 or so on a 24 MP frame.
pub const SIZE: f64 = 0.01;
/// The smallest and largest spot.
pub const SIZES: (f64, f64) = (0.002, 0.1);

/// A frame's way up: how the camera was held (the raw's Exif
/// orientation), then its quarter turns.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Way {
    pub held: Orientation,
    pub turned: Orientation,
}

/// Where a point on the sensor (a share of its width and height) is in a
/// frame `way` up.
fn shown(p: Point, way: Way) -> Point {
    let (x, y) = way.held.map(p.x, p.y, 1.0, 1.0);
    let (x, y) = way.turned.map(x, y, 1.0, 1.0);
    Point::new(x, y)
}

/// Where a point of a frame `way` up is on the sensor.
fn sensor(p: Point, way: Way) -> Point {
    let (x, y) = way.turned.inverse().map(p.x, p.y, 1.0, 1.0);
    let (x, y) = way.held.inverse().map(x, y, 1.0, 1.0);
    Point::new(x, y)
}

/// `spots` of a frame `from` up, where the same dust is in a frame `to`
/// up.
pub fn carry(spots: &[Spot], from: Way, to: Way) -> Vec<Spot> {
    if from == to {
        return spots.to_vec();
    }
    let moved = |p: Point| shown(sensor(p, from), to);
    spots
        .iter()
        .map(|spot| {
            let points: Vec<Point> = spot.points.iter().map(|p| moved(*p)).collect();
            // The source is an offset from the spot: turned with it.
            let source_offset = spot.source_offset.zip(spot.points.first()).map(|(offset, at)| {
                let (a, b) = (moved(*at), moved(Point::new(at.x + offset.x, at.y + offset.y)));
                Point::new(b.x - a.x, b.y - a.y)
            });
            Spot { points, source_offset, ..spot.clone() }
        })
        .collect()
}

/// A heal circle at `at`, `size` across (a share of the long edge), as
/// Lightroom's Heal makes one: feathered, at full opacity, its source
/// found by the pipeline.
pub fn heal(at: Point, size: f64) -> Spot {
    Spot { mode: SpotMode::Heal, points: vec![at], size: size.clamp(SIZES.0, SIZES.1), feather: 50.0, opacity: 100.0, source_offset: None }
}

/// The dust Lightcraft finds on `source` (the raw as the camera was held,
/// linear), as heal circles on a frame turned by `turned` past that, at
/// `sensitivity` (0..100). None within `existing`'s circles.
pub fn find(source: &Rgb32f, turned: Orientation, sensitivity: f32, existing: &[Spot]) -> Vec<Spot> {
    // Lightcraft's detector reads a rendered picture: the source, its
    // brightest per cent as white, with a display's gamma.
    let mut lum: Vec<f32> = source.data.iter().map(|p| 0.2627 * p[0] + 0.678 * p[1] + 0.0593 * p[2]).filter(|v| v.is_finite()).collect();
    lum.sort_by(f32::total_cmp);
    let white = lum.get(lum.len() * 99 / 100).copied().unwrap_or(1.0).max(1e-4);
    let mut picture = Rgba8::new(source.width, source.height);
    for (out, p) in picture.data.iter_mut().zip(&source.data) {
        let v = |c: f32| ((c.max(0.0) / white).min(1.0).powf(1.0 / 2.2) * 255.0).round() as u8;
        *out = [v(p[0]), v(p[1]), v(p[2]), 255];
    }
    let near = |at: Point, spots: &[Spot]| spots.iter().any(|s| s.points.first().is_some_and(|p| (p.x - at.x).hypot(p.y - at.y) < s.size.max(SIZE)));
    let mut found: Vec<Spot> = Vec::new();
    for dust in lightcraft_pipeline::dust::detect(&picture, sensitivity) {
        let (x, y) = turned.map(dust.x, dust.y, 1.0, 1.0);
        let at = Point::new(x, y);
        if !near(at, existing) && !near(at, &found) {
            // A circle a little larger than the speck, to heal it whole.
            found.push(heal(at, (dust.radius * 2.0 * 1.5).max(SIZES.0)));
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Point, b: Point) -> bool {
        (a.x - b.x).abs() < 1e-9 && (a.y - b.y).abs() < 1e-9
    }

    #[test]
    fn dust_carried_to_a_frame_held_or_turned_another_way_is_where_it_is_on_the_sensor() {
        let level = Way::default();
        let spot = heal(Point::new(0.2, 0.1), SIZE);
        // The same way up: as it is.
        assert_eq!(carry(std::slice::from_ref(&spot), level, level), std::slice::from_ref(&spot));
        // Held a quarter turn clockwise (Exif 6): the sensor's top left is the frame's top right.
        let portrait = Way { held: Orientation::Rotate90, turned: Orientation::Normal };
        let there = carry(std::slice::from_ref(&spot), level, portrait);
        assert!(close(there[0].points[0], Point::new(0.9, 0.2)), "{:?}", there[0].points);
        assert_eq!(there[0].size, spot.size, "the size is a share of the long edge, whichever way up");
        // And back, through a frame turned by hand as well.
        let turned = Way { held: Orientation::Rotate90, turned: Orientation::Rotate270 };
        let back = carry(&carry(&there, portrait, turned), turned, level);
        assert!(close(back[0].points[0], spot.points[0]), "{:?}", back[0].points);
        // A source offset turns with its spot.
        let cloned = Spot { source_offset: Some(Point::new(0.05, 0.0)), ..spot.clone() };
        let offset = carry(&[cloned], level, portrait)[0].source_offset.unwrap();
        assert!(close(offset, Point::new(0.0, 0.05)), "{offset:?}");
    }

    #[test]
    fn dust_is_found_on_a_clear_sky_once_and_not_where_theres_a_spot() {
        // A smooth gradient of sky with one soft dark speck.
        let (w, h) = (600, 400);
        let speck = (420.0, 120.0);
        let source = Rgb32f::from_fn(w, h, |x, y| {
            let sky = 0.25 + 0.15 * y as f32 / h as f32;
            let d = ((x as f32 - speck.0).powi(2) + (y as f32 - speck.1).powi(2)).sqrt();
            let dark = 0.25 * (-(d / 3.0).powi(2)).exp();
            [sky * (1.0 - dark), sky * (1.0 - dark), sky * 1.1 * (1.0 - dark)]
        });
        let found = find(&source, Orientation::Normal, 60.0, &[]);
        assert_eq!(found.len(), 1, "{found:?}");
        let at = found[0].points[0];
        assert!((at.x - 0.7).abs() < 0.01 && (at.y - 0.3).abs() < 0.01, "{at:?}");
        assert_eq!(found[0].mode, SpotMode::Heal);
        assert!(find(&source, Orientation::Normal, 60.0, &found).is_empty(), "not found again");
        // On a frame turned clockwise, where it is on that frame.
        let turned = find(&source, Orientation::Rotate90, 60.0, &[]);
        assert!((turned[0].points[0].x - 0.7).abs() < 0.01 && (turned[0].points[0].y - 0.7).abs() < 0.01, "{:?}", turned[0].points);
    }
}

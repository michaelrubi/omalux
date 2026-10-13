//! Stacks: frames shot together, like a burst, by capture time and how alike
//! the frames look.
//!
//! Ported from Omacull (`omacull-engine/src/stacks.rs`).
//! OmaLux always stacks by capture time and look, so there is no stacking
//! setting.

use lightcraft_raster::Rgba8;

/// Frames this close in seconds are one burst.
pub const GAP: f64 = 2.0;
/// Frames this close that look alike are one moment, shot a few times.
pub const SIMILAR_GAP: f64 = 30.0;
/// Signatures closer than this look alike.
const ALIKE: f32 = 18.0;

const COLUMNS: usize = 16;
const ROWS: usize = 12;

/// What a frame looks like, very roughly: its brightness on a 16 × 12 grid,
/// from the camera's own thumbnail, for telling a new scene from the same
/// one shot again.
#[derive(Clone, Debug, PartialEq)]
pub struct Signature {
    grid: Vec<u8>,
}

impl Signature {
    pub fn of(thumbnail: &Rgba8) -> Self {
        let small = shrunk(thumbnail, COLUMNS.max(ROWS));
        let mut grid = vec![0u8; COLUMNS * ROWS];
        if small.width > 0 && small.height > 0 {
            for (i, cell) in grid.iter_mut().enumerate() {
                let (x, y) = (i % COLUMNS * small.width / COLUMNS, i / COLUMNS * small.height / ROWS);
                let [r, g, b, _] = small.get(x, y);
                *cell = ((u32::from(r) * 54 + u32::from(g) * 183 + u32::from(b) * 19) >> 8) as u8;
            }
        }
        Self { grid }
    }

    /// How different two frames look: the average difference in level,
    /// once the overall brightness is evened out (a burst's exposures
    /// wander).
    pub fn distance(&self, other: &Signature) -> f32 {
        let mean = |g: &[u8]| g.iter().map(|&v| f32::from(v)).sum::<f32>() / g.len().max(1) as f32;
        let shift = mean(&self.grid) - mean(&other.grid);
        let difference = |(&a, &b): (&u8, &u8)| (f32::from(a) - f32::from(b) - shift).abs();
        let sum: f32 = self.grid.iter().zip(&other.grid).map(difference).sum();
        sum / self.grid.len().max(1) as f32
    }
}

/// Shrunk so its long edge is at most `long_edge`, each pixel the
/// average of the ones it covers.
fn shrunk(thumbnail: &Rgba8, long_edge: usize) -> Rgba8 {
    if thumbnail.width == 0 || thumbnail.height == 0 {
        return thumbnail.clone();
    }
    let scale = long_edge as f64 / thumbnail.width.max(thumbnail.height) as f64;
    if scale >= 1.0 {
        return thumbnail.clone();
    }
    let ow = ((thumbnail.width as f64 * scale).round() as usize).max(1);
    let oh = ((thumbnail.height as f64 * scale).round() as usize).max(1);
    let span = |o: usize, of: usize, from: usize| (o * from / of, ((o + 1) * from / of).max(o * from / of + 1).min(from));
    let mut data = Vec::with_capacity(ow * oh);
    for oy in 0..oh {
        let (y0, y1) = span(oy, oh, thumbnail.height);
        for ox in 0..ow {
            let (x0, x1) = span(ox, ow, thumbnail.width);
            let mut sum = [0u32; 4];
            for y in y0..y1 {
                for x in x0..x1 {
                    let px = thumbnail.get(x, y);
                    for c in 0..4 {
                        sum[c] += u32::from(px[c]);
                    }
                }
            }
            let n = ((y1 - y0) * (x1 - x0)).max(1) as u32;
            data.push(sum.map(|s| ((s + n / 2) / n) as u8));
        }
    }
    Rgba8 { width: ow, height: oh, data }
}

/// An Exif capture time (`2026:10:04 15:43:16.123`) in seconds, counted
/// from an arbitrary day: for the gaps between frames.
pub fn seconds(captured: &str) -> Option<f64> {
    let (date, clock) = captured.split_once(' ')?;
    let mut date = date.split(':').map(|p| p.parse::<i64>().ok());
    let (year, month, day) = (date.next()??, date.next()??, date.next()??);
    let mut clock = clock.split(':');
    let (hour, minute) = (clock.next()?.parse::<f64>().ok()?, clock.next()?.parse::<f64>().ok()?);
    let second = clock.next()?.parse::<f64>().ok()?;
    // Days from a fixed day to this one (Howard Hinnant's days_from_civil).
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let days = era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(days as f64 * 86_400.0 + hour * 3600.0 + minute * 60.0 + second)
}

/// The stacks among frames in order, each a run of two or more, from
/// their capture times (seconds) and signatures. Frames whose time isn't
/// known are never stacked.
pub fn group(frames: &[(Option<f64>, Option<&Signature>)]) -> Vec<Vec<usize>> {
    let together = |a: usize, b: usize| {
        let (Some(ta), Some(tb)) = (frames[a].0, frames[b].0) else { return false };
        let gap = (tb - ta).abs();
        let alike = match (frames[a].1, frames[b].1) {
            (Some(sa), Some(sb)) => sa.distance(sb) < ALIKE,
            // Without thumbnails to go by, a burst is still a burst.
            _ => gap <= GAP,
        };
        gap <= SIMILAR_GAP && alike
    };
    let mut stacks = Vec::new();
    let mut run = vec![];
    for i in 0..frames.len() {
        if run.last().is_some_and(|&last| !together(last, i)) {
            if run.len() > 1 {
                stacks.push(std::mem::take(&mut run));
            }
            run.clear();
        }
        run.push(i);
    }
    if run.len() > 1 {
        stacks.push(run);
    }
    stacks
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picture(f: impl Fn(usize, usize) -> u8) -> Rgba8 {
        let (w, h) = (64, 48);
        Rgba8::from_fn(w, h, |x, y| [f(x, y), f(x, y), f(x, y), 255])
    }

    #[test]
    fn capture_times_count_across_days() {
        let t = |s| seconds(s).unwrap();
        assert_eq!(t("2026:10:04 15:43:17") - t("2026:10:04 15:43:16.5"), 0.5);
        assert_eq!(t("2026:10:05 00:00:00") - t("2026:10:04 23:59:59"), 1.0);
        assert_eq!(t("2026:03:01 00:00:00") - t("2026:02:28 00:00:00"), 86_400.0);
        assert_eq!(t("2024:03:01 00:00:00") - t("2024:02:28 00:00:00"), 2.0 * 86_400.0, "a leap year");
        assert_eq!(seconds("    :  :     :  :  "), None);
    }

    #[test]
    fn the_same_scene_looks_alike_and_another_doesnt() {
        let scene = picture(|x, y| (x * 3 + y) as u8);
        let brighter = picture(|x, y| (x * 3 + y + 20) as u8);
        let other = picture(|x, _| if x < 32 { 230 } else { 10 });
        let a = Signature::of(&scene);
        assert!(a.distance(&Signature::of(&brighter)) < 1.0, "exposure aside");
        assert!(a.distance(&Signature::of(&other)) > ALIKE);
    }

    #[test]
    fn bursts_are_stacked_by_their_gaps() {
        let scene = Signature::of(&picture(|x, y| (x * 3 + y) as u8));
        let other = Signature::of(&picture(|x, _| if x < 32 { 230 } else { 10 }));
        let frames = [
            (Some(0.0), Some(&scene)),
            (Some(0.2), Some(&scene)),
            (Some(0.4), Some(&scene)),
            (Some(10.0), Some(&scene)),
            (Some(14.0), Some(&scene)),
            (Some(14.5), Some(&other)),
            (None, Some(&other)),
            (Some(15.0), Some(&other)),
        ];
        assert_eq!(group(&frames), [vec![0, 1, 2, 3, 4]]);

        // Without thumbnails, frames close enough are still a burst.
        let no_thumbs = [
            (Some(0.0), None),
            (Some(1.5), None),
            (Some(4.0), None),
        ];
        assert_eq!(group(&no_thumbs), [vec![0, 1]]);
    }
}

//! Exposure fusion: the alternative to the HDR slider's local tone
//! mapping that M4 tries (see "HDR" in docs/DESIGN.md). Several exposures
//! of one frame, each developed from the same raw, mixed so that every
//! part comes from the exposure that shows it best.
//!
//! Ported from `crates/omapix-engine/src/fusion.rs` (Mertens, Kautz and Van
//! Reeth).

use rayon::prelude::*;

use lightcraft_raster::Rgb32f;

/// A grey image and its size.
type Plane = (Vec<f32>, usize, usize);

/// Half the size: every other pixel, each the average of the five by five
/// around it, the nearer the more.
fn halved((p, w, h): &Plane) -> Plane {
    const NEAR: [f32; 5] = [1.0 / 16.0, 4.0 / 16.0, 6.0 / 16.0, 4.0 / 16.0, 1.0 / 16.0];
    let (w, h) = (*w, *h);
    let (w2, h2) = (w.div_ceil(2), h.div_ceil(2));
    let around = |i: usize, k: usize, n: usize| (2 * i + k).saturating_sub(2).min(n - 1);
    let mut across = vec![0.0; w2 * h];
    across.par_chunks_mut(w2).zip(p.par_chunks(w)).for_each(|(out, row)| {
        for (x, o) in out.iter_mut().enumerate() {
            *o = (0..5).map(|k| NEAR[k] * row[around(x, k, w)]).sum();
        }
    });
    let mut out = vec![0.0; w2 * h2];
    out.par_chunks_mut(w2).enumerate().for_each(|(y, out)| {
        for (k, near) in NEAR.iter().enumerate() {
            let row = &across[around(y, k, h) * w2..][..w2];
            for (o, v) in out.iter_mut().zip(row) {
                *o += near * v;
            }
        }
    });
    (out, w2, h2)
}

/// Twice the size, `w` × `h`: [`halved`] the other way.
fn doubled((p, w2, h2): &Plane, w: usize, h: usize) -> Vec<f32> {
    let (w2, h2) = (*w2, *h2);
    // The pixels of the smaller one that make pixel `i`, and how much of each.
    let from = |i: usize, n: usize| {
        let at = i / 2;
        let next = (at + 1).min(n - 1);
        if i.is_multiple_of(2) {
            [(at.saturating_sub(1), 0.125), (at, 0.75), (next, 0.125)]
        } else {
            [(at, 0.5), (next, 0.5), (at, 0.0)]
        }
    };
    let mut across = vec![0.0; w * h2];
    across.par_chunks_mut(w).zip(p.par_chunks(w2)).for_each(|(out, row)| {
        for (x, o) in out.iter_mut().enumerate() {
            *o = from(x, w2).iter().map(|&(i, k)| k * row[i]).sum();
        }
    });
    let mut out = vec![0.0; w * h];
    out.par_chunks_mut(w).enumerate().for_each(|(y, out)| {
        for (i, k) in from(y, h2) {
            for (o, v) in out.iter_mut().zip(&across[i * w..][..w]) {
                *o += k * v;
            }
        }
    });
    out
}

/// `plane` at full size, half, a quarter and so on down to a few pixels.
fn sizes(plane: Plane) -> Vec<Plane> {
    let mut levels = vec![plane];
    while levels.last().is_some_and(|(_, w, h)| *w.min(h) >= 16) {
        if let Some(last) = levels.last() {
            levels.push(halved(last));
        }
    }
    levels
}

/// `plane`'s detail at each size: what each of its [`sizes`] has that the
/// next one down doesn't, and the smallest as it is. They add back up to
/// it ([`whole`]).
fn details(plane: Plane) -> Vec<Plane> {
    let mut levels = sizes(plane);
    for i in 0..levels.len().saturating_sub(1) {
        let (below, rest) = levels.split_at_mut(i + 1);
        let (p, w, h) = &mut below[i];
        let smooth = doubled(&rest[0], *w, *h);
        p.par_iter_mut().zip(smooth).for_each(|(v, s)| *v -= s);
    }
    levels
}

fn whole(mut levels: Vec<Plane>) -> Vec<f32> {
    while levels.len() > 1 {
        let Some(small) = levels.pop() else { break };
        let Some((p, w, h)) = levels.last_mut() else { break };
        let smooth = doubled(&small, *w, *h);
        p.par_iter_mut().zip(smooth).for_each(|(v, s)| *v += s);
    }
    levels.pop().map(|l| l.0).unwrap_or_default()
}

/// How much say a pixel has: most when it's mid grey, less the nearer
/// black or white it is, and a little more for being colourful. None
/// for a pixel that isn't finite.
fn say(p: [f32; 3]) -> f32 {
    let [r, g, b] = p;
    if !r.is_finite() || !g.is_finite() || !b.is_finite() {
        return 0.0;
    }
    let [r, g, b] = [r.clamp(0.0, 1.0), g.clamp(0.0, 1.0), b.clamp(0.0, 1.0)];
    let grey = 0.299 * r + 0.587 * g + 0.114 * b;
    let mean = (r + g + b) / 3.0;
    let colour = (((r - mean).powi(2) + (g - mean).powi(2) + (b - mean).powi(2)) / 3.0).sqrt();
    let exposed = (-(grey - 0.5).powi(2) / (2.0 * 0.2 * 0.2)).exp();
    exposed + 0.2 * colour + 1e-6
}

/// Fuse `exposures` of one frame: the same size, display-referred (sRGB's
/// curve, 0..1). `None` if there are none or their sizes differ.
pub fn fuse(exposures: &[Rgb32f]) -> Option<Rgb32f> {
    let first = exposures.first()?;
    let (w, h) = (first.width, first.height);
    if w == 0 || h == 0 || exposures.iter().any(|e| e.width != w || e.height != h) {
        return None;
    }
    let total_pixels = w.checked_mul(h)?;
    if exposures.iter().any(|e| e.data.len() != total_pixels) {
        return None;
    }

    // A finite pixel from whichever exposure has one, to stand in where
    // another exposure has NaNs so its edge isn't taken for detail.
    let stand_in: Vec<[f32; 3]> = (0..total_pixels)
        .into_par_iter()
        .map(|i| {
            for exp in exposures {
                let p = exp.data[i];
                if p[0].is_finite() && p[1].is_finite() && p[2].is_finite() {
                    return p;
                }
            }
            [0.0, 0.0, 0.0]
        })
        .collect();

    // Each exposure's share of the say at each pixel.
    let mut shares: Vec<Vec<f32>> = exposures
        .iter()
        .map(|exp| exp.data.par_iter().map(|&p| say(p)).collect())
        .collect();
    let total: Vec<f32> = (0..total_pixels)
        .into_par_iter()
        .map(|i| shares.iter().map(|s| s[i]).sum())
        .collect();
    for share in &mut shares {
        share.par_iter_mut().zip(&total).for_each(|(s, t)| {
            *s = if *t > 0.0 { *s / t } else { 0.0 };
        });
    }

    let mut merged: [Vec<Plane>; 3] = Default::default();
    for (exp, share) in exposures.iter().zip(shares) {
        let share = sizes((share, w, h));
        for (c, merged) in merged.iter_mut().enumerate() {
            let channel: Vec<f32> = exp
                .data
                .par_iter()
                .zip(&stand_in)
                .map(|(p, under)| if p[c].is_finite() { p[c] } else { under[c] })
                .collect();
            let mut detail = details((channel, w, h));
            for ((d, ..), (s, ..)) in detail.iter_mut().zip(&share) {
                d.par_iter_mut().zip(s).for_each(|(d, s)| *d *= s);
            }
            if merged.is_empty() {
                *merged = detail;
            } else {
                for ((m, ..), (d, ..)) in merged.iter_mut().zip(detail) {
                    m.par_iter_mut().zip(d).for_each(|(m, d)| *m += d);
                }
            }
        }
    }
    let [r, g, b] = merged.map(whole);
    let out: Vec<[f32; 3]> = (0..total_pixels)
        .into_par_iter()
        .map(|i| {
            let c = |v: f32| if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 };
            [c(r[i]), c(g[i]), c(b[i])]
        })
        .collect();
    Some(Rgb32f { width: w, height: h, data: out })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn none_and_mismatched_sizes_are_none() {
        assert!(fuse(&[]).is_none());
        let a = Rgb32f::filled(20, 20, [0.5, 0.5, 0.5]);
        let b = Rgb32f::filled(20, 30, [0.5, 0.5, 0.5]);
        let c = Rgb32f::filled(30, 20, [0.5, 0.5, 0.5]);
        assert!(fuse(&[a.clone(), b]).is_none());
        assert!(fuse(&[a.clone(), c]).is_none());
        let zero = Rgb32f::new(0, 0);
        assert!(fuse(&[zero]).is_none());
    }

    #[test]
    fn one_exposure_comes_back_as_it_went_in() {
        let (w, h) = (80, 60);
        let img = Rgb32f::from_fn(w, h, |x, y| [x as f32 / w as f32, y as f32 / h as f32, (x + y) as f32 / (w + h) as f32]);
        let fused = fuse(std::slice::from_ref(&img)).expect("fuses one exposure");
        for (orig, fused) in img.data.iter().zip(&fused.data) {
            for c in 0..3 {
                assert!((orig[c] - fused[c]).abs() < 1e-4, "channel {c}: orig {}, fused {}", orig[c], fused[c]);
            }
        }
    }

    #[test]
    fn detail_at_every_size_adds_back_up_to_the_image() {
        // An odd size, so the halves don't divide evenly.
        let (w, h) = (301, 173);
        let plane: Vec<f32> = (0..w * h).map(|i| (((i * 7919) % 1000) as f32 + (i % w) as f32) / 1500.0).collect();
        let levels = details((plane.clone(), w, h));
        assert!(levels.len() > 3 && levels.last().is_some_and(|(_, w, h)| *w.min(h) < 16));
        let back = whole(levels);
        let off = plane.iter().zip(&back).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
        assert!(off < 1e-4, "{off}");
    }

    #[test]
    fn dark_and_bright_exposure_keeps_detail_in_both_halves() {
        // Left half is dark, right half is bright.
        // Detail is fine alternating steps in each half.
        let (w, h) = (200, 100);
        let dark = Rgb32f::from_fn(w, h, |x, y| {
            if x < 100 {
                // Crushed to 0: no detail.
                [0.0; 3]
            } else {
                // Well exposed around mid grey with fine detail.
                let step = if (x + y) % 2 == 0 { 0.05 } else { -0.05 };
                [0.5 + step; 3]
            }
        });
        let bright = Rgb32f::from_fn(w, h, |x, y| {
            if x < 100 {
                // Well exposed around mid grey with fine detail.
                let step = if (x + y) % 2 == 0 { 0.05 } else { -0.05 };
                [0.5 + step; 3]
            } else {
                // Burnt out to 1.0: no detail.
                [1.0; 3]
            }
        });

        let fused = fuse(&[dark.clone(), bright.clone()]).expect("fuses exposures");

        // Local contrast measured as mean absolute difference between adjacent horizontal pixels.
        let contrast = |img: &Rgb32f, x_start: usize, x_end: usize| -> f32 {
            let mut sum = 0.0;
            let mut count = 0;
            for y in 20..80 {
                for x in x_start..x_end {
                    sum += (img.get(x + 1, y)[0] - img.get(x, y)[0]).abs();
                    count += 1;
                }
            }
            sum / count as f32
        };

        // Dark half (left): fused has more detail than the dark exposure.
        let dark_half_dark = contrast(&dark, 20, 80);
        let dark_half_fused = contrast(&fused, 20, 80);
        assert_eq!(dark_half_dark, 0.0);
        assert!(dark_half_fused > 0.04, "dark half fused contrast: {dark_half_fused}");

        // Bright half (right): fused has more detail than the bright exposure.
        let bright_half_bright = contrast(&bright, 120, 180);
        let bright_half_fused = contrast(&fused, 120, 180);
        assert_eq!(bright_half_bright, 0.0);
        assert!(bright_half_fused > 0.04, "bright half fused contrast: {bright_half_fused}");
    }

    #[test]
    fn flat_mid_grey_pair_fuses_to_mid_grey() {
        let g1 = Rgb32f::filled(64, 64, [0.5, 0.5, 0.5]);
        let g2 = Rgb32f::filled(64, 64, [0.5, 0.5, 0.5]);
        let fused = fuse(&[g1, g2]).expect("fuses pair");
        assert!(fused.data.iter().all(|p| (p[0] - 0.5).abs() < 1e-4 && (p[1] - 0.5).abs() < 1e-4 && (p[2] - 0.5).abs() < 1e-4));
    }

    #[test]
    fn input_with_nans_gives_finite_output() {
        let mut nan1 = Rgb32f::filled(40, 40, [0.5, 0.5, 0.5]);
        nan1.data[0] = [f32::NAN, f32::NAN, f32::NAN];
        nan1.data[5] = [f32::INFINITY, 0.5, 0.5];
        nan1.data[10] = [0.5, f32::NEG_INFINITY, 0.5];
        let normal = Rgb32f::filled(40, 40, [0.5, 0.5, 0.5]);
        let fused = fuse(&[nan1, normal]).expect("fuses");
        assert!(fused.data.iter().all(|p| p[0].is_finite() && p[1].is_finite() && p[2].is_finite()));

        let mut nan_only = Rgb32f::filled(40, 40, [0.5, 0.5, 0.5]);
        nan_only.data[0] = [f32::NAN, 0.5, 0.5];
        let fused_only = fuse(&[nan_only]).expect("fuses");
        assert!(fused_only.data.iter().all(|p| p[0].is_finite() && p[1].is_finite() && p[2].is_finite()));
    }
}


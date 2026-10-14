//! How a lens correction compares with the camera's own JPEG, which the
//! ILCE-7M3 corrects for distortion, vignetting and lateral CA (its
//! "Lens Comp." on Auto): M5's measure for picking the default source.
//!
//! Given a render and the camera's JPEG of the same frame:
//! 1. Geometry: split the frame into a 24 × 16 grid of patches, find each
//!    patch's sub-pixel shift on luminance by searching for minimum SSD of
//!    gradients, and keep patches with sufficient texture and a clear peak.
//!    Fit and remove a similarity (scale and translation) over all patches,
//!    reporting fitted scale, residual displacement by radius bins, median and
//!    95th percentile residuals, and patch count.
//! 2. Vignetting: after aligning by the fitted similarity, measure the difference
//!    in L* (render minus JPEG) by radius bin over unclipped pixels, reporting
//!    the corner bin minus the centre bin.

use lightcraft_raster::Rgba8;
use rayon::prelude::*;

const GRID_COLS: usize = 24;
const GRID_ROWS: usize = 16;
const PATCH_R: usize = 14;
const SEARCH_R: isize = 14;
const NUM_BINS: usize = 5;

/// One radius bin from centre (0.0) to corner (1.0).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RadiusBin {
    /// Normalized radius range [r_min, r_max].
    pub r_min: f64,
    pub r_max: f64,
    /// Number of patches in this radius bin.
    pub patches: usize,
    /// Mean residual displacement in pixels across patches in this bin.
    pub mean_residual: f64,
    /// Difference in L* (render minus JPEG) across unclipped pixels in this bin.
    pub delta_l: f64,
    /// Number of unclipped pixels contributing to `delta_l`.
    pub unclipped_pixels: usize,
}

/// The result of measuring a render against the camera's JPEG.
#[derive(Clone, Debug, PartialEq)]
pub struct Measure {
    /// Fitted similarity scale (one scale relative to the JPEG).
    pub scale: f64,
    /// Fitted translation [tx, ty] in pixels.
    pub translation: [f64; 2],
    /// Number of patches with enough texture and a clear peak used in the fit.
    pub patches_used: usize,
    /// Median residual displacement across used patches in pixels.
    pub median_residual: f64,
    /// 95th percentile residual displacement across used patches in pixels.
    pub p95_residual: f64,
    /// Residuals and L* differences divided into radius bins from centre to corner.
    pub bins: Vec<RadiusBin>,
    /// Corner bin's delta_l minus centre bin's delta_l.
    pub vignetting_diff: f64,
}

/// Measures the geometry and vignetting differences between `render` and `jpeg`.
/// Both are sRGB Rgba8 images; if their sizes differ, `render` is resampled to match `jpeg`.
pub fn check(render: &Rgba8, jpeg: &Rgba8) -> Result<Measure, String> {
    if jpeg.width < 64 || jpeg.height < 64 {
        return Err(format!("image too small to measure: {}×{}", jpeg.width, jpeg.height));
    }
    let resampled;
    let render = if (render.width, render.height) == (jpeg.width, jpeg.height) {
        render
    } else {
        resampled = lightcraft_raster::resample::resize(&render.to_linear(), jpeg.width, jpeg.height, lightcraft_raster::resample::Filter::Box).to_srgb8();
        &resampled
    };

    let (w, h) = (jpeg.width, jpeg.height);
    let (lum_r, gx_r, gy_r) = luminance_and_gradients(render);
    let (lum_j, gx_j, gy_j) = luminance_and_gradients(jpeg);

    let patches: Vec<(f64, f64, f64, f64)> = (0..GRID_ROWS)
        .into_par_iter()
        .flat_map(|row| {
            let mut row_patches = Vec::new();
            for col in 0..GRID_COLS {
                let xc = (col as f64 + 0.5) * w as f64 / GRID_COLS as f64;
                let yc = (row as f64 + 0.5) * h as f64 / GRID_ROWS as f64;
                let ixc = xc.round() as usize;
                let iyc = yc.round() as usize;

                let bound = (PATCH_R as isize) + SEARCH_R + 1;
                if (ixc as isize) < bound
                    || (ixc as isize) + bound >= (w as isize)
                    || (iyc as isize) < bound
                    || (iyc as isize) + bound >= (h as isize)
                {
                    continue;
                }

                if let Some((u, v)) = match_patch(ixc, iyc, w, &gx_r, &gy_r, &gx_j, &gy_j) {
                    let tx = xc - w as f64 / 2.0;
                    let ty = yc - h as f64 / 2.0;
                    row_patches.push((tx, ty, u, v));
                }
            }
            row_patches
        })
        .collect();

    if patches.len() < 8 {
        return Err(format!("too few reliable patches found: {}", patches.len()));
    }

    let (mut scale, mut translation) = fit_similarity(&patches)?;
    let mut inliers = patches;
    let residuals: Vec<f64> = inliers
        .iter()
        .map(|&(x, y, u, v)| {
            let pred_u = (scale - 1.0) * x + translation[0];
            let pred_v = (scale - 1.0) * y + translation[1];
            (u - pred_u).hypot(v - pred_v)
        })
        .collect();

    let mut sorted_res = residuals.clone();
    sorted_res.sort_by(f64::total_cmp);
    let initial_median = sorted_res[sorted_res.len() / 2];
    let outlier_cutoff = (3.5 * initial_median + 2.0).max(1.0);

    let retained: Vec<(f64, f64, f64, f64)> = inliers
        .iter()
        .copied()
        .zip(&residuals)
        .filter(|&(_, &r)| r <= outlier_cutoff)
        .map(|(p, _)| p)
        .collect();

    if retained.len() >= 8 {
        inliers = retained;
        let (s, t) = fit_similarity(&inliers)?;
        scale = s;
        translation = t;
    }

    let final_residuals: Vec<(f64, f64, f64)> = inliers
        .iter()
        .map(|&(x, y, u, v)| {
            let pred_u = (scale - 1.0) * x + translation[0];
            let pred_v = (scale - 1.0) * y + translation[1];
            let res = (u - pred_u).hypot(v - pred_v);
            let r = x.hypot(y);
            (r, res, res)
        })
        .collect();

    let mut all_res: Vec<f64> = final_residuals.iter().map(|&(_, res, _)| res).collect();
    all_res.sort_by(f64::total_cmp);
    let median_residual = all_res[all_res.len() / 2];
    let p95_residual = all_res[((all_res.len() - 1) as f64 * 0.95).round() as usize];

    let max_radius = ((w as f64 / 2.0).powi(2) + (h as f64 / 2.0).powi(2)).sqrt();
    let bin_width = 1.0 / NUM_BINS as f64;

    let (vignette_sums, vignette_counts) = measure_vignetting(render, jpeg, scale, translation, max_radius);

    let mut bins = Vec::with_capacity(NUM_BINS);
    for b in 0..NUM_BINS {
        let r_min = b as f64 * bin_width;
        let r_max = (b + 1) as f64 * bin_width;
        let bin_res: Vec<f64> = final_residuals
            .iter()
            .filter(|&&(r, ..)| {
                let norm_r = r / max_radius;
                if b == NUM_BINS - 1 {
                    norm_r >= r_min && norm_r <= r_max + 1e-4
                } else {
                    norm_r >= r_min && norm_r < r_max
                }
            })
            .map(|&(_, res, _)| res)
            .collect();

        let mean_residual = if bin_res.is_empty() { 0.0 } else { bin_res.iter().sum::<f64>() / bin_res.len() as f64 };
        let count = vignette_counts[b];
        let delta_l = if count > 0 { vignette_sums[b] / count as f64 } else { 0.0 };

        bins.push(RadiusBin {
            r_min,
            r_max: r_max.min(1.0),
            patches: bin_res.len(),
            mean_residual,
            delta_l,
            unclipped_pixels: count,
        });
    }

    let vignetting_diff = bins.last().map(|b| b.delta_l).unwrap_or(0.0) - bins.first().map(|b| b.delta_l).unwrap_or(0.0);

    let _ = (lum_r, lum_j);
    Ok(Measure {
        scale,
        translation,
        patches_used: inliers.len(),
        median_residual,
        p95_residual,
        bins,
        vignetting_diff,
    })
}

fn luminance_and_gradients(img: &Rgba8) -> (Vec<f32>, Vec<f32>, Vec<f32>) {
    let (w, h) = (img.width, img.height);
    let mut lum = Vec::with_capacity(w * h);
    for p in &img.data {
        lum.push(0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32);
    }

    let mut gx = vec![0.0f32; w * h];
    let mut gy = vec![0.0f32; w * h];

    for y in 0..h {
        let row = y * w;
        for x in 0..w {
            let left = if x > 0 { lum[row + x - 1] } else { lum[row + x] };
            let right = if x + 1 < w { lum[row + x + 1] } else { lum[row + x] };
            gx[row + x] = (right - left) * 0.5;

            let top = if y > 0 { lum[(y - 1) * w + x] } else { lum[row + x] };
            let bottom = if y + 1 < h { lum[(y + 1) * w + x] } else { lum[row + x] };
            gy[row + x] = (bottom - top) * 0.5;
        }
    }

    (lum, gx, gy)
}

fn match_patch(
    ixc: usize,
    iyc: usize,
    w: usize,
    gx_r: &[f32],
    gy_r: &[f32],
    gx_j: &[f32],
    gy_j: &[f32],
) -> Option<(f64, f64)> {
    let r = PATCH_R as isize;
    let mut m_xx = 0.0f64;
    let mut m_yy = 0.0f64;
    let mut m_xy = 0.0f64;

    for dy in -r..=r {
        let y = (iyc as isize + dy) as usize;
        let row_off = y * w;
        for dx in -r..=r {
            let x = (ixc as isize + dx) as usize;
            let gx = gx_j[row_off + x] as f64;
            let gy = gy_j[row_off + x] as f64;
            m_xx += gx * gx;
            m_yy += gy * gy;
            m_xy += gx * gy;
        }
    }

    let np = ((2 * r + 1) * (2 * r + 1)) as f64;
    let tr = m_xx + m_yy;
    let det = m_xx * m_yy - m_xy * m_xy;
    if det <= 0.0 || tr <= 0.0 {
        return None;
    }
    let lambda_min = (tr - (tr * tr - 4.0 * det).max(0.0).sqrt()) / (2.0 * np);
    if lambda_min < 2.5 {
        return None;
    }

    let s = SEARCH_R;
    let stride = (2 * s + 1) as usize;
    let mut table = vec![0.0f64; stride * stride];
    let mut best_ssd = f64::INFINITY;
    let mut best_u = 0isize;
    let mut best_v = 0isize;
    let mut sum_ssd = 0.0f64;

    for v in -s..=s {
        let y_render_base = (iyc as isize + v) as usize;
        for u in -s..=s {
            let x_render_base = (ixc as isize + u) as usize;
            let mut ssd = 0.0f64;

            for dy in -r..=r {
                let yj = (iyc as isize + dy) as usize;
                let yr = (y_render_base as isize + dy) as usize;
                let row_j = yj * w;
                let row_r = yr * w;
                for dx in -r..=r {
                    let xj = (ixc as isize + dx) as usize;
                    let xr = (x_render_base as isize + dx) as usize;
                    let dgx = gx_r[row_r + xr] as f64 - gx_j[row_j + xj] as f64;
                    let dgy = gy_r[row_r + xr] as f64 - gy_j[row_j + xj] as f64;
                    ssd += dgx * dgx + dgy * dgy;
                }
            }

            table[(v + s) as usize * stride + (u + s) as usize] = ssd;
            sum_ssd += ssd;
            if ssd < best_ssd {
                best_ssd = ssd;
                best_u = u;
                best_v = v;
            }
        }
    }

    if best_u == -s || best_u == s || best_v == -s || best_v == s {
        return None;
    }

    for dj in -1..=1 {
        for di in -1..=1 {
            if di == 0 && dj == 0 {
                continue;
            }
            let n_val = table[(best_v + dj + s) as usize * stride + (best_u + di + s) as usize];
            if n_val < best_ssd {
                return None;
            }
        }
    }

    let mean_ssd = sum_ssd / ((stride * stride) as f64);
    if best_ssd > 0.70 * mean_ssd {
        return None;
    }

    let mut neighbor_diff_sum = 0.0f64;
    for dj in -1..=1 {
        for di in -1..=1 {
            if di == 0 && dj == 0 {
                continue;
            }
            neighbor_diff_sum += table[(best_v + dj + s) as usize * stride + (best_u + di + s) as usize] - best_ssd;
        }
    }
    if neighbor_diff_sum / 8.0 < 0.5 {
        return None;
    }

    let s_0_0 = best_ssd;
    let s_m1_0 = table[(best_v + s) as usize * stride + (best_u - 1 + s) as usize];
    let s_1_0 = table[(best_v + s) as usize * stride + (best_u + 1 + s) as usize];
    let s_0_m1 = table[(best_v - 1 + s) as usize * stride + (best_u + s) as usize];
    let s_0_1 = table[(best_v + 1 + s) as usize * stride + (best_u + s) as usize];
    let s_m1_m1 = table[(best_v - 1 + s) as usize * stride + (best_u - 1 + s) as usize];
    let s_1_m1 = table[(best_v - 1 + s) as usize * stride + (best_u + 1 + s) as usize];
    let s_m1_1 = table[(best_v + 1 + s) as usize * stride + (best_u - 1 + s) as usize];
    let s_1_1 = table[(best_v + 1 + s) as usize * stride + (best_u + 1 + s) as usize];

    let c_x = (s_1_0 - s_m1_0) * 0.5;
    let c_y = (s_0_1 - s_0_m1) * 0.5;
    let c_xx = (s_1_0 + s_m1_0 - 2.0 * s_0_0) * 0.5;
    let c_yy = (s_0_1 + s_0_m1 - 2.0 * s_0_0) * 0.5;
    let c_xy = (s_1_1 + s_m1_m1 - s_1_m1 - s_m1_1) * 0.25;

    let det = 4.0 * c_xx * c_yy - c_xy * c_xy;
    if det <= 0.0 || c_xx <= 0.0 || c_yy <= 0.0 {
        return None;
    }

    let du = (-2.0 * c_yy * c_x + c_xy * c_y) / det;
    let dv = (-2.0 * c_xx * c_y + c_xy * c_x) / det;
    if du.abs() > 1.0 || dv.abs() > 1.0 {
        return None;
    }

    Some((best_u as f64 + du, best_v as f64 + dv))
}

fn fit_similarity(patches: &[(f64, f64, f64, f64)]) -> Result<(f64, [f64; 2]), String> {
    let n = patches.len() as f64;
    let mut sum_x = 0.0f64;
    let mut sum_y = 0.0f64;
    let mut sum_r2 = 0.0f64;
    let mut sum_u = 0.0f64;
    let mut sum_v = 0.0f64;
    let mut sum_xu_yv = 0.0f64;

    for &(x, y, u, v) in patches {
        sum_x += x;
        sum_y += y;
        sum_r2 += x * x + y * y;
        sum_u += u;
        sum_v += v;
        sum_xu_yv += x * u + y * v;
    }

    let a00 = n;
    let a11 = n;
    let a02 = sum_x;
    let a12 = sum_y;
    let a22 = sum_r2;

    let d = a00 * a11 * a22 - a00 * a12 * a12 - a11 * a02 * a02;
    if d.abs() < 1e-9 {
        return Err("singular matrix fitting similarity".into());
    }

    let inv00 = (a11 * a22 - a12 * a12) / d;
    let inv01 = (a02 * a12) / d;
    let inv02 = (-a11 * a02) / d;
    let inv11 = (a00 * a22 - a02 * a02) / d;
    let inv12 = (-a00 * a12) / d;
    let inv22 = (a00 * a11) / d;

    let tx = inv00 * sum_u + inv01 * sum_v + inv02 * sum_xu_yv;
    let ty = inv01 * sum_u + inv11 * sum_v + inv12 * sum_xu_yv;
    let alpha = inv02 * sum_u + inv12 * sum_v + inv22 * sum_xu_yv;

    Ok((1.0 + alpha, [tx, ty]))
}

fn measure_vignetting(
    render: &Rgba8,
    jpeg: &Rgba8,
    scale: f64,
    translation: [f64; 2],
    max_radius: f64,
) -> ([f64; NUM_BINS], [usize; NUM_BINS]) {
    let (w, h) = (jpeg.width, jpeg.height);
    let cx = w as f64 / 2.0;
    let cy = h as f64 / 2.0;
    let bin_width = 1.0 / NUM_BINS as f64;

    let (sums, counts): (Vec<[f64; NUM_BINS]>, Vec<[usize; NUM_BINS]>) = (0..h)
        .into_par_iter()
        .map(|y| {
            let mut local_sums = [0.0f64; NUM_BINS];
            let mut local_counts = [0usize; NUM_BINS];

            let row_j = y * w;
            for x in 0..w {
                let pj = jpeg.data[row_j + x];
                if pj[0] == 0 || pj[0] == 255 || pj[1] == 0 || pj[1] == 255 || pj[2] == 0 || pj[2] == 255 {
                    continue;
                }

                let tx = x as f64 + 0.5 - cx;
                let ty = y as f64 + 0.5 - cy;
                let rx = cx + scale * tx + translation[0] - 0.5;
                let ry = cy + scale * ty + translation[1] - 0.5;

                if rx < 0.0 || rx >= (w - 1) as f64 || ry < 0.0 || ry >= (h - 1) as f64 {
                    continue;
                }

                let x0 = rx.floor() as usize;
                let y0 = ry.floor() as usize;
                let fx = (rx - x0 as f64) as f32;
                let fy = (ry - y0 as f64) as f32;

                let p00 = render.data[y0 * w + x0];
                let p10 = render.data[y0 * w + x0 + 1];
                let p01 = render.data[(y0 + 1) * w + x0];
                let p11 = render.data[(y0 + 1) * w + x0 + 1];

                let sample_channel = |c: usize| -> f32 {
                    let top = (1.0 - fx) * p00[c] as f32 + fx * p10[c] as f32;
                    let bottom = (1.0 - fx) * p01[c] as f32 + fx * p11[c] as f32;
                    (1.0 - fy) * top + fy * bottom
                };

                let rr = sample_channel(0);
                let gr = sample_channel(1);
                let br = sample_channel(2);

                if rr <= 0.5 || rr >= 254.5 || gr <= 0.5 || gr >= 254.5 || br <= 0.5 || br >= 254.5 {
                    continue;
                }

                let lr = crate::compare::lab([rr.round() as u8, gr.round() as u8, br.round() as u8])[0];
                let lj = crate::compare::lab([pj[0], pj[1], pj[2]])[0];
                let dl = lr - lj;

                let r = tx.hypot(ty) / max_radius;
                let b = (r / bin_width).floor().clamp(0.0, (NUM_BINS - 1) as f64) as usize;
                local_sums[b] += dl;
                local_counts[b] += 1;
            }

            (local_sums, local_counts)
        })
        .unzip();

    let mut total_sums = [0.0f64; NUM_BINS];
    let mut total_counts = [0usize; NUM_BINS];
    for s in sums {
        for b in 0..NUM_BINS {
            total_sums[b] += s[b];
        }
    }
    for c in counts {
        for b in 0..NUM_BINS {
            total_counts[b] += c[b];
        }
    }

    (total_sums, total_counts)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Generates a rich procedural textured image with sharp features, gradients and variation.
    fn synthetic_scene(w: usize, h: usize) -> Rgba8 {
        let mut img = Rgba8::new(w, h);
        for y in 0..h {
            let fy = y as f64 / h as f64 * 16.0;
            for x in 0..w {
                let fx = x as f64 / w as f64 * 24.0;
                let v = (fx.sin() * fy.cos()
                    + 0.5 * (3.0 * fx + 0.5).sin() * (3.0 * fy).cos()
                    + 0.25 * (7.0 * fx + 1.2).sin() * (7.0 * fy + 0.8).cos()
                    + 0.125 * (15.0 * fx).cos() * (15.0 * fy).sin())
                    * 0.5
                    + 0.5;
                let val = (v.clamp(0.0, 1.0) * 180.0 + 35.0).round() as u8;
                let r = val;
                let g = ((val as f64 * 0.95 + 10.0).clamp(0.0, 255.0)) as u8;
                let b = ((val as f64 * 1.05 - 5.0).clamp(0.0, 255.0)) as u8;
                img.data[y * w + x] = [r, g, b, 255];
            }
        }
        img
    }

    #[test]
    fn unwarped_same_image_measures_near_zero() {
        let img = synthetic_scene(600, 400);
        let m = check(&img, &img).expect("check identical image");
        assert!((m.scale - 1.0).abs() < 1e-4, "scale: {}", m.scale);
        assert!(m.translation[0].abs() < 0.05 && m.translation[1].abs() < 0.05, "translation: {:?}", m.translation);
        assert!(m.median_residual < 0.05, "median residual: {}", m.median_residual);
        assert!(m.p95_residual < 0.15, "p95 residual: {}", m.p95_residual);
        assert!(m.vignetting_diff.abs() < 0.1, "vignetting diff: {}", m.vignetting_diff);
        assert!(m.patches_used > 100, "patches used: {}", m.patches_used);
        for b in &m.bins {
            assert!(b.mean_residual < 0.1, "bin residual: {:?}", b);
            assert!(b.delta_l.abs() < 0.1, "bin delta_l: {:?}", b);
        }
    }

    #[test]
    fn shifted_and_scaled_copy_measures_right_scale_and_near_zero_residual() {
        let (w, h) = (600, 400);
        let base = synthetic_scene(w, h);
        let true_scale = 1.018;
        let true_tx = 2.4;
        let true_ty = -1.6;

        let cx = w as f64 / 2.0;
        let cy = h as f64 / 2.0;

        let mut transformed = Rgba8::filled(w, h, [128, 128, 128, 255]);
        for y in 0..h {
            for x in 0..w {
                let dx = x as f64 + 0.5 - cx;
                let dy = y as f64 + 0.5 - cy;
                let sx = cx + (dx - true_tx) / true_scale - 0.5;
                let sy = cy + (dy - true_ty) / true_scale - 0.5;

                if sx >= 0.0 && sx < (w - 1) as f64 && sy >= 0.0 && sy < (h - 1) as f64 {
                    let x0 = sx.floor() as usize;
                    let y0 = sy.floor() as usize;
                    let fx = (sx - x0 as f64) as f32;
                    let fy = (sy - y0 as f64) as f32;

                    let p00 = base.data[y0 * w + x0];
                    let p10 = base.data[y0 * w + x0 + 1];
                    let p01 = base.data[(y0 + 1) * w + x0];
                    let p11 = base.data[(y0 + 1) * w + x0 + 1];

                    let mut p = [0u8; 4];
                    for c in 0..3 {
                        let top = (1.0 - fx) * p00[c] as f32 + fx * p10[c] as f32;
                        let bottom = (1.0 - fx) * p01[c] as f32 + fx * p11[c] as f32;
                        p[c] = ((1.0 - fy) * top + fy * bottom).round() as u8;
                    }
                    p[3] = 255;
                    transformed.data[y * w + x] = p;
                }
            }
        }

        let m = check(&transformed, &base).expect("check shifted and scaled");
        assert!((m.scale - true_scale).abs() < 0.003, "scale {} vs true {}", m.scale, true_scale);
        assert!((m.translation[0] - true_tx).abs() < 0.25, "tx {} vs {}", m.translation[0], true_tx);
        assert!((m.translation[1] - true_ty).abs() < 0.25, "ty {} vs {}", m.translation[1], true_ty);
        assert!(m.median_residual < 0.25, "median residual: {}", m.median_residual);
        assert!(m.p95_residual < 0.50, "p95 residual: {}", m.p95_residual);
    }

    #[test]
    fn radially_distorted_and_vignetted_image_measures_distortion_and_falloff() {
        let (w, h) = (600, 400);
        let base = synthetic_scene(w, h);
        let cx = w as f64 / 2.0;
        let cy = h as f64 / 2.0;
        let max_r = cx.hypot(cy);

        let k1 = 0.04;
        let falloff_factor = 0.35;

        let mut distorted = Rgba8::filled(w, h, [128, 128, 128, 255]);
        for y in 0..h {
            for x in 0..w {
                let dx = x as f64 + 0.5 - cx;
                let dy = y as f64 + 0.5 - cy;
                let r_norm = dx.hypot(dy) / max_r;

                let s = 1.0 + k1 * r_norm * r_norm;
                let sx = cx + dx * s - 0.5;
                let sy = cy + dy * s - 0.5;

                if sx >= 0.0 && sx < (w - 1) as f64 && sy >= 0.0 && sy < (h - 1) as f64 {
                    let x0 = sx.floor() as usize;
                    let y0 = sy.floor() as usize;
                    let fx = (sx - x0 as f64) as f32;
                    let fy = (sy - y0 as f64) as f32;

                    let p00 = base.data[y0 * w + x0];
                    let p10 = base.data[y0 * w + x0 + 1];
                    let p01 = base.data[(y0 + 1) * w + x0];
                    let p11 = base.data[(y0 + 1) * w + x0 + 1];

                    let vignetting_gain = 1.0 - falloff_factor * r_norm * r_norm;

                    let mut p = [0u8; 4];
                    for c in 0..3 {
                        let top = (1.0 - fx) * p00[c] as f32 + fx * p10[c] as f32;
                        let bottom = (1.0 - fx) * p01[c] as f32 + fx * p11[c] as f32;
                        let val = ((1.0 - fy) * top + fy * bottom) as f64 * vignetting_gain;
                        p[c] = val.clamp(0.0, 255.0).round() as u8;
                    }
                    p[3] = 255;
                    distorted.data[y * w + x] = p;
                }
            }
        }

        let m = check(&distorted, &base).expect("check distorted");
        assert!(m.median_residual > 0.3, "median residual should reflect distortion: {}", m.median_residual);
        assert!(
            m.bins.last().unwrap().mean_residual > m.bins.first().unwrap().mean_residual,
            "residuals must grow with radius: centre {} vs corner {}",
            m.bins.first().unwrap().mean_residual,
            m.bins.last().unwrap().mean_residual
        );
        assert!(m.vignetting_diff < -3.0, "vignetting difference should be negative: {}", m.vignetting_diff);
    }
}

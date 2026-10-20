//! Sony's own lens corrections: the distortion, vignetting and lateral
//! chromatic aberration tables the camera writes into the raw IFD of every
//! ARW (`0x7037`, `0x7032`, `0x7035`), whatever its lens-compensation
//! settings, for the lens on it and the focal length, aperture and focus
//! distance it was shot at. Read for the ILCE-7M3 only (other cameras are
//! on the roadmap's "Later" list), as Lightroom's profile correction is
//! put to the pipeline: one [`EmbeddedLens`], found from the raw alone.
//!
//! Distortion fitting and zoom are lifted from Lightcraft (MIT OR Apache-2.0)
//! at `701f78e`, `vendor/lightcraft/crates/raw/src/vendor/arw_lens.rs`.
//! Lateral chromatic aberration per-plane scaling and radial vignetting gain
//! are ported from darktable (GPL-3.0) at `3ecbc24`, `src/iop/lens.cc`
//! (`_init_coeffs_md_v1` / `_init_coeffs_md_v2`) and `src/common/exif.cc`.

use lightcraft_develop::EmbeddedLens;
use lightcraft_raw::{Opcode, Rect};
use lightcraft_tiff::{Ifd, Tiff, Value};

const VIGNETTING: u16 = 0x7032;
const CA: u16 = 0x7035;
const DISTORTION: u16 = 0x7037;
const SAMPLES: usize = 1024;
const MAX_ERROR: f64 = 0.0005;
const MAX_VIG_ERROR: f64 = 0.01;

/// Where knot `i` of `n` knots (0..16, n = 16) sits in radius [0, 1] relative to the default crop's half diagonal.
///
/// Lightcraft puts knot `i` at `i / (n - 1)` (validated against Sony's exports on the A7R IVA).
/// darktable puts knot `i` at `(i + 0.5) / (n - 1)`.
///
/// Kept here in this single function so M5's camera-JPEG comparison can switch knot positions in one place.
fn knot_radius(i: usize, n: usize) -> f64 {
    i as f64 / (n - 1) as f64
}

/// Interpolate table values piecewise-linearly across the knots at radius `r` (0.0..=1.0).
fn interpolate_knots(knots: &[i16], r: f64) -> f64 {
    let n = knots.len();
    let r = r.clamp(0.0, 1.0);
    let mut i = 0;
    while i + 2 < n && knot_radius(i + 1, n) <= r {
        i += 1;
    }
    let r0 = knot_radius(i, n);
    let r1 = knot_radius(i + 1, n);
    let frac = if (r1 - r0).abs() > 1e-12 {
        ((r - r0) / (r1 - r0)).clamp(0.0, 1.0)
    } else {
        0.0
    };
    knots[i] as f64 + (knots[i + 1] as f64 - knots[i] as f64) * frac
}

/// Small fixed-size least-squares system; partial pivoting, no input-sized allocations.
fn solve<const N: usize>(mut a: [[f64; N]; N], mut b: [f64; N]) -> Option<[f64; N]> {
    for col in 0..N {
        let pivot = (col..N).max_by(|&i, &j| a[i][col].abs().total_cmp(&a[j][col].abs()))?;
        if !a[pivot][col].is_finite() || a[pivot][col].abs() < 1e-12 {
            return None;
        }
        a.swap(col, pivot);
        b.swap(col, pivot);
        let pivot_row = a[col];
        for row in col + 1..N {
            let factor = a[row][col] / pivot_row[col];
            for (dst, src) in a[row][col..N].iter_mut().zip(&pivot_row[col..N]) {
                *dst -= factor * *src;
            }
            b[row] -= factor * b[col];
        }
    }
    let mut x = [0.0; N];
    for i in (0..N).rev() {
        x[i] = (b[i] - (i + 1..N).map(|j| a[i][j] * x[j]).sum::<f64>()) / a[i][i];
    }
    x.iter().all(|v| v.is_finite()).then_some(x)
}

/// The raw image IFD: the SubIFD carrying CFA sensor data, or IFD0 when testing a synthesized IFD.
fn raw_ifd(tiff: &Tiff) -> Option<&Ifd> {
    use lightcraft_tiff::tags as t;
    tiff.all_ifds()
        .into_iter()
        .filter(|i| i.u16(t::PHOTOMETRIC) == Some(t::photometric::CFA) || i.contains(0x7010))
        .max_by_key(|i| i.u64(t::IMAGE_WIDTH).unwrap_or(0).saturating_mul(i.u64(t::IMAGE_LENGTH).unwrap_or(0)))
        .or_else(|| tiff.ifds.first())
}

/// The raw IFD's DNG-style `DefaultCropOrigin` / `DefaultCropSize`, clipped to the `w × h` frame.
fn dng_default_crop(ifd: &Ifd, w: usize, h: usize) -> Option<Rect> {
    use lightcraft_tiff::tags as t;
    if let (Some([x, y]), Some([cw, ch])) = (ifd.u64s(t::DEFAULT_CROP_ORIGIN).as_deref(), ifd.u64s(t::DEFAULT_CROP_SIZE).as_deref())
        && *cw > 0
        && *ch > 0
    {
        Some(Rect::new(*x as usize, *y as usize, *cw as usize, *ch as usize).clipped(w, h))
    } else {
        None
    }
}

/// Build the DNG `WarpRectilinear` opcode from the distortion and lateral chromatic aberration tables.
fn build_warp(ifd: &Ifd, active: Rect, crop: Rect) -> Option<Opcode> {
    let Value::SShort(values) = &ifd.get(DISTORTION)?.value else { return None };
    let [16, dist_table @ ..] = values.as_slice() else { return None };
    let dist_table: &[i16; 16] = dist_table.try_into().ok()?;
    if dist_table.iter().all(|v| *v == 0) || dist_table.iter().any(|v| !(-4096..=4096).contains(v)) {
        return None;
    }

    let ca = match ifd.get(CA) {
        Some(entry) => match &entry.value {
            Value::SShort(values) => {
                let [32, ca_table @ ..] = values.as_slice() else { return None };
                let (r, b) = ca_table.split_at(16);
                let r: &[i16; 16] = r.try_into().ok()?;
                let b: &[i16; 16] = b.try_into().ok()?;
                if r.iter().chain(b.iter()).any(|v| !(-4096..=4096).contains(v)) {
                    return None;
                }
                Some((r, b))
            }
            _ => return None,
        },
        None => None,
    };

    let scale_g = |r: f64| {
        1.0 + interpolate_knots(dist_table, r) / 16384.0
    };
    let scale_r = |r: f64| {
        let g = scale_g(r);
        match ca {
            Some((r_knots, _)) => g * (1.0 + interpolate_knots(r_knots, r) / 2097152.0),
            None => g,
        }
    };
    let scale_b = |r: f64| {
        let g = scale_g(r);
        match ca {
            Some((_, b_knots)) => g * (1.0 + interpolate_knots(b_knots, r) / 2097152.0),
            None => g,
        }
    };

    let (cw, ch) = (crop.width as f64 - 1.0, crop.height as f64 - 1.0);
    let diagonal = cw.hypot(ch);
    let nearest = cw.min(ch) / diagonal;
    let max_scale = (0..16)
        .map(|i| knot_radius(i, 16))
        .filter(|&r| r >= nearest)
        .map(scale_g)
        .fold(scale_g(nearest), f64::max);
    let zoom = 1.0 / max_scale;

    let basis = |r: f64| {
        let r2 = r * r;
        [r, r * r2, r * r2 * r2, r * r2 * r2 * r2]
    };

    let (ax, ay) = (active.width as f64 - 1.0, active.height as f64 - 1.0);
    let (cx, cy) = (crop.x as f64 + cw / 2.0, crop.y as f64 + ch / 2.0);
    let farthest = cx.max(ax - cx).hypot(cy.max(ay - cy));
    let ratio2: f64 = (2.0 * farthest / diagonal).powi(2);

    let fit_plane = |scale_fn: &dyn Fn(f64) -> f64| -> Option<[f64; 6]> {
        let mut matrix = [[0.0; 4]; 4];
        let mut rhs = [0.0; 4];
        let mut previous = -1.0;
        for j in 0..=SAMPLES {
            let r = j as f64 / SAMPLES as f64;
            let source = r * scale_fn(r) * zoom;
            if source <= previous {
                return None;
            }
            previous = source;
            let terms = basis(r);
            for (i, &a) in terms.iter().enumerate() {
                rhs[i] += a * source;
                for (k, &b) in terms.iter().enumerate() {
                    matrix[i][k] += a * b;
                }
            }
        }
        let mut k = solve(matrix, rhs)?;
        for j in 0..=SAMPLES {
            let r = j as f64 / SAMPLES as f64;
            let fit = basis(r).iter().zip(k).map(|(a, b)| a * b).sum::<f64>();
            let r2 = r * r;
            let derivative = k[0] + r2 * (3.0 * k[1] + r2 * (5.0 * k[2] + r2 * 7.0 * k[3]));
            if (fit - r * scale_fn(r) * zoom).abs() > MAX_ERROR || derivative <= 0.0 {
                return None;
            }
        }
        for (i, coefficient) in k.iter_mut().enumerate() {
            *coefficient *= ratio2.powi(i as i32);
        }
        Some([k[0], k[1], k[2], k[3], 0.0, 0.0])
    };

    let plane_r = fit_plane(&scale_r)?;
    let plane_g = fit_plane(&scale_g)?;
    let plane_b = fit_plane(&scale_b)?;

    Some(Opcode::WarpRectilinear {
        planes: vec![plane_r, plane_g, plane_b],
        center: [cx / ax, cy / ay],
    })
}

/// Build the DNG `FixVignetteRadial` opcode from the vignetting table.
fn build_vignette(ifd: &Ifd, active: Rect, crop: Rect) -> Option<Opcode> {
    let Value::SShort(values) = &ifd.get(VIGNETTING)?.value else { return None };
    let [16, table @ ..] = values.as_slice() else { return None };
    let table: &[i16; 16] = table.try_into().ok()?;
    if table.iter().all(|v| *v == 0) || table.iter().any(|v| !(0..=16384).contains(v)) {
        return None;
    }
    if table.windows(2).any(|w| w[1] < w[0]) {
        return None;
    }

    let (cw, ch) = (crop.width as f64 - 1.0, crop.height as f64 - 1.0);
    let diagonal = cw.hypot(ch);
    let (ax, ay) = (active.width as f64 - 1.0, active.height as f64 - 1.0);
    let (cx, cy) = (crop.x as f64 + cw / 2.0, crop.y as f64 + ch / 2.0);
    let farthest = cx.max(ax - cx).hypot(cy.max(ay - cy));
    let ratio2: f64 = (2.0 * farthest / diagonal).powi(2);

    let gain_at = |r: f64| {
        let val = interpolate_knots(table, r) / 8192.0;
        let stops = (val - 1.0).exp2() - 0.5;
        stops.exp2()
    };

    let basis = |r: f64| {
        let r2 = r * r;
        let r4 = r2 * r2;
        let r6 = r4 * r2;
        let r8 = r4 * r4;
        let r10 = r6 * r4;
        [r2, r4, r6, r8, r10]
    };

    let mut matrix = [[0.0; 5]; 5];
    let mut rhs = [0.0; 5];
    let mut previous_gain = -1.0;
    for j in 0..=SAMPLES {
        let r = j as f64 / SAMPLES as f64;
        let g = gain_at(r);
        if g < previous_gain - 1e-9 {
            return None;
        }
        previous_gain = g;
        let target = g - 1.0;
        let terms = basis(r);
        for (i, &a) in terms.iter().enumerate() {
            rhs[i] += a * target;
            for (k, &b) in terms.iter().enumerate() {
                matrix[i][k] += a * b;
            }
        }
    }
    let mut k = solve(matrix, rhs)?;
    for j in 0..=SAMPLES {
        let r = j as f64 / SAMPLES as f64;
        let fit = 1.0 + basis(r).iter().zip(k).map(|(a, b)| a * b).sum::<f64>();
        let target = gain_at(r);
        if (fit - target).abs() > MAX_VIG_ERROR {
            return None;
        }
    }
    for (i, coefficient) in k.iter_mut().enumerate() {
        *coefficient *= ratio2.powi((i + 1) as i32);
    }
    Some(Opcode::FixVignetteRadial { k, center: [cx / ax, cy / ay] })
}

/// Sony's corrections for the raw `bytes`, whose header is `raw`,
/// re-expressed for its default-cropped, EXIF-oriented image as
/// [`crate::raw::embedded_lens`] does. `None` for another camera, a raw
/// without the tables, or tables that can't be trusted.
pub fn corrections(bytes: &[u8], raw: &lightcraft_raw::RawInfo) -> Option<EmbeddedLens> {
    let (warp, vignette) = opcodes(bytes, raw)?;
    // The opcodes are centred and normalised on the DNG default crop, in active-area coordinates, as Lightcraft's
    // are; the image keeps the decoder's own crop, which `embedded_lens` re-expresses them for.
    let mut raw_clone = raw.clone();
    raw_clone.opcodes.list3.clear();
    if let Some(w) = warp {
        raw_clone.opcodes.list3.push(w);
    }
    if let Some(v) = vignette {
        raw_clone.opcodes.list3.push(v);
    }
    crate::raw::embedded_lens(&raw_clone)
}

/// The distortion Sony's tables describe, as the camera applies it to its JPEG when its Distortion Comp. is on:
/// what the camera match pairs the sensor with that JPEG through ([`crate::camera_preview::proxies`]). `None` as
/// for [`corrections`], or without a distortion table.
pub(crate) fn geometry(bytes: &[u8], raw: &lightcraft_raw::RawInfo) -> Option<crate::camera_preview::LensGeometry> {
    let (warp, _) = opcodes(bytes, raw)?;
    crate::camera_preview::lens_geometry(&[warp?])
}

/// Sony's tables as DNG opcodes, `(WarpRectilinear, FixVignetteRadial)`: at least one of them.
fn opcodes(bytes: &[u8], raw: &lightcraft_raw::RawInfo) -> Option<(Option<Opcode>, Option<Opcode>)> {
    let tiff = Tiff::parse(bytes).ok()?;
    let ifd0 = tiff.ifds.first()?;
    let model = raw.metadata.model.clone().or_else(|| ifd0.string(lightcraft_tiff::tags::MODEL))?;
    if model != "ILCE-7M3" {
        return None;
    }
    let ifd = raw_ifd(&tiff)?;
    let active = raw.active_area;
    let crop = dng_default_crop(ifd, active.width, active.height).unwrap_or(raw.crop);
    let crop = crop.clipped(active.width, active.height);
    if active.width < 2 || active.height < 2 || crop.width < 2 || crop.height < 2 {
        return None;
    }
    if (crop.width as f64 / crop.height as f64 - 1.5).abs() > 0.005 {
        return None;
    }

    let warp = build_warp(ifd, active, crop);
    let vignette = build_vignette(ifd, active, crop);
    (warp.is_some() || vignette.is_some()).then_some((warp, vignette))
}

#[cfg(test)]
mod tests {
    use super::*;
    use lightcraft_geom::Point;
    use lightcraft_tiff::{ByteOrder, IfdBuilder, ImageData, TiffWriter, tags as t};

    const FE85_DIST: [i16; 16] = [-3, -1, 2, 4, 9, 13, 19, 23, 28, 31, 31, 28, 21, 9, -9, -35];
    const FE85_VIG: [i16; 16] = [0, 64, 432, 1024, 1792, 2624, 3520, 4480, 5456, 6448, 7440, 8432, 9408, 10368, 11312, 12224];
    const FE85_CA_R: [i16; 16] = [128, 0, -128, -128, -128, -128, -128, -128, -128, -128, -128, -128, -128, -128, -128, -128];
    const FE85_CA_B: [i16; 16] = [-128, -256, -256, -384, -384, -384, -384, -384, -384, -384, -256, -256, -256, -256, -256, -256];

    fn build_raw(
        model: &str,
        (w, h): (usize, usize),
        (crop_w, crop_h): (usize, usize),
        dist: Option<Vec<i16>>,
        vig: Option<Vec<i16>>,
        ca: Option<Vec<i16>>,
    ) -> (Vec<u8>, lightcraft_raw::RawInfo) {
        let mut raw = IfdBuilder::new();
        raw.set(t::MAKE, Value::Ascii("SONY".into()));
        raw.set(t::MODEL, Value::Ascii(model.into()));
        raw.set(t::IMAGE_WIDTH, Value::Long(vec![w as u32]));
        raw.set(t::IMAGE_LENGTH, Value::Long(vec![h as u32]));
        raw.set(t::BITS_PER_SAMPLE, Value::Short(vec![16]));
        raw.set(t::SAMPLES_PER_PIXEL, Value::Short(vec![1]));
        raw.set(t::PHOTOMETRIC, Value::Short(vec![t::photometric::CFA]));
        raw.set(t::COMPRESSION, Value::Short(vec![1]));
        raw.set(t::WHITE_LEVEL, Value::Long(vec![16383]));
        raw.set(0x74c7, Value::Long(vec![0, 0]));
        raw.set(t::DEFAULT_CROP_ORIGIN, Value::Long(vec![0, 0]));
        raw.set(t::DEFAULT_CROP_SIZE, Value::Long(vec![crop_w as u32, crop_h as u32]));
        raw.set(0x74c8, Value::Long(vec![crop_w as u32, crop_h as u32]));
        if let Some(dist) = dist {
            raw.set(DISTORTION, Value::SShort(dist));
        }
        if let Some(vig) = vig {
            raw.set(VIGNETTING, Value::SShort(vig));
        }
        if let Some(ca) = ca {
            raw.set(CA, Value::SShort(ca));
        }
        raw.set_image(ImageData::Strips { rows_per_strip: h as u32, strips: vec![vec![0; w * h * 2]] });
        let bytes = TiffWriter { order: ByteOrder::Little, ..Default::default() }.write(&[raw]).unwrap();
        let header = lightcraft_raw::decode(&bytes).unwrap();
        let info = header.info();
        (bytes, info)
    }

    #[test]
    fn synthetic_tables_reproduce_knots_within_tolerance() {
        let dist = std::iter::once(16).chain(FE85_DIST).collect::<Vec<_>>();
        let vig = std::iter::once(16).chain(FE85_VIG).collect::<Vec<_>>();
        let ca = std::iter::once(32).chain(FE85_CA_R).chain(FE85_CA_B).collect::<Vec<_>>();
        let (bytes, info) = build_raw("ILCE-7M3", (6000, 4000), (6000, 4000), Some(dist), Some(vig), Some(ca));
        let lens = corrections(&bytes, &info).expect("corrections found");
        let warp = lens.warp.expect("warp present");
        let vig_lens = lens.vignette.expect("vignette present");

        assert_ne!(warp.planes[0], warp.planes[1], "red plane differs from green");
        assert_ne!(warp.planes[2], warp.planes[1], "blue plane differs from green");

        let (cw, ch): (f64, f64) = (5999.0, 3999.0);
        let diagonal = cw.hypot(ch);
        let nearest = cw.min(ch) / diagonal;
        let scale_g = |r: f64| 1.0 + interpolate_knots(&FE85_DIST, r) / 16384.0;
        let max_scale = (0..16)
            .map(|i| knot_radius(i, 16))
            .filter(|&r| r >= nearest)
            .map(scale_g)
            .fold(scale_g(nearest), f64::max);
        let zoom = 1.0 / max_scale;

        let center = Point::new(warp.center.x * 6000.0, warp.center.y * 4000.0);
        for i in 0..16 {
            let r = knot_radius(i, 16);
            let pt = Point::new(center.x + r * (diagonal / 2.0), center.y);
            let target_g = r * scale_g(r) * zoom;
            let target_r = target_g * (1.0 + FE85_CA_R[i] as f64 / 2097152.0);
            let target_b = target_g * (1.0 + FE85_CA_B[i] as f64 / 2097152.0);

            let s_r = lightcraft_pipeline::optics::embedded_warp(&warp, pt, 0, 6000.0, 4000.0);
            let s_g = lightcraft_pipeline::optics::embedded_warp(&warp, pt, 1, 6000.0, 4000.0);
            let s_b = lightcraft_pipeline::optics::embedded_warp(&warp, pt, 2, 6000.0, 4000.0);

            let eval_r = (s_r.x - center.x) / (diagonal / 2.0);
            let eval_g = (s_g.x - center.x) / (diagonal / 2.0);
            let eval_b = (s_b.x - center.x) / (diagonal / 2.0);

            assert!((eval_g - target_g).abs() < MAX_ERROR, "green knot {i}: {eval_g} vs {target_g}");
            assert!((eval_r - target_r).abs() < MAX_ERROR, "red knot {i}: {eval_r} vs {target_r}");
            assert!((eval_b - target_b).abs() < MAX_ERROR, "blue knot {i}: {eval_b} vs {target_b}");
            if i > 0 {
                let expected_r_diff = target_g * (FE85_CA_R[i] as f64 / 2097152.0);
                let actual_r_diff = eval_r - eval_g;
                assert!((actual_r_diff - expected_r_diff).abs() < 2.0 * MAX_ERROR);
            }
        }

        let corner_pt = Point::new(6000.0, 4000.0);
        let eval_vig_gain = lightcraft_pipeline::optics::embedded_vignette_gain(&vig_lens, corner_pt, 6000.0, 4000.0);
        let target_vig_val = FE85_VIG[15] as f64 / 8192.0;
        let target_vig_gain = ((target_vig_val - 1.0).exp2() - 0.5).exp2();
        assert!(
            (eval_vig_gain - target_vig_gain).abs() < MAX_VIG_ERROR,
            "corner vignette gain: {eval_vig_gain} vs {target_vig_gain}"
        );
    }

    #[test]
    fn refuses_unvalidated_model_wrong_length_all_zero_and_non_3_2_crops() {
        let dist = std::iter::once(16).chain(FE85_DIST).collect::<Vec<_>>();
        let vig = std::iter::once(16).chain(FE85_VIG).collect::<Vec<_>>();
        let ca = std::iter::once(32).chain(FE85_CA_R).chain(FE85_CA_B).collect::<Vec<_>>();

        // Wrong camera model: both are None
        let (bytes, info) = build_raw("ILCE-7RM4", (6000, 4000), (6000, 4000), Some(dist.clone()), Some(vig.clone()), Some(ca.clone()));
        assert!(corrections(&bytes, &info).is_none());

        // Non-3:2 crop (16:9): both are None
        let (bytes, info) = build_raw("ILCE-7M3", (6000, 4000), (6000, 3375), Some(dist.clone()), Some(vig.clone()), Some(ca.clone()));
        assert!(corrections(&bytes, &info).is_none());

        // Wrong length distortion table (15 instead of 16 knots): warp is None, vignette is Some
        let bad_dist = std::iter::once(15).chain(FE85_DIST[..15].iter().copied()).collect::<Vec<_>>();
        let (bytes, info) = build_raw("ILCE-7M3", (6000, 4000), (6000, 4000), Some(bad_dist), Some(vig.clone()), Some(ca.clone()));
        let lens = corrections(&bytes, &info).expect("vignette still found");
        assert!(lens.warp.is_none(), "warp should be None for bad length distortion");
        assert!(lens.vignette.is_some(), "vignette should still be Some");

        // Wrong length vignette table (10 instead of 16 knots): vignette is None, warp is Some
        let bad_vig = std::iter::once(10).chain(FE85_VIG[..10].iter().copied()).collect::<Vec<_>>();
        let (bytes, info) = build_raw("ILCE-7M3", (6000, 4000), (6000, 4000), Some(dist.clone()), Some(bad_vig), Some(ca.clone()));
        let lens = corrections(&bytes, &info).expect("warp still found");
        assert!(lens.warp.is_some(), "warp should still be Some");
        assert!(lens.vignette.is_none(), "vignette should be None for bad length vignetting");

        // All zero distortion table: warp is None, vignette is Some
        let zero_dist = std::iter::once(16).chain(std::iter::repeat_n(0, 16)).collect::<Vec<_>>();
        let (bytes, info) = build_raw("ILCE-7M3", (6000, 4000), (6000, 4000), Some(zero_dist), Some(vig.clone()), Some(ca.clone()));
        let lens = corrections(&bytes, &info).expect("vignette still found");
        assert!(lens.warp.is_none(), "warp should be None for all-zero distortion");
        assert!(lens.vignette.is_some(), "vignette should still be Some");
        assert!(geometry(&bytes, &info).is_none(), "no geometry without a warp");

        // All zero vignette table: vignette is None, warp is Some
        let zero_vig = std::iter::once(16).chain(std::iter::repeat_n(0, 16)).collect::<Vec<_>>();
        let (bytes, info) = build_raw("ILCE-7M3", (6000, 4000), (6000, 4000), Some(dist.clone()), Some(zero_vig), Some(ca.clone()));
        let lens = corrections(&bytes, &info).expect("warp still found");
        assert!(lens.warp.is_some(), "warp should still be Some");
        assert!(lens.vignette.is_none(), "vignette should be None for all-zero vignetting");
        // the camera match's geometry is the warp's green plane, centred as the warp is
        let geometry = geometry(&bytes, &info).expect("a geometry with a distortion table");
        let Some((Some(Opcode::WarpRectilinear { planes, center }), _)) = opcodes(&bytes, &info) else { panic!("a warp") };
        assert_eq!(Some(geometry), crate::camera_preview::lens_geometry(&[Opcode::WarpRectilinear { planes: vec![planes[1]], center }]));

        // Both all zero: corrections returns None
        let zero_dist = std::iter::once(16).chain(std::iter::repeat_n(0, 16)).collect::<Vec<_>>();
        let zero_vig = std::iter::once(16).chain(std::iter::repeat_n(0, 16)).collect::<Vec<_>>();
        let (bytes, info) = build_raw("ILCE-7M3", (6000, 4000), (6000, 4000), Some(zero_dist), Some(zero_vig), None);
        assert!(corrections(&bytes, &info).is_none());
    }

    #[test]
    fn corpus_a7m3_has_valid_sony_corrections() {
        for (name, bytes) in crate::raw::tests::a7m3_corpus() {
            let header = lightcraft_raw::decode(&bytes).unwrap();
            let info = header.info();
            let lens = corrections(&bytes, &info).unwrap_or_else(|| panic!("{name}: corrections"));
            let warp = lens.warp.unwrap_or_else(|| panic!("{name}: warp"));
            let vig = lens.vignette.unwrap_or_else(|| panic!("{name}: vignette"));

            assert_ne!(warp.planes[0], warp.planes[1], "{name}: red differs from green");
            assert_ne!(warp.planes[2], warp.planes[1], "{name}: blue differs from green");

            let (w, h) = (info.crop.width as f64, info.crop.height as f64);
            let corner_pt = Point::new(w, h);
            let center = Point::new(warp.center.x * w, warp.center.y * h);
            let s_g = lightcraft_pipeline::optics::embedded_warp(&warp, corner_pt, 1, w, h);
            let s_r = lightcraft_pipeline::optics::embedded_warp(&warp, corner_pt, 0, w, h);
            let s_b = lightcraft_pipeline::optics::embedded_warp(&warp, corner_pt, 2, w, h);

            let r_out = corner_pt.dist(center);
            let r_g = s_g.dist(center);
            let r_r = s_r.dist(center);
            let r_b = s_b.dist(center);

            let corner_disp = r_out - r_g;
            let ca_r = r_r - r_g;
            let ca_b = r_b - r_g;

            let vig_gain = lightcraft_pipeline::optics::embedded_vignette_gain(&vig, corner_pt, w, h);
            let vig_stops = vig_gain.log2();

            println!("{name}: corner disp = {corner_disp:.2} px, vig = {vig_stops:.3} stops ({vig_gain:.2}x), CA: R {ca_r:+.2} px, B {ca_b:+.2} px");

            assert!(corner_disp.abs() < 50.0, "{name}: plausible corner displacement");
            assert!(vig_stops > 0.1 && vig_stops < 2.5, "{name}: plausible vignetting stops {vig_stops}");
            assert!(ca_r.abs() < 5.0 && ca_b.abs() < 5.0, "{name}: plausible CA sizes");

            let found = crate::lens_profiles::for_raw(&info, &bytes, crate::lens_profiles::Source::Sony, crate::sidecar::PROCESS_VERSION)
                .unwrap_or_else(|| panic!("{name}: for_raw Sony"));
            assert_eq!(found.source, crate::lens_profiles::Source::Sony, "{name}: Source::Sony");
        }
    }
}

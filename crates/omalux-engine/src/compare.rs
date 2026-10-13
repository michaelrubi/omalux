//! How close a render is to the camera's own JPEG: CIEDE2000 per pixel,
//! overall and on the pixels where the errors show (saturated colours,
//! skin, sky), as "Quality bar" in DESIGN.md measures the camera match.
//!
//! CIEDE2000 as G. Sharma, W. Wu and E. N. Dalal give it ("The CIEDE2000
//! color-difference formula: implementation notes, supplementary test
//! data, and mathematical observations", 2005), checked against their test
//! data. Both images are sRGB; Lab is taken relative to D65, sRGB's white.

use lightcraft_raster::Rgba8;

/// CIE L*a*b*.
pub type Lab = [f64; 3];

/// An sRGB pixel in L*a*b* (D65).
pub fn lab(rgb: [u8; 3]) -> Lab {
    let lin = rgb.map(|c| f64::from(lightcraft_color::transfer::srgb_to_linear(f32::from(c) / 255.0)));
    let x = 0.4124564 * lin[0] + 0.3575761 * lin[1] + 0.1804375 * lin[2];
    let y = 0.2126729 * lin[0] + 0.7151522 * lin[1] + 0.0721750 * lin[2];
    let z = 0.0193339 * lin[0] + 0.1191920 * lin[1] + 0.9503041 * lin[2];
    let f = |t: f64| if t > (6.0f64 / 29.0).powi(3) { t.cbrt() } else { t / (3.0 * (6.0f64 / 29.0).powi(2)) + 4.0 / 29.0 };
    let (fx, fy, fz) = (f(x / 0.95047), f(y), f(z / 1.08883));
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

/// The CIEDE2000 colour difference between two colours.
pub fn ciede2000(lab1: Lab, lab2: Lab) -> f64 {
    let [l1, a1, b1] = lab1;
    let [l2, a2, b2] = lab2;
    let c_bar = (a1.hypot(b1) + a2.hypot(b2)) / 2.0;
    let g = 0.5 * (1.0 - (c_bar.powi(7) / (c_bar.powi(7) + 25f64.powi(7))).sqrt());
    let (a1p, a2p) = ((1.0 + g) * a1, (1.0 + g) * a2);
    let (c1p, c2p) = (a1p.hypot(b1), a2p.hypot(b2));
    let hue = |b: f64, a: f64| if b == 0.0 && a == 0.0 { 0.0 } else { b.atan2(a).to_degrees().rem_euclid(360.0) };
    let (h1p, h2p) = (hue(b1, a1p), hue(b2, a2p));
    let dl = l2 - l1;
    let dc = c2p - c1p;
    let dh = if c1p * c2p == 0.0 {
        0.0
    } else if (h2p - h1p).abs() <= 180.0 {
        h2p - h1p
    } else if h2p - h1p > 180.0 {
        h2p - h1p - 360.0
    } else {
        h2p - h1p + 360.0
    };
    let dhh = 2.0 * (c1p * c2p).sqrt() * (dh / 2.0).to_radians().sin();
    let l_bar = (l1 + l2) / 2.0;
    let c_bar_p = (c1p + c2p) / 2.0;
    let h_bar = if c1p * c2p == 0.0 {
        h1p + h2p
    } else if (h1p - h2p).abs() <= 180.0 {
        (h1p + h2p) / 2.0
    } else if h1p + h2p < 360.0 {
        (h1p + h2p + 360.0) / 2.0
    } else {
        (h1p + h2p - 360.0) / 2.0
    };
    let t = 1.0 - 0.17 * (h_bar - 30.0).to_radians().cos() + 0.24 * (2.0 * h_bar).to_radians().cos()
        + 0.32 * (3.0 * h_bar + 6.0).to_radians().cos()
        - 0.20 * (4.0 * h_bar - 63.0).to_radians().cos();
    let d_theta = 30.0 * (-((h_bar - 275.0) / 25.0).powi(2)).exp();
    let rc = 2.0 * (c_bar_p.powi(7) / (c_bar_p.powi(7) + 25f64.powi(7))).sqrt();
    let sl = 1.0 + 0.015 * (l_bar - 50.0).powi(2) / (20.0 + (l_bar - 50.0).powi(2)).sqrt();
    let sc = 1.0 + 0.045 * c_bar_p;
    let sh = 1.0 + 0.015 * c_bar_p * t;
    let rt = -(2.0 * d_theta).to_radians().sin() * rc;
    ((dl / sl).powi(2) + (dc / sc).powi(2) + (dhh / sh).powi(2) + rt * (dc / sc) * (dhh / sh)).sqrt()
}

/// The pixels a difference is summarised over, chosen by the reference's
/// colour (the camera's JPEG).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Region {
    All,
    /// Chroma above 50: where a camera's colour rendering differs most.
    Saturated,
    /// Skin's hues and chroma (hue 20° to 70°, chroma 10 to 45, L* 25 to 90).
    Skin,
    /// Blue sky (hue 225° to 285°, chroma 10 and up, L* 40 and up).
    Sky,
}

impl Region {
    pub const ALL: [Region; 4] = [Region::All, Region::Saturated, Region::Skin, Region::Sky];

    pub fn label(self) -> &'static str {
        match self {
            Region::All => "all",
            Region::Saturated => "saturated",
            Region::Skin => "skin",
            Region::Sky => "sky",
        }
    }

    fn holds(self, [l, a, b]: Lab) -> bool {
        let chroma = a.hypot(b);
        let hue = b.atan2(a).to_degrees().rem_euclid(360.0);
        match self {
            Region::All => true,
            Region::Saturated => chroma > 50.0,
            Region::Skin => (20.0..=70.0).contains(&hue) && (10.0..=45.0).contains(&chroma) && (25.0..=90.0).contains(&l),
            Region::Sky => (225.0..=285.0).contains(&hue) && chroma >= 10.0 && l >= 40.0,
        }
    }
}

/// ΔE2000 over one region.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Summary {
    /// Share of the frame's pixels in the region.
    pub share: f64,
    pub mean: f64,
    pub p95: f64,
    /// Mean L* of the image less the reference's: below zero, the image is
    /// darker (as where the camera's DRO lifted the JPEG's shadows).
    pub lightness: f64,
}

/// ΔE2000 of `image` against `reference` (the same size), over each
/// [`Region`]. Pixels the camera clipped (any channel at 255 or 0 in the
/// reference) are left out: there's nothing there to match.
pub fn compare(image: &Rgba8, reference: &Rgba8) -> Result<Vec<(Region, Summary)>, String> {
    if (image.width, image.height) != (reference.width, reference.height) {
        return Err(format!("{}×{} against {}×{}", image.width, image.height, reference.width, reference.height));
    }
    let pairs: Vec<(Lab, f64, f64)> = image
        .data
        .iter()
        .zip(&reference.data)
        .filter(|(_, r)| r[..3].iter().all(|&c| c > 0 && c < 255))
        .map(|(p, r)| {
            let (reference, image) = (lab([r[0], r[1], r[2]]), lab([p[0], p[1], p[2]]));
            (reference, ciede2000(image, reference), image[0] - reference[0])
        })
        .collect();
    let total = image.data.len().max(1) as f64;
    Ok(Region::ALL
        .into_iter()
        .map(|region| {
            let held: Vec<_> = pairs.iter().filter(|(r, ..)| region.holds(*r)).collect();
            if held.is_empty() {
                return (region, Summary::default());
            }
            let mut de: Vec<f64> = held.iter().map(|(_, d, _)| *d).collect();
            de.sort_by(f64::total_cmp);
            let mean = de.iter().sum::<f64>() / de.len() as f64;
            let p95 = de[((de.len() - 1) as f64 * 0.95).round() as usize];
            let lightness = held.iter().map(|(.., l)| *l).sum::<f64>() / held.len() as f64;
            (region, Summary { share: de.len() as f64 / total, mean, p95, lightness })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pairs from Sharma, Wu and Dalal's test data (their table 1), with
    /// the ΔE2000 they give.
    #[test]
    fn matches_the_published_test_data() {
        let data: [(Lab, Lab, f64); 8] = [
            ([50.0, 2.6772, -79.7751], [50.0, 0.0, -82.7485], 2.0425),
            ([50.0, 3.1571, -77.2803], [50.0, 0.0, -82.7485], 2.8615),
            ([50.0, 0.0, 0.0], [50.0, -1.0, 2.0], 2.3669),
            ([50.0, 2.49, -0.001], [50.0, -2.49, 0.0011], 7.2195),
            ([50.0, 2.5, 0.0], [73.0, 25.0, -18.0], 27.1492),
            ([50.0, 2.5, 0.0], [50.0, 3.2592, 0.335], 1.0),
            ([60.2574, -34.0099, 36.2677], [60.4626, -34.1751, 39.4387], 1.2644),
            ([2.0776, 0.0795, -1.135], [0.9033, -0.0636, -0.5514], 0.9082),
        ];
        for (a, b, want) in data {
            let got = ciede2000(a, b);
            assert!((got - want).abs() < 1e-4, "{a:?} {b:?}: {got} against {want}");
            assert!((ciede2000(b, a) - want).abs() < 1e-4, "symmetric");
        }
    }

    #[test]
    fn srgb_lab_of_white_grey_and_red() {
        let close = |a: Lab, b: Lab| a.iter().zip(&b).all(|(x, y)| (x - y).abs() < 0.05);
        assert!(close(lab([255, 255, 255]), [100.0, 0.0, 0.0]), "{:?}", lab([255; 3]));
        assert!(close(lab([119, 119, 119]), [50.03, 0.0, 0.0]), "{:?}", lab([119; 3]));
        assert!(close(lab([255, 0, 0]), [53.24, 80.09, 67.20]), "{:?}", lab([255, 0, 0]));
    }

    #[test]
    fn regions_summarise_only_their_pixels() {
        let mut reference = Rgba8::new(4, 1);
        reference.data = vec![[200, 40, 40, 255], [224, 172, 140, 255], [110, 160, 230, 255], [255, 255, 255, 255]];
        let mut image = reference.clone();
        image.data[0] = [190, 50, 50, 255];
        let summaries = compare(&image, &reference).unwrap();
        let get = |r: Region| summaries.iter().find(|(s, _)| *s == r).unwrap().1;
        // the clipped white is left out; three pixels count, one of them off
        assert_eq!(get(Region::All).share, 0.75);
        assert!(get(Region::Saturated).mean > 1.0 && get(Region::Saturated).share == 0.25);
        // the changed pixel is darker than the reference's, by its L* difference
        let want = lab([190, 50, 50])[0] - lab([200, 40, 40])[0];
        assert!(want < 0.0 && (get(Region::Saturated).lightness - want).abs() < 1e-9);
        assert!((get(Region::All).lightness - want / 3.0).abs() < 1e-9);
        assert_eq!(get(Region::Skin).lightness, 0.0);
        assert_eq!(get(Region::Skin).mean, 0.0);
        assert_eq!(get(Region::Skin).share, 0.25);
        assert_eq!(get(Region::Sky).share, 0.25);
        assert!(compare(&Rgba8::new(2, 2), &reference).is_err());
    }
}

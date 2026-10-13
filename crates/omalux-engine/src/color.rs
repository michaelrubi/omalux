//! Showing frames in the monitor's colours, on `moxcms` as Lightcraft
//! does, rather than Little CMS 2 as Omacull and Omapix do: one colour
//! engine (M0 found Lightcraft's covers it).
//!
//! Hyprland sends what's drawn straight to a monitor it takes for sRGB, so
//! on a wide-gamut one every colour comes out too saturated unless it's
//! converted to the monitor's profile first. Lightcraft's
//! [`DisplayProfile`] does the converting (matrix and LUT profiles both),
//! and its pipeline renders into a display's primaries directly. What
//! OmaLux adds is Omacull's way of finding a profile where there's no ICC
//! file: the colours the monitor's EDID reports ([`edid_profile`]).

use lightcraft_codecs::display::DisplayProfile;
use lightcraft_codecs::{Trc, icc};
use lightcraft_color::{RgbSpace, Xy};

/// A monitor's colours as its EDID gives them: the red, green, blue and
/// white it shows, as an ICC matrix profile. `None` if the EDID has none,
/// or they aren't believable as a monitor's (many report zeros or
/// leftovers).
///
/// Its tone curve is sRGB's when the EDID says gamma 2.2 (or nothing),
/// which is a nominal figure: greys then come out as they would with no
/// profile, and only colours change.
///
/// Ported from Omacull's `ColorProfile::from_edid` (GPL-3.0-or-later).
pub fn edid_profile(edid: &[u8]) -> Option<Vec<u8>> {
    const HEADER: [u8; 8] = [0, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0];
    if edid.len() < 128 || edid[..8] != HEADER {
        return None;
    }
    // Ten bits each: the low two packed into bytes 25 and 26.
    let xy = |i: usize| {
        let low = u16::from(edid[25 + i / 4] >> (6 - 2 * (i % 4))) & 3;
        f64::from((u16::from(edid[27 + i]) << 2) | low) / 1024.0
    };
    let [red, green, blue, white] = [0, 2, 4, 6].map(|i| (xy(i), xy(i + 1)));
    let area = ((green.0 - red.0) * (blue.1 - red.1) - (blue.0 - red.0) * (green.1 - red.1)).abs() / 2.0;
    let believable = red.0 > 0.55
        && red.1 < 0.4
        && green.1 > 0.5
        && blue.0 < 0.25
        && blue.1 < 0.2
        && (0.25..0.4).contains(&white.0)
        && (0.25..0.42).contains(&white.1)
        && [red, green, blue].iter().all(|c| c.1 > 0.0 && c.0 + c.1 <= 1.0)
        // sRGB's triangle is 0.112; Rec. 2020's 0.212.
        && (0.07..0.25).contains(&area);
    if !believable {
        return None;
    }
    let gamma = match edid[23] {
        0xff => 2.2,
        g => (f64::from(g) + 100.0) / 100.0,
    };
    let trc = if (gamma - 2.2).abs() < 0.05 { Trc::Srgb } else { Trc::Gamma(gamma as f32) };
    let point = |(x, y): (f64, f64)| Xy::new(x, y);
    let space = RgbSpace { name: "Monitor (EDID)", r: point(red), g: point(green), b: point(blue), white: point(white) };
    Some(icc::write_matrix_trc(&space, &trc))
}

/// The monitor's profile, ready to convert to, from its ICC file's bytes
/// or else its EDID.
pub fn display(icc: Option<&[u8]>, edid: Option<&[u8]>) -> Result<Option<DisplayProfile>, String> {
    match (icc, edid.and_then(edid_profile)) {
        (Some(icc), _) => DisplayProfile::from_icc(icc).map(Some),
        (None, Some(from_edid)) => DisplayProfile::from_icc(&from_edid).map(Some),
        (None, None) => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lightcraft_raster::Rgba8;

    /// An EDID header and chromaticities, as a monitor reports them.
    pub(crate) fn edid(colours: [(f64, f64); 4], gamma: u8) -> Vec<u8> {
        let mut edid = vec![0u8; 128];
        edid[..8].copy_from_slice(&[0, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0]);
        edid[23] = gamma;
        for (i, value) in colours.iter().flat_map(|&(x, y)| [x, y]).enumerate() {
            let bits = (value * 1024.0).round() as u16;
            edid[27 + i] = (bits >> 2) as u8;
            edid[25 + i / 4] |= ((bits & 3) as u8) << (6 - 2 * (i % 4));
        }
        edid
    }

    const P3: [(f64, f64); 4] = [(0.68, 0.32), (0.265, 0.69), (0.15, 0.06), (0.3127, 0.329)];

    fn shown(profile: &DisplayProfile, rgb: [u8; 3]) -> [u8; 3] {
        let mut image = Rgba8::filled(1, 1, [rgb[0], rgb[1], rgb[2], 255]);
        profile.from_srgb(&mut image).unwrap();
        let [r, g, b, _] = image.data[0];
        [r, g, b]
    }

    #[test]
    fn edid_colours_make_a_profile_when_theyre_believable() {
        assert!(edid_profile(&edid(P3, 120)).is_some());
        assert!(edid_profile(&edid([(0.0, 0.0); 4], 120)).is_none());
        assert!(edid_profile(&edid(P3, 120)[..100]).is_none());
        assert!(display(None, Some(&edid([(0.0, 0.0); 4], 120))).unwrap().is_none());
        assert!(display(Some(b"not a profile"), None).is_err());
    }

    #[test]
    fn on_a_wide_gamut_monitor_srgb_is_less_saturated_and_grey_stays_grey() {
        let p3 = display(None, Some(&edid(P3, 120))).unwrap().unwrap();
        let red = shown(&p3, [255, 0, 0]);
        assert!(red[0] < 240 && red[1] > 20, "{red:?}");
        // sRGB's curve for gamma 2.2: greys are left as they are
        for v in [0, 64, 128, 200, 255] {
            let g = shown(&p3, [v; 3]);
            assert!(g.iter().all(|c| c.abs_diff(v) <= 1), "{v}: {g:?}");
        }
        // an sRGB monitor shows sRGB as it is, but for the EDID's 10-bit chromaticities
        let srgb = display(None, Some(&edid([(0.64, 0.33), (0.30, 0.60), (0.15, 0.06), (0.3127, 0.329)], 120))).unwrap().unwrap();
        for rgb in [[255, 0, 0], [30, 160, 90], [250, 240, 10]] {
            let s = shown(&srgb, rgb);
            assert!(s.iter().zip(rgb).all(|(a, b)| a.abs_diff(b) <= 4), "{rgb:?}: {s:?}");
        }
    }
}

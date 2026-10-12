//! Export: what Omapix opens. 16-bit TIFFs in Linear ProPhoto RGB with
//! their ICC profile, as Michael's darktable exports were (decided
//! 2026-10-12; see "Pipeline" in DESIGN.md).
//!
//! Lightcraft's pipeline renders linear output as 32-bit float and its
//! 16-bit output display-encoded; this takes the float render (after the
//! output's shoulder and gamut mapping have brought everything into 0..1)
//! down to 16-bit integers, linear. 16-bit linear has four times the levels
//! of the A7 III's 14 bits in every stop.

use lightcraft_codecs::{EncodeImage, EncodeMeta, Samples, TiffCompression, Trc, encode, icc};
use lightcraft_develop::DevelopSettings;
use lightcraft_pipeline::{DeepSamples, OutputDepth, OutputSpace, RenderRequest, SourceInfo};
use lightcraft_raster::Rgb32f;

/// A frame rendered for export: linear ProPhoto RGB, 16 bits a channel,
/// interleaved.
#[derive(Clone, Debug, PartialEq)]
pub struct Linear16 {
    pub width: usize,
    pub height: usize,
    pub samples: Vec<u16>,
}

/// Render `src` at full size for export, on the CPU pipeline.
pub fn render(src: &Rgb32f, info: &SourceInfo, settings: &DevelopSettings) -> Result<Linear16, String> {
    // the crop at the source's own resolution
    let (w, h) = lightcraft_pipeline::native_output_size(src.width, src.height, settings);
    let (w, h) = ((w.round() as usize).max(1), (h.round() as usize).max(1));
    let request = RenderRequest { space: OutputSpace::ProPhoto, depth: OutputDepth::F32Linear, ..RenderRequest::fit(w, h) };
    let rendered = lightcraft_pipeline::render(src, info, settings, &request);
    let deep = rendered.deep.ok_or("the pipeline made no linear render")?;
    let DeepSamples::F32(linear) = deep.samples else { return Err("the pipeline's linear render isn't float".into()) };
    Ok(Linear16 { width: deep.width, height: deep.height, samples: linear.iter().map(|&v| to_u16(v)).collect() })
}

/// A linear sample, 0..1, as 16 bits; out of range and NaN clamped.
fn to_u16(v: f32) -> u16 {
    if v.is_nan() { 0 } else { (v.clamp(0.0, 1.0) * 65535.0).round() as u16 }
}

/// Linear ProPhoto RGB's ICC profile.
pub fn profile() -> Vec<u8> {
    icc::write_matrix_trc(&OutputSpace::ProPhoto.rgb_space(), &Trc::Linear)
}

/// `image` as a TIFF, with its profile (Deflate, as Omapix writes them).
pub fn tiff(image: &Linear16) -> Result<Vec<u8>, String> {
    let (w, h) = (u32::try_from(image.width).map_err(|e| e.to_string())?, u32::try_from(image.height).map_err(|e| e.to_string())?);
    let icc = profile();
    let meta = EncodeMeta { icc: Some(&icc), ..Default::default() };
    encode::encode_tiff(&EncodeImage::new(w, h, 3, Samples::U16(&image.samples)), TiffCompression::Deflate, &meta).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn samples_are_linear_and_clamped() {
        assert_eq!([0.0, 1.0, 0.5, -0.1, 2.0, f32::NAN, f32::INFINITY].map(to_u16), [0, 65535, 32768, 0, 65535, 0, 65535]);
        // linear: an 18 % grey is 18 % of the range, not display-encoded
        assert_eq!(to_u16(0.18), 11796);
    }

    #[test]
    fn the_profile_is_linear_prophoto() {
        let info = icc::parse(&profile()).unwrap();
        assert!(info.description.as_deref().is_some_and(|d| d.contains("ProPhoto") && d.contains("linear")), "{info:?}");
    }

    #[test]
    fn a_rendered_frame_is_a_16_bit_tiff_with_its_profile() {
        // A scene-referred gradient from black to past white: what a raw gives the pipeline.
        let mut src = Rgb32f::new(64, 48);
        for (i, p) in src.data.iter_mut().enumerate() {
            let x = (i % 64) as f32 / 63.0;
            *p = [x * 1.5, x * 1.2, x];
        }
        let info = SourceInfo { raw: true, ..Default::default() };
        let image = render(&src, &info, &DevelopSettings::default()).unwrap();
        assert_eq!((image.width, image.height, image.samples.len()), (64, 48, 64 * 48 * 3));
        // dark to bright along each row, the whole range used
        let row: Vec<u16> = image.samples[..64 * 3].chunks(3).map(|p| p[1]).collect();
        assert!(row.windows(2).all(|w| w[0] <= w[1]), "{row:?}");
        assert!(row[0] < 64 && row[63] > 60000, "{row:?}");

        let bytes = tiff(&image).unwrap();
        let mut decoder = tiff::decoder::Decoder::new(std::io::Cursor::new(&bytes)).unwrap();
        assert_eq!(decoder.dimensions().unwrap(), (64, 48));
        assert_eq!(decoder.colortype().unwrap(), tiff::ColorType::RGB(16));
        let icc = decoder.find_tag(tiff::tags::Tag::IccProfile).unwrap().unwrap().into_u8_vec().unwrap();
        assert_eq!(icc, profile());
        let tiff::decoder::DecodingResult::U16(samples) = decoder.read_image().unwrap() else { panic!("not 16-bit") };
        assert_eq!(samples, image.samples);
    }
}

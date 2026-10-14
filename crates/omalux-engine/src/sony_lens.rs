//! Sony's own lens corrections: the distortion, vignetting and lateral
//! chromatic aberration tables the camera writes into the raw IFD of every
//! ARW (`0x7037`, `0x7032`, `0x7035`), whatever its lens-compensation
//! settings, for the lens on it and the focal length, aperture and focus
//! distance it was shot at. Read for the ILCE-7M3 only (other cameras are
//! on the roadmap's "Later" list), as Lightroom's profile correction is
//! put to the pipeline: one [`EmbeddedLens`], found from the raw alone.
//!
//! Not written yet (M5): see "Lens corrections" in docs/DESIGN.md.

use lightcraft_develop::EmbeddedLens;

/// Sony's corrections for the raw `bytes`, whose header is `raw`,
/// re-expressed for its default-cropped, EXIF-oriented image as
/// [`crate::raw::embedded_lens`] does. `None` for another camera, a raw
/// without the tables, or tables that can't be trusted.
pub fn corrections(bytes: &[u8], raw: &lightcraft_raw::RawInfo) -> Option<EmbeddedLens> {
    let _ = (bytes, raw);
    None
}

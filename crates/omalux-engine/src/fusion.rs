//! Exposure fusion: the alternative to the HDR slider's local tone
//! mapping that M4 tries (see "HDR" in docs/DESIGN.md). Several exposures
//! of one frame, each developed from the same raw, mixed so that every
//! part comes from the exposure that shows it best.

use lightcraft_raster::Rgb32f;

/// Fuse `exposures` of one frame: the same size, display-referred (sRGB's
/// curve, 0..1). `None` if there are none or their sizes differ.
pub fn fuse(exposures: &[Rgb32f]) -> Option<Rgb32f> {
    let _ = exposures;
    None
}

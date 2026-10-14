//! Adobe's DNG Camera Profiles (`.dcp`): Lightroom's Camera Standard and
//! Camera Vivid for the ILCE-7M3, the other candidate for the camera match
//! (see "The look" in docs/DESIGN.md). Read from where Michael put them
//! (Adobe's free DNG Converter installs them), never shipped.
//!
//! A DCP is a TIFF with its own magic number, holding the camera-profile
//! tags of the DNG specification (chapter 6) in one IFD. They become a
//! [`ColorData`], the colour model Lightcraft's pipeline renders a DNG's
//! own profile with; Lightcraft itself doesn't read `.dcp` files.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use lightcraft_raw::ColorData;

/// A camera profile read from a `.dcp` file.
#[derive(Clone, Debug, PartialEq)]
pub struct Dcp {
    /// `ProfileName`: "Camera Standard", "Camera Vivid", "Adobe Standard".
    pub name: String,
    /// `UniqueCameraModel`, the camera it's for: "Sony ILCE-7M3".
    pub camera: String,
    /// Its colour model: calibration illuminants, colour and forward
    /// matrices, hue/saturation maps, look table and tone curve.
    pub color: ColorData,
}

/// Read a profile from a `.dcp` file's bytes.
pub fn parse(bytes: &[u8]) -> Result<Dcp, String> {
    let _ = bytes;
    Err("reading DCP profiles isn't built yet".into())
}

/// Read a profile from a `.dcp` file.
pub fn load(path: &Path) -> Result<Dcp, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    parse(&bytes).map_err(|e| format!("{}: {e}", path.display()))
}

/// Where profiles are looked for: `OMALUX_DCP`, else
/// `~/.config/omalux/dcp`.
pub fn dir() -> Option<PathBuf> {
    None
}

/// The profile in [`dir`] for camera `model` (Exif's, "ILCE-7M3") and
/// Creative Style `style` ("Standard", "Vivid"): the one named "Camera
/// `style`" whose camera is `model`'s. Read once per process.
pub fn find(model: &str, style: &str) -> Option<Arc<Dcp>> {
    let _ = (model, style);
    None
}

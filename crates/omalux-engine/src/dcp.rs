//! Adobe's DNG Camera Profiles (`.dcp`): Lightroom's Camera Standard and
//! Camera Vivid for the ILCE-7M3, the other candidate for the camera match
//! (see "The look" in docs/DESIGN.md). Read from where Michael put them
//! (Adobe's free DNG Converter installs them), never shipped.
//!
//! A DCP is a TIFF with its own magic number, holding the camera-profile
//! tags of the DNG specification (chapter 6) in one IFD. They become a
//! [`ColorData`], the colour model Lightcraft's pipeline renders a DNG's
//! own profile with; Lightcraft itself doesn't read `.dcp` files.
//! Tag reading is modelled on Lightcraft's DNG colour tags in `crates/raw/src/dng.rs`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use lightcraft_color::Mat3;
use lightcraft_raw::ColorData;
use lightcraft_raw::profile::{HsvTable, ProfileLook, ToneCurve};
use lightcraft_tiff::{Tiff, tags};

/// Largest profile file read (Adobe's are rarely over 1 MB).
const MAX_FILE: usize = 8 << 20;

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

/// Convert 9 floats from a tag into an invertible 3×3 matrix.
fn mat3(v: Option<Vec<f64>>) -> Option<Mat3> {
    let v = v?;
    let arr: [f64; 9] = v.as_slice().try_into().ok()?;
    if arr.iter().any(|x| !x.is_finite()) {
        return None;
    }
    let m = Mat3([[arr[0], arr[1], arr[2]], [arr[3], arr[4], arr[5]], [arr[6], arr[7], arr[8]]]);
    (m.determinant().abs() > 1e-12).then_some(m)
}

/// Read a profile from a `.dcp` file's bytes.
pub fn parse(bytes: &[u8]) -> Result<Dcp, String> {
    if bytes.len() > MAX_FILE {
        return Err(format!("{} bytes is too large for a DCP profile (limit 8 MB)", bytes.len()));
    }
    if bytes.len() < 8 {
        return Err("file too short to be a DCP profile".into());
    }
    let mut tiff_bytes = bytes.to_vec();
    if bytes.starts_with(b"IIRC") {
        tiff_bytes[2] = 0x2A;
        tiff_bytes[3] = 0x00;
    } else if bytes.starts_with(b"MMCR") {
        tiff_bytes[2] = 0x00;
        tiff_bytes[3] = 0x2A;
    } else {
        return Err("not a DCP profile (expected IIRC or MMCR header)".into());
    }

    let tiff = Tiff::parse(&tiff_bytes).map_err(|e| e.to_string())?;
    let ifd = tiff.ifds.first().ok_or_else(|| "no IFDs in DCP file".to_string())?;

    let name = ifd
        .string(tags::PROFILE_NAME)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| "missing or empty ProfileName".to_string())?;
    let camera = ifd.string(tags::UNIQUE_CAMERA_MODEL).unwrap_or_default();

    let cm1 = mat3(ifd.f64s(tags::COLOR_MATRIX_1)).ok_or_else(|| "missing or invalid ColorMatrix1".to_string())?;
    let cm2 = mat3(ifd.f64s(tags::COLOR_MATRIX_2));
    let fm1 = mat3(ifd.f64s(tags::FORWARD_MATRIX_1));
    let fm2 = mat3(ifd.f64s(tags::FORWARD_MATRIX_2));

    let table = |dims_tag: u16, data_tag: u16, enc_tag: u16| -> Option<HsvTable> {
        let dims = ifd.u64s(dims_tag)?;
        let data = ifd.f64s(data_tag)?;
        let enc = ifd.u64s(enc_tag).and_then(|v| v.first().copied()).unwrap_or(0);
        HsvTable::from_tags(&dims, &data, enc)
    };

    let hue_sat_map = [
        table(tags::PROFILE_HUE_SAT_MAP_DIMS, tags::PROFILE_HUE_SAT_MAP_DATA_1, tags::PROFILE_HUE_SAT_MAP_ENCODING),
        table(tags::PROFILE_HUE_SAT_MAP_DIMS, tags::PROFILE_HUE_SAT_MAP_DATA_2, tags::PROFILE_HUE_SAT_MAP_ENCODING),
    ];
    let look_table = table(tags::PROFILE_LOOK_TABLE_DIMS, tags::PROFILE_LOOK_TABLE_DATA, tags::PROFILE_LOOK_TABLE_ENCODING);
    let tone_curve = ifd.f64s(tags::PROFILE_TONE_CURVE).and_then(|v| ToneCurve::from_tag(&v));

    let baseline_exposure = ifd
        .f64s(tags::BASELINE_EXPOSURE_OFFSET)
        .and_then(|v| v.first().copied())
        .filter(|v| v.is_finite())
        .unwrap_or(0.0);

    let color = ColorData {
        illuminant: [
            ifd.u16(tags::CALIBRATION_ILLUMINANT_1).unwrap_or(0),
            ifd.u16(tags::CALIBRATION_ILLUMINANT_2).unwrap_or(0),
        ],
        color_matrix: [Some(cm1), cm2],
        forward_matrix: [fm1, fm2],
        baseline_exposure,
        profile: ProfileLook {
            hue_sat_map,
            look_table,
            tone_curve,
            gain_table_map: None,
        },
        ..Default::default()
    };

    Ok(Dcp { name, camera, color })
}

/// Read a profile from a `.dcp` file.
pub fn load(path: &Path) -> Result<Dcp, String> {
    let size = std::fs::metadata(path).map_err(|e| format!("{}: {e}", path.display()))?.len();
    if size > MAX_FILE as u64 {
        return Err(format!("{}: {size} bytes is too large for a DCP profile", path.display()));
    }
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    parse(&bytes).map_err(|e| format!("{}: {e}", path.display()))
}

/// Where profiles are looked for: `OMALUX_DCP`, else
/// `~/.config/omalux/dcp`.
pub fn dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("OMALUX_DCP") {
        return Some(PathBuf::from(dir));
    }
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(config.join("omalux/dcp"))
}

/// Collect every `.dcp` file in `dir` and its subdirectories up to three levels down.
fn collect_dcps(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if depth < 3 {
                collect_dcps(&path, depth + 1, out);
            }
        } else if path.is_file()
            && path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("dcp"))
        {
            out.push(path);
        }
    }
}

/// Search for a profile matching camera `model` and Creative Style `style` within `dir`.
fn find_in(dir: &Path, model: &str, style: &str) -> Option<Dcp> {
    let model = model.trim();
    let style = style.trim();
    if model.is_empty() || style.is_empty() {
        return None;
    }
    let target_name = format!("camera {style}").to_lowercase();
    let target_model = model.to_lowercase();

    let mut paths = Vec::new();
    collect_dcps(dir, 0, &mut paths);
    paths.sort();

    for path in paths {
        match load(&path) {
            Ok(dcp) => {
                if dcp.name.trim().to_lowercase() == target_name
                    && dcp.camera.trim().to_lowercase().ends_with(&target_model)
                {
                    return Some(dcp);
                }
            }
            Err(e) => {
                log::warn!("ignoring DCP profile {e}");
            }
        }
    }
    None
}

fn cache() -> &'static Mutex<HashMap<(String, String), Option<Arc<Dcp>>>> {
    static CACHE: OnceLock<Mutex<HashMap<(String, String), Option<Arc<Dcp>>>>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// The profile in [`dir`] for camera `model` (Exif's, "ILCE-7M3") and
/// Creative Style `style` ("Standard", "Vivid"): the one named "Camera
/// `style`" whose camera is `model`'s. Read once per process.
pub fn find(model: &str, style: &str) -> Option<Arc<Dcp>> {
    let model = model.trim();
    let style = style.trim();
    if model.is_empty() || style.is_empty() {
        return None;
    }
    let key = (model.to_lowercase(), style.to_lowercase());
    let mut cache = cache().lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(hit) = cache.get(&key) {
        return hit.clone();
    }
    let profile = dir().and_then(|d| find_in(&d, model, style)).map(Arc::new);
    cache.insert(key, profile.clone());
    profile
}

#[cfg(test)]
mod tests {
    use super::*;
    use lightcraft_color::D50;
    use lightcraft_tiff::{ByteOrder, IfdBuilder, TiffWriter, Value};

    fn patch_magic(order: ByteOrder, mut bytes: Vec<u8>) -> Vec<u8> {
        match order {
            ByteOrder::Little => {
                bytes[2] = b'R';
                bytes[3] = b'C';
            }
            ByteOrder::Big => {
                bytes[2] = b'C';
                bytes[3] = b'R';
            }
        }
        bytes
    }

    fn matrix_rationals(m: &Mat3) -> Vec<(i32, i32)> {
        m.0.iter().flatten().map(|&x| lightcraft_tiff::writer::srational(x)).collect()
    }

    fn a7m3_matrices() -> (Mat3, Mat3, Mat3, Mat3) {
        let cm1 = Mat3([[1.375, -0.592, 0.037], [-0.219, 1.125, 0.326], [-0.022, 0.098, 0.586]]);
        let cm2 = Mat3([[0.907, -0.245, -0.087], [-0.315, 1.065, 0.215], [-0.112, 0.180, 0.440]]);
        let fm1 = Mat3([[0.678, 0.203, 0.083], [0.105, 1.012, -0.117], [0.056, -0.465, 1.234]]);
        let fm2 = Mat3([[0.655, 0.322, -0.013], [0.180, 1.101, -0.281], [0.064, -0.269, 1.030]]);
        (cm1, cm2, fm1, fm2)
    }

    fn full_ifd() -> IfdBuilder {
        let (cm1, cm2, fm1, fm2) = a7m3_matrices();
        let mut ifd = IfdBuilder::new();
        ifd.set(tags::PROFILE_NAME, Value::Ascii("Camera Standard".into()));
        ifd.set(tags::UNIQUE_CAMERA_MODEL, Value::Ascii("Sony ILCE-7M3".into()));
        ifd.set(tags::CALIBRATION_ILLUMINANT_1, Value::Short(vec![17]));
        ifd.set(tags::CALIBRATION_ILLUMINANT_2, Value::Short(vec![21]));
        ifd.set(tags::COLOR_MATRIX_1, Value::SRational(matrix_rationals(&cm1)));
        ifd.set(tags::COLOR_MATRIX_2, Value::SRational(matrix_rationals(&cm2)));
        ifd.set(tags::FORWARD_MATRIX_1, Value::SRational(matrix_rationals(&fm1)));
        ifd.set(tags::FORWARD_MATRIX_2, Value::SRational(matrix_rationals(&fm2)));
        ifd.set(tags::PROFILE_HUE_SAT_MAP_DIMS, Value::Long(vec![2, 2, 1]));
        ifd.set(tags::PROFILE_HUE_SAT_MAP_DATA_1, Value::Float(vec![0.0, 1.0, 1.0, 5.0, 1.2, 1.0, -3.0, 0.8, 1.0, 2.0, 1.1, 1.0]));
        ifd.set(tags::PROFILE_HUE_SAT_MAP_DATA_2, Value::Float(vec![1.0, 1.0, 1.0, 6.0, 1.2, 1.0, -2.0, 0.8, 1.0, 3.0, 1.1, 1.0]));
        ifd.set(tags::PROFILE_HUE_SAT_MAP_ENCODING, Value::Long(vec![0]));
        ifd.set(tags::PROFILE_LOOK_TABLE_DIMS, Value::Long(vec![2, 2, 1]));
        ifd.set(tags::PROFILE_LOOK_TABLE_DATA, Value::Float(vec![0.0, 1.0, 1.0, 1.0, 1.0, 1.0, 2.0, 1.0, 1.0, 3.0, 1.0, 1.0]));
        ifd.set(tags::PROFILE_LOOK_TABLE_ENCODING, Value::Long(vec![0]));
        ifd.set(tags::PROFILE_TONE_CURVE, Value::Float(vec![0.0, 0.0, 0.25, 0.2, 0.5, 0.5, 0.75, 0.8, 1.0, 1.0]));
        ifd.set(tags::BASELINE_EXPOSURE_OFFSET, Value::SRational(vec![lightcraft_tiff::writer::srational(0.5)]));
        ifd
    }

    #[test]
    fn profile_with_every_tag_round_trips() {
        let (cm1, cm2, fm1, fm2) = a7m3_matrices();
        for order in [ByteOrder::Little, ByteOrder::Big] {
            let tiff_bytes = TiffWriter::new(order, false).write(&[full_ifd()]).unwrap();
            let dcp_bytes = patch_magic(order, tiff_bytes);
            let dcp = parse(&dcp_bytes).unwrap();

            assert_eq!(dcp.name, "Camera Standard");
            assert_eq!(dcp.camera, "Sony ILCE-7M3");
            assert_eq!(dcp.color.illuminant, [17, 21]);
            assert_eq!(dcp.color.color_matrix[0], Some(cm1));
            assert_eq!(dcp.color.color_matrix[1], Some(cm2));
            assert_eq!(dcp.color.forward_matrix[0], Some(fm1));
            assert_eq!(dcp.color.forward_matrix[1], Some(fm2));
            assert!((dcp.color.baseline_exposure - 0.5).abs() < 1e-6);

            let hue_sat = &dcp.color.profile.hue_sat_map;
            let t1 = hue_sat[0].as_ref().unwrap();
            assert_eq!((t1.hue_divisions, t1.sat_divisions, t1.val_divisions), (2, 2, 1));
            assert_eq!(t1.data.len(), 4);
            assert_eq!(t1.data[0], [0.0, 1.0, 1.0]);
            assert_eq!(t1.data[1], [5.0, 1.2, 1.0]);

            let t2 = hue_sat[1].as_ref().unwrap();
            assert_eq!((t2.hue_divisions, t2.sat_divisions, t2.val_divisions), (2, 2, 1));
            assert_eq!(t2.data[0], [1.0, 1.0, 1.0]);

            let look = dcp.color.profile.look_table.as_ref().unwrap();
            assert_eq!((look.hue_divisions, look.sat_divisions, look.val_divisions), (2, 2, 1));
            assert_eq!(look.data[3], [3.0, 1.0, 1.0]);

            let curve = dcp.color.profile.tone_curve.as_ref().unwrap();
            assert_eq!(curve.points.len(), 5);
            assert_eq!(curve.points[0], [0.0, 0.0]);
            assert_eq!(curve.points[4], [1.0, 1.0]);
        }
    }

    #[test]
    fn bad_files_are_rejected_without_panics() {
        for order in [ByteOrder::Little, ByteOrder::Big] {
            let plain_tiff = TiffWriter::new(order, false).write(&[full_ifd()]).unwrap();
            assert!(parse(&plain_tiff).is_err(), "plain TIFF (magic 42) must be rejected");
        }
        assert!(parse(b"").is_err(), "empty file");
        assert!(parse(b"II").is_err(), "too short");
        assert!(parse(b"IIRC").is_err(), "header only");
        assert!(parse(b"garbage not a tiff").is_err(), "garbage");

        let valid = patch_magic(ByteOrder::Little, TiffWriter::new(ByteOrder::Little, false).write(&[full_ifd()]).unwrap());
        assert!(parse(&valid[..valid.len() / 2]).is_err(), "truncated DCP");

        let mut no_name = full_ifd();
        no_name.entries.remove(&tags::PROFILE_NAME);
        let bytes = patch_magic(ByteOrder::Little, TiffWriter::new(ByteOrder::Little, false).write(&[no_name]).unwrap());
        assert!(parse(&bytes).is_err(), "missing ProfileName");

        let mut no_cm1 = full_ifd();
        no_cm1.entries.remove(&tags::COLOR_MATRIX_1);
        let bytes = patch_magic(ByteOrder::Little, TiffWriter::new(ByteOrder::Little, false).write(&[no_cm1]).unwrap());
        assert!(parse(&bytes).is_err(), "missing ColorMatrix1");

        let huge = vec![0u8; (8 << 20) + 1];
        assert!(parse(&huge).is_err(), "file larger than 8 MB");
    }

    #[test]
    fn profile_with_only_color_matrix_and_name() {
        let (cm1, ..) = a7m3_matrices();
        let mut ifd = IfdBuilder::new();
        ifd.set(tags::PROFILE_NAME, Value::Ascii("Camera Standard".into()));
        ifd.set(tags::COLOR_MATRIX_1, Value::SRational(matrix_rationals(&cm1)));

        let tiff_bytes = TiffWriter::new(ByteOrder::Little, false).write(&[ifd]).unwrap();
        let dcp_bytes = patch_magic(ByteOrder::Little, tiff_bytes);
        let dcp = parse(&dcp_bytes).unwrap();

        assert_eq!(dcp.name, "Camera Standard");
        assert_eq!(dcp.camera, "");
        assert_eq!(dcp.color.color_matrix[0], Some(cm1));
        assert_eq!(dcp.color.color_matrix[1], None);
        assert_eq!(dcp.color.forward_matrix, [None, None]);
        assert_eq!(dcp.color.illuminant, [0, 0]);
        assert_eq!(dcp.color.baseline_exposure, 0.0);
        assert!(dcp.color.profile.is_empty());
    }

    #[test]
    fn find_in_picks_matching_profile_in_subfolders() {
        let (cm1, ..) = a7m3_matrices();
        let temp_dir = std::env::temp_dir().join(format!("omalux-dcp-find-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);

        let sony_dir = temp_dir.join("Camera").join("Sony ILCE-7M3");
        let nikon_dir = temp_dir.join("Camera").join("Nikon D850");
        let adobe_dir = temp_dir.join("Adobe");
        std::fs::create_dir_all(&sony_dir).unwrap();
        std::fs::create_dir_all(&nikon_dir).unwrap();
        std::fs::create_dir_all(&adobe_dir).unwrap();

        let make_dcp = |name: &str, camera: &str| -> Vec<u8> {
            let mut ifd = IfdBuilder::new();
            ifd.set(tags::PROFILE_NAME, Value::Ascii(name.into()));
            ifd.set(tags::UNIQUE_CAMERA_MODEL, Value::Ascii(camera.into()));
            ifd.set(tags::COLOR_MATRIX_1, Value::SRational(matrix_rationals(&cm1)));
            let tiff_bytes = TiffWriter::new(ByteOrder::Little, false).write(&[ifd]).unwrap();
            patch_magic(ByteOrder::Little, tiff_bytes)
        };

        std::fs::write(sony_dir.join("Sony ILCE-7M3 Camera Standard.dcp"), make_dcp("Camera Standard", "Sony ILCE-7M3")).unwrap();
        std::fs::write(sony_dir.join("Sony ILCE-7M3 Camera Vivid.DCP"), make_dcp("Camera Vivid", "Sony ILCE-7M3")).unwrap();
        std::fs::write(adobe_dir.join("Sony ILCE-7M3 Adobe Standard.dcp"), make_dcp("Adobe Standard", "Sony ILCE-7M3")).unwrap();
        std::fs::write(nikon_dir.join("Nikon D850 Camera Vivid.dcp"), make_dcp("Camera Vivid", "Nikon D850")).unwrap();
        std::fs::write(temp_dir.join("damaged.dcp"), b"IIRC\x00\x00\x00\x00corruptcontent").unwrap();

        let found = find_in(&temp_dir, "ILCE-7M3", "Vivid").expect("should find Camera Vivid");
        assert_eq!(found.name, "Camera Vivid");
        assert_eq!(found.camera, "Sony ILCE-7M3");

        let found_std = find_in(&temp_dir, "ILCE-7M3", "Standard").expect("should find Camera Standard");
        assert_eq!(found_std.name, "Camera Standard");

        assert!(find_in(&temp_dir, "ILCE-7M3", "Portrait").is_none());
        assert!(find_in(&temp_dir, "Canon EOS R5", "Vivid").is_none());

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn colour_model_is_usable_by_lightcraft() {
        let (cm1, ..) = a7m3_matrices();

        // 1. With full matrices (ColorMatrix1/2 + ForwardMatrix1/2)
        let tiff_bytes = TiffWriter::new(ByteOrder::Little, false).write(&[full_ifd()]).unwrap();
        let dcp = parse(&patch_magic(ByteOrder::Little, tiff_bytes)).unwrap();
        assert!(lightcraft_raw::color::has_matrix(&dcp.color));
        let cam_to_xyz = lightcraft_raw::color::camera_to_xyz_d50(&dcp.color, D50);
        assert!(cam_to_xyz.0.iter().flatten().all(|v| v.is_finite()));
        assert!(cam_to_xyz.inverse().is_some());

        // 2. With ColorMatrix1 only
        let mut ifd = IfdBuilder::new();
        ifd.set(tags::PROFILE_NAME, Value::Ascii("Camera Standard".into()));
        ifd.set(tags::COLOR_MATRIX_1, Value::SRational(matrix_rationals(&cm1)));
        let tiff_bytes = TiffWriter::new(ByteOrder::Little, false).write(&[ifd]).unwrap();
        let dcp_single = parse(&patch_magic(ByteOrder::Little, tiff_bytes)).unwrap();
        assert!(lightcraft_raw::color::has_matrix(&dcp_single.color));
        let cam_to_xyz_single = lightcraft_raw::color::camera_to_xyz_d50(&dcp_single.color, D50);
        assert!(cam_to_xyz_single.0.iter().flatten().all(|v| v.is_finite()));
        assert!(cam_to_xyz_single.inverse().is_some());
    }
}


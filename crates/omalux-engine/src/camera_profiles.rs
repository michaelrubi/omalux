//! Camera profiles, pooled from many photos of one camera model in one Creative Style: OmaLux's Camera match
//! (see "The look" in docs/DESIGN.md).
//!
//! A raw file without colour matrices (Sony ARW) can have its look fitted to its own embedded camera JPEG
//! (`camera_preview`), but one photo shows too little of some colours (a lime shirt covering a few dozen proxy
//! pixels next to a hillside of foliage at the same hue), and a look fitted per photo isn't the same from one
//! frame of a burst to the next. [`Pool`] pools the colour pairs of many photos per model and style and fits one
//! matrix and hue/saturation/value table. Lightcraft's photos of the model then fit only their tone and chroma
//! curves; OmaLux's take those from the profile too, so the whole look is fixed: the camera renders every frame
//! of a style through the same processing, and what varies in a per-photo fit is the fit's error.
//!
//! The tone curve is the camera's without its Dynamic Range Optimizer, whose local shadow lift is put back per
//! frame (`dro`): pooled from the photos shot with DRO off when there are enough of them, else Sony's own curve
//! for Standard without DRO, which Lightcraft pooled from ten bodies (`assets/sony-dro-off.json`).
//!
//! Profiles are JSON files (`<model>-<style>.json`) in [`dir`]: `OMALUX_CAMERA_PROFILES`, else
//! `~/.config/omalux/camera-profiles`, else built in ([`BUNDLED`]). They are read once per process; a damaged or
//! hostile file is ignored with a warning. They hold aggregate colour statistics only, never image content.
//! The `calibrate` example fits them.
//!
//! Lifted from Lightcraft (MIT OR Apache-2.0) at `701f78e`, `crates/engine/src/camera_profiles.rs`, and brought up
//! to `0da998f2` (the pairs follow the lens warp the camera applied to its JPEG, `c854e172`; the rest is other
//! cameras' bundled profiles), with OmaLux's folder; the Creative Style, the tone curve and the chroma curve are
//! OmaLux's.
use lightcraft_color::Mat3;
use lightcraft_pipeline::tone::CameraTone;
use lightcraft_raw::profile::HsvTable;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

/// Format version of the profile files.
const VERSION: u32 = 1;
/// Largest profile file read (a 5 × 72 × 5 table is ~60 KB of JSON).
const MAX_FILE: u64 = 4 << 20;

/// Profiles built into OmaLux: `(model, style, JSON)`.
///
/// The ILCE-7M3's Standard is fitted over Michael's four shoots of 2026 (1376 photos on the FE 35 and 85 mm f/1.8,
/// all with DRO Auto, so its tone curve is Sony's own without DRO). There's none for Vivid yet: he has shot nothing
/// in it, and a frame in a style without a profile gets the look fitted to its own JPEG.
pub const BUNDLED: &[(&str, &str, &str)] = &[("ILCE-7M3", "Standard", include_str!("../assets/camera-profiles/ILCE-7M3-Standard.json"))];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CameraProfile {
    pub version: u32,
    /// The camera model as the files report it (Exif `Model`), e.g. `ILCE-7M4`.
    pub model: String,
    /// The Creative Style as the camera writes it ("Standard", "Vivid"); empty for a camera that records none.
    #[serde(default)]
    pub style: String,
    /// Photos and colour pairs the profile was fitted on.
    pub files: usize,
    pub samples: usize,
    /// White-balanced camera RGB (with the baseline exposure) → linear Rec.2020, rows.
    matrix: [[f64; 3]; 3],
    /// Hue/saturation/value correction after `matrix` (linear ProPhoto RGB).
    pub hue_sat: Option<HsvTable>,
    /// The style's tone curve without DRO, with its chroma curve. A profile without one gives no fixed look.
    #[serde(default)]
    pub tone: Option<CameraTone>,
    /// Photos shot with DRO off that `tone` was pooled from; 0 when it's Sony's own curve for Standard.
    #[serde(default)]
    pub tone_files: usize,
}

impl CameraProfile {
    pub fn new(model: &str, style: &str, files: usize, samples: usize, matrix: Mat3, hue_sat: Option<HsvTable>) -> CameraProfile {
        CameraProfile { version: VERSION, model: model.to_owned(), style: style.to_owned(), files, samples, matrix: matrix.0, hue_sat, tone: None, tone_files: 0 }
    }

    pub fn matrix(&self) -> Mat3 {
        Mat3(self.matrix)
    }

    /// Whether the data is usable (bounded matrix, table shape matching its data).
    fn valid(&self) -> bool {
        let matrix = self.matrix.iter().flatten().all(|v| v.is_finite() && v.abs() < 8.0) && Mat3(self.matrix).inverse().is_some();
        let table = self.hue_sat.as_ref().is_none_or(|t| {
            let dims = (1..=4096).contains(&t.hue_divisions) && (2..=4096).contains(&t.sat_divisions) && (1..=4096).contains(&t.val_divisions);
            let len = t.hue_divisions.checked_mul(t.sat_divisions).and_then(|n| n.checked_mul(t.val_divisions));
            dims && len == Some(t.data.len()) && t.data.iter().flatten().all(|v| v.is_finite()) && t.data.iter().all(|e| e[1] >= 0.0 && e[2] >= 0.0)
        });
        self.version == VERSION && !self.model.is_empty() && matrix && table
    }
}

/// Where camera profiles are read from and written to.
pub fn dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("OMALUX_CAMERA_PROFILES") {
        return Some(PathBuf::from(dir));
    }
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(config.join("omalux/camera-profiles"))
}

/// File name of the profile for `model` in `style`: letters, digits, `-` and `_` kept, anything else `_`.
pub fn file_name(model: &str, style: &str) -> String {
    let safe = |s: &str| -> String { s.trim().chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect() };
    if style.trim().is_empty() { format!("{}.json", safe(model)) } else { format!("{}-{}.json", safe(model), safe(style)) }
}

/// Read and validate a profile file.
pub fn load(path: &Path) -> Result<CameraProfile, String> {
    let size = std::fs::metadata(path).map_err(|e| format!("{}: {e}", path.display()))?.len();
    if size > MAX_FILE {
        return Err(format!("{}: {size} bytes is too large for a camera profile", path.display()));
    }
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    parse(&bytes, &path.display().to_string())
}

/// Parse and validate a profile's JSON (`origin` names it in errors).
fn parse(json: &[u8], origin: &str) -> Result<CameraProfile, String> {
    let profile: CameraProfile = serde_json::from_slice(json).map_err(|e| format!("{origin}: {e}"))?;
    if !profile.valid() {
        return Err(format!("{origin}: not a usable camera profile (version {}, model {:?})", profile.version, profile.model));
    }
    Ok(profile)
}

/// The built-in profile for `model` in `style`, if any.
fn bundled(model: &str, style: &str) -> Option<CameraProfile> {
    let (_, _, json) = BUNDLED.iter().find(|(m, s, _)| *m == model && *s == style)?;
    parse(json.as_bytes(), &format!("built-in profile {model} {style}")).inspect_err(|e| log::warn!("ignoring camera profile {e}")).ok()
}

/// Write `profile` to `dir` (created if needed) as `<model>-<style>.json`; returns the path.
pub fn save(profile: &CameraProfile, dir: &Path) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let path = dir.join(file_name(&profile.model, &profile.style));
    let json = serde_json::to_vec_pretty(profile).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(path)
}

type Cache = Mutex<HashMap<(String, String), Option<Arc<CameraProfile>>>>;

fn cache() -> &'static Cache {
    static CACHE: OnceLock<Cache> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// The profile for camera `model` in Creative Style `style`: from [`dir`], else built in; read once per process.
pub fn get(model: &str, style: &str) -> Option<Arc<CameraProfile>> {
    let (model, style) = (model.trim(), style.trim());
    if model.is_empty() {
        return None;
    }
    let mut cache = cache().lock().unwrap_or_else(PoisonError::into_inner);
    let key = (model.to_owned(), style.to_owned());
    if let Some(hit) = cache.get(&key) {
        return hit.clone();
    }
    let path = dir().map(|d| d.join(file_name(model, style)));
    let profile = match path {
        Some(path) if path.is_file() => match load(&path) {
            Ok(p) if p.model.trim() == model && p.style.trim() == style => Some(Arc::new(p)),
            Ok(p) => {
                log::warn!("ignoring camera profile {}: it is for {:?} {:?}, not {model:?} {style:?}", path.display(), p.model, p.style);
                None
            }
            Err(e) => {
                log::warn!("ignoring camera profile {e}");
                None
            }
        },
        _ => None,
    };
    let profile = profile.or_else(|| bundled(model, style).map(Arc::new));
    cache.insert(key, profile.clone());
    profile
}

/// A photo's colour pairs: white-balanced camera RGB and its camera JPEG's linear Rec.2020.
type Pairs = Vec<([f64; 3], [f64; 3])>;

/// What's pooled for one camera model and Creative Style.
#[derive(Default)]
struct Pooled {
    files: usize,
    pairs: Pairs,
    /// The photos shot with DRO off, and their pairs: the camera's tone curve shows in these alone.
    tone_files: usize,
    tone_pairs: Pairs,
}

/// Per camera model and Creative Style: the photos read and their pooled colour pairs.
#[derive(Default)]
pub struct Pool {
    models: HashMap<(String, String), Pooled>,
}

/// Most colour pairs kept per photo, so a few busy photos can't dominate a profile.
const PAIRS_PER_FILE: usize = 4000;

impl Pool {
    /// Add one raw file's colour pairs. `Ok(None)` when the file can't contribute (not an ARW,
    /// NEF or RAF without colour matrices, no usable camera JPEG, too little colour); else its model and style.
    pub fn add(&mut self, bytes: &[u8]) -> Result<Option<(String, String)>, String> {
        let mut raw = lightcraft_raw::decode(bytes).map_err(|e| e.to_string())?;
        // colour only: geometric lens corrections would stop the sensor proxy from binning (the pairs still follow
        // the warp the camera applied to its JPEG)
        let lens = crate::camera_preview::camera_lens_geometry(&raw, bytes);
        raw.opcodes.list3.retain(|op| !op.is_lens_correction());
        let Some(model) = raw.metadata.model.as_deref().map(str::trim).filter(|m| !m.is_empty()) else { return Ok(None) };
        let Some(pairs) = crate::camera_preview::profile_pairs(&raw, bytes, lens.as_ref()) else { return Ok(None) };
        let shot = crate::sony::settings(bytes);
        let key = (model.to_owned(), shot.creative_style.unwrap_or_default());
        let step = pairs.len().div_ceil(PAIRS_PER_FILE).max(1);
        let entry = self.models.entry(key.clone()).or_default();
        entry.files += 1;
        if shot.dro == Some(crate::sony::Dro::Off) {
            entry.tone_files += 1;
            entry.tone_pairs.extend(pairs.iter().step_by(step));
        }
        entry.pairs.extend(pairs.into_iter().step_by(step));
        Ok(Some(key))
    }

    /// Fit a profile per model and style with at least `min_files` photos.
    pub fn fit(&self, min_files: usize) -> Vec<Result<CameraProfile, String>> {
        let mut models: Vec<_> = self.models.iter().collect();
        models.sort_by(|a, b| a.0.cmp(b.0));
        models
            .into_iter()
            .filter(|(_, pooled)| pooled.files >= min_files)
            .map(|((model, style), pooled)| {
                let (matrix, hue_sat) = crate::camera_preview::fit_profile(&pooled.pairs).ok_or_else(|| format!("{model} {style}: no usable colour fit"))?;
                let mut profile = CameraProfile::new(model, style, pooled.files, pooled.pairs.len(), matrix, hue_sat);
                let own = (pooled.tone_files >= min_files).then(|| tone(&profile, &pooled.tone_pairs)).flatten();
                profile.tone_files = if own.is_some() { pooled.tone_files } else { 0 };
                profile.tone = own.or_else(crate::camera_preview::sony_dro_off_tone).map(|tone| chroma(&profile, tone, &pooled.pairs));
                Ok(profile)
            })
            .collect()
    }

    /// Add another pool's photos and pairs (pools filled on separate threads).
    pub fn merge(&mut self, other: Pool) {
        for (key, pooled) in other.models {
            let entry = self.models.entry(key).or_default();
            entry.files += pooled.files;
            entry.pairs.extend(pooled.pairs);
            entry.tone_files += pooled.tone_files;
            entry.tone_pairs.extend(pooled.tone_pairs);
        }
    }

    /// Photos read per model and style, and how many of them were shot with DRO off.
    pub fn files(&self) -> Vec<(String, String, usize, usize)> {
        let mut v: Vec<_> = self.models.iter().map(|((model, style), pooled)| (model.clone(), style.clone(), pooled.files, pooled.tone_files)).collect();
        v.sort();
        v
    }
}

/// `profile`'s colour as the per-photo fit's look, with `tone`: what the fit's own curve fitting takes.
fn look(profile: &CameraProfile, tone: CameraTone) -> crate::camera_preview::CameraLook {
    crate::camera_preview::CameraLook { matrix: profile.matrix(), tone, hue_sat: profile.hue_sat.clone() }
}

/// The camera's tone curve from pairs of photos shot without DRO, through `profile`'s colour: the per-photo fit's
/// quantile matching, over all of them at once.
fn tone(profile: &CameraProfile, pairs: &Pairs) -> Option<CameraTone> {
    use crate::camera_preview::{HueSat, luma};
    let correction = profile.hue_sat.as_ref().and_then(HueSat::new);
    let scene = |x: [f64; 3]| {
        let p = profile.matrix().apply(x);
        correction.as_ref().map_or(p, |c| c.apply(p.map(|v| v as f32)).map(f64::from)).map(|v| v.max(0.0))
    };
    let lumas = pairs.iter().map(|(x, y)| (luma(scene(*x)), luma(*y))).filter(|(x, y)| x.is_finite() && *x > 0.0 && y.is_finite()).collect();
    crate::camera_preview::fit_tone(lumas)
}

/// `tone` with the chroma curve the per-photo fit would give it, fitted to all the pairs; as it was when the curve
/// doesn't help the held-out ones.
fn chroma(profile: &CameraProfile, tone: CameraTone, pairs: &Pairs) -> CameraTone {
    crate::camera_preview::fit_chroma(pairs, &look(profile, tone)).unwrap_or(tone)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile() -> CameraProfile {
        let table = HsvTable { hue_divisions: 4, sat_divisions: 2, val_divisions: 1, data: vec![[5.0, 1.2, 1.0]; 8], srgb_value: false };
        let mut profile = CameraProfile::new("ILCE-7M4", "Vivid", 12, 3456, Mat3([[1.6, -0.5, -0.1], [-0.2, 1.3, -0.1], [-0.1, -0.2, 1.3]]), Some(table));
        profile.tone = crate::camera_preview::sony_dro_off_tone();
        profile
    }

    #[test]
    fn profiles_round_trip_and_reject_bad_files() {
        let dir = std::env::temp_dir().join(format!("omalux-camera-profiles-{}", std::process::id()));
        let path = save(&profile(), &dir).unwrap();
        assert_eq!(path.file_name().unwrap(), "ILCE-7M4-Vivid.json");
        assert!(load(&path).unwrap().tone.is_some(), "its tone curve comes back");
        assert_eq!(load(&path).unwrap(), profile());
        let bad = |p: CameraProfile| {
            let path = dir.join("bad.json");
            std::fs::write(&path, serde_json::to_vec(&p).unwrap()).unwrap();
            load(&path).is_err()
        };
        let mut p = profile();
        p.matrix[0][0] = f64::NAN;
        assert!(bad(p), "non-finite matrix");
        let mut p = profile();
        p.matrix = [[0.0; 3]; 3];
        assert!(bad(p), "singular matrix");
        let mut p = profile();
        if let Some(t) = p.hue_sat.as_mut() {
            t.data.truncate(3);
        }
        assert!(bad(p), "table data doesn't match its shape");
        let mut p = profile();
        p.version = 99;
        assert!(bad(p), "unknown version");
        std::fs::write(dir.join("junk.json"), b"{not json").unwrap();
        assert!(load(&dir.join("junk.json")).is_err());
        assert!(load(&dir.join("missing.json")).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn bundled_profiles_are_valid_and_named_after_their_model() {
        for (model, style, json) in BUNDLED {
            let p = parse(json.as_bytes(), model).unwrap();
            assert_eq!((p.model.as_str(), p.style.as_str()), (*model, *style));
            assert!(p.files >= 5 && p.hue_sat.is_some() && p.tone.is_some(), "{model} {style}: {} photos", p.files);
            assert_eq!(bundled(model, style), Some(p));
        }
        assert!(bundled("No Such Camera", "Standard").is_none());
    }

    #[test]
    fn file_names_are_safe() {
        assert_eq!(file_name("ILCE-7M4", ""), "ILCE-7M4.json");
        assert_eq!(file_name("ILCE-7M3", "Standard"), "ILCE-7M3-Standard.json");
        assert_eq!(file_name(" DSC-RX100M3 ", " Vivid "), "DSC-RX100M3-Vivid.json");
        assert_eq!(file_name("../../etc/passwd", "../x"), "______etc_passwd-___x.json");
        assert_eq!(file_name("Ω 1/2", ""), "__1_2.json");
    }

    #[test]
    fn pool_skips_files_that_cannot_contribute() {
        let mut pool = Pool::default();
        assert!(pool.add(b"not a raw file").is_err());
        assert!(pool.fit(1).is_empty());
        assert!(pool.files().is_empty());
    }
}

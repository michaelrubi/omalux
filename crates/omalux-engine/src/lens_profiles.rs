//! Lens profile corrections from lensfun's database ([`lightcraft_lensdb`]).
//!
//! When a raw carries no lens correction of its own (DNG opcodes, or a camera correction a raw reader converts,
//! [`crate::raw::embedded_lens`]), the lens named in its EXIF is looked up in a lensfun-format database: the
//! directory in `OMALUX_LENSFUN_DIR`, else the system's (`pacman -S lensfun`: `/usr/share/lensfun/version_1`;
//! CC-BY-SA, never shipped with OmaLux). The profile is converted to the same [`lightcraft_develop::EmbeddedLens`]
//! form, so "Enable Profile Corrections", its amount sliders and the GPU path treat both alike. Without a database
//! nothing changes. Sony's own corrections ([`crate::sony_lens`]) are the other [`Source`], and the default ([`DEFAULT`]).
//!
//! Lifted from Lightcraft (MIT OR Apache-2.0) at `701f78e`, `crates/engine/src/lens_profiles.rs`, with OmaLux's
//! folder and the choice of source; checked against `0da998f2`: the frame's crop factor in an in-camera crop mode
//! (`4ddbb5a3`) is taken from process version 3 on (it changes the export of an APS-C frame corrected from
//! lensfun's); Nikon's non-square pixels (`2dad18d2`) are another camera's.
use std::path::PathBuf;
use std::sync::OnceLock;

use lightcraft_develop::EmbeddedLens;
use lightcraft_lensdb::{LensDb, Shot};

/// The environment variable naming the lens database directory.
pub const ENV: &str = "OMALUX_LENSFUN_DIR";

/// Where Arch's lensfun package puts the database.
const SYSTEM: &str = "/usr/share/lensfun/version_1";

/// The configured database, read once per process; `None` when none is configured or it holds no lenses.
pub fn database() -> Option<&'static LensDb> {
    static DB: OnceLock<Option<LensDb>> = OnceLock::new();
    DB.get_or_init(|| {
        let dir = std::env::var_os(ENV).map(PathBuf::from).unwrap_or_else(|| PathBuf::from(SYSTEM));
        if !dir.is_dir() {
            return None;
        }
        match LensDb::load_dir(&dir) {
            Ok((db, errors)) => {
                for (file, e) in errors {
                    log::warn!("lens database: skipped {file}: {e}");
                }
                (!db.lenses.is_empty()).then_some(db)
            }
            Err(e) => {
                log::warn!("{e}");
                None
            }
        }
    })
    .as_ref()
}

/// The database profile for a photo with metadata `m`, for its `w × h` (unoriented, default-cropped) image,
/// re-expressed after `orientation`, as process version `process` scales it. `None` without a database, lens name,
/// focal length or matching profile.
pub fn lookup(db: &LensDb, m: &lightcraft_meta::Metadata, w: f64, h: f64, orientation: lightcraft_geom::Orientation, process: u32) -> Option<EmbeddedLens> {
    let focal = m.focal_length.filter(|f| f.is_finite() && *f > 0.0)?;
    let camera = db.find_camera(m.make.as_deref().unwrap_or_default(), m.model.as_deref().unwrap_or_default());
    let lens = db.find_lens(m.lens_make.as_deref(), m.lens_model.as_deref()?, camera)?;
    let crop = frame_crop_factor(camera.map(|c| c.crop_factor), focal, m.focal_length_35mm, process);
    let shot = Shot { focal, aperture: m.f_number, distance: None, crop_factor: crop, width: w, height: h };
    let mut corr = lens.correction(&shot)?;
    fill(&mut corr, w, h);
    Some(lightcraft_pipeline::optics::reorient_lens(&corr, orientation, w, h))
}

/// The crop factor of the recorded frame, which scales the profile's radii: the body's (`camera`), else what the
/// 35 mm equivalent focal length implies. From process version 3 (Lightcraft's `4ddbb5a3`), a body shooting a crop
/// of its sensor (APS-C / Super 35 mode, or switched there by an APS-C lens) records a larger 35 mm equivalent than
/// its own crop factor gives, and that crop wins; within 10 % of the body's, the difference is the tag's
/// whole-millimetre rounding and the body's value stands. Before 3 the body's always stood.
fn frame_crop_factor(camera: Option<f64>, focal: f64, focal_35mm: Option<f64>, process: u32) -> Option<f64> {
    let frame = focal_35mm.map(|f35| f35 / focal).filter(|c| c.is_finite() && (0.1..=20.0).contains(c));
    match (camera, frame) {
        (Some(body), Some(frame)) if process >= 3 && frame > body * 1.1 => Some(frame),
        (body, frame) => body.or(frame),
    }
}

/// Enlarge the picture under `lens`'s warp, for its `w × h` image, until it fills the frame, as Lightroom's profile
/// correction does: correcting pincushion distortion pulls the edges in, and would leave the corners empty. (Sony's
/// tables come with that zoom already: [`crate::sony_lens`].) The warp is radial about its centre, so scaling it is
/// scaling where every output pixel samples from, about that centre.
fn fill(lens: &mut EmbeddedLens, w: f64, h: f64) {
    const STEPS: usize = 64;
    let Some(warp) = lens.warp.as_mut() else { return };
    let (cx, cy) = (warp.center.x * w, warp.center.y * h);
    // how far along the ray from the centre a sample at offset d may go and stay on the picture
    let room = |d: f64, centre: f64, size: f64| if d > 1e-9 { (size - 0.5 - centre) / d } else if d < -1e-9 { (0.5 - centre) / d } else { f64::INFINITY };
    let mut zoom: f64 = 1.0;
    for i in 0..=STEPS {
        let t = i as f64 / STEPS as f64;
        for (x, y) in [(t * w, 0.0), (t * w, h), (0.0, t * h), (w, t * h)] {
            for ch in 0..3 {
                let s = lightcraft_pipeline::optics::embedded_warp(warp, lightcraft_geom::Point::new(x, y), ch, w, h);
                zoom = zoom.min(room(s.x - cx, cx, w)).min(room(s.y - cy, cy, h));
            }
        }
    }
    if zoom.is_finite() && zoom > 0.0 {
        for plane in &mut warp.planes {
            plane[..4].iter_mut().for_each(|k| *k *= zoom);
        }
    }
}

/// Where a frame's profile correction comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Source {
    /// lensfun's database ([`lookup`]): a model per lens, focal length and aperture.
    Lensfun,
    /// The tables the camera wrote into the raw ([`crate::sony_lens`]), which follow the focus distance too.
    Sony,
}

/// The source a frame's correction is taken from when it has both: Sony's, which M5's comparison with the camera's
/// own JPEG picked (the `lens` example). Its distortion lands on the JPEG's to a quarter of a pixel at every radius
/// at the JPEG's own scale, lensfun's to a third; it follows the focus distance; and it's in the raw, where a system
/// package's update can't change a render.
pub const DEFAULT: Source = Source::Sony;

/// A frame's profile correction, and what the Lens Corrections panel says of it.
#[derive(Clone, Debug, PartialEq)]
pub struct Found {
    pub lens: EmbeddedLens,
    pub source: Source,
    /// The lens, as the panel names it: lensfun's name for it, or the one the raw gives.
    pub name: String,
}

/// The profile correction for the raw `bytes`, whose header is `raw`: [`crate::raw::embedded_lens`] if the raw
/// reader found one, else `source`'s, else the other source's, as process version `process` has them.
pub fn for_raw(raw: &lightcraft_raw::RawInfo, bytes: &[u8], source: Source, process: u32) -> Option<Found> {
    let meta = lightcraft_meta::extract(bytes);
    let named = meta.lens_model.clone().unwrap_or_default();
    if let Some(lens) = crate::raw::embedded_lens(raw) {
        return Some(Found { lens, source: Source::Sony, name: named });
    }
    let lensfun = || {
        let db = database()?;
        let c = raw.crop.clipped(raw.active_area.width, raw.active_area.height);
        let (w, h) = if c.width == 0 || c.height == 0 { (raw.active_area.width, raw.active_area.height) } else { (c.width, c.height) };
        let lens = lookup(db, &meta, w as f64, h as f64, raw.orientation, process)?;
        let camera = db.find_camera(meta.make.as_deref().unwrap_or_default(), meta.model.as_deref().unwrap_or_default());
        let name = db.find_lens(meta.lens_make.as_deref(), meta.lens_model.as_deref()?, camera)?.models.first().cloned().unwrap_or_default();
        Some(Found { lens, source: Source::Lensfun, name })
    };
    let sony = || crate::sony_lens::corrections(bytes, raw).map(|lens| Found { lens, source: Source::Sony, name: named.clone() });
    match source {
        Source::Lensfun => lensfun().or_else(sony),
        Source::Sony => sony().or_else(lensfun),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lightcraft_geom::Orientation;

    const PV: u32 = crate::sidecar::PROCESS_VERSION;

    const DB: &str = r#"<lensdatabase>
      <camera><maker>Testcam</maker><model>TC-1</model><mount>T</mount><cropfactor>1.5</cropfactor></camera>
      <lens><maker>Testcam</maker><model>Testcam 18-55mm f/3.5-5.6</model><mount>T</mount><cropfactor>1.5</cropfactor>
        <calibration>
          <distortion model="poly3" focal="18" k1="-0.03" />
          <vignetting model="pa" focal="18" aperture="3.5" distance="1000" k1="-0.4" k2="0" k3="0" />
        </calibration>
      </lens></lensdatabase>"#;

    fn meta() -> lightcraft_meta::Metadata {
        lightcraft_meta::Metadata {
            make: Some("Testcam".into()),
            model: Some("TC-1".into()),
            lens_model: Some("18-55mm F3.5-5.6".into()),
            focal_length: Some(18.0),
            f_number: Some(3.5),
            ..Default::default()
        }
    }

    #[test]
    fn exif_lens_is_found_and_converted() {
        let db = LensDb::parse(DB).unwrap();
        let lens = lookup(&db, &meta(), 6000.0, 4000.0, Orientation::Normal, PV).unwrap();
        assert!(lens.warp.is_some() && lens.vignette.is_some());
        let rotated = lookup(&db, &meta(), 6000.0, 4000.0, Orientation::Rotate90, PV).unwrap();
        assert_eq!(rotated.warp.unwrap().center, lightcraft_geom::Point::new(0.5, 0.5));
        // no lens name, no focal length, an unknown lens: nothing
        assert!(lookup(&db, &lightcraft_meta::Metadata { lens_model: None, ..meta() }, 6000.0, 4000.0, Orientation::Normal, PV).is_none());
        assert!(lookup(&db, &lightcraft_meta::Metadata { focal_length: None, ..meta() }, 6000.0, 4000.0, Orientation::Normal, PV).is_none());
        let other = lightcraft_meta::Metadata { lens_model: Some("70-200mm F4".into()), ..meta() };
        assert!(lookup(&db, &other, 6000.0, 4000.0, Orientation::Normal, PV).is_none());
    }

    /// Correcting pincushion distortion pulls the edges in: the picture is enlarged until every output pixel
    /// samples the picture, and no more than that. Barrel distortion's correction needs nothing.
    #[test]
    fn a_pincushion_correction_fills_the_frame() {
        let pincushion = DB.replace("k1=\"-0.03\"", "k1=\"0.03\"");
        let (w, h) = (6000.0, 4000.0);
        let corner = |db: &str| {
            let lens = lookup(&LensDb::parse(db).unwrap(), &meta(), w, h, Orientation::Normal, PV).unwrap();
            (0..3).map(|ch| lightcraft_pipeline::optics::embedded_warp(&lens.warp.unwrap(), lightcraft_geom::Point::new(0.0, 0.0), ch, w, h)).collect::<Vec<_>>()
        };
        for s in corner(&pincushion) {
            assert!(s.x >= 0.5 && s.y >= 0.5 && (s.x < 3.0 || s.y < 3.0), "the corner samples the picture's corner: {s:?}");
        }
        let unscaled = LensDb::parse(DB).unwrap().lenses[0].correction(&Shot { focal: 18.0, aperture: Some(3.5), distance: None, crop_factor: Some(1.5), width: w, height: h }).unwrap();
        let scaled = lookup(&LensDb::parse(DB).unwrap(), &meta(), w, h, Orientation::Normal, PV).unwrap().warp.unwrap().planes[1][0];
        let zoom = scaled / unscaled.warp.unwrap().planes[1][0];
        assert!((0.999..=1.0).contains(&zoom), "barrel: as it was, but for the half pixel kept in: {zoom}");
    }

    /// A synthetic grid recorded through the profile's barrel distortion comes out of the pipeline's geometry
    /// stage with straight lines.
    #[test]
    fn pipeline_straightens_a_distorted_grid() {
        let db = LensDb::parse(DB).unwrap();
        let profile = &db.lenses[0];
        let (w, h) = (600usize, 400usize);
        let lens = lookup(&db, &meta(), w as f64, h as f64, Orientation::Normal, PV).unwrap();
        // record the grid: each sensor pixel sees the scene point whose distorted position it is
        let n = h as f64 / 2.0;
        let undistort = |x: f64, y: f64| {
            let (dx, dy) = ((x - w as f64 / 2.0) / n, (y - h as f64 / 2.0) / n);
            let rd = dx.hypot(dy);
            let (mut lo, mut hi) = (0.0, 3.0);
            for _ in 0..60 {
                let mid = (lo + hi) / 2.0;
                if mid * profile.distortion_ratio(18.0, mid) < rd {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            let s = if rd > 0.0 { lo / rd } else { 1.0 };
            (w as f64 / 2.0 + dx * s * n, h as f64 / 2.0 + dy * s * n)
        };
        // horizontal scene lines every 50 px (2 px wide, dark on light)
        let line = |v: f64| if v.rem_euclid(50.0) < 2.0 { 0.02f32 } else { 0.8 };
        let src = lightcraft_raster::Rgb32f::from_fn(w, h, |x, y| {
            let (_, sy) = undistort(x as f64 + 0.5, y as f64 + 0.5);
            [line(sy); 3]
        });
        // the dark row nearest y = 51 in each column, sub-pixel (centroid of darkness), as rendered
        let line_rows = |img: &lightcraft_raster::Rgba8| -> Vec<f64> {
            (20..w - 20)
                .step_by(10)
                .map(|x| {
                    let (mut sw, mut swy) = (0.0, 0.0);
                    for y in 35..70 {
                        let d = 255.0 - img.get(x, y)[1] as f64;
                        if d > 60.0 {
                            sw += d;
                            swy += d * (y as f64 + 0.5);
                        }
                    }
                    swy / sw.max(1e-9)
                })
                .collect()
        };
        let spread = |rows: &[f64]| rows.iter().cloned().fold(f64::MIN, f64::max) - rows.iter().cloned().fold(f64::MAX, f64::min);
        let req = lightcraft_pipeline::RenderRequest::fit(w, h);
        let mut s = lightcraft_develop::DevelopSettings::default();
        let info = lightcraft_pipeline::SourceInfo { lens: Some(lens), ..Default::default() };
        let before = spread(&line_rows(&lightcraft_pipeline::render(&src, &info, &s, &req).image));
        s.optics.lens_profile = true;
        let after = spread(&line_rows(&lightcraft_pipeline::render(&src, &info, &s, &req).image));
        assert!(before > 2.0, "the recorded line is curved: {before} px");
        // 8-bit output and a 2 px line: the centroid measurement itself is good to about a pixel
        assert!(after < 1.0 && after < before / 8.0, "corrected line bent by {after} px (uncorrected {before})");
    }

    /// A full-frame body in APS-C / Super 35 mode (an ILCE-1M2 at 70 mm records 105 mm equivalent) frames only the
    /// middle of the image circle: from process version 3 the profile is applied at the frame's crop factor, not
    /// the body's, so the crop's corners don't get the full frame's corner correction. (Lightcraft's `4ddbb5a3`.)
    #[test]
    fn in_camera_crop_mode_uses_the_frames_crop_factor() {
        let db = LensDb::parse(
            r#"<lensdatabase>
              <camera><maker>Sony</maker><model>ILCE-1M2</model><mount>Sony E</mount><cropfactor>1</cropfactor></camera>
              <lens><maker>Sigma</maker><model>24-70mm F2.8 DG DN | Art 019</model><mount>Sony E</mount><cropfactor>1.0</cropfactor>
                <calibration>
                  <distortion model="ptlens" focal="70" a="0.0019947" b="0.0035263" c="0.0005016" />
                  <vignetting model="pa" focal="70" aperture="2.8" distance="1000" k1="-0.4" k2="0" k3="0" />
                </calibration>
              </lens></lensdatabase>"#,
        )
        .unwrap();
        let m = |f35: Option<f64>| lightcraft_meta::Metadata {
            make: Some("SONY".into()),
            model: Some("ILCE-1M2".into()),
            lens_model: Some("24-70mm F2.8 DG DN | Art 019".into()),
            focal_length: Some(70.0),
            focal_length_35mm: f35,
            f_number: Some(2.8),
            ..Default::default()
        };
        let at = |crop: f64, w: f64, h: f64| {
            let shot = Shot { focal: 70.0, aperture: Some(2.8), distance: None, crop_factor: Some(crop), width: w, height: h };
            let mut corr = db.lenses[0].correction(&shot).unwrap();
            fill(&mut corr, w, h);
            corr
        };
        let aps_c = lookup(&db, &m(Some(105.0)), 5616.0, 3744.0, Orientation::Normal, 3).unwrap();
        assert_eq!(aps_c, at(1.5, 5616.0, 3744.0));
        assert_eq!(lookup(&db, &m(Some(105.0)), 5616.0, 3744.0, Orientation::Normal, 2).unwrap(), at(1.0, 5616.0, 3744.0), "process version 2 keeps the body's");
        // full frame, a 35 mm equivalent rounded off by a millimetre, or none at all: the body's crop factor
        for f35 in [Some(70.0), Some(71.0), None] {
            assert_eq!(lookup(&db, &m(f35), 8640.0, 5760.0, Orientation::Normal, 3).unwrap(), at(1.0, 8640.0, 5760.0), "{f35:?}");
        }
        assert_eq!(frame_crop_factor(None, 70.0, Some(105.0), 3), Some(1.5), "no body in the database: the 35 mm equivalent's");
        assert_eq!(frame_crop_factor(Some(1.0), 0.0, Some(105.0), 3), Some(1.0), "a hostile focal length is ignored");
    }
}

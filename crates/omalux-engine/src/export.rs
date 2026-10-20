//! Export: what Omapix opens. 16-bit TIFFs in Linear ProPhoto RGB with
//! their ICC profile, as Michael's darktable exports were (decided
//! 2026-10-12; see "Pipeline" in DESIGN.md).
//!
//! Lightcraft's pipeline renders linear output as 32-bit float and its
//! 16-bit output display-encoded; this takes the float render (after the
//! output's shoulder and gamut mapping have brought everything into 0..1)
//! down to 16-bit integers, linear. 16-bit linear has four times the levels
//! of the A7 III's 14 bits in every stop.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use lightcraft_codecs::{EncodeImage, EncodeMeta, Samples, TiffCompression, Trc, encode, icc};
use lightcraft_develop::DevelopSettings;
use lightcraft_pipeline::{DeepSamples, OutputDepth, OutputSpace, RenderRequest, SourceInfo};
use lightcraft_raster::Rgb32f;

use crate::look::{self, Camera, Profile};

/// A frame rendered for export: linear ProPhoto RGB, 16 bits a channel,
/// interleaved.
#[derive(Clone, Debug, PartialEq)]
pub struct Linear16 {
    pub width: usize,
    pub height: usize,
    pub samples: Vec<u16>,
}

/// The size `src` exports at under `settings`: the crop at the source's
/// own resolution.
pub fn full_size(src: &Rgb32f, settings: &DevelopSettings) -> (usize, usize) {
    let (w, h) = lightcraft_pipeline::native_output_size(src.width, src.height, settings);
    ((w.round() as usize).max(1), (h.round() as usize).max(1))
}

/// Render `src` at full size for export, on the CPU pipeline.
pub fn render(src: &Rgb32f, info: &SourceInfo, camera: &Camera, settings: &DevelopSettings) -> Result<Linear16, String> {
    let (w, h) = full_size(src, settings);
    let request = RenderRequest { space: OutputSpace::ProPhoto, depth: OutputDepth::F32Linear, ..RenderRequest::fit(w, h) };
    let (settings, info) = look::effective(settings, info, camera);
    let rendered = lightcraft_pipeline::render(src, &info, &settings, &request);
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

/// What an export did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Exported {
    /// Wrote this TIFF, rendered from the edit `record` names: what to
    /// keep in the sidecar.
    Written { tiff: PathBuf, record: crate::sidecar::Exported },
    /// This TIFF is the frame's already: of its edit as it stands, or one
    /// OmaLux didn't make and so never writes over.
    There(PathBuf),
}

impl Exported {
    pub fn tiff(&self) -> &Path {
        match self {
            Exported::Written { tiff, .. } | Exported::There(tiff) => tiff,
        }
    }
}

/// When a file was last written, in seconds since 1970.
fn modified(path: &Path) -> Option<u64> {
    let time = fs::metadata(path).and_then(|m| m.modified()).ok()?;
    time.duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_secs())
}

/// The first of `<stem>-2.tif`, `<stem>-3.tif`… in `to` that isn't there.
fn beside(to: &Path, stem: &str) -> PathBuf {
    (2..).map(|n| to.join(format!("{stem}-{n}.tif"))).find(|path| !path.exists()).unwrap_or_else(|| to.join(format!("{stem}-new.tif")))
}

/// Export `raw` to `to`, `<to>/<stem>.tif` the first time.
///
/// Decodes at full size, takes `edit` or the edit in its sidecar or a
/// fresh raw's as-shot settings, under the edit's process version, renders to 16-bit Linear ProPhoto RGB, and
/// writes the TIFF in one step via a temporary file.
///
/// `last` is the frame's last export, from its sidecar. A TIFF already
/// there of the edit as it stands is left as it is. One of an edit since
/// is written over if nothing has saved into it since it was exported, and
/// otherwise a new one is written beside it (`<stem>-2.tif`), so Omapix's
/// retouching is never lost. A TIFF OmaLux has no record of is never
/// written over.
///
/// Returns an error if the raw cannot be read or decoded, if its sidecar
/// has an edit with a newer process version, or if rendering/writing fails.
pub fn export(raw: &Path, to: &Path, edit: Option<&crate::sidecar::Edit>, last: Option<&crate::sidecar::Exported>) -> Result<Exported, String> {
    let stem = raw.file_stem().ok_or_else(|| format!("{}: not a file", raw.display()))?.to_string_lossy().into_owned();
    let first = to.join(format!("{stem}.tif"));
    let there = |path: &Path| path.try_exists().map_err(|e| format!("{}: {e}", path.display()));
    if last.is_none() && there(&first)? {
        return Ok(Exported::There(first));
    }

    let saved = match edit {
        Some(edit) => Some(edit.clone()),
        None => {
            let xmp = crate::sidecar::read(raw).map_err(|e| format!("{}: {e}", raw.display()))?;
            xmp.as_deref().map(crate::sidecar::edit).transpose().map_err(|e| format!("{}: {e}", raw.display()))?.flatten()
        }
    };
    let process = saved.as_ref().map_or(crate::sidecar::PROCESS_VERSION, |edit| edit.process);
    let saved = saved.map(|edit| edit.settings);
    let bytes = fs::read(raw).map_err(|e| format!("{}: {e}", raw.display()))?;
    let profile = saved.as_ref().map_or(Profile::Camera, Profile::of);
    let crate::raw::Loaded { image: full, info, camera, .. } = crate::raw::load(&bytes, usize::MAX, profile, process)?;
    let fresh = look::fresh(info.as_shot_temp, info.as_shot_tint);
    let dev_settings = saved.unwrap_or_else(|| fresh.clone());
    // An edit that's a fresh raw's is named as having none.
    let key = crate::sidecar::export_key(Some(&dev_settings).filter(|s| **s != fresh));

    let target = match last {
        None => first,
        Some(last) => {
            let previous = to.join(&last.file);
            if !there(&previous)? {
                if there(&first)? { beside(to, &stem) } else { first }
            } else if last.settings == key {
                return Ok(Exported::There(previous));
            } else if modified(&previous) == Some(last.modified) {
                previous
            } else {
                beside(to, &stem)
            }
        }
    };

    let image = render(&full, &info, &camera, &dev_settings)?;
    let tiff_bytes = tiff(&image)?;

    fs::create_dir_all(to).map_err(|e| format!("{}: {e}", to.display()))?;
    let mut tmp_name = target.clone().into_os_string();
    tmp_name.push(".omalux-tmp");
    let temporary = PathBuf::from(tmp_name);
    let written = (|| {
        let mut file = fs::File::create(&temporary)?;
        file.write_all(&tiff_bytes)?;
        file.sync_all()?;
        fs::rename(&temporary, &target)
    })();
    if written.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    written.map_err(|e| format!("{}: {e}", target.display()))?;

    let file = target.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let record = crate::sidecar::Exported { file, settings: key, modified: modified(&target).unwrap_or(0) };
    Ok(Exported::Written { tiff: target, record })
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
        let image = render(&src, &info, &Camera::default(), &DevelopSettings::default()).unwrap();
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

    #[test]
    fn a_crop_and_a_quarter_turn_export_at_their_own_size() {
        let src = Rgb32f::filled(60, 40, [0.2; 3]);
        let info = SourceInfo::default();
        let mut settings = DevelopSettings::default();
        crate::crop::set_aspect(&mut settings, (60.0, 40.0), crate::crop::Aspect::Ratio(1, 1));
        let square = render(&src, &info, &Camera::default(), &settings).unwrap();
        assert_eq!((square.width, square.height), (40, 40));
        crate::crop::reset(&mut settings);
        crate::crop::turn(&mut settings, true);
        let turned = render(&src, &info, &Camera::default(), &settings).unwrap();
        assert_eq!((turned.width, turned.height), (40, 60));
    }

    /// A folder of its own for a test, removed when dropped.
    struct Folder(PathBuf);

    impl Folder {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!("omalux-export-{}-{name}", std::process::id()));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn export_raw_to_folder() {
        for (name, bytes) in crate::raw::tests::a7m3_corpus() {
            let folder = Folder::new(&name.replace('.', "-"));
            let raw_path = folder.0.join(&name);
            fs::write(&raw_path, &bytes).unwrap();

            let out = folder.0.join("omalux");
            let stem = raw_path.file_stem().unwrap().to_string_lossy();
            let target = out.join(format!("{stem}.tif"));

            // 1. The TIFF is where it should be, 16-bit with the profile.
            let Exported::Written { tiff, record } = export(&raw_path, &out, None, None).unwrap() else { panic!("not written") };
            assert_eq!((tiff, record.file.as_str(), record.settings.as_str()), (target.clone(), format!("{stem}.tif").as_str(), "fresh"));
            assert!(target.exists());

            let tiff_bytes = fs::read(&target).unwrap();
            let mut decoder = tiff::decoder::Decoder::new(std::io::Cursor::new(&tiff_bytes)).unwrap();
            assert_eq!(decoder.colortype().unwrap(), tiff::ColorType::RGB(16));
            let icc = decoder.find_tag(tiff::tags::Tag::IccProfile).unwrap().unwrap().into_u8_vec().unwrap();
            assert_eq!(icc, profile());
            let tiff::decoder::DecodingResult::U16(default_samples) = decoder.read_image().unwrap() else { panic!("not 16-bit") };
            assert!(!default_samples.is_empty());

            // 2. Of the edit as it stands, it's left as it is.
            assert_eq!(export(&raw_path, &out, None, Some(&record)).unwrap(), Exported::There(target.clone()));
            // Edited since, and nothing saved into it: written over.
            let mut since = look::fresh(6500.0, 0.0);
            since.light.exposure = 1.0;
            let Exported::Written { tiff, record: again } = export(&raw_path, &out, Some(&crate::sidecar::Edit::new(since.clone())), Some(&record)).unwrap() else { panic!("not written") };
            assert_eq!(tiff, target);
            assert_ne!(again.settings, record.settings);
            // Saved into since (Omapix's retouching): a new one beside it.
            let later = std::time::SystemTime::now() + std::time::Duration::from_secs(10);
            fs::File::options().append(true).open(&target).unwrap().set_modified(later).unwrap();
            let retouched = fs::read(&target).unwrap();
            since.light.exposure = 0.5;
            let Exported::Written { tiff, .. } = export(&raw_path, &out, Some(&crate::sidecar::Edit::new(since.clone())), Some(&again)).unwrap() else { panic!("not written") };
            assert_eq!(tiff, out.join(format!("{stem}-2.tif")));
            assert_eq!(fs::read(&target).unwrap(), retouched, "the retouched one is left as it was");
            // One OmaLux has no record of is never written over.
            let dummy = b"existing untouched tiff data";
            fs::write(&target, dummy).unwrap();
            assert_eq!(export(&raw_path, &out, None, None).unwrap(), Exported::There(target.clone()));
            assert_eq!(fs::read(&target).unwrap(), dummy);

            // 3. The sidecar's edit is what's rendered.
            let mut edited_settings = look::fresh(6500.0, 0.0);
            edited_settings.light.exposure = -2.0;
            crate::sidecar::update(&raw_path, |xmp| crate::sidecar::with_edit(xmp, &crate::sidecar::Edit::new(edited_settings.clone()))).unwrap();

            let out_edit = folder.0.join("omalux-edit");
            assert!(matches!(export(&raw_path, &out_edit, None, None).unwrap(), Exported::Written { .. }));
            let target_edit = out_edit.join(format!("{stem}.tif"));
            let mut decoder = tiff::decoder::Decoder::new(std::io::Cursor::new(fs::read(&target_edit).unwrap())).unwrap();
            let tiff::decoder::DecodingResult::U16(edit_samples) = decoder.read_image().unwrap() else { panic!("not 16-bit") };
            assert_ne!(edit_samples, default_samples);

            let crate::raw::Loaded { image: full, info, camera, .. } = crate::raw::load(&bytes, usize::MAX, Profile::Camera, crate::sidecar::PROCESS_VERSION).unwrap();
            let expected_image = render(&full, &info, &camera, &edited_settings).unwrap();
            assert_eq!(edit_samples, expected_image.samples);

            // 4. A newer process version is refused.
            crate::sidecar::update(&raw_path, |xmp| {
                let with = crate::sidecar::with_edit(xmp, &crate::sidecar::Edit::new(edited_settings.clone()))?;
                Ok(with.replace(&format!(">{}</omalux:processVersion>", crate::sidecar::PROCESS_VERSION), ">99</omalux:processVersion>"))
            }).unwrap();

            let out_newer = folder.0.join("omalux-newer");
            let err = export(&raw_path, &out_newer, None, None).unwrap_err();
            assert!(err.contains("newer OmaLux"), "unexpected error: {err}");
            assert!(!out_newer.join(format!("{stem}.tif")).exists());
        }
    }
}

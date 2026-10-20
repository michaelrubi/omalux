//! A raw, decoded and developed to what Lightcraft's pipeline takes: linear
//! Rec. 2020, highlights rebuilt, white-balanced as shot, through the
//! camera match ([`crate::camera_preview`]), oriented, no larger than asked.
//!
//! Lifted from Lightcraft (MIT OR Apache-2.0) at `701f78e`,
//! `crates/engine/src/files.rs`: its raw path only, without what OmaLux
//! doesn't read (DNG semantic masks and gain table maps, other images) or
//! doesn't have yet (the denoised twin, which M6 brings back). Brought up
//! to `0da998f2`: the colour step in lanes (`6467d543`) and no copy of a
//! full-size source to fit it (`0d13a7d5`). The smallest embedded JPEG
//! that covers a preview (`14e0d128`) isn't taken: an ILCE-7M3's ARW holds
//! only its 1616 px preview beside a 160 px thumbnail, so it's always the
//! one. The camera look through the lens warp (`c854e172`) is taken from
//! process version 3 on: see [`load`].

use std::sync::Arc;

use crate::camera_preview::CameraLook;
use crate::look::{Camera, Profile};
use lightcraft_color::cct::xy_to_temp_tint;
use lightcraft_geom::Orientation;
use lightcraft_pipeline::SourceInfo;
use lightcraft_raster::Rgb32f;
use lightcraft_raster::resample::{Filter, fit};

/// Lens corrections embedded in a raw's `OpcodeList3` (`WarpRectilinear`, `WarpRectilinear2`, `FixVignetteRadial`: a DNG's own, or the
/// raw reader's equivalent of the camera's correction, e.g. Panasonic / Leica RW2 distortion), re-expressed for the
/// default-cropped, EXIF-oriented image.
pub fn embedded_lens(raw: &lightcraft_raw::RawInfo) -> Option<lightcraft_develop::EmbeddedLens> {
    use lightcraft_develop::{EmbeddedLens, EmbeddedVignette, EmbeddedWarp, EmbeddedWarpStage, WARP_STAGES};
    use lightcraft_geom::Point;
    use lightcraft_raw::Opcode;
    let (aw, ah) = (raw.active_area.width as f64, raw.active_area.height as f64);
    if aw < 2.0 || ah < 2.0 {
        return None;
    }
    let c = raw.crop.clipped(raw.active_area.width, raw.active_area.height);
    let (cx0, cy0, cw, ch) =
        if c.width == 0 || c.height == 0 { (0.0, 0.0, aw, ah) } else { (c.x as f64, c.y as f64, c.width as f64, c.height as f64) };
    let long = cw.max(ch);
    // opcode centres are relative to the (uncropped) active area, in pixel-index units
    let centre = |rel: [f64; 2]| -> (Point, f64) {
        let (px, py) = (rel[0] * (aw - 1.0), rel[1] * (ah - 1.0));
        let m = [(0.0, 0.0), (aw - 1.0, 0.0), (0.0, ah - 1.0), (aw - 1.0, ah - 1.0)]
            .iter()
            .map(|&(x, y)| (x - px).hypot(y - py))
            .fold(0.0, f64::max)
            .max(1e-9);
        (Point::new((px + 0.5 - cx0) / cw, (py + 0.5 - cy0) / ch), m / long)
    };
    let mut lens = EmbeddedLens::default();
    // opcodes in list order, minus the ones the DNG 1.6 skip rule drops (a WarpRectilinear that follows an optional
    // WarpRectilinear2)
    let ops: Vec<&Opcode> = lightcraft_raw::opcodes::applied_ops(&raw.opcodes.list3).collect();
    // A DNG with a WarpRectilinear2 keeps every warp opcode as a chain of stages (each warps the one before's output);
    // without one the first WarpRectilinear is the lens warp, as it always was.
    let chain = ops.iter().any(|op| matches!(op, Opcode::WarpRectilinear2 { .. }));
    let mut stages = Vec::new();
    for op in ops {
        match op {
            Opcode::WarpRectilinear2 { planes, center, reciprocal, .. } if !planes.is_empty() => {
                let (center, radius) = centre(*center);
                let p = |i: usize| planes[i.min(planes.len() - 1)];
                stages.push(EmbeddedWarpStage { planes: [p(0), p(1), p(2)], center, radius, reciprocal: *reciprocal });
            }
            Opcode::WarpRectilinear { planes, center } if chain && !planes.is_empty() => {
                // WarpRectilinear is the even-power case: kr0 + kr1 r² + kr2 r⁴ + kr3 r⁶, no radius limit
                let (center, radius) = centre(*center);
                let p = |i: usize| {
                    let k = planes[i.min(planes.len() - 1)];
                    let mut s = [0.0; lightcraft_raw::opcodes::WR2_LEN];
                    s[0] = k[0];
                    s[2] = k[1];
                    s[4] = k[2];
                    s[6] = k[3];
                    s[15] = k[4];
                    s[16] = k[5];
                    s[18] = 1e6;
                    s
                };
                stages.push(EmbeddedWarpStage { planes: [p(0), p(1), p(2)], center, radius, reciprocal: false });
            }
            Opcode::WarpRectilinear { planes, center } if !chain && !planes.is_empty() && lens.warp.is_none() => {
                let (center, radius) = centre(*center);
                let p = |i: usize| planes[i.min(planes.len() - 1)];
                lens.warp = Some(EmbeddedWarp { planes: [p(0), p(1), p(2)], center, radius });
            }
            Opcode::FixVignetteRadial { k, center } if lens.vignette.is_none() => {
                let (center, radius) = centre(*center);
                lens.vignette = Some(EmbeddedVignette { k: *k, center, radius });
            }
            _ => {}
        }
    }
    // (more stages than a lens keeps: the first ones; real files carry two)
    for (slot, st) in lens.stages.iter_mut().zip(stages.into_iter().take(WARP_STAGES)) {
        *slot = Some(st);
    }
    if lens.warp.is_none() && lens.vignette.is_none() && !lens.has_stages() {
        return None;
    }
    Some(lightcraft_pipeline::optics::reorient_lens(&lens, raw.orientation, cw, ch))
}

/// Why a raw that failed to decode should show its embedded preview instead (`Ok`), or the error
/// to report (`Err`). Variants we can't decode yet always fall back. So does any other failure of a
/// recognised raw (damaged or oversized raw data, unreadable TIFF structure): its preview may still be
/// intact. Only a file that is not a raw reports an error.
fn preview_reason(e: lightcraft_raw::RawError) -> Result<String, String> {
    use lightcraft_raw::RawError;
    match e {
        RawError::Unsupported(why) => Ok(why),
        RawError::NotRaw => Err(RawError::NotRaw.to_string()),
        e => Ok(e.to_string()),
    }
}

/// Sensor clip level (normalised) for highlight reconstruction.
const HIGHLIGHT_CLIP: f32 = 0.99;

/// How a raw's camera RGB becomes the linear Rec.2020 the pipeline takes.
pub(crate) struct CameraColourModel<'a> {
    /// White-balance multipliers (as shot).
    pub wb: [f32; 3],
    /// White-balanced camera RGB → linear Rec.2020: the camera matrix, or a look fitted to the camera's JPEG on top.
    pub matrix: lightcraft_color::Mat3,
    /// Baseline exposure, as a gain.
    pub gain: f32,
    /// A DNG profile's hue/saturation map and look table.
    pub tables: Option<&'a lightcraft_raw::profile::ProfileTables>,
    /// A fitted camera look's hue/saturation table.
    pub hue_sat: Option<&'a crate::camera_preview::HueSat<'a>>,
}

impl CameraColourModel<'_> {
    /// Rebuild sensor-clipped channels; fully clipped highlights become [`clipped_white`] of the matrix, so they
    /// come out of [`Self::apply`] neutral.
    pub fn rebuild_highlights(&self, img: &mut Rgb32f) {
        lightcraft_raw::highlight::reconstruct_with(img, self.wb, HIGHLIGHT_CLIP, clipped_white(&self.matrix));
    }

    /// [`Self::rebuild_highlights`] for a binned image whose clipped channels `mask` tells
    /// ([`lightcraft_raw::RawImage::develop_binned_masked`]).
    pub fn rebuild_highlights_masked(&self, img: &mut Rgb32f, mask: &[u8]) {
        lightcraft_raw::highlight::reconstruct_masked(img, self.wb, HIGHLIGHT_CLIP, clipped_white(&self.matrix), mask);
    }

    /// White balance, matrix, tables and baseline exposure, in place.
    pub fn apply(&self, img: &mut Rgb32f) {
        let (wb, m, gain) = (self.wb, self.matrix.to_f32(), self.gain);
        if let (None, Some(hue_sat)) = (self.tables, self.hue_sat) {
            // a camera look with a table: `pixel`, `LANES` pixels at a time so the table step vectorises (same bits)
            const LANES: usize = 16;
            lightcraft_raster::par_rows(&mut img.data, img.width, |_, row| {
                for chunk in row.chunks_mut(LANES) {
                    let mut rgb = [[0.0f32; LANES]; 3];
                    for (l, p) in chunk.iter().enumerate() {
                        let c = [p[0] * wb[0], p[1] * wb[1], p[2] * wb[2]];
                        for (i, rgb) in rgb.iter_mut().enumerate() {
                            rgb[l] = (m[i][0] * c[0] + m[i][1] * c[1] + m[i][2] * c[2]) * gain;
                        }
                    }
                    let out = hue_sat.apply_lanes(rgb);
                    for (l, p) in chunk.iter_mut().enumerate() {
                        *p = [out[0][l], out[1][l], out[2][l]].map(|v| v.max(0.0));
                    }
                }
            });
            return;
        }
        img.map_in_place(|p| self.pixel(&m, p));
    }

    /// [`Self::apply`] to one pixel; `m` is the matrix as `f32`.
    fn pixel(&self, m: &[[f32; 3]; 3], p: [f32; 3]) -> [f32; 3] {
        let (wb, gain) = (self.wb, self.gain);
        let c = [p[0] * wb[0], p[1] * wb[1], p[2] * wb[2]];
        let rgb = [
            m[0][0] * c[0] + m[0][1] * c[1] + m[0][2] * c[2],
            m[1][0] * c[0] + m[1][1] * c[1] + m[1][2] * c[2],
            m[2][0] * c[0] + m[2][1] * c[1] + m[2][2] * c[2],
        ];
        let rgb = match self.tables {
            Some(tables) => tables.apply(rgb, gain),
            None => rgb.map(|v| v * gain),
        };
        self.hue_sat.map_or(rgb, |h| h.apply(rgb)).map(|v| v.max(0.0))
    }
}

/// The white-balanced camera RGB that `model` (white-balanced camera RGB → linear Rec.2020) renders
/// neutral, as bright as it renders `[1, 1, 1]`: what fully clipped highlights are rebuilt to, so they come out
/// neutral whatever the colour model does to the camera's own neutral (a look fitted to the camera's JPEG or a
/// camera profile often tints it by a few per cent, which any darkening of the highlights would show).
/// `[1, 1, 1]` (the camera's own neutral, as before this existed) when the model can't be trusted with it: not
/// invertible or nearly so, an answer far from the camera's neutral (a sane colour model moves white by a few per
/// cent), or one that isn't neutral through the `f32` matrix the decode applies.
pub(crate) fn clipped_white(model: &lightcraft_color::Mat3) -> [f32; 3] {
    const CAMERA: [f32; 3] = [1.0; 3];
    /// Largest condition number (max row sum norm) of a model trusted for this.
    const CONDITION: f64 = 1e3;
    /// Largest factor between the answer and the camera's neutral, per channel.
    const SPREAD: f64 = 4.0;
    let norm = |m: &lightcraft_color::Mat3| m.0.iter().map(|row| row.iter().map(|v| v.abs()).sum::<f64>()).fold(0.0, f64::max);
    let Some(inverse) = model.inverse() else { return CAMERA };
    let condition = norm(model) * norm(&inverse);
    if !condition.is_finite() || condition > CONDITION {
        return CAMERA;
    }
    let luma = |v: [f64; 3]| {
        let c = model.apply(v);
        (0..3).map(|i| c[i] * f64::from(lightcraft_color::LUMA_2020[i])).sum::<f64>()
    };
    let n = inverse.apply([1.0; 3]);
    let (target, own) = (luma([1.0; 3]), luma(n));
    if !(target > 0.0 && own > 0.0 && target.is_finite() && own.is_finite()) {
        return CAMERA;
    }
    let white = n.map(|v| v * target / own);
    if !white.iter().all(|v| v.is_finite() && (1.0 / SPREAD..=SPREAD).contains(v)) {
        return CAMERA;
    }
    let white = white.map(|v| v as f32);
    // neutral through the matrix as the decode applies it
    let m = model.to_f32();
    let out: [f32; 3] = std::array::from_fn(|i| m[i][0] * white[0] + m[i][1] * white[1] + m[i][2] * white[2]);
    let (lo, hi) = (out[0].min(out[1]).min(out[2]), out[0].max(out[1]).max(out[2]));
    if lo > 0.0 && hi.is_finite() && hi - lo <= 1e-3 * hi { white } else { CAMERA }
}

/// The largest block size to bin a raw's mosaic by for a source of at most `max_edge` pixels:
/// the binned image must keep at least 90 % of `max_edge`. `None` = demosaic at full size.
pub fn bin_factor(raw: &lightcraft_raw::RawImage, max_edge: usize) -> Option<usize> {
    let c = raw.crop.clipped(raw.active_area.width, raw.active_area.height);
    let long = if c.width > 1 && c.height > 1 { c.width.max(c.height) } else { raw.active_area.width.max(raw.active_area.height) };
    let need = (max_edge.saturating_mul(9) / 10).max(1);
    let need3 = (max_edge.saturating_mul(3) / 4).max(1);
    [8usize, 6, 4, 3, 2].into_iter().find(|&k| raw.can_bin(k) && (long / k >= need || (k == 3 && !raw.can_bin(2) && long / k >= need3)))
}

/// How long each step of a [`load`] took, in milliseconds.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Timings {
    /// Reading the raw data (unpacking, decompressing).
    pub decode: f64,
    /// The camera match: fitting the look to the camera's JPEG.
    pub look: f64,
    /// Demosaicing (or binning, for small sizes).
    pub demosaic: f64,
    /// Highlights, colour, fitting to size and orienting.
    pub finish: f64,
}

/// A raw as the pipeline takes it.
#[derive(Clone, Debug)]
pub struct Loaded {
    /// Linear Rec.2020, no larger than asked, oriented.
    pub image: Rgb32f,
    /// The facts the pipeline interprets settings against.
    pub info: SourceInfo,
    /// What the camera did to its own JPEG ([`crate::look`]).
    pub camera: Camera,
    pub timings: Timings,
}

/// Decode a raw into a linear Rec.2020 image no larger than `max_edge`, oriented, with the look `profile` starts
/// from: fitted to its own JPEG ([`Profile::Fitted`]), or its camera's fixed one (Camera match and Flat, which
/// share a development), as process version `process` develops it. A raw that can't be decoded yet shows its
/// embedded JPEG.
///
/// Whatever is rendered from it goes through [`crate::look::effective`] first.
///
/// From process version 3, the look fitted to a frame's JPEG and DRO's lift are measured through the lens warp the
/// camera applied to its JPEG ([`crate::camera_preview::camera_lens_geometry`]): they follow an FE 35 frame's JPEG
/// better. Under 1 and 2 they're measured on the raw as it is, so edits made then render as they did.
///
/// Runs on a rayon worker: its many short parallel loops then start on the worker's own queue
/// instead of each one waking the pool from outside and waiting for it.
pub fn load(bytes: &[u8], max_edge: usize, profile: Profile, process: u32) -> Result<Loaded, String> {
    rayon::scope(|_| load_now(bytes, max_edge, profile, process))
}

fn load_now(bytes: &[u8], max_edge: usize, profile: Profile, process: u32) -> Result<Loaded, String> {
    let ms = |t: web_time::Instant| t.elapsed().as_secs_f64() * 1e3;
    let mut timings = Timings::default();
    if lightcraft_raw::probe(bytes).is_none() {
        return Err("not a raw".into());
    }
    let t = web_time::Instant::now();
    let mut raw = match lightcraft_raw::decode(bytes).map_err(preview_reason) {
        Ok(r) => r,
        Err(Ok(why)) => {
            // show the camera's embedded JPEG (rendered, not raw) until the variant is supported
            return load_embedded_preview(bytes, max_edge)
                .map(|(image, info)| Loaded { image, info, camera: Camera::default(), timings })
                .ok_or(format!("unsupported raw ({why}) without an embedded preview"));
        }
        Err(Err(e)) => return Err(e),
    };
    timings.decode = ms(t);
    // Embedded lens corrections are applied by the pipeline ("Enable Profile Corrections"), not baked in
    // (removed before the camera look's binned sensor proxy too, which needs an empty `OpcodeList3`).
    let info = raw.info();
    let lens = crate::lens_profiles::for_raw(&info, bytes, crate::lens_profiles::DEFAULT, process);
    // The full-size frame's long edge: noise reduction's radii are in its pixels, so a smaller
    // development looks like the full-size one scaled down (Lightcraft's catalogue gives it the same).
    let native_long = Some(info.developed_size()).map(|(w, h)| w.max(h)).filter(|&n| n > 0);
    // The warp the camera applied to its JPEG, read before the raw's own lens corrections go (see above).
    let warp = (process >= 3).then(|| crate::camera_preview::camera_lens_geometry(&raw, bytes)).flatten();
    raw.opcodes.list3.retain(|op| !op.is_lens_correction());
    // Which scale white balance is edited on, and the as-shot white on it, from the file's own colour tags or
    // the camera's measured matrices
    let scale = WbScaleOf::new(raw.format, &raw.color, raw.wb_multipliers, &raw.metadata);
    let t = web_time::Instant::now();
    // The look: the camera's fixed profile for its Creative Style, or where there's none (and for an edit made
    // before there were any) the one fitted to this frame's own JPEG.
    let shot = crate::sony::settings(bytes);
    let fixed = if profile.fitted() { None } else { fixed_look(&raw, shot.creative_style.as_deref().unwrap_or_default(), process) };
    let camera = Camera {
        style: shot.creative_style,
        dro: shot.dro,
        lift: 0.0,
        fixed: fixed.is_some(),
        lens: lens.as_ref().map(|found| found.name.clone()),
        capture: crate::look::Capture { size: Some(info.developed_size()), ..crate::look::Capture::of(&raw.metadata) },
    };
    // (through the lens warp from process version 3: see above)
    let (xy, transform, camera_look) = fixed.unwrap_or_else(|| crate::camera_preview::starting_colour(&mut raw, bytes, warp.as_ref()));
    // What DRO did to the camera's JPEG that the raw doesn't have, for Camera match to put back.
    let lifted = !profile.fitted() && camera.dro.is_some_and(|dro| dro != crate::sony::Dro::Off);
    let lift = camera_look.as_ref().filter(|_| lifted).and_then(|look| crate::dro::measure(&raw, bytes, &transform, look, warp.as_ref()));
    let camera = Camera { lift: lift.unwrap_or(0.0), ..camera };
    timings.look = ms(t);
    // Previews bin the mosaic straight to (about) the size they need; only larger levels (exports, 1:1)
    // demosaic the whole sensor.
    let t = web_time::Instant::now();
    let bin = bin_factor(&raw, max_edge);
    // (binned: block means, and which colours had a clipped sample in each block)
    let binned = match bin {
        Some(k) => raw.develop_binned_masked(k, HIGHLIGHT_CLIP).map_err(|e| e.to_string())?,
        None => None,
    };
    let (mut img, clip_mask) = match binned {
        Some((img, mask)) => (img, Some(mask)),
        None => {
            let method = if max_edge <= 600 { lightcraft_raw::Method::Bilinear } else { lightcraft_raw::Method::Ahd };
            (raw.develop(method).map_err(|e| e.to_string())?, None)
        }
    };
    timings.demosaic = ms(t);
    let t = web_time::Instant::now();
    // the samples aren't needed any more (the colour model below reads only the tags)
    raw.data = lightcraft_raw::RawData::U16(Vec::new());
    let hue_sat = camera_look.as_ref().and_then(|p| p.hue_sat.as_ref()).and_then(crate::camera_preview::HueSat::new);
    // A DNG's own profile look (hue/saturation map, look table), DNG spec chapter 6.
    let tables = lightcraft_raw::profile::ProfileTables::new(&raw.color.profile, lightcraft_raw::color::illuminant_weight(&raw.color, xy));
    let colour = CameraColourModel {
        wb: transform.wb,
        matrix: camera_look.as_ref().map(|p| p.matrix.mul(&transform.matrix)).unwrap_or(transform.matrix),
        gain: 2f32.powf(transform.baseline_exposure as f32),
        tables: tables.as_ref(),
        hue_sat: hue_sat.as_ref(),
    };
    // camera RGB → what the pipeline takes: clipped highlights rebuilt, white balance and the colour model applied,
    // fitted to `max_edge` and upright
    match &clip_mask {
        Some(mask) => colour.rebuild_highlights_masked(&mut img, mask),
        None => colour.rebuild_highlights(&mut img),
    }
    colour.apply(&mut img);
    // a full-size render keeps the picture (fitting it to its own size would copy it, 288 MB at 24 MP)
    let img = if img.width.max(img.height) > max_edge { fit(&img, max_edge, max_edge, Filter::Box) } else { img };
    let img = img.into_oriented(raw.orientation);
    timings.finish = ms(t);
    // White balance re-evaluates the file's own colour model (when it has one and no file-local look matrix sits
    // on top of it). A model's measured matrices never do: they only give the Kelvin scale its numbers, and the
    // picture is adapted from the as-shot white as every fitted look is.
    let camera_color = (scale.own_matrix && !transform.matrix_is_fallback && camera_look.is_none()).then(|| {
        let tags = lightcraft_raw::ColorData { profile: Default::default(), ..raw.color.clone() };
        Arc::new(lightcraft_pipeline::CameraColor { tags, developed_for: xy })
    });
    let camera_tone = camera_look.as_ref().map(|p| p.tone);
    let (temp, tint) = scale.as_shot(xy);
    let info = SourceInfo {
        raw: true,
        as_shot_temp: temp,
        as_shot_tint: tint,
        lens: lens.map(|found| found.lens),
        relative_wb: scale.relative,
        legacy_relative_wb: scale.legacy_relative,
        camera_color,
        camera_tone,
        mattes: None,
        local_tone: None,
        native_long,
    };
    Ok(Loaded { image: img, info, camera, timings })
}

/// The look of `raw`'s camera in Creative Style `style`, from its profile ([`crate::camera_profiles`]): the as-shot
/// white, the camera transform at it and the profile's colour and tone, as
/// [`crate::camera_preview::starting_colour`] gives a fitted one. `None` without a profile for them, or for a raw
/// with colour matrices of its own.
fn fixed_look(raw: &lightcraft_raw::RawImage, style: &str, process: u32) -> Option<(lightcraft_color::Xy, lightcraft_raw::color::CameraTransform, Option<CameraLook>)> {
    let profile = crate::camera_profiles::get(raw.metadata.model.as_deref()?, style, process)?;
    let xy = lightcraft_raw::color::as_shot_white_xy(raw);
    let transform = lightcraft_raw::color::camera_transform(raw, xy);
    if !transform.matrix_is_fallback {
        return None;
    }
    // (the look's matrix goes on top of the transform's, which the profile's own replaces)
    let look = CameraLook { matrix: profile.matrix().mul(&transform.matrix.inverse()?), tone: profile.tone?, hue_sat: profile.hue_sat.clone() };
    Some((xy, transform, Some(look)))
}

/// Which white-balance scale a raw is edited on, decided from the file alone.
///
/// A file with colour matrices of its own (DNG) has an absolute scale from its tags. A file without
/// (`camera_preview::file_local_look`) has one when its camera's matrices are measured
/// (`lightcraft_raw::spectral`, the ILCE-7M3 among them): they turn its as-shot multipliers into Kelvin and tint,
/// nothing else — the look stays the file's fit. Any other raw is edited relative to its as-shot look (6500 K / 0
/// means as shot).
pub(crate) struct WbScaleOf {
    own_matrix: bool,
    relative: bool,
    /// Kelvin through the camera's measured matrices: edited on the relative scale before those were used.
    legacy_relative: bool,
    /// The as-shot white through the measured matrices, when that is where the scale comes from.
    spectral_white: Option<lightcraft_color::Xy>,
}

impl WbScaleOf {
    pub(crate) fn new(
        format: lightcraft_raw::RawFormat,
        color: &lightcraft_raw::ColorData,
        wb_multipliers: Option<[f32; 3]>,
        meta: &lightcraft_meta::Metadata,
    ) -> Self {
        let own_matrix = lightcraft_raw::color::has_matrix(color);
        let fitted = crate::camera_preview::file_local_look(format) && !own_matrix;
        let camera = fitted.then(|| meta.model.as_deref().and_then(|model| lightcraft_raw::spectral::find(meta.make.as_deref(), model))).flatten();
        let spectral_white = camera.and_then(|c| lightcraft_raw::spectral::as_shot_white_through(c, color, wb_multipliers));
        WbScaleOf { own_matrix, relative: fitted && camera.is_none(), legacy_relative: fitted && camera.is_some(), spectral_white }
    }

    /// The as-shot temperature and tint on the file's scale: the relative scale's reference, the white through
    /// the measured matrices, else `own` (the white the file's own tags give). Whole numbers.
    pub(crate) fn as_shot(&self, own: lightcraft_color::Xy) -> (f64, f64) {
        if self.relative {
            return (6500.0, 0.0);
        }
        let (t, tint) = xy_to_temp_tint(self.spectral_white.unwrap_or(own));
        (t.round(), tint.round())
    }
}

/// Orientation for an embedded preview: its own EXIF orientation when it has one, else the raw file's.
fn preview_orientation(raw_bytes: &[u8], jpeg_orientation: u16) -> Orientation {
    if jpeg_orientation > 1 {
        return Orientation::from_exif(jpeg_orientation);
    }
    lightcraft_meta::extract(raw_bytes).orientation.unwrap_or(Orientation::Normal)
}

/// The camera's JPEG, decoded in the colour space the raw says it's in: what previews show and the
/// camera match fits to.
pub(crate) fn decode_raw_preview(bytes: &[u8], opts: lightcraft_codecs::DecodeOptions) -> Option<lightcraft_codecs::Decoded> {
    let jpeg = lightcraft_raw::embedded_preview(bytes)?;
    match lightcraft_raw::embedded_preview_color_space(bytes) {
        Some(space) => {
            let space = match space {
                lightcraft_raw::PreviewColorSpace::Srgb => lightcraft_codecs::NamedSpace::Srgb,
                lightcraft_raw::PreviewColorSpace::AdobeRgb => lightcraft_codecs::NamedSpace::AdobeRgb,
            };
            lightcraft_codecs::decode_jpeg_with_fallback(&jpeg, opts, space).ok()
        }
        None => lightcraft_codecs::decode(&jpeg, opts).ok(),
    }
}

/// The embedded preview of a raw file as a working-space image no larger than `max_edge`, oriented.
pub fn load_embedded_preview(bytes: &[u8], max_edge: usize) -> Option<(Rgb32f, SourceInfo)> {
    let d = decode_raw_preview(bytes, fit_box(max_edge))?;
    let img = d.to_working();
    let img = if img.width.max(img.height) > max_edge { fit(&img, max_edge, max_edge, Filter::Mitchell) } else { img };
    Some((img.oriented(preview_orientation(bytes, d.orientation)), SourceInfo::default()))
}

/// The embedded preview of a raw file for display (sRGB, oriented, no larger than `max_edge`): what's
/// shown until the raw itself has been developed.
pub fn embedded_preview_srgb(bytes: &[u8], max_edge: usize) -> Option<lightcraft_raster::Rgba8> {
    let mut d = decode_raw_preview(bytes, fit_box(max_edge))?;
    if d.image.width.max(d.image.height) > max_edge {
        d.image = fit(&d.image, max_edge, max_edge, Filter::Box);
        d.alpha = None;
    }
    let o = preview_orientation(bytes, d.orientation);
    Some(d.to_srgb8().oriented(o))
}

/// Decode options fitting into a `max_edge` square; `usize::MAX` (a full-size load) or any edge past
/// `u32::MAX` asks for the largest box rather than wrapping to a small one.
fn fit_box(max_edge: usize) -> lightcraft_codecs::DecodeOptions {
    let e = u32::try_from(max_edge).unwrap_or(u32::MAX);
    lightcraft_codecs::DecodeOptions::fit(e, e)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// The CC0 ILCE-7M3 raws Lightcraft's `cargo xtask corpus` downloads from raw.pixls.us, where the
    /// vendored crates' corpus tests look for them; `None` (the test skips) without them.
    pub(crate) fn a7m3_corpus() -> Vec<(String, Vec<u8>)> {
        let dir = std::env::var_os("LIGHTCRAFT_CORPUS")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vendor/lightcraft/corpus"))
            .join("raw");
        ["arw-sony-a7m3-compressed.arw", "arw-sony-a7m3-uncompressed.arw"]
            .into_iter()
            .filter_map(|name| match std::fs::read(dir.join(name)) {
                Ok(bytes) => Some((name.to_owned(), bytes)),
                Err(_) => {
                    eprintln!("skip: {} absent", dir.join(name).display());
                    None
                }
            })
            .collect()
    }

    #[test]
    fn corpus_a7m3_opens_with_a_camera_look_and_kelvin_white_balance() {
        for (name, bytes) in a7m3_corpus() {
            let Loaded { image: img, info, timings, .. } = load(&bytes, 1200, Profile::Camera, crate::sidecar::PROCESS_VERSION).unwrap();
            assert_eq!(img.width.max(img.height), 1200, "{name}");
            assert!(img.data.iter().flatten().all(|v| v.is_finite() && *v >= 0.0), "{name}");
            assert!(info.raw && info.camera_tone.is_some(), "{name}: a camera look");
            // Its spectral matrices put white balance on a Kelvin scale (as shot, not 6500 / 0).
            assert!(!info.relative_wb && (2500.0..10000.0).contains(&info.as_shot_temp), "{name}: {info:?}");
            assert!(timings.decode > 0.0 && timings.demosaic > 0.0, "{name}: {timings:?}");
            // Noise reduction is sized to the full frame, which is what a full-size load is.
            let full = load(&bytes, usize::MAX, Profile::Camera, crate::sidecar::PROCESS_VERSION).unwrap();
            assert_eq!(info.native_long, Some(full.image.width.max(full.image.height)), "{name}");
            assert_eq!(full.info.native_long, info.native_long, "{name}");
            let preview = embedded_preview_srgb(&bytes, 512).unwrap();
            assert_eq!(preview.width.max(preview.height), 512, "{name}");
        }
    }

    #[test]
    fn what_isnt_a_raw_is_an_error_not_a_panic() {
        assert!(load(b"", 100, Profile::Camera, crate::sidecar::PROCESS_VERSION).is_err());
        assert!(load(b"not a raw at all", 100, Profile::Camera, crate::sidecar::PROCESS_VERSION).is_err());
        assert!(load(&[0xff, 0xd8, 0xff, 0xe0, 0, 0x10], 100, Profile::Camera, crate::sidecar::PROCESS_VERSION).is_err());
        // a TIFF header that goes nowhere
        assert!(load(b"II*\0\x08\0\0\0\0\0", 100, Profile::Camera, crate::sidecar::PROCESS_VERSION).is_err());
        assert!(embedded_preview_srgb(b"II*\0\x08\0\0\0\0\0", 100).is_none());
    }

    #[test]
    fn white_balance_scale_follows_the_files_matrices_then_the_cameras() {
        use lightcraft_raw::{ColorData, RawFormat};
        let meta = |make: &str, model: &str| lightcraft_meta::Metadata { make: Some(make.into()), model: Some(model.into()), ..Default::default() };
        let mul = Some([2.2f32, 1.0, 1.6]);
        let d65 = lightcraft_color::D65;
        // a covered camera: Kelvin through the spectral matrices, near the daylight these multipliers describe
        let a7m3 = WbScaleOf::new(RawFormat::Arw, &ColorData::default(), mul, &meta("SONY", "ILCE-7M3"));
        assert!(!a7m3.own_matrix && !a7m3.relative && a7m3.legacy_relative && a7m3.spectral_white.is_some());
        let (t, tint) = a7m3.as_shot(d65);
        assert!((3500.0..7000.0).contains(&t) && t != 6500.0 && tint.abs() < 30.0 && t.fract() == 0.0 && tint.fract() == 0.0, "{t} {tint}");
        // the same camera without multipliers: the scale is Kelvin still, the white falls back to the tags' (D65)
        let bare = WbScaleOf::new(RawFormat::Arw, &ColorData::default(), None, &meta("SONY", "ILCE-7M3"));
        assert!(bare.legacy_relative && bare.spectral_white.is_none());
        assert_eq!(bare.as_shot(d65), {
            let (t, tint) = xy_to_temp_tint(d65);
            (t.round(), tint.round())
        });
        // a camera without measured matrices: relative, whatever the multipliers
        let a7m2 = WbScaleOf::new(RawFormat::Arw, &ColorData::default(), mul, &meta("SONY", "ILCE-7M2"));
        assert!(a7m2.relative && !a7m2.legacy_relative && a7m2.spectral_white.is_none());
        assert_eq!(a7m2.as_shot(d65), (6500.0, 0.0));
    }

    fn yellowing_look() -> lightcraft_color::Mat3 {
        lightcraft_color::Mat3([[0.42, 0.548, 0.04], [0.246, 0.728, 0.033], [-0.247, 0.679, 0.447]])
    }

    /// A camera look with a table applied in lanes, on rows that aren't a whole number of them, gives the bits of
    /// the pixel-by-pixel path, awkward pixels (black, negative, huge, NaN, infinite) included.
    #[test]
    fn camera_look_in_lanes_matches_pixel_by_pixel() {
        let data = (0..72 * 5 * 5).map(|i| [((i * 37) % 61) as f32 - 30.0, 0.6 + ((i * 11) % 13) as f32 / 13.0, 1.0]).collect();
        let table = lightcraft_raw::profile::HsvTable { hue_divisions: 72, sat_divisions: 5, val_divisions: 5, data, srgb_value: true };
        let hue_sat = crate::camera_preview::HueSat::new(&table).unwrap();
        let colour = CameraColourModel { wb: [2.1, 1.0, 1.5], matrix: yellowing_look(), gain: 1.3, tables: None, hue_sat: Some(&hue_sat) };
        let special = [0.0, -0.0, -0.01, 1e-30, 0.3, 1.0, 5.0, 1e30, f32::INFINITY, f32::NAN];
        let img = Rgb32f::from_fn(37, 5, |x, y| {
            let k = y * 37 + x;
            if k % 5 == 0 {
                [special[k % 10], special[k / 10 % 10], special[k / 100 % 10]]
            } else {
                [(x as f32 * 0.37).sin().abs(), ((x + y) as f32 * 0.11).cos().abs(), y as f32 / 4.0 - 0.05]
            }
        });
        let mut lanes = img.clone();
        colour.apply(&mut lanes);
        let m = colour.matrix.to_f32();
        for (p, got) in img.data.iter().zip(&lanes.data) {
            assert_eq!(got.map(f32::to_bits), colour.pixel(&m, *p).map(f32::to_bits), "{p:?}");
        }
    }

    #[test]
    fn clipped_white_renders_neutral_through_the_colour_model() {
        use lightcraft_color::{LUMA_2020, Mat3};
        let look = yellowing_look();
        let white = clipped_white(&look);
        let out = look.apply(white.map(f64::from));
        assert!(out.iter().all(|v| (v - out[1]).abs() < 1e-5), "{white:?} renders {out:?}");
        let luma = |c: [f64; 3]| (0..3).map(|i| c[i] * f64::from(LUMA_2020[i])).sum::<f64>();
        assert!((luma(out) - luma(look.apply([1.0; 3]))).abs() < 1e-5, "as bright as the camera's neutral");
        assert_eq!(clipped_white(&Mat3::IDENTITY), [1.0; 3]);
        // a model that can't be inverted, or only just, keeps the camera's neutral
        assert_eq!(clipped_white(&Mat3([[1.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]])), [1.0; 3]);
        assert_eq!(clipped_white(&Mat3([[1.0, 1.0, 0.0], [1.0, 1.0 + 1e-8, 0.0], [0.0, 0.0, 1.0]])), [1.0; 3]);
        // so does one whose white would overflow the rebuilt highlights (2.5e38 in red here)
        assert_eq!(clipped_white(&Mat3([[3e-39, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]])), [1.0; 3]);
        // or move the camera's neutral implausibly far
        assert_eq!(clipped_white(&Mat3([[0.1, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]])), [1.0; 3]);
        // what it answers renders neutral through the f32 matrix, and fully clipped highlights stay finite
        for model in [look, Mat3([[1.9, -0.7, -0.2], [-0.3, 1.4, -0.1], [0.0, -0.4, 1.4]])] {
            let w = clipped_white(&model);
            let m = model.to_f32();
            let out: [f32; 3] = std::array::from_fn(|i| m[i][0] * w[0] + m[i][1] * w[1] + m[i][2] * w[2]);
            assert!(out.iter().all(|v| (v - out[1]).abs() <= 1e-3 * out[1]), "{w:?} renders {out:?}");
            let colour = CameraColourModel { wb: [2.5, 1.0, 1.6], matrix: model, gain: 1.0, tables: None, hue_sat: None };
            let mut img = Rgb32f::filled(4, 4, [1.0; 3]);
            colour.rebuild_highlights(&mut img);
            colour.apply(&mut img);
            assert!(img.data.iter().flatten().all(|v| v.is_finite()), "{:?}", img.get(0, 0));
        }
    }
}

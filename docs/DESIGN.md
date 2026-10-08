# OmaLux design

OmaLux is a fast, keyboard-first raw developer for Omarchy. It does one
job: take the picks from a culled shoot and get each one's exposure,
colour, noise and lens problems to a good editable state, then hand them
to [Omapix](https://github.com/michaelrubi/omapix) as 16-bit TIFFs. It
is the second stage of one workflow, and replaces darktable in it:

    cull (Omacull) → develop (OmaLux) → retouch (Omapix) → export

It is an independent project, not part of Omarchy.

## Principles

1. **Prep, not finish.** The complaint about darktable is that there is
   too much going on. The goal is not a perfect output but an editable
   one: the exposure, the colour and the noise where Omapix can take
   over. Anything else has to argue its way in.
2. **Pull everything out of the raw.** Float pipeline from the sensor
   data on, careful demosaicing and highlight recovery, denoising on the
   raw itself, and 16-bit wide-gamut output, so nothing the raw held is
   lost before Omapix gets it.
3. **Never wait.** A slider drag shows its result in the next frame. The
   pipeline runs on the GPU; full resolution is only worked out for what
   is on screen at 100% and for export.
4. **Lightroom muscle memory.** The panels, the slider names, what they do
   and the keys are Lightroom Classic's, as Omacull's are Bridge's and
   Omapix's Photoshop's. Where OmaLux behaves differently from Lightroom
   without a reason written down here, that's a bug.
5. **The files are the database.** Edits live in the XMP sidecar beside
   each raw, the same one Omacull writes its marks to. No catalogue, no
   import step. Raws are never moved, renamed, modified or deleted.
6. **Auto is a button.** Auto white balance, exposure, tone and level are
   deliberate presses. Nothing is changed on opening a frame.
7. **An edit never changes under you.** Every sidecar records the version
   of the pipeline it was made with (Lightroom's process version). A newer
   OmaLux renders an old edit the way it was made, or says it can't.
8. **Omarchy-native.** Wayland, colours from the active Omarchy theme and
   following theme switches live, behaves well as a Hyprland tile.
9. **Opinionated, not configurable.** Same as Omacull and Omapix: good
   defaults over preference dialogs.

## Scope

Decided in the 2026-10-08 scoping interview.

### Who it's for

One photographer's shoots first: a Sony ILCE-7M3 (A7 III), mostly
outdoors, ISO 100 to 6400 with most of it at the low end, with three
primes (FE 35mm, 50mm and 85mm f/1.8). Omacull takes a shoot of 200 to
1000 frames down to 10 to 50 picks; those are what OmaLux develops.
Similar frames are worked as a set: one is developed fully, and its
settings pasted onto the others. Almost all the real editing happens
afterwards in Omapix.

### In

- **Input:** raws only, Sony ARW first. A folder opens showing its picks
  (rated 1 or more); a filter shows everything.
- **Marks:** Lightroom's, as Omacull has them: P picks, X rejects, U
  unmarks, 0 to 5 stars. Written where Omacull writes them, so either
  tool can change a mark.
- **Profile (the starting look):**
  - **Camera match**, the default: the frame starts out looking like the
    camera's own JPEG, which is what was on the camera's screen at the
    shoot, as Lightroom's Camera Standard profile does. See "The look".
  - **Flat**: low contrast, all the highlight and shadow room there is.
    For frames that need the most latitude in Omapix.
- **White balance:** As Shot, Auto, the eyedropper (click something
  neutral), Temp and Tint.
- **Light:** Exposure, Contrast, Highlights, Shadows, Whites, Blacks.
- **HDR:** one slider that brightens the shadows and holds back the
  highlights locally, so more of what the raw holds is visible, without
  the flat, haloed look of pulling Highlights and Shadows to their ends.
  See "HDR".
- **Presence:** Vibrance and Saturation.
- **Detail:**
  - **AI Denoise:** a button and a strength, run on the raw before
    demosaicing, as Lightroom's Denoise is.
  - **Noise Reduction:** Luminance and Color sliders, the manual way.
  - **Sharpening:** Amount, Radius, Detail and Masking, as Lightroom's.
- **Lens Corrections:** profile corrections (distortion and vignetting)
  found from the lens the raw names, and Remove Chromatic Aberration.
  Defringe for the purple and green fringes fast primes leave around
  highlights at f/1.8.
- **Crop & Straighten:** aspect ratios, the crop rectangle, the
  straighten angle (and a line to draw along), 90° turns.
- **Auto:** Auto white balance, Auto exposure, Auto tone (sets Whites and
  Blacks so nothing clips that needn't) and Auto level (straightens the
  horizon). Each is a button, and Shift+double-click on a slider sets
  that one, as in Lightroom.
- **Histogram:** always on, at the top of the right panel as in
  Lightroom, of the developed frame as it will be exported, with the
  clipping warnings (J).
- **Before/after:** `\` toggles between the edit and the frame with
  none.
- **Inspection:** 100% zoom (Z), on a full-resolution development of
  what's on screen.
- **Copy and paste:**
  - Ctrl+C copies every setting of the current frame.
  - Ctrl+Shift+C opens a checklist of the groups to copy (White Balance,
    Profile, Light, HDR, Presence, AI Denoise, Noise Reduction,
    Sharpening, Lens Corrections, Crop), remembering the last choice.
    What's chosen at copy time is what's pasted; there is no second
    checklist at paste time. This is the opposite of darktable's
    copy and paste, on purpose.
  - Ctrl+V pastes what was copied onto the selection, or the current
    frame. Ctrl+Alt+V pastes the previous frame's settings (Lightroom's
    Paste from Previous). A key pastes onto the current frame's whole
    stack in one go.
- **Stacks:** frames grouped by capture time and look, as Omacull's
  stacks are, among the picks. They're how "paste onto everything like
  this one" works, and collapse in the filmstrip.
- **Handoff:** Ctrl+E (Lightroom's Edit In) exports the selection as
  16-bit TIFFs and opens them in Omapix, a tab each, the way darktable's
  "edit in Omapix" target does now. Ctrl+Shift+E exports without opening
  Omapix.
- **Export:** 16-bit TIFF in Linear ProPhoto RGB with its ICC profile
  (what Michael exports from darktable today, and what Omapix opens), into
  a subfolder of the shoot, `omalux/` by default, set in `config.toml`.
- **Undo and redo** per frame, and Ctrl+Shift+R resets a frame.

### Out (for now)

Each of these has a place in the roadmap's "Later" or is someone else's
job:

- Local adjustments: subject masks, gradients, brushes. A maybe, later.
- HSL, the colour grading wheels and the tone curve. Later.
- Virtual copies (two edits of one raw). Later.
- JPEGs and phone photos as input. Later; raws only to begin with.
- JPEG export. Later: there are times a frame goes straight out.
- Loading DCP camera profiles (Adobe's own, or ones made with
  `dcamprof`). Later.
- Match Total Exposures across a set. Later.
- Presets saved by name. Copy and paste covers the need for now.
- Healing, liquify and anything else that changes pixels by hand: Omapix.
- Culling beyond marks: compare, survey, faces, signals are Omacull's.
  Omacull stays a separate tool so it stays fast.
- A catalogue, collections, keywords, search across shoots.
- HDR output (HDR displays, gain-map files), Merge to HDR and panoramas
  (Omapix has those), print, books, maps, tethering, video.
- Any connection to darktable once OmaLux replaces it.

## Keys

Lightroom Classic's Develop module, provisional until M1 settles them.
All rebindable in `~/.config/omalux/hotkeys.toml`, as Omacull's and
Omapix's are.

| Key | Action |
|-----|--------|
| ← → Home End | Step through the frames the filter shows |
| P X U 0–5 | Pick, reject, unmark, stars |
| `\` | Before/after |
| Z | 100% zoom (tap to toggle, hold for a look) |
| , . | Select the previous or next slider |
| = − (Shift for more) | Nudge the selected slider |
| Double-click a slider's name | Reset it |
| Shift+double-click a slider | Set it automatically |
| W | White balance eyedropper |
| Ctrl+U | Auto tone |
| Ctrl+Shift+U | Auto white balance |
| R | Crop & Straighten |
| J | Clipping warnings |
| Ctrl+C, Ctrl+Shift+C | Copy all settings, choose what to copy |
| Ctrl+V, Ctrl+Alt+V | Paste, paste from previous |
| Ctrl+Z, Ctrl+Shift+Z | Undo, redo |
| Ctrl+Shift+R | Reset the frame |
| Ctrl+E | Export and edit in Omapix |
| Ctrl+Shift+E | Export |
| Ctrl+O | Open a folder |

Keys inside a tool (crop's X to swap the aspect, for one) only apply
while it's open, as in Lightroom.

## Built on Lightcraft

[Lightcraft](https://github.com/storytold/lightcraft) (MIT OR
Apache-2.0) is a clean-room, pure-Rust reimplementation of all of
Lightroom: library, catalogue, culling, every Develop panel, masking,
merges, an MCP server and a web build. That is far more than OmaLux wants,
and its UI is its own, not Omarchy's. But it's built in layers, and
everything below its UI depends on nothing but itself:

```
geom, color, raster, tiff     (L0)
raw, codecs, meta, develop    (L1)
pipeline                      (L2, the CPU pipeline: the reference)
gpu                           (L3, the same stages as wgpu compute shaders)
catalog → engine → ui-egui, mcp → apps
```

So OmaLux takes the bottom of it whole and builds its own top: what was
going to take most of the roadmap is there already, tested, on the same
stack (Rust 2024, egui 0.36, wgpu). Checked against Lightcraft at
`7b47ba7` (2026-10-09).

### What's taken whole (vendored)

| Crate | What OmaLux gets from it |
|-------|--------------------------|
| `geom`, `color`, `raster`, `tiff` | Maths, colour spaces and transfer functions, float images, TIFF reading |
| `raw` | Pure-Rust raw decoders, Sony ARW among them (the ILCE-7M3's compressed and uncompressed files are its benchmark), demosaicing (AHD, PPG), highlight recovery |
| `codecs` | JPEG decoding (the embedded previews), TIFF encoding, ICC profiles, colour conversion (`moxcms`) |
| `meta` | Exif and makernotes, XMP parsing, and `xmp_merge`: writing into a sidecar another tool wrote, keeping everything it doesn't own byte for byte |
| `develop` | `DevelopSettings`, every control's range and default, copy groups |
| `pipeline` | The CPU pipeline: scene-referred, linear Rec. 2020 float; white balance, Light with edge-aware local tone mapping, Presence, Detail (sharpening, noise reduction), Optics (distortion, vignetting, lateral CA auto and manual, defringe), crop, straighten and Upright, auto tone and white balance, output |
| `gpu` | The same stages as wgpu compute shaders, checked against the CPU pipeline to within 1/255, with a CPU fallback and careful backend selection |
| `scenes` | Procedural test photos, for tests that can't ship real raws |

### What's lifted in part (copied from `engine` and `ui-egui`)

- **Camera colour from the camera's JPEG** (`engine/camera_preview.rs`,
  `camera_profiles.rs`): a guarded fit of a matrix, a hue/saturation
  table, a tone curve and a chroma curve to the embedded JPEG, and pooled
  profiles fitted over many shoots (`lightcraft-cli calibrate`), as
  bundled for the ILCE-7M4. This is OmaLux's camera match. They depend
  only on the vendored crates.
- **Export:** the TIFF and ICC part of `engine/export.rs`.
- **Sidecars:** how `engine/sidecar.rs` drives `xmp_merge`.
- **Widgets and behaviour** from `ui-egui`, one at a time, restyled with
  Omarchy's theme: slider behaviour (drag, wheel, double-click reset,
  drafts while dragging), the histogram, the crop overlay, the copy
  settings checklist, before/after.
- **Patterns:** a render worker pool off the UI thread (drafts during
  drags, full quality on release, neighbours prepared ahead); the
  never-crash rules (no panics on anything read from a file); profiling
  with per-stage timings.

### What's left

The library and catalogue (OmaLux is folder and sidecar), import, the
command registry and control channel (OmaLux has Omacull's `Command`),
MCP, the web build, merges (Omapix has them), SAM 3 masks on candle
(Omapix's models are on ONNX Runtime), Lightroom catalogue import,
presets, localisation, and the UI as a whole.

### How it's kept

- **Vendored at a pinned commit** under `vendor/lightcraft/`, with the
  crates' own names, licences and `NOTICE`, and `vendor/lightcraft/UPSTREAM.md`
  recording the commit and every local change. Lightcraft moves fast
  (hundreds of commits a month, camera colour among them), so it's
  re-synced on purpose, not followed: diff upstream since the pinned
  commit, take what helps, re-run the renders the process version
  guards (below), bump the commit.
- **Changes to the vendored crates stay clean-room and permissive**, so
  they can be offered back upstream: no code ported from GPL projects goes
  into them. What is GPL-derived (lensfun's models, anything ported from
  darktable or RawTherapee) lives in OmaLux's own crates and reaches the
  pipeline through the vendored crates' existing inputs (a `Warp`'s
  coefficients, a decoded Bayer image).
- **Licence:** OmaLux is GPL-3.0-or-later; MIT and Apache-2.0 code can
  be part of it. The vendored crates keep their MIT OR Apache-2.0 headers
  and licence files; the README credits Lightcraft.
- **The settings struct is taken whole**, HSL, grading, masks and all.
  OmaLux's panels only show the controls in scope, so most of the "Later"
  list is a matter of showing a panel, not building one.

## The edit and the sidecar

- **One sidecar per raw**, the one Omacull writes: `DSC01234.ARW.xmp`, or
  `DSC01234.xmp` if that's the one that exists (Omacull's `sidecar =
  "adobe"`). A new one is named darktable's way, as Omacull names them.
- **The rating** stays in `xmp:Rating` (-1 rejected, 1 a pick), where
  Omacull reads and writes it. Lightcraft's own `lc:flag` isn't written.
- **The edit** is Lightcraft's `DevelopSettings` as JSON in
  `omalux:settings` (an exact round trip, as Lightcraft's `lc:settings`
  is), beside `omalux:processVersion`.
- **Written with `xmp_merge`:** OmaLux owns the `omalux:` namespace and
  `xmp:Rating`, and everything else in the file is copied byte for byte;
  writes are atomic (temp file, fsync, rename) and go to a disk thread.
  A drag writes once, when it ends.
- **darktable is gone** once OmaLux replaces it, so its history in a
  sidecar is left alone and never read. (If darktable ever rewrote one of
  these sidecars, it would drop OmaLux's part. That's a reason to stop
  using darktable on a shoot, not something to build around.)
- **The process version** is OmaLux's, not Lightcraft's schema version:
  it names how a frame was rendered. Re-syncing Lightcraft can change a
  rendering, so a set of reference frames (Michael's, and raw.pixls.us's
  A7 III files) is rendered before and after each re-sync; if they
  differ, the version is bumped and older edits keep the old behaviour
  where it can be kept, or say they'll look different.
- **Undo** history lives in memory for the session, per frame.

## The look

Camera match is the default, so it has to be stable: two frames of one
burst with the same settings must look the same, or pasting settings
across a stack means nothing.

- **Lightcraft's fit** develops a small proxy of the raw and fits it to
  the camera's embedded JPEG (a 3×3 chromaticity matrix, a
  hue/saturation table, a tone curve and a chroma curve, each kept only if
  it beats the neutral fallback on held-out pixels). Per file, that's
  unstable across a burst.
- **A pooled profile** for the ILCE-7M3, fitted with Lightcraft's
  `calibrate` over Michael's shoots and bundled, as Lightcraft bundles the
  ILCE-7M4's, fixes the colour. Lightcraft still fits the tone curve per
  file when a profile exists; OmaLux uses the profile's own tone too, so
  the whole look is fixed per camera. Whether a pooled tone curve looks
  right across his shoots is for M4 to find out.
- **Flat** skips the fitted curve and keeps a gentle one that never clips.
- **White balance is relative** until a camera is calibrated: Lightcraft
  doesn't assume Sony's colour matrix is a DNG one, so Temp and Tint are a
  scale around As Shot (6500/0 is its neutral reference), not measured
  Kelvin. Fine for what OmaLux does; measured Kelvin is later.
- **Loading DCP profiles** (Adobe's own, or made with `dcamprof`) is
  later. Lightcraft refuses them on principle; OmaLux needn't.

## HDR

Pulling Highlights down and Shadows up across a whole frame flattens it.
Lightcraft's Highlights and Shadows already work locally (a guided filter
on log luminance, so a blown sky comes back without grey halos). The HDR
slider goes further on the same machinery: both at once, by an amount, so
more of what the raw holds is visible. If that can't reach the look,
exposure fusion of virtual exposures of the one raw is the alternative
(Omapix's `fusion.rs`, which Merge to HDR uses). M4 tries both.

## Pipeline

Lightcraft's, as it stands: float throughout, scene-referred in linear
Rec. 2020 until the output transform, with gamut mapping instead of
clipping and a filmic shoulder for raws. The CPU pipeline is the
reference; the GPU runs the same stages and is tested against it. Its
budget is OmaLux's: a slider update in under 16 ms on a draft-sized
image, the loupe in under 60 ms at about 2.5 MP (Lightcraft measured 4 ms
and 30 ms on an Apple M4 Pro; M0 measures the RTX 4070 laptop on Vulkan).

What OmaLux adds, and where:

- **AI Denoise** on the Bayer data, between decoding and demosaicing in
  `raw`'s decode path: the denoised mosaic replaces the original, and is
  cached on disk per frame, since it takes seconds.
- **Lens profile data** into `pipeline`'s `Warp` and vignetting: Optics
  already corrects from data embedded in DNG and RW2 files; OmaLux
  supplies the coefficients for ARWs (below).
- **16-bit linear output.** Lightcraft's 16-bit output is display-encoded
  and its linear output is 32-bit float. OmaLux exports 16-bit Linear
  ProPhoto as Michael's darktable exports were; M0 checks that against
  what Omapix does with it, and against 16-bit ProPhoto with its own
  curve, which keeps more shadow precision in 16 bits.
- **Demosaicing:** Lightcraft has AHD and PPG. RCD or AMaZE (from
  RawTherapee, GPL, so in OmaLux's own crate) only if AHD leaves detail
  behind at 100% on real frames.

How it runs in the app:

- **On the GPU**, with the UI in egui on the same device, as Omacull and
  Omapix do. Lightcraft's backend selection (only the backends meant, so
  a crashing driver elsewhere can't take the process down) is kept.
- **Drafts during drags, full quality on release,** on a worker pool off
  the UI thread; at 100% only what's on screen at full size.
- **Neighbours prepared ahead**, as Omacull prefetches previews: stepping
  to the next frame shows its embedded preview at once, then its render.
- **The filmstrip** shows the embedded previews, and the developed frame
  once it's been rendered, cached in `~/.cache/omalux/`.

## Lens corrections

All three of Michael's lenses are in the lensfun database with
distortion, chromatic aberration and vignetting data (checked
2026-10-08 in `data/db/mil-sony.xml`: FE 35mm f/1.8, FE 50mm f/1.8,
FE 85mm f/1.8). Sony also writes its own correction data into every ARW.
Lightcraft corrects from neither (lensfun is GPL territory to it), but
its Optics stage takes the coefficients. So OmaLux, in its own crate:

- **reads lensfun's database** from the system package (`pacman -S
  lensfun`, under `/usr/share/lensfun`) with its own XML reader, and
  turns its models (ptlens and poly3 distortion, poly3 TCA, pa
  vignetting) into the `Warp`'s; the data is CC-BY-SA 3.0 and isn't
  shipped;
- **reads Sony's embedded corrections**, which darktable reads too. A
  darktable user reported they under-correct distortion where lensfun
  didn't; M0 compares them on real frames and picks the default.

Lightcraft's automatic lateral CA estimate and its defringe cover the
rest: Remove Chromatic Aberration and the fringes at f/1.8.

## AI

Following Omapix and Omacull, not Lightcraft (whose SAM 3 runs on
candle): models on the system's ONNX Runtime opened at run time, CUDA
where the library has it (Arch's `onnxruntime-cuda`, as Omapix uses), the
CPU otherwise. Models are fetched by a script and checked by size and
SHA-256; OmaLux itself never goes online. Where Omapix has a model on disk
already, it's used and not fetched again.

- **Raw denoise** is the one AI feature Lightcraft lacks. Omapix's NIND is
  the copy darktable installs, so it goes away with darktable, and it
  works on developed images anyway. Two models work on the raw itself:
  - RawNIND's Bayer model (GPL-3.0; Benoit Brummer, who made NIND).
  - RawForge's TreeNet models (MIT; Light, Super Light and Heavy), which
    testers on pixls.us compared favourably with Topaz's raw denoise.

  M0 runs both on real A7 III frames at ISO 3200 to 6400 and picks one.
- **Later:** subject and sky masks would reuse Omapix's BiRefNet and SAM
  through Lightcraft's mask inputs.

## Architecture

```
vendor/lightcraft/   lightcraft-geom, -color, -raster, -tiff, -raw, -codecs,
                     -meta, -develop, -pipeline, -gpu, -scenes, as upstream
                     with UPSTREAM.md listing every local change
crates/
  omalux-engine      folder scan, sidecars (on xmp_merge), the edit and its
                     process version, camera profiles and the fit (lifted
                     from Lightcraft), lens data (lensfun, Sony), stacks
                     (from Omacull), export (no UI or GPU dependencies;
                     testable headless)
  omalux-ai          ONNX Runtime, raw denoise
  omalux             the app: egui UI, panels, views, input, theme
```

- **Two families of conventions.** OmaLux's own crates follow Omacull's
  and Omapix's: theme, hotkeys, monitor profiles, the `Command` enum with
  an `OMALUX_SCRIPT` hook, the headless `Harness`, packaging, copied from
  them and not shared. The vendored crates keep Lightcraft's (its
  never-crash lints, its tests, the CPU pipeline as the GPU's oracle).
- **One colour engine.** Lightcraft uses `moxcms` (pure Rust); Omacull
  and Omapix use Little CMS 2 for monitor profiles. OmaLux uses `moxcms`
  for both unless M0 finds the monitor handling needs lcms2.
- **Config:** `~/.config/omalux/config.toml`, written on first run with
  every setting explained: where exports go, the program Ctrl+E opens.
- **Omacull hands over** with its existing `developer = "omalux"`
  setting: Ctrl+E in Omacull starts `omalux <folder>`.

## Prior art

What exists, what to take and what to leave. From the 2026-10-08 scoping
session; M0 checks the specifics the design leans on.

| Tool | Take | Leave |
|------|------|-------|
| Lightcraft (MIT OR Apache-2.0) | The bottom half of OmaLux, as code: see "Built on Lightcraft" | The library, the catalogue, MCP, the web build, its UI |
| Lightroom Classic (proprietary) | Panel and slider names and behaviour, keys, Auto, Copy Settings checklist, before/after, raw AI Denoise, Camera Matching profiles | Catalogue, import, modules beyond Develop, subscription |
| darktable | Reading Sony's embedded lens data | Too much going on; scene-referred controls exposed; its copy and paste |
| RawTherapee / ART | RCD and AMaZE demosaicing if AHD isn't enough (GPL-3, in OmaLux's crate), DCP profiles later | Density of settings |
| vkdt | Ideas for highlight inpainting and the local laplacian | The node graph as the interface |
| RapidRAW (AGPL-3.0) | Ideas only | A web UI in Tauri; its code |
| lensfun | The lens database | Its C library; we read the XML |
| RawNIND, RawForge | Raw denoise models | Their Python runtimes |
| Omacull | Sidecar conventions, colour management, stacks, preview prefetch, the `Command` and `Harness` | Culling, which stays there |
| Omapix | Conventions, ONNX Runtime on CUDA, exposure fusion, `--round-trip` | Everything that edits pixels by hand |

What doesn't exist on Linux, and is the reason for the project: a raw
developer with Lightroom's few, good controls and its keys, that runs
entirely on the GPU, denoises raws with AI locally, copies and pastes
settings the obvious way, keeps edits in sidecars beside the raws, and
looks and behaves like an Omarchy app. Lightcraft has the engine for it;
OmaLux is the small, Omarchy-shaped tool on top.

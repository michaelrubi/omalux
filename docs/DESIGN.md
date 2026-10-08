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

## The edit and the sidecar

- **One sidecar per raw**, the one Omacull writes: `DSC01234.ARW.xmp`, or
  `DSC01234.xmp` if that's the one that exists (Omacull's `sidecar =
  "adobe"`). A new one is named darktable's way, as Omacull names them.
- **The rating** stays in `xmp:Rating`, where Omacull reads and writes it.
- **The edit** is OmaLux's own: an `rdf:Description` in its own namespace
  (`omalux:`), one attribute per setting, plus `omalux:ProcessVersion`.
  Only settings that differ from the default are written, so an empty edit
  is an absent one.
- **Written the way Omacull writes ratings:** OmaLux's part is spliced
  into the file and everything else is left byte for byte as it was;
  writes are atomic (a temp file, renamed over); they go to a disk thread
  so a slow disk never holds up a frame. A drag writes once, when it ends.
- **darktable is gone** once OmaLux replaces it, so its history in a
  sidecar is left alone and never read. (If darktable ever rewrote one of
  these sidecars, it would drop OmaLux's part. That's a reason to stop
  using darktable on a shoot, not something to build around.)
- **Undo** history lives in memory for the session, per frame.

## The look

Camera match is the default, so it has to be stable: two frames of one
burst with the same settings must look the same, or pasting settings
across a stack means nothing. So the look is fitted once per camera (and
Creative Style), not per frame:

- The A7 III embeds a 1616×1080 JPEG of how it rendered each frame.
  Fitting OmaLux's neutral rendering of the raw to those JPEGs, over many
  frames of real shoots, gives a tone curve and a colour transform (a 3D
  LUT) that make OmaLux start where the camera did. RawTherapee's
  auto-matched tone curve does this per frame; doing it per camera keeps
  it stable.
- The fit is a tool that's run once and its result shipped as a profile
  file. Other cameras get a neutral starting look until someone fits one.
- Flat skips the fitted curve and keeps a gentle one that never clips.
- Loading DCP profiles (the open format Adobe's own profiles are in) is
  for later; it would let anyone with Adobe's DNG Converter use Adobe
  Color or Adobe Portrait.

## HDR

Pulling Highlights down and Shadows up across a whole frame flattens it.
Lightroom's, and every good raw developer's, highlights and shadows work
locally: dark areas are lifted by more than dark details are. OmaLux's
HDR slider does that with exposure fusion of one raw: the frame is
developed at a few virtual exposures and blended, weighted by how well
exposed each pixel is in each, across a pyramid so there are no halos.
Omapix has exposure fusion already (`omapix-engine`'s `fusion.rs`) for
Merge to HDR; vkdt's local laplacian is the other reference. Highlights
and Shadows use the same local machinery at gentler strengths.

## Pipeline

Float throughout, linear until the display or output transform. The order
is a starting point for M1 to settle:

1. **Decode** the raw (`rawler`): sensor data, black and white levels,
   the camera's colour matrix.
2. **AI Denoise** (if on), on the Bayer data. Slow, so it runs once per
   frame and its result is cached on disk.
3. **Highlight recovery** of clipped channels.
4. **White balance**, as channel multipliers from Temp and Tint through
   the camera's matrix.
5. **Demosaic** (RCD or AMaZE, from RawTherapee's `librtprocess`, ported
   to compute shaders). Lateral chromatic aberration is corrected here.
6. **Camera to working space** (linear ProPhoto or Rec. 2020).
7. **Lens:** distortion and vignetting. **Crop & straighten**.
8. **Noise Reduction** (manual) on linear data.
9. **Tone:** Exposure, HDR, Highlights, Shadows, Whites, Blacks, Contrast.
10. **Presence:** Vibrance, Saturation.
11. **The look:** the profile's curve and colour transform.
12. **Sharpening**, on luminance.
13. **Output:** to the monitor's profile for display, to Linear ProPhoto
    16-bit for export.

How it runs:

- **On the GPU** (wgpu compute shaders, WGSL), with the UI in egui on the
  same device, as Omacull and Omapix do. Nothing is copied back to the
  CPU to be shown.
- **Interactive:** each frame's raw is decoded, demosaiced and cached at
  a little over screen size once, and sliders run on that, so a drag is
  a few milliseconds. At 100% only the tiles on screen are worked out at
  full size. Export runs the same shaders at full size, in tiles: a 24 MP
  frame in float RGBA is 384 MB, and the laptop's GPU (an RTX 4070, 8 GB)
  shares that with everything else.
- **Neighbours prepared ahead**, as Omacull prefetches previews: stepping
  to the next frame shows it with its edit straight away.
- **The filmstrip** shows Omacull-style thumbnails at first and the
  developed frame once it's been seen, cached in `~/.cache/omalux/`.

## Lens corrections

All three of Michael's lenses are in the lensfun database with
distortion, chromatic aberration and vignetting data (checked
2026-10-08 in `data/db/mil-sony.xml`: FE 35mm f/1.8, FE 50mm f/1.8,
FE 85mm f/1.8). Sony also writes its own correction data into every ARW.
Both are read:

- **lensfun's database** from the system package (`pacman -S lensfun`,
  under `/usr/share/lensfun`), read with our own XML reader and the
  models it uses (ptlens and poly3 distortion, poly3 TCA, pa vignetting).
  The data is CC-BY-SA 3.0; OmaLux doesn't ship it.
- **Sony's embedded data**, which darktable reads too. A darktable user
  reported it under-corrects distortion where lensfun didn't; M0 compares
  them on real frames and picks the default.

## AI

Following Omapix and Omacull: models on the system's ONNX Runtime opened
at run time, CUDA where the library has it (Arch's `onnxruntime-cuda`, as
Omapix uses), the CPU otherwise. Models are fetched by a script and
checked by size and SHA-256; OmaLux itself never goes online. Where
Omapix has a model on disk already, it's used and not fetched again.

- **Raw denoise.** Omapix's NIND is the copy darktable installs, so it
  goes away with darktable, and it works on developed images anyway.
  Two models work on the raw itself:
  - RawNIND's Bayer model (GPL-3.0; Benoit Brummer, who made NIND).
  - RawForge's TreeNet models (MIT; Light, Super Light and Heavy), which
    testers on pixls.us compared favourably with Topaz's raw denoise.

  M0 runs both on real A7 III frames at ISO 3200 to 6400 and picks one.
- **Later:** subject and sky masks would reuse Omapix's BiRefNet and SAM.

## Architecture

Same stack and layout as Omacull and Omapix: Rust 2024, egui on wgpu,
Little CMS 2, GPL-3.0-or-later. A separate repository. Theme, hotkeys,
monitor profiles, the `Command` pattern, the headless harness and
packaging are copied from them, not shared as crates, as Omacull did.
Omacull's raw container reader and makernotes, sidecar splicing and
stacking are copied the same way.

```
crates/
  omalux-engine    folder scan, raw metadata and makernotes, the edit and
                   its settings, XMP read/write, lens data, stacks,
                   auto white balance, exposure, tone and level, export
                   to TIFF (no UI dependencies; testable headless)
  omalux-pipeline  the develop pipeline as wgpu compute shaders, with no
                   UI: run on a headless device by tests and export
  omalux-ai        ONNX Runtime, raw denoise
  omalux           the app: egui UI, panels, views, input, theme
```

- **Commands:** every action goes through one `Command` enum, so
  shortcuts, menus and headless test scripts never diverge, with an
  `OMALUX_SCRIPT` hook.
- **Testing the pipeline:** each stage is checked against a small CPU
  reference of the same maths on small images. Whether the GPU side runs
  in tests on a software Vulkan device (lavapipe) or only where there is
  a GPU is for M0 to settle.
- **Config:** `~/.config/omalux/config.toml`, written on first run with
  every setting explained: where exports go, the program Ctrl+E opens.
- **Omacull hands over** with its existing `developer = "omalux"`
  setting: Ctrl+E in Omacull starts `omalux <folder>`.

## Prior art

What exists, what to take and what to leave. From the 2026-10-08 scoping
session; M0 checks the specifics the design leans on.

| Tool | Take | Leave |
|------|------|-------|
| Lightroom Classic (proprietary) | Panel and slider names and behaviour, keys, Auto, Copy Settings checklist, before/after, raw AI Denoise, Camera Matching profiles | Catalogue, import, modules beyond Develop, subscription |
| darktable | Highlight recovery and demosaic ideas, reading Sony's embedded lens data | Too much going on; scene-referred controls exposed; its copy and paste |
| RawTherapee / ART | Demosaicing (AMaZE, RCD in `librtprocess`, GPL-3), the auto-matched tone curve, DCP profiles, CA correction | Density of settings |
| vkdt | The whole pipeline on the GPU; local laplacian; highlight inpainting | The node graph as the interface |
| RapidRAW (AGPL-3.0) | Closest in spirit: Rust, `rawler`, a WGSL pipeline, Lightroom-like sliders, sidecars | A web UI in Tauri; its code (AGPL), ideas only |
| Filmulator | Few controls that do a lot | Its film-development model as the only look |
| lensfun | The lens database | Its C library; we read the XML |
| RawNIND, RawForge | Raw denoise models | Their Python runtimes |
| Omacull | Raw reader, makernotes, sidecar splicing, colour management, stacks, preview prefetch | Culling, which stays there |
| Omapix | Conventions, ONNX Runtime on CUDA, exposure fusion, 16-bit TIFF with ICC, `--round-trip` | Everything that edits pixels by hand |

What doesn't exist on Linux, and is the reason for the project: a raw
developer with Lightroom's few, good controls and its keys, that runs
entirely on the GPU, denoises raws with AI locally, copies and pastes
settings the obvious way, keeps edits in sidecars beside the raws, and
looks and behaves like an Omarchy app.

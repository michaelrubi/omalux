# OmaLux roadmap

What's planned, in the order we plan to do it. [DESIGN.md](DESIGN.md)
covers the scope and architecture.

Status: scoped on 2026-10-08, and re-planned the same day on
[Lightcraft](https://github.com/storytold/lightcraft)'s engine (see
"Built on Lightcraft" in DESIGN.md). M0 started 2026-10-12: Lightcraft
vendored, the workspace and an empty window up, and the spikes run on
the laptop and Michael's frames. On 2026-10-13 what was left of it moved
to the milestones that need it first, and M0 closed on its last question,
why the GPU loupe's times jump between frames: the GPU's power states
(see M0). M1 started the same day.

Each milestone ends with something Michael can use on a real shoot. M1 is
the point where OmaLux replaces darktable for a clean, low-ISO shoot;
everything after it closes the gap to what darktable and Lightroom did
for him, in order of how often he needs it.

Most of the develop engine is Lightcraft's, so the milestones up to M3 are
mostly OmaLux's own parts (the folder, the sidecar, the UI, the handoff)
around controls that already work; M4 to M6 were what Lightcraft didn't
have, though since the plan it has added lens profiles and raw denoise
(see "Built on Lightcraft" in DESIGN.md), which makes M5 and M6 smaller.
The order of M3 to M6 can change once M1 is in daily use.

## M0. Vendor, spikes and scaffold

Bring Lightcraft in, and answer the questions the design depends on with
real A7 III raws before building UI: Michael's (low and high ISO, all
three primes) and raw.pixls.us's (the CC0 ILCE-7M3 files Lightcraft
benchmarks on, which tests can download).

Each item says where it stands. "On the laptop" is what needs Michael's
frames, the RTX 4070 or Lightroom: the tools for it are built, and the
commands are in AGENTS.md.

M0 closed on the GPU loupe's times (below), since M1's loupe is built on
them. The rest moved on 2026-10-13 to the milestone that needs it first,
since M1 needs none of it: the reference set, the 100% comparison and
the DRO test to M4, the lens comparison to M5, the denoise comparison to
M6, and the look of the handoff in Omapix to M1's export.

- **Vendor Lightcraft** at a pinned commit (`701f78e` or later) into
  `vendor/lightcraft/`: `geom`, `color`, `raster`, `tiff`, `raw`,
  `codecs`, `meta`, `develop`, `pipeline`, `gpu`, `scenes`, and the new
  `denoise`, `denoise-core` and `lensdb` if the spikes below keep them,
  with their licences, `NOTICE` and an `UPSTREAM.md`. Their tests pass
  in OmaLux's workspace. `codecs` trimmed of the formats OmaLux never
  reads or writes (WebP, JPEG XL, AVIF) if that's a small change.
  *Done* at `701f78e`, unchanged, plus `inpaint` (`pipeline` needs it)
  and `heif` (`codecs` names it). 984 tests pass, 8 ignored as upstream
  ignores them. Trimmed by features only: AVIF and JPEG XL raws off; JPEG
  XL in `codecs` stays (without it, an unused-import warning would want a
  local change), and WebP isn't behind a feature.
- **The reference set** (see "Quality bar" in DESIGN.md): 20 to 30 of
  Michael's frames, with the camera's JPEG, Lightroom Classic's renders
  (Adobe Color, and Camera Standard or Vivid), Capture One's default
  render (free trials) and his darktable export of each. Every spike
  below is judged on it. *Michael's to gather;* moved to M4, which
  judges the look against it.
- **Render real ARWs** through Lightcraft's pipeline from a small example
  binary: decode time, how the camera match fits each frame (CIEDE2000
  against the camera's JPEG), and how AHD and the highlight
  reconstruction hold up at 100% against Lightroom's and Capture One's
  renders of the same frame. If AHD loses detail, try RCD. *On the
  laptop:* the `spike` example does all of it but the looking (it writes
  the export, the render beside the camera's JPEG, and a 100% crop).
  Lightcraft's raw loading and per-file camera match are lifted into
  `omalux-engine` for it; CIEDE2000 is checked against Sharma's test
  data. On the laptop (2026-10-13), `scripts/fetch-corpus.sh` and
  `cargo test --workspace` put raw.pixls.us's two ILCE-7M3 files through
  it, the first real ARWs to go through OmaLux's code: both decode
  (compressed in 12 ms, uncompressed in 77 ms) and open with a camera
  look and a Kelvin white balance. Michael's own frames (2026-10-13, the
  Run Dinah picks, all DRO Auto): the render is darker than the camera's
  JPEG on every frame, and the gap follows the ΔE2000: the frame DRO
  barely touched is 0.7 L* darker at ΔE 2.0 overall (skin 1.7), the one
  it lifted most 6.1 darker at 5.3 (skin 12.6 darker at 10.2), while the
  sky, which DRO leaves alone, stays within 0.5. That's DESIGN.md's
  "DRO is matched" measured on his frames: the colour fits, the shadow
  lift is missing. *Moved to M4:* the 100% comparison with Lightroom and
  Capture One (and AHD against RCD), which needs the reference set, and
  a DRO Off frame beside a DRO Auto one of the same scene, to settle it.
- **His camera settings:** he shoots Standard and Vivid; confirm from the
  makernotes how his shoots split between them, and which DRO settings
  (Off, Auto, Lv1 to Lv5) they use, since DRO is matched (see "The look"
  in DESIGN.md). Read the Creative Style, which Lightcraft doesn't yet.
  *Read* (`omalux_engine::sony`: the Creative Style and the DRO level,
  from tags `0xb020` and `0xb025`), tested on made-up maker notes only.
  *On the laptop:* the `spike` example counts them over his shoots, which
  is also the first check that the A7 III writes them where expected.
  First count (2026-10-13, the picks of three shoots, 53 frames): all
  Standard, DRO Auto, and both read where expected.
- **The GPU on the laptop.** Lightcraft's GPU pipeline on the RTX 4070
  under Hyprland and Vulkan: slider update and loupe times against the
  16 ms and 60 ms budgets, full-size export time, VRAM. That its CPU and
  GPU results agree there too. *On the laptop,* in the `spike` example.
  First runs (2026-10-13, ISO 100 picks, FE 85): a drag is 1 to 4 ms
  on the GPU (9.5 on the CPU), and the two agree to 1 level in 255; the
  loupe is 17 to 163 ms on the GPU from frame to frame with no fallback
  to the CPU (29 to 68 on the CPU), and a second run put the slow ones on
  other frames, so it isn't the frames' content; still to explain. The
  export is about 1.9 s. *Done (2026-10-13):* it's the GPU's power
  states. The `idle` example renders the loupe back to back and after
  the GPU has sat idle: 23 ms back to back (17 for a portrait frame,
  which fits the loupe with fewer pixels, the other part of the spread),
  the same after 200 ms idle, and 100 to 250 ms after a second or more,
  for about a second of steady work, on mains with the performance
  profile. `spike` decodes each frame on the CPU first, so its GPU
  renders start cold. Something small for the GPU every 100 ms doesn't
  hold it up (the driver goes by load), so it can't be kept awake
  cheaply. A drag isn't touched: drafts from a cold GPU are 3 to 4 ms
  (9 to 10 on the CPU). So M1's loupe doesn't count on a render in 60
  ms after a pause: stepping shows the embedded preview at once and
  the neighbours' renders are prepared ahead, as DESIGN.md has it, and
  the first render after a pause comes in up to a quarter of a second
  later. Found meanwhile:
  the GPU writes 8-bit output only, so the 16-bit export renders on the
  CPU (what `spike` times), and Lightcraft refuses software adapters, so
  it can't be tried without a real GPU.
- **Lens data.** lensfun's corrections for the FE 35, 50 and 85 f/1.8
  through Lightcraft's `lensdb`, and Sony's embedded ones (Lightcraft
  reads them only for the ILCE-7RM4A so far); compare them, and
  Lightroom's profile correction, on frames with straight lines and a
  plain sky; pick the default. *Partly:* `lensdb` finds all three in
  lensfun's current `mil-sony.xml` and gives each distortion and TCA,
  and vignetting at every aperture but one: the FE 35 at f/1.8, whose
  steep falloff its polynomial fit can't follow closely enough, so it's
  dropped (worth an upstream issue). *On the laptop:* the `lens` example
  renders each frame with lensfun's correction and without, and reports
  Sony's table; trying Sony's on the A7 III means opening Lightcraft's
  ILCE-7RM4A-only gate (`vendor/arw_lens.rs`) on a test branch. First
  run (2026-10-13, 53 frames, all FE 85 f/1.8): lensfun matches the
  lens on every frame and gives it distortion and vignetting; Sony's
  table is there on every frame and changes from frame to frame at the
  same focal length and aperture, so it follows the focus distance,
  which lensfun's model doesn't. Over all four of his shoots (1326
  frames): 1118 on the FE 85 (169 at f/1.8), 208 on the FE 35, all at
  f/4 to f/8 (so the FE 35's missing f/1.8 vignetting hasn't come up
  yet), and none on the FE 50. *Moved to M5:* the FE 35's renders, Sony's
  table tried, and the pick.
- **Raw denoise.** RawNIND's Bayer model against RawForge's TreeNet (Light
  and Heavy) on ISO 3200 to 6400 frames, and both against Lightroom's
  Denoise: how they look at 100%, how long they take, how much VRAM.
  Pick one. Run it through Lightcraft's `denoise` crates (their wgpu
  executor) and through ONNX Runtime on CUDA, and keep Lightcraft's
  unless ONNX Runtime is much faster. *Not started:* needs the models,
  the laptop and Michael's high-ISO frames, which he has yet to shoot:
  his four shoots are at ISO 100 (1135 frames), 640 (143) and 800 (48).
  `omalux-ai` opens the system's ONNX Runtime, as Omacull's does. *Moved
  to M6:* nothing before it depends on the pick, and his shoots so far
  are at ISO 800 or below.
- **The sidecar.** Write `omalux:settings` and `xmp:Rating` with
  `xmp_merge` into sidecars Omacull wrote and darktable wrote; check
  Omacull still reads and writes the rating, and the rest of the file is
  untouched. *Done:* the edit goes in with `xmp_merge`; marks are written
  as Omacull writes them (the value's characters replaced), because
  `xmp_merge` would move the rating into OmaLux's own description.
  Tested on a darktable 5.6 sidecar and ones Omacull's own code wrote,
  and round-tripped through Omacull's `rate` example: edits and marks
  interleave, and darktable's history stays byte for byte.
- **The handoff.** 16-bit Linear ProPhoto TIFFs with their ICC profile
  (decided: see "Pipeline" in DESIGN.md) open right in Omapix, and
  `omapix --round-trip a.tif b.tif` opens several as tabs. *Done* up to
  the window: Omapix's own loader (`load_round_trip`) opens OmaLux's
  exports at 16 bits with the profile "ProPhoto RGB (linear)", samples
  unchanged, and its `--round-trip` opens each path given. *Moved to
  M1's export:* that they look right there, as tabs.
- **One colour engine:** whether Omacull's monitor-profile handling works
  on `moxcms`, so lcms2 isn't needed. *Done: yes.* Lightcraft's
  `DisplayProfile` does the converting; OmaLux adds Omacull's EDID
  reading. Against Omacull's lcms2, on three monitor EDIDs: within 1 LSB
  on all 4096 sRGB colours tried (mean ΔE2000 0.26, worst 1.0).
- **Scaffold.** The workspace (`omalux-engine`, `omalux-ai`, `omalux`
  beside the vendored crates), an empty themed window, with theme,
  hotkeys, `Command`, `OMALUX_SCRIPT` and the headless test harness
  carried over from Omacull. `make install`. `AGENTS.md` updated with the
  commands. *Done;* the window opens a folder by name only, for now.
  Installed on the laptop and seen on Hyprland (2026-10-13): the theme
  looks right.

Also found: Lightcraft has the ILCE-7M3's colour matrices measured from
its spectral sensitivities, so Temp and Tint are Kelvin already, not the
relative scale DESIGN.md expected (it's updated).

## M1. Develop basics

The minimum that replaces darktable for a clean shoot that needs no
denoising: open it, set exposure and white balance, send it to Omapix.
The controls are Lightcraft's; the work is OmaLux's shell around them.

Each item says where it stands. Built on 2026-10-13, Claude on the render
path and Antigravity on the parts ported from Omacull (the recent list,
marks, the filmstrip, export, the histogram), each on a branch of its own
merged into `m1`. Everything here has headless tests; none of it has
been tried by hand yet, which is Michael's to do from the installed
binary.

- Open a folder from the command line (so Omacull's Ctrl+E with
  `developer = "omalux"` works), a picker, or the recent list. Shows the
  picks; a filter shows all. *Done:* Ctrl+O, the recent shoots listed
  when none is open, Ctrl+W to close, Ctrl+L for the picks or everything
  (Lightroom's Enable Filters). A shoot reopens on the frame and filter
  it was left with (`~/.config/omalux/state.toml`); one with no picks
  yet opens on all of it.
- Filmstrip and loupe, stepping with the arrows: the embedded preview at
  once, the render after it, neighbours prepared ahead. *Done:* the next
  two frames and the previous one are developed ahead. Thumbnails are
  the embedded previews, cached in `~/.cache/omalux/thumbnails/`; the
  developed frame in the filmstrip (DESIGN.md's "Pipeline") isn't there
  yet. Nothing is converted to the monitor's profile yet: *left*, with
  Omacull's monitor watching to port.
- Marks: P, X, U, 0 to 5, into `xmp:Rating` as Omacull writes them.
  *Done,* on a disk thread; a mark that can't be written is taken back.
- The render worker pool (lifted from Lightcraft's pattern): drafts while
  dragging, full quality on release, on the GPU with the CPU fallback.
  *Done:* two threads decode and one renders, and what's wanted replaces
  what was waiting. `OMALUX_GPU=0` renders on the CPU.
- Lightcraft's per-file camera match as the starting look, until M4's
  pooled profile. *Done.*
- **White balance:** As Shot, Temp, Tint (Kelvin, through Lightcraft's
  spectral matrices for the ILCE-7M3), the eyedropper (W). *Done.* Temp's
  track is spaced by ratio, as Lightroom's is, not by Kelvin as
  Lightcraft's.
- **Light:** Exposure, Contrast, Highlights, Shadows, Whites, Blacks.
  *Done.*
- **Presence:** Vibrance, Saturation. *Done.*
- The histogram at the top of the right panel, with clipping warnings
  (J). *Done:* the render's own histogram; J or a click on a triangle
  colours blown highlights red and crushed shadows blue in the loupe.
- Before/after (`\`), 100% zoom (Z). *Done.* Z, or a click on the frame:
  a tap toggles and a hold is a look, a drag or the wheel pans, and
  stepping stays at 100% on the same spot, all as Omacull's zoom behaves.
  The current frame is decoded at full size when it's first zoomed, and
  only the window on screen is rendered from it; until that's in, the
  render that fits is shown enlarged. On a copy of one of Michael's
  frames: the first window (2532×1596) in 150 ms, the next after a
  slider nudge in 7.
- Sliders by mouse (drag, wheel, double-click to reset) and by key (`,`
  `.` to select, `=` `−` to nudge), Lightroom's behaviour as Lightcraft
  implements it, in Omarchy's theme. *Done,* but for typing a value into
  a slider, which Lightcraft has and this doesn't yet.
- Edits saved in the sidecar as DESIGN.md describes, with the process
  version; undo, redo, reset. The reference renders the process version
  guards, set up from the first build. *Done:* a drag is one step, saved
  when it ends, on the same disk thread as the marks; undo history is
  per frame, for the session. An edit made with a newer OmaLux is shown
  as a fresh raw and never written over. *Left:* the reference renders.
- **Export:** 16-bit TIFF with ICC into `<shoot>/omalux/` (set in
  `config.toml`); Ctrl+E opens them in Omapix, Ctrl+Shift+E just exports.
  From M0: that they look right in Omapix, as tabs. *Done* for the
  current frame (the selection is M2's): `export_folder` and `editor` in
  `config.toml`. A TIFF that's already there is never written over,
  since Omapix saves its retouching back into it and keeps its layers
  beside it; Ctrl+E opens the one that's there. *Open:* how a frame is
  exported again after its edit changes, which that rule leaves no way
  to do short of deleting the TIFF; and Michael's look at them in Omapix.

## M2. Copy, paste and stacks

What makes 10 to 50 picks quick: develop one, paste onto the others.
Lightcraft's copy groups, with copy-time choice, are already how DESIGN.md
wants it.

- Multi-select in the filmstrip (Ctrl+click, Shift+click, Shift+arrows,
  Ctrl+A), as Omacull has it.
- Ctrl+C copies everything; Ctrl+Shift+C opens the checklist of groups
  (restyled from Lightcraft's), which remembers the last choice; Ctrl+V
  pastes onto the selection; Ctrl+Alt+V pastes from the previous frame.
- Stacks by capture time and look among the picks, as Omacull makes
  them, collapsed in the filmstrip; a key pastes onto the whole stack.
- One step of undo for a paste, however many frames it reached.
- Exporting the selection.

## M3. Detail, crop and auto

Everything here is in Lightcraft's pipeline already: this milestone is
the panels, keys and checks on real frames.

- **Detail:** Sharpening (Amount, Radius, Detail, Masking) and Noise
  Reduction (Luminance, Color).
- **Lens:** Remove Chromatic Aberration (Lightcraft's automatic lateral
  CA) and Defringe, checked on f/1.8 frames.
- **Crop & Straighten** (R): aspect ratios, the rectangle, the angle and
  a line to straighten along, 90° turns. In the copy checklist.
- **Auto:** white balance (Ctrl+Shift+U), exposure, tone (Ctrl+U) and
  level (Lightcraft's Upright Level), each a button, and
  Shift+double-click on a slider.

## M4. The look and HDR

- **From M0, first:** Michael gathers the reference set (see M0), and
  shoots one scene twice in Standard, DRO Off and DRO Auto, to confirm
  that the camera match's gap on his frames is DRO's lift. The 100%
  comparison of the render with Lightroom's and Capture One's, AHD
  against RCD if AHD loses detail.
- **Camera match, fixed per camera:** the target is the camera's own
  JPEG. Two candidates, measured the same way on held-out frames
  (CIEDE2000 against the camera's JPEG, overall and on saturated, skin and
  sky pixels), and the closer one becomes the default:
  - the ILCE-7M3 profile fitted with Lightcraft's `calibrate` over
    Michael's shoots and bundled, one for Standard and one for Vivid,
    with its tone curve used too rather than fitted per file;
  - Adobe's Camera Standard and Camera Vivid DCPs for the ILCE-7M3 (from
    Adobe's free DNG Converter, read from where Michael put them, never
    shipped), through a DCP reader in OmaLux's own crate feeding the DNG
    colour model Lightcraft's pipeline already has. With them, Temp and
    Tint are measured Kelvin.

  Either way, at least as close to the camera as Lightroom's Camera
  Standard and Camera Vivid are.
- **DRO matched:** frames shot with DRO on open with its local shadow
  lift, on the HDR slider's local tone mapping, at a strength pooled per
  level (Lv1 to Lv5) or measured from the frame's JPEG (Auto), as part of
  the profile, with the slider at 0. Checked against the camera's JPEGs
  of DRO frames. Needs the HDR work below, so it lands with it.
- **Side by side** with Lightroom and Capture One on the reference set,
  at fit and at 100%; what loses goes back to its stage.
- **Flat**, one key away.
- **HDR:** the slider, on Lightcraft's local tone mapping, or exposure
  fusion of virtual exposures if that can't reach the look. Both tried
  on real frames.
- The process version bumped for edits made before this, which keep
  rendering as they did.

## M5. Lens profiles

- **From M0, first:** pick the default. lensfun's correction against
  Sony's embedded table (opening Lightcraft's ILCE-7RM4A-only gate,
  `vendor/arw_lens.rs`, on a test branch) and Lightroom's, on FE 35 and
  FE 85 frames with straight lines and a plain sky (the `lens` example;
  Michael's shoots have no FE 50 frames). Sony's table follows the focus
  distance, which lensfun's model doesn't (see M0).
- Profile corrections for ARWs: distortion and vignetting from lensfun's
  database (Lightcraft's `lensdb`) or Sony's embedded data (whichever
  the comparison picked as the default), found from the lens the raw names, fed to
  Lightcraft's Optics. On by default. Checked against Lightroom's
  correction of the same frames.
- Lateral CA from the profile where it has it, Lightcraft's automatic
  estimate where it doesn't.

## M6. AI Denoise

- **From M0, first:** the comparison of models and runtimes (see M0's
  "Raw denoise"), on ISO 3200 to 6400 frames Michael shoots for it.
- A button and a strength, on the raw's Bayer data before demosaicing,
  with the model the comparison picked, on whichever runtime it picked (Lightcraft's
  `denoise` crates on wgpu, or ONNX Runtime on CUDA). Its result cached on
  disk per frame; the model fetched by a script, as Omacull's and
  Omapix's are.
- What Denoise and Lightcraft's Sharpening and Noise Reduction do to each
  other, checked at 100% on real ISO 6400 frames.

## M7. Release

- Arch package: `packaging/arch/PKGBUILD` building `omalux-git`.
- README, CONTRIBUTING, screenshots; Lightcraft credited.
- `config.toml` with every setting explained.
- Omacull's README and `config.toml` mention `developer = "omalux"`.

## Ongoing: keeping up with Lightcraft

Lightcraft moves fast. Every so often, and before each milestone:

- Diff upstream since the pinned commit; take fixes to the vendored
  crates (Sony decoding and camera colour above all), leave the rest.
- Re-run the reference renders; bump the process version if a render
  changed.
- Offer OmaLux's own changes to the vendored crates back upstream, where
  they'd help Lightcraft (they're kept clean-room and MIT/Apache so they
  can be).

## Later

From the scoping interview, roughly in the order they were wanted. Most
are in Lightcraft already and need only a panel; those that aren't say so.

- **JPEG export** (Lightcraft's exporter has it).
- **Virtual copies**: two edits of one raw. Lightcraft keeps them in its
  catalogue; OmaLux would need them in the sidecar.
- **JPEGs and phone photos as input** (Lightcraft's codecs read them).
- **Local adjustments:** a subject mask and a linear gradient, then a
  brush (Lightcraft's masks; its subject and sky are heuristics, so the
  subject would come from Omapix's BiRefNet). A maybe.
- **HSL, the colour grading wheels and the tone curve** (Lightcraft's
  Color Mixer, Color Grading and Tone Curve).
- **Match Total Exposures** across a set.
- **A measured colour profile**: a ColorChecker shot in daylight and
  tungsten, made into a dual-illuminant DCP with `dcamprof`, for colour
  beyond the camera's own if Michael wants it (M4 reads DCPs already).
- **Other cameras:** Lightcraft decodes Nikon, Canon, Fuji and more;
  each would want a pooled camera-match profile and lens data.
- **Culling features** in OmaLux, if any turn out to be missed. Omacull
  stays the culler.

## Not planned

Healing and pixel retouching (Omapix's), a catalogue, keywords and
search, HDR output, Merge to HDR and panoramas, print, books, maps,
tethering, video, Lightcraft's MCP server and web build, and anything to
do with darktable. See "Out" in DESIGN.md. Any of these can be reopened
once the tool is in daily use.

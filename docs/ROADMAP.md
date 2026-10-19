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
(see M0). M1 started the same day, and M2, M3 and M4 after it; M5 on
2026-10-14.

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
  yet. Everything shown is in the monitor's colours where
  Hyprland takes it for sRGB and it isn't: which monitor the window is
  on comes from `hyprctl`, its profile from
  `~/.config/omalux/monitors.toml` or else its EDID (Omacull's monitor
  watching, ported); frames are rendered into its primaries, and the
  embedded previews and thumbnails converted. The laptop's panel
  (eDP-2) is found and gets its EDID's profile.
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
  track is Lightroom's, as Lightcraft measured it since (2026-10-15):
  7000 K in the middle, 2000 to 7000 K evenly to its left, logarithmic to
  50000 K on the right. Spacing it by ratio, as M1 first did, put 7000 K
  at 0.39 and 20000 K at 0.72 where Lightroom has 0.5 and 0.85.
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
  implements it, in Omarchy's theme. *Done,* with typing a value: click
  a slider's number, and Return, Tab or a click elsewhere applies what
  was typed, Esc keeps the old one.
- Edits saved in the sidecar as DESIGN.md describes, with the process
  version; undo, redo, reset. The reference renders the process version
  guards, set up from the first build. *Done:* a drag is one step, saved
  when it ends, on the same disk thread as the marks; undo history is
  per frame, for the session. An edit made with a newer OmaLux is shown
  as a fresh raw and never written over. The reference renders are the
  two CC0 ILCE-7M3 raws developed five ways (fresh, and edits that
  between them move every control M1 shows), kept as small PNGs in
  `crates/omalux-engine/testdata/reference/`; a test renders them again
  and fails on a change as small as 0.05 EV, saying to bump the process
  version. `OMALUX_REFERENCE` adds a folder of Michael's own frames,
  with their references beside them, outside the repo.
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

Each item says where it stands. Built on 2026-10-13, on `m2`: Claude on
the commands, the copy groups, paste and its undo, Antigravity on the
parts ported from Omacull and Lightcraft (the selection, stacks, the
checklist), each on a branch of its own. Everything here has headless
tests, and a script has copied, pasted and undone on copies of the
corpus raws; none of it has been tried by hand yet, which is Michael's
to do from the installed binary.

- Multi-select in the filmstrip (Ctrl+click, Shift+click, Shift+arrows,
  Ctrl+A), as Omacull has it. *Done,* with Ctrl+D for none. Unlike
  Omacull's, a plain step or click clears the selection, as in
  Lightroom, since here it's where a paste lands (see "Selecting" in
  DESIGN.md); so does changing the filter. The status bar says how many
  are selected.
- Ctrl+C copies everything; Ctrl+Shift+C opens the checklist of groups
  (restyled from Lightcraft's), which remembers the last choice; Ctrl+V
  pastes onto the selection; Ctrl+Alt+V pastes from the previous frame.
  *Done.* The groups are the panels there are (White Balance, Light,
  Presence); each later panel brings its own. What was copied lasts the
  session, across shoots. A paste reaches frames whose raws haven't
  been decoded, and saves each; a frame whose edit this OmaLux can't
  read is left alone, and said. A white balance copied as shot is
  pasted as each frame's own. Paste from Previous takes every setting
  of the frame the cursor came from, onto the current frame alone, as
  Lightroom's does.
- Stacks by capture time and look among the picks, as Omacull makes
  them, collapsed in the filmstrip; a key pastes onto the whole stack.
  *Done:* each raw's capture time and thumbnail are read from its
  headers when the shoot opens (800 frames in 24 ms from a warm disk;
  not timed cold, nor on one of Michael's shoots), and frames up to 30
  seconds apart that look alike are a stack, with no setting. G opens
  one out and closes it up (Omacull's key), and Ctrl+Shift+V pastes
  onto it. A stack is only the frames the filter lets through, so a
  paste never reaches one that isn't shown. *Open:* how his shoots
  stack, which only his frames can say; and that Ctrl+A with stacks
  closed up selects the frame standing for each and not the rest, so
  an export of "everything" needs the stacks opened out first
  (Lightroom's way, and a trap there too).
- One step of undo for a paste, however many frames it reached.
  *Done:* Ctrl+Z on the frame it was made from, or on any frame it
  reached, takes it back from all of them, and Ctrl+Shift+Z makes it
  again; a frame edited since keeps its edit and the paste under it.
- Exporting the selection. *Done:* Ctrl+E and Ctrl+Shift+E export the
  selection, or the current frame with none.

## M3. Detail, crop and auto

Nearly everything here is in Lightcraft's pipeline already: this
milestone is the panels, keys and checks on real frames. What wasn't is
Auto tone (below).

Each item says where it stands. Built on 2026-10-13, on `m3`: Claude on
the commands, the engine (`omalux_engine::crop` and `auto`), the copy
groups, and the crop drawn over the frame; Antigravity on the panels
(Detail, Lens Corrections, the Auto buttons, the crop tool's), each on
a branch of its own. Everything here has headless tests, and a script
has cropped, turned and run each Auto on copies of Michael's raws from
the built binary; none of it has been tried by hand yet, which is
Michael's to do from the installed binary.

- **Detail:** Sharpening (Amount, Radius, Detail, Masking) and Noise
  Reduction (Luminance, Color). *Done,* under Lightroom's names
  (Lightcraft's differ). A fresh raw opens with Lightcraft's raw
  defaults, which are Lightroom's: Sharpening at 40 and Color at 25,
  which is what a double-click goes back to. Sharpening and Noise
  Reduction are copy groups of their own. The develop panel scrolls
  now, under the histogram. *Open:* how they look at 100% on his
  frames, which is for his eye.
- **Lens:** Remove Chromatic Aberration (Lightcraft's automatic lateral
  CA) and Defringe, checked on f/1.8 frames. *Done,* in a Lens
  Corrections panel, which is a copy group; M5's profile corrections
  join it. The check (the `fringe` example, on copies of nine of the
  176 frames he has at f/1.8, all on the FE 85): the lens's lateral CA
  is small, blue 0.4 to 1.4 pixels at the corners of the full-size
  frame and red under a quarter of one, so Remove Chromatic Aberration
  changes little on it; fringed pixels beside blown highlights are few
  (about 40 in a million at most, and the count takes in real greens
  and pinks), and Defringe at 8 takes the green ones down by a half to
  three quarters, most visibly the rims of out-of-focus highlights
  through leaves. *Open:* none of the nine fringes badly,
  so the hard case (a dark edge against a blown sky at f/1.8) hasn't
  been found on his shoots yet.
- **Crop & Straighten** (R): aspect ratios, the rectangle, the angle and
  a line to straighten along, 90° turns. In the copy checklist. *Done*
  as DESIGN.md's "Crop & Straighten" has it: the frame whole with the
  crop over it, the picture turning under an upright crop as in
  Lightroom (Lightcraft's own tool turns the crop over a still
  picture); handles, moving, turning from outside, Ctrl+drag for a
  line, the lock (A), X, Return, Esc; Lightroom's aspect list, Angle,
  Auto, Rotate Left and Right (Ctrl+[ and Ctrl+]) and Reset
  (Ctrl+Alt+R) in the panel. A drag is one step, saved when it ends.
  The crop is a copy group, in Ctrl+C's everything too; quarter turns
  are never pasted. *Open:* the filmstrip's thumbnails are the
  camera's JPEGs, so they show neither the crop nor a turn until M1's
  developed thumbnails are there; a crop pasted onto a frame that
  hasn't been decoded yet isn't brought inside a frame of another
  shape until it's next cropped (its shape isn't known until then);
  and there's no overlay but thirds (a finer grid while turning).
- **Auto:** white balance (Ctrl+Shift+U), exposure, tone (Ctrl+U) and
  level (Lightcraft's Upright Level), each a button, and
  Shift+double-click on a slider. *Done.* White balance, exposure and
  level are Lightcraft's. Auto tone isn't in Lightcraft, whose Auto
  leaves Whites and Blacks at 0, so it's OmaLux's own (see "Auto" in
  DESIGN.md): the first version set Whites as high and Blacks as low as
  they'd go without clipping, and on his frames that came to +100 and
  -100 for very little change, so it brings them in only. On eleven
  frames (nine of his, the two CC0 ones): Whites between 0 and -99,
  below -55 on seven of them (one backlit frame goes from 15.5% of it
  blown to 0.3% at -63); Blacks 0 on all of them (Lightcraft's Blacks
  moves the dark end very little, so it's seldom what opens a shadow).
  It takes 35 to 50 ms on the loupe's source, and level about 65. Each says so in the status bar when it has
  nothing to do. *Open:* whether Whites that far down is what he
  wants from Auto on a backlit frame (it only reaches the top of the
  histogram: the rest of the picture hardly moves), which is for his
  eye; and Auto white balance is kept as the Kelvin it found, so a
  paste gives other frames that Kelvin and not an Auto of their own.

## M4. The look and HDR

Each item says where it stands. Started on 2026-10-13, on `m4`: Claude on
the look itself (the profile as a setting, the camera's fixed profile,
DRO's lift, HDR's folding, the panel and keys, the process version),
Antigravity on three parts with nothing shared (the DCP reader, exposure
fusion ported from Omapix with the `hdr` example, the `quality` example
that compares against the reference set), each on a branch of its own.
Everything here has headless tests, and the profile and the lift are
measured on Michael's raws (read, never written to); none of it has been
tried by hand yet, and what's left is mostly his: the reference set, a
DRO Off frame beside a DRO Auto one, Adobe's DCPs, and his eye.

- **From M0, first:** Michael gathers the reference set (see M0), and
  shoots one scene twice in Standard, DRO Off and DRO Auto, to confirm
  that the camera match's gap on his frames is DRO's lift. The 100%
  comparison of the render with Lightroom's and Capture One's, AHD
  against RCD if AHD loses detail. *Michael's to gather.* The tool is
  built: the `quality` example takes a folder of raws with Lightroom's,
  Capture One's and darktable's TIFFs of each beside them, and writes
  them side by side with OmaLux's render and the camera's JPEG, at fit
  and at 100%, with each one's ΔE2000 against the camera (the naming is
  in AGENTS.md). The gap being DRO's is as good as shown without the
  pair (see "DRO matched"); RCD waits for the 100% comparison to ask
  for it.
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

  *The first is done, for Standard, and is what a frame opens with.* A
  profile is per camera and Creative Style and holds the whole look
  (matrix, hue/saturation table, tone curve, chroma curve); the
  `calibrate` example fits them, and the ILCE-7M3's Standard is built
  in, fitted over his four shoots (1376 raws). All of them have DRO
  Auto, which hides the camera's own curve, so its tone curve is Sony's
  for Standard without DRO, which Lightcraft pooled from ten bodies;
  `calibrate` pools the camera's own once there are five frames shot
  with DRO off. Measured with the `match` example, in the middle of the
  frame, on a shoot the profile wasn't fitted to (Run Dinah Barbie, 158
  frames): the per-frame fit M1 opened with comes to ΔE2000 4.51 from
  the camera's JPEG and 4.4 L* darker; the profile alone 4.61 and 3.6
  darker; with DRO's lift, which is Camera match, 3.16 and 0.8 darker
  (saturated 5.18 from 7.67, skin 3.66 from 5.76, sky 5.33 from 6.67).
  On every sixth raw of all four shoots (229 frames, their neighbours
  fitted to): 5.01, 4.88 and 4.04. On the two CC0 raws, shot with DRO
  off on other bodies, the per-frame fit is the closer by 0.4 (3.6 and
  4.0 against 4.0 and 4.4): it fits each frame's own tone curve, which
  a fixed one can't, and is the price of two frames of a burst looking
  alike. Developing with the profile and measuring the lift takes 43 ms
  of a 205 ms decode. There's no Vivid profile: he has shot nothing in
  it, and a frame in a style without one gets the look fitted to its
  own JPEG, with the lift. *The second is half done:* the DCP reader is
  built (`omalux_engine::dcp`, tested on DCPs made in the tests) and
  isn't wired into the develop path, which wants real ones to be
  checked against: Adobe's Camera Standard and Camera Vivid for the
  ILCE-7M3, in `~/.config/omalux/dcp/`. *Open:* the DCP measured beside
  the profile; both against Lightroom's Camera Standard, which needs
  the reference set; a Vivid profile; and the saturated colours, which
  are the furthest off (ΔE 5 to 6) and 1.5 to 4 L* dark.
- **DRO matched:** frames shot with DRO on open with its local shadow
  lift, on the HDR slider's local tone mapping, at a strength pooled per
  level (Lv1 to Lv5) or measured from the frame's JPEG (Auto), as part of
  the profile, with the slider at 0. Checked against the camera's JPEGs
  of DRO frames. Needs the HDR work below, so it lands with it. *Done*
  for what his shoots have, which is Auto on every frame: the lift is
  measured when a raw is decoded, by developing it small with Shadows
  at one value after another and taking the one closest in lightness
  to the JPEG, and Camera match renders it under the Shadows slider,
  which stays at 0. On the 229 held-out frames it finds none on 56
  (where the render was already as light as the JPEG, within 0.1 L*
  on average), and up to 110 on the rest; the frames DRO lifted most
  go from 10 L* darker than the JPEG to 1.5. Frames of one burst get
  lifts a few points apart (30 to 38 over one, 58 on every frame of
  another). The fixed levels are measured the same way: he has no frame
  at one to pool a strength from. *Open:* whether a few points of lift
  between the frames of a burst show when settings are pasted across
  it (it could be rounded, or taken from the stack's first frame);
  the frames lifted most are still ΔE 9 from the JPEG pixel for pixel,
  since Lightcraft's local tone mapping isn't Sony's, which is for his
  eye; the levels; and the DRO Off and Auto pair, which would show the
  lift against the camera's own curve.
- **Side by side** with Lightroom and Capture One on the reference set,
  at fit and at 100%; what loses goes back to its stage. *Waits for the
  reference set;* the `quality` example makes the sheets.
- **Flat**, one key away. *Done:* F, or a click on the profile's name
  at the top of the develop panel (Lightroom's "Profile", which names
  it after the Creative Style: "Camera Standard"). Flat is Camera
  match's colour without the camera's tone curve or DRO's lift, on
  Lightcraft's own curve for raws. The profile is a setting, so it's
  undone, copied and pasted (a group of its own) and saved like the
  rest. *Open:* whether Lightcraft's curve is flat enough to be worth
  the name, which is for his eye.
- **HDR:** the slider, on Lightcraft's local tone mapping, or exposure
  fusion of virtual exposures if that can't reach the look. Both tried
  on real frames. *Done* as the first: HDR is a slider of its own
  between Light and Presence, 0 to 100, taking that much from
  Highlights and giving it to Shadows under their sliders, past their
  ends (Shadows at 100 with HDR at 100 is 200 to the pipeline); a copy
  group of its own. Both tried, with the `hdr` example, which renders
  HDR at 0, 50 and 100 beside a fusion of five exposures two stops
  either side: on four of his frames HDR at 100 brings the darkest
  quarter up from L* 12 to 27 and fusion to 19, with no halos to see
  at 1600 pixels, and fusion takes about five times as long as one
  render. *Open:* which
  he likes, on his frames (the sheets are for his eye); whether 100
  should reach further or bring some contrast back; and fusion as a
  stage of the pipeline, which it isn't, if he prefers it.
- The process version bumped for edits made before this, which keep
  rendering as they did. *Done:* 2. The look is carried by the profile
  setting, so an edit from 1 keeps Lightcraft's own profile id and
  with it the look fitted to its frame's JPEG, with no lift; its raw is
  decoded for that look. The reference renders M1 wrote are checked
  against unchanged, beside new ones of what a frame opens with now.
  Reset, or choosing a profile, brings such a frame to Camera match.

Also found: Lightcraft has moved 256 commits since the pin (looked at
`b3d392b4`, 2026-10-13). In the vendored crates it's other cameras
(Canon, Nikon, Olympus, Pentax), Kelvin from Sony's presets for bodies
without measured matrices (the ILCE-7M3 has them) and its own UI's
controls, so nothing was taken. The one change that bears on M4 is in
the per-frame fit, which is lifted, not vendored: the camera's toe
fitted from dark JPEG tones (`c11764a1`). It's worth lifting when the
ILCE-7M3's own tone curve is pooled from DRO Off frames.

## M5. Lens profiles

Each item says where it stands. Started on 2026-10-14, on `m5`: Claude on
the skeleton (the source of a frame's correction, `lens_profiles::Source`),
the default, the panel and the render path; Antigravity on Sony's tables
(`omalux_engine::sony_lens`) and the comparison (`lens_check` and the
`lens` example), each on a branch of its own. Everything here has
headless tests, and the comparison ran on copies of 13 of Michael's raws
(FE 35 at f/5.6 and f/8, FE 85 at f/1.8, f/2.5 and f/4); none of it has
been tried by hand yet.

- **From M0, first:** pick the default. lensfun's correction against
  Sony's embedded table (opening Lightcraft's ILCE-7RM4A-only gate,
  `vendor/arw_lens.rs`, on a test branch) and Lightroom's, on FE 35 and
  FE 85 frames with straight lines and a plain sky (the `lens` example;
  Michael's shoots have no FE 50 frames). Sony's table follows the focus
  distance, which lensfun's model doesn't (see M0). *Picked: Sony's,*
  without opening the gate: OmaLux reads the tables in its own crate.
  His maker notes have the camera's lens compensation on Auto for all
  three, so the camera's JPEG is corrected by Sony and the `lens` example
  measures each correction against it (patches matched across the frame,
  a scale and a shift taken out). Distortion, median residual: Sony 0.21
  to 0.27 px at every radius and at the JPEG's own scale (1.000), lensfun
  0.26 to 0.37, none 0.36 to 0.48 (at the JPEG's 1616 pixels; the FE 35's
  corners 1.7 px uncorrected, 0.7 with lensfun's, 0.25 with Sony's).
  Darktable's knot radii put Sony's at 0.33 px on the FE 35, Lightcraft's
  at 0.23, so Lightcraft's are kept. *Open:* Lightroom's correction of
  the same frames, beside the raws as `NAME.lightroom-lens.tif` (the
  example takes it), which is Michael's to export; and vignetting, which
  the JPEG can't settle: Sony's table brings the corners up by about half
  what lensfun's does (0.9 stops at f/1.8 on the FE 85, 0.8 on the FE 35
  at f/5.6), and the JPEG's corners are darker than either (the camera's
  own shading compensation looks partial, and DRO lifts locally), so how
  much is right is for his eye; the Vignetting amount goes to 200.
- Profile corrections for ARWs: distortion and vignetting from lensfun's
  database (Lightcraft's `lensdb`) or Sony's embedded data (whichever
  the comparison picked as the default), found from the lens the raw names, fed to
  Lightcraft's Optics. On by default. Checked against Lightroom's
  correction of the same frames. *Done* but for Lightroom's: a fresh
  raw opens with Enable Profile Corrections on, which edits saved before
  keep off (so they render as they did, and the process version stays
  at 2). The Lens Corrections panel has the checkbox, the lens found ("No
  profile for this lens" without one) and Lightroom's Amount for
  Distortion and Vignetting. lensfun's correction, the fallback, is
  enlarged to fill the frame as Lightroom's is (it left the corners
  empty on pincushion); Sony's has that already. Reference renders of a
  fresh raw with them on are new beside the old.
- Lateral CA from the profile where it has it, Lightcraft's automatic
  estimate where it doesn't. *Done:* Sony's CA table warps the red and
  blue planes (under a pixel at the corners of the full frame on his
  lenses), and where the profile does that, Remove Chromatic
  Aberration's estimate is left out rather than added to it.

## Before M6: a look without the edit, HDR's ends, menus and keys

Michael's asks of 2026-10-14, while there are no high-ISO frames to
denoise. On `m6-prep`: Claude on the skeleton, the panel switches and
HDR's ends; Antigravity on the legend and the menus, each on a branch of
its own. None of it tried by hand yet.

- A look without the adjustments. `\` (Lightroom's Before/After) was
  already there for all of them; each panel but the Profile now has
  Lightroom's switch on its heading for one at a time. *Done.* P stays
  Pick, as in Lightroom.
- More control of HDR: Highlights and Shadows under its Amount. *Done:*
  shown always, not behind a toggle.
- Right-click menus on the frame and the filmstrip, each item with its
  key. *Done:* a right-click in the filmstrip goes to the frame first,
  keeping the selection if the frame is in it, as Lightroom does.
- The keys' legend on Ctrl+/ (Lightroom's Develop Shortcuts), showing
  the keys in use, rebindings and all. *Done*, with the clicks and the
  crop tool's keys that aren't commands.

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

craft-watch (`~/dev/craft-watch/lightcraft-omalux.md`) lists what
upstream's new commits might bring, for going through here.

**Re-synced before M6** (2026-10-15, on `lightcraft-sync`): `701f78e` to
`0da998f2`, 955 commits (see UPSTREAM.md). The vendored crates' fixes are
in: never-crash hardening in raw decoding, TIFF reading, the demosaic,
the blur and defringe, and a faster AHD. Taken with them, from
craft-watch's notes:

- *Done:* Temp's track is Lightroom's, measured by Lightcraft (see M1's
  white balance).
- *Done:* noise reduction is sized to the full frame, so the loupe and
  the filmstrip look like the export scaled down. Exports are unchanged,
  so the process version stays at 2; the reference renders were written
  again. No references of Michael's own frames (`OMALUX_REFERENCE`) had
  been made yet, so none needed writing again.
- *Done* (2026-10-15, on `lightcraft-engine`): the modules lifted from
  `engine` (`raw`, `camera_preview`, `camera_profiles`, `lens_profiles`,
  `crop`) brought up to `0da998f2`, for the ILCE-7M3; each one's header
  says what was taken. The reference renders are unchanged, so the
  process version stays at 2.
  - The camera look's colour step runs sixteen pixels at a time
    (`6467d543`), with the same bits (a test compares every kind of
    pixel with the one-by-one path). On a full-size frame (24 MP, the
    Standard profile's table), best of 27 runs: 1.79 s to 0.63 s on one
    thread, 0.47–0.51 to 0.17–0.18 s on four, 137–161 to 67–100 ms on all twenty
    (the machine busy with other builds). A full-size decode no longer
    copies its picture to fit it (`0d13a7d5`): 288 MB and about 40 ms
    less. The `spike` example's own times moved within their noise on
    that machine (full-size decode, best of two runs: 540 to 515 ms and
    673 to 557 ms; export 2.25 s to 2.48 s).
  - The camera look matched through the lens warp the camera applied to
    its JPEG (`c854e172`). An ILCE-7M3's raw carries no warp of its own,
    so OmaLux's is built from Sony's tables (`sony_lens::geometry`); the
    warped sensor proxy is taken where it lines up with the JPEG better.
    On the corpus raws (Distortion Comp. Off) it never does. On copies
    of 23 of Michael's frames (lens compensation Auto) it lines up much
    better on the FE 35 (0.47–0.69 to 0.83–0.87 at the profile's 192 px)
    and barely on the FE 85 (0.005 at most). It's used for `calibrate`'s
    pairs only: through it the look fitted to a frame changed on 12 of
    the 23 and DRO's lift by 2 to 5 on 7, which would change the export
    of edits under both process versions. *Done* (2026-10-19, on `pv3`):
    process version 3, with the per-frame fit and the lift through it,
    the Standard profile fitted again through it, and `4ddbb5a3`
    (below). An edit keeps the version it was made with when it's saved
    again (six of the Run Dinah Barbie picks have edits from 2), and the
    bundled profiles carry the version they're used from, so 2's
    Standard stays. On the corpus raws 3 renders as 2 did; on a copy of
    an FE 35 frame (MJR09822) Camera match's lift moves, up to 3 levels
    at 256 px. The Standard profile fitted again (the same 1376 raws)
    comes closer to the JPEGs by a little: on Run Dinah Barbie with `match`,
    both under 3, Camera match 2.69 to 2.67 ΔE2000 (saturated 3.99 to 3.97,
    skin 3.15 to 3.13, sky 3.95 to 3.86). *Open:* no way yet to bring an edit up to the newest
    version (Lightroom's Update process version).
  - `match` on the corpus raws gives the same before and after: fitted
    10.04, profile and camera 10.40 (mean ΔE2000). That's far from
    M4's numbers because since M5 a fresh raw opens with profile
    corrections on and these JPEGs have none: with them off it's 3.76
    and 4.33. *Open:* `match` should develop a frame whose JPEG isn't
    corrected without them.
  - Turning a crop shrinks it about its centre, keeping its shape,
    instead of swapping it for the largest crop of that shape
    (`053b7420`, in the vendored `geom` since the re-sync; a test now
    has it). The whole frame still follows the largest crop and grows
    back towards 0°; a crop of the user's own never grows. That's
    Lightroom's Constrain to Image as far as known. *Open, for Michael
    by hand:* a crop dragged in from a corner and turned out and back in
    one drag stays at its smaller size (Lightcraft's does too); whether
    Lightroom's grows back.
  - *Not taken:* the smallest embedded JPEG that covers a thumbnail
    (`14e0d128`): an ILCE-7M3's ARW holds only its 1616 × 1080 preview
    and a 160 × 120 thumbnail, so it's always the 1616. The frame's crop
    factor for lensfun's profile in an in-camera crop mode (`4ddbb5a3`):
    an APS-C frame's corners get the full frame's correction now, and
    fixing it changes the export of such an edit, so it's taken from
    process version 3 on. Everything else in those files since `701f78e`
    is other cameras, DNGs, merges, previews read from a catalogue, or
    DNG writing.
- *Waiting:* M4's toe fit (`c11764a1`), for when there are DRO Off frames.
- *Later, maybe:* Lightcraft now decodes Sony's lens tables itself
  (`RawImage::lens_metadata`) but applies them only on the ILCE-7RM4A.
  OmaLux's own reader stays. M5's measurements of Sony's correction on
  the ILCE-7M3 against its JPEGs could go upstream with an offer to open
  its gate for this body.
- *Not taken:* Dehaze's zoomed proxy (OmaLux has no Dehaze), DNG and
  HDR-merge writing, spot healing, mask overlays, and export and command
  fixes in `engine`, whose OmaLux equivalents are its own.

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

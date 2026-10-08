# OmaLux roadmap

What's planned, in the order we plan to do it. [DESIGN.md](DESIGN.md)
covers the scope and architecture.

Status: scoped on 2026-10-08, and re-planned the same day on
[Lightcraft](https://github.com/storytold/lightcraft)'s engine (see
"Built on Lightcraft" in DESIGN.md). Nothing built yet; M0 is next.

Each milestone ends with something Michael can use on a real shoot. M1 is
the point where OmaLux replaces darktable for a clean, low-ISO shoot;
everything after it closes the gap to what darktable and Lightroom did
for him, in order of how often he needs it.

Most of the develop engine is Lightcraft's, so the milestones up to M3 are
mostly OmaLux's own parts (the folder, the sidecar, the UI, the handoff)
around controls that already work; M4 to M6 are what Lightcraft doesn't
have. The order of M3 to M6 can change once M1 is in daily use.

## M0. Vendor, spikes and scaffold

Bring Lightcraft in, and answer the questions the design depends on with
real A7 III raws before building UI: Michael's (low and high ISO, all
three primes) and raw.pixls.us's (the CC0 ILCE-7M3 files Lightcraft
benchmarks on, which tests can download).

- **Vendor Lightcraft** at a pinned commit into `vendor/lightcraft/`:
  `geom`, `color`, `raster`, `tiff`, `raw`, `codecs`, `meta`, `develop`,
  `pipeline`, `gpu`, `scenes`, with their licences, `NOTICE` and an
  `UPSTREAM.md`. Their tests pass in OmaLux's workspace. `codecs` trimmed
  of the formats OmaLux never reads or writes (WebP, JPEG XL, AVIF) if
  that's a small change.
- **Render real ARWs** through Lightcraft's pipeline from a small example
  binary: decode time, how the camera match fits each frame, and how AHD
  holds detail at 100% against darktable's export of the same frame.
- **The GPU on the laptop.** Lightcraft's GPU pipeline on the RTX 4070
  under Hyprland and Vulkan: slider update and loupe times against the
  16 ms and 60 ms budgets, full-size export time, VRAM. That its CPU and
  GPU results agree there too.
- **Lens data.** Read Sony's embedded corrections from the ARWs and
  lensfun's for the FE 35, 50 and 85 f/1.8; check both can be expressed as
  Lightcraft's `Warp` and vignetting; compare them on frames with straight
  lines and a plain sky; pick the default.
- **Raw denoise.** RawNIND's Bayer model against RawForge's TreeNet (Light
  and Heavy) on ISO 3200 to 6400 frames: how they look at 100%, how long
  they take on CUDA and on the CPU, how much VRAM. Pick one. Write down
  how the Bayer data has to be packed, and where in `raw`'s decode it
  goes.
- **The sidecar.** Write `omalux:settings` and `xmp:Rating` with
  `xmp_merge` into sidecars Omacull wrote and darktable wrote; check
  Omacull still reads and writes the rating, and the rest of the file is
  untouched.
- **The handoff.** 16-bit TIFFs with an ICC profile, Linear ProPhoto and
  ProPhoto with its curve, open right in Omapix, and `omapix --round-trip
  a.tif b.tif` opens several as tabs. Pick the one to export.
- **One colour engine:** whether Omacull's monitor-profile handling works
  on `moxcms`, so lcms2 isn't needed.
- **Scaffold.** The workspace (`omalux-engine`, `omalux-ai`, `omalux`
  beside the vendored crates), an empty themed window, with theme,
  hotkeys, `Command`, `OMALUX_SCRIPT` and the headless test harness
  carried over from Omacull. `make install`. `AGENTS.md` updated with the
  commands.

## M1. Develop basics

The minimum that replaces darktable for a clean shoot that needs no
denoising: open it, set exposure and white balance, send it to Omapix.
The controls are Lightcraft's; the work is OmaLux's shell around them.

- Open a folder from the command line (so Omacull's Ctrl+E with
  `developer = "omalux"` works), a picker, or the recent list. Shows the
  picks; a filter shows all.
- Filmstrip and loupe, stepping with the arrows: the embedded preview at
  once, the render after it, neighbours prepared ahead.
- Marks: P, X, U, 0 to 5, into `xmp:Rating` as Omacull writes them.
- The render worker pool (lifted from Lightcraft's pattern): drafts while
  dragging, full quality on release, on the GPU with the CPU fallback.
- Lightcraft's per-file camera match as the starting look, until M4's
  pooled profile.
- **White balance:** As Shot, Temp, Tint (relative, as Lightcraft has
  them for uncalibrated ARWs), the eyedropper (W).
- **Light:** Exposure, Contrast, Highlights, Shadows, Whites, Blacks.
- **Presence:** Vibrance, Saturation.
- The histogram at the top of the right panel, with clipping warnings
  (J).
- Before/after (`\`), 100% zoom (Z).
- Sliders by mouse (drag, wheel, double-click to reset) and by key (`,`
  `.` to select, `=` `−` to nudge), Lightroom's behaviour as Lightcraft
  implements it, in Omarchy's theme.
- Edits saved in the sidecar as DESIGN.md describes, with the process
  version; undo, redo, reset. The reference renders the process version
  guards, set up from the first build.
- **Export:** 16-bit TIFF with ICC into `<shoot>/omalux/` (set in
  `config.toml`); Ctrl+E opens them in Omapix, Ctrl+Shift+E just exports.

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

- **Camera match, pooled:** the ILCE-7M3 profile fitted with Lightcraft's
  `calibrate` over Michael's shoots and bundled, with its tone curve used
  too rather than fitted per file, so a burst with the same settings looks
  the same. Checked against the camera's JPEGs as Lightcraft checks its
  7M4 profile (ΔE on held-out frames).
- **Flat**, one key away.
- **HDR:** the slider, on Lightcraft's local tone mapping, or exposure
  fusion of virtual exposures if that can't reach the look. Both tried
  on real frames.
- The process version bumped for edits made before this, which keep
  rendering as they did.

## M5. Lens profiles

- Profile corrections for ARWs: distortion and vignetting from lensfun's
  database or Sony's embedded data (whichever M0 picked as the default),
  found from the lens the raw names, fed to Lightcraft's Optics. On by
  default.
- Lateral CA from the profile where it has it, Lightcraft's automatic
  estimate where it doesn't.

## M6. AI Denoise

- A button and a strength, on the raw's Bayer data before demosaicing,
  with the model M0 picked, on CUDA through ONNX Runtime. Its result
  cached on disk per frame; the model fetched by a script, as Omacull's
  and Omapix's are.
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
- **DCP profiles**: Adobe's own (from the DNG Converter) or made with
  `dcamprof`. Not in Lightcraft, by its rules.
- **Measured white balance in Kelvin**, once the A7 III is calibrated.
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

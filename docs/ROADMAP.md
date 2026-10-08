# OmaLux roadmap

What's planned, in the order we plan to do it. [DESIGN.md](DESIGN.md)
covers the scope and architecture.

Status: scoped on 2026-10-08. Nothing built yet; M0 is next.

Each milestone ends with something Michael can use on a real shoot. M1 is
the point where OmaLux replaces darktable for a clean, low-ISO shoot;
everything after it closes the gap to what darktable and Lightroom did
for him, in order of how often he needs it. The order of M3 to M6 can
change once M1 is in daily use.

## M0. Spikes and scaffold

Answer the questions the design depends on, with a real A7 III shoot
(low and high ISO, all three primes), before building UI.

- **Raw decode.** `rawler` on real ARWs: how long a 24 MP frame takes to
  decode, the black and white levels and the colour matrix it gives,
  against exiftool's.
- **The GPU pipeline.** A wgpu compute spike: demosaic, white balance,
  matrix and a tone curve on a 24 MP frame on the RTX 4070 laptop. How
  long at screen size (the target is a slider drag in a frame) and at
  full size in tiles, and how much VRAM it holds. Which demosaic (RCD or
  AMaZE) is worth porting first.
- **Testing the pipeline.** Whether wgpu runs on a software Vulkan device
  (lavapipe) in `cargo test`, or the GPU tests only run where there's a
  GPU and the CPU reference carries the rest.
- **Lens data.** Read Sony's embedded corrections from the ARWs and
  lensfun's for the FE 35, 50 and 85 f/1.8. Compare them on frames with
  straight lines and a plain sky; pick the default.
- **Raw denoise.** RawNIND's Bayer model against RawForge's TreeNet (Light
  and Heavy) on ISO 3200 to 6400 frames: how they look at 100%, how long
  they take on CUDA and on the CPU, how much VRAM. Pick one. Write down
  how the Bayer data has to be packed for it.
- **The sidecar.** Splice an `omalux:` description into sidecars Omacull
  wrote and darktable wrote; check Omacull still reads and writes the
  rating, and the rest of the file is untouched.
- **The handoff.** A 16-bit Linear ProPhoto TIFF with its ICC profile,
  written by `omalux-engine`, opens right in Omapix, and `omapix
  --round-trip a.tif b.tif` opens several as tabs.
- **Scaffold.** The workspace (`omalux-engine`, `omalux-pipeline`,
  `omalux-ai`, `omalux`), an empty themed window, with theme, hotkeys,
  `Command`, `OMALUX_SCRIPT` and the headless test harness carried over
  from Omacull. `make install`. `AGENTS.md` updated with the commands.

## M1. Develop basics

The minimum that replaces darktable for a clean shoot that needs no
denoising: open it, set exposure and white balance, send it to Omapix.

- Open a folder from the command line (so Omacull's Ctrl+E with
  `developer = "omalux"` works), a picker, or the recent list. Shows the
  picks; a filter shows all.
- Filmstrip and loupe, stepping with the arrows, neighbours prepared
  ahead.
- Marks: P, X, U, 0 to 5, into `xmp:Rating` as Omacull writes them.
- The pipeline: decode, highlight recovery, white balance, demosaic, the
  camera's matrix, the tone stage and output, on the GPU.
- A neutral starting look, as a stand-in until M3's camera match.
- **White balance:** As Shot, Temp, Tint, the eyedropper (W).
- **Light:** Exposure, Contrast, Highlights, Shadows, Whites, Blacks.
- **Presence:** Vibrance, Saturation.
- The histogram at the top of the right panel, with clipping warnings
  (J).
- Before/after (`\`), 100% zoom (Z).
- Sliders by mouse (drag, wheel, double-click to reset) and by key (`,`
  `.` to select, `=` `−` to nudge).
- Edits saved in the sidecar as DESIGN.md describes, with the process
  version; undo, redo, reset.
- **Export:** 16-bit Linear ProPhoto TIFF with ICC into `<shoot>/omalux/`
  (set in `config.toml`); Ctrl+E opens them in Omapix, Ctrl+Shift+E
  just exports.

## M2. Copy, paste and stacks

What makes 10 to 50 picks quick: develop one, paste onto the others.

- Multi-select in the filmstrip (Ctrl+click, Shift+click, Shift+arrows,
  Ctrl+A), as Omacull has it.
- Ctrl+C copies everything; Ctrl+Shift+C opens the checklist of groups,
  which remembers the last choice; Ctrl+V pastes onto the selection;
  Ctrl+Alt+V pastes from the previous frame.
- Stacks by capture time and look among the picks, as Omacull makes
  them, collapsed in the filmstrip; a key pastes onto the whole stack.
- One step of undo for a paste, however many frames it reached.
- Exporting the selection.

## M3. The look and HDR

- **Camera match:** the fitting tool (OmaLux's neutral rendering fitted
  to the embedded JPEGs over real shoots, a tone curve and a 3D LUT) and
  the A7 III's profile it makes, shipped as the default.
- **Flat**, one key away.
- **HDR:** the slider, by exposure fusion of virtual exposures across a
  pyramid. Highlights and Shadows moved onto the same local machinery.
- The process version bumped for edits made before this, which keep
  rendering as they did.

## M4. Lens corrections

- Profile corrections: distortion and vignetting, from lensfun or Sony's
  embedded data (whichever M0 picked as the default), found from the lens
  the raw names. On by default, as Lightroom's are once enabled.
- Remove Chromatic Aberration (lateral), at demosaic.
- Defringe: purple and green fringes around highlights at f/1.8.

## M5. Noise and detail

- **AI Denoise:** a button and a strength, on the raw's Bayer data, with
  the model M0 picked, on CUDA. Its result cached on disk per frame; the
  model fetched by a script, as Omacull's and Omapix's are.
- **Noise Reduction:** Luminance and Color.
- **Sharpening:** Amount, Radius, Detail, Masking.
- What Denoise and Sharpening do to each other, checked at 100% on real
  ISO 6400 frames.

## M6. Crop and auto

- **Crop & Straighten** (R): aspect ratios, the rectangle, the angle and
  a line to straighten along, 90° turns. In the copy checklist.
- **Auto white balance** (Ctrl+Shift+U), **Auto exposure**, **Auto tone**
  (Ctrl+U: Whites and Blacks so nothing clips that needn't) and **Auto
  level** (the horizon), each a button, and Shift+double-click on a
  slider.

## M7. Release

- Arch package: `packaging/arch/PKGBUILD` building `omalux-git`.
- README, CONTRIBUTING, screenshots.
- `config.toml` with every setting explained.
- Omacull's README and `config.toml` mention `developer = "omalux"`.

## Later

From the scoping interview, roughly in the order they were wanted:

- **JPEG export**, for frames that go straight out without Omapix.
- **Virtual copies**: two edits of one raw.
- **JPEGs and phone photos as input.**
- **Local adjustments:** a subject mask (Omapix's BiRefNet) and a linear
  gradient; a brush after that. A maybe.
- **HSL and the colour grading wheels**, and the tone curve.
- **Match Total Exposures** across a set.
- **DCP profiles**: Adobe's own (from the DNG Converter) or made with
  `dcamprof`.
- **Other cameras:** Nikon, Canon and Fuji raws through `rawler`, with
  camera-match profiles fitted for each.
- **Culling features** in OmaLux, if any turn out to be missed. Omacull
  stays the culler.

## Not planned

Healing and pixel retouching (Omapix's), a catalogue, keywords and
search, HDR output, Merge to HDR and panoramas, print, books, maps,
tethering, video, and anything to do with darktable. See "Out" in
DESIGN.md. Any of these can be reopened once the tool is in daily use.

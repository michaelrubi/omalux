# AGENTS.md

OmaLux is a fast, keyboard-first raw developer for Omarchy: the picks of a culled folder of raws in, 16-bit TIFFs ready to retouch out. It is the second stage of Michael's workflow and replaces darktable in it: cull in Omacull (`~/dev/omacull`), develop in OmaLux, retouch in Omapix (`~/dev/omapix`), export.

**Status:** scoped on 2026-10-08 and re-planned on [Lightcraft](https://github.com/storytold/lightcraft)'s engine. M0 under way: Lightcraft vendored at `701f78e`, the workspace and an empty themed window, the sidecar, the 16-bit export and the colour engine settled; the spikes that need real ARWs, the RTX 4070 or Lightroom have their tools built (`spike`, `lens`) and wait for Michael's laptop. raw.pixls.us's two ILCE-7M3 files pass the corpus tests on the laptop; none of Michael's own frames has gone through yet (raw.pixls.us is blocked in the cloud sessions, so fetch the corpus on the laptop). Read [docs/DESIGN.md](docs/DESIGN.md) for scope and architecture and [docs/ROADMAP.md](docs/ROADMAP.md) for the milestones and the "Later" list.

## Principles

- **Prep, not finish**: exposure, colour, noise, lens and crop, to an editable state for Omapix. Everything else has to argue its way in. Michael's complaint about darktable is that there's too much going on.
- **Pull everything out of the raw**: float pipeline, raw-domain AI denoise, 16-bit Linear ProPhoto out.
- **Never wait**: the pipeline is on the GPU; a slider drag shows in the next frame.
- **Lightroom Classic muscle memory**: its panel and slider names, behaviour and keys. A difference without a reason in DESIGN.md is a bug.
- **The files are the database**: edits in the raw's XMP sidecar (the one Omacull writes marks to), in an `omalux:` namespace with a process version. Raws are never moved, modified or deleted. No catalogue.
- **Auto is a button**: nothing is applied on its own.
- **Engine/UI separation**: `omalux-engine` has no UI or GPU dependencies and is headlessly testable.
- **Built on Lightcraft**: its lower crates (raw decoding, colour, `DevelopSettings`, the CPU pipeline and its GPU twin, XMP merge) are vendored under `vendor/lightcraft/`; OmaLux builds the folder, sidecar, UI and handoff on top. See "Built on Lightcraft" in DESIGN.md.

## Conventions

- OmaLux's own crates: same stack and conventions as Omacull and Omapix: Rust 2024, egui on wgpu, GPL-3.0-or-later (colour through Lightcraft's `moxcms` rather than Little CMS 2, as M0 settled). When in doubt about theme, hotkeys, the `Command` pattern, the headless `Harness` tests, ONNX Runtime or packaging, look at how Omacull (and behind it Omapix) does it and copy that. Code is copied between the Oma projects, not shared as crates.
- Vendored Lightcraft crates (`vendor/lightcraft/`) keep Lightcraft's conventions, licences (MIT OR Apache-2.0) and never-crash lints. Every local change to them is listed in `vendor/lightcraft/UPSTREAM.md`, kept small, and kept clean-room so it could go upstream: nothing ported from GPL code goes into them. GPL-derived work (lensfun's models, ports from darktable or RawTherapee) lives in OmaLux's own crates and feeds the vendored ones through their existing inputs. Re-sync from upstream on purpose, then re-run the reference renders and bump the process version if a render changed.
- Lightcraft's UI (`ui-egui`) and `engine` are not vendored: lift single widgets or functions from them, restyled to Omarchy's theme, and say where they came from. Lifted modules (`raw.rs`, `camera_preview.rs`, `camera_profiles.rs`, `lens_profiles.rs` in `omalux-engine`) name the file and commit, and keep Lightcraft's ways (no panics on file input).
- Keep diffs minimal and surgical. No speculative abstractions or unnecessary dependencies.
- Sidecar writes change only OmaLux's part (and the rating, for marks); everything else stays byte for byte. Test against sidecars written by Omacull and by darktable.
- Copy and paste: what's chosen at copy time is what's pasted. Never add a paste-time checklist (darktable's copy and paste is one of the things being replaced).
- Lightroom's algorithms are proprietary; take its behaviour, not guesses at its code. RapidRAW is AGPL: ideas only, no code. darktable, RawTherapee, ART and vkdt are GPL and can be ported from, with credit, into OmaLux's own crates only.
- Sony ARW from an ILCE-7M3 is the only target; other cameras are on the roadmap's "Later" list. Say so wherever they come up.
- Hand testing: Michael tests from the installed binary, not `cargo run`. After a change he'll try by hand, run `make install` and ask him to restart OmaLux.
- Whenever non-trivial UI or engine logic is added, write a headless test. GPU pipeline stages are checked against Lightcraft's CPU pipeline, as Lightcraft does.

## Build and test commands

```bash
# Run the tests: OmaLux's crates, and the vendored Lightcraft crates (their
# corpus tests skip without the raws below; the GPU's without an adapter)
cargo test --workspace

# Lint: no warnings
cargo clippy --workspace --all-targets

# Build and install to ~/.local (the copy Michael actually runs), with its
# launcher entry and icon
make install

# Run the app, with a shoot (or a raw in it)
cargo run --release -p omalux -- path/to/shoot

# Drive the UI without a keyboard: comma-separated Command names (only
# Open and Quit so far)
OMALUX_SCRIPT="Quit" cargo run --release -p omalux

# raw.pixls.us's CC0 ILCE-7M3 raws into vendor/lightcraft/corpus/raw
# (gitignored; SHA-256 checked), for the corpus tests
scripts/fetch-corpus.sh

# M0's render spike on real raws: camera settings, decode and develop
# times, the camera match's ΔE2000 against the camera's JPEG, the loupe and
# a drag on the CPU and GPU, the 16-bit export; --out writes the TIFFs and
# PNGs to compare at 100% (OMALUX_GPU=0 for the CPU only)
cargo run --release -p omalux-engine --example spike -- path/to/shoot --out /tmp/spike

# M0's lens spike: lensfun's match and Sony's table per frame, and renders
# with lensfun's correction and without (OMALUX_LENSFUN_DIR, else
# /usr/share/lensfun/version_1)
cargo run --release -p omalux-engine --example lens -- path/to/shoot --out /tmp/lens

# A raw's sidecar as OmaLux reads it, and an edit or a mark written to it
cargo run -p omalux-engine --example sidecar -- path/to/DSC01234.ARW --exposure 0.5
cargo run -p omalux-engine --example sidecar -- path/to/DSC01234.ARW --rating 3
```

Real A7 III test raws: raw.pixls.us's CC0 ILCE-7M3 files (`scripts/fetch-corpus.sh`), never committed. `LIGHTCRAFT_CORPUS` points the tests elsewhere.

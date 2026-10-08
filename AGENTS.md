# AGENTS.md

OmaLux is a fast, keyboard-first raw developer for Omarchy: the picks of a culled folder of raws in, 16-bit TIFFs ready to retouch out. It is the second stage of Michael's workflow and replaces darktable in it: cull in Omacull (`~/dev/omacull`), develop in OmaLux, retouch in Omapix (`~/dev/omapix`), export.

**Status:** scoped on 2026-10-08 and re-planned on [Lightcraft](https://github.com/storytold/lightcraft)'s engine; nothing built yet. M0 (vendoring, spikes and scaffold) is next. Read [docs/DESIGN.md](docs/DESIGN.md) for scope and architecture and [docs/ROADMAP.md](docs/ROADMAP.md) for the milestones and the "Later" list.

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

- OmaLux's own crates: same stack and conventions as Omacull and Omapix: Rust 2024, egui on wgpu, GPL-3.0-or-later (colour through Lightcraft's `moxcms` rather than Little CMS 2, unless M0 says otherwise). When in doubt about theme, hotkeys, the `Command` pattern, the headless `Harness` tests, ONNX Runtime or packaging, look at how Omacull (and behind it Omapix) does it and copy that. Code is copied between the Oma projects, not shared as crates.
- Vendored Lightcraft crates (`vendor/lightcraft/`) keep Lightcraft's conventions, licences (MIT OR Apache-2.0) and never-crash lints. Every local change to them is listed in `vendor/lightcraft/UPSTREAM.md`, kept small, and kept clean-room so it could go upstream: nothing ported from GPL code goes into them. GPL-derived work (lensfun's models, ports from darktable or RawTherapee) lives in OmaLux's own crates and feeds the vendored ones through their existing inputs. Re-sync from upstream on purpose, then re-run the reference renders and bump the process version if a render changed.
- Lightcraft's UI (`ui-egui`) and `engine` are not vendored: lift single widgets or functions from them, restyled to Omarchy's theme, and say where they came from.
- Keep diffs minimal and surgical. No speculative abstractions or unnecessary dependencies.
- Sidecar writes change only OmaLux's part (and the rating, for marks); everything else stays byte for byte. Test against sidecars written by Omacull and by darktable.
- Copy and paste: what's chosen at copy time is what's pasted. Never add a paste-time checklist (darktable's copy and paste is one of the things being replaced).
- Lightroom's algorithms are proprietary; take its behaviour, not guesses at its code. RapidRAW is AGPL: ideas only, no code. darktable, RawTherapee, ART and vkdt are GPL and can be ported from, with credit, into OmaLux's own crates only.
- Sony ARW from an ILCE-7M3 is the only target; other cameras are on the roadmap's "Later" list. Say so wherever they come up.
- Hand testing: Michael tests from the installed binary, not `cargo run`. After a change he'll try by hand, run `make install` and ask him to restart OmaLux.
- Whenever non-trivial UI or engine logic is added, write a headless test. GPU pipeline stages are checked against Lightcraft's CPU pipeline, as Lightcraft does.

## Build and test commands

None yet: M0 sets up the workspace. Expect the same as Omacull's (`cargo test`, `cargo clippy --workspace --all-targets`, `make install`, `OMALUX_SCRIPT=...`), and add them here when they exist. Real A7 III test raws: raw.pixls.us's CC0 ILCE-7M3 files (the ones Lightcraft's `cargo xtask corpus` downloads), never committed.

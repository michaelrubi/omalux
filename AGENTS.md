# AGENTS.md

OmaLux is a fast, keyboard-first raw developer for Omarchy: the picks of a culled folder of raws in, 16-bit TIFFs ready to retouch out. It is the second stage of Michael's workflow and replaces darktable in it: cull in Omacull (`~/dev/omacull`), develop in OmaLux, retouch in Omapix (`~/dev/omapix`), export.

**Status:** scoped on 2026-10-08; nothing built yet. M0 (spikes and scaffold) is next. Read [docs/DESIGN.md](docs/DESIGN.md) for scope and architecture and [docs/ROADMAP.md](docs/ROADMAP.md) for the milestones and the "Later" list.

## Principles

- **Prep, not finish**: exposure, colour, noise, lens and crop, to an editable state for Omapix. Everything else has to argue its way in. Michael's complaint about darktable is that there's too much going on.
- **Pull everything out of the raw**: float pipeline, raw-domain AI denoise, 16-bit Linear ProPhoto out.
- **Never wait**: the pipeline is on the GPU; a slider drag shows in the next frame.
- **Lightroom Classic muscle memory**: its panel and slider names, behaviour and keys. A difference without a reason in DESIGN.md is a bug.
- **The files are the database**: edits in the raw's XMP sidecar (the one Omacull writes marks to), in an `omalux:` namespace with a process version. Raws are never moved, modified or deleted. No catalogue.
- **Auto is a button**: nothing is applied on its own.
- **Engine/UI separation**: `omalux-engine` has no UI or GPU dependencies and is headlessly testable; `omalux-pipeline` has the GPU but no UI.

## Conventions

- Same stack and conventions as Omacull and Omapix: Rust 2024, egui on wgpu, Little CMS 2, GPL-3.0-or-later. When in doubt about theme, hotkeys, the `Command` pattern, the headless `Harness` tests, ONNX Runtime or packaging, look at how Omacull (and behind it Omapix) does it and copy that. Code is copied between the projects, not shared as crates.
- Keep diffs minimal and surgical. No speculative abstractions or unnecessary dependencies.
- Sidecar writes change only OmaLux's part (and the rating, for marks); everything else stays byte for byte. Test against sidecars written by Omacull and by darktable.
- Copy and paste: what's chosen at copy time is what's pasted. Never add a paste-time checklist (darktable's copy and paste is one of the things being replaced).
- Lightroom's algorithms are proprietary; take its behaviour, not guesses at its code. RapidRAW is AGPL: ideas only, no code. darktable, RawTherapee, ART and vkdt are GPL and can be ported from, with credit.
- Sony ARW from an ILCE-7M3 is the only target; other cameras are on the roadmap's "Later" list. Say so wherever they come up.
- Hand testing: Michael tests from the installed binary, not `cargo run`. After a change he'll try by hand, run `make install` and ask him to restart OmaLux.
- Whenever non-trivial UI or engine logic is added, write a headless test. Pipeline stages are checked against a small CPU reference of the same maths.

## Build and test commands

None yet: M0 sets up the workspace. Expect the same as Omacull's (`cargo test`, `cargo clippy --workspace --all-targets`, `make install`, `OMALUX_SCRIPT=...`), and add them here when they exist.

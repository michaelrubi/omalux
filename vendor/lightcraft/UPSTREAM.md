# Vendored Lightcraft

The lower crates of [Lightcraft](https://github.com/storytold/lightcraft)
(MIT OR Apache-2.0, by the ArtCraft Team and the Lightcraft contributors),
taken whole. See "Built on Lightcraft" in `docs/DESIGN.md` for why and how
they're kept.

- **Upstream:** https://github.com/storytold/lightcraft
- **Pinned at:** `701f78ea` ("Release 0.7.1: fix the macOS build"),
  2026-10-12. Workspace version 0.7.1.
- **Vendored on:** 2026-10-12.

## What's here

| Directory | Crate | Why |
|-----------|-------|-----|
| `crates/geom`, `color`, `raster`, `tiff` | L0 | Maths, colour, float images, TIFF reading |
| `crates/raw`, `codecs`, `meta`, `develop` | L1 | Raw decoding, JPEG and TIFF codecs and ICC, Exif/makernotes/XMP and `xmp_merge`, `DevelopSettings` |
| `crates/pipeline` | L2 | The CPU pipeline, the reference |
| `crates/gpu` | L3 | The same stages on wgpu |
| `crates/scenes` | | Procedural test photos |
| `crates/inpaint` | L1 | Not in DESIGN.md's list: `pipeline` depends on it (spot healing) |
| `crates/heif` | L1 | Not in DESIGN.md's list: `codecs` names it, as an optional dependency and in its tests. OmaLux's build leaves the feature off |
| `crates/denoise`, `denoise-core` | | Raw AI denoise; kept or dropped by M0's denoise spike |
| `crates/lensdb` | | lensfun's database; kept or dropped by M0's lens spike |

Also `LICENSE-MIT`, `LICENSE-APACHE` and `NOTICE` from upstream's root,
`clippy.toml` and `rustfmt.toml` (so clippy and rustfmt treat these crates
as upstream does), and `docs/raf-corpus.sha256` (a test in `raw` reads it).

Not vendored: everything else (`catalog`, `engine`, `ui-egui`, `mcp`, the
apps, `xtask`, `assets`). Single functions lifted from `engine` into
OmaLux's own crates say where they came from.

## How OmaLux builds them

From OmaLux's workspace (`Cargo.toml` at the root), which carries
Lightcraft's `[workspace.package]` and `[workspace.lints]` for them and the
same `[workspace.dependencies]` versions, with the lockfile seeded from
upstream's. Not changes to the crates:

- `lightcraft-raw` is depended on without JPEG XL DNGs, and
  `lightcraft-codecs` without AVIF (`default-features = false`). Their own
  tests, run as workspace members, still build them with their defaults.
  codecs keeps `jxl`: without it, its `jxl_encode.rs` has unused imports,
  and fixing that would be a local change for nothing OmaLux needs. WebP
  isn't behind a feature, and stays.
- wgpu is built with Vulkan only (Lightcraft's adds Metal and DX12).

Their tests need real raws for the corpus tests, which skip without them:
`vendor/lightcraft/corpus/` (gitignored), or `LIGHTCRAFT_CORPUS`.

## Local changes

Every change to a file under `vendor/lightcraft/` is listed here, with why,
kept small and clean-room (nothing ported from GPL code), so it could go
upstream.

- **`light.hdr`** (M4, 2026-10-13): `Light` has an `hdr` amount (0 to 100,
  not written while it's 0, so settings without it keep their JSON and
  `hash64`), with its row in the control table (`develop/src/settings.rs`,
  `controls.rs`). It's OmaLux's HDR slider, kept where the other Light
  sliders are so that undo, copy and paste, the sidecar and the slider
  widget carry it as they carry those. The pipeline doesn't read it:
  `omalux_engine::look::effective` folds it into Highlights and Shadows
  before a render, as Lightcraft's own profiles are folded in
  (`pipeline::profiles::effective`). Offered upstream, the folding would
  go there.
- **`light.hdrHighlights`, `light.hdrShadows`** (2026-10-14): two more
  amounts beside `light.hdr` (-100 to 100, not written while they're 0),
  each moving how much of HDR goes to its end of the range (darker to
  the left and brighter to the right, as Light's sliders), with their
  rows in the control table. Same files, same reasons; `look::effective`
  folds them in with HDR.

## Re-syncing

On purpose, not followed (see "How it's kept" in DESIGN.md): diff upstream
since the pinned commit, take what helps (Sony decoding and camera colour
above all), re-apply the local changes above, run the tests and the
reference renders, bump the process version if a render changed, and
update the commit here.

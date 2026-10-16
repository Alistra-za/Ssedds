# Third-party notices

Tessalith (`tessalith.exe`, the GUI, and `sdc.exe`, the command line) is GPL-3.0-or-later (see `LICENSE`).

The licences of the Rust crates compiled into the two executables, with their texts, are in `THIRD_PARTY_LICENSES.md` (generated). Regenerate with
`cargo about generate --frozen --workspace -c about.toml about.hbs -o THIRD_PARTY_LICENSES.md` (needs `cargo install cargo-about --locked --features cli`).
Where a crate offers a choice of licences, `about.toml` records the choice: MIT where offered; Apache-2.0 for `self_cell`, `moxcms` and `pxfm`. `self_cell`'s other option, GPL-2.0-only, is not compatible with GPL-3.0-or-later and is not used. No other licence in the tree is incompatible with GPL-3.0-or-later.

## Shipped in the executables (non-crate components)

- **bc7e.ispc** (bc7enc_rdo commit b943862), Copyright (C) 2018-2021 Binomial LLC, Apache License 2.0 (`LICENSES/Apache-2.0-bc7e.txt`).
  Tessalith ships a modified copy: patch `crates/encoders-sys/vendor/bc7e/patches/0001-exact-opaque-alpha.patch` (change notice at the top of `bc7e.ispc`).
- **rgbcx** (bc7enc_rdo commit b943862), Richard Geldreich, Jr., MIT or Public Domain (Unlicense); Tessalith uses MIT (`LICENSES/bc7enc_rdo-LICENSE.txt`).
  Tessalith ships a modified `rgbcx.h`: patch "rgbcx BC4 round-to-nearest" (see `crates/encoders-sys/vendor/README-VENDOR.md`).
- **Intel ISPC Texture Compressor** (commit 79ddbc9), Copyright (c) 2016-2019 Intel Corporation, MIT (`LICENSES/ISPCTextureCompressor-license.txt`).
- The ISPC objects of the above were compiled with Intel ISPC 1.31.0 (BSD-3-Clause compiler; the objects carry the source files' licences).
- **Studio HDRI** `crates/render3d/assets/hdri/studio_small_09_2k.hdr` (Poly Haven `studio_small_09`, https://polyhaven.com/a/studio_small_09), embedded in the GUI as the 3D preview background and ambient light. The repository's own record (`crates/render3d/assets/hdri/README.md`) states CC0 1.0 Universal. That README is the only record; the repository holds no licence file from the download itself.
- **PGPatcher JSON schemas** `crates/gui/assets/pbr.schema.json` and `crates/gui/assets/complex_material.schema.json` (https://github.com/hakasapl/PGPatcher, (c) hakasapl and contributors, GPL-3.0), vendored unmodified and embedded in the GUI to validate exported JSON. `pbr.schema.json` was fetched from `main` on 2026-10-09; `complex_material.schema.json` is upstream commit 0176567.
- **PBR landscape template values** in `crates/gui/assets/cs_pbr_templates.json`: values by Faultier (see `docs/CREDITS.md`). Used with Faultier's permission; no licence attached.
- The icons (`tessalith.ico`, `tessalith-256.png`, `sdc.ico`), the emblem (`docs/assets/tessalith-emblem.png`) and the shader `preview.wgsl` are Tessalith's own; the icon and emblem art was made by the project author.

Tessalith uses the Windows system fonts (Segoe UI, Segoe UI Symbol, Segoe UI Emoji, Consolas) at run time and ships no fonts (egui's bundled default fonts are disabled).

NVIDIA Texture Tools (NVTT) is never bundled, redistributed or linked; Tessalith can call a user-installed `nvcompress.exe` as a separate process.

## Repository only (not in the executables)

- `crates/nif/tests/fixtures/*.nif`: test NIF files from nifly (https://github.com/ousnius/nifly, `tests/expected`), GPL-3.0 per `crates/nif/tests/fixtures/README.md`. Only tests read them; no shipped code embeds them.
- The `nif`, `pgpsim` and `render3d` sources cite nifly and PGPatcher source files and behaviour in comments as the reference they follow.
- The projects credited in `docs/CREDITS.md` for ideas only (AlphaBloom, TruePBR-Manager) contribute no code or files.

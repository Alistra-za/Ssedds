# Third-party notices

sdc is GPL-3.0-or-later. It statically links:

- **bc7e.ispc** (bc7enc_rdo commit b943862), Copyright (C) 2018-2021 Binomial LLC, Apache License 2.0 (LICENSES/Apache-2.0-bc7e.txt).
  sdc ships a modified copy: patch `crates/encoders-sys/vendor/bc7e/patches/0001-exact-opaque-alpha.patch` (see that file's change notice).
- **rgbcx** (bc7enc_rdo commit b943862), Richard Geldreich, Jr., MIT or Public Domain (Unlicense) — sdc uses MIT (LICENSES/bc7enc_rdo-LICENSE.txt).
- **Intel ISPC Texture Compressor** (commit 79ddbc9), Copyright (c) 2016-2019 Intel Corporation, MIT (LICENSES/ISPCTextureCompressor-license.txt).
- **bcdec_rs** (crate), MIT. Other Rust crates: see `cargo about`/`cargo deny` output in release notes.
- **image** (0.25, png/tga/tiff/exr/hdr/bmp/jpeg decoders), MIT or Apache-2.0; **exr** (1.74) and **lebe** (0.5), BSD-3-Clause; **zune-jpeg**, **zune-core**, **zune-inflate**, MIT or Apache-2.0 or Zlib; **pulp**, MIT. sdc uses the permissive option compatible with GPL-3.0-or-later.

ISPC objects were compiled with Intel ISPC 1.31.0 (BSD-3-Clause compiler; the objects carry the source files' licences).
NVIDIA Texture Tools is never redistributed or linked; sdc can call a user-installed nvcompress.exe as a separate process.

- **egui / eframe / egui-wgpu / egui_kittest** (0.36.2), **wgpu**, **rfd** (0.17), **toml_edit** (0.25) and their dependencies, the GUI (sdc-gui.exe) only. The licence families in that dependency tree (Windows build) are MIT, Apache-2.0, Zlib, ISC, Unicode-3.0, BSL-1.0 and 0BSD; where a crate offers a choice (MIT or Apache-2.0, BSD-2-Clause/BSD-3-Clause or Apache-2.0, Unlicense or MIT, Zlib or MIT, and in one case GPL-2.0-only or Apache-2.0) sdc uses the permissive option compatible with GPL-3.0-or-later. Run `cargo about` or `cargo deny` for the full per-crate list.

sdc-gui uses the Windows system fonts (Segoe UI, Segoe UI Symbol, Segoe UI Emoji, Consolas) at run time and ships no fonts.

`crates/gui/assets/pbr.schema.json` — PGPatcher PBR JSON schema, © hakasapl and contributors, GPL-3.0 (https://github.com/hakasapl/PGPatcher). Vendored unmodified, fetched from `main` on 2026-10-09; used to validate exported JSON.

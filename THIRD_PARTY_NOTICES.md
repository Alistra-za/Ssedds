# Third-party notices

sdc is GPL-3.0-or-later. It statically links:

- **bc7e.ispc** (bc7enc_rdo commit b943862), Copyright (C) 2018-2021 Binomial LLC, Apache License 2.0 (LICENSES/Apache-2.0-bc7e.txt).
  sdc ships a modified copy: patch `crates/encoders-sys/vendor/bc7e/patches/0001-exact-opaque-alpha.patch` (see that file's change notice).
- **rgbcx** (bc7enc_rdo commit b943862), Richard Geldreich, Jr., MIT or Public Domain (Unlicense) — sdc uses MIT (LICENSES/bc7enc_rdo-LICENSE.txt).
- **Intel ISPC Texture Compressor** (commit 79ddbc9), Copyright (c) 2016-2019 Intel Corporation, MIT (LICENSES/ISPCTextureCompressor-license.txt).
- **bcdec_rs** (crate), MIT. Other Rust crates: see `cargo about`/`cargo deny` output in release notes.

ISPC objects were compiled with Intel ISPC 1.31.0 (BSD-3-Clause compiler; the objects carry the source files' licences).
NVIDIA Texture Tools is never redistributed or linked; sdc can call a user-installed nvcompress.exe as a separate process.

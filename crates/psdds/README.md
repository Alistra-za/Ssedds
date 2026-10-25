# psdds

Tessalith DDS bridge for host plugins: settings rules, encode and decode of Skyrim DDS textures, and a TruePBR `_rmaos`/`_n` pair encoder, behind a C ABI. Photoshop links it statically (`psdds.lib`); other hosts load `psdds.dll`.

## C ABI
The declarations and their contracts are in `photoshop/psdds.h`; that header is the reference. In short:
- settings and presets: `psdds_defaults`, `psdds_resolve`, `psdds_format_allowed`, `psdds_quality_count`, `psdds_quality_label`, `psdds_preset_*`;
- `psdds_encode` and `psdds_decode` (RGBA float in, DDS bytes out, and back), released with `psdds_free`;
- the depth-aware forms (ABI 3), for 8, 16 and 32-bit documents and HDR: `psdds_format_offered` (BC6H and RGBA16F only for 16 and 32-bit Colour), `psdds_resolve_depth`, `psdds_encode_depth`, `psdds_decode_depth` (BC6H, RGBA16F and RGBA32F come back as 32-bit float RGBA) and `psdds_dds_openable_float`;
- host helpers, the one copy of the size, openable, open-alpha, save-alpha and file-size rules: `psdds_check_size`, `psdds_dds_openable`, `psdds_opens_with_alpha`, `psdds_alpha_used`, `psdds_max_file_bytes`;
- `psdds_rmaos_encode` and `psdds_rmaos_default_quality`: a TruePBR pair from in-memory images;
- `psdds_abi_version` and `psdds_version`.

All calls are thread-safe. Errors are one plain sentence in a caller buffer.

## Build
```
.\crates\psdds\build.ps1          # target\psdds\x86_64-pc-windows-msvc\release\psdds.dll, psdds.lib, psdds.dll.lib
.\crates\psdds\build.ps1 -Test    # also loads the built DLL and checks it stays mapped after FreeLibrary
```

## ABI policy
`PSDDS_ABI_VERSION` changes on any change to the header's declarations or documented behaviour. A host requires an exact match with `psdds_abi_version()` and ships its own copy of the DLL. The current ABI is 3 (Note DE adds the depth-aware exports; the old exports keep their behaviour except as recorded there). See Notes DD and DE in `docs/DECISIONS.md`.

## Licence
MIT. It links `crates/engine` and `crates/encoders-sys`, which are under the Mozilla Public License 2.0 (`MPL-2.0`, no Exhibit B; each crate carries the text as `LICENSE`; Notes DC1, DC41); their source is at https://github.com/Alistra-za/Tessalith. `notices.ps1` names that repository and the commit a binary was built from (MPL-2.0 section 3.2).
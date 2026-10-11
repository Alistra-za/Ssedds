# sdc M6 Track B: inputs and formats, implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

## OWNER ANSWERS (2026-10-11: "Take all picks"; every default below is confirmed)

1. **`hdr_sky` and the HDR check (Note O2).** O2 gives `pbr_emissive` a "real HDR source" test before BC6H. Should `hdr_sky` get the same test? [Default: no. `hdr_sky` is unchanged and stays BC6H for every source. Task 4 is written so that adding `Subtype::HdrSky` to `emissive_hdr_choice` is a one-line change, but then `hdr_sky` needs a non-BC6H default (BC7 UNORM, linear) and a decision on its tag.]
2. **`rgba16f` outside Community Shaders.** [Default: `rgba16f` is `cs`-only, like BC6H: under `cpm`/`vanilla` it is an error-severity `CFG-UNAVAILABLE` (ruling S4). Nobody has confirmed that the vanilla or ENB shaders read an R16G16B16A16_FLOAT texture correctly.]
3. **Where a header patch is written.** [Default: the CLI needs an explicit `--out-dir <dir>` or `--in-place`. The GUI writes a patched copy to the export folder, at the path the DDS item already maps to, and never touches the source in the mod folder (ruling S7).]

**Goal:** sdc reads OpenEXR, Radiance HDR, BMP and JPEG sources. It reads a float source as linear light for every output format, and picks BC6H for `pbr_emissive` only when the source really is HDR. It writes and reads uncompressed DDS (`rgba8` written as B8G8R8A8, `r8`, `rg8`, `rgba16f`). It re-encodes a DDS input on explicit request. It patches fixable DDS header faults without touching the pixels. It reports ICC profiles and NaN/∞ samples at plan time.

**Architecture:**
- **Load** (`load.rs`): `load_image_meta` returns `(Rgba32F, SourceInfo, SourceMeta)`. `SourceMeta` carries `icc_profile` and, for a `.dds` input, `dds_dxgi`. `load_image` stays a thin wrapper, so the pipeline, the bake, packing and the preview all read the new formats without code changes. DDS inputs decode mip 0 through `decode::decode_level_rgba`.
- **Analyse** (`analyse.rs`):
  - statistics are taken over finite samples only;
  - non-finite samples are counted per channel;
  - `Analysis` gains `icc_profile`, `nonfinite` and `dds_dxgi`.
- **Resolve** (`policy.rs`):
  - `IN-ICC-IGNORED`, `IN-NONFINITE` and `LOSS-REENCODE` are raised from the analysis;
  - the `IN-FLOAT-LDR` guard is retired;
  - the new fact `analysis:hdr-source` (`emissive_hdr_choice`) runs after every layer, with its `FMT-HDR-SOURCE` notice;
  - `reencode_requested` decides whether a DDS input is planned at all.
- **Preprocess** (`ops.rs`): `preprocess_with(img, p, SourceTransfer)`. A float source is never sRGB-decoded, because it is already linear light. Quantise then encodes it per `color.content` exactly as it does decoded integer colour (ruling S1).
- **Formats** (`config.rs`, `dds.rs`, `encode.rs`, `quantize.rs`, `validate/`):
  - four new `Format` values and `Backend::Uncompressed`;
  - `Format::pixel_format`, `is_block` and `is_hdr`;
  - `dds::header_bytes`, shared by the writer and the patcher;
  - uncompressed levels are packed by `encode::pack_uncompressed`.
- **Header patch** (`dds_patch.rs`): `plan_patch(bytes, &PatchRequest) -> Result<Option<HeaderPatch>, Unpatchable>`. It builds a new header with `header_bytes` and appends the original bytes after the old header, unchanged. It is reached from the new `sdc patch-header` verb and from a GUI button.
- **GUI:** the new formats in the dropdowns, an uncompressed preview through the CPU-decode path, the HDR reason under Compression, and the header-patch choice on a DDS item.

**Tech Stack:** Rust 2021 workspace (MSRV 1.88; `sdc-gui` 1.95), egui/eframe 0.36.2.
- New `image` features: `exr`, `hdr`, `bmp`, `jpeg`. `image` stays at 0.25.10, the version `Cargo.lock` already pins.
- New transitive crates, all in the local cargo cache:
  - `exr` 1.74.2, `lebe` 0.5.3 (BSD-3-Clause);
  - `bit_field` 0.10.3, `num-complex` 0.4.6, `zune-inflate` 0.2.54, `pulp` 0.22.3 and `pulp-wasm-simd-flag` 0.1.1, `reborrow` 0.5.5, `bytemuck`, `libm`, `paste`, `num-traits` (MIT, Apache-2.0 and/or Zlib).
- `zune-jpeg` 0.5.15 and `zune-core` 0.5.3 are already in `Cargo.lock`.
- All of these were published well before 2026-09-27. If `cargo` resolves a newer `exr`, pin it with `cargo update -p exr --precise 1.74.2`.

**Spec:** `docs/superpowers/specs/2026-10-08-skyrim-dds-converter-design.md`: §1 (inputs, outputs), §3.2 (`format`, `encoder.backend`), §4.1/§4.2 (`pbr_emissive`), §5.1 (load, analyse, the interim float guard), §5.4, §5.7, §5.8, §8 (`reencode`, verbs), §10 (catalogue). Also:
- `docs/DECISIONS.md`: Note O (O1, O2), Note N (N11), Note R, Spike S8.
- The roadmap: `docs/superpowers/plans/2026-10-08-skyrim-dds-converter-phase0-2.md`, section "M6 — Presets completion and remaining inputs". This plan takes only the items named in Global Constraints.

## Global Constraints

- **Scope (M6 Track B only):**
  - inputs OpenEXR, Radiance HDR, BMP, JPEG;
  - the float-source transfer split;
  - O2 for `pbr_emissive`;
  - O1 uncompressed DDS output and input;
  - DDS re-encode opt-in;
  - the DDS header patch;
  - the ICC info event;
  - `IN-NONFINITE` at plan time, and a NaN-safe analysis.

  Not in this plan: presets completion, `presets diff/export/validate`, schema migrations, user rule tables, NPOT resize, tent/Mitchell/Kaiser, progress bars, the CPM `_m` idea, the two output modes, and the other carried items.
- **Branch:** `feat/m6-inputs-formats`. The controller creates it from `main` 58006ab (v0.2.0). Never commit to `main`. Never push.
- **Gates:** run them unpiped before every commit. All three must pass. In Git Bash run `export PATH="$HOME/.cargo/bin:$PATH"` first. `.cargo/config.toml` already pins `x86_64-pc-windows-msvc` with `+crt-static`, so never pass `--target`.
  ```bash
  cargo fmt --all --check
  cargo clippy --workspace --all-targets -- -D warnings
  cargo test --workspace
  ```
  When `fmt --check` fails, run `cargo fmt --all`, then re-run all three.
- **Output-affecting changes:**
  - `ENGINE_VERSION` is `CARGO_PKG_VERSION`, and `--on-existing if-changed` re-encodes when it changes.
  - The workspace version goes from `0.2.0` to `0.3.0` in Task 12, the last task, and not before.
  - `presets/sdc-base.toml` stays at 1.1.0. No built-in default changes; O2 is an analysis fact, not a preset change.
- **Code catalogue:** `crates/engine/src/codes.rs` `codes_serialise_as_their_strings_and_are_unique` asserts `Code::ALL.len()`. Each task that adds a code updates that number:
  - 63 at the start;
  - 64 after Task 1 (`IN-ICC-IGNORED`);
  - 65 after Task 4 (`FMT-HDR-SOURCE`);
  - 67 after Task 9 (`DDS-HEADER-PATCHED`, `DDS-HEADER-UNPATCHABLE`).

  `IN-FLOAT-LDR` stays in the catalogue (ruling S2).
- **Schemas:** a change to `Code`, `Format`, `Backend` or any type in `schema/*.json` needs `cargo run -p sdc -- presets schema all --out schema`. Commit the regenerated files with the task. `crates/engine/tests/schemas.rs` fails on drift. `Analysis` is not in any published schema.
- **Golden and `gui_repro`:** `crates/engine/tests/golden.rs` and `crates/cli/tests/gui_repro.rs` must stay green.
  - Every expectation that changes is listed by name in the task that changes it ("Expectations that change"). Change only those.
  - If any other test fails, stop and report `DONE_WITH_CONCERNS` with the test name. Do not edit that test.
- **Python:** never run `python -` or `python3 -` with a heredoc or stdin. They hang on this machine.
- **Commits:**
  - Commit only explicit paths. No `git add -A` and no `git add .`.
  - End every message with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- **Off limits:**
  - Never write to `F:/ai/dds/bench` or `F:/ai/dds/converter-nif`, and never touch `nif/*` branches. `G:/` is read-only.
  - DECISIONS Note Q is reserved for the NIF team. This plan's rulings go in a **new Note S**, after Note R (Task 12).
  - Never bundle or link NVTT.
  - Task 6 has one environment-gated test that *runs* `texconv.exe` (from `F:/ai/dds/bench/tools/dl_texconv/`) and the user-installed `nvddsinfo.exe`. Both only read files from a tempdir and write into a tempdir; nothing is written under the bench.
- **Downloads:** none, beyond cargo fetching the crates above, which are already cached. If a step seems to need anything else, stop and report `NEEDS_CONTEXT`.
- **Licences:** there is no `deny.toml` yet (M7 adds `cargo deny`).
  - The new crates are BSD-3-Clause (`exr`, `lebe`) and MIT/Apache-2.0/Zlib. All are GPL-3.0-or-later compatible.
  - `THIRD_PARTY_NOTICES.md` names `exr` explicitly (Task 1).
  - `image` is built with `default-features = false`, so `exr` builds without `rayon`, and decode stays deterministic.

## Rulings (the owner reviews these; Task 12 records each as a DECISIONS Note S entry)

### S1. Float-source transfer split (the ruling this plan had to make)

**A float source is linear light.** This covers 32-bit float TIFF, OpenEXR, Radiance HDR, and a float or half DDS (RGBA16F, RGBA32F, BC6H). For an LDR output:
- Preprocess never sRGB-decodes it. Step 5 runs only for integer sources.
- Quantise clamps each channel to [0, 1] and encodes it per `color.content`: `srgb` uses the exact sRGB OETF (`linear_to_srgb`), and `linear` stores the value as it is. That is the same encoding a decoded 8-bit sRGB source already gets, so the integer path is untouched.
- Values below 0 or above 1 in R, G, B (or A when the source has alpha) raise the existing **`LOSS-CLAMP-HDR` (warning, non-blocking)** at Resolve: "float values range {lo} to {hi}: values outside [0, 1] are clamped".

**Why `color.content` and not `color.tag`:** the tag is only the DDS flag. Vanilla `diffuse` and `glow` are `content = srgb` with `tag = unorm`, and their stored bytes must still be sRGB-encoded. Keying on the tag would write linear values into a vanilla diffuse and make it visibly dark, which is the bug the interim guard was blocking. For every PBR colour sub-type the two agree (`srgb`/`srgb`).

Consequences:
- A float normal, `_msn` or `_flow` keeps today's stored-value reading ([0, 1] encoded). Negative values clamp and raise `LOSS-CLAMP-HDR`.
- A float data map (`content = linear`) is written as stored values.
- `analysis:float-source` (content `linear` for a float source written as BC6H) is unchanged.
- An explicit `color.content = srgb` on a float source no longer causes an sRGB decode. Content then describes only the output encoding.

### S2. `IN-FLOAT-LDR` is retired, not deleted

It is never emitted again. Its catalogue entry stays, with severity changed from Blocking to Info and its §10 row marked "retired (M6)". This keeps `--allow IN-FLOAT-LDR` and `[files] allow = ["IN-FLOAT-LDR"]` loading: deleting the code would turn an existing `sdc.toml` into a config error. The catalogue count is unchanged.

### S3. O2: the HDR check before BC6H for `pbr_emissive`

`is_hdr_source(f)` holds when `info.is_float` is set and some **finite** R, G or B maximum is **strictly above 1.0** (the owner's "a peak above 1.0 in any colour channel"; not luminance).

`emissive_hdr_choice` runs after every layer (after `backend_follows_format`, before `float_source_is_linear`):
- For `pbr_emissive` with a non-explicit BC6H format and a source that is not HDR, it sets `format = bc7`, provenance `analysis:hdr-source`. The backend follows unless it was explicit.
- On every `pbr_emissive`, `color.tag` follows the final format unless a layer set it: BC6H gives `n/a`, anything else gives `srgb`. So `--set file:<glob>.format=bc6h_uf16` alone forces BC6H on an 8-bit `_g`, and `--set file:<glob>.format=bc7` alone forces BC7 sRGB on an HDR `_g`. The O2 test "override wins in both directions" needs no extra `color.tag`.
- "Explicit" is `field_explicit`. A user or project preset, config, rule, `[files]` entry or `--set` keeps its format. This matches the `skin-wet-form` precedent (DECISIONS R-skin-1) and is a little more permissive than O2's "below a suffix fact and `--set`": a user config that pins BC6H for every emissive keeps it.

Two more parts:
- `sdc-base` keeps `pbr_emissive = bc6h_uf16`, so no preset bump.
- Every available `pbr_emissive` carries the new Info **`FMT-HDR-SOURCE`** naming the reason, e.g. "8-bit source, no HDR → BC7 sRGB", "float source, peak 4.00 > 1.0: HDR → BC6H", or "…; format bc6h_uf16 from user (the HDR check is skipped)". Inspect, `--dry-run` and the GUI show it like `SKIN-WET-FORM`.

`hdr_sky` is unchanged (open question 1).

### S4. O1 uncompressed output formats

`Format` gains the following values (all `cfg_enum!` lowercase ids; DXGI = D3D format number):

| Value | DXGI | Header under `header = auto` | `color.tag` |
|---|---|---|---|
| `rgba8` | 87 `B8G8R8A8_UNORM`, 91 `B8G8R8A8_UNORM_SRGB` | **legacy** RGB masks (`DDPF_RGB` + `DDPF_ALPHAPIXELS`, 32 bits; R `0xff0000`, G `0xff00`, B `0xff`, A `0xff000000`) for UNORM; DX10 for sRGB | `srgb` / `unorm` per sub-type |
| `r8` | 61 | DX10 only | `unorm`; `srgb` is `CFG-INVALID` |
| `rg8` | 49 | DX10 only | `unorm`; `srgb` is `CFG-INVALID` |
| `rgba16f` | 10 | DX10 only | `n/a`, like BC6H |

- **Header fields:** uncompressed files carry `DDSD_PITCH` and the row pitch `w × bytes_per_pixel`, tightly packed, not `DDSD_LINEARSIZE`. `check_header` already demands this.
- **Backend:** new `Backend::Uncompressed` (`"uncompressed"`). It is the only backend that fits an uncompressed format, and `backend_follows_format` picks it. No BC encoder runs. Mips are full and box as usual.
- **Validation:** unchanged except that it is per format. PSNR is measured on R for `r8`, on R and G for `rg8`, and on RGB(A) for `rgba8`; it is 99 dB, because the format is lossless. There are no BC-only checks.
- **`rgba16f`:** is HDR (`Format::is_hdr`). It uses the `Hdr` domain (negatives clamp to 0, as for BC6H UF16) and is `cs`-only (open question 2). It keeps alpha as half floats when the alpha policy keeps alpha, else 1.0.
- **Normals:** `r8`/`rg8` on a normal map raise `FMT-BC5-NORMAL`, like BC4/BC5.

### S5. O1 uncompressed DDS input

- **Reading:**
  - `DdsPixelFormat::Bgra8` becomes `Bgra8 { srgb }`; DXGI 91 is now read.
  - `as_format` maps `Bgra8{srgb}` to `(Rgba8, srgb)`, `R8` to `R8`, `Rg8` to `Rg8` and `Rgba16f` to `Rgba16f`, so `sdc validate` judges these files fully.
  - RGBA-order `Rgba8` (DXGI 28/29), `Bgrx8` and `Rgba32f` stay "not produced by sdc": header-only validation. They are still inspected, previewed and re-encoded.
  - The reader assumes tight rows. A third-party file with a padded pitch shows `VAL-HEADER` ("pitch … != row pitch …") and decodes as stored.
- **N11 ends:**
  - the GUI previews every uncompressed layout through the CPU decode;
  - BC2 and signed BC6H still show "cannot be previewed".

### S6. DDS re-encode opt-in

- **The field:** `reencode` is a **plan-time selector**, not a `Profile` field. It decides whether a DDS input is planned at all, before it is loaded or its sub-type is known.
  - It is settable unscoped (`--set reencode=true`), file-scoped (`--set file:<glob>.reencode=true`) and as a `[files]` entry (`reencode = true`). Its value must be a boolean.
  - A `subtype:` scope is a usage error, because the sub-type is decided after the file is loaded.
  - It joins `FIELD_PATHS` and is filtered out of profile layering.
- **Decoding:** a DDS input decodes **mip 0 only**; every other mip is regenerated. `Analysis.dds_dxgi` records its DXGI format (legacy headers are mapped to their DXGI equivalent).
- **Warnings and conflicts:**
  - **`LOSS-REENCODE` (warning)** fires only for a block-compressed input. An uncompressed input re-encodes with no loss notice.
  - A re-encoded DDS takes part in duplicate-output detection, so `rock.png` plus an opted-in `rock.dds` gives `IN-DUPLICATE` for both.
  - An opted-in DDS whose output path is its own path **fails at plan**. This can only happen with an explicit file input whose folder is the output root, because a folder walk already skips files under the output root. It is reported as `file_failed` (stage `load`, "re-encoding would overwrite the source DDS: choose another output folder"; exit 1). It is never written.
  - A cubemap or array DDS fails to load with that reason.
  - A re-encoded DDS member does not take part in the variance bake: its set still sees a DDS, so `SET-COMPANION-DDS` stays.

### S7. DDS header patch (owner idea 2026-10-11)

`dds_patch::plan_patch` rebuilds the header with `dds::header_bytes` and appends `bytes[old_header_len..]` unchanged, trailing bytes included. The payload is therefore byte-identical by construction, and a test also asserts it.

**Patchable faults:**
- wrong or missing flags (`CAPS|HEIGHT|WIDTH|PIXELFORMAT|LINEARSIZE` or `PITCH`, and `MIPMAPCOUNT`);
- missing `DDSCAPS_TEXTURE|COMPLEX|MIPMAP`;
- a wrong linear size or pitch;
- a wrong mip count: the count of whole levels that exactly fills the payload, else the header's count when it fits;
- non-zero reserved fields and `DDPF_NORMAL`;
- DX10 dimension, array or misc fields;
- `ATI1`/`ATI2` to DX10 `BC4_UNORM`/`BC5_UNORM`;
- a legacy header that cannot express the requested tag, which becomes DX10;
- a UNORM to sRGB (or back) tag swap, **only** on an explicit request: `--tag`, a `--subtype` profile, or the GUI's tag choice. A file-name guess never causes one.

Every other header is rebuilt in the form it already had.

**Refused (`DDS-HEADER-UNPATCHABLE`, warning, with the reason):**
- unreadable or not a DDS;
- a cubemap, array or volume texture;
- a format sdc does not write (BC2, signed BC6H, RGBA-order RGBA8, BGRX8, RGBA32F);
- sRGB requested on a format with no sRGB form;
- a payload that holds no whole mip chain the header could describe;
- **missing mips** when the given profile wants a full chain;
- a **BC1 with transparent texels** (sdc's BC1 is opaque-only, Note F);
- the **wrong block format** when `--format` names another one.

**Reporting and output:**
- Success is the Info **`DDS-HEADER-PATCHED`**, listing the fixes.
- CLI: `sdc patch-header <dds…> (--out-dir <dir> | --in-place) [--target] [--subtype] [--tag srgb|unorm] [--format <fmt>] [--dry-run]`. Exit 1 when any file is unreadable or refused.
- GUI: a DDS item shows its header problems, a tag choice ("keep", "sRGB", "UNORM") and "Patch header…" then "Patch". It writes a copy to `<export folder>/<job key>`, never over the source, and refuses when the two paths are the same.
- A patch is not recorded in the manifest. It is not a conversion.

### S8. ICC profiles

The loader asks the decoder for an embedded ICC profile (`ImageDecoder::icc_profile`). It ignores the profile's content and reads values as stored. A non-empty profile raises the Info **`IN-ICC-IGNORED`** at Resolve: "embedded ICC profile ignored: values are read as stored (colour content as sRGB)".

### S9. `IN-NONFINITE` at plan time; NaN-safe analysis

- `analyse` leaves NaN and ±∞ out of min, max, mean, the normal-length statistics and the peak, and counts them per channel in `Analysis.nonfinite`.
- Resolve raises `IN-NONFINITE` (warning, `data.samples`) with the count that Preprocess will replace: R, G and B always; A only when the alpha policy keeps alpha. So `inspect` and `--dry-run` report it.
- The pipeline no longer emits it at run time, so it appears once per file.

### S10. New inputs

`.exr`, `.hdr`, `.bmp`, `.jpg` and `.jpeg` are discovered in folders (`IMAGE_EXTS`) and decoded by `image` 0.25.10:
- EXR: the first RGB(A) layer, as f32. `is_float` is true.
- HDR: RGBE, as f32 RGB. `is_float` is true.
- BMP: 8-bit, with or without alpha.
- JPEG: 8-bit, lossy.

Anything `image` cannot decode (a multi-part EXR without an RGB layer, a CMYK JPEG it rejects, a truncated file) is `IN-DECODE` (`file_failed` at load), never a panic. A `rock.png` next to a `rock.jpg` maps to one output and stays `IN-DUPLICATE`, as today.

## Review Focus

Most likely first. Each item's test is in the task named.

1. **Re-encoding a DDS into its own folder** (output root equal to the input folder) must fail and leave the source byte-identical. An opted-in `rock.dds` next to `rock.png` must give `IN-DUPLICATE` for both, never write one over the other. Tests: Task 8, `reencode_into_its_own_folder_fails_and_leaves_the_source_alone` and `an_opted_in_dds_and_a_same_stem_image_are_duplicates`.
2. **Third-party uncompressed DDS** (BGRX8, RGBA-order RGBA8, RGBA32F, a padded pitch) is inspected, validated (header-only where sdc never writes the format), previewed and re-encoded without a panic. Tests: Task 7, `third_party_uncompressed_layouts_load_as_float_rgba` and `a_padded_pitch_is_a_header_issue_not_a_panic`; Task 11, `a_bgrx8_dds_input_previews_from_the_cpu_decode`.
3. **A float source above 1.0 where BC6H is unavailable** (`vanilla`/`cpm` diffuse, or `rgba8`) converts with `LOSS-CLAMP-HDR` and is no longer blocked. A float data map is written as stored values, not gamma-encoded. Tests: Task 3, `a_float_source_under_vanilla_is_srgb_encoded_into_unorm` and `a_float_data_map_is_written_as_stored_values`.
4. **Broken or odd input files** (garbage EXR, a truncated HDR, a zero-byte JPEG) fail that file with `IN-DECODE` and the batch continues. Test: Task 1, `broken_new_format_files_fail_with_in_decode_and_the_batch_continues`.
5. **Header patch edge cases:**
   - trailing bytes after the chain are kept;
   - a header mip count higher than the payload holds is lowered, or refused when no whole chain fits;
   - a valid file reports "nothing to patch" and writes nothing.

   Tests: Task 9, `trailing_bytes_after_the_chain_are_kept`, `a_short_payload_is_refused` and `a_valid_file_needs_no_patch`; Task 10, `a_valid_file_writes_nothing`.

## Interface registry (names every task must use)

| Name | Where | Task |
|---|---|---|
| `pub struct SourceMeta { pub icc_profile: bool, pub dds_dxgi: Option<u32> }` (derive `Debug, Clone, Copy, Default, PartialEq, Eq`) | `engine::load` | 1 (field `dds_dxgi` filled in Task 7) |
| `pub fn load_image_meta(path: &Path) -> Result<(Rgba32F, SourceInfo, SourceMeta), LoadError>` | `engine::load` | 1 |
| `Analysis.icc_profile: bool`, `Analysis.nonfinite: [u64; 4]`, `Analysis.dds_dxgi: Option<u32>` | `engine::analyse` | 1, 2, 7 |
| `Code::InIccIgnored` `"IN-ICC-IGNORED"` Info | `engine::codes` | 1 |
| `pub enum SourceTransfer { Stored, Linear }` + `SourceTransfer::of(&SourceInfo)` | `engine::ops` | 3 |
| `pub fn preprocess_with(img: Rgba32F, p: &Profile, transfer: SourceTransfer) -> Work` (`preprocess(img, p)` stays and uses `Stored`) | `engine::ops` | 3 |
| `pub fn is_hdr_source(f: &FileFacts) -> bool` | `engine::policy` | 4 |
| `Code::FmtHdrSource` `"FMT-HDR-SOURCE"` Info | `engine::codes` | 4 |
| `Format::{Rgba8, R8, Rg8, Rgba16f}`, `Backend::Uncompressed` | `engine::config` | 5 |
| `Format::pixel_format(self, srgb: bool) -> DdsPixelFormat`, `Format::block_bytes(self) -> Option<usize>`, `Format::bytes_per_pixel(self) -> usize`, `Format::is_block(self) -> bool`, `Format::is_hdr(self) -> bool` | `engine::config` | 5 |
| `DdsPixelFormat::Bgra8 { srgb: bool }`, `pub fn from_dxgi(d: u32) -> Option<DdsPixelFormat>`, `DdsPixelFormat::dxgi(self) -> u32`, `DdsPixelFormat::bytes_per_pixel(self) -> usize` (now `pub`) | `engine::dds` | 5 |
| `pub fn header_bytes(desc: &DdsDesc, mips: u32) -> Result<Vec<u8>, DdsError>`, `pub fn legacy_masks(format: Format) -> Option<(u32, u32, [u32; 4])>`, `DDPF_ALPHAPIXELS`, `DDPF_RGB` | `engine::dds` | 5 |
| `pub fn decode_level_rgba(fmt: DdsPixelFormat, w: u32, h: u32, data: &[u8], bc1_model: Option<Bc1Model>) -> Result<Vec<[f32; 4]>, DdsError>` | `engine::decode` | 7 |
| `pub const REENCODE_FIELD: &str = "reencode"`, `pub fn reencode_requested(ctx: &ResolveCtx, input: &InputFile) -> bool` | `engine::overrides` / `engine::policy` | 8 |
| `pub fn duplicate_outputs_with(files: &[InputFile], include: impl Fn(&InputFile) -> bool) -> Vec<Vec<usize>>` | `engine::group` | 8 |
| `pub struct PatchRequest { pub srgb: Option<bool>, pub expect_format: Option<Format>, pub require_full_mips: bool }` (derive `Default`), `pub struct HeaderPatch { pub fixes: Vec<String>, pub bytes: Vec<u8>, pub old_header_len: usize, pub new_header_len: usize }`, `pub struct Unpatchable(pub String)`, `pub fn plan_patch(bytes: &[u8], req: &PatchRequest) -> Result<Option<HeaderPatch>, Unpatchable>`, `HeaderPatch::warning(&self) -> Warning`, `Unpatchable::warning(&self) -> Warning` | `engine::dds_patch` (new, `pub mod dds_patch;` in `lib.rs`) | 9 |
| `Code::DdsHeaderPatched` `"DDS-HEADER-PATCHED"` Info, `Code::DdsHeaderUnpatchable` `"DDS-HEADER-UNPATCHABLE"` Warning | `engine::codes` | 9 |
| `Command::PatchHeader(PatchHeaderArgs)`, `verbs::patch_header(a: PatchHeaderArgs) -> Result<i32, EngineError>` | `sdc` (cli) | 10 |
| `pub struct DdsHeaderView { pub issues: Vec<String>, pub patch: Result<Option<Vec<String>>, String>, pub tag: Option<bool> }`, `AppModel::dds_header(&mut self, key: &str) -> Option<DdsHeaderView>`, `AppModel::set_dds_patch_tag(&mut self, key: &str, srgb: Option<bool>)`, `AppModel::patch_dds_header(&mut self, key: &str) -> Result<PathBuf, String>` | `sdc_gui::model` (new `model/dds_patch.rs`) | 11 |

## File structure

| File | Responsibility | Tasks |
|---|---|---|
| `Cargo.toml` | `image` features; version 0.3.0 | 1, 12 |
| `crates/engine/src/load.rs` | `SourceMeta`, ICC, DDS dispatch | 1, 7 |
| `crates/engine/src/discover.rs` | `IMAGE_EXTS` | 1 |
| `crates/engine/src/analyse.rs` | finite-only statistics, new `Analysis` fields | 1, 2, 7 |
| `crates/engine/src/codes.rs` | four new codes, `IN-FLOAT-LDR` retired | 1, 3, 4, 9 |
| `crates/engine/src/policy.rs` | ICC, NaN, float split, O2, re-encode, uncompressed rules | 1–8 |
| `crates/engine/src/ops.rs` | `SourceTransfer`, `preprocess_with`, `domain_for` | 3, 5 |
| `crates/engine/src/pipeline.rs` | transfer on load; no run-time `IN-NONFINITE` | 2, 3 |
| `crates/engine/src/config.rs` | `Format`, `Backend` values and helpers | 5 |
| `crates/engine/src/dds.rs` | writer/reader for uncompressed, `header_bytes` | 5 |
| `crates/engine/src/encode.rs`, `quantize.rs` | uncompressed packing, `rgba16f` alpha | 5 |
| `crates/engine/src/validate/mod.rs` | per-format channels | 6 |
| `crates/engine/src/decode.rs` | `decode_level_rgba` | 7 |
| `crates/engine/src/overrides.rs`, `group.rs`, `api.rs`, `presets.rs` | `reencode` | 8 |
| `crates/engine/src/dds_patch.rs` (new) | header patch | 9 |
| `crates/cli/src/{args,main,verbs}.rs` | `patch-header` verb | 10 |
| `crates/gui/src/model/{fields,preview,dds_patch,mod}.rs`, `render/{mod,upload}.rs`, `ui/settings_panel.rs` | GUI | 5 (compile only), 11 |
| `docs/…`, `THIRD_PARTY_NOTICES.md` | docs, Note S | 1, 12 |

---

### Task 1: OpenEXR, Radiance HDR, BMP and JPEG inputs; the ICC info event

**Files:**
- Modify: `Cargo.toml` (workspace `image` features), `crates/engine/src/discover.rs` (`IMAGE_EXTS`), `crates/engine/src/load.rs`, `crates/engine/src/analyse.rs` (`Analysis.icc_profile`), `crates/engine/src/api.rs` (plan load site, about line 820), `crates/engine/src/codes.rs`, `crates/engine/src/policy.rs` (`check_profile`), `THIRD_PARTY_NOTICES.md`
- Regenerate: `schema/*.json` (new code)
- Test: `crates/engine/tests/load.rs`, `crates/engine/tests/discover_group.rs`, `crates/engine/tests/golden.rs`, `crates/engine/src/codes.rs` (unit)

**Interfaces:**
- Consumes: nothing new.
- Produces: `SourceMeta`, `load_image_meta`, `Analysis.icc_profile`, `Code::InIccIgnored`. Code count 64.

**Expectations that change:** `codes_serialise_as_their_strings_and_are_unique` (63 → 64).

- [ ] **Step 1: Turn on the decoders.** In `Cargo.toml` `[workspace.dependencies]`:

```toml
image = { version = "0.25", default-features = false, features = ["png", "tga", "tiff", "exr", "hdr", "bmp", "jpeg"] }
```

Run `cargo tree -p engine -i exr`. Expect `exr v1.74.2`. If a newer one appears, run `cargo update -p exr --precise 1.74.2`.

- [ ] **Step 2: Write the failing load tests.** Append to `crates/engine/tests/load.rs`:

```rust
use image::{ExtendedColorType, ImageEncoder};

#[test]
fn exr_and_hdr_load_as_float_rgb() {
    let d = tempfile::tempdir().unwrap();
    let img = ImageBuffer::from_fn(4, 4, |x, y| {
        Rgb([if (x, y) == (1, 1) { 2.5f32 } else { 0.25 }, 0.5, 0.75])
    });
    for (name, tol) in [("a.exr", 1e-6f32), ("a.hdr", 0.02)] {
        let p = d.path().join(name);
        image::DynamicImage::ImageRgb32F(img.clone()).save(&p).unwrap();
        let (got, info) = load_image(&p).unwrap();
        assert_eq!(
            (info.bit_depth, info.channels, info.has_alpha, info.is_float),
            (32, 3, false, true),
            "{name}"
        );
        assert!((got.at(1, 1)[0] - 2.5).abs() <= 2.5 * tol, "{name}: {:?}", got.at(1, 1));
        assert_eq!(got.at(0, 0)[3], 1.0, "{name}");
    }
}

#[test]
fn exr_with_alpha_loads_rgba_float() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("a.exr");
    image::DynamicImage::ImageRgba32F(ImageBuffer::from_fn(2, 2, |_, _| {
        Rgba([4.0f32, 0.5, 0.25, 0.5])
    }))
    .save(&p)
    .unwrap();
    let (img, info) = load_image(&p).unwrap();
    assert_eq!((info.channels, info.has_alpha, info.is_float), (4, true, true));
    assert_eq!(img.at(1, 1), [4.0, 0.5, 0.25, 0.5]);
}

#[test]
fn bmp_loads_exactly_and_jpeg_within_its_loss() {
    let d = tempfile::tempdir().unwrap();
    let src = ImageBuffer::from_fn(16, 16, |x, y| Rgb([x as u8 * 16, y as u8 * 16, 90u8]));
    let bmp = d.path().join("a.bmp");
    src.save(&bmp).unwrap();
    let (img, info) = load_image(&bmp).unwrap();
    assert_eq!((info.bit_depth, info.channels, info.has_alpha, info.is_float), (8, 3, false, false));
    assert_eq!(img.at(3, 5), [48.0 / 255.0, 80.0 / 255.0, 90.0 / 255.0, 1.0]);
    let jpg = d.path().join("a.jpg");
    src.save(&jpg).unwrap();
    let (img, info) = load_image(&jpg).unwrap();
    assert_eq!((info.bit_depth, info.channels, info.is_float), (8, 3, false));
    for (x, y) in [(0, 0), (7, 9), (15, 15)] {
        let want = src.get_pixel(x, y).0;
        for c in 0..3 {
            let got = (img.at(x, y)[c] * 255.0).round() as i32;
            assert!((got - want[c] as i32).abs() <= 12, "({x},{y}) c{c}: {got} vs {}", want[c]);
        }
    }
}

/// A minimal ICC header: size (big-endian) then the `acsp` signature at byte 36.
fn icc_blob() -> Vec<u8> {
    let mut v = vec![0u8; 132];
    v[..4].copy_from_slice(&132u32.to_be_bytes());
    v[36..40].copy_from_slice(b"acsp");
    v
}

#[test]
fn an_embedded_icc_profile_is_reported_and_ignored() {
    let d = tempfile::tempdir().unwrap();
    let px: Vec<u8> = (0..2 * 2 * 3).map(|i| (i * 20) as u8).collect();
    let with = d.path().join("icc.png");
    let mut f = std::fs::File::create(&with).unwrap();
    let mut enc = image::codecs::png::PngEncoder::new(&mut f);
    enc.set_icc_profile(icc_blob()).unwrap();
    enc.write_image(&px, 2, 2, ExtendedColorType::Rgb8).unwrap();
    drop(f);
    let without = d.path().join("plain.png");
    image::codecs::png::PngEncoder::new(std::fs::File::create(&without).unwrap())
        .write_image(&px, 2, 2, ExtendedColorType::Rgb8)
        .unwrap();
    let (a, _, meta) = load_image_meta(&with).unwrap();
    let (b, _, plain) = load_image_meta(&without).unwrap();
    assert!(meta.icc_profile && !plain.icc_profile);
    assert_eq!(a, b, "the profile changes no value");
    assert_eq!(meta.dds_dxgi, None);
}

#[test]
fn a_garbage_exr_is_a_decode_error_not_a_panic() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("bad.exr");
    std::fs::write(&p, [0x76, 0x2f, 0x31, 0x01, 2, 0, 0, 0, b'x', b'y']).unwrap();
    assert!(matches!(load_image(&p), Err(LoadError::Decode { .. })));
}
```

In `crates/engine/tests/discover_group.rs` add (it already imports `engine::discover::*`; add the import if missing):

```rust
#[test]
fn folders_discover_every_supported_image_extension() {
    let d = tempfile::tempdir().unwrap();
    let t = d.path().join("in");
    std::fs::create_dir_all(&t).unwrap();
    for n in ["a.png", "b.tga", "c.tif", "d.tiff", "e.exr", "f.hdr", "g.bmp", "h.jpg", "i.JPEG", "j.dds", "k.txt"] {
        std::fs::write(t.join(n), b"x").unwrap();
    }
    let found = engine::discover::discover(&[t], &d.path().join("out"), false).unwrap();
    let names: Vec<String> = found.iter().map(|f| f.rel.to_string_lossy().into_owned()).collect();
    assert_eq!(names.len(), 10, "{names:?}");
    assert!(!names.iter().any(|n| n.ends_with(".txt")));
}
```

- [ ] **Step 3: Run them and see them fail.** Run `cargo test -p engine --test load --test discover_group`. Expect: a compile error on `load_image_meta`; once Step 4 starts, the discover test fails with 5 names instead of 10.

- [ ] **Step 4: Implement.**

`crates/engine/src/discover.rs`:

```rust
pub const IMAGE_EXTS: [&str; 9] = ["png", "tga", "tif", "tiff", "exr", "hdr", "bmp", "jpg", "jpeg"];
```

`crates/engine/src/load.rs`:
- Replace the module doc's ICC sentence with "Embedded ICC profiles are detected and ignored (DECISIONS S8)."
- Add `use image::ImageDecoder;`.
- Add the following and rewrite `load_image` as the wrapper:

```rust
/// What `load_image_meta` learns besides the pixels.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SourceMeta {
    /// The file embeds a non-empty ICC profile; values are still read as stored (DECISIONS S8, `IN-ICC-IGNORED`).
    pub icc_profile: bool,
    /// DXGI format of a DDS input (DECISIONS S6); `None` for images.
    pub dds_dxgi: Option<u32>,
}

/// Decode `path` to float RGBA. The dimension limit is checked from the header before the full pixel buffer is
/// decoded or allocated.
pub fn load_image(path: &Path) -> Result<(Rgba32F, SourceInfo), LoadError> {
    load_image_meta(path).map(|(img, info, _)| (img, info))
}

/// `load_image`, with the ICC flag (and, from Task 7, a DDS input's DXGI format).
pub fn load_image_meta(path: &Path) -> Result<(Rgba32F, SourceInfo, SourceMeta), LoadError> {
    let err = |e: image::ImageError| LoadError::Decode {
        path: path.to_path_buf(),
        msg: e.to_string(),
    };
    let (w, h) = image::image_dimensions(path).map_err(err)?;
    if w.max(h) > MAX_DIM {
        return Err(LoadError::TooLarge {
            path: path.to_path_buf(),
            dim: w.max(h),
        });
    }
    // The explicit MAX_DIM header check bounds the size, so the crate's default
    // 512 MiB allocation cap (which rejects valid 16384 px images) is lifted.
    let mut reader = image::ImageReader::open(path)
        .and_then(image::ImageReader::with_guessed_format)
        .map_err(|e| LoadError::Decode {
            path: path.to_path_buf(),
            msg: e.to_string(),
        })?;
    reader.no_limits();
    let mut decoder = reader.into_decoder().map_err(err)?;
    // A decoder that cannot read a profile, or finds none, is not an error: the profile is ignored either way.
    let icc_profile = decoder
        .icc_profile()
        .ok()
        .flatten()
        .is_some_and(|p| !p.is_empty());
    let img = image::DynamicImage::from_decoder(decoder).map_err(err)?;
    let ct = img.color();
    let channels = ct.channel_count();
    let info = SourceInfo {
        bit_depth: (ct.bits_per_pixel() / u16::from(channels)) as u8,
        channels,
        has_alpha: ct.has_alpha(),
        is_float: matches!(ct, image::ColorType::Rgb32F | image::ColorType::Rgba32F),
    };
    let px = img.to_rgba32f().pixels().map(|p| p.0).collect();
    Ok((
        Rgba32F {
            width: img.width(),
            height: img.height(),
            px,
        },
        info,
        SourceMeta {
            icc_profile,
            dds_dxgi: None,
        },
    ))
}
```

`crates/engine/src/analyse.rs`:
- Add to `Analysis`, after `peak_luminance`:

```rust
    /// The source embeds an ICC profile, which sdc ignores (DECISIONS S8). Set by the caller from `SourceMeta`.
    pub icc_profile: bool,
```

- In `analyse()`'s literal, add `icc_profile: false,`.

`crates/engine/src/api.rs`, at the plan's load site (`let (img, info) = load_image(&f.path)?;`, about line 820):
- Import `load_image_meta` next to `load_image`.
- Replace that site with:

```rust
                    let (img, info, meta) = load_image_meta(&f.path)?;
                    cache.count_loads(1);
                    enter(Stage::Analyse);
                    let mut a = analyse(&img, &info);
                    a.icc_profile = meta.icc_profile;
```

`crates/engine/src/codes.rs`:
- Add `InIccIgnored = "IN-ICC-IGNORED" => Info,` after `InFloatLdr`.
- Extend the module doc list with "IN-ICC-IGNORED (an embedded ICC profile, M6)".
- In the tests, add `assert_eq!(Code::InIccIgnored.severity(), Severity::Info);` and change `63` to `64`.

`crates/engine/src/policy.rs` `check_profile`: after the `DIM-FLOW-SMALL` block, add:

```rust
    if a.icc_profile {
        add(
            InIccIgnored,
            "embedded ICC profile ignored: values are read as stored (colour content as sRGB)".into(),
        );
    }
```

`THIRD_PARTY_NOTICES.md`: after the `bcdec_rs` bullet, add:

```markdown
- **image** (0.25, png/tga/tiff/exr/hdr/bmp/jpeg decoders), MIT or Apache-2.0; **exr** (1.74) and **lebe** (0.5), BSD-3-Clause; **zune-jpeg**, **zune-core**, **zune-inflate**, MIT or Apache-2.0 or Zlib; **pulp**, MIT. sdc uses the permissive option compatible with GPL-3.0-or-later.
```

- [ ] **Step 5: Golden cases, one per new input type.** Append to `crates/engine/tests/golden.rs`:

```rust
/// M6 (DECISIONS S10): an OpenEXR and a Radiance HDR emissive in a PBR folder are float HDR sources → BC6H.
#[test]
fn exr_and_radiance_hdr_float_emissives_are_bc6h() {
    for ext in ["exr", "hdr"] {
        let d = tempfile::tempdir().unwrap();
        put(d.path(), &format!("textures/pbr/lamp_g.{ext}"), hdr_emissive());
        let o = convert(d.path(), Target::Cs, &[], &[]);
        assert_eq!((o.summary.converted, o.summary.exit_code()), (1, 0), "{ext}: {:?}", warnings(&o));
        assert_eq!(warnings(&o), vec![], "{ext}");
        assert_profile(&o, &format!("lamp_g.{ext}"), "pbr_emissive", "remove", false, "bc6h_uf16");
        assert_dds(&dds(d.path(), "textures/pbr/lamp_g.dds"), DdsPixelFormat::Bc6hUf16, HeaderKind::Dx10, 7);
    }
}

/// M6: a BMP diffuse converts like a PNG (lossless 8-bit source).
#[test]
fn bmp_diffuse_is_bc7_unorm() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "textures/a/rock.bmp", rgb(64));
    let o = convert(d.path(), Target::Vanilla, &["file:**/rock.bmp.subtype=diffuse"], &[]);
    assert_eq!(o.summary.converted, 1, "{:?}", warnings(&o));
    assert_eq!(warnings(&o), vec![]);
    assert_dds(&dds(d.path(), "textures/a/rock.dds"), DdsPixelFormat::Bc7 { srgb: false }, HeaderKind::Dx10, 7);
}

/// M6: a JPEG diffuse converts; its mip 0 matches the decoded JPEG within BC7 error.
#[test]
fn jpeg_diffuse_is_bc7_unorm_close_to_the_decoded_jpeg() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "textures/a/rock.jpg", rgb(64));
    let o = convert(d.path(), Target::Vanilla, &["file:**/rock.jpg.subtype=diffuse"], &[]);
    assert_eq!(o.summary.converted, 1, "{:?}", warnings(&o));
    let f = dds(d.path(), "textures/a/rock.dds");
    assert_dds(&f, DdsPixelFormat::Bc7 { srgb: false }, HeaderKind::Dx10, 7);
    let src = image::open(d.path().join("in/textures/a/rock.jpg")).unwrap().to_rgb8();
    for (i, p) in levels(&f)[0].iter().enumerate() {
        let s = src.get_pixel(i as u32 % 64, i as u32 / 64).0;
        for c in 0..3 {
            assert!((p[c] as i32 - s[c] as i32).abs() <= 8, "texel {i} c{c}: {} vs {}", p[c], s[c]);
        }
    }
}

/// DECISIONS S8: an embedded ICC profile is an info, never a block; the output matches the same PNG without it.
#[test]
fn an_icc_profile_is_reported_as_info_and_ignored() {
    use image::ImageEncoder;
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("in/textures/a/rock.png");
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    let px = rgb(64).to_rgb8().into_raw();
    let mut f = std::fs::File::create(&p).unwrap();
    let mut enc = image::codecs::png::PngEncoder::new(&mut f);
    let mut icc = vec![0u8; 132];
    icc[..4].copy_from_slice(&132u32.to_be_bytes());
    icc[36..40].copy_from_slice(b"acsp");
    enc.set_icc_profile(icc).unwrap();
    enc.write_image(&px, 64, 64, image::ExtendedColorType::Rgb8).unwrap();
    drop(f);
    let o = convert(d.path(), Target::Vanilla, &["file:**/rock.png.subtype=diffuse"], &[]);
    assert_eq!(o.summary.converted, 1, "{:?}", warnings(&o));
    assert_eq!(warning_of(&o, "rock.png", Code::InIccIgnored).severity, Severity::Info);
}

/// Review Focus 4: a broken file of each new type fails alone with IN-DECODE; the batch goes on.
#[test]
fn broken_new_format_files_fail_with_in_decode_and_the_batch_continues() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "textures/a/good.png", rgb(64));
    for (n, bytes) in [
        ("bad.exr", &b"\x76\x2f\x31\x01garbage"[..]),
        ("bad.hdr", &b"#?RADIANCE\n"[..]),
        ("bad.jpg", &b""[..]),
        ("bad.bmp", &b"BM"[..]),
    ] {
        std::fs::write(d.path().join("in/textures/a").join(n), bytes).unwrap();
    }
    let o = convert(d.path(), Target::Vanilla, &["subtype=diffuse"], &[]);
    assert_eq!((o.summary.converted, o.summary.failed), (1, 4), "{:?}", warnings(&o));
    assert_eq!(o.summary.exit_code(), 1);
    assert!(warnings(&o).iter().filter(|c| **c == Code::InDecode).count() == 4);
}
```

Also update the doc comment of `hdr_emissive()`: "The loader reads float32 TIFF, OpenEXR and Radiance HDR."

- [ ] **Step 6: Regenerate the schemas and run the gates.** Run `cargo run -p sdc -- presets schema all --out schema`, then the three gates. Expect everything to pass. If `broken_new_format_files_…` counts a failure as a skip rather than `failed`, read how `load_code` maps `LoadError::Decode` (`IN-DECODE`) in `api.rs` before changing the assertion: the expected count is 4 failed.

- [ ] **Step 7: Commit.**

```bash
git add Cargo.toml Cargo.lock THIRD_PARTY_NOTICES.md crates/engine/src/discover.rs crates/engine/src/load.rs crates/engine/src/analyse.rs crates/engine/src/api.rs crates/engine/src/codes.rs crates/engine/src/policy.rs crates/engine/tests/load.rs crates/engine/tests/discover_group.rs crates/engine/tests/golden.rs schema/
git commit -m "feat(load): OpenEXR, Radiance HDR, BMP and JPEG inputs; IN-ICC-IGNORED info (M6, DECISIONS S8, S10)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: NaN-safe analysis and `IN-NONFINITE` at plan time

**Files:**
- Modify: `crates/engine/src/analyse.rs`, `crates/engine/src/policy.rs` (`check_profile`), `crates/engine/src/pipeline.rs` (`prepare`, `prepare_then`, `process_file`), `crates/engine/src/session.rs` (the two `prepare(` calls, about lines 245 and 272)
- Test: `crates/engine/src/analyse.rs` (unit), `crates/engine/tests/api.rs`, `crates/cli/tests/cli_inspect_validate.rs`

**Interfaces:**
- Consumes: Task 1's `Analysis`.
- Produces: `Analysis.nonfinite: [u64; 4]`. `IN-NONFINITE` on the planned job, with `data.samples`. `pipeline::prepare(job, cancel, tracker, keep)`: the `rep` and `job_key` parameters are removed.

**Expectations that change:** none. `nonfinite_source_samples_raise_a_warning` still finds one warning with `samples == 1`; its new assertion pins "exactly one".

- [ ] **Step 1: Write the failing tests.** In `crates/engine/src/analyse.rs` `mod tests`:

```rust
    #[test]
    fn nan_and_infinite_samples_are_counted_and_left_out_of_the_statistics() {
        let mut img = Rgba32F::new(4, 1, [0.5, 0.5, 1.0, 1.0]);
        img.px[0] = [f32::NAN, 0.5, 1.0, 1.0];
        img.px[1] = [0.5, f32::INFINITY, 1.0, f32::NAN];
        img.px[2] = [f32::NEG_INFINITY, 0.5, 1.0, 1.0];
        let a = analyse(&img, &RGBA8);
        assert_eq!(a.nonfinite, [2, 1, 0, 1]);
        assert_eq!((a.channels[0].min, a.channels[0].max), (0.5, 0.5));
        assert!((a.channels[0].mean - 0.5).abs() < 1e-12, "mean over finite samples");
        assert!(a.channels[1].mean.is_finite() && a.channels[1].max == 0.5);
        assert!((a.mean_alpha - 1.0).abs() < 1e-12);
        // Only the fully finite pixel (index 3) is measured: (0, 0, 1) has unit length.
        assert!(a.normal_len_err < 1e-6 && (a.normal_len_mean - 1.0).abs() < 1e-6);
        assert!(a.peak_luminance.is_finite());
    }

    #[test]
    fn an_all_nan_channel_reports_zeros_not_nan() {
        let img = Rgba32F::new(2, 2, [f32::NAN, 0.5, 0.5, 1.0]);
        let a = analyse(&img, &RGBA8);
        assert_eq!(a.nonfinite[0], 4);
        assert_eq!((a.channels[0].min, a.channels[0].max, a.channels[0].mean), (0.0, 0.0, 0.0));
        assert_eq!((a.normal_len_err, a.normal_len_mean), (0.0, 0.0));
    }
```

In `crates/engine/tests/api.rs`, in `nonfinite_source_samples_raise_a_warning`, after `let w = …` add:

```rust
    let n = ev
        .iter()
        .filter(|e| matches!(&e.kind, EventKind::Warning(w) if w.code == Code::InNonfinite))
        .count();
    assert_eq!(n, 1, "reported once, at plan time");
```

and add the following test:

```rust
/// DECISIONS S9: `--dry-run` (plan only) reports IN-NONFINITE; alpha samples count only when alpha is kept.
#[test]
fn the_plan_reports_nonfinite_samples_before_any_output() {
    let d = tempfile::tempdir().unwrap();
    let t = d.path().join("textures");
    std::fs::create_dir_all(&t).unwrap();
    let img = ImageBuffer::from_fn(8, 8, |x, y| {
        let bad = x == 0 && y == 0;
        Rgba([if bad { f32::NAN } else { 0.5 }, 0.25, 0.75, if bad { f32::NAN } else { 1.0 }])
    });
    image::DynamicImage::ImageRgba32F(img).save(t.join("hot.tif")).unwrap();
    for (policy, want) in [("remove", 1u64), ("data", 2)] {
        let mut req = BatchRequest::new(vec![t.clone()], d.path().join("out"), Target::Cs);
        req.overrides = vec![
            parse_set("subtype=generic_data").unwrap(),
            parse_set(&format!("alpha.policy={policy}")).unwrap(),
        ];
        let plan = Engine::new()
            .unwrap()
            .plan(req, &Reporter::new(Arc::new(NullSink), "t"), &CancelToken::default())
            .unwrap();
        let w = plan.jobs[0]
            .warnings
            .iter()
            .find(|w| w.code == Code::InNonfinite)
            .expect("IN-NONFINITE at plan");
        assert_eq!(w.data["samples"], want, "{policy}");
        assert!(!d.path().join("out/hot.dds").exists());
    }
}
```

(`api.rs` already imports `ImageBuffer`, `Rgb`, `Rgba`, `parse_set`, `NullSink`, `Arc`. Add any that are missing to its `use` lines.)

In `crates/cli/tests/cli_inspect_validate.rs`:

```rust
#[test]
fn inspect_reports_nonfinite_samples_of_an_image() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("hot.tif");
    image::DynamicImage::ImageRgb32F(ImageBuffer::from_fn(4, 4, |x, _| {
        Rgb([if x == 0 { f32::NAN } else { 0.5 }, 0.5, 0.5])
    }))
    .save(&p)
    .unwrap();
    let o = sdc().args(["inspect", "--set", "subtype=generic_data"]).arg(&p).output().unwrap();
    let all = format!("{}{}", text(&o.stdout), text(&o.stderr));
    assert!(all.contains("IN-NONFINITE"), "{all}");
}
```

If `inspect` has no `--set` flag, read `InspectArgs` in `crates/cli/src/args.rs`. If it is missing there, drop the `--set` argument: the warning still shows on the suggested sub-type.

- [ ] **Step 2: Run them and see them fail.** Run `cargo test -p engine analyse:: --lib` and `cargo test -p engine --test api the_plan_reports`. Expect: no `nonfinite` field, and no IN-NONFINITE in the plan.

- [ ] **Step 3: Implement `analyse`.** Add the field after `icc_profile`:

```rust
    /// NaN and ±∞ samples per channel (R, G, B, A). They are left out of every statistic above; Preprocess replaces
    /// them (DECISIONS S9).
    pub nonfinite: [u64; 4],
```

Rewrite the loop and the finishing block of `analyse()`. Keep everything not shown here as it is:

```rust
    let mut nonfinite = [0u64; 4];
    let mut finite = [0u64; 4];
    let mut len_n = 0u64;
    for p in &img.px {
        for (c, (s, &v)) in ch.iter_mut().zip(p).enumerate() {
            if !v.is_finite() {
                nonfinite[c] += 1;
                continue;
            }
            s.min = s.min.min(v);
            s.max = s.max.max(v);
            s.mean += f64::from(v);
            finite[c] += 1;
        }
        // … the alpha histogram, gray, bb and rh lines stay as they are (code8 maps NaN to 0) …
        if p[..3].iter().all(|v| v.is_finite()) {
            let v = [p[0] * 2.0 - 1.0, p[1] * 2.0 - 1.0, p[2] * 2.0 - 1.0];
            let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
            len_err += f64::from((len - 1.0).abs());
            len_sum += f64::from(len);
            len_n += 1;
            peak = peak.max(0.2126 * p[0] + 0.7152 * p[1] + 0.0722 * p[2]);
        }
    }
    for (c, s) in ch.iter_mut().enumerate() {
        if finite[c] == 0 {
            // No finite sample: report zeros rather than the fold identities.
            (s.min, s.max) = (0.0, 0.0);
        }
        s.mean /= finite[c].max(1) as f64;
    }
    let ln = len_n.max(1) as f64;
```

In the `Analysis` literal, use `normal_len_err: len_err / ln`, `normal_len_mean: len_sum / ln` and `nonfinite,`. Delete the old `for c in &mut ch { if count == 0 … }` block. `n` is still used for `non_white_share`, `bb` and `rh`.

- [ ] **Step 4: Raise it at Resolve.** In `check_profile`, just before the `if bc6h_outside_cs {` block (after the last use of the `add` closure), add:

```rust
    // DECISIONS S9: the count Preprocess will replace: R, G, B always, A only when the alpha is kept.
    let kept_alpha = p.alpha.policy != AlphaPolicy::Remove;
    let samples: u64 = a.nonfinite[..3].iter().sum::<u64>() + if kept_alpha { a.nonfinite[3] } else { 0 };
    if samples > 0 {
        w.push(
            Warning::new(
                InNonfinite,
                format!("{samples} NaN or infinite source samples are replaced before processing"),
            )
            .with_data(json!({ "samples": samples })),
        );
    }
```

- [ ] **Step 5: Stop the run-time emission.** In `pipeline.rs`:
- Delete the `if work.nonfinite_samples > 0 { rep.emit_job(…) }` block in `prepare_then`.
- Remove the `rep: &Reporter` and `job_key: &str` parameters from `prepare` and `prepare_then`, and update their doc comments.
- Update the three call sites: `pipeline.rs` `prepare` (the wrapper), `pipeline.rs` `process_file` (`prepare_then(job, cancel, tracker, Keep::Encode, || { … })`), and `session.rs` (two `prepare(` calls: drop the reporter and the `""` key arguments).
- Remove `Code` from the `use crate::codes::{Code, Warning};` line only if clippy reports it unused; the test hook still uses it.

- [ ] **Step 6: Run the gates.** Expect all to pass.

- [ ] **Step 7: Commit.**

```bash
git add crates/engine/src/analyse.rs crates/engine/src/policy.rs crates/engine/src/pipeline.rs crates/engine/src/session.rs crates/engine/tests/api.rs crates/cli/tests/cli_inspect_validate.rs
git commit -m "feat(analyse): NaN-safe statistics; IN-NONFINITE reported at plan time, once (M6, DECISIONS S9)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: The float-source transfer split (retires the `IN-FLOAT-LDR` guard)

**Files:**
- Modify: `crates/engine/src/ops.rs` (`SourceTransfer`, `preprocess_with`), `crates/engine/src/pipeline.rs` (`prepare_then` load), `crates/engine/src/bake/mod.rs` (about line 202), `crates/engine/src/policy.rs` (`check_profile` float block, `float_source_is_linear` doc), `crates/engine/src/codes.rs`
- Regenerate: `schema/*.json` (severity is not in the schema, but re-run anyway: the doc comment may be)
- Test: `crates/engine/src/ops.rs` (unit), `crates/engine/tests/golden.rs`, `crates/engine/tests/api.rs`

**Interfaces:**
- Consumes: `SourceInfo::is_float`.
- Produces: `SourceTransfer`, `preprocess_with`. `IN-FLOAT-LDR` is never emitted (Info, retired).

**Expectations that change:**
- `golden.rs::float_source_to_an_ldr_format_is_blocked_until_allowed` is replaced by `float_source_to_bc7_is_linear_light_encoded_as_srgb`.
- `golden.rs::in_range_float_source_has_no_clamp_warning` drops its `--allow` and asserts the encoded values.
- `api.rs::nonfinite_source_samples_raise_a_warning` drops `req.allow = [Code::InFloatLdr].into();` and its comment.
- `codes.rs::severities_follow_spec_section_10` gains an `InFloatLdr` → Info assertion.

- [ ] **Step 1: Write the failing tests.** In `crates/engine/src/ops.rs` `mod tests`:

```rust
    #[test]
    fn a_float_source_is_never_srgb_decoded() {
        let p = profile(Subtype::Diffuse); // content srgb
        let img = Rgba32F::new(1, 1, [0.5, 0.5, 0.5, 1.0]);
        let stored = preprocess_with(img.clone(), &p, SourceTransfer::Stored);
        let linear = preprocess_with(img, &p, SourceTransfer::Linear);
        assert!((stored.img.px[0][0] - srgb_to_linear(0.5)).abs() < 1e-6, "integer: decoded");
        assert_eq!(linear.img.px[0][0], 0.5, "float: already linear");
        assert_eq!(linear.domain, Domain::LinearLight);
        let info = crate::image::SourceInfo { bit_depth: 32, channels: 3, has_alpha: false, is_float: true };
        assert_eq!(SourceTransfer::of(&info), SourceTransfer::Linear);
    }
```

(`profile()` is the existing helper in that module. If its signature differs, build the profile the same way the neighbouring tests do.)

In `golden.rs`, delete `float_source_to_an_ldr_format_is_blocked_until_allowed` and `in_range_float_source_has_no_clamp_warning`, then add:

```rust
/// Mean decoded value of channel `c` over mip 0.
fn mean_code(f: &DdsFile, c: usize) -> f64 {
    let l = &levels(f)[0];
    l.iter().map(|p| p[c] as f64).sum::<f64>() / l.len() as f64
}

/// DECISIONS S1: a float source is linear light. To BC7 under cs it converts without --allow; values above 1 are
/// clamped (LOSS-CLAMP-HDR, a warning) and every value is sRGB-encoded: a constant linear 0.25 blue decodes near
/// code 137 (sRGB 0.537), not 64 (the stored value).
#[test]
fn float_source_to_bc7_is_linear_light_encoded_as_srgb() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "textures/a/lamp.tif", hdr_emissive());
    let o = convert(d.path(), Target::Cs, &["subtype=diffuse"], &[]);
    assert_eq!((o.summary.converted, o.summary.exit_code()), (1, 0), "{:?}", warnings(&o));
    assert!(!warnings(&o).contains(&Code::InFloatLdr));
    assert_eq!(warning_of(&o, "lamp.tif", Code::LossClampHdr).severity, Severity::Warning);
    assert_profile(&o, "lamp.tif", "diffuse", "remove", false, "bc7");
    let b = mean_code(&dds(d.path(), "textures/a/lamp.dds"), 2);
    assert!((b - 137.0).abs() <= 3.0, "blue {b}");
}

/// A float source inside [0, 1] has nothing to clamp: no LOSS-CLAMP-HDR, and linear 0.5 decodes near 188.
#[test]
fn in_range_float_source_has_no_clamp_warning() {
    let d = tempfile::tempdir().unwrap();
    put(
        d.path(),
        "textures/a/plate.tif",
        DynamicImage::ImageRgb32F(ImageBuffer::from_fn(64, 64, |x, y| {
            Rgb([x as f32 / 63.0, y as f32 / 63.0, 0.5])
        })),
    );
    let o = convert(d.path(), Target::Cs, &["subtype=diffuse"], &[]);
    assert_eq!(o.summary.converted, 1, "{:?}", warnings(&o));
    assert!(!warnings(&o).contains(&Code::LossClampHdr));
    assert!(!warnings(&o).contains(&Code::InFloatLdr));
    let b = mean_code(&dds(d.path(), "textures/a/plate.dds"), 2);
    assert!((b - 188.0).abs() <= 3.0, "blue {b}");
}

/// Review Focus 3: under vanilla (no BC6H) a float diffuse is still sRGB-encoded, into a UNORM file (content srgb,
/// tag unorm), not written as stored values.
#[test]
fn a_float_source_under_vanilla_is_srgb_encoded_into_unorm() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "textures/a/lamp.tif", hdr_emissive());
    let o = convert(d.path(), Target::Vanilla, &["subtype=diffuse"], &[]);
    assert_eq!(o.summary.converted, 1, "{:?}", warnings(&o));
    let f = dds(d.path(), "textures/a/lamp.dds");
    assert_eq!(f.format, DdsPixelFormat::Bc7 { srgb: false });
    assert!((mean_code(&f, 2) - 137.0).abs() <= 3.0);
}

/// Review Focus 3: a float data map (content linear) is written as stored values.
#[test]
fn a_float_data_map_is_written_as_stored_values() {
    let d = tempfile::tempdir().unwrap();
    put(
        d.path(),
        "textures/a/mask.tif",
        DynamicImage::ImageRgb32F(ImageBuffer::from_fn(64, 64, |_, _| Rgb([0.25, 0.5, 0.75]))),
    );
    let o = convert(d.path(), Target::Cs, &["subtype=generic_data"], &[]);
    assert_eq!(o.summary.converted, 1, "{:?}", warnings(&o));
    let f = dds(d.path(), "textures/a/mask.dds");
    for (c, want) in [(0, 64.0), (1, 128.0), (2, 191.0)] {
        assert!((mean_code(&f, c) - want).abs() <= 2.0, "channel {c}: {}", mean_code(&f, c));
    }
}
```

- [ ] **Step 2: Run them and see them fail.** Run `cargo test -p engine --test golden float_ in_range a_float`. Expect: `skip_code` IN-FLOAT-LDR (not converted), and `preprocess_with` undefined.

- [ ] **Step 3: Implement the split in `ops.rs`.**

```rust
use crate::image::SourceInfo;

/// How a source's stored values relate to light (DECISIONS S1). An integer source holds stored values whose meaning
/// `color.content` gives (`srgb` → decoded in step 5). A float source is linear light already and is never decoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceTransfer {
    Stored,
    Linear,
}

impl SourceTransfer {
    pub fn of(info: &SourceInfo) -> Self {
        if info.is_float {
            SourceTransfer::Linear
        } else {
            SourceTransfer::Stored
        }
    }
}

/// `preprocess_with` for an integer source (tests, bench and callers without a `SourceInfo`).
pub fn preprocess(img: Rgba32F, p: &Profile) -> Work {
    preprocess_with(img, p, SourceTransfer::Stored)
}
```

Rename the existing `pub fn preprocess(mut img: Rgba32F, p: &Profile) -> Work` to `pub fn preprocess_with(mut img: Rgba32F, p: &Profile, transfer: SourceTransfer) -> Work`. Change the step-5 condition to:

```rust
    if p.color.content == ColorContent::Srgb
        && transfer == SourceTransfer::Stored
        && matches!(domain, Domain::LinearLight | Domain::Hdr)
    {
```

Update the module doc: "(5) sRGB decode (integer sources only, DECISIONS S1)".

- [ ] **Step 4: Pass the transfer from real loads.** In `pipeline.rs` `prepare_then`:

```rust
    let (img, transfer) = match &job.pack {
        Some(pk) => {
            // The gray source fixed at plan time (Note M6), not `p.channels.grayscale_source` of the final profile.
            let asm = assemble(pk, pk.gray).map_err(|e| fail(Stage::Load)(e.to_string()))?;
            let t = SourceTransfer::of(&asm.info);
            (asm.image, t)
        }
        None => {
            let (img, info) =
                load_image(&job.input).map_err(|e| fail(Stage::Load)(e.to_string()))?;
            (img, SourceTransfer::of(&info))
        }
    };
```

Then `let work = preprocess_with(img, p, transfer);`, and import `crate::ops::{preprocess_with, SourceTransfer}`. In `bake/mod.rs` at about line 202:

```rust
        let (img, info) = crate::load::load_image(&self.normal).map_err(|e| e.to_string())?;
        let mut work = crate::ops::preprocess_with(
            img,
            &self.normal_profile,
            crate::ops::SourceTransfer::of(&info),
        );
```

- [ ] **Step 5: Retire the guard in `policy.rs`.** Replace the whole "Final review I4, an interim guard…" block in `check_profile` with:

```rust
    // DECISIONS S1: a float source is linear light; Quantise clamps it to [0, 1] and encodes it per color.content.
    // Values outside that range are lost: a warning, never a block. (IN-FLOAT-LDR is retired, DECISIONS S2.)
    if f.info.is_float && !bc6h {
        let channels = if f.info.has_alpha { 4 } else { 3 };
        let (lo, hi) = a.channels[..channels]
            .iter()
            .fold((f32::MAX, f32::MIN), |(lo, hi), c| (lo.min(c.min), hi.max(c.max)));
        if lo < 0.0 || hi > 1.0 {
            add(
                LossClampHdr,
                format!("float values range {lo} to {hi}: values outside [0, 1] are clamped"),
            );
        }
    }
```

Change the doc comment of `float_source_is_linear` from "Float sources written to LDR formats keep the sub-type's content (a separate carry)." to "A float source written to an LDR format keeps the sub-type's content, which then names only the output encoding: the source is linear light either way (DECISIONS S1)."

`codes.rs`:
- Change `InFloatLdr = "IN-FLOAT-LDR" => Blocking,` to `=> Info,`, with the comment `// Retired in M6 (DECISIONS S2): never emitted; kept so --allow and [files] allow lists still load.`
- In the module doc, replace "(a float source written to an LDR format, an interim guard until the M6 source-transfer split)" with "(retired in M6: never emitted)".
- Add `assert_eq!(Code::InFloatLdr.severity(), Severity::Info);` to `severities_follow_spec_section_10`.

`crates/engine/tests/api.rs`: remove the two `IN-FLOAT-LDR` lines in `nonfinite_source_samples_raise_a_warning`.

- [ ] **Step 6: Run the gates.** `golden::hdr_float_emissive_is_bc6h` and `hdr_float_emissive_content_is_linear` must pass unchanged: BC6H is untouched.

- [ ] **Step 7: Commit.**

```bash
git add crates/engine/src/ops.rs crates/engine/src/pipeline.rs crates/engine/src/bake/mod.rs crates/engine/src/policy.rs crates/engine/src/codes.rs crates/engine/tests/golden.rs crates/engine/tests/api.rs schema/
git commit -m "feat(transfer): float sources are linear light for every format; IN-FLOAT-LDR retired (M6, DECISIONS S1, S2)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: O2, the HDR check before BC6H for `pbr_emissive`

**Files:**
- Modify: `crates/engine/src/policy.rs` (`is_hdr_source`, `emissive_hdr_choice`, `hdr_source_notice`, the `layered_profile_with` call order, `resolve_decided`), `crates/engine/src/codes.rs`
- Regenerate: `schema/*.json`
- Test: `crates/engine/tests/hdr_source.rs` (create), `crates/engine/tests/golden.rs`, `crates/engine/tests/policy_checks.rs`

**Interfaces:**
- Consumes: Task 3's split (a float `_g` at or below 1.0 now encodes correctly to BC7 sRGB).
- Produces: `is_hdr_source`, provenance `analysis:hdr-source`, `Code::FmtHdrSource` (Info). Code count 65.

**Expectations that change:**
- `golden::bc6h_emissive_in_a_pbr_set` becomes `an_8_bit_emissive_is_bc7_srgb_not_bc6h`: BC7 sRGB, warnings `[FMT-HDR-SOURCE]`.
- `golden::hdr_float_emissive_is_bc6h`: warnings `vec![Code::FmtHdrSource]`.
- `golden::exr_and_radiance_hdr_float_emissives_are_bc6h` (Task 1): warnings `vec![Code::FmtHdrSource]`.
- `policy_checks::bc6h_and_tag_na_must_agree`: the first half (an 8-bit `_g` with `subtype:pbr_emissive.format=bc7` and nothing else) is now `Ready` with tag `srgb` (S3: the tag follows the format); the `hdr_sky` half is unchanged.
- `codes` count 64 → 65.

- [ ] **Step 1: Write the failing tests.** Create `crates/engine/tests/hdr_source.rs`:

```rust
//! M6 Task 4 (DECISIONS O2, S3): pbr_emissive is BC6H only for a float source peaking above 1.0.
use engine::api::*;
use engine::codes::Code;
use engine::config::*;
use engine::events::*;
use engine::overrides::parse_set;
use engine::policy::{AcceptSuggestions, JobStatus, ResolvedJob};
use image::{DynamicImage, ImageBuffer, Rgb};
use std::path::Path;
use std::sync::Arc;

fn plan_one(dir: &Path, name: &str, img: DynamicImage, sets: &[&str]) -> ResolvedJob {
    let p = dir.join("in/textures/pbr").join(name);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    img.save(&p).unwrap();
    let mut req = BatchRequest::new(vec![dir.join("in")], dir.join("out"), Target::Cs);
    req.recursive = true;
    req.accept = AcceptSuggestions::High;
    req.overrides = sets.iter().map(|s| parse_set(s).unwrap()).collect();
    let plan = Engine::new()
        .unwrap()
        .plan(req, &Reporter::new(Arc::new(NullSink), "t"), &CancelToken::default())
        .unwrap();
    plan.jobs.into_iter().next().expect("one job")
}

fn float_g(peak: f32) -> DynamicImage {
    DynamicImage::ImageRgb32F(ImageBuffer::from_fn(16, 16, move |x, _| {
        Rgb([peak * x as f32 / 15.0, 0.2, 0.1])
    }))
}

fn eight_bit_g() -> DynamicImage {
    DynamicImage::ImageRgb8(ImageBuffer::from_fn(16, 16, |x, _| Rgb([(x * 16) as u8, 40, 20])))
}

fn reason(j: &ResolvedJob) -> String {
    j.warnings
        .iter()
        .find(|w| w.code == Code::FmtHdrSource)
        .expect("FMT-HDR-SOURCE")
        .message
        .clone()
}

fn shape(j: &ResolvedJob) -> (Format, ColorTag, Backend, String) {
    (j.profile.format, j.profile.color.tag, j.profile.encoder.backend, j.provenance["format"].to_string())
}

#[test]
fn an_8_bit_emissive_is_bc7_srgb_as_an_analysis_fact() {
    let d = tempfile::tempdir().unwrap();
    let j = plan_one(d.path(), "lamp_g.png", eight_bit_g(), &[]);
    assert_eq!(shape(&j), (Format::Bc7, ColorTag::Srgb, Backend::Bc7e, "analysis:hdr-source".into()));
    assert_eq!(j.status, JobStatus::Ready, "{:?}", j.warnings);
    assert!(reason(&j).contains("8-bit source, no HDR → BC7 sRGB"), "{}", reason(&j));
}

#[test]
fn a_float_emissive_above_one_is_bc6h() {
    let d = tempfile::tempdir().unwrap();
    let j = plan_one(d.path(), "lamp_g.tif", float_g(4.0), &[]);
    assert_eq!((j.profile.format, j.profile.color.tag), (Format::Bc6hUf16, ColorTag::NotApplicable));
    assert_eq!(j.profile.encoder.backend, Backend::IspcBc6h);
    assert!(reason(&j).contains("peak 4.00 > 1.0: HDR → BC6H"), "{}", reason(&j));
}

#[test]
fn a_float_emissive_at_or_below_one_is_bc7_srgb() {
    for peak in [0.9f32, 1.0] {
        let d = tempfile::tempdir().unwrap();
        let j = plan_one(d.path(), "lamp_g.tif", float_g(peak), &[]);
        assert_eq!((j.profile.format, j.profile.color.tag), (Format::Bc7, ColorTag::Srgb), "{peak}");
        assert!(reason(&j).contains("no HDR → BC7 sRGB"), "{peak}: {}", reason(&j));
    }
}

#[test]
fn an_explicit_format_wins_in_both_directions_and_the_tag_follows() {
    let d = tempfile::tempdir().unwrap();
    let j = plan_one(d.path(), "lamp_g.png", eight_bit_g(), &["file:**/lamp_g.png.format=bc6h_uf16"]);
    assert_eq!(shape(&j), (Format::Bc6hUf16, ColorTag::NotApplicable, Backend::IspcBc6h, "user".into()));
    assert_eq!(j.status, JobStatus::Ready, "{:?}", j.warnings);
    assert!(reason(&j).contains("the HDR check is skipped"), "{}", reason(&j));
    let d = tempfile::tempdir().unwrap();
    let j = plan_one(d.path(), "lamp_g.tif", float_g(4.0), &["file:**/lamp_g.tif.format=bc7"]);
    assert_eq!((j.profile.format, j.profile.color.tag), (Format::Bc7, ColorTag::Srgb));
    assert_eq!(j.status, JobStatus::Ready, "{:?}", j.warnings);
    // An explicit tag is kept, even when it disagrees (and CFG-INVALID then reports it).
    let d = tempfile::tempdir().unwrap();
    let j = plan_one(
        d.path(),
        "lamp_g.png",
        eight_bit_g(),
        &["file:**/lamp_g.png.format=bc6h_uf16", "file:**/lamp_g.png.color.tag=srgb"],
    );
    assert!(j.warnings.iter().any(|w| w.code == Code::CfgInvalid));
}

#[test]
fn hdr_sky_is_unchanged() {
    let d = tempfile::tempdir().unwrap();
    let j = plan_one(d.path(), "dome.tif", float_g(0.5), &["subtype=hdr_sky"]);
    assert_eq!(j.profile.format, Format::Bc6hUf16);
    assert!(j.warnings.iter().all(|w| w.code != Code::FmtHdrSource));
}
```

In `golden.rs`:
- Rename `bc6h_emissive_in_a_pbr_set` to `an_8_bit_emissive_is_bc7_srgb_not_bc6h`.
- Expect `warnings(&o) == vec![Code::FmtHdrSource]`, `assert_profile(…, "bc7")`, tag `"srgb"` and `assert_dds(…, DdsPixelFormat::Bc7 { srgb: true }, HeaderKind::Dx10, 7)`.
- In `hdr_float_emissive_is_bc6h` and `exr_and_radiance_hdr_float_emissives_are_bc6h`, change `vec![]` to `vec![Code::FmtHdrSource]`.

In `policy_checks.rs::bc6h_and_tag_na_must_agree`, replace the first `run(…)` and its two asserts with:

```rust
    // DECISIONS S3: on pbr_emissive the tag follows the format unless a layer set it.
    assert_eq!(j.status, JobStatus::Ready, "{:?}", j.warnings);
    assert_eq!(j.profile.color.tag, ColorTag::Srgb);
```

(keep `let j = run(…)` itself). Leave the second `run` (with an explicit `color.tag=srgb`) and the `hdr_sky` half unchanged.

- [ ] **Step 2: Run them and see them fail.** Run `cargo test -p engine --test hdr_source`. Expect: no `FmtHdrSource`, and BC6H for the 8-bit `_g`.

- [ ] **Step 3: Implement.** `codes.rs`:
- Add `FmtHdrSource = "FMT-HDR-SOURCE" => Info,` after `FmtPerceptualData`.
- Add to the doc list "FMT-HDR-SOURCE (why `pbr_emissive` is or is not BC6H, O2)".
- Count 64 → 65.

`policy.rs`:

```rust
/// DECISIONS O2/S3: a float source with a finite R, G or B above 1.0.
pub fn is_hdr_source(f: &FileFacts) -> bool {
    f.info.is_float && f.analysis.channels[..3].iter().any(|c| c.max > 1.0)
}

/// DECISIONS O2/S3, the fact `analysis:hdr-source`: a `pbr_emissive` whose BC6H format no layer chose explicitly
/// becomes BC7 unless the source is HDR; then, on every `pbr_emissive`, `color.tag` follows the final format
/// (`n/a` for BC6H, else `srgb`) unless a layer set it. Runs after every layer, so an explicit `format` or tag from
/// a user/project preset, config, rule, `[files]` or `--set` is kept.
fn emissive_hdr_choice(
    ctx: &ResolveCtx,
    p: &mut Profile,
    prov: &mut BTreeMap<String, Provenance>,
    f: &FileFacts,
) {
    if p.subtype != Subtype::PbrEmissive {
        return;
    }
    let fact = || Provenance::AnalysisFact("hdr-source".into());
    if p.format == Format::Bc6hUf16 && !field_explicit(ctx, prov, "format") && !is_hdr_source(f) {
        p.format = Format::Bc7;
        prov.insert("format".into(), fact());
        if !field_explicit(ctx, prov, "encoder.backend") {
            p.encoder.backend = default_backend(p.format);
            prov.insert("encoder.backend".into(), fact());
        }
    }
    if !field_explicit(ctx, prov, "color.tag") {
        let tag = if p.format == Format::Bc6hUf16 {
            ColorTag::NotApplicable
        } else {
            ColorTag::Srgb
        };
        if p.color.tag != tag {
            p.color.tag = tag;
            prov.insert("color.tag".into(), fact());
        }
    }
}

/// The `FMT-HDR-SOURCE` notice: why a `pbr_emissive` is or is not BC6H (DECISIONS S3).
fn hdr_source_notice(
    ctx: &ResolveCtx,
    p: &Profile,
    f: &FileFacts,
    prov: &BTreeMap<String, Provenance>,
) -> Warning {
    let hdr = is_hdr_source(f);
    let peak = f
        .info
        .is_float
        .then(|| f.analysis.channels[..3].iter().map(|c| c.max).fold(0.0f32, f32::max));
    let source = match peak {
        Some(v) if hdr => format!("float source, peak {v:.2} > 1.0: HDR"),
        Some(v) => format!("float source, peak {v:.2} ≤ 1.0: no HDR"),
        None => format!("{}-bit source, no HDR", f.info.bit_depth),
    };
    let by_fact = matches!(prov.get("format"), Some(Provenance::AnalysisFact(x)) if x == "hdr-source");
    let explicit = !by_fact && field_explicit(ctx, prov, "format");
    let chosen = match (p.format, p.color.tag) {
        (Format::Bc6hUf16, _) => "BC6H".to_string(),
        (fmt, ColorTag::Srgb) => format!("{} sRGB", fmt.id().to_uppercase()),
        (fmt, _) => format!("{} UNORM", fmt.id().to_uppercase()),
    };
    let message = if explicit {
        let src = prov.get("format").map_or("builtin".into(), |s| s.to_string());
        format!("{source}; format {} from {src} (the HDR check is skipped)", p.format.id())
    } else {
        format!("{source} → {chosen}")
    };
    Warning::new(Code::FmtHdrSource, message).with_data(json!({
        "hdr": hdr, "float": f.info.is_float, "peak": peak, "format": p.format, "explicit": explicit
    }))
}
```

- In `layered_profile_with`, call `emissive_hdr_choice(ctx, &mut profile, &mut prov, f);` right after `backend_follows_format(…)` and before `float_source_is_linear(…)`.
- In `resolve_decided`, after the `SkinWet` notice, add:

```rust
    if available && profile.subtype == Subtype::PbrEmissive {
        warnings.push(hdr_source_notice(ctx, &profile, f, &provenance));
    }
```

- [ ] **Step 4: Run the gates.** Expect all to pass, including `preview::a_bc6h_preview_from_a_smaller_mip_measures_the_peak_of_that_mip` (its peak is 100, so it stays BC6H) and `policy_layers::float_source_to_bc6h_resolves_linear_content_as_an_analysis_fact`.

- [ ] **Step 5: Regenerate the schemas and commit.**

```bash
cargo run -p sdc -- presets schema all --out schema
git add crates/engine/src/policy.rs crates/engine/src/codes.rs crates/engine/tests/hdr_source.rs crates/engine/tests/golden.rs crates/engine/tests/policy_checks.rs schema/
git commit -m "feat(policy): pbr_emissive is BC6H only for a float source above 1.0; FMT-HDR-SOURCE (M6, DECISIONS O2, S3)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: Uncompressed formats: `Format`, `Backend`, DDS writer and reader, packing

**Files:**
- Modify: `crates/engine/src/config.rs`, `crates/engine/src/dds.rs`, `crates/engine/src/decode.rs` (the `Bgra8` arm), `crates/engine/src/encode.rs`, `crates/engine/src/quantize.rs`, `crates/engine/src/ops.rs` (`domain_for`), `crates/engine/src/policy.rs` (`backend_fits`, `default_backend`), `crates/cli/src/verbs.rs` (`carries_alpha`, the `format_name` test), `crates/engine/tests/dds_read.rs` (`Bgra8` literals at about lines 678 and 1063)
- Modify for compilation only (exhaustive matches; behaviour in Task 11): `crates/gui/src/model/preview.rs` (`pixel_format` delegates to `Format::pixel_format`), `crates/gui/src/model/fields.rs` (`format_label` arms), `crates/gui/src/render/upload.rs` (`wgpu_format` returns `Option`, `block_layout`), `crates/gui/src/render/mod.rs` (`set_encoded` takes the BC path only for block formats)
- Regenerate: `schema/*.json`
- Test: `crates/engine/tests/dds_write.rs`, `crates/engine/tests/dds_read.rs`, `crates/engine/src/encode.rs` and `quantize.rs` (unit), `crates/engine/src/config.rs` (unit), `crates/gui/tests/fields.rs`

**Interfaces:**
- Consumes: nothing new.
- Produces: everything in the registry rows for Task 5.

**Expectations that change:**
- `verbs::tests::format_name_splits_the_srgb_tag`: `Bgra8` becomes `("Bgra8", Some(false))`.
- `config::tests::unknown_fields_are_rejected`: `block_bytes()` returns an `Option`.
- `dds_write::resolve_header_matrix` and `dxgi_values` gain rows. Their existing rows are unchanged.

- [ ] **Step 1: Write the failing tests.** Append to `crates/engine/tests/dds_write.rs`:

```rust
#[test]
fn uncompressed_formats_have_their_dxgi_header_and_pitch() {
    for (f, srgb, dxgi, bpp) in [
        (Format::Rgba8, false, 87, 4),
        (Format::Rgba8, true, 91, 4),
        (Format::R8, false, 61, 1),
        (Format::Rg8, false, 49, 2),
        (Format::Rgba16f, false, 10, 8),
    ] {
        assert_eq!(dxgi_format(f, srgb), dxgi, "{f:?}");
        let d = DdsDesc { width: 37, height: 5, format: f, srgb, header: ResolvedHeader::Dx10 };
        let levels = chain(f, 37, 5);
        let b = dds_bytes(&d, &levels).unwrap();
        assert_eq!(u(&b, 8) & (DDSD_PITCH | DDSD_LINEARSIZE), DDSD_PITCH, "{f:?}: pitch, not linear size");
        assert_eq!(u(&b, 20), 37 * bpp, "{f:?} row pitch");
        assert_eq!(u(&b, 128), dxgi);
        assert_payload(&b, 148, &levels);
        let r = read_dds(&b).unwrap();
        assert_eq!(r.format.as_format(), Some((f, srgb)), "{f:?} reads back as itself");
        assert!(check_header(&r).is_empty(), "{f:?}: {:?}", check_header(&r));
    }
}

#[test]
fn rgba8_unorm_auto_is_legacy_b8g8r8a8_masks() {
    assert_eq!(resolve_header(Format::Rgba8, false, HeaderForm::Auto), ResolvedHeader::Legacy);
    assert_eq!(resolve_header(Format::Rgba8, true, HeaderForm::Auto), ResolvedHeader::Dx10);
    for f in [Format::R8, Format::Rg8, Format::Rgba16f] {
        assert_eq!(resolve_header(f, false, HeaderForm::Auto), ResolvedHeader::Dx10, "{f:?}");
        assert_eq!(resolve_header(f, false, HeaderForm::Legacy), ResolvedHeader::Dx10, "{f:?}");
    }
    let d = DdsDesc { width: 4, height: 4, format: Format::Rgba8, srgb: false, header: ResolvedHeader::Legacy };
    let levels = chain(Format::Rgba8, 4, 4);
    let b = dds_bytes(&d, &levels).unwrap();
    assert_eq!(u(&b, 80), DDPF_RGB | DDPF_ALPHAPIXELS);
    assert_eq!((u(&b, 88), u(&b, 92), u(&b, 96), u(&b, 100), u(&b, 104)), (32, 0xff_0000, 0xff00, 0xff, 0xff00_0000));
    assert_payload(&b, 128, &levels);
    assert_eq!(read_dds(&b).unwrap().format, DdsPixelFormat::Bgra8 { srgb: false });
    // A legacy sRGB rgba8 has no form.
    let d = DdsDesc { srgb: true, ..d };
    assert!(matches!(dds_bytes(&d, &levels), Err(DdsError::NoLegacyForm { .. })));
}

#[test]
fn header_bytes_is_the_head_of_dds_bytes() {
    let d = DdsDesc { width: 64, height: 32, format: Format::Bc7, srgb: true, header: ResolvedHeader::Dx10 };
    let levels = chain(Format::Bc7, 64, 32);
    let b = dds_bytes(&d, &levels).unwrap();
    assert_eq!(header_bytes(&d, levels.len() as u32).unwrap(), b[..148].to_vec());
}
```

In `crates/engine/tests/dds_read.rs`:
- Change `DdsPixelFormat::Bgra8` to `DdsPixelFormat::Bgra8 { srgb: false }` and `P::Bgra8` to `P::Bgra8 { srgb: false }`.
- Add:

```rust
#[test]
fn dxgi_round_trips_through_the_pixel_format() {
    for d in [2u32, 10, 28, 29, 49, 61, 71, 72, 74, 75, 77, 78, 80, 83, 87, 88, 91, 95, 96, 98, 99] {
        let f = from_dxgi(d).unwrap_or_else(|| panic!("{d}"));
        assert_eq!(f.dxgi(), d);
    }
    assert_eq!(from_dxgi(91), Some(DdsPixelFormat::Bgra8 { srgb: true }));
    assert_eq!(from_dxgi(12345), None);
}
```

In `crates/engine/src/encode.rs` `mod tests`:

```rust
    #[test]
    fn uncompressed_levels_are_packed_as_their_dxgi_layout() {
        let mut p = bc7_exact_profile();
        let px = vec![[10u8, 20, 30, 40], [50, 60, 70, 80]];
        for (f, want) in [
            (Format::Rgba8, vec![30u8, 20, 10, 40, 70, 60, 50, 80]),
            (Format::R8, vec![10, 50]),
            (Format::Rg8, vec![10, 20, 50, 60]),
        ] {
            p.format = f;
            p.encoder.backend = Backend::Uncompressed;
            assert_eq!(encode_level(&EncoderInput::Ldr(px.clone()), 2, 1, &p, &EncodeCtx::default()).unwrap(), want, "{f:?}");
        }
        p.format = Format::Rgba16f;
        let h = [half::f16::from_f32(2.0).to_bits(), 0, 0x3c00, 0x3800];
        let out = encode_level(&EncoderInput::Hdr(vec![h]), 1, 1, &p, &EncodeCtx::default()).unwrap();
        assert_eq!(out, h.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<u8>>());
    }
```

In `crates/engine/src/quantize.rs` `mod tests`:

```rust
    #[test]
    fn rgba16f_keeps_alpha_as_half_and_bc6h_forces_one() {
        let mut p = profile(Subtype::PbrEmissive);
        p.format = Format::Rgba16f;
        let w = work([2.0, 0.5, 0.25, 0.5], Domain::Hdr, true);
        let EncoderInput::Hdr(h) = to_encoder_input(&w.img, &w, &p) else { panic!() };
        assert_eq!(h[0][3], f16::from_f32(0.5).to_bits());
        let w = work([2.0, 0.5, 0.25, 0.5], Domain::Hdr, false);
        let EncoderInput::Hdr(h) = to_encoder_input(&w.img, &w, &p) else { panic!() };
        assert_eq!(h[0][3], f16::ONE.to_bits());
        assert_eq!(hdr([2.0, 0.5, 0.25, 0.5])[3], f16::ONE.to_bits(), "BC6H unchanged");
    }
```

In `crates/engine/src/config.rs` tests:
- Change the two `block_bytes()` asserts to `Some(16)` and `Some(8)`.
- Add `assert_eq!(Format::Rgba8.block_bytes(), None);`.
- Add a test:

```rust
    #[test]
    fn uncompressed_formats_serialise_and_classify() {
        for (f, id) in [(Format::Rgba8, "rgba8"), (Format::R8, "r8"), (Format::Rg8, "rg8"), (Format::Rgba16f, "rgba16f")] {
            assert_eq!(f.id(), id);
            assert!(!f.is_block());
        }
        assert!(Format::Rgba16f.is_hdr() && Format::Bc6hUf16.is_hdr() && !Format::Rgba8.is_hdr());
        assert!(Format::Rgba8.has_alpha() && Format::Rgba16f.has_alpha() && !Format::R8.has_alpha());
        assert_eq!(serde_json::to_string(&Backend::Uncompressed).unwrap(), "\"uncompressed\"");
        assert_eq!((Format::Rgba8.bytes_per_pixel(), Format::Rgba16f.bytes_per_pixel()), (4, 8));
    }
```

In `crates/gui/tests/fields.rs`:

```rust
#[test]
fn uncompressed_format_labels_say_so() {
    assert_eq!(format_label(Format::Rgba8, ColorTag::Srgb), "RGBA8 · sRGB (uncompressed)");
    assert_eq!(format_label(Format::R8, ColorTag::Unorm), "R8 · linear (one channel, uncompressed)");
    assert_eq!(format_label(Format::Rg8, ColorTag::Unorm), "RG8 · linear (two channels, uncompressed)");
    assert_eq!(format_label(Format::Rgba16f, ColorTag::NotApplicable), "RGBA16F · HDR linear (uncompressed)");
}
```

- [ ] **Step 2: Run them and see them fail.** Run `cargo test -p engine --test dds_write`. Expect compile errors on the new variants.

- [ ] **Step 3: Implement `config.rs`.**

```rust
cfg_enum!(Format, "lowercase" { Bc1, Bc3, Bc4, Bc5, Bc6hUf16 = "bc6h_uf16", Bc7, Rgba8, R8, Rg8, Rgba16f });
cfg_enum!(Backend, "snake_case" { Bc7e, Rgbcx, IspcBc6h, Nvtt, Uncompressed });
```

Replace the `impl Format` body's `block_bytes` and `has_alpha` and add the helpers:

```rust
    /// The DDS pixel format this format and tag are written as (DECISIONS S4: `rgba8` is B8G8R8A8).
    pub fn pixel_format(self, srgb: bool) -> crate::dds::DdsPixelFormat {
        use crate::dds::DdsPixelFormat as P;
        match self {
            Format::Bc1 => P::Bc1 { srgb },
            Format::Bc3 => P::Bc3 { srgb },
            Format::Bc4 => P::Bc4,
            Format::Bc5 => P::Bc5,
            Format::Bc6hUf16 => P::Bc6hUf16,
            Format::Bc7 => P::Bc7 { srgb },
            Format::Rgba8 => P::Bgra8 { srgb },
            Format::R8 => P::R8,
            Format::Rg8 => P::Rg8,
            Format::Rgba16f => P::Rgba16f,
        }
    }
    /// Bytes per 4x4 block, or `None` for an uncompressed format.
    pub fn block_bytes(self) -> Option<usize> {
        self.pixel_format(false).block_bytes()
    }
    /// Bytes per texel of an uncompressed format (0 for a block format).
    pub fn bytes_per_pixel(self) -> usize {
        self.pixel_format(false).bytes_per_pixel()
    }
    pub fn is_block(self) -> bool {
        self.block_bytes().is_some()
    }
    /// Float formats: no `_SRGB`/UNORM tag (`n/a`), the `Hdr` domain, `cs` only (DECISIONS S4).
    pub fn is_hdr(self) -> bool {
        matches!(self, Format::Bc6hUf16 | Format::Rgba16f)
    }
    pub fn has_alpha(self) -> bool {
        matches!(self, Format::Bc1 | Format::Bc3 | Format::Bc7 | Format::Rgba8 | Format::Rgba16f)
    }
```

- [ ] **Step 4: Implement `dds.rs`.** Add the following (and make `DdsPixelFormat::bytes_per_pixel` `pub`):

```rust
/// Legacy uncompressed pixel format: RGB bit masks are valid.
pub const DDPF_RGB: u32 = 0x40;
/// Legacy uncompressed pixel format: the alpha mask is valid.
pub const DDPF_ALPHAPIXELS: u32 = 0x1;

/// The legacy pixel-format block `(flags, bit count, [R, G, B, A] masks)` of an uncompressed format: only `rgba8`
/// UNORM has one, B8G8R8A8 (DECISIONS S4).
pub fn legacy_masks(format: Format) -> Option<(u32, u32, [u32; 4])> {
    match format {
        Format::Rgba8 => Some((DDPF_RGB | DDPF_ALPHAPIXELS, 32, [0xff_0000, 0xff00, 0xff, 0xff00_0000])),
        _ => None,
    }
}
```

Changes to existing functions:
- `legacy_fourcc`: add `| Format::Rgba8 | Format::R8 | Format::Rg8 | Format::Rgba16f` to the `None` arm.
- `resolve_header`: `let legacy_ok = !srgb && (legacy_fourcc(format).is_some() || legacy_masks(format).is_some());`, and the `Auto` arm matches `Format::Bc1 | Format::Bc3 | Format::Rgba8`. Update the doc to "`Auto` picks legacy only for BC1/BC3/RGBA8 UNORM".
- `dxgi_format`: add `(Format::Rgba8, false) => 87, (Format::Rgba8, true) => 91, (Format::R8, _) => 61, (Format::Rg8, _) => 49, (Format::Rgba16f, _) => 10`.
- `checked_level_size(format, w, h)` becomes `format.pixel_format(false).checked_level_size(w, h)`. Update its doc to "whole blocks for a block format, `w * h * bytes_per_pixel` otherwise".

Split `dds_bytes`. Its level checks stay; the header goes into:

```rust
/// The magic, the 124-byte header and (for DX10) the 20-byte DX10 block of a `desc` file with `mips` levels. A block
/// format carries `DDSD_LINEARSIZE` and the mip-0 size, an uncompressed one `DDSD_PITCH` and the row pitch.
pub fn header_bytes(desc: &DdsDesc, mips: u32) -> Result<Vec<u8>, DdsError> {
    let (w, h) = (desc.width, desc.height);
    if w == 0 || h == 0 {
        return Err(DdsError::ZeroDimension { width: w, height: h });
    }
    let block = desc.format.is_block();
    let size = if block {
        checked_level_size(desc.format, w, h)
    } else {
        checked_level_size(desc.format, w, 1)
    }
    .ok_or(DdsError::SizeOverflow { level: 0, width: w, height: h })?;
    let size = u32::try_from(size).map_err(|_| DdsError::LinearSizeOverflow(size))?;
    let mut hd = [0u32; 31];
    hd[0] = 124;
    hd[1] = DDSD_CAPS
        | DDSD_HEIGHT
        | DDSD_WIDTH
        | DDSD_PIXELFORMAT
        | if block { DDSD_LINEARSIZE } else { DDSD_PITCH }
        | if mips > 1 { DDSD_MIPMAPCOUNT } else { 0 };
    hd[2] = h;
    hd[3] = w;
    hd[4] = size;
    hd[5] = 1; // depth: texconv writes 1 for 2D textures; not a reserved field
    hd[6] = mips;
    hd[18] = 32;
    match desc.header {
        ResolvedHeader::Legacy => {
            let no_form = DdsError::NoLegacyForm { format: desc.format, srgb: desc.srgb };
            if desc.srgb {
                return Err(no_form);
            }
            if let Some(cc) = legacy_fourcc(desc.format) {
                hd[19] = DDPF_FOURCC;
                hd[20] = u32::from_le_bytes(cc);
            } else if let Some((flags, bits, m)) = legacy_masks(desc.format) {
                hd[19] = flags;
                hd[21] = bits;
                hd[22..26].copy_from_slice(&m);
            } else {
                return Err(no_form);
            }
        }
        ResolvedHeader::Dx10 => {
            hd[19] = DDPF_FOURCC;
            hd[20] = u32::from_le_bytes(*b"DX10");
        }
    }
    hd[26] = DDSCAPS_TEXTURE | if mips > 1 { DDSCAPS_COMPLEX | DDSCAPS_MIPMAP } else { 0 };
    let mut out = Vec::with_capacity(148);
    out.extend_from_slice(b"DDS ");
    for v in hd {
        out.extend_from_slice(&v.to_le_bytes());
    }
    if desc.header == ResolvedHeader::Dx10 {
        for v in [dxgi_format(desc.format, desc.srgb), DIMENSION_TEXTURE2D, 0, 1, 0] {
            out.extend_from_slice(&v.to_le_bytes());
        }
    }
    Ok(out)
}
```

`dds_bytes` keeps its zero-dimension, level-count and level-size checks, then does `let mut out = header_bytes(desc, levels.len() as u32)?; out.reserve(…); for l in levels { out.extend_from_slice(l) }`. Delete the old inline header code and the old `fourcc` match.

Reader changes:
- In `DdsPixelFormat`, `Bgra8` becomes `Bgra8 { srgb: bool }`. Update its uses: `bytes_per_pixel` (`Bgra8 { .. }`); the legacy mask match (`=> Bgra8 { srgb: false }`); `from_dxgi` (`87 => Bgra8 { srgb: false }, 91 => Bgra8 { srgb: true }`); and `decode.rs` (`Bgra8 { .. } | Bgrx8 =>`).
- `fn from_dxgi` becomes `pub fn from_dxgi`.
- Add the inverse:

```rust
    /// The DXGI number of this pixel format (a legacy header's format maps to its DXGI equivalent).
    pub fn dxgi(self) -> u32 {
        use DdsPixelFormat::*;
        match self {
            Rgba32f => 2,
            Rgba16f => 10,
            Rgba8 { srgb } => if srgb { 29 } else { 28 },
            Rg8 => 49,
            R8 => 61,
            Bc1 { srgb } => if srgb { 72 } else { 71 },
            Bc2 { srgb } => if srgb { 75 } else { 74 },
            Bc3 { srgb } => if srgb { 78 } else { 77 },
            Bc4 => 80,
            Bc5 => 83,
            Bgra8 { srgb } => if srgb { 91 } else { 87 },
            Bgrx8 => 88,
            Bc6hUf16 => 95,
            Bc6hSf16 => 96,
            Bc7 { srgb } => if srgb { 99 } else { 98 },
        }
    }
```

`as_format`: add `P::Bgra8 { srgb } => (Format::Rgba8, srgb), P::R8 => (Format::R8, false), P::Rg8 => (Format::Rg8, false), P::Rgba16f => (Format::Rgba16f, false),`. Update its doc to "`None` for formats sdc never writes (BC2, signed BC6H, RGBA-order RGBA8, BGRX8, RGBA32F)". Update the `check_header` doc: "(which sdc writes since M6)".

- [ ] **Step 5: Implement the encode, quantize and domain changes.**

`ops.rs` `domain_for`: `if p.format.is_hdr() { Domain::Hdr }`.

`quantize.rs` `to_encoder_input`, HDR branch:

```rust
    if p.format.is_hdr() {
        let one = f16::ONE.to_bits();
        // DECISIONS S4: rgba16f stores alpha as a half when it is kept; BC6H has no alpha.
        let keep_alpha = p.format == Format::Rgba16f && work.alpha_present;
        return EncoderInput::Hdr(
            level
                .px
                .iter()
                .map(|q| [half_bits(q[0]), half_bits(q[1]), half_bits(q[2]), if keep_alpha { half_bits(q[3]) } else { one }])
                .collect(),
        );
    }
```

`encode.rs`:

```rust
/// Uncompressed level bytes (DECISIONS S4): `rgba8` as B8G8R8A8, `r8` as R, `rg8` as R then G, `rgba16f` as four
/// little-endian halves per texel. Rows are tightly packed (pitch `w * bytes_per_pixel`).
fn pack_uncompressed(input: &EncoderInput, format: Format) -> Vec<u8> {
    match (input, format) {
        (EncoderInput::Ldr(px), Format::Rgba8) => px.iter().flat_map(|p| [p[2], p[1], p[0], p[3]]).collect(),
        (EncoderInput::Ldr(px), Format::R8) => px.iter().map(|p| p[0]).collect(),
        (EncoderInput::Ldr(px), Format::Rg8) => px.iter().flat_map(|p| [p[0], p[1]]).collect(),
        (EncoderInput::Hdr(px), Format::Rgba16f) => {
            px.iter().flat_map(|p| p.iter().flat_map(|h| h.to_le_bytes())).collect()
        }
        _ => unreachable!("encode_level_with checks the input kind against the format"),
    }
}
```

In `encode_level_with`:
- The assert becomes `matches!(input, EncoderInput::Hdr(_)) == p.format.is_hdr()`.
- Directly after it, add:

```rust
    if !p.format.is_block() {
        let n = match input {
            EncoderInput::Ldr(v) => v.len(),
            EncoderInput::Hdr(v) => v.len(),
        };
        assert_eq!(n, w as usize * h as usize, "pixel count does not match width*height");
        return if cancelled() { Err(Cancelled) } else { Ok(pack_uncompressed(input, p.format)) };
    }
```

- In the `Ldr` arm, use `p.format.block_bytes().expect("block format")`.
- In `encode_band`, use `let bs = f.block_bytes().expect("block format");`, and the last arm becomes `Format::Bc7 | Format::Bc6hUf16 | Format::Rgba8 | Format::R8 | Format::Rg8 | Format::Rgba16f => unreachable!("handled elsewhere")`.
- Update the `encode_level` doc ("HDR input exactly for BC6H and RGBA16F; uncompressed formats are packed, not encoded").

`policy.rs`:
- `backend_fits`: `Backend::Uncompressed => !f.is_block()`, and `Backend::Nvtt`, `Bc7e`, `Rgbcx` and `IspcBc6h` stay as they are.
- `default_backend`: add `Format::Rgba8 | Format::R8 | Format::Rg8 | Format::Rgba16f => Backend::Uncompressed`.

`crates/cli/src/verbs.rs`: `carries_alpha` matches `Bgra8 { .. }`, and the test expects `("Bgra8".into(), Some(false))`.

GUI compile fixes:
- `model/preview.rs` `pixel_format(format, srgb)` body becomes `format.pixel_format(srgb)`.
- `model/fields.rs` `format_label` adds:

```rust
        Format::Rgba8 => format!("RGBA8 · {space} (uncompressed)"),
        Format::R8 => "R8 · linear (one channel, uncompressed)".into(),
        Format::Rg8 => "RG8 · linear (two channels, uncompressed)".into(),
        Format::Rgba16f => "RGBA16F · HDR linear (uncompressed)".into(),
```

- `render/upload.rs`: `pub fn wgpu_format(format: Format) -> Option<wgpu::TextureFormat>` (`Some(…)` for the six BC formats, `None` otherwise), with its callers using `?` or `let Some … else { return None }`. `block_layout` uses `format.block_bytes().unwrap_or(0) as u32`.
- `render/mod.rs` `set_encoded`: `let reason = if r.format.is_block() && can_upload_bc(…) { … } else if !r.format.is_block() { UNCOMPRESSED } else if !self.supports_bc { NO_BC } else { NOT_MULTIPLE_OF_4 };`, with `const UNCOMPRESSED: &str = "uncompressed format: shown from the CPU decode";` next to the other reason constants.

- [ ] **Step 6: Regenerate the schemas and run the gates.** Run `cargo run -p sdc -- presets schema all --out schema`. In `config::tests::schema_lists_enum_values` add the needles `"rgba16f"` and `"uncompressed"`.

- [ ] **Step 7: Commit.**

```bash
git add crates/engine/src/config.rs crates/engine/src/dds.rs crates/engine/src/decode.rs crates/engine/src/encode.rs crates/engine/src/quantize.rs crates/engine/src/ops.rs crates/engine/src/policy.rs crates/engine/tests/dds_write.rs crates/engine/tests/dds_read.rs crates/cli/src/verbs.rs crates/gui/src/model/preview.rs crates/gui/src/model/fields.rs crates/gui/src/render/upload.rs crates/gui/src/render/mod.rs crates/gui/tests/fields.rs schema/
git commit -m "feat(dds): uncompressed rgba8 (B8G8R8A8), r8, rg8 and rgba16f: format model, writer, reader, packing (M6, DECISIONS O1, S4, S5)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 6: Uncompressed outputs: Resolve rules, Validate, goldens, external readers

**Files:**
- Modify: `crates/engine/src/policy.rs` (`check_profile`, `stores_alpha`, `float_source_is_linear`), `crates/engine/src/validate/mod.rs` (the `ch` match)
- Test: `crates/engine/tests/golden.rs`, `crates/engine/tests/policy_checks.rs`, `crates/engine/tests/uncompressed_interop.rs` (create, env-gated)

**Interfaces:**
- Consumes: Task 5.
- Produces: the S4 rules; nothing new by name.

**Expectations that change:** none.

- [ ] **Step 1: Write the failing goldens.** Append to `golden.rs`:

```rust
/// DECISIONS S4: `rgba8` UNORM is a legacy B8G8R8A8 file, lossless (mip 0 equals the source), full chain, 99 dB.
#[test]
fn rgba8_output_is_legacy_b8g8r8a8_and_lossless() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "textures/a/rock.png", rgb(64));
    let o = convert(d.path(), Target::Vanilla, &["file:**/rock.png.subtype=diffuse", "format=rgba8"], &[]);
    assert_eq!((o.summary.converted, o.summary.exit_code()), (1, 0), "{:?}", warnings(&o));
    let f = dds(d.path(), "textures/a/rock.dds");
    assert_dds(&f, DdsPixelFormat::Bgra8 { srgb: false }, HeaderKind::Legacy, 7);
    let src = rgb(64).to_rgb8();
    for (i, p) in levels(&f)[0].iter().enumerate() {
        let s = src.get_pixel(i as u32 % 64, i as u32 / 64).0;
        assert_eq!(&p[..3], &s[..], "texel {i}");
    }
    assert_eq!(validation_of(&o, "rock.png").min_mip_psnr_db, Some(99.0));
    assert_eq!(planned(&o, "rock.png")["profile"]["encoder"]["backend"], "uncompressed");
}

/// A cs PBR albedo as rgba8 is DX10 B8G8R8A8_UNORM_SRGB (91).
#[test]
fn rgba8_srgb_pbr_albedo_is_dx10_91() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "textures/pbr/iron.png", rgb(64));
    let o = convert(d.path(), Target::Cs, &["subtype=pbr_albedo", "format=rgba8"], &[]);
    assert_eq!(o.summary.converted, 1, "{:?}", warnings(&o));
    let f = dds(d.path(), "textures/pbr/iron.dds");
    assert_dds(&f, DdsPixelFormat::Bgra8 { srgb: true }, HeaderKind::Dx10, 7);
    assert_eq!(f.dx10.unwrap().dxgi, 91);
}

/// `r8` and `rg8` are DX10 UNORM and keep R (and G) exactly.
#[test]
fn r8_and_rg8_outputs_are_dx10_and_exact() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "textures/a/hill.png", gray_ramp(64));
    put(d.path(), "textures/a/vec.png", rgb(64));
    let o = convert(
        d.path(),
        Target::Cs,
        &[
            "file:**/hill.png.subtype=height",
            "file:**/hill.png.format=r8",
            "file:**/vec.png.subtype=generic_two_channel",
            "file:**/vec.png.format=rg8",
        ],
        &[],
    );
    assert_eq!(o.summary.converted, 2, "{:?}", warnings(&o));
    let h = dds(d.path(), "textures/a/hill.dds");
    assert_dds(&h, DdsPixelFormat::R8, HeaderKind::Dx10, 7);
    assert_eq!(h.dx10.unwrap().dxgi, 61);
    assert!(levels(&h)[0].iter().enumerate().all(|(i, p)| p[0] == ((i as u32 % 64) * 4) as u8));
    let v = dds(d.path(), "textures/a/vec.dds");
    assert_dds(&v, DdsPixelFormat::Rg8, HeaderKind::Dx10, 7);
    let src = rgb(64).to_rgb8();
    for (i, p) in levels(&v)[0].iter().enumerate() {
        let s = src.get_pixel(i as u32 % 64, i as u32 / 64).0;
        assert_eq!((p[0], p[1]), (s[0], s[1]), "texel {i}");
    }
}

/// `rgba16f` keeps float values (half precision), tag n/a, DXGI 10; it is cs-only (open question 2).
#[test]
fn rgba16f_output_keeps_float_values_and_is_cs_only() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "textures/pbr/lamp_g.tif", hdr_emissive());
    let o = convert(d.path(), Target::Cs, &["file:**/lamp_g.tif.format=rgba16f"], &[]);
    assert_eq!(o.summary.converted, 1, "{:?}", warnings(&o));
    assert_eq!(planned(&o, "lamp_g.tif")["profile"]["color"]["tag"], "n/a");
    let f = dds(d.path(), "textures/pbr/lamp_g.dds");
    assert_dds(&f, DdsPixelFormat::Rgba16f, HeaderKind::Dx10, 7);
    let (w, h, r) = f.levels[0].clone();
    let Decoded::Hdr(px) = decode_level(f.format, w, h, &f.data[r], None).unwrap() else { panic!() };
    let src = hdr_emissive().to_rgb32f();
    for (p, s) in px.iter().zip(src.pixels()) {
        for c in 0..3 {
            assert!((p[c] - s.0[c]).abs() <= 2e-3 * s.0[c].max(1.0), "{} vs {}", p[c], s.0[c]);
        }
    }
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "textures/a/lamp.tif", hdr_emissive());
    let o = convert(d.path(), Target::Vanilla, &["subtype=glow", "format=rgba16f", "color.tag=n/a"], &[]);
    assert_eq!(o.summary.converted, 0);
    let w = warning_of(&o, "lamp.tif", Code::CfgUnavailable);
    assert_eq!(w.severity, Severity::Error);
}
```

In `policy_checks.rs`, using that file's `run(target, path, suffix, set_suffixes, analysis, sets, allow)` and `warning(&j, code)` helpers exactly as `bc6h_and_tag_na_must_agree` uses them:

```rust
/// DECISIONS S4: r8/rg8 have no _SRGB form; on a normal map they block like BC4/BC5.
#[test]
fn r8_and_rg8_are_unorm_only_and_block_on_normals() {
    let j = run(Target::Cs, "textures/pbr/x.png", None, &[], opaque(), &["subtype=pbr_albedo", "format=r8"], &[]);
    assert!(warning(&j, Code::CfgInvalid).message.contains("has no _SRGB form"), "{:?}", j.warnings);
    let j = run(Target::Cs, "textures/a/x_n.png", Some("n"), &["n"], opaque(), &["subtype=normal", "format=rg8", "channels.green_convention=directx"], &[]);
    assert!(j.warnings.iter().any(|w| w.code == Code::FmtBc5Normal), "{:?}", j.warnings);
}
```

Create `crates/engine/tests/uncompressed_interop.rs`:

```rust
//! DECISIONS O1 exit test "texconv and nvddsinfo read every output". Environment-gated (skipped otherwise, so the
//! suite carries no absolute path): `SDC_TEXCONV` = texconv.exe, `SDC_NVDDSINFO` = nvddsinfo.exe. Both only read
//! the tempdir outputs; texconv writes its PNGs into another tempdir.
use engine::api::*;
use engine::config::Target;
use engine::events::*;
use engine::overrides::parse_set;
use engine::policy::AcceptSuggestions;
use image::{DynamicImage, ImageBuffer, Rgb};
use std::sync::Arc;

#[test]
fn texconv_and_nvddsinfo_read_every_uncompressed_output() {
    let tex = std::env::var_os("SDC_TEXCONV");
    let info = std::env::var_os("SDC_NVDDSINFO");
    if tex.is_none() && info.is_none() {
        eprintln!("skipped: set SDC_TEXCONV and/or SDC_NVDDSINFO");
        return;
    }
    let d = tempfile::tempdir().unwrap();
    let src = d.path().join("in/textures/a");
    std::fs::create_dir_all(&src).unwrap();
    let rgb = DynamicImage::ImageRgb8(ImageBuffer::from_fn(64, 64, |x, y| Rgb([(x * 4) as u8, (y * 4) as u8, 90])));
    for n in ["c8.png", "s8.png", "r.png", "rg.png"] {
        rgb.save(src.join(n)).unwrap();
    }
    DynamicImage::ImageRgb32F(ImageBuffer::from_fn(64, 64, |x, _| Rgb([x as f32 / 16.0, 0.5, 0.25])))
        .save(d.path().join("in/textures/a/h_g.tif"))
        .unwrap();
    let mut req = BatchRequest::new(vec![d.path().join("in")], d.path().join("out"), Target::Cs);
    req.recursive = true;
    req.accept = AcceptSuggestions::High;
    req.on_existing = OnExisting::Overwrite;
    req.overrides = [
        "file:**/c8.png.subtype=diffuse", "file:**/c8.png.format=rgba8",
        "file:**/s8.png.subtype=pbr_albedo", "file:**/s8.png.format=rgba8",
        "file:**/r.png.subtype=height", "file:**/r.png.format=r8",
        "file:**/rg.png.subtype=generic_two_channel", "file:**/rg.png.format=rg8",
        "file:**/h_g.tif.subtype=pbr_emissive", "file:**/h_g.tif.format=rgba16f",
    ]
    .iter()
    .map(|s| parse_set(s).unwrap())
    .collect();
    let e = Engine::new().unwrap();
    let rep = Reporter::new(Arc::new(NullSink), "interop");
    let plan = e.plan(req, &rep, &CancelToken::default()).unwrap();
    let s = e.run(plan, &rep, &CancelToken::default()).unwrap();
    assert_eq!(s.converted, 5);
    let pngs = tempfile::tempdir().unwrap();
    for n in ["c8", "s8", "r", "rg", "h_g"] {
        let f = d.path().join(format!("out/textures/a/{n}.dds"));
        if let Some(t) = &tex {
            let o = std::process::Command::new(t)
                .args(["-nologo", "-y", "-ft", "png", "-o"])
                .arg(pngs.path())
                .arg(&f)
                .output()
                .unwrap();
            assert!(o.status.success(), "texconv {n}: {}", String::from_utf8_lossy(&o.stdout));
        }
        if let Some(i) = &info {
            let o = std::process::Command::new(i).arg(&f).output().unwrap();
            let out = String::from_utf8_lossy(&o.stdout);
            assert!(o.status.success() && out.contains("64"), "nvddsinfo {n}: {out}");
        }
    }
}
```

- [ ] **Step 2: Run them and see them fail.** Run `cargo test -p engine --test golden rgba r8 rgba16f` and `cargo test -p engine --test policy_checks r8_`. Expect: `rgba16f` with `color.tag` passes CFG checks wrongly, the sRGB `r8` gets no CFG-INVALID, and no FMT-BC5-NORMAL.

- [ ] **Step 3: Implement the Resolve rules in `check_profile`.**
- `let bc6h = p.format == Format::Bc6hUf16;` (the tag block) becomes `let hdr = p.format.is_hdr();`, and every later use of `bc6h` in `check_profile` becomes `hdr`: the tag rule, `TAG-UNORM-PBR`, the float block, `LOSS-BITDEPTH`.
- The first message becomes `format!("{} has no _SRGB or UNORM form; set color.tag=n/a", if p.format == Format::Bc6hUf16 { "BC6H".into() } else { p.format.id().to_uppercase() })`.
- After the tag rule, add:

```rust
    if matches!(p.format, Format::R8 | Format::Rg8) && p.srgb_tag() {
        add(
            CfgInvalid,
            format!("{} has no _SRGB form; set color.tag=unorm", p.format.id().to_uppercase()),
        );
    }
```

- `bc6h_outside_cs` becomes `let hdr_outside_cs = p.format.is_hdr() && p.target != Target::Cs;`. Its error message: BC6H keeps "BC6H is usable only under Community Shaders [D#3]; choose another format", and `rgba16f` gets "RGBA16F is usable only under Community Shaders (DECISIONS S4); choose another format".
- `FMT-BC5-NORMAL`: `matches!(p.format, Format::Bc4 | Format::Bc5 | Format::R8 | Format::Rg8)`, message "BC4/BC5/R8/RG8 on a normal: Skyrim reads Z directly, so Z decodes -1 and alpha is lost [SC §2]".
- `stores_alpha`: `matches!(f, Format::Bc3 | Format::Bc7 | Format::Rgba8 | Format::Rgba16f)`.
- `float_source_is_linear`: `p.format.is_hdr()` instead of `== Format::Bc6hUf16`.
- `emissive_hdr_choice` (Task 4), the tag rule: `let tag = if p.format.is_hdr() { ColorTag::NotApplicable } else { ColorTag::Srgb };`. Without this, `--set format=rgba16f` on a `_g` gets tag `srgb` and `CFG-INVALID`, and `rgba16f_output_keeps_float_values_and_is_cs_only` converts nothing.
- `hdr_source_notice` (Task 4), the `chosen` match: the first arm becomes `(f, _) if f.is_hdr() => if f == Format::Bc6hUf16 { "BC6H".to_string() } else { "RGBA16F".to_string() },`.

`validate/mod.rs`, the `ch` match:

```rust
        let ch: Vec<usize> = match p.format {
            Format::Bc4 | Format::R8 => vec![0],
            Format::Bc5 | Format::Rg8 => vec![0, 1],
            Format::Bc1 => vec![0, 1, 2],
            _ if alpha_present => vec![0, 1, 2, 3],
            _ => vec![0, 1, 2],
        };
```

- [ ] **Step 4: Run the gates.** Expect all to pass. The interop test prints "skipped".

- [ ] **Step 5: Controller step (not the implementer): the external readers.** Run this once in Git Bash and paste the output into the task report:

```bash
SDC_TEXCONV="F:/ai/dds/bench/tools/dl_texconv/texconv.exe" SDC_NVDDSINFO="C:/Program Files/NVIDIA Corporation/NVIDIA Texture Tools/nvddsinfo.exe" cargo test -p engine --test uncompressed_interop -- --nocapture
```

Expect PASS. A texconv failure on one format is a finding: record it in Note S (Task 12). Do not change the writer silently.

- [ ] **Step 6: Commit.**

```bash
git add crates/engine/src/policy.rs crates/engine/src/validate/mod.rs crates/engine/tests/golden.rs crates/engine/tests/policy_checks.rs crates/engine/tests/uncompressed_interop.rs
git commit -m "feat(policy): uncompressed formats resolve, validate and round-trip; golden case per format (M6, DECISIONS O1, S4)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 7: DDS input decoding, DXGI in `Analysis`, full `validate` of sdc's uncompressed files

**Files:**
- Modify: `crates/engine/src/decode.rs` (`decode_level_rgba`), `crates/engine/src/load.rs` (DDS dispatch, `load_dds`), `crates/engine/src/analyse.rs` (`Analysis.dds_dxgi`), `crates/engine/src/api.rs` (the load site from Task 1)
- Test: `crates/engine/tests/load.rs`, `crates/engine/tests/dds_read.rs`, `crates/cli/tests/cli_inspect_validate.rs`

**Interfaces:**
- Consumes: `DdsPixelFormat::dxgi`, `from_dxgi` (Task 5).
- Produces: `decode_level_rgba`; `load_image`/`load_image_meta` read `.dds`; `Analysis.dds_dxgi`.

**Expectations that change:** none. The CLI already judges `Bgra8`/`R8`/`Rg8`/`Rgba16f` fully through `as_format` (Task 5); this task pins that.

- [ ] **Step 1: Write the failing tests.** Append to `crates/engine/tests/load.rs`:

```rust
/// A one-level DX10 DDS with DXGI format `dxgi` and the given level bytes (tight rows).
fn dx10(w: u32, h: u32, dxgi: u32, pitch: u32, data: &[u8]) -> Vec<u8> {
    let mut hd = [0u32; 31];
    hd[0] = 124;
    hd[1] = 0x1 | 0x2 | 0x4 | 0x8 | 0x1000;
    hd[2] = h;
    hd[3] = w;
    hd[4] = pitch;
    hd[5] = 1;
    hd[6] = 1;
    hd[18] = 32;
    hd[19] = 0x4;
    hd[20] = u32::from_le_bytes(*b"DX10");
    hd[26] = 0x1000;
    let mut b = b"DDS ".to_vec();
    for v in hd {
        b.extend_from_slice(&v.to_le_bytes());
    }
    for v in [dxgi, 3, 0, 1, 0] {
        b.extend_from_slice(&v.to_le_bytes());
    }
    b.extend_from_slice(data);
    b
}

/// Review Focus 2: every uncompressed layout of spec §1 loads mip 0 as float RGBA, with its DXGI in `SourceMeta`.
#[test]
fn third_party_uncompressed_layouts_load_as_float_rgba() {
    let d = tempfile::tempdir().unwrap();
    let half = |v: f32| half::f16::from_f32(v).to_le_bytes();
    let rgba16: Vec<u8> = [2.0f32, 0.5, 0.25, 0.75].iter().flat_map(|v| half(*v)).collect();
    let rgba32: Vec<u8> = [3.0f32, 0.5, 0.25, 0.5].iter().flat_map(|v| v.to_le_bytes()).collect();
    type Case<'a> = (u32, u32, &'a [u8], [f32; 4], (u8, u8, bool, bool));
    let cases: [Case; 7] = [
        (28, 4, &[10, 20, 30, 40], [10.0, 20.0, 30.0, 40.0].map(|v| v / 255.0), (8, 4, true, false)),
        (87, 4, &[30, 20, 10, 40], [10.0, 20.0, 30.0, 40.0].map(|v| v / 255.0), (8, 4, true, false)),
        (88, 4, &[30, 20, 10, 7], [10.0 / 255.0, 20.0 / 255.0, 30.0 / 255.0, 1.0], (8, 3, false, false)),
        (61, 1, &[51], [0.2, 0.2, 0.2, 1.0], (8, 1, false, false)),
        (49, 2, &[51, 102], [0.2, 0.4, 0.0, 1.0], (8, 2, false, false)),
        (10, 8, &rgba16, [2.0, 0.5, 0.25, 0.75], (16, 4, true, true)),
        (2, 16, &rgba32, [3.0, 0.5, 0.25, 0.5], (32, 4, true, true)),
    ];
    for (dxgi, bpp, px, want, (bits, ch, alpha, float)) in cases {
        let p = d.path().join(format!("f{dxgi}.dds"));
        std::fs::write(&p, dx10(1, 1, dxgi, bpp, px)).unwrap();
        let (img, info, meta) = load_image_meta(&p).unwrap_or_else(|e| panic!("{dxgi}: {e}"));
        assert_eq!((info.bit_depth, info.channels, info.has_alpha, info.is_float), (bits, ch, alpha, float), "{dxgi}");
        for c in 0..4 {
            assert!((img.at(0, 0)[c] - want[c]).abs() < 1e-3, "{dxgi} c{c}: {:?}", img.at(0, 0));
        }
        assert_eq!(meta.dds_dxgi, Some(dxgi));
    }
}

#[test]
fn a_cubemap_dds_is_a_decode_error() {
    let d = tempfile::tempdir().unwrap();
    let mut b = dx10(1, 1, 28, 4, &[0; 4 * 6]);
    b[136..140].copy_from_slice(&4u32.to_le_bytes()); // DX10 misc: TEXTURECUBE
    let p = d.path().join("cube.dds");
    std::fs::write(&p, b).unwrap();
    let e = load_image(&p).unwrap_err();
    assert!(e.to_string().contains("cubemap or array"), "{e}");
}
```

(Add `half` to `crates/engine`'s `[dev-dependencies]` only if the test crate cannot see it. `half` is already a normal dependency of `engine`, but integration tests need it in dev-dependencies: add `half.workspace = true` there.)

In `crates/engine/tests/dds_read.rs`:

```rust
#[test]
fn decode_level_rgba_keeps_float_alpha_and_scales_ldr() {
    use engine::decode::decode_level_rgba;
    let h: Vec<u8> = [2.0f32, 0.5, 0.25, 0.75].iter().flat_map(|v| half::f16::from_f32(*v).to_le_bytes()).collect();
    assert_eq!(decode_level_rgba(DdsPixelFormat::Rgba16f, 1, 1, &h, None).unwrap(), vec![[2.0, 0.5, 0.25, 0.75]]);
    assert_eq!(
        decode_level_rgba(DdsPixelFormat::R8, 1, 1, &[51], None).unwrap(),
        vec![[0.2, 0.2, 0.2, 1.0]]
    );
    assert_eq!(decode_level_rgba(DdsPixelFormat::Rgba16f, 2, 2, &h, None).unwrap_err(), DdsError::Truncated);
}

/// Review Focus 2: a padded row pitch is reported by `check_header`, and the level still decodes as stored.
#[test]
fn a_padded_pitch_is_a_header_issue_not_a_panic() {
    let mut hd = [0u32; 31];
    hd[0] = 124; hd[1] = 0x1 | 0x2 | 0x4 | 0x8 | 0x1000; hd[2] = 2; hd[3] = 3; hd[4] = 16; hd[6] = 1;
    hd[18] = 32; hd[19] = 0x40 | 0x1; hd[21] = 32; hd[22] = 0xff_0000; hd[23] = 0xff00; hd[24] = 0xff; hd[25] = 0xff00_0000;
    hd[26] = 0x1000;
    let mut b = b"DDS ".to_vec();
    for v in hd { b.extend_from_slice(&v.to_le_bytes()); }
    b.extend_from_slice(&[7u8; 3 * 2 * 4]);
    let f = read_dds(&b).unwrap();
    assert!(check_header(&f).iter().any(|m| m.contains("pitch 16 != row pitch 12")), "{:?}", check_header(&f));
    assert!(decode_level(f.format, 3, 2, &f.data[f.levels[0].2.clone()], None).is_ok());
}
```

(`dds_read.rs` already imports `engine::dds::*` and `engine::decode::*`. Add `half.workspace = true` to dev-dependencies if needed, as above.)

In `crates/cli/tests/cli_inspect_validate.rs`:

```rust
/// DECISIONS S5: sdc's own uncompressed output is judged fully by `validate`, not header-only.
#[test]
fn validate_judges_an_uncompressed_sdc_output_in_full() {
    let d = tempfile::tempdir().unwrap();
    let src = d.path().join("in");
    std::fs::create_dir_all(&src).unwrap();
    ImageBuffer::from_fn(16, 16, |x, y| Rgb([x as u8 * 16, y as u8 * 16, 128]))
        .save(src.join("wall.png"))
        .unwrap();
    let out = d.path().join("out");
    let o = sdc()
        .args(["convert", "--set", "subtype=diffuse", "--set", "format=rgba8", "-o"])
        .arg(&out)
        .arg(src.join("wall.png"))
        .output()
        .unwrap();
    assert_eq!(code(&o), 0, "{}", text(&o.stderr));
    let o = sdc().args(["validate", "--json"]).arg(out.join("wall.dds")).output().unwrap();
    assert_eq!(code(&o), 0, "{}", text(&o.stderr));
    let v: serde_json::Value = serde_json::from_str(text(&o.stdout).lines().next().unwrap()).unwrap();
    assert_eq!((v["judged"].as_bool(), v["passed"].as_bool()), (Some(true), Some(true)), "{v}");
}
```

(If `serde_json` is not a dev-dependency of `crates/cli`, check whether that test file already parses JSON another way, and follow it.)

- [ ] **Step 2: Run them and see them fail.** Expect: `load_image` treats `.dds` as an unknown image format, and there is no `decode_level_rgba`.

- [ ] **Step 3: Implement.** `decode.rs`:

```rust
/// Decode a level to float RGBA: LDR formats as `code / 255`, BC6H as RGB with alpha 1, RGBA16F/RGBA32F with their
/// own alpha. For the DDS input loader (re-encode, DECISIONS S6) and the GUI preview.
pub fn decode_level_rgba(
    fmt: DdsPixelFormat,
    w: u32,
    h: u32,
    data: &[u8],
    bc1_model: Option<Bc1Model>,
) -> Result<Vec<[f32; 4]>, DdsError> {
    use DdsPixelFormat::*;
    match fmt {
        Rgba16f | Rgba32f => {
            let need = level_len(fmt, w, h, data)?;
            let data = &data[..need];
            Ok(if fmt == Rgba16f {
                data.as_chunks::<8>()
                    .0
                    .iter()
                    .map(|c| [0, 1, 2, 3].map(|k| half::f16::from_le_bytes([c[k * 2], c[k * 2 + 1]]).to_f32()))
                    .collect()
            } else {
                data.as_chunks::<16>()
                    .0
                    .iter()
                    .map(|c| [0, 1, 2, 3].map(|k| f32::from_le_bytes([c[k * 4], c[k * 4 + 1], c[k * 4 + 2], c[k * 4 + 3]])))
                    .collect()
            })
        }
        _ => Ok(match decode_level(fmt, w, h, data, bc1_model)? {
            Decoded::Ldr(v) => v.iter().map(|p| p.map(|c| f32::from(c) / 255.0)).collect(),
            Decoded::Hdr(v) => v.iter().map(|p| [p[0], p[1], p[2], 1.0]).collect(),
        }),
    }
}
```

`load.rs`: at the top of `load_image_meta`:

```rust
    if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("dds"))
    {
        return load_dds(path);
    }
```

and add:

```rust
/// `SourceInfo` of a DDS input's pixel format (DECISIONS S6).
fn dds_source_info(f: crate::dds::DdsPixelFormat) -> SourceInfo {
    use crate::dds::DdsPixelFormat::*;
    let (bit_depth, channels, has_alpha, is_float) = match f {
        Bc1 { .. } | Bc2 { .. } | Bc3 { .. } | Bc7 { .. } | Rgba8 { .. } | Bgra8 { .. } => (8, 4, true, false),
        Bgrx8 => (8, 3, false, false),
        Bc4 | R8 => (8, 1, false, false),
        Bc5 | Rg8 => (8, 2, false, false),
        Bc6hUf16 | Bc6hSf16 => (16, 3, false, true),
        Rgba16f => (16, 4, true, true),
        Rgba32f => (32, 4, true, true),
    };
    SourceInfo { bit_depth, channels, has_alpha, is_float }
}

/// A DDS input (DECISIONS S6): mip 0 decoded to float RGBA; every other mip is regenerated. 2D only.
fn load_dds(path: &Path) -> Result<(Rgba32F, SourceInfo, SourceMeta), LoadError> {
    let bad = |msg: String| LoadError::Decode { path: path.to_path_buf(), msg };
    let bytes = std::fs::read(path).map_err(|e| bad(e.to_string()))?;
    let f = crate::dds::read_dds(&bytes).map_err(|e| bad(e.to_string()))?;
    drop(bytes);
    if f.is_cubemap || f.dx10.is_some_and(|d| d.array_size > 1) {
        return Err(bad("a cubemap or array DDS cannot be re-encoded (2D only)".into()));
    }
    if f.width.max(f.height) > MAX_DIM {
        return Err(LoadError::TooLarge { path: path.to_path_buf(), dim: f.width.max(f.height) });
    }
    let (w, h, r) = f.levels[0].clone();
    let px = crate::decode::decode_level_rgba(f.format, w, h, &f.data[r], None).map_err(|e| bad(e.to_string()))?;
    Ok((
        Rgba32F { width: w, height: h, px },
        dds_source_info(f.format),
        SourceMeta { icc_profile: false, dds_dxgi: Some(f.format.dxgi()) },
    ))
}
```

`analyse.rs`:
- Add the field below:

```rust
    /// DXGI format of a DDS input (DECISIONS S6, spec §5.1); `None` for images. Set by the caller from `SourceMeta`.
    pub dds_dxgi: Option<u32>,
```

- Add `dds_dxgi: None,` in the literal.

`api.rs`: after `a.icc_profile = meta.icc_profile;` add `a.dds_dxgi = meta.dds_dxgi;`.

- [ ] **Step 4: Run the gates.** Expect all to pass.

- [ ] **Step 5: Commit.**

```bash
git add crates/engine/Cargo.toml crates/engine/src/decode.rs crates/engine/src/load.rs crates/engine/src/analyse.rs crates/engine/src/api.rs crates/engine/tests/load.rs crates/engine/tests/dds_read.rs crates/cli/tests/cli_inspect_validate.rs
git commit -m "feat(load): DDS inputs decode mip 0 to float RGBA with their DXGI format in Analysis (M6, DECISIONS S5, S6)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 8: DDS re-encode opt-in (`reencode`, `LOSS-REENCODE`)

**Files:**
- Modify: `crates/engine/src/overrides.rs` (`FIELD_PATHS`, `REENCODE_FIELD`, `parse_set`), `crates/engine/src/presets.rs` (`file_entries`: boolean check), `crates/engine/src/policy.rs` (`glob_match_input`, `reencode_requested`, the override filter in `layered_profile_with`, `LOSS-REENCODE` in `check_profile`), `crates/engine/src/group.rs` (`duplicate_outputs_with`), `crates/engine/src/api.rs` (dups and the skip loop)
- Test: `crates/engine/tests/overrides.rs`, `crates/engine/tests/golden.rs`

**Interfaces:**
- Consumes: Task 7 (`.dds` loads, `Analysis.dds_dxgi`).
- Produces: `REENCODE_FIELD`, `reencode_requested`, `duplicate_outputs_with`.

**Expectations that change:** `gui/tests/fields.rs` asserts every GUI field is in `FIELD_PATHS`; it still holds, since `reencode` is only added. No other.

- [ ] **Step 1: Write the failing tests.** In `crates/engine/tests/overrides.rs` (it imports `engine::overrides::*`):

```rust
#[test]
fn reencode_is_a_boolean_file_or_batch_field_never_subtype_scoped() {
    assert!(parse_set("reencode=true").is_ok());
    assert!(parse_set("file:**/old/*.dds.reencode=true").is_ok());
    let e = parse_set("subtype:diffuse.reencode=true").unwrap_err();
    assert!(e.contains("before the sub-type"), "{e}");
    let e = parse_set("reencode=yes").unwrap_err();
    assert!(e.contains("true or false"), "{e}");
    assert!(FIELD_PATHS.contains(&REENCODE_FIELD));
}
```

In `golden.rs`:

```rust
/// Convert `png` (64 px rgb) with `sets` into a fresh tempdir and return the written DDS bytes.
fn converted_bytes(target: Target, sets: &[&str]) -> Vec<u8> {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "textures/a/rock.png", rgb(64));
    let o = convert(d.path(), target, sets, &[]);
    assert_eq!(o.summary.converted, 1, "{:?}", warnings(&o));
    std::fs::read(d.path().join("out/textures/a/rock.dds")).unwrap()
}

/// DECISIONS S6: a compressed DDS opted in with `reencode` converts, with LOSS-REENCODE (warning).
#[test]
fn an_opted_in_bc7_dds_is_reencoded_with_loss_reencode() {
    let bc7 = converted_bytes(Target::Vanilla, &["subtype=diffuse"]);
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(d.path().join("in/textures/old")).unwrap();
    std::fs::write(d.path().join("in/textures/old/rock.dds"), &bc7).unwrap();
    let o = convert(
        d.path(),
        Target::Vanilla,
        &["file:**/rock.dds.reencode=true", "file:**/rock.dds.subtype=diffuse"],
        &[],
    );
    assert_eq!((o.summary.converted, o.summary.exit_code()), (1, 0), "{:?}", warnings(&o));
    let w = warning_of(&o, "rock.dds", Code::LossReencode);
    assert!(w.severity == Severity::Warning && w.message.contains("BC7"), "{}", w.message);
    assert_dds(&dds(d.path(), "textures/old/rock.dds"), DdsPixelFormat::Bc7 { srgb: false }, HeaderKind::Dx10, 7);
}

/// O1 exit test "a re-encode round trip": an uncompressed rgba8 DDS re-encodes to the same BC7 bytes as its PNG
/// source, and to the same rgba8 mip 0; no LOSS-REENCODE (nothing was compressed).
#[test]
fn an_uncompressed_dds_round_trips_through_reencode() {
    let rgba8 = converted_bytes(Target::Vanilla, &["subtype=diffuse", "format=rgba8"]);
    let direct_bc7 = converted_bytes(Target::Vanilla, &["subtype=diffuse"]);
    for (fmt, want) in [("bc7", direct_bc7.clone()), ("rgba8", rgba8.clone())] {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("in/textures/a")).unwrap();
        std::fs::write(d.path().join("in/textures/a/rock.dds"), &rgba8).unwrap();
        let o = convert(
            d.path(),
            Target::Vanilla,
            &["reencode=true", "subtype=diffuse", &format!("format={fmt}")],
            &[],
        );
        assert_eq!(o.summary.converted, 1, "{fmt}: {:?}", warnings(&o));
        assert!(!warnings(&o).contains(&Code::LossReencode), "{fmt}");
        let got = std::fs::read(d.path().join("out/textures/a/rock.dds")).unwrap();
        assert_eq!(got, want, "{fmt}: byte-identical to the direct conversion");
    }
}

/// Review Focus 1: output root == input folder would overwrite the source: the file fails, the source is untouched.
#[test]
fn reencode_into_its_own_folder_fails_and_leaves_the_source_alone() {
    let rgba8 = converted_bytes(Target::Vanilla, &["subtype=diffuse", "format=rgba8"]);
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("mod");
    std::fs::create_dir_all(root.join("textures")).unwrap();
    let src = root.join("textures/rock.dds");
    std::fs::write(&src, &rgba8).unwrap();
    // An explicit file input: a folder walk skips everything under the output root, so only an explicit file can
    // map onto itself (`discover::make` gives `output == path`).
    let mut req = BatchRequest::new(vec![src.clone()], root.join("textures"), Target::Vanilla);
    req.overrides = ["reencode=true", "subtype=diffuse"].iter().map(|s| parse_set(s).unwrap()).collect();
    req.on_existing = OnExisting::Overwrite;
    let o = run(req);
    assert_eq!((o.summary.converted, o.summary.failed, o.summary.exit_code()), (0, 1, 1));
    assert_eq!(std::fs::read(&src).unwrap(), rgba8);
}

/// Review Focus 1: an opted-in DDS and a same-stem image map to one output: both are IN-DUPLICATE.
#[test]
fn an_opted_in_dds_and_a_same_stem_image_are_duplicates() {
    let rgba8 = converted_bytes(Target::Vanilla, &["subtype=diffuse", "format=rgba8"]);
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "textures/a/rock.png", rgb(64));
    std::fs::write(d.path().join("in/textures/a/rock.dds"), &rgba8).unwrap();
    let o = convert(d.path(), Target::Vanilla, &["reencode=true", "subtype=diffuse"], &[]);
    assert_eq!(skip_code(&o, "rock.png"), Some(Code::InDuplicate));
    assert_eq!(skip_code(&o, "rock.dds"), Some(Code::InDuplicate));
    // Without the opt-in the DDS takes no part, as before.
    let o = convert(d.path(), Target::Vanilla, &["subtype=diffuse"], &[]);
    assert_eq!(skip_code(&o, "rock.dds"), Some(Code::InDdsSkipped));
    assert_eq!(o.summary.converted, 1);
}
```

- [ ] **Step 2: Run them and see them fail.** Expect: `parse_set("reencode=true")` is "unknown field", and the DDS is skipped.

- [ ] **Step 3: Implement.** `overrides.rs`:
- Add `"reencode",` as the second entry of `FIELD_PATHS`, and update its doc to "…plus `reencode`, a plan-time selector (DECISIONS S6)".
- Add `pub const REENCODE_FIELD: &str = "reencode";`.
- In `parse_set`, inside the `subtype:` branch after the `field == "subtype"` check:

```rust
        if field == REENCODE_FIELD {
            return Err(format!(
                "--set '{s}': reencode is decided per file, before the sub-type: use reencode=true or file:<glob>.reencode=true"
            ));
        }
```

- Before `Ok(SetOverride { … })`:

```rust
    if field == REENCODE_FIELD && !value.is_boolean() {
        return Err(format!("--set '{s}': reencode takes true or false"));
    }
```

`presets.rs` `file_entries`: after the unknown-field check, add `if field == crate::overrides::REENCODE_FIELD && !value.is_boolean() { return Err(bad("reencode takes true or false".into())); }`. Compute `value` first; reorder the two statements if needed.

`policy.rs`:
- Split `glob_match` into `fn glob_match_input(glob: &str, input: &InputFile) -> bool` (the current body, with `f.input` replaced by `input`) and `fn glob_match(glob: &str, f: &FileFacts) -> bool { glob_match_input(glob, f.input) }`.
- Add:

```rust
/// DECISIONS S6: whether a DDS input is re-encoded. The highest-precedence `reencode` override that reaches the
/// file decides: `[files]`, then unscoped, file-glob and path `--set` (a sub-type scope is refused at parse time).
pub fn reencode_requested(ctx: &ResolveCtx, input: &InputFile) -> bool {
    ranked_overrides(ctx)
        .into_iter()
        .rev()
        .filter(|o| o.field == crate::overrides::REENCODE_FIELD)
        .find(|o| match &o.scope {
            Scope::All => true,
            Scope::File(g) => glob_match_input(g, input),
            Scope::Path(k) => ctx.files.rel_of(input).is_some_and(|rel| path_key_matches(k, &rel)),
            Scope::Subtype(_) => false,
        })
        .is_some_and(|o| o.value == Value::Bool(true))
}
```

- In `layered_profile_with`, change the override filter to `.filter(|o| o.field != "subtype" && o.field != crate::overrides::REENCODE_FIELD)`.
- In `check_profile`, after the `IN-ICC-IGNORED` block:

```rust
    // DECISIONS S6: a re-encoded block-compressed DDS keeps its compression error and gains a second one [SC §7].
    if let Some(src) = a.dds_dxgi.and_then(crate::dds::from_dxgi).filter(|f| f.block_bytes().is_some()) {
        let name = match src.as_format() {
            Some((fmt, srgb)) => format!("{}{}", fmt.id().to_uppercase(), if srgb { "_SRGB" } else { "" }),
            None => format!("{src:?}"),
        };
        add(
            LossReencode,
            format!("compressed DDS ({name}) re-encoded: its compression error is kept and a new one is added; convert the source image if you have it"),
        );
    }
```

`group.rs`:

```rust
/// Groups of two or more inputs that map to one output path, compared case-insensitively, among the inputs
/// `include` accepts. A DDS takes part only when it is re-encoded (DECISIONS S6).
pub fn duplicate_outputs_with(files: &[InputFile], include: impl Fn(&InputFile) -> bool) -> Vec<Vec<usize>> {
    let mut outs: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, f) in files.iter().enumerate() {
        if include(f) {
            outs.entry(fold(&f.output)).or_default().push(i);
        }
    }
    outs.into_values().filter(|v| v.len() > 1).collect()
}

/// `duplicate_outputs_with` over every image input (DDS inputs are skipped, not written).
pub fn duplicate_outputs(files: &[InputFile]) -> Vec<Vec<usize>> {
    duplicate_outputs_with(files, |f| f.kind != InputKind::Dds)
}
```

`api.rs`:
- Replace `let dups = crate::group::duplicate_outputs(&files);` with:

```rust
        let reencode = |f: &InputFile| f.kind == InputKind::Dds && crate::policy::reencode_requested(&ctx, f);
        let dups = crate::group::duplicate_outputs_with(&files, |f| f.kind != InputKind::Dds || reencode(f));
```

- Before the skip loop, add `let mut self_overwrite: Vec<FailedFile> = vec![];`.
- In the loop, replace the DDS block with:

```rust
                if files[i].kind == InputKind::Dds {
                    if !reencode(&files[i]) {
                        skipped.push(SkippedFile {
                            path: files[i].path.clone(),
                            output: files[i].output.clone(),
                            set: Some(set.key.clone()),
                            code: Some(Code::InDdsSkipped),
                            reason: "DDS inputs are not re-encoded by default".into(),
                        });
                        continue;
                    }
                    // DECISIONS S6: never write over the source DDS (output root == input folder).
                    let same = |p: &Path| crate::overrides::absolute_or_same(p).to_string_lossy().to_lowercase();
                    if same(&files[i].path) == same(&files[i].output) {
                        self_overwrite.push(FailedFile {
                            path: files[i].path.clone(),
                            output: files[i].output.clone(),
                            stage: Stage::Load,
                            error: "re-encoding would overwrite the source DDS: choose another output folder".into(),
                        });
                        continue;
                    }
                }
                work.push((set, &files[i]));
```

- After `let mut failed = vec![];`, add `failed.extend(self_overwrite);`.

`absolute_or_same` is `pub(crate)` in `overrides.rs`, which is enough. Check that `Engine::run` emits a `file_failed` event and counts `summary.failed` for every `plan.failed` entry. If a plan-time failure is only emitted as a Warning at plan time, emit the `FileFailed` event the same way the load-failure branch does.

- [ ] **Step 4: Run the gates.** Expect all to pass, including `golden::a_stray_dds_input_is_skipped_on_purpose_and_exits_zero` and `api::a_dds_next_to_a_same_stem_image_is_skipped_and_the_image_still_plans` unchanged.

- [ ] **Step 5: Commit.**

```bash
git add crates/engine/src/overrides.rs crates/engine/src/presets.rs crates/engine/src/policy.rs crates/engine/src/group.rs crates/engine/src/api.rs crates/engine/tests/overrides.rs crates/engine/tests/golden.rs
git commit -m "feat(plan): DDS re-encode opt-in (reencode=true), LOSS-REENCODE, duplicate and self-overwrite guards (M6, DECISIONS S6)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 9: DDS header patch, engine

**Files:**
- Create: `crates/engine/src/dds_patch.rs`
- Modify: `crates/engine/src/lib.rs` (`pub mod dds_patch;`), `crates/engine/src/codes.rs`
- Regenerate: `schema/*.json`
- Test: `crates/engine/tests/dds_patch.rs` (create)

**Interfaces:**
- Consumes: `dds::{header_bytes, read_dds, check_header, checked_level_size, full_chain_len, dxgi_format, resolve_header, DdsDesc, ResolvedHeader, HeaderKind}`, `decode::decode_level`.
- Produces: `PatchRequest`, `HeaderPatch`, `Unpatchable`, `plan_patch`, `Code::DdsHeaderPatched`, `Code::DdsHeaderUnpatchable`. Code count 67.

**Expectations that change:** `codes` count 65 → 67.

- [ ] **Step 1: Write the failing tests.** Create `crates/engine/tests/dds_patch.rs`:

```rust
//! M6 Task 9 (DECISIONS S7): header-only fixes, payload byte-identical; unpatchable faults are refused with a reason.
use engine::codes::{Code, Severity};
use engine::config::{Format, HeaderForm};
use engine::dds::*;
use engine::dds_patch::*;

fn put_u32(b: &mut [u8], off: usize, v: u32) {
    b[off..off + 4].copy_from_slice(&v.to_le_bytes());
}
fn u(b: &[u8], off: usize) -> u32 {
    u32::from_le_bytes(b[off..off + 4].try_into().unwrap())
}

/// A valid full-chain 64x64 file; level `i` is filled with byte `0x10 + i`.
fn good(format: Format, srgb: bool, header: ResolvedHeader) -> Vec<u8> {
    let mut levels = vec![];
    let (mut w, mut h) = (64u32, 64u32);
    loop {
        levels.push(vec![0x10 + levels.len() as u8; level_size(format, w, h)]);
        if w == 1 && h == 1 {
            break;
        }
        (w, h) = ((w / 2).max(1), (h / 2).max(1));
    }
    dds_bytes(&DdsDesc { width: 64, height: 64, format, srgb, header }, &levels).unwrap()
}

/// Mip count 1, no MIPMAPCOUNT flag, no COMPLEX|MIPMAP cap: the common "mips not declared" fault.
fn undeclared_mips(mut b: Vec<u8>) -> Vec<u8> {
    put_u32(&mut b, 28, 1);
    let flags = u(&b, 8) & !DDSD_MIPMAPCOUNT;
    put_u32(&mut b, 8, flags);
    put_u32(&mut b, 108, DDSCAPS_TEXTURE);
    b
}

fn patched(b: &[u8], req: &PatchRequest) -> HeaderPatch {
    plan_patch(b, req).unwrap().expect("a patch")
}

#[test]
fn a_wrong_mip_count_is_patched_and_the_payload_is_byte_identical() {
    let orig = undeclared_mips(good(Format::Bc7, false, ResolvedHeader::Dx10));
    let p = patched(&orig, &PatchRequest::default());
    assert_eq!((p.old_header_len, p.new_header_len), (148, 148));
    assert_eq!(&p.bytes[148..], &orig[148..], "payload byte-identical");
    assert!(p.fixes.iter().any(|f| f.contains("mip count 1 → 7")), "{:?}", p.fixes);
    let f = read_dds(&p.bytes).unwrap();
    assert_eq!(f.mip_count, 7);
    assert!(check_header(&f).is_empty(), "{:?}", check_header(&f));
    let w = p.warning();
    assert_eq!((w.code, w.severity), (Code::DdsHeaderPatched, Severity::Info));
}

#[test]
fn a_wrong_srgb_tag_is_swapped_only_on_request() {
    let orig = good(Format::Bc7, false, ResolvedHeader::Dx10);
    assert!(plan_patch(&orig, &PatchRequest::default()).unwrap().is_none(), "no guess");
    let p = patched(&orig, &PatchRequest { srgb: Some(true), ..Default::default() });
    assert_eq!(u(&p.bytes, 128), 99);
    assert_eq!(&p.bytes[148..], &orig[148..]);
    assert!(p.fixes.iter().any(|f| f.contains("UNORM → sRGB")), "{:?}", p.fixes);
}

#[test]
fn a_legacy_dxt1_that_needs_srgb_becomes_dx10() {
    let orig = good(Format::Bc1, false, resolve_header(Format::Bc1, false, HeaderForm::Auto));
    assert_eq!(&orig[84..88], b"DXT1");
    let p = patched(&orig, &PatchRequest { srgb: Some(true), ..Default::default() });
    assert_eq!((p.old_header_len, p.new_header_len), (128, 148));
    assert_eq!(&p.bytes[148..], &orig[128..]);
    assert_eq!(read_dds(&p.bytes).unwrap().format, DdsPixelFormat::Bc1 { srgb: true });
}

#[test]
fn ati2_becomes_dx10_bc5() {
    let mut orig = good(Format::Bc5, false, ResolvedHeader::Legacy);
    orig[84..88].copy_from_slice(b"ATI2");
    let p = patched(&orig, &PatchRequest::default());
    assert_eq!(u(&p.bytes, 128), 83);
    assert_eq!(&p.bytes[148..], &orig[128..]);
}

#[test]
fn missing_linear_size_reserved_and_normal_flag_are_fixed() {
    let mut orig = good(Format::Bc7, false, ResolvedHeader::Dx10);
    put_u32(&mut orig, 20, 0);
    orig[40..44].copy_from_slice(b"NVTT");
    let pf = u(&orig, 80) | DDPF_NORMAL;
    put_u32(&mut orig, 80, pf);
    let p = patched(&orig, &PatchRequest::default());
    assert!(check_header(&read_dds(&p.bytes).unwrap()).is_empty());
    assert_eq!(&p.bytes[148..], &orig[148..]);
}

#[test]
fn a_valid_file_needs_no_patch() {
    for b in [good(Format::Bc7, true, ResolvedHeader::Dx10), good(Format::Rgba8, false, ResolvedHeader::Legacy)] {
        assert!(plan_patch(&b, &PatchRequest::default()).unwrap().is_none());
    }
}

/// With trailing bytes no level count fills the payload exactly, so the header's own count (7, it fits) is kept;
/// the reserved-field fault is fixed and the 7 trailing bytes are copied.
#[test]
fn trailing_bytes_after_the_chain_are_kept() {
    let mut orig = good(Format::Bc7, false, ResolvedHeader::Dx10);
    orig[40..44].copy_from_slice(b"NVTT");
    orig.extend_from_slice(&[0xAB; 7]);
    let p = patched(&orig, &PatchRequest::default());
    assert_eq!(&p.bytes[148..], &orig[148..]);
    assert!(p.bytes.ends_with(&[0xAB; 7]));
    assert_eq!(read_dds(&p.bytes).unwrap().mip_count, 7);
    assert!(p.fixes.iter().any(|f| f.contains("reserved")), "{:?}", p.fixes);
}

fn refused(b: &[u8], req: &PatchRequest) -> String {
    let e = plan_patch(b, req).unwrap_err();
    assert_eq!(e.warning().code, Code::DdsHeaderUnpatchable);
    e.0
}

#[test]
fn a_short_payload_is_refused() {
    let mut b = good(Format::Bc7, false, ResolvedHeader::Dx10);
    b.truncate(b.len() - 5);
    assert!(refused(&b, &PatchRequest::default()).contains("no whole mip chain"));
}

#[test]
fn unpatchable_faults_are_refused_with_a_reason() {
    let bc4 = good(Format::Bc4, false, ResolvedHeader::Dx10);
    assert!(refused(&bc4, &PatchRequest { srgb: Some(true), ..Default::default() }).contains("no _SRGB form"));
    let bc7 = good(Format::Bc7, false, ResolvedHeader::Dx10);
    assert!(refused(&bc7, &PatchRequest { expect_format: Some(Format::Bc3), ..Default::default() })
        .contains("wrong block format"));
    // One level stored, a full chain wanted.
    let one = dds_bytes(
        &DdsDesc { width: 64, height: 64, format: Format::Bc7, srgb: false, header: ResolvedHeader::Dx10 },
        &[vec![0; level_size(Format::Bc7, 64, 64)]],
    )
    .unwrap();
    assert!(refused(&one, &PatchRequest { require_full_mips: true, ..Default::default() }).contains("missing mips"));
    // BC1 in 3-colour mode with index 3 everywhere: transparent texels.
    let mut bc1 = good(Format::Bc1, false, ResolvedHeader::Dx10);
    for blk in bc1[148..].chunks_mut(8) {
        blk.copy_from_slice(&[0, 0, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff]);
    }
    assert!(refused(&bc1, &PatchRequest::default()).contains("BC1 with 1-bit alpha"));
    // A cubemap and garbage.
    let mut cube = good(Format::Bc7, false, ResolvedHeader::Dx10);
    put_u32(&mut cube, 136, 4);
    let n = cube.len() - 148;
    cube.extend(std::iter::repeat_n(0u8, n * 5));
    assert!(refused(&cube, &PatchRequest::default()).contains("cubemap"));
    assert!(refused(b"not a dds at all", &PatchRequest::default()).contains("unreadable"));
}
```

In `codes.rs`:
- Add `DdsHeaderPatched = "DDS-HEADER-PATCHED" => Info,` and `DdsHeaderUnpatchable = "DDS-HEADER-UNPATCHABLE" => Warning,` after `SkinWetForm`.
- Add both to the module doc.
- Count 65 → 67.

- [ ] **Step 2: Run them and see them fail.** Run `cargo test -p engine --test dds_patch`. Expect: `dds_patch` is unresolved.

- [ ] **Step 3: Implement `crates/engine/src/dds_patch.rs`.**

```rust
//! DDS header patch (DECISIONS S7, owner idea 2026-10-11): rebuild a DDS header that is wrong where no pixel needs to
//! change, and copy everything after the old header byte for byte. Faults that need re-encoding are refused.
use crate::codes::{Code, Warning};
use crate::config::Format;
use crate::dds::*;
use crate::decode::{decode_level, Decoded};
use serde_json::json;

/// What the caller states about the file. `Default` asks for structural fixes only.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PatchRequest {
    /// Tag to write (`Some(true)` = `_SRGB`); `None` keeps the file's. Only an explicit choice sets it, never a guess.
    pub srgb: Option<bool>,
    /// The block format the file must be (`sdc patch-header --format`); another one is refused.
    pub expect_format: Option<Format>,
    /// The profile wants a full mip chain: fewer stored levels are refused ("missing mips").
    pub require_full_mips: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HeaderPatch {
    /// One line per fix, e.g. "mip count 1 → 7".
    pub fixes: Vec<String>,
    /// The patched file: the new header, then `old bytes[old_header_len..]` unchanged.
    pub bytes: Vec<u8>,
    pub old_header_len: usize,
    pub new_header_len: usize,
}

/// Why a file cannot be fixed by its header alone.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct Unpatchable(pub String);

impl HeaderPatch {
    pub fn warning(&self) -> Warning {
        Warning::new(
            Code::DdsHeaderPatched,
            format!("header patched, pixels copied byte for byte: {}", self.fixes.join("; ")),
        )
        .with_data(json!({ "fixes": self.fixes }))
    }
}

impl Unpatchable {
    pub fn warning(&self) -> Warning {
        Warning::new(Code::DdsHeaderUnpatchable, format!("not patched: {}", self.0))
    }
}

fn tag_name(srgb: bool) -> &'static str {
    if srgb { "sRGB" } else { "UNORM" }
}

/// Plan a header-only fix of `bytes`. `Ok(None)`: the header is already what sdc would write.
///
/// Memory: the file is copied twice while it is parsed (the reader keeps its own copy of the payload).
pub fn plan_patch(bytes: &[u8], req: &PatchRequest) -> Result<Option<HeaderPatch>, Unpatchable> {
    let refuse = |m: String| Err(Unpatchable(m));
    if bytes.len() < 128 || &bytes[..4] != b"DDS " {
        return refuse("unreadable: not a DDS file".into());
    }
    let header_count = u32::from_le_bytes(bytes[28..32].try_into().unwrap()).max(1);
    // Parse with the mip count forced to 1, so a count the payload cannot hold is diagnosed here, not by the reader.
    let mut probe = bytes.to_vec();
    probe[28..32].copy_from_slice(&1u32.to_le_bytes());
    let f = read_dds(&probe).map_err(|e| Unpatchable(format!("unreadable: {e}")))?;
    drop(probe);
    if f.is_cubemap || f.dx10.is_some_and(|d| d.array_size > 1) {
        return refuse("a cubemap or array: sdc patches 2D textures only".into());
    }
    let Some((format, srgb_now)) = f.format.as_format() else {
        return refuse(format!("{:?} is not a format sdc writes: it can be validated but not patched", f.format));
    };
    let fmt_name = format.id().to_uppercase();
    let srgb = req.srgb.unwrap_or(srgb_now);
    if srgb && dxgi_format(format, true) == dxgi_format(format, false) {
        return refuse(format!("{fmt_name} has no _SRGB form: it needs re-encoding as another format"));
    }
    if let Some(e) = req.expect_format.filter(|e| *e != format) {
        return refuse(format!(
            "wrong block format: the file is {fmt_name}, {} expected: it needs re-encoding",
            e.id().to_uppercase()
        ));
    }
    let full = full_chain_len(f.width, f.height);
    let payload = f.data.len();
    let cum = |n: usize| -> Option<usize> {
        (0..n).try_fold(0usize, |s, i| {
            s.checked_add(checked_level_size(format, (f.width >> i).max(1), (f.height >> i).max(1))?)
        })
    };
    // The level count that exactly fills the payload; else the header's count when it fits (trailing bytes kept).
    let stored = (1..=full).find(|&n| cum(n) == Some(payload)).or_else(|| {
        let n = (header_count as usize).min(full);
        cum(n).is_some_and(|c| c <= payload).then_some(n)
    });
    let Some(stored) = stored else {
        return refuse(format!(
            "the payload ({payload} bytes) holds no whole mip chain the header could describe: it needs re-encoding"
        ));
    };
    if req.require_full_mips && stored < full {
        return refuse(format!("missing mips: {stored} of {full} levels stored: it needs re-encoding"));
    }
    if format == Format::Bc1 {
        let (w, h, r) = f.levels[0].clone();
        if let Ok(Decoded::Ldr(px)) = decode_level(f.format, w, h, &f.data[r], None) {
            if px.iter().any(|p| p[3] < 255) {
                return refuse("BC1 with 1-bit alpha: sdc writes BC1 opaque only; re-encode as BC7 or BC3".into());
            }
        }
    }
    let ati = matches!(&f.fourcc, b"ATI1" | b"ATI2");
    let keep_legacy = f.header_kind == HeaderKind::Legacy
        && !ati
        && resolve_header(format, srgb, crate::config::HeaderForm::Legacy) == ResolvedHeader::Legacy;
    let desc = DdsDesc {
        width: f.width,
        height: f.height,
        format,
        srgb,
        header: if keep_legacy { ResolvedHeader::Legacy } else { ResolvedHeader::Dx10 },
    };
    let header = header_bytes(&desc, stored as u32).map_err(|e| Unpatchable(e.to_string()))?;
    let old_header_len = bytes.len() - payload;
    if bytes[..old_header_len] == header[..] {
        return Ok(None);
    }
    let mut out = Vec::with_capacity(header.len() + payload);
    out.extend_from_slice(&header);
    out.extend_from_slice(&bytes[old_header_len..]);
    let new = read_dds(&out).map_err(|e| Unpatchable(format!("internal: the patched file does not read: {e}")))?;
    let issues = check_header(&new);
    if !issues.is_empty() {
        return refuse(format!("the header is still invalid after the patch: {}", issues.join("; ")));
    }
    let mut fixes = vec![];
    if header_count as usize != stored {
        fixes.push(format!("mip count {header_count} → {stored}"));
    }
    if f.flags != new.flags {
        fixes.push(format!("flags {:#x} → {:#x}", f.flags, new.flags));
    }
    if f.caps != new.caps {
        fixes.push(format!("caps {:#x} → {:#x}", f.caps, new.caps));
    }
    if f.linear_size != new.linear_size {
        fixes.push(format!("linear size / pitch {} → {}", f.linear_size, new.linear_size));
    }
    if f.reserved_nonzero {
        fixes.push("reserved fields cleared".into());
    }
    if f.pf_flags & DDPF_NORMAL != 0 {
        fixes.push("non-standard DDPF_NORMAL cleared".into());
    }
    if f.header_kind != new.header_kind || ati {
        fixes.push(format!(
            "{} header ({}) → DX10 {fmt_name}",
            if f.header_kind == HeaderKind::Legacy { "legacy" } else { "DX10" },
            String::from_utf8_lossy(&f.fourcc)
        ));
    } else if let (Some(a), Some(b)) = (f.dx10, new.dx10) {
        if (a.dimension, a.misc, a.array_size, a.misc2) != (b.dimension, b.misc, b.array_size, b.misc2) {
            fixes.push("DX10 dimension/array/misc fields set to a 2D texture".into());
        }
    }
    if srgb != srgb_now {
        fixes.push(format!("tag {fmt_name} {} → {}", tag_name(srgb_now), tag_name(srgb)));
    }
    if fixes.is_empty() {
        fixes.push("other header fields normalised (depth, pixel-format size)".into());
    }
    Ok(Some(HeaderPatch { fixes, bytes: out, old_header_len, new_header_len: header.len() }))
}
```

In `lib.rs` add `pub mod dds_patch;` next to `pub mod dds;`.

Do not change anything else to make a test pass. If the tag-swap test fails because the fix line reads "tag BC7 UNORM → sRGB", that already contains "UNORM → sRGB" and passes.

- [ ] **Step 4: Run the gates, regenerate the schemas, commit.**

```bash
cargo run -p sdc -- presets schema all --out schema
git add crates/engine/src/dds_patch.rs crates/engine/src/lib.rs crates/engine/src/codes.rs crates/engine/tests/dds_patch.rs schema/
git commit -m "feat(dds): header-only patch with a byte-identical payload; DDS-HEADER-PATCHED / -UNPATCHABLE (M6, DECISIONS S7)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 10: `sdc patch-header` verb

**Files:**
- Modify: `crates/cli/src/args.rs` (`Command::PatchHeader`, `PatchHeaderArgs`, `TagArg`), `crates/cli/src/main.rs` (dispatch), `crates/cli/src/verbs.rs` (`patch_header`)
- Test: `crates/cli/tests/cli_patch_header.rs` (create)

**Interfaces:**
- Consumes: Task 9's `plan_patch`, `PatchRequest`; `write_atomic`; the preset registry (`Engine::new()?.registry()`) and `availability`, as `validate_files` uses them.
- Produces: `sdc patch-header`.

**Expectations that change:** none.

- [ ] **Step 1: Write the failing tests.** Create `crates/cli/tests/cli_patch_header.rs`:

```rust
//! M6 Task 10 (DECISIONS S7): `sdc patch-header`.
use image::{ImageBuffer, Rgb};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn sdc() -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_sdc"));
    c.env("APPDATA", std::env::temp_dir().join("sdc-test-appdata-none"));
    c
}
fn code(o: &Output) -> i32 {
    o.status.code().unwrap()
}
fn text(b: &[u8]) -> String {
    String::from_utf8(b.to_vec()).unwrap()
}

/// `<root>/wall.dds`, a BC7 UNORM diffuse with 7 mips whose header lacks the MIPMAPCOUNT flag and the
/// COMPLEX|MIPMAP caps (`check_header` flags both, so `sdc validate` fails it; `validate_files` passes no
/// `expected_levels`, so a wrong *count* alone would not fail it).
fn broken(root: &Path) -> PathBuf {
    let src = root.join("src");
    std::fs::create_dir_all(&src).unwrap();
    ImageBuffer::from_fn(64, 64, |x, y| Rgb([x as u8 * 4, y as u8 * 4, 90]))
        .save(src.join("wall.png"))
        .unwrap();
    let out = root.join("conv");
    let o = sdc()
        .args(["convert", "--target", "vanilla", "--set", "subtype=diffuse", "-o"])
        .arg(&out)
        .arg(src.join("wall.png"))
        .output()
        .unwrap();
    assert_eq!(code(&o), 0, "{}", text(&o.stderr));
    let mut b = std::fs::read(out.join("wall.dds")).unwrap();
    let flags = u32::from_le_bytes(b[8..12].try_into().unwrap()) & !0x2_0000; // DDSD_MIPMAPCOUNT
    b[8..12].copy_from_slice(&flags.to_le_bytes());
    b[108..112].copy_from_slice(&0x1000u32.to_le_bytes()); // DDSCAPS_TEXTURE only
    let p = root.join("wall.dds");
    std::fs::write(&p, b).unwrap();
    p
}

#[test]
fn out_dir_gets_a_patched_copy_that_validates_and_the_source_is_untouched() {
    let d = tempfile::tempdir().unwrap();
    let p = broken(d.path());
    let before = std::fs::read(&p).unwrap();
    let o = sdc().args(["validate"]).arg(&p).output().unwrap();
    assert_eq!(code(&o), 4, "broken first: {}", text(&o.stdout));
    let fixed = d.path().join("fixed");
    let o = sdc().args(["patch-header", "--out-dir"]).arg(&fixed).arg(&p).output().unwrap();
    assert_eq!(code(&o), 0, "{}{}", text(&o.stdout), text(&o.stderr));
    assert!(text(&o.stdout).contains("DDS-HEADER-PATCHED"), "{}", text(&o.stdout));
    let after = std::fs::read(fixed.join("wall.dds")).unwrap();
    assert_eq!(&after[148..], &before[148..], "payload byte-identical");
    assert_eq!(std::fs::read(&p).unwrap(), before, "source untouched");
    let o = sdc().args(["validate"]).arg(fixed.join("wall.dds")).output().unwrap();
    assert_eq!(code(&o), 0, "{}", text(&o.stdout));
}

#[test]
fn in_place_and_dry_run() {
    let d = tempfile::tempdir().unwrap();
    let p = broken(d.path());
    let before = std::fs::read(&p).unwrap();
    let o = sdc().args(["patch-header", "--in-place", "--dry-run"]).arg(&p).output().unwrap();
    assert_eq!(code(&o), 0);
    assert_eq!(std::fs::read(&p).unwrap(), before, "dry run writes nothing");
    let o = sdc().args(["patch-header", "--in-place"]).arg(&p).output().unwrap();
    assert_eq!(code(&o), 0, "{}", text(&o.stderr));
    let after = std::fs::read(&p).unwrap();
    assert_ne!(u32::from_le_bytes(after[8..12].try_into().unwrap()) & 0x2_0000, 0, "MIPMAPCOUNT restored");
    assert_eq!(u32::from_le_bytes(after[108..112].try_into().unwrap()), 0x1000 | 0x8 | 0x40_0000);
}

#[test]
fn a_subtype_profile_fixes_the_tag_and_an_impossible_tag_is_refused() {
    let d = tempfile::tempdir().unwrap();
    let p = broken(d.path());
    let fixed = d.path().join("fixed");
    let o = sdc()
        .args(["patch-header", "--target", "cs", "--subtype", "pbr_albedo", "--out-dir"])
        .arg(&fixed)
        .arg(&p)
        .output()
        .unwrap();
    assert_eq!(code(&o), 0, "{}", text(&o.stdout));
    assert_eq!(u32::from_le_bytes(std::fs::read(fixed.join("wall.dds")).unwrap()[128..132].try_into().unwrap()), 99);
    let o = sdc()
        .args(["patch-header", "--tag", "srgb", "--format", "bc4", "--dry-run", "--in-place"])
        .arg(&p)
        .output()
        .unwrap();
    assert_eq!(code(&o), 1);
    assert!(text(&o.stdout).contains("DDS-HEADER-UNPATCHABLE"), "{}", text(&o.stdout));
}

#[test]
fn a_valid_file_writes_nothing() {
    let d = tempfile::tempdir().unwrap();
    let p = broken(d.path());
    let fixed = d.path().join("fixed");
    sdc().args(["patch-header", "--in-place"]).arg(&p).output().unwrap();
    let o = sdc().args(["patch-header", "--out-dir"]).arg(&fixed).arg(&p).output().unwrap();
    assert_eq!(code(&o), 0);
    assert!(text(&o.stdout).contains("nothing to patch"), "{}", text(&o.stdout));
    assert!(!fixed.join("wall.dds").exists());
}

#[test]
fn an_output_choice_is_required() {
    let d = tempfile::tempdir().unwrap();
    let p = broken(d.path());
    let o = sdc().args(["patch-header"]).arg(&p).output().unwrap();
    assert_eq!(code(&o), 2);
}
```

- [ ] **Step 2: Run them and see them fail.** Run `cargo test -p sdc --test cli_patch_header`. Expect: the subcommand is unrecognised (exit 2 in every test except the last).

- [ ] **Step 3: Implement.** `args.rs`:
- In `Command` add:

```rust
    /// Fix DDS header faults that need no pixel change (mip count, flags, caps, legacy/DX10, an explicit tag).
    PatchHeader(PatchHeaderArgs),
```

- Add:

```rust
#[derive(Clone, Copy, ValueEnum)]
pub enum TagArg {
    Srgb,
    Unorm,
}

#[derive(Args)]
#[command(group(clap::ArgGroup::new("dest").required(true).args(["out_dir", "in_place"])))]
pub struct PatchHeaderArgs {
    #[arg(required = true)]
    pub dds: Vec<PathBuf>,
    /// Write each patched file into this folder (by file name). The input is never changed.
    #[arg(long)]
    pub out_dir: Option<PathBuf>,
    /// Rewrite each input in place (atomically).
    #[arg(long)]
    pub in_place: bool,
    /// Target whose rules give the tag and mip expectation of `--subtype` (default cs).
    #[arg(long, value_enum)]
    pub target: Option<TargetArg>,
    /// Sub-type the files are meant to be: sets the tag and requires a full mip chain when its profile has one.
    #[arg(long)]
    pub subtype: Option<String>,
    /// Tag to write; overrides `--subtype`'s. Without either, the file's tag is kept.
    #[arg(long, value_enum)]
    pub tag: Option<TagArg>,
    /// Block format the files must be; another one is refused.
    #[arg(long)]
    pub format: Option<String>,
    /// Report what would change; write nothing.
    #[arg(long)]
    pub dry_run: bool,
}
```

`main.rs`: `Command::PatchHeader(a) => verbs::patch_header(a),`.

`verbs.rs`:

```rust
/// `sdc patch-header` (DECISIONS S7). Exit 1 when a file is unreadable or refused, else 0; usage errors are 2.
pub fn patch_header(a: PatchHeaderArgs) -> Result<i32, EngineError> {
    use engine::dds_patch::{plan_patch, PatchRequest};
    let target = a.target.map_or(Target::Cs, Target::from);
    let mut req = PatchRequest::default();
    if let Some(id) = &a.subtype {
        let st: Subtype = serde_json::from_value(Value::String(id.clone()))
            .map_err(|_| EngineError::Usage(format!("--subtype {id}: unknown sub-type")))?;
        if !matches!(availability(target, st), Availability::Available) {
            return Err(EngineError::Usage(format!("--subtype {id} is unavailable under {target:?} (CFG-UNAVAILABLE)")));
        }
        let p = Engine::new()?.registry().base_profile(target, st);
        req.srgb = Some(p.srgb_tag());
        req.require_full_mips = p.mips.levels == MipLevels::Keyword(MipKeyword::Full);
    }
    if let Some(t) = a.tag {
        req.srgb = Some(matches!(t, TagArg::Srgb));
    }
    if let Some(f) = &a.format {
        req.expect_format = Some(
            serde_json::from_value(Value::String(f.clone()))
                .map_err(|_| EngineError::Usage(format!("--format {f}: unknown format")))?,
        );
    }
    let mut names = std::collections::BTreeSet::new();
    if a.out_dir.is_some() {
        for p in &a.dds {
            let n = p.file_name().unwrap_or_default().to_string_lossy().to_lowercase();
            if !names.insert(n.clone()) {
                return Err(EngineError::Usage(format!("two inputs are named {n}: --out-dir would write one over the other")));
            }
        }
    }
    let mut exit = 0;
    for path in &a.dds {
        let shown = path.display();
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                println!("{shown}: unreadable: {e}");
                exit = 1;
                continue;
            }
        };
        match plan_patch(&bytes, &req) {
            Ok(None) => println!("{shown}: header already valid; nothing to patch"),
            Ok(Some(p)) => {
                let w = p.warning();
                println!("{shown}: {}: {}", w.code.as_str(), w.message);
                if !a.dry_run {
                    let out = match &a.out_dir {
                        Some(dir) => dir.join(path.file_name().unwrap_or_default()),
                        None => path.clone(),
                    };
                    engine::dds::write_atomic(&out, &p.bytes)
                        .map_err(|e| EngineError::Fatal(format!("{}: {e}", out.display())))?;
                }
            }
            Err(u) => {
                let w = u.warning();
                println!("{shown}: {}: {}", w.code.as_str(), w.message);
                exit = 1;
            }
        }
    }
    Ok(exit)
}
```

Add `PatchHeaderArgs`, `TagArg` and `MipLevels`/`MipKeyword` to the existing `use` lines (`engine::config::*` may already cover the last two). Follow Note J (result verbs print to stdout).

- [ ] **Step 4: Run the gates, then commit.**

```bash
git add crates/cli/src/args.rs crates/cli/src/main.rs crates/cli/src/verbs.rs crates/cli/tests/cli_patch_header.rs
git commit -m "feat(cli): sdc patch-header (--out-dir | --in-place, --subtype, --tag, --format, --dry-run) (M6, DECISIONS S7)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 11: GUI: format dropdowns, uncompressed preview, HDR reason, header-patch choice

**Files:**
- Create: `crates/gui/src/model/dds_patch.rs`
- Modify: `crates/gui/src/model/mod.rs` (`pub mod dds_patch;`, `AppModel.dds_patch: dds_patch::DdsPatchState` initialised with `Default::default()` in every constructor), `crates/gui/src/model/fields.rs` (`format_options`, `field_lock`), `crates/gui/src/model/preview.rs` (`decode_levels`, `dds_preview`), `crates/gui/src/ui/settings_panel.rs` (HDR line, `other_warnings` filter, the DDS-item header panel, `Action` variants)
- Test: `crates/gui/tests/fields.rs`, `crates/gui/tests/preview_model.rs`, `crates/gui/tests/ui_smoke.rs`

**Interfaces:**
- Consumes: Task 5 formats, Task 7 `decode_level_rgba`, Task 9 `plan_patch`.
- Produces: `DdsHeaderView`, `AppModel::{dds_header, set_dds_patch_tag, patch_dds_header}`.

**Expectations that change:** `gui/tests/advanced.rs`, `edges.rs` and `preview_model.rs::a_dds_input_previews_its_own_bytes` stay green: BC files still decode the same way.

- [ ] **Step 1: Write the failing tests.** `crates/gui/tests/fields.rs`:

```rust
#[test]
fn uncompressed_formats_are_offered_and_rgba16f_only_under_cs() {
    for f in [Format::Rgba8, Format::R8, Format::Rg8] {
        assert!(format_options(Target::Vanilla).contains(&f), "{f:?}");
    }
    assert!(format_options(Target::Cs).contains(&Format::Rgba16f));
    assert!(!format_options(Target::Cpm).contains(&Format::Rgba16f));
}

#[test]
fn uncompressed_locks_explain_themselves() {
    let reg = engine::presets::PresetRegistry::builtin().unwrap();
    let mut p = reg.base_profile(Target::Cs, Subtype::Height);
    p.format = Format::R8;
    assert_eq!(field_lock(Target::Cs, &p, "color.tag"), Some("R8 and RG8 have no _SRGB form"));
    assert_eq!(field_lock(Target::Cs, &p, "encoder.effort"), Some("no encoder runs for uncompressed formats"));
    p.format = Format::Rgba16f;
    assert_eq!(field_lock(Target::Cs, &p, "color.tag"), Some("float formats have no _SRGB form"));
}
```

(`fields.rs` imports `sdc_gui::model::fields::*` and `engine::config::*`; add `field_lock` to its `use` line if it is listed by name.)

`crates/gui/tests/preview_model.rs`:

```rust
/// Writes a 16x16 full-chain DDS of `pf` with every texel `px` (raw level bytes per texel).
fn raw_dds(path: &std::path::Path, dxgi: u32, bpp: u32, texel: &[u8]) {
    let mut levels = vec![];
    let mut s = 16u32;
    loop {
        levels.extend(std::iter::repeat_n(texel, (s * s) as usize).flatten().copied());
        if s == 1 { break; }
        s /= 2;
    }
    let mut hd = [0u32; 31];
    hd[0] = 124; hd[1] = 0x1 | 0x2 | 0x4 | 0x8 | 0x1000 | 0x2_0000; hd[2] = 16; hd[3] = 16; hd[4] = 16 * bpp;
    hd[5] = 1; hd[6] = 5; hd[18] = 32; hd[19] = 0x4; hd[20] = u32::from_le_bytes(*b"DX10"); hd[26] = 0x1000 | 0x8 | 0x40_0000;
    let mut b = b"DDS ".to_vec();
    for v in hd { b.extend_from_slice(&v.to_le_bytes()); }
    for v in [dxgi, 3, 0, 1, 0] { b.extend_from_slice(&v.to_le_bytes()); }
    b.extend(levels);
    std::fs::write(path, b).unwrap();
}

/// DECISIONS S5: N11 ends. sdc's own rgba8 and a third-party BGRX8 preview from the CPU decode.
#[test]
fn uncompressed_dds_inputs_preview_from_the_cpu_decode() {
    for (name, dxgi, bpp, texel, want) in [
        ("own.dds", 87u32, 4u32, vec![30u8, 20, 10, 40], [10.0f32, 20.0, 30.0, 40.0]),
        ("third.dds", 88, 4, vec![30, 20, 10, 0], [10.0, 20.0, 30.0, 255.0]),
    ] {
        let d = tempfile::tempdir().unwrap();
        let root = support::mod_folder(d.path());
        raw_dds(&root.join("landscape").join(name), dxgi, bpp, &texel);
        let mut m = opened(d.path());
        m.expand(&format!("landscape/{name}"));
        settle(&mut m);
        let EncodedState::Done(out) = &m.preview.encoded else {
            panic!("{name}: {:?}", m.preview.encoded)
        };
        let px = out.decoded[0][0];
        for c in 0..4 {
            assert!((px[c] * 255.0 - want[c]).abs() < 0.5, "{name} c{c}: {px:?}");
        }
    }
}
```

`crates/gui/tests/ui_smoke.rs`:

```rust
#[test]
fn an_8_bit_emissive_says_why_it_is_bc7() {
    let d = tempfile::tempdir().unwrap();
    let t = d.path().join("textures/pbr");
    std::fs::create_dir_all(&t).unwrap();
    image::ImageBuffer::from_fn(16, 16, |x, _| image::Rgb([(x * 16) as u8, 40u8, 20]))
        .save(t.join("lamp_g.png"))
        .unwrap();
    let mut m = AppModel::new(GuiSettings::default(), None, session());
    m.open_folder(d.path().join("textures"));
    assert!(m.wait_idle(std::time::Duration::from_secs(60)));
    m.select_item("pbr/lamp_g.png");
    let mut h = harness(m);
    h.run();
    h.get_by_label_contains("8-bit source, no HDR → BC7 sRGB");
}

#[test]
fn a_dds_with_undeclared_mips_offers_a_header_patch_and_writes_a_copy() {
    use engine::config::Format;
    use engine::dds::*;
    let d = tempfile::tempdir().unwrap();
    let root = mod_folder(d.path());
    let mut levels = vec![];
    let mut s = 64u32;
    loop {
        levels.push(vec![0u8; level_size(Format::Bc7, s, s)]);
        if s == 1 { break; }
        s /= 2;
    }
    let mut b = dds_bytes(&DdsDesc { width: 64, height: 64, format: Format::Bc7, srgb: false, header: ResolvedHeader::Dx10 }, &levels).unwrap();
    b[28..32].copy_from_slice(&1u32.to_le_bytes());
    let src = root.join("landscape/broken.dds");
    std::fs::write(&src, &b).unwrap();
    let mut m = opened(d.path());
    m.select_item("landscape/broken.dds");
    let mut h = harness(m);
    h.run();
    h.get_by_label_contains("mip count 1 → 7");
    press(&mut h, "Patch header…");
    press(&mut h, "Patch");
    let out = d.path().join("export/landscape/broken.dds");
    let patched = std::fs::read(&out).expect("a patched copy in the export folder");
    assert_eq!(&patched[148..], &b[148..]);
    assert_eq!(std::fs::read(&src).unwrap(), b, "the source is untouched");
    h.get_by_label_contains("DDS-HEADER-PATCHED");
}
```

(`opened()` sets `export_folder` to `<dir>/export`. `support` is already `mod support;` with `use support::*;` in both test files.)

- [ ] **Step 2: Run them and see them fail.** Run `cargo test -p sdc-gui --test fields --test preview_model --test ui_smoke`. Expect: the new options are missing, "cannot be previewed", no HDR line, and no patch panel.

- [ ] **Step 3: Implement `fields.rs`.**

```rust
pub fn format_options(target: Target) -> Vec<Format> {
    [
        Format::Bc1, Format::Bc3, Format::Bc4, Format::Bc5, Format::Bc6hUf16, Format::Bc7,
        Format::Rgba8, Format::R8, Format::Rg8, Format::Rgba16f,
    ]
    .into_iter()
    .filter(|f| !f.is_hdr() || target == Target::Cs)
    .collect()
}
```

In `field_lock`, replace the first `color.tag` arm with these three, placed before the `target != Cs` arm:

```rust
        "color.tag" if p.format == Format::Bc6hUf16 => Some("BC6H has no _SRGB form"),
        "color.tag" if p.format == Format::Rgba16f => Some("float formats have no _SRGB form"),
        "color.tag" if matches!(p.format, Format::R8 | Format::Rg8) => Some("R8 and RG8 have no _SRGB form"),
        "encoder.effort" | "encoder.perceptual" if !p.format.is_block() => {
            Some("no encoder runs for uncompressed formats")
        }
```

- [ ] **Step 4: Implement the uncompressed preview.** In `model/preview.rs`:
- Replace `to_rgba` and `decode_levels`' inner decode with `engine::decode::decode_level_rgba(r.format.pixel_format(r.srgb), w, h, data, Some(bc1))`. Keep the "a mip that fails comes back black" fallback, and resize to `pixels` as now.
- In `dds_preview`, replace the `as_format` early return with:

```rust
    // DECISIONS S5: every uncompressed layout previews (N11 ends); BC2 and signed BC6H still cannot.
    let (format, srgb) = match f.format.as_format() {
        Some(v) => v,
        None if f.format.block_bytes().is_none() => {
            let float = matches!(f.format, engine::dds::DdsPixelFormat::Rgba32f);
            (if float { Format::Rgba16f } else { Format::Rgba8 }, false)
        }
        None => {
            return Err(format!("{:?} cannot be previewed (BC2 and signed BC6H are not supported)", f.format));
        }
    };
```

- After `levels`, when `f.format.as_format().is_none()`, build `decoded` directly from the file's own format (`decode_level_rgba(f.format, w, h, &f.data[r.clone()], Some(bc1))` per kept level) instead of `decode_levels(&result, bc1)`, because the stand-in `format` above is only the display kind. The render takes the CPU path for every non-block format (Task 5).
- Remove `pixel_format` if nothing else uses it. Otherwise leave it delegating.

- [ ] **Step 5: Implement the HDR reason.** In `settings_panel.rs` `group()`, after the grid, when `name == "Compression"`:

```rust
            if name == "Compression" {
                if let Some(w) = cx.warnings.iter().find(|w| w.code == Code::FmtHdrSource) {
                    ui.weak(&w.message);
                }
            }
```

Pass the item's `warnings` into `Ctx`: add a `warnings: Vec<Warning>` field set from the cloned `warnings` in `draw`, and read it as `cx.warnings`. In `other_warnings`, also filter out `Code::FmtHdrSource`, so it is shown once.

- [ ] **Step 6: Implement the header-patch model.** Create `crates/gui/src/model/dds_patch.rs`:

```rust
//! The DDS header-patch choice for a DDS item (DECISIONS S7). Never automatic: the user picks the tag and presses
//! Patch; the patched copy goes to the export folder, never over the source.
use engine::dds_patch::{plan_patch, PatchRequest};
use engine::policy::SourceStamp;
use std::collections::BTreeMap;
use std::path::PathBuf;

/// What the panel shows for one DDS item.
#[derive(Debug, Clone, PartialEq)]
pub struct DdsHeaderView {
    /// `check_header` findings of the file as it is.
    pub issues: Vec<String>,
    /// The fixes a patch would make (`Ok(None)`: nothing to patch), or why it cannot be patched.
    pub patch: Result<Option<Vec<String>>, String>,
    /// The tag choice: `None` keeps the file's.
    pub tag: Option<bool>,
}

#[derive(Debug, Clone, Default)]
pub struct DdsPatchState {
    /// key → (stamp, tag choice, view); recomputed when the file or the choice changes.
    cache: BTreeMap<String, (Option<SourceStamp>, Option<bool>, DdsHeaderView)>,
    /// The key whose "Patch header…" was pressed and awaits "Patch" or "Cancel".
    pub confirming: Option<String>,
    /// The last result line, shown under the panel.
    pub last: Option<String>,
}

impl DdsPatchState {
    pub(crate) fn view(&mut self, key: &str, path: &std::path::Path) -> DdsHeaderView {
        let stamp = SourceStamp::of(path);
        let tag = self.cache.get(key).and_then(|(_, t, _)| *t);
        if let Some((s, t, v)) = self.cache.get(key) {
            if *s == stamp && *t == tag {
                return v.clone();
            }
        }
        let view = compute(path, tag);
        self.cache.insert(key.to_string(), (stamp, tag, view.clone()));
        view
    }

    pub(crate) fn set_tag(&mut self, key: &str, tag: Option<bool>) {
        let e = self.cache.entry(key.to_string()).or_insert((None, None, empty()));
        e.0 = None; // force a recompute
        e.1 = tag;
    }
}

fn empty() -> DdsHeaderView {
    DdsHeaderView { issues: vec![], patch: Ok(None), tag: None }
}

fn compute(path: &std::path::Path, tag: Option<bool>) -> DdsHeaderView {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => return DdsHeaderView { issues: vec![], patch: Err(e.to_string()), tag },
    };
    let issues = engine::dds::read_dds(&bytes).map(|f| engine::dds::check_header(&f)).unwrap_or_default();
    let patch = plan_patch(&bytes, &PatchRequest { srgb: tag, ..Default::default() })
        .map(|p| p.map(|p| p.fixes))
        .map_err(|u| u.0);
    DdsHeaderView { issues, patch, tag }
}

/// Write the patched copy of `src` to `dest` (DECISIONS S7); the caller has resolved both paths.
pub(crate) fn write_patch(src: &std::path::Path, dest: &PathBuf, tag: Option<bool>) -> Result<String, String> {
    let same = |p: &std::path::Path| std::path::absolute(p).unwrap_or_default().to_string_lossy().to_lowercase();
    if same(src) == same(dest) {
        return Err("the export folder is the mod folder: patching would overwrite the source; choose another export folder".into());
    }
    let bytes = std::fs::read(src).map_err(|e| e.to_string())?;
    match plan_patch(&bytes, &PatchRequest { srgb: tag, ..Default::default() }) {
        Ok(Some(p)) => {
            engine::dds::write_atomic(dest, &p.bytes).map_err(|e| e.to_string())?;
            let w = p.warning();
            Ok(format!("{}: {}", w.code.as_str(), w.message))
        }
        Ok(None) => Ok("header already valid; nothing to patch".into()),
        Err(u) => Err(u.warning().message),
    }
}
```

In `model/mod.rs`:
- Add `pub mod dds_patch;` and the field `pub dds_patch: dds_patch::DdsPatchState,` to `AppModel`, initialised with `Default::default()` wherever `AppModel` is built (`new`, `placeholder`).
- Add:

```rust
    /// The header check and patch plan of DDS item `key` (DECISIONS S7); `None` for other items.
    pub fn dds_header(&mut self, key: &str) -> Option<dds_patch::DdsHeaderView> {
        let item = self.item(key).filter(|i| i.kind == ItemKind::Dds)?;
        let path = item.input.clone();
        Some(self.dds_patch.view(key, &path))
    }

    pub fn set_dds_patch_tag(&mut self, key: &str, srgb: Option<bool>) {
        self.dds_patch.set_tag(key, srgb);
    }

    /// Write the patched copy of DDS item `key` to `<export folder>/<job key>`.
    pub fn patch_dds_header(&mut self, key: &str) -> Result<std::path::PathBuf, String> {
        let item = self.item(key).filter(|i| i.kind == ItemKind::Dds).ok_or("not a DDS item")?;
        let export = self.settings.export_folder.clone().ok_or("set an export folder first")?;
        let (src, dest) = (item.input.clone(), export.join(&item.job_key));
        let tag = self.dds_header(key).and_then(|v| v.tag);
        let r = dds_patch::write_patch(&src, &dest, tag);
        self.dds_patch.confirming = None;
        self.dds_patch.last = Some(match &r {
            Ok(m) => m.clone(),
            Err(e) => format!("DDS-HEADER-UNPATCHABLE: {e}"),
        });
        r.map(|_| dest)
    }
```

Adjust the field names (`item.input`, `item.job_key`, `self.settings.export_folder`) to the real ones if they differ: `TextureItem` has `input` and `job_key`, and `GuiSettings` has `export_folder`.

- [ ] **Step 7: Implement the header-patch panel.** In `settings_panel.rs` `draw`:
- Add `PatchTag(Option<bool>)`, `PatchAsk`, `PatchDo` and `PatchCancel` to `enum Action`.
- Before the `ui.add_enabled_ui` closure, compute `let header = model.dds_header(key);` and `let confirming = model.dds_patch.confirming.as_deref() == Some(key);`, and clone `model.dds_patch.last`.
- In the `None =>` branch, replace the `if kind == ItemKind::Dds { ui.label("Already a DDS file; it is not converted."); }` line with:

```rust
            if kind == ItemKind::Dds {
                ui.label("Already a DDS file; it is not converted (opt in with reencode in sdc.toml).");
                if let Some(v) = &header {
                    dds_header_panel(ui, v, confirming, last.as_deref(), &mut actions);
                }
            }
```

- Add the panel function:

```rust
fn dds_header_panel(
    ui: &mut egui::Ui,
    v: &crate::model::dds_patch::DdsHeaderView,
    confirming: bool,
    last: Option<&str>,
    actions: &mut Vec<Action>,
) {
    ui.add_space(6.0);
    ui.label(egui::RichText::new("DDS header").strong());
    if v.issues.is_empty() && matches!(v.patch, Ok(None)) {
        ui.label("Header: valid");
    }
    for i in &v.issues {
        ui.label(egui::RichText::new(i).small());
    }
    ui.horizontal(|ui| {
        ui.label("Tag on patch:");
        for (label, tag) in [("keep", None), ("sRGB", Some(true)), ("UNORM", Some(false))] {
            if ui.radio(v.tag == tag, label).clicked() {
                actions.push(Action::PatchTag(tag));
            }
        }
    });
    match &v.patch {
        Ok(Some(fixes)) => {
            for f in fixes {
                ui.label(format!("• {f}"));
            }
            if confirming {
                ui.label("Writes a copy with a fixed header to the export folder; the pixel data is copied byte for byte.");
                ui.horizontal(|ui| {
                    if ui.button("Patch").clicked() {
                        actions.push(Action::PatchDo);
                    }
                    if ui.button("Cancel").clicked() {
                        actions.push(Action::PatchCancel);
                    }
                });
            } else if ui.button("Patch header…").clicked() {
                actions.push(Action::PatchAsk);
            }
        }
        Ok(None) => {}
        Err(reason) => {
            ui.colored_label(RED, format!("Cannot patch: {reason}"));
        }
    }
    if let Some(l) = last {
        ui.label(egui::RichText::new(l).small());
    }
}
```

- In the action loop, add:

```rust
            Action::PatchTag(t) => model.set_dds_patch_tag(key, t),
            Action::PatchAsk => model.dds_patch.confirming = Some(key.to_string()),
            Action::PatchCancel => model.dds_patch.confirming = None,
            Action::PatchDo => {
                let _ = model.patch_dds_header(key);
            }
```

`patch_dds_header` stores the result line in `dds_patch.last`, so the error is shown, not dropped.

- [ ] **Step 8: Run the gates.** `ui_smoke` tests need the system fonts (as today).

- [ ] **Step 9: Commit.**

```bash
git add crates/gui/src/model/dds_patch.rs crates/gui/src/model/mod.rs crates/gui/src/model/fields.rs crates/gui/src/model/preview.rs crates/gui/src/ui/settings_panel.rs crates/gui/tests/fields.rs crates/gui/tests/preview_model.rs crates/gui/tests/ui_smoke.rs
git commit -m "feat(gui): uncompressed formats and preview, the HDR reason, the explicit DDS header-patch choice (M6, DECISIONS S3, S5, S7)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 12: Docs, DECISIONS Note S, workspace 0.3.0

**Files:**
- Modify: `docs/superpowers/specs/2026-10-08-skyrim-dds-converter-design.md`, `docs/gui.md`, `docs/DECISIONS.md`, `docs/superpowers/plans/2026-10-08-skyrim-dds-converter-phase0-2.md` (M6 section), `Cargo.toml`, `Cargo.lock`

**Interfaces:**
- Consumes: everything above.
- Produces: no code. The version is 0.3.0.

**Expectations that change:** none. No test pins the version.

- [ ] **Step 1: Update the spec.**
  - **§1 Outputs:** "DDS in BC1, BC3, BC4, BC5, BC6H or BC7, or uncompressed (`rgba8` as B8G8R8A8, `r8`, `rg8`, `rgba16f`; DECISIONS O1, S4)".
  - **§3.2:**
    - the `format` values gain `rgba8`, `r8`, `rg8`, `rgba16f`;
    - `encoder.backend` gains `uncompressed` (no encoder runs);
    - note `reencode` as a plan-time selector (S6).
  - **§4.2:** the `pbr_emissive` row becomes "**BC6H_UF16 for an HDR source** (float, peak > 1.0); otherwise BC7_UNORM_SRGB (`analysis:hdr-source`, DECISIONS O2, S3)". Add a bullet for the uncompressed formats' tags and `rgba16f` cs-only.
  - **§5.1:**
    - delete "OpenEXR, Radiance HDR, BMP and JPEG arrive in M6" and "(the info event lands in M6)";
    - state the ICC info (`IN-ICC-IGNORED`) and NaN-safe statistics;
    - replace the "Interim float guard" paragraph with the S1 split in three sentences;
    - change the `Analysis` finding to "carries a DDS input's DXGI format".
  - **§5.7:** uncompressed files use `DDSD_PITCH` with a tight row pitch, and `rgba8` UNORM under `auto` is a legacy B8G8R8A8 header.
  - **§5.8:** per-format PSNR channels (`r8` R, `rg8` RG) and no BC-only checks for uncompressed formats.
  - **§8:**
    - the "DDS input in `convert`" row becomes "Skipped (`IN-DDS-SKIPPED`) unless `--set [file:<glob>.]reencode=true` or `[files] reencode = true` (`LOSS-REENCODE` for a block-compressed input; a re-encode over its own source fails)";
    - add the verb `sdc patch-header` with its flags and exit codes.
  - **§10:**
    - `LOSS-CLAMP-HDR`: "Float values outside [0, 1] written to an LDR format → clamped (DECISIONS S1)";
    - `LOSS-REENCODE` drops "(M6)" and "Not emitted yet";
    - `IN-FLOAT-LDR` becomes "info, **retired (M6, DECISIONS S2)**: never emitted; `--allow IN-FLOAT-LDR` still parses";
    - new rows `IN-ICC-IGNORED` (info), `FMT-HDR-SOURCE` (info), `DDS-HEADER-PATCHED` (info) and `DDS-HEADER-UNPATCHABLE` (warning), each with its trigger and fix.

- [ ] **Step 2: Update `docs/gui.md`.**
  - Replace the line "A DDS in a format sdc does not write (uncompressed, BC2, signed BC6H) shows "cannot be previewed"." with "Uncompressed DDS files preview from the CPU decode; BC2 and signed BC6H show "cannot be previewed" (N11 ended in M6)."
  - Add to the settings panel section:
    - Compression shows the `FMT-HDR-SOURCE` reason for an emissive;
    - the format dropdown lists the uncompressed formats (`RGBA16F` only under `cs`), with Effort and Perceptual locked for them;
    - a DDS item shows its header check, a "Tag on patch" choice and "Patch header…" then "Patch", which writes a copy to the export folder.

- [ ] **Step 3: Add DECISIONS Note S.** Append after the last Note R entry (the end of the file):

```markdown
## Note S — M6 Track B: inputs and formats (plan rulings, 2026-10-11)

Rulings of `docs/superpowers/plans/2026-10-11-sdc-m6-inputs-formats.md`, for owner review. Owner confirmed the defaults on 2026-10-11: `hdr_sky` keeps BC6H for every source (S3), `rgba16f` is `cs`-only (S4), header-patch destination as S7.

- **S1 Float-source transfer split** (replaces the final-review I4 interim guard): a float source (32-bit float TIFF, OpenEXR, Radiance HDR, float or half DDS) is linear light. Preprocess never sRGB-decodes it; for an LDR output Quantise clamps each channel to [0, 1] and encodes it per `color.content` (`srgb` → sRGB OETF, `linear` → stored), the encoding a decoded integer source already gets. Keyed on content, not the tag: vanilla `diffuse`/`glow` are content `srgb` with tag `unorm` and must stay gamma-encoded. Values outside [0, 1] raise `LOSS-CLAMP-HDR` (warning). A float normal, `_msn` or `_flow` keeps today's stored-value reading. An explicit `color.content = srgb` no longer decodes a float source.
- **S2 `IN-FLOAT-LDR` retired:** never emitted, kept in the catalogue as Info so `--allow IN-FLOAT-LDR` and `[files] allow` lists still load (catalogue count unchanged).
- **S3 O2 for `pbr_emissive`:** BC6H only when the source is float and some finite R, G or B is above 1.0; otherwise BC7 with tag `srgb`, as the fact `analysis:hdr-source`, applied after every layer and skipped when any explicit layer (user/project preset, config, rule, `[files]`, `--set`) set `format`. On `pbr_emissive` the tag follows the final format (`n/a` for BC6H, else `srgb`) unless a layer set it, so `--set file:<glob>.format=…` alone wins in both directions. `FMT-HDR-SOURCE` (info) states the reason on every available `pbr_emissive`. `sdc-base` is unchanged (still 1.1.0). `hdr_sky` is unchanged (owner question).
- **S4 Uncompressed outputs (O1):** `rgba8` (DXGI 87/91, B8G8R8A8; legacy RGB masks under `header = auto` for UNORM), `r8` (61), `rg8` (49), `rgba16f` (10, tag `n/a`, the `Hdr` domain, alpha kept as half, `cs` only). `DDSD_PITCH` with a tight row pitch. Backend `uncompressed`. `r8`/`rg8` refuse `srgb` (`CFG-INVALID`) and block on normals (`FMT-BC5-NORMAL`). texconv/nvddsinfo check: <paste the Task 6 Step 5 result>.
- **S5 Uncompressed input (O1):** `Bgra8` gains its sRGB form (DXGI 91); sdc's own uncompressed files are fully validated; RGBA-order RGBA8, BGRX8 and RGBA32F stay header-only in `validate` but are inspected, previewed and re-encoded. The reader assumes tight rows: a padded pitch is `VAL-HEADER`. N11 ends; BC2 and signed BC6H still cannot be previewed.
- **S6 DDS re-encode:** `reencode` is a plan-time selector (not a `Profile` field), boolean, unscoped/file/`[files]` only. Mip 0 is decoded, the rest regenerated; `Analysis.dds_dxgi` records the input's DXGI. `LOSS-REENCODE` (warning) only for block-compressed inputs. A re-encoded DDS takes part in duplicate detection; one whose output is its own path fails at plan; cubemaps and arrays fail to load; a re-encoded DDS member does not bake.
- **S7 DDS header patch (owner idea 2026-10-11):** header-only, payload byte-identical (new header + the old bytes after the old header). Patchable: flags, caps, linear size/pitch, mip count (the whole-level count that fills the payload), reserved fields, `DDPF_NORMAL`, DX10 fields, `ATI1`/`ATI2` → DX10, legacy → DX10 where the tag needs it, and the tag only on an explicit `--tag`, `--subtype` or GUI choice. Refused with a reason (`DDS-HEADER-UNPATCHABLE`): unreadable, cubemap/array, a format sdc does not write, sRGB on a format without one, a payload holding no whole chain, missing mips when a full chain is wanted, BC1 with transparent texels, the wrong block format under `--format`. `DDS-HEADER-PATCHED` (info) lists the fixes. CLI `sdc patch-header` needs `--out-dir` or `--in-place`; the GUI writes a copy to the export folder only. Not recorded in the manifest.
- **S8 ICC:** detected and ignored; `IN-ICC-IGNORED` (info).
- **S9 `IN-NONFINITE`:** reported at plan time (so `inspect` and `--dry-run` show it), once; the analysis statistics skip non-finite samples.
- **S10 Inputs:** `.exr` (first RGB(A) layer, f32), `.hdr`, `.bmp`, `.jpg`/`.jpeg` through `image` 0.25.10 features; `exr` 1.74.2 and `lebe` are BSD-3-Clause (THIRD_PARTY_NOTICES). Undecodable files are `IN-DECODE`.
- **S11 Version:** workspace 0.3.0: outputs changed (float sources, 8-bit emissives), so `if-changed` re-encodes once.
```

Also, in Note O:
- after O1 append " **Done in M6 (Note S4, S5).**";
- after O2 append " **Done in M6 (Note S3); `hdr_sky` open.**";
- after N11 append " **Ended in M6 (Note S5).**".

- [ ] **Step 4: Update the roadmap's M6 section.** After each carried item this plan finished, add " **Done (M6 Track B, Note S).**":
  - the float-source split;
  - `FIELD_PATHS` lacks `reencode` (the `resize` half stays open);
  - `IN-NONFINITE`;
  - Load: record the DXGI format;
  - the Owner idea (header patch);
  - O1;
  - O2.

  Leave every other M6 item untouched.

- [ ] **Step 5: Bump the version.** In `Cargo.toml`, set `version = "0.3.0"` under `[workspace.package]`. Run `cargo build --workspace` so that `Cargo.lock` updates the workspace crates' versions, then run the three gates.

- [ ] **Step 6: Commit.**

```bash
git add docs/superpowers/specs/2026-10-08-skyrim-dds-converter-design.md docs/gui.md docs/DECISIONS.md docs/superpowers/plans/2026-10-08-skyrim-dds-converter-phase0-2.md Cargo.toml Cargo.lock
git commit -m "docs: M6 Track B in the spec, gui.md, DECISIONS Note S and the roadmap; workspace 0.3.0 (if-changed re-encodes)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

## Self-review (run while writing this plan)

1. **Scope coverage** (roadmap items 1–7):
   - 1, the inputs: Task 1, with goldens for EXR, HDR, BMP and JPEG.
   - 2, the float split: Task 3. The goldens for float TIFF diffuse (rewritten) and float BC6H (unchanged) are kept.
   - 3, O2 and its four tests: Task 4. `hdr_sky` is flagged at the top.
   - 4, O1 output: Tasks 5 and 6 (a golden per format, an env-gated texconv/nvddsinfo check). O1 input: Task 7. The round trip: Task 8. The preview: Task 11.
   - 5, the re-encode: Task 8, with `FIELD_PATHS` and `LOSS-REENCODE`; the DXGI in `Analysis` is Task 7.
   - 6, the header patch: Tasks 9, 10 and 11. Its three tests (byte identity, validate passes, refusal with a reason) are in Tasks 9 and 10.
   - 7, ICC: Task 1. `IN-NONFINITE` and NaN-safety: Task 2.
2. **Placeholders:** the only deliberate fill-in is Note S4's "<paste the Task 6 Step 5 result>". That is a measured result the controller supplies, not a design gap.
3. **Type consistency:**
   - `load_image_meta`, `SourceMeta { icc_profile, dds_dxgi }` and `Analysis { icc_profile, nonfinite, dds_dxgi }` are used identically in Tasks 1, 2, 7 and 8.
   - `Format::{is_block, is_hdr, pixel_format, block_bytes -> Option}` come from Task 5 and are used in Tasks 6, 9 and 11.
   - `PatchRequest { srgb, expect_format, require_full_mips }` comes from Task 9 and is used in Tasks 10 and 11.
   - `reencode_requested(ctx, input)` and `REENCODE_FIELD` come from Task 8.
4. **Code counts:** 63 → 64 (Task 1) → 65 (Task 4) → 67 (Task 9). `IN-FLOAT-LDR` is kept (S2).
5. **Review Focus** items 1–5 each name their test and task.

## Execution notes for the controller

- Tasks 1–4 are sequential: the float split before O2, because the peak ≤ 1 case needs it.
- Task 5 must precede Tasks 6, 7, 9 and 11.
- Tasks 9–10 do not depend on Tasks 6–8, so they could run in parallel worktrees. Keep them in order on one branch anyway: they share `codes.rs`, `schema/` and the code count.
- Task 6 Step 5 is the controller's: it runs an external exe and must not be delegated to an implementer without the paths above.
- After Task 12, request a whole-branch review. The reviewer's first checks are Note S1 (content versus tag) and S3 (the precedence deviation).

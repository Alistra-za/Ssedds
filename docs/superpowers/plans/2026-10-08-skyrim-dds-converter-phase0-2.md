# Skyrim DDS Converter (`sdc`) — Phase 0–2 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Install the toolchain, run the Phase 0 spikes as gated tasks, then build the `sdc` engine and a validated CLI MVP that converts PNG/TGA/TIFF (8/16-bit) sources into Skyrim SE/AE DDS files (BC1/BC3/BC4/BC5/BC6H/BC7) for the `cs`, `cpm` and `vanilla` targets.

**Architecture:** One Cargo workspace on `x86_64-pc-windows-msvc`. `encoders-sys` links committed ISPC objects (bc7e, ISPC TC) and builds rgbcx plus a C++ shim with MSVC. `engine` holds config, presets, policy, the float pipeline (preprocess → mips → coverage → quantise → encode), the DDS writer and reader, validation, events and the manifest. `cli` (`sdc.exe`) and `bench` (`sdc-bench`) are thin front ends over the engine API. The bench crate turns the existing `F:/ai/dds/bench` measurements into hard gates.

**Tech Stack:** Rust stable (MSVC target, static CRT), `cc` + MSVC for C++, ISPC 1.31.0 objects (pre-built, committed), `image` 0.25, `half` 2, `bcdec_rs` 0.2, `rayon` 1, `serde`/`serde_json`/`toml` 0.8, `schemars` 1, `sha2` 0.10, `globset` 0.4, `walkdir` 2, `tempfile` 3, `thiserror` 2, `anyhow` 1, `clap` 4, `ctrlc` 3, `humantime` 2; tests use `assert_cmd` 2 and `predicates` 3.

**Spec:** `F:/ai/dds/converter/docs/superpowers/specs/2026-10-08-skyrim-dds-converter-design.md` (the design), and `F:/ai/dds/converter/docs/DECISIONS.md` (owner decisions; these win where the two differ). Executors read both.

## Phase numbering used in this plan

The brief asked for "Phase 0 (spikes) and Phase 1–2 (engine skeleton + CLI MVP)". The project plan's roadmap (`F:/ai/dds/reports/Skyrim DDS converter project plan.md` §11) numbers phases differently. This plan uses its own numbers and maps them:

| This plan | Contents | Project-plan roadmap phases covered |
|---|---|---|
| **Phase 0** (Tasks 1–17) | Toolchain, workspace, vendored encoders, spikes S3, S4, S5, S7, gates of spec §11.2 for the encoders | Phase 0 (S1, S2, S6, S8 are owner-run, Task 46) |
| **Phase 1** (Tasks 18–39, incl. 26a, 27a) | Engine: config, presets (incl. suffix aliases), exporter suffix vocabulary, policy, pipeline, DDS I/O, validation, events, manifest, scheduler, golden tests | Phase 1 (golden cases, synthetic subset), Phase 2 (core engine) |
| **Phase 2** (Tasks 40–46) | CLI MVP and the MVP exit gate | Phase 3 (CLI), part of Phase 5 (three built-in "Quality" presets) |
| **M2.5** (milestone) | Channel packing of raw single-channel maps (spec §6.4, DECISIONS Note H), right after the CLI MVP | Not in the project-plan roadmap (owner addition, Note H); sits between Phases 3 and 4 |
| **M3–M8** (milestones) | GUI, variance bake, NVTT backend, preset completion + remaining inputs, quality validation + CI, packaging | Phases 4–7 |

## Global Constraints

Every task's requirements implicitly include these lines (values copied from the spec and `DECISIONS.md`).

- Licence: **GPL-3.0-or-later**. Vendored code: Apache-2.0 (bc7e.ispc), MIT/Unlicense (rgbcx), MIT (ISPC TC, bcdec_rs). No TruePBR-Manager (CC BY-NC 4.0) or AlphaBloom code is copied.
- Platform: **Windows x64 only**; Rust stable on **`x86_64-pc-windows-msvc`**; `+crt-static`; contributors need Rust and the MSVC Build Tools; **no ISPC at build time** (ISPC objects are committed with SHA-256 hashes and rebuilt only by `tools/regen-ispc.ps1` with **ISPC 1.31.0**).
- Vendored commits: bc7enc_rdo **`b943862`** (full `b9438627eef73a1157e84201b6fa6eb2ffd6d9f0`), ISPC Texture Compressor **`79ddbc9`**. The only change to any vendored encoder is `vendor/bc7e/patches/0001-exact-opaque-alpha.patch`.
- ISPC flags (as the bench built them): bc7e `-O2 --target=sse2,sse4,avx,avx2 --opt=fast-math --opt=disable-assertions`; ISPC TC kernel `-O2 --target=sse2,sse4,avx,avx2 --opt=fast-math`.
- Bench root: `F:/ai/dds/bench`, overridable with env `SDC_BENCH_ROOT`. `sdc-bench` reads it in place and writes only under `target/bench/`.
- Defaults that are owner requirements: BC7 = bc7e `veryslow` (u5), perceptual **off**; BC1 = rgbcx level 18, 3-colour mode on, **transparent-black off**; BC1 decoder model batch-wide, default **`cBC1Ideal`**; BC4/BC5 rgbcx HQ, search radius `default` = **5** (what the bench used), `max` = **10**; BC6H = ISPC TC `veryslow`, UF16 only; bc7e `slow` (u4) is **not offered**.
- Alpha: `remove` **drops the channel before any mip or encode step** (A = 255 is synthesised only when the 8-bit block input is built). Threshold default **127**, range **5–200**, set **per texture**; comparison default `gt`. Coverage preservation **on by default for `cutout` and `cutout_blend`**, switchable per image (DECISIONS Note E). `alpha.exact_opaque` is **on by default only for `envmask` and `complex_material`**.
- Colour: `_SRGB` is written only under `cs` and only for PBR colour (`pbr_albedo`, `pbr_emissive` when not BC6H, `pbr_subsurface`, `pbr_fuzz`); never under `vanilla` or `cpm`. BC6H only under `cs`. BC4/BC5 are unsigned; DX10 `BC4_UNORM`/`BC5_UNORM` by default, legacy `BC4U`/`BC5U` as an option, **`ATI1`/`ATI2` never written**.
- Mips: float32 throughout; full chain to 1×1 unless `none` (`interface/`, `sky/`). No ≤128 px BC7 fallback (DECISIONS #7).
- DDS writer: flags `CAPS|HEIGHT|WIDTH|PIXELFORMAT|LINEARSIZE` (+`MIPMAPCOUNT`), caps `TEXTURE` (+`COMPLEX|MIPMAP`), never `DDPF_NORMAL`, legacy `DXT1`/`DXT5` for BC1/BC3 UNORM, DX10 for every `_SRGB`, BC6H, BC7 and (default) BC4/BC5; reserved fields zero; atomic write (temp file in the target directory, then rename).
- Images above **16384 px** are rejected. Provenance goes to `<output_root>/_sdc/manifest.jsonl` by default; per-file `<name>.dds.json` is optional.
- CLI: human output on **stderr**; with `--json`, stdout carries **only JSON Lines**. Exit codes, highest precedence first: **3** fatal > **2** usage/config > **1** partial > **4** validation failed > **0** success.
- Never auto-decide: a file converts only when every field is decided by an explicit rule, flag, or a high-confidence suggestion accepted with `--accept-suggestions high`; diffuse alpha meaning is never high confidence.
- Credits (`docs/CREDITS.md`): AlphaBloom (tkRiverGamer) for Auto Coverage, Ignacio Castaño for coverage-preserving mips, jiayev's TruePBR-Manager for TruePBR slot defaults.

## Approvals the owner must give during execution

| Task | Action | Why approval |
|---|---|---|
| 1 | Install the VS 2022 "Desktop development with C++" workload (or VS Build Tools) — **UAC prompt** | System install |
| 1 | Install rustup (`winget install Rustlang.Rustup`) and the stable MSVC toolchain | Download + install |
| 2 | First `cargo build` fetches crates from crates.io | Network download |
| 3 | Fetch the GPL-3.0 licence text from `https://www.gnu.org/licenses/gpl-3.0.txt` | Download |
| 15 | Download `encoder/basisu_bc7e_scalar.cpp` from `BinomialLLC/basis_universal` (spike S4) | Download of third-party source |
| 46 | Owner runs S1, S2, S6, S8 in a Skyrim/CS install | Owner's game install and time |

---

## File Structure

```text
F:/ai/dds/converter/
├── Cargo.toml                       workspace, shared deps, profiles
├── rust-toolchain.toml              stable + x86_64-pc-windows-msvc, clippy, rustfmt
├── .cargo/config.toml               +crt-static for the MSVC target
├── .gitignore
├── LICENSE                          GPL-3.0 text
├── LICENSES/                        Apache-2.0 (bc7e), bc7enc_rdo LICENSE, ISPC TC license.txt
├── THIRD_PARTY_NOTICES.md
├── presets/                         built-in presets (TOML), embedded with include_str!
│   ├── sdc-base.toml                per-sub-type defaults (§4.1/§4.2); inherited by every preset
│   ├── cs-quality.toml  cpm-quality.toml  vanilla-quality.toml
├── schema/                          generated JSON Schemas (profile, preset, event, manifest)
├── tools/regen-ispc.ps1             rebuilds ISPC objects with ISPC 1.31.0 and compares hashes
├── docs/DECISIONS.md, docs/CREDITS.md, docs/superpowers/…
├── crates/
│   ├── encoders-sys/                safe wrappers over bc7e, rgbcx, ISPC TC
│   │   ├── build.rs                 cc: shim + rgbcx + ispc_texcomp; links ISPC objects
│   │   ├── shim/sdc_shim.cpp        extern "C" functions; owns the encoder parameter tables
│   │   ├── vendor/                  bc7enc_rdo/, ispctc/, bc7e/patches/, ispc-objs/{bc7e,ispctc}/, SHA256SUMS
│   │   ├── src/lib.rs               init(), versions(), gather_blocks()
│   │   ├── src/bc7.rs  src/rgbcx.rs  src/bc6h.rs  src/bc7_fixup.rs  src/ffi.rs
│   │   └── tests/vendor_manifest.rs  tests/encode_smoke.rs
│   ├── engine/
│   │   ├── src/lib.rs               module list + re-exports
│   │   ├── src/image.rs             Rgba32F, SourceInfo
│   │   ├── src/codes.rs             warning-code catalogue (§10), Severity, Warning
│   │   ├── src/config.rs            Profile and every enum of §3.2
│   │   ├── src/presets.rs           PresetFile, ConfigFile, Rule, PresetRegistry, JSON merge, suffix aliases (Task 26a)
│   │   ├── src/discover.rs          InputFile discovery, output and game-relative paths, canonical output names for suffix aliases and exporter vocabulary
│   │   ├── src/group.rs             TextureSet grouping, suffix parsing (incl. preset suffix aliases and the exporter vocabulary, Task 27a), IN-DUPLICATE
│   │   ├── src/load.rs              PNG/TGA/TIFF → Rgba32F
│   │   ├── src/analyse.rs           Analysis statistics (§5.1)
│   │   ├── src/suggest.rs           §4.3 suggestions with confidence
│   │   ├── src/overrides.rs         --set parsing (scoped field overrides)
│   │   ├── src/policy.rs            Resolve: layering, provenance, availability, checks → ResolvedJob
│   │   ├── src/pack.rs              (milestone M2.5) channel-packing recipes → virtual jobs with several sources
│   │   ├── src/ops.rs               Preprocess (§5.4) → Work
│   │   ├── src/dilate.rs            push-pull RGB dilation under α = 0
│   │   ├── src/mips.rs              float mip chain per domain (§5.5)
│   │   ├── src/coverage.rs          coverage-preserving alpha scaling (§5.5)
│   │   ├── src/quantize.rs          float level → encoder input (u8 RGBA or f16 RGBA)
│   │   ├── src/encode.rs            per-level band-parallel block compression (§5.6)
│   │   ├── src/dds.rs               DDS writer (§5.7) and reader
│   │   ├── src/decode.rs            BCn decode for validation
│   │   ├── src/validate/mod.rs      §5.8 checks; src/validate/alpha.rs decoded-alpha statistics
│   │   ├── src/events.rs            EngineEvent, EventKind, EventSink, Reporter, sinks
│   │   ├── src/manifest.rs          ManifestRecord, Manifest (JSON Lines)
│   │   ├── src/pipeline.rs          process one ResolvedJob end to end
│   │   ├── src/api.rs               Engine, BatchRequest, Plan, BatchSummary, CancelToken, scheduler
│   │   ├── src/sysmem.rs            physical RAM (default memory budget)
│   │   ├── src/schemas.rs           JSON Schema generation for schema/
│   │   ├── build.rs                 embeds the git commit (manifest provenance)
│   │   └── tests/                   per-module integration tests; golden.rs = golden cases (synthetic fixtures)
│   ├── cli/                         sdc.exe: src/main.rs, src/args.rs, src/human.rs, src/verbs.rs; tests/cli_*.rs
│   └── bench/                       sdc-bench: src/{lib,corpus,metrics,codec,ddsraw,gates}.rs, src/main.rs; tests/gates.rs
└── spikes/s4-scalar/                standalone crate (outside workspace members) for spike S4
```

## Interface registry (names every task must use)

| Symbol | Defined in | Signature |
|---|---|---|
| `encoders_sys::Bc1Model` | Task 4 | `enum { Ideal = 0, Nvidia = 1, Amd = 2, IdealRound4 = 3 }` (values = `rgbcx::bc1_approx_mode`) |
| `encoders_sys::init` | Task 4 | `fn init(model: Bc1Model) -> Result<(), InitError>` |
| `encoders_sys::active_bc1_model` | Task 4 | `fn active_bc1_model() -> Option<Bc1Model>` |
| `encoders_sys::versions` | Task 4 | `fn versions() -> Versions` (`bc7enc_rdo`, `ispctc`, `ispc`, `bc7e_patch: Option<&'static str>`) |
| `encoders_sys::gather_blocks` | Task 5 | `fn gather_blocks(px: &[[u8; 4]], width: u32, height: u32) -> Vec<[u8; 4]>` |
| `encoders_sys::Bc7Level`, `Bc7Params` | Task 5 | `enum Bc7Level { Ultrafast, Veryfast, Fast, Basic, Slow, Veryslow, Slowest }`; `struct Bc7Params { level, perceptual: bool, exact_opaque: bool }` |
| `encoders_sys::bc7e_compress_blocks` | Task 5 | `fn bc7e_compress_blocks(pixels: &[[u8; 4]], out: &mut [u8], p: Bc7Params)` |
| `encoders_sys::BC7E_EXACT_OPAQUE_NATIVE` | Task 5 (value changes in Task 14) | `const bool` |
| `encoders_sys::{bc1_encode_block, bc3_encode_block, bc4_encode_block, bc5_encode_block, bc1_decode_block}` | Task 6 | see Task 6 |
| `encoders_sys::{Bc6hLevel, bc6h_compress_rows}` | Task 7 | `fn bc6h_compress_rows(rgba_f16: &[[u16; 4]], width: u32, rows: u32, out: &mut [u8], level: Bc6hLevel)` |
| `encoders_sys::bc7_fixup::force_opaque_mode6` | Task 13 | `fn force_opaque_mode6(block: &mut [u8; 16]) -> bool` |
| `sdc_bench::{corpus, metrics, codec, ddsraw, gates}` | Tasks 8–14, 38 | see tasks |
| `engine::validate::alpha::{AlphaStats, alpha_stats, opaque_block_violations}` | Task 12 | see Task 12 |
| `engine::image::{Rgba32F, SourceInfo}` | Task 18 | see Task 18 |
| `engine::codes::{Code, Severity, Warning}` | Task 18 | see Task 18 |
| `engine::config::*` (`Profile`, enums) | Task 19 | see Task 19 |
| `engine::presets::{PresetFile, ConfigFile, Rule, RuleMatch, AlphaContent, PresetRegistry, merge_json, leaf_paths}` | Task 20 | see Task 20 |
| `engine::discover::{InputFile, discover}` | Task 21 | see Task 21 |
| `engine::group::{TextureSet, split_suffix, group_sets}` | Task 21 | see Task 21 |
| `engine::load::{load_image, LoadError}` | Task 22 | `fn load_image(path: &Path) -> Result<(Rgba32F, SourceInfo), LoadError>` |
| `engine::analyse::{Analysis, analyse}` | Task 23 | `fn analyse(img: &Rgba32F, info: &SourceInfo) -> Analysis` |
| `engine::suggest::{Confidence, Suggestion, SuggestContext, suggest}` | Task 24 | see Task 24 |
| `engine::overrides::{SetOverride, Scope, parse_set}` | Task 25 | see Task 25 |
| `engine::policy::{Provenance, ResolvedJob, JobStatus, ResolveCtx, resolve_file, availability, Availability}` | Tasks 25–26 | see tasks |
| `engine::presets::{PresetFile::suffix_aliases, PresetError::Alias}`, `PresetRegistry::{alias_for, alias_table}` | Task 26a | `fn alias_for(&self, alias: &str) -> Option<(&str, &str)>` (canonical, preset id); `fn alias_table(&self) -> &BTreeMap<String, String>` |
| `engine::group::{SuffixSplit, split_suffix_with, group_sets_with}` | Task 26a | `fn split_suffix_with(stem: &str, aliases: &BTreeMap<String, String>) -> SuffixSplit` |
| `engine::discover::canonicalise_outputs` | Task 26a | `fn canonicalise_outputs(files: &mut [InputFile], aliases: &BTreeMap<String, String>)` |
| `engine::policy::{AliasUse, preset_for}`, `FileFacts::alias`, `ResolvedJob::suffix_alias` | Task 26a | see Task 26a |
| `engine::dilate::dilate_under_zero_alpha` | Task 27 | `fn dilate_under_zero_alpha(img: &mut Rgba32F)` |
| `engine::config::{GreenConvention, RawMap}`, `ChannelSpec::green_convention` | Task 27a | `enum GreenConvention { Directx, Opengl }`; `enum RawMap { Roughness, Metallic, Ao, Specular, Glossiness, Height, Opacity, Fuzz }` with `id()`, `starts_rmaos()`; `green_convention: Option<GreenConvention>` |
| `engine::group::{VocabRole, Vocab, VOCABULARY, split_vocab, vocab_of, is_vocabulary}` | Task 27a | `fn vocab_of(stem: &str) -> Option<&'static Vocab>`; `split_suffix_with` checks the vocabulary after aliases |
| `engine::suggest::apply_vocabulary`, `engine::presets::Provenance::Suffix`, `engine::codes::Code::InPackingInput` | Task 27a | `fn apply_vocabulary(suggestions: &mut [Suggestion], v: &Vocab)`; `suffix:<term>`; `IN-PACKING-INPUT` (warning) |
| `engine::policy::{vocab_for, JobStatus::PackingInput}` | Task 27a | `fn vocab_for(f: &FileFacts) -> Option<&'static Vocab>`; `PackingInput { role: RawMap }` |
| `engine::ops::{Work, Domain, preprocess, srgb_to_linear, linear_to_srgb}` | Task 28 | `fn preprocess(img: Rgba32F, p: &Profile) -> Work` |
| `engine::dds::ResolvedHeader` | Task 33 | `enum { Dx10, Legacy }` |
| `sdc_bench::engine_gates::{engine_parity, full_chain_exact_opaque}` | Task 38 | `fn(root: &Path) -> anyhow::Result<GateReport>` |
| `engine::mips::{level_count, build_chain}` | Task 29 | `fn build_chain(work: &Work, p: &Profile) -> Vec<Rgba32F>` |
| `engine::coverage::{coverage_fraction, preserve_coverage}` | Task 30 | see Task 30 |
| `engine::quantize::{EncoderInput, to_encoder_input}` | Task 31 | `fn to_encoder_input(level: &Rgba32F, work: &Work, p: &Profile) -> EncoderInput` |
| `engine::encode::{EncodeCtx, Cancelled, encode_level}` | Task 32 | `fn encode_level(input: &EncoderInput, w: u32, h: u32, p: &Profile, ctx: &EncodeCtx) -> Result<Vec<u8>, Cancelled>` |
| `engine::dds::{DdsDesc, resolve_header, dxgi_format, dds_bytes, write_atomic, read_dds, DdsFile, DdsPixelFormat, HeaderKind}` | Tasks 33–34 | see tasks |
| `engine::decode::{Decoded, decode_level}` | Task 34 | see Task 34 |
| `engine::validate::{ValidationInput, ValidationReport, validate}` | Task 35 | see Task 35 |
| `engine::events::{EngineEvent, EventKind, EventSink, Reporter, VecSink, JsonLinesSink, Stage}` | Task 36 | see Task 36 |
| `engine::manifest::{ManifestRecord, Manifest}` | Task 36 | see Task 36 |
| `engine::api::{Engine, BatchRequest, Plan, BatchSummary, CancelToken, OnExisting, EngineError, SkippedFile, FailedFile}` | Task 37 | `Engine::plan(&self, BatchRequest, &Reporter) -> Result<Plan, EngineError>`; `Engine::run(&self, Plan, &Reporter, &CancelToken) -> Result<BatchSummary, EngineError>` (spec §2's `EventSink` is wrapped in a `Reporter` that numbers events) |
| `engine::policy::AcceptSuggestions` | Task 25 | `enum { None, High }` |
| `engine::pipeline::{process_file, FileError, FileOutcome}` | Task 37 | see Task 37 |
| `engine::sysmem::total_ram` | Task 37 | `fn total_ram() -> u64` |
| `engine::schemas::{SchemaKind, schema_json, write_all}` | Task 43 | see Task 43 |
| `sdc` CLI modules `args`, `human`, `verbs` | Tasks 40–43 | see tasks |

---
# Phase 0 — Toolchain, vendored encoders and spikes

Phase 0 gate: **Task 9 (S3) must pass before any Phase 1 task starts.** Tasks 10–11 (remaining encoder gates) and 12–14 (S5) must pass before Task 32 (encode stage). Tasks 15 (S4) and 16 (S7) are non-blocking. Task 17 records the results and asks the owner to confirm Rust (DECISIONS #14).

### Task 1: Toolchain install (OWNER APPROVAL, UAC)

**Files:** none in the repository.

**Interfaces:** Produces a machine where `cargo`, `rustc` (host `x86_64-pc-windows-msvc`), `link.exe` and `cl.exe` work from a normal PowerShell.

- [ ] **Step 1: Detect what is installed**

Run (PowerShell):
```powershell
$vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
& $vswhere -all -products * -format text -property installationPath
& $vswhere -all -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
Get-Command rustup, cargo -ErrorAction SilentlyContinue
```
Expected today: the first command lists the VS 2022 Community path (or nothing); the second prints nothing (no C++ tools); `rustup`/`cargo` not found. If the second command already prints a path, skip Step 2. If `cargo` exists, skip Step 3.

- [ ] **Step 2: Install the MSVC C++ toolchain — ASK THE OWNER FIRST (system install, UAC prompt)**

Tell the owner: "Installing the Visual Studio 'Desktop development with C++' workload (MSVC v143, Windows 11 SDK, ~6–8 GB). A UAC prompt will appear." Wait for a clear yes.

If Step 1 listed a VS 2022 installation path `<VSPATH>`:
```powershell
& "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vs_installer.exe" modify --installPath "<VSPATH>" --add Microsoft.VisualStudio.Workload.NativeDesktop --includeRecommended --passive --norestart
```
If no VS 2022 was listed:
```powershell
winget install --id Microsoft.VisualStudio.2022.BuildTools --exact --override "--wait --passive --norestart --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
```
Re-run the second `vswhere` command from Step 1. Expected: it prints an installation path.

- [ ] **Step 3: Install Rust — ASK THE OWNER FIRST (download + install)**

Tell the owner: "Installing rustup (Rust toolchain manager) via winget, then the stable x86_64-pc-windows-msvc toolchain with clippy and rustfmt." Wait for a clear yes.
```powershell
winget install --id Rustlang.Rustup --exact
# open a NEW PowerShell so PATH includes %USERPROFILE%\.cargo\bin
rustup toolchain install stable-x86_64-pc-windows-msvc --component clippy rustfmt
rustup default stable-x86_64-pc-windows-msvc
```

- [ ] **Step 4: Verify linking works**

```powershell
rustc -vV          # Expected: "host: x86_64-pc-windows-msvc"
$p = Join-Path $env:TEMP "sdc-probe"; cargo new --bin $p; cargo run --manifest-path "$p\Cargo.toml"
```
Expected: `Hello, world!`. A `link.exe not found` error means Step 2 did not complete; re-run it. Delete `$p` afterwards (`Remove-Item -Recurse -Force $p`).

No commit (nothing in the repository changes).

---

### Task 2: Workspace skeleton

**Files:**
- Create: `Cargo.toml`, `rust-toolchain.toml`, `.cargo/config.toml`, `.gitignore`, `.gitattributes`
- Create: `crates/encoders-sys/Cargo.toml`, `crates/encoders-sys/src/lib.rs`
- Create: `crates/engine/Cargo.toml`, `crates/engine/src/lib.rs`
- Create: `crates/cli/Cargo.toml`, `crates/cli/src/main.rs`
- Create: `crates/bench/Cargo.toml`, `crates/bench/src/lib.rs`, `crates/bench/src/main.rs`
- Test: `crates/cli/tests/static_crt.rs`

**Interfaces:**
- Produces: packages `encoders-sys` (lib `encoders_sys`), `engine` (lib `engine`), `sdc` (bin `sdc`), `sdc-bench` (lib `sdc_bench`, bin `sdc-bench`).

- [ ] **Step 1: Make sure the repository exists**

Run: `git -C F:/ai/dds/converter rev-parse --is-inside-work-tree`
If it fails, run `git -C F:/ai/dds/converter init -b main` and commit the existing `docs/` (`git add docs && git commit -m "docs: spec, decisions, plan"`). The coordinator normally does this before execution starts.

- [ ] **Step 2: Write the workspace files**

`Cargo.toml`:
```toml
[workspace]
resolver = "2"
members = ["crates/encoders-sys", "crates/engine", "crates/cli", "crates/bench"]
exclude = ["spikes"]

[workspace.package]
version = "0.1.0"
edition = "2021"
license = "GPL-3.0-or-later"
publish = false

[workspace.dependencies]
anyhow = "1"
thiserror = "2"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = "0.8"
schemars = "1"
image = { version = "0.25", default-features = false, features = ["png", "tga", "tiff"] }
half = "2"
bcdec_rs = "0.2"
rayon = "1"
sha2 = "0.10"
hex = "0.4"
globset = "0.4"
walkdir = "2"
tempfile = "3"
clap = { version = "4", features = ["derive"] }
ctrlc = "3"
humantime = "2"
cc = "1"
assert_cmd = "2"
predicates = "3"
encoders-sys = { path = "crates/encoders-sys" }
engine = { path = "crates/engine" }

[profile.dev]
opt-level = 1

[profile.dev.package."*"]
opt-level = 2

[profile.dev.package.encoders-sys]
opt-level = 3

[profile.release]
lto = "thin"
codegen-units = 1
```

`rust-toolchain.toml`:
```toml
[toolchain]
channel = "stable"
targets = ["x86_64-pc-windows-msvc"]
components = ["clippy", "rustfmt"]
```

`.cargo/config.toml`:
```toml
[build]
target = "x86_64-pc-windows-msvc"

[target.x86_64-pc-windows-msvc]
rustflags = ["-C", "target-feature=+crt-static"]
```
Note: the `cc` crate reads `CARGO_CFG_TARGET_FEATURE`, sees `crt-static` and compiles C++ with `/MT`, so the C++ objects and the Rust code share one static CRT.

Because `[build] target` is set, binaries land in `target/x86_64-pc-windows-msvc/<profile>/`. All later paths use that.

`.gitignore`:
```gitignore
/target
/spikes/*/target
_sdc/
*.tmp
```

`.gitattributes` (vendored files must stay byte-identical to their SHA-256 manifest):
```gitattributes
crates/encoders-sys/vendor/** -text
tests/** -text
```

- [ ] **Step 3: Write the four package stubs**

`crates/encoders-sys/Cargo.toml`:
```toml
[package]
name = "encoders-sys"
version.workspace = true
edition.workspace = true
license.workspace = true
publish.workspace = true
build = "build.rs"
links = "sdc_encoders"

[dependencies]
thiserror.workspace = true

[build-dependencies]
cc.workspace = true

[dev-dependencies]
sha2.workspace = true
hex.workspace = true
bcdec_rs.workspace = true
half.workspace = true
```
Create `crates/encoders-sys/build.rs` with just `fn main() {}` for now (Task 4 replaces it), and `crates/encoders-sys/src/lib.rs` with:
```rust
//! Safe wrappers over the vendored BC encoders (bc7e, rgbcx, ISPC TC).
```

`crates/engine/Cargo.toml`:
```toml
[package]
name = "engine"
version.workspace = true
edition.workspace = true
license.workspace = true
publish.workspace = true

[dependencies]
encoders-sys.workspace = true
serde.workspace = true
serde_json.workspace = true
thiserror.workspace = true
```
`crates/engine/src/lib.rs`:
```rust
//! sdc engine: configuration, policy, pipeline, DDS I/O, validation, events.
```

`crates/cli/Cargo.toml`:
```toml
[package]
name = "sdc"
version.workspace = true
edition.workspace = true
license.workspace = true
publish.workspace = true

[[bin]]
name = "sdc"
path = "src/main.rs"

[dependencies]
engine.workspace = true

[dev-dependencies]
assert_cmd.workspace = true
predicates.workspace = true
tempfile.workspace = true
```
`crates/cli/src/main.rs` (Task 40 replaces the body):
```rust
fn main() {
    println!("sdc {}", env!("CARGO_PKG_VERSION"));
}
```

`crates/bench/Cargo.toml`:
```toml
[package]
name = "sdc-bench"
version.workspace = true
edition.workspace = true
license.workspace = true
publish.workspace = true

[lib]
name = "sdc_bench"
path = "src/lib.rs"

[[bin]]
name = "sdc-bench"
path = "src/main.rs"

[dependencies]
encoders-sys.workspace = true
engine.workspace = true
anyhow.workspace = true
image.workspace = true
half.workspace = true
bcdec_rs.workspace = true
rayon.workspace = true
clap.workspace = true

[dev-dependencies]
tempfile.workspace = true
```
`crates/bench/src/lib.rs`:
```rust
//! sdc-bench: reproduces F:/ai/dds/bench numbers through sdc's encoders and gates them.
```
`crates/bench/src/main.rs`:
```rust
fn main() {
    eprintln!("sdc-bench: no subcommand yet");
}
```

- [ ] **Step 4: Write the failing static-CRT test**

`crates/cli/tests/static_crt.rs`:
```rust
//! The shipped exe must not need the VC++ redistributable (spec §2: static CRT, single exe).
#[test]
fn sdc_exe_does_not_import_vcruntime() {
    let exe = std::fs::read(env!("CARGO_BIN_EXE_sdc")).expect("read sdc.exe");
    let hay = String::from_utf8_lossy(&exe).to_ascii_lowercase();
    assert!(!hay.contains("vcruntime140.dll"), "sdc.exe imports vcruntime140.dll: +crt-static is not in effect");
}
```

- [ ] **Step 5: Run the test**

Run (OWNER APPROVAL: the first build downloads crates from crates.io): `cargo test -p sdc --test static_crt`
Expected: PASS. If it fails with "imports vcruntime140.dll", `.cargo/config.toml` is not being read — run from `F:/ai/dds/converter`.

Before `.cargo/config.toml` exists the test fails; if you want to see it fail, temporarily rename the file, run, and rename it back.

- [ ] **Step 6: Check formatting and lints**

Run: `cargo fmt --all -- --check; cargo clippy --workspace --all-targets -- -D warnings`
Expected: no output, exit 0.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml rust-toolchain.toml .cargo .gitignore .gitattributes crates
git commit -m "build: Cargo workspace skeleton on x86_64-pc-windows-msvc with static CRT"
```

---

### Task 3: Vendor the encoders, licences and the ISPC regeneration script

**Files:**
- Create: `crates/encoders-sys/vendor/bc7enc_rdo/{rgbcx.h, rgbcx.cpp, rgbcx_table4.h, rgbcx_table4_small.h, bc7e.ispc, LICENSE}`
- Create: `crates/encoders-sys/vendor/ispctc/{ispc_texcomp.h, ispc_texcomp.cpp, kernel.ispc, license.txt}`
- Create: `crates/encoders-sys/vendor/ispc-objs/bc7e/{bc7e.obj, bc7e_sse2.obj, bc7e_sse4.obj, bc7e_avx.obj, bc7e_avx2.obj, bc7e_ispc.h}`
- Create: `crates/encoders-sys/vendor/ispc-objs/ispctc/{kernel.obj, kernel_sse2.obj, kernel_sse4.obj, kernel_avx.obj, kernel_avx2.obj, kernel_ispc.h}`
- Create: `crates/encoders-sys/vendor/README-VENDOR.md`, `crates/encoders-sys/vendor/SHA256SUMS`
- Create: `tools/regen-ispc.ps1`, `tools/hash-vendor.ps1`
- Create: `LICENSE`, `LICENSES/Apache-2.0-bc7e.txt`, `LICENSES/bc7enc_rdo-LICENSE.txt`, `LICENSES/ISPCTextureCompressor-license.txt`, `THIRD_PARTY_NOTICES.md`, `docs/CREDITS.md`
- Test: `crates/encoders-sys/tests/vendor_manifest.rs`

**Interfaces:**
- Produces: vendored files at the paths above; `tools/regen-ispc.ps1 [-Ispc <path>] [-Bc7eSource <path>] [-Update]` exits 0 when regenerated objects hash-match the committed ones; `tools/hash-vendor.ps1` rewrites `vendor/SHA256SUMS`.

- [ ] **Step 1: Write the failing manifest test**

`crates/encoders-sys/tests/vendor_manifest.rs`:
```rust
use sha2::{Digest, Sha256};
use std::path::Path;

#[test]
fn every_vendored_file_matches_sha256sums() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("vendor");
    let sums = std::fs::read_to_string(root.join("SHA256SUMS")).expect("vendor/SHA256SUMS missing");
    let mut n = 0;
    for line in sums.lines().filter(|l| !l.trim().is_empty()) {
        let (hash, rel) = line.split_once("  ").expect("format: <sha256>  <relative path>");
        let bytes = std::fs::read(root.join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"));
        assert_eq!(hex::encode(Sha256::digest(&bytes)), hash, "hash mismatch: {rel}");
        n += 1;
    }
    assert!(n >= 22, "expected at least 22 vendored files, found {n}");
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p encoders-sys --test vendor_manifest`
Expected: FAIL with "vendor/SHA256SUMS missing".

- [ ] **Step 3: Copy the vendored files from the bench**

PowerShell:
```powershell
$b = "F:\ai\dds\bench"; $v = "F:\ai\dds\converter\crates\encoders-sys\vendor"
New-Item -ItemType Directory -Force "$v\bc7enc_rdo","$v\ispctc","$v\ispc-objs\bc7e","$v\ispc-objs\ispctc" | Out-Null
foreach ($f in "rgbcx.h","rgbcx.cpp","rgbcx_table4.h","rgbcx_table4_small.h","bc7e.ispc","LICENSE") { Copy-Item "$b\tools\bc7enc_rdo\$f" "$v\bc7enc_rdo\$f" }
foreach ($f in "ispc_texcomp.h","ispc_texcomp.cpp","kernel.ispc") { Copy-Item "$b\tools\ISPCTextureCompressor\ispc_texcomp\$f" "$v\ispctc\$f" }
Copy-Item "$b\tools\ISPCTextureCompressor\license.txt" "$v\ispctc\license.txt"
foreach ($s in "","_sse2","_sse4","_avx","_avx2") {
  Copy-Item "$b\build\bc7enc_rdo\bc7e$s.o" "$v\ispc-objs\bc7e\bc7e$s.obj"
  Copy-Item "$b\build\ispctc\kernel$s.o"   "$v\ispc-objs\ispctc\kernel$s.obj"
}
Copy-Item "$b\build\bc7enc_rdo\bc7e_ispc.h" "$v\ispc-objs\bc7e\bc7e_ispc.h"
Copy-Item "$b\build\ispctc\kernel_ispc.h"   "$v\ispc-objs\ispctc\kernel_ispc.h"
git -C "$b\tools\bc7enc_rdo" rev-parse HEAD                 # Expected: b9438627eef73a1157e84201b6fa6eb2ffd6d9f0
git -C "$b\tools\ISPCTextureCompressor" rev-parse --short HEAD   # Expected: 79ddbc9
git -C "$b\tools\bc7enc_rdo" status --short; git -C "$b\tools\ISPCTextureCompressor" status --short   # Expected: no output (clean)
```
The `.o` files are MSVC-ABI COFF objects (ISPC on Windows emits COFF); renaming to `.obj` only helps `lib.exe`.

- [ ] **Step 4: Write `vendor/README-VENDOR.md`**

```markdown
# Vendored encoder sources and objects

| Path | Upstream | Commit | Licence |
|---|---|---|---|
| bc7enc_rdo/ (rgbcx.*, bc7e.ispc pristine) | https://github.com/richgel999/bc7enc_rdo | b9438627eef73a1157e84201b6fa6eb2ffd6d9f0 | rgbcx: MIT or Unlicense; bc7e.ispc: Apache-2.0 |
| ispctc/ | https://github.com/GameTechDev/ISPCTextureCompressor | 79ddbc9 | MIT |
| ispc-objs/bc7e/ | built from bc7enc_rdo/bc7e.ispc (Task 14: from bc7e/bc7e.ispc, patched) with ISPC 1.31.0: `-O2 --target=sse2,sse4,avx,avx2 --opt=fast-math --opt=disable-assertions` | — | Apache-2.0 |
| ispc-objs/ispctc/ | built from ispctc/kernel.ispc with ISPC 1.31.0: `-O2 --target=sse2,sse4,avx,avx2 --opt=fast-math` | — | MIT |

Objects were first copied from F:/ai/dds/bench/build (the exact objects the bench measured).
Regenerate only with `tools/regen-ispc.ps1`; then run `tools/hash-vendor.ps1`.
```

- [ ] **Step 5: Write `tools/hash-vendor.ps1` and generate the manifest**

```powershell
# Rewrites crates/encoders-sys/vendor/SHA256SUMS ("<sha256>  <relative/path>", sorted, ASCII).
$ErrorActionPreference = "Stop"
$v = (Resolve-Path "$PSScriptRoot\..\crates\encoders-sys\vendor").Path
Get-ChildItem $v -Recurse -File | Where-Object Name -ne "SHA256SUMS" | Sort-Object FullName | ForEach-Object {
  "{0}  {1}" -f (Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLower(), ($_.FullName.Substring($v.Length + 1) -replace '\\', '/')
} | Set-Content -Encoding ascii "$v\SHA256SUMS"
```
Run: `pwsh -File tools/hash-vendor.ps1`

- [ ] **Step 6: Run the manifest test**

Run: `cargo test -p encoders-sys --test vendor_manifest`
Expected: PASS (23 files: 6 + 4 + 6 + 6 + README-VENDOR.md).

- [ ] **Step 7: Write `tools/regen-ispc.ps1`**

```powershell
# Rebuilds the ISPC objects with ISPC 1.31.0 into a temp dir and compares SHA-256 with the vendored objects.
# -Update copies the new objects and main headers into vendor/ispc-objs (then run tools/hash-vendor.ps1).
param(
  [string]$Ispc = "F:\ai\dds\bench\tools\dl_ispc\ispc-v1.31.0-windows\bin\ispc.exe",
  [string]$Bc7eSource = "$PSScriptRoot\..\crates\encoders-sys\vendor\bc7enc_rdo\bc7e.ispc",
  [switch]$Update,
  # with -Update: replace only the bc7e objects (ISPC TC keeps the objects the bench measured)
  [switch]$Bc7eOnly
)
$ErrorActionPreference = "Stop"
$vendor = (Resolve-Path "$PSScriptRoot\..\crates\encoders-sys\vendor").Path
$ver = & $Ispc --version | Out-String
if ($ver -notmatch "1\.31\.0") { throw "ISPC 1.31.0 required; got: $ver" }
$tmp = Join-Path ([IO.Path]::GetTempPath()) ("sdc-ispc-" + [guid]::NewGuid())
New-Item -ItemType Directory "$tmp\bc7e", "$tmp\ispctc" | Out-Null
& $Ispc -O2 $Bc7eSource -o "$tmp\bc7e\bc7e.obj" -h "$tmp\bc7e\bc7e_ispc.h" --target=sse2,sse4,avx,avx2 --opt=fast-math --opt=disable-assertions
if ($LASTEXITCODE) { throw "ispc failed on $Bc7eSource" }
& $Ispc -O2 "$vendor\ispctc\kernel.ispc" -o "$tmp\ispctc\kernel.obj" -h "$tmp\ispctc\kernel_ispc.h" --target=sse2,sse4,avx,avx2 --opt=fast-math
if ($LASTEXITCODE) { throw "ispc failed on kernel.ispc" }
$diff = 0
foreach ($dir in "bc7e", "ispctc") {
  foreach ($o in Get-ChildItem "$tmp\$dir" -Filter *.obj) {
    $old = "$vendor\ispc-objs\$dir\$($o.Name)"
    $same = (Test-Path $old) -and ((Get-FileHash $old).Hash -eq (Get-FileHash $o.FullName).Hash)
    if (-not $same) { $diff++ }
    "{0,-5} {1}/{2}" -f ($(if ($same) { "MATCH" } else { "DIFF" })), $dir, $o.Name
  }
}
if ($Update) {
  Copy-Item "$tmp\bc7e\*.obj", "$tmp\bc7e\bc7e_ispc.h" "$vendor\ispc-objs\bc7e\" -Force
  if (-not $Bc7eOnly) { Copy-Item "$tmp\ispctc\*.obj", "$tmp\ispctc\kernel_ispc.h" "$vendor\ispc-objs\ispctc\" -Force }
  "Updated vendor/ispc-objs. Now run tools/hash-vendor.ps1."
}
Remove-Item -Recurse -Force $tmp
exit $(if ($Update) { 0 } else { $diff })
```

- [ ] **Step 8: Run the regeneration check (informational)**

Run: `pwsh -File tools/regen-ispc.ps1`
Expected: ten `MATCH` lines and exit 0. If any line says `DIFF`, keep the bench objects (they are what the bench measured and what S3 tests) and record "ISPC regeneration is not byte-reproducible: <files>" for Task 17's DECISIONS entry; it does not block S3.

- [ ] **Step 9: Licences, notices and credits — OWNER APPROVAL for the GPL text download**

```powershell
Invoke-WebRequest https://www.gnu.org/licenses/gpl-3.0.txt -OutFile LICENSE
New-Item -ItemType Directory -Force LICENSES | Out-Null
Copy-Item crates\encoders-sys\vendor\bc7enc_rdo\LICENSE LICENSES\bc7enc_rdo-LICENSE.txt
Copy-Item crates\encoders-sys\vendor\ispctc\license.txt LICENSES\ISPCTextureCompressor-license.txt
```
Create `LICENSES/Apache-2.0-bc7e.txt` containing the Apache-2.0 text section of `bc7enc_rdo/LICENSE` (the file states "bc7e.ispc uses the Apache 2.0 license"; if it carries only a pointer, copy the full Apache-2.0 text from `C:\Program Files\Git\usr\share\licenses` if present, otherwise ask the owner to approve fetching `https://www.apache.org/licenses/LICENSE-2.0.txt`).

`THIRD_PARTY_NOTICES.md`:
```markdown
# Third-party notices

sdc is GPL-3.0-or-later. It statically links:

- **bc7e.ispc** (bc7enc_rdo commit b943862), Copyright (C) 2018-2021 Binomial LLC, Apache License 2.0 (LICENSES/Apache-2.0-bc7e.txt).
  sdc ships a modified copy: patch `crates/encoders-sys/vendor/bc7e/patches/0001-exact-opaque-alpha.patch` (see that file's change notice).
- **rgbcx** (bc7enc_rdo commit b943862), Richard Geldreich, Jr., MIT or Public Domain (Unlicense) — sdc uses MIT (LICENSES/bc7enc_rdo-LICENSE.txt).
- **Intel ISPC Texture Compressor** (commit 79ddbc9), Copyright (c) 2016-2019 Intel Corporation, MIT (LICENSES/ISPCTextureCompressor-license.txt).
- **bcdec_rs** (crate), MIT. Other Rust crates: see `cargo about`/`cargo deny` output in release notes.

ISPC objects were compiled with Intel ISPC 1.31.0 (BSD-3-Clause compiler; the objects carry the source files' licences).
NVIDIA Texture Tools is never redistributed or linked; sdc can call a user-installed nvcompress.exe as a separate process.
```

`docs/CREDITS.md`:
```markdown
# Credits

- **AlphaBloom** (tkRiverGamer): the "Auto Coverage" idea that sdc's coverage-preserving mip step rebuilds (no code copied).
- **Ignacio Castaño**: coverage-preserving alpha mipmaps (the method sdc's solver follows).
- **jiayev/TruePBR-Manager**: TruePBR per-slot default formats that sdc's `cs` defaults align with (no code copied; CC BY-NC 4.0 code is not used).
- **Rich Geldreich / Binomial LLC**: bc7e and rgbcx. **Intel**: ISPC Texture Compressor. **iOrange**: bcdec (bcdec_rs port by its authors).
```

- [ ] **Step 10: Commit**

```bash
git add crates/encoders-sys/vendor crates/encoders-sys/tests tools LICENSE LICENSES THIRD_PARTY_NOTICES.md docs/CREDITS.md
git commit -m "vendor: bc7enc_rdo b943862, ISPC TC 79ddbc9, ISPC 1.31.0 objects with SHA-256 manifest"
```

---

### Task 4: `encoders-sys` build script, C++ shim and one-time initialisation

**Files:**
- Create: `crates/encoders-sys/build.rs` (replace stub), `crates/encoders-sys/shim/sdc_shim.cpp`, `crates/encoders-sys/src/ffi.rs`
- Modify: `crates/encoders-sys/src/lib.rs`
- Test: `crates/encoders-sys/tests/init.rs`

**Interfaces:**
- Consumes: vendored files (Task 3).
- Produces:
  - `pub enum Bc1Model { Ideal = 0, Nvidia = 1, Amd = 2, IdealRound4 = 3 }` (`#[repr(i32)]`, `Copy`, `Debug`, `PartialEq`, `Eq`)
  - `pub enum InitError { ModelMismatch { active: Bc1Model, requested: Bc1Model } }`
  - `pub fn init(model: Bc1Model) -> Result<(), InitError>`; `pub fn active_bc1_model() -> Option<Bc1Model>`; `pub(crate) fn ensure_init()` (initialises with `Ideal` if nobody did)
  - `pub struct Versions { pub bc7enc_rdo: &'static str, pub ispctc: &'static str, pub ispc: &'static str, pub bc7e_patch: Option<&'static str> }`; `pub fn versions() -> Versions`
  - cfg `sdc_bc7e_patched`, set by `build.rs` when `bc7e_ispc.h` contains `m_exact_opaque_alpha`.
  - C ABI (in `ffi.rs`): `sdc_init`, `sdc_bc7e_compress`, `sdc_bc1_encode`, `sdc_bc3_encode`, `sdc_bc4_encode`, `sdc_bc5_encode`, `sdc_bc1_decode`, `sdc_bc6h_compress`, `sdc_bc7e_patched`.

- [ ] **Step 1: Write the failing test**

`crates/encoders-sys/tests/init.rs`:
```rust
use encoders_sys::{active_bc1_model, init, versions, Bc1Model, InitError};

#[test]
fn init_is_once_and_rejects_a_second_model() {
    assert_eq!(init(Bc1Model::Ideal), Ok(()));
    assert_eq!(init(Bc1Model::Ideal), Ok(()));
    assert_eq!(active_bc1_model(), Some(Bc1Model::Ideal));
    assert_eq!(
        init(Bc1Model::Nvidia),
        Err(InitError::ModelMismatch { active: Bc1Model::Ideal, requested: Bc1Model::Nvidia })
    );
    let v = versions();
    assert_eq!(v.bc7enc_rdo, "b943862");
    assert_eq!(v.ispctc, "79ddbc9");
    assert_eq!(v.ispc, "1.31.0");
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p encoders-sys --test init`
Expected: FAIL to compile ("unresolved imports `encoders_sys::init`…").

- [ ] **Step 3: Write the C++ shim**

`crates/encoders-sys/shim/sdc_shim.cpp`:
```cpp
// C ABI over rgbcx, bc7e (ISPC) and ISPC Texture Compressor for the sdc encoders-sys crate.
// Parameter structs stay on the C++ side, so Rust never mirrors ISPC struct layouts.
#include <cstdint>
#include <cstring>
#include "rgbcx.h"
#include "bc7e_ispc.h"
#include "ispc_texcomp.h"

#ifndef SDC_BC7E_PATCHED
#define SDC_BC7E_PATCHED 0
#endif

namespace {
ispc::bc7e_compress_block_params g_bc7[7][2][2];  // [level u0..u6][perceptual][exact_opaque]
bc6h_enc_settings g_bc6[5];                       // veryfast, fast, basic, slow, veryslow

void init_bc7_level(ispc::bc7e_compress_block_params* p, int level, bool perceptual) {
    switch (level) {
    case 0: ispc::bc7e_compress_block_params_init_ultrafast(p, perceptual); break;
    case 1: ispc::bc7e_compress_block_params_init_veryfast(p, perceptual); break;
    case 2: ispc::bc7e_compress_block_params_init_fast(p, perceptual); break;
    case 3: ispc::bc7e_compress_block_params_init_basic(p, perceptual); break;
    case 4: ispc::bc7e_compress_block_params_init_slow(p, perceptual); break;
    case 5: ispc::bc7e_compress_block_params_init_veryslow(p, perceptual); break;
    default: ispc::bc7e_compress_block_params_init_slowest(p, perceptual); break;
    }
}
}  // namespace

extern "C" {

// Must run exactly once (Rust guards it with OnceLock): rgbcx::init writes global state.
void sdc_init(int bc1_mode) {
    rgbcx::init(static_cast<rgbcx::bc1_approx_mode>(bc1_mode));
    ispc::bc7e_compress_block_init();
    for (int l = 0; l < 7; l++)
        for (int s = 0; s < 2; s++)
            for (int e = 0; e < 2; e++) {
                ispc::bc7e_compress_block_params* p = &g_bc7[l][s][e];
                std::memset(p, 0, sizeof(*p));  // rdo_bc_encoder.cpp does the same before init
                init_bc7_level(p, l, s != 0);
#if SDC_BC7E_PATCHED
                p->m_exact_opaque_alpha = (e != 0);
#endif
            }
    GetProfile_bc6h_veryfast(&g_bc6[0]);
    GetProfile_bc6h_fast(&g_bc6[1]);
    GetProfile_bc6h_basic(&g_bc6[2]);
    GetProfile_bc6h_slow(&g_bc6[3]);
    GetProfile_bc6h_veryslow(&g_bc6[4]);
}

void sdc_bc7e_compress(uint32_t n, uint64_t* blocks, const uint32_t* px, int level, int perceptual, int exact) {
    ispc::bc7e_compress_blocks(n, blocks, px, &g_bc7[level][perceptual ? 1 : 0][exact ? 1 : 0]);
}

void sdc_bc1_encode(uint8_t* dst, const uint8_t* px, uint32_t level, int allow3, int black) {
    rgbcx::encode_bc1(level, dst, px, allow3 != 0, black != 0);
}

void sdc_bc3_encode(uint8_t* dst, const uint8_t* px, uint32_t level, uint32_t rad) {
    rgbcx::encode_bc3_hq(level, dst, px, rad, rgbcx::BC4_USE_ALL_MODES);
}

void sdc_bc4_encode(uint8_t* dst, const uint8_t* px, uint32_t stride, uint32_t rad) {
    rgbcx::encode_bc4_hq(dst, px, stride, rad, rgbcx::BC4_USE_ALL_MODES);
}

void sdc_bc5_encode(uint8_t* dst, const uint8_t* px, uint32_t rad) {
    rgbcx::encode_bc5_hq(dst, px, 0, 1, 4, rad, rgbcx::BC4_USE_ALL_MODES);
}

int sdc_bc1_decode(const uint8_t* src, uint8_t* px, int mode) {
    return rgbcx::unpack_bc1(src, px, true, static_cast<rgbcx::bc1_approx_mode>(mode)) ? 1 : 0;
}

void sdc_bc6h_compress(const uint16_t* rgba16f, int32_t width, int32_t rows, int32_t stride_bytes, uint8_t* dst, int level) {
    rgba_surface s;
    s.ptr = reinterpret_cast<uint8_t*>(const_cast<uint16_t*>(rgba16f));
    s.width = width;
    s.height = rows;
    s.stride = stride_bytes;
    CompressBlocksBC6H(&s, dst, &g_bc6[level]);
}

int sdc_bc7e_patched(void) { return SDC_BC7E_PATCHED; }

}  // extern "C"
```

- [ ] **Step 4: Write `build.rs`**

```rust
use std::path::PathBuf;

fn main() {
    let root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let v = root.join("vendor");
    let bc7e = v.join("ispc-objs/bc7e");
    let tc = v.join("ispc-objs/ispctc");

    let header = std::fs::read_to_string(bc7e.join("bc7e_ispc.h")).expect("vendor/ispc-objs/bc7e/bc7e_ispc.h");
    let patched = header.contains("m_exact_opaque_alpha");
    println!("cargo::rustc-check-cfg=cfg(sdc_bc7e_patched)");
    if patched {
        println!("cargo::rustc-cfg=sdc_bc7e_patched");
    }

    let mut b = cc::Build::new();
    b.cpp(true)
        .std("c++17")
        .flag("/EHsc")
        .define("NOMINMAX", None)
        .define("SDC_BC7E_PATCHED", if patched { "1" } else { "0" })
        .include(v.join("bc7enc_rdo"))
        .include(v.join("ispctc"))
        .include(&bc7e)
        .include(&tc)
        .file(root.join("shim/sdc_shim.cpp"))
        .file(v.join("bc7enc_rdo/rgbcx.cpp"))
        .file(v.join("ispctc/ispc_texcomp.cpp"));
    for s in ["", "_sse2", "_sse4", "_avx", "_avx2"] {
        b.object(bc7e.join(format!("bc7e{s}.obj")));
        b.object(tc.join(format!("kernel{s}.obj")));
    }
    b.compile("sdc_encoders");

    println!("cargo::rerun-if-changed=shim/sdc_shim.cpp");
    println!("cargo::rerun-if-changed=vendor");
}
```

- [ ] **Step 5: Write `ffi.rs` and the init/versions part of `lib.rs`**

`crates/encoders-sys/src/ffi.rs`:
```rust
//! Raw C ABI of shim/sdc_shim.cpp. Only this crate calls these.
extern "C" {
    pub fn sdc_init(bc1_mode: i32);
    pub fn sdc_bc7e_compress(n: u32, blocks: *mut u64, px: *const u32, level: i32, perceptual: i32, exact: i32);
    pub fn sdc_bc1_encode(dst: *mut u8, px: *const u8, level: u32, allow3: i32, black: i32);
    pub fn sdc_bc3_encode(dst: *mut u8, px: *const u8, level: u32, rad: u32);
    pub fn sdc_bc4_encode(dst: *mut u8, px: *const u8, stride: u32, rad: u32);
    pub fn sdc_bc5_encode(dst: *mut u8, px: *const u8, rad: u32);
    pub fn sdc_bc1_decode(src: *const u8, px: *mut u8, mode: i32) -> i32;
    pub fn sdc_bc6h_compress(rgba16f: *const u16, width: i32, rows: i32, stride_bytes: i32, dst: *mut u8, level: i32);
    pub fn sdc_bc7e_patched() -> i32;
}
```

`crates/encoders-sys/src/lib.rs`:
```rust
//! Safe wrappers over the vendored BC encoders (bc7e, rgbcx, ISPC TC).
mod ffi;

use std::sync::OnceLock;

/// rgbcx BC1 decoder model; values equal `rgbcx::bc1_approx_mode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum Bc1Model {
    Ideal = 0,
    Nvidia = 1,
    Amd = 2,
    IdealRound4 = 3,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InitError {
    #[error("encoders already initialised with BC1 model {active:?}; {requested:?} needs a new process (rgbcx::init is global)")]
    ModelMismatch { active: Bc1Model, requested: Bc1Model },
}

static ACTIVE: OnceLock<Bc1Model> = OnceLock::new();

/// Initialise rgbcx and bc7e once per process. Later calls succeed only with the same model.
pub fn init(model: Bc1Model) -> Result<(), InitError> {
    let active = *ACTIVE.get_or_init(|| {
        unsafe { ffi::sdc_init(model as i32) };
        model
    });
    if active == model {
        Ok(())
    } else {
        Err(InitError::ModelMismatch { active, requested: model })
    }
}

pub fn active_bc1_model() -> Option<Bc1Model> {
    ACTIVE.get().copied()
}

pub(crate) fn ensure_init() {
    let _ = ACTIVE.get_or_init(|| {
        unsafe { ffi::sdc_init(Bc1Model::Ideal as i32) };
        Bc1Model::Ideal
    });
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Versions {
    pub bc7enc_rdo: &'static str,
    pub ispctc: &'static str,
    pub ispc: &'static str,
    pub bc7e_patch: Option<&'static str>,
}

pub fn versions() -> Versions {
    Versions {
        bc7enc_rdo: "b943862",
        ispctc: "79ddbc9",
        ispc: "1.31.0",
        bc7e_patch: if cfg!(sdc_bc7e_patched) { Some("0001-exact-opaque-alpha") } else { None },
    }
}
```
Add `thiserror.workspace = true` is already in `[dependencies]` (Task 2).

- [ ] **Step 6: Run the test**

Run: `cargo test -p encoders-sys --test init`
Expected: PASS. If linking fails with unresolved `__chkstk`/`_fltused`, the build is not using the MSVC linker — check `rustc -vV` host. If it fails with LNK1107 on a `.obj`, check that Task 3 copied binary-identical files (`cargo test -p encoders-sys --test vendor_manifest`).

- [ ] **Step 7: Confirm no ISPC is needed at build time**

Run: `Get-Command ispc -ErrorAction SilentlyContinue; cargo clean -p encoders-sys; cargo build -p encoders-sys`
Expected: `Get-Command` prints nothing (ISPC not on PATH) and the build succeeds (S3 criterion "builds without ISPC installed").

- [ ] **Step 8: Commit**

```bash
git add crates/encoders-sys
git commit -m "feat(encoders-sys): build rgbcx, ISPC TC host and C shim with MSVC; link ISPC objects; once-only init"
```

---

### Task 5: BC7 (bc7e) wrapper and block gathering

**Files:**
- Create: `crates/encoders-sys/src/bc7.rs`
- Modify: `crates/encoders-sys/src/lib.rs` (add `mod bc7; pub use bc7::*;` and `gather_blocks`)
- Test: `crates/encoders-sys/tests/bc7.rs`

**Interfaces:**
- Consumes: `ffi::sdc_bc7e_compress`, `ensure_init` (Task 4).
- Produces:
  - `pub fn gather_blocks(px: &[[u8; 4]], width: u32, height: u32) -> Vec<[u8; 4]>` — row-major blocks, 16 texels each in row-major order, edge texels replicated where the image does not fill a block.
  - `pub enum Bc7Level { Ultrafast = 0, Veryfast = 1, Fast = 2, Basic = 3, Slow = 4, Veryslow = 5, Slowest = 6 }` (`Copy`, `Debug`, `PartialEq`, `Eq`)
  - `pub struct Bc7Params { pub level: Bc7Level, pub perceptual: bool, pub exact_opaque: bool }` (`Copy`)
  - `pub const BC7E_EXACT_OPAQUE_NATIVE: bool` (= `cfg!(sdc_bc7e_patched)`)
  - `pub fn bc7e_compress_blocks(pixels: &[[u8; 4]], out: &mut [u8], p: Bc7Params)` — `pixels.len() == 16 * n`, `out.len() == 16 * n`; panics if `p.exact_opaque && !BC7E_EXACT_OPAQUE_NATIVE`.

- [ ] **Step 1: Write the failing tests**

`crates/encoders-sys/tests/bc7.rs`:
```rust
use encoders_sys::{bc7e_compress_blocks, gather_blocks, Bc7Level, Bc7Params};

const U5: Bc7Params = Bc7Params { level: Bc7Level::Veryslow, perceptual: false, exact_opaque: false };

fn decode(block: &[u8]) -> Vec<[u8; 4]> {
    let mut out = [0u8; 64];
    bcdec_rs::bc7(block, &mut out, 16);
    out.chunks(4).map(|c| [c[0], c[1], c[2], c[3]]).collect()
}

#[test]
fn gather_blocks_orders_blocks_row_major_and_replicates_edges() {
    // 5x3 image, texel value = (x, y, 0, 255)
    let px: Vec<[u8; 4]> = (0..3).flat_map(|y| (0..5).map(move |x| [x as u8, y as u8, 0, 255])).collect();
    let b = gather_blocks(&px, 5, 3);
    assert_eq!(b.len(), 2 * 16); // 2x1 blocks
    assert_eq!(b[0], [0, 0, 0, 255]);
    assert_eq!(b[5], [1, 1, 0, 255]); // block 0, row 1, col 1
    assert_eq!(b[15], [3, 2, 0, 255]); // block 0, row 3 replicates row 2
    assert_eq!(b[16], [4, 0, 0, 255]); // block 1 starts at x = 4
    assert_eq!(b[17], [4, 0, 0, 255]); // x = 5 replicates x = 4
}

#[test]
fn solid_block_round_trips_exactly() {
    let px = vec![[200u8, 100, 50, 255]; 16];
    let mut out = [0u8; 16];
    bc7e_compress_blocks(&px, &mut out, U5);
    assert!(decode(&out).iter().all(|p| *p == [200, 100, 50, 255]));
}

#[test]
fn opaque_gradient_decodes_alpha_at_least_254() {
    let px: Vec<[u8; 4]> = (0..16).map(|i| [i as u8 * 16, 255 - i as u8 * 10, 7 * i as u8, 255]).collect();
    let mut out = [0u8; 16];
    bc7e_compress_blocks(&px, &mut out, U5);
    assert!(decode(&out).iter().all(|p| p[3] >= 254));
}

#[test]
#[should_panic(expected = "16 bytes per block")]
fn rejects_mismatched_output_length() {
    let px = vec![[0u8; 4]; 16];
    let mut out = [0u8; 8];
    bc7e_compress_blocks(&px, &mut out, U5);
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p encoders-sys --test bc7`
Expected: FAIL to compile (unresolved `gather_blocks`, `bc7e_compress_blocks`).

- [ ] **Step 3: Implement**

`crates/encoders-sys/src/bc7.rs`:
```rust
use crate::{ensure_init, ffi};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Bc7Level {
    Ultrafast = 0,
    Veryfast = 1,
    Fast = 2,
    Basic = 3,
    Slow = 4,
    Veryslow = 5,
    Slowest = 6,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bc7Params {
    pub level: Bc7Level,
    pub perceptual: bool,
    /// Force mode-6 p-bits to 1 on all-255 blocks (needs the patched bc7e objects, Task 14).
    pub exact_opaque: bool,
}

/// True once the vendored bc7e objects carry patch 0001 (exact opaque alpha).
pub const BC7E_EXACT_OPAQUE_NATIVE: bool = cfg!(sdc_bc7e_patched);

/// Compress `pixels.len() / 16` blocks. `pixels` holds 16 texels per block (see `gather_blocks`).
pub fn bc7e_compress_blocks(pixels: &[[u8; 4]], out: &mut [u8], p: Bc7Params) {
    assert_eq!(pixels.len() % 16, 0, "pixels must hold whole 4x4 blocks");
    let n = pixels.len() / 16;
    assert_eq!(out.len(), n * 16, "out must hold 16 bytes per block");
    assert!(
        !p.exact_opaque || BC7E_EXACT_OPAQUE_NATIVE,
        "exact_opaque needs the patched bc7e objects; use bc7_fixup::force_opaque_mode6 instead"
    );
    if n == 0 {
        return;
    }
    ensure_init();
    // Copy into u32/u64 buffers so the ISPC code sees naturally aligned data.
    let src: Vec<u32> = pixels.iter().map(|c| u32::from_le_bytes(*c)).collect();
    let mut dst = vec![0u64; n * 2];
    unsafe {
        ffi::sdc_bc7e_compress(
            n as u32,
            dst.as_mut_ptr(),
            src.as_ptr(),
            p.level as i32,
            p.perceptual as i32,
            p.exact_opaque as i32,
        )
    };
    for (i, w) in dst.iter().enumerate() {
        out[i * 8..i * 8 + 8].copy_from_slice(&w.to_le_bytes());
    }
}
```

Add to `crates/encoders-sys/src/lib.rs`:
```rust
mod bc7;
pub use bc7::{bc7e_compress_blocks, Bc7Level, Bc7Params, BC7E_EXACT_OPAQUE_NATIVE};

/// Re-arrange a row-major RGBA8 image into 4x4 blocks (row-major blocks, row-major texels inside a block).
/// Texels past the right or bottom edge replicate the nearest edge texel.
pub fn gather_blocks(px: &[[u8; 4]], width: u32, height: u32) -> Vec<[u8; 4]> {
    let (w, h) = (width as usize, height as usize);
    assert!(w > 0 && h > 0, "empty image");
    assert_eq!(px.len(), w * h, "pixel count does not match width*height");
    let (bw, bh) = (w.div_ceil(4), h.div_ceil(4));
    let mut out = Vec::with_capacity(bw * bh * 16);
    for by in 0..bh {
        for bx in 0..bw {
            for y in 0..4 {
                let sy = (by * 4 + y).min(h - 1);
                for x in 0..4 {
                    let sx = (bx * 4 + x).min(w - 1);
                    out.push(px[sy * w + sx]);
                }
            }
        }
    }
    out
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p encoders-sys --test bc7`
Expected: 4 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/encoders-sys
git commit -m "feat(encoders-sys): bc7e block compression wrapper and 4x4 block gathering"
```

---

### Task 6: rgbcx wrappers (BC1, BC3, BC4, BC5 encode; BC1 decode per decoder model)

**Files:**
- Create: `crates/encoders-sys/src/rgbcx.rs`
- Modify: `crates/encoders-sys/src/lib.rs` (`mod rgbcx; pub use rgbcx::*;`)
- Test: `crates/encoders-sys/tests/rgbcx.rs`

**Interfaces:**
- Consumes: `ffi`, `ensure_init`, `Bc1Model` (Task 4).
- Produces:
  - `pub const RGBCX_MAX_LEVEL: u32 = 18;` `pub const BC45_RADIUS_DEFAULT: u32 = 5;` `pub const BC45_RADIUS_MAX: u32 = 10;`
  - `pub fn bc1_encode_block(px: &[[u8; 4]; 16], level: u32, allow_3color: bool, transparent_black: bool) -> [u8; 8]`
  - `pub fn bc3_encode_block(px: &[[u8; 4]; 16], level: u32, search_radius: u32) -> [u8; 16]`
  - `pub fn bc4_encode_block(px: &[[u8; 4]; 16], channel: usize, search_radius: u32) -> [u8; 8]`
  - `pub fn bc5_encode_block(px: &[[u8; 4]; 16], search_radius: u32) -> [u8; 16]` (channels 0 and 1)
  - `pub fn bc1_decode_block(block: &[u8; 8], model: Bc1Model) -> [[u8; 4]; 16]`

- [ ] **Step 1: Write the failing tests**

`crates/encoders-sys/tests/rgbcx.rs`:
```rust
use encoders_sys::*;

fn ramp_block() -> [[u8; 4]; 16] {
    let mut b = [[0u8; 4]; 16];
    for (i, p) in b.iter_mut().enumerate() {
        *p = [(i * 17) as u8, (255 - i * 17) as u8, (i * 5) as u8, (i * 16) as u8];
    }
    b
}

#[test]
fn bc1_with_transparent_black_off_always_decodes_opaque() {
    // Black texels are what tempt rgbcx into 3-colour transparent-black mode.
    let mut b = ramp_block();
    for p in b.iter_mut().take(6) {
        *p = [0, 0, 0, 255];
    }
    let enc = bc1_encode_block(&b, RGBCX_MAX_LEVEL, true, false);
    for model in [Bc1Model::Ideal, Bc1Model::Nvidia, Bc1Model::Amd, Bc1Model::IdealRound4] {
        assert!(bc1_decode_block(&enc, model).iter().all(|p| p[3] == 255), "{model:?}");
    }
}

#[test]
fn bc4_ramp_is_close() {
    let b = ramp_block();
    let enc = bc4_encode_block(&b, 0, BC45_RADIUS_DEFAULT);
    let mut dec = [0u8; 16];
    bcdec_rs::bc4(&enc, &mut dec, 4, false);
    for (i, d) in dec.iter().enumerate() {
        assert!((*d as i32 - b[i][0] as i32).abs() <= 10, "texel {i}: {d} vs {}", b[i][0]);
    }
}

#[test]
fn bc5_encodes_red_and_green() {
    let b = ramp_block();
    let enc = bc5_encode_block(&b, BC45_RADIUS_DEFAULT);
    let mut dec = [0u8; 32];
    bcdec_rs::bc5(&enc, &mut dec, 8, false);
    for i in 0..16 {
        assert!((dec[i * 2] as i32 - b[i][0] as i32).abs() <= 10);
        assert!((dec[i * 2 + 1] as i32 - b[i][1] as i32).abs() <= 10);
    }
}

#[test]
fn bc3_keeps_alpha() {
    let b = ramp_block();
    let enc = bc3_encode_block(&b, RGBCX_MAX_LEVEL, BC45_RADIUS_DEFAULT);
    let mut dec = [0u8; 64];
    bcdec_rs::bc3(&enc, &mut dec, 16);
    for i in 0..16 {
        assert!((dec[i * 4 + 3] as i32 - b[i][3] as i32).abs() <= 10);
    }
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p encoders-sys --test rgbcx`
Expected: FAIL to compile (unresolved names).

- [ ] **Step 3: Implement**

`crates/encoders-sys/src/rgbcx.rs`:
```rust
use crate::{ensure_init, ffi, Bc1Model};

pub const RGBCX_MAX_LEVEL: u32 = 18;
/// Search radius the bench used for `-3/-4/-5` (rdo_bc_params::m_bc345_search_rad = 5). Spec value `default`.
pub const BC45_RADIUS_DEFAULT: u32 = 5;
/// Spec value `max` (bench `-hr10`).
pub const BC45_RADIUS_MAX: u32 = 10;

fn bytes(px: &[[u8; 4]; 16]) -> [u8; 64] {
    let mut b = [0u8; 64];
    for (i, p) in px.iter().enumerate() {
        b[i * 4..i * 4 + 4].copy_from_slice(p);
    }
    b
}

pub fn bc1_encode_block(px: &[[u8; 4]; 16], level: u32, allow_3color: bool, transparent_black: bool) -> [u8; 8] {
    ensure_init();
    let src = bytes(px);
    let mut out = [0u8; 8];
    unsafe { ffi::sdc_bc1_encode(out.as_mut_ptr(), src.as_ptr(), level, allow_3color as i32, transparent_black as i32) };
    out
}

pub fn bc3_encode_block(px: &[[u8; 4]; 16], level: u32, search_radius: u32) -> [u8; 16] {
    ensure_init();
    let src = bytes(px);
    let mut out = [0u8; 16];
    unsafe { ffi::sdc_bc3_encode(out.as_mut_ptr(), src.as_ptr(), level, search_radius) };
    out
}

pub fn bc4_encode_block(px: &[[u8; 4]; 16], channel: usize, search_radius: u32) -> [u8; 8] {
    assert!(channel < 4);
    ensure_init();
    let src = bytes(px);
    let mut out = [0u8; 8];
    unsafe { ffi::sdc_bc4_encode(out.as_mut_ptr(), src.as_ptr().add(channel), 4, search_radius) };
    out
}

pub fn bc5_encode_block(px: &[[u8; 4]; 16], search_radius: u32) -> [u8; 16] {
    ensure_init();
    let src = bytes(px);
    let mut out = [0u8; 16];
    unsafe { ffi::sdc_bc5_encode(out.as_mut_ptr(), src.as_ptr(), search_radius) };
    out
}

/// Decode one BC1 block with rgbcx under the given decoder model (validation and vendor previews).
pub fn bc1_decode_block(block: &[u8; 8], model: Bc1Model) -> [[u8; 4]; 16] {
    ensure_init();
    let mut px = [0u8; 64];
    unsafe { ffi::sdc_bc1_decode(block.as_ptr(), px.as_mut_ptr(), model as i32) };
    let mut out = [[0u8; 4]; 16];
    for (i, o) in out.iter_mut().enumerate() {
        o.copy_from_slice(&px[i * 4..i * 4 + 4]);
    }
    out
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p encoders-sys --test rgbcx`
Expected: 4 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/encoders-sys
git commit -m "feat(encoders-sys): rgbcx BC1/BC3/BC4/BC5 encode and BC1 decode wrappers"
```

---

### Task 7: BC6H (ISPC TC) wrapper

**Files:**
- Create: `crates/encoders-sys/src/bc6h.rs`
- Modify: `crates/encoders-sys/src/lib.rs` (`mod bc6h; pub use bc6h::*;`)
- Test: `crates/encoders-sys/tests/bc6h.rs`

**Interfaces:**
- Produces: `pub enum Bc6hLevel { Veryfast = 0, Fast = 1, Basic = 2, Slow = 3, Veryslow = 4 }`; `pub fn bc6h_compress_rows(rgba_f16: &[[u16; 4]], width: u32, rows: u32, out: &mut [u8], level: Bc6hLevel)` — `width % 4 == 0`, `rows % 4 == 0`, `rgba_f16.len() == width * rows` (row-major, raw IEEE half bits), `out.len() == (width/4) * (rows/4) * 16`, blocks in raster order.

- [ ] **Step 1: Write the failing test**

`crates/encoders-sys/tests/bc6h.rs`:
```rust
use encoders_sys::{bc6h_compress_rows, Bc6hLevel};
use half::f16;

#[test]
fn constant_hdr_block_round_trips_within_two_percent() {
    let c = [1.5f32, 0.25, 0.0];
    let px = vec![[f16::from_f32(c[0]).to_bits(), f16::from_f32(c[1]).to_bits(), f16::from_f32(c[2]).to_bits(), f16::ONE.to_bits()]; 16];
    let mut out = [0u8; 16];
    bc6h_compress_rows(&px, 4, 4, &mut out, Bc6hLevel::Veryslow);
    let mut dec = [0f32; 48];
    bcdec_rs::bc6h_float(&out, &mut dec, 12, false);
    for t in 0..16 {
        for ch in 0..3 {
            let d = dec[t * 3 + ch];
            assert!((d - c[ch]).abs() <= 0.02 * c[ch].max(0.05), "texel {t} ch {ch}: {d}");
        }
    }
}

#[test]
#[should_panic(expected = "multiple of 4")]
fn rejects_unaligned_width() {
    let px = vec![[0u16; 4]; 6 * 4];
    let mut out = [0u8; 32];
    bc6h_compress_rows(&px, 6, 4, &mut out, Bc6hLevel::Veryslow);
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p encoders-sys --test bc6h`
Expected: FAIL to compile.

- [ ] **Step 3: Implement**

`crates/encoders-sys/src/bc6h.rs`:
```rust
use crate::{ensure_init, ffi};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Bc6hLevel {
    Veryfast = 0,
    Fast = 1,
    Basic = 2,
    Slow = 3,
    Veryslow = 4,
}

/// BC6H UF16 compression of a strip of `rows` rows (a multiple of 4) of a `width`-wide RGBA16F image.
pub fn bc6h_compress_rows(rgba_f16: &[[u16; 4]], width: u32, rows: u32, out: &mut [u8], level: Bc6hLevel) {
    assert!(width > 0 && width % 4 == 0 && rows % 4 == 0, "width and rows must be a multiple of 4");
    assert_eq!(rgba_f16.len(), (width * rows) as usize, "pixel count does not match width*rows");
    assert_eq!(out.len(), (width / 4 * (rows / 4) * 16) as usize, "out must hold 16 bytes per block");
    if rows == 0 {
        return;
    }
    ensure_init();
    unsafe {
        ffi::sdc_bc6h_compress(
            rgba_f16.as_ptr() as *const u16,
            width as i32,
            rows as i32,
            (width * 8) as i32,
            out.as_mut_ptr(),
            level as i32,
        )
    };
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p encoders-sys --test bc6h`
Expected: 2 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/encoders-sys
git commit -m "feat(encoders-sys): ISPC TC BC6H UF16 strip compression wrapper"
```

---

### Task 8: Bench harness basics (corpus access, metrics identical to the Python bench, codecs)

**Files:**
- Create: `crates/bench/src/corpus.rs`, `crates/bench/src/metrics.rs`, `crates/bench/src/codec.rs`, `crates/bench/src/ddsraw.rs`
- Modify: `crates/bench/src/lib.rs`
- Test: `crates/bench/tests/basics.rs`

**Interfaces:**
- Produces:
  - `corpus::bench_root() -> PathBuf` (`SDC_BENCH_ROOT` or `F:\ai\dds\bench`)
  - `corpus::Rgba8 { width: u32, height: u32, px: Vec<[u8; 4]> }`; `corpus::load_png_rgba8(path: &Path) -> anyhow::Result<Rgba8>`
  - `corpus::HalfImage { width: u32, height: u32, px: Vec<[u16; 4]> }`; `corpus::load_f16(path: &Path) -> anyhow::Result<HalfImage>`
  - `corpus::set_files(root: &Path, dir: &str, set: &str, ext: &str) -> anyhow::Result<Vec<PathBuf>>` (sorted)
  - `corpus::ResultRow { variant, set, image: String, values: BTreeMap<String, String> }` with `fn f(&self, key: &str) -> Option<f64>`; `corpus::read_results(path: &Path, variant: &str) -> anyhow::Result<Vec<ResultRow>>`; `corpus::find<'a>(rows: &'a [ResultRow], set: &str, image: &str) -> Option<&'a ResultRow>`; `corpus::stem(p: &Path) -> String`
  - `metrics::psnr_from_mse(mse: f64) -> f64`; `metrics::channel_psnr(src: &[[u8; 4]], dec: &[[u8; 4]], ch: &[usize]) -> f64`; `metrics::rgb_psnr`, `metrics::a_psnr` (same args); `metrics::mpsnr(reference: &[[f32; 3]], decoded: &[[f32; 3]]) -> f64`; `metrics::mean(v: &[f64]) -> f64`
  - `codec::encode_bc7_image(img: &Rgba8, p: Bc7Params) -> Vec<u8>`; `codec::RgbcxKind { Bc1 { transparent_black: bool }, Bc3, Bc4, Bc5 }`; `codec::encode_rgbcx_image(img: &Rgba8, kind: RgbcxKind) -> Vec<u8>`; `codec::encode_bc6h_image(img: &HalfImage) -> Vec<u8>`; `codec::decode_bc7_image(data: &[u8], w: u32, h: u32) -> Vec<[u8; 4]>`; `codec::decode_bc6h_image(data: &[u8], w: u32, h: u32) -> Vec<[f32; 3]>`; `codec::half_to_rgb_f32(img: &HalfImage) -> Vec<[f32; 3]>`
  - `ddsraw::payload(bytes: &[u8]) -> anyhow::Result<&[u8]>` (skips the 128-byte header, or 148 with `DX10`)

- [ ] **Step 1: Write the failing tests**

`crates/bench/tests/basics.rs`:
```rust
use sdc_bench::{corpus, ddsraw, metrics};

#[test]
fn psnr_matches_the_python_formula() {
    assert_eq!(metrics::psnr_from_mse(0.0), 99.0);
    assert!((metrics::psnr_from_mse(1.0) - 48.130804).abs() < 1e-5);
    let a = vec![[10u8, 20, 30, 255]; 4];
    let mut b = a.clone();
    b[0][0] = 12; // one channel off by 2 over 12 samples -> mse = 4/12
    assert!((metrics::rgb_psnr(&a, &b) - metrics::psnr_from_mse(4.0 / 12.0)).abs() < 1e-9);
    assert_eq!(metrics::a_psnr(&a, &b), 99.0);
}

#[test]
fn mpsnr_of_identical_images_is_99() {
    let r = vec![[0.5f32, 2.0, 10.0]; 16];
    assert_eq!(metrics::mpsnr(&r, &r), 99.0);
}

#[test]
fn ddsraw_skips_legacy_and_dx10_headers() {
    let mut legacy = vec![0u8; 128 + 8];
    legacy[..4].copy_from_slice(b"DDS ");
    legacy[84..88].copy_from_slice(b"DXT1");
    assert_eq!(ddsraw::payload(&legacy).unwrap().len(), 8);
    let mut dx10 = vec![0u8; 148 + 16];
    dx10[..4].copy_from_slice(b"DDS ");
    dx10[84..88].copy_from_slice(b"DX10");
    assert_eq!(ddsraw::payload(&dx10).unwrap().len(), 16);
}

#[test]
fn f16_loader_reads_the_bench_format() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("x.f16");
    let mut bytes = vec![];
    for v in [0x0036_3146u32, 1, 1] {
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    for h in [0x3C00u16, 0x4000, 0x0000, 0x3C00] {
        bytes.extend_from_slice(&h.to_le_bytes());
    }
    std::fs::write(&p, bytes).unwrap();
    let img = corpus::load_f16(&p).unwrap();
    assert_eq!((img.width, img.height), (1, 1));
    assert_eq!(img.px[0], [0x3C00, 0x4000, 0, 0x3C00]);
}

#[test]
fn reads_bench_results_csv() {
    let root = corpus::bench_root();
    let rows = corpus::read_results(&root.join("results/results.csv"), "bc7e_u5").unwrap();
    let r = corpus::find(&rows, "rgb_real", "Bricks104").expect("Bricks104 row");
    assert!((r.f("rgb_psnr").unwrap() - 47.81554222506591).abs() < 1e-12);
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p sdc-bench --test basics`
Expected: FAIL to compile.

- [ ] **Step 3: Implement the modules**

`crates/bench/src/lib.rs`:
```rust
//! sdc-bench: reproduces F:/ai/dds/bench numbers through sdc's encoders and gates them.
pub mod codec;
pub mod corpus;
pub mod ddsraw;
pub mod metrics;
```

`crates/bench/src/corpus.rs`:
```rust
use anyhow::{anyhow, bail, Context, Result};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub fn bench_root() -> PathBuf {
    std::env::var_os("SDC_BENCH_ROOT").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"F:\ai\dds\bench"))
}

pub struct Rgba8 {
    pub width: u32,
    pub height: u32,
    pub px: Vec<[u8; 4]>,
}

/// Same as PIL `Image.open(p).convert("RGBA")` for the bench's 8-bit PNGs.
pub fn load_png_rgba8(path: &Path) -> Result<Rgba8> {
    let img = image::open(path).with_context(|| path.display().to_string())?.to_rgba8();
    let (width, height) = img.dimensions();
    let px = img.pixels().map(|p| p.0).collect();
    Ok(Rgba8 { width, height, px })
}

pub struct HalfImage {
    pub width: u32,
    pub height: u32,
    pub px: Vec<[u16; 4]>,
}

/// Bench `.f16`: magic "F16\0" (0x00363146), u32 width, u32 height, then RGBA half floats.
pub fn load_f16(path: &Path) -> Result<HalfImage> {
    let b = std::fs::read(path).with_context(|| path.display().to_string())?;
    let u = |i: usize| u32::from_le_bytes(b[i..i + 4].try_into().unwrap());
    if b.len() < 12 || u(0) != 0x0036_3146 {
        bail!("{}: not an F16 file", path.display());
    }
    let (width, height) = (u(4), u(8));
    let n = (width * height) as usize;
    if b.len() != 12 + n * 8 {
        bail!("{}: truncated", path.display());
    }
    let px = (0..n)
        .map(|i| {
            let o = 12 + i * 8;
            [0, 1, 2, 3].map(|c| u16::from_le_bytes([b[o + c * 2], b[o + c * 2 + 1]]))
        })
        .collect();
    Ok(HalfImage { width, height, px })
}

pub fn stem(p: &Path) -> String {
    p.file_stem().unwrap().to_string_lossy().into_owned()
}

pub fn set_files(root: &Path, dir: &str, set: &str, ext: &str) -> Result<Vec<PathBuf>> {
    let d = root.join(dir).join(set);
    let mut v: Vec<PathBuf> = std::fs::read_dir(&d)
        .with_context(|| d.display().to_string())?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case(ext)))
        .collect();
    v.sort();
    Ok(v)
}

pub struct ResultRow {
    pub variant: String,
    pub set: String,
    pub image: String,
    pub values: BTreeMap<String, String>,
}

impl ResultRow {
    pub fn f(&self, key: &str) -> Option<f64> {
        self.values.get(key).and_then(|v| v.parse().ok())
    }
}

/// Reads results.csv / results2.csv (no quoted fields in successful rows; rows with errors are skipped).
pub fn read_results(path: &Path, variant: &str) -> Result<Vec<ResultRow>> {
    let text = std::fs::read_to_string(path).with_context(|| path.display().to_string())?;
    let mut lines = text.lines();
    let header: Vec<&str> = lines.next().ok_or_else(|| anyhow!("empty csv"))?.split(',').collect();
    let mut rows = vec![];
    for line in lines {
        let f: Vec<&str> = line.split(',').collect();
        if f.len() != header.len() || f[0] != variant || !f[header.len() - 1].is_empty() {
            continue;
        }
        let values: BTreeMap<String, String> = header.iter().zip(&f).map(|(k, v)| (k.to_string(), v.to_string())).collect();
        rows.push(ResultRow { variant: f[0].into(), set: f[2].into(), image: f[3].into(), values });
    }
    Ok(rows)
}

pub fn find<'a>(rows: &'a [ResultRow], set: &str, image: &str) -> Option<&'a ResultRow> {
    rows.iter().find(|r| r.set == set && r.image == image)
}
```

`crates/bench/src/metrics.rs`:
```rust
//! Metrics computed exactly as F:/ai/dds/bench/scripts/run_bench.py and run_bench2.py (float64).

pub fn psnr_from_mse(mse: f64) -> f64 {
    if mse <= 1e-12 {
        99.0
    } else {
        10.0 * (255.0f64 * 255.0 / mse).log10()
    }
}

pub fn channel_psnr(src: &[[u8; 4]], dec: &[[u8; 4]], ch: &[usize]) -> f64 {
    assert_eq!(src.len(), dec.len());
    let mut sum = 0.0f64;
    for (s, d) in src.iter().zip(dec) {
        for &c in ch {
            let e = s[c] as f64 - d[c] as f64;
            sum += e * e;
        }
    }
    psnr_from_mse(sum / (src.len() * ch.len()) as f64)
}

pub fn rgb_psnr(src: &[[u8; 4]], dec: &[[u8; 4]]) -> f64 {
    channel_psnr(src, dec, &[0, 1, 2])
}

pub fn a_psnr(src: &[[u8; 4]], dec: &[[u8; 4]]) -> f64 {
    channel_psnr(src, dec, &[3])
}

/// Multi-exposure PSNR (run_bench2.py hdr_metrics): exposures 2^-6..2^6, gamma 1/2.2, clip to [0, 255].
pub fn mpsnr(reference: &[[f32; 3]], decoded: &[[f32; 3]]) -> f64 {
    assert_eq!(reference.len(), decoded.len());
    let n = (reference.len() * 3) as f64;
    let mut mses = vec![];
    for c in -6..=6 {
        let s = 2f64.powi(c);
        let t = |v: f32| (255.0 * ((v as f64).max(0.0) * s).powf(1.0 / 2.2)).clamp(0.0, 255.0);
        let mut sum = 0.0;
        for (r, d) in reference.iter().zip(decoded) {
            for k in 0..3 {
                let e = t(r[k]) - t(d[k]);
                sum += e * e;
            }
        }
        mses.push(sum / n);
    }
    psnr_from_mse(mean(&mses))
}

pub fn mean(v: &[f64]) -> f64 {
    v.iter().sum::<f64>() / v.len() as f64
}
```

`crates/bench/src/ddsraw.rs`:
```rust
use anyhow::{bail, Result};

/// Block payload of a single-level DDS (mip 0 only, as all bench outputs are).
pub fn payload(bytes: &[u8]) -> Result<&[u8]> {
    if bytes.len() < 128 || &bytes[..4] != b"DDS " {
        bail!("not a DDS file");
    }
    let off = if &bytes[84..88] == b"DX10" { 148 } else { 128 };
    if bytes.len() < off {
        bail!("truncated DDS header");
    }
    Ok(&bytes[off..])
}
```

`crates/bench/src/codec.rs`:
```rust
use crate::corpus::{HalfImage, Rgba8};
use encoders_sys::*;
use rayon::prelude::*;

/// bc7e over block rows, 64 blocks per call (as rdo_bc_encoder.cpp does).
pub fn encode_bc7_image(img: &Rgba8, p: Bc7Params) -> Vec<u8> {
    let blocks = gather_blocks(&img.px, img.width, img.height);
    let row = img.width.div_ceil(4) as usize * 16; // texels per block row == bytes per block row for BC7
    let mut out = vec![0u8; blocks.len()];
    out.par_chunks_mut(row).zip(blocks.par_chunks(row)).for_each(|(o, b)| {
        for (oc, bc) in o.chunks_mut(64 * 16).zip(b.chunks(64 * 16)) {
            bc7e_compress_blocks(bc, oc, p);
        }
    });
    out
}

#[derive(Debug, Clone, Copy)]
pub enum RgbcxKind {
    Bc1 { transparent_black: bool },
    Bc3,
    Bc4,
    Bc5,
}

/// rgbcx exactly as bc7enc_rdo's tool configures it (level 18, 3-colour on, radius 5, all BC4 modes).
pub fn encode_rgbcx_image(img: &Rgba8, kind: RgbcxKind) -> Vec<u8> {
    let blocks = gather_blocks(&img.px, img.width, img.height);
    let bsize = match kind {
        RgbcxKind::Bc1 { .. } | RgbcxKind::Bc4 => 8,
        RgbcxKind::Bc3 | RgbcxKind::Bc5 => 16,
    };
    let mut out = vec![0u8; blocks.len() / 16 * bsize];
    out.par_chunks_mut(bsize).zip(blocks.par_chunks(16)).for_each(|(o, b)| {
        let b: &[[u8; 4]; 16] = b.try_into().unwrap();
        match kind {
            RgbcxKind::Bc1 { transparent_black } => o.copy_from_slice(&bc1_encode_block(b, RGBCX_MAX_LEVEL, true, transparent_black)),
            RgbcxKind::Bc3 => o.copy_from_slice(&bc3_encode_block(b, RGBCX_MAX_LEVEL, BC45_RADIUS_DEFAULT)),
            RgbcxKind::Bc4 => o.copy_from_slice(&bc4_encode_block(b, 0, BC45_RADIUS_DEFAULT)),
            RgbcxKind::Bc5 => o.copy_from_slice(&bc5_encode_block(b, BC45_RADIUS_DEFAULT)),
        }
    });
    out
}

pub fn encode_bc6h_image(img: &HalfImage) -> Vec<u8> {
    let w = img.width;
    let strip = (w * 4) as usize;
    let mut out = vec![0u8; (w / 4 * (img.height / 4) * 16) as usize];
    out.par_chunks_mut((w / 4 * 16) as usize).zip(img.px.par_chunks(strip)).for_each(|(o, rows)| {
        bc6h_compress_rows(rows, w, 4, o, Bc6hLevel::Veryslow);
    });
    out
}

fn scatter<T: Copy + Default, const N: usize>(w: u32, h: u32, data: &[u8], bsize: usize, dec: impl Fn(&[u8]) -> [T; N]) -> Vec<T> {
    let (bw, bh) = (w.div_ceil(4) as usize, h.div_ceil(4) as usize);
    let per = N / 16;
    let mut img = vec![T::default(); (w * h) as usize * per];
    for by in 0..bh {
        for bx in 0..bw {
            let px = dec(&data[(by * bw + bx) * bsize..][..bsize]);
            for y in 0..4 {
                for x in 0..4 {
                    let (gx, gy) = (bx * 4 + x, by * 4 + y);
                    if gx < w as usize && gy < h as usize {
                        let o = (gy * w as usize + gx) * per;
                        img[o..o + per].copy_from_slice(&px[(y * 4 + x) * per..][..per]);
                    }
                }
            }
        }
    }
    img
}

pub fn decode_bc7_image(data: &[u8], w: u32, h: u32) -> Vec<[u8; 4]> {
    let flat = scatter::<u8, 64>(w, h, data, 16, |b| {
        let mut o = [0u8; 64];
        bcdec_rs::bc7(b, &mut o, 16);
        o
    });
    flat.chunks(4).map(|c| [c[0], c[1], c[2], c[3]]).collect()
}

pub fn decode_bc6h_image(data: &[u8], w: u32, h: u32) -> Vec<[f32; 3]> {
    let flat = scatter::<f32, 48>(w, h, data, 16, |b| {
        let mut o = [0f32; 48];
        bcdec_rs::bc6h_float(b, &mut o, 12, false);
        o
    });
    flat.chunks(3).map(|c| [c[0], c[1], c[2]]).collect()
}

pub fn half_to_rgb_f32(img: &HalfImage) -> Vec<[f32; 3]> {
    img.px.iter().map(|p| [0, 1, 2].map(|c| half::f16::from_bits(p[c]).to_f32())).collect()
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p sdc-bench --test basics`
Expected: 5 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/bench
git commit -m "feat(bench): corpus access, bench-identical metrics, BC codecs over encoders-sys"
```

---
### Task 9: GATE — spike S3: the workspace reproduces the bench (bc7e u5 within 0.01 dB; rgbcx BC4 bit-exact)

This is the spec §12 S3 gate and the first two rows of the §11.2 gate table. **Nothing in Phase 1 starts until it passes.**

**Files:**
- Create: `crates/bench/src/gates.rs`
- Modify: `crates/bench/src/lib.rs` (`pub mod gates;`), `crates/bench/src/main.rs`
- Test: `crates/bench/tests/gates.rs`

**Interfaces:**
- Consumes: `corpus`, `metrics`, `codec`, `ddsraw` (Task 8); `encoders_sys::{init, Bc1Model, Bc7Params, Bc7Level}`.
- Produces:
  - `pub struct GateReport { pub name: String, pub pass: bool, pub lines: Vec<String> }` with `fn new(name: &str) -> Self`, `fn check(&mut self, ok: bool, line: String)`, `fn info(&mut self, line: String)`, and `Display`
  - `pub const BC7_SETS: [&str; 10]`
  - `pub fn bc7e_u5(root: &Path, require_identical_bytes: bool) -> anyhow::Result<GateReport>`
  - `pub fn rgbcx_bit_exact(root: &Path, bench_out: &str, images_dir: &str, sets: &[&str], kind: RgbcxKind) -> anyhow::Result<GateReport>`
  - `pub fn run(name: &str, root: &Path) -> anyhow::Result<Vec<GateReport>>` (dispatch; later tasks add arms)
  - CLI: `sdc-bench gate <name>` exits 0 when every report passes, 1 otherwise, 3 on I/O error.

Expected per-image values for `rgb_real` (from `results/results.csv`, variant `bc7e_u5`; the gate reads them from the CSV, this table is for the reviewer): Bricks104 47.8155, Fabric084 52.8348, Metal049A 57.4864, PaintedPlaster017 55.4633, Rock064 41.7424, Tiles141 49.7528, Wood094 51.7448; mean **50.977** (spec: 50.98).

- [ ] **Step 1: Write the failing tests**

`crates/bench/tests/gates.rs`:
```rust
use sdc_bench::{codec::RgbcxKind, corpus::bench_root, gates};

#[test]
fn s3_bc7e_u5_reproduces_bench_within_0_01_db() {
    let r = gates::bc7e_u5(&bench_root(), false).unwrap();
    assert!(r.pass, "{r}");
}

#[test]
fn s3_rgbcx_bc4_mask_real_is_bit_exact() {
    let r = gates::rgbcx_bit_exact(&bench_root(), "out2/bc4_rgbcx", "images2", &["mask_real"], RgbcxKind::Bc4).unwrap();
    assert!(r.pass, "{r}");
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test --release -p sdc-bench --test gates`
Expected: FAIL to compile (`gates` module missing).

- [ ] **Step 3: Implement `gates.rs`**

```rust
use crate::codec::*;
use crate::corpus::*;
use crate::{ddsraw, metrics};
use anyhow::{anyhow, bail, Result};
use encoders_sys::{init, Bc1Model, Bc7Level, Bc7Params};
use std::fmt;
use std::path::Path;

pub struct GateReport {
    pub name: String,
    pub pass: bool,
    pub lines: Vec<String>,
}

impl GateReport {
    pub fn new(name: &str) -> Self {
        GateReport { name: name.to_string(), pass: true, lines: vec![] }
    }
    pub fn check(&mut self, ok: bool, line: String) {
        self.pass &= ok;
        self.lines.push(format!("{} {line}", if ok { "ok  " } else { "FAIL" }));
    }
    pub fn info(&mut self, line: String) {
        self.lines.push(format!("info {line}"));
    }
}

impl fmt::Display for GateReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "== {} : {}", self.name, if self.pass { "PASS" } else { "FAIL" })?;
        for l in &self.lines {
            writeln!(f, "   {l}")?;
        }
        Ok(())
    }
}

pub const BC7_SETS: [&str; 10] = [
    "rgb_real", "rgb_grad", "alpha_real", "alpha_grad", "cutout_real",
    "cutout_grad", "packed_real", "packed_grad", "stray_real", "stray_grad",
];

pub const U5: Bc7Params = Bc7Params { level: Bc7Level::Veryslow, perceptual: false, exact_opaque: false };

/// Spec §12 S3 and §11.2 rows 1-2: bc7e u5 (veryslow, linear) on every bench BC7 set, mip 0.
pub fn bc7e_u5(root: &Path, require_identical_bytes: bool) -> Result<GateReport> {
    init(Bc1Model::Ideal)?;
    let rows = read_results(&root.join("results/results.csv"), "bc7e_u5")?;
    let mut r = GateReport::new("bc7e u5 reproduces results.csv (S3, §11.2)");
    let (mut identical, mut total) = (0, 0);
    for set in BC7_SETS {
        let (mut ours, mut theirs) = (vec![], vec![]);
        for src in set_files(root, "images", set, "png")? {
            let stem = stem(&src);
            let img = load_png_rgba8(&src)?;
            let enc = encode_bc7_image(&img, U5);
            let bench = std::fs::read(root.join("out/bc7e_u5").join(set).join(format!("{stem}.dds")))?;
            let same = ddsraw::payload(&bench)? == enc.as_slice();
            total += 1;
            identical += same as usize;
            if require_identical_bytes {
                r.check(same, format!("{set}/{stem}: block bytes identical to bench"));
            }
            let dec = decode_bc7_image(&enc, img.width, img.height);
            let (rgb, a) = (metrics::rgb_psnr(&img.px, &dec), metrics::a_psnr(&img.px, &dec));
            let row = find(&rows, set, &stem).ok_or_else(|| anyhow!("no results.csv row for {set}/{stem}"))?;
            let (er, ea) = (row.f("rgb_psnr").unwrap(), row.f("a_psnr").unwrap());
            r.check(
                (rgb - er).abs() <= 0.01 && (a - ea).abs() <= 0.01,
                format!("{set}/{stem}: rgb {rgb:.4} (bench {er:.4}), a {a:.4} (bench {ea:.4}), bytes {}", if same { "identical" } else { "DIFFER" }),
            );
            ours.push(rgb);
            theirs.push(er);
        }
        if set == "rgb_real" {
            let (m, mb) = (metrics::mean(&ours), metrics::mean(&theirs));
            r.check((m - 50.98).abs() <= 0.01 && (m - mb).abs() <= 0.01, format!("rgb_real mean RGB PSNR {m:.4} dB (bench {mb:.4}, spec 50.98)"));
        }
    }
    r.info(format!("{identical}/{total} images have block payloads byte-identical to the bench"));
    Ok(r)
}

/// Spec §11.2 rgbcx row: our rgbcx output must equal bc7enc_rdo's bytes for the same images.
pub fn rgbcx_bit_exact(root: &Path, bench_out: &str, images_dir: &str, sets: &[&str], kind: RgbcxKind) -> Result<GateReport> {
    init(Bc1Model::Ideal)?;
    let mut r = GateReport::new(&format!("rgbcx {kind:?} bit-exact vs {bench_out}"));
    for set in sets {
        let files = set_files(root, images_dir, set, "png")?;
        if files.is_empty() {
            bail!("no images in {images_dir}/{set}");
        }
        for src in files {
            let stem = stem(&src);
            let enc = encode_rgbcx_image(&load_png_rgba8(&src)?, kind);
            let bench = std::fs::read(root.join(bench_out).join(set).join(format!("{stem}.dds")))?;
            r.check(ddsraw::payload(&bench)? == enc.as_slice(), format!("{set}/{stem}"));
        }
    }
    Ok(r)
}

pub fn run(name: &str, root: &Path) -> Result<Vec<GateReport>> {
    Ok(match name {
        "s3" => vec![
            bc7e_u5(root, false)?,
            rgbcx_bit_exact(root, "out2/bc4_rgbcx", "images2", &["mask_real"], RgbcxKind::Bc4)?,
        ],
        other => bail!("unknown gate '{other}'"),
    })
}
```

Replace `crates/bench/src/main.rs`:
```rust
use clap::{Parser, Subcommand};
use sdc_bench::{corpus::bench_root, gates};
use std::process::ExitCode;

#[derive(Parser)]
#[command(name = "sdc-bench", about = "sdc regression harness over F:/ai/dds/bench (SDC_BENCH_ROOT)")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run a named gate (s3, …) and print its report.
    Gate { name: String },
}

fn main() -> ExitCode {
    let root = bench_root();
    match Cli::parse().cmd {
        Cmd::Gate { name } => match gates::run(&name, &root) {
            Ok(reports) => {
                let mut ok = true;
                for r in &reports {
                    println!("{r}");
                    ok &= r.pass;
                }
                if ok { ExitCode::SUCCESS } else { ExitCode::from(1) }
            }
            Err(e) => {
                eprintln!("error: {e:#}");
                ExitCode::from(3)
            }
        },
    }
}
```
Add `pub mod gates;` to `crates/bench/src/lib.rs`.

- [ ] **Step 4: Run the gate**

Run: `cargo test --release -p sdc-bench --test gates` and `cargo run --release -p sdc-bench -- gate s3`
Expected: both tests PASS; the printed report shows `rgb_real mean RGB PSNR 50.977…`, every row `ok`, and an info line such as `39/39 images … byte-identical` (identical bytes are expected because the objects and parameters are the bench's own; fewer is allowed by the spec criterion but must be explained in Step 6).

If the gate FAILS: (1) check `ddsraw` payload equality first — if bytes differ only on some sets, compare `gather_blocks` against rdo_bc_encoder's `get_block` for images whose size is not a multiple of 4; (2) confirm perceptual is off and the level is `Veryslow` (u5); (3) if bytes are identical but PSNR differs, the metric differs from `run_bench.py` — re-check `channel_psnr` pools all three channels into one MSE. Do not loosen the tolerance.

- [ ] **Step 5: Commit**

```bash
git add crates/bench
git commit -m "test(bench): S3 gate - bc7e u5 reproduces bench within 0.01 dB, rgbcx BC4 bit-exact"
```

- [ ] **Step 6: Record the result in DECISIONS.md**

Append to `docs/DECISIONS.md` (fill the values from the gate output):
```markdown
## Spike S3 — workspace links vendored encoders and reproduces the bench (<date>)

- Result: **PASS** (criterion: builds without ISPC on PATH; rgb_real mean RGB PSNR within 0.01 dB of 50.98; rgbcx BC4 bit-exact on mask_real).
- rgb_real mean RGB PSNR: <value> dB (results.csv mean 50.977); largest per-image |Δ| over all 10 BC7 sets: <value> dB.
- Block payloads byte-identical to the bench: <n>/<total>.
- rgbcx BC4 mask_real: <n>/7 bit-exact.
- Toolchain: rustc <version>, MSVC <cl.exe version>, objects from ISPC 1.31.0 (bench build).
```
```bash
git add docs/DECISIONS.md
git commit -m "docs: record spike S3 result"
```

---

### Task 10: GATE — rgbcx BC1/BC3/BC4/BC5 bit-exact, and the shipped transparent-black-off setting

Spec §11.2 rgbcx row: bit-exact in the bench's configuration (BC1 transparent-black **on**, as the bench ran), plus a second check that the shipped **off** setting equals `bc7enc` run with that mode disabled (`-b`).

**Files:**
- Modify: `crates/bench/src/gates.rs`
- Test: `crates/bench/tests/gates.rs`

**Interfaces:**
- Consumes: `rgbcx_bit_exact` (Task 9), `codec::encode_rgbcx_image` (Task 8).
- Produces: `pub fn rgbcx_all(root: &Path) -> anyhow::Result<Vec<GateReport>>`; `pub fn bc1_black_off_vs_tool(root: &Path, work: &Path) -> anyhow::Result<GateReport>`; `run("rgbcx", …)`.

- [ ] **Step 1: Write the failing tests**

Append to `crates/bench/tests/gates.rs`:
```rust
#[test]
fn rgbcx_bc1_bc3_bc4_bc5_bit_exact_in_bench_configuration() {
    for r in gates::rgbcx_all(&bench_root()).unwrap() {
        assert!(r.pass, "{r}");
    }
}

#[test]
fn bc1_transparent_black_off_matches_bc7enc_dash_b() {
    let work = tempfile::tempdir().unwrap();
    let r = gates::bc1_black_off_vs_tool(&bench_root(), work.path()).unwrap();
    assert!(r.pass, "{r}");
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test --release -p sdc-bench --test gates -- rgbcx bc1_transparent`
Expected: FAIL to compile.

- [ ] **Step 3: Implement**

Add to `crates/bench/src/gates.rs`:
```rust
pub const OPAQUE_SETS: [&str; 2] = ["rgb_real", "rgb_grad"];
pub const ALPHA_SETS: [&str; 8] = [
    "cutout_real", "cutout_grad", "alpha_real", "alpha_grad", "packed_real", "packed_grad", "stray_real", "stray_grad",
];

pub fn rgbcx_all(root: &Path) -> Result<Vec<GateReport>> {
    Ok(vec![
        rgbcx_bit_exact(root, "out/bc1_rgbcx_L18", "images", &OPAQUE_SETS, RgbcxKind::Bc1 { transparent_black: true })?,
        rgbcx_bit_exact(root, "out/bc3_rgbcx_L18", "images", &ALPHA_SETS, RgbcxKind::Bc3)?,
        rgbcx_bit_exact(root, "out2/bc4_rgbcx", "images2", &["mask_real", "mask_grad"], RgbcxKind::Bc4)?,
        rgbcx_bit_exact(root, "out2/bc5_rgbcx", "images2", &["normal_real", "normal_grad"], RgbcxKind::Bc5)?,
    ])
}

/// Runs the bench's own bc7enc.exe with -b (3-colour mode never used for black) and compares bytes
/// with sdc's shipped BC1 setting (transparent-black off).
pub fn bc1_black_off_vs_tool(root: &Path, work: &Path) -> Result<GateReport> {
    init(Bc1Model::Ideal)?;
    let tool = root.join("build/bc7enc_rdo/bc7enc.exe");
    let mut r = GateReport::new("rgbcx BC1 transparent-black OFF == bc7enc -1 -b");
    for set in OPAQUE_SETS {
        for src in set_files(root, "images", set, "png")? {
            let stem = stem(&src);
            let dst = work.join(format!("{set}_{stem}.dds"));
            let st = std::process::Command::new(&tool).args(["-q", "-g", "-1", "-b"]).arg(&src).arg(&dst).output()?;
            if !st.status.success() || !dst.exists() {
                bail!("bc7enc failed on {}: {}", src.display(), String::from_utf8_lossy(&st.stderr));
            }
            let ours = encode_rgbcx_image(&load_png_rgba8(&src)?, RgbcxKind::Bc1 { transparent_black: false });
            r.check(ddsraw::payload(&std::fs::read(&dst)?)? == ours.as_slice(), format!("{set}/{stem}"));
        }
    }
    Ok(r)
}
```
Add the dispatch arm in `run`:
```rust
        "rgbcx" => {
            let mut v = rgbcx_all(root)?;
            let work = std::env::temp_dir().join("sdc-bench-bc1-black-off");
            std::fs::create_dir_all(&work)?;
            v.push(bc1_black_off_vs_tool(root, &work)?);
            v
        }
```

- [ ] **Step 4: Run the gates**

Run: `cargo test --release -p sdc-bench --test gates`
Expected: all PASS. If a BC1/BC3/BC4/BC5 image differs, the most likely cause is a compiler difference between llvm-mingw clang `-O3` (bench) and MSVC `/O2` (sdc) in rgbcx float code. Diagnose by rebuilding encoders-sys with `/fp:strict` (`b.flag("/fp:strict")` in build.rs) and re-running; if that fixes it, keep `/fp:strict` and note it in DECISIONS; if not, list the differing blocks (index, ours, bench) and stop for owner review — the spec requires bit-exactness.

- [ ] **Step 5: Commit**

```bash
git add crates/bench
git commit -m "test(bench): rgbcx BC1/BC3/BC4/BC5 bit-exact gates incl. transparent-black off vs bc7enc -b"
```

---

### Task 11: GATE — ISPC BC6H `veryslow` reproduces 51.50 dB mPSNR on `hdr_real`

**Files:**
- Modify: `crates/bench/src/gates.rs`
- Test: `crates/bench/tests/gates.rs`

**Interfaces:**
- Consumes: `codec::{encode_bc6h_image, decode_bc6h_image, half_to_rgb_f32}`, `metrics::mpsnr`, `corpus::load_f16` (Task 8).
- Produces: `pub fn bc6h_veryslow(root: &Path) -> anyhow::Result<GateReport>`; `run("bc6h", …)`.

Reference numbers (`results2.csv`, `bc6_ispc_veryslow`, `hdr_real`): dikhololo_night 41.8965, kloofendal_48d_partly_cloudy_puresky 59.1694, studio_small_08 55.7057, venice_sunset 49.2262; mean **51.499**.

- [ ] **Step 1: Write the failing test**

Append to `crates/bench/tests/gates.rs`:
```rust
#[test]
fn bc6h_veryslow_reproduces_51_50_db() {
    let r = gates::bc6h_veryslow(&bench_root()).unwrap();
    assert!(r.pass, "{r}");
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test --release -p sdc-bench --test gates bc6h`
Expected: FAIL to compile.

- [ ] **Step 3: Implement**

Add to `gates.rs`:
```rust
/// Spec §11.2 last row. Input is the bench's raw `.f16` twin of each HDR image (identical data to `.ref.npy`).
/// The bench decoded with texconv; sdc decodes with bcdec_rs (spec §11.4 asserts they agree exactly for BC6H).
pub fn bc6h_veryslow(root: &Path) -> Result<GateReport> {
    init(Bc1Model::Ideal)?;
    let rows = read_results(&root.join("results/results2.csv"), "bc6_ispc_veryslow")?;
    let mut r = GateReport::new("ISPC bc6h_veryslow reproduces results2.csv mPSNR");
    for set in ["hdr_real", "hdr_grad"] {
        let mut ours = vec![];
        let mut theirs = vec![];
        for src in set_files(root, "images2", set, "f16")? {
            let stem = stem(&src);
            let img = load_f16(&src)?;
            let enc = encode_bc6h_image(&img);
            let bench = std::fs::read(root.join("out2/bc6_ispc_veryslow").join(set).join(format!("{stem}.dds")))?;
            let same = ddsraw::payload(&bench)? == enc.as_slice();
            let m = metrics::mpsnr(&half_to_rgb_f32(&img), &decode_bc6h_image(&enc, img.width, img.height));
            let e = find(&rows, set, &stem).and_then(|row| row.f("mpsnr")).ok_or_else(|| anyhow!("no row {set}/{stem}"))?;
            r.check((m - e).abs() <= 0.01, format!("{set}/{stem}: mPSNR {m:.4} (bench {e:.4}), bytes {}", if same { "identical" } else { "DIFFER" }));
            ours.push(m);
            theirs.push(e);
        }
        if set == "hdr_real" {
            let m = metrics::mean(&ours);
            r.check((m - 51.50).abs() <= 0.01, format!("hdr_real mean mPSNR {m:.4} (bench {:.4}, spec 51.50)", metrics::mean(&theirs)));
        }
    }
    Ok(r)
}
```
Add the arm: `"bc6h" => vec![bc6h_veryslow(root)?],`.

- [ ] **Step 4: Run the gate**

Run: `cargo test --release -p sdc-bench --test gates bc6h`
Expected: PASS. If bytes are identical but mPSNR differs, the decoder disagrees with texconv: decode one block with `texconv -f R32G32B32A32_FLOAT` (bench `tools/dl_texconv/texconv.exe`) and compare floats; record the finding and stop for review (spec §11.4 assumes they agree).

- [ ] **Step 5: Commit**

```bash
git add crates/bench
git commit -m "test(bench): ISPC BC6H veryslow reproduces 51.50 dB mPSNR gate"
```

---

### Task 12: Spike S5 (part a) — decoded-alpha statistics and the all-255-block check, proven on bench outputs

Spec §12 S5: "Implement §5.8's check". The check lives in the engine from the start (Task 35 builds on it).

**Files:**
- Create: `crates/engine/src/validate/mod.rs`, `crates/engine/src/validate/alpha.rs`
- Modify: `crates/engine/src/lib.rs` (`pub mod validate;`), `crates/bench/src/codec.rs` (BC1 image decode), `crates/bench/src/gates.rs`
- Test: `crates/engine/src/validate/alpha.rs` (unit tests), `crates/bench/tests/gates.rs`

**Interfaces:**
- Produces:
  - `engine::validate::alpha::AlphaStats { pixels: u64, eq255: u64, eq254: u64, below254: u64, min: u8 }` (`Copy`, `Default`, serde) with `fn share_254(&self) -> f64`, `fn share_not_255(&self) -> f64`
  - `engine::validate::alpha::alpha_stats(decoded: &[[u8; 4]]) -> AlphaStats`
  - `engine::validate::alpha::opaque_block_violations(input: &[[u8; 4]], decoded: &[[u8; 4]], width: u32, height: u32) -> u64` — texels whose decoded alpha ≠ 255 inside 4×4 blocks whose encoder-input alpha is 255 on every in-image texel
  - `sdc_bench::codec::decode_bc1_image(data: &[u8], w: u32, h: u32, model: Bc1Model) -> Vec<[u8; 4]>`
  - `sdc_bench::gates::alpha_scan(root: &Path) -> anyhow::Result<GateReport>`; `run("alpha-scan", …)`

- [ ] **Step 1: Write the failing unit tests**

`crates/engine/src/validate/alpha.rs` (tests first; the functions follow in Step 3):
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_255_254_and_below() {
        let px = [[0, 0, 0, 255], [0, 0, 0, 254], [0, 0, 0, 10], [0, 0, 0, 255]];
        let s = alpha_stats(&px);
        assert_eq!((s.pixels, s.eq255, s.eq254, s.below254, s.min), (4, 2, 1, 1, 10));
        assert!((s.share_254() - 0.25).abs() < 1e-12);
        assert!((s.share_not_255() - 0.5).abs() < 1e-12);
    }

    #[test]
    fn violations_only_count_blocks_whose_input_is_all_255() {
        // 8x4 image = two blocks. Block 0 input all 255, decoded has two 254s. Block 1 input has a 200.
        let mut input = vec![[0u8, 0, 0, 255]; 32];
        input[6] = [0, 0, 0, 200]; // row 0, x = 6 (block 1)
        let mut dec = input.clone();
        dec[0] = [0, 0, 0, 254];
        dec[9] = [0, 0, 0, 254]; // row 1, x = 1 (block 0)
        dec[7] = [0, 0, 0, 100]; // block 1: not counted
        assert_eq!(opaque_block_violations(&input, &dec, 8, 4), 2);
    }
}
```
`crates/engine/src/validate/mod.rs`:
```rust
//! Validation (spec §5.8). `alpha` holds the decoded-alpha statistics used by spike S5 and by Validate.
pub mod alpha;
```
Add `pub mod validate;` to `crates/engine/src/lib.rs`.

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p engine validate::alpha`
Expected: FAIL to compile (`alpha_stats` not found).

- [ ] **Step 3: Implement**

Put above the tests in `alpha.rs`:
```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlphaStats {
    pub pixels: u64,
    pub eq255: u64,
    pub eq254: u64,
    pub below254: u64,
    pub min: u8,
}

impl AlphaStats {
    pub fn share_254(&self) -> f64 {
        if self.pixels == 0 { 0.0 } else { self.eq254 as f64 / self.pixels as f64 }
    }
    pub fn share_not_255(&self) -> f64 {
        if self.pixels == 0 { 0.0 } else { (self.pixels - self.eq255) as f64 / self.pixels as f64 }
    }
}

pub fn alpha_stats(decoded: &[[u8; 4]]) -> AlphaStats {
    let mut s = AlphaStats { min: 255, ..Default::default() };
    for p in decoded {
        s.pixels += 1;
        match p[3] {
            255 => s.eq255 += 1,
            254 => s.eq254 += 1,
            _ => s.below254 += 1,
        }
        s.min = s.min.min(p[3]);
    }
    s
}

pub fn opaque_block_violations(input: &[[u8; 4]], decoded: &[[u8; 4]], width: u32, height: u32) -> u64 {
    let (w, h) = (width as usize, height as usize);
    assert_eq!(input.len(), w * h);
    assert_eq!(decoded.len(), w * h);
    let mut bad = 0u64;
    for by in (0..h).step_by(4) {
        for bx in (0..w).step_by(4) {
            let texels = || (by..(by + 4).min(h)).flat_map(move |y| (bx..(bx + 4).min(w)).map(move |x| y * w + x));
            if texels().all(|i| input[i][3] == 255) {
                bad += texels().filter(|&i| decoded[i][3] != 255).count() as u64;
            }
        }
    }
    bad
}
```

- [ ] **Step 4: Run the unit tests**

Run: `cargo test -p engine validate::alpha`
Expected: 2 passed.

- [ ] **Step 5: Write the failing scan test**

Append to `crates/bench/tests/gates.rs`:
```rust
#[test]
fn s5a_alpha_checker_agrees_with_recorded_measurements() {
    // DECISIONS Note C: bc7e -u6 Bricks104 decodes 254 on 21.0% of pixels; NVTT Production on 0%.
    let r = gates::alpha_scan(&bench_root()).unwrap();
    assert!(r.pass, "{r}");
}
```

- [ ] **Step 6: Implement the scan**

Add to `crates/bench/src/codec.rs`:
```rust
pub fn decode_bc1_image(data: &[u8], w: u32, h: u32, model: Bc1Model) -> Vec<[u8; 4]> {
    let flat = scatter::<u8, 64>(w, h, data, 8, |b| {
        let px = bc1_decode_block(b.try_into().unwrap(), model);
        let mut o = [0u8; 64];
        for (i, p) in px.iter().enumerate() {
            o[i * 4..i * 4 + 4].copy_from_slice(p);
        }
        o
    });
    flat.chunks(4).map(|c| [c[0], c[1], c[2], c[3]]).collect()
}
```
Add to `crates/bench/src/gates.rs`:
```rust
fn dds_dims(bytes: &[u8]) -> (u32, u32) {
    let u = |i: usize| u32::from_le_bytes(bytes[i..i + 4].try_into().unwrap());
    (u(16), u(12)) // (width, height)
}

/// S5a: decoded alpha at mip 0 for existing bench outputs of the opaque sets (no new encodes).
pub fn alpha_scan(root: &Path) -> Result<GateReport> {
    use engine::validate::alpha::alpha_stats;
    init(Bc1Model::Ideal)?;
    let mut r = GateReport::new("S5a decoded alpha on bench outputs (opaque sets, mip 0)");
    let variants = ["bc7e_u5", "bc7e_u6", "ispc_slow", "nvtt_production", "cmp_q1.0", "bc1_rgbcx_L18"];
    for v in variants {
        for set in OPAQUE_SETS {
            let dir = root.join("out").join(v).join(set);
            if !dir.exists() {
                r.info(format!("{v}/{set}: missing, skipped"));
                continue;
            }
            for src in set_files(root, "images", set, "png")? {
                let stem = stem(&src);
                let bytes = std::fs::read(dir.join(format!("{stem}.dds")))?;
                let (w, h) = dds_dims(&bytes);
                let payload = ddsraw::payload(&bytes)?;
                let dec = if v.starts_with("bc1") {
                    decode_bc1_image(payload, w, h, Bc1Model::Ideal)
                } else {
                    decode_bc7_image(payload, w, h)
                };
                let s = alpha_stats(&dec);
                r.info(format!("{v}/{set}/{stem}: 254 share {:.2}%, below-254 {}, min {}", s.share_254() * 100.0, s.below254, s.min));
                if v == "bc7e_u6" && stem == "Bricks104" {
                    r.check((s.share_254() * 100.0 - 21.0).abs() <= 0.1, format!("bc7e_u6 Bricks104 254 share {:.2}% (Note C: 21.0%)", s.share_254() * 100.0));
                }
                if v == "nvtt_production" {
                    r.check(s.eq255 == s.pixels, format!("nvtt_production {set}/{stem} all 255"));
                }
                if v.starts_with("bc7e") {
                    r.check(s.min >= 254, format!("{v} {set}/{stem} min alpha {} >= 254", s.min));
                }
            }
        }
    }
    Ok(r)
}
```
Add the arm `"alpha-scan" => vec![alpha_scan(root)?],`.

- [ ] **Step 7: Run the scan**

Run: `cargo test --release -p sdc-bench --test gates s5a; cargo run --release -p sdc-bench -- gate alpha-scan > target/bench/s5a.txt`
Expected: PASS. `target/bench/s5a.txt` lists per-image 254 shares (bc1_rgbcx_L18 may show alpha 0 texels, because the bench ran with transparent-black on — that is the reason sdc ships it off).

- [ ] **Step 8: Commit**

```bash
git add crates/engine crates/bench
git commit -m "feat(engine): decoded-alpha statistics and all-255-block check; S5a scan over bench outputs"
```

---

### Task 13: Spike S5 (part b) — byte-patch fallback for exact opaque alpha, measured

The first fallback of spec §5.6: rewrite mode-6 blocks of all-255 source blocks to p-bit = 1 and alpha endpoint 127.

BC7 mode 6 layout (LSB first): bits 0–6 mode (`0b1000000`), R0 7–13, R1 14–20, G0 21–27, G1 28–34, B0 35–41, B1 42–48, **A0 49–55, A1 56–62, P0 63, P1 64**, indices 65–127.

**Files:**
- Create: `crates/encoders-sys/src/bc7_fixup.rs`
- Modify: `crates/encoders-sys/src/lib.rs` (`pub mod bc7_fixup;`), `crates/bench/src/gates.rs`
- Test: `crates/encoders-sys/tests/bc7_fixup.rs`, `crates/bench/tests/gates.rs`

**Interfaces:**
- Produces:
  - `encoders_sys::bc7_fixup::force_opaque_mode6(block: &mut [u8; 16]) -> bool` (true if the block was mode 6 and was rewritten)
  - `sdc_bench::gates::ExactMode { BytePatch, Native }`; `sdc_bench::gates::exact_opaque(root: &Path, mode: ExactMode) -> anyhow::Result<GateReport>`; `run("exact-opaque-bytepatch", …)`

- [ ] **Step 1: Write the failing test**

`crates/encoders-sys/tests/bc7_fixup.rs`:
```rust
use encoders_sys::bc7_fixup::force_opaque_mode6;

fn mode6_block(rgb: u128, alpha: u128, p0: u128, p1: u128) -> [u8; 16] {
    let mut v: u128 = 1 << 6; // mode 6
    for (i, c) in [rgb, rgb, rgb, rgb, rgb, rgb, alpha, alpha].iter().enumerate() {
        v |= c << (7 + 7 * i);
    }
    v |= p0 << 63 | p1 << 64; // indices stay 0
    v.to_le_bytes()
}

fn decode(b: &[u8; 16]) -> [u8; 64] {
    let mut o = [0u8; 64];
    bcdec_rs::bc7(b, &mut o, 16);
    o
}

#[test]
fn rewrites_mode6_pbits_so_alpha_decodes_255() {
    let mut b = mode6_block(10, 127, 0, 0);
    assert_eq!(decode(&b)[3], 254);
    assert!(force_opaque_mode6(&mut b));
    let d = decode(&b);
    assert!(d.chunks(4).all(|p| p[3] == 255));
    assert_eq!(d[0], 21); // RGB endpoint shifts by one LSB: (10 << 1 | 1)
}

#[test]
fn leaves_other_modes_alone() {
    let mut b = [0u8; 16];
    b[0] = 0b0010_0000; // mode 5
    let before = b;
    assert!(!force_opaque_mode6(&mut b));
    assert_eq!(b, before);
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p encoders-sys --test bc7_fixup`
Expected: FAIL to compile.

- [ ] **Step 3: Implement**

`crates/encoders-sys/src/bc7_fixup.rs`:
```rust
//! Post-encode fallback for exact opaque alpha (spec §5.6): only valid on blocks whose source alpha is 255 throughout.

/// If `block` is BC7 mode 6, set both alpha endpoints to 127 and both p-bits to 1, so alpha decodes to 255.
/// RGB endpoints with p-bit 0 move up by one LSB (unoptimised). Returns true if the block was rewritten.
pub fn force_opaque_mode6(block: &mut [u8; 16]) -> bool {
    if block[0] & 0x7f != 0x40 {
        return false;
    }
    let mut v = u128::from_le_bytes(*block);
    let alpha_mask: u128 = ((1u128 << 14) - 1) << 49;
    v = (v & !alpha_mask) | (127u128 << 49) | (127u128 << 56);
    v |= (1u128 << 63) | (1u128 << 64);
    *block = v.to_le_bytes();
    true
}
```
Add `pub mod bc7_fixup;` to `lib.rs`.

- [ ] **Step 4: Run the test**

Run: `cargo test -p encoders-sys --test bc7_fixup`
Expected: 2 passed.

- [ ] **Step 5: Write the failing measurement test**

Append to `crates/bench/tests/gates.rs`:
```rust
#[test]
fn s5b_byte_patch_gives_255_everywhere_on_opaque_sets() {
    let r = gates::exact_opaque(&bench_root(), gates::ExactMode::BytePatch).unwrap();
    assert!(r.pass, "{r}");
}
```

- [ ] **Step 6: Implement the measurement**

Add to `gates.rs`:
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExactMode {
    BytePatch,
    Native,
}

/// Spec §11.2 exact-opaque row at mip 0: opaque sets plus stray sets after `remove` (alpha forced to 255 before encoding).
/// Pass: decoded alpha == 255 on every pixel; per-set mean RGB PSNR within 0.05 dB of plain u5. Worst image reported.
pub fn exact_opaque(root: &Path, mode: ExactMode) -> Result<GateReport> {
    use encoders_sys::bc7_fixup::force_opaque_mode6;
    init(Bc1Model::Ideal)?;
    let mut r = GateReport::new(&format!("exact opaque alpha ({mode:?}), mip 0"));
    for set in ["rgb_real", "rgb_grad", "stray_real", "stray_grad"] {
        let mut deltas = vec![];
        for src in set_files(root, "images", set, "png")? {
            let stem = stem(&src);
            let mut img = load_png_rgba8(&src)?;
            img.px.iter_mut().for_each(|p| p[3] = 255); // alpha `remove`
            let plain = encode_bc7_image(&img, U5);
            let fixed = match mode {
                ExactMode::BytePatch => {
                    let mut b = plain.clone();
                    for blk in b.chunks_mut(16) {
                        force_opaque_mode6(blk.try_into().unwrap());
                    }
                    b
                }
                ExactMode::Native => encode_bc7_image(&img, Bc7Params { exact_opaque: true, ..U5 }),
            };
            let dp = decode_bc7_image(&plain, img.width, img.height);
            let df = decode_bc7_image(&fixed, img.width, img.height);
            let not255 = df.iter().filter(|p| p[3] != 255).count();
            r.check(not255 == 0, format!("{set}/{stem}: {not255} texels with alpha != 255"));
            let d = metrics::rgb_psnr(&img.px, &df) - metrics::rgb_psnr(&img.px, &dp);
            r.info(format!("{set}/{stem}: dRGB {d:+.4} dB"));
            deltas.push(d);
        }
        let worst = deltas.iter().cloned().fold(f64::INFINITY, f64::min);
        let m = metrics::mean(&deltas);
        r.check(m >= -0.05, format!("{set}: mean dRGB {m:+.4} dB (limit -0.05), worst image {worst:+.4} dB"));
    }
    Ok(r)
}
```
Add the arm `"exact-opaque-bytepatch" => vec![exact_opaque(root, ExactMode::BytePatch)?],`.

- [ ] **Step 7: Run it and save the report**

Run: `cargo test --release -p sdc-bench --test gates s5b; cargo run --release -p sdc-bench -- gate exact-opaque-bytepatch > target/bench/s5b.txt`
Expected: alpha checks all `ok`. The dRGB lines are the measured cost of the fallback. If a set's mean is below −0.05 dB the test fails: that is a valid measurement (the byte patch is then only a last-resort fallback behind the native patch, Task 14) — record it in DECISIONS and mark this test `#[ignore = "byte patch exceeds 0.05 dB; see DECISIONS S5"]` rather than loosening the limit.

- [ ] **Step 8: Commit**

```bash
git add crates/encoders-sys crates/bench
git commit -m "feat(encoders-sys): BC7 mode-6 byte-patch fallback for exact opaque alpha; S5b measurement"
```

---

### Task 14: Spike S5 (part c) — bc7e patch 0001 "exact opaque alpha" and its gates

The native mechanism of spec §3.3/§5.6: a field in `bc7e_compress_block_params` makes every mode-6 encode of an opaque block use p-bits = 1 (alpha endpoint stays 127), so bc7e re-optimises RGB under the constraint. Flag off must be bit-identical to upstream. If this task overruns one day, the spec allows the patch to move to Phase 2 under the same gate; the byte-patch fallback (Task 13) then ships and Task 32 uses it.

How bc7e reaches 254 today (`bc7e.ispc`, b943862): opaque blocks go to `handle_opaque_block` (line 3909) or `handle_opaque_block_mode6` (line 4633); both set up mode 6 (`m_comp_bits = 7`, `m_has_pbits = true`, `m_endpoints_share_pbit = false`) and call `color_cell_compression(6, …)`. P-bits are picked for RGB only in `find_optimal_solution` (pbit-search branch, lines ~1834–1846, and compensated-rounding branch, loop at line 1904) and in `pack_mode6_to_one_color` (loop at line 1091). Solid blocks use mode 5 (`handle_block_solid`), and modes 0–5 decode alpha 255, so only mode 6 needs constraining. The struct already has an unused bool, `m_unused0` (line 58), so renaming it keeps the struct layout and the C header ABI.

**Files:**
- Create: `crates/encoders-sys/vendor/bc7e/bc7e.ispc` (patched copy), `crates/encoders-sys/vendor/bc7e/patches/0001-exact-opaque-alpha.patch`
- Modify: `crates/encoders-sys/vendor/ispc-objs/bc7e/*` (regenerated), `crates/encoders-sys/vendor/SHA256SUMS`, `crates/encoders-sys/vendor/README-VENDOR.md`, `tools/regen-ispc.ps1` (default `-Bc7eSource`), `crates/bench/src/gates.rs`, `crates/bench/src/main.rs`
- Test: `crates/bench/tests/gates.rs`, `crates/encoders-sys/tests/bc7.rs`

**Interfaces:**
- Produces: `BC7E_EXACT_OPAQUE_NATIVE == true`; `versions().bc7e_patch == Some("0001-exact-opaque-alpha")`; `sdc_bench::gates::dump_bc7e(root: &Path, out: &Path) -> anyhow::Result<usize>` (writes a hash list); `sdc_bench::gates::bc7e_matches_dump(root: &Path, hashes: &Path) -> anyhow::Result<GateReport>`; `sdc_bench::gates::pre_patch_hashes() -> PathBuf` (`crates/bench/tests/data/pre-patch-bc7e.sha256`, committed); `run("exact-opaque-native", …)`; CLI `sdc-bench dump-bc7e --out <file>`.

- [ ] **Step 1: Record SHA-256s of the unpatched flag-off output (permanent reference for bit-identity)**

The reference must survive `cargo clean`, fresh clones and CI after the unpatched objects are gone, so the hashes are committed. (If Task 9 reported every payload byte-identical to the bench, the bench's own `out/bc7e_u5` files are a second, independent reference; Task 9's gate with `require_identical_bytes = true` checks it.)

Add `sha2.workspace = true` and `hex.workspace = true` to `crates/bench/Cargo.toml` `[dependencies]`, then add to `gates.rs`:
```rust
fn sha256_hex(b: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(b))
}

/// Writes "<set>/<stem> <sha256 of the u5 flag-off BC7 payload>" for every BC7 bench image.
pub fn dump_bc7e(root: &Path, out: &Path) -> Result<usize> {
    init(Bc1Model::Ideal)?;
    let mut lines = vec![];
    for set in BC7_SETS {
        for src in set_files(root, "images", set, "png")? {
            let enc = encode_bc7_image(&load_png_rgba8(&src)?, U5);
            lines.push(format!("{set}/{} {}", stem(&src), sha256_hex(&enc)));
        }
    }
    std::fs::create_dir_all(out.parent().unwrap())?;
    std::fs::write(out, lines.join("\n") + "\n")?;
    Ok(lines.len())
}

/// §11.2 "bc7e patch, flag off: bit-identical to unpatched upstream on every set".
pub fn bc7e_matches_dump(root: &Path, hashes: &Path) -> Result<GateReport> {
    init(Bc1Model::Ideal)?;
    let text = std::fs::read_to_string(hashes)?;
    let want: std::collections::BTreeMap<&str, &str> = text.lines().filter_map(|l| l.split_once(' ')).collect();
    let mut r = GateReport::new("bc7e flag off is bit-identical to the recorded pre-patch hashes");
    for set in BC7_SETS {
        for src in set_files(root, "images", set, "png")? {
            let key = format!("{set}/{}", stem(&src));
            let got = sha256_hex(&encode_bc7_image(&load_png_rgba8(&src)?, U5));
            r.check(want.get(key.as_str()) == Some(&got.as_str()), key);
        }
    }
    Ok(r)
}

pub fn pre_patch_hashes() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/pre-patch-bc7e.sha256")
}
```
Add the dispatch arm to `gates::run`:
```rust
        "exact-opaque-native" => vec![exact_opaque(root, ExactMode::Native)?, bc7e_matches_dump(root, &pre_patch_hashes())?],
```
Add a `DumpBc7e` variant to `Cmd` in `main.rs`:
```rust
    /// Record SHA-256s of u5 flag-off BC7 payloads of every bench image (reference for the patch bit-identity gate).
    DumpBc7e {
        #[arg(long)]
        out: std::path::PathBuf,
    },
```
```rust
        Cmd::DumpBc7e { out } => match gates::dump_bc7e(&root, &out) {
            Ok(n) => {
                println!("{n}");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("error: {e:#}");
                ExitCode::from(3)
            }
        },
```
Then run, **before patching**, and commit the file:
```powershell
cargo run --release -p sdc-bench -- dump-bc7e --out crates/bench/tests/data/pre-patch-bc7e.sha256
git add crates/bench; git commit -m "test(bench): record pre-patch bc7e u5 payload hashes"
```
Expected: `39` (it must equal Task 9's image total).

- [ ] **Step 2: Write the failing tests**

Append to `crates/bench/tests/gates.rs`:
```rust
#[test]
fn s5c_native_patch_flag_off_is_bit_identical() {
    let r = gates::bc7e_matches_dump(&bench_root(), &gates::pre_patch_hashes()).unwrap();
    assert!(r.pass, "{r}");
}

#[test]
fn s5c_native_patch_flag_on_gives_255_within_0_05_db() {
    let r = gates::exact_opaque(&bench_root(), gates::ExactMode::Native).unwrap();
    assert!(r.pass, "{r}");
}
```
Append to `crates/encoders-sys/tests/bc7.rs`:
```rust
#[test]
fn patched_objects_are_vendored() {
    assert!(encoders_sys::BC7E_EXACT_OPAQUE_NATIVE);
    assert_eq!(encoders_sys::versions().bc7e_patch, Some("0001-exact-opaque-alpha"));
}
```

- [ ] **Step 3: Run them to verify they fail**

Run: `cargo test --release -p encoders-sys --test bc7 patched; cargo test --release -p sdc-bench --test gates s5c_native_patch_flag_on`
Expected: `patched_objects_are_vendored` FAILS (`BC7E_EXACT_OPAQUE_NATIVE` is false); the flag-on gate panics with "exact_opaque needs the patched bc7e objects". `s5c_native_patch_flag_off_is_bit_identical` already PASSES at this point (same unpatched objects that produced the hashes) — that is expected; it must still pass after Step 5.

- [ ] **Step 4: Create the patched source**

```powershell
$v = "crates\encoders-sys\vendor"
New-Item -ItemType Directory -Force "$v\bc7e\patches" | Out-Null
Copy-Item "$v\bc7enc_rdo\bc7e.ispc" "$v\bc7e\bc7e.ispc"
```
Edit `vendor/bc7e/bc7e.ispc` (the file is tab-indented; match by the quoted text and line numbers):

1. Insert at the top of the file, before line 1:
```c
// MODIFIED by the sdc project (patch 0001-exact-opaque-alpha, <date>), per Apache-2.0 section 4(b).
// bc7e_compress_block_params::m_exact_opaque_alpha (formerly m_unused0) makes mode-6 encodes of opaque
// blocks use p-bits = 1, so alpha decodes to exactly 255. With the flag false the output is bit-identical
// to bc7enc_rdo b943862.
```
2. Line 58 `	bool m_unused0;` → `	bool m_exact_opaque_alpha;`
3. In `struct color_cell_compressor_params` (line ~812), after `	uniform bool m_perceptual;` add `	uniform bool m_force_pbit1;`
4. In `color_cell_compressor_params_clear`, after `	p->m_endpoints_share_pbit = false;` (line ~828) add `	p->m_force_pbit1 = false;`
5. In `pack_mode6_to_one_color` (line 1091) `	for (uniform uint32_t p = 0; p < 4; p++)` → `	for (uniform uint32_t p = (pParams->m_force_pbit1 ? 3 : 0); p < 4; p++)` (p = 3 is lo_p = 1, hi_p = 1; leave the identical loops at lines 1034 and 1149 alone).
6. In `find_optimal_solution`, pbit-search, non-shared branch (lines ~1836–1843): wrap the first three trials so only (1, 1) runs when forced:
```c
				if (!pParams->m_force_pbit1)
				{
				pbits[0] = 0; pbits[1] = 0;
				evaluate_solution(&lo[0], &hi[0], pbits, pParams, pResults, num_pixels, pPixels);

				pbits[0] = 0; pbits[1] = 1;
				evaluate_solution(&lo[0], &hi[1], pbits, pParams, pResults, num_pixels, pPixels);

				pbits[0] = 1; pbits[1] = 0;
				evaluate_solution(&lo[1], &hi[0], pbits, pParams, pResults, num_pixels, pPixels);
				}
				
				pbits[0] = 1; pbits[1] = 1;
				evaluate_solution(&lo[1], &hi[1], pbits, pParams, pResults, num_pixels, pPixels);
```
7. In the compensated-rounding, non-shared branch, the loop at line 1904 (directly after `float best_err1 = 1e+9;`): `for (uniform int p = 0; p < 2; p++)` → `for (uniform int p = (pParams->m_force_pbit1 ? 1 : 0); p < 2; p++)`
8. In `handle_opaque_block`, inside `if (pComp_params->m_opaque_settings.m_use_mode[6])`: after `		pParams->m_perceptual = pComp_params->m_perceptual;` add `		pParams->m_force_pbit1 = pComp_params->m_exact_opaque_alpha;`, and after `		best_err = color_cell_compression(6, pParams, &results6, pComp_params, 16, pPixels, true);` (line 3934) add `		pParams->m_force_pbit1 = false;`
9. In `handle_opaque_block_mode6` the same two insertions (after its `pParams->m_perceptual = …;` and after its `color_cell_compression(6, …)` call at line 4656).

Produce the patch file (git exits 1 when files differ; that is expected):
```powershell
git diff --no-index --text --output=crates/encoders-sys/vendor/bc7e/patches/0001-exact-opaque-alpha.patch crates/encoders-sys/vendor/bc7enc_rdo/bc7e.ispc crates/encoders-sys/vendor/bc7e/bc7e.ispc
```

- [ ] **Step 5: Regenerate the bc7e objects from the patched source**

In `tools/regen-ispc.ps1` change the `$Bc7eSource` default to `"$PSScriptRoot\..\crates\encoders-sys\vendor\bc7e\bc7e.ispc"`. Then:
```powershell
pwsh -File tools/regen-ispc.ps1 -Update -Bc7eOnly
pwsh -File tools/hash-vendor.ps1
Select-String -Path crates/encoders-sys/vendor/ispc-objs/bc7e/bc7e_ispc.h -Pattern m_exact_opaque_alpha
```
Expected: the `Select-String` prints the field. Update `README-VENDOR.md` row `ispc-objs/bc7e/` to "built from bc7e/bc7e.ispc (pristine + patch 0001)".

- [ ] **Step 6: Run the tests**

Run: `cargo test --release -p encoders-sys; cargo test --release -p sdc-bench --test gates`
Expected: everything PASSES, including Task 9's gate (flag-off numbers unchanged) and the two new S5c tests.

If the flag-on gate reports texels ≠ 255: dump one failing block (index from the gate), check `block[0] & 0x7f == 0x40` (mode 6) and bits 63/64; a remaining p-bit 0 means another mode-6 path picks p-bits — search `bc7e.ispc` for `m_pbits[` assignments reached from `color_cell_compression` and constrain that path the same way. If not solved within the spike day, revert Steps 4–5 (`git checkout -- crates/encoders-sys/vendor tools/regen-ispc.ps1`), keep the byte patch as the shipped mechanism, and record "native patch moved to Phase 2 (same gate)".

- [ ] **Step 7: Commit**

```bash
git add crates/encoders-sys tools crates/bench
git commit -m "feat(bc7e): patch 0001 exact opaque alpha (mode-6 p-bit = 1), objects rebuilt with ISPC 1.31.0; S5c gates"
```

- [ ] **Step 8: Record S5 in DECISIONS.md**

Append (values from `target/bench/s5a.txt`, `s5b.txt` and `cargo run --release -p sdc-bench -- gate exact-opaque-native`):
```markdown
## Spike S5 — smallest-mip alpha check and exact opaque alpha cost (<date>)

- Check implemented (`engine::validate::alpha`); agrees with Note C (bc7e -u6 Bricks104 254 share <value>%) and NVTT (0%).
- Native patch 0001: flag off bit-identical on <n>/39 images; flag on: alpha == 255 on every texel of rgb_real, rgb_grad, stray_* (mip 0).
  Mean dRGB per set: rgb_real <v>, rgb_grad <v>, stray_real <v>, stray_grad <v> dB; worst image <name> <v> dB.
- Byte-patch fallback: mean dRGB rgb_real <v>, rgb_grad <v>, stray_real <v>, stray_grad <v> dB.
- Full-chain (every mip) part of the gate: Task 38.
- Shipped mechanism: <native patch | byte patch>. Open item §14 #5: dRGB <= 0.05 dB on all opaque sets → <propose / do not propose> exact_opaque on everywhere (owner decides).
```
```bash
git add docs/DECISIONS.md
git commit -m "docs: record spike S5 (mip 0) results"
```

---

### Task 15: Spike S4 — scalar bc7e (`basisu_bc7e_scalar.cpp`) as a fallback (non-blocking; OWNER APPROVAL for the download)

Question (spec §12 S4): is the pure C++17 port of bc7e viable? Pass: ΔPSNR ≤ 0.01 dB per image vs ISPC u5, throughput recorded; adopt only if ≤ 4× slower. The scalar port exports the same API names as `bc7e.ispc`, so it cannot share a binary with the ISPC objects: the spike is a standalone crate outside the workspace members.

**Files:**
- Create: `spikes/s4-scalar/Cargo.toml`, `spikes/s4-scalar/build.rs`, `spikes/s4-scalar/shim.cpp`, `spikes/s4-scalar/src/main.rs`, `spikes/s4-scalar/vendor/basisu_bc7e_scalar.cpp` (downloaded)
- Modify: `crates/bench/src/main.rs` (add `speed-bc7e`)
- Test: the spike's own run output (it is a measurement, not shipped code)

**Interfaces:**
- Consumes: `crates/bench/src/metrics.rs`, `crates/bench/src/corpus.rs` via `#[path]` (no link to `encoders-sys`).
- Produces: a recorded measurement in `docs/DECISIONS.md`.

- [ ] **Step 1: Download — ASK THE OWNER FIRST**

Tell the owner: "Spike S4 needs `encoder/basisu_bc7e_scalar.cpp` from https://github.com/BinomialLLC/basis_universal (Apache-2.0, one file, ~300 KB). OK to download into `spikes/s4-scalar/vendor/`?" On a yes:
```powershell
New-Item -ItemType Directory -Force spikes/s4-scalar/vendor | Out-Null
$sha = (Invoke-RestMethod https://api.github.com/repos/BinomialLLC/basis_universal/commits/master).sha
Invoke-WebRequest "https://raw.githubusercontent.com/BinomialLLC/basis_universal/$sha/encoder/basisu_bc7e_scalar.cpp" -OutFile spikes/s4-scalar/vendor/basisu_bc7e_scalar.cpp
"$sha" | Set-Content spikes/s4-scalar/vendor/COMMIT
```
Treat the file as untrusted input: read it before compiling (it is compiled, not executed by the build).

- [ ] **Step 2: Learn the port's API surface**

Run: `Select-String -Path spikes/s4-scalar/vendor/basisu_bc7e_scalar.cpp -Pattern '^namespace|bc7e_compress_blocks\(|bc7e_compress_block_init\(|params_init_veryslow\(|#include'`
Note the enclosing namespace (if any) and any includes it needs from basis_universal; if it includes other basisu headers, download those too (same approval) or stop and record "not self-contained".

- [ ] **Step 3: Write the spike crate**

`spikes/s4-scalar/Cargo.toml`:
```toml
[package]
name = "s4-scalar"
version = "0.0.0"
edition = "2021"
publish = false

[workspace]

[dependencies]
anyhow = "1"
image = { version = "0.25", default-features = false, features = ["png"] }
bcdec_rs = "0.2"
rayon = "1"

[build-dependencies]
cc = "1"

[profile.release]
codegen-units = 1
```
`spikes/s4-scalar/build.rs`:
```rust
fn main() {
    cc::Build::new()
        .cpp(true)
        .std("c++17")
        .flag("/EHsc")
        .opt_level(3)
        .file("shim.cpp")
        .compile("s4scalar");
    println!("cargo::rerun-if-changed=shim.cpp");
    println!("cargo::rerun-if-changed=vendor/basisu_bc7e_scalar.cpp");
}
```
`spikes/s4-scalar/shim.cpp` (set `SCALAR_NS` to the namespace found in Step 2, or leave it empty when the functions are global):
```cpp
#include "vendor/basisu_bc7e_scalar.cpp"
#define SCALAR_NS /* e.g. bc7e_scalar:: */
extern "C" void s4_init() { SCALAR_NS bc7e_compress_block_init(); }
extern "C" void s4_compress(uint32_t n, uint64_t* blocks, const uint32_t* px) {
    static SCALAR_NS bc7e_compress_block_params p;
    static bool once = false;
    if (!once) { memset(&p, 0, sizeof(p)); SCALAR_NS bc7e_compress_block_params_init_veryslow(&p, false); once = true; }
    SCALAR_NS bc7e_compress_blocks(n, blocks, px, &p);
}
```
(Call `s4_init` once from main before any thread starts; the static `p` is initialised on the first call from main, single-threaded.)

`spikes/s4-scalar/src/main.rs`:
```rust
#[path = "../../../crates/bench/src/metrics.rs"]
mod metrics;
use rayon::prelude::*;
use std::time::Instant;

extern "C" {
    fn s4_init();
    fn s4_compress(n: u32, blocks: *mut u64, px: *const u32);
}

fn main() -> anyhow::Result<()> {
    let root = std::path::PathBuf::from(std::env::var("SDC_BENCH_ROOT").unwrap_or(r"F:\ai\dds\bench".into()));
    unsafe { s4_init() };
    let warm = [0u32; 16];
    let mut wb = [0u64; 2];
    unsafe { s4_compress(1, wb.as_mut_ptr(), warm.as_ptr()) };
    let (mut mpix, mut secs1, mut secs_n) = (0.0, 0.0, 0.0);
    for set in ["rgb_real", "alpha_real"] {
        let mut files: Vec<_> = std::fs::read_dir(root.join("images").join(set))?.filter_map(|e| e.ok().map(|e| e.path())).collect();
        files.sort();
        for f in files {
            let img = image::open(&f)?.to_rgba8();
            let (w, h) = img.dimensions();
            let px: Vec<[u8; 4]> = img.pixels().map(|p| p.0).collect();
            let mut blocks = vec![0u32; (w * h) as usize];
            let bw = (w / 4) as usize;
            for (i, chunk) in blocks.chunks_mut(16).enumerate() {
                let (bx, by) = (i % bw, i / bw);
                for t in 0..16 {
                    chunk[t] = u32::from_le_bytes(px[(by * 4 + t / 4) * w as usize + bx * 4 + t % 4]);
                }
            }
            let n = blocks.len() / 16;
            let mut out1 = vec![0u64; n * 2];
            let t = Instant::now();
            unsafe { s4_compress(n as u32, out1.as_mut_ptr(), blocks.as_ptr()) };
            secs1 += t.elapsed().as_secs_f64();
            let mut out_n = vec![0u64; n * 2];
            let t = Instant::now();
            out_n.par_chunks_mut(128).zip(blocks.par_chunks(64 * 16)).for_each(|(o, b)| unsafe {
                s4_compress((b.len() / 16) as u32, o.as_mut_ptr(), b.as_ptr())
            });
            secs_n += t.elapsed().as_secs_f64();
            mpix += (w * h) as f64 / 1e6;
            let bytes: Vec<u8> = out_n.iter().flat_map(|v| v.to_le_bytes()).collect();
            let mut dec = vec![[0u8; 4]; (w * h) as usize];
            for i in 0..n {
                let mut o = [0u8; 64];
                bcdec_rs::bc7(&bytes[i * 16..i * 16 + 16], &mut o, 16);
                let (bx, by) = (i % bw, i / bw);
                for t in 0..16 {
                    dec[(by * 4 + t / 4) * w as usize + bx * 4 + t % 4].copy_from_slice(&o[t * 4..t * 4 + 4]);
                }
            }
            let bench = std::fs::read(root.join("out/bc7e_u5").join(set).join(f.file_stem().unwrap()).with_extension("dds"))?;
            let off = if &bench[84..88] == b"DX10" { 148 } else { 128 };
            println!(
                "{set}/{}: rgb {:.4} a {:.4} bytes-identical-to-ISPC {}",
                f.file_stem().unwrap().to_string_lossy(),
                metrics::rgb_psnr(&px, &dec),
                metrics::a_psnr(&px, &dec),
                bench[off..] == bytes[..]
            );
        }
    }
    println!("scalar u5: {:.3} Mpix/s single-thread, {:.3} Mpix/s on {} threads", mpix / secs1, mpix / secs_n, rayon::current_num_threads());
    Ok(())
}
```
This assumes the bench images are multiples of 4 (they are).

Add to `crates/bench/src/main.rs` a `SpeedBc7e` subcommand that times `codec::encode_bc7_image` (u5) over `rgb_real` + `alpha_real` and prints `ISPC u5: <x> Mpix/s on <n> threads` (and the same with `rayon::ThreadPoolBuilder::new().num_threads(1)` for single-thread):
```rust
    /// Throughput of ISPC bc7e u5 over rgb_real + alpha_real (for spike S4).
    SpeedBc7e,
```
```rust
        Cmd::SpeedBc7e => {
            let mut imgs = vec![];
            for set in ["rgb_real", "alpha_real"] {
                for f in sdc_bench::corpus::set_files(&root, "images", set, "png").unwrap() {
                    imgs.push(sdc_bench::corpus::load_png_rgba8(&f).unwrap());
                }
            }
            let mpix: f64 = imgs.iter().map(|i| (i.width * i.height) as f64 / 1e6).sum();
            let run = || {
                let t = std::time::Instant::now();
                for img in &imgs {
                    let _ = sdc_bench::codec::encode_bc7_image(img, gates::U5);
                }
                mpix / t.elapsed().as_secs_f64()
            };
            let all = run();
            let one = rayon::ThreadPoolBuilder::new().num_threads(1).build().unwrap().install(run);
            println!("ISPC u5: {one:.3} Mpix/s single-thread, {all:.3} Mpix/s on {} threads", rayon::current_num_threads());
            ExitCode::SUCCESS
        }
```
Both the spike and `speed-bc7e` time only the encode calls (images are loaded first).

- [ ] **Step 4: Run both and compare**

```powershell
cargo run --release --manifest-path spikes/s4-scalar/Cargo.toml > target/bench/s4.txt
cargo run --release -p sdc-bench -- speed-bc7e >> target/bench/s4.txt
```
Pass criteria: every image's `rgb`/`a` within 0.01 dB of the bench `bc7e_u5` values (Task 9 table / results.csv); scalar multi-thread Mpix/s ≥ ISPC multi-thread Mpix/s ÷ 4.

- [ ] **Step 5: Record and commit**

Append to `docs/DECISIONS.md`:
```markdown
## Spike S4 — scalar bc7e fallback (<date>)

- basis_universal commit <sha>; self-contained: <yes/no>.
- Quality: max per-image |dPSNR| vs ISPC u5 = <v> dB; bytes identical on <n>/14 images.
- Speed: scalar <a> Mpix/s (1 thread), <b> Mpix/s (16 threads); ISPC u5 <c> / <d> Mpix/s → scalar is <x>× slower.
- Verdict: <adopt as fallback (≤ 4×) | do not adopt>. Open item §14 #11 stays "not built" unless ISPC objects fail on a needed configuration.
```
```bash
git add spikes/s4-scalar crates/bench docs/DECISIONS.md
git commit -m "spike(S4): measure scalar bc7e port against ISPC u5"
```

---

### Task 16: Spike S7 — NVTT fed one mip at a time (non-blocking)

Question (spec §12 S7): can `nvcompress` encode pre-built levels one by one so the engine assembles the chain? Pass: output passes §5.8 (decoded alpha 255 with `-noalpha`); mip-0 PSNR equals the bench's direct `nvtt_production` encode within 0.01 dB; overhead ≤ 0.2 s per level; flags confirmed in `nvcompress -h`. NVTT 3.2.5 is installed at `C:\Program Files\NVIDIA Corporation\NVIDIA Texture Tools\` (no download).

**Files:**
- Create: `crates/bench/src/s7.rs`
- Modify: `crates/bench/src/lib.rs` (`pub mod s7;`), `crates/bench/src/main.rs` (`S7Nvtt` subcommand)
- Test: `crates/bench/tests/s7.rs`

**Interfaces:**
- Produces: `sdc_bench::s7::nvcompress_path() -> PathBuf`; `sdc_bench::s7::box_down_rgba8(img: &Rgba8) -> Rgba8`; `sdc_bench::s7::run(root: &Path, work: &Path) -> anyhow::Result<GateReport>`

- [ ] **Step 1: Write the failing test**

`crates/bench/tests/s7.rs`:
```rust
use sdc_bench::{corpus::{bench_root, Rgba8}, s7};

#[test]
fn box_down_averages_2x2_with_rounding() {
    let img = Rgba8 { width: 2, height: 2, px: vec![[0, 0, 0, 255], [1, 0, 0, 255], [1, 0, 0, 255], [1, 0, 0, 255]] };
    let d = s7::box_down_rgba8(&img);
    assert_eq!((d.width, d.height), (1, 1));
    assert_eq!(d.px[0], [1, 0, 0, 255]); // (0+1+1+1)/4 = 0.75 -> 1
}

#[test]
#[ignore = "spike S7: runs nvcompress ~70 times; run with --ignored"]
fn s7_nvtt_per_mip_feeding() {
    let work = tempfile::tempdir().unwrap();
    let r = s7::run(&bench_root(), work.path()).unwrap();
    assert!(r.pass, "{r}");
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --release -p sdc-bench --test s7`
Expected: FAIL to compile.

- [ ] **Step 3: Implement**

`crates/bench/src/s7.rs`:
```rust
use crate::codec::decode_bc7_image;
use crate::corpus::*;
use crate::gates::GateReport;
use crate::{ddsraw, metrics};
use anyhow::{bail, Result};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

pub fn nvcompress_path() -> PathBuf {
    PathBuf::from(r"C:\Program Files\NVIDIA Corporation\NVIDIA Texture Tools\nvcompress.exe")
}

pub fn box_down_rgba8(img: &Rgba8) -> Rgba8 {
    let (w, h) = ((img.width / 2).max(1), (img.height / 2).max(1));
    let mut px = Vec::with_capacity((w * h) as usize);
    for y in 0..h {
        for x in 0..w {
            let mut acc = [0u32; 4];
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let sx = (x * 2 + dx).min(img.width - 1);
                let sy = (y * 2 + dy).min(img.height - 1);
                let p = img.px[(sy * img.width + sx) as usize];
                for c in 0..4 {
                    acc[c] += p[c] as u32;
                }
            }
            px.push(acc.map(|a| ((a + 2) / 4) as u8));
        }
    }
    Rgba8 { width: w, height: h, px }
}

fn save_png(img: &Rgba8, p: &Path) -> Result<()> {
    let flat: Vec<u8> = img.px.iter().flatten().copied().collect();
    image::save_buffer(p, &flat, img.width, img.height, image::ColorType::Rgba8)?;
    Ok(())
}

pub fn run(root: &Path, work: &Path) -> Result<GateReport> {
    let nv = nvcompress_path();
    let mut r = GateReport::new("S7 NVTT per-mip feeding (BC7 Production, -noalpha)");
    let help = Command::new(&nv).arg("-h").output()?;
    let help = String::from_utf8_lossy(&help.stdout).to_string() + &String::from_utf8_lossy(&help.stderr);
    for flag in ["-nomips", "-bc7", "-bc6", "-production", "-noalpha"] {
        r.check(help.contains(flag), format!("nvcompress -h lists {flag}"));
    }
    r.info(format!("nvcompress banner: {}", help.lines().next().unwrap_or("")));
    let rows = read_results(&root.join("results/results.csv"), "nvtt_production")?;
    let mut per_level = vec![];
    for src in set_files(root, "images", "rgb_real", "png")?.into_iter().take(3) {
        let stem = stem(&src);
        let mut level = load_png_rgba8(&src)?;
        let mut chain_bytes = 0usize;
        for l in 0.. {
            let png = work.join(format!("{stem}_{l}.png"));
            let dds = work.join(format!("{stem}_{l}.dds"));
            save_png(&level, &png)?;
            let t = Instant::now();
            let out = Command::new(&nv).args(["-silent", "-nomips", "-bc7", "-production", "-noalpha"]).arg(&png).arg(&dds).output()?;
            per_level.push(t.elapsed().as_secs_f64());
            if !out.status.success() {
                bail!("nvcompress failed on {}", png.display());
            }
            let bytes = std::fs::read(&dds)?;
            let payload = ddsraw::payload(&bytes)?;
            chain_bytes += payload.len();
            let dec = decode_bc7_image(payload, level.width, level.height);
            let a = engine::validate::alpha::alpha_stats(&dec);
            r.check(a.eq255 == a.pixels, format!("{stem} mip {l} ({}x{}): decoded alpha all 255", level.width, level.height));
            if l == 0 {
                let p = metrics::rgb_psnr(&level.px, &dec);
                let e = find(&rows, "rgb_real", &stem).and_then(|row| row.f("rgb_psnr")).unwrap_or(f64::NAN);
                r.check((p - e).abs() <= 0.01, format!("{stem} mip 0 RGB PSNR {p:.4} vs direct encode {e:.4}"));
            }
            if level.width == 1 && level.height == 1 {
                break;
            }
            level = box_down_rgba8(&level);
        }
        r.info(format!("{stem}: assembled chain payload {chain_bytes} bytes"));
    }
    let mean = metrics::mean(&per_level);
    let max = per_level.iter().cloned().fold(0.0, f64::max);
    r.check(mean <= 0.2, format!("per-level nvcompress time: mean {mean:.3} s, max {max:.3} s (limit 0.2 s mean)"));
    Ok(r)
}
```
Add an `S7Nvtt` subcommand to `main.rs`:
```rust
    /// Spike S7: NVTT fed one mip at a time.
    S7Nvtt,
```
```rust
        Cmd::S7Nvtt => {
            let work = std::path::PathBuf::from("target/bench/s7");
            std::fs::create_dir_all(&work).unwrap();
            match sdc_bench::s7::run(&root, &work) {
                Ok(r) => {
                    println!("{r}");
                    if r.pass { ExitCode::SUCCESS } else { ExitCode::from(1) }
                }
                Err(e) => {
                    eprintln!("error: {e:#}");
                    ExitCode::from(3)
                }
            }
        }
``` The BC6H half of S7 (`-bc6` on RGBA16F levels) is deferred to milestone M5 where the NVTT backend is built; this spike confirms the flag exists and the per-level mechanics.

- [ ] **Step 4: Run**

Run: `cargo test --release -p sdc-bench --test s7; cargo test --release -p sdc-bench --test s7 -- --ignored`
Expected: unit test PASS; the ignored spike prints its report. A failure of the 0.2 s overhead check is a measurement, not a bug: record it.

- [ ] **Step 5: Record and commit**

Append to `docs/DECISIONS.md`:
```markdown
## Spike S7 — NVTT per-mip feeding (<date>)

- nvcompress <banner version>; flags present: -nomips -bc7 -bc6 -production -noalpha: <yes/no each>.
- Decoded alpha 255 at every level: <yes/no>. Mip-0 PSNR vs direct encode: max |Δ| <v> dB.
- Per-level overhead: mean <v> s, max <v> s (limit 0.2 s).
- Verdict for open item §14 #10: <one -nomips process per level | NVTT mip 0 only>.
```
```bash
git add crates/bench docs/DECISIONS.md
git commit -m "spike(S7): NVTT per-mip feeding measured"
```

---

### Task 17: Phase 0 exit review (owner decision point)

**Files:**
- Modify: `docs/DECISIONS.md`

- [ ] **Step 1: Run every Phase 0 gate in one go**

Run: `cargo test --release --workspace; cargo run --release -p sdc-bench -- gate s3; cargo run --release -p sdc-bench -- gate rgbcx; cargo run --release -p sdc-bench -- gate bc6h; cargo run --release -p sdc-bench -- gate alpha-scan`
Expected: all PASS (S4/S7 results are recorded, not gating).

- [ ] **Step 2: Write the Phase 0 summary**

Append to `docs/DECISIONS.md`:
```markdown
## Phase 0 exit (<date>)

| Gate | Result |
|---|---|
| S3 bc7e u5 rgb_real within 0.01 dB, BC4 bit-exact | <PASS/FAIL> |
| rgbcx BC1/BC3/BC4/BC5 bit-exact (+ transparent-black off vs bc7enc -b) | <PASS/FAIL> |
| ISPC BC6H veryslow 51.50 dB | <PASS/FAIL> |
| S5 exact opaque (mip 0): mechanism <native/byte patch>, dRGB <v> dB | <PASS/FAIL> |
| S4 scalar bc7e | <adopt/do not adopt> |
| S7 NVTT per mip | <viable/mip 0 only> |
| ISPC regeneration byte-reproducible (Task 3 Step 8) | <yes/no> |

Question for the owner (DECISIONS #14): confirm Rust for the engine (S3 and S4 are in). Phase 1 starts after a yes.
```

- [ ] **Step 3: Ask the owner**

Tell the owner the table and ask: "Confirm Rust (DECISIONS #14); if S5 measured dRGB ≤ 0.05 dB, should `alpha.exact_opaque` default on everywhere (open item §14 #5)?; and do you accept that the MVP renormalises `_n` without the variance bake (DECISIONS #6), which arrives in milestone M4?" Record the answers verbatim under the summary.

- [ ] **Step 4: Commit**

```bash
git add docs/DECISIONS.md
git commit -m "docs: Phase 0 exit summary and owner decisions"
```

---
# Phase 1 — Core engine

Entry: Task 17 recorded, owner confirmed Rust. Exit: Tasks 18–39 pass, including Task 26a (preset suffix aliases, DECISIONS Note G), the Task 38 gates (full-chain exact opaque, engine-path parity) and the Task 39 golden tests.

Rules for every Phase 1 task: code goes under `crates/engine/src/`; run `cargo test -p engine` (debug is fine; it uses opt-level 1) and `cargo clippy -p engine --all-targets -- -D warnings` before committing. The snippets favour explicit index loops for readability; if clippy flags `needless_range_loop`, `manual_is_multiple_of` or `new_without_default`, apply its suggested rewrite (behaviour-neutral) rather than adding `allow`s.

Scope note (owner to confirm at Task 17): DECISIONS #6's variance bake into `_rmaos`/`_rfaos` roughness is milestone M4. Until then a PBR set with `_n` and a roughness member is converted with plain renormalisation, and `set_resolved` reports `bake: false`.

### Task 18: Engine foundations — image type and the warning-code catalogue

**Files:**
- Modify: `crates/engine/Cargo.toml` (full dependency list for Phase 1), `crates/engine/src/lib.rs`
- Create: `crates/engine/src/image.rs`, `crates/engine/src/codes.rs`
- Test: unit tests in both files

**Interfaces:**
- Produces:
  - `pub struct Rgba32F { pub width: u32, pub height: u32, pub px: Vec<[f32; 4]> }` with `fn new(width: u32, height: u32, fill: [f32; 4]) -> Self`, `fn at(&self, x: u32, y: u32) -> [f32; 4]`, `fn at_mut(&mut self, x: u32, y: u32) -> &mut [f32; 4]`, `fn est_bytes(&self) -> u64`
  - `pub struct SourceInfo { pub bit_depth: u8, pub channels: u8, pub has_alpha: bool, pub is_float: bool }` (serde, JsonSchema)
  - `pub enum Severity { Info, Warning, Blocking, Error }` (serde lowercase, `Ord`)
  - `pub enum Code { … }` — every §10 code plus **`VAL-MIP-ERROR`** (warning) and **`CFG-INVALID`** (error), serialised as the code string; `fn severity(self) -> Severity`, `fn as_str(self) -> &'static str`, `fn parse(s: &str) -> Option<Code>`, `fn effective(self, allow: &BTreeSet<Code>) -> Severity`, `const ALL: &[Code]`
  - `pub struct Warning { pub code: Code, pub severity: Severity, pub message: String, pub data: serde_json::Value }` with `fn new(code: Code, message: impl Into<String>) -> Self`, `fn with_data(self, data: serde_json::Value) -> Self`

- [ ] **Step 1: Set the engine's dependencies**

`crates/engine/Cargo.toml` `[dependencies]` and `[dev-dependencies]`:
```toml
[dependencies]
encoders-sys.workspace = true
serde.workspace = true
serde_json.workspace = true
toml.workspace = true
schemars.workspace = true
thiserror.workspace = true
image.workspace = true
half.workspace = true
bcdec_rs.workspace = true
rayon.workspace = true
sha2.workspace = true
hex.workspace = true
globset.workspace = true
walkdir.workspace = true
tempfile.workspace = true
humantime.workspace = true

[dev-dependencies]
tempfile.workspace = true
```

- [ ] **Step 2: Write the failing tests**

`crates/engine/src/codes.rs` — tests at the bottom (implementation in Step 4):
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn severities_follow_spec_section_10() {
        assert_eq!(Code::FmtBc5Normal.severity(), Severity::Blocking);
        assert_eq!(Code::TagSrgbVanilla.severity(), Severity::Error);
        assert_eq!(Code::Alpha254.severity(), Severity::Info);
        assert_eq!(Code::ValCoverage.severity(), Severity::Warning);
        assert_eq!(Code::ValMipError.severity(), Severity::Warning);
        assert_eq!(Code::CfgInvalid.severity(), Severity::Error);
    }

    #[test]
    fn codes_serialise_as_their_strings_and_are_unique() {
        assert_eq!(serde_json::to_string(&Code::LossAlphaData).unwrap(), "\"LOSS-ALPHA-DATA\"");
        assert_eq!(Code::parse("VAL-PGPATCHER-50"), Some(Code::ValPgpatcher50));
        let names: BTreeSet<_> = Code::ALL.iter().map(|c| c.as_str()).collect();
        assert_eq!(names.len(), Code::ALL.len());
        assert_eq!(Code::ALL.len(), 47);
    }

    #[test]
    fn allow_downgrades_blocking_only() {
        let allow: BTreeSet<Code> = [Code::FmtBc5Normal, Code::TagSrgbVanilla].into();
        assert_eq!(Code::FmtBc5Normal.effective(&allow), Severity::Warning);
        assert_eq!(Code::TagSrgbVanilla.effective(&allow), Severity::Error);
    }
}
```
`crates/engine/src/image.rs` tests:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indexing_is_row_major() {
        let mut img = Rgba32F::new(3, 2, [0.0; 4]);
        *img.at_mut(2, 1) = [1.0, 0.5, 0.25, 1.0];
        assert_eq!(img.px[5], [1.0, 0.5, 0.25, 1.0]);
        assert_eq!(img.at(2, 1)[1], 0.5);
        assert_eq!(img.est_bytes(), 3 * 2 * 16);
    }
}
```

- [ ] **Step 3: Run them to verify they fail**

Run: `cargo test -p engine -- codes image`
Expected: FAIL to compile.

- [ ] **Step 4: Implement**

`crates/engine/src/image.rs`:
```rust
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Float RGBA image, row-major. Values are in the stage's own domain (stored, linear light, vector, …).
#[derive(Debug, Clone, PartialEq)]
pub struct Rgba32F {
    pub width: u32,
    pub height: u32,
    pub px: Vec<[f32; 4]>,
}

impl Rgba32F {
    pub fn new(width: u32, height: u32, fill: [f32; 4]) -> Self {
        Rgba32F { width, height, px: vec![fill; (width as usize) * (height as usize)] }
    }
    pub fn at(&self, x: u32, y: u32) -> [f32; 4] {
        self.px[(y * self.width + x) as usize]
    }
    pub fn at_mut(&mut self, x: u32, y: u32) -> &mut [f32; 4] {
        &mut self.px[(y * self.width + x) as usize]
    }
    pub fn est_bytes(&self) -> u64 {
        self.px.len() as u64 * 16
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SourceInfo {
    pub bit_depth: u8,
    pub channels: u8,
    pub has_alpha: bool,
    pub is_float: bool,
}
```

`crates/engine/src/codes.rs`:
```rust
//! Warning and error codes (spec §10). Two codes are added by this plan: VAL-MIP-ERROR (§5.8 per-mip error
//! warning, which §10 does not name) and CFG-INVALID (a field value outside its allowed range or an
//! encoder/format mismatch).
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Info,
    Warning,
    Blocking,
    Error,
}

macro_rules! codes {
    ($($v:ident = $s:literal => $sev:ident),+ $(,)?) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema)]
        pub enum Code { $(#[serde(rename = $s)] $v),+ }
        impl Code {
            pub const ALL: &'static [Code] = &[$(Code::$v),+];
            pub fn as_str(self) -> &'static str { match self { $(Code::$v => $s),+ } }
            pub fn severity(self) -> Severity { match self { $(Code::$v => Severity::$sev),+ } }
            pub fn parse(s: &str) -> Option<Code> { match s { $($s => Some(Code::$v),)+ _ => None } }
        }
    };
}

codes! {
    LossAlphaData = "LOSS-ALPHA-DATA" => Blocking,
    LossClampHdr = "LOSS-CLAMP-HDR" => Warning,
    LossBitdepth = "LOSS-BITDEPTH" => Info,
    LossReencode = "LOSS-REENCODE" => Warning,
    ChanNotGray = "CHAN-NOT-GRAY" => Warning,
    ChanConstantAlphaData = "CHAN-CONSTANT-ALPHA-DATA" => Info,
    ChanNormalLength = "CHAN-NORMAL-LENGTH" => Warning,
    ChanAoInverted = "CHAN-AO-INVERTED" => Warning,
    ChanGlossAsRough = "CHAN-GLOSS-AS-ROUGH" => Warning,
    FmtBc5Normal = "FMT-BC5-NORMAL" => Blocking,
    FmtLandscapeBc1 = "FMT-LANDSCAPE-BC1" => Blocking,
    FmtBc1Alpha = "FMT-BC1-ALPHA" => Blocking,
    FmtPerceptualData = "FMT-PERCEPTUAL-DATA" => Error,
    AlphaExactOffM = "ALPHA-EXACT-OFF-M" => Warning,
    Alpha254 = "ALPHA-254" => Info,
    TagSrgbVanilla = "TAG-SRGB-VANILLA" => Error,
    TagSrgbNonpbr = "TAG-SRGB-NONPBR" => Blocking,
    TagUnormPbr = "TAG-UNORM-PBR" => Warning,
    TagSrgbLod = "TAG-SRGB-LOD" => Blocking,
    TagContentMismatch = "TAG-CONTENT-MISMATCH" => Warning,
    CfgUnavailable = "CFG-UNAVAILABLE" => Blocking,
    CfgInvalid = "CFG-INVALID" => Error,
    DimNpot = "DIM-NPOT" => Blocking,
    DimSmall = "DIM-SMALL" => Blocking,
    DimFlowSmall = "DIM-FLOW-SMALL" => Warning,
    DimTooLarge = "DIM-TOO-LARGE" => Error,
    SetMissingCompanion = "SET-MISSING-COMPANION" => Info,
    SetCompanionOutside = "SET-COMPANION-OUTSIDE" => Info,
    SetCompanionDds = "SET-COMPANION-DDS" => Warning,
    SetBakeRatio = "SET-BAKE-RATIO" => Warning,
    SugLowConfidence = "SUG-LOW-CONFIDENCE" => Blocking,
    SugContradiction = "SUG-CONTRADICTION" => Warning,
    ValAlphaGuarantee = "VAL-ALPHA-GUARANTEE" => Error,
    ValCoverage = "VAL-COVERAGE" => Warning,
    ValMComplexMargin = "VAL-M-COMPLEX-MARGIN" => Warning,
    ValMGrayscale = "VAL-M-GRAYSCALE" => Warning,
    ValMHeightClip = "VAL-M-HEIGHT-CLIP" => Warning,
    ValPgpatcher50 = "VAL-PGPATCHER-50" => Warning,
    ValBc6hPeak = "VAL-BC6H-PEAK" => Warning,
    ValHeader = "VAL-HEADER" => Error,
    ValMipError = "VAL-MIP-ERROR" => Warning,
    EncNvttMissing = "ENC-NVTT-MISSING" => Blocking,
    EncNvttVersion = "ENC-NVTT-VERSION" => Blocking,
    InDuplicate = "IN-DUPLICATE" => Blocking,
    InDdsSkipped = "IN-DDS-SKIPPED" => Info,
    InDecode = "IN-DECODE" => Error,
    OutUnknownExisting = "OUT-UNKNOWN-EXISTING" => Warning,
}

impl Code {
    /// `--allow <code>` (or GUI acknowledgement) turns a blocking code into a warning; errors stay errors.
    pub fn effective(self, allow: &BTreeSet<Code>) -> Severity {
        match self.severity() {
            Severity::Blocking if allow.contains(&self) => Severity::Warning,
            s => s,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Warning {
    pub code: Code,
    pub severity: Severity,
    pub message: String,
    #[serde(default)]
    pub data: serde_json::Value,
}

impl Warning {
    pub fn new(code: Code, message: impl Into<String>) -> Self {
        Warning { code, severity: code.severity(), message: message.into(), data: serde_json::Value::Null }
    }
    pub fn with_data(mut self, data: serde_json::Value) -> Self {
        self.data = data;
        self
    }
}
```
`crates/engine/src/lib.rs`:
```rust
//! sdc engine: configuration, policy, pipeline, DDS I/O, validation, events.
pub mod codes;
pub mod image;
pub mod validate;
```

- [ ] **Step 5: Run the tests**

Run: `cargo test -p engine -- codes image`
Expected: 4 passed.

- [ ] **Step 6: Commit**

```bash
git add crates/engine
git commit -m "feat(engine): Rgba32F image type and the warning-code catalogue"
```

---

### Task 19: Configuration model (`Profile` and every §3.2 field) with JSON Schema

**Files:**
- Create: `crates/engine/src/config.rs`
- Modify: `crates/engine/src/lib.rs` (`pub mod config;`)
- Test: unit tests in `config.rs`

**Interfaces:**
- Produces (all `Debug, Clone, Copy (enums), PartialEq, Serialize, Deserialize, JsonSchema`; structs carry `#[serde(deny_unknown_fields)]`):
  - `Target { Cs, Cpm, Vanilla }` (lowercase)
  - `Category { Color, Normal, MixedPacked, Grayscale, Hdr }` (snake_case)
  - `Subtype { Diffuse, LandscapeDiffuse, PbrAlbedo, PbrEmissive, Glow, PbrSubsurface, PbrFuzz, SubsurfaceTint, BacklightTint, UiSky, Normal, LandscapeNormal, PbrNormal, Msn, CoatNormal, Envmask, ComplexMaterial, Rmaos, Rfaos, Flow, Height, MsnSpecular, GenericTwoChannel, GenericData, HdrSky }` (snake_case) with `const ALL: [Subtype; 25]`, `fn category(self) -> Category`, `fn is_pbr_color(self) -> bool`, `fn alpha_from_mesh(self) -> bool`, `fn is_roughness_member(self) -> bool`, `fn id(self) -> String`
  - `ColorContent { Srgb, Linear }`; `ColorTag { Srgb, Unorm, #[serde(rename = "n/a")] NotApplicable }`
  - `AlphaPolicy { Remove, Cutout, Blend, CutoutBlend, Data }`; `AlphaCompare { Gt, Ge }`; `GraySource { R, Luma }`
  - `MipLevels` (untagged): `Keyword(MipKeyword)` | `Count(u32)`, `MipKeyword { Full, None }`; `MipFilter { Box }` (Tent/Mitchell/Kaiser arrive in M6); `NormalMips { Renormalize, RenormalizeBake }`; `CoverageMode { Auto, On, Off }`
  - `Format { Bc1, Bc3, Bc4, Bc5, #[serde(rename = "bc6h_uf16")] Bc6hUf16, Bc7 }` with `fn block_bytes(self) -> usize`, `fn has_alpha(self) -> bool`
  - `HeaderForm { Auto, Dx10, Legacy }`; `Backend { Bc7e, Rgbcx, IspcBc6h, Nvtt }`; `Effort { Ultrafast, Veryfast, Fast, Basic, Veryslow, Slowest }`; `SearchRadius { Default, Max }`
  - `ColorSpec { content, tag }`, `AlphaSpec { policy, threshold: u8, compare, exact_opaque: bool }`, `ChannelSpec { swizzle: Option<String>, flip_green: bool, reconstruct_z: bool, grayscale_source: GraySource }`, `MipSpec { levels: MipLevels, filter: MipFilter, normal: NormalMips, coverage: CoverageMode }`, `EncoderSpec { backend, effort, rgbcx_level: u32, perceptual: bool, bc1_transparent_black: bool, bc45_search_radius: SearchRadius }`, `ValidationSpec { min_mip_psnr_db: f64, coverage_tolerance_pts: f64, complex_m_warn_alpha: u8, bc6h_min_peak_ratio: f64, pgpatcher_non_white_share: f64 }`
  - `Profile { target, category, subtype, color, alpha, channels, mips, format, header, encoder, validation }` with `fn coverage_enabled(&self) -> bool`, `fn alpha_weighted_mips(&self) -> bool`, `fn srgb_tag(&self) -> bool`

Deviations from spec §3.2, by design (recorded in the report): `header` and `mips.coverage` gain an `auto` value (spec's "legacy for BC1/BC3 UNORM, else dx10" and "on for cutout/cutout_blend" are computed from other fields); rgbcx's "level 18" lives in `encoder.rgbcx_level` because `encoder.effort` names are bc7e/BC6H levels; `mips.edge` is omitted until a non-box filter exists (box at power-of-two sizes ignores it [MIP]).

- [ ] **Step 1: Write the failing tests**

At the bottom of `crates/engine/src/config.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serde_names_match_the_spec() {
        assert_eq!(serde_json::to_string(&Format::Bc6hUf16).unwrap(), "\"bc6h_uf16\"");
        assert_eq!(serde_json::to_string(&ColorTag::NotApplicable).unwrap(), "\"n/a\"");
        assert_eq!(serde_json::to_string(&AlphaPolicy::CutoutBlend).unwrap(), "\"cutout_blend\"");
        assert_eq!(serde_json::to_string(&Subtype::MsnSpecular).unwrap(), "\"msn_specular\"");
        assert_eq!(serde_json::from_str::<MipLevels>("\"full\"").unwrap(), MipLevels::Keyword(MipKeyword::Full));
        assert_eq!(serde_json::from_str::<MipLevels>("3").unwrap(), MipLevels::Count(3));
    }

    #[test]
    fn subtypes_map_to_the_categories_of_section_4_1() {
        assert_eq!(Subtype::ALL.len(), 25);
        assert_eq!(Subtype::PbrEmissive.category(), Category::Color);
        assert_eq!(Subtype::Msn.category(), Category::Normal);
        assert_eq!(Subtype::Envmask.category(), Category::Grayscale);
        assert_eq!(Subtype::Flow.category(), Category::MixedPacked);
        assert_eq!(Subtype::HdrSky.category(), Category::Hdr);
        assert!(Subtype::Rfaos.is_roughness_member());
        assert!(Subtype::PbrFuzz.is_pbr_color());
        assert!(!Subtype::Glow.is_pbr_color());
        assert_eq!(Subtype::ComplexMaterial.id(), "complex_material");
    }

    #[test]
    fn schema_lists_enum_values() {
        let s = serde_json::to_string(&schemars::schema_for!(Profile)).unwrap();
        for needle in ["bc6h_uf16", "n/a", "cutout_blend", "renormalize_bake", "ispc_bc6h"] {
            assert!(s.contains(needle), "{needle} missing from schema");
        }
    }
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p engine config`
Expected: FAIL to compile.

- [ ] **Step 3: Implement `config.rs`**

```rust
//! The configuration model (spec §3.2): one set of types for CLI, GUI, presets and the published schema.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

macro_rules! cfg_enum {
    ($(#[$m:meta])* $name:ident, $case:literal { $($v:ident $(= $ren:literal)?),+ $(,)? }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema)]
        #[serde(rename_all = $case)]
        $(#[$m])*
        pub enum $name { $($(#[serde(rename = $ren)])? $v),+ }
    };
}

cfg_enum!(Target, "lowercase" { Cs, Cpm, Vanilla });
cfg_enum!(Category, "snake_case" { Color, Normal, MixedPacked, Grayscale, Hdr });
cfg_enum!(Subtype, "snake_case" {
    Diffuse, LandscapeDiffuse, PbrAlbedo, PbrEmissive, Glow, PbrSubsurface, PbrFuzz, SubsurfaceTint,
    BacklightTint, UiSky, Normal, LandscapeNormal, PbrNormal, Msn, CoatNormal, Envmask, ComplexMaterial,
    Rmaos, Rfaos, Flow, Height, MsnSpecular, GenericTwoChannel, GenericData, HdrSky
});
cfg_enum!(ColorContent, "lowercase" { Srgb, Linear });
cfg_enum!(ColorTag, "lowercase" { Srgb, Unorm, NotApplicable = "n/a" });
cfg_enum!(AlphaPolicy, "snake_case" { Remove, Cutout, Blend, CutoutBlend, Data });
cfg_enum!(AlphaCompare, "lowercase" { Gt, Ge });
cfg_enum!(GraySource, "lowercase" { R, Luma });
cfg_enum!(MipKeyword, "lowercase" { Full, None });
cfg_enum!(MipFilter, "lowercase" { Box });
cfg_enum!(NormalMips, "snake_case" { Renormalize, RenormalizeBake });
cfg_enum!(CoverageMode, "lowercase" { Auto, On, Off });
cfg_enum!(Format, "lowercase" { Bc1, Bc3, Bc4, Bc5, Bc6hUf16 = "bc6h_uf16", Bc7 });
cfg_enum!(HeaderForm, "lowercase" { Auto, Dx10, Legacy });
cfg_enum!(Backend, "snake_case" { Bc7e, Rgbcx, IspcBc6h, Nvtt });
cfg_enum!(Effort, "lowercase" { Ultrafast, Veryfast, Fast, Basic, Veryslow, Slowest });
cfg_enum!(SearchRadius, "lowercase" { Default, Max });

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum MipLevels {
    Keyword(MipKeyword),
    Count(u32),
}

impl Subtype {
    pub const ALL: [Subtype; 25] = {
        use Subtype::*;
        [
            Diffuse, LandscapeDiffuse, PbrAlbedo, PbrEmissive, Glow, PbrSubsurface, PbrFuzz, SubsurfaceTint,
            BacklightTint, UiSky, Normal, LandscapeNormal, PbrNormal, Msn, CoatNormal, Envmask, ComplexMaterial,
            Rmaos, Rfaos, Flow, Height, MsnSpecular, GenericTwoChannel, GenericData, HdrSky,
        ]
    };

    pub fn category(self) -> Category {
        use Subtype::*;
        match self {
            Diffuse | LandscapeDiffuse | PbrAlbedo | PbrEmissive | Glow | PbrSubsurface | PbrFuzz | SubsurfaceTint
            | BacklightTint | UiSky => Category::Color,
            Normal | LandscapeNormal | PbrNormal | Msn | CoatNormal => Category::Normal,
            Envmask | Height | MsnSpecular => Category::Grayscale,
            ComplexMaterial | Rmaos | Rfaos | Flow | GenericTwoChannel | GenericData => Category::MixedPacked,
            HdrSky => Category::Hdr,
        }
    }

    /// PBR colour data: the only sub-types that may carry `_SRGB` (cs only) [D#4, D#5].
    pub fn is_pbr_color(self) -> bool {
        matches!(self, Subtype::PbrAlbedo | Subtype::PbrEmissive | Subtype::PbrSubsurface | Subtype::PbrFuzz)
    }

    /// Alpha meaning depends on the mesh (NiAlphaProperty), so it is never a high-confidence guess [SC §1].
    pub fn alpha_from_mesh(self) -> bool {
        matches!(self, Subtype::Diffuse | Subtype::PbrAlbedo | Subtype::UiSky)
    }

    pub fn is_roughness_member(self) -> bool {
        matches!(self, Subtype::Rmaos | Subtype::Rfaos)
    }

    pub fn id(self) -> String {
        serde_json::to_value(self).unwrap().as_str().unwrap().to_owned()
    }
}

impl Format {
    pub fn block_bytes(self) -> usize {
        match self {
            Format::Bc1 | Format::Bc4 => 8,
            _ => 16,
        }
    }
    pub fn has_alpha(self) -> bool {
        matches!(self, Format::Bc1 | Format::Bc3 | Format::Bc7)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ColorSpec {
    pub content: ColorContent,
    pub tag: ColorTag,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AlphaSpec {
    pub policy: AlphaPolicy,
    /// 8-bit alpha-test threshold, per texture; allowed 5..=200 (checked in policy).
    pub threshold: u8,
    pub compare: AlphaCompare,
    pub exact_opaque: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ChannelSpec {
    /// Four characters from r, g, b, a, 0, 1 (e.g. "bgra", "rrr1"); None = identity.
    #[serde(default)]
    pub swizzle: Option<String>,
    pub flip_green: bool,
    pub reconstruct_z: bool,
    pub grayscale_source: GraySource,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MipSpec {
    pub levels: MipLevels,
    pub filter: MipFilter,
    pub normal: NormalMips,
    pub coverage: CoverageMode,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EncoderSpec {
    pub backend: Backend,
    pub effort: Effort,
    pub rgbcx_level: u32,
    pub perceptual: bool,
    pub bc1_transparent_black: bool,
    pub bc45_search_radius: SearchRadius,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ValidationSpec {
    pub min_mip_psnr_db: f64,
    pub coverage_tolerance_pts: f64,
    pub complex_m_warn_alpha: u8,
    pub bc6h_min_peak_ratio: f64,
    pub pgpatcher_non_white_share: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub target: Target,
    pub category: Category,
    pub subtype: Subtype,
    pub color: ColorSpec,
    pub alpha: AlphaSpec,
    pub channels: ChannelSpec,
    pub mips: MipSpec,
    pub format: Format,
    pub header: HeaderForm,
    pub encoder: EncoderSpec,
    pub validation: ValidationSpec,
}

impl Profile {
    /// Coverage preservation: `auto` = on for cutout and cutout_blend (DECISIONS Note E).
    pub fn coverage_enabled(&self) -> bool {
        match self.mips.coverage {
            CoverageMode::On => true,
            CoverageMode::Off => false,
            CoverageMode::Auto => matches!(self.alpha.policy, AlphaPolicy::Cutout | AlphaPolicy::CutoutBlend),
        }
    }
    pub fn alpha_weighted_mips(&self) -> bool {
        matches!(self.alpha.policy, AlphaPolicy::Cutout | AlphaPolicy::Blend | AlphaPolicy::CutoutBlend)
    }
    pub fn srgb_tag(&self) -> bool {
        self.color.tag == ColorTag::Srgb
    }
}
```
Add `pub mod config;` to `lib.rs`.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p engine config`
Expected: 3 passed. If `schema_for!` output lacks a renamed value, check that `#[serde(rename = …)]` sits on the variant (schemars honours serde attributes).

- [ ] **Step 5: Commit**

```bash
git add crates/engine
git commit -m "feat(engine): configuration model with serde and JSON Schema"
```

---

### Task 20: Presets — built-in data, inheritance, JSON-merge layering, registry

**Files:**
- Create: `presets/sdc-base.toml`, `presets/cs-quality.toml`, `presets/cpm-quality.toml`, `presets/vanilla-quality.toml`
- Create: `crates/engine/src/presets.rs`
- Modify: `crates/engine/src/lib.rs` (`pub mod presets;`)
- Test: `crates/engine/tests/presets.rs`

**Interfaces:**
- Consumes: `config::*` (Task 19).
- Produces:
  - `pub enum AlphaContent { None, Constant255, Varying }` (snake_case serde)
  - `pub struct RuleMatch { pub glob: Option<String>, pub suffix: Option<String>, pub set_has: Option<String>, pub alpha: Option<AlphaContent> }`
  - `pub struct Rule { #[serde(rename = "match")] pub matcher: RuleMatch, pub subtype: Option<Subtype>, pub overrides: serde_json::Value }`
  - `pub struct PresetFile { pub schema_version: u32, pub id: String, pub version: String, pub base: Option<String>, pub target: Option<Target>, pub hidden: bool, pub description: String, pub defaults: serde_json::Value, pub subtypes: BTreeMap<Subtype, serde_json::Value>, pub rules: Vec<Rule> }`
  - `pub struct ConfigFile { pub preset: Option<String>, pub target: Option<Target>, pub defaults: serde_json::Value, pub subtypes: BTreeMap<Subtype, serde_json::Value>, pub rules: Vec<Rule> }` with `fn load(path: &Path) -> Result<ConfigFile, PresetError>`
  - `pub enum Provenance { Builtin, Preset(String), UserConfig, ProjectConfig, Rule(String), Analysis, Suggestion, Set, User }` (snake_case serde, `Ord`)
  - `pub fn merge_json(base: &mut Value, patch: &Value)`; `pub fn leaf_paths(v: &Value, prefix: &str, out: &mut Vec<String>)`
  - `pub struct PresetRegistry` with `fn builtin() -> Result<Self, PresetError>`, `fn add_toml(&mut self, text: &str) -> Result<String, PresetError>`, `fn add_file(&mut self, path: &Path) -> Result<String, PresetError>`, `fn get(&self, id: &str) -> Option<&PresetFile>`, `fn chain(&self, id: &str) -> Result<Vec<&PresetFile>, PresetError>` (root base first), `fn selectable(&self) -> Vec<&PresetFile>`, `fn sha256(&self, id: &str) -> Option<String>`, `fn layers(&self, id: &str, subtype: Subtype) -> Result<Vec<(Provenance, Value)>, PresetError>`, `fn profile(&self, id: &str, target: Target, subtype: Subtype) -> Result<Profile, PresetError>`
  - `pub const BASE_ID: &str = "sdc-base"`; `pub const DEFAULT_PRESET: &str = "cs-quality"`; `pub fn default_preset_for(t: Target) -> &'static str`
  - `pub enum PresetError { Parse(String), Unknown(String), Cycle(String), NewerSchema(u32), Invalid(String) }`

- [ ] **Step 1: Write the built-in preset data**

`presets/sdc-base.toml` (spec §4.1/§4.2 as data; every built-in preset inherits it):
```toml
schema_version = 1
id = "sdc-base"
version = "1.0.0"
hidden = true
description = "Per-sub-type defaults from spec §4.1/§4.2. Not selectable; every preset inherits it."

[defaults]
color = { content = "linear", tag = "unorm" }
alpha = { policy = "remove", threshold = 127, compare = "gt", exact_opaque = false }
channels = { flip_green = false, reconstruct_z = false, grayscale_source = "r" }
mips = { levels = "full", filter = "box", normal = "renormalize", coverage = "auto" }
format = "bc7"
header = "auto"
encoder = { backend = "bc7e", effort = "veryslow", rgbcx_level = 18, perceptual = false, bc1_transparent_black = false, bc45_search_radius = "default" }
validation = { min_mip_psnr_db = 30.0, coverage_tolerance_pts = 2.0, complex_m_warn_alpha = 247, bc6h_min_peak_ratio = 0.8, pgpatcher_non_white_share = 0.5 }

[subtypes.diffuse]
color = { content = "srgb", tag = "unorm" }
[subtypes.landscape_diffuse]
color = { content = "srgb", tag = "unorm" }
alpha = { policy = "data" }
[subtypes.pbr_albedo]
color = { content = "srgb", tag = "srgb" }
[subtypes.pbr_emissive]
color = { content = "srgb", tag = "n/a" }
format = "bc6h_uf16"
encoder = { backend = "ispc_bc6h" }
[subtypes.glow]
color = { content = "srgb", tag = "unorm" }
[subtypes.pbr_subsurface]
color = { content = "srgb", tag = "srgb" }
alpha = { policy = "data" }
[subtypes.pbr_fuzz]
color = { content = "srgb", tag = "srgb" }
alpha = { policy = "data" }
[subtypes.subsurface_tint]
color = { content = "srgb", tag = "unorm" }
[subtypes.backlight_tint]
color = { content = "srgb", tag = "unorm" }
[subtypes.ui_sky]
color = { content = "srgb", tag = "unorm" }
mips = { levels = "none" }
[subtypes.normal]
alpha = { policy = "data" }
[subtypes.landscape_normal]
alpha = { policy = "data" }
[subtypes.pbr_normal]
alpha = { policy = "remove" }
[subtypes.msn]
alpha = { policy = "remove" }
[subtypes.coat_normal]
alpha = { policy = "data" }
[subtypes.envmask]
alpha = { policy = "remove", exact_opaque = true }
format = "bc4"
encoder = { backend = "rgbcx" }
[subtypes.complex_material]
alpha = { policy = "data", exact_opaque = true }
[subtypes.rmaos]
alpha = { policy = "data" }
[subtypes.rfaos]
alpha = { policy = "data" }
[subtypes.flow]
format = "bc5"
encoder = { backend = "rgbcx" }
[subtypes.height]
format = "bc4"
encoder = { backend = "rgbcx" }
[subtypes.msn_specular]
format = "bc4"
encoder = { backend = "rgbcx" }
[subtypes.generic_two_channel]
format = "bc5"
encoder = { backend = "rgbcx" }
[subtypes.generic_data]
alpha = { policy = "data" }
[subtypes.hdr_sky]
color = { content = "linear", tag = "n/a" }
format = "bc6h_uf16"
encoder = { backend = "ispc_bc6h" }

[[rules]]
match = { glob = "**/interface/**" }
overrides = { mips = { levels = "none" } }

[[rules]]
match = { glob = "**/sky/**" }
overrides = { mips = { levels = "none" } }
```

`presets/cs-quality.toml`:
```toml
schema_version = 1
id = "cs-quality"
version = "1.0.0"
base = "sdc-base"
target = "cs"
description = "CS – Quality (default): Community Shaders incl. TruePBR; bc7e veryslow; BC6H emissive."
```
`presets/cpm-quality.toml`:
```toml
schema_version = 1
id = "cpm-quality"
version = "1.0.0"
base = "sdc-base"
target = "cpm"
description = "CPM – Quality: CPM / ENB complex material; no _SRGB, no BC6H."
```
`presets/vanilla-quality.toml`:
```toml
schema_version = 1
id = "vanilla-quality"
version = "1.0.0"
base = "sdc-base"
target = "vanilla"
description = "Vanilla – Quality: Bethesda shaders; no _SRGB, no BC6H, no complex _m."
```
(Availability per target, the "—" cells of §4.2, is code in `policy::availability`, Task 26, because it is a rule, not a default.)

- [ ] **Step 2: Write the failing tests**

`crates/engine/tests/presets.rs`:
```rust
use engine::config::*;
use engine::presets::*;
use serde_json::json;

#[test]
fn merge_is_deep_and_leaf_paths_are_dotted() {
    let mut a = json!({"alpha": {"policy": "remove", "threshold": 127}, "format": "bc7"});
    merge_json(&mut a, &json!({"alpha": {"policy": "data"}}));
    assert_eq!(a, json!({"alpha": {"policy": "data", "threshold": 127}, "format": "bc7"}));
    let mut paths = vec![];
    leaf_paths(&json!({"alpha": {"policy": "data"}, "format": "bc1"}), "", &mut paths);
    paths.sort();
    assert_eq!(paths, vec!["alpha.policy", "format"]);
}

#[test]
fn builtin_defaults_match_spec_table_4_2() {
    let r = PresetRegistry::builtin().unwrap();
    let p = r.profile("cs-quality", Target::Cs, Subtype::PbrAlbedo).unwrap();
    assert_eq!((p.format, p.color.tag, p.color.content), (Format::Bc7, ColorTag::Srgb, ColorContent::Srgb));
    let p = r.profile("vanilla-quality", Target::Vanilla, Subtype::Diffuse).unwrap();
    assert_eq!((p.format, p.color.tag), (Format::Bc7, ColorTag::Unorm));
    let p = r.profile("cs-quality", Target::Cs, Subtype::Flow).unwrap();
    assert_eq!((p.format, p.encoder.backend), (Format::Bc5, Backend::Rgbcx));
    let p = r.profile("cs-quality", Target::Cs, Subtype::Envmask).unwrap();
    assert_eq!((p.format, p.alpha.policy, p.alpha.exact_opaque), (Format::Bc4, AlphaPolicy::Remove, true));
    let p = r.profile("cs-quality", Target::Cs, Subtype::ComplexMaterial).unwrap();
    assert_eq!((p.alpha.policy, p.alpha.exact_opaque), (AlphaPolicy::Data, true));
    let p = r.profile("cs-quality", Target::Cs, Subtype::PbrEmissive).unwrap();
    assert_eq!((p.format, p.color.tag, p.encoder.backend), (Format::Bc6hUf16, ColorTag::NotApplicable, Backend::IspcBc6h));
    let p = r.profile("cs-quality", Target::Cs, Subtype::Normal).unwrap();
    assert_eq!((p.alpha.policy, p.alpha.threshold, p.encoder.effort, p.encoder.perceptual), (AlphaPolicy::Data, 127, Effort::Veryslow, false));
}

#[test]
fn every_builtin_subtype_profile_deserialises() {
    let r = PresetRegistry::builtin().unwrap();
    for id in ["cs-quality", "cpm-quality", "vanilla-quality"] {
        for st in Subtype::ALL {
            r.profile(id, Target::Cs, st).unwrap_or_else(|e| panic!("{id}/{st:?}: {e}"));
        }
    }
}

#[test]
fn chain_inheritance_and_errors() {
    let mut r = PresetRegistry::builtin().unwrap();
    assert_eq!(r.chain("cs-quality").unwrap().iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), ["sdc-base", "cs-quality"]);
    assert!(r.selectable().iter().all(|p| p.id != "sdc-base"));
    let id = r
        .add_toml("schema_version = 1\nid = \"mine\"\nversion = \"0.1.0\"\nbase = \"cs-quality\"\n[subtypes.diffuse]\nformat = \"bc1\"\n")
        .unwrap();
    assert_eq!(r.profile(&id, Target::Cs, Subtype::Diffuse).unwrap().format, Format::Bc1);
    assert!(matches!(r.add_toml("schema_version = 2\nid = \"x\"\nversion = \"1.0.0\"\n"), Err(PresetError::NewerSchema(2))));
    assert!(matches!(r.chain("nope"), Err(PresetError::Unknown(_))));
    let bad = r.add_toml("schema_version = 1\nid = \"bad\"\nversion = \"1.0.0\"\nbase = \"sdc-base\"\n[defaults]\nformat = \"bc9\"\n").unwrap();
    assert!(matches!(r.profile(&bad, Target::Cs, Subtype::Diffuse), Err(PresetError::Invalid(_))));
    assert_eq!(r.sha256("cs-quality").unwrap().len(), 64);
}
```

- [ ] **Step 3: Run them to verify they fail**

Run: `cargo test -p engine --test presets`
Expected: FAIL to compile.

- [ ] **Step 4: Implement `presets.rs`**

```rust
//! Presets (spec §3.5): versioned bundles of profile defaults plus ordered rules, with single inheritance.
use crate::config::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::Path;

pub const SCHEMA_VERSION: u32 = 1;
pub const BASE_ID: &str = "sdc-base";
pub const DEFAULT_PRESET: &str = "cs-quality";

pub fn default_preset_for(t: Target) -> &'static str {
    match t {
        Target::Cs => "cs-quality",
        Target::Cpm => "cpm-quality",
        Target::Vanilla => "vanilla-quality",
    }
}

const BUILTIN: [&str; 4] = [
    include_str!("../../../presets/sdc-base.toml"),
    include_str!("../../../presets/cs-quality.toml"),
    include_str!("../../../presets/cpm-quality.toml"),
    include_str!("../../../presets/vanilla-quality.toml"),
];

#[derive(Debug, Clone, thiserror::Error, PartialEq)]
pub enum PresetError {
    #[error("preset parse error: {0}")]
    Parse(String),
    #[error("unknown preset '{0}'")]
    Unknown(String),
    #[error("preset inheritance cycle at '{0}'")]
    Cycle(String),
    #[error("preset schema_version {0} is newer than this sdc supports ({SCHEMA_VERSION})")]
    NewerSchema(u32),
    #[error("invalid profile: {0}")]
    Invalid(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AlphaContent {
    None,
    Constant255,
    Varying,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RuleMatch {
    /// Glob over the game-relative path (lower case, '/'), e.g. "**/armor/**/*_n.png".
    #[serde(default)]
    pub glob: Option<String>,
    /// Recognised suffix without underscore, e.g. "n", "rmaos"; "" = no suffix.
    #[serde(default)]
    pub suffix: Option<String>,
    /// The file's texture set contains a member with this suffix.
    #[serde(default)]
    pub set_has: Option<String>,
    #[serde(default)]
    pub alpha: Option<AlphaContent>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    #[serde(rename = "match")]
    pub matcher: RuleMatch,
    #[serde(default)]
    pub subtype: Option<Subtype>,
    #[serde(default)]
    pub overrides: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PresetFile {
    pub schema_version: u32,
    pub id: String,
    pub version: String,
    #[serde(default)]
    pub base: Option<String>,
    #[serde(default)]
    pub target: Option<Target>,
    #[serde(default)]
    pub hidden: bool,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub defaults: Value,
    #[serde(default)]
    pub subtypes: BTreeMap<Subtype, Value>,
    #[serde(default)]
    pub rules: Vec<Rule>,
}

/// User config (%APPDATA%\sdc\config.toml) and project config (sdc.toml / --config).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConfigFile {
    #[serde(default)]
    pub preset: Option<String>,
    #[serde(default)]
    pub target: Option<Target>,
    #[serde(default)]
    pub defaults: Value,
    #[serde(default)]
    pub subtypes: BTreeMap<Subtype, Value>,
    #[serde(default)]
    pub rules: Vec<Rule>,
}

impl ConfigFile {
    pub fn load(path: &Path) -> Result<ConfigFile, PresetError> {
        let text = std::fs::read_to_string(path).map_err(|e| PresetError::Parse(format!("{}: {e}", path.display())))?;
        toml::from_str(&text).map_err(|e| PresetError::Parse(format!("{}: {e}", path.display())))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Provenance {
    Builtin,
    Preset(String),
    UserConfig,
    ProjectConfig,
    Rule(String),
    Analysis,
    Suggestion,
    Set,
    User,
}

pub fn merge_json(base: &mut Value, patch: &Value) {
    match (base, patch) {
        (Value::Object(b), Value::Object(p)) => {
            for (k, v) in p {
                merge_json(b.entry(k.clone()).or_insert(Value::Null), v);
            }
        }
        (_, Value::Null) => {}
        (b, p) => *b = p.clone(),
    }
}

pub fn leaf_paths(v: &Value, prefix: &str, out: &mut Vec<String>) {
    match v {
        Value::Object(m) => {
            for (k, child) in m {
                let p = if prefix.is_empty() { k.clone() } else { format!("{prefix}.{k}") };
                leaf_paths(child, &p, out);
            }
        }
        Value::Null => {}
        _ => out.push(prefix.to_string()),
    }
}

#[derive(Debug, Clone, Default)]
pub struct PresetRegistry {
    presets: BTreeMap<String, PresetFile>,
}

impl PresetRegistry {
    pub fn builtin() -> Result<Self, PresetError> {
        let mut r = PresetRegistry::default();
        for text in BUILTIN {
            r.add_toml(text)?;
        }
        Ok(r)
    }

    pub fn add_toml(&mut self, text: &str) -> Result<String, PresetError> {
        let p: PresetFile = toml::from_str(text).map_err(|e| PresetError::Parse(e.to_string()))?;
        if p.schema_version > SCHEMA_VERSION {
            return Err(PresetError::NewerSchema(p.schema_version));
        }
        let id = p.id.clone();
        self.presets.insert(id.clone(), p);
        Ok(id)
    }

    pub fn add_file(&mut self, path: &Path) -> Result<String, PresetError> {
        let text = std::fs::read_to_string(path).map_err(|e| PresetError::Parse(format!("{}: {e}", path.display())))?;
        self.add_toml(&text)
    }

    pub fn get(&self, id: &str) -> Option<&PresetFile> {
        self.presets.get(id)
    }

    pub fn selectable(&self) -> Vec<&PresetFile> {
        self.presets.values().filter(|p| !p.hidden).collect()
    }

    /// The inheritance chain, root base first.
    pub fn chain(&self, id: &str) -> Result<Vec<&PresetFile>, PresetError> {
        let mut out = vec![];
        let mut cur = Some(id.to_string());
        while let Some(c) = cur {
            if out.iter().any(|p: &&PresetFile| p.id == c) {
                return Err(PresetError::Cycle(c));
            }
            let p = self.presets.get(&c).ok_or_else(|| PresetError::Unknown(c.clone()))?;
            out.push(p);
            cur = p.base.clone();
        }
        out.reverse();
        Ok(out)
    }

    /// SHA-256 of the preset's canonical JSON form (manifest provenance, spec §3.6).
    pub fn sha256(&self, id: &str) -> Option<String> {
        let p = self.presets.get(id)?;
        let canon = serde_json::to_vec(&serde_json::to_value(p).ok()?).ok()?;
        Some(hex::encode(Sha256::digest(canon)))
    }

    /// Patches in precedence order: for each preset in the chain, `defaults` then `subtypes.<st>`.
    /// The hidden base preset is labelled Builtin.
    pub fn layers(&self, id: &str, subtype: Subtype) -> Result<Vec<(Provenance, Value)>, PresetError> {
        let mut v = vec![];
        for p in self.chain(id)? {
            let prov = if p.id == BASE_ID { Provenance::Builtin } else { Provenance::Preset(p.id.clone()) };
            v.push((prov.clone(), p.defaults.clone()));
            if let Some(s) = p.subtypes.get(&subtype) {
                v.push((prov, s.clone()));
            }
        }
        Ok(v)
    }

    /// Profile from presets alone (no config, rules, analysis or overrides). Used by tests and `presets show`.
    pub fn profile(&self, id: &str, target: Target, subtype: Subtype) -> Result<Profile, PresetError> {
        let mut doc = Value::Object(Map::new());
        for (_, patch) in self.layers(id, subtype)? {
            merge_json(&mut doc, &patch);
        }
        finish_profile(doc, target, subtype)
    }
}

/// Insert identity fields and deserialise; errors name the offending field.
pub fn finish_profile(mut doc: Value, target: Target, subtype: Subtype) -> Result<Profile, PresetError> {
    merge_json(
        &mut doc,
        &serde_json::json!({ "target": target, "subtype": subtype, "category": subtype.category() }),
    );
    serde_json::from_value(doc).map_err(|e| PresetError::Invalid(e.to_string()))
}
```
Note: `toml::from_str` into `serde_json::Value` fields works because `Value` implements `Deserialize` for any format. `merge_json` treats `Null` patches as "no change" so absent sections never erase defaults.

Add `pub mod presets;` to `lib.rs`.

- [ ] **Step 5: Run the tests**

Run: `cargo test -p engine --test presets`
Expected: 4 passed.

- [ ] **Step 6: Commit**

```bash
git add presets crates/engine
git commit -m "feat(engine): built-in presets as data, inheritance, JSON-merge layering and registry"
```

---

### Task 21: Discover inputs and group texture sets

**Files:**
- Create: `crates/engine/src/discover.rs`, `crates/engine/src/group.rs`
- Modify: `crates/engine/src/lib.rs`
- Test: `crates/engine/tests/discover_group.rs`

**Interfaces:**
- Produces:
  - `discover::InputKind { Image, Dds }`; `discover::InputFile { pub path: PathBuf, pub rel: PathBuf, pub game_path: String, pub output: PathBuf, pub kind: InputKind }`
  - `discover::IMAGE_EXTS: [&str; 4] = ["png", "tga", "tif", "tiff"]`
  - `discover::discover(inputs: &[PathBuf], output_root: &Path, recursive: bool) -> std::io::Result<Vec<InputFile>>` — sorted by `rel`; folders walked (depth 1 unless `recursive`), skipping `_sdc` and anything under `output_root`; unknown extensions inside folders ignored; explicit file inputs always included (as `Image` unless `.dds`)
  - `discover::game_path(abs: &Path, rel: &Path) -> String` — lower case, '/'-separated, from the last `textures` component (inclusive) of the absolute path, else `rel`
  - `group::SUFFIXES: [&str; 19]` (longest first); `group::split_suffix(stem: &str) -> (String, Option<String>)`
  - `group::TextureSet { pub key: String, pub members: Vec<usize>, pub suffixes: BTreeSet<String> }`
  - `group::group_sets(files: &[InputFile]) -> (Vec<TextureSet>, Vec<Vec<usize>>)` — sets (key = lower-case output dir + '/' + base) and duplicate groups (two or more inputs mapping to one output, case-insensitive)

- [ ] **Step 1: Write the failing tests**

`crates/engine/tests/discover_group.rs`:
```rust
use engine::discover::*;
use engine::group::*;
use std::path::{Path, PathBuf};

fn touch(p: &Path) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, b"x").unwrap();
}

#[test]
fn suffixes_longest_first_case_insensitive() {
    assert_eq!(split_suffix("wood01_n"), ("wood01".into(), Some("n".into())));
    assert_eq!(split_suffix("Armor_RMAOS"), ("Armor".into(), Some("rmaos".into())));
    assert_eq!(split_suffix("body_msn"), ("body".into(), Some("msn".into())));
    assert_eq!(split_suffix("rock_em"), ("rock".into(), Some("em".into())));
    assert_eq!(split_suffix("hair_flow"), ("hair".into(), Some("flow".into())));
    assert_eq!(split_suffix("plain"), ("plain".into(), None));
    assert_eq!(split_suffix("_n"), ("_n".into(), None));
}

#[test]
fn discovers_mirrors_paths_and_finds_game_path() {
    let d = tempfile::tempdir().unwrap();
    let src = d.path().join("mod/textures/armor/iron");
    for f in ["iron_n.png", "iron.png", "iron_m.tga", "notes.txt", "old.dds", "sub/deep.png"] {
        touch(&src.join(f));
    }
    let out = d.path().join("out");
    let files = discover(&[src.clone()], &out, false).unwrap();
    let rels: Vec<String> = files.iter().map(|f| f.rel.to_string_lossy().replace('\\', "/")).collect();
    assert_eq!(rels, ["iron.png", "iron_m.tga", "iron_n.png", "old.dds"]);
    let n = files.iter().find(|f| f.rel == PathBuf::from("iron_n.png")).unwrap();
    assert_eq!(n.game_path, "textures/armor/iron/iron_n.png");
    assert_eq!(n.output, out.join("iron_n.dds"));
    assert_eq!(files.iter().filter(|f| f.kind == InputKind::Dds).count(), 1);
    assert_eq!(discover(&[src], &out, true).unwrap().len(), 5);
}

#[test]
fn groups_sets_and_flags_duplicates() {
    let d = tempfile::tempdir().unwrap();
    let src = d.path().join("in");
    for f in ["wood01.png", "wood01_n.png", "wood01_n.tga", "Wood01_RMAOS.png", "rock_n.png"] {
        touch(&src.join(f));
    }
    let files = discover(&[src], &d.path().join("out"), false).unwrap();
    let (sets, dups) = group_sets(&files);
    let wood = sets.iter().find(|s| s.key.ends_with("/wood01")).unwrap();
    assert_eq!(wood.members.len(), 4);
    assert!(wood.suffixes.contains("rmaos") && wood.suffixes.contains("n"));
    assert_eq!(dups.len(), 1);
    assert_eq!(dups[0].len(), 2);
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p engine --test discover_group`
Expected: FAIL to compile.

- [ ] **Step 3: Implement**

`crates/engine/src/discover.rs`:
```rust
use serde::Serialize;
use std::path::{Path, PathBuf};

pub const IMAGE_EXTS: [&str; 4] = ["png", "tga", "tif", "tiff"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum InputKind {
    Image,
    Dds,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InputFile {
    pub path: PathBuf,
    pub rel: PathBuf,
    pub game_path: String,
    pub output: PathBuf,
    pub kind: InputKind,
}

fn ext_lower(p: &Path) -> String {
    p.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default()
}

pub fn game_path(abs: &Path, rel: &Path) -> String {
    let comps: Vec<String> = abs.components().map(|c| c.as_os_str().to_string_lossy().to_ascii_lowercase()).collect();
    match comps.iter().rposition(|c| c == "textures") {
        Some(i) => comps[i..].join("/"),
        None => rel.to_string_lossy().replace('\\', "/").to_ascii_lowercase(),
    }
}

fn make(path: PathBuf, rel: PathBuf, output_root: &Path) -> InputFile {
    let kind = if ext_lower(&path) == "dds" { InputKind::Dds } else { InputKind::Image };
    let abs = std::path::absolute(&path).unwrap_or_else(|_| path.clone());
    let game_path = game_path(&abs, &rel);
    let output = output_root.join(&rel).with_extension("dds");
    InputFile { path, rel, game_path, output, kind }
}

pub fn discover(inputs: &[PathBuf], output_root: &Path, recursive: bool) -> std::io::Result<Vec<InputFile>> {
    let out_abs = std::path::absolute(output_root)?;
    let mut files = vec![];
    for input in inputs {
        if input.is_dir() {
            let walker = walkdir::WalkDir::new(input).min_depth(1).max_depth(if recursive { usize::MAX } else { 1 });
            for e in walker.into_iter().filter_entry(|e| e.file_name() != "_sdc") {
                let e = e.map_err(std::io::Error::other)?;
                let p = e.path();
                if !e.file_type().is_file() || std::path::absolute(p)?.starts_with(&out_abs) {
                    continue;
                }
                let ext = ext_lower(p);
                if ext == "dds" || IMAGE_EXTS.contains(&ext.as_str()) {
                    let rel = p.strip_prefix(input).unwrap().to_path_buf();
                    files.push(make(p.to_path_buf(), rel, output_root));
                }
            }
        } else {
            let rel = PathBuf::from(input.file_name().ok_or_else(|| std::io::Error::other("input has no file name"))?);
            files.push(make(input.clone(), rel, output_root));
        }
    }
    files.sort_by(|a, b| a.rel.cmp(&b.rel));
    Ok(files)
}
```
(`std::path::absolute` is stable since Rust 1.79.)

`crates/engine/src/group.rs`:
```rust
use crate::discover::InputFile;
use std::collections::{BTreeMap, BTreeSet};

/// Spec §6.1 suffix list, longest first so `_msn` wins over `_n` and `_envmask` over `_m`.
pub const SUFFIXES: [&str; 19] = [
    "envmask", "rmaos", "rfaos", "flow", "msn", "cnr", "crn", "em", "bl", "sk", "n", "p", "m", "g", "s", "f", "b", "d", "e",
];

pub fn split_suffix(stem: &str) -> (String, Option<String>) {
    let lower = stem.to_ascii_lowercase();
    for s in SUFFIXES {
        let tail = format!("_{s}");
        if lower.ends_with(&tail) && lower.len() > tail.len() {
            return (stem[..stem.len() - tail.len()].to_string(), Some(s.to_string()));
        }
    }
    (stem.to_string(), None)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextureSet {
    pub key: String,
    pub members: Vec<usize>,
    pub suffixes: BTreeSet<String>,
}

fn out_key(f: &InputFile) -> String {
    f.output.to_string_lossy().replace('\\', "/").to_ascii_lowercase()
}

pub fn group_sets(files: &[InputFile]) -> (Vec<TextureSet>, Vec<Vec<usize>>) {
    let mut sets: BTreeMap<String, TextureSet> = BTreeMap::new();
    let mut outs: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, f) in files.iter().enumerate() {
        outs.entry(out_key(f)).or_default().push(i);
        let stem = f.rel.file_stem().unwrap_or_default().to_string_lossy().into_owned();
        let (base, suffix) = split_suffix(&stem);
        let dir = f.output.parent().map(|p| p.to_string_lossy().replace('\\', "/").to_ascii_lowercase()).unwrap_or_default();
        let key = format!("{dir}/{}", base.to_ascii_lowercase());
        let set = sets.entry(key.clone()).or_insert_with(|| TextureSet { key, members: vec![], suffixes: BTreeSet::new() });
        set.members.push(i);
        if let Some(s) = suffix {
            set.suffixes.insert(s);
        }
    }
    let dups = outs.into_values().filter(|v| v.len() > 1).collect();
    (sets.into_values().collect(), dups)
}
```
Add `pub mod discover; pub mod group;` to `lib.rs`.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p engine --test discover_group`
Expected: 3 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/engine
git commit -m "feat(engine): input discovery with game-relative paths and texture-set grouping"
```

---

### Task 22: Load PNG/TGA/TIFF (8/16-bit) to float RGBA

**Files:**
- Create: `crates/engine/src/load.rs`
- Modify: `crates/engine/src/lib.rs`
- Test: `crates/engine/tests/load.rs`

**Interfaces:**
- Produces: `pub const MAX_DIM: u32 = 16384`; `pub enum LoadError { TooLarge { path: PathBuf, dim: u32 }, Decode { path: PathBuf, msg: String } }`; `pub fn load_image(path: &Path) -> Result<(Rgba32F, SourceInfo), LoadError>`. 8-bit values load as `k / 255.0` exactly as `image::DynamicImage::to_rgba32f` produces; grey sources replicate into RGB; missing alpha loads as 1.0 with `has_alpha = false`.

- [ ] **Step 1: Write the failing tests**

`crates/engine/tests/load.rs`:
```rust
use engine::load::*;
use image::{ImageBuffer, Luma, Rgb, Rgba};

#[test]
fn png8_rgba_loads_exactly() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("a.png");
    ImageBuffer::from_fn(4, 4, |x, y| Rgba([x as u8 * 60, y as u8 * 60, 7, 200])).save(&p).unwrap();
    let (img, info) = load_image(&p).unwrap();
    assert_eq!((info.bit_depth, info.channels, info.has_alpha, info.is_float), (8, 4, true, false));
    assert_eq!(img.at(3, 2), [180.0 / 255.0, 120.0 / 255.0, 7.0 / 255.0, 200.0 / 255.0]);
}

#[test]
fn png16_and_tga_and_tiff() {
    let d = tempfile::tempdir().unwrap();
    let p16 = d.path().join("b.png");
    ImageBuffer::from_fn(2, 2, |_, _| Rgb([65535u16, 0, 32768])).save(&p16).unwrap();
    let (img, info) = load_image(&p16).unwrap();
    assert_eq!((info.bit_depth, info.has_alpha), (16, false));
    assert_eq!(img.at(0, 0)[3], 1.0);
    assert!((img.at(1, 1)[2] - 32768.0 / 65535.0).abs() < 1e-6);

    let tga = d.path().join("c.tga");
    ImageBuffer::from_fn(2, 2, |_, _| Rgb([10u8, 20, 30])).save(&tga).unwrap();
    assert!(!load_image(&tga).unwrap().1.has_alpha);

    let tif = d.path().join("d.tif");
    ImageBuffer::from_fn(2, 2, |_, _| Luma([1000u16])).save(&tif).unwrap();
    let (img, info) = load_image(&tif).unwrap();
    assert_eq!(info.bit_depth, 16);
    assert_eq!(img.at(0, 0)[0], img.at(0, 0)[2]);
}

#[test]
fn rejects_over_16384_and_garbage() {
    let d = tempfile::tempdir().unwrap();
    let big = d.path().join("big.png");
    ImageBuffer::from_fn(16385, 1, |_, _| Luma([0u8])).save(&big).unwrap();
    assert!(matches!(load_image(&big), Err(LoadError::TooLarge { dim: 16385, .. })));
    let bad = d.path().join("bad.png");
    std::fs::write(&bad, b"not a png").unwrap();
    assert!(matches!(load_image(&bad), Err(LoadError::Decode { .. })));
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p engine --test load`
Expected: FAIL to compile.

- [ ] **Step 3: Implement**

`crates/engine/src/load.rs`:
```rust
use crate::image::{Rgba32F, SourceInfo};
use std::path::{Path, PathBuf};

pub const MAX_DIM: u32 = 16384;

#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("{}: dimension {dim} exceeds 16384 (D3D11 limit)", path.display())]
    TooLarge { path: PathBuf, dim: u32 },
    #[error("{}: {msg}", path.display())]
    Decode { path: PathBuf, msg: String },
}

pub fn load_image(path: &Path) -> Result<(Rgba32F, SourceInfo), LoadError> {
    let err = |e: image::ImageError| LoadError::Decode { path: path.to_path_buf(), msg: e.to_string() };
    let (w, h) = image::image_dimensions(path).map_err(err)?;
    if w.max(h) > MAX_DIM {
        return Err(LoadError::TooLarge { path: path.to_path_buf(), dim: w.max(h) });
    }
    let img = image::open(path).map_err(err)?;
    let ct = img.color();
    let channels = ct.channel_count();
    let info = SourceInfo {
        bit_depth: (ct.bits_per_pixel() / channels as u16) as u8,
        channels,
        has_alpha: ct.has_alpha(),
        is_float: matches!(ct, image::ColorType::Rgb32F | image::ColorType::Rgba32F),
    };
    let f = img.to_rgba32f();
    let px = f.pixels().map(|p| p.0).collect();
    Ok((Rgba32F { width: w, height: h, px }, info))
}
```
Add `pub mod load;` to `lib.rs`. ICC profiles are ignored by `image` (spec §5.1 asks for an info event; Task 37 emits it when `image` reports one — the `png` decoder exposes `icc_profile()`; if not available through `DynamicImage`, record in the M6 milestone).

- [ ] **Step 4: Run the tests**

Run: `cargo test -p engine --test load`
Expected: 3 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/engine
git commit -m "feat(engine): load PNG/TGA/TIFF 8/16-bit to float RGBA with 16384 px limit"
```

---

### Task 23: Analyse (spec §5.1 statistics)

**Files:**
- Create: `crates/engine/src/analyse.rs`
- Modify: `crates/engine/src/lib.rs`
- Test: unit tests in `analyse.rs`

**Interfaces:**
- Consumes: `Rgba32F`, `SourceInfo`, `presets::AlphaContent`.
- Produces:
  - `pub struct ChannelStats { pub min: f32, pub max: f32, pub mean: f64 }`
  - `pub struct AlphaHistogram { pub eq255: u64, pub eq254: u64, pub eq0: u64, pub le5: u64, pub ge250: u64 }`
  - `pub struct Analysis { pub width: u32, pub height: u32, pub is_pot: bool, pub channels: [ChannelStats; 4], pub alpha: AlphaContent, pub alpha_hist: AlphaHistogram, pub grayscale: bool, pub non_white_share: f64, pub mean_alpha: f64, pub normal_len_err: f64, pub blue_mostly_black: bool, pub red_mostly_high: bool, pub peak_luminance: f32 }` (Serialize, JsonSchema)
  - `pub fn analyse(img: &Rgba32F, info: &SourceInfo) -> Analysis`
  - `pub fn code8(v: f32) -> u8` (round to nearest 8-bit code, clamped)

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::{Rgba32F, SourceInfo};

    const RGBA8: SourceInfo = SourceInfo { bit_depth: 8, channels: 4, has_alpha: true, is_float: false };

    #[test]
    fn alpha_classification() {
        let opaque = Rgba32F::new(4, 4, [0.5, 0.5, 0.5, 1.0]);
        assert_eq!(analyse(&opaque, &RGBA8).alpha, AlphaContent::Constant255);
        let no_alpha = SourceInfo { channels: 3, has_alpha: false, ..RGBA8 };
        assert_eq!(analyse(&opaque, &no_alpha).alpha, AlphaContent::None);
        let mut cut = opaque.clone();
        cut.px[0][3] = 0.0;
        let a = analyse(&cut, &RGBA8);
        assert_eq!(a.alpha, AlphaContent::Varying);
        assert_eq!((a.alpha_hist.eq0, a.alpha_hist.eq255, a.alpha_hist.le5), (1, 15, 1));
        assert!((a.non_white_share - 1.0 / 16.0).abs() < 1e-12);
        assert!(a.is_pot && a.grayscale);
    }

    #[test]
    fn grayscale_tolerance_is_4_codes() {
        let mut img = Rgba32F::new(2, 1, [0.5, 0.5, 0.5, 1.0]);
        img.px[1] = [0.5, 0.5 + 4.0 / 255.0, 0.5, 1.0];
        assert!(analyse(&img, &RGBA8).grayscale);
        img.px[1] = [0.5, 0.5 + 6.0 / 255.0, 0.5, 1.0];
        assert!(!analyse(&img, &RGBA8).grayscale);
    }

    #[test]
    fn normal_length_error_and_roughness_hints() {
        let flat = Rgba32F::new(4, 4, [0.5, 0.5, 1.0, 1.0]); // (0, 0, 1) encoded
        assert!(analyse(&flat, &RGBA8).normal_len_err < 0.01);
        let rough = Rgba32F::new(4, 4, [0.95, 0.0, 0.02, 1.0]);
        let a = analyse(&rough, &RGBA8);
        assert!(a.blue_mostly_black && a.red_mostly_high);
    }
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p engine analyse`
Expected: FAIL to compile.

- [ ] **Step 3: Implement**

```rust
//! Analyse (spec §5.1): statistics used by suggestions, warnings and validation.
use crate::image::{Rgba32F, SourceInfo};
use crate::presets::AlphaContent;
use schemars::JsonSchema;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, JsonSchema)]
pub struct ChannelStats {
    pub min: f32,
    pub max: f32,
    pub mean: f64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, JsonSchema)]
pub struct AlphaHistogram {
    pub eq255: u64,
    pub eq254: u64,
    pub eq0: u64,
    pub le5: u64,
    pub ge250: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct Analysis {
    pub width: u32,
    pub height: u32,
    pub is_pot: bool,
    pub channels: [ChannelStats; 4],
    pub alpha: AlphaContent,
    pub alpha_hist: AlphaHistogram,
    pub grayscale: bool,
    pub non_white_share: f64,
    /// Predicted 1x1 alpha (box average of the base).
    pub mean_alpha: f64,
    /// Mean |length - 1| of RGB decoded as `*2 - 1`.
    pub normal_len_err: f64,
    pub blue_mostly_black: bool,
    pub red_mostly_high: bool,
    pub peak_luminance: f32,
}

pub fn code8(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

pub fn analyse(img: &Rgba32F, info: &SourceInfo) -> Analysis {
    let n = img.px.len().max(1) as f64;
    let mut ch = [ChannelStats { min: f32::MAX, max: f32::MIN, mean: 0.0 }; 4];
    let mut hist = AlphaHistogram::default();
    let (mut gray, mut len_err, mut bb, mut rh, mut peak) = (true, 0.0f64, 0u64, 0u64, 0f32);
    for p in &img.px {
        for c in 0..4 {
            ch[c].min = ch[c].min.min(p[c]);
            ch[c].max = ch[c].max.max(p[c]);
            ch[c].mean += p[c] as f64;
        }
        match code8(p[3]) {
            255 => hist.eq255 += 1,
            254 => hist.eq254 += 1,
            0 => hist.eq0 += 1,
            _ => {}
        }
        let a = code8(p[3]);
        hist.le5 += (a <= 5) as u64;
        hist.ge250 += (a >= 250) as u64;
        let (r, g, b) = (code8(p[0]) as i32, code8(p[1]) as i32, code8(p[2]) as i32);
        gray &= (r - g).abs() <= 4 && (g - b).abs() <= 4;
        let v = [p[0] * 2.0 - 1.0, p[1] * 2.0 - 1.0, p[2] * 2.0 - 1.0];
        len_err += ((v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt() - 1.0).abs() as f64;
        bb += (p[2] < 0.1) as u64;
        rh += (p[0] > 0.9) as u64;
        peak = peak.max(0.2126 * p[0] + 0.7152 * p[1] + 0.0722 * p[2]);
    }
    for c in &mut ch {
        c.mean /= n;
    }
    let alpha = if !info.has_alpha {
        AlphaContent::None
    } else if hist.eq255 as usize == img.px.len() {
        AlphaContent::Constant255
    } else {
        AlphaContent::Varying
    };
    Analysis {
        width: img.width,
        height: img.height,
        is_pot: img.width.is_power_of_two() && img.height.is_power_of_two(),
        channels: ch,
        alpha,
        alpha_hist: hist,
        grayscale: gray,
        non_white_share: (img.px.len() as u64 - hist.eq255) as f64 / n,
        mean_alpha: ch[3].mean,
        normal_len_err: len_err / n,
        blue_mostly_black: bb as f64 / n > 0.5,
        red_mostly_high: rh as f64 / n > 0.5,
        peak_luminance: peak,
    }
}
```
Add `pub mod analyse;` to `lib.rs`.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p engine analyse`
Expected: 3 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/engine
git commit -m "feat(engine): analysis statistics (alpha histogram, grayscale, normal length, hints)"
```

---

### Task 24: Suggestions (spec §4.3) with confidence

**Files:**
- Create: `crates/engine/src/suggest.rs`
- Modify: `crates/engine/src/lib.rs`
- Test: unit tests in `suggest.rs`

**Interfaces:**
- Consumes: `config::{Subtype, Target, AlphaPolicy}`, `presets::AlphaContent`.
- Produces:
  - `pub enum Confidence { Low, Medium, High }` (lowercase serde, `Ord`)
  - `pub struct Suggestion { pub subtype: Subtype, pub confidence: Confidence, pub reason: String }` (Serialize, JsonSchema)
  - `pub struct SuggestContext<'a> { pub target: Target, pub suffix: Option<&'a str>, pub game_path: &'a str, pub set_suffixes: &'a BTreeSet<String>, pub alpha: AlphaContent, pub is_float: bool }`
  - `pub fn is_pbr_set(ctx: &SuggestContext) -> bool`
  - `pub fn suggest(ctx: &SuggestContext) -> Vec<Suggestion>` (best first; empty = no idea)
  - `pub fn suggest_alpha(subtype: Subtype, alpha: AlphaContent, near_binary_share: f64) -> Option<(AlphaPolicy, Confidence, String)>` — `near_binary_share` = share of texels with alpha ≤ 5 or ≥ 250

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn ctx<'a>(t: Target, suffix: Option<&'a str>, path: &'a str, set: &'a BTreeSet<String>, alpha: AlphaContent) -> SuggestContext<'a> {
        SuggestContext { target: t, suffix, game_path: path, set_suffixes: set, alpha, is_float: false }
    }

    #[test]
    fn suffix_rules() {
        let none = BTreeSet::new();
        let pbr: BTreeSet<String> = ["rmaos".to_string(), "n".to_string()].into();
        let msn: BTreeSet<String> = ["msn".to_string(), "s".to_string()].into();
        let top = |c: &SuggestContext| suggest(c)[0].clone();
        assert_eq!(top(&ctx(Target::Cs, Some("n"), "textures/a/x_n.png", &none, AlphaContent::Varying)).subtype, Subtype::Normal);
        assert_eq!(top(&ctx(Target::Cs, Some("n"), "textures/a/x_n.png", &pbr, AlphaContent::None)).subtype, Subtype::PbrNormal);
        assert_eq!(top(&ctx(Target::Cs, None, "textures/a/x.png", &pbr, AlphaContent::None)).subtype, Subtype::PbrAlbedo);
        let d = top(&ctx(Target::Cs, None, "textures/a/x.png", &none, AlphaContent::None));
        assert_eq!((d.subtype, d.confidence), (Subtype::Diffuse, Confidence::Medium));
        assert_eq!(top(&ctx(Target::Cs, Some("s"), "textures/a/x_s.png", &msn, AlphaContent::None)).subtype, Subtype::MsnSpecular);
        assert_eq!(top(&ctx(Target::Cs, Some("s"), "textures/a/x_s.png", &pbr, AlphaContent::None)).subtype, Subtype::PbrSubsurface);
        let s = top(&ctx(Target::Cs, Some("s"), "textures/a/x_s.png", &none, AlphaContent::None));
        assert_eq!((s.subtype, s.confidence), (Subtype::SubsurfaceTint, Confidence::Low));
        assert_eq!(top(&ctx(Target::Cs, Some("m"), "textures/a/x_m.png", &none, AlphaContent::Varying)).subtype, Subtype::ComplexMaterial);
        assert_eq!(top(&ctx(Target::Cs, Some("m"), "textures/a/x_m.png", &none, AlphaContent::Constant255)).subtype, Subtype::Envmask);
        assert_eq!(top(&ctx(Target::Vanilla, Some("m"), "textures/a/x_m.png", &none, AlphaContent::Varying)).subtype, Subtype::Envmask);
        assert_eq!(top(&ctx(Target::Cs, Some("rfaos"), "textures/a/x_rfaos.png", &none, AlphaContent::None)).subtype, Subtype::Rfaos);
        assert_eq!(top(&ctx(Target::Cs, None, "textures/landscape/dirt01.png", &none, AlphaContent::Varying)).subtype, Subtype::LandscapeDiffuse);
        assert_eq!(top(&ctx(Target::Cs, Some("n"), "textures/landscape/dirt01_n.png", &none, AlphaContent::Varying)).subtype, Subtype::LandscapeNormal);
        assert_eq!(top(&ctx(Target::Cs, Some("g"), "textures/pbr/x_g.png", &none, AlphaContent::None)).subtype, Subtype::PbrEmissive);
        assert_eq!(top(&ctx(Target::Vanilla, Some("g"), "textures/pbr/x_g.png", &none, AlphaContent::None)).subtype, Subtype::Glow);
        assert!(suggest(&ctx(Target::Cs, Some("e"), "textures/a/x_e.png", &none, AlphaContent::None)).is_empty());
    }

    #[test]
    fn alpha_suggestions() {
        assert_eq!(suggest_alpha(Subtype::Diffuse, AlphaContent::None, 0.0).unwrap().0, AlphaPolicy::Remove);
        let (p, c, _) = suggest_alpha(Subtype::Diffuse, AlphaContent::Varying, 0.95).unwrap();
        assert_eq!((p, c), (AlphaPolicy::Cutout, Confidence::Low));
        assert_eq!(suggest_alpha(Subtype::Diffuse, AlphaContent::Varying, 0.5).unwrap().0, AlphaPolicy::Blend);
        assert_eq!(suggest_alpha(Subtype::Rmaos, AlphaContent::Constant255, 1.0).unwrap(), (AlphaPolicy::Remove, Confidence::High, "alpha is constant 255".into()));
        assert!(suggest_alpha(Subtype::Normal, AlphaContent::Constant255, 1.0).is_none());
    }
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p engine suggest`
Expected: FAIL to compile.

- [ ] **Step 3: Implement**

```rust
//! Suggestions (spec §4.3): ranked (subtype, confidence, reason). Suggestions never override explicit choices.
use crate::config::{AlphaPolicy, Subtype, Target};
use crate::presets::AlphaContent;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct Suggestion {
    pub subtype: Subtype,
    pub confidence: Confidence,
    pub reason: String,
}

pub struct SuggestContext<'a> {
    pub target: Target,
    pub suffix: Option<&'a str>,
    pub game_path: &'a str,
    pub set_suffixes: &'a BTreeSet<String>,
    pub alpha: AlphaContent,
    pub is_float: bool,
}

fn has_component(path: &str, name: &str) -> bool {
    path.split('/').any(|c| c == name)
}

/// Spec §4.3 rule 1: under cs, a set is PBR if its path has a `pbr` component or it has _rmaos/_rfaos/_cnr/_f.
pub fn is_pbr_set(c: &SuggestContext) -> bool {
    c.target == Target::Cs
        && (has_component(c.game_path, "pbr") || ["rmaos", "rfaos", "cnr", "f"].iter().any(|s| c.set_suffixes.contains(*s)))
}

pub fn suggest(c: &SuggestContext) -> Vec<Suggestion> {
    use Confidence::*;
    use Subtype::*;
    let s = |st: Subtype, conf: Confidence, why: &str| Suggestion { subtype: st, confidence: conf, reason: why.to_string() };
    let pbr = is_pbr_set(c);
    let landscape = c.game_path.starts_with("textures/landscape/") || has_component(c.game_path, "landscape");
    let ui_or_sky = has_component(c.game_path, "interface") || has_component(c.game_path, "sky");
    match c.suffix {
        Some("n") if landscape => vec![s(LandscapeNormal, High, "_n under textures/landscape")],
        Some("n") if pbr => vec![s(PbrNormal, High, "_n in a PBR set"), s(Normal, Low, "_n")],
        Some("n") => vec![s(Normal, High, "_n, non-PBR set")],
        Some("msn") => vec![s(Msn, High, "_msn model-space normal")],
        Some("rmaos") => vec![s(Rmaos, High, "_rmaos")],
        Some("rfaos") => vec![s(Rfaos, High, "_rfaos (CS advanced skin)")],
        Some("cnr") | Some("crn") => vec![s(CoatNormal, High, "_cnr/_crn coat normal + roughness")],
        Some("flow") => vec![s(Flow, High, "_flow hair flow map")],
        Some("p") => vec![s(Height, High, "_p height/parallax")],
        Some("f") => vec![s(PbrFuzz, High, "_f fuzz")],
        Some("g") if pbr => vec![s(PbrEmissive, High, "_g in a PBR set"), s(Glow, Low, "_g")],
        Some("g") => vec![s(Glow, High, "_g, non-PBR set")],
        Some("s") if c.set_suffixes.contains("msn") => vec![s(MsnSpecular, High, "_s with _msn in the set")],
        Some("s") if pbr => vec![s(PbrSubsurface, High, "_s in a PBR set (subsurface / coat colour)")],
        Some("s") => vec![s(SubsurfaceTint, Low, "_s, no PBR set, no _msn (PGPatcher subsurface tint?)")],
        Some("m") | Some("em") | Some("envmask") => {
            if c.target == Target::Vanilla {
                vec![s(Envmask, High, "_m under vanilla")]
            } else if c.alpha == AlphaContent::Varying {
                vec![s(ComplexMaterial, High, "_m with non-white alpha"), s(Envmask, Low, "_m")]
            } else {
                vec![s(Envmask, High, "_m with no or all-255 alpha")]
            }
        }
        Some("b") | Some("bl") => vec![s(BacklightTint, High, "_b/_bl backlight")],
        Some("sk") => vec![s(BacklightTint, Low, "_sk (unconfirmed meaning)")],
        Some("d") | None if landscape => vec![s(LandscapeDiffuse, High, "colour under textures/landscape")],
        Some("d") | None if ui_or_sky && c.is_float => vec![s(HdrSky, Medium, "float source under sky/")],
        Some("d") | None if ui_or_sky => vec![s(UiSky, Medium, "colour under interface/ or sky/")],
        Some("d") | None if pbr => vec![s(PbrAlbedo, Medium, "colour member of a PBR set")],
        Some("d") | None => vec![s(Diffuse, Medium, "colour member, non-PBR set")],
        Some(_) => vec![],
    }
}

/// Alpha-policy proposals. No alpha (or constant 255 where alpha is optional) resolves to remove with high
/// confidence; varying alpha on a mesh-dependent sub-type is only ever a low-confidence proposal.
pub fn suggest_alpha(st: Subtype, alpha: AlphaContent, near_binary_share: f64) -> Option<(AlphaPolicy, Confidence, String)> {
    match alpha {
        AlphaContent::None => Some((AlphaPolicy::Remove, Confidence::High, "source has no alpha channel".into())),
        AlphaContent::Constant255 if st.alpha_from_mesh() || st.is_roughness_member() => {
            Some((AlphaPolicy::Remove, Confidence::High, "alpha is constant 255".into()))
        }
        AlphaContent::Varying if st.alpha_from_mesh() => {
            if near_binary_share >= 0.9 {
                Some((AlphaPolicy::Cutout, Confidence::Low, "≥90% of alpha is ≤5 or ≥250; the mesh decides".into()))
            } else {
                Some((AlphaPolicy::Blend, Confidence::Low, "soft alpha; the mesh decides".into()))
            }
        }
        _ => None,
    }
}
```
Add `pub mod suggest;` to `lib.rs`.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p engine suggest`
Expected: 2 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/engine
git commit -m "feat(engine): suffix/path/set suggestions with confidence and alpha proposals"
```

---

### Task 25: Resolve (part 1) — `--set` overrides, sub-type decision and layered profile with provenance

**Files:**
- Create: `crates/engine/src/overrides.rs`, `crates/engine/src/policy.rs`
- Modify: `crates/engine/src/lib.rs`
- Test: `crates/engine/tests/overrides.rs`, `crates/engine/tests/policy_layers.rs`

**Interfaces:**
- Consumes: `presets::*`, `config::*`, `suggest::*`, `analyse::Analysis`, `discover::InputFile`, `group::TextureSet`.
- Produces:
  - `overrides::Scope { All, Subtype(Subtype), File(String) }`; `overrides::SetOverride { pub scope: Scope, pub field: String, pub value: serde_json::Value }`; `overrides::FIELD_PATHS: &[&str]`; `overrides::parse_set(s: &str) -> Result<SetOverride, String>`; `overrides::to_patch(field: &str, value: &Value) -> Value` (dotted path → nested object)
  - `policy::AcceptSuggestions { None, High }`
  - `policy::ResolveCtx<'a> { pub target: Target, pub registry: &'a PresetRegistry, pub preset: &'a str, pub user_config: Option<&'a ConfigFile>, pub project_config: Option<&'a ConfigFile>, pub overrides: &'a [SetOverride], pub accept: AcceptSuggestions, pub allow: &'a BTreeSet<Code> }`
  - `policy::FileFacts<'a> { pub input: &'a InputFile, pub suffix: Option<&'a str>, pub set: &'a TextureSet, pub analysis: &'a Analysis, pub info: &'a SourceInfo }`
  - `policy::SubtypeDecision { pub subtype: Subtype, pub provenance: Provenance, pub decided: bool, pub suggestions: Vec<Suggestion> }`
  - `policy::decide_subtype(ctx: &ResolveCtx, f: &FileFacts) -> SubtypeDecision`
  - `policy::layered_profile(ctx: &ResolveCtx, f: &FileFacts, st: Subtype) -> Result<(Profile, BTreeMap<String, Provenance>), PresetError>`
  - `policy::rule_matches(m: &RuleMatch, f: &FileFacts) -> bool`

Precedence (spec §8, lowest → highest): built-in (`sdc-base`) → preset chain → user config → project config → rule matches (first match per source, sources in the same order) → analysis facts (no alpha / constant-255 → `remove`, only where `alpha.policy` is not already set by a preset, config or rule) → `--set` (unscoped, then sub-type-scoped, then file-scoped, in command-line order). Analysis-derived `remove` is applied regardless of `--accept-suggestions` because it is a fact about the source, not a guess (recorded as a plan decision).

- [ ] **Step 1: Write the failing override tests**

`crates/engine/tests/overrides.rs`:
```rust
use engine::config::Subtype;
use engine::overrides::*;
use serde_json::json;

#[test]
fn parses_scopes_and_typed_values() {
    let o = parse_set("subtype:rmaos.format=bc1").unwrap();
    assert_eq!((o.scope, o.field.as_str(), o.value), (Scope::Subtype(Subtype::Rmaos), "format", json!("bc1")));
    let o = parse_set("file:*.png.alpha.policy=blend").unwrap();
    assert_eq!((o.scope, o.field.as_str()), (Scope::File("*.png".into()), "alpha.policy"));
    let o = parse_set("file:**/wood01_*.png.alpha.threshold=100").unwrap();
    assert_eq!((o.field.as_str(), o.value), ("alpha.threshold", json!(100)));
    let o = parse_set("alpha.exact_opaque=true").unwrap();
    assert_eq!((o.scope, o.value), (Scope::All, json!(true)));
    assert_eq!(parse_set("mips.levels=none").unwrap().value, json!("none"));
    assert!(parse_set("colour.tag=srgb").is_err());
    assert!(parse_set("subtype:nope.format=bc7").is_err());
    assert!(parse_set("format").is_err());
    assert_eq!(to_patch("alpha.policy", &json!("data")), json!({"alpha": {"policy": "data"}}));
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p engine --test overrides`
Expected: FAIL to compile.

- [ ] **Step 3: Implement `overrides.rs`**

```rust
//! `--set` overrides (spec §8): `field=value`, `subtype:<id>.field=value`, `file:<glob>.field=value`.
use crate::config::Subtype;
use serde_json::{Map, Value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    All,
    Subtype(Subtype),
    File(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct SetOverride {
    pub scope: Scope,
    pub field: String,
    pub value: Value,
}

/// Every settable dotted path (Profile fields minus identity fields target/category).
pub const FIELD_PATHS: &[&str] = &[
    "subtype", "color.content", "color.tag", "alpha.policy", "alpha.threshold", "alpha.compare", "alpha.exact_opaque",
    "channels.swizzle", "channels.flip_green", "channels.reconstruct_z", "channels.grayscale_source", "mips.levels",
    "mips.filter", "mips.normal", "mips.coverage", "format", "header", "encoder.backend", "encoder.effort",
    "encoder.rgbcx_level", "encoder.perceptual", "encoder.bc1_transparent_black", "encoder.bc45_search_radius",
    "validation.min_mip_psnr_db", "validation.coverage_tolerance_pts", "validation.complex_m_warn_alpha",
    "validation.bc6h_min_peak_ratio", "validation.pgpatcher_non_white_share",
];

fn parse_value(raw: &str) -> Value {
    match toml::from_str::<toml::Table>(&format!("v = {raw}")) {
        Ok(t) => serde_json::to_value(&t["v"]).unwrap_or(Value::String(raw.to_string())),
        Err(_) => Value::String(raw.to_string()),
    }
}

pub fn parse_set(s: &str) -> Result<SetOverride, String> {
    let (key, raw) = s.split_once('=').ok_or_else(|| format!("--set '{s}': expected <field>=<value>"))?;
    let value = parse_value(raw.trim());
    let known = |f: &str| FIELD_PATHS.contains(&f);
    if let Some(rest) = key.strip_prefix("subtype:") {
        let (id, field) = rest.split_once('.').ok_or_else(|| format!("--set '{s}': expected subtype:<id>.<field>"))?;
        let st: Subtype = serde_json::from_value(Value::String(id.to_string())).map_err(|_| format!("--set '{s}': unknown sub-type '{id}'"))?;
        if !known(field) {
            return Err(format!("--set '{s}': unknown field '{field}'"));
        }
        return Ok(SetOverride { scope: Scope::Subtype(st), field: field.into(), value });
    }
    if let Some(rest) = key.strip_prefix("file:") {
        let field = FIELD_PATHS
            .iter()
            .filter(|f| rest.len() > f.len() + 1 && rest.ends_with(&format!(".{f}")))
            .max_by_key(|f| f.len())
            .ok_or_else(|| format!("--set '{s}': expected file:<glob>.<field>"))?;
        let glob = &rest[..rest.len() - field.len() - 1];
        return Ok(SetOverride { scope: Scope::File(glob.to_string()), field: field.to_string(), value });
    }
    if !known(key) {
        return Err(format!("--set '{s}': unknown field '{key}'"));
    }
    Ok(SetOverride { scope: Scope::All, field: key.to_string(), value })
}

pub fn to_patch(field: &str, value: &Value) -> Value {
    let mut v = value.clone();
    for part in field.rsplit('.') {
        let mut m = Map::new();
        m.insert(part.to_string(), v);
        v = Value::Object(m);
    }
    v
}
```
Add `pub mod overrides;` to `lib.rs`. Run `cargo test -p engine --test overrides` → 1 passed.

- [ ] **Step 4: Write the failing layering tests**

`crates/engine/tests/policy_layers.rs`:
```rust
use engine::analyse::analyse;
use engine::config::*;
use engine::discover::{InputFile, InputKind};
use engine::group::TextureSet;
use engine::image::{Rgba32F, SourceInfo};
use engine::overrides::parse_set;
use engine::policy::*;
use engine::presets::*;
use std::collections::BTreeSet;
use std::path::PathBuf;

pub fn input(game: &str) -> InputFile {
    let name = game.rsplit('/').next().unwrap();
    InputFile { path: PathBuf::from(name), rel: PathBuf::from(name), game_path: game.into(), output: PathBuf::from(name).with_extension("dds"), kind: InputKind::Image }
}

const RGBA8: SourceInfo = SourceInfo { bit_depth: 8, channels: 4, has_alpha: true, is_float: false };
const RGB8: SourceInfo = SourceInfo { bit_depth: 8, channels: 3, has_alpha: false, is_float: false };

#[test]
fn explicit_set_decides_subtype_and_beats_presets() {
    let reg = PresetRegistry::builtin().unwrap();
    let overrides = [parse_set("file:**/x_n.png.subtype=normal").unwrap(), parse_set("subtype:normal.format=bc1").unwrap()];
    let allow = BTreeSet::new();
    let ctx = ResolveCtx { target: Target::Cs, registry: &reg, preset: "cs-quality", user_config: None, project_config: None, overrides: &overrides, accept: AcceptSuggestions::None, allow: &allow };
    let img = Rgba32F::new(4, 4, [0.5, 0.5, 1.0, 0.5]);
    let a = analyse(&img, &RGBA8);
    let f = input("textures/a/x_n.png");
    let set = TextureSet { key: "x".into(), members: vec![0], suffixes: ["n".to_string()].into() };
    let facts = FileFacts { input: &f, suffix: Some("n"), set: &set, analysis: &a, info: &RGBA8 };
    let d = decide_subtype(&ctx, &facts);
    assert_eq!((d.subtype, d.decided, d.provenance.clone()), (Subtype::Normal, true, Provenance::User));
    let (p, prov) = layered_profile(&ctx, &facts, Subtype::Normal).unwrap();
    assert_eq!(p.format, Format::Bc1);
    assert_eq!(prov["format"], Provenance::User);
    assert_eq!(prov["alpha.policy"], Provenance::Builtin);
}

#[test]
fn suggestions_decide_only_when_high_and_accepted() {
    let reg = PresetRegistry::builtin().unwrap();
    let allow = BTreeSet::new();
    let img = Rgba32F::new(4, 4, [0.5, 0.5, 0.5, 1.0]);
    let a = analyse(&img, &RGB8);
    let f = input("textures/a/x_rmaos.png");
    let set = TextureSet { key: "x".into(), members: vec![0], suffixes: ["rmaos".to_string()].into() };
    let facts = FileFacts { input: &f, suffix: Some("rmaos"), set: &set, analysis: &a, info: &RGB8 };
    for (accept, decided) in [(AcceptSuggestions::None, false), (AcceptSuggestions::High, true)] {
        let ctx = ResolveCtx { target: Target::Cs, registry: &reg, preset: "cs-quality", user_config: None, project_config: None, overrides: &[], accept, allow: &allow };
        let d = decide_subtype(&ctx, &facts);
        assert_eq!((d.subtype, d.decided), (Subtype::Rmaos, decided));
    }
    // RGB source: alpha fact -> remove, recorded as Analysis
    let ctx = ResolveCtx { target: Target::Cs, registry: &reg, preset: "cs-quality", user_config: None, project_config: None, overrides: &[], accept: AcceptSuggestions::High, allow: &allow };
    let (p, prov) = layered_profile(&ctx, &facts, Subtype::Rmaos).unwrap();
    assert_eq!((p.alpha.policy, prov["alpha.policy"].clone()), (AlphaPolicy::Remove, Provenance::Analysis));
}

#[test]
fn rules_and_project_config_layer_in_order() {
    let reg = PresetRegistry::builtin().unwrap();
    let project: ConfigFile = toml::from_str(
        "[subtypes.diffuse]\nformat = \"bc1\"\n[[rules]]\nmatch = { glob = \"**/ui/**\" }\nsubtype = \"diffuse\"\noverrides = { mips = { levels = 1 } }\n",
    )
    .unwrap();
    let allow = BTreeSet::new();
    let ctx = ResolveCtx { target: Target::Vanilla, registry: &reg, preset: "vanilla-quality", user_config: None, project_config: Some(&project), overrides: &[], accept: AcceptSuggestions::None, allow: &allow };
    let img = Rgba32F::new(4, 4, [0.5, 0.5, 0.5, 1.0]);
    let a = analyse(&img, &RGB8);
    let f = input("textures/ui/icon.png");
    let set = TextureSet { key: "i".into(), members: vec![0], suffixes: BTreeSet::new() };
    let facts = FileFacts { input: &f, suffix: None, set: &set, analysis: &a, info: &RGB8 };
    let d = decide_subtype(&ctx, &facts);
    assert_eq!((d.subtype, d.decided, d.provenance.clone()), (Subtype::Diffuse, true, Provenance::Rule("project#0".into())));
    let (p, prov) = layered_profile(&ctx, &facts, d.subtype).unwrap();
    assert_eq!((p.format, p.mips.levels), (Format::Bc1, MipLevels::Count(1)));
    assert_eq!(prov["format"], Provenance::ProjectConfig);
    assert_eq!(prov["mips.levels"], Provenance::Rule("project#0".into()));
}
```

- [ ] **Step 5: Run to verify it fails**

Run: `cargo test -p engine --test policy_layers`
Expected: FAIL to compile.

- [ ] **Step 6: Implement the layering part of `policy.rs`**

```rust
//! Resolve (spec §5.2): merge preset, config, rules, analysis and overrides into an immutable ResolvedJob.
use crate::analyse::Analysis;
use crate::codes::Code;
use crate::config::*;
use crate::discover::InputFile;
use crate::group::TextureSet;
use crate::image::SourceInfo;
use crate::overrides::{to_patch, Scope, SetOverride};
pub use crate::presets::Provenance;
use crate::presets::*;
use crate::suggest::*;
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum AcceptSuggestions {
    None,
    High,
}

pub struct ResolveCtx<'a> {
    pub target: Target,
    pub registry: &'a PresetRegistry,
    pub preset: &'a str,
    pub user_config: Option<&'a ConfigFile>,
    pub project_config: Option<&'a ConfigFile>,
    pub overrides: &'a [SetOverride],
    pub accept: AcceptSuggestions,
    pub allow: &'a BTreeSet<Code>,
}

pub struct FileFacts<'a> {
    pub input: &'a InputFile,
    pub suffix: Option<&'a str>,
    pub set: &'a TextureSet,
    pub analysis: &'a Analysis,
    pub info: &'a SourceInfo,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SubtypeDecision {
    pub subtype: Subtype,
    pub provenance: Provenance,
    pub decided: bool,
    pub suggestions: Vec<Suggestion>,
}

fn glob_match(glob: &str, f: &FileFacts) -> bool {
    let Ok(g) = globset::GlobBuilder::new(&glob.to_ascii_lowercase()).literal_separator(true).build() else { return false };
    let m = g.compile_matcher();
    let rel = f.input.rel.to_string_lossy().replace('\\', "/").to_ascii_lowercase();
    m.is_match(&f.input.game_path) || m.is_match(&rel)
}

pub fn rule_matches(m: &RuleMatch, f: &FileFacts) -> bool {
    m.glob.as_deref().is_none_or(|g| glob_match(g, f))
        && m.suffix.as_deref().is_none_or(|s| f.suffix.unwrap_or("") == s.to_ascii_lowercase())
        && m.set_has.as_deref().is_none_or(|s| f.set.suffixes.contains(&s.to_ascii_lowercase()))
        && m.alpha.is_none_or(|a| a == f.analysis.alpha)
}

/// Rule sources in precedence order: preset chain (base first), user config, project config.
fn rule_sources<'a>(ctx: &'a ResolveCtx) -> Vec<(String, &'a [Rule])> {
    let mut v: Vec<(String, &[Rule])> = vec![];
    if let Ok(chain) = ctx.registry.chain(ctx.preset) {
        for p in chain {
            v.push((p.id.clone(), &p.rules));
        }
    }
    if let Some(c) = ctx.user_config {
        v.push(("user".into(), &c.rules));
    }
    if let Some(c) = ctx.project_config {
        v.push(("project".into(), &c.rules));
    }
    v
}

fn first_match<'a>(rules: &'a [Rule], f: &FileFacts) -> Option<(usize, &'a Rule)> {
    rules.iter().enumerate().find(|(_, r)| rule_matches(&r.matcher, f))
}

fn override_applies(o: &SetOverride, st: Option<Subtype>, f: &FileFacts) -> bool {
    match &o.scope {
        Scope::All => true,
        Scope::Subtype(s) => Some(*s) == st,
        Scope::File(g) => glob_match(g, f),
    }
}

pub fn decide_subtype(ctx: &ResolveCtx, f: &FileFacts) -> SubtypeDecision {
    let suggestions = suggest(&SuggestContext {
        target: ctx.target,
        suffix: f.suffix,
        game_path: &f.input.game_path,
        set_suffixes: &f.set.suffixes,
        alpha: f.analysis.alpha,
        is_float: f.info.is_float,
    });
    // --set subtype (unscoped or file-scoped), last one wins
    if let Some(o) = ctx.overrides.iter().rev().find(|o| o.field == "subtype" && override_applies(o, None, f)) {
        if let Ok(st) = serde_json::from_value::<Subtype>(o.value.clone()) {
            return SubtypeDecision { subtype: st, provenance: Provenance::User, decided: true, suggestions };
        }
    }
    // rules: highest-precedence source with a matching rule that names a sub-type
    for (src, rules) in rule_sources(ctx).into_iter().rev() {
        if let Some((i, r)) = first_match(rules, f) {
            if let Some(st) = r.subtype {
                return SubtypeDecision { subtype: st, provenance: Provenance::Rule(format!("{src}#{i}")), decided: true, suggestions };
            }
        }
    }
    match suggestions.first() {
        Some(s) => {
            let decided = ctx.accept == AcceptSuggestions::High && s.confidence == Confidence::High;
            SubtypeDecision { subtype: s.subtype, provenance: Provenance::Suggestion, decided, suggestions }
        }
        None => SubtypeDecision { subtype: Subtype::GenericData, provenance: Provenance::Suggestion, decided: false, suggestions },
    }
}

fn apply(doc: &mut Value, prov: &mut BTreeMap<String, Provenance>, patch: &Value, source: Provenance) {
    let mut paths = vec![];
    leaf_paths(patch, "", &mut paths);
    merge_json(doc, patch);
    for p in paths {
        prov.insert(p, source.clone());
    }
}

fn config_patches(c: &ConfigFile, st: Subtype) -> Vec<Value> {
    let mut v = vec![c.defaults.clone()];
    if let Some(s) = c.subtypes.get(&st) {
        v.push(s.clone());
    }
    v
}

pub fn layered_profile(ctx: &ResolveCtx, f: &FileFacts, st: Subtype) -> Result<(Profile, BTreeMap<String, Provenance>), PresetError> {
    let mut doc = Value::Object(Map::new());
    let mut prov = BTreeMap::new();
    for (source, patch) in ctx.registry.layers(ctx.preset, st)? {
        apply(&mut doc, &mut prov, &patch, source);
    }
    for (cfg, source) in [(ctx.user_config, Provenance::UserConfig), (ctx.project_config, Provenance::ProjectConfig)] {
        if let Some(c) = cfg {
            for patch in config_patches(c, st) {
                apply(&mut doc, &mut prov, &patch, source.clone());
            }
        }
    }
    for (src, rules) in rule_sources(ctx) {
        if let Some((i, r)) = first_match(rules, f) {
            apply(&mut doc, &mut prov, &r.overrides, Provenance::Rule(format!("{src}#{i}")));
        }
    }
    let policy_is_default = matches!(prov.get("alpha.policy"), None | Some(Provenance::Builtin));
    if policy_is_default {
        if let Some((policy, Confidence::High, _)) = suggest_alpha(st, f.analysis.alpha, near_binary(f.analysis)) {
            apply(&mut doc, &mut prov, &to_patch("alpha.policy", &serde_json::to_value(policy).unwrap()), Provenance::Analysis);
        }
    }
    for o in ctx.overrides.iter().filter(|o| o.field != "subtype") {
        if override_applies(o, Some(st), f) {
            apply(&mut doc, &mut prov, &to_patch(&o.field, &o.value), Provenance::User);
        }
    }
    let profile = finish_profile(doc, ctx.target, st)?;
    Ok((profile, prov))
}

pub fn near_binary(a: &Analysis) -> f64 {
    let n = (a.width as f64 * a.height as f64).max(1.0);
    (a.alpha_hist.le5 + a.alpha_hist.ge250) as f64 / n
}
```
Add `pub mod policy;` to `lib.rs`. (`Option::is_none_or` is stable since Rust 1.82.)

- [ ] **Step 7: Run the tests**

Run: `cargo test -p engine --test overrides --test policy_layers`
Expected: 4 passed.

- [ ] **Step 8: Commit**

```bash
git add crates/engine
git commit -m "feat(engine): --set overrides, sub-type decision and layered profile with provenance"
```

---

### Task 26: Resolve (part 2) — availability, checks, status and `resolve_file`

**Files:**
- Modify: `crates/engine/src/policy.rs`
- Test: `crates/engine/tests/policy_checks.rs`

**Interfaces:**
- Consumes: Task 25.
- Produces:
  - `policy::Availability { Available, Unavailable { fallback: Option<Subtype> } }`; `policy::availability(t: Target, s: Subtype) -> Availability`
  - `policy::check_profile(p: &Profile, f: &FileFacts) -> Vec<Warning>`
  - `policy::JobStatus { Ready, Unresolved { reasons: Vec<String> }, Blocked { codes: Vec<Code> }, Error { codes: Vec<Code> } }` (serde tag `state`)
  - `policy::ResolvedJob { pub input: PathBuf, pub output: PathBuf, pub game_path: String, pub set_key: String, pub profile: Profile, pub provenance: BTreeMap<String, Provenance>, pub suggestions: Vec<Suggestion>, pub warnings: Vec<Warning>, pub status: JobStatus }` (Serialize, JsonSchema)
  - `policy::resolve_file(ctx: &ResolveCtx, f: &FileFacts) -> ResolvedJob`

Checks implemented (code → trigger):
`CFG-UNAVAILABLE` (§4.2 "—" cells; BC6H outside `cs`), `CFG-INVALID` (threshold outside 5–200; backend/format mismatch; swizzle not 4 of `rgba01`), `TAG-SRGB-VANILLA` (srgb tag and target ≠ cs), `TAG-SRGB-NONPBR` (srgb tag, cs, not PBR colour), `TAG-UNORM-PBR` (unorm tag on PBR colour, format ≠ BC6H), `TAG-SRGB-LOD` (srgb tag and a `lod` path component), `TAG-CONTENT-MISMATCH` (linear content with srgb tag, or Color category with linear content and a non-float source), `FMT-BC5-NORMAL` (Normal category with BC4/BC5), `FMT-LANDSCAPE-BC1`, `FMT-BC1-ALPHA` (BC1 and policy ≠ remove), `FMT-PERCEPTUAL-DATA` (perceptual and not (Color and policy ≠ data)), `LOSS-ALPHA-DATA` (varying source alpha and (policy remove or no-alpha format)), `LOSS-BITDEPTH` (16-bit/float source, format ≠ BC6H), `CHAN-NOT-GRAY` (Grayscale, not grayscale), `CHAN-CONSTANT-ALPHA-DATA` (policy data, alpha constant 255), `CHAN-NORMAL-LENGTH` (Normal category, `normal_len_err` > 0.1, not `reconstruct_z`), `CHAN-AO-INVERTED`, `CHAN-GLOSS-AS-ROUGH` (roughness member hints), `ALPHA-EXACT-OFF-M` (envmask/complex_material, alpha-carrying format, flag off), `DIM-NPOT` (not power of two), `DIM-SMALL` (dimension < 4), `DIM-FLOW-SMALL` (flow ≤ 32), `ENC-NVTT-MISSING` (backend nvtt — the backend arrives in M5), `SUG-LOW-CONFIDENCE` (unresolved). Blocking codes in `allow` become warnings. `DIM-TOO-LARGE` and `IN-DECODE` come from Load (Task 37).

- [ ] **Step 1: Write the failing tests**

`crates/engine/tests/policy_checks.rs`:
```rust
use engine::analyse::analyse;
use engine::codes::Code;
use engine::config::*;
use engine::discover::{InputFile, InputKind};
use engine::group::TextureSet;
use engine::image::{Rgba32F, SourceInfo};
use engine::overrides::parse_set;
use engine::policy::*;
use engine::presets::*;
use std::collections::BTreeSet;
use std::path::PathBuf;

const RGBA8: SourceInfo = SourceInfo { bit_depth: 8, channels: 4, has_alpha: true, is_float: false };

fn run(target: Target, game: &str, suffix: Option<&str>, sets: &[&str], img: Rgba32F, sets_cli: &[&str], allow: &[Code]) -> ResolvedJob {
    let reg = PresetRegistry::builtin().unwrap();
    let overrides: Vec<_> = sets_cli.iter().map(|s| parse_set(s).unwrap()).collect();
    let allow: BTreeSet<Code> = allow.iter().copied().collect();
    let ctx = ResolveCtx { target, registry: &reg, preset: default_preset_for(target), user_config: None, project_config: None, overrides: &overrides, accept: AcceptSuggestions::High, allow: &allow };
    let name = game.rsplit('/').next().unwrap();
    let f = InputFile { path: name.into(), rel: name.into(), game_path: game.into(), output: PathBuf::from(name).with_extension("dds"), kind: InputKind::Image };
    let set = TextureSet { key: "k".into(), members: vec![0], suffixes: sets.iter().map(|s| s.to_string()).collect() };
    let a = analyse(&img, &RGBA8);
    resolve_file(&ctx, &FileFacts { input: &f, suffix, set: &set, analysis: &a, info: &RGBA8 })
}

fn codes(j: &ResolvedJob) -> Vec<Code> {
    j.warnings.iter().map(|w| w.code).collect()
}

fn opaque() -> Rgba32F {
    Rgba32F::new(64, 64, [0.5, 0.5, 1.0, 1.0])
}

#[test]
fn ready_normal_under_cs() {
    let j = run(Target::Cs, "textures/a/x_n.png", Some("n"), &["n"], opaque(), &[], &[]);
    assert_eq!(j.status, JobStatus::Ready, "{:?}", j.warnings);
    assert_eq!(j.profile.subtype, Subtype::Normal);
}

#[test]
fn bc5_on_normal_blocks_unless_allowed() {
    let j = run(Target::Cs, "textures/a/x_n.png", Some("n"), &["n"], opaque(), &["subtype:normal.format=bc5", "subtype:normal.encoder.backend=rgbcx"], &[]);
    assert_eq!(j.status, JobStatus::Blocked { codes: vec![Code::FmtBc5Normal] });
    let j = run(Target::Cs, "textures/a/x_n.png", Some("n"), &["n"], opaque(), &["subtype:normal.format=bc5", "subtype:normal.encoder.backend=rgbcx"], &[Code::FmtBc5Normal]);
    assert_eq!(j.status, JobStatus::Ready);
    assert!(codes(&j).contains(&Code::FmtBc5Normal));
}

#[test]
fn pbr_albedo_unavailable_under_vanilla_and_srgb_is_an_error_there() {
    let j = run(Target::Vanilla, "textures/a/x.png", None, &[], opaque(), &["subtype=pbr_albedo"], &[]);
    assert!(matches!(j.status, JobStatus::Error { .. } | JobStatus::Blocked { .. }));
    assert!(codes(&j).contains(&Code::CfgUnavailable));
    let j = run(Target::Vanilla, "textures/a/x.png", None, &[], opaque(), &["subtype=diffuse", "color.tag=srgb"], &[]);
    assert_eq!(j.status, JobStatus::Error { codes: vec![Code::TagSrgbVanilla] });
}

#[test]
fn diffuse_with_varying_alpha_is_unresolved_until_policy_is_set() {
    let mut img = opaque();
    img.px[0][3] = 0.0;
    let j = run(Target::Cs, "textures/a/x.png", None, &[], img.clone(), &["subtype=diffuse"], &[]);
    assert!(matches!(j.status, JobStatus::Unresolved { .. }), "{:?}", j.status);
    let j = run(Target::Cs, "textures/a/x.png", None, &[], img, &["subtype=diffuse", "alpha.policy=cutout"], &[]);
    assert_eq!(j.status, JobStatus::Ready, "{:?}", j.warnings);
    assert!(j.profile.coverage_enabled());
}

#[test]
fn threshold_range_and_npot() {
    let j = run(Target::Cs, "textures/a/x_n.png", Some("n"), &["n"], opaque(), &["alpha.threshold=250"], &[]);
    assert!(codes(&j).contains(&Code::CfgInvalid));
    let j = run(Target::Cs, "textures/a/x_n.png", Some("n"), &["n"], Rgba32F::new(60, 64, [0.5, 0.5, 1.0, 1.0]), &[], &[]);
    assert_eq!(j.status, JobStatus::Blocked { codes: vec![Code::DimNpot] });
}

#[test]
fn availability_table() {
    assert_eq!(availability(Target::Cpm, Subtype::PbrNormal), Availability::Unavailable { fallback: Some(Subtype::Normal) });
    assert_eq!(availability(Target::Vanilla, Subtype::ComplexMaterial), Availability::Unavailable { fallback: Some(Subtype::Envmask) });
    assert_eq!(availability(Target::Cpm, Subtype::ComplexMaterial), Availability::Available);
    assert_eq!(availability(Target::Vanilla, Subtype::Flow), Availability::Unavailable { fallback: None });
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p engine --test policy_checks`
Expected: FAIL to compile.

- [ ] **Step 3: Implement**

Append to `policy.rs`:
```rust
use crate::codes::{Severity, Warning};
use serde_json::json;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Availability {
    Available,
    Unavailable { fallback: Option<Subtype> },
}

/// The "—" cells of spec §4.2.
pub fn availability(t: Target, s: Subtype) -> Availability {
    use Subtype::*;
    let un = |f: Option<Subtype>| Availability::Unavailable { fallback: f };
    match (t, s) {
        (Target::Cs, _) => Availability::Available,
        (_, PbrAlbedo) => un(Some(Diffuse)),
        (_, PbrEmissive) => un(Some(Glow)),
        (_, PbrNormal) => un(Some(Normal)),
        (_, PbrSubsurface | PbrFuzz | CoatNormal | Rmaos | Rfaos | Flow | HdrSky) => un(None),
        (Target::Vanilla, ComplexMaterial) => un(Some(Envmask)),
        _ => Availability::Available,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, schemars::JsonSchema)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum JobStatus {
    Ready,
    Unresolved { reasons: Vec<String> },
    Blocked { codes: Vec<Code> },
    Error { codes: Vec<Code> },
}

#[derive(Debug, Clone, serde::Serialize, schemars::JsonSchema)]
pub struct ResolvedJob {
    pub input: PathBuf,
    pub output: PathBuf,
    pub game_path: String,
    pub set_key: String,
    pub profile: Profile,
    pub provenance: BTreeMap<String, Provenance>,
    pub suggestions: Vec<Suggestion>,
    pub warnings: Vec<Warning>,
    pub status: JobStatus,
}

fn backend_fits(b: Backend, f: Format) -> bool {
    match b {
        Backend::Bc7e => f == Format::Bc7,
        Backend::Rgbcx => matches!(f, Format::Bc1 | Format::Bc3 | Format::Bc4 | Format::Bc5),
        Backend::IspcBc6h => f == Format::Bc6hUf16,
        Backend::Nvtt => matches!(f, Format::Bc7 | Format::Bc6hUf16),
    }
}

pub fn check_profile(p: &Profile, f: &FileFacts) -> Vec<Warning> {
    use Code::*;
    let a = f.analysis;
    let mut w = vec![];
    let mut add = |c: Code, m: String| w.push(Warning::new(c, m));
    let st = p.subtype;
    if let Availability::Unavailable { fallback } = availability(p.target, st) {
        add(CfgUnavailable, format!("{} is unavailable under {:?}; fallback: {:?}", st.id(), p.target, fallback));
    }
    if p.format == Format::Bc6hUf16 && p.target != Target::Cs {
        add(CfgUnavailable, "BC6H is usable only under Community Shaders [D#3]".into());
    }
    if !(5..=200).contains(&p.alpha.threshold) {
        add(CfgInvalid, format!("alpha.threshold {} outside 5..=200", p.alpha.threshold));
    }
    if !backend_fits(p.encoder.backend, p.format) {
        add(CfgInvalid, format!("encoder {:?} cannot write {:?}", p.encoder.backend, p.format));
    }
    if let Some(s) = &p.channels.swizzle {
        if s.len() != 4 || !s.chars().all(|c| "rgba01".contains(c)) {
            add(CfgInvalid, format!("channels.swizzle '{s}' must be 4 of r,g,b,a,0,1"));
        }
    }
    let srgb = p.srgb_tag();
    if srgb && p.target != Target::Cs {
        add(TagSrgbVanilla, "_SRGB is not usable under vanilla or CPM shaders [D#4]".into());
    } else if srgb && !st.is_pbr_color() {
        add(TagSrgbNonpbr, "_SRGB on a non-PBR sub-type renders darker [SC §6]".into());
    }
    if p.color.tag == ColorTag::Unorm && st.is_pbr_color() && p.format != Format::Bc6hUf16 && p.target == Target::Cs {
        add(TagUnormPbr, "UNORM on PBR colour looks washed out with linear lighting off".into());
    }
    if srgb && f.input.game_path.split('/').any(|c| c == "lod") {
        add(TagSrgbLod, "LOD shaders reject _SRGB".into());
    }
    if (p.color.content == ColorContent::Linear && srgb)
        || (p.category == Category::Color && p.color.content == ColorContent::Linear && !f.info.is_float)
    {
        add(TagContentMismatch, "colour content and DDS tag disagree".into());
    }
    if p.category == Category::Normal && matches!(p.format, Format::Bc4 | Format::Bc5) {
        add(FmtBc5Normal, "BC4/BC5 on a normal: Skyrim reads Z directly, so Z decodes -1 and alpha is lost [SC §2]".into());
    }
    if st == Subtype::LandscapeDiffuse && p.format == Format::Bc1 {
        add(FmtLandscapeBc1, "BC1 destroys the landscape snow mask / height alpha".into());
    }
    if p.format == Format::Bc1 && p.alpha.policy != AlphaPolicy::Remove {
        add(FmtBc1Alpha, "BC1 in v1 is opaque only".into());
    }
    if p.encoder.perceptual && !(p.category == Category::Color && p.alpha.policy != AlphaPolicy::Data) {
        add(FmtPerceptualData, "perceptual BC7 is allowed only for colour without data alpha [B2 §1]".into());
    }
    if a.alpha == AlphaContent::Varying && (p.alpha.policy == AlphaPolicy::Remove || !p.format.has_alpha()) {
        add(LossAlphaData, "non-constant alpha would be discarded".into());
    }
    if (f.info.bit_depth > 8 || f.info.is_float) && p.format != Format::Bc6hUf16 {
        add(LossBitdepth, format!("{}-bit source written with 8-bit endpoints", f.info.bit_depth));
    }
    if p.category == Category::Grayscale && !a.grayscale {
        add(ChanNotGray, format!("R/G/B differ by more than 4/255; keeping {:?}", p.channels.grayscale_source));
    }
    if p.alpha.policy == AlphaPolicy::Data && a.alpha == AlphaContent::Constant255 {
        add(ChanConstantAlphaData, "data alpha is constant 255; remove would save bits".into());
    }
    if p.category == Category::Normal && st != Subtype::Msn && a.normal_len_err > 0.1 && !p.channels.reconstruct_z {
        add(ChanNormalLength, format!("mean |length-1| = {:.3}; is this a normal map, or does it need Z rebuilt?", a.normal_len_err));
    }
    if st.is_roughness_member() && a.blue_mostly_black {
        add(ChanAoInverted, "blue (AO) is mostly black: AO may be inverted".into());
    }
    if st.is_roughness_member() && a.red_mostly_high {
        add(ChanGlossAsRough, "red (roughness) is mostly high: is this gloss?".into());
    }
    if matches!(st, Subtype::Envmask | Subtype::ComplexMaterial) && p.format.has_alpha() && !p.alpha.exact_opaque && p.alpha.policy != AlphaPolicy::Remove {
        add(AlphaExactOffM, "exact opaque alpha is off on an _m that stores alpha; 254 may reach PGPatcher/ENB".into());
    }
    if !a.is_pot {
        add(DimNpot, format!("{}x{} is not a power of two", a.width, a.height));
    }
    if a.width.min(a.height) < 4 {
        add(DimSmall, "base dimension below 4 px".into());
    }
    if st == Subtype::Flow && a.width.min(a.height) <= 32 {
        add(DimFlowSmall, "CS ignores _flow maps of 32 px or less".into());
    }
    if p.encoder.backend == Backend::Nvtt {
        add(EncNvttMissing, "the NVTT backend arrives in milestone M5".into());
    }
    w
}

pub fn resolve_file(ctx: &ResolveCtx, f: &FileFacts) -> ResolvedJob {
    let d = decide_subtype(ctx, f);
    let mut reasons = vec![];
    if !d.decided {
        reasons.push(match d.suggestions.first() {
            Some(s) => format!("sub-type '{}' is a {:?}-confidence suggestion ({})", s.subtype.id(), s.confidence, s.reason),
            None => "no sub-type could be suggested; set one".into(),
        });
    }
    let (profile, provenance) = match layered_profile(ctx, f, d.subtype) {
        Ok(v) => v,
        Err(e) => {
            // The built-in base preset is valid by construction (Task 20 test), so this cannot fail.
            // Engine::plan (Task 37) validates presets, configs and rules up front, so this path is rare.
            let fallback = ctx.registry.profile(BASE_ID, ctx.target, d.subtype).expect("sdc-base profile is valid");
            return ResolvedJob {
                input: f.input.path.clone(),
                output: f.input.output.clone(),
                game_path: f.input.game_path.clone(),
                set_key: f.set.key.clone(),
                profile: fallback,
                provenance: BTreeMap::new(),
                suggestions: d.suggestions,
                warnings: vec![Warning::new(Code::CfgInvalid, e.to_string())],
                status: JobStatus::Error { codes: vec![Code::CfgInvalid] },
            };
        }
    };
    let mut provenance = provenance;
    provenance.insert("subtype".into(), d.provenance.clone());
    if profile.subtype.alpha_from_mesh()
        && f.analysis.alpha == AlphaContent::Varying
        && matches!(provenance.get("alpha.policy"), None | Some(Provenance::Builtin) | Some(Provenance::Preset(_)))
    {
        let hint = suggest_alpha(profile.subtype, f.analysis.alpha, near_binary(f.analysis)).map(|(p, _, why)| format!("{p:?}: {why}"));
        reasons.push(format!("alpha meaning is unconfirmed (proposal {})", hint.unwrap_or_default()));
    }
    let mut warnings = check_profile(&profile, f);
    if !reasons.is_empty() {
        warnings.push(Warning::new(Code::SugLowConfidence, reasons.join("; ")).with_data(json!({ "suggestions": d.suggestions })));
    }
    for w in &mut warnings {
        w.severity = w.code.effective(ctx.allow);
    }
    let errors: Vec<Code> = warnings.iter().filter(|w| w.severity == Severity::Error).map(|w| w.code).collect();
    let blocking: Vec<Code> = warnings
        .iter()
        .filter(|w| w.severity == Severity::Blocking && w.code != Code::SugLowConfidence)
        .map(|w| w.code)
        .collect();
    // Precedence: errors, then unresolved (the user must decide first; e.g. an unconfirmed diffuse alpha also
    // triggers LOSS-ALPHA-DATA against the default `remove`), then blocking codes.
    let status = if !errors.is_empty() {
        JobStatus::Error { codes: errors }
    } else if !reasons.is_empty() && !ctx.allow.contains(&Code::SugLowConfidence) {
        JobStatus::Unresolved { reasons }
    } else if !blocking.is_empty() {
        JobStatus::Blocked { codes: blocking }
    } else {
        JobStatus::Ready
    };
    ResolvedJob {
        input: f.input.path.clone(),
        output: f.input.output.clone(),
        game_path: f.input.game_path.clone(),
        set_key: f.set.key.clone(),
        profile,
        provenance,
        suggestions: d.suggestions,
        warnings,
        status,
    }
}
```
Note on `pbr_albedo_unavailable…`: with `subtype=pbr_albedo` under vanilla the base profile carries `tag = srgb`, so both `CFG-UNAVAILABLE` (blocking) and `TAG-SRGB-VANILLA` (error) fire; status is `Error`. The test accepts either.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p engine --test policy_checks`
Expected: 6 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/engine
git commit -m "feat(engine): availability per target, §10 checks and job status in resolve"
```

---
### Task 26a: Preset suffix aliases (DECISIONS Note G) — registry, alias-aware split, canonical output names, alias preset chain in Resolve

**Files:**
- Modify: `crates/engine/src/presets.rs`, `crates/engine/src/group.rs`, `crates/engine/src/discover.rs`, `crates/engine/src/policy.rs`
- Modify: `crates/engine/tests/policy_layers.rs`, `crates/engine/tests/policy_checks.rs` (add `alias: None` to each `FileFacts` literal)
- Test: `crates/engine/tests/suffix_aliases.rs`

**Interfaces:**
- Consumes: Tasks 20, 21, 24, 25, 26.
- Produces:
  - `presets::PresetFile` gains `pub suffix_aliases: BTreeMap<String, String>` (alias → canonical; `#[serde(default, skip_serializing_if = "BTreeMap::is_empty")]`, so the built-in presets' SHA-256 values do not change)
  - `presets::PresetError::Alias(String)` (message starts `CFG-INVALID:`; `Engine::plan` reports it as a usage error, exit 2)
  - `PresetRegistry::alias_for(&self, alias: &str) -> Option<(&str, &str)>` → `(canonical suffix, preset id)`, case-insensitive; `PresetRegistry::alias_table(&self) -> &BTreeMap<String, String>` (alias → canonical over all loaded presets)
  - `group::SuffixSplit { pub base: String, pub suffix: Option<String>, pub alias: Option<String> }`; `group::split_suffix_with(stem: &str, aliases: &BTreeMap<String, String>) -> SuffixSplit`; `group::group_sets_with(files: &[InputFile], aliases: &BTreeMap<String, String>) -> (Vec<TextureSet>, Vec<Vec<usize>>)`. `split_suffix` and `group_sets` keep their signatures and behaviour (no aliases), so Task 21's tests are untouched.
  - `discover::canonicalise_outputs(files: &mut [InputFile], aliases: &BTreeMap<String, String>)`; `discover` is unchanged.
  - `policy::FileFacts` gains `pub alias: Option<&'a str>`; `policy::AliasUse { pub alias: String, pub canonical: String, pub preset: String }`; `policy::preset_for<'a>(ctx: &ResolveCtx<'a>, f: &FileFacts) -> &'a str`; `policy::ResolvedJob` gains `pub suffix_alias: Option<AliasUse>` (omitted from JSON when `None`)

Rules (spec §3.7, DECISIONS Note G). An alias is `[a-z0-9]+`, is not a §6.1 suffix, maps to a §6.1 suffix, and is unique across loaded presets; otherwise `add_toml` fails and the registry is unchanged. The stem segment after the last `_` is looked up in the alias table before the suffix list, so matching is exact (`rmaosultrahigh` does not make `_nultrahigh` match). `FileFacts.suffix` carries the canonical suffix and `FileFacts.alias` the alias. For an aliased file, `preset_for` returns the alias preset, whose chain replaces the batch preset's chain in `layers()` and in the rule sources; config, analysis and `--set` layers are unchanged and still win. Outputs are canonicalised before grouping, so an alias/canonical collision is an ordinary `IN-DUPLICATE` group. An alias raises the top suggestion to `High` (except canonical `s`) and prefixes every suggestion reason with `suffix alias _<alias> → _<canonical> (preset <id>)`; deciding still needs `--accept-suggestions high`, a rule or `--set`.

- [ ] **Step 1: Write the failing tests**

`crates/engine/tests/suffix_aliases.rs`:
```rust
use engine::analyse::analyse;
use engine::config::*;
use engine::discover::*;
use engine::group::*;
use engine::image::{Rgba32F, SourceInfo};
use engine::overrides::parse_set;
use engine::policy::*;
use engine::presets::*;
use engine::suggest::Confidence;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

const ULTRA: &str = r#"
schema_version = 1
id = "ultra"
version = "1.0.0"
base = "cs-quality"
target = "cs"

[suffix_aliases]
rmaosultrahigh = "rmaos"
backlit = "sk"
skintint = "s"

[subtypes.rmaos]
mips = { levels = 1 }
"#;

const RGB8: SourceInfo = SourceInfo { bit_depth: 8, channels: 3, has_alpha: false, is_float: false };

fn registry() -> PresetRegistry {
    let mut r = PresetRegistry::builtin().unwrap();
    r.add_toml(ULTRA).unwrap();
    r
}

fn preset_with(id: &str, aliases: &str) -> String {
    format!("schema_version = 1\nid = \"{id}\"\nversion = \"1.0.0\"\nbase = \"cs-quality\"\n[suffix_aliases]\n{aliases}\n")
}

fn touch(p: &Path) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, b"x").unwrap();
}

fn resolve(reg: &PresetRegistry, name: &str, sets_cli: &[&str], accept: AcceptSuggestions) -> ResolvedJob {
    let overrides: Vec<_> = sets_cli.iter().map(|s| parse_set(s).unwrap()).collect();
    let allow = BTreeSet::new();
    let ctx = ResolveCtx { target: Target::Cs, registry: reg, preset: "cs-quality", user_config: None, project_config: None, overrides: &overrides, accept, allow: &allow };
    let stem = Path::new(name).file_stem().unwrap().to_string_lossy().into_owned();
    let split = split_suffix_with(&stem, reg.alias_table());
    let f = InputFile {
        path: PathBuf::from(name),
        rel: PathBuf::from(name),
        game_path: format!("textures/armor/{}", name.to_ascii_lowercase()),
        output: PathBuf::from(name).with_extension("dds"),
        kind: InputKind::Image,
    };
    let set = TextureSet { key: "textures/armor/iron".into(), members: vec![0], suffixes: split.suffix.iter().cloned().collect() };
    let a = analyse(&Rgba32F::new(64, 64, [0.5, 0.5, 0.5, 1.0]), &RGB8);
    resolve_file(&ctx, &FileFacts { input: &f, suffix: split.suffix.as_deref(), alias: split.alias.as_deref(), set: &set, analysis: &a, info: &RGB8 })
}

#[test]
fn registry_validates_and_looks_up_aliases() {
    let mut r = registry();
    assert_eq!(r.alias_for("rmaosultrahigh"), Some(("rmaos", "ultra")));
    assert_eq!(r.alias_for("RmaosUltraHigh"), Some(("rmaos", "ultra")));
    assert_eq!(r.alias_for("nultrahigh"), None);
    assert_eq!(r.alias_table().get("backlit").map(String::as_str), Some("sk"));
    // an alias may not be a canonical suffix, must name a canonical suffix, and is [a-z0-9]+
    for bad in ["rmaos = \"rmaos\"", "n = \"rmaos\"", "foo = \"bar\"", "foo = \"\"", "\"ultra_high\" = \"rmaos\"", "UltraHigh = \"rmaos\""] {
        assert!(matches!(r.add_toml(&preset_with("bad", bad)), Err(PresetError::Alias(_))), "{bad}");
        assert!(r.get("bad").is_none(), "{bad}");
    }
    // an alias declared by a second preset is rejected and the registry is unchanged
    assert!(matches!(r.add_toml(&preset_with("other", "rmaosultrahigh = \"rmaos\"")), Err(PresetError::Alias(_))));
    assert!(r.get("other").is_none());
    assert_eq!(r.alias_for("rmaosultrahigh"), Some(("rmaos", "ultra")));
    // reloading the same preset id replaces its aliases without a self-conflict
    r.add_toml(ULTRA).unwrap();
    assert_eq!(r.alias_table().len(), 3);
    // built-in presets declare no aliases
    assert!(PresetRegistry::builtin().unwrap().alias_table().is_empty());
}

#[test]
fn alias_matching_is_exact_and_case_insensitive() {
    let r = registry();
    let s = |stem: &str| split_suffix_with(stem, r.alias_table());
    let split = |base: &str, suffix: Option<&str>, alias: Option<&str>| SuffixSplit { base: base.into(), suffix: suffix.map(Into::into), alias: alias.map(Into::into) };
    assert_eq!(s("iron_rmaosultrahigh"), split("iron", Some("rmaos"), Some("rmaosultrahigh")));
    assert_eq!(s("Iron_RMAOSUltraHigh"), split("Iron", Some("rmaos"), Some("rmaosultrahigh")));
    // declaring rmaosultrahigh does not make another "<suffix>ultrahigh" match
    assert_eq!(s("iron_nultrahigh"), split("iron_nultrahigh", None, None));
    // no separator inside an alias
    assert_eq!(s("iron_rmaos_ultrahigh").alias, None);
    // a stem that is only the alias is left whole, as with built-in suffixes
    assert_eq!(s("_rmaosultrahigh"), split("_rmaosultrahigh", None, None));
    // built-in suffixes still split, and the alias-free splitter is unchanged
    assert_eq!(s("iron_n"), split("iron", Some("n"), None));
    assert_eq!(split_suffix("iron_rmaosultrahigh"), ("iron_rmaosultrahigh".into(), None));
}

#[test]
fn outputs_use_the_canonical_suffix_and_collisions_are_duplicates() {
    let r = registry();
    let d = tempfile::tempdir().unwrap();
    let src = d.path().join("in");
    for f in ["iron.png", "iron_n.png", "iron_rmaosultrahigh.png", "iron_nultrahigh.png", "rock_rmaos.png", "Rock_RMAOSULTRAHIGH.tga"] {
        touch(&src.join(f));
    }
    let out = d.path().join("out");
    let mut files = discover(&[src], &out, false).unwrap();
    canonicalise_outputs(&mut files, r.alias_table());
    let output_of = |name: &str| files.iter().find(|f| f.rel.as_path() == Path::new(name)).unwrap().output.clone();
    assert_eq!(output_of("iron_rmaosultrahigh.png"), out.join("iron_rmaos.dds"));
    assert_eq!(output_of("Rock_RMAOSULTRAHIGH.tga"), out.join("Rock_rmaos.dds"));
    assert_eq!(output_of("iron_nultrahigh.png"), out.join("iron_nultrahigh.dds"));
    assert_eq!(output_of("iron_n.png"), out.join("iron_n.dds"));
    let (sets, dups) = group_sets_with(&files, r.alias_table());
    let iron = sets.iter().find(|s| s.key.ends_with("/iron")).unwrap();
    assert_eq!(iron.members.len(), 3);
    assert!(iron.suffixes.contains("rmaos") && iron.suffixes.contains("n"));
    assert!(!iron.suffixes.contains("rmaosultrahigh"));
    // rock_rmaos.png and Rock_RMAOSULTRAHIGH.tga both write rock_rmaos.dds: one IN-DUPLICATE group
    assert_eq!(dups.len(), 1);
    let names: BTreeSet<String> = dups[0].iter().map(|&i| files[i].rel.to_string_lossy().into_owned()).collect();
    assert_eq!(names, BTreeSet::from(["rock_rmaos.png".to_string(), "Rock_RMAOSULTRAHIGH.tga".to_string()]));
}

#[test]
fn alias_preset_layers_on_its_chain_and_provenance_records_it() {
    let reg = registry();
    let j = resolve(&reg, "iron_rmaosultrahigh.png", &[], AcceptSuggestions::High);
    assert_eq!(j.status, JobStatus::Ready, "{:?}", j.warnings);
    assert_eq!(j.profile.subtype, Subtype::Rmaos);
    assert_eq!(j.profile.mips.levels, MipLevels::Count(1));
    assert_eq!(j.provenance["mips.levels"], Provenance::Preset("ultra".into()));
    assert_eq!(j.provenance["mips.levels"].to_string(), "preset:ultra");
    assert_eq!(j.suffix_alias, Some(AliasUse { alias: "rmaosultrahigh".into(), canonical: "rmaos".into(), preset: "ultra".into() }));
    assert!(j.suggestions[0].reason.starts_with("suffix alias _rmaosultrahigh → _rmaos (preset ultra)"), "{}", j.suggestions[0].reason);
    // the canonical name alone resolves with the batch preset only
    let plain = resolve(&reg, "iron_rmaos.png", &[], AcceptSuggestions::High);
    assert_eq!(plain.profile.mips.levels, MipLevels::Keyword(MipKeyword::Full));
    assert_eq!(plain.provenance["mips.levels"], Provenance::Builtin);
    assert_eq!(plain.suffix_alias, None);
    assert!(!plain.provenance.values().any(|p| *p == Provenance::Preset("ultra".into())));
    // --set still beats the alias preset
    let j = resolve(&reg, "iron_rmaosultrahigh.png", &["subtype:rmaos.mips.levels=none"], AcceptSuggestions::High);
    assert_eq!((j.profile.mips.levels, j.provenance["mips.levels"].clone()), (MipLevels::Keyword(MipKeyword::None), Provenance::User));
}

#[test]
fn alias_raises_suffix_confidence_except_for_s() {
    let reg = registry();
    // _sk alone is a low-confidence backlight; the alias declares it
    let j = resolve(&reg, "iron_backlit.png", &[], AcceptSuggestions::High);
    assert_eq!((j.profile.subtype, j.suggestions[0].confidence), (Subtype::BacklightTint, Confidence::High));
    assert_eq!(j.provenance["subtype"], Provenance::Suggestion);
    // _s keeps its ranked readings
    let j = resolve(&reg, "iron_skintint.png", &[], AcceptSuggestions::High);
    assert_eq!(j.profile.subtype, Subtype::SubsurfaceTint);
    assert!(j.suggestions[0].confidence < Confidence::High);
    assert!(matches!(j.status, JobStatus::Unresolved { .. }), "{:?}", j.status);
    // an alias decides nothing without --accept-suggestions high
    let j = resolve(&reg, "iron_rmaosultrahigh.png", &[], AcceptSuggestions::None);
    assert!(matches!(j.status, JobStatus::Unresolved { .. }), "{:?}", j.status);
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p engine --test suffix_aliases`
Expected: FAIL to compile (`split_suffix_with`, `alias_for`, `FileFacts.alias` … not found).

- [ ] **Step 3: Implement the preset field and registry lookup in `presets.rs`**

Add `use crate::group::SUFFIXES;` to the imports. Add to `PresetError`:
```rust
    #[error("invalid suffix alias: {0}")]
    Alias(String),
```
Add to `PresetFile`, after `rules`:
```rust
    /// Exact suffix aliases (spec §3.7, DECISIONS Note G): `alias = "canonical"`, e.g. `rmaosultrahigh = "rmaos"`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub suffix_aliases: BTreeMap<String, String>,
```
Replace the `PresetRegistry` struct and its `add_toml` with:
```rust
#[derive(Debug, Clone, Default)]
pub struct PresetRegistry {
    presets: BTreeMap<String, PresetFile>,
    /// Alias → canonical suffix, over every loaded preset.
    aliases: BTreeMap<String, String>,
    /// Alias → id of the preset that declares it.
    alias_owner: BTreeMap<String, String>,
}
```
```rust
    pub fn add_toml(&mut self, text: &str) -> Result<String, PresetError> {
        let p: PresetFile = toml::from_str(text).map_err(|e| PresetError::Parse(e.to_string()))?;
        if p.schema_version > SCHEMA_VERSION {
            return Err(PresetError::NewerSchema(p.schema_version));
        }
        self.check_aliases(&p)?;
        let id = p.id.clone();
        // Reloading a preset id replaces its aliases.
        let owner = &self.alias_owner;
        self.aliases.retain(|a, _| owner.get(a) != Some(&id));
        self.alias_owner.retain(|_, o| *o != id);
        for (alias, canonical) in &p.suffix_aliases {
            self.aliases.insert(alias.clone(), canonical.clone());
            self.alias_owner.insert(alias.clone(), id.clone());
        }
        self.presets.insert(id.clone(), p);
        Ok(id)
    }

    /// Spec §3.7: `[a-z0-9]+`, not a §6.1 suffix, maps to a §6.1 suffix, unique across loaded presets.
    fn check_aliases(&self, p: &PresetFile) -> Result<(), PresetError> {
        let bad = |m: String| PresetError::Alias(format!("CFG-INVALID: preset '{}': {m}", p.id));
        for (alias, canonical) in &p.suffix_aliases {
            if alias.is_empty() || !alias.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()) {
                return Err(bad(format!("alias '{alias}' must be lower-case ASCII letters and digits, without '_'")));
            }
            if SUFFIXES.contains(&alias.as_str()) {
                return Err(bad(format!("alias '{alias}' is a canonical suffix (spec §6.1)")));
            }
            if !SUFFIXES.contains(&canonical.as_str()) {
                return Err(bad(format!("alias '{alias}' maps to '{canonical}', which is not a canonical suffix (spec §6.1)")));
            }
            if let Some(owner) = self.alias_owner.get(alias).filter(|o| **o != p.id) {
                return Err(bad(format!("alias '{alias}' is already declared by preset '{owner}'")));
            }
        }
        Ok(())
    }

    /// `(canonical suffix, declaring preset id)` for an alias, case-insensitive.
    pub fn alias_for(&self, alias: &str) -> Option<(&str, &str)> {
        let a = alias.to_ascii_lowercase();
        Some((self.aliases.get(&a)?.as_str(), self.alias_owner.get(&a)?.as_str()))
    }

    /// Alias → canonical suffix over every loaded preset (input to `split_suffix_with`,
    /// `canonicalise_outputs` and `group_sets_with`).
    pub fn alias_table(&self) -> &BTreeMap<String, String> {
        &self.aliases
    }
```
Run: `cargo test -p engine --test presets` → still passes (built-in hashes and profiles unchanged).

- [ ] **Step 4: Implement the alias-aware split, grouping and output names (`group.rs`, `discover.rs`)**

Add to `group.rs` (after `split_suffix`, which stays as it is):
```rust
/// Result of `split_suffix_with`: `suffix` is canonical (an alias's target), `alias` the alias matched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuffixSplit {
    pub base: String,
    pub suffix: Option<String>,
    pub alias: Option<String>,
}

/// `split_suffix` with preset suffix aliases (spec §3.7, DECISIONS Note G). The stem segment after the
/// last '_' is looked up (lower-case) in `aliases` (alias → canonical) before the §6.1 list. Aliases
/// contain no '_' and are never a §6.1 suffix, so a match is exact and unambiguous.
pub fn split_suffix_with(stem: &str, aliases: &BTreeMap<String, String>) -> SuffixSplit {
    if let Some((base, tail)) = stem.rsplit_once('_') {
        let tail = tail.to_ascii_lowercase();
        if !base.is_empty() {
            if let Some(canonical) = aliases.get(&tail) {
                return SuffixSplit { base: base.to_string(), suffix: Some(canonical.clone()), alias: Some(tail) };
            }
        }
    }
    let (base, suffix) = split_suffix(stem);
    SuffixSplit { base, suffix, alias: None }
}
```
Replace `group_sets` with a delegating wrapper plus `group_sets_with` (same body, alias-aware split):
```rust
pub fn group_sets(files: &[InputFile]) -> (Vec<TextureSet>, Vec<Vec<usize>>) {
    group_sets_with(files, &BTreeMap::new())
}

/// `group_sets` with suffix aliases: an aliased member joins its set under the canonical suffix.
/// Call `discover::canonicalise_outputs` first so alias/canonical collisions are duplicate groups.
pub fn group_sets_with(files: &[InputFile], aliases: &BTreeMap<String, String>) -> (Vec<TextureSet>, Vec<Vec<usize>>) {
    let mut sets: BTreeMap<String, TextureSet> = BTreeMap::new();
    let mut outs: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, f) in files.iter().enumerate() {
        outs.entry(fold(&f.output)).or_default().push(i);
        let stem = f.rel.file_stem().unwrap_or_default().to_string_lossy().into_owned();
        let SuffixSplit { base, suffix, .. } = split_suffix_with(&stem, aliases);
        let dir = f.output.parent().map(fold).unwrap_or_default();
        let key = format!("{dir}/{}", base.to_lowercase());
        let set = sets.entry(key.clone()).or_insert_with(|| TextureSet { key, members: vec![], suffixes: BTreeSet::new() });
        set.members.push(i);
        if let Some(s) = suffix {
            set.suffixes.insert(s);
        }
    }
    let dups = outs.into_values().filter(|v| v.len() > 1).collect();
    (sets.into_values().collect(), dups)
}
```
Add to `discover.rs` (and `use std::collections::BTreeMap;`):
```rust
/// Spec §3.7: an input whose stem ends in a preset suffix alias is written under its canonical
/// suffix (`iron_rmaosultrahigh.png` → `iron_rmaos.dds`). Only `output` changes; `rel` and
/// `game_path` keep the source name, so rule globs match what is on disk. Run before grouping.
pub fn canonicalise_outputs(files: &mut [InputFile], aliases: &BTreeMap<String, String>) {
    if aliases.is_empty() {
        return;
    }
    for f in files {
        let stem = f.rel.file_stem().unwrap_or_default().to_string_lossy().into_owned();
        let split = crate::group::split_suffix_with(&stem, aliases);
        if let (Some(_), Some(canonical)) = (&split.alias, &split.suffix) {
            f.output.set_file_name(format!("{}_{canonical}.dds", split.base));
        }
    }
}
```
Run: `cargo test -p engine --test discover_group` → still passes.

- [ ] **Step 5: Use the alias preset in Resolve (`policy.rs`) and update the `FileFacts` literals**

In `FileFacts`, document `suffix` as canonical and add the alias:
```rust
pub struct FileFacts<'a> {
    pub input: &'a InputFile,
    /// Canonical suffix (an alias already mapped to its target, spec §3.7).
    pub suffix: Option<&'a str>,
    /// The suffix alias the file name matched (`group::split_suffix_with`), if any.
    pub alias: Option<&'a str>,
    pub set: &'a TextureSet,
    pub analysis: &'a Analysis,
    pub info: &'a SourceInfo,
}
```
Add:
```rust
/// A suffix alias in use for one file (spec §3.7, DECISIONS Note G).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, schemars::JsonSchema)]
pub struct AliasUse {
    pub alias: String,
    pub canonical: String,
    pub preset: String,
}

/// `(alias, canonical, preset id)` when the file matched an alias the registry knows.
fn alias_parts<'a>(ctx: &ResolveCtx<'a>, f: &FileFacts) -> Option<(String, &'a str, &'a str)> {
    let alias = f.alias?.to_ascii_lowercase();
    let (canonical, preset) = ctx.registry.alias_for(&alias)?;
    Some((alias, canonical, preset))
}

/// The preset whose chain resolves this file: the alias preset for an aliased file, else the batch preset.
pub fn preset_for<'a>(ctx: &ResolveCtx<'a>, f: &FileFacts) -> &'a str {
    alias_parts(ctx, f).map_or(ctx.preset, |(_, _, p)| p)
}

fn alias_use(ctx: &ResolveCtx, f: &FileFacts) -> Option<AliasUse> {
    alias_parts(ctx, f).map(|(alias, c, p)| AliasUse { alias, canonical: c.to_string(), preset: p.to_string() })
}
```
Give `rule_sources` the file's preset:
```rust
/// Rule sources in precedence order: the file's preset chain (base first; the alias preset's chain for an
/// aliased file), user config, project config.
fn rule_sources<'a>(ctx: &'a ResolveCtx, preset: &str) -> Vec<(String, &'a [Rule])> {
    let mut v: Vec<(String, &[Rule])> = vec![];
    if let Ok(chain) = ctx.registry.chain(preset) {
        for p in chain {
            v.push((p.id.clone(), &p.rules));
        }
    }
    if let Some(c) = ctx.user_config {
        v.push(("user".into(), &c.rules));
    }
    if let Some(c) = ctx.project_config {
        v.push(("project".into(), &c.rules));
    }
    v
}
```
In `decide_subtype`, make the suggestions mutable and apply the alias right after `suggest(…)`:
```rust
    let mut suggestions = suggest(&SuggestContext {
        target: ctx.target,
        suffix: f.suffix,
        game_path: &f.input.game_path,
        set_suffixes: &f.set.suffixes,
        alpha: f.analysis.alpha,
        is_float: f.info.is_float,
    });
    if let Some((alias, canonical, preset)) = alias_parts(ctx, f) {
        for s in &mut suggestions {
            s.reason = format!("suffix alias _{alias} → _{canonical} (preset {preset}); {}", s.reason);
        }
        // Spec §3.7: an alias declares the suffix; `_s` keeps its ranked readings (§4.3 item 2).
        if canonical != "s" {
            if let Some(top) = suggestions.first_mut() {
                top.confidence = Confidence::High;
            }
        }
    }
```
and change its rule loop to `for (src, rules) in rule_sources(ctx, preset_for(ctx, f)).into_iter().rev() {`.

In `layered_profile`, use the file's preset for both the preset layers and the rules:
```rust
    let preset = preset_for(ctx, f);
    for (source, patch) in ctx.registry.layers(preset, st)? {
        apply(&mut doc, &mut prov, &patch, source);
    }
```
```rust
    for (src, rules) in rule_sources(ctx, preset) {
```
Add to `ResolvedJob`, after `status`:
```rust
    /// The suffix alias this file matched (spec §3.7); its preset chain resolved the profile.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suffix_alias: Option<AliasUse>,
```
In `resolve_file`, compute `let suffix_alias = alias_use(ctx, f);` as the first line and add `suffix_alias,` to both `ResolvedJob { … }` literals (the `Err` arm returns, so the move is fine).

In `crates/engine/tests/policy_layers.rs` (three literals) and `crates/engine/tests/policy_checks.rs` (the literal in `run`), add `alias: None,` after `suffix: …,` in every `FileFacts { … }`.

- [ ] **Step 6: Run the tests**

Run: `cargo test -p engine --test suffix_aliases`
Expected: 5 passed.
Run: `cargo test -p engine` and `cargo clippy -p engine --all-targets -- -D warnings`
Expected: all pass (Tasks 20, 21, 25 and 26 tests unchanged apart from `alias: None`); no clippy warnings.

- [ ] **Step 7: Commit**

```bash
git add crates/engine
git commit -m "feat(engine): exact preset suffix aliases with canonical output names and alias preset layering (Note G)"
```

---

### Task 27: Push-pull RGB dilation under α = 0

**Files:**
- Create: `crates/engine/src/dilate.rs`
- Modify: `crates/engine/src/lib.rs`
- Test: unit tests in `dilate.rs`

**Interfaces:**
- Produces: `pub fn dilate_under_zero_alpha(img: &mut Rgba32F)` — replaces RGB of every texel whose alpha rounds to code 0 with colour pulled from the nearest coarser level of a push-pull pyramid built from texels with alpha > 0; alpha is never changed; an image with no visible texel is left unchanged.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fills_invisible_rgb_from_visible_neighbours() {
        let mut img = Rgba32F::new(4, 4, [0.0, 1.0, 0.0, 0.0]); // green garbage, invisible
        for y in 0..4 {
            for x in 0..2 {
                *img.at_mut(x, y) = [1.0, 0.0, 0.0, 1.0]; // left half visible red
            }
        }
        dilate_under_zero_alpha(&mut img);
        for p in &img.px {
            assert!(p[0] > 0.99 && p[1] < 0.01, "{p:?}");
        }
        assert_eq!(img.at(3, 3)[3], 0.0);
    }

    #[test]
    fn all_invisible_is_unchanged() {
        let mut img = Rgba32F::new(2, 2, [0.2, 0.3, 0.4, 0.0]);
        let before = img.clone();
        dilate_under_zero_alpha(&mut img);
        assert_eq!(img, before);
    }
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p engine dilate`
Expected: FAIL to compile.

- [ ] **Step 3: Implement**

```rust
//! Push-pull dilation (spec §5.4 step 4): colour under α = 0 is filled before any filtering [MIP].
use crate::image::Rgba32F;

struct Level {
    w: u32,
    h: u32,
    rgb: Vec<[f32; 3]>,
    wt: Vec<f32>,
}

pub fn dilate_under_zero_alpha(img: &mut Rgba32F) {
    let visible = |a: f32| (a * 255.0).round() >= 1.0;
    if !img.px.iter().any(|p| visible(p[3])) {
        return;
    }
    let mut levels = vec![Level {
        w: img.width,
        h: img.height,
        rgb: img.px.iter().map(|p| [p[0], p[1], p[2]]).collect(),
        wt: img.px.iter().map(|p| if visible(p[3]) { 1.0 } else { 0.0 }).collect(),
    }];
    // push: weighted 2x2 averages down to 1x1
    while levels.last().map(|l| l.w > 1 || l.h > 1).unwrap() {
        let p = levels.last().unwrap();
        let (w, h) = ((p.w / 2).max(1), (p.h / 2).max(1));
        let mut rgb = vec![[0.0; 3]; (w * h) as usize];
        let mut wt = vec![0.0; (w * h) as usize];
        for y in 0..h {
            for x in 0..w {
                let (mut s, mut sw) = ([0.0f32; 3], 0.0f32);
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let (sx, sy) = ((x * 2 + dx).min(p.w - 1), (y * 2 + dy).min(p.h - 1));
                    let i = (sy * p.w + sx) as usize;
                    for c in 0..3 {
                        s[c] += p.rgb[i][c] * p.wt[i];
                    }
                    sw += p.wt[i];
                }
                let o = (y * w + x) as usize;
                if sw > 0.0 {
                    rgb[o] = s.map(|v| v / sw);
                    wt[o] = 1.0;
                }
            }
        }
        levels.push(Level { w, h, rgb, wt });
    }
    // pull: fill empty texels from the parent, coarse to fine
    for l in (0..levels.len() - 1).rev() {
        let (lo, hi) = levels.split_at_mut(l + 1);
        let (cur, parent) = (&mut lo[l], &hi[0]);
        for y in 0..cur.h {
            for x in 0..cur.w {
                let i = (y * cur.w + x) as usize;
                if cur.wt[i] == 0.0 {
                    let pi = ((y / 2).min(parent.h - 1) * parent.w + (x / 2).min(parent.w - 1)) as usize;
                    cur.rgb[i] = parent.rgb[pi];
                    cur.wt[i] = 1.0;
                }
            }
        }
    }
    for (p, c) in img.px.iter_mut().zip(&levels[0].rgb) {
        if !visible(p[3]) {
            p[0] = c[0];
            p[1] = c[1];
            p[2] = c[2];
        }
    }
}
```
Add `pub mod dilate;` to `lib.rs`.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p engine dilate`
Expected: 2 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/engine
git commit -m "feat(engine): push-pull RGB dilation under zero alpha"
```

---

### Task 27a: Industry suffix vocabulary (DECISIONS Note H) — exporter terms as Skyrim roles, green convention, raw maps as packing inputs

Lands in Phase 1 now; channel packing itself is milestone M2.5. Tasks 18–27 (incl. 26a) are implemented, so the code below is written against the current `group.rs`, `discover.rs`, `suggest.rs`, `policy.rs`, `presets.rs`, `overrides.rs`, `codes.rs` and `config.rs`.

**Files:**
- Modify: `crates/engine/src/config.rs` (`GreenConvention`, `RawMap`, `ChannelSpec::green_convention`)
- Modify: `crates/engine/src/group.rs` (vocabulary table and split; `split_suffix_with` checks it after aliases)
- Modify: `crates/engine/src/discover.rs` (`canonicalise_outputs` writes Skyrim names for vocabulary files, also with no aliases)
- Modify: `crates/engine/src/suggest.rs` (`_d` high; raw packing inputs mark a `cs` set PBR; `apply_vocabulary`)
- Modify: `crates/engine/src/presets.rs` (`Provenance::Suffix`; an alias may not be a vocabulary term)
- Modify: `crates/engine/src/overrides.rs` (`channels.green_convention` is settable)
- Modify: `crates/engine/src/codes.rs` (`IN-PACKING-INPUT`; catalogue count 47 → 48)
- Modify: `crates/engine/src/policy.rs` (green-convention fact layer and flip, unresolved plain `_normal`, `JobStatus::PackingInput`)
- Test: `crates/engine/tests/suffix_vocabulary.rs`; unit tests in `suggest.rs` and `codes.rs`

**Interfaces:**
- Consumes: Tasks 19, 20, 21, 24, 25, 26, 26a.
- Produces:
  - `config::GreenConvention { Directx, Opengl }` (serde `directx` / `opengl`); `config::RawMap { Roughness, Metallic, Ao, Specular, Glossiness, Height, Opacity, Fuzz }` (serde lower-case) with `RawMap::ALL: [RawMap; 8]`, `RawMap::id(self) -> String`, `RawMap::starts_rmaos(self) -> bool`
  - `config::ChannelSpec` gains `pub green_convention: Option<GreenConvention>` (`#[serde(default, skip_serializing_if = "Option::is_none")]`, so the built-in presets and their SHA-256 values do not change)
  - `group::VocabRole { Colour, Normal(Option<GreenConvention>), Emissive, PackingInput(RawMap) }`; `group::Vocab { pub term: &'static str, pub role: VocabRole }` with `Vocab::canonical(&self) -> Option<String>`; `group::VOCABULARY: [Vocab; 31]` (a `static`, longest term first); `group::split_vocab(stem: &str) -> Option<(String, &'static Vocab)>`; `group::vocab_of(stem: &str) -> Option<&'static Vocab>`; `group::is_vocabulary(term: &str) -> bool`. `split_suffix`, `SUFFIXES` (19), `split_suffix_with`, `group_sets`, `group_sets_with` and `canonicalise_outputs` keep their signatures; `SuffixSplit` and `FileFacts` are unchanged (Resolve reads the term from `FileFacts.input.rel`).
  - `suggest::apply_vocabulary(suggestions: &mut [Suggestion], v: &Vocab)`
  - `presets::Provenance::Suffix(String)` (shown and parsed as `suffix:<term>`)
  - `codes::Code::InPackingInput` (`"IN-PACKING-INPUT"`, warning)
  - `policy::vocab_for(f: &FileFacts) -> Option<&'static Vocab>`; `policy::JobStatus::PackingInput { role: RawMap }` (serialises `{"state": "packing_input", "role": "roughness"}`)

Rules (spec §4.4, §3.2, §5.2, §6.4, DECISIONS Note H). Split order: preset alias (§3.7), then the vocabulary (whole trailing `_<term>`, case-insensitive, longest first, a base must precede), then the §6.1 list. Canonical suffix: colour terms → none (the colour member), normal terms → `n`, emissive terms → `g`, raw maps → their role id, which enters the set's suffix list. Output names: colour terms `<base>.dds`, normal `<base>_n.dds`, emissive `<base>_g.dds`; `_d` (a §6.1 suffix) keeps its stem; raw maps keep their name and are never written on their own. Suggestions: `_d` and colour terms give the colour member high confidence (not `hdr_sky`); alpha meaning is still never assumed; every reason of a vocabulary file starts `exporter suffix _<term> (<role>)`. Under `cs`, a raw roughness, glossiness, metallic or AO input marks the set PBR. Green convention: `_normalgl`/`_normaldx` apply `channels.green_convention` as a fact layer at the analysis position (provenance `suffix:<term>`; `--set` still wins); after layering, `opengl` sets `flip_green = true` with the same provenance (Task 28 flips it unchanged); a plain `_normal`/`_nrm` with no stated convention is `Unresolved` and neither `--accept-suggestions high` nor `--allow SUG-LOW-CONFIDENCE` settles it; `msn` and non-Normal sub-types ignore the field; Skyrim `_n` needs no statement. Raw maps: `JobStatus::PackingInput` with one `IN-PACKING-INPUT` warning and the `sdc-base` `generic_data` profile as an unused placeholder, unless a rule or `--set` names a sub-type for the file, which makes it an ordinary file. An alias may not equal a vocabulary term.

- [ ] **Step 1: Write the failing tests**

`crates/engine/tests/suffix_vocabulary.rs`:
```rust
use engine::analyse::analyse;
use engine::codes::{Code, Severity};
use engine::config::*;
use engine::discover::*;
use engine::group::*;
use engine::image::{Rgba32F, SourceInfo};
use engine::overrides::parse_set;
use engine::policy::*;
use engine::presets::*;
use engine::suggest::Confidence;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

const RGB8: SourceInfo = SourceInfo { bit_depth: 8, channels: 3, has_alpha: false, is_float: false };
const RGBA8: SourceInfo = SourceInfo { bit_depth: 8, channels: 4, has_alpha: true, is_float: false };

fn grey() -> Rgba32F {
    Rgba32F::new(64, 64, [0.5, 0.5, 0.5, 1.0])
}

fn flat_normal() -> Rgba32F {
    Rgba32F::new(64, 64, [0.5, 0.5, 1.0, 1.0])
}

fn touch(p: &Path) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, b"x").unwrap();
}

/// Resolves `name` under cs as a member of `textures/armor/iron`, whose other members have the suffixes `companions`.
fn resolve_img(name: &str, companions: &[&str], sets_cli: &[&str], allow: &[Code], img: &Rgba32F, info: &SourceInfo) -> ResolvedJob {
    let reg = PresetRegistry::builtin().unwrap();
    let overrides: Vec<_> = sets_cli.iter().map(|s| parse_set(s).unwrap()).collect();
    let allow: BTreeSet<Code> = allow.iter().copied().collect();
    let ctx = ResolveCtx { target: Target::Cs, registry: &reg, preset: "cs-quality", user_config: None, project_config: None, overrides: &overrides, accept: AcceptSuggestions::High, allow: &allow };
    let stem = Path::new(name).file_stem().unwrap().to_string_lossy().into_owned();
    let split = split_suffix_with(&stem, reg.alias_table());
    let f = InputFile {
        path: PathBuf::from(name),
        rel: PathBuf::from(name),
        game_path: format!("textures/armor/{}", name.to_ascii_lowercase()),
        output: PathBuf::from(name).with_extension("dds"),
        kind: InputKind::Image,
    };
    let mut suffixes: BTreeSet<String> = companions.iter().map(|s| s.to_string()).collect();
    suffixes.extend(split.suffix.clone());
    let set = TextureSet { key: "textures/armor/iron".into(), members: vec![0], suffixes };
    let a = analyse(img, info);
    resolve_file(&ctx, &FileFacts { input: &f, suffix: split.suffix.as_deref(), alias: split.alias.as_deref(), set: &set, analysis: &a, info })
}

#[test]
fn vocabulary_splits_to_skyrim_roles() {
    let none = BTreeMap::new();
    let s = |stem: &str| {
        let x = split_suffix_with(stem, &none);
        (x.base, x.suffix, x.alias)
    };
    for stem in ["iron_colormap", "iron_basecolor", "iron_Base_Color", "iron_albedo", "iron_diffuse", "iron_color", "iron_col"] {
        assert_eq!(s(stem), ("iron".into(), None, None), "{stem}");
        assert_eq!(vocab_of(stem).unwrap().role, VocabRole::Colour, "{stem}");
    }
    for stem in ["iron_normal", "iron_NRM", "iron_normalgl", "iron_normaldx"] {
        assert_eq!(s(stem), ("iron".into(), Some("n".into()), None), "{stem}");
    }
    assert_eq!(vocab_of("iron_normal").unwrap().role, VocabRole::Normal(None));
    assert_eq!(vocab_of("iron_NormalGL").unwrap().role, VocabRole::Normal(Some(GreenConvention::Opengl)));
    assert_eq!(vocab_of("iron_normaldx").unwrap().role, VocabRole::Normal(Some(GreenConvention::Directx)));
    for stem in ["iron_emissive", "iron_emission"] {
        assert_eq!(s(stem).1.as_deref(), Some("g"), "{stem}");
    }
    for (stem, role) in [
        ("iron_roughness", "roughness"), ("iron_rough", "roughness"),
        ("iron_metallic", "metallic"), ("iron_metalness", "metallic"), ("iron_metal", "metallic"),
        ("iron_ao", "ao"), ("iron_ambientocclusion", "ao"), ("iron_occlusion", "ao"),
        ("iron_specular", "specular"), ("iron_spec", "specular"),
        ("iron_glossiness", "glossiness"), ("iron_gloss", "glossiness"),
        ("iron_height", "height"), ("iron_displacement", "height"), ("iron_disp", "height"),
        ("iron_opacity", "opacity"), ("iron_fuzz", "fuzz"), ("iron_sheen", "fuzz"),
    ] {
        assert_eq!(s(stem), ("iron".into(), Some(role.into()), None), "{stem}");
        assert!(matches!(vocab_of(stem).unwrap().role, VocabRole::PackingInput(r) if r.id() == role), "{stem}");
    }
    // Skyrim suffixes keep their meaning next to the exporter terms
    for (skyrim, exporter, role) in [
        ("iron_s", "iron_spec", "specular"),
        ("iron_p", "iron_height", "height"),
        ("iron_m", "iron_metal", "metallic"),
        ("iron_g", "iron_gloss", "glossiness"),
        ("iron_f", "iron_fuzz", "fuzz"),
    ] {
        assert_eq!(s(skyrim).1, Some(skyrim[5..].to_string()), "{skyrim}");
        assert!(vocab_of(skyrim).is_none(), "{skyrim}");
        assert_eq!(s(exporter).1.as_deref(), Some(role), "{exporter}");
    }
    // `_d` stays a §6.1 suffix; a stem that is only a term is left whole; split_suffix and SUFFIXES are unchanged
    assert_eq!(s("iron_d"), ("iron".into(), Some("d".into()), None));
    assert_eq!(s("_normal"), ("_normal".into(), None, None));
    assert_eq!(split_suffix("iron_basecolor"), ("iron_basecolor".into(), None));
    assert_eq!(SUFFIXES.len(), 19);
    // longest first, so `_base_color` is one term
    assert_eq!(VOCABULARY.len(), 31);
    assert!(VOCABULARY.windows(2).all(|w| w[0].term.len() >= w[1].term.len()));
    assert_eq!(vocab_of("iron_base_color").unwrap().term, "base_color");
}

#[test]
fn outputs_use_skyrim_names_and_collisions_are_duplicates() {
    let d = tempfile::tempdir().unwrap();
    let src = d.path().join("in");
    for f in [
        "iron_basecolor.png", "iron_normalgl.png", "iron_emissive.png", "iron_roughness.png", "iron_AO.png",
        "wood_d.png", "rock.png", "rock_albedo.tga", "rock_n.png", "rock_normal.png",
    ] {
        touch(&src.join(f));
    }
    let out = d.path().join("out");
    let mut files = discover(&[src], &out, false).unwrap();
    // no preset aliases loaded: the vocabulary still applies
    canonicalise_outputs(&mut files, &BTreeMap::new());
    let output_of = |name: &str| files.iter().find(|f| f.rel.as_path() == Path::new(name)).unwrap().output.clone();
    assert_eq!(output_of("iron_basecolor.png"), out.join("iron.dds"));
    assert_eq!(output_of("iron_normalgl.png"), out.join("iron_n.dds"));
    assert_eq!(output_of("iron_emissive.png"), out.join("iron_g.dds"));
    // raw maps keep their name (never written on their own); `_d` keeps its stem
    assert_eq!(output_of("iron_roughness.png"), out.join("iron_roughness.dds"));
    assert_eq!(output_of("wood_d.png"), out.join("wood_d.dds"));
    let (sets, dups) = group_sets(&files);
    let iron = sets.iter().find(|s| s.key.ends_with("/iron")).unwrap();
    assert_eq!(iron.members.len(), 5);
    assert_eq!(iron.suffixes, BTreeSet::from(["n", "g", "roughness", "ao"].map(String::from)));
    // rock.png + rock_albedo.tga → rock.dds; rock_n.png + rock_normal.png → rock_n.dds
    let mut groups: Vec<BTreeSet<String>> = dups.iter().map(|g| g.iter().map(|&i| files[i].rel.to_string_lossy().into_owned()).collect()).collect();
    groups.sort();
    assert_eq!(
        groups,
        vec![
            BTreeSet::from(["rock.png".to_string(), "rock_albedo.tga".to_string()]),
            BTreeSet::from(["rock_n.png".to_string(), "rock_normal.png".to_string()]),
        ]
    );
}

#[test]
fn colour_terms_and_d_are_high_confidence_but_alpha_meaning_is_never_assumed() {
    let j = resolve_img("iron_colormap.png", &[], &[], &[], &grey(), &RGB8);
    assert_eq!(j.status, JobStatus::Ready, "{:?}", j.warnings);
    assert_eq!((j.profile.subtype, j.suggestions[0].confidence), (Subtype::Diffuse, Confidence::High));
    assert!(j.suggestions[0].reason.starts_with("exporter suffix _colormap (colour); "), "{}", j.suggestions[0].reason);
    let j = resolve_img("iron_d.png", &[], &[], &[], &grey(), &RGB8);
    assert_eq!((j.profile.subtype, j.suggestions[0].confidence, j.status.clone()), (Subtype::Diffuse, Confidence::High, JobStatus::Ready));
    // a bare stem stays medium
    let j = resolve_img("iron.png", &[], &[], &[], &grey(), &RGB8);
    assert_eq!(j.suggestions[0].confidence, Confidence::Medium);
    assert!(matches!(j.status, JobStatus::Unresolved { .. }), "{:?}", j.status);
    // the set still picks the sub-type: a raw roughness companion makes the cs set PBR
    let j = resolve_img("iron_basecolor.png", &["roughness"], &[], &[], &grey(), &RGB8);
    assert_eq!((j.profile.subtype, j.suggestions[0].confidence), (Subtype::PbrAlbedo, Confidence::High));
    // varying alpha: the colour role is certain, the alpha meaning is not
    let mut img = grey();
    for y in 0..64 {
        for x in 0..64 {
            img.at_mut(x, y)[3] = x as f32 / 63.0;
        }
    }
    let j = resolve_img("iron_albedo.png", &[], &[], &[], &img, &RGBA8);
    assert_eq!(j.suggestions[0].confidence, Confidence::High);
    match &j.status {
        JobStatus::Unresolved { reasons } => assert!(reasons.iter().any(|r| r.contains("alpha meaning is unconfirmed")), "{reasons:?}"),
        s => panic!("{s:?}"),
    }
}

#[test]
fn plain_normal_stays_unresolved_until_the_green_convention_is_stated() {
    let green_reason = |j: &ResolvedJob| matches!(&j.status, JobStatus::Unresolved { reasons } if reasons.iter().any(|r| r.contains("green convention")));
    for name in ["iron_normal.png", "iron_nrm.png"] {
        let j = resolve_img(name, &[], &[], &[], &flat_normal(), &RGB8);
        assert_eq!((j.profile.subtype, j.suggestions[0].confidence), (Subtype::Normal, Confidence::High));
        assert_eq!(j.profile.channels.green_convention, None);
        assert!(green_reason(&j), "{name}: {:?}", j.status);
        // not a suggestion: --allow SUG-LOW-CONFIDENCE does not settle it
        let j = resolve_img(name, &[], &[], &[Code::SugLowConfidence], &flat_normal(), &RGB8);
        assert!(green_reason(&j), "{name}: {:?}", j.status);
    }
    let j = resolve_img("iron_normal.png", &[], &["channels.green_convention=directx"], &[], &flat_normal(), &RGB8);
    assert_eq!(j.status, JobStatus::Ready, "{:?}", j.warnings);
    assert_eq!((j.profile.channels.green_convention, j.profile.channels.flip_green), (Some(GreenConvention::Directx), false));
    assert_eq!(j.provenance["channels.green_convention"], Provenance::User);
    let j = resolve_img("iron_normal.png", &[], &["file:iron_normal.png.channels.green_convention=opengl"], &[], &flat_normal(), &RGB8);
    assert_eq!(j.status, JobStatus::Ready, "{:?}", j.warnings);
    assert!(j.profile.channels.flip_green);
    assert_eq!(j.provenance["channels.flip_green"], Provenance::User);
    // Skyrim's own _n is DirectX by definition
    let j = resolve_img("iron_n.png", &[], &[], &[], &flat_normal(), &RGB8);
    assert_eq!(j.status, JobStatus::Ready, "{:?}", j.warnings);
    assert!(!j.profile.channels.flip_green);
}

#[test]
fn normalgl_flips_to_directx_and_normaldx_does_not() {
    let j = resolve_img("iron_normalgl.png", &[], &[], &[], &flat_normal(), &RGB8);
    assert_eq!(j.status, JobStatus::Ready, "{:?}", j.warnings);
    assert_eq!((j.profile.channels.green_convention, j.profile.channels.flip_green), (Some(GreenConvention::Opengl), true));
    assert_eq!(j.provenance["channels.green_convention"], Provenance::Suffix("normalgl".into()));
    assert_eq!(j.provenance["channels.flip_green"].to_string(), "suffix:normalgl");
    assert_eq!("suffix:normalgl".parse::<Provenance>().unwrap(), Provenance::Suffix("normalgl".into()));
    let j = resolve_img("iron_normaldx.png", &[], &[], &[], &flat_normal(), &RGB8);
    assert_eq!(j.status, JobStatus::Ready, "{:?}", j.warnings);
    assert_eq!((j.profile.channels.green_convention, j.profile.channels.flip_green), (Some(GreenConvention::Directx), false));
    // --set beats the suffix
    let j = resolve_img("iron_normalgl.png", &[], &["channels.green_convention=directx"], &[], &flat_normal(), &RGB8);
    assert_eq!((j.profile.channels.flip_green, j.provenance["channels.green_convention"].clone()), (false, Provenance::User));
    // msn ignores the convention
    let j = resolve_img("iron_normalgl.png", &[], &["subtype=msn"], &[], &flat_normal(), &RGB8);
    assert_eq!((j.profile.subtype, j.profile.channels.green_convention, j.profile.channels.flip_green), (Subtype::Msn, None, false));
}

#[test]
fn raw_maps_are_packing_inputs_not_errors() {
    for (name, role) in [
        ("iron_roughness.png", RawMap::Roughness),
        ("iron_spec.png", RawMap::Specular),
        ("iron_height.png", RawMap::Height),
        ("iron_metal.png", RawMap::Metallic),
        ("iron_opacity.png", RawMap::Opacity),
    ] {
        let j = resolve_img(name, &[], &[], &[], &grey(), &RGB8);
        assert_eq!(j.status, JobStatus::PackingInput { role }, "{name}");
        assert_eq!(j.warnings.len(), 1, "{name}");
        assert_eq!((j.warnings[0].code, j.warnings[0].severity), (Code::InPackingInput, Severity::Warning));
        assert!(j.suggestions.is_empty());
    }
    let j = resolve_img("iron_roughness.png", &[], &[], &[], &grey(), &RGB8);
    assert_eq!(serde_json::to_value(&j.status).unwrap(), serde_json::json!({ "state": "packing_input", "role": "roughness" }));
    // the Skyrim suffixes next to them resolve as before
    assert_eq!(resolve_img("iron_p.png", &[], &[], &[], &grey(), &RGB8).profile.subtype, Subtype::Height);
    assert_eq!(resolve_img("iron_m.png", &[], &[], &[], &grey(), &RGB8).profile.subtype, Subtype::Envmask);
    assert_eq!(resolve_img("iron_s.png", &[], &[], &[], &grey(), &RGB8).profile.subtype, Subtype::SubsurfaceTint);
    // a sub-type named by --set makes a raw map an ordinary file
    let j = resolve_img("iron_height.png", &[], &["file:iron_height.png.subtype=height"], &[], &grey(), &RGB8);
    assert_eq!(j.status, JobStatus::Ready, "{:?}", j.warnings);
    assert_eq!(j.profile.subtype, Subtype::Height);
}

#[test]
fn aliases_may_not_shadow_the_vocabulary() {
    let mut r = PresetRegistry::builtin().unwrap();
    for bad in ["normal = \"n\"", "nrm = \"n\"", "roughness = \"rmaos\"", "basecolor = \"d\"", "spec = \"s\""] {
        let toml = format!("schema_version = 1\nid = \"bad\"\nversion = \"1.0.0\"\nbase = \"cs-quality\"\n[suffix_aliases]\n{bad}\n");
        let e = r.add_toml(&toml);
        assert!(matches!(e, Err(PresetError::Alias(_))), "{bad}");
        assert!(e.unwrap_err().to_string().contains("exporter vocabulary"), "{bad}");
        assert!(r.get("bad").is_none(), "{bad}");
    }
    assert!(is_vocabulary("normalgl") && !is_vocabulary("n") && !is_vocabulary("rmaosultrahigh"));
}
```

Add to the `tests` module of `suggest.rs`:
```rust
    #[test]
    fn d_is_high_and_raw_packing_inputs_mark_a_cs_set_pbr() {
        let none = BTreeSet::new();
        let d = suggest(&ctx(Target::Cs, Some("d"), "textures/a/x_d.png", &none, AlphaContent::None))[0].clone();
        assert_eq!((d.subtype, d.confidence), (Subtype::Diffuse, Confidence::High));
        let sky = suggest(&ctx(Target::Cs, Some("d"), "textures/sky/x_d.png", &none, AlphaContent::None))[0].clone();
        assert_eq!((sky.subtype, sky.confidence), (Subtype::UiSky, Confidence::High));
        for (raw, pbr) in [
            ("roughness", true), ("glossiness", true), ("metallic", true), ("ao", true),
            ("specular", false), ("fuzz", false), ("height", false), ("opacity", false),
        ] {
            let set = set_of(&[raw]);
            assert_eq!(is_pbr_set(&ctx(Target::Cs, None, "textures/a/x.png", &set, AlphaContent::None)), pbr, "{raw}");
            assert!(!is_pbr_set(&ctx(Target::Cpm, None, "textures/a/x.png", &set, AlphaContent::None)), "{raw}");
        }
        // a raw packing role gets no sub-type suggestion
        assert!(suggest(&ctx(Target::Cs, Some("roughness"), "textures/a/x_roughness.png", &none, AlphaContent::None)).is_empty());
    }

    #[test]
    fn colour_vocabulary_raises_the_colour_member_only() {
        let none = BTreeSet::new();
        let mut sg = suggest(&ctx(Target::Cs, None, "textures/a/x_basecolor.png", &none, AlphaContent::None));
        apply_vocabulary(&mut sg, crate::group::vocab_of("x_basecolor").unwrap());
        assert_eq!((sg[0].subtype, sg[0].confidence), (Subtype::Diffuse, Confidence::High));
        assert!(sg[0].reason.starts_with("exporter suffix _basecolor (colour); "), "{}", sg[0].reason);
        // a float source under sky/ may be an HDRI: stays medium
        let mut f = ctx(Target::Cs, None, "textures/sky/x_albedo.exr", &none, AlphaContent::None);
        f.is_float = true;
        let mut sg = suggest(&f);
        apply_vocabulary(&mut sg, crate::group::vocab_of("x_albedo").unwrap());
        assert_eq!((sg[0].subtype, sg[0].confidence), (Subtype::HdrSky, Confidence::Medium));
        // normal terms only prefix the reason
        let mut sg = suggest(&ctx(Target::Cs, Some("n"), "textures/a/x_normalgl.png", &none, AlphaContent::None));
        apply_vocabulary(&mut sg, crate::group::vocab_of("x_normalgl").unwrap());
        assert_eq!((sg[0].subtype, sg[0].confidence), (Subtype::Normal, Confidence::High));
        assert!(sg[0].reason.starts_with("exporter suffix _normalgl (normal); "));
    }
```

In `codes.rs` tests, change `assert_eq!(Code::ALL.len(), 47);` to `assert_eq!(Code::ALL.len(), 48);` and add to `severities_follow_spec_section_10`:
```rust
        assert_eq!(Code::InPackingInput.severity(), Severity::Warning);
        assert_eq!(Code::parse("IN-PACKING-INPUT"), Some(Code::InPackingInput));
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p engine --test suffix_vocabulary`
Expected: FAIL to compile (`vocab_of`, `VocabRole`, `GreenConvention`, `RawMap`, `JobStatus::PackingInput`, `Provenance::Suffix` … not found).

- [ ] **Step 3: Configuration types (`config.rs`), settable field (`overrides.rs`), code (`codes.rs`)**

In `config.rs`, after the `SearchRadius` line:
```rust
cfg_enum!(GreenConvention, "lowercase" { Directx, Opengl });
cfg_enum!(RawMap, "lowercase" { Roughness, Metallic, Ao, Specular, Glossiness, Height, Opacity, Fuzz });

impl RawMap {
    pub const ALL: [RawMap; 8] = {
        use RawMap::*;
        [Roughness, Metallic, Ao, Specular, Glossiness, Height, Opacity, Fuzz]
    };

    /// The role id a raw map counts as after Discover (spec §4.4): `roughness`, `ao`, …
    pub fn id(self) -> String {
        serde_json::to_value(self).unwrap().as_str().unwrap().to_owned()
    }

    /// Spec §4.3 item 1, §6.4: an input that starts the cs `_rmaos`/`_rfaos` recipe, so its set is PBR.
    pub fn starts_rmaos(self) -> bool {
        matches!(self, RawMap::Roughness | RawMap::Glossiness | RawMap::Metallic | RawMap::Ao)
    }
}
```
In `ChannelSpec`, after `grayscale_source`:
```rust
    /// Green (Y) convention of a normal source (spec §3.2, §4.4, DECISIONS Note H); None = unstated.
    /// `opengl` makes Resolve set `flip_green`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub green_convention: Option<GreenConvention>,
```
In `overrides.rs` `FIELD_PATHS`, after `"channels.flip_green",` add `"channels.green_convention",`.

In `codes.rs`, after `InDdsSkipped`:
```rust
    InPackingInput = "IN-PACKING-INPUT" => Warning,
```

- [ ] **Step 4: Vocabulary and split (`group.rs`), output names (`discover.rs`)**

In `group.rs`, add `use crate::config::{GreenConvention, RawMap};` and, after `split_suffix`:
```rust
/// What an exporter suffix stands for (spec §4.4, DECISIONS Note H).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VocabRole {
    /// The colour member (no suffix): written `<base>.dds`, high confidence for the colour role.
    Colour,
    /// `_n`; `Some` when the term states the green convention (`_normalgl`, `_normaldx`).
    Normal(Option<GreenConvention>),
    /// `_g` (`glow` or `pbr_emissive`).
    Emissive,
    /// A raw single-channel map for channel packing (spec §6.4); never written on its own.
    PackingInput(RawMap),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Vocab {
    /// Lower-case, without the leading '_'.
    pub term: &'static str,
    pub role: VocabRole,
}

impl Vocab {
    /// The canonical suffix the file counts as after Discover: none for colour, `n`, `g`, or the packing role id.
    pub fn canonical(&self) -> Option<String> {
        match self.role {
            VocabRole::Colour => None,
            VocabRole::Normal(_) => Some("n".into()),
            VocabRole::Emissive => Some("g".into()),
            VocabRole::PackingInput(r) => Some(r.id()),
        }
    }
}

const fn term(term: &'static str, role: VocabRole) -> Vocab {
    Vocab { term, role }
}

/// Spec §4.4 exporter vocabulary, longest term first so `_base_color` wins over `_color`. None of these is a
/// §6.1 suffix, and none ends in `_<§6.1 suffix>`, so a stem has at most one reading.
pub static VOCABULARY: [Vocab; 31] = {
    use GreenConvention::*;
    use RawMap::*;
    use VocabRole::*;
    [
        term("ambientocclusion", PackingInput(Ao)),
        term("displacement", PackingInput(Height)),
        term("base_color", Colour),
        term("glossiness", PackingInput(Glossiness)),
        term("roughness", PackingInput(Roughness)),
        term("metalness", PackingInput(Metallic)),
        term("basecolor", Colour),
        term("occlusion", PackingInput(Ao)),
        term("normalgl", Normal(Some(Opengl))),
        term("normaldx", Normal(Some(Directx))),
        term("colormap", Colour),
        term("metallic", PackingInput(Metallic)),
        term("specular", PackingInput(Specular)),
        term("emissive", Emissive),
        term("emission", Emissive),
        term("diffuse", Colour),
        term("opacity", PackingInput(Opacity)),
        term("albedo", Colour),
        term("normal", Normal(None)),
        term("height", PackingInput(Height)),
        term("color", Colour),
        term("rough", PackingInput(Roughness)),
        term("metal", PackingInput(Metallic)),
        term("gloss", PackingInput(Glossiness)),
        term("sheen", PackingInput(Fuzz)),
        term("spec", PackingInput(Specular)),
        term("disp", PackingInput(Height)),
        term("fuzz", PackingInput(Fuzz)),
        term("nrm", Normal(None)),
        term("col", Colour),
        term("ao", PackingInput(Ao)),
    ]
};

/// The vocabulary term a stem ends in (case-insensitive) and the base before it. A stem that is only a term
/// (`_normal`) is left alone, as with §6.1 suffixes.
pub fn split_vocab(stem: &str) -> Option<(String, &'static Vocab)> {
    let lower = stem.to_ascii_lowercase();
    VOCABULARY.iter().find_map(|v| {
        let tail = format!("_{}", v.term);
        // The tail is ASCII, so the cut falls on a char boundary even in non-ASCII stems.
        (lower.ends_with(&tail) && lower.len() > tail.len()).then(|| (stem[..stem.len() - tail.len()].to_string(), v))
    })
}

pub fn vocab_of(stem: &str) -> Option<&'static Vocab> {
    split_vocab(stem).map(|(_, v)| v)
}

/// Whether `term` (lower-case, no '_') is a vocabulary term; preset aliases may not be one (spec §3.7).
pub fn is_vocabulary(term: &str) -> bool {
    VOCABULARY.iter().any(|v| v.term == term)
}
```
In `split_suffix_with`, between the alias block and the `split_suffix` fallback, add:
```rust
    // Spec §4.4: exporter vocabulary after aliases, before the §6.1 list.
    if let Some((base, v)) = split_vocab(stem) {
        return SuffixSplit {
            base,
            suffix: v.canonical(),
            alias: None,
        };
    }
```
and extend its doc comment: "…before the vocabulary (§4.4) and the §6.1 list. Aliases contain no '_' and are never a §6.1 suffix or a vocabulary term, so a match is exact and unambiguous."

In `discover.rs`, replace `canonicalise_outputs` (signature unchanged; the early return for an empty alias table goes, because the vocabulary applies without aliases):
```rust
/// Spec §3.7, §4.4: an input whose stem ends in a preset suffix alias or an exporter vocabulary term is
/// written under its Skyrim name: `iron_rmaosultrahigh.png` → `iron_rmaos.dds`, `iron_normalgl.png` →
/// `iron_n.dds`, `iron_basecolor.png` → `iron.dds`. Raw packing inputs keep their name (they are never
/// written on their own). Only `output` changes; `rel` and `game_path` keep the source name, so rule globs
/// match what is on disk. Run before grouping.
pub fn canonicalise_outputs(files: &mut [InputFile], aliases: &BTreeMap<String, String>) {
    use crate::group::{split_suffix_with, vocab_of, VocabRole};
    for f in files {
        let stem = f.rel.file_stem().unwrap_or_default().to_string_lossy().into_owned();
        let split = split_suffix_with(&stem, aliases);
        let suffixed = || split.suffix.as_ref().map(|c| format!("{}_{c}.dds", split.base));
        let name = match (&split.alias, vocab_of(&stem)) {
            (Some(_), _) => suffixed(),
            (None, Some(v)) => match v.role {
                VocabRole::PackingInput(_) => None,
                VocabRole::Colour => Some(format!("{}.dds", split.base)),
                VocabRole::Normal(_) | VocabRole::Emissive => suffixed(),
            },
            (None, None) => None,
        };
        if let Some(n) = name {
            f.output.set_file_name(n);
        }
    }
}
```
Run: `cargo test -p engine --test discover_group --test suffix_aliases` → still pass.

- [ ] **Step 5: Suggestions (`suggest.rs`) and alias validation and provenance (`presets.rs`)**

In `suggest.rs`, change the imports to `use crate::config::{AlphaPolicy, RawMap, Subtype, Target};` plus `use crate::group::{Vocab, VocabRole};`. Replace `is_pbr_set`:
```rust
/// Spec §4.3 rule 1: under cs, a set is PBR if its path has a `pbr` component, it has _rmaos/_rfaos/_cnr/_f, or it
/// has a raw packing input that starts the _rmaos/_rfaos recipe (§4.4, §6.4).
pub fn is_pbr_set(c: &SuggestContext) -> bool {
    c.target == Target::Cs
        && (has_component(c.game_path, "pbr")
            || ["rmaos", "rfaos", "cnr", "f"].iter().any(|s| c.set_suffixes.contains(*s))
            || RawMap::ALL.iter().any(|r| r.starts_rmaos() && c.set_suffixes.contains(&r.id())))
}
```
In `suggest`, add `let colour = if c.suffix == Some("d") { High } else { Medium };` after `ui_or_sky`, and use it in three colour arms (the landscape arm is already high; `hdr_sky` stays medium):
```rust
        Some("d") | None if ui_or_sky => vec![s(UiSky, colour, "colour under interface/ or sky/")],
        Some("d") | None if pbr => vec![s(PbrAlbedo, colour, "colour member of a PBR set")],
        Some("d") | None => vec![s(Diffuse, colour, "colour member, non-PBR set")],
```
Add after `suggest`:
```rust
/// Spec §4.3 item 7, §4.4 (DECISIONS Note H): an exporter vocabulary term declares the file's role. Every reason
/// is prefixed `exporter suffix _<term> (<role>)`; a colour term raises a colour-member top suggestion to high
/// confidence (not `hdr_sky`: a float source under sky/ may be an HDRI). Alpha meaning is untouched.
pub fn apply_vocabulary(suggestions: &mut [Suggestion], v: &Vocab) {
    let role = match v.role {
        VocabRole::Colour => "colour",
        VocabRole::Normal(_) => "normal",
        VocabRole::Emissive => "emissive",
        VocabRole::PackingInput(_) => "packing input",
    };
    for s in suggestions.iter_mut() {
        s.reason = format!("exporter suffix _{} ({role}); {}", v.term, s.reason);
    }
    if v.role == VocabRole::Colour {
        if let Some(top) = suggestions.first_mut().filter(|t| {
            matches!(t.subtype, Subtype::Diffuse | Subtype::PbrAlbedo | Subtype::LandscapeDiffuse | Subtype::UiSky)
        }) {
            top.confidence = Confidence::High;
        }
    }
}
```
In `presets.rs`, add the variant `Suffix(String)` to `Provenance` (after `Fallback`, doc: "A fact stated by an exporter vocabulary term, e.g. `_normalgl` (spec §4.4)"), the `Display` arm `Provenance::Suffix(t) => write!(f, "suffix:{t}"),` and, in `FromStr`, before the `match`:
```rust
        if let Some(t) = s.strip_prefix("suffix:") {
            return Ok(Provenance::Suffix(t.to_string()));
        }
```
In `check_aliases`, after the `SUFFIXES.contains(&alias.as_str())` check:
```rust
            if crate::group::is_vocabulary(alias) {
                return Err(bad(format!(
                    "suffix alias '{alias}' is an exporter vocabulary term (spec §4.4)"
                )));
            }
```
Run: `cargo test -p engine suggest` → the new unit tests pass; `suffix_rules`, `diffuse_is_never_high_confidence` (bare stems only) and the others are unchanged.

- [ ] **Step 6: Resolve (`policy.rs`)**

Change the group import to `use crate::group::{vocab_of, TextureSet, Vocab, VocabRole};`. Add:
```rust
/// The exporter vocabulary term the file name ends in (spec §4.4), if any. Aliases are matched first and never
/// equal a term, so an aliased file has none.
pub fn vocab_for(f: &FileFacts) -> Option<&'static Vocab> {
    if f.alias.is_some() {
        return None;
    }
    vocab_of(&f.input.rel.file_stem().unwrap_or_default().to_string_lossy())
}

/// Sub-types that read `channels.green_convention` (spec §3.2): every Normal sub-type except `msn`.
fn takes_green(st: Subtype) -> bool {
    st.category() == Category::Normal && st != Subtype::Msn
}
```
In `decide_subtype`, right after the alias block (before the `--set subtype` lookup):
```rust
    if let Some(v) = vocab_for(f) {
        apply_vocabulary(&mut suggestions, v);
    }
```
In `layered_profile`, between the analysis-derived alpha block and the `--set` loop:
```rust
    // Spec §4.4: `_normalgl`/`_normaldx` state the green convention, a fact at the analysis position (--set wins).
    if let Some(Vocab { term, role: VocabRole::Normal(Some(g)) }) = vocab_for(f) {
        if takes_green(st) {
            let value = serde_json::to_value(g).expect("GreenConvention serialises");
            apply(
                &mut doc,
                &mut prov,
                &mut sources,
                &to_patch("channels.green_convention", &value),
                Provenance::Suffix(term.to_string()),
                GENERIC,
            );
        }
    }
```
and after `backend_follows_format(&mut profile, &mut prov, &sources);`:
```rust
    // Spec §3.2, §5.4: a stated OpenGL convention is flipped to DirectX in Preprocess (Task 28 reads flip_green).
    if takes_green(st) && profile.channels.green_convention == Some(GreenConvention::Opengl) {
        profile.channels.flip_green = true;
        if let Some(src) = prov.get("channels.green_convention").cloned() {
            prov.insert("channels.flip_green".into(), src);
        }
    }
```
Add the status variant to `JobStatus` (after `Unresolved`):
```rust
    /// A raw single-channel map (spec §4.4): a channel-packing input (§6.4, milestone M2.5), not converted on
    /// its own and not an error.
    PackingInput {
        role: RawMap,
    },
```
Add:
```rust
/// Spec §4.4: the job of a raw packing input. Nothing is written; the profile is the built-in `generic_data`
/// placeholder and is never used. Milestone M2.5 consumes these files in packing recipes.
fn packing_input_job(ctx: &ResolveCtx, f: &FileFacts, term: &str, role: RawMap) -> ResolvedJob {
    let profile = ctx
        .registry
        .profile(BASE_ID, ctx.target, Subtype::GenericData)
        .expect("sdc-base profile is valid");
    let msg = format!(
        "_{term} is a raw {} map: a channel-packing input (spec §6.4), not converted on its own; set a sub-type for it to convert it alone",
        role.id()
    );
    ResolvedJob {
        input: f.input.path.clone(),
        output: f.input.output.clone(),
        game_path: f.input.game_path.clone(),
        set_key: f.set.key.clone(),
        profile,
        provenance: BTreeMap::new(),
        suggestions: vec![],
        warnings: vec![Warning::new(Code::InPackingInput, msg).with_data(json!({ "role": role, "term": term }))],
        status: JobStatus::PackingInput { role },
        suffix_alias: None,
    }
}
```
In `resolve_file`, right after `let d = decide_subtype(ctx, f);`:
```rust
    // Spec §4.4: a raw map is a packing input unless a rule or --set names its sub-type.
    if let Some(Vocab { term, role: VocabRole::PackingInput(role) }) = vocab_for(f) {
        if !matches!(d.provenance, Provenance::User | Provenance::Rule(_)) {
            return packing_input_job(ctx, f, term, *role);
        }
    }
```
After the "alpha meaning is unconfirmed" block:
```rust
    // Spec §4.4 (DECISIONS Note H): a plain `_normal`/`_nrm` has no default green convention. It is not a suggestion,
    // so neither --accept-suggestions high nor --allow SUG-LOW-CONFIDENCE settles it.
    let mut green_unstated = false;
    if let Some(Vocab { term, role: VocabRole::Normal(None) }) = vocab_for(f) {
        if takes_green(profile.subtype) && profile.channels.green_convention.is_none() {
            green_unstated = true;
            reasons.push(format!(
                "green convention of _{term} is unstated: set channels.green_convention=opengl or directx (preset, config, rule or --set)"
            ));
        }
    }
```
and change the status condition to:
```rust
    } else if (!reasons.is_empty() && !ctx.allow.contains(&Code::SugLowConfidence)) || green_unstated {
        JobStatus::Unresolved { reasons }
```

- [ ] **Step 7: Run the tests**

Run: `cargo test -p engine --test suffix_vocabulary`
Expected: 7 passed.
Run: `cargo test -p engine` and `cargo clippy -p engine --all-targets -- -D warnings`
Expected: all pass (codes catalogue 48; Tasks 21, 24–26a tests unchanged); no clippy warnings.

- [ ] **Step 8: Commit**

```bash
git add crates/engine
git commit -m "feat(engine): industry suffix vocabulary, green convention and raw maps as packing inputs (Note H)"
```

---

### Task 28: Preprocess (spec §5.4)

**Files:**
- Create: `crates/engine/src/ops.rs`
- Modify: `crates/engine/src/lib.rs`
- Test: unit tests in `ops.rs`

**Interfaces:**
- Consumes: `Profile`, `Rgba32F`, `dilate_under_zero_alpha` (Task 27).
- Produces:
  - `pub enum Domain { LinearLight, Stored, Normal, Msn, SignedRg, Hdr }`
  - `pub struct Work { pub img: Rgba32F, pub alpha_present: bool, pub domain: Domain, pub roughness_r_squared: bool }`
  - `pub fn domain_for(p: &Profile) -> Domain`
  - `pub fn srgb_to_linear(v: f32) -> f32`; `pub fn linear_to_srgb(v: f32) -> f32` (exact piecewise sRGB curve, not a 2.2 power)
  - `pub fn preprocess(img: Rgba32F, p: &Profile) -> Work`

Order (spec §5.4): (1) `remove` drops alpha (A := 1, `alpha_present = false`); (2) swizzle, then Grayscale takes R (or luma) into R=G=B; (3) normals (never `msn`): `*2 − 1`, optional green flip (`channels.flip_green`; from Task 27a Resolve also sets it for `channels.green_convention = opengl`), optional Z rebuild `z = √(1 − x² − y²)`, clamp z ≥ 0; `msn` and `flow` decode `*2 − 1` only; (4) dilation for cutout/blend/cutout_blend; (5) `srgb` content → linear light (RGB only). Resize (step 6) is milestone M6; NPOT is blocked by `DIM-NPOT` before this stage.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::*;
    use crate::presets::PresetRegistry;

    fn profile(st: Subtype) -> Profile {
        PresetRegistry::builtin().unwrap().profile("cs-quality", Target::Cs, st).unwrap()
    }

    #[test]
    fn srgb_curve_round_trips_every_8_bit_code() {
        for k in 0..=255u32 {
            let v = k as f32 / 255.0;
            let back = (linear_to_srgb(srgb_to_linear(v)) * 255.0 + 0.5).floor() as u32;
            assert_eq!(back, k);
        }
        assert!((srgb_to_linear(0.5) - 0.214_041).abs() < 1e-5);
    }

    #[test]
    fn remove_drops_alpha_and_srgb_content_goes_linear() {
        let img = Rgba32F::new(2, 2, [0.5, 0.5, 0.5, 0.3]);
        let w = preprocess(img, &profile(Subtype::Diffuse)); // diffuse default: remove, srgb
        assert!(!w.alpha_present);
        assert_eq!(w.domain, Domain::LinearLight);
        assert_eq!(w.img.px[0][3], 1.0);
        assert!((w.img.px[0][0] - 0.214_041).abs() < 1e-5);
    }

    #[test]
    fn normals_decode_flip_and_rebuild_z() {
        let mut p = profile(Subtype::Normal);
        p.channels.flip_green = true;
        p.channels.reconstruct_z = true;
        let img = Rgba32F::new(1, 1, [0.5, 1.0, 0.0, 0.8]);
        let w = preprocess(img, &p);
        assert_eq!(w.domain, Domain::Normal);
        let v = w.img.px[0];
        assert!((v[0]).abs() < 1e-6 && (v[1] + 1.0).abs() < 1e-6 && v[2].abs() < 1e-6);
        assert!(w.alpha_present && (v[3] - 0.8).abs() < 1e-6); // A = specular data, untouched
    }

    #[test]
    fn grayscale_luma_and_swizzle() {
        let mut p = profile(Subtype::Height);
        p.channels.grayscale_source = GraySource::Luma;
        let w = preprocess(Rgba32F::new(1, 1, [1.0, 0.0, 0.0, 1.0]), &p);
        assert!((w.img.px[0][0] - 0.2126).abs() < 1e-6 && w.img.px[0][1] == w.img.px[0][0]);
        let mut p = profile(Subtype::GenericData);
        p.channels.swizzle = Some("bgr1".into());
        let w = preprocess(Rgba32F::new(1, 1, [0.1, 0.2, 0.3, 0.4]), &p);
        assert_eq!(w.img.px[0], [0.3, 0.2, 0.1, 1.0]);
    }

    #[test]
    fn domains() {
        assert_eq!(domain_for(&profile(Subtype::Msn)), Domain::Msn);
        assert_eq!(domain_for(&profile(Subtype::Flow)), Domain::SignedRg);
        assert_eq!(domain_for(&profile(Subtype::PbrEmissive)), Domain::Hdr);
        assert_eq!(domain_for(&profile(Subtype::Rmaos)), Domain::Stored);
        assert!(preprocess(Rgba32F::new(1, 1, [0.5; 4]), &profile(Subtype::Rfaos)).roughness_r_squared);
    }
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p engine ops`
Expected: FAIL to compile.

- [ ] **Step 3: Implement**

```rust
//! Preprocess (spec §5.4): channel and colour operations before mips.
use crate::config::*;
use crate::dilate::dilate_under_zero_alpha;
use crate::image::Rgba32F;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Domain {
    /// sRGB-encoded colour decoded to linear light (re-encoded at quantise).
    LinearLight,
    /// Stored values (linear data, grayscale, packed channels).
    Stored,
    /// Unit vectors in [-1, 1] (DirectX normal maps).
    Normal,
    /// Model-space normals: R = X, G = Z, B = Y; length-only renormalisation.
    Msn,
    /// `_flow`: signed RG vector, never normalised.
    SignedRg,
    /// Linear float for BC6H.
    Hdr,
}

pub struct Work {
    pub img: Rgba32F,
    pub alpha_present: bool,
    pub domain: Domain,
    pub roughness_r_squared: bool,
}

pub fn domain_for(p: &Profile) -> Domain {
    if p.format == Format::Bc6hUf16 {
        Domain::Hdr
    } else if p.subtype == Subtype::Msn {
        Domain::Msn
    } else if p.category == Category::Normal {
        Domain::Normal
    } else if p.subtype == Subtype::Flow {
        Domain::SignedRg
    } else if p.color.content == ColorContent::Srgb {
        Domain::LinearLight
    } else {
        Domain::Stored
    }
}

pub fn srgb_to_linear(v: f32) -> f32 {
    if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
}

pub fn linear_to_srgb(v: f32) -> f32 {
    if v <= 0.003_130_8 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 }
}

fn swizzle(px: [f32; 4], s: &str) -> [f32; 4] {
    let mut out = [0.0; 4];
    for (i, c) in s.chars().take(4).enumerate() {
        out[i] = match c {
            'r' => px[0],
            'g' => px[1],
            'b' => px[2],
            'a' => px[3],
            '0' => 0.0,
            _ => 1.0,
        };
    }
    out
}

pub fn preprocess(mut img: Rgba32F, p: &Profile) -> Work {
    let domain = domain_for(p);
    let alpha_present = p.alpha.policy != AlphaPolicy::Remove;
    for px in &mut img.px {
        if !alpha_present {
            px[3] = 1.0; // (1) the channel is dropped; A = 255 is synthesised at quantise
        }
        if let Some(s) = &p.channels.swizzle {
            *px = swizzle(*px, s); // (2)
        }
        if p.category == Category::Grayscale {
            let g = match p.channels.grayscale_source {
                GraySource::R => px[0],
                GraySource::Luma => 0.2126 * px[0] + 0.7152 * px[1] + 0.0722 * px[2],
            };
            px[0] = g;
            px[1] = g;
            px[2] = g;
        }
        match domain {
            Domain::Normal => {
                let mut v = [px[0] * 2.0 - 1.0, px[1] * 2.0 - 1.0, px[2] * 2.0 - 1.0];
                if p.channels.flip_green {
                    v[1] = -v[1];
                }
                if p.channels.reconstruct_z {
                    v[2] = (1.0 - v[0] * v[0] - v[1] * v[1]).max(0.0).sqrt();
                }
                v[2] = v[2].max(0.0);
                px[0] = v[0];
                px[1] = v[1];
                px[2] = v[2];
            }
            Domain::Msn => {
                for c in 0..3 {
                    px[c] = px[c] * 2.0 - 1.0;
                }
            }
            Domain::SignedRg => {
                px[0] = px[0] * 2.0 - 1.0;
                px[1] = px[1] * 2.0 - 1.0;
            }
            _ => {}
        }
    }
    if alpha_present && p.alpha_weighted_mips() {
        dilate_under_zero_alpha(&mut img); // (4)
    }
    if p.color.content == ColorContent::Srgb && matches!(domain, Domain::LinearLight | Domain::Hdr) {
        for px in &mut img.px {
            for c in 0..3 {
                px[c] = srgb_to_linear(px[c]); // (5)
            }
        }
    }
    Work { img, alpha_present, domain, roughness_r_squared: p.subtype.is_roughness_member() }
}
```
Add `pub mod ops;` to `lib.rs`.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p engine ops`
Expected: 5 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/engine
git commit -m "feat(engine): preprocess - alpha remove, swizzle/grayscale, normal decode, dilation, sRGB decode"
```

---

### Task 29: Mip chain (spec §5.5)

**Files:**
- Create: `crates/engine/src/mips.rs`
- Modify: `crates/engine/src/lib.rs`
- Test: unit tests in `mips.rs`

**Interfaces:**
- Consumes: `ops::{Work, Domain}`, `Profile`.
- Produces: `pub fn level_count(w: u32, h: u32, levels: &MipLevels) -> u32` (full = ⌊log2 max⌋ + 1); `pub fn build_chain(work: &Work, p: &Profile) -> Vec<Rgba32F>` — finalised levels in the Work's domain.

Method: an *accumulation* chain is built by recursive 2×2 box filtering of the unnormalised base (equivalent to averaging the base footprint at power-of-two sizes, spec §5.5); each output level is a *finalised* copy (√ for the roughness R channel, renormalised vectors for Normal/Msn at levels ≥ 1, negatives clamped for HDR, [0, 1] clamp for linear light). Alpha-weighted RGB (cutout/blend/cutout_blend) carries the child alpha as the weight, which equals the base-footprint weighted average; when a footprint has no alpha, the plain average is used. Alpha is always a plain box. Mip 0 of a normal map stays as authored.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::*;
    use crate::ops::{preprocess, Domain, Work};
    use crate::presets::PresetRegistry;

    fn profile(st: Subtype) -> Profile {
        PresetRegistry::builtin().unwrap().profile("cs-quality", Target::Cs, st).unwrap()
    }
    fn work(img: Rgba32F, domain: Domain, alpha: bool) -> Work {
        Work { img, alpha_present: alpha, domain, roughness_r_squared: false }
    }

    #[test]
    fn level_counts() {
        assert_eq!(level_count(256, 64, &MipLevels::Keyword(MipKeyword::Full)), 9);
        assert_eq!(level_count(256, 64, &MipLevels::Keyword(MipKeyword::None)), 1);
        assert_eq!(level_count(256, 64, &MipLevels::Count(3)), 3);
        assert_eq!(level_count(256, 64, &MipLevels::Count(40)), 9);
        assert_eq!(level_count(1, 1, &MipLevels::Keyword(MipKeyword::Full)), 1);
    }

    #[test]
    fn box_chain_of_checkerboard_and_non_square() {
        let mut img = Rgba32F::new(4, 2, [0.0, 0.0, 0.0, 1.0]);
        for y in 0..2 {
            for x in 0..4 {
                img.at_mut(x, y)[0] = ((x + y) % 2) as f32;
            }
        }
        let c = build_chain(&work(img, Domain::Stored, false), &profile(Subtype::GenericData));
        assert_eq!(c.iter().map(|l| (l.width, l.height)).collect::<Vec<_>>(), [(4, 2), (2, 1), (1, 1)]);
        assert!(c[1].px.iter().all(|p| (p[0] - 0.5).abs() < 1e-6));
        assert!((c[2].px[0][0] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn alpha_weighted_rgb_ignores_invisible_colour() {
        let mut img = Rgba32F::new(2, 2, [0.0, 1.0, 0.0, 0.0]);
        img.px[0] = [1.0, 0.0, 0.0, 1.0];
        let mut p = profile(Subtype::Diffuse);
        p.alpha.policy = AlphaPolicy::Blend;
        let c = build_chain(&work(img, Domain::LinearLight, true), &p);
        assert_eq!(c[1].px[0], [1.0, 0.0, 0.0, 0.25]);
    }

    #[test]
    fn normals_renormalised_from_level_1_only() {
        let mut img = Rgba32F::new(2, 1, [0.0, 0.0, 1.0, 1.0]);
        img.px[0] = [1.0, 0.0, 0.0, 1.0];
        let c = build_chain(&work(img.clone(), Domain::Normal, true), &profile(Subtype::Normal));
        assert_eq!(c[0].px, img.px);
        let v = c[1].px[0];
        assert!((v[0] - 0.707_106_8).abs() < 1e-5 && (v[2] - 0.707_106_8).abs() < 1e-5);
    }

    #[test]
    fn roughness_is_averaged_as_alpha_r_squared() {
        let mut img = Rgba32F::new(2, 1, [0.0, 0.0, 0.0, 1.0]);
        img.px[1][0] = 1.0;
        let w = preprocess(img, &profile(Subtype::Rmaos));
        let c = build_chain(&w, &profile(Subtype::Rmaos));
        assert!((c[1].px[0][0] - 0.5f32.sqrt()).abs() < 1e-6);
        assert_eq!(c[0].px[1][0], 1.0);
    }
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p engine mips`
Expected: FAIL to compile.

- [ ] **Step 3: Implement**

```rust
//! Float mip chain (spec §5.5). Box filter only in v1 Phase 1; other kernels arrive in milestone M6.
use crate::config::*;
use crate::image::Rgba32F;
use crate::ops::{Domain, Work};

pub fn level_count(w: u32, h: u32, levels: &MipLevels) -> u32 {
    let full = 32 - w.max(h).max(1).leading_zeros();
    match levels {
        MipLevels::Keyword(MipKeyword::Full) => full,
        MipLevels::Keyword(MipKeyword::None) => 1,
        MipLevels::Count(n) => (*n).clamp(1, full),
    }
}

fn downsample(prev: &Rgba32F, weighted: bool) -> Rgba32F {
    let (w, h) = ((prev.width / 2).max(1), (prev.height / 2).max(1));
    let xs = |x: u32| if prev.width > 1 { vec![2 * x, 2 * x + 1] } else { vec![0] };
    let ys = |y: u32| if prev.height > 1 { vec![2 * y, 2 * y + 1] } else { vec![0] };
    let mut out = Rgba32F::new(w, h, [0.0; 4]);
    for y in 0..h {
        for x in 0..w {
            let kids: Vec<[f32; 4]> = ys(y).iter().flat_map(|&cy| xs(x).into_iter().map(move |cx| (cx, cy))).map(|(cx, cy)| prev.at(cx, cy)).collect();
            let n = kids.len() as f32;
            let sum_a: f32 = kids.iter().map(|k| k[3]).sum();
            let mut px = [0.0f32; 4];
            for c in 0..3 {
                px[c] = if weighted && sum_a > 0.0 {
                    kids.iter().map(|k| k[c] * k[3]).sum::<f32>() / sum_a
                } else {
                    kids.iter().map(|k| k[c]).sum::<f32>() / n
                };
            }
            px[3] = sum_a / n;
            *out.at_mut(x, y) = px;
        }
    }
    out
}

fn finalize(acc: &Rgba32F, work: &Work, level: u32) -> Rgba32F {
    let mut img = acc.clone();
    for px in &mut img.px {
        if work.roughness_r_squared {
            px[0] = px[0].max(0.0).sqrt();
        }
        match work.domain {
            Domain::Normal | Domain::Msn if level > 0 => {
                let len = (px[0] * px[0] + px[1] * px[1] + px[2] * px[2]).sqrt();
                if len > 1e-8 {
                    for c in 0..3 {
                        px[c] /= len;
                    }
                } else if work.domain == Domain::Normal {
                    px[0] = 0.0;
                    px[1] = 0.0;
                    px[2] = 1.0;
                }
            }
            Domain::Hdr => {
                for c in 0..3 {
                    px[c] = px[c].max(0.0);
                }
            }
            Domain::LinearLight => {
                for c in 0..3 {
                    px[c] = px[c].clamp(0.0, 1.0);
                }
            }
            _ => {}
        }
    }
    img
}

pub fn build_chain(work: &Work, p: &Profile) -> Vec<Rgba32F> {
    let n = level_count(work.img.width, work.img.height, &p.mips.levels);
    let weighted = work.alpha_present && p.alpha_weighted_mips();
    let mut acc = work.img.clone();
    if work.roughness_r_squared {
        for px in &mut acc.px {
            px[0] *= px[0];
        }
    }
    let mut out = Vec::with_capacity(n as usize);
    for l in 0..n {
        if l > 0 {
            acc = downsample(&acc, weighted);
        }
        out.push(finalize(&acc, work, l));
    }
    out
}
```
Add `pub mod mips;` to `lib.rs`.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p engine mips`
Expected: 5 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/engine
git commit -m "feat(engine): float mip chain per domain (linear light, alpha-weighted, normals, roughness, HDR)"
```

---

### Task 30: Coverage-preserving alpha (rebuild of AlphaBloom's Auto Coverage, credited)

**Files:**
- Create: `crates/engine/src/coverage.rs`
- Modify: `crates/engine/src/lib.rs`
- Test: unit tests in `coverage.rs`

**Interfaces:**
- Produces:
  - `pub fn passes(code: u8, threshold: u8, compare: AlphaCompare) -> bool`
  - `pub fn coverage_fraction(alpha: impl Iterator<Item = f32>, threshold: u8, compare: AlphaCompare) -> f64` (alpha in [0, 1], tested on the rounded 8-bit code)
  - `pub fn preserve_coverage(chain: &mut [Rgba32F], threshold: u8, compare: AlphaCompare)` — level 0 is the reference; every level ≥ 1 is scaled

Algorithm (spec §5.5): `p` = pass fraction of level 0; for each level ≥ 1 with `n` texels, `k = round(p · n)`; if `k == 0` the level is left unchanged (avoids AlphaBloom's floored target); else `a` = the k-th largest float alpha of the level (if `a == 0`, the smallest positive alpha; if none, unchanged); `τ` = the smallest passing code value ((t + 1)/255 for `gt`, t/255 for `ge`); every alpha is scaled by `τ / a` and clamped to [0, 1], so the boundary texel lands on a passing code. No 8-bit LUT; counter and solver share `passes`.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AlphaCompare;
    use crate::image::Rgba32F;

    fn alpha_of(img: &Rgba32F) -> impl Iterator<Item = f32> + '_ {
        img.px.iter().map(|p| p[3])
    }

    /// 64x64 sparse "foliage": alpha = u^4 for a deterministic uniform u (continuous values, so the
    /// quantile is not degenerate; a binary base would make whole mip levels tie). Box-filtered down.
    fn foliage_chain() -> Vec<Rgba32F> {
        let mut base = Rgba32F::new(64, 64, [0.0; 4]);
        let mut s: u32 = 12345;
        for p in &mut base.px {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let u = (s >> 8) as f32 / (1u32 << 24) as f32;
            p[3] = u * u * u * u;
        }
        let mut chain = vec![base];
        while chain.last().unwrap().width > 1 {
            let p = chain.last().unwrap();
            let mut n = Rgba32F::new(p.width / 2, p.height / 2, [0.0; 4]);
            for y in 0..n.height {
                for x in 0..n.width {
                    let a = (p.at(2 * x, 2 * y)[3] + p.at(2 * x + 1, 2 * y)[3] + p.at(2 * x, 2 * y + 1)[3] + p.at(2 * x + 1, 2 * y + 1)[3]) / 4.0;
                    n.at_mut(x, y)[3] = a;
                }
            }
            chain.push(n);
        }
        chain
    }

    #[test]
    fn comparisons() {
        assert!(passes(128, 127, AlphaCompare::Gt) && !passes(127, 127, AlphaCompare::Gt));
        assert!(passes(127, 127, AlphaCompare::Ge));
    }

    #[test]
    fn without_preservation_sparse_foliage_vanishes() {
        let c = foliage_chain();
        let base = coverage_fraction(alpha_of(&c[0]), 127, AlphaCompare::Gt);
        assert!((base - 0.159).abs() < 0.02, "base coverage {base}"); // P(u^4 > 0.5) = 1 - 0.5^0.25
        assert!(coverage_fraction(alpha_of(&c[3]), 127, AlphaCompare::Gt) < 0.05);
    }

    #[test]
    fn preservation_keeps_coverage_within_2_points_where_resolvable() {
        let mut c = foliage_chain();
        let base = coverage_fraction(alpha_of(&c[0]), 127, AlphaCompare::Gt);
        preserve_coverage(&mut c, 127, AlphaCompare::Gt);
        for l in &c[1..] {
            let cov = coverage_fraction(alpha_of(l), 127, AlphaCompare::Gt);
            if l.px.len() >= 64 {
                assert!((cov - base).abs() <= 0.02, "{}x{}: {cov} vs base {base}", l.width, l.height);
            }
        }
    }

    #[test]
    fn zero_target_levels_are_left_alone() {
        let mut base = Rgba32F::new(4, 4, [0.0; 4]);
        base.px[0][3] = 1.0; // 1/16 coverage
        let mut lvl1 = Rgba32F::new(2, 2, [0.0; 4]);
        lvl1.px[0][3] = 0.25; // k = round(4/16) = 0 -> unchanged
        let mut c = vec![base, lvl1.clone()];
        preserve_coverage(&mut c, 127, AlphaCompare::Gt);
        assert_eq!(c[1], lvl1);
    }
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p engine coverage`
Expected: FAIL to compile.

- [ ] **Step 3: Implement**

```rust
//! Coverage-preserving alpha mips. Rebuilds the idea of AlphaBloom's "Auto Coverage" (tkRiverGamer) and
//! Ignacio Castaño's method (see docs/CREDITS.md); no code is copied.
use crate::config::AlphaCompare;
use crate::image::Rgba32F;

fn code8(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

pub fn passes(code: u8, threshold: u8, compare: AlphaCompare) -> bool {
    match compare {
        AlphaCompare::Gt => code > threshold,
        AlphaCompare::Ge => code >= threshold,
    }
}

pub fn coverage_fraction(alpha: impl Iterator<Item = f32>, threshold: u8, compare: AlphaCompare) -> f64 {
    let (mut n, mut pass) = (0u64, 0u64);
    for a in alpha {
        n += 1;
        pass += passes(code8(a), threshold, compare) as u64;
    }
    if n == 0 { 0.0 } else { pass as f64 / n as f64 }
}

pub fn preserve_coverage(chain: &mut [Rgba32F], threshold: u8, compare: AlphaCompare) {
    let Some((base, rest)) = chain.split_first_mut() else { return };
    let p = coverage_fraction(base.px.iter().map(|q| q[3]), threshold, compare);
    let tau = match compare {
        AlphaCompare::Gt => (threshold as f32 + 1.0) / 255.0,
        AlphaCompare::Ge => threshold as f32 / 255.0,
    };
    for level in rest {
        let n = level.px.len();
        let k = (p * n as f64).round() as usize;
        if k == 0 {
            continue;
        }
        let mut sorted: Vec<f32> = level.px.iter().map(|q| q[3]).collect();
        sorted.sort_by(|a, b| b.partial_cmp(a).unwrap());
        let mut a = sorted[k.min(n) - 1];
        if a <= 0.0 {
            match sorted.iter().rev().find(|&&v| v > 0.0) {
                Some(&v) => a = v,
                None => continue,
            }
        }
        let scale = tau / a;
        for q in &mut level.px {
            q[3] = (q[3] * scale).clamp(0.0, 1.0);
        }
    }
}
```
Add `pub mod coverage;` to `lib.rs`.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p engine coverage`
Expected: 4 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/engine
git commit -m "feat(engine): coverage-preserving alpha mips (quantile solve, credited to AlphaBloom/Castaño)"
```

---

### Task 31: Quantise a float level to encoder input

**Files:**
- Create: `crates/engine/src/quantize.rs`
- Modify: `crates/engine/src/lib.rs`
- Test: unit tests in `quantize.rs`

**Interfaces:**
- Consumes: `ops::{Work, Domain, linear_to_srgb}`, `Profile`.
- Produces: `pub enum EncoderInput { Ldr(Vec<[u8; 4]>), Hdr(Vec<[u16; 4]>) }`; `pub fn unorm8(v: f32) -> u8` (`floor(clamp(v)·255 + 0.5)`); `pub fn to_encoder_input(level: &Rgba32F, work: &Work, p: &Profile) -> EncoderInput`.

Per domain: LinearLight → exact sRGB encode then `unorm8`; Normal/Msn → `v·0.5 + 0.5`; SignedRg → R,G `v·0.5 + 0.5`, B = 0; Stored → as is; alpha = `unorm8(a)` when present, else **255** (the only place A = 255 is synthesised for `remove`). BC6H → IEEE half of `clamp(v, 0, 65504)`, A = 1.0.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::*;
    use crate::ops::{preprocess, Domain, Work};
    use crate::presets::PresetRegistry;

    fn profile(st: Subtype) -> Profile {
        PresetRegistry::builtin().unwrap().profile("cs-quality", Target::Cs, st).unwrap()
    }

    #[test]
    fn eight_bit_sources_survive_load_preprocess_quantise_exactly() {
        for st in [Subtype::Diffuse, Subtype::GenericData, Subtype::Normal, Subtype::Msn] {
            let mut img = Rgba32F::new(256, 1, [0.0; 4]);
            for k in 0..256 {
                let v = k as f32 / 255.0;
                img.px[k] = [v, v, v, v];
            }
            let p = profile(st);
            let w = preprocess(img, &p);
            let EncoderInput::Ldr(q) = to_encoder_input(&w.img, &w, &p) else { panic!() };
            for k in 0..256 {
                assert_eq!(q[k][0] as usize, k, "{st:?} code {k}");
            }
        }
    }

    #[test]
    fn removed_alpha_is_255_and_hdr_is_half() {
        let p = profile(Subtype::Diffuse); // remove
        let w = preprocess(Rgba32F::new(1, 1, [0.5, 0.5, 0.5, 0.2]), &p);
        let EncoderInput::Ldr(q) = to_encoder_input(&w.img, &w, &p) else { panic!() };
        assert_eq!(q[0][3], 255);
        let p = profile(Subtype::PbrEmissive);
        let w = Work { img: Rgba32F::new(1, 1, [2.0, -1.0, 0.5, 1.0]), alpha_present: false, domain: Domain::Hdr, roughness_r_squared: false };
        let EncoderInput::Hdr(h) = to_encoder_input(&w.img, &w, &p) else { panic!() };
        assert_eq!(h[0], [0x4000, 0x0000, 0x3800, 0x3C00]);
    }
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p engine quantize`
Expected: FAIL to compile.

- [ ] **Step 3: Implement**

```rust
//! Float level -> encoder input (u8 RGBA for BC1/3/4/5/7, half RGBA for BC6H).
use crate::config::*;
use crate::image::Rgba32F;
use crate::ops::{linear_to_srgb, Domain, Work};
use half::f16;

#[derive(Debug, Clone, PartialEq)]
pub enum EncoderInput {
    Ldr(Vec<[u8; 4]>),
    Hdr(Vec<[u16; 4]>),
}

pub fn unorm8(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0 + 0.5).floor() as u8
}

pub fn to_encoder_input(level: &Rgba32F, work: &Work, p: &Profile) -> EncoderInput {
    if p.format == Format::Bc6hUf16 {
        let h = |v: f32| f16::from_f32(v.clamp(0.0, 65504.0)).to_bits();
        return EncoderInput::Hdr(level.px.iter().map(|q| [h(q[0]), h(q[1]), h(q[2]), f16::ONE.to_bits()]).collect());
    }
    EncoderInput::Ldr(
        level
            .px
            .iter()
            .map(|q| {
                let rgb = match work.domain {
                    Domain::LinearLight => [linear_to_srgb(q[0]), linear_to_srgb(q[1]), linear_to_srgb(q[2])],
                    Domain::Normal | Domain::Msn => [q[0] * 0.5 + 0.5, q[1] * 0.5 + 0.5, q[2] * 0.5 + 0.5],
                    Domain::SignedRg => [q[0] * 0.5 + 0.5, q[1] * 0.5 + 0.5, 0.0],
                    Domain::Stored | Domain::Hdr => [q[0], q[1], q[2]],
                };
                let a = if work.alpha_present { unorm8(q[3]) } else { 255 };
                [unorm8(rgb[0]), unorm8(rgb[1]), unorm8(rgb[2]), a]
            })
            .collect(),
    )
}
```
Add `pub mod quantize;` to `lib.rs`.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p engine quantize`
Expected: 2 passed. If an sRGB code fails to round-trip, keep the exact curve and compute the encode in f64 (`linear_to_srgb` on `f64`) instead of loosening the test — Task 38's engine-path parity gate depends on exactness.

- [ ] **Step 5: Commit**

```bash
git add crates/engine
git commit -m "feat(engine): quantise float levels to encoder input (exact 8-bit round trip)"
```

---

### Task 32: Encode stage — band-parallel compression with exact-opaque handling

Entry: Tasks 10–14 passed (encoder gates; S5 mechanism decided).

**Files:**
- Create: `crates/engine/src/encode.rs`
- Modify: `crates/engine/src/lib.rs`
- Test: `crates/engine/tests/encode.rs`

**Interfaces:**
- Consumes: `quantize::EncoderInput`, `Profile`, `encoders_sys::*`.
- Produces:
  - `pub struct EncodeCtx { pub cancel: Option<Arc<AtomicBool>> }` (`Default`)
  - `pub struct Cancelled;` (error type)
  - `pub fn bc7_level(e: Effort) -> Bc7Level`; `pub fn bc6h_level(e: Effort) -> Bc6hLevel`; `pub fn radius(r: SearchRadius) -> u32`
  - `pub fn encode_level(input: &EncoderInput, w: u32, h: u32, p: &Profile, ctx: &EncodeCtx) -> Result<Vec<u8>, Cancelled>` — blocks in raster order, `ceil(w/4)·ceil(h/4)·block_bytes` bytes; one block row per rayon task; bc7e called with ≤ 64 blocks per call (as the bench); with `alpha.exact_opaque` and BC7: native flag when `BC7E_EXACT_OPAQUE_NATIVE`, else `force_opaque_mode6` on every block whose input alpha is 255 throughout; cancellation checked before each band.

- [ ] **Step 1: Write the failing tests**

`crates/engine/tests/encode.rs`:
```rust
use engine::config::*;
use engine::encode::*;
use engine::presets::PresetRegistry;
use engine::quantize::EncoderInput;
use std::sync::{atomic::AtomicBool, Arc};

fn profile(st: Subtype) -> Profile {
    PresetRegistry::builtin().unwrap().profile("cs-quality", Target::Cs, st).unwrap()
}

fn gradient(w: u32, h: u32) -> Vec<[u8; 4]> {
    (0..w * h).map(|i| [(i * 7 % 256) as u8, (i * 13 % 256) as u8, (i * 3 % 256) as u8, 255]).collect()
}

#[test]
fn sizes_per_format_including_sub_block_levels() {
    for (st, fmt, bytes) in [(Subtype::Diffuse, Format::Bc7, 16), (Subtype::Envmask, Format::Bc4, 8), (Subtype::Flow, Format::Bc5, 16)] {
        let mut p = profile(st);
        p.format = fmt;
        let out = encode_level(&EncoderInput::Ldr(gradient(8, 4)), 8, 4, &p, &EncodeCtx::default()).ok().unwrap();
        assert_eq!(out.len(), 2 * bytes);
        let out = encode_level(&EncoderInput::Ldr(gradient(2, 1)), 2, 1, &p, &EncodeCtx::default()).ok().unwrap();
        assert_eq!(out.len(), bytes);
    }
    let p = profile(Subtype::PbrEmissive);
    let hdr = EncoderInput::Hdr(vec![[0x3C00, 0x3C00, 0x3C00, 0x3C00]; 2]);
    assert_eq!(encode_level(&hdr, 2, 1, &p, &EncodeCtx::default()).ok().unwrap().len(), 16);
}

#[test]
fn exact_opaque_bc7_decodes_255_everywhere() {
    let mut p = profile(Subtype::ComplexMaterial); // exact_opaque on by default
    p.alpha.policy = AlphaPolicy::Remove;
    let px = gradient(64, 64);
    let out = encode_level(&EncoderInput::Ldr(px), 64, 64, &p, &EncodeCtx::default()).ok().unwrap();
    for b in out.chunks(16) {
        let mut d = [0u8; 64];
        bcdec_rs::bc7(b, &mut d, 16);
        assert!(d.chunks(4).all(|t| t[3] == 255));
    }
}

#[test]
fn cancellation_stops_the_level() {
    let ctx = EncodeCtx { cancel: Some(Arc::new(AtomicBool::new(true))) };
    assert!(encode_level(&EncoderInput::Ldr(gradient(16, 16)), 16, 16, &profile(Subtype::Diffuse), &ctx).is_err());
}
```
Add `bcdec_rs.workspace = true` to engine `[dev-dependencies]` (it is already a normal dependency; tests can use it directly, so no change is needed).

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p engine --test encode`
Expected: FAIL to compile.

- [ ] **Step 3: Implement**

```rust
//! Compress (spec §5.6): one block row per rayon task; bc7e gets <= 64 blocks per call.
use crate::config::*;
use crate::quantize::EncoderInput;
use encoders_sys::bc7_fixup::force_opaque_mode6;
use encoders_sys::*;
use rayon::prelude::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[derive(Debug, Clone, Default)]
pub struct EncodeCtx {
    pub cancel: Option<Arc<AtomicBool>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("cancelled")]
pub struct Cancelled;

pub fn bc7_level(e: Effort) -> Bc7Level {
    match e {
        Effort::Ultrafast => Bc7Level::Ultrafast,
        Effort::Veryfast => Bc7Level::Veryfast,
        Effort::Fast => Bc7Level::Fast,
        Effort::Basic => Bc7Level::Basic,
        Effort::Veryslow => Bc7Level::Veryslow,
        Effort::Slowest => Bc7Level::Slowest,
    }
}

pub fn bc6h_level(e: Effort) -> Bc6hLevel {
    match e {
        Effort::Ultrafast | Effort::Veryfast => Bc6hLevel::Veryfast,
        Effort::Fast => Bc6hLevel::Fast,
        Effort::Basic => Bc6hLevel::Basic,
        Effort::Veryslow | Effort::Slowest => Bc6hLevel::Veryslow,
    }
}

pub fn radius(r: SearchRadius) -> u32 {
    match r {
        SearchRadius::Default => BC45_RADIUS_DEFAULT,
        SearchRadius::Max => BC45_RADIUS_MAX,
    }
}

fn encode_band(b: &[[u8; 4]], o: &mut [u8], p: &Profile) {
    let e = &p.encoder;
    match p.format {
        Format::Bc7 => {
            let native = p.alpha.exact_opaque && BC7E_EXACT_OPAQUE_NATIVE;
            let params = Bc7Params { level: bc7_level(e.effort), perceptual: e.perceptual, exact_opaque: native };
            for (oc, bc) in o.chunks_mut(64 * 16).zip(b.chunks(64 * 16)) {
                bc7e_compress_blocks(bc, oc, params);
            }
            if p.alpha.exact_opaque && !native {
                for (blk, src) in o.chunks_mut(16).zip(b.chunks(16)) {
                    if src.iter().all(|t| t[3] == 255) {
                        force_opaque_mode6(blk.try_into().unwrap());
                    }
                }
            }
        }
        f => {
            let bs = f.block_bytes();
            for (blk, src) in o.chunks_mut(bs).zip(b.chunks(16)) {
                let src: &[[u8; 4]; 16] = src.try_into().unwrap();
                match f {
                    Format::Bc1 => blk.copy_from_slice(&bc1_encode_block(src, e.rgbcx_level, true, e.bc1_transparent_black)),
                    Format::Bc3 => blk.copy_from_slice(&bc3_encode_block(src, e.rgbcx_level, radius(e.bc45_search_radius))),
                    Format::Bc4 => blk.copy_from_slice(&bc4_encode_block(src, 0, radius(e.bc45_search_radius))),
                    Format::Bc5 => blk.copy_from_slice(&bc5_encode_block(src, radius(e.bc45_search_radius))),
                    Format::Bc7 | Format::Bc6hUf16 => unreachable!("handled elsewhere"),
                }
            }
        }
    }
}

fn pad_hdr(px: &[[u16; 4]], w: u32, h: u32) -> (Vec<[u16; 4]>, u32, u32) {
    let (pw, ph) = (w.div_ceil(4) * 4, h.div_ceil(4) * 4);
    let mut out = Vec::with_capacity((pw * ph) as usize);
    for y in 0..ph {
        for x in 0..pw {
            out.push(px[(y.min(h - 1) * w + x.min(w - 1)) as usize]);
        }
    }
    (out, pw, ph)
}

pub fn encode_level(input: &EncoderInput, w: u32, h: u32, p: &Profile, ctx: &EncodeCtx) -> Result<Vec<u8>, Cancelled> {
    let cancelled = || ctx.cancel.as_ref().is_some_and(|c| c.load(Ordering::Relaxed));
    let out = match input {
        EncoderInput::Ldr(px) => {
            let blocks = gather_blocks(px, w, h);
            let (bw, bs) = (w.div_ceil(4) as usize, p.format.block_bytes());
            let mut out = vec![0u8; blocks.len() / 16 * bs];
            out.par_chunks_mut(bw * bs).zip(blocks.par_chunks(bw * 16)).for_each(|(o, b)| {
                if !cancelled() {
                    encode_band(b, o, p);
                }
            });
            out
        }
        EncoderInput::Hdr(px) => {
            let (padded, pw, ph) = pad_hdr(px, w, h);
            let level = bc6h_level(p.encoder.effort);
            let mut out = vec![0u8; (pw / 4 * (ph / 4) * 16) as usize];
            out.par_chunks_mut((pw / 4 * 16) as usize).zip(padded.par_chunks((pw * 4) as usize)).for_each(|(o, rows)| {
                if !cancelled() {
                    bc6h_compress_rows(rows, pw, 4, o, level);
                }
            });
            out
        }
    };
    if cancelled() { Err(Cancelled) } else { Ok(out) }
}
```
Add `pub mod encode;` to `lib.rs`.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p engine --test encode`
Expected: 3 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/engine
git commit -m "feat(engine): band-parallel encode stage with exact-opaque native flag or byte-patch fallback"
```

---

### Task 33: DDS writer (spec §5.7)

**Files:**
- Create: `crates/engine/src/dds.rs`
- Modify: `crates/engine/src/lib.rs`
- Test: `crates/engine/tests/dds_write.rs`

**Interfaces:**
- Produces:
  - flag constants `DDSD_CAPS, DDSD_HEIGHT, DDSD_WIDTH, DDSD_PIXELFORMAT, DDSD_MIPMAPCOUNT, DDSD_LINEARSIZE, DDPF_FOURCC, DDPF_NORMAL, DDSCAPS_COMPLEX, DDSCAPS_TEXTURE, DDSCAPS_MIPMAP`
  - `pub enum ResolvedHeader { Dx10, Legacy }`; `pub fn resolve_header(format: Format, srgb: bool, h: HeaderForm) -> ResolvedHeader`
  - `pub fn dxgi_format(format: Format, srgb: bool) -> u32`; `pub fn legacy_fourcc(format: Format) -> Option<[u8; 4]>`
  - `pub struct DdsDesc { pub width: u32, pub height: u32, pub format: Format, pub srgb: bool, pub header: ResolvedHeader }`
  - `pub fn level_size(format: Format, w: u32, h: u32) -> usize`
  - `pub fn dds_bytes(desc: &DdsDesc, levels: &[Vec<u8>]) -> Vec<u8>`
  - `pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()>`

Header rules: `auto` → legacy `DXT1`/`DXT5` for BC1/BC3 UNORM, DX10 otherwise; explicit `legacy` is honoured for BC1/BC3 UNORM and BC4/BC5 (`BC4U`/`BC5U`), and silently becomes DX10 where no legacy form exists (`_SRGB`, BC6H, BC7); `ATI1`/`ATI2` are never written. DX10 block: `TEXTURE2D` (3), misc 0, array size 1, misc2 0.

- [ ] **Step 1: Write the failing tests**

`crates/engine/tests/dds_write.rs`:
```rust
use engine::config::{Format, HeaderForm};
use engine::dds::*;

fn u(b: &[u8], off: usize) -> u32 {
    u32::from_le_bytes(b[off..off + 4].try_into().unwrap())
}

fn chain(format: Format, w: u32, h: u32) -> Vec<Vec<u8>> {
    let mut v = vec![];
    let (mut w, mut h) = (w, h);
    loop {
        v.push(vec![0xAB; level_size(format, w, h)]);
        if w == 1 && h == 1 {
            break;
        }
        w = (w / 2).max(1);
        h = (h / 2).max(1);
    }
    v
}

#[test]
fn bc7_srgb_full_chain_header() {
    let d = DdsDesc { width: 256, height: 64, format: Format::Bc7, srgb: true, header: resolve_header(Format::Bc7, true, HeaderForm::Legacy) };
    assert_eq!(d.header, ResolvedHeader::Dx10);
    let levels = chain(Format::Bc7, 256, 64);
    let b = dds_bytes(&d, &levels);
    assert_eq!(&b[..4], b"DDS ");
    assert_eq!(u(&b, 4), 124);
    assert_eq!(u(&b, 8), DDSD_CAPS | DDSD_HEIGHT | DDSD_WIDTH | DDSD_PIXELFORMAT | DDSD_LINEARSIZE | DDSD_MIPMAPCOUNT);
    assert_eq!((u(&b, 12), u(&b, 16), u(&b, 20), u(&b, 28)), (64, 256, 64 * 16 * 16, 9));
    assert_eq!(u(&b, 80) & DDPF_NORMAL, 0);
    assert_eq!(&b[84..88], b"DX10");
    assert_eq!(u(&b, 108), DDSCAPS_TEXTURE | DDSCAPS_COMPLEX | DDSCAPS_MIPMAP);
    assert_eq!((u(&b, 128), u(&b, 132), u(&b, 136), u(&b, 140), u(&b, 144)), (99, 3, 0, 1, 0));
    assert!(b[32..76].iter().all(|&x| x == 0), "reserved1 must be zero");
    assert_eq!(b.len(), 148 + levels.iter().map(Vec::len).sum::<usize>());
}

#[test]
fn legacy_forms() {
    assert_eq!(resolve_header(Format::Bc1, false, HeaderForm::Auto), ResolvedHeader::Legacy);
    assert_eq!(resolve_header(Format::Bc4, false, HeaderForm::Auto), ResolvedHeader::Dx10);
    assert_eq!(resolve_header(Format::Bc4, false, HeaderForm::Legacy), ResolvedHeader::Legacy);
    assert_eq!(resolve_header(Format::Bc1, true, HeaderForm::Auto), ResolvedHeader::Dx10);
    for (f, cc) in [(Format::Bc1, b"DXT1"), (Format::Bc3, b"DXT5"), (Format::Bc4, b"BC4U"), (Format::Bc5, b"BC5U")] {
        let d = DdsDesc { width: 4, height: 4, format: f, srgb: false, header: ResolvedHeader::Legacy };
        let b = dds_bytes(&d, &[vec![0; level_size(f, 4, 4)]]);
        assert_eq!(&b[84..88], cc);
        assert_eq!(u(&b, 8) & DDSD_MIPMAPCOUNT, 0);
        assert_eq!(u(&b, 108), DDSCAPS_TEXTURE);
        assert_eq!(b.len(), 128 + level_size(f, 4, 4));
    }
    assert_eq!(dxgi_format(Format::Bc6hUf16, false), 95);
    assert_eq!(dxgi_format(Format::Bc1, true), 72);
}

#[test]
fn atomic_write_replaces_and_leaves_no_temp() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("sub/x.dds");
    write_atomic(&p, b"one").unwrap();
    write_atomic(&p, b"two").unwrap();
    assert_eq!(std::fs::read(&p).unwrap(), b"two");
    assert_eq!(std::fs::read_dir(p.parent().unwrap()).unwrap().count(), 1);
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p engine --test dds_write`
Expected: FAIL to compile.

- [ ] **Step 3: Implement the writer part of `dds.rs`**

```rust
//! DDS container (spec §5.7): the engine's own writer, and the reader used by Validate and `sdc validate`.
use crate::config::{Format, HeaderForm};
use std::io::Write;
use std::path::Path;

pub const DDSD_CAPS: u32 = 0x1;
pub const DDSD_HEIGHT: u32 = 0x2;
pub const DDSD_WIDTH: u32 = 0x4;
pub const DDSD_PIXELFORMAT: u32 = 0x1000;
pub const DDSD_MIPMAPCOUNT: u32 = 0x2_0000;
pub const DDSD_LINEARSIZE: u32 = 0x8_0000;
pub const DDPF_FOURCC: u32 = 0x4;
pub const DDPF_NORMAL: u32 = 0x8000_0000;
pub const DDSCAPS_COMPLEX: u32 = 0x8;
pub const DDSCAPS_TEXTURE: u32 = 0x1000;
pub const DDSCAPS_MIPMAP: u32 = 0x40_0000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ResolvedHeader {
    Dx10,
    Legacy,
}

pub fn legacy_fourcc(format: Format) -> Option<[u8; 4]> {
    match format {
        Format::Bc1 => Some(*b"DXT1"),
        Format::Bc3 => Some(*b"DXT5"),
        Format::Bc4 => Some(*b"BC4U"),
        Format::Bc5 => Some(*b"BC5U"),
        _ => None,
    }
}

pub fn resolve_header(format: Format, srgb: bool, h: HeaderForm) -> ResolvedHeader {
    let legacy_ok = !srgb && legacy_fourcc(format).is_some();
    match h {
        HeaderForm::Auto if !srgb && matches!(format, Format::Bc1 | Format::Bc3) => ResolvedHeader::Legacy,
        HeaderForm::Legacy if legacy_ok => ResolvedHeader::Legacy,
        _ => ResolvedHeader::Dx10,
    }
}

pub fn dxgi_format(format: Format, srgb: bool) -> u32 {
    match (format, srgb) {
        (Format::Bc1, false) => 71,
        (Format::Bc1, true) => 72,
        (Format::Bc3, false) => 77,
        (Format::Bc3, true) => 78,
        (Format::Bc4, _) => 80,
        (Format::Bc5, _) => 83,
        (Format::Bc6hUf16, _) => 95,
        (Format::Bc7, false) => 98,
        (Format::Bc7, true) => 99,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DdsDesc {
    pub width: u32,
    pub height: u32,
    pub format: Format,
    pub srgb: bool,
    pub header: ResolvedHeader,
}

pub fn level_size(format: Format, w: u32, h: u32) -> usize {
    w.div_ceil(4) as usize * h.div_ceil(4) as usize * format.block_bytes()
}

pub fn dds_bytes(desc: &DdsDesc, levels: &[Vec<u8>]) -> Vec<u8> {
    let mips = levels.len() as u32;
    let mut h = [0u32; 31];
    h[0] = 124;
    h[1] = DDSD_CAPS | DDSD_HEIGHT | DDSD_WIDTH | DDSD_PIXELFORMAT | DDSD_LINEARSIZE | if mips > 1 { DDSD_MIPMAPCOUNT } else { 0 };
    h[2] = desc.height;
    h[3] = desc.width;
    h[4] = level_size(desc.format, desc.width, desc.height) as u32;
    h[6] = mips;
    h[18] = 32;
    h[19] = DDPF_FOURCC;
    h[20] = u32::from_le_bytes(match desc.header {
        ResolvedHeader::Legacy => legacy_fourcc(desc.format).expect("resolve_header guarantees a legacy form"),
        ResolvedHeader::Dx10 => *b"DX10",
    });
    h[26] = DDSCAPS_TEXTURE | if mips > 1 { DDSCAPS_COMPLEX | DDSCAPS_MIPMAP } else { 0 };
    let mut out = Vec::with_capacity(148 + levels.iter().map(Vec::len).sum::<usize>());
    out.extend_from_slice(b"DDS ");
    for v in h {
        out.extend_from_slice(&v.to_le_bytes());
    }
    if desc.header == ResolvedHeader::Dx10 {
        for v in [dxgi_format(desc.format, desc.srgb), 3, 0, 1, 0] {
            out.extend_from_slice(&v.to_le_bytes());
        }
    }
    for l in levels {
        out.extend_from_slice(l);
    }
    out
}

pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;
    let mut tmp = tempfile::Builder::new().prefix(".sdc-").suffix(".tmp").tempfile_in(dir)?;
    tmp.write_all(bytes)?;
    tmp.as_file().sync_all()?;
    tmp.persist(path).map_err(|e| e.error)?;
    Ok(())
}
```
Add `pub mod dds;` to `lib.rs`.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p engine --test dds_write`
Expected: 3 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/engine
git commit -m "feat(engine): DDS writer with spec §5.7 flags, legacy/DX10 forms and atomic writes"
```

---

### Task 34: DDS reader and BCn decode for validation

**Files:**
- Modify: `crates/engine/src/dds.rs` (reader + header check)
- Create: `crates/engine/src/decode.rs`
- Modify: `crates/engine/src/lib.rs`
- Test: `crates/engine/tests/dds_read.rs`

**Interfaces:**
- Produces:
  - `pub enum DdsPixelFormat { Bc1 { srgb: bool }, Bc2 { srgb: bool }, Bc3 { srgb: bool }, Bc4, Bc5, Bc6hUf16, Bc6hSf16, Bc7 { srgb: bool }, Rgba8 { srgb: bool }, Bgra8, Bgrx8, R8, Rg8, Rgba16f, Rgba32f }` with `fn block_bytes(self) -> Option<usize>`, `fn level_size(self, w: u32, h: u32) -> usize`, `fn as_format(self) -> Option<(Format, bool)>`
  - `pub enum HeaderKind { Legacy, Dx10 }`; `pub struct Dx10Fields { pub dxgi: u32, pub dimension: u32, pub misc: u32, pub array_size: u32, pub misc2: u32 }`
  - `pub struct DdsFile { pub width: u32, pub height: u32, pub mip_count: u32, pub format: DdsPixelFormat, pub header_kind: HeaderKind, pub fourcc: [u8; 4], pub dx10: Option<Dx10Fields>, pub flags: u32, pub pf_flags: u32, pub caps: u32, pub caps2: u32, pub linear_size: u32, pub reserved_nonzero: bool, pub is_cubemap: bool, pub levels: Vec<(u32, u32, std::ops::Range<usize>)>, pub data: Vec<u8> }` (`levels` = face 0)
  - `pub enum DdsError { NotDds, Truncated, Unsupported(String) }`
  - `pub fn read_dds(bytes: &[u8]) -> Result<DdsFile, DdsError>`
  - `pub fn check_header(f: &DdsFile) -> Vec<String>` (§5.7 violations; empty = OK)
  - `decode::Decoded { Ldr(Vec<[u8; 4]>), Hdr(Vec<[f32; 3]>) }`; `decode::decode_level(fmt: DdsPixelFormat, w: u32, h: u32, data: &[u8], bc1_model: Option<Bc1Model>) -> Result<Decoded, DdsError>` (BC1 with `Some(model)` decodes through rgbcx at that model; BC4 → `[r, r, r, 255]`; BC5 → `[r, g, 0, 255]`)

- [ ] **Step 1: Write the failing tests**

`crates/engine/tests/dds_read.rs`:
```rust
use engine::config::{Format, HeaderForm};
use engine::dds::*;
use engine::decode::*;

#[test]
fn round_trips_writer_output_and_passes_header_check() {
    for (f, srgb, h) in [(Format::Bc7, true, HeaderForm::Auto), (Format::Bc1, false, HeaderForm::Auto), (Format::Bc4, false, HeaderForm::Legacy), (Format::Bc6hUf16, false, HeaderForm::Auto)] {
        let desc = DdsDesc { width: 16, height: 8, format: f, srgb, header: resolve_header(f, srgb, h) };
        let levels: Vec<Vec<u8>> = [(16, 8), (8, 4), (4, 2), (2, 1), (1, 1)].iter().map(|&(w, h)| vec![0u8; level_size(f, w, h)]).collect();
        let file = read_dds(&dds_bytes(&desc, &levels)).unwrap();
        assert_eq!((file.width, file.height, file.mip_count, file.levels.len()), (16, 8, 5, 5));
        assert_eq!(file.format.as_format(), Some((f, srgb)));
        assert!(check_header(&file).is_empty(), "{f:?}: {:?}", check_header(&file));
    }
}

#[test]
fn reads_bench_outputs_and_rejects_garbage() {
    let root = std::path::PathBuf::from(std::env::var("SDC_BENCH_ROOT").unwrap_or(r"F:\ai\dds\bench".into()));
    let b = std::fs::read(root.join("out/bc7e_u5/rgb_real/Bricks104.dds")).unwrap();
    let f = read_dds(&b).unwrap();
    assert_eq!(f.format, DdsPixelFormat::Bc7 { srgb: false });
    let Decoded::Ldr(px) = decode_level(f.format, f.width, f.height, &f.data[f.levels[0].2.clone()], None).unwrap() else { panic!() };
    assert_eq!(px.len(), (f.width * f.height) as usize);
    let b = std::fs::read(root.join("out2/bc4_rgbcx/mask_real/Bricks104_Roughness.dds")).unwrap();
    assert_eq!(read_dds(&b).unwrap().format, DdsPixelFormat::Bc4);
    assert!(matches!(read_dds(b"nope"), Err(DdsError::NotDds)));
    let mut t = dds_bytes(&DdsDesc { width: 8, height: 8, format: Format::Bc7, srgb: false, header: ResolvedHeader::Dx10 }, &[vec![0; 64]]);
    t.truncate(160);
    assert!(matches!(read_dds(&t), Err(DdsError::Truncated)));
}

#[test]
fn header_check_catches_violations() {
    let desc = DdsDesc { width: 8, height: 8, format: Format::Bc7, srgb: false, header: ResolvedHeader::Dx10 };
    let mut b = dds_bytes(&desc, &[vec![0; 64], vec![0; 16], vec![0; 16], vec![0; 16]]);
    b[80..84].copy_from_slice(&(DDPF_FOURCC | DDPF_NORMAL).to_le_bytes()); // NVTT -normal style flag
    b[108..112].copy_from_slice(&DDSCAPS_TEXTURE.to_le_bytes()); // missing COMPLEX|MIPMAP
    let v = check_header(&read_dds(&b).unwrap());
    assert!(v.iter().any(|s| s.contains("DDPF_NORMAL")));
    assert!(v.iter().any(|s| s.contains("MIPMAP")));
}

#[test]
fn bc4_and_bc5_decode_layouts() {
    let block4 = [255u8, 0, 0, 0, 0, 0, 0, 0]; // endpoint0 = 255, all indices 0
    let Decoded::Ldr(p) = decode_level(DdsPixelFormat::Bc4, 4, 4, &block4, None).unwrap() else { panic!() };
    assert_eq!(p[0], [255, 255, 255, 255]);
    let mut block5 = [0u8; 16];
    block5[0] = 200;
    block5[8] = 100;
    let Decoded::Ldr(p) = decode_level(DdsPixelFormat::Bc5, 4, 4, &block5, None).unwrap() else { panic!() };
    assert_eq!(p[0], [200, 100, 0, 255]);
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p engine --test dds_read`
Expected: FAIL to compile.

- [ ] **Step 3: Implement the reader (append to `dds.rs`)**

```rust
use std::ops::Range;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum DdsPixelFormat {
    Bc1 { srgb: bool },
    Bc2 { srgb: bool },
    Bc3 { srgb: bool },
    Bc4,
    Bc5,
    Bc6hUf16,
    Bc6hSf16,
    Bc7 { srgb: bool },
    Rgba8 { srgb: bool },
    Bgra8,
    Bgrx8,
    R8,
    Rg8,
    Rgba16f,
    Rgba32f,
}

impl DdsPixelFormat {
    pub fn block_bytes(self) -> Option<usize> {
        use DdsPixelFormat::*;
        match self {
            Bc1 { .. } | Bc4 => Some(8),
            Bc2 { .. } | Bc3 { .. } | Bc5 | Bc6hUf16 | Bc6hSf16 | Bc7 { .. } => Some(16),
            _ => None,
        }
    }
    fn bytes_per_pixel(self) -> usize {
        use DdsPixelFormat::*;
        match self {
            Rgba8 { .. } | Bgra8 | Bgrx8 => 4,
            R8 => 1,
            Rg8 => 2,
            Rgba16f => 8,
            Rgba32f => 16,
            _ => 0,
        }
    }
    pub fn level_size(self, w: u32, h: u32) -> usize {
        match self.block_bytes() {
            Some(b) => w.div_ceil(4) as usize * h.div_ceil(4) as usize * b,
            None => (w * h) as usize * self.bytes_per_pixel(),
        }
    }
    pub fn as_format(self) -> Option<(Format, bool)> {
        use DdsPixelFormat as P;
        Some(match self {
            P::Bc1 { srgb } => (Format::Bc1, srgb),
            P::Bc3 { srgb } => (Format::Bc3, srgb),
            P::Bc4 => (Format::Bc4, false),
            P::Bc5 => (Format::Bc5, false),
            P::Bc6hUf16 => (Format::Bc6hUf16, false),
            P::Bc7 { srgb } => (Format::Bc7, srgb),
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum HeaderKind {
    Legacy,
    Dx10,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct Dx10Fields {
    pub dxgi: u32,
    pub dimension: u32,
    pub misc: u32,
    pub array_size: u32,
    pub misc2: u32,
}

#[derive(Debug, Clone)]
pub struct DdsFile {
    pub width: u32,
    pub height: u32,
    pub mip_count: u32,
    pub format: DdsPixelFormat,
    pub header_kind: HeaderKind,
    pub fourcc: [u8; 4],
    pub dx10: Option<Dx10Fields>,
    pub flags: u32,
    pub pf_flags: u32,
    pub caps: u32,
    pub caps2: u32,
    pub linear_size: u32,
    pub reserved_nonzero: bool,
    pub is_cubemap: bool,
    pub levels: Vec<(u32, u32, Range<usize>)>,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DdsError {
    #[error("not a DDS file")]
    NotDds,
    #[error("truncated DDS file")]
    Truncated,
    #[error("unsupported DDS format: {0}")]
    Unsupported(String),
}

fn from_dxgi(d: u32) -> Option<DdsPixelFormat> {
    use DdsPixelFormat::*;
    Some(match d {
        71 => Bc1 { srgb: false },
        72 => Bc1 { srgb: true },
        74 => Bc2 { srgb: false },
        75 => Bc2 { srgb: true },
        77 => Bc3 { srgb: false },
        78 => Bc3 { srgb: true },
        80 => Bc4,
        83 => Bc5,
        95 => Bc6hUf16,
        96 => Bc6hSf16,
        98 => Bc7 { srgb: false },
        99 => Bc7 { srgb: true },
        28 => Rgba8 { srgb: false },
        29 => Rgba8 { srgb: true },
        87 => Bgra8,
        88 => Bgrx8,
        61 => R8,
        49 => Rg8,
        10 => Rgba16f,
        2 => Rgba32f,
        _ => return None,
    })
}

pub fn read_dds(bytes: &[u8]) -> Result<DdsFile, DdsError> {
    if bytes.len() < 4 || &bytes[..4] != b"DDS " {
        return Err(DdsError::NotDds);
    }
    if bytes.len() < 128 {
        return Err(DdsError::Truncated);
    }
    let u = |i: usize| u32::from_le_bytes(bytes[i..i + 4].try_into().unwrap());
    let (flags, height, width, linear_size, mips) = (u(8), u(12), u(16), u(20), u(28).max(1));
    let reserved_nonzero = bytes[32..76].iter().any(|&b| b != 0);
    let (pf_flags, fourcc) = (u(80), [bytes[84], bytes[85], bytes[86], bytes[87]]);
    let (bits, rm, gm, bm, am) = (u(88), u(92), u(96), u(100), u(104));
    let (caps, caps2) = (u(108), u(112));
    let mut off = 128;
    let mut dx10 = None;
    use DdsPixelFormat::*;
    let format = if pf_flags & DDPF_FOURCC != 0 {
        match &fourcc {
            b"DX10" => {
                if bytes.len() < 148 {
                    return Err(DdsError::Truncated);
                }
                let f = Dx10Fields { dxgi: u(128), dimension: u(132), misc: u(136), array_size: u(140), misc2: u(144) };
                off = 148;
                dx10 = Some(f);
                from_dxgi(f.dxgi).ok_or_else(|| DdsError::Unsupported(format!("DXGI {}", f.dxgi)))?
            }
            b"DXT1" => Bc1 { srgb: false },
            b"DXT2" | b"DXT3" => Bc2 { srgb: false },
            b"DXT4" | b"DXT5" => Bc3 { srgb: false },
            b"ATI1" | b"BC4U" => Bc4,
            b"ATI2" | b"BC5U" => Bc5,
            [113, 0, 0, 0] => Rgba16f,
            [116, 0, 0, 0] => Rgba32f,
            other => return Err(DdsError::Unsupported(format!("FourCC {:?}", String::from_utf8_lossy(other)))),
        }
    } else {
        match (bits, rm, gm, bm, am) {
            (32, 0xff, 0xff00, 0xff_0000, 0xff00_0000) => Rgba8 { srgb: false },
            (32, 0xff_0000, 0xff00, 0xff, 0xff00_0000) => Bgra8,
            (32, 0xff_0000, 0xff00, 0xff, 0) => Bgrx8,
            (8, 0xff, 0, 0, 0) => R8,
            (16, 0xff, 0xff00, 0, 0) => Rg8,
            _ => return Err(DdsError::Unsupported(format!("uncompressed layout {bits} bits, masks {rm:x}/{gm:x}/{bm:x}/{am:x}"))),
        }
    };
    let is_cubemap = caps2 & 0x200 != 0 || dx10.is_some_and(|d| d.misc & 0x4 != 0);
    let mut levels = vec![];
    let (mut w, mut h, mut pos) = (width.max(1), height.max(1), 0usize);
    for _ in 0..mips {
        let size = format.level_size(w, h);
        levels.push((w, h, pos..pos + size));
        pos += size;
        w = (w / 2).max(1);
        h = (h / 2).max(1);
    }
    let faces = if is_cubemap { 6 } else { 1 };
    if bytes.len() < off + pos * faces {
        return Err(DdsError::Truncated);
    }
    Ok(DdsFile {
        width,
        height,
        mip_count: mips,
        format,
        header_kind: if dx10.is_some() { HeaderKind::Dx10 } else { HeaderKind::Legacy },
        fourcc,
        dx10,
        flags,
        pf_flags,
        caps,
        caps2,
        linear_size,
        reserved_nonzero,
        is_cubemap,
        levels,
        data: bytes[off..].to_vec(),
    })
}

/// Spec §5.7 rules for a written (compressed, 2D) file.
pub fn check_header(f: &DdsFile) -> Vec<String> {
    let mut v = vec![];
    let need = DDSD_CAPS | DDSD_HEIGHT | DDSD_WIDTH | DDSD_PIXELFORMAT | DDSD_LINEARSIZE;
    if f.flags & need != need {
        v.push(format!("flags {:#x} lack CAPS|HEIGHT|WIDTH|PIXELFORMAT|LINEARSIZE", f.flags));
    }
    if f.caps & DDSCAPS_TEXTURE == 0 {
        v.push("caps lack TEXTURE".into());
    }
    if f.mip_count > 1 {
        if f.flags & DDSD_MIPMAPCOUNT == 0 {
            v.push("mips present but flags lack MIPMAPCOUNT".into());
        }
        if f.caps & (DDSCAPS_COMPLEX | DDSCAPS_MIPMAP) != DDSCAPS_COMPLEX | DDSCAPS_MIPMAP {
            v.push("mips present but caps lack COMPLEX|MIPMAP".into());
        }
    }
    let full = 32 - f.width.max(f.height).max(1).leading_zeros();
    if f.mip_count > full {
        v.push(format!("mip count {} exceeds {} for {}x{}", f.mip_count, full, f.width, f.height));
    }
    if f.linear_size as usize != f.format.level_size(f.width, f.height) {
        v.push(format!("linear size {} != mip 0 size {}", f.linear_size, f.format.level_size(f.width, f.height)));
    }
    if f.pf_flags & DDPF_NORMAL != 0 {
        v.push("DDPF_NORMAL set (non-standard)".into());
    }
    if f.reserved_nonzero {
        v.push("reserved1 fields are not zero".into());
    }
    if &f.fourcc == b"ATI1" || &f.fourcc == b"ATI2" {
        v.push("ATI1/ATI2 FourCC is never written by sdc".into());
    }
    if let Some(d) = f.dx10 {
        if d.dimension != 3 || d.array_size != 1 || d.misc2 != 0 {
            v.push(format!("DX10 header: dimension {}, array {}, misc2 {} (want 3, 1, 0)", d.dimension, d.array_size, d.misc2));
        }
    }
    v
}
```

- [ ] **Step 4: Implement `decode.rs`**

```rust
//! Decode one DDS level for validation and inspection (bcdec_rs; BC1 optionally via rgbcx at a decoder model).
use crate::dds::{DdsError, DdsPixelFormat};
use encoders_sys::{bc1_decode_block, Bc1Model};

#[derive(Debug, Clone, PartialEq)]
pub enum Decoded {
    Ldr(Vec<[u8; 4]>),
    Hdr(Vec<[f32; 3]>),
}

fn blocks<T: Copy + Default>(w: u32, h: u32, data: &[u8], bs: usize, per: usize, mut dec: impl FnMut(&[u8], &mut [T])) -> Result<Vec<T>, DdsError> {
    let (bw, bh) = (w.div_ceil(4) as usize, h.div_ceil(4) as usize);
    if data.len() < bw * bh * bs {
        return Err(DdsError::Truncated);
    }
    let mut img = vec![T::default(); (w * h) as usize * per];
    let mut tmp = vec![T::default(); 16 * per];
    for by in 0..bh {
        for bx in 0..bw {
            dec(&data[(by * bw + bx) * bs..][..bs], &mut tmp);
            for y in 0..4 {
                for x in 0..4 {
                    let (gx, gy) = (bx * 4 + x, by * 4 + y);
                    if gx < w as usize && gy < h as usize {
                        let o = (gy * w as usize + gx) * per;
                        img[o..o + per].copy_from_slice(&tmp[(y * 4 + x) * per..][..per]);
                    }
                }
            }
        }
    }
    Ok(img)
}

fn quads(v: Vec<u8>) -> Vec<[u8; 4]> {
    v.chunks(4).map(|c| [c[0], c[1], c[2], c[3]]).collect()
}

pub fn decode_level(fmt: DdsPixelFormat, w: u32, h: u32, data: &[u8], bc1_model: Option<Bc1Model>) -> Result<Decoded, DdsError> {
    use DdsPixelFormat::*;
    Ok(match fmt {
        Bc1 { .. } => Decoded::Ldr(quads(blocks(w, h, data, 8, 4, |b, o| match bc1_model {
            Some(m) => {
                for (i, p) in bc1_decode_block(b.try_into().unwrap(), m).iter().enumerate() {
                    o[i * 4..i * 4 + 4].copy_from_slice(p);
                }
            }
            None => bcdec_rs::bc1(b, o, 16),
        })?)),
        Bc2 { .. } => Decoded::Ldr(quads(blocks(w, h, data, 16, 4, |b, o| bcdec_rs::bc2(b, o, 16))?)),
        Bc3 { .. } => Decoded::Ldr(quads(blocks(w, h, data, 16, 4, |b, o| bcdec_rs::bc3(b, o, 16))?)),
        Bc7 { .. } => Decoded::Ldr(quads(blocks(w, h, data, 16, 4, |b, o| bcdec_rs::bc7(b, o, 16))?)),
        Bc4 => {
            let r = blocks(w, h, data, 8, 1, |b, o| bcdec_rs::bc4(b, o, 4, false))?;
            Decoded::Ldr(r.iter().map(|&v| [v, v, v, 255]).collect())
        }
        Bc5 => {
            let rg = blocks(w, h, data, 16, 2, |b, o| bcdec_rs::bc5(b, o, 8, false))?;
            Decoded::Ldr(rg.chunks(2).map(|c| [c[0], c[1], 0, 255]).collect())
        }
        Bc6hUf16 | Bc6hSf16 => {
            let signed = fmt == Bc6hSf16;
            let f = blocks(w, h, data, 16, 3, |b, o| bcdec_rs::bc6h_float(b, o, 12, signed))?;
            Decoded::Hdr(f.chunks(3).map(|c| [c[0], c[1], c[2]]).collect())
        }
        Rgba8 { .. } => Decoded::Ldr(quads(data[..(w * h * 4) as usize].to_vec())),
        Bgra8 | Bgrx8 => Decoded::Ldr(
            data[..(w * h * 4) as usize].chunks(4).map(|c| [c[2], c[1], c[0], if fmt == Bgrx8 { 255 } else { c[3] }]).collect(),
        ),
        R8 => Decoded::Ldr(data[..(w * h) as usize].iter().map(|&v| [v, v, v, 255]).collect()),
        Rg8 => Decoded::Ldr(data[..(w * h * 2) as usize].chunks(2).map(|c| [c[0], c[1], 0, 255]).collect()),
        Rgba16f => Decoded::Hdr(
            data[..(w * h * 8) as usize]
                .chunks(8)
                .map(|c| [0, 1, 2].map(|k| half::f16::from_le_bytes([c[k * 2], c[k * 2 + 1]]).to_f32()))
                .collect(),
        ),
        Rgba32f => Decoded::Hdr(
            data[..(w * h * 16) as usize].chunks(16).map(|c| [0, 1, 2].map(|k| f32::from_le_bytes(c[k * 4..k * 4 + 4].try_into().unwrap()))).collect(),
        ),
    })
}
```
Add `pub mod decode;` to `lib.rs`. The block decoders' 16-texel temp buffers are written by `bcdec_rs` with a row pitch of `4 * per` units (16 bytes for RGBA8, 4 for R8, 8 for RG8, 12 floats for BC6H), matching the pitches passed above.

- [ ] **Step 5: Run the tests**

Run: `cargo test -p engine --test dds_read`
Expected: 4 passed.

- [ ] **Step 6: Commit**

```bash
git add crates/engine
git commit -m "feat(engine): DDS reader (BC1-7, legacy/DX10, common uncompressed, cubemap flag), header check, decode"
```

---
### Task 35: Validate (spec §5.8) including the smallest-mip alpha rules

**Files:**
- Modify: `crates/engine/src/validate/mod.rs`, `crates/engine/src/validate/alpha.rs` (add `JsonSchema` derive to `AlphaStats`)
- Test: `crates/engine/tests/validate.rs`

**Interfaces:**
- Consumes: `dds::{read_dds, check_header}`, `decode::decode_level`, `quantize::EncoderInput`, `coverage::coverage_fraction`, `validate::alpha::*`.
- Produces:
  - `pub struct ValidationInput<'a> { pub bytes: &'a [u8], pub profile: &'a Profile, pub encoder_input: Option<&'a [EncoderInput]>, pub base_coverage: Option<f64>, pub source_peak: Option<f32>, pub bc1_model: Bc1Model, pub expected_levels: Option<u32> }`
  - `pub struct ValidationSummary { pub passed: bool, pub levels: u32, pub min_mip_psnr_db: Option<f64>, pub alpha: Vec<AlphaStats>, pub coverage: Vec<f64>, pub bc6h_peak_ratio: Option<f64> }` (serde, JsonSchema, Default)
  - `pub struct ValidationReport { pub summary: ValidationSummary, pub warnings: Vec<Warning> }`
  - `pub fn validate(inp: &ValidationInput) -> ValidationReport` — `passed` = no `Error`-severity warning

Rules (per level unless stated):

| Check | Rule | Code |
|---|---|---|
| Header | `read_dds` succeeds; `check_header` empty; format/tag equal the profile; mip count equals `expected_levels` | `VAL-HEADER` (error) |
| Per-mip error | with `encoder_input`: PSNR of the decoded level against the encoder input on the format's channels (BC4: R; BC5: RG; BC1: RGB; BC3/BC7: RGB, plus A when alpha is present) below `validation.min_mip_psnr_db` | `VAL-MIP-ERROR` (warning) |
| `remove` guarantee | BC7 with `exact_opaque` off: every decoded alpha ≥ 254 (and `ALPHA-254` info with the 254 share at mip 0 and the smallest mip); BC7 with `exact_opaque` on, and BC1: every decoded alpha = 255 | `VAL-ALPHA-GUARANTEE` (error), `ALPHA-254` (info) |
| Exact opaque blocks | BC7 with `exact_opaque`, any policy, with `encoder_input`: `opaque_block_violations == 0` | `VAL-ALPHA-GUARANTEE` |
| Coverage | coverage enabled and `base_coverage` given: levels with ≥ 256 texels (16×16) within `coverage_tolerance_pts` points of the base | `VAL-COVERAGE` |
| Complex `_m` | smallest-mip decoded alpha ≥ `complex_m_warn_alpha` (247); base R≈G≈B (±4) on > 95 % with mean > 25/255; > 25 % of base alpha ≤ 4 or ≥ 251 | `VAL-M-COMPLEX-MARGIN`, `VAL-M-GRAYSCALE`, `VAL-M-HEIGHT-CLIP` |
| PGPatcher | `envmask` storing alpha: base share α ≠ 255 ≥ `pgpatcher_non_white_share` | `VAL-PGPATCHER-50` |
| BC6H peak | decoded ÷ source peak luminance < `bc6h_min_peak_ratio` | `VAL-BC6H-PEAK` |

The 256-texel floor for the coverage check is this plan's reading of "coverage within 2 points of mip 0": a 1×1 level is 0 % or 100 %, and at 8×8 a single texel flipped by a one-code BC7 alpha error is already 1.6 points, because the solver puts the boundary texel exactly on the lowest passing code. At 16×16 one flip is 0.4 points. Recorded in the report as an ambiguity.

- [ ] **Step 1: Write the failing tests**

`crates/engine/tests/validate.rs`:
```rust
use engine::codes::Code;
use engine::config::*;
use engine::dds::*;
use engine::encode::{encode_level, EncodeCtx};
use engine::presets::PresetRegistry;
use engine::quantize::EncoderInput;
use engine::validate::*;
use encoders_sys::Bc1Model;

fn profile(st: Subtype) -> Profile {
    PresetRegistry::builtin().unwrap().profile("cs-quality", Target::Cs, st).unwrap()
}

/// Encode explicit levels (largest first) with the engine and wrap them in a DDS.
fn dds(p: &Profile, levels: &[(u32, u32, Vec<[u8; 4]>)]) -> (Vec<u8>, Vec<EncoderInput>) {
    let inputs: Vec<EncoderInput> = levels.iter().map(|(_, _, px)| EncoderInput::Ldr(px.clone())).collect();
    let enc: Vec<Vec<u8>> = levels.iter().zip(&inputs).map(|((w, h, _), i)| encode_level(i, *w, *h, p, &EncodeCtx::default()).unwrap()).collect();
    let (w, h, _) = &levels[0];
    let desc = DdsDesc { width: *w, height: *h, format: p.format, srgb: p.srgb_tag(), header: resolve_header(p.format, p.srgb_tag(), p.header) };
    (dds_bytes(&desc, &enc), inputs)
}

fn codes(r: &ValidationReport) -> Vec<Code> {
    r.warnings.iter().map(|w| w.code).collect()
}

fn mode6_p0_payload(blocks: usize) -> Vec<u8> {
    let mut v: u128 = 1 << 6;
    for i in 0..8 {
        v |= (if i >= 6 { 127u128 } else { 10 }) << (7 + 7 * i);
    }
    v.to_le_bytes().repeat(blocks)
}

#[test]
fn clean_bc7_passes_and_header_matches() {
    let p = profile(Subtype::Diffuse);
    let px: Vec<[u8; 4]> = (0..64).map(|i| [i as u8 * 4, 100, 200 - i as u8, 255]).collect();
    let (b, inputs) = dds(&p, &[(8, 8, px), (4, 4, vec![[90, 100, 150, 255]; 16]), (2, 2, vec![[90, 100, 150, 255]; 4]), (1, 1, vec![[90, 100, 150, 255]])]);
    let r = validate(&ValidationInput { bytes: &b, profile: &p, encoder_input: Some(&inputs), base_coverage: None, source_peak: None, bc1_model: Bc1Model::Ideal, expected_levels: Some(4) });
    assert!(r.summary.passed, "{:?}", r.warnings);
    assert_eq!(r.summary.levels, 4);
    assert_eq!(r.summary.alpha.len(), 4);
}

#[test]
fn alpha_254_is_info_with_flag_off_and_an_error_with_flag_on() {
    let off = profile(Subtype::Diffuse);
    let mut on = off.clone();
    on.alpha.exact_opaque = true;
    let desc = DdsDesc { width: 4, height: 4, format: Format::Bc7, srgb: false, header: ResolvedHeader::Dx10 };
    let b = dds_bytes(&desc, &[mode6_p0_payload(1)]);
    let input = |p: &Profile| validate(&ValidationInput { bytes: &b, profile: p, encoder_input: None, base_coverage: None, source_peak: None, bc1_model: Bc1Model::Ideal, expected_levels: None });
    let r = input(&off);
    assert!(r.summary.passed);
    assert!(codes(&r).contains(&Code::Alpha254));
    let r = input(&on);
    assert!(!r.summary.passed);
    assert!(codes(&r).contains(&Code::ValAlphaGuarantee));
}

#[test]
fn header_and_format_mismatch_are_errors() {
    let p = profile(Subtype::PbrAlbedo); // expects BC7_UNORM_SRGB
    let desc = DdsDesc { width: 4, height: 4, format: Format::Bc7, srgb: false, header: ResolvedHeader::Dx10 };
    let b = dds_bytes(&desc, &[vec![0x20; 16]]);
    let r = validate(&ValidationInput { bytes: &b, profile: &p, encoder_input: None, base_coverage: None, source_peak: None, bc1_model: Bc1Model::Ideal, expected_levels: Some(1) });
    assert!(codes(&r).contains(&Code::ValHeader));
    assert!(!r.summary.passed);
}

#[test]
fn coverage_drift_and_complex_m_margin() {
    let mut p = profile(Subtype::Diffuse);
    p.alpha.policy = AlphaPolicy::Cutout;
    let half: Vec<[u8; 4]> = (0..1024).map(|i| [128, 128, 128, if i % 32 < 16 { 255 } else { 0 }]).collect();
    let (b, inputs) = dds(&p, &[(32, 32, half), (16, 16, vec![[128, 128, 128, 0]; 256])]);
    let r = validate(&ValidationInput { bytes: &b, profile: &p, encoder_input: Some(&inputs), base_coverage: Some(0.5), source_peak: None, bc1_model: Bc1Model::Ideal, expected_levels: Some(2) });
    assert!(codes(&r).contains(&Code::ValCoverage));

    let p = profile(Subtype::ComplexMaterial);
    let lv = |n: u32| (n, n, vec![[80u8, 160, 40, 250]; (n * n) as usize]);
    let (b, inputs) = dds(&p, &[lv(4), lv(2), lv(1)]);
    let r = validate(&ValidationInput { bytes: &b, profile: &p, encoder_input: Some(&inputs), base_coverage: None, source_peak: None, bc1_model: Bc1Model::Ideal, expected_levels: Some(3) });
    assert!(codes(&r).contains(&Code::ValMComplexMargin), "{:?}", r.warnings);
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p engine --test validate`
Expected: FAIL to compile.

- [ ] **Step 3: Implement**

In `validate/alpha.rs` change the `AlphaStats` derive to include `schemars::JsonSchema`.

`crates/engine/src/validate/mod.rs`:
```rust
//! Validation (spec §5.8): re-read, decode every mip, check.
pub mod alpha;

use crate::codes::{Code, Severity, Warning};
use crate::config::*;
use crate::coverage::coverage_fraction;
use crate::dds::{check_header, read_dds};
use crate::decode::{decode_level, Decoded};
use crate::quantize::EncoderInput;
use alpha::{alpha_stats, opaque_block_violations, AlphaStats};
use encoders_sys::Bc1Model;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

pub struct ValidationInput<'a> {
    pub bytes: &'a [u8],
    pub profile: &'a Profile,
    pub encoder_input: Option<&'a [EncoderInput]>,
    pub base_coverage: Option<f64>,
    pub source_peak: Option<f32>,
    pub bc1_model: Bc1Model,
    pub expected_levels: Option<u32>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ValidationSummary {
    pub passed: bool,
    pub levels: u32,
    pub min_mip_psnr_db: Option<f64>,
    pub alpha: Vec<AlphaStats>,
    pub coverage: Vec<f64>,
    pub bc6h_peak_ratio: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ValidationReport {
    pub summary: ValidationSummary,
    pub warnings: Vec<Warning>,
}

fn psnr(a: &[[u8; 4]], b: &[[u8; 4]], ch: &[usize]) -> f64 {
    let mut s = 0.0f64;
    for (x, y) in a.iter().zip(b) {
        for &c in ch {
            let e = x[c] as f64 - y[c] as f64;
            s += e * e;
        }
    }
    let mse = s / (a.len() * ch.len()).max(1) as f64;
    if mse <= 1e-12 { 99.0 } else { 10.0 * (65025.0 / mse).log10() }
}

fn finish(mut summary: ValidationSummary, warnings: Vec<Warning>) -> ValidationReport {
    summary.passed = !warnings.iter().any(|w| w.severity == Severity::Error);
    ValidationReport { summary, warnings }
}

pub fn validate(inp: &ValidationInput) -> ValidationReport {
    let p = inp.profile;
    let v = &p.validation;
    let mut s = ValidationSummary::default();
    let mut w: Vec<Warning> = vec![];
    let file = match read_dds(inp.bytes) {
        Ok(f) => f,
        Err(e) => {
            w.push(Warning::new(Code::ValHeader, e.to_string()));
            return finish(s, w);
        }
    };
    for msg in check_header(&file) {
        w.push(Warning::new(Code::ValHeader, msg));
    }
    if file.format.as_format() != Some((p.format, p.srgb_tag())) {
        w.push(Warning::new(Code::ValHeader, format!("file format {:?} != expected {:?} (srgb {})", file.format, p.format, p.srgb_tag())));
    }
    if inp.expected_levels.is_some_and(|n| n != file.mip_count) {
        w.push(Warning::new(Code::ValHeader, format!("mip count {} != expected {:?}", file.mip_count, inp.expected_levels)));
    }
    s.levels = file.mip_count;
    let model = matches!(p.format, Format::Bc1).then_some(inp.bc1_model);
    let mut decoded = vec![];
    for (lw, lh, range) in &file.levels {
        match decode_level(file.format, *lw, *lh, &file.data[range.clone()], model) {
            Ok(d) => decoded.push((*lw, *lh, d)),
            Err(e) => {
                w.push(Warning::new(Code::ValHeader, format!("decode failed: {e}")));
                return finish(s, w);
            }
        }
    }
    let alpha_present = p.alpha.policy != AlphaPolicy::Remove;
    // per-mip error and exact-opaque blocks
    if let Some(enc) = inp.encoder_input {
        let ch: Vec<usize> = match p.format {
            Format::Bc4 => vec![0],
            Format::Bc5 => vec![0, 1],
            Format::Bc1 => vec![0, 1, 2],
            _ if alpha_present => vec![0, 1, 2, 3],
            _ => vec![0, 1, 2],
        };
        for (l, ((lw, lh, d), e)) in decoded.iter().zip(enc).enumerate() {
            if let (Decoded::Ldr(d), EncoderInput::Ldr(e)) = (d, e) {
                let q = psnr(e, d, &ch);
                s.min_mip_psnr_db = Some(s.min_mip_psnr_db.map_or(q, |m| m.min(q)));
                if q < v.min_mip_psnr_db {
                    w.push(Warning::new(Code::ValMipError, format!("mip {l}: {q:.2} dB < {:.1} dB", v.min_mip_psnr_db)).with_data(json!({"level": l, "psnr": q})));
                }
                if p.format == Format::Bc7 && p.alpha.exact_opaque {
                    let bad = opaque_block_violations(e, d, *lw, *lh);
                    if bad > 0 {
                        w.push(Warning::new(Code::ValAlphaGuarantee, format!("mip {l}: {bad} texels of all-255 blocks decode below 255")));
                    }
                }
            }
        }
    }
    if p.format.has_alpha() {
        let ldr: Vec<(u32, u32, &Vec<[u8; 4]>)> = decoded.iter().filter_map(|(a, b, d)| if let Decoded::Ldr(px) = d { Some((*a, *b, px)) } else { None }).collect();
        s.alpha = ldr.iter().map(|(_, _, px)| alpha_stats(px)).collect();
        if p.alpha.policy == AlphaPolicy::Remove {
            let guarantee = if p.format == Format::Bc7 && !p.alpha.exact_opaque { 254 } else { 255 };
            for (l, st) in s.alpha.iter().enumerate() {
                if st.min < guarantee {
                    w.push(Warning::new(Code::ValAlphaGuarantee, format!("mip {l}: decoded alpha {} below the guarantee {guarantee}", st.min)));
                }
            }
            if guarantee == 254 && s.alpha.iter().any(|a| a.eq254 > 0) {
                let (first, last) = (s.alpha[0], *s.alpha.last().unwrap());
                w.push(Warning::new(Code::Alpha254, format!("decoded alpha 254 on {:.1}% of mip 0 and {:.1}% of the smallest mip", first.share_254() * 100.0, last.share_254() * 100.0))
                    .with_data(json!({"mip0_share": first.share_254(), "smallest_share": last.share_254()})));
            }
        }
        if p.coverage_enabled() {
            if let Some(base) = inp.base_coverage {
                for (l, (lw, lh, px)) in ldr.iter().enumerate() {
                    let c = coverage_fraction(px.iter().map(|q| q[3] as f32 / 255.0), p.alpha.threshold, p.alpha.compare);
                    s.coverage.push(c);
                    if (lw * lh) >= 256 && ((c - base).abs() * 100.0) > v.coverage_tolerance_pts {
                        w.push(Warning::new(Code::ValCoverage, format!("mip {l}: coverage {:.1}% vs base {:.1}%", c * 100.0, base * 100.0)));
                    }
                }
            }
        }
        if p.subtype == Subtype::ComplexMaterial {
            if let Some((_, _, px)) = ldr.last() {
                let a = px.iter().map(|q| q[3]).max().unwrap_or(0);
                if a >= v.complex_m_warn_alpha {
                    w.push(Warning::new(Code::ValMComplexMargin, format!("smallest-mip alpha {a}/255 is within the 4-code margin of CS's 251/255 complex gate")));
                }
            }
            if let Some((_, _, px)) = ldr.first() {
                let n = px.len().max(1) as f64;
                let gray = px.iter().filter(|q| (q[0] as i32 - q[1] as i32).abs() <= 4 && (q[1] as i32 - q[2] as i32).abs() <= 4).count() as f64 / n;
                let mean = px.iter().map(|q| (q[0] as f64 + q[1] as f64 + q[2] as f64) / 3.0).sum::<f64>() / n;
                if gray > 0.95 && mean > 25.0 {
                    w.push(Warning::new(Code::ValMGrayscale, "complex _m with R≈G≈B: CS rejects it".to_string()));
                }
                let clip = px.iter().filter(|q| q[3] <= 4 || q[3] >= 251).count() as f64 / n;
                if clip > 0.25 {
                    w.push(Warning::new(Code::ValMHeightClip, format!("{:.0}% of height alpha is ≤4 or ≥251", clip * 100.0)));
                }
            }
        }
        if p.subtype == Subtype::Envmask && alpha_present {
            if let Some(a0) = s.alpha.first() {
                if a0.share_not_255() >= v.pgpatcher_non_white_share {
                    w.push(Warning::new(Code::ValPgpatcher50, format!("{:.0}% of alpha is not 255: PGPatcher treats this _m as complex", a0.share_not_255() * 100.0)));
                }
            }
        }
    }
    if p.format == Format::Bc6hUf16 {
        if let (Some(peak), Some((_, _, Decoded::Hdr(px)))) = (inp.source_peak, decoded.first()) {
            let dec_peak = px.iter().map(|c| 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]).fold(0.0f32, f32::max);
            if peak > 0.0 {
                let r = (dec_peak / peak) as f64;
                s.bc6h_peak_ratio = Some(r);
                if r < v.bc6h_min_peak_ratio {
                    w.push(Warning::new(Code::ValBc6hPeak, format!("BC6H keeps {:.0}% of the source peak", r * 100.0)));
                }
            }
        }
    }
    finish(s, w)
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p engine --test validate`
Expected: 4 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/engine
git commit -m "feat(engine): validation - header, per-mip error, decoded-alpha guarantees, coverage, _m and BC6H checks"
```

---

### Task 36: Events (JSON Lines) and the batch manifest

**Files:**
- Create: `crates/engine/src/events.rs`, `crates/engine/src/manifest.rs`, `crates/engine/build.rs`
- Modify: `crates/engine/src/lib.rs`, `crates/engine/Cargo.toml` (`build = "build.rs"`)
- Test: `crates/engine/tests/events_manifest.rs`

**Interfaces:**
- Produces:
  - `events::EVENT_SCHEMA: u32 = 1`
  - `events::Stage { Discover, Group, Load, Analyse, Resolve, Preprocess, Mipmaps, Compress, Write, Validate }` (snake_case)
  - `events::EventKind` (serde `tag = "kind"`, snake_case): `BatchStarted { target: Target, preset: String, inputs: usize }`, `FileDiscovered { output: String }`, `SetResolved { members: Vec<String>, pbr: bool, bake: bool }`, `FilePlanned { job: serde_json::Value }`, `FileStarted`, `Progress { stage: Stage, done: u32, total: u32 }`, `Warning(crate::codes::Warning)`, `FileDone { output: String, validation: ValidationSummary }`, `FileSkipped { code: Option<Code>, reason: String }`, `FileFailed { stage: Stage, error: String }`, `BatchDone { converted: u32, up_to_date: u32, skipped: u32, failed: u32, validation_failed: u32, cancelled: Vec<String> }`
  - `events::EngineEvent { pub schema: u32, pub seq: u64, pub ts: String, pub batch: String, pub set: Option<String>, pub path: Option<String>, #[serde(flatten)] pub kind: EventKind }` (Serialize, Deserialize, JsonSchema)
  - `events::EventSink` trait: `fn emit(&self, ev: &EngineEvent)` (`Send + Sync`); `events::VecSink` (`Default`, `fn take(&self) -> Vec<EngineEvent>`); `events::JsonLinesSink<W: Write + Send>` (`fn new(w: W) -> Self`); `events::NullSink`
  - `events::Reporter { pub batch: String, … }` with `fn new(sink: Arc<dyn EventSink>, batch: impl Into<String>) -> Self`, `fn emit(&self, set: Option<&str>, path: Option<&Path>, kind: EventKind)`; `events::new_batch_id() -> String`
  - `manifest::EncoderVersions { bc7enc_rdo, ispctc, ispc: String, bc7e_patch: Option<String> }` with `fn current() -> Self`
  - `manifest::PresetRef { id: String, version: String, sha256: String }`
  - `manifest::ManifestRecord { schema: u32, ts: String, engine_version: String, engine_commit: String, encoders: EncoderVersions, isa: String, preset: PresetRef, profile: Profile, profile_sha256: String, provenance: BTreeMap<String, Provenance>, source: String, source_sha256: String, output: String, validation: ValidationSummary }`
  - `manifest::Manifest` with `fn at(output_root: &Path) -> Self`, `fn path(&self) -> &Path`, `fn append(&self, r: &ManifestRecord) -> io::Result<()>`, `fn load(&self) -> io::Result<BTreeMap<String, ManifestRecord>>` (key = `output`, last record wins, malformed lines skipped)
  - `manifest::{sha256_file(path: &Path) -> io::Result<String>, sha256_json<T: Serialize>(v: &T) -> String, dispatched_isa() -> &'static str, engine_commit() -> &'static str, ENGINE_VERSION: &str}`

- [ ] **Step 1: Write the failing tests**

`crates/engine/tests/events_manifest.rs`:
```rust
use engine::codes::{Code, Warning};
use engine::config::*;
use engine::events::*;
use engine::manifest::*;
use engine::presets::PresetRegistry;
use std::sync::Arc;

#[test]
fn events_serialise_as_flat_json_lines_with_seq() {
    let sink = Arc::new(VecSink::default());
    let rep = Reporter::new(sink.clone(), "b1");
    rep.emit(None, None, EventKind::BatchStarted { target: Target::Cs, preset: "cs-quality".into(), inputs: 2 });
    rep.emit(Some("set"), Some(std::path::Path::new("a/x.png")), EventKind::Warning(Warning::new(Code::DimNpot, "npot")));
    rep.emit(None, None, EventKind::FileStarted);
    let ev = sink.take();
    assert_eq!(ev.iter().map(|e| e.seq).collect::<Vec<_>>(), [0, 1, 2]);
    let line = serde_json::to_string(&ev[1]).unwrap();
    assert!(line.contains("\"kind\":\"warning\"") && line.contains("\"code\":\"DIM-NPOT\"") && line.contains("\"schema\":1"), "{line}");
    assert!(line.contains("\"path\":\"a/x.png\""));
    let back: EngineEvent = serde_json::from_str(&line).unwrap();
    assert_eq!(back, ev[1]);

    let mut buf = Vec::new();
    {
        let jl = JsonLinesSink::new(&mut buf);
        jl.emit(&ev[0]);
        jl.emit(&ev[2]);
    }
    let text = String::from_utf8(buf).unwrap();
    assert_eq!(text.lines().count(), 2);
    assert!(text.lines().nth(1).unwrap().contains("\"kind\":\"file_started\""));
}

#[test]
fn manifest_appends_and_last_record_wins() {
    let d = tempfile::tempdir().unwrap();
    let m = Manifest::at(d.path());
    let profile = PresetRegistry::builtin().unwrap().profile("cs-quality", Target::Cs, Subtype::Normal).unwrap();
    let mut r = ManifestRecord {
        schema: 1,
        ts: "t".into(),
        engine_version: ENGINE_VERSION.into(),
        engine_commit: engine_commit().into(),
        encoders: EncoderVersions::current(),
        isa: dispatched_isa().into(),
        preset: PresetRef { id: "cs-quality".into(), version: "1.0.0".into(), sha256: "x".into() },
        profile_sha256: sha256_json(&profile),
        profile,
        provenance: Default::default(),
        source: "in/x_n.png".into(),
        source_sha256: "aa".into(),
        output: "x_n.dds".into(),
        validation: Default::default(),
    };
    m.append(&r).unwrap();
    r.source_sha256 = "bb".into();
    m.append(&r).unwrap();
    std::fs::write(m.path(), std::fs::read_to_string(m.path()).unwrap() + "not json\n").unwrap();
    let all = m.load().unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all["x_n.dds"].source_sha256, "bb");
    assert!(m.path().ends_with("_sdc/manifest.jsonl"));
    assert!(["sse2", "sse4", "avx", "avx2"].contains(&dispatched_isa()));
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p engine --test events_manifest`
Expected: FAIL to compile.

- [ ] **Step 3: Implement**

`crates/engine/build.rs`:
```rust
fn main() {
    let commit = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown".into());
    println!("cargo::rustc-env=SDC_GIT_COMMIT={commit}");
    println!("cargo::rerun-if-changed=../../.git/HEAD");
    println!("cargo::rerun-if-changed=../../.git/refs/heads");
    println!("cargo::rerun-if-changed=../../.git/packed-refs");
}
```
Add `build = "build.rs"` under `[package]` in `crates/engine/Cargo.toml`.

`crates/engine/src/events.rs`:
```rust
//! Engine events (spec §5.9). The CLI writes them as JSON Lines; the GUI receives the same values.
use crate::codes::Code;
use crate::config::Target;
use crate::validate::ValidationSummary;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

pub const EVENT_SCHEMA: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Discover,
    Group,
    Load,
    Analyse,
    Resolve,
    Preprocess,
    Mipmaps,
    Compress,
    Write,
    Validate,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EventKind {
    BatchStarted { target: Target, preset: String, inputs: usize },
    FileDiscovered { output: String },
    SetResolved { members: Vec<String>, pbr: bool, bake: bool },
    FilePlanned { job: serde_json::Value },
    FileStarted,
    Progress { stage: Stage, done: u32, total: u32 },
    Warning(crate::codes::Warning),
    FileDone { output: String, validation: ValidationSummary },
    FileSkipped { code: Option<Code>, reason: String },
    FileFailed { stage: Stage, error: String },
    BatchDone { converted: u32, up_to_date: u32, skipped: u32, failed: u32, validation_failed: u32, cancelled: Vec<String> },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EngineEvent {
    pub schema: u32,
    pub seq: u64,
    pub ts: String,
    pub batch: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub set: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(flatten)]
    pub kind: EventKind,
}

pub trait EventSink: Send + Sync {
    fn emit(&self, ev: &EngineEvent);
}

#[derive(Default)]
pub struct VecSink(Mutex<Vec<EngineEvent>>);

impl VecSink {
    pub fn take(&self) -> Vec<EngineEvent> {
        std::mem::take(&mut *self.0.lock().unwrap())
    }
}

impl EventSink for VecSink {
    fn emit(&self, ev: &EngineEvent) {
        self.0.lock().unwrap().push(ev.clone());
    }
}

pub struct NullSink;

impl EventSink for NullSink {
    fn emit(&self, _: &EngineEvent) {}
}

pub struct JsonLinesSink<W: Write + Send>(Mutex<W>);

impl<W: Write + Send> JsonLinesSink<W> {
    pub fn new(w: W) -> Self {
        JsonLinesSink(Mutex::new(w))
    }
}

impl<W: Write + Send> EventSink for JsonLinesSink<W> {
    fn emit(&self, ev: &EngineEvent) {
        let mut w = self.0.lock().unwrap();
        let _ = serde_json::to_writer(&mut *w, ev);
        let _ = w.write_all(b"\n");
        let _ = w.flush();
    }
}

pub struct Reporter {
    sink: Arc<dyn EventSink>,
    seq: AtomicU64,
    /// Held across numbering and delivery so `seq` order equals delivery order even with parallel files.
    order: Mutex<()>,
    pub batch: String,
}

impl Reporter {
    pub fn new(sink: Arc<dyn EventSink>, batch: impl Into<String>) -> Self {
        Reporter { sink, seq: AtomicU64::new(0), order: Mutex::new(()), batch: batch.into() }
    }

    pub fn emit(&self, set: Option<&str>, path: Option<&Path>, kind: EventKind) {
        let _order = self.order.lock().unwrap();
        let ev = EngineEvent {
            schema: EVENT_SCHEMA,
            seq: self.seq.fetch_add(1, Ordering::SeqCst),
            ts: humantime::format_rfc3339_millis(std::time::SystemTime::now()).to_string(),
            batch: self.batch.clone(),
            set: set.map(str::to_string),
            path: path.map(|p| p.to_string_lossy().replace('\\', "/")),
            kind,
        };
        self.sink.emit(&ev);
    }
}

pub fn new_batch_id() -> String {
    let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    format!("{ms:x}-{:x}", std::process::id())
}
```
(Events from parallel files interleave, but the `order` lock makes `seq` strictly increasing in delivery order, so JSON Lines order always equals `seq` order.)

`crates/engine/src/manifest.rs`:
```rust
//! Batch manifest (spec §3.6): one JSON Lines file per output root, last record per output wins.
use crate::config::Profile;
use crate::presets::Provenance;
use crate::validate::ValidationSummary;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub const ENGINE_VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn engine_commit() -> &'static str {
    env!("SDC_GIT_COMMIT")
}

/// The ISA the ISPC dispatcher picks on this CPU (targets sse2, sse4, avx, avx2), mirrored with Rust feature detection.
pub fn dispatched_isa() -> &'static str {
    if std::arch::is_x86_feature_detected!("avx2") {
        "avx2"
    } else if std::arch::is_x86_feature_detected!("avx") {
        "avx"
    } else if std::arch::is_x86_feature_detected!("sse4.1") {
        "sse4"
    } else {
        "sse2"
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EncoderVersions {
    pub bc7enc_rdo: String,
    pub ispctc: String,
    pub ispc: String,
    pub bc7e_patch: Option<String>,
}

impl EncoderVersions {
    pub fn current() -> Self {
        let v = encoders_sys::versions();
        EncoderVersions { bc7enc_rdo: v.bc7enc_rdo.into(), ispctc: v.ispctc.into(), ispc: v.ispc.into(), bc7e_patch: v.bc7e_patch.map(String::from) }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PresetRef {
    pub id: String,
    pub version: String,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ManifestRecord {
    pub schema: u32,
    pub ts: String,
    pub engine_version: String,
    pub engine_commit: String,
    pub encoders: EncoderVersions,
    pub isa: String,
    pub preset: PresetRef,
    pub profile: Profile,
    pub profile_sha256: String,
    pub provenance: BTreeMap<String, Provenance>,
    pub source: String,
    pub source_sha256: String,
    pub output: String,
    pub validation: ValidationSummary,
}

pub struct Manifest {
    path: PathBuf,
    lock: Mutex<()>,
}

impl Manifest {
    pub fn at(output_root: &Path) -> Self {
        Manifest { path: output_root.join("_sdc").join("manifest.jsonl"), lock: Mutex::new(()) }
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn append(&self, r: &ManifestRecord) -> io::Result<()> {
        let _g = self.lock.lock().unwrap();
        std::fs::create_dir_all(self.path.parent().unwrap())?;
        let mut f = std::fs::OpenOptions::new().create(true).append(true).open(&self.path)?;
        let mut line = serde_json::to_vec(r).map_err(io::Error::other)?;
        line.push(b'\n');
        f.write_all(&line)
    }
    pub fn load(&self) -> io::Result<BTreeMap<String, ManifestRecord>> {
        let mut map = BTreeMap::new();
        let f = match std::fs::File::open(&self.path) {
            Ok(f) => f,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(map),
            Err(e) => return Err(e),
        };
        for line in io::BufReader::new(f).lines() {
            if let Ok(r) = serde_json::from_str::<ManifestRecord>(&line?) {
                map.insert(r.output.clone(), r);
            }
        }
        Ok(map)
    }
}

pub fn sha256_file(path: &Path) -> io::Result<String> {
    Ok(hex::encode(Sha256::digest(std::fs::read(path)?)))
}

pub fn sha256_json<T: Serialize>(v: &T) -> String {
    hex::encode(Sha256::digest(serde_json::to_vec(v).unwrap_or_default()))
}
```
Add `pub mod events; pub mod manifest;` to `lib.rs`.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p engine --test events_manifest`
Expected: 2 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/engine
git commit -m "feat(engine): JSON Lines events with sinks and the batch manifest"
```

---

### Task 37: Engine API, per-file pipeline and the batch scheduler

**Files:**
- Create: `crates/engine/src/pipeline.rs`, `crates/engine/src/api.rs`, `crates/engine/src/sysmem.rs`
- Modify: `crates/engine/src/lib.rs`
- Test: `crates/engine/tests/api.rs`

**Interfaces:**
- Consumes: everything in Phase 1 so far.
- Produces:
  - `api::OnExisting { IfChanged, Skip, Overwrite }` (kebab-case serde, default `IfChanged`)
  - `api::BatchRequest { pub inputs: Vec<PathBuf>, pub output_root: PathBuf, pub target: Target, pub preset: Option<String>, pub preset_files: Vec<PathBuf>, pub user_config: Option<PathBuf>, pub project_config: Option<PathBuf>, pub overrides: Vec<SetOverride>, pub accept: AcceptSuggestions, pub allow: BTreeSet<Code>, pub recursive: bool, pub on_existing: OnExisting, pub jobs: usize, pub mem_budget: u64, pub bc1_model: Bc1Model, pub sidecar: bool, pub dry_run: bool }` with `fn new(inputs: Vec<PathBuf>, output_root: PathBuf, target: Target) -> Self` (defaults: preset None → `default_preset_for(target)`, accept None, jobs 0 = all cores, mem_budget 0 = 50 % of RAM, `Bc1Model::Ideal`, `IfChanged`)
  - `api::SkippedFile { pub path: PathBuf, pub code: Option<Code>, pub reason: String }`; `api::FailedFile { pub path: PathBuf, pub stage: Stage, pub error: String }`
  - `api::Plan { pub request: BatchRequest, pub registry: PresetRegistry, pub preset_id: String, pub jobs: Vec<ResolvedJob>, pub skipped: Vec<SkippedFile>, pub failed: Vec<FailedFile> }`
  - `api::BatchSummary { pub converted: u32, pub up_to_date: u32, pub skipped: u32, pub failed: u32, pub validation_failed: u32, pub cancelled: Vec<PathBuf> }` with `fn exit_code(&self) -> i32` (1 partial > 4 validation > 0; up-to-date files do not count as skipped)
  - `api::CancelToken` (`Clone`, `Default`) with `fn cancel(&self)`, `fn is_cancelled(&self) -> bool`, `fn flag(&self) -> Arc<AtomicBool>`
  - `api::EngineError { Usage(String), Fatal(String) }` with `fn exit_code(&self) -> i32` (2 / 3)
  - `api::Engine` with `fn new() -> Result<Self, EngineError>`, `fn registry(&self) -> &PresetRegistry`, `fn plan(&self, req: BatchRequest, rep: &Reporter) -> Result<Plan, EngineError>`, `fn run(&self, plan: Plan, rep: &Reporter, cancel: &CancelToken) -> Result<BatchSummary, EngineError>`
  - `pipeline::FileError { Cancelled, Failed(Stage, String) }`; `pipeline::FileOutcome { pub output: PathBuf, pub report: ValidationReport, pub record: ManifestRecord }`; `pipeline::process_file(job: &ResolvedJob, req: &BatchRequest, preset: &PresetRef, cancel: &CancelToken, rep: &Reporter) -> Result<FileOutcome, FileError>`
  - `sysmem::total_ram() -> u64`

Behaviour (spec §5.9, §8): `plan` validates the preset and every `--set` value up front (bad ones → `Usage`), loads configs (`Usage` on parse errors; the project config defaults to `sdc.toml` in the first input folder), discovers (`Usage` if an input does not exist), groups, skips `IN-DUPLICATE` pairs and DDS inputs (`IN-DDS-SKIPPED`), loads and analyses each image in parallel (load errors become `FailedFile` at `Load`, with `DIM-TOO-LARGE` or `IN-DECODE`), resolves, and emits `batch_started`, `file_discovered`, `set_resolved`, `file_planned` and every warning. `run` stops after reporting when `dry_run`; otherwise it initialises the encoders with the batch BC1 model (`Fatal` on mismatch), applies `on_existing` against the manifest (`if-changed`: re-encode when source hash, profile hash or engine version differ; an existing output with no record is kept with `OUT-UNKNOWN-EXISTING`), admits files by estimated bytes (`w·h·16·1.34·3`) against the memory budget, processes them on a rayon pool of `jobs` threads under `catch_unwind` (admission happens on the calling thread via `ThreadPool::in_place_scope`, never inside a pool thread, to avoid budget deadlocks with nested band parallelism), checks cancellation between bands and before each file, writes the DDS atomically, re-reads and validates it, appends a manifest record (and a `<name>.dds.json` sidecar with `--sidecar`), and emits `file_done`/`file_skipped`/`file_failed` and `batch_done`. A job with `JobStatus::PackingInput` (Task 27a) is reported as `file_skipped` with `IN-PACKING-INPUT` but not counted in `BatchSummary::skipped`, so it does not make the batch partial.

Suffix aliases (Task 26a, spec §3.7): `plan` checks every alias preset's chain for every sub-type up front (usage error otherwise), canonicalises output names right after `discover` (so `file_discovered` shows the canonical output and alias/canonical collisions become `IN-DUPLICATE`), groups with `group_sets_with`, passes the canonical suffix and the alias to `FileFacts`, and `run` writes the alias preset's id, version and SHA-256 into the manifest `PresetRef` of an aliased file.

- [ ] **Step 1: Write the failing tests**

`crates/engine/tests/api.rs`:
```rust
use engine::api::*;
use engine::codes::Code;
use engine::config::Target;
use engine::events::*;
use engine::manifest::Manifest;
use engine::overrides::parse_set;
use engine::policy::AcceptSuggestions;
use image::{ImageBuffer, Rgb, Rgba};
use std::path::Path;
use std::sync::Arc;

fn fixtures(dir: &Path) {
    let t = dir.join("textures/armor");
    std::fs::create_dir_all(&t).unwrap();
    ImageBuffer::from_fn(64, 64, |x, y| Rgba([128 + (x as u8 % 8), 128, 250, 100 + y as u8])).save(t.join("iron_n.png")).unwrap();
    ImageBuffer::from_fn(64, 64, |x, y| Rgb([x as u8 * 4, y as u8 * 4, 90])).save(t.join("iron.png")).unwrap();
    ImageBuffer::from_fn(64, 64, |x, _| Rgba([200, 30, 30, if x < 32 { 255 } else { 0 }])).save(t.join("leaf.png")).unwrap();
}

fn kinds(ev: &[EngineEvent], name: &str) -> usize {
    ev.iter().filter(|e| serde_json::to_value(&e.kind).unwrap()["kind"] == name).count()
}

fn request(dir: &Path) -> BatchRequest {
    let mut r = BatchRequest::new(vec![dir.join("textures")], dir.join("out"), Target::Cs);
    r.recursive = true;
    r.accept = AcceptSuggestions::High;
    r.overrides = vec![parse_set("file:**/iron.png.subtype=diffuse").unwrap(), parse_set("file:**/leaf.png.subtype=diffuse").unwrap()];
    r
}

fn run(req: BatchRequest) -> (BatchSummary, Vec<EngineEvent>) {
    let sink = Arc::new(VecSink::default());
    let rep = Reporter::new(sink.clone(), new_batch_id());
    let e = Engine::new().unwrap();
    let plan = e.plan(req, &rep).unwrap();
    let s = e.run(plan, &rep, &CancelToken::default()).unwrap();
    (s, sink.take())
}

#[test]
fn converts_skips_unresolved_and_reruns_as_up_to_date() {
    let d = tempfile::tempdir().unwrap();
    fixtures(d.path());
    let (s, ev) = run(request(d.path()));
    // leaf.png: diffuse with varying alpha -> alpha meaning unconfirmed -> skipped
    assert_eq!((s.converted, s.skipped, s.failed), (2, 1, 0), "{ev:#?}");
    assert_eq!(s.exit_code(), 1);
    assert!(d.path().join("out/armor/iron_n.dds").exists());
    assert_eq!(kinds(&ev, "file_done"), 2);
    assert_eq!(kinds(&ev, "batch_done"), 1);
    assert!(ev.windows(2).all(|w| w[0].seq < w[1].seq));
    assert_eq!(Manifest::at(&d.path().join("out")).load().unwrap().len(), 2);

    let mut req = request(d.path());
    req.overrides.push(parse_set("file:**/leaf.png.alpha.policy=cutout").unwrap());
    let (s, _) = run(req);
    assert_eq!((s.converted, s.up_to_date, s.skipped), (1, 2, 0));
    assert_eq!(s.exit_code(), 0);
}

#[test]
fn dry_run_writes_nothing_and_plans_every_file() {
    let d = tempfile::tempdir().unwrap();
    fixtures(d.path());
    let mut req = request(d.path());
    req.dry_run = true;
    let (_, ev) = run(req);
    assert_eq!(kinds(&ev, "file_planned"), 3);
    assert!(!d.path().join("out").exists());
}

#[test]
fn bad_override_value_is_a_usage_error_and_cancel_cancels() {
    let d = tempfile::tempdir().unwrap();
    fixtures(d.path());
    let mut req = request(d.path());
    req.overrides.push(parse_set("format=bc9").unwrap());
    let rep = Reporter::new(Arc::new(NullSink), "x");
    assert!(matches!(Engine::new().unwrap().plan(req, &rep), Err(EngineError::Usage(_))));

    let e = Engine::new().unwrap();
    let plan = e.plan(request(d.path()), &rep).unwrap();
    let c = CancelToken::default();
    c.cancel();
    let s = e.run(plan, &rep, &c).unwrap();
    assert_eq!(s.cancelled.len(), 2);
    assert_eq!(s.exit_code(), 1);
    let left: Vec<_> = walkdir::WalkDir::new(d.path().join("out")).into_iter().filter_map(|e| e.ok()).filter(|e| e.path().extension().is_some_and(|x| x == "tmp")).collect();
    assert!(left.is_empty());
}

#[test]
fn duplicates_and_dds_inputs_are_skipped_with_codes() {
    let d = tempfile::tempdir().unwrap();
    fixtures(d.path());
    let t = d.path().join("textures/armor");
    ImageBuffer::from_fn(4, 4, |_, _| Rgb([1u8, 2, 3])).save(t.join("iron_n.tga")).unwrap();
    std::fs::write(t.join("old.dds"), b"DDS ").unwrap();
    let rep = Reporter::new(Arc::new(NullSink), "x");
    let plan = Engine::new().unwrap().plan(request(d.path()), &rep).unwrap();
    let codes: Vec<_> = plan.skipped.iter().map(|s| s.code).collect();
    assert_eq!(codes.iter().filter(|c| **c == Some(Code::InDuplicate)).count(), 2);
    assert!(codes.contains(&Some(Code::InDdsSkipped)));
}
```
Add `walkdir.workspace = true` and `image.workspace = true` to engine `[dev-dependencies]` (both are already normal dependencies, so tests can use them; no change needed).

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p engine --test api`
Expected: FAIL to compile.

- [ ] **Step 3: Implement `sysmem.rs`**

```rust
//! Physical memory size (Windows) for the default --mem-budget (50% of RAM).
#[repr(C)]
struct MemoryStatusEx {
    length: u32,
    memory_load: u32,
    total_phys: u64,
    avail_phys: u64,
    total_page_file: u64,
    avail_page_file: u64,
    total_virtual: u64,
    avail_virtual: u64,
    avail_extended_virtual: u64,
}

#[link(name = "kernel32")]
extern "system" {
    fn GlobalMemoryStatusEx(buf: *mut MemoryStatusEx) -> i32;
}

pub fn total_ram() -> u64 {
    let mut m = MemoryStatusEx { length: std::mem::size_of::<MemoryStatusEx>() as u32, memory_load: 0, total_phys: 0, avail_phys: 0, total_page_file: 0, avail_page_file: 0, total_virtual: 0, avail_virtual: 0, avail_extended_virtual: 0 };
    if unsafe { GlobalMemoryStatusEx(&mut m) } != 0 { m.total_phys } else { 8 << 30 }
}
```

- [ ] **Step 4: Implement `pipeline.rs`**

```rust
//! One ResolvedJob end to end: load → preprocess → mips → coverage → quantise → encode → write → validate.
use crate::api::{BatchRequest, CancelToken};
use crate::config::Format;
use crate::coverage::{coverage_fraction, preserve_coverage};
use crate::dds::{dds_bytes, resolve_header, write_atomic, DdsDesc};
use crate::encode::{encode_level, EncodeCtx};
use crate::events::{EventKind, Reporter, Stage};
use crate::load::load_image;
use crate::manifest::*;
use crate::mips::build_chain;
use crate::ops::preprocess;
use crate::policy::ResolvedJob;
use crate::quantize::{to_encoder_input, EncoderInput};
use crate::validate::{validate, ValidationInput, ValidationReport};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq)]
pub enum FileError {
    Cancelled,
    Failed(Stage, String),
}

pub struct FileOutcome {
    pub output: PathBuf,
    pub report: ValidationReport,
    pub record: ManifestRecord,
}

fn fail(stage: Stage) -> impl Fn(String) -> FileError {
    move |e| FileError::Failed(stage, e)
}

pub fn process_file(job: &ResolvedJob, req: &BatchRequest, preset: &PresetRef, cancel: &CancelToken, rep: &Reporter) -> Result<FileOutcome, FileError> {
    // Test hook (Task 44): panic on the named input file to prove per-file panic isolation.
    if std::env::var_os("SDC_TEST_PANIC_ON").is_some_and(|v| job.input.file_name() == Some(v.as_os_str())) {
        panic!("test hook: SDC_TEST_PANIC_ON");
    }
    let p = &job.profile;
    let path = Some(job.input.as_path());
    let source_sha256 = sha256_file(&job.input).map_err(|e| fail(Stage::Load)(e.to_string()))?;
    let (img, _) = load_image(&job.input).map_err(|e| fail(Stage::Load)(e.to_string()))?;
    let work = preprocess(img, p);
    let mut chain = build_chain(&work, p);
    let base_coverage = (work.alpha_present && p.coverage_enabled()).then(|| {
        let c = coverage_fraction(chain[0].px.iter().map(|q| q[3]), p.alpha.threshold, p.alpha.compare);
        preserve_coverage(&mut chain, p.alpha.threshold, p.alpha.compare);
        c
    });
    let inputs: Vec<EncoderInput> = chain.iter().map(|l| to_encoder_input(l, &work, p)).collect();
    let ectx = EncodeCtx { cancel: Some(cancel.flag()) };
    let total = chain.len() as u32;
    let mut levels = Vec::with_capacity(chain.len());
    for (i, (l, inp)) in chain.iter().zip(&inputs).enumerate() {
        levels.push(encode_level(inp, l.width, l.height, p, &ectx).map_err(|_| FileError::Cancelled)?);
        rep.emit(Some(&job.set_key), path, EventKind::Progress { stage: Stage::Compress, done: i as u32 + 1, total });
    }
    let srgb = p.srgb_tag();
    let desc = DdsDesc { width: chain[0].width, height: chain[0].height, format: p.format, srgb, header: resolve_header(p.format, srgb, p.header) };
    if cancel.is_cancelled() {
        return Err(FileError::Cancelled);
    }
    write_atomic(&job.output, &dds_bytes(&desc, &levels)).map_err(|e| fail(Stage::Write)(e.to_string()))?;
    let written = std::fs::read(&job.output).map_err(|e| fail(Stage::Validate)(e.to_string()))?;
    let source_peak = (p.format == Format::Bc6hUf16).then(|| chain[0].px.iter().map(|c| 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]).fold(0.0f32, f32::max));
    let report = validate(&ValidationInput {
        bytes: &written,
        profile: p,
        encoder_input: Some(&inputs),
        base_coverage,
        source_peak,
        bc1_model: req.bc1_model,
        expected_levels: Some(total),
    });
    let rel_out = job.output.strip_prefix(&req.output_root).unwrap_or(&job.output).to_string_lossy().replace('\\', "/");
    let record = ManifestRecord {
        schema: 1,
        ts: humantime::format_rfc3339_millis(std::time::SystemTime::now()).to_string(),
        engine_version: ENGINE_VERSION.into(),
        engine_commit: engine_commit().into(),
        encoders: EncoderVersions::current(),
        isa: dispatched_isa().into(),
        preset: preset.clone(),
        profile: p.clone(),
        profile_sha256: sha256_json(p),
        provenance: job.provenance.clone(),
        source: job.input.to_string_lossy().replace('\\', "/"),
        source_sha256,
        output: rel_out,
        validation: report.summary.clone(),
    };
    if req.sidecar {
        let side = job.output.with_extension("dds.json");
        let text = serde_json::to_string_pretty(&record).unwrap_or_default();
        write_atomic(&side, text.as_bytes()).map_err(|e| fail(Stage::Write)(e.to_string()))?;
    }
    Ok(FileOutcome { output: job.output.clone(), report, record })
}
```

- [ ] **Step 5: Implement `api.rs`**

```rust
//! Engine API (spec §2): Engine::plan(BatchRequest) -> Plan, Engine::run(Plan, events, cancel).
use crate::analyse::analyse;
use crate::codes::{Code, Warning};
use crate::config::*;
use crate::discover::{canonicalise_outputs, discover, InputKind};
use crate::events::{EventKind, Reporter, Stage};
use crate::group::{group_sets_with, split_suffix_with};
use crate::load::{load_image, LoadError};
use crate::manifest::{sha256_file, sha256_json, Manifest, PresetRef, ENGINE_VERSION};
use crate::overrides::{to_patch, SetOverride};
use crate::pipeline::{process_file, FileError};
use crate::policy::*;
use crate::presets::*;
use crate::suggest::{is_pbr_set, SuggestContext};
use encoders_sys::Bc1Model;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum OnExisting {
    #[default]
    IfChanged,
    Skip,
    Overwrite,
}

#[derive(Debug, Clone)]
pub struct BatchRequest {
    pub inputs: Vec<PathBuf>,
    pub output_root: PathBuf,
    pub target: Target,
    pub preset: Option<String>,
    pub preset_files: Vec<PathBuf>,
    pub user_config: Option<PathBuf>,
    pub project_config: Option<PathBuf>,
    pub overrides: Vec<SetOverride>,
    pub accept: AcceptSuggestions,
    pub allow: BTreeSet<Code>,
    pub recursive: bool,
    pub on_existing: OnExisting,
    pub jobs: usize,
    pub mem_budget: u64,
    pub bc1_model: Bc1Model,
    pub sidecar: bool,
    pub dry_run: bool,
}

impl BatchRequest {
    pub fn new(inputs: Vec<PathBuf>, output_root: PathBuf, target: Target) -> Self {
        BatchRequest {
            inputs,
            output_root,
            target,
            preset: None,
            preset_files: vec![],
            user_config: None,
            project_config: None,
            overrides: vec![],
            accept: AcceptSuggestions::None,
            allow: BTreeSet::new(),
            recursive: false,
            on_existing: OnExisting::IfChanged,
            jobs: 0,
            mem_budget: 0,
            bc1_model: Bc1Model::Ideal,
            sidecar: false,
            dry_run: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SkippedFile {
    pub path: PathBuf,
    pub code: Option<Code>,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct FailedFile {
    pub path: PathBuf,
    pub stage: Stage,
    pub error: String,
}

pub struct Plan {
    pub request: BatchRequest,
    pub registry: PresetRegistry,
    pub preset_id: String,
    pub jobs: Vec<ResolvedJob>,
    pub skipped: Vec<SkippedFile>,
    pub failed: Vec<FailedFile>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct BatchSummary {
    pub converted: u32,
    pub up_to_date: u32,
    pub skipped: u32,
    pub failed: u32,
    pub validation_failed: u32,
    pub cancelled: Vec<PathBuf>,
}

impl BatchSummary {
    /// Spec §8 precedence for a batch that ran: 1 partial > 4 validation failed > 0. (2 and 3 come from EngineError.)
    pub fn exit_code(&self) -> i32 {
        if self.skipped + self.failed > 0 || !self.cancelled.is_empty() {
            1
        } else if self.validation_failed > 0 {
            4
        } else {
            0
        }
    }
}

#[derive(Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst)
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
    pub fn flag(&self) -> Arc<AtomicBool> {
        self.0.clone()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("{0}")]
    Usage(String),
    #[error("{0}")]
    Fatal(String),
}

impl EngineError {
    pub fn exit_code(&self) -> i32 {
        match self {
            EngineError::Usage(_) => 2,
            EngineError::Fatal(_) => 3,
        }
    }
}

pub struct Engine {
    registry: PresetRegistry,
}

struct Budget {
    limit: u64,
    used: Mutex<u64>,
    cv: Condvar,
}

impl Budget {
    fn acquire(&self, n: u64) {
        let mut used = self.used.lock().unwrap();
        while *used > 0 && *used + n > self.limit {
            used = self.cv.wait(used).unwrap();
        }
        *used += n;
    }
    fn release(&self, n: u64) {
        *self.used.lock().unwrap() -= n;
        self.cv.notify_all();
    }
}

impl Engine {
    pub fn new() -> Result<Self, EngineError> {
        Ok(Engine { registry: PresetRegistry::builtin().map_err(|e| EngineError::Fatal(e.to_string()))? })
    }

    pub fn registry(&self) -> &PresetRegistry {
        &self.registry
    }

    pub fn plan(&self, req: BatchRequest, rep: &Reporter) -> Result<Plan, EngineError> {
        let usage = |m: String| EngineError::Usage(m);
        let mut registry = self.registry.clone();
        for f in &req.preset_files {
            registry.add_file(f).map_err(|e| usage(e.to_string()))?;
        }
        let user_config = req.user_config.as_deref().filter(|p| p.exists()).map(ConfigFile::load).transpose().map_err(|e| usage(e.to_string()))?;
        let project_path = req.project_config.clone().or_else(|| req.inputs.first().filter(|p| p.is_dir()).map(|p| p.join("sdc.toml")).filter(|p| p.exists()));
        let project_config = project_path.as_deref().map(ConfigFile::load).transpose().map_err(|e| usage(e.to_string()))?;
        let preset_id = req
            .preset
            .clone()
            .or_else(|| project_config.as_ref().and_then(|c| c.preset.clone()))
            .or_else(|| user_config.as_ref().and_then(|c| c.preset.clone()))
            .unwrap_or_else(|| default_preset_for(req.target).to_string());
        let chain = registry.chain(&preset_id).map_err(|e| usage(e.to_string()))?;
        // Spec §3.7: every alias preset's chain must build a valid profile for every sub-type.
        for alias in registry.alias_table().keys() {
            let (_, pid) = registry.alias_for(alias).expect("alias in table");
            for st in Subtype::ALL {
                registry.profile(pid, req.target, st).map_err(|e| usage(format!("preset '{pid}' (suffix alias '{alias}'), sub-type {}: {e}", st.id())))?;
            }
        }
        // Spec §8: an invalid preset, config, rule or --set value is a usage error (exit 2) before any work.
        let configs: Vec<&ConfigFile> = user_config.iter().chain(project_config.iter()).collect();
        let rules: Vec<&Rule> = chain.iter().flat_map(|p| p.rules.iter()).chain(configs.iter().flat_map(|c| c.rules.iter())).collect();
        for o in req.overrides.iter().filter(|o| o.field == "subtype") {
            serde_json::from_value::<Subtype>(o.value.clone()).map_err(|_| usage(format!("--set subtype: unknown sub-type {}", o.value)))?;
        }
        for st in Subtype::ALL {
            let mut doc = serde_json::Value::Object(Default::default());
            for (_, patch) in registry.layers(&preset_id, st).map_err(|e| usage(e.to_string()))? {
                merge_json(&mut doc, &patch);
            }
            for c in &configs {
                merge_json(&mut doc, &c.defaults);
                if let Some(s) = c.subtypes.get(&st) {
                    merge_json(&mut doc, s);
                }
            }
            finish_profile(doc.clone(), req.target, st).map_err(|e| usage(format!("preset '{preset_id}' / config, sub-type {}: {e}", st.id())))?;
            for r in &rules {
                let mut d = doc.clone();
                merge_json(&mut d, &r.overrides);
                finish_profile(d, req.target, st).map_err(|e| usage(format!("rule {:?}: {e}", r.matcher)))?;
            }
            for o in req.overrides.iter().filter(|o| o.field != "subtype") {
                let mut d = doc.clone();
                merge_json(&mut d, &to_patch(&o.field, &o.value));
                finish_profile(d, req.target, st).map_err(|e| usage(format!("--set {}={}: {e}", o.field, o.value)))?;
            }
        }
        // Spec §8: an unsupported setting for the whole batch exits 2 before work starts.
        for o in req.overrides.iter().filter(|o| o.scope == crate::overrides::Scope::All) {
            let bad = match o.field.as_str() {
                "format" => o.value == "bc6h_uf16" && req.target != Target::Cs,
                "color.tag" => o.value == "srgb" && req.target != Target::Cs,
                "subtype" => serde_json::from_value::<Subtype>(o.value.clone())
                    .is_ok_and(|st| !matches!(availability(req.target, st), Availability::Available)),
                _ => false,
            };
            if bad {
                return Err(usage(format!("--set {}={} is unavailable for the whole batch under {:?} (CFG-UNAVAILABLE / TAG-SRGB-VANILLA)", o.field, o.value, req.target)));
            }
        }
        for i in &req.inputs {
            if !i.exists() {
                return Err(usage(format!("input not found: {}", i.display())));
            }
        }
        let mut files = discover(&req.inputs, &req.output_root, req.recursive).map_err(|e| usage(e.to_string()))?;
        canonicalise_outputs(&mut files, registry.alias_table());
        rep.emit(None, None, EventKind::BatchStarted { target: req.target, preset: preset_id.clone(), inputs: files.len() });
        for f in &files {
            rep.emit(None, Some(&f.path), EventKind::FileDiscovered { output: f.output.to_string_lossy().replace('\\', "/") });
        }
        let (sets, dups) = group_sets_with(&files, registry.alias_table());
        let mut skipped = vec![];
        let dup_idx: BTreeSet<usize> = dups.iter().flatten().copied().collect();
        for &i in &dup_idx {
            skipped.push(SkippedFile { path: files[i].path.clone(), code: Some(Code::InDuplicate), reason: "two inputs map to one output; exclude one".into() });
        }
        let ctx = ResolveCtx {
            target: req.target,
            registry: &registry,
            preset: &preset_id,
            user_config: user_config.as_ref(),
            project_config: project_config.as_ref(),
            overrides: &req.overrides,
            accept: req.accept,
            allow: &req.allow,
        };
        let mut work = vec![];
        for set in &sets {
            for &i in &set.members {
                if dup_idx.contains(&i) {
                    continue;
                }
                if files[i].kind == InputKind::Dds {
                    skipped.push(SkippedFile { path: files[i].path.clone(), code: Some(Code::InDdsSkipped), reason: "DDS inputs are not re-encoded by default".into() });
                    continue;
                }
                work.push((set, i));
            }
        }
        let results: Vec<_> = work
            .par_iter()
            .map(|&(set, i)| {
                let f = &files[i];
                match load_image(&f.path) {
                    Err(e) => Err((f.path.clone(), e)),
                    Ok((img, info)) => {
                        let a = analyse(&img, &info);
                        drop(img);
                        let stem = f.rel.file_stem().unwrap_or_default().to_string_lossy().into_owned();
                        let split = split_suffix_with(&stem, registry.alias_table());
                        let facts = FileFacts { input: f, suffix: split.suffix.as_deref(), alias: split.alias.as_deref(), set, analysis: &a, info: &info };
                        Ok(resolve_file(&ctx, &facts))
                    }
                }
            })
            .collect();
        let mut jobs = vec![];
        let mut failed = vec![];
        for r in results {
            match r {
                Ok(job) => jobs.push(job),
                Err((path, e)) => {
                    let code = if matches!(e, LoadError::TooLarge { .. }) { Code::DimTooLarge } else { Code::InDecode };
                    rep.emit(None, Some(&path), EventKind::Warning(Warning::new(code, e.to_string())));
                    failed.push(FailedFile { path, stage: Stage::Load, error: e.to_string() });
                }
            }
        }
        for set in &sets {
            let any = set.members.iter().find(|&&i| files[i].kind == InputKind::Image);
            if let Some(&i) = any {
                let pbr = is_pbr_set(&SuggestContext { target: req.target, suffix: None, game_path: &files[i].game_path, set_suffixes: &set.suffixes, alpha: AlphaContent::None, is_float: false });
                let members = set.members.iter().map(|&m| files[m].path.to_string_lossy().replace('\\', "/")).collect();
                rep.emit(Some(&set.key), None, EventKind::SetResolved { members, pbr, bake: false });
            }
        }
        for job in &jobs {
            rep.emit(Some(&job.set_key), Some(&job.input), EventKind::FilePlanned { job: serde_json::to_value(job).unwrap_or_default() });
            for w in &job.warnings {
                rep.emit(Some(&job.set_key), Some(&job.input), EventKind::Warning(w.clone()));
            }
        }
        Ok(Plan { request: req, registry, preset_id, jobs, skipped, failed })
    }

    pub fn run(&self, plan: Plan, rep: &Reporter, cancel: &CancelToken) -> Result<BatchSummary, EngineError> {
        let req = &plan.request;
        let mut summary = BatchSummary { failed: plan.failed.len() as u32, ..Default::default() };
        let skip = |path: &PathBuf, code: Option<Code>, reason: String, s: &mut BatchSummary| {
            rep.emit(None, Some(path), EventKind::FileSkipped { code, reason });
            s.skipped += 1;
        };
        for s in &plan.skipped {
            skip(&s.path, s.code, s.reason.clone(), &mut summary);
        }
        for f in &plan.failed {
            rep.emit(None, Some(&f.path), EventKind::FileFailed { stage: f.stage, error: f.error.clone() });
        }
        let mut ready = vec![];
        for job in &plan.jobs {
            match &job.status {
                JobStatus::Ready => ready.push(job),
                JobStatus::Unresolved { reasons } => skip(&job.input, Some(Code::SugLowConfidence), reasons.join("; "), &mut summary),
                // Spec §4.4 (Task 27a): a raw packing input is reported but does not make the batch partial (exit 1).
                JobStatus::PackingInput { role } => rep.emit(None, Some(&job.input), EventKind::FileSkipped {
                    code: Some(Code::InPackingInput),
                    reason: format!("raw {} map: channel-packing input (milestone M2.5), not converted on its own", role.id()),
                }),
                JobStatus::Blocked { codes } | JobStatus::Error { codes } => {
                    skip(&job.input, codes.first().copied(), format!("blocked by {codes:?}; use --allow for blocking codes"), &mut summary)
                }
            }
        }
        if req.dry_run {
            rep.emit(None, None, done_event(&summary));
            return Ok(summary);
        }
        encoders_sys::init(req.bc1_model).map_err(|e| EngineError::Fatal(e.to_string()))?;
        std::fs::create_dir_all(&req.output_root).map_err(|e| EngineError::Fatal(format!("output root: {e}")))?;
        let manifest = Manifest::at(&req.output_root);
        let existing = manifest.load().map_err(|e| EngineError::Fatal(format!("manifest: {e}")))?;
        let preset_ref = |id: &str| PresetRef {
            id: id.to_string(),
            version: plan.registry.get(id).map(|p| p.version.clone()).unwrap_or_default(),
            sha256: plan.registry.sha256(id).unwrap_or_default(),
        };
        let mut todo = vec![];
        for job in ready {
            if job.output.exists() {
                let rel = job.output.strip_prefix(&req.output_root).unwrap_or(&job.output).to_string_lossy().replace('\\', "/");
                match req.on_existing {
                    OnExisting::Overwrite => {}
                    OnExisting::Skip => {
                        skip(&job.input, None, "output exists (--on-existing skip)".into(), &mut summary);
                        continue;
                    }
                    OnExisting::IfChanged => match existing.get(&rel) {
                        None => {
                            rep.emit(None, Some(&job.input), EventKind::Warning(Warning::new(Code::OutUnknownExisting, "existing output has no manifest record; kept")));
                            skip(&job.input, Some(Code::OutUnknownExisting), "existing output without manifest record".into(), &mut summary);
                            continue;
                        }
                        Some(r) => {
                            let same = sha256_file(&job.input).ok().as_deref() == Some(r.source_sha256.as_str())
                                && sha256_json(&job.profile) == r.profile_sha256
                                && r.engine_version == ENGINE_VERSION;
                            if same {
                                summary.up_to_date += 1;
                                rep.emit(None, Some(&job.input), EventKind::FileSkipped { code: None, reason: "up to date".into() });
                                continue;
                            }
                        }
                    },
                }
            }
            todo.push(job);
        }
        let limit = if req.mem_budget > 0 { req.mem_budget } else { crate::sysmem::total_ram() / 2 };
        let budget = Budget { limit, used: Mutex::new(0), cv: Condvar::new() };
        let pool = rayon::ThreadPoolBuilder::new().num_threads(req.jobs).build().map_err(|e| EngineError::Fatal(e.to_string()))?;
        let summary_m = Mutex::new(summary);
        // Admission runs on the calling thread (outside the pool) and only spawns admitted files, so a pool
        // thread never blocks on the budget. A pool thread blocked in `acquire` while its own nested
        // band-parallel encode stole another file's job could otherwise hold budget forever (deadlock).
        pool.in_place_scope(|scope| {
            for job in &todo {
                if cancel.is_cancelled() {
                    summary_m.lock().unwrap().cancelled.push(job.input.clone());
                    continue;
                }
                let est = image::image_dimensions(&job.input).map(|(w, h)| w as u64 * h as u64 * 16 * 134 / 100 * 3).unwrap_or(0);
                budget.acquire(est);
                // Spec §3.7: an aliased file records its alias preset.
                let preset = preset_ref(job.suffix_alias.as_ref().map_or(plan.preset_id.as_str(), |a| a.preset.as_str()));
                let (summary_m, budget, manifest) = (&summary_m, &budget, &manifest);
                scope.spawn(move |_| {
                    if cancel.is_cancelled() {
                        budget.release(est);
                        summary_m.lock().unwrap().cancelled.push(job.input.clone());
                        return;
                    }
                    rep.emit(Some(&job.set_key), Some(&job.input), EventKind::FileStarted);
                    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| process_file(job, req, &preset, cancel, rep)));
                    budget.release(est);
                    let mut s = summary_m.lock().unwrap();
                    match r {
                        Ok(Ok(out)) => {
                            if let Err(e) = manifest.append(&out.record) {
                                rep.emit(None, Some(&job.input), EventKind::FileFailed { stage: Stage::Write, error: format!("manifest: {e}") });
                                s.failed += 1;
                                return;
                            }
                            for w in &out.report.warnings {
                                rep.emit(Some(&job.set_key), Some(&job.input), EventKind::Warning(w.clone()));
                            }
                            s.converted += 1;
                            if !out.report.summary.passed {
                                s.validation_failed += 1;
                            }
                            rep.emit(Some(&job.set_key), Some(&job.input), EventKind::FileDone { output: out.output.to_string_lossy().replace('\', "/"), validation: out.report.summary });
                        }
                        Ok(Err(FileError::Cancelled)) => s.cancelled.push(job.input.clone()),
                        Ok(Err(FileError::Failed(stage, error))) => {
                            s.failed += 1;
                            rep.emit(Some(&job.set_key), Some(&job.input), EventKind::FileFailed { stage, error });
                        }
                        Err(panic) => {
                            let msg = panic.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| panic.downcast_ref::<String>().cloned()).unwrap_or_else(|| "panic".into());
                            s.failed += 1;
                            rep.emit(Some(&job.set_key), Some(&job.input), EventKind::FileFailed { stage: Stage::Compress, error: format!("internal error: {msg}") });
                        }
                    }
                });
            }
        });
        let summary = summary_m.into_inner().unwrap();
        rep.emit(None, None, done_event(&summary));
        Ok(summary)
    }
}

fn done_event(s: &BatchSummary) -> EventKind {
    EventKind::BatchDone {
        converted: s.converted,
        up_to_date: s.up_to_date,
        skipped: s.skipped,
        failed: s.failed,
        validation_failed: s.validation_failed,
        cancelled: s.cancelled.iter().map(|p| p.to_string_lossy().replace('\\', "/")).collect(),
    }
}
```
Add `pub mod api; pub mod pipeline; pub mod sysmem;` to `lib.rs`.

Notes for the implementer:
- `plan` loads and analyses images on the global rayon pool without the memory budget; batches of many 8K/16K sources can therefore peak at (threads × image size) during planning. Acceptable for the MVP; M7's 8K/16K memory-admission tests must cover planning too (keep only the `Analysis`, as the code does).
- `job.profile` equality in `if-changed` uses the profile hash only; the preset hash is recorded but a preset change shows up through the profile it produces.
- The cancelled-file count in the test is 2: `leaf.png` is skipped as unresolved before scheduling.
- The `SDC_TEST_PANIC_ON` hook is exercised by the CLI test in Task 44.

- [ ] **Step 6: Run the tests**

Run: `cargo test -p engine --test api`
Expected: 4 passed. Then `cargo test -p engine` (everything) — all pass.

- [ ] **Step 7: Commit**

```bash
git add crates/engine
git commit -m "feat(engine): Engine plan/run API, per-file pipeline, memory-budgeted rayon scheduler, cancellation, panic isolation"
```

---

### Task 38: GATE — engine-path bench parity and the full-chain exact-opaque gate (S5, all mips)

Spec §11.2 exact-opaque row requires 255 "at every mip of full chains"; spec §12 S5 requires flag-off ≥ 254 at every mip with the 254 share recorded per level (including 1×1), and 255 everywhere for flag on and BC1. The parity gate proves load → preprocess → quantise is lossless for 8-bit sources, so Phase 0's bench reproduction carries over to the engine.

**Files:**
- Create: `crates/bench/src/engine_gates.rs`
- Modify: `crates/bench/src/lib.rs`, `crates/bench/src/gates.rs` (dispatch arms)
- Test: `crates/bench/tests/engine_gates.rs`

**Interfaces:**
- Consumes: `engine::{load, ops, mips, quantize, encode, decode, presets, validate::alpha}`, `sdc_bench::{corpus, ddsraw, gates::GateReport}`.
- Produces: `engine_gates::engine_parity(root: &Path) -> anyhow::Result<GateReport>`; `engine_gates::full_chain_exact_opaque(root: &Path) -> anyhow::Result<GateReport>`; `run("engine-parity" | "full-chain", …)`.

- [ ] **Step 1: Write the failing tests**

`crates/bench/tests/engine_gates.rs`:
```rust
use sdc_bench::{corpus::bench_root, engine_gates};

#[test]
fn engine_path_reproduces_bench_bytes() {
    let r = engine_gates::engine_parity(&bench_root()).unwrap();
    assert!(r.pass, "{r}");
}

#[test]
fn full_chain_exact_opaque_gate() {
    let r = engine_gates::full_chain_exact_opaque(&bench_root()).unwrap();
    assert!(r.pass, "{r}");
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test --release -p sdc-bench --test engine_gates`
Expected: FAIL to compile.

- [ ] **Step 3: Implement**

`crates/bench/src/engine_gates.rs`:
```rust
use crate::corpus::*;
use crate::gates::GateReport;
use crate::{ddsraw, metrics};
use anyhow::Result;
use engine::config::*;
use engine::decode::{decode_level, Decoded};
use engine::encode::{encode_level, EncodeCtx};
use engine::load::load_image;
use engine::mips::build_chain;
use engine::ops::preprocess;
use engine::presets::PresetRegistry;
use engine::quantize::{to_encoder_input, EncoderInput};
use engine::validate::alpha::alpha_stats;
use std::path::Path;

fn profile(st: Subtype) -> Profile {
    PresetRegistry::builtin().unwrap().profile("cs-quality", Target::Cs, st).unwrap()
}

/// Encodes every level of `src` through the engine; returns (encoder inputs, decoded levels, dims).
fn engine_chain(src: &Path, p: &Profile) -> Result<Vec<(u32, u32, Vec<[u8; 4]>, Vec<[u8; 4]>)>> {
    let (img, _) = load_image(src)?;
    let work = preprocess(img, p);
    let mut out = vec![];
    for l in build_chain(&work, p) {
        let inp = to_encoder_input(&l, &work, p);
        let bytes = encode_level(&inp, l.width, l.height, p, &EncodeCtx::default()).map_err(|_| anyhow::anyhow!("cancelled"))?;
        let (fmt, srgb) = (p.format, p.srgb_tag());
        let pf = engine::dds::read_dds(&engine::dds::dds_bytes(
            &engine::dds::DdsDesc { width: l.width, height: l.height, format: fmt, srgb, header: engine::dds::resolve_header(fmt, srgb, p.header) },
            &[bytes.clone()],
        ))?
        .format;
        let Decoded::Ldr(dec) = decode_level(pf, l.width, l.height, &bytes, Some(encoders_sys::Bc1Model::Ideal))? else { anyhow::bail!("unexpected HDR") };
        let EncoderInput::Ldr(inp) = inp else { anyhow::bail!("unexpected HDR input") };
        out.push((l.width, l.height, inp, dec));
    }
    Ok(out)
}

pub fn engine_parity(root: &Path) -> Result<GateReport> {
    encoders_sys::init(encoders_sys::Bc1Model::Ideal)?;
    let mut r = GateReport::new("engine path (load→preprocess→quantise→encode) == bench bc7e u5 bytes, mip 0");
    let mut p = profile(Subtype::GenericData);
    p.mips.levels = MipLevels::Keyword(MipKeyword::None);
    p.alpha.policy = AlphaPolicy::Remove;
    for set in ["rgb_real", "rgb_grad"] {
        for src in set_files(root, "images", set, "png")? {
            let (img, _) = load_image(&src)?;
            let work = preprocess(img, &p);
            let level = &build_chain(&work, &p)[0];
            let ours = encode_level(&to_encoder_input(level, &work, &p), level.width, level.height, &p, &EncodeCtx::default()).map_err(|_| anyhow::anyhow!("cancelled"))?;
            let bench = std::fs::read(root.join("out/bc7e_u5").join(set).join(format!("{}.dds", stem(&src))))?;
            r.check(ddsraw::payload(&bench)? == ours.as_slice(), format!("{set}/{}", stem(&src)));
        }
    }
    Ok(r)
}

pub fn full_chain_exact_opaque(root: &Path) -> Result<GateReport> {
    encoders_sys::init(encoders_sys::Bc1Model::Ideal)?;
    let mut r = GateReport::new("full-chain decoded alpha after remove (S5 / §11.2), every mip");
    let base = {
        let mut p = profile(Subtype::Diffuse); // sRGB content, linear-light box mips, full chain
        p.alpha.policy = AlphaPolicy::Remove;
        p
    };
    let off = Profile { alpha: AlphaSpec { exact_opaque: false, ..base.alpha.clone() }, ..base.clone() };
    let on = Profile { alpha: AlphaSpec { exact_opaque: true, ..base.alpha.clone() }, ..base.clone() };
    let bc1 = Profile { format: Format::Bc1, encoder: EncoderSpec { backend: Backend::Rgbcx, ..base.encoder.clone() }, ..off.clone() };
    for set in ["rgb_real", "rgb_grad", "stray_real", "stray_grad"] {
        let mut d_rgb = vec![];
        for src in set_files(root, "images", set, "png")? {
            let name = format!("{set}/{}", stem(&src));
            let c_off = engine_chain(&src, &off)?;
            let c_on = engine_chain(&src, &on)?;
            let c_bc1 = engine_chain(&src, &bc1)?;
            let shares: Vec<String> = c_off.iter().map(|(w, h, _, d)| format!("{w}x{h}:{:.2}%", alpha_stats(d).share_254() * 100.0)).collect();
            r.info(format!("{name} flag off 254 share per level: {}", shares.join(" ")));
            r.check(c_off.iter().all(|(_, _, _, d)| alpha_stats(d).min >= 254), format!("{name}: flag off >= 254 at every mip"));
            r.check(c_on.iter().all(|(_, _, _, d)| alpha_stats(d).eq255 as usize == d.len()), format!("{name}: flag on == 255 at every mip"));
            r.check(c_bc1.iter().all(|(_, _, _, d)| alpha_stats(d).eq255 as usize == d.len()), format!("{name}: BC1 == 255 at every mip"));
            let (_, _, inp, d_off) = &c_off[0];
            let (_, _, _, d_on) = &c_on[0];
            d_rgb.push(metrics::rgb_psnr(inp, d_on) - metrics::rgb_psnr(inp, d_off));
        }
        let m = metrics::mean(&d_rgb);
        r.check(m >= -0.05, format!("{set}: mean dRGB (flag on - off) at mip 0 {m:+.4} dB (limit -0.05)"));
    }
    Ok(r)
}
```
Add `pub mod engine_gates;` to `crates/bench/src/lib.rs` and the arms to `gates::run`:
```rust
        "engine-parity" => vec![crate::engine_gates::engine_parity(root)?],
        "full-chain" => vec![crate::engine_gates::full_chain_exact_opaque(root)?],
```

- [ ] **Step 4: Run the gates**

Run: `cargo test --release -p sdc-bench --test engine_gates; cargo run --release -p sdc-bench -- gate full-chain > target/bench/s5-full-chain.txt`
Expected: both PASS. A parity failure means quantise is not lossless for 8-bit input (check `unorm8` and that `generic_data` content is `linear`). A full-chain failure at small mips with the flag on means a mode-6 path the patch misses (Task 14 Step 6) or, with the byte-patch fallback, an encode path that skipped `force_opaque_mode6`.

- [ ] **Step 5: Record and commit**

Append to `docs/DECISIONS.md` under the S5 entry:
```markdown
- Full chain (Task 38): flag off >= 254 at every mip on rgb_real, rgb_grad, stray_* (254 share per level in target/bench/s5-full-chain.txt; 1x1 share <v>%);
  flag on and BC1: 255 at every mip; mean dRGB at mip 0: rgb_real <v>, rgb_grad <v>, stray_real <v>, stray_grad <v> dB.
- Engine path reproduces bench bc7e u5 bytes on rgb_real/rgb_grad: <n>/14.
```
```bash
git add crates/bench docs/DECISIONS.md
git commit -m "test(bench): engine-path parity and full-chain exact-opaque gates"
```

---

### Task 39: Golden tests (spec §11.1, synthetic fixtures)

Fixtures are generated in the test (CC0 by construction, nothing committed); each case runs the full `Engine::plan` + `Engine::run` and asserts format, tag, header form, mip count, alpha policy outcome and warning codes. Byte hashes are not asserted (they belong to the `pinned-isa` CI job, milestone M7).

Suffix aliases (Task 26a): add one case that writes a preset file `ultra.toml` (`base = "cs-quality"`, `[suffix_aliases] rmaosultrahigh = "rmaos"`, `[subtypes.rmaos] mips = { levels = 1 }`) to the temp dir, passes it in `BatchRequest.preset_files`, converts `textures/armor/iron_rmaosultrahigh.png` with `--accept-suggestions high`, and asserts that `iron_rmaos.dds` exists with 1 mip, no `iron_rmaosultrahigh.dds` is written, and the manifest record has `preset.id == "ultra"` and `provenance["mips.levels"] == "preset:ultra"`.

**Files:**
- Test: `crates/engine/tests/golden.rs`

**Interfaces:**
- Consumes: `engine::api::*`, `engine::dds::read_dds`, `engine::decode::decode_level`, `engine::events::*`.

- [ ] **Step 1: Write the golden tests**

`crates/engine/tests/golden.rs`:
```rust
use engine::api::*;
use engine::codes::Code;
use engine::config::Target;
use engine::dds::{read_dds, DdsPixelFormat, HeaderKind};
use engine::decode::{decode_level, Decoded};
use engine::events::*;
use engine::overrides::parse_set;
use engine::policy::AcceptSuggestions;
use image::{DynamicImage, ImageBuffer, Rgb, Rgba};
use std::path::Path;
use std::sync::Arc;

struct Out {
    summary: BatchSummary,
    events: Vec<EngineEvent>,
}

fn convert(dir: &Path, target: Target, sets: &[&str], allow: &[Code]) -> Out {
    let mut req = BatchRequest::new(vec![dir.join("in")], dir.join("out"), target);
    req.recursive = true;
    req.accept = AcceptSuggestions::High;
    req.overrides = sets.iter().map(|s| parse_set(s).unwrap()).collect();
    req.allow = allow.iter().copied().collect();
    req.on_existing = OnExisting::Overwrite;
    let sink = Arc::new(VecSink::default());
    let rep = Reporter::new(sink.clone(), "golden");
    let e = Engine::new().unwrap();
    let plan = e.plan(req, &rep).unwrap();
    let summary = e.run(plan, &rep, &CancelToken::default()).unwrap();
    Out { summary, events: sink.take() }
}

fn put(dir: &Path, rel: &str, img: DynamicImage) {
    let p = dir.join("in").join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    img.save(p).unwrap();
}

fn warnings(o: &Out) -> Vec<Code> {
    o.events.iter().filter_map(|e| if let EventKind::Warning(w) = &e.kind { Some(w.code) } else { None }).collect()
}

fn dds(dir: &Path, rel: &str) -> engine::dds::DdsFile {
    read_dds(&std::fs::read(dir.join("out").join(rel)).unwrap()).unwrap()
}

fn rgb(w: u32) -> DynamicImage {
    DynamicImage::ImageRgb8(ImageBuffer::from_fn(w, w, |x, y| Rgb([(x * 4) as u8, (y * 4) as u8, 90])))
}

fn rgba(w: u32, f: impl Fn(u32, u32) -> [u8; 4]) -> DynamicImage {
    DynamicImage::ImageRgba8(ImageBuffer::from_fn(w, w, |x, y| Rgba(f(x, y))))
}

#[test]
fn vanilla_diffuse_bc7_unorm_full_chain() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "textures/rock/rock.png", rgb(64));
    let o = convert(d.path(), Target::Vanilla, &["file:**/rock.png.subtype=diffuse"], &[]);
    assert_eq!(o.summary.converted, 1);
    let f = dds(d.path(), "textures/rock/rock.dds");
    assert_eq!((f.format, f.header_kind, f.mip_count), (DdsPixelFormat::Bc7 { srgb: false }, HeaderKind::Dx10, 7));
}

#[test]
fn cs_pbr_albedo_is_srgb_and_vanilla_refuses_it() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "textures/pbr/iron.png", rgb(64));
    convert(d.path(), Target::Cs, &["subtype=pbr_albedo"], &[]);
    assert_eq!(dds(d.path(), "textures/pbr/iron.dds").format, DdsPixelFormat::Bc7 { srgb: true });
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "textures/pbr/iron.png", rgb(64));
    // file-scoped: an unscoped `subtype=pbr_albedo` under vanilla is a batch-wide usage error (exit 2, Task 37)
    let o = convert(d.path(), Target::Vanilla, &["file:**/iron.png.subtype=pbr_albedo"], &[]);
    assert_eq!((o.summary.converted, o.summary.skipped), (0, 1));
    assert!(warnings(&o).contains(&Code::CfgUnavailable) && warnings(&o).contains(&Code::TagSrgbVanilla));
}

#[test]
fn vanilla_normal_keeps_specular_alpha() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "textures/a/iron_n.png", rgba(64, |x, _| [128, 128, 255, (x * 4) as u8]));
    let o = convert(d.path(), Target::Vanilla, &[], &[]);
    assert_eq!(o.summary.converted, 1, "{:?}", warnings(&o));
    let f = dds(d.path(), "textures/a/iron_n.dds");
    let Decoded::Ldr(px) = decode_level(f.format, 64, 64, &f.data[f.levels[0].2.clone()], None).unwrap() else { panic!() };
    assert!(px.iter().any(|p| p[3] < 200));
}

#[test]
fn landscape_diffuse_alpha_is_data() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "textures/landscape/dirt01.png", rgba(64, |x, y| [100, 80, 60, ((x + y) * 2) as u8]));
    let o = convert(d.path(), Target::Cs, &[], &[]);
    assert_eq!(o.summary.converted, 1, "{:?}", warnings(&o));
    assert_eq!(dds(d.path(), "textures/landscape/dirt01.dds").format, DdsPixelFormat::Bc7 { srgb: false });
}

#[test]
fn complex_m_near_251_warns_and_plain_m_is_bc4() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "textures/a/iron_m.png", rgba(64, |_, _| [80, 160, 40, 250]));
    put(d.path(), "textures/b/rock_m.png", DynamicImage::ImageRgb8(ImageBuffer::from_fn(64, 64, |x, _| Rgb([(x * 4) as u8; 3]))));
    let o = convert(d.path(), Target::Cs, &[], &[]);
    assert_eq!(o.summary.converted, 2, "{:?}", warnings(&o));
    assert!(warnings(&o).contains(&Code::ValMComplexMargin));
    assert_eq!(dds(d.path(), "textures/a/iron_m.dds").format, DdsPixelFormat::Bc7 { srgb: false });
    assert_eq!(dds(d.path(), "textures/b/rock_m.dds").format, DdsPixelFormat::Bc4);
}

#[test]
fn flow_is_bc5_and_sky_has_no_mips() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "textures/hair/h_flow.png", rgb(64));
    put(d.path(), "textures/sky/grad.png", rgb(64));
    let o = convert(d.path(), Target::Cs, &["file:**/grad.png.subtype=diffuse"], &[]);
    assert_eq!(o.summary.converted, 2, "{:?}", warnings(&o));
    assert_eq!(dds(d.path(), "textures/hair/h_flow.dds").format, DdsPixelFormat::Bc5);
    assert_eq!(dds(d.path(), "textures/sky/grad.dds").mip_count, 1);
}

#[test]
fn cutout_keeps_coverage_and_blend_converts() {
    let d = tempfile::tempdir().unwrap();
    let mut s: u32 = 7;
    let alpha: Vec<u8> = (0..64 * 64)
        .map(|_| {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let u = (s >> 8) as f32 / (1u32 << 24) as f32;
            (u.powi(4) * 255.0) as u8
        })
        .collect();
    put(d.path(), "textures/plants/leaf.png", rgba(64, |x, y| [40, 160, 40, alpha[(y * 64 + x) as usize]]));
    put(d.path(), "textures/decals/blood.png", rgba(64, |x, y| [150, 0, 0, (255 - ((x as i32 - 32).pow(2) + (y as i32 - 32).pow(2)).min(255)) as u8]));
    let o = convert(
        d.path(),
        Target::Cs,
        &["file:**/leaf.png.subtype=diffuse", "file:**/leaf.png.alpha.policy=cutout", "file:**/blood.png.subtype=diffuse", "file:**/blood.png.alpha.policy=blend"],
        &[],
    );
    assert_eq!(o.summary.converted, 2, "{:?}", warnings(&o));
    assert!(!warnings(&o).contains(&Code::ValCoverage));
}

#[test]
fn bc6h_emissive_in_a_pbr_set() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "textures/pbr/lamp_g.png", rgb(64));
    let o = convert(d.path(), Target::Cs, &[], &[]);
    assert_eq!(o.summary.converted, 1, "{:?}", warnings(&o));
    assert_eq!(dds(d.path(), "textures/pbr/lamp_g.dds").format, DdsPixelFormat::Bc6hUf16);
}

#[test]
fn bc5_on_normal_blocks_until_allowed() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "textures/a/iron_n.png", rgb(64));
    let sets = ["subtype:normal.format=bc5", "subtype:normal.encoder.backend=rgbcx"];
    let o = convert(d.path(), Target::Cs, &sets, &[]);
    assert_eq!(o.summary.skipped, 1);
    let o = convert(d.path(), Target::Cs, &sets, &[Code::FmtBc5Normal]);
    assert_eq!(o.summary.converted, 1);
    assert_eq!(dds(d.path(), "textures/a/iron_n.dds").format, DdsPixelFormat::Bc5);
}

#[test]
fn bc1_compact_is_legacy_dxt1_and_bc4_legacy_header_is_bc4u() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "textures/a/wall.png", rgb(64));
    put(d.path(), "textures/a/wall_p.png", DynamicImage::ImageRgb8(ImageBuffer::from_fn(64, 64, |x, _| Rgb([(x * 4) as u8; 3]))));
    let o = convert(
        d.path(),
        Target::Vanilla,
        &["file:**/wall.png.subtype=diffuse", "subtype:diffuse.format=bc1", "subtype:diffuse.encoder.backend=rgbcx", "subtype:height.header=legacy"],
        &[],
    );
    assert_eq!(o.summary.converted, 2, "{:?}", warnings(&o));
    let f = dds(d.path(), "textures/a/wall.dds");
    assert_eq!((&f.fourcc, f.format), (b"DXT1", DdsPixelFormat::Bc1 { srgb: false }));
    assert_eq!(&dds(d.path(), "textures/a/wall_p.dds").fourcc, b"BC4U");
}

#[test]
fn rmaos_with_constant_alpha_removes_it_and_16_bit_reports_bitdepth() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "textures/pbr/iron_rmaos.png", rgba(64, |x, _| [(x * 4) as u8, 0, 200, 255]));
    put(d.path(), "textures/pbr/iron_n.png", DynamicImage::ImageRgb16(ImageBuffer::from_fn(64, 64, |_, _| Rgb([32768u16, 32768, 65535]))));
    let o = convert(d.path(), Target::Cs, &[], &[]);
    assert_eq!(o.summary.converted, 2, "{:?}", warnings(&o));
    assert!(warnings(&o).contains(&Code::LossBitdepth));
    assert!(o.events.iter().any(|e| matches!(&e.kind, EventKind::FilePlanned { job } if job["profile"]["subtype"] == "rmaos" && job["profile"]["alpha"]["policy"] == "remove")));
}
```

- [ ] **Step 2: Run them**

Run: `cargo test -p engine --test golden`
Expected: all PASS. These tests exercise code that already exists (Tasks 18–37), so a failure here is a real defect in an earlier task: fix it there, re-run that task's tests, and note the fix in the commit message.

- [ ] **Step 3: Commit**

```bash
git add crates/engine/tests/golden.rs
git commit -m "test(engine): golden cases per sub-type x target with synthetic fixtures"
```

---
# Phase 2 — CLI MVP

Entry: Phase 1 exit (Tasks 18–39 and 26a green, Task 38 gates recorded). Exit (Task 45): every gate green in one run, the CLI converts and audits a real mod folder, and the owner has the S1/S2/S6/S8 asset pack (Task 46).

### Task 40: `sdc` CLI skeleton — arguments, human/JSON output, exit codes

**Files:**
- Modify: `crates/cli/Cargo.toml`, `crates/cli/src/main.rs`
- Create: `crates/cli/src/args.rs`, `crates/cli/src/human.rs`
- Test: `crates/cli/tests/cli_basics.rs`

**Interfaces:**
- Consumes: `engine::api::*`, `engine::events::*`.
- Produces:
  - `args::Cli { cmd: Command }`; `args::Command { Convert(ConvertArgs), Inspect(InspectArgs), Validate(ValidateArgs), Presets(PresetsCmd), Encoders }`
  - `args::BatchArgs { inputs: Vec<PathBuf>, target: TargetArg, preset: Option<String>, preset_file: Vec<PathBuf>, config: Option<PathBuf>, set: Vec<String>, accept_suggestions: AcceptArg, allow: Vec<String>, recursive: bool, json: bool }`
  - `args::ConvertArgs { batch: BatchArgs, output: Option<PathBuf>, jobs: usize, mem_budget: Option<String>, on_existing: OnExistingArg, dry_run: bool, sidecar: bool, nvtt: Option<PathBuf>, bc1_decoder: Bc1Arg }`
  - `args::build_request(b: &BatchArgs, output: PathBuf) -> Result<BatchRequest, String>` (parses `--set`, `--allow`, `--preset <id|file.toml>`, `--preset-file <file.toml>` (repeatable; loaded, not selected — how a preset declaring suffix aliases (Task 26a, spec §3.7) is loaded beside the batch preset), user config `%APPDATA%\sdc\config.toml`)
  - `args::parse_bytes(s: &str) -> Result<u64, String>` (`"8G"`, `"512M"`, plain bytes)
  - `human::HumanSink` (`EventSink` writing concise lines to stderr); `human::TeeSink(Vec<Arc<dyn EventSink>>)`
  - exit codes: clap usage errors 2; `EngineError::exit_code()`; `BatchSummary::exit_code()`

- [ ] **Step 1: Set the CLI dependencies**

`crates/cli/Cargo.toml` `[dependencies]`:
```toml
[dependencies]
engine.workspace = true
encoders-sys.workspace = true
clap.workspace = true
ctrlc.workspace = true
serde_json.workspace = true
schemars.workspace = true
anyhow.workspace = true

[dev-dependencies]
assert_cmd.workspace = true
predicates.workspace = true
tempfile.workspace = true
image.workspace = true
serde_json.workspace = true
```

- [ ] **Step 2: Write the failing tests**

`crates/cli/tests/cli_basics.rs`:
```rust
use assert_cmd::Command;
use image::{ImageBuffer, Rgb};

fn sdc() -> Command {
    let mut c = Command::cargo_bin("sdc").unwrap();
    c.env("APPDATA", std::env::temp_dir().join("sdc-test-appdata-none"));
    c
}

#[test]
fn help_and_version_exit_0_bad_flag_exits_2() {
    sdc().arg("--help").assert().code(0);
    sdc().arg("--version").assert().code(0).stdout(predicates::str::contains("sdc"));
    sdc().args(["convert", "--no-such-flag", "x"]).assert().code(2);
    sdc().args(["convert", "x", "--set", "colour.tag=srgb"]).assert().code(2);
    sdc().args(["convert", "does-not-exist"]).assert().code(2);
}

#[test]
fn json_mode_puts_only_json_lines_on_stdout() {
    let d = tempfile::tempdir().unwrap();
    let t = d.path().join("in/textures/a");
    std::fs::create_dir_all(&t).unwrap();
    ImageBuffer::from_fn(16, 16, |x, y| Rgb([x as u8 * 16, y as u8 * 16, 128])).save(t.join("wall_n.png")).unwrap();
    let out = sdc()
        .args(["convert", "--json", "--recursive", "--accept-suggestions", "high", "-o"])
        .arg(d.path().join("out"))
        .arg(d.path().join("in"))
        .assert()
        .code(0)
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(out).unwrap();
    assert!(text.lines().count() >= 4);
    for line in text.lines() {
        let v: serde_json::Value = serde_json::from_str(line).unwrap_or_else(|e| panic!("not JSON: {line}: {e}"));
        assert_eq!(v["schema"], 1);
    }
    assert!(text.lines().last().unwrap().contains("\"kind\":\"batch_done\""));
    assert!(d.path().join("out/textures/a/wall_n.dds").exists());
}

#[test]
fn parse_bytes_units() {
    // exercised through --mem-budget: a bad unit is a usage error
    sdc().args(["convert", "--mem-budget", "8Q", "x"]).assert().code(2);
}
```

- [ ] **Step 3: Run to verify they fail**

Run: `cargo test -p sdc --test cli_basics`
Expected: FAIL (`--json` and the subcommands do not exist yet; the stub prints a version line).

- [ ] **Step 4: Implement `args.rs`**

```rust
use clap::{Args, Parser, Subcommand, ValueEnum};
use encoders_sys::Bc1Model;
use engine::api::{BatchRequest, OnExisting};
use engine::codes::Code;
use engine::config::Target;
use engine::overrides::parse_set;
use engine::policy::AcceptSuggestions;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "sdc", version, about = "Skyrim SE/AE DDS converter (working name)")]
pub struct Cli {
    #[command(subcommand)]
    pub cmd: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Convert images or folders to DDS.
    Convert(ConvertArgs),
    /// Analyse inputs; print proposed sub-type, profile and warnings; DDS headers and mip statistics.
    Inspect(InspectArgs),
    /// Run the spec §5.8 checks on existing DDS files.
    Validate(ValidateArgs),
    /// Inspect presets and print schemas.
    #[command(subcommand)]
    Presets(PresetsCmd),
    /// Built-in encoder versions and NVTT detection.
    Encoders,
}

#[derive(Clone, Copy, ValueEnum)]
pub enum TargetArg {
    Cs,
    Cpm,
    Vanilla,
}

impl From<TargetArg> for Target {
    fn from(t: TargetArg) -> Target {
        match t {
            TargetArg::Cs => Target::Cs,
            TargetArg::Cpm => Target::Cpm,
            TargetArg::Vanilla => Target::Vanilla,
        }
    }
}

#[derive(Clone, Copy, ValueEnum)]
pub enum AcceptArg {
    None,
    High,
}

#[derive(Clone, Copy, ValueEnum)]
pub enum OnExistingArg {
    IfChanged,
    Skip,
    Overwrite,
}

#[derive(Clone, Copy, ValueEnum)]
pub enum Bc1Arg {
    Ideal,
    IdealRound4,
    Nvidia,
    Amd,
}

impl From<Bc1Arg> for Bc1Model {
    fn from(b: Bc1Arg) -> Bc1Model {
        match b {
            Bc1Arg::Ideal => Bc1Model::Ideal,
            Bc1Arg::IdealRound4 => Bc1Model::IdealRound4,
            Bc1Arg::Nvidia => Bc1Model::Nvidia,
            Bc1Arg::Amd => Bc1Model::Amd,
        }
    }
}

#[derive(Args, Clone)]
pub struct BatchArgs {
    /// Image files or folders.
    #[arg(required = true)]
    pub inputs: Vec<PathBuf>,
    #[arg(long, value_enum, default_value = "cs")]
    pub target: TargetArg,
    /// Preset id, or a path to a preset .toml file.
    #[arg(long)]
    pub preset: Option<String>,
    /// Load a preset .toml without selecting it, e.g. one declaring suffix aliases (repeatable).
    #[arg(long = "preset-file")]
    pub preset_file: Vec<PathBuf>,
    /// Project config (default: sdc.toml in the first input folder).
    #[arg(long)]
    pub config: Option<PathBuf>,
    /// Field override: field=value, subtype:<id>.field=value, file:<glob>.field=value.
    #[arg(long = "set")]
    pub set: Vec<String>,
    #[arg(long, value_enum, default_value = "none")]
    pub accept_suggestions: AcceptArg,
    /// Acknowledge a blocking warning code (repeatable).
    #[arg(long)]
    pub allow: Vec<String>,
    #[arg(long)]
    pub recursive: bool,
    /// Write JSON Lines events to stdout (human output stays on stderr).
    #[arg(long)]
    pub json: bool,
}

#[derive(Args)]
pub struct ConvertArgs {
    #[command(flatten)]
    pub batch: BatchArgs,
    /// Output root (default: ./sdc-out).
    #[arg(short = 'o', long)]
    pub output: Option<PathBuf>,
    /// Worker threads (0 = all cores).
    #[arg(long, default_value_t = 0)]
    pub jobs: usize,
    /// Memory budget, e.g. 8G or 512M (default 50% of RAM).
    #[arg(long)]
    pub mem_budget: Option<String>,
    #[arg(long, value_enum, default_value = "if-changed")]
    pub on_existing: OnExistingArg,
    #[arg(long)]
    pub dry_run: bool,
    /// Also write <name>.dds.json next to each output.
    #[arg(long)]
    pub sidecar: bool,
    /// Path to nvcompress.exe (NVTT backend arrives in milestone M5).
    #[arg(long)]
    pub nvtt: Option<PathBuf>,
    #[arg(long, value_enum, default_value = "ideal")]
    pub bc1_decoder: Bc1Arg,
}

#[derive(Args)]
pub struct InspectArgs {
    #[command(flatten)]
    pub batch: BatchArgs,
}

#[derive(Args)]
pub struct ValidateArgs {
    #[arg(required = true)]
    pub dds: Vec<PathBuf>,
    #[arg(long, value_enum, default_value = "cs")]
    pub target: TargetArg,
    /// Sub-type the files are meant to be (default: inferred from the file name).
    #[arg(long)]
    pub subtype: Option<String>,
    #[arg(long)]
    pub json: bool,
    #[arg(long, value_enum, default_value = "ideal")]
    pub bc1_decoder: Bc1Arg,
}

#[derive(Subcommand)]
pub enum PresetsCmd {
    /// List selectable presets.
    List {
        #[arg(long)]
        json: bool,
    },
    /// Show a preset's resolved profile(s) with provenance.
    Show {
        id: String,
        #[arg(long)]
        subtype: Option<String>,
        #[arg(long, value_enum, default_value = "cs")]
        target: TargetArg,
    },
    /// Print a JSON Schema (profile, preset, config, event, manifest) or write all to a folder.
    Schema {
        /// Task 43 changes this to `#[arg(value_enum)] which: engine::schemas::SchemaKind`.
        which: String,
        #[arg(long)]
        out: Option<PathBuf>,
    },
}

pub fn parse_bytes(s: &str) -> Result<u64, String> {
    let s = s.trim();
    let (num, mul) = match s.chars().last() {
        Some('G' | 'g') => (&s[..s.len() - 1], 1u64 << 30),
        Some('M' | 'm') => (&s[..s.len() - 1], 1u64 << 20),
        Some(c) if c.is_ascii_digit() => (s, 1),
        _ => return Err(format!("--mem-budget '{s}': use bytes or a G/M suffix")),
    };
    num.parse::<u64>().map(|n| n * mul).map_err(|_| format!("--mem-budget '{s}': not a number"))
}

pub fn build_request(b: &BatchArgs, output: PathBuf) -> Result<BatchRequest, String> {
    let mut r = BatchRequest::new(b.inputs.clone(), output, b.target.into());
    match &b.preset {
        Some(p) if p.ends_with(".toml") => {
            let path = PathBuf::from(p);
            let text = std::fs::read_to_string(&path).map_err(|e| format!("--preset {p}: {e}"))?;
            let id = toml_id(&text).ok_or_else(|| format!("--preset {p}: no id"))?;
            r.preset_files.push(path);
            r.preset = Some(id);
        }
        Some(p) => r.preset = Some(p.clone()),
        None => {}
    }
    r.preset_files.extend(b.preset_file.iter().cloned());
    r.project_config = b.config.clone();
    r.user_config = std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("sdc").join("config.toml"));
    r.overrides = b.set.iter().map(|s| parse_set(s)).collect::<Result<_, _>>()?;
    r.allow = b.allow.iter().map(|a| Code::parse(a).ok_or_else(|| format!("--allow {a}: unknown code"))).collect::<Result<_, _>>()?;
    r.accept = match b.accept_suggestions {
        AcceptArg::None => AcceptSuggestions::None,
        AcceptArg::High => AcceptSuggestions::High,
    };
    r.recursive = b.recursive;
    Ok(r)
}

fn toml_id(text: &str) -> Option<String> {
    text.lines().find_map(|l| l.trim().strip_prefix("id").map(|r| r.trim_start_matches([' ', '=']).trim().trim_matches('"').to_string()))
}

impl From<OnExistingArg> for OnExisting {
    fn from(o: OnExistingArg) -> OnExisting {
        match o {
            OnExistingArg::IfChanged => OnExisting::IfChanged,
            OnExistingArg::Skip => OnExisting::Skip,
            OnExistingArg::Overwrite => OnExisting::Overwrite,
        }
    }
}
```
`PresetsCmd::Schema` takes a plain `String` until Task 43 introduces `engine::schemas::SchemaKind`.

- [ ] **Step 5: Implement `human.rs`**

```rust
use engine::codes::Severity;
use engine::events::{EngineEvent, EventKind, EventSink};
use std::io::Write;
use std::sync::{Arc, Mutex};

/// Concise human log on stderr (spec §8: human output on stderr).
pub struct HumanSink(Mutex<std::io::Stderr>);

impl HumanSink {
    pub fn new() -> Self {
        HumanSink(Mutex::new(std::io::stderr()))
    }
}

impl Default for HumanSink {
    fn default() -> Self {
        Self::new()
    }
}

impl EventSink for HumanSink {
    fn emit(&self, ev: &EngineEvent) {
        let path = ev.path.clone().unwrap_or_default();
        let line = match &ev.kind {
            EventKind::FileDone { output, validation } => Some(format!("ok    {path} -> {output}{}", if validation.passed { "" } else { "  [VALIDATION FAILED]" })),
            EventKind::FileSkipped { code, reason } => Some(format!("skip  {path}: {}{reason}", code.map(|c| format!("{} ", c.as_str())).unwrap_or_default())),
            EventKind::FileFailed { stage, error } => Some(format!("FAIL  {path} at {stage:?}: {error}")),
            EventKind::Warning(w) if w.severity >= Severity::Warning => Some(format!("{:<5} {path}: {} {}", format!("{:?}", w.severity).to_lowercase(), w.code.as_str(), w.message)),
            EventKind::BatchDone { converted, up_to_date, skipped, failed, validation_failed, cancelled } => Some(format!(
                "done: {converted} converted, {up_to_date} up to date, {skipped} skipped, {failed} failed, {validation_failed} failed validation, {} cancelled",
                cancelled.len()
            )),
            _ => None,
        };
        if let Some(l) = line {
            let _ = writeln!(self.0.lock().unwrap(), "{l}");
        }
    }
}

pub struct TeeSink(pub Vec<Arc<dyn EventSink>>);

impl EventSink for TeeSink {
    fn emit(&self, ev: &EngineEvent) {
        for s in &self.0 {
            s.emit(ev);
        }
    }
}
```

- [ ] **Step 6: Implement `main.rs` (convert only; other verbs come in Tasks 42–43)**

```rust
mod args;
mod human;

use args::*;
use clap::Parser;
use engine::api::{CancelToken, Engine, EngineError};
use engine::events::{new_batch_id, EventSink, JsonLinesSink, Reporter};
use std::process::ExitCode;
use std::sync::Arc;

fn reporter(json: bool) -> Reporter {
    let mut sinks: Vec<Arc<dyn EventSink>> = vec![Arc::new(human::HumanSink::new())];
    if json {
        sinks.push(Arc::new(JsonLinesSink::new(std::io::stdout())));
    }
    Reporter::new(Arc::new(human::TeeSink(sinks)), new_batch_id())
}

fn convert(a: ConvertArgs) -> Result<i32, EngineError> {
    let out = a.output.clone().unwrap_or_else(|| std::path::PathBuf::from("sdc-out"));
    let mut req = build_request(&a.batch, out).map_err(EngineError::Usage)?;
    req.jobs = a.jobs;
    req.mem_budget = a.mem_budget.as_deref().map(parse_bytes).transpose().map_err(EngineError::Usage)?.unwrap_or(0);
    req.on_existing = a.on_existing.into();
    req.dry_run = a.dry_run;
    req.sidecar = a.sidecar;
    req.bc1_model = a.bc1_decoder.into();
    if a.nvtt.is_some() {
        eprintln!("note: --nvtt is accepted but the NVTT backend arrives in milestone M5");
    }
    let rep = reporter(a.batch.json);
    let engine = Engine::new()?;
    let plan = engine.plan(req, &rep)?;
    let cancel = CancelToken::default();
    let c2 = cancel.clone();
    let _ = ctrlc::set_handler(move || {
        eprintln!("cancelling: finishing current bands…");
        c2.cancel();
    });
    Ok(engine.run(plan, &rep, &cancel)?.exit_code())
}

fn run(cli: Cli) -> Result<i32, EngineError> {
    match cli.cmd {
        Command::Convert(a) => convert(a),
        _ => Err(EngineError::Usage("not implemented yet".into())),
    }
}

fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(c) => c,
        Err(e) => {
            let code = if e.use_stderr() { 2 } else { 0 };
            let _ = e.print();
            return ExitCode::from(code);
        }
    };
    match run(cli) {
        Ok(code) => ExitCode::from(code as u8),
        Err(e) => {
            eprintln!("sdc: {e}");
            ExitCode::from(e.exit_code() as u8)
        }
    }
}
```

- [ ] **Step 7: Run the tests**

Run: `cargo test -p sdc --test cli_basics --test static_crt`
Expected: all PASS. (`wall_n.png` is a non-PBR `_n`: high-confidence `normal`, so `--accept-suggestions high` converts it.)

- [ ] **Step 8: Commit**

```bash
git add crates/cli
git commit -m "feat(cli): sdc convert with JSON Lines on stdout, human log on stderr and spec exit codes"
```

---

### Task 41: Config precedence, scoped overrides and batch options end to end

**Files:**
- Test: `crates/cli/tests/cli_convert.rs`
- Modify (only if a test exposes a defect): `crates/cli/src/args.rs`, `crates/engine/src/api.rs`

**Interfaces:**
- Consumes: Task 40.

This task proves spec §8's precedence (built-in → preset → user config → project config → rules → accepted suggestions → `--set`) and the batch options through the real binary.

Suffix aliases (Task 26a): add one test that writes `ultra.toml` (`base = "cs-quality"`, `[suffix_aliases] wallultra = "n"`), runs `sdc convert <in> -o <out> --preset-file ultra.toml --accept-suggestions high` on a `wall_wallultra.png` normal map, and asserts `wall_n.dds` is written and `wall_wallultra.dds` is not.

- [ ] **Step 1: Write the tests**

`crates/cli/tests/cli_convert.rs`:
```rust
use assert_cmd::Command;
use engine::dds::{read_dds, DdsPixelFormat};
use image::{ImageBuffer, Rgb};
use std::path::Path;

fn sdc(appdata: &Path) -> Command {
    let mut c = Command::cargo_bin("sdc").unwrap();
    c.env("APPDATA", appdata);
    c
}

fn setup() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    let t = d.path().join("in/textures/a");
    std::fs::create_dir_all(&t).unwrap();
    for n in ["wall.png", "floor.png"] {
        ImageBuffer::from_fn(32, 32, |x, y| Rgb([x as u8 * 8, y as u8 * 8, 90])).save(t.join(n)).unwrap();
    }
    d
}

fn fmt(d: &Path, rel: &str) -> DdsPixelFormat {
    read_dds(&std::fs::read(d.join("out").join(rel)).unwrap()).unwrap().format
}

#[test]
fn precedence_user_config_then_project_config_then_set() {
    let d = setup();
    let appdata = d.path().join("appdata");
    std::fs::create_dir_all(appdata.join("sdc")).unwrap();
    // user config: diffuse -> bc1 (rgbcx)
    std::fs::write(appdata.join("sdc/config.toml"), "[subtypes.diffuse]\nformat = \"bc1\"\nencoder = { backend = \"rgbcx\" }\n").unwrap();
    // project config in the input root: a rule decides the sub-type for every file
    std::fs::write(d.path().join("in/sdc.toml"), "[[rules]]\nmatch = { glob = \"**/*.png\" }\nsubtype = \"diffuse\"\n").unwrap();
    sdc(&appdata).args(["convert", "--recursive", "--target", "vanilla", "-o"]).arg(d.path().join("out")).arg(d.path().join("in")).assert().code(0);
    assert_eq!(fmt(d.path(), "textures/a/wall.dds"), DdsPixelFormat::Bc1 { srgb: false });
    // --set beats the user config for one file
    sdc(&appdata)
        .args(["convert", "--recursive", "--target", "vanilla", "--on-existing", "overwrite", "--set", "file:**/floor.png.format=bc7", "--set", "file:**/floor.png.encoder.backend=bc7e", "-o"])
        .arg(d.path().join("out"))
        .arg(d.path().join("in"))
        .assert()
        .code(0);
    assert_eq!(fmt(d.path(), "textures/a/floor.dds"), DdsPixelFormat::Bc7 { srgb: false });
    assert_eq!(fmt(d.path(), "textures/a/wall.dds"), DdsPixelFormat::Bc1 { srgb: false });
}

#[test]
fn unresolved_files_are_skipped_with_exit_1_and_dry_run_writes_nothing() {
    let d = setup();
    let appdata = d.path().join("appdata");
    sdc(&appdata).args(["convert", "--recursive", "--dry-run", "-o"]).arg(d.path().join("out")).arg(d.path().join("in")).assert().code(1);
    assert!(!d.path().join("out/textures").exists());
    sdc(&appdata)
        .args(["convert", "--recursive", "--accept-suggestions", "high", "-o"])
        .arg(d.path().join("out"))
        .arg(d.path().join("in"))
        .assert()
        .code(1)
        .stderr(predicates::str::contains("SUG-LOW-CONFIDENCE"));
}

#[test]
fn if_changed_skip_and_unknown_existing() {
    let d = setup();
    let appdata = d.path().join("appdata");
    let out = d.path().join("out");
    let run = |extra: &[&str]| {
        let mut c = sdc(&appdata);
        c.args(["convert", "--recursive", "--set", "subtype=diffuse", "-o"]).arg(&out).args(extra).arg(d.path().join("in"));
        c
    };
    run(&[]).assert().code(0);
    run(&[]).assert().code(0).stderr(predicates::str::contains("2 up to date"));
    std::fs::remove_file(out.join("_sdc/manifest.jsonl")).unwrap();
    run(&[]).assert().code(1).stderr(predicates::str::contains("OUT-UNKNOWN-EXISTING"));
    run(&["--on-existing", "overwrite"]).assert().code(0);
}
```
Note: the second test's first command exits 1 in dry-run because the two colour files are unresolved (`diffuse` is a medium-confidence suggestion).

- [ ] **Step 2: Run them**

Run: `cargo test -p sdc --test cli_convert`
Expected: all PASS. Any failure is a defect in Tasks 25/37/40: fix it there with a regression test in that task's test file.

- [ ] **Step 3: Commit**

```bash
git add crates/cli crates/engine
git commit -m "test(cli): config precedence, scoped overrides, dry-run, if-changed and unknown-existing outputs"
```

---

### Task 42: `inspect` and `validate` verbs

**Files:**
- Modify: `crates/cli/src/main.rs`
- Create: `crates/cli/src/verbs.rs`
- Test: `crates/cli/tests/cli_inspect_validate.rs`

**Interfaces:**
- Consumes: `engine::api`, `engine::dds`, `engine::decode`, `engine::validate`, `engine::suggest`, `engine::group::split_suffix`.
- Produces: `verbs::inspect(a: InspectArgs) -> Result<i32, EngineError>`; `verbs::validate_files(a: ValidateArgs) -> Result<i32, EngineError>`.

`inspect` = a dry-run plan (no output root needed; a temp root under `%TEMP%` is used for path mapping) that prints, per image, sub-type with confidence, format/tag/alpha/status and warnings; for `.dds` inputs it prints the header (format, size, mips, header kind, cubemap) and per-mip decoded-alpha statistics. With `--json`, it prints `file_planned` events (images) and one JSON object per DDS (`{"kind":"dds_info", …}`). Exit 0 (inspection never fails unless a usage error).

`validate` builds a profile per file from `--subtype` (or the suffix suggestion, else `generic_data`) under `--target`, but takes **format and tag from the file itself** (so third-party files are judged on header rules and alpha/`_m` rules, not on sdc's preferred format), and runs `engine::validate::validate` without encoder input. Exit 4 if any file fails validation, 1 if a file cannot be read, else 0.

Suffix aliases (Task 26a): `inspect` goes through `Engine::plan`, so its `file_planned` output carries `suffix_alias` and canonical output names. `validate` loads no user presets, so it uses the alias-free `split_suffix`; judge an aliased DDS with `--subtype`.

- [ ] **Step 1: Write the failing tests**

`crates/cli/tests/cli_inspect_validate.rs`:
```rust
use assert_cmd::Command;
use image::{ImageBuffer, Rgb};

fn sdc() -> Command {
    let mut c = Command::cargo_bin("sdc").unwrap();
    c.env("APPDATA", std::env::temp_dir().join("sdc-test-appdata-none"));
    c
}

#[test]
fn inspect_reports_suggestions_and_dds_headers() {
    let d = tempfile::tempdir().unwrap();
    ImageBuffer::from_fn(16, 16, |_, _| Rgb([128u8, 128, 255])).save(d.path().join("rock_n.png")).unwrap();
    sdc().args(["inspect"]).arg(d.path().join("rock_n.png")).assert().code(0).stdout(predicates::str::contains("normal")).stdout(predicates::str::contains("high"));
    let bench = std::path::PathBuf::from(std::env::var("SDC_BENCH_ROOT").unwrap_or(r"F:\ai\dds\bench".into())).join("out/bc7e_u5/rgb_real/Bricks104.dds");
    sdc().args(["inspect", "--json"]).arg(&bench).assert().code(0).stdout(predicates::str::contains("\"kind\":\"dds_info\"")).stdout(predicates::str::contains("Bc7"));
}

#[test]
fn validate_flags_254_as_info_and_passes_bench_bc7() {
    let bench = std::path::PathBuf::from(std::env::var("SDC_BENCH_ROOT").unwrap_or(r"F:\ai\dds\bench".into())).join("out/bc7e_u6/rgb_real/Bricks104.dds");
    sdc().args(["validate", "--subtype", "diffuse", "--target", "vanilla"]).arg(&bench).assert().code(0).stderr(predicates::str::contains("ALPHA-254"));
    let d = tempfile::tempdir().unwrap();
    let junk = d.path().join("junk.dds");
    std::fs::write(&junk, b"not a dds").unwrap();
    sdc().arg("validate").arg(&junk).assert().code(1);
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p sdc --test cli_inspect_validate`
Expected: FAIL (verbs not implemented: exit 2 "not implemented yet").

- [ ] **Step 3: Implement `verbs.rs`**

```rust
use crate::args::*;
use crate::human::HumanSink;
use engine::api::{Engine, EngineError};
use engine::codes::Severity;
use engine::config::*;
use engine::dds::read_dds;
use engine::decode::{decode_level, Decoded};
use engine::events::{new_batch_id, EventKind, Reporter, VecSink};
use engine::group::split_suffix;
use engine::presets::default_preset_for;
use engine::suggest::{suggest, SuggestContext};
use engine::validate::alpha::alpha_stats;
use engine::validate::{validate, ValidationInput};
use std::collections::BTreeSet;
use std::sync::Arc;

fn dds_info(path: &std::path::Path) -> Result<serde_json::Value, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let f = read_dds(&bytes).map_err(|e| e.to_string())?;
    let mut mips = vec![];
    for (w, h, r) in &f.levels {
        let alpha = match decode_level(f.format, *w, *h, &f.data[r.clone()], None) {
            Ok(Decoded::Ldr(px)) => serde_json::to_value(alpha_stats(&px)).unwrap(),
            _ => serde_json::Value::Null,
        };
        mips.push(serde_json::json!({ "width": w, "height": h, "alpha": alpha }));
    }
    Ok(serde_json::json!({
        "kind": "dds_info", "path": path.to_string_lossy(), "format": format!("{:?}", f.format),
        "header": format!("{:?}", f.header_kind), "width": f.width, "height": f.height,
        "mip_count": f.mip_count, "cubemap": f.is_cubemap, "header_issues": engine::dds::check_header(&f), "mips": mips
    }))
}

pub fn inspect(a: InspectArgs) -> Result<i32, EngineError> {
    let (dds, images): (Vec<_>, Vec<_>) = a.batch.inputs.iter().cloned().partition(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("dds")));
    for p in &dds {
        let v = dds_info(p).map_err(|e| EngineError::Usage(format!("{}: {e}", p.display())))?;
        if a.batch.json {
            println!("{v}");
        } else {
            println!("{}: {} {}x{} mips {} ({}) issues {}", p.display(), v["format"], v["width"], v["height"], v["mip_count"], v["header"], v["header_issues"]);
            for (i, m) in v["mips"].as_array().unwrap().iter().enumerate() {
                println!("  mip {i} {}x{} alpha {}", m["width"], m["height"], m["alpha"]);
            }
        }
    }
    if images.is_empty() {
        return Ok(0);
    }
    let mut b = a.batch.clone();
    b.inputs = images;
    let mut req = build_request(&b, std::env::temp_dir().join("sdc-inspect")).map_err(EngineError::Usage)?;
    req.dry_run = true;
    let sink = Arc::new(VecSink::default());
    let rep = Reporter::new(sink.clone(), new_batch_id());
    let plan = Engine::new()?.plan(req, &rep)?;
    for ev in sink.take() {
        if let EventKind::FilePlanned { job } = &ev.kind {
            if a.batch.json {
                println!("{}", serde_json::to_string(&ev).unwrap());
            } else {
                let sug = &job["suggestions"][0];
                println!(
                    "{}\n  sub-type {} ({} confidence: {})\n  format {} tag {} alpha {}\n  status {}",
                    job["input"].as_str().unwrap_or(""),
                    job["profile"]["subtype"],
                    sug["confidence"],
                    sug["reason"],
                    job["profile"]["format"],
                    job["profile"]["color"]["tag"],
                    job["profile"]["alpha"]["policy"],
                    job["status"]
                );
                for w in job["warnings"].as_array().unwrap() {
                    println!("  {} {} {}", w["severity"], w["code"], w["message"]);
                }
            }
        }
    }
    let _ = plan;
    Ok(0)
}

pub fn validate_files(a: ValidateArgs) -> Result<i32, EngineError> {
    let reg = Engine::new()?.registry().clone();
    let target: Target = a.target.into();
    let human = HumanSink::new();
    let rep = Reporter::new(Arc::new(human), new_batch_id());
    let (mut unreadable, mut failed) = (false, false);
    for path in &a.dds {
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("FAIL  {}: {e}", path.display());
                unreadable = true;
                continue;
            }
        };
        let file = match read_dds(&bytes) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("FAIL  {}: {e}", path.display());
                unreadable = true;
                continue;
            }
        };
        let st: Subtype = match &a.subtype {
            Some(s) => serde_json::from_value(serde_json::Value::String(s.clone())).map_err(|_| EngineError::Usage(format!("--subtype {s}: unknown")))?,
            None => {
                let stem = path.file_stem().unwrap_or_default().to_string_lossy().into_owned();
                let suffix = split_suffix(&stem).1;
                let set = BTreeSet::new();
                let ctx = SuggestContext { target, suffix: suffix.as_deref(), game_path: &path.to_string_lossy().replace('\\', "/").to_ascii_lowercase(), set_suffixes: &set, alpha: engine::presets::AlphaContent::Varying, is_float: false };
                suggest(&ctx).first().map(|s| s.subtype).unwrap_or(Subtype::GenericData)
            }
        };
        let mut p = reg.profile(default_preset_for(target), target, st).map_err(|e| EngineError::Fatal(e.to_string()))?;
        if let Some((fmt, srgb)) = file.format.as_format() {
            p.format = fmt;
            p.color.tag = if fmt == Format::Bc6hUf16 { ColorTag::NotApplicable } else if srgb { ColorTag::Srgb } else { ColorTag::Unorm };
        }
        let r = validate(&ValidationInput { bytes: &bytes, profile: &p, encoder_input: None, base_coverage: None, source_peak: None, bc1_model: a.bc1_decoder.into(), expected_levels: None });
        for w in &r.warnings {
            rep.emit(None, Some(path), EventKind::Warning(w.clone()));
            if a.json {
                println!("{}", serde_json::json!({"path": path.to_string_lossy(), "warning": w}));
            }
        }
        if a.json {
            println!("{}", serde_json::json!({"path": path.to_string_lossy(), "subtype": st, "summary": r.summary}));
        }
        let info: Vec<_> = r.warnings.iter().filter(|w| w.severity == Severity::Info).map(|w| format!("{} {}", w.code.as_str(), w.message)).collect();
        eprintln!("{} {} ({})  {}", if r.summary.passed { "ok   " } else { "FAIL " }, path.display(), st.id(), info.join("; "));
        failed |= !r.summary.passed;
    }
    Ok(if unreadable { 1 } else if failed { 4 } else { 0 })
}
```
In `main.rs` add `mod verbs;` and dispatch `Command::Inspect(a) => verbs::inspect(a)`, `Command::Validate(a) => verbs::validate_files(a)`.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p sdc --test cli_inspect_validate`
Expected: 2 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/cli
git commit -m "feat(cli): inspect (plans and DDS headers/mip alpha) and validate verbs"
```

---

### Task 43: `presets`, `encoders` and the committed JSON Schemas

**Files:**
- Create: `crates/engine/src/schemas.rs`, `schema/profile.schema.json`, `schema/preset.schema.json`, `schema/config.schema.json`, `schema/event.schema.json`, `schema/manifest.schema.json`
- Modify: `crates/engine/src/lib.rs`, `crates/engine/Cargo.toml` (add `clap` with `derive` for `ValueEnum`), `crates/cli/src/main.rs`, `crates/cli/src/verbs.rs`
- Test: `crates/engine/tests/schemas.rs`, `crates/cli/tests/cli_presets.rs`

**Interfaces:**
- Produces: `engine::schemas::SchemaKind { All, Profile, Preset, Config, Event, Manifest }` (`clap::ValueEnum`); `engine::schemas::schema_json(kind: SchemaKind) -> String` (pretty JSON + trailing newline); `engine::schemas::write_all(dir: &Path) -> io::Result<()>`; CLI `sdc presets list [--json]`, `sdc presets show <id> [--subtype <id>] [--target <t>]`, `sdc presets schema <kind> [--out <dir>]`, `sdc encoders`.

Suffix aliases (Task 26a): `presets list --json` adds `"suffix_aliases": p.suffix_aliases` to each object and the human line appends `aliases: <alias>→<canonical>, …` when non-empty; `presets show` prints the preset's alias table before the profiles. Built-ins declare none, so this shows only for a loaded user preset (`presets` verbs gain `--preset-file` in M6). `schema/preset.schema.json` gains `suffix_aliases` automatically.

- [ ] **Step 1: Write the failing drift test**

`crates/engine/tests/schemas.rs`:
```rust
use engine::schemas::*;

#[test]
fn committed_schemas_match_the_types() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schema");
    for (kind, file) in [(SchemaKind::Profile, "profile"), (SchemaKind::Preset, "preset"), (SchemaKind::Config, "config"), (SchemaKind::Event, "event"), (SchemaKind::Manifest, "manifest")] {
        let committed = std::fs::read_to_string(dir.join(format!("{file}.schema.json"))).unwrap_or_default().replace("\r\n", "\n");
        assert_eq!(committed, schema_json(kind), "schema/{file}.schema.json is stale: run `sdc presets schema all --out schema`");
    }
}
```
`crates/cli/tests/cli_presets.rs`:
```rust
use assert_cmd::Command;

#[test]
fn presets_list_show_and_encoders() {
    let sdc = || Command::cargo_bin("sdc").unwrap();
    sdc().args(["presets", "list"]).assert().code(0).stdout(predicates::str::contains("cs-quality")).stdout(predicates::str::contains("vanilla-quality"));
    sdc().args(["presets", "show", "cs-quality", "--subtype", "pbr_albedo"]).assert().code(0).stdout(predicates::str::contains("\"tag\": \"srgb\"")).stdout(predicates::str::contains("builtin"));
    sdc().args(["presets", "show", "nope"]).assert().code(2);
    sdc().args(["encoders"]).assert().code(0).stdout(predicates::str::contains("b943862")).stdout(predicates::str::contains("1.31.0"));
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p engine --test schemas; cargo test -p sdc --test cli_presets`
Expected: FAIL to compile.

- [ ] **Step 3: Implement `schemas.rs`**

Add to `crates/engine/Cargo.toml` `[dependencies]`: `clap.workspace = true`.
```rust
//! Published JSON Schemas (spec §2 `schema/`, §3): generated from the Rust types so they cannot drift.
use crate::config::Profile;
use crate::events::EngineEvent;
use crate::manifest::ManifestRecord;
use crate::presets::{ConfigFile, PresetFile};

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum SchemaKind {
    All,
    Profile,
    Preset,
    Config,
    Event,
    Manifest,
}

pub fn schema_json(kind: SchemaKind) -> String {
    let v = match kind {
        SchemaKind::Profile => serde_json::to_value(schemars::schema_for!(Profile)),
        SchemaKind::Preset => serde_json::to_value(schemars::schema_for!(PresetFile)),
        SchemaKind::Config => serde_json::to_value(schemars::schema_for!(ConfigFile)),
        SchemaKind::Event => serde_json::to_value(schemars::schema_for!(EngineEvent)),
        SchemaKind::Manifest => serde_json::to_value(schemars::schema_for!(ManifestRecord)),
        SchemaKind::All => panic!("schema_json(All): use write_all"),
    }
    .unwrap();
    serde_json::to_string_pretty(&v).unwrap() + "\n"
}

pub fn write_all(dir: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    for (k, n) in [(SchemaKind::Profile, "profile"), (SchemaKind::Preset, "preset"), (SchemaKind::Config, "config"), (SchemaKind::Event, "event"), (SchemaKind::Manifest, "manifest")] {
        std::fs::write(dir.join(format!("{n}.schema.json")), schema_json(k))?;
    }
    Ok(())
}
```
Add `pub mod schemas;` to `lib.rs`. In `args.rs` change `PresetsCmd::Schema`'s field to:
```rust
        #[arg(value_enum)]
        which: engine::schemas::SchemaKind,
```

- [ ] **Step 4: Implement the verbs**

Append to `crates/cli/src/verbs.rs`:
```rust
pub fn presets(cmd: PresetsCmd) -> Result<i32, EngineError> {
    let engine = Engine::new()?;
    let reg = engine.registry();
    match cmd {
        PresetsCmd::List { json } => {
            for p in reg.selectable() {
                if json {
                    println!("{}", serde_json::json!({"id": p.id, "version": p.version, "target": p.target, "description": p.description, "suffix_aliases": p.suffix_aliases}));
                } else {
                    println!("{:<18} {:<8} {:<8} {}", p.id, p.version, p.target.map(|t| format!("{t:?}").to_lowercase()).unwrap_or_default(), p.description);
                }
            }
            Ok(0)
        }
        PresetsCmd::Show { id, subtype, target } => {
            reg.chain(&id).map_err(|e| EngineError::Usage(e.to_string()))?;
            let target: Target = target.into();
            let subtypes: Vec<Subtype> = match subtype {
                Some(s) => vec![serde_json::from_value(serde_json::Value::String(s.clone())).map_err(|_| EngineError::Usage(format!("--subtype {s}: unknown")))?],
                None => Subtype::ALL.to_vec(),
            };
            for st in subtypes {
                let profile = reg.profile(&id, target, st).map_err(|e| EngineError::Usage(e.to_string()))?;
                let mut prov = std::collections::BTreeMap::new();
                for (source, patch) in reg.layers(&id, st).map_err(|e| EngineError::Usage(e.to_string()))? {
                    let mut paths = vec![];
                    engine::presets::leaf_paths(&patch, "", &mut paths);
                    for p in paths {
                        prov.insert(p, source.clone());
                    }
                }
                println!("{}", serde_json::to_string_pretty(&serde_json::json!({"subtype": st, "profile": profile, "provenance": prov})).unwrap());
            }
            Ok(0)
        }
        PresetsCmd::Schema { which, out } => {
            match (which, out) {
                (engine::schemas::SchemaKind::All, Some(dir)) => engine::schemas::write_all(&dir).map_err(|e| EngineError::Fatal(e.to_string()))?,
                (engine::schemas::SchemaKind::All, None) => return Err(EngineError::Usage("schema all needs --out <dir>".into())),
                (k, _) => print!("{}", engine::schemas::schema_json(k)),
            }
            Ok(0)
        }
    }
}

pub fn encoders() -> Result<i32, EngineError> {
    let v = encoders_sys::versions();
    println!("bc7e (bc7enc_rdo {}) patch {}", v.bc7enc_rdo, v.bc7e_patch.unwrap_or("none"));
    println!("rgbcx (bc7enc_rdo {})", v.bc7enc_rdo);
    println!("ISPC Texture Compressor {} (BC6H)", v.ispctc);
    println!("ISPC {} objects; dispatched ISA on this CPU: {}", v.ispc, engine::manifest::dispatched_isa());
    let nv = std::path::Path::new(r"C:\Program Files\NVIDIA Corporation\NVIDIA Texture Tools\nvcompress.exe");
    println!("NVTT: {} (backend arrives in milestone M5)", if nv.exists() { format!("found at {}", nv.display()) } else { "not found".into() });
    Ok(0)
}
```
Dispatch in `main.rs`: `Command::Presets(c) => verbs::presets(c)`, `Command::Encoders => verbs::encoders()`.

- [ ] **Step 5: Generate the committed schemas and run the tests**

Run: `cargo run -p sdc -- presets schema all --out schema; cargo test -p engine --test schemas; cargo test -p sdc --test cli_presets`
Expected: 5 files written; both tests PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/engine crates/cli schema
git commit -m "feat(cli): presets list/show/schema, encoders verb; committed JSON Schemas with drift test"
```

---

### Task 44: CLI edge cases — exit-code precedence, panic isolation, read-only outputs, non-ASCII paths

**Files:**
- Test: `crates/cli/tests/cli_edges.rs`, `crates/engine/tests/exit_codes.rs`

**Interfaces:**
- Consumes: Tasks 37, 40–43 (`SDC_TEST_PANIC_ON` hook from Task 37).

- [ ] **Step 1: Write the tests**

`crates/engine/tests/exit_codes.rs`:
```rust
use engine::api::*;

#[test]
fn precedence_partial_over_validation_over_success() {
    let mut s = BatchSummary::default();
    assert_eq!(s.exit_code(), 0);
    s.up_to_date = 3;
    assert_eq!(s.exit_code(), 0);
    s.validation_failed = 1;
    assert_eq!(s.exit_code(), 4);
    s.skipped = 1;
    assert_eq!(s.exit_code(), 1);
    assert_eq!(EngineError::Usage("x".into()).exit_code(), 2);
    assert_eq!(EngineError::Fatal("x".into()).exit_code(), 3);
}
```
`crates/cli/tests/cli_edges.rs`:
```rust
use assert_cmd::Command;
use image::{ImageBuffer, Rgb};
use std::path::Path;

fn sdc() -> Command {
    let mut c = Command::cargo_bin("sdc").unwrap();
    c.env("APPDATA", std::env::temp_dir().join("sdc-test-appdata-none"));
    c
}

fn img(p: &Path) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    ImageBuffer::from_fn(16, 16, |x, y| Rgb([x as u8 * 16, y as u8 * 16, 128])).save(p).unwrap();
}

#[test]
fn a_panicking_file_fails_alone() {
    let d = tempfile::tempdir().unwrap();
    img(&d.path().join("in/boom.png"));
    img(&d.path().join("in/fine.png"));
    sdc()
        .env("SDC_TEST_PANIC_ON", "boom.png")
        .args(["convert", "--set", "subtype=diffuse", "-o"])
        .arg(d.path().join("out"))
        .arg(d.path().join("in"))
        .assert()
        .code(1)
        .stderr(predicates::str::contains("internal error"));
    assert!(d.path().join("out/fine.dds").exists());
    assert!(!d.path().join("out/boom.dds").exists());
}

#[test]
fn read_only_output_fails_at_write_and_non_ascii_paths_work() {
    let d = tempfile::tempdir().unwrap();
    img(&d.path().join("in/ärmor/ïron.png"));
    let out = d.path().join("out");
    sdc().args(["convert", "--recursive", "--set", "subtype=diffuse", "-o"]).arg(&out).arg(d.path().join("in")).assert().code(0);
    let dds = out.join("ärmor/ïron.dds");
    assert!(dds.exists());
    let mut perm = std::fs::metadata(&dds).unwrap().permissions();
    perm.set_readonly(true);
    std::fs::set_permissions(&dds, perm.clone()).unwrap();
    sdc()
        .args(["convert", "--recursive", "--on-existing", "overwrite", "--set", "subtype=diffuse", "-o"])
        .arg(&out)
        .arg(d.path().join("in"))
        .assert()
        .code(1)
        .stderr(predicates::str::contains("Write"));
    #[allow(clippy::permissions_set_readonly_false)]
    perm.set_readonly(false);
    std::fs::set_permissions(&dds, perm).unwrap();
}

#[test]
fn output_root_that_is_a_file_is_fatal() {
    let d = tempfile::tempdir().unwrap();
    img(&d.path().join("in/a.png"));
    std::fs::write(d.path().join("outfile"), b"x").unwrap();
    sdc().args(["convert", "--set", "subtype=diffuse", "-o"]).arg(d.path().join("outfile")).arg(d.path().join("in")).assert().code(3);
}
```

- [ ] **Step 2: Run them**

Run: `cargo test -p engine --test exit_codes; cargo test -p sdc --test cli_edges`
Expected: all PASS. If the read-only case exits 0, `tempfile::persist` replaced a read-only file: on Windows `MoveFileEx` with replace fails on read-only targets, so a pass of 0 means the attribute was not set — check `set_readonly`.

- [ ] **Step 3: Manual Ctrl-C check**

Run a large conversion (any folder with a few dozen 2K–4K PNGs, e.g. `F:\ai\dds\bench\images` copied 5 times) with `cargo run --release -p sdc -- convert --set subtype=diffuse -o target/ctrlc-out <folder>` and press Ctrl-C after a few files. Expected: "cancelling…", `done: … N cancelled`, exit code 1, and no `*.tmp` files under `target/ctrlc-out` (`Get-ChildItem -Recurse -Filter *.tmp target/ctrlc-out` prints nothing).

- [ ] **Step 4: Commit**

```bash
git add crates/cli crates/engine
git commit -m "test(cli): exit-code precedence, panic isolation, read-only outputs, non-ASCII paths, fatal output root"
```

---

### Task 45: MVP exit gate — all gates in one command, header oracle spot check, a real mod folder

**Files:**
- Modify: `crates/bench/src/gates.rs` (`run("all", …)`), `crates/bench/src/main.rs` (`--gates` flag)
- Modify: `docs/DECISIONS.md`

**Interfaces:**
- Produces: `sdc-bench --gates` (= `gate all`): s3, rgbcx, bc6h, alpha-scan, exact-opaque (native if `BC7E_EXACT_OPAQUE_NATIVE`, else bytepatch), engine-parity, full-chain.

- [ ] **Step 1: Add the aggregate**

In `gates::run` add:
```rust
        "all" => {
            let mut v = vec![];
            for g in ["s3", "rgbcx", "bc6h", "alpha-scan", "engine-parity", "full-chain"] {
                v.extend(run(g, root)?);
            }
            v.push(exact_opaque(root, if encoders_sys::BC7E_EXACT_OPAQUE_NATIVE { ExactMode::Native } else { ExactMode::BytePatch })?);
            v
        }
```
In `crates/bench/src/main.rs` make the subcommand optional and add the flag:
```rust
#[derive(Parser)]
#[command(name = "sdc-bench", about = "sdc regression harness over F:/ai/dds/bench (SDC_BENCH_ROOT)")]
struct Cli {
    /// Run every gate (same as `gate all`).
    #[arg(long)]
    gates: bool,
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

fn print_reports(r: anyhow::Result<Vec<gates::GateReport>>) -> ExitCode {
    match r {
        Ok(reports) => {
            let ok = reports.iter().all(|r| r.pass);
            for r in &reports {
                println!("{r}");
            }
            if ok { ExitCode::SUCCESS } else { ExitCode::from(1) }
        }
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::from(3)
        }
    }
}
```
and at the top of `main`: `let cli = Cli::parse(); if cli.gates { return print_reports(gates::run("all", &root)); }`, then `match cli.cmd { Some(Cmd::Gate { name }) => print_reports(gates::run(&name, &root)), …, None => { eprintln!("use --gates or a subcommand"); ExitCode::from(2) } }` (other arms unchanged, wrapped in `Some(…)`).

- [ ] **Step 2: Run everything**

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --release
cargo run --release -p sdc-bench -- --gates > target/bench/mvp-gates.txt; $LASTEXITCODE
```
Expected: fmt/clippy clean, all tests pass, `--gates` exit code 0.

- [ ] **Step 3: Header oracle spot check (local tools, no download)**

Convert the golden fixtures once to a folder (`cargo test -p engine --test golden` writes to temp dirs; instead run `sdc convert` on `tests/golden`-like inputs of your choice), then run on five outputs (BC1 legacy, BC4 DX10, BC5, BC6H, BC7 `_SRGB`):
```powershell
& "C:\Program Files\NVIDIA Corporation\NVIDIA Texture Tools\nvddsinfo.exe" <file.dds>
& F:\ai\dds\bench\tools\dl_texconv\texconv.exe -nologo -y -f R8G8B8A8_UNORM -o target/oracle <file.dds>
```
Expected: `nvddsinfo` reports the format, size and mip count sdc wrote; `texconv` decodes every file without error. (The pinned DirectXTex `texdiag` oracle job is milestone M7.) Record any disagreement as a bug before continuing.

- [ ] **Step 4: Convert a real mod folder (owner chooses the folder)**

Ask the owner for a source-texture folder from a real mod (PNG/TGA/TIFF). Run:
```powershell
cargo run --release -p sdc -- convert "<owner folder>" -o target/mvp-out --recursive --target cs --accept-suggestions high --json > target/mvp.jsonl
$LASTEXITCODE
cargo run --release -p sdc -- validate --target cs (Get-ChildItem -Recurse target/mvp-out -Filter *.dds).FullName
```
Expected: exit 0 or 1 (1 = some files need a decision — list them with `Select-String '"kind":"file_skipped"' target/mvp.jsonl`), never 3; `validate` exits 0. Show the owner the skipped list and the warning-code counts.

- [ ] **Step 5: Record and commit**

Append to `docs/DECISIONS.md`:
```markdown
## Phase 2 exit — CLI MVP (<date>)

- `sdc-bench --gates`: PASS (report target/bench/mvp-gates.txt).
- Header spot check with nvddsinfo/texconv: <ok / issues>.
- Real folder <name>: <n> inputs, <c> converted, <s> skipped (top reasons: …), <f> failed; validation: <ok / codes>.
- Ready for owner review; next milestones: M2.5 channel packing, M3 GUI, M4 variance bake, M5 NVTT, M6 presets/inputs, M7 CI, M8 packaging.
```
```bash
git add crates/bench docs/DECISIONS.md
git commit -m "test: MVP exit gate - all bench gates, header spot check, real mod folder"
```

---

### Task 46: Owner-run in-game spikes S1, S2, S6, S8 (asset pack + checklist)

These spikes need the owner's Skyrim SE/AE install with Community Shaders and cannot run in CI. Defaults they could change are already decided (DECISIONS #2–5, #7), so they do not block Phases 1–2; they feed open items §14 #6–9 and the in-game checklist of milestone M7.

**Files:**
- Modify: `crates/bench/src/main.rs` (`GenSpikeAssets { out }` subcommand)
- Create: `docs/spikes/in-game-checklist.md`
- Modify: `docs/DECISIONS.md`

**Interfaces:**
- Produces: `sdc-bench gen-spike-assets --out <dir>` writing `s2/small_<n>_<size>.png` (200 images, 16–128 px, varied content) and `s8/m_<alpha>_m.png` (64×64, RGB (80,160,40), constant alpha 245, 248, 250, 252, 255, plus `s8/heightmask_m.png` with black RGB and an alpha gradient).

- [ ] **Step 1: Implement the generator**

Add to `crates/bench/src/main.rs`:
```rust
    /// Write the S2 small-texture and S8 _m variant source PNGs.
    GenSpikeAssets {
        #[arg(long)]
        out: std::path::PathBuf,
    },
```
```rust
        Cmd::GenSpikeAssets { out } => {
            let sizes = [16u32, 32, 64, 128];
            std::fs::create_dir_all(out.join("s2")).unwrap();
            std::fs::create_dir_all(out.join("s8")).unwrap();
            for n in 0..200u32 {
                let s = sizes[(n % 4) as usize];
                let img = image::ImageBuffer::from_fn(s, s, |x, y| image::Rgba([(x * 255 / s) as u8, (y * 255 / s) as u8, (n * 37 % 256) as u8, 255]));
                img.save(out.join(format!("s2/small_{n:03}_{s}.png"))).unwrap();
            }
            for a in [245u8, 248, 250, 252, 255] {
                image::ImageBuffer::from_fn(64, 64, |_, _| image::Rgba([80u8, 160, 40, a])).save(out.join(format!("s8/m_{a}_m.png"))).unwrap();
            }
            image::ImageBuffer::from_fn(64, 64, |x, _| image::Rgba([0u8, 0, 0, (x * 4) as u8])).save(out.join("s8/heightmask_m.png")).unwrap();
            println!("wrote {}", out.display());
            ExitCode::SUCCESS
        }
```

- [ ] **Step 2: Write the checklist**

`docs/spikes/in-game-checklist.md`:
```markdown
# In-game spike checklist (owner-run)

Record: Skyrim version, CS version, ENB/CPM state, date. Paste screenshots into docs/spikes/<spike>/.

## S1 — BC6H `_g` and `_SRGB` `_g`/`_f` under CS
1. Take a TruePBR set you know. Convert `_g` twice:
   `sdc convert <set> -o out/s1a --target cs --accept-suggestions high` (BC6H_UF16, default)
   `sdc convert <set> -o out/s1b --target cs --accept-suggestions high --set "file:**/*_g.png.format=bc7" --set "file:**/*_g.png.encoder.backend=bc7e" --set "file:**/*_g.png.color.tag=srgb"`
2. Convert `_f` as `_SRGB` (default) and as UNORM (`--set "file:**/*_f.png.color.tag=unorm"`).
3. Screenshot both variants with linear lighting on and off.
Pass: BC6H and `_SRGB` `_g` look alike in both LL modes, no load errors; `_SRGB` `_f` matches the authoring reference and the UNORM twin is brighter.

## S2 — small BC7 textures
`sdc-bench gen-spike-assets --out spike-src`, then `sdc convert spike-src/s2 -o out/s2 --set subtype=diffuse --target cs`.
Assign the 200 outputs to common meshes (e.g. replace clutter textures) and play 10 minutes. Pass: no crash.

## S6 — plain `_m` with alpha, flag off (PGPatcher 50% rule)
Collect ≥ 20 real plain `_m` sources from your mods (not committed to the repo). Run
`sdc convert <folder> -o out/s6 --set subtype=envmask --set format=bc7 --set encoder.backend=bc7e --set alpha.policy=data --set alpha.exact_opaque=false --allow ALPHA-EXACT-OFF-M --json > s6.jsonl`
Record per file `validation.alpha[0]` (share of α ≠ 255 = 1 − eq255/pixels) from the `file_done` events.
Pass/decision: if any file ≥ 40%, `ALPHA-EXACT-OFF-M` becomes blocking (open item §14 #9).

## S8 — CS complex-material detection boundary
`sdc convert spike-src/s8 -o out/s8 --target cs --accept-suggestions high --json > s8.jsonl` (each `_m` resolves to `complex_material`; 255 resolves to `envmask`).
Check `VAL-M-COMPLEX-MARGIN` warnings and the smallest-mip alpha in `validation.alpha` for each file, then load each as the `_m` of a test mesh.
Pass: complex shading/parallax on for 245–250, off for 252–255; the margin (open item §14 #7) is set from the observed boundary.
```

- [ ] **Step 3: Generate the pack and hand it over**

Run: `cargo run --release -p sdc-bench -- gen-spike-assets --out target/spike-src`
Tell the owner where the pack and checklist are; ask them to run S1, S2, S6, S8 and report results.

- [ ] **Step 4: Record and commit**

When the owner reports, append each result to `docs/DECISIONS.md` (`## Spike S1 …`, etc.) in the same shape as the S3 entry, then:
```bash
git add crates/bench docs
git commit -m "docs: in-game spike checklist (S1, S2, S6, S8) and asset generator"
```

---

# Later milestones (to be expanded into their own plans)

Each milestone gets its own `writing-plans` document when its entry criteria are met.

### M2.5 — Channel packing (spec §6.4, DECISIONS Note H)

Expanded into `docs/superpowers/plans/2026-10-09-sdc-m2-5-channel-packing.md` (Tasks 1–9); decisions in DECISIONS Note M.

Owner timing: its own milestone **right after the CLI MVP**, before the GUI. This section fixes its scope.

- **Entry:** Phase 2 exit (Task 45); Task 27a in place (raw maps are discovered, grouped under their role id and resolved as `JobStatus::PackingInput` with `IN-PACKING-INPUT`).
- **Scope:**
  - New module `crates/engine/src/pack.rs`: per set and target, builds the recipes of spec §6.4 (`cs` `_rmaos` / `_rfaos` / `_p`; `cpm` complex `_m`; `vanilla` `_m` env mask only when a metallic or specular source exists) from the set's raw inputs; triggers (roughness or gloss, or — owner rule — AO or metallic, so missing roughness never blocks); per-channel source or neutral default (AO 1, metallic 0, specular A 1 = TruePBR default, roughness `packing.roughness_default` 0.5 (confirmed by the owner 2026-10-09), env mask from metallic else specular, `cpm` `_m` height 0.5 **[Open]**); gloss ↔ roughness = 1 − x in both directions; R (or luma) of each source, source alpha ignored; equal dimensions required; float32 throughout so 16-bit sources keep precision.
  - Virtual jobs with several sources: the packed image is assembled after Load and enters the normal pipeline at Preprocess with the packed sub-type's profile (mips per that sub-type, encode, write, validate, §10 checks). `ResolvedJob.input: PathBuf` gains a multi-source form (e.g. `sources: Vec<PackSource { path, channel, op }>` with `input` = the first source) so `FileFacts`, rule globs (matched against the packed output's name) and `--set file:` scopes work; `Engine::plan` runs `pack` between Group and Resolve.
  - **Global fill values (owner requirement, 2026-10-09, DECISIONS Note H last bullet; spec §3.2, §6.4, §14 #22):** for every packed channel without a source map the user can set a global fill value, as a slider in the GUI and as a CLI option (e.g. `--fill metallic=255` writes 255 instead of the neutral 0). Shipped shape (Note M5): profile field `packing.fill.<role>` (a §4.4 raw-map role id, 8-bit code 0–255, resolved like any field so presets, configs and `--set` can set it), CLI `--fill <role>=<0..255>` as shorthand for the unscoped `--set`, GUI slider per role in the global settings (the GUI half lands with M3's settings panel). A fill replaces the neutral default (and `packing.roughness_default`) only for a channel with no source map and never overwrites a mapped channel; provenance is `fill:<v>` where the user supplied a value and `default:<v>` where the neutral default applied, in `set_resolved`, `file_planned` and the manifest. A filled channel raises no `PACK-DEFAULTED` (the user chose it).
  - Policy: new profile fields `packing.enabled` (default `true`), `packing.roughness_default` (0.5), `packing.prefer` (`prepacked` / `raw`) in `config.rs`, `sdc-base.toml`, `FIELD_PATHS` and the schema; raw maps stay `PackingInput` when packing is off; a raw map whose sub-type a rule or `--set` names is an ordinary file and leaves every recipe.
  - Conflicts: a pre-packed source of the same output (`iron_rmaos.png` + `iron_roughness.png`, `iron_p.png` + `iron_height.png`, `iron_m.png` + raw maps under `cpm`/`vanilla`) → recipe status `conflict`, `PACK-CONFLICT`; the pre-packed file wins by default (`packing.prefer = prepacked`).
  - Codes (catalogue 52 → 56; it was 51 → 55 before the final review added `IN-FLOAT-LDR`): `PACK-DEFAULTED` (warning, names output and channel), `PACK-SIZE-MISMATCH` (error, lists every source and size), `PACK-CONFLICT` (warning), `PACK-UNUSED-INPUT` (warning: e.g. metallic next to a fuzz source, fuzz under `cpm`, opacity, specular-only sets).
  - Events and manifest: `set_resolved` lists recipes (output, sub-type, per-channel source or `default:<v>`, status); `file_planned` carries all sources; the manifest record lists every source with its SHA-256 and per-channel provenance; `if-changed` re-encodes when any source changes.
  - CLI: `sdc inspect` prints each set's recipes and defaulted channels; `convert --dry-run` shows the virtual jobs; Task 37's `PackingInput` arm stays for `packing.enabled = false`.
  - Interplay: a packed `_rmaos`/`_rfaos` is a roughness member for M4's variance bake; under `cs` it makes the set PBR (already true from Task 27a's `is_pbr_set`).
- **Exit:** golden cases per recipe and target — `cs` roughness + metallic + AO + specular → `_rmaos` equal to a reference packing of the sources (bit-exact before encode, decoded within the format's per-mip floor); AO-only set → `_rmaos` with roughness 0.5, metallic 0, specular 1 and one `PACK-DEFAULTED` per defaulted channel (provenance `default:<v>`); the same set with `--fill metallic=255` → metallic 255 with provenance `fill:255`, the other channels unchanged; fuzz set → `_rfaos`; height → `_p` BC4; gloss-only `cs` set → roughness = 1 − gloss; `cpm` → complex `_m` whose smallest-mip alpha stays below the complex-detection limit; `vanilla` metallic → `_m` envmask, height-only `vanilla` set → `PACK-UNUSED-INPUT`; size mismatch → `PACK-SIZE-MISMATCH` naming both files; pre-packed conflict → `PACK-CONFLICT` and the pre-packed output; 16-bit sources keep > 8-bit precision into the encoder input; manifest lists all sources. Open items §14 #17–#22 answered by the owner or carried with their shipped defaults (#16 is closed).
- **Carried items** (from the final review and the SDD ledger):
  - Virtual jobs have several sources, but per-file events are keyed by the input path alone (`input` = first source); leave room for the M3 job key so one input path (`iron_roughness.png` feeding `_rmaos`, `iron_height.png` feeding `_p`) can belong to two outputs (final review I5).
  - The plan/run source-drift check (size and mtime recorded in `ResolvedJob`, final review I7) must cover every source of a virtual job, not only `input`.
  - Test that a raw map whose sub-type a rule or `--set` names leaves every recipe and converts under its source stem (Task 27a minor).
  - `Provenance` has no `default:`/`fill:` variant yet; extend the provenance vocabulary and the schemas together (spec §5.2, §6.4).
  - Spike S8 (DECISIONS): a packed `cpm` `_m` is complex in CS only if its smallest-mip RGB is not grey (some pair of channels ≥ 4/255 apart) or is black, and its alpha is below 251/255. A constant or grey env-mask fill can make the RGB grey; add a golden case where `VAL-M-GRAYSCALE` fires for that, and one where the default recipe keeps the `_m` complex.

### M3 — GUI (`sdc-gui`, project-plan Phase 4)

- **Scope:** spec §9: source list grouped by set with confidence badges and blocking highlights; settings panel with the two labelled colour fields; preset inspector with provenance and diff; advanced panel; GPU preview (wgpu `TEXTURE_COMPRESSION_BC`, sources as RGBA16F, mip slider, channel isolation, split/diff heatmap, smallest-mip alpha, coverage per mip, normal length, BC6H peak readouts); batch progress and report export; engine in-process over the same `EngineEvent` channel; isolated-worker mode (spawns `sdc.exe convert --json`) shipped **off**.
- **Entry:** Phase 2 exit and M2.5; event schema stable (`schema/event.schema.json`, including M2.5's recipe fields) after the single schema bump of the carried items below (job key, typed `job`, skip kind).
- **Exit:** usability pass with modders; preview max 8192 by default, 16384 as a setting, smaller-mip fallback when the adapter lacks 16384 (open item §14 #13); crash rate from Phases 3–4 recorded for the isolated-worker decision (DECISIONS #18).
- **Known constraint to design for:** `rgbcx::init` is process-global (Task 4 `InitError::ModelMismatch`), so a GUI that changes the BC1 decoder model between batches must restart its encoder process or use the isolated worker.
- **Carried items** (final review I5, I6 and §4; SDD ledger):
  - Partial-Profile schema (Task 43 I1, deferred): `preset.schema.json` and `config.schema.json` type `defaults`, `subtypes` and `overrides` as `true` because they are `serde_json::Value`, so editors validate nothing inside them; generate a partial `Profile` schema (every field optional) and reference it through `schemars` `schema_with`. Also encode the threshold range 5–200 and mip count ≥ 1 (Task 19 minor).
  - I5, one `EVENT_SCHEMA` bump: give every per-file event a stable job key (the `manifest::output_key` or a job id) instead of the input path alone; type `file_planned.job` (derive `Deserialize` for `ResolvedJob` and embed its schema, so the drift test sees it); add an enum skip kind (`up_to_date`, `on_existing_skip`, `unresolved`, `blocked`, `duplicate`, `dds_input`, `packing_input`, `unknown_existing`) in place of prose in `reason`.
  - I6, additive: a per-file allow (`BTreeMap<PathBuf, BTreeSet<Code>>`, or `allow` scoped like `--set`) so acknowledging `FMT-BC5-NORMAL` on one file does not acknowledge it on all; and an exact-path scope (`Scope::Path`, matched on the canonical input path), because `file:` globs treat `[`, `]`, `{`, `}` and `*` as syntax and match the basename when the glob has no `/` (ruling of Task 25).
  - The GUI must pass a `CancelToken` to both `Engine::plan` and `Engine::run` (public API change accepted at the Codex-CLI fix); a token cannot be reset, so create one per batch.
  - `rgbcx` init is global (Known constraint above): a BC1 decoder-model change needs a fresh process or the isolated worker.
  - `Plan` is consumed by `run` and is not `Clone`, so "run again" re-plans and reloads every image; keep the `Analysis` in the plan (an option raised in final review I7) so per-file edits can re-resolve without reloading.
  - `Reporter` holds its ordering lock across `sink.emit`, so a slow GUI sink back-pressures every worker; the GUI sink should only push into a channel.
  - The per-file stage column of spec §9 needs per-stage events (m15, listed under M7; pull it forward if the column ships in M3).
  - `BatchRequest` is an open `pub` struct that M2.5, M5 and M6 extend: document `BatchRequest::new` plus field mutation as the contract. `EngineError` is three string variants, so the GUI cannot point at the offending config field.
  - `manifest::append` does not normalise `output`; callers must pass `output_key` (Task 36 minor), which matters once the GUI is a second caller.
- **Consumed by M3a** (built on `feat/m3a-gui`, 2026-10-10; plan `docs/superpowers/plans/2026-10-10-sdc-m3a-converter-gui.md`):
  - Partial-Profile schema (Task 3), except that its "threshold range 5–200 / mip count ≥ 1" sub-item is still open; the I5 event-schema bump with job key, typed `file_planned.job` and skip kind (Tasks 4–5); I6 per-file allow and exact-path scope (Tasks 2–3).
  - `CancelToken` per batch, plan and run (Tasks 11–15); `Analysis` kept for re-resolving (the session cache, Task 7); the GUI sink only pushes into a channel (Task 11).
  - `manifest::append` as a second caller: unchanged, because the GUI calls `run`, which appends with `output_key`.
  - From "Carried from M2.5": the event-identity items (Task 5: pack-source terminal events, job key), `SourceRef` roles and invert (Task 6) and the `file:` scope on packed outputs (Task 2's key rule: the folder of the first source plus the virtual name).
  - The BC1 decoder model is read once at launch; a change in the GUI shows "restart sdc-gui to apply".
- **Still open after M3a** (unchanged): the isolated-worker mode (M3a ships in-process only; DECISIONS #18 decided after the owner's usability pass); per-stage progress events and the per-file stage column (m15, M7); set membership editing and PGPatcher JSON (M3b); texture edits (M3c); documenting `BatchRequest::new` plus field mutation as the contract and a structured `EngineError`; the non-event "Carried from M2.5" items below (wording, code shape, rare paths, unspecified behaviour, test gaps).
- **Carried from M2.5** (final review of `feat/m2.5-packing`, SDD ledger):
  - **Event identity (I5):**
    - Only `sources[0]` of a packed job emits `file_planned`. A healthy raw source consumed by a recipe, including a failed or cancelled one, gets no per-file terminal event.
    - At the I5 bump, record each pack source's role and invert flag in `SourceRef`. This structurally replaces the `pack.*` provenance comparison that `is_up_to_date` now does.
  - **File scope on packed outputs:** the packed output's virtual input name takes the first source's extension (`pack/plan.rs`). So `--set 'file:**/iron_rmaos.png.…'` misses when the roughness source is `.tga`. Either match `file:` scopes on the stem for virtual inputs, or document `iron_rmaos.*`.
  - **Wording:** the duplicate-role reason says "another … source is used: <first>" even when the first map is itself unused, as with two `_opacity` maps. Reword it to "comes first".
  - **Code shape:**
    - `assemble(job, gray)` takes `gray` separately from `job.gray`; read `job.gray` inside instead.
    - Raw maps are decoded three times (pass-1 Load/Analyse, pass-2 assembly, run). Pass 1 could check headers and decode without `analyse`.
  - **Rare paths:**
    - A pass-2 `PlanFailure::Pack`, from a source resized between passes, emits no coded warning, and under `prefer = raw` writes neither file.
    - A pre-packed file that failed to load lets the recipe run under `prefer = prepacked`. This is defensible; add one sentence to Note M9.
  - **Unspecified:**
    - A pre-packed `_rmaos` next to raw maps that include fuzz writes both `_rmaos` and `_rfaos`; precedence between recipes is not specified.
    - A recipe output can overwrite a DDS input of the same name when `--output` is the input folder.
  - **Test gaps:**
    - `FillSpec::get`: only 2 of 8 roles are tested, and NaN `roughness_default` is untested.
    - The post-decode size mismatch lists only two files.
    - No test for: `inspect --fill`, fill losing to a scoped `--set`, a `cpm` set without height, a pass-2 panic or Load failure.
- **Carried from M3a** (final review and SDD ledger of `feat/m3a-gui`; fix first in M3b):
  - **Fix first:** a stale `after_export` survives a second close/open request made while a cancelled export winds down, and can later close the window or switch folder unasked (`crates/gui/src/model/mod.rs` `run_after_export`, `resolve_export_running`, `request_close`/`request_open`; reset it in `open_folder` and on "Keep running").
  - **Model and threads:** every edit spawns a full plan (debounce; skip building a cancelled plan's view); `Panicked` carries no generation; `busy()` includes preview work (affects the threshold slider's pending value and the "No textures" hints); `check_sources` and thumbnail hashing run on the UI thread; f32→f16 conversion and GPU upload run on the UI thread (a 16384² upload has no OOM error scope); the analysis/pack cache is unbounded; columns are not re-planned after an export (no "up to date" shown).
  - **Edge behaviour:** `plan_failed`'s user-config fallback drops unsaved edits and can unlock a still-bad `sdc.toml`; an I2 rejection of a GUI edit blames `sdc.toml` (unreachable through bounded controls); the set view's pending click is not keyed by set; the drawer can be closed mid-run with no way to reopen it; Ctrl+S saves while a modal is open; modal Esc/backdrop is ignored; the Subtype combo remembers its first choice; the heatmap toggle persists across textures; the size combo shows 16384 when it is not allowed; a hand-written extension-less `[files]` key and the GUI's key for the same file become two entries.
  - **Previews:** a DDS input with no engine `Format` (uncompressed, BC2, signed BC6H) shows "cannot be previewed" (DECISIONS N11); the debounce is stamped on the next tick; mip-0 alpha checks read the first kept mip in a size-limited preview (documented); no decoded-pixel LRU (spec §4.4 note).
  - **Engine nits:** `SkippedFile::kind()` falls through to `Duplicate`; `invert_of` uses `any` (per-channel invert lost if one file feeds two channels); `file_entries` parsed three times; `..` in roots not resolved (`std::path::absolute`); stale doc comments (api.rs `file_allow` "(Task 3)", `is_up_to_date` "the channel each feeds", `SourceRef` role/invert, codes.rs doubled "and").
  - **Tests to add:** late-preview-result drop deterministically; precedence of the threshold/mip schema sub-item above; untested error paths (`[files]` invalid sub-type, `finish_profile` failure, `file_allow` ∪ `[files].allow`); packed-job thumbnail; `preview_source`; BC5 normal-length branch; lock assertions for `bc1_transparent_black`/`rgbcx_level`; OpenUnsaved Cancel/Save-failure; export cancel / ExportDone error / export panic; tonemap/view_linear exclusivity; the mip slider UI path.
  - **Hygiene:** `value_text` duplicates `display_value`; `field_views` recomputed per group; `set_view` repeats the member-thumbnail code; settings ignore save errors; `.sdc-tmp` left if a rename fails; `remove_fill` can drop a `[defaults]` header comment.

### M4 — Texture-set variance bake (spec §6.2–6.3)

- **Scope:** `mips.normal = renormalize_bake`; `_n` base normals kept as a summed-area table; per roughness-member level: footprint average length L → κ → v → α′² = saturate(α² + min(2v, t²)), t = **0.25** (tunable per preset); 1:1 and 2:1 size ratios; `SET-BAKE-RATIO`, `SET-COMPANION-OUTSIDE`, `SET-COMPANION-DDS`, `SET-MISSING-COMPANION`; `set_resolved` with `bake: true`; set ordering (`_n` before roughness members) in the scheduler.
- **Entry:** Phase 1 mips and scheduler.
- **Exit:** golden cases `_n` + `_rmaos` and `_n` + `_rfaos` at 1:1 and 2:1 assert roughness widening matches a reference implementation of the formula (f64) within 1/255; renormalise-only paths unchanged.
- **Carried items:**
  - `mips::build_chain` does not read `p.mips.normal` (`RenormalizeBake` degrades to `Renormalize`); M4 must read it (Task 29).
  - The four `SET-*` codes (`SET-MISSING-COMPANION`, `SET-COMPANION-OUTSIDE`, `SET-COMPANION-DDS`, `SET-BAKE-RATIO`) are catalogued but never emitted until this milestone (final review m11).
  - Normal and roughness channels are alpha-weighted if a cutout policy is forced on them; check Validate and the bake path (Task 29 minor).
  - The source-drift check (size and mtime, final review I7) must also cover the `_n` companion that feeds a bake.
  - The retained `_n` summed-area table counts against the `--mem-budget` admission; see the 16K peak memory carry under M6 (Task 29 minor).
  - **Coverage defects found by mipcheck round 2** (`F:/ai/dds/mipcheck/RESULTS-2.md` B1/B6; fix first in M4, or earlier if the owner wants):
    1. **Overshoot on tied alpha.** `coverage::nearest_group` never offers "no texel passes" as a candidate and can only choose whole groups of tied alpha. On thin strands the coverage overshoots up to ×32.
       - Smallest reproduction: a 4×4 image with alpha 255 at (0,0), (0,2) and (2,0). Mip 1 passes 3 of 4 texels where k = 1.
       - Fix: add 0 passes as a candidate; leave a level unchanged when the best count is more than 2× off k; optionally break ties spatially (dither) so exact coverage is reachable.
    2. **Threshold-edge loss after BC7.** The remap puts the anchor exactly on the first passing code (128 for `ge 128`), and BC7 error drops it below. Results: 8² mip 35 → 0 on the sparse image; Fence007A at 16² goes 94 → 78.
       - Fix: put the anchor 1–2 codes above the threshold, or re-check coverage after encoding.
  - B2 confirms the `SignedRg` accumulation path gives the length before normalising (M4's Toksvig input). It also left an owner question: should mip 0 be renormalised? Several real `_n` sources are as short as |n| = 0.34.

### M5 — NVTT as an optional external process (spec §7.5)

- **Scope:** detection (configured path, then the default install path), version check (3.2.5 tested; others `ENC-NVTT-VERSION` blocking), per-mip feeding per S7's verdict (or mip 0 only with an event), BC7 and BC6H Production, temporary RGBA8/RGBA16F files, `ENC-NVTT-MISSING`; NVTT as the second exact-opaque fallback.
- **Entry:** S7 recorded (Task 16).
- **Exit:** NVTT outputs pass §5.8; per-level PSNR equals direct encodes within 0.01 dB; BC6H peak ratio ≥ 0.95 on `hdr_real` venice_sunset (bench: NVTT keeps peaks).
- **Carried items:**
  - Always pass `-nocuda` for BC7 Production (DECISIONS Spike S7); the finding was hand-measured on one image, so add a repeatable measurement (Task 16 minor).
  - The BC6H half of S7 (`-bc6` on RGBA16F levels) was never run; S7 fed 8-bit levels, M5 feeds the float chain (Task 16 minor).
  - mipcheck round 2 (`F:/ai/dds/mipcheck/RESULTS-2.md` B3–B5), for nvcompress 3.2.5:
    - It reads a DX10 `R32G32B32A32_FLOAT` DDS **silently as zeros**. Feed it a legacy-header float DDS or `.hdr`, and add a mip-0 round-trip check.
    - `-normal` normalises the encoded [0,1] vector, not the signed normal, so never use it for `_n` levels.
    - Its `kaiser` is not the nvtt_export Kaiser.
    - It does not produce BC6H black rings.
  - S7's alpha check ran on opaque `rgb_real` only and does not prove the `-noalpha` effect; test it on an image with varying alpha (Task 16 minor).
  - `ENC-NVTT-VERSION` is catalogued but never emitted; `ENC-NVTT-MISSING` fires only when the NVTT backend is selected (final review m11).

### M6 — Presets completion and remaining inputs

- **Scope:** built-ins "CS – Compact", "Vanilla – Compact", "Fast preview" (bc7e `fast`, smallest-mip validation only); `sdc presets diff/export/validate`; `--preset-file` for the `presets` verbs (user presets and their suffix aliases, Task 26a); schema migrations for older `schema_version`; user rule tables; inputs OpenEXR, Radiance HDR, BMP, JPEG, and the float-source transfer split for 32-bit float TIFF (see Carried items); DDS re-encode opt-in (`--set file:<glob>.reencode=true`, `LOSS-REENCODE`); NPOT resize policy (`resize=pot-down`, polyphase in the content's domain); tent/Mitchell/Kaiser filters and `mips.edge`; ICC-profile info event; progress bars.
- **Entry:** Phase 2 exit.
- **Exit:** preset tests per target; golden cases for each new input type; preset changelog entries.
- **Owner additions (2026-10-10, DECISIONS Note O):**
  - **Uncompressed DDS, as output and input (O1).**
    - Output: new `format` values for uncompressed DDS:
      - `rgba8` (UNORM/SRGB per `color.tag`), written as `B8G8R8A8` (legacy header where `header=auto` allows);
      - `r8`, `rg8`;
      - `rgba16f` for float content.
      - No BC encoder backend runs for these; full mips; same validation, minus the BC checks.
    - Input: read the uncompressed layouts spec §1 already lists (RGBA8, BGRA8, BGRX8, R8, RG8, RGBA16F, RGBA32F) for inspect, validate, re-encode and GUI preview. This also ends the M3a limit N11 ("cannot be previewed").
    - Tests:
      - one golden case per format;
      - texconv and nvddsinfo read every output;
      - a re-encode round trip.
  - **HDR check before BC6H for `pbr_emissive` (O2).** BC6H is chosen only for a real HDR source: float (16F/32F EXR, HDR, float TIFF or float DDS) **and** a peak above 1.0 in any colour channel. Otherwise the default is `bc7` with tag `srgb` (`BC7_UNORM_SRGB`).
    - The choice is an analysis fact (`analysis:hdr-source`). It sits below a suffix fact and `--set`, so `--set file:<glob>.format=bc6h_uf16` still forces BC6H.
    - Inspect, plan and the GUI show the reason (e.g. "8-bit source, no HDR → BC7 sRGB").
    - A float source that peaks at or below 1.0 depends on the float-source transfer split (Carried item below).
    - Tests:
      - 8-bit `_g` → BC7_UNORM_SRGB;
      - float `_g` with peak > 1 → BC6H;
      - float `_g` with peak ≤ 1 → BC7 sRGB;
      - an explicit `--set` override wins in both directions.
    - Ask the owner whether `hdr_sky` should get the same check.
  - **Owner idea to evaluate (2026-10-10): CPM `_m` without parallax.**
    - **The idea:** a `cpm` `_m` given as a file (not packed) whose alpha holds no height is forced to alpha 255 today (exact opaque). That makes it a plain env mask, so PGPatcher and the CS complex-material gate don't treat it as CPM.
      - Instead, write a **constant alpha 0** (owner).
      - **Why 0 and not 128** (owner, confirmed against the S8 DXBC in `out/s8-renderdoc.md`):
        - Parallax runs per pixel only where 4/255 < alpha < 251/255, so alpha 0–4 means "complex material, no parallax".
        - 128 sits inside the parallax window, so it runs parallax on a flat height, and any alpha noise becomes visible parallax.
      - **Complex-material gate:** the smallest-mip alpha must be ≤ 250. 251–255 is complex off (255 also reads as a plain env mask to PGPatcher).
      - **Caveat from the shader:**
        - alpha 0 keeps complex material on **only if the smallest-mip RGB is not grey** (pairwise differences ≥ 4/255);
        - the black-RGB exception needs 4 < alpha < 251, so a grey or black `_m` at alpha 0 is complex off;
        - `VAL-M-GRAYSCALE` already flags the grey case.
      - **Done (owner yes, 2026-10-10, M3b branch Task 3b):** the packed no-height default is 0.
      - Keep the exact-opaque option for real env masks.
    - **Env-mask sub-types:** offer or force **BC4**, or uncompressed (Note O1), for a single-channel env mask.
    - **To check first:**
      - which channel the vanilla env-mask shader samples (BC4 stores R only);
      - PGPatcher's own complex-material test on a constant alpha (the "≥50% non-white" rule, DECISIONS line 46, still Open).
- **Carried items** (final review, SDD ledger):
  - Tent, Mitchell and Kaiser mips (mipcheck `RESULTS-2.md` B8):
    - Build each level **directly from mip 0**, with the kernel widened 2^N, and keep float values between levels.
    - Measured: recursion costs 1.5–2.5 dB (tent), 1–1.5 dB (Mitchell) and ≤ 0.5 dB (Kaiser) against a Lanczos-8 reference. 8-bit levels in between add up to 5 codes of drift with Kaiser.
    - Box stays recursive; it is identical either way.
  - Float source → LDR colour output needs a source-transfer versus output-encode split (Task 39 carry). It replaces the interim blocking `IN-FLOAT-LDR` guard and its `LOSS-CLAMP-HDR` use added at the final review (I4); keep the golden cases for float TIFF diffuse and float BC6H.
  - `FIELD_PATHS` lacks `reencode` and `resize` (Task 25 carry); spec §8 promises `--set file:<glob>.reencode=true`, which is a usage error today.
  - Double work per file (m16): every converted file is decoded twice (plan, then run) and hashed twice when an output exists; reuse the plan-time load or hash for `is_up_to_date`.
  - Serial hashing on reruns (Task 37 M8): hash the sources of an `if-changed` rerun in parallel.
  - `IN-NONFINITE` is not reported by `--dry-run` or `inspect` (m7, Task 37 M5); `analyse` already visits every sample and can count them. Its mean and `normal_len_err` are also poisoned by NaN (Task 23 minor).
  - Project config discovery reads `sdc.toml` from the first input only, and only when it is a folder (m10): document it or warn when another input root contains an `sdc.toml`.
  - 16K peak memory is about 13 GiB (Task 29 minor), and Preprocess is single-threaded with a per-channel `powf` (Task 28 minor); revisit admission and the scheduler together.
  - Discovery (m6, Task 21 minors): one unreadable subfolder aborts the whole `convert` as a usage error where spec §8 says `file_failed` and continue; the same file given twice, or a folder plus a file at its top level, becomes `IN-DUPLICATE` for both copies (dedupe on the canonical path, add a test); symlinks are skipped silently.
  - Load: record the DXGI format of a DDS input in `Analysis` for the DDS re-encode opt-in (Task 23 carry).
  - Revisit the BC4/BC5 search radius default (5, as the bench ran; 10 gains about 0.09 dB on real masks) when the presets are completed (Task 6 ruling).
  - Preset and rule tests: `add_toml` silently overwrites a duplicate or built-in id and `merge_json` inserts null for absent keys (Task 20 minors); rule overrides still apply when `--set` changes the sub-type, and the precedence edge cases lack tests (Task 25 minors); the landscape suffix match is too broad, and `hdr_sky` is also suggested under `interface/` while its reason text says `sky/` (Task 24 minors); a deferred `CFG-INVALID` on an unavailable profile is untested on the fallback path (Task 39 M1/M2); an explicit `flip_green` that disagrees with `green_convention` is unwarned (Task 27a minor).

### M7 — Quality validation and CI (project-plan Phase 6)

- **Scope:** GitHub Actions jobs from spec §2 (`check` with `cargo deny`, `test` with schema drift, `oracle` with pinned DirectXTex `texdiag`/`texconv` — a download needing approval, `determinism` with a `pinned-isa` feature backed by a single-target ISPC object set that `regen-ispc.ps1` must learn to build, `bench-gates` on a CC0 subset committed under `tests/corpus/` via Git LFS, manual `regen-ispc`); fuzzing of the DDS reader and loaders; per-mip metrics, linear-light thin-line test, normal angular error, coverage comparison against DirectXTex `--keep-coverage` (spec §11.3); in-game verification checklist (spec §11.5) on a fixed CS install.
- **Entry:** M6 (or Phase 2 for the CI subset).
- **Exit:** all gates green in CI; open items §14 #2 (compare), #3 (coverage for cutout_blend), #4 (t), #8 (BC6H peak threshold) decided from evidence.
- **Carried items:**
  - `dispatched_isa` in the manifest is derived from CPUID (`manifest.rs`); under `pinned-isa` it must come from `encoders-sys` (Task 36 carry).
  - Bit-identity of flag-off bc7e output was proven only on the host ISA (avx2); verify the sse2, sse4 and avx objects on other hardware or with a forced ISA in the determinism job (Task 14 carry).
  - Per-stage progress events (m15): `Progress` is emitted only for Compress, so the GUI's per-file stage column cannot be rendered (also listed under M3).
  - Task 34 minor: the DDS reader's BC1 decode with a model gives alpha 255 for a 3-colour block's index 3, where `bcdec` gives 0; this matters for third-party BC1 with transparency. The DX10 mask branch also ignores a signed `pf_flags`.
  - Bench-dependent tests (m1): `crates/bench/tests/*` hard-fail without `F:\ai\dds\bench`; `dds_read.rs` reads only `SDC_BENCH_ROOT` with no default, so its bench part never runs; `cli_inspect_validate.rs` skips silently off-machine. Use one shared `bench_root()` helper and the `tests/corpus` subset.
  - Bench metrics: `mpsnr` `max(0)` swallows NaN, empty input divides by zero, and `mpsnr` is tested only at identity (Task 8 minors); the S3 gate does not assert per-set image counts, so a deleted PNG shrinks it silently (Task 9 minor).

### M8 — Packaging (project-plan Phase 7)

- **Scope:** final tool name (DECISIONS Note F), both exes as single static binaries, `LICENSES/`, `THIRD_PARTY_NOTICES.md`, schemas, user guide, NVTT detection docs, Nexus page; code signing and antivirus false-positive testing (DECISIONS #16).
- **Entry:** M7 green.
- **Exit:** release candidate.
- **Carried items:**
  - Licensing hygiene before any public push (m13): add a licence file and copyright line to `spikes/s4-scalar/vendor/` (or drop the vendored copy and keep the URL and hash in DECISIONS), consider SPDX headers in `crates/*/src`, and configure `cargo about` or `cargo deny` or drop the reference in `THIRD_PARTY_NOTICES.md`.
  - Pin the `rust-toolchain.toml` channel at the first release (Task 2 minor); add a repo-wide eol rule to `.gitattributes` (Task 2 minor).
  - `SHA256SUMS` is written CRLF by `hash-vendor.ps1`, so `sha256sum -c` fails; the Intel licence year in the notices reads 2016-2019 against the licence's 2017 (Task 3 minors).

### Out of scope here

Spec §13 future extensions (PGPatcher PBR JSON generator, RDO, reserved-header hash, cubemap assembly, NIF-aware alpha, BSA, MO2/Synthesis) — not planned until the owner asks.

---

# Self-review notes (writing-plans checklist)

**Spec coverage (Phases 0–2):** §1 inputs PNG/TGA/TIFF 8/16 (Task 22; others M6), outputs BC1/3/4/5/6H/7 (Tasks 5–7, 32–33), mirror tree + manifest (Tasks 21, 36–37); §2 workspace, static CRT, no ISPC at build (Tasks 2–4), CI (M7); §3.1–3.2 config model (Task 19), §3.3 alpha policies + exact opaque (Tasks 13–14, 27–30, 32, 35), §3.4 colour fields (Tasks 19, 26, 28, 31), §3.5 presets (Task 20; Compact/Fast preview M6), §3.7 preset suffix aliases (Task 26a; surfaced in Tasks 37, 39–43), §4.4 exporter suffix vocabulary and green convention (Task 27a; `PackingInput` arm in Task 37), §3.6 manifest/sidecar (Task 36–37); §4 sub-type tables (Task 20), availability (Task 26), suggestions (Task 24); §5 stages (Tasks 21–37); §6 grouping (Task 21), bake (M4), channel packing §6.4 (M2.5); §7 vendoring, licences, threading, effort levels (Tasks 3–7, 32), NVTT (S7 Task 16, M5), scalar fallback (S4 Task 15); §8 CLI verbs/options/exit codes/edge cases (Tasks 40–44); `presets diff/export/validate` (M6); §9 GUI (M3); §10 codes (Task 18; triggers in Tasks 26, 35, 37); §11.1 golden (Task 39), §11.2 gates (Tasks 9–11, 13–14, 38, 45), §11.3–11.5 (M7, Task 45 spot check); §12 spikes (Tasks 9, 12–17, 46).

**Decisions this plan makes where the spec is silent** (merged into spec §3.2, §5.8 and §8 at the final review, 2026-10-09; kept here for the record; all called out in the report): `header`/`mips.coverage` gain `auto`; `encoder.rgbcx_level` field; `mips.edge` deferred; codes `VAL-MIP-ERROR` and `CFG-INVALID`; coverage check only on levels ≥ 256 texels; analysis-derived `remove` applies without `--accept-suggestions`; unresolved outranks blocked in job status; up-to-date files do not count as "skipped" for exit code 1; default output root `./sdc-out`; validation PSNR floor 30 dB; status of `_e` suffix = no suggestion; an invalid preset/config/rule value and an unscoped `--set` that is unavailable under the target are usage errors (exit 2) found in `plan`; MVP renormalises `_n` without the M4 variance bake; Task 27a: colour vocabulary written `<base>.dds` while `_d` keeps its stem, raw roughness/gloss/metallic/AO inputs mark a `cs` set PBR before packing exists, raw maps are `PackingInput` (not counted for exit 1), the green convention is not settled by `--allow SUG-LOW-CONFIDENCE`, and Resolve reads the vocabulary term from `FileFacts.input.rel` (no new `FileFacts`/`SuffixSplit`/`SuggestContext` fields).

# Skyrim SE/AE DDS texture converter: design specification

| Field | Value |
|---|---|
| Date | 2026-10-08 |
| Status | Approved direction. Phases 0–2 (spikes S3–S7, engine, CLI MVP) are built on `feat/phase0-2` and reviewed; milestone M2.5 (channel packing, §6.4) is built on `feat/m2.5-packing`. Re-synced with the code and DECISIONS at the final review, 2026-10-09 |
| Project licence | GPL-3.0-or-later |
| Platform | Windows x64 only |
| Working name | `sdc` (Skyrim DDS Converter): `sdc.exe` (CLI) and `sdc-gui.exe` (GUI); final name decided at packaging (§14) |
| Builds on | The plan's approved "Recommended starting direction" [PLAN], amended by the owner's answers [D] |

**Tags.** Every substantive statement carries one tag, as in the plan. **[Requirement]**: decided by the owner (plan requirements or `DECISIONS.md`). **[Finding]**: backed by research notes, local benchmarks or a fetched source, cited. **[Proposal]**: a design choice made here. **[Open]**: undecided; the shipped default and the trigger that changes it are stated, and the item is listed in §14.

**Citations.** [PLAN] `F:/ai/dds/reports/Skyrim DDS converter project plan.md`; [D] `F:/ai/dds/converter/docs/DECISIONS.md` (answers [D#n], notes [D-A], [D-B], [D-C], [D-G], [D-H]); [B1] `reports/BC7 encoder benchmark results.md`; [B2] `reports/BC4 BC5 BC6H and BC7 data benchmark.md`; [MIP] `reports/Mipmap filters and alpha coverage.md`; [SC], [EI], [LF] the notes `skyrim_conventions.md`, `encoder_integration.md`, `language_frameworks.md` in `research_notes/Skyrim DDS converter project plan/`; [TPM] jiayev/TruePBR-Manager (README, `SPEC.md` §3.10, §6–8, `src/core/PBRTextureSet.cpp`, `master` branch, fetched 2026-10-08).

**Changes from the plan.** The owner's answers override or extend the plan in seven places. (1) The tool serves texture artists converting source images; it is not a DDS→DDS game-folder fixer. (2) Alpha *Remove* drops the channel before mips [D#8]. Because bc7e can still decode such blocks as 254 [D-A, D-C], the plan's "constrained bc7e" option becomes a per-profile *exact opaque alpha* flag backed by a small bc7e patch, on by default for `_m` only (global default decided: S5 measured an RGB cost above 0.05 dB, so it stays off elsewhere [D, Phase 0 exit]), with byte-patch and NVTT fallbacks and smallest-mip validation as proof (§3.3, §5.6, §5.8). (3) The ≤128 px BC7 fallback is dropped [D#7]. (4) BC4/BC5 headers, `_SRGB` under vanilla, `_g`/`_f` colour space and BC6H under CS are decided [D#2–5]. (5) Normal mips are renormalised, with a variance bake into roughness when the set has an `_rmaos` or `_rfaos` [D#6, D-B]. (6) `_rfaos` is a real CS map (Roughness, Fuzz, AO, Specular) for advanced skin [D-B]. (7) Windows only [D#12].

---

## 1. Purpose and scope

**[Requirement]** `sdc` converts artists' source images into DDS textures that load and render correctly in Skyrim Special/Anniversary Edition. A GUI and a CLI share one engine. The user selects one of three **target pipelines**:

| Target | Meaning |
|---|---|
| `cs` — Community Shaders (**default**) | CS including TruePBR (`textures/pbr/**`), CS Extended Materials complex `_m`, hair `_flow`, BC6H emissive |
| `cpm` — CPM / ENB complex material | Non-PBR shading; complex `_m` (R env, G gloss, B metal, A height); landscape diffuse alpha as parallax height |
| `vanilla` — Vanilla SE/AE | Bethesda shaders: no `_SRGB`, no BC6H, no complex `_m`; landscape diffuse alpha as snow mask |

**Inputs.** **[Requirement]** Artists' source images: PNG (8/16-bit), TGA, TIFF (8/16-bit, 32-bit float), OpenEXR, Radiance HDR, BMP and JPEG, as single files or folders. DDS input is secondary: read for preview, inspection, validation and, on explicit request, re-encoding [D, scope]. **[Proposal]** The DDS reader accepts BC1–BC7 with legacy (`DXT1`–`DXT5`, `ATI1`, `ATI2`, `BC4U`, `BC5U`) or DX10 headers, the common uncompressed layouts (RGBA8, BGRA8, BGRX8, R8, RG8, RGBA16F, RGBA32F) and cubemaps.

**Outputs.** **[Requirement]** DDS in BC1, BC3, BC4, BC5, BC6H or BC7 for SE/AE, tagged per target; Legendary Edition is out of scope [PLAN §1]. **[Proposal]** The output tree mirrors the input tree under an output root, each output keeps its source stem (`wood01_n.png` → `wood01_n.dds`), and a batch manifest records what was done (§3.6). Two exceptions rename the output to the Skyrim name: a preset suffix alias (§3.7) and an exporter vocabulary suffix (§4.4, e.g. `iron_basecolor.png` → `iron.dds`, `iron_normalgl.png` → `iron_n.dds`). **[Requirement]** Outputs always use Skyrim names [D-H].

**The user decides.** **[Requirement]** Path and suffix inference *proposes* a category, sub-type, preset and defaults; the user always decides category, encoder and settings [D, scope]. **[Finding]** Suffixes are convention only, `_s` has several meanings, and diffuse alpha meaning depends on the mesh's NiAlphaProperty, which the converter cannot see [SC §1]. Low-confidence proposals therefore block conversion until confirmed (§8, §9).

**Non-goals for v1.** **[Proposal]** Bulk fixing of installed game folders, mesh/NIF patching (PGPatcher's job), LOD generation, BSA I/O, NIF-aware alpha detection, cubemap assembly from face images, FaceGen tint generation, Legendary Edition output. **[Requirement]** No non-Windows builds [D#12].

**Quality model.** **[Finding]** Format choice moves quality 5–15 dB and Skyrim's semantic errors are binary, while top BC7 encoders sit within 0.16–0.32 dB [B1 §1, §6], [SC §2, §6]. **[Proposal]** Priorities: (1) semantic correctness per sub-type and target; (2) correct mips; (3) the best measured encoder at practical speed.

---

## 2. Repository and crate layout

**[Proposal]** One Cargo workspace at `F:/ai/dds/converter`:

```text
converter/
├── Cargo.toml, rust-toolchain.toml (stable, x86_64-pc-windows-msvc), .cargo/config.toml (+crt-static)
├── crates/
│   ├── engine/         library: config model, policy, pipeline, mips, DDS I/O, validation, events
│   ├── encoders-sys/   vendored encoders: prebuilt ISPC objects, rgbcx and ISPC TC host C++, C shims, patches
│   ├── cli/            sdc.exe (clap)
│   ├── gui/            sdc-gui.exe (eframe on wgpu); milestone M3, not yet created
│   └── bench/          sdc-bench: regression harness over F:/ai/dds/bench
├── presets/            built-in presets (TOML), embedded at compile time
├── schema/             generated JSON Schemas (preset, profile, config, event, manifest, dds-info, dds-validation)
├── crates/engine/tests/golden.rs   per sub-type × target cases; the sources are synthesised in the test, no images are committed
├── tests/corpus/       CC0 bench subset (Git LFS); milestone M7, not yet created
├── LICENSES/, THIRD_PARTY_NOTICES.md
└── docs/               DECISIONS.md, specs, CREDITS.md, user guide
```

| Unit | What it does | How it is used | Depends on |
|---|---|---|---|
| `engine` | Configuration model, presets, suffix rules, pipeline stages (§5), texture-set jobs (§6), DDS reader/writer, decoders, validation, scheduler, event stream | Linked by `cli`, `gui`, `bench` through `Engine::plan(BatchRequest, &Reporter, &CancelToken) -> Result<Plan, EngineError>` and `Engine::run(Plan, &Reporter, &CancelToken) -> Result<BatchSummary, EngineError>`; a `Reporter` wraps an `Arc<dyn EventSink>` (§5.9) | `encoders-sys`; `image` (PNG, TGA, TIFF; `exr` joins with OpenEXR input in M6), `half`, `bcdec_rs`, `rayon`, `serde`, `toml`, `serde_json`, `schemars`, `sha2`, `hex`, `globset`, `walkdir`, `humantime`, `thiserror`. Schemas are checked by regenerating them and comparing with the committed files, so `jsonschema` is not a dependency |
| `encoders-sys` | Safe wrappers over bc7e (BC7), rgbcx (BC1/3/4/5 encode, BC1 decode per decoder model) and ISPC TC (BC6H); reports vendored versions | Called only by `engine::encode`; one-time global init (§7.3) | Committed ISPC objects and headers; rgbcx and ISPC TC host C++ built by `build.rs` with the `cc` crate and MSVC; C shims for the C++ APIs |
| `cli` | Commands, config and preset loading, human log on stderr, JSON Lines on stdout, exit codes | Users, scripts, CI, the GUI's isolated-worker mode | `engine`, `clap`, `ctrlc`, `anyhow`; `indicatif` progress bars arrive in M6 |
| `gui` | Source list, controls, preset inspector, GPU preview, progress, reports | Users | `engine` in-process, `eframe`/`egui`, `egui_wgpu`, `wgpu` (`TEXTURE_COMPRESSION_BC`), `rfd` |
| `bench` | Re-runs the bench corpora through the engine; compares with recorded results; per-mip metrics and alpha counters | Local and self-hosted CI; the gates of §11.2 | `engine`; `F:/ai/dds/bench` (`SDC_BENCH_ROOT`, default that path), its `results/results.csv`, `results2.csv` and image sets. There is no fallback yet: the committed subset `tests/corpus` and portable gates are M7, and without the bench folder the bench crate's tests fail |
| `presets/` | Built-in presets as data | Embedded with `include_str!`, schema-validated at start-up | `engine` schema types |

**Bench link.** **[Finding]** `F:/ai/dds/bench` holds the corpora, a reference decoder and about 1,770 recorded encodes [PLAN §11]. **[Proposal]** `sdc-bench` reads it in place and writes only to `target/bench/`; the CC0 gate images will also be committed to `tests/corpus/` for hosted CI in M7.

**Build requirements.** **[Requirement]** Rust stable on `x86_64-pc-windows-msvc`. **[Finding]** The GNU target would need ISPC from source or a `__chkstk`/`_fltused` shim, and prebuilt ISPC kernels exist for windows-msvc but not windows-gnu [EI §1, §2, §7]. **[Proposal]** Contributors need Rust and the MSVC Build Tools. **No ISPC at build time:** the bc7e objects (patched source, §7.1) and ISPC TC objects are built once with pinned ISPC 1.31.0 by the manual `regen-ispc` workflow and committed with SHA-256 hashes. A static CRT makes each binary a single exe.

**CI outline.** **[Proposal]** GitHub Actions on `windows-latest`:

| Job | Runs | Fails when |
|---|---|---|
| `check` | `cargo fmt --check`, `clippy -D warnings`, `cargo deny` | Lint, non-GPL-compatible licence, advisory |
| `test` | Unit and golden tests (§11.1); regenerated schemas equal committed ones | Failure or drift |
| `oracle` | Pinned DirectXTex `texdiag`/`texconv` cross-check of golden outputs (§11.4) | Header or decode disagreement |
| `determinism` | Golden set encoded twice with the `pinned-isa` feature; hashes compared with committed values | Any byte difference |
| `bench-gates` | `sdc-bench --gates` on the committed subset (§11.2) | Gate outside tolerance |
| `regen-ispc` (manual) | Rebuilds ISPC objects with ISPC 1.31.0 | SHA-256 differs from the manifest |
| `release` (tag) | Packages both exes, licences, schemas | — |

---

## 3. Configuration model

**[Requirement]** One configuration model is the single source of truth for CLI and GUI. **[Proposal]** It is defined once as Rust types with `serde` and `schemars`, so the published JSON Schema and load-time validation cannot drift [LF §3].

### 3.1 Resolution

```text
Target pipeline × Category × Skyrim sub-type  →  Profile
cs|cpm|vanilla    Color|Normal|Mixed-Packed|Grayscale|HDR     (tables in §4)
```

**[Proposal]** *Category* is the coarse user-facing choice; *sub-type* is the Skyrim slot meaning (`pbr_albedo`, `normal`, `rmaos`, `flow`, …) and carries the rules. Each sub-type belongs to one category; choosing a category narrows the sub-type list. Some sub-types exist only under some targets (§4.2); an unavailable combination is a blocking error for that file, never a silent substitution.

### 3.2 Profile fields

| Field | Values | Default | Notes |
|---|---|---|---|
| `target` | `cs` / `cpm` / `vanilla` | Batch | Changes defaults and validation strictness |
| `category`, `subtype` | §4.1 | Suggester, confirmed by user | |
| `color.content` | `srgb` / `linear` | Sub-type | **Explicit**: transfer of stored values; drives the mip domain |
| `color.tag` | `srgb` / `unorm` / `n/a` | Sub-type × target | **Explicit**: DXGI suffix in the header; `n/a` for BC6H |
| `alpha.policy` | `remove` / `cutout` / `blend` / `cutout_blend` / `data` | Sub-type + analysis | §3.3 |
| `alpha.threshold` | 5–200, **set per texture** | **127** [D#10] | `cutout`, `cutout_blend`; each image carries its own value (GUI per-row field, CLI per-file override / job-file entry) [Requirement] |
| `alpha.compare` | `gt` / `ge` | `gt` **[Open]** | Shared by coverage counter and solver |
| `alpha.exact_opaque` | bool | `true` for `envmask`, `complex_material`; `false` elsewhere (**[Requirement]** owner, Phase 0 exit: `_m` / env masks only) | All-255 source blocks decode to exactly 255 (§3.3) |
| `channels` | `swizzle`, `flip_green`, `reconstruct_z`, `grayscale_source: r / luma` | Sub-type | Preprocess (§5.4) |
| `channels.green_convention` | `directx` / `opengl` / unset | Unset; a `_normalgl`/`_normaldx` suffix states it (§4.4) | Green (Y) convention of a normal source. `opengl` makes Resolve set `flip_green = true` (OpenGL → DirectX in Preprocess, §5.4). A plain `_normal`/`_nrm` source stays unresolved until a preset, config, rule or `--set` states it [D-H]. Ignored for `msn` and non-Normal sub-types |
| `mips.levels` | `full` / `none` / `n` | `full`; `none` for `interface/`, `sky/` | Full chain to 1×1 |
| `mips.filter` | `box` / `tent` / `mitchell` / `kaiser{width,alpha,stretch}` | `box` | Mitchell and Kaiser for Color only. Only `box` exists in the engine so far; the others arrive in M6 |
| `mips.edge` | `clamp` / `wrap` | `wrap` | Irrelevant to box at power-of-two [MIP]. Not a `Profile` field yet; it arrives with the other filters in M6 |
| `mips.normal` | `renormalize` / `renormalize_bake` | `renormalize`; set job may upgrade (§6) | Normal category |
| `mips.coverage` | `auto` / `on` / `off`, **per texture** | `auto` = on for `cutout` and `cutout_blend`; switchable per image [Requirement] | §5.5. Applies only to `cutout` and `cutout_blend`: on `blend`, `data` or `remove` the field is a no-op even when set on, because blended alpha keeps box-averaged opacity (§5.5) |
| `format` | `bc1`/`bc3`/`bc4`/`bc5`/`bc6h_uf16`/`bc7` | Sub-type × target | Always unsigned |
| `header` | `auto` / `dx10` / `legacy` | `auto` = `legacy` for BC1/BC3 UNORM, else `dx10`. An explicit `legacy` on BC6H, BC7 or any `_SRGB` is `CFG-INVALID` rather than silently becoming DX10 | BC4/BC5 legacy = `BC4U`/`BC5U`; `ATI1`/`ATI2` never written [D#2] |
| `encoder.backend` | `bc7e` / `rgbcx` / `ispc_bc6h` / `nvtt` | Format | §7 |
| `encoder.effort` | Named levels (§7.4) | bc7e `veryslow` (= u5); rgbcx 18; BC6H `veryslow` | |
| `encoder.perceptual` | bool | `false` | Locked off unless Color with alpha ≠ `data` |
| `encoder.bc1_transparent_black` | bool | `false` | rgbcx 3-colour transparent-black mode; `true` with a BC1 output is `CFG-INVALID` in v1 (DECISIONS Notes F, K) |
| `encoder.bc45_search_radius` | `default` / `max` | `default` | `max` = radius 10 |
| `encoder.rgbcx_level` | 0–18 | 18 | rgbcx quality level for BC1/BC3/BC4/BC5; read only when the backend is rgbcx, where a value above 18 is `CFG-INVALID` |
| `validation` | `min_mip_psnr_db`, `coverage_tolerance_pts`, `complex_m_warn_alpha`, `bc6h_min_peak_ratio`, `pgpatcher_non_white_share` (§5.8) | 30 dB, 2 points, 247, 0.8, 0.5 | The thresholds behind `VAL-MIP-ERROR`, `VAL-COVERAGE`, `VAL-M-COMPLEX-MARGIN`, `VAL-BC6H-PEAK` and `VAL-PGPATCHER-50` |
| `packing` | `enabled`, `roughness_default`, `prefer: prepacked / raw`, `fill` (per-role values) | `true`, 0.5 (**[Requirement]** confirmed 2026-10-09 [D-H]), `prepacked`, none | Packed outputs only (§6.4); shipped in M2.5. **[Requirement]** The user can set a **global fill value for each packed channel that has no source map** (GUI slider, CLI option), which replaces the neutral default for that channel [D-H]. **[Decided]** (plan M2.5, DECISIONS Note M5) Field `packing.fill.<role>` (a raw-map role id of §4.4, e.g. `metallic`), 8-bit code 0–255; the CLI shorthand is `--fill <role>=<0..255>` (e.g. `--fill metallic=255`), equivalent to an unscoped `--set packing.fill.<role>=…` |
| `output` | `path`, `on_existing` | Batch; path `./sdc-out` when no `-o` is given | §8 |

**[Proposal] Batch-level settings.** `bc1_decoder_model` (default `cBC1Ideal`, **[Requirement]** owner 2026-10-08; NVIDIA / AMD / IdealRound4 selectable) is batch-level because `rgbcx::init` sets it as global, non-thread-safe state [EI §1]; a conflicting preset value raises an info event. `jobs`, `mem_budget`, the NVTT path and the preview limit are also batch-level.

### 3.3 Alpha policy

| Policy | Meaning | Preprocess | Mips | Formats |
|---|---|---|---|---|
| `remove` | Alpha carries nothing | **Channel dropped before any mip or encode step** [D#8] | RGB only | BC1, BC7, BC4/BC5, BC6H |
| `cutout{t}` | Alpha-tested at *t* | Dilate RGB under α = 0 | Alpha-weighted RGB; box alpha; coverage preserved | BC7, BC3 |
| `blend` | Straight-alpha blending | Dilate RGB under α = 0 | Alpha-weighted RGB; box alpha | BC7, BC3 |
| `cutout_blend{t}` | Mesh tests and blends [D#10] | Dilate RGB under α = 0 | Alpha-weighted RGB; box alpha; coverage preserved at *t* by default, switchable per image [Requirement] | BC7, BC3 |
| `data` | Independent data (spec, height, weight, F0) | — | Separate box; never weighted | BC7, BC3 |

**[Requirement]** Cutout versus blend is chosen per image; threshold default 127, common range 5–200 [D#10]. **[Proposal]** For colour sources with non-constant alpha, the suggester proposes `cutout{127}` if ≥ 90% of texels are ≤ 5 or ≥ 250, else `blend`; for diffuse sub-types this is always low confidence because the mesh decides [SC §1]. Sources without alpha, or with alpha constant at 255, resolve to `remove` with high confidence. **[Proposal]** Nothing is premultiplied on disk, since NiAlphaProperty blending is straight alpha [PLAN §2]. BC1 1-bit alpha is not offered in v1. **[Finding]** On cutouts it loses anti-aliased edges and is 6.5–11.9 dB worse in visible RGB than BC7 [B1 §6].

**Exact opaque alpha.** **[Finding]** BC7 mode 6 stores 7-bit endpoints plus one p-bit per endpoint shared by RGB and A, so alpha 255 needs p-bit = 1. bc7e's opaque path (`bc7e.ispc:3909`, `handle_opaque_block`) picks the p-bit for RGB only [B1 §5]. Example: the bench's bc7e `-u6` encode of `rgb_real/Bricks104`, an RGB source with no alpha, decodes to 254 on 21.0% of pixels, in mode-6 blocks [D-C]. NVTT constrains the p-bit, emits no 254 and is within 0.32 dB of bc7e on real colour [B1 §1, §5]. **[Requirement]** The flag `alpha.exact_opaque` makes every block whose source alpha is 255 throughout decode to exactly 255. It is **on by default only for `_m`** (`complex_material`, and `envmask` when written in an alpha-carrying format), because CS complex detection, ENB's "pure white = flat" rule and PGPatcher's ≥ 50% non-white rule read `_m` alpha [SC §4]. It is **off** for ordinary colour, `_n` and data maps, where 254 is harmless, so they pay no quality cost. **[Finding]** S5 measured a mean RGB cost of −0.515 dB (`rgb_real`) and −1.020 dB (`rgb_grad`) for the native patch, above the 0.05 dB condition for proposing it everywhere (DECISIONS, Spike S5). **[Requirement]** The owner therefore kept the scope above: `_m` / env masks only (Phase 0 exit). Widening it would need a cheaper mechanism. **[Proposal]** The flag works per block, so it also covers flat (alpha 255) regions inside a complex `_m`; the patch must constrain every bc7e path that can encode an all-255 block.

### 3.4 Colour space

**[Requirement]** Colour space is shown as two labelled fields: "Content: sRGB-encoded colour / Linear data" and "DDS tag: `_SRGB` / UNORM (linear)" [PLAN §8]. **[Requirement]** `_SRGB` is written only for PBR colour data — albedo, `_g` (when not BC6H), `_f` and `_s` colour — and only under `cs`; never under `vanilla` [D#4, D#5]. **[Proposal]** `cpm` is non-PBR and never writes `_SRGB` either. **[Finding]** Vanilla-path diffuse stores gamma-encoded colour in UNORM; an `_SRGB` tag darkens it in vanilla, CS non-PBR paths and LOD [SC §5, §6]. **[Proposal]** Content transfer drives filtering regardless of tag: vanilla diffuse is filtered in linear light and written UNORM; TruePBR albedo is filtered in linear light and written `_SRGB`.

### 3.5 Presets

**[Proposal]** A preset is a versioned bundle of profile defaults plus an ordered rule table:

| Field | Purpose |
|---|---|
| `schema_version` | Newer refused, older migrated |
| `id`, `version` (semver), `base` | Identity and single inheritance; user presets store only differences |
| `target` | Target the preset serves |
| `subtypes.<id>` | Overrides of any §3.2 field |
| `rules[]` | `{ match: { glob, suffix, set_has, content predicate }, subtype, overrides }`, first match wins |
| `suffix_aliases` | `alias = "canonical"` table: exact filename suffixes bound to a canonical suffix and to this preset (§3.7) |

Built-ins (read-only): "CS – Quality" (default), "CS – Compact" (BC1 wherever alpha is removed), "CPM – Quality", "Vanilla – Quality", "Vanilla – Compact", "Fast preview" (bc7e `fast`, smallest-mip validation only). **[Proposal]** The three Quality presets (`cs-quality`, `cpm-quality`, `vanilla-quality`, all based on the hidden `sdc-base`, an id user presets cannot replace) ship with the CLI MVP; the Compact presets and "Fast preview" land in M6. **Every field is inspectable** (GUI inspector, `sdc presets show`, both with provenance) and **overridable per field** at preset, rule, file or command-line level. Changing a built-in default bumps the preset version and the changelog, because CS behaviour shifts between releases [SC §4].

### 3.6 Reproducibility

**[Requirement]** Preset hash and encoder versions go to a sidecar; a reserved-header hash is deferred [D#15]. **[Proposal]** Each output gets a sidecar record: engine version and commit; encoder names, vendored commits (bc7enc_rdo `b943862`, ISPC TC `79ddbc9`), the bc7e patch id and ISPC 1.31.0; the ISA the ISPC dispatcher selected (today derived from a CPUID check that mirrors the ISPC runtime; `encoders-sys` reports it under `pinned-isa` in M7); preset id, version and SHA-256 of its canonical form; the resolved profile with provenance; the source SHA-256; the batch BC1 decoder model; validation results. By default records go to one batch manifest, `<output_root>/_sdc/manifest.jsonl`, which keeps mod folders clean; a per-file `<name>.dds.json` is optional. Reserved header fields stay zero. **[Finding]** ISPC multi-target dispatch can change bits across CPUs [LF §9]. **[Proposal]** Guarantee: same input, preset, build and dispatched ISA give byte-identical output, enforced in CI with `pinned-isa` (§2).

### 3.7 Preset suffix aliases

**[Requirement]** A preset may declare exact suffix aliases that bind a filename suffix to a canonical sub-type suffix and to that preset's settings, e.g. `rmaosultrahigh → rmaos` [D-G]. Matching is exact, with no separator: `iron_rmaosultrahigh.png` matches only an alias `rmaosultrahigh`, and declaring it does not make `_nultrahigh` match; each alias is declared individually. The file is grouped and written under its canonical suffix (`iron_rmaosultrahigh.png` → set `iron`, output `iron_rmaos.dds`). The alias's preset may define any preset field and is layered like any preset; `--set` and per-file overrides still win. Provenance records the alias. A source whose alias and canonical forms collide on one output (`iron_rmaos.png` and `iron_rmaosultrahigh.png`) is `IN-DUPLICATE`.

**[Proposal]** The details:

| Topic | Rule |
|---|---|
| Field | `suffix_aliases` in a preset file, a table `alias = "canonical"` (e.g. `[suffix_aliases]` with `rmaosultrahigh = "rmaos"`). Config files (`config.toml`, `sdc.toml`) declare none. Built-in presets declare none; aliases come from user and project presets. |
| Validity | An alias is lower-case ASCII letters and digits (`[a-z0-9]+`, no `_`) and must not equal a §6.1 suffix or an exporter vocabulary term (§4.4); its value must be a §6.1 suffix without the `_` (never a vocabulary term or packing role). An alias is unique across all loaded presets. A second preset declaring it, or any invalid entry, is a config error (`CFG-INVALID`, exit 2, §8) and that preset is not loaded. Reloading a preset under the same id replaces its aliases. |
| Activation | The aliases of every loaded preset are active, whether or not it is the batch preset. `--preset <file>` loads and selects a preset; `--preset-file <file>` (repeatable) loads one without selecting it (§8). |
| Matching | The stem segment after the last `_` is looked up, case-insensitively, in the alias table before the exporter vocabulary (§4.4) and the §6.1 list are tried; the segment must equal an alias exactly, and a base name must precede the `_`. Because aliases contain no `_` and never equal a §6.1 suffix or a vocabulary term, a stem has at most one reading. |
| Grouping and naming | Everywhere after Discover, the file counts as having its canonical suffix: set key, the set's suffix list (PBR detection, `_s` disambiguation, the bake), rule `suffix` and `set_has` matches, and the suggester. The output is `<base>_<canonical>.dds` in the mirrored folder. Rule globs still match the source path and name. |
| Layering | For an aliased file, the alias preset's inheritance chain is the preset chain in the §3.5/§8 order: `defaults` tables base → child, then `subtypes.<id>` tables base → child, then user config, project config, rule matches (the alias chain's rules are that file's preset rules), analysis facts and `--set`. An alias preset therefore sets `base` to the preset it refines (e.g. `base = "cs-quality"`); a batch preset that is not in its chain does not apply to the file. |
| Provenance | Fields set by the alias chain carry `preset:<alias preset id>`. The `ResolvedJob` records `suffix_alias = { alias, canonical, preset }`; each suggestion's reason begins `suffix alias _<alias> → _<canonical> (preset <id>)`; the manifest's preset reference (id, version, SHA-256, §3.6) names the alias preset. |
| Duplicates | Output names are canonicalised before grouping, so an alias source and a canonical source (or two aliases of one canonical suffix) on the same base collide on one output and are both `IN-DUPLICATE` (§6.1). |
| Confidence | An alias match is an explicit user declaration of the suffix, so the top suggestion for the canonical suffix is raised to high confidence. `_s` is the exception: its four readings (§4.3 item 2) keep their ranking, and the alias preset adds a rule naming the sub-type to fix one. As for any high-confidence suggestion, the file converts only with `--accept-suggestions high` or when a rule or `--set` decides. |

---

## 4. Suffix and category tables

### 4.1 Sub-types

**[Proposal]** Defaults live as data in `presets/`; formats and tags per target are in §4.2. The `cs` TruePBR defaults follow TruePBR-Manager's per-slot defaults: Diffuse, Subsurface, Fuzz, Coat Colour → BC7 sRGB; Emissive → BC6H UF16; Displacement → BC4 linear; Normal, RMAOS, Coat-Normal-Roughness → BC7 linear [TPM `defaultCompressionForSlot`, SPEC §8.3].

| Sub-type (category) | Cue | Channels | Content | Alpha default | Mips | Encoder | Warnings |
|---|---|---|---|---|---|---|---|
| `diffuse` (Color) | `_d`, a colour vocabulary term (§4.4) or no suffix, non-PBR set | RGB, A per mesh | sRGB | `remove` if no/255 alpha; else proposed `cutout{127}`/`blend`, to confirm | Box, linear light (tent/Mitchell/Kaiser optional) | bc7e veryslow; BC1 rgbcx L18 | `SUG-LOW-CONFIDENCE`, `TAG-SRGB-NONPBR` |
| `landscape_diffuse` (Color) | `textures/landscape/**` | RGB; **A data**: snow mask (vanilla) or height (CS/CPM) | sRGB RGB, linear A | `data`, `remove` locked | RGB linear light; A separate | bc7e veryslow | `FMT-LANDSCAPE-BC1` [SC §4] |
| `pbr_albedo` (Color) | No suffix in a PBR set | RGB, A opacity | sRGB | As `diffuse` | As `diffuse` | bc7e veryslow | `TAG-UNORM-PBR` |
| `pbr_emissive` (Color) | `_g` in a PBR set | RGB emissive | sRGB (8/16-bit) or linear (float) | `remove` | Box, linear float, negatives clamped | ISPC `bc6h_veryslow`; NVTT for strong highlights | `VAL-BC6H-PEAK` |
| `glow` (Color) | `_g`, non-PBR | RGB | sRGB | `remove` | Box, linear light | bc7e / rgbcx L18 | — |
| `pbr_subsurface` (Color) | `_s` in a PBR set | RGB subsurface **or** coat colour; A opacity/strength | sRGB RGB, linear A | `data` | RGB linear light; A separate | bc7e veryslow | §4.3 |
| `pbr_fuzz` (Color) | `_f` | RGB fuzz colour; A weight | sRGB RGB, linear A | `data` | As above | bc7e veryslow | — |
| `subsurface_tint` (Color) | `_s`, no PBR set, no `_msn` | RGB tint | sRGB | `remove` | Box, linear light | bc7e / rgbcx L18 | Low confidence |
| `backlight_tint` (Color) | `_b`, `_bl`, `_sk` | RGB | sRGB | `remove` | Box, linear light | bc7e / rgbcx L18 | `_sk` unconfirmed [SC §1] |
| `ui_sky` (Color) | `interface/**`, `sky/**` | RGB(A) | sRGB | Per content | **None** | bc7e; NVTT suggested for gradients | — |
| `normal` (Normal) | `_n`, non-PBR | XYZ (DirectX); **A specular** | linear | `data` | Box; mips ≥ 1 renormalised; A separate | bc7e veryslow; NVTT optional | `FMT-BC5-NORMAL`; BC1 blocked while A varies |
| `landscape_normal` (Normal) | `textures/landscape/**_n` | As `normal` | linear | `data` | As `normal` | As `normal` | As `normal` |
| `pbr_normal` (Normal) | `_n` in a PBR set | XYZ; A unused | linear | `remove` | Box, renormalised; bake into roughness member (§6) | bc7e veryslow; BC1 option | `FMT-BC5-NORMAL` |
| `msn` (Normal) | `_msn` | **R = X, G = Z, B = Y**; A ignored | linear | `remove` | Box; renormalise length only | bc7e / rgbcx L18 | No green flip or Z rebuild [SC §2]; `FMT-BC5-NORMAL` |
| `coat_normal` (Normal) | `_cnr` (landscape `_crn`) | XYZ; A coat roughness | linear | `data` | RGB renormalised; A separate | bc7e veryslow | `FMT-BC5-NORMAL` |
| `envmask` (Grayscale) | `_m`/`_em`/`_envmask`, alpha absent or 255 | R env mask | linear | `remove`, no alpha written [D#9]; `exact_opaque` on if overridden to BC7 | Box | rgbcx BC4 HQ | `CHAN-NOT-GRAY`, `VAL-PGPATCHER-50` |
| `complex_material` (Mixed-Packed) | `_m` with non-white alpha | **R env, G gloss, B metal, A height** | linear | `data`; `exact_opaque` on | Box, **full chain to 1×1**; A separate | bc7e veryslow, exact opaque | `VAL-M-*` |
| `rmaos` (Mixed-Packed) | `_rmaos` | **R rough, G metal, B AO, A specular F0** | linear | `data`; `remove` proposed if A is 255 | Box per channel; R on α = r²; bake (§6) | bc7e veryslow; NVTT optional; BC1 if A removed | `CHAN-AO-INVERTED`, `CHAN-GLOSS-AS-ROUGH` [SC §3] |
| `rfaos` (Mixed-Packed) | `_rfaos`, CS advanced skin [D-B] | **R rough, G fuzz mask, B AO, A specular F0** | linear | As `rmaos` | As `rmaos`, bake included | As `rmaos` | As `rmaos` |
| `flow` (Mixed-Packed) | `_flow` | Signed RG vector (`*2 − 1`) | linear | n/a | Box after decode; **not** normalised | rgbcx BC5 HQ | `DIM-FLOW-SMALL` |
| `height` (Grayscale) | `_p` | R height; 0.5 neutral in CS PBR | linear | `remove` | Box, never sharpened | rgbcx BC4 HQ | — |
| `msn_specular` (Grayscale) | `_s` with `_msn` in set | R specular | linear | `remove` | Box | rgbcx BC4 HQ | — |
| `generic_two_channel` (Mixed-Packed) | User-assigned | Two data channels | linear | n/a | Box | rgbcx BC5 HQ | Niche use [D#1] |
| `generic_data` (Mixed-Packed) | User-assigned | Up to four data channels | linear | `data` | Box per channel | bc7e veryslow | — |
| `hdr_sky` (HDR) | Float source for sky/HDRI | Linear radiance | linear | `remove` | None under `sky/`, else box | ISPC `bc6h_veryslow`; NVTT if a sun is present | `VAL-BC6H-PEAK` |

**[Requirement]** BC5 is the default only for `_flow`, selectable elsewhere (e.g. an `_rmaos` with all-black AO) [D#1]. BC5 or BC4 on any Normal sub-type raises a blocking warning. **[Finding]** CS reads `_n` blue directly with no Z reconstruction, so BC5 decodes Z = −1 and drops specular alpha; the bench's "BC5 beats BC7" result rebuilt Z, which Skyrim never does [SC §2], [B2 §4].

### 4.2 Format and tag per target

`—` = unavailable (`CFG-UNAVAILABLE`, blocking); the bracketed fallback is offered to the user.

| Sub-type | `cs` (default) | `cpm` | `vanilla` |
|---|---|---|---|
| `diffuse` | BC7_UNORM (Compact: BC1_UNORM if `remove`) | same | same |
| `landscape_diffuse` | BC7_UNORM (A height) | BC7_UNORM (A height) | BC7_UNORM (A snow mask) |
| `pbr_albedo` | **BC7_UNORM_SRGB** (BC1_UNORM_SRGB if `remove`) | — (→ `diffuse`) | — (→ `diffuse`) |
| `pbr_emissive` | **BC6H_UF16** [D#3, TPM]; option BC7/BC1_UNORM_SRGB | — (→ `glow`) | — (→ `glow`) |
| `pbr_subsurface`, `pbr_fuzz` | **BC7_UNORM_SRGB** | — | — |
| `glow`, `subsurface_tint`, `backlight_tint` | BC7_UNORM / BC1_UNORM | same | same |
| `ui_sky` | BC7_UNORM, no mips | same | same |
| `normal`, `landscape_normal`, `msn` | BC7_UNORM (`msn`: BC1 option) | same | same |
| `pbr_normal` | BC7_UNORM (BC1 option) | — (→ `normal`) | — (→ `normal`) |
| `coat_normal` | BC7_UNORM | — | — |
| `envmask`, `height`, `msn_specular` | BC4_UNORM | same | same |
| `complex_material` | BC7_UNORM (BC3 option) | BC7_UNORM (BC3 option) | — (→ `envmask`, `LOSS-ALPHA-DATA`) |
| `rmaos`, `rfaos` | BC7_UNORM; BC1_UNORM if A removed | — | — |
| `flow` | **BC5_UNORM** [Requirement] | — (CS feature) | — (CS feature) |
| `generic_two_channel` | BC5_UNORM | same | same |
| `generic_data` | BC7_UNORM | same | same |
| `hdr_sky` | **BC6H_UF16** (optional sun/HDRI use [D#3]) | — | — |

- **[Requirement]** `_SRGB` appears only under `cs`, only for PBR colour [D#4, D#5]. BC6H appears only under `cs` [D#3]. **[Proposal]** BC6H_UF16 has no `_SRGB` form: an 8/16-bit sRGB `_g` is decoded to linear float before encoding, the tag shows `n/a`, negatives clamp to 0.
- **[Requirement]** BC4/BC5 are unsigned. **[Proposal]** DX10 `BC4_UNORM`/`BC5_UNORM` by default, legacy `BC4U`/`BC5U` as an option, `ATI1`/`ATI2` never written [D#2].
- **[Finding]** LOD shaders reject sRGB [SC §4]. **[Proposal]** `_SRGB` under a `lod/` path raises `TAG-SRGB-LOD` in every target.
- **[Proposal]** BC7 perceptual is opt-in for Color with non-`data` alpha only. **[Finding]** It costs +37% normal angular error, −1.6 dB on packed data and −2.7 to −3.5 dB with data in alpha [B2 §1], [B1 §4].

### 4.3 Suggestion and disambiguation

**[Proposal]** For each file the suggester returns ranked `(subtype, confidence, reason)` entries from the file name, the path (a `textures/` component gives the game-relative path), the texture set (§6) and Analyse results.

1. **PBR sets.** Under `cs`, a set is PBR if its path has a `pbr` component or it contains `_rmaos`, `_rfaos`, `_cnr` or `_f`. **[Proposal]** It is also PBR if it contains a raw packing input that starts the `_rmaos`/`_rfaos` recipe (roughness, glossiness, metallic or AO; §4.4, §6.4), because the packed map will join the set; this applies from Phase 1, before packing exists. Otherwise a no-suffix colour file is `diffuse` (medium confidence). **[Requirement]** `_d` and the colour vocabulary (§4.4) are high confidence for the colour role; the path and set still pick `diffuse`, `pbr_albedo`, `landscape_diffuse` or `ui_sky`, and alpha meaning is never assumed [D-H]. **[Proposal]** An `hdr_sky` reading (float source under `sky/`) stays medium.
2. **`_s` has four readings** [SC §1], [TPM README]: MSN specular (set has `_msn` → `msn_specular`); TruePBR subsurface and coat colour (both slot TX07 → `pbr_subsurface`); PGPatcher's "Subsurface Tint" (otherwise → `subsurface_tint`, low confidence).
3. **`_m`** → `complex_material` if alpha is not constant 255, else `envmask`; always `envmask` under `vanilla`.
4. **`_rfaos`** → `rfaos`, high confidence [D-B]; green is a fuzz mask, not metalness.
5. Analysis can raise or lower confidence but never overrides an explicit choice; contradictions (an `_n` with constant alpha in a non-PBR set, a `pbr` path with a vanilla `_s`) are reported.
6. **Suffix aliases** (§3.7) are read as their canonical suffix; an alias match raises the top suggestion to high confidence, except for `_s` [D-G].
7. **Exporter vocabulary** (§4.4). **[Proposal]** A vocabulary term is read as its Skyrim role: colour terms as the colour member at high confidence (item 1), normal terms as `_n`, emissive terms as `_g`; raw maps get no sub-type suggestion (they are packing inputs). Each suggestion reason begins `exporter suffix _<term> (<role>)`.

### 4.4 Industry suffix vocabulary

**[Requirement]** Common exporter suffixes (Substance Painter/Designer, Quixel, ambientCG, Unreal/Unity) are recognised and read as Skyrim roles. `_d` and `_colormap` (and `_basecolor`, `_base_color`, `_albedo`, `_diffuse`, `_color`, `_col`) can safely be assumed albedo/diffuse: high confidence for the colour role, while the target and the PBR set still pick `diffuse` or `pbr_albedo` and alpha meaning is never assumed. `_normal`, `_nrm`, `_normalgl` and `_normaldx` map to `_n`. Raw single-channel maps are channel-packing inputs (§6.4). Outputs always use Skyrim names. The vocabulary lands in Phase 1; packing is its own milestone after the CLI MVP [D-H].

| Family | Exporter terms | Skyrim role (canonical suffix) | Output | Notes |
|---|---|---|---|---|
| Colour | `_colormap`, `_basecolor`, `_base_color`, `_albedo`, `_diffuse`, `_color`, `_col` | Colour member (no suffix) | `<base>.dds` | High confidence for the colour role [Requirement]; sub-type by path and set (§4.3 item 1) |
| Colour (Skyrim) | `_d` | `_d` (a §6.1 suffix) | `<base>_d.dds` (stem kept, as before) | Raised from medium to high [Requirement] |
| Normal | `_normal`, `_nrm` | `_n` | `<base>_n.dds` | Green convention unset: `unresolved` until stated (below) [Requirement] |
| Normal, OpenGL | `_normalgl` | `_n` with `channels.green_convention = opengl` | `<base>_n.dds` | Green flipped to DirectX in Preprocess (§5.4) [Requirement] |
| Normal, DirectX | `_normaldx` | `_n` with `channels.green_convention = directx` | `<base>_n.dds` | No flip [Requirement] |
| Emissive | `_emissive`, `_emission` | `_g` (`glow` or `pbr_emissive`) | `<base>_g.dds` | As `_g` [Proposal] |
| Raw roughness | `_roughness`, `_rough` | Packing input `roughness` | None on its own | §6.4 |
| Raw metallic | `_metallic`, `_metalness`, `_metal` | Packing input `metallic` | None on its own | §6.4 |
| Raw AO | `_ao`, `_ambientocclusion`, `_occlusion` | Packing input `ao` | None on its own | §6.4 |
| Raw specular | `_specular`, `_spec` | Packing input `specular` | None on its own | §6.4 |
| Raw glossiness | `_glossiness`, `_gloss` | Packing input `glossiness` | None on its own | §6.4 |
| Raw height | `_height`, `_displacement`, `_disp` | Packing input `height` | None on its own | §6.4 |
| Raw opacity | `_opacity` | Packing input `opacity` | None on its own | No recipe yet (§6.4, **[Open]**) |
| Raw fuzz | `_fuzz`, `_sheen` | Packing input `fuzz` | None on its own | §6.4 |

The terms Note H names are requirements; the synonyms (`_rough`, `_metalness`, `_metal`, `_ambientocclusion`, `_occlusion`, `_spec`, `_gloss`, `_displacement`, `_disp`, `_sheen`), the emissive family and `_opacity` are **[Proposal]**.

**[Proposal]** The details:

| Topic | Rule |
|---|---|
| Matching | Order: preset aliases (§3.7), then the vocabulary, then the §6.1 list. A term matches as the whole trailing `_<term>` of the stem, case-insensitively, longest first (`_base_color` before `_color`), with a base name before it. The §6.1 list is unchanged and no term ends in `_<§6.1 suffix>`, so a stem has at most one reading. An alias may not equal a term (§3.7). |
| Canonical suffix | Everywhere after Discover a vocabulary file counts as its canonical suffix, as an aliased file does (§3.7): colour terms as no suffix (the colour member), normal terms as `n`, emissive terms as `g`. A raw map counts as its role id (`roughness`, `metallic`, `ao`, `specular`, `glossiness`, `height`, `opacity`, `fuzz`), which enters the set's suffix list, so rule `suffix` and `set_has` matches and PBR detection see it. Rule globs still match the source name. |
| Output naming | Colour terms are written `<base>.dds`, because Skyrim's colour member carries no suffix (§6.1) and TruePBR's albedo slot is suffix-less too. `_d` is itself a §6.1 suffix and keeps its stem, so `iron_d.png` stays `iron_d.dds`. Normal terms → `<base>_n.dds`, emissive terms → `<base>_g.dds`, in the mirrored folder. Raw maps have no output of their own (§6.4). |
| Duplicates | Output names are canonicalised before grouping, so `iron.png` + `iron_basecolor.png`, `iron_n.png` + `iron_normal.png` or `iron_g.png` + `iron_emissive.png` collide on one output and are both `IN-DUPLICATE` (§6.1). |
| Green convention | The suffix-stated value of `_normalgl`/`_normaldx` is a fact layer at the "analysis facts" position of §8 (above presets, configs and rules, below `--set`), with provenance `suffix:<term>`. A plain `_normal`/`_nrm` file is `unresolved` while `channels.green_convention` is unset after layering; neither `--accept-suggestions high` nor `--allow SUG-LOW-CONFIDENCE` settles it, because it is not a suggestion. Analyse's green-convention guess (§5.1) is reported only, never applied. Skyrim's own `_n` is DirectX by definition (§4.1) and needs no statement. `msn` ignores the field. |
| Raw maps not packed | A raw map consumed by a recipe has no job of its own (Note M8). One whose recipe is off (`packing.enabled = false`) stays `packing_input` (with its role) and raises `IN-PACKING-INPUT` (warning); one that no recipe uses is `packing_input` with `PACK-UNUSED-INPUT`. Either writes nothing; `inspect` lists it and `convert` reports it as `file_skipped` with that code, but neither makes the batch partial (exit 1, Note M4, §8). A rule or `--set` that names a sub-type for a raw map makes it an ordinary file written under its source stem (e.g. `--set 'file:**/iron_height.png.subtype=height'` → `iron_height.dds`); it is then not a packing source. |

**[Proposal]** Conflicts with Skyrim suffixes are resolved by keeping every §6.1 meaning:

| Skyrim suffix | Keeps | Exporter term | Reads as |
|---|---|---|---|
| `_s` | Its four readings (§4.3 item 2) | `_spec`, `_specular` | Raw specular input (`_rmaos` A; env-mask source for a `cpm`/`vanilla` `_m`) |
| `_p` | `height` (parallax) | `_height`, `_displacement`, `_disp` | Raw height input (packed into `_p` under `cs`, `_m` A under `cpm`) |
| `_m`, `_em` | `envmask` / `complex_material` | `_metal`, `_metallic`, `_metalness` | Raw metallic input |
| `_g` | `glow` / `pbr_emissive` | `_gloss`, `_glossiness` | Raw glossiness input |
| `_f` | `pbr_fuzz` (RGB fuzz colour, A weight) | `_fuzz`, `_sheen` | Raw single-channel fuzz mask (`_rfaos` G) |
| `_e` | No suggestion | `_emissive`, `_emission` | `_g` |
| `_d` | Diffuse colour, stem kept | `_diffuse` | Colour member, written `<base>.dds` |

**[Open]** `_opacity` has no recipe in Note H (proposal for M2.5: the colour member's alpha, with alpha meaning still confirmed by the user). An RGB `_sheen` export may be a sheen colour that belongs in `_f` rather than the `_rfaos` mask; v1 reads it as a mask and `CHAN-NOT-GRAY` reports a coloured source.

---

## 5. Pipeline stages

**[Proposal]** `Discover → Group → Load → Analyse → Resolve → Preprocess → Mipmaps → Compress → Write → Validate`, with `Report` observing all; dry-run stops after Resolve. Each stage is an `engine` module:

| Module | What it does | How it is used | Depends on | Scope |
|---|---|---|---|---|
| `discover` | Enumerates inputs; derives relative and game-relative paths; finds existing outputs and manifest records | First step of `convert`, `inspect`, `validate` | File system, manifest | Batch |
| `group` | Builds texture sets (§6.1) | Feeds Resolve and set ordering | Suffix list | Batch → set |
| `load` | Decodes to float32 RGBA, keeping bit depth, channels, transfer | Per file | `image`, `exr`, DDS reader, `bcdec_rs` | Texture |
| `analyse` | Statistics (§5.1) | Suggestions, warnings, bake | `load` | Texture |
| `policy` (Resolve) | Merges preset, rules, suggestions, analysis, overrides into an immutable `ResolvedJob` with per-field provenance | CLI and GUI; dry-run output | `analyse`, presets, set context | **Set** |
| `pack` (M2.5) | Builds the per-target packing recipes of a set (§6.4) into virtual jobs with several sources | After Resolve (pass 1, Note M6), then around the packed output's own Resolve (pass 2: assemble, analyse, resolve) | `group`, `load`, `analyse` | **Set** |
| `ops` (Preprocess) | Channel and colour operations (§5.4) | Per file | `ResolvedJob` | Texture |
| `mips` | Float mip chain (§5.5) | Per file | `ops`; for a roughness member, the set's `_n` | Texture (set dependency) |
| `encode` | Block compression (§5.6) | Per mip and band | `encoders-sys`, NVTT process | Texture |
| `dds` | Container writing (§5.7) | Per file | `encode` | Texture |
| `validate` | Parse the bytes (in memory during conversion, from disk for `sdc validate`), decode, check (§5.8) | Per file; `validate` command | DDS reader, decoders | Texture |
| `report` | Event stream (§5.9) | CLI serialises; GUI consumes | — | Batch |
| `scheduler` | Memory-budgeted admission, set ordering, rayon pool, cancellation, panic isolation | Wraps all stages | `rayon` | Batch |

**Per texture vs per set.** **[Proposal]** Load, Analyse, Preprocess, Compress, Write and Validate run per texture. Group and Resolve run per texture set, because `_s` disambiguation, PBR detection and the bake decision need companions. Mipmaps is per texture with one dependency: a *roughness member* (`_rmaos` or `_rfaos`) waits until its set's `_n` is loaded and prepared (§6.2). Sets run in parallel.

### 5.1 Load and Analyse

**[Proposal]** Load produces float32 RGBA in the source's own transfer; 16-bit and float data are never quantised to 8 bits before encoding. **[Finding]** Early 8-bit quantisation is one of AlphaBloom's fidelity defects [MIP §AlphaBloom]. ICC profiles are ignored (the info event lands in M6); images above 16384 px are rejected (D3D11 limit [SC §5]). The loader reads PNG, TGA and TIFF (8/16-bit and 32-bit float) today; OpenEXR, Radiance HDR, BMP and JPEG arrive in M6. Analyse computes per-channel min/max/mean and constancy; an alpha histogram (255, 254, 0, ≤ 5, ≥ 250, CS parallax band); a grayscale test (within 4/255); for normals, length statistics, Z presence and a green-convention guess; for `_m`, the PGPatcher non-white share and predicted 1×1 alpha; HDR range and peak; size and power-of-two status; the DXGI format of DDS inputs. **[Finding]** The engine's `Analysis` does not yet carry the CS parallax alpha band, a Z-presence flag, the green-convention guess or a DDS input's DXGI format (the last arrives with the DDS re-encode opt-in, M6); it keeps the other statistics, plus `blue_mostly_black` and `red_mostly_high` flags, and no rule depends on the missing ones.

**[Requirement]** Near-opaque alpha (DECISIONS Note L): source alpha whose minimum is ≥ 251/255 (no texel below 251) is *opaque within BC tolerance* and classed exactly like constant 255, so `remove` resolves as a high-confidence analysis fact, no `LOSS-ALPHA-DATA` is raised and the alpha meaning is not unconfirmed. When some texel is 251–254, `ALPHA-NEAR-OPAQUE` (info) reports the minimum. One texel below 251 leaves the alpha varying, and an explicit `alpha.policy` (e.g. `--set alpha.policy=data`) still wins. The test is on the stored float: an 8-bit source compares its codes, a 16-bit or float source compares against 251/255 itself.

**[Proposal]** Interim float guard (final review I4, until M6). The transfer of a float source is not yet separated from the output encoding, so a 32-bit float TIFF written to an LDR format would be treated like integer data (sRGB-decoded as gamma-encoded, values above 1 clamped) and could come out visibly darker without any error. Until M6 adds the source-transfer versus output-encode split, a float source resolved to any format other than BC6H is blocked with `IN-FLOAT-LDR` (acknowledge with `--allow IN-FLOAT-LDR`, after which the values convert as stored), and values outside [0, 1] raise `LOSS-CLAMP-HDR`. A float source written as BC6H is linear content (`analysis:float-source`, §5.2).

### 5.2 Resolve

**[Proposal]** One `ResolvedJob` per output: paths, target, category, sub-type, both colour fields, alpha policy and flags, channel operations, mip spec, format, header, encoder, validation rules, each with provenance (`builtin`, `preset:<id>`, `user_config`, `project_config`, `rule:<source>#<n>` — `<source>` is a preset id, `user` or `project` and `<n>` the 0-based rule index —, `analysis` / `analysis:<fact>` — e.g. `analysis:float-source`: a float source written as BC6H is linear content —, `suffix:<term>` for a fact stated by an exporter term such as `_normalgl` (§4.4), `suggestion`, `user` for `--set` and per-file edits, and `fallback` for a sub-type replaced by its §4.2 fallback after `--allow CFG-UNAVAILABLE`; there is no `set` source yet; a set-level source for decisions such as the M4 bake is added with that milestone). Low-confidence sub-types and unconfirmed alpha meanings are `unresolved`: the CLI skips them unless a rule or flag decides; the GUI blocks Run. **[Requirement]** A plain `_normal`/`_nrm` source whose green convention is unstated is `unresolved` as well [D-H]. **[Proposal]** A raw packing input resolves to `packing_input` (§4.4) and is not converted on its own; from M2.5 it is consumed by a packing recipe (§6.4).

### 5.3 Why channel surgery precedes mips

**[Finding]** Stray alpha costs about 1 dB (real) to 17 dB (smooth), and removing it before encoding is the only complete fix [B1 §4]; dilation must precede filtering [MIP].

### 5.4 Preprocess

1. **Alpha `remove` drops the channel** [D#8].
2. Swizzles and channel extraction (Grayscale takes R; luma optional).
3. Normals (never `msn`): `*2 − 1` decode; OpenGL→DirectX green flip when `channels.flip_green` is set (**[Proposal]** Resolve sets it, with the provenance of `channels.green_convention`, when that field is `opengl`, §3.2, §4.4); Z rebuild `z = √(1 − x² − y²)` for two-channel sources; clamp z ≥ 0. Mip 0 otherwise stays as authored.
4. Push-pull RGB dilation under α = 0 for `cutout`, `blend`, `cutout_blend`.
5. `srgb` content decoded to linear with the exact sRGB curve, not a 2.2 power.
6. Resize only on request (NPOT policy or size cap), polyphase, in the content's filtering domain.

### 5.5 Mipmaps

**[Proposal]** Float32 throughout; each level is built from the base footprint (identical to recursive box at power-of-two sizes; avoids re-ringing for sinc [MIP §"Recursive chains"]); full chain to 1×1 unless `none` (`interface/`, `sky/` [SC §5]).

| Content | Filter | Domain | Extra |
|---|---|---|---|
| Colour (`srgb`) | Box; tent; Mitchell (1/3, 1/3) or Kaiser (w3, α4) selectable | Linear light, re-encoded after | Clamp [0, 1] |
| Colour + `cutout`/`blend`/`cutout_blend` | Alpha-weighted box on RGB; box on alpha | RGB linear light | Stored un-premultiplied |
| Data, Grayscale, Mixed-Packed | Box, channel-independent | Stored values | Alpha separate; never sharpened |
| Roughness member R | Box on α = r² | α domain, √ back | Variance bake (§6.2) |
| `_flow` | Box after `*2 − 1` | Signed vector | Not normalised [MIP] |
| Normals | Box only | Vector | Mips ≥ 1 renormalised; `msn` by length only |
| HDR | Box (tent at most) | Linear float | Negatives clamped [MIP] |

**[Finding]** Averaging gamma-encoded values darkens thin bright detail; box is area-exact and never overshoots; sinc kernels help only LDR colour [MIP §1, §"Averaging sRGB"].

**Coverage preservation (Cutout and Cutout+Blend; on by default, switchable per image; threshold *t* set per texture).** **[Requirement]** Rebuild AlphaBloom's Auto Coverage in the new engine, with credit [D#13]. **[Finding]** Castaño's method rescales each mip's alpha so its alpha-test coverage matches mip 0; AlphaBloom's exact-quantile solve has no ×4 cap, so sparse low mips can recover where DirectXTex and NVTT cannot [MIP §"Alpha-tested", §AlphaBloom]. **[Proposal]** For each mip ≥ 1, after filtering and before encoding: count base texels passing `alpha <compare> t`; find the quantile *a* in the mip's float alpha giving the same pass fraction; scale alpha by *t/a*, clamped to [0, 1]. The rebuild avoids AlphaBloom's suspected defects [MIP §AlphaBloom, 2–4, 10]: a mip whose target count rounds to zero is left unchanged (not scaled by t/255); the solve uses float alpha with no truncating 8-bit LUT, and the boundary texel lands at or above *t*; no "force opaque" runs before coverage; counter and solver share the `compare` field. **[Decided]** (owner, 2026-10-09) Implementation note: the uniform *t/a* scale is replaced by a monotone piecewise remap that keeps 0 and 1 (`[0, a] → [0, T]`, `[a, 1] → [T, 1]`, *T* the smallest float whose 8-bit code passes), anchored on the distinct alpha whose pass count is nearest the target (larger on a tie), so opaque interiors stay at 1.0 (when the anchor is itself 1.0, the next lower distinct alpha is anchored just below *T* so the lower, already-passing groups fail); controller ruling, owner check (DECISIONS, owner checks pending). **[Proposal]** Two documented limits (`coverage.rs`). A level whose target count rounds to zero stays unchanged and is a box-filtered descendant of level 0: it passes only where its footprint holds a passing base texel, so ordinary content stays within about one texel of the expected count, but this is not a hard bound, because failing alpha just below the threshold around sparse passing texels can lift several footprints at once. A monotone remap cannot split equal alphas, so on binary alpha the achievable count is the nearest group (the larger on a tie) and small mips can over-cover by up to the tie group; the §11.5 in-game check watches for thick distant edges. **[Finding]** Scaling saturates gradients, so blended alpha must keep box-averaged opacity [MIP]. **[Requirement]** `cutout_blend` still gets the coverage step by default at its threshold *t*, because the alpha-test pass is what thins out at distance (owner decision, DECISIONS Note E); it is switchable per image. **[Open]** If in-game checks (§11.5) show the blended edge fading wrongly at distance, a "partial coverage" option is added.

### 5.6 Compress

**[Proposal]** Each mip is split into 4-row block bands on the shared rayon pool; mips below 4×4 are padded by edge replication. bc7e takes block-linear 16-texel groups; ISPC TC takes strided surfaces [EI §1, §2].

| Format | Default | Evidence |
|---|---|---|
| BC7 | bc7e `veryslow` (= u5), linear | **[Finding]** u5 = u6 within ±0.03 dB at 0.22 vs 0.28 s/Mpix; ties NVTT Production and Compressonator on real colour [B1 §1, §7] |
| BC1 | rgbcx L18; 3-colour mode allowed; **transparent-black OFF**; batch decoder model `cBC1Ideal` **[Requirement]** | **[Finding]** Best BC1; transparent-black defaults on and decodes index 3 as transparent black [B1 §6], [EI §1] |
| BC3 | rgbcx L18 | **[Finding]** Best BC3 on all alpha sets [B1 §6] |
| BC4 | rgbcx HQ (bench `-4`); `max` = radius 10 | **[Finding]** +0.7 to +2.3 dB over others; `-hr10` +0.09 dB [B2 §3] |
| BC5 | rgbcx HQ, R and G | **[Finding]** Best BC5 [B2 §4] |
| BC6H UF16 | ISPC TC `bc6h_veryslow` | **[Finding]** Best mPSNR (51.50 dB), fastest (0.19 s/Mpix), but cut the venice sun's peak to 22% [B2 §6] |
| BC7/BC6H via NVTT (optional) | `nvcompress` Production, per mip (§7.5) | **[Finding]** Leads on gradients (+3.9 dB), linear data (+0.3–0.4 dB), keeps BC6H peaks (≥ 95%), at 40–60× the time [B1 §1], [B2 §5, §6] |

**Exact opaque alpha.** **[Proposal]** With `alpha.exact_opaque` set and format BC7, the engine sets a field added to `bc7e_compress_block_params` by the vendored patch (§7.1): for every block whose source alpha is 255 throughout, mode-6 endpoints are evaluated and stored with p-bit = 1 and alpha endpoint 127, so bc7e re-optimises RGB under the constraint and alpha decodes to exactly 255. With the field off, the bc7e call is bit-identical to upstream (gate §11.2); the engine's lossless rotated mode 4/5 fix below applies in any case and may change block bytes, never decoded RGB. Fallbacks, chosen at build time in order if the patch fails its gate (§11.2): first, a post-encode byte patch rewriting such mode-6 blocks to p-bit = 1 and alpha endpoint 127, accepting an unoptimised one-LSB RGB endpoint shift; second, routing the file to NVTT Production if installed. BC7 mode 4/5 blocks with a channel rotation (which can decode an opaque block's alpha below 255, down to 239 on noise) get a lossless post-encode fix setting the rotated slot's endpoints to all-ones; RGB is unchanged. It applies to every block whose source alpha is 255 throughout, with `exact_opaque` on or off and under any alpha policy; the flag governs only the mode-6 constraint, so with it off only mode 6 can decode 254. BC1 (transparent-black off) and BC3 need no constraint. Validate's smallest-mip check is the proof in every case.

### 5.7 Write DDS

**[Finding]** Compressonator's BC7 headers omit required flags, a community tool wrote unloadable `BC7_UNORM_SRGB` files with wrong mip counts, and the bc7enc_rdo tool never writes `_SRGB` [B1 §8], [SC §5], [EI §1]. **[Proposal]** The engine's own writer:

- Flags `CAPS | HEIGHT | WIDTH | PIXELFORMAT | LINEARSIZE`, plus `MIPMAPCOUNT` with mips; caps `TEXTURE`, plus `COMPLEX | MIPMAP` with mips; linear size of mip 0; mip count consistent with dimensions.
- Never `DDPF_NORMAL` (**[Finding]** NVTT's `-normal` only sets that non-standard flag [B2 §1]).
- Legacy `DXT1`/`DXT5` for BC1/BC3 UNORM by default; DX10 for all `_SRGB`, BC6H, BC7, and by default BC4/BC5. DX10 fields: `TEXTURE2D`, array size 1, `miscFlags2` 0; reserved fields zero.
- Atomic write: temp file in the target directory, then rename.

### 5.8 Validate

**[Proposal]** Conversion validates the exact bytes it is about to write, before the atomic write, so what is checked is what lands on disk (a failure after the write removes the output); `sdc validate` reads a file from disk. Both parse with the engine's own DDS reader; every mip is decoded with `bcdec_rs` (BC1 also with rgbcx at the batch model); check:

| Check | Rule | Failure |
|---|---|---|
| Header | §5.7 rules | Error |
| Per-mip error | Channel PSNR against the encoder input (the quantised pre-encode chain), floor `validation.min_mip_psnr_db` = 30 dB (`VAL-MIP-ERROR`); the angular error for normals arrives with the per-mip metrics of M7 (§11.3) | Warning below threshold |
| **Smallest-mip alpha** | **Decode the smallest mip (and every level) and check alpha**: `remove` → ≥ the encoder guarantee below; with `exact_opaque`, exactly 255 on every all-255 source block; `cutout` → coverage within 2 points of mip 0, checked on levels of 256 texels or more (a single flipped texel moves smaller levels by more than the tolerance); `complex_material` → CS's smallest-mip rule (S8, read from the shader): complex when the RGB is not grey (some pair of channels ≥ 4/255 apart) or is black (all channels < 4/255 with 4/255 < alpha < 251/255), and alpha < 251/255; warn from 247/255 (a 4-code margin) and say "off" from 251/255 (**[Decided]**, S8) | Per rule |
| PGPatcher count | `_m` outputs storing alpha: base-mip share of α ≠ 255 vs the ≥ 50% rule | Warning |
| BC6H peak | Decoded ÷ source peak ≥ 0.8 (**[Open]**) | Warning |

**[Proposal]** A file whose validation reports an error is still written and counted (exit 4, §8); a later `if-changed` run re-encodes it instead of calling it up to date.

**Decoded-alpha guarantees** [D-A]. **[Finding]** Removing alpha fixes stray alpha but not decoded alpha: on RGB-only input bc7e decoded 254 on 28% of pixels (ISPC 15%, texconv 2.4%, Compressonator 1.1%, NVTT 0%) [B1 §5]; `Bricks104` shows 21.0% [D-C].

| Encoder / format | Decoded alpha after `remove` and on all-255 blocks |
|---|---|
| bc7e, `exact_opaque` off (default outside `_m`) | ≥ 254 (`ALPHA-254` info with the share at mip 0 and the smallest mip). Only mode 6 yields 254: the rotated mode 4/5 fix (§5.6) is unconditional for all-255 blocks |
| bc7e, `exact_opaque` on (default for `_m`), or byte-patch fallback | 255 |
| NVTT Production; rgbcx BC1 with transparent-black off | 255 |
| BC4, BC5, BC6H | No alpha (reads 1.0) |

**[Finding]** 254 never trips CS's complex gate (smallest-mip alpha < 251/255); the exposures are PGPatcher's ≥ 50% rule and ENB's "pure white = flat" rule [D-A], [SC §4]. **[Proposal]** Both concern only `_m`, hence the flag's default scope; a plain `_m` is BC4 without alpha [D#9], so it matters there only if the user overrides to BC7.

### 5.9 Report and scheduler

**[Proposal]** Every stage emits `EngineEvent`s carrying `schema: 1`, `seq`, timestamp, batch id, and set key and path where relevant. Kinds: `batch_started`, `file_discovered`, `set_resolved` (carries `recipes`: output, sub-type, channel provenance, status `run`/`conflict`/`failed`/`disabled`, reason), `file_planned` (dry-run `ResolvedJob`), `file_started`, `progress`, `warning` (code, severity, message, data), `file_done`, `file_skipped`, `file_failed`, `batch_done`. The CLI writes them as JSON Lines; the GUI receives the same values over a channel, so CLI stdout is valid GUI input (§9.4). The event schema is published; breaking changes bump `schema`. **[Finding]** An 8K RGBA32F buffer is 1 GiB plus about 33% for mips [LF §5]. **[Proposal]** Files are admitted by estimated bytes against `--mem-budget` (default 50% of RAM); large files use band parallelism; cancellation is checked between bands, since native calls cannot be interrupted [LF §5]; each file runs under `catch_unwind`, in planning as well as in conversion (a panic while loading, analysing or resolving fails that file at that stage). `progress` events are emitted only while compressing today; per-stage events (M7) will feed the GUI's stage column. A `Reporter` holds an ordering lock while delivering each event so `seq` order equals delivery order; a slow sink therefore back-pressures the workers, and a GUI sink should only forward into a channel.

---

## 6. Texture-set jobs

### 6.1 Grouping

**[Proposal]** A texture set is the inputs sharing a directory and a basename: the stem minus one recognised suffix, longest match first (`_n`, `_msn`, `_rmaos`, `_rfaos`, `_p`, `_m`, `_em`, `_envmask`, `_g`, `_s`, `_f`, `_cnr`, `_crn`, `_b`, `_bl`, `_sk`, `_d`, `_e`, `_flow`). A stem with no suffix is the colour member. Preset suffix aliases (§3.7) are matched first and stand for their canonical suffix; exporter vocabulary terms (§4.4) are matched next and stand for their canonical suffix or packing role. Matching is case-insensitive. Two inputs mapping to one output (`wood01_n.png`, `wood01_n.tga`) are `IN-DUPLICATE` and both are skipped until one is excluded. Sets drive suggestion, `_s` and PBR disambiguation, and the bake.

### 6.2 Variance bake into roughness

**[Requirement]** Renormalise normal mips unless the set has an `_rmaos` — or an `_rfaos` [D-B] — in which case bake normal variance into its roughness [D#6]. *Roughness member* below means either. **[Proposal]** The CLI MVP renormalises `_n` mips and does not bake (owner: the bake arrives in M4); `mips.normal = renormalize_bake` is accepted and behaves as `renormalize` until then. **[Finding]** Unity HDRP, The Order: 1886 (vMF fit) and Call of Duty: WWII average full-resolution normals over each mip footprint — never previously renormalised mips — convert the shortened length to variance, widen GGX roughness, then renormalise the stored normal [MIP §"Normal maps"]. **[Proposal]** The bake applies to a PBR set under `cs` that contains a `pbr_normal` and a roughness member, both as source images in the batch; if both `_rmaos` and `_rfaos` exist, each is baked. For each level ℓ of the roughness member:

1. Let *F* be the UV footprint of one level-ℓ texel. Average the `_n` **base-level** unit normals (after flip and Z rebuild) over *F*, giving length *L*. If *F* covers at most one `_n` texel, *v* = 0.
2. κ = (3L − L³)/(1 − L²); *v* = 0.25/κ; *v* = 0 when *L* ≥ 1 − 10⁻⁶ [MIP, Unity `CommonMaterial.hlsl`].
3. α = r², box-averaged over *F* from the roughness member's base.
4. α′² = saturate(α² + min(2v, t²)); stored r′ = (α′²)^¼. Clamp *t* = **0.25** **[Requirement]** (owner, 2026-10-08), from Unity HDRP's `NORMALMAP_HIGHEST_VARIANCE = 0.03125` = 0.25²/2, i.e. 2v ≤ 0.0625 = t² [MIP notes, normal_maps.md]. Tunable per preset.
5. The `_n` level is renormalised as usual.

If the `_n` base is larger than the roughness member's base, its mip 0 correctly receives variance. If their size ratio is not a power of two, the bake is disabled for the set (`SET-BAKE-RATIO`).

### 6.3 Ordering and edge cases

**[Proposal]** In a set, `_n` loads first and its base normals are kept as a summed-area table (O(1) footprint averages) until the roughness member's mips are built, then released. Other members are independent.

| Situation | Behaviour | Event |
|---|---|---|
| `_n` and a roughness member both in the batch as sources | Bake | `set_resolved` with `bake: true` |
| No roughness member | Renormalise only [D#6] | — |
| Roughness member on disk but not selected | Renormalise only; unselected file never written | `SET-COMPANION-OUTSIDE` |
| Roughness member only as DDS | Renormalise only; DDS not re-encoded unless the user opts in (then decoded, `LOSS-REENCODE`) | `SET-COMPANION-DDS` |
| `_n` absent | Roughness member processed without bake (R still on α = r²) | — |
| Non-PBR set | Renormalise only; no bake into `_n` alpha or `_m` green | — |

### 6.4 Channel packing

**[Requirement]** Raw single-channel maps (§4.4: roughness, metallic, AO, specular, glossiness, height, fuzz, …) in one texture set are **packed automatically per target**: `cs` `_rmaos`/`_rfaos` + `_p`; `cpm` complex `_m`; `vanilla` `_m` only when a source exists. Raw maps are not written on their own by default; this is overridable per set. Missing channels take neutral defaults plus a warning naming each defaulted channel (AO white, metallic black, specular alpha white = the TruePBR default). Missing roughness does **not** block when an AO or metallic map is found; roughness then takes a neutral default with a warning. **[Requirement]** Global fill values (owner, 2026-10-09 [D-H]): for every packed channel without a source map the user can set a fill value, as a slider in the GUI and as a CLI option (e.g. `--fill metallic=255` writes 255 instead of the neutral 0); the neutral defaults below apply only when no value is given. Packing is its own milestone right after the CLI MVP (plan M2.5) [D-H].

**Recipes.** Channel layouts are those of §4.1 (`rmaos`, `rfaos` [D-B], `height`, `complex_material`, `envmask`); sources and triggers are **[Proposal]** unless marked.

| Target | Output (sub-type) | R | G | B | A | Runs when |
|---|---|---|---|---|---|---|
| `cs` | `<base>_rmaos.dds` (`rmaos`) | roughness, else 1 − glossiness | metallic | AO | specular | A roughness or glossiness source exists, or (owner: roughness missing does not block) an AO or metallic source exists [Requirement]; and no fuzz source |
| `cs` | `<base>_rfaos.dds` (`rfaos`) | roughness, else 1 − glossiness | fuzz | AO | specular | As `_rmaos`, and a fuzz source exists |
| `cs` | `<base>_p.dds` (`height`, BC4) | height | — | — | — | A height source exists |
| `cpm` | `<base>_m.dds` (`complex_material`) | env mask | glossiness, else 1 − roughness | metallic | height | A roughness, glossiness, metallic or height source exists |
| `vanilla` | `<base>_m.dds` (`envmask`, BC4) | env mask | — | — | — | A metallic or specular source exists [Requirement: only when a source exists] |

**Defaults for a channel without a source.** Each defaulted channel raises `PACK-DEFAULTED` (warning) naming the output and the channel.

| Channel | Default | Tag |
|---|---|---|
| AO | 1.0 (white) | [Requirement] |
| Metallic | 0.0 (black) | [Requirement] |
| Specular (`_rmaos`/`_rfaos` A) | 1.0 (white), the TruePBR default; with TruePBR's default `specular_level` 0.04 this is F0 0.04 | [Requirement] |
| Roughness | 0.5, set by the preset field `packing.roughness_default` (glossiness default 1 − that value) | [Requirement] 0.5 confirmed by the owner 2026-10-09 [D-H] |
| Env mask (`cpm`/`vanilla` `_m` R) | Metallic, else specular; under `cpm` without either: `fill.metallic`, else `fill.specular`, else 0 | **[Decided]** (Note M1) |
| Height (`cpm` `_m` A) | 0.5. A white default would make CS read the `_m` as a plain env mask (1×1 alpha ≥ 251/255, §5.8) and ENB as flat; a constant height has no relief. 128/255 is below CS's 251/255 gate (S8); the packed RGB must also not be grey at the smallest mip (§5.8) | **[Decided]** (S8) |

**[Decided]** (plan M2.5, DECISIONS Note M) Fill values: a fill takes the place of the table default for a channel that has no source map and is never applied to a channel that has one. The field is `packing.fill.<role>` (a §4.4 raw-map role id such as `metallic`) holding an 8-bit code 0–255, and `--fill <role>=<0..255>` sets it for the whole batch like an unscoped `--set`; the GUI offers one slider per role in the global settings. A fill for a role that no recipe of the target packs has no effect. `packing.roughness_default` remains the preset-level neutral default for roughness, and a roughness fill wins over it. A filled channel is recorded as `fill:<code>` and raises no `PACK-DEFAULTED` (Note M5).

**[Decided]** (plan M2.5, DECISIONS Note M) Rules:

| Topic | Rule |
|---|---|
| Activation and overrides | Recipes run automatically per target when their trigger holds. Packing settings are profile fields of the packed output, resolved like any other field (§8 precedence): `packing.enabled` (default `true`), `packing.roughness_default` (0.5), `packing.prefer` (`prepacked` / `raw`, default `prepacked`). Presets, configs, rules (e.g. `match = { set_has = "roughness" }`), `--set 'subtype:rmaos.packing.enabled=false'` and file-scoped `--set` on the packed output's name override them per set. With `packing.enabled = false` the raw maps stay packing inputs (`IN-PACKING-INPUT`) and nothing is packed. |
| Raw maps on their own | Never written by default. A rule or `--set` naming a sub-type for a raw map converts it on its own under its source stem (§4.4) and removes it from every recipe. |
| Sources | Each raw map contributes its R channel (luma with `channels.grayscale_source = luma`); `CHAN-NOT-GRAY` names a source whose R/G/B differ; source alpha is ignored. A raw map that no recipe of the target uses (e.g. fuzz under `cpm`, metallic next to a fuzz source, opacity, height under `vanilla`, a set whose only rmaos-family input is specular) raises `PACK-UNUSED-INPUT` (warning) and is not written. |
| Gloss ↔ roughness | Converted in both directions per texel in stored values: roughness = 1 − glossiness, glossiness = 1 − roughness. When both sources exist, the target's own quantity is used (roughness for `cs`, glossiness for `cpm`) and the other is reported with `PACK-UNUSED-INPUT`. |
| Dimensions | All sources of one recipe must have the same width and height; otherwise the recipe fails with `PACK-SIZE-MISMATCH` (error) listing every source file and its size. No resizing in v1. |
| Precision | Sources load as float32 (§5.1), so 16-bit inputs keep their precision until the encoder; conversions run in float. Bit-depth loss is reported as for any file (`LOSS-BITDEPTH`). |
| Pipeline | A recipe is a virtual job with several sources. From Preprocess on it is an ordinary job of the packed sub-type: that sub-type's profile, alpha policy, mip rules (an `_rmaos`/`_rfaos` filters R on α = r², and the variance bake of M4 applies to a packed roughness member like any other), encoder, writer, validation and §10 checks (`CHAN-AO-INVERTED`, `CHAN-GLOSS-AS-ROUGH`, `VAL-M-*`, …). |
| Provenance | Each output channel records its source (`iron_roughness.png:R`, `1 − iron_gloss.png:R`) or `default:<value>` for a neutral default or `fill:<value>` for a user-set fill. The manifest record lists every source with its SHA-256 (§3.6), and `if-changed` re-encodes when any source changes. |
| Pre-packed conflict | If the set already holds a source for the packed output (`iron_rmaos.png` and raw `iron_roughness.png`; `iron_p.png` and `iron_height.png`; `iron_m.png` and raw maps under `cpm`/`vanilla`), the recipe gets the explicit status `conflict` and `PACK-CONFLICT` (warning) names both sides. With `packing.prefer = prepacked` (default: the pre-packed file wins, with the warning) the pre-packed file converts as usual and the recipe's raw inputs are reported unused; with `raw` the recipe runs and the pre-packed file is skipped with `PACK-CONFLICT`. |
| No source | A recipe whose trigger does not hold does not run; nothing is reported for it. |
| Reporting | `set_resolved` lists each recipe: output, sub-type, per-channel source or default, status (`run`, `conflict`, `failed`, `disabled`) and reason. `sdc inspect` prints the same; `file_planned` carries the virtual job with all its sources. |
| Set membership | The packed output joins its set under its suffix, so it counts for PBR detection (§4.3 item 1), `set_has` rules and the bake (§6.2). |
| Packing settings | Read from the packed output's profile resolved with a placeholder analysis (Note M6); a rule matching on `alpha` cannot change them. The same profile's `channels.grayscale_source` is the one used to assemble the image (stored in the job at plan time). |
| Duplicate roles | The first raw map of a role by path is used; others are `PACK-UNUSED-INPUT` (Note M13). |

---

## 7. Encoder integration

### 7.1 Vendored components

| Component | Role | Form | Licence | Status |
|---|---|---|---|---|
| bc7e.ispc (bc7enc_rdo `b943862`) + one project patch | BC7 | Prebuilt multi-target objects (`sse2,sse4,avx,avx2`) and `bc7e_ispc.h` from ISPC 1.31.0, plus a single-target set for `pinned-isa`. Patch `vendor/bc7e/patches/0001-exact-opaque-alpha.patch` adds the exact-opaque field (§5.6) | Apache-2.0; patched file carries a change notice | **[Finding]** BinomialLLC/bc7e archived 2026-01-28; the bc7enc_rdo copy has a determinism fix [EI §1] |
| rgbcx (same commit) | BC1/3/4/5 encode; BC1 decode | C++ via `build.rs` + C shim | MIT or Unlicense | Active [EI §1] |
| ISPC TC (`79ddbc9`) | BC6H | Prebuilt kernel objects + `ispc_texcomp.cpp` + header | MIT | **[Finding]** Archived 2024-09-23; small, stable [EI §2] |
| bcdec_rs | Decode for validation and preview | Crate | MIT | [EI §10] |

**[Proposal]** The vendored commits are those the bench measured, so the gates (§11.2) prove the integration. The patch is the only change to any vendored encoder, kept as a separate file so it can be reviewed, re-applied or reverted.

### 7.2 Licences and credits

**[Requirement]** GPL-3.0-or-later; Apache-2.0 and MIT components are acceptable [PLAN §1]. **[Finding]** Apache-2.0 is GPLv3-compatible, not GPLv2-only-compatible [EI §1]. **[Proposal]** The patched `bc7e.ispc` states its change and date (Apache-2.0 §4(b)); licences and NOTICEs ship in `LICENSES/` and `THIRD_PARTY_NOTICES.md`; `cargo deny` enforces an allow-list. `docs/CREDITS.md` credits AlphaBloom (tkRiverGamer) for Auto Coverage, Ignacio Castaño for coverage-preserving mips, and jiayev's TruePBR-Manager for TruePBR slot defaults. **[Finding]** TruePBR-Manager is CC BY-NC 4.0 [TPM LICENSE], which is not GPL-compatible. **[Proposal]** No TruePBR-Manager or AlphaBloom code is copied; the engine reimplements published algorithms and aligns with documented formats and defaults. (Design analysis, not legal advice.)

### 7.3 Threading

**[Finding]** `rgbcx::init()` sets global state and is not thread-safe; afterwards encode/decode are thread-safe. `bc7e_compress_block_init()` must run once. ISPC TC is stateless [EI §1, §2]. **[Proposal]** `encoders-sys` runs both initialisers once (`std::sync::Once`) with the batch's BC1 model. All parallelism is the engine's rayon pool calling per-block or per-band functions; the libraries' OpenMP paths are not compiled.

### 7.4 Effort levels

**[Proposal]** bc7e named levels `ultrafast` … `slowest` (= u0 … u6), default `veryslow` (u5), "Fast preview" `fast` (u2). **[Finding]** u4 is out of order (worse than u3 on gradients) [B1 §7], so it is not offered. rgbcx: level 18; BC4/BC5 radius `default`/`max`. ISPC BC6H: `veryslow`. NVTT: "Production" only. **[Finding]** NVTT `-production` and `-highest` are byte-identical for BC7 and BC6H [B1 §1], [B2 §1].

### 7.5 NVTT 3 as an optional external process

**[Requirement]** NVTT 3 is optional, user-installed, run as a separate process, never redistributed or linked [PLAN §1]. **[Finding]** Its licence forbids use that would subject it to an open-source licence; a separate user-installed process is the clean path [EI §5]. **[Proposal]** Detection checks a configured path, then `C:\Program Files\NVIDIA Corporation\NVIDIA Texture Tools\nvcompress.exe`; versions other than the tested 3.2.5 need confirmation. **[Finding]** Per-mip feeding works (spike S7 passed, DECISIONS): each pre-built level is sent as its own `nvcompress -nomips -bc7 -production` (or `-bc6 -production`) job on a temporary RGBA8 (RGBA16F) file and the engine assembles the chain; mip-0 PSNR equals the bench's direct encode to four decimals. Per-process overhead is 0.142 s with CUDA and 0.015 s with `-nocuda`; the cost is CUDA context creation, and BC7 Production runs on the CPU either way with byte-identical output. **[Proposal]** The backend always passes `-nocuda` for BC7 Production. The BC6H half (`-bc6` on RGBA16F levels) is untested and arrives with the NVTT backend in M5; if a level cannot be fed, NVTT encodes mip 0 only and bc7e the rest, with an event.

### 7.6 Fallback, oracle, exclusions

**[Finding]** S4 ran and the scalar port was not adopted: 3.8–4.8× slower, and 0.0759 dB off the ISPC result on 1 of 11 images against the 0.01 dB limit (DECISIONS, Spike S4). **[Proposal]** `basisu_bc7e_scalar.cpp` (Apache-2.0 C++17 port of bc7e) stays unbuilt; it becomes a fallback only if the ISPC objects fail on a needed Windows x64 configuration and the owner accepts a looser tolerance. **[Proposal]** DirectXTex (`texconv`, `texdiag`) is a CI oracle only (§11.4). **[Finding]** texconv is the weakest BC7 encoder at every setting [B1 §1]. **[Requirement]** Oodle is excluded. **[Proposal]** Compressonator is excluded: stagnant, unbuildable dependency, never uses BC7 modes 4/5, non-compliant BC7 headers [EI §4], [B1 §4, §8].

---

## 8. CLI design

**[Requirement]** Dry-run, JSON output, exit codes and profiles [PLAN §8]. **[Proposal]** Verb-based commands:

| Command | Purpose | Writes |
|---|---|---|
| `sdc convert <inputs…> [-o <root>]` | Convert images or folders | Yes, unless `--dry-run` |
| `sdc inspect <inputs…>` | Analyse; print proposed sub-type, profile, warnings; DDS headers and mip statistics | No |
| `sdc validate <dds…> [--target <t>] [--subtype <id>]` | §5.8 checks on existing DDS; `--target` defaults to `cs` | No |
| `sdc presets list / show / diff / export / validate / schema` | Inspect, compare, export, check presets; print schemas | `export` only |
| `sdc encoders` | Built-in encoder versions; NVTT detection | No |

Options: `--target`, `--preset <id|file>`, `--preset-file <file>` (repeatable; loads a preset, e.g. one declaring suffix aliases, without selecting it, §3.7), `--config <file>`, `--set <field>=<value>` (scoped: `--set 'subtype:rmaos.format=bc1'`, `--set 'file:<glob>.alpha.policy=blend'`), `--accept-suggestions {none,high}` (default `none`), `--allow <code>`, `--recursive`, `--jobs`, `--mem-budget`, `--on-existing {if-changed,skip,overwrite}`, `--dry-run`, `--json`, `--sidecar`, `--nvtt <path>`, `--bc1-decoder {ideal,ideal-round4,nvidia,amd}`, and `--fill <role>=<0..255>` (repeatable, §3.2), an unscoped `--set packing.fill.<role>` applied after every `--set` (Note M5).

**Batch behaviour.** **[Proposal]** Files are grouped, resolved, then scheduled in parallel under the memory budget. A file converts only when every field is decided by an explicit rule, flag or a high-confidence suggestion accepted with `--accept-suggestions high`; diffuse alpha meaning is never high confidence. Unresolved files are skipped and listed. `--dry-run` stops after Resolve and emits `file_planned` events.

**Rules the engine applies where this spec was silent.** **[Proposal]** (Merged from the plan's decision list at the final review.) (1) A fact the analysis measured, not a meaning it guessed, applies without `--accept-suggestions`: `remove` for a source with no alpha, constant 255 alpha or near-opaque alpha (§5.1); an explicit `alpha.policy` still wins. (2) A file that is both unresolved and blocked reports `unresolved` first, because the decision comes before the acknowledgement. (3) A file that is up to date is not "skipped" for the exit code. (4) The default output root is `./sdc-out`. (5) An invalid value in a preset, config or rule, and an unscoped `--set` whose value is unavailable under the target, are usage errors (exit 2) found while planning, before any file is read; a combination that fails only after layering (for example `CFG-INVALID` on one file) fails that file (exit 1). (6) The project config `sdc.toml` is read from the first input only, and only when it is a folder, or from `--config`; an `sdc.toml` beside a single-file input or in a later input is ignored silently until M6 documents or warns about it. (7) A source whose size or modification time changed between planning and conversion fails at Load ("source changed since planning; re-run"), so a plan-time decision such as `remove` is never applied to different pixels.

**Streams and exit codes.** **[Proposal]** Human output on stderr (result verbs `inspect`, `validate`, `presets`, `encoders` print their report on stdout [D Note J]); with `--json`, stdout carries only JSON Lines. The highest-precedence applicable code is returned: **3** fatal (output-root I/O, out of memory, internal error, a second Ctrl-C) > **2** usage or config error (bad flag, invalid preset or config, batch-wide unavailable combination) > **1** partial (any file failed, or skipped for a reason other than an intentional skip — IN-DDS-SKIPPED, `--on-existing skip`, IN-PACKING-INPUT do not count [D Note I]) > **4** completed but validation failed on an output > **0** all converted and validated.

**Config precedence.** **[Proposal]** Lowest to highest: built-in defaults → preset and its `base` chain (for a file matched by a suffix alias, the alias preset's chain, §3.7) → user config `%APPDATA%\sdc\config.toml` → project config (`sdc.toml` in the input root, or `--config`) → preset rule matches → suggestions accepted by flag → `--set` overrides (GUI per-file edits sit here). Presets and config are TOML validated against the schema; machine outputs are JSON.

| Edge case | Behaviour |
|---|---|
| Output exists | `if-changed` (default) re-encodes when the manifest shows a different source hash, profile hash, engine version or encoder versions, a BC1 decoder model other than the batch's (BC1 and BC3 outputs, whose colour blocks rgbcx encodes against it), or a record whose validation did not pass (a failed output is re-encoded and reported again, never counted up to date); a record it cannot compare (newer schema, missing encoders or preset) also re-encodes. An output with no manifest record is kept (`OUT-UNKNOWN-EXISTING`) unless `overwrite`; the engine never leaves its own output without a record (a failure after the write removes the output) |
| Invalid or unreadable input | `file_failed` at Load; batch continues |
| Same stem, different formats | `IN-DUPLICATE`; both skipped |
| Raw packing input (§4.4) | Consumed by a packing recipe (§6.4). One whose recipe is off (`IN-PACKING-INPUT`) or that no recipe uses (`PACK-UNUSED-INPUT`) is reported as `file_skipped` but not counted for exit 1 (Note M4) |
| DDS input in `convert` | Skipped (`IN-DDS-SKIPPED`) unless `--set file:<glob>.reencode=true` (`LOSS-REENCODE`). The opt-in is M6: `reencode` (like `resize`) is not a settable field yet, so today it is a usage error |
| Unsupported setting for a file (BC6H under `vanilla`, BC5 on `_n` without `--allow`) | File skipped with its blocking code; exit 1 |
| Unsupported setting for the whole batch | Exit 2 before work starts |
| Encoder failure or panic | That file fails, at whichever stage it was in (Load, Analyse and Resolve during planning included); others continue; exit 1 |
| Ctrl-C | First: current bands finish, no partial output is written, `batch_done` lists cancelled files; exit 1. Second: exits 3 at once without waiting for running bands; it may leave `.sdc-*.tmp` files in the output folders and an output without a manifest record (kept as `OUT-UNKNOWN-EXISTING` on the next run), and the JSON stream ends without `batch_done` [D Note K] |
| Read-only or locked output | `file_failed` at Write |
| Non-ASCII or mixed-case paths | Supported; set matching is case-insensitive |

---

## 9. GUI design

**[Proposal]** `sdc-gui` runs the engine in-process through the same API and event type as the CLI.

**Screens and workflow.**

1. **Source list.** Drag-and-drop or dialog; folder tree; file table grouped by set (path, proposed sub-type with confidence badge, target, format, colour fields, alpha policy, status, warnings). Items needing confirmation are highlighted and block Run. Bulk actions: apply sub-type or alpha policy, accept high-confidence suggestions, apply a preset.
2. **Settings panel**, collapsible groups with explicit labels. *Type*: target, category, sub-type, each with the reason for its default. *Colour*: "Content: sRGB-encoded colour / Linear data" and "DDS tag: `_SRGB` / UNORM (linear)", with reasons such as "vanilla path: UNORM required; filtering in linear light". *Alpha*: Remove / Cutout / Blend / Cutout + Blend / Data, threshold slider 5–200 (127), comparison, exact opaque alpha. *Mipmaps* (levels, filter, edge, normal handling), separate from *Compression* (format, encoder, effort). Locked fields show why.
3. **Preset inspector.** Each resolved field with provenance; diff against base; save, import and export TOML; built-ins read-only. A **Profiles** view hosts user presets and rule tables.
4. **Advanced.** Perceptual, BC1 transparent-black, BC4/BC5 radius and header form, NVTT path and status, BC1 decoder model (batch), sidecar, isolated worker.
5. **Preview.** GPU viewport with zoom/pan; R/G/B/A isolation and alpha-as-mask; mip slider; side-by-side and split source vs encoded; difference heatmap; readouts for decoded smallest-mip alpha, α ≠ 255 share, coverage per mip, normal length, BC6H peak ratio; `_SRGB` and UNORM views of the same bytes.
6. **Batch progress and report.** Per-file stage and progress (the stage column needs the per-stage events of M7); warnings grouped by code with links; export as JSON Lines or Markdown.

**Preview implementation.** **[Finding]** wgpu `TEXTURE_COMPRESSION_BC` lets the GPU decode uploaded blocks; an 8K BC7 mip 0 takes 64 MB versus 256 MB as CPU-decoded RGBA8 [LF §2]. **[Proposal]** Encoded previews upload blocks and draw via an `egui_wgpu` paint callback; sources upload as RGBA16F; `bcdec_rs` CPU decode serves heatmaps and metrics. **[Requirement]** Preview maximum 8192 by default, 16384 as a setting [D#19]. **[Proposal]** The GUI requests the adapter's real limits and, if 16384 is unavailable, previews from the first mip that fits and says so (**[Open]**).

**Interaction.** **[Proposal]** Suggestions are marked "suggested"; low-confidence items and blocking warnings need per-file or per-selection confirmation (the GUI form of `--allow`). The engine's `--allow` is batch-wide today and `file:` scopes are globs, so per-file acknowledgement and an exact-path `--set` scope are M3 entry work (plan, M3 carried items).

**Engine coupling.** **[Proposal]** `Engine::plan` and `Engine::run` each take a `&CancelToken` (a fresh one per batch). `Engine::run` runs on a worker thread and streams `EngineEvent`s over a channel. Because events serialise exactly as the CLI's JSON Lines, an isolated-worker mode can spawn `sdc.exe convert --json` and parse stdout into the same events, isolating native crashes. **[Open]** It ships off; the trigger for default-on is the crash rate measured in Phases 3–4 [D#18].

---

## 10. Error handling and warnings catalogue

**[Proposal]** Severities: **info** (recorded), **warning** (shown; proceeds), **blocking** (file not converted until acknowledged in the GUI or with `--allow <code>`), **error** (file fails). Events carry code, file and measured data.

| Code | Severity | Trigger → message | Resolution |
|---|---|---|---|
| `LOSS-ALPHA-DATA` | blocking | Non-constant alpha (minimum < 251) discarded by `remove` or a no-alpha format | Keep alpha or acknowledge |
| `LOSS-CLAMP-HDR` | warning | Float values outside [0, 1] written to LDR → clamped. Raised by the interim float guard (§5.1) until the M6 transfer split | HDR category under `cs`, or accept |
| `LOSS-BITDEPTH` | info | 16-bit/float source to 8-bit-endpoint format | — |
| `LOSS-REENCODE` (M6) | warning | Compressed DDS re-encoded; cannot improve it [SC §7]. Not emitted yet: DDS re-encode lands in M6 | Use the source |
| `CHAN-NOT-GRAY` | warning | Grayscale sub-type, R/G/B differ > 4/255 → only R (or luma) kept | Pick channel or sub-type |
| `CHAN-CONSTANT-ALPHA-DATA` | info | `data` alpha is constant; `remove` would save bits | Accept or switch |
| `CHAN-NORMAL-LENGTH` | warning | Normal source far from unit length, or no Z | Check category; Z rebuild |
| `CHAN-AO-INVERTED` | warning | Roughness member B mostly black → AO may be inverted [SC §3] | Check source |
| `CHAN-GLOSS-AS-ROUGH` | warning | Roughness member R mostly high → may be gloss | Invert or confirm |
| `FMT-BC5-NORMAL` | **blocking** | BC5/BC4 on a Normal sub-type → Z decodes −1, alpha lost [SC §2] | BC7 |
| `FMT-LANDSCAPE-BC1` | blocking | BC1 on `landscape_diffuse` → destroys snow mask/height [SC §4] | BC7 or BC3 |
| `FMT-BC1-ALPHA` | blocking | BC1 with policy ≠ `remove` (v1 BC1 has no alpha) | BC7/BC3 or `remove` |
| `FMT-PERCEPTUAL-DATA` | error | Perceptual where locked [B2 §1] | — |
| `ALPHA-EXACT-OFF-M` | blocking | `exact_opaque` off on an `_m` written as BC7 (BC3's alpha block stores 255 exactly) → 254 may reach PGPatcher/ENB checks; blocking since S6 (7 of 28 real `_m` at ≥ 40% alpha ≠ 255) | Keep it on, or `--allow` |
| `ALPHA-254` | info | Decoded 254 present (bc7e with flag off, or a validated third-party DDS); harmless to CS's 251/255 gate [D-A] | Turn flag on if alpha is read |
| `ALPHA-NEAR-OPAQUE` | info | Source alpha 251–254 present, minimum ≥ 251: treated as opaque (BC encode artefact); alpha removed (DECISIONS Note L) | — |
| `TAG-SRGB-VANILLA` | error | `_SRGB` under `vanilla` or `cpm` [D#4] | UNORM |
| `TAG-SRGB-NONPBR` | blocking | `_SRGB` on a non-PBR sub-type under `cs` → renders darker [SC §6] | UNORM |
| `TAG-UNORM-PBR` | warning | UNORM on PBR colour → washed out with LL off [SC §6] | `_SRGB` |
| `TAG-SRGB-LOD` | blocking | `_SRGB` under `lod/` → LOD shaders reject sRGB [SC §4] | UNORM |
| `TAG-CONTENT-MISMATCH` | warning | Linear content with `_SRGB` tag, or colour sub-type with linear content | Check both fields |
| `CFG-INVALID` | error | A field value outside its allowed range, or a combination that cannot be honoured: threshold outside 5–200, `rgbcx_level` > 18 with the rgbcx backend, a backend that cannot write the format, `header = legacy` on BC6H, BC7 or `_SRGB`, a malformed `channels.swizzle`, `bc1_transparent_black` with BC1, a BC6H/`color.tag` mismatch, a user preset replacing `sdc-base`, an invalid suffix alias. In a preset, config, rule or `--set` value it is found during planning (exit 2, §8); on a combination reached after layering it fails that file (exit 1) | Fix the value |
| `CFG-UNAVAILABLE` | blocking | Sub-type or format unavailable under the target (BC6H outside `cs`, `flow` under `vanilla`) | Accept fallback or change target |
| `DIM-NPOT` | blocking | Non-power-of-two dimension [SC §5] | Resize policy, e.g. `--set resize=pot-down` (M6) |
| `DIM-SMALL` | blocking | Base dimension < 4 px [SC §5] | Resize or acknowledge |
| `DIM-FLOW-SMALL` | warning | `_flow` dimension ≤ 32 → CS ignores it [SC §3] | ≥ 64 px |
| `DIM-TOO-LARGE` | error | Dimension > 16384 | Downscale |
| `SET-MISSING-COMPANION` (M4) | info | PBR set lacks `_n` or roughness member; `_msn` without `_s`; `_flow` without `_n` | Add or ignore |
| `SET-COMPANION-OUTSIDE` (M4) | info | Roughness member on disk, not in batch → no bake | Add it |
| `SET-COMPANION-DDS` (M4) | warning | Roughness member only as DDS → no bake | Opt in or supply source |
| `SET-BAKE-RATIO` (M4) | warning | `_n`/roughness-member size ratio not power of two → bake off | Resize |
| `SUG-LOW-CONFIDENCE` | blocking | Sub-type or alpha meaning unconfirmed | Confirm |
| `SUG-CONTRADICTION` | warning | Content contradicts suffix | Confirm or change |
| `VAL-ALPHA-GUARANTEE` | error | Decoded alpha below guarantee, or ≠ 255 on an all-255 block with the flag on | Report bug; the shipped mechanism is chosen by the §11.2 gate |
| `VAL-COVERAGE` | warning | Cutout coverage drift > 2 points, checked on levels of 256 texels or more (§5.8) | Adjust threshold |
| `VAL-M-COMPLEX-MARGIN` | warning | Complex `_m` smallest-mip alpha ≥ 247/255: within the margin below CS's 251/255 gate, or at/above it (complex off); data `smallest_alpha`, `complex` [S8] | Adjust height range |
| `VAL-M-GRAYSCALE` | warning | Complex `_m` whose smallest mip is grey (channels < 4/255 apart) and not black-with-height → CS reads it as a plain env mask [S8] | Check channels |
| `VAL-M-HEIGHT-CLIP` | warning | Large `_m` alpha areas ≤ 4/255 or ≥ 251/255 → no parallax (CS), flat (ENB) | Remap height |
| `VAL-PGPATCHER-50` | warning | Alpha-carrying plain `_m` has ≥ 50% non-white → PGPatcher treats as complex | BC4 or flag on |
| `VAL-BC6H-PEAK` | warning | BC6H peak ratio < 0.8 [B2 §6] | Try NVTT Production |
| `VAL-HEADER` | error | Written header fails §5.7, or the encoder input handed to Validate does not match the file | Report bug |
| `VAL-MIP-ERROR` | warning | A mip's channel PSNR against the encoder input is below `validation.min_mip_psnr_db` (default 30 dB; §5.8); names the mip and its PSNR | Check the source, raise the effort or accept |
| `ENC-NVTT-MISSING` | blocking | NVTT chosen but not found | Install, set path, or built-in |
| `ENC-NVTT-VERSION` (M5) | blocking | Untested NVTT version | Acknowledge |
| `IN-DUPLICATE` | blocking | Two inputs, one output | Exclude one |
| `IN-DDS-SKIPPED` | info | DDS input not re-encoded by default | Opt in |
| `IN-PACKING-INPUT` | warning | Raw single-channel map (§4.4) is a channel-packing input, not converted on its own (its recipe is off, `packing.enabled = false`) | Enable packing, or set a sub-type for it |
| `IN-GREEN-UNSTATED` | blocking | A plain `_normal`/`_nrm` states no green convention (§4.4); never settled by `--accept-suggestions` or `--allow` | `--set 'file:<name>.channels.green_convention=opengl\|directx'` |
| `IN-DECODE` | error | Unreadable source | Fix file |
| `IN-NONFINITE` | warning | Source has NaN or infinite samples; Preprocess replaced them (NaN and −∞ → 0, +∞ → 1, or the half-float maximum for HDR colour) | Fix the source |
| `IN-FLOAT-LDR` | blocking | 32-bit float source resolved to a format other than BC6H; its transfer is not separated from the output encoding until M6 → "32-bit float source written to <fmt>: transfer handling lands in M6; acknowledge with --allow IN-FLOAT-LDR to convert as stored values" (interim guard, final review I4) | BC6H for HDR content under `cs`, an 8/16-bit source, or `--allow IN-FLOAT-LDR` |
| `OUT-UNKNOWN-EXISTING` | warning | Existing output without manifest record kept | `--on-existing overwrite` |
| `PACK-DEFAULTED` | warning | A packed channel has no source → neutral default (names output and channel) [D-H] | Supply the map or accept |
| `PACK-SIZE-MISMATCH` | error | Sources of one recipe differ in size (lists every source and its size) | Resize the sources |
| `PACK-CONFLICT` | warning | A pre-packed source and raw maps target one output; pre-packed wins by default. Also the intentional skip of the pre-packed file under `packing.prefer = raw` (Note M9) | Remove one side or set `packing.prefer` |
| `PACK-UNUSED-INPUT` | warning | Raw map used by no recipe under the target | Change target, set a sub-type, or remove it |

---

## 11. Testing strategy

### 11.1 Golden-image tests

**[Proposal]** `crates/engine/tests/golden.rs` synthesises a small source (64–256 px) per sub-type × target, including a vanilla `_n` with spec alpha, landscape diffuse with height alpha, complex `_m` with 1×1 alpha near 251, plain `_m`, `_n` + `_rmaos` and `_n` + `_rfaos` at 1:1 and 2:1, `_flow`, cutout foliage, a blended decal, a gradient sky and an HDR emissive. Each case asserts format, tag, header form, mip count, alpha policy, warning codes and per-mip metric floors. Byte hashes are asserted only under `pinned-isa`, since dispatched ISA may change bits [LF §9].

### 11.2 Bench reproduction gates

**[Proposal]** `sdc-bench --gates`, on `F:/ai/dds/bench` or the committed subset:

| Gate | Pass criterion |
|---|---|
| bc7e u5, flag off, `rgb_real` (mip 0) | Mean RGB PSNR within **0.01 dB** of the bench's 50.98 dB, each image within 0.01 dB, same decoder [B1 §3] |
| bc7e u5 on alpha and packed sets | Each image within 0.01 dB of `results.csv` |
| bc7e patch, flag off | Bit-identical to unpatched upstream on every set |
| bc7e patch, flag on (and byte-patch fallback) | Decoded alpha **== 255 on every pixel** of the opaque sets (`rgb_real`, `rgb_grad`, `stray_*` after `remove`) at every mip of full chains; mean RGB PSNR per set within **0.05 dB** of unpatched u5, worst image reported. *Measured −0.08 to −1.02 dB (S5): the 0.05 dB limit decides only whether the flag goes on everywhere (§14 #5, not proposed); `sdc-bench --gates` gates alpha == 255 and flag-off bit-identity, and reports ΔRGB as info [D S5].* |
| rgbcx BC1/BC3 L18, BC4 `-4`, BC5 `-5` | **Bit-exact** versus bc7enc_rdo in the bench's configuration [B1 §6], [B2 §3–4], including BC1 transparent-black **on** as the bench ran; a second bit-exact check compares the shipped **off** setting with `bc7enc` run with that mode disabled |
| ISPC `bc6h_veryslow`, `hdr_real` | mPSNR within 0.01 dB of 51.50 dB [B2 §6] |

### 11.3 Per-mip metrics and the smallest-mip alpha check

**[Finding]** The bench scores mip 0 only, in sRGB code values, without Skyrim sets [B1 §10], [B2 §8]. **[Proposal]** `sdc-bench` adds per-mip PSNR (stored and linear-light), per-mip normal angular error, per-mip coverage after BC7 for cutouts (none vs our solver vs DirectXTex `--keep-coverage`, as [MIP] recommends), a thin-bright-line image for linear-light mips, and the **smallest-mip alpha check**: for every `remove` output and all-255 block, decoded alpha per level against §5.8, with the 254 share recorded per level.

### 11.4 DirectXTex oracle

**[Proposal]** Pinned `texdiag info` must report the expected format, size and mip count for every golden output; `texconv` RGBA decodes must equal `bcdec_rs` exactly for BC4, BC5, BC6H and BC7, and within one code value for BC1/BC3 colour (decoder models differ [B1 §6]).

### 11.5 Robustness and in-game spot checks

**[Proposal]** Fuzz the DDS reader and loaders; test cancellation, atomic writes, `if-changed`, panic isolation and 8K/16K memory admission. Before each release, a documented checklist runs on a fixed CS install with its version recorded [SC §4]: TruePBR set with BC6H `_g` and `_SRGB` `_f` (LL on and off); a `_rfaos` skin set; vanilla diffuse and `_n` under `vanilla`; a CPM `_m` set; `_flow` hair; cutout and cutout + blend foliage at distance; the S8 `_m` variants.

---

## 12. Phase 0 spikes

**[Proposal]** Each spike takes at most one day; results go into `docs/DECISIONS.md`. **[Finding]** Status after Phase 2: S3 passed; S4 not adopted; S5 passed its alpha and bit-identity gates but missed the 0.05 dB RGB limit (the mechanism ships, scoped to `_m`); S7 passed. After the in-game round: S1 closed by the owner's answer (PBR `_f`/`_g` sRGB, BC6H for an emissive `_g`); S2 passed (no crash); S8 closed from a RenderDoc read of the CS shader (gate exactly 251/255, plus a smallest-mip grey rule); S6 closed offline (7 of 28 real `_m` at ≥ 40%, so `ALPHA-EXACT-OFF-M` is blocking; owner OK). The asset pack and checklist are in `docs/spikes/in-game-checklist.md`.

| Spike | Question | Method | Pass criterion | Priority |
|---|---|---|---|---|
| **S1** | Does BC6H `_g` render under CS, and do `_SRGB` `_g`/`_f` match the reference? (Other S1 items answered [D#2–5]) | TruePBR set: `_g` as BC6H_UF16 and BC7_UNORM_SRGB; `_f` as `_SRGB` and UNORM; screenshots with LL on/off | BC6H and `_SRGB` `_g` look alike in both LL modes without load errors; `_SRGB` `_f` matches the authoring reference, UNORM twin brighter | High |
| **S2** | Do small BC7 textures crash? | 200 BC7 textures of 16–128 px with full chains on common meshes | No crash in 10 minutes; else reinstate a fallback | Low [D#7] |
| **S3** | Does the workspace link the vendored encoders and reproduce a bench number? | Workspace + `encoders-sys` with committed objects; bc7e u5 on `rgb_real` | Builds without ISPC installed; mean RGB PSNR within 0.01 dB of 50.98; rgbcx BC4 bit-exact on `mask_real` | High |
| **S4** | Is scalar bc7e a viable fallback? | `basisu_bc7e_scalar.cpp` as a bench variant at u5 | ΔPSNR ≤ 0.01 dB per image and throughput recorded; adopt only if ≤ 4× slower | Medium |
| **S5** | Does decoded smallest-mip alpha validation work, and what does exact opaque alpha cost? | Implement §5.8's check; prototype the bc7e patch; full-chain `remove` encodes of `rgb_real`, `rgb_grad` with bc7e (flag off/on), NVTT, rgbcx BC1 | Flag off: ≥ 254 at every mip, 254 share per level recorded (incl. 1×1). Flag on, NVTT, BC1: 255 everywhere. Patch gate: alpha == 255 on every pixel, ΔRGB ≤ 0.05 dB. If the patch overruns the day, the check is proven on flag-off output and the patch moves to Phase 2 under the same gate. ΔRGB ≤ 0.05 dB → propose flag on everywhere | High |
| **S6** | How often would a plain `_m` stored with alpha cross PGPatcher's 50% rule without the flag? | ≥ 20 real plain `_m` as BC7, `remove`, flag off; count α ≠ 255 at mip 0 | Data recorded; if any file ≥ 40%, `ALPHA-EXACT-OFF-M` becomes blocking. Moot for defaults once S5's patch passes | Low |
| **S7** | Can NVTT be fed one mip at a time? | `nvcompress -nomips -bc7/-bc6 -production` per pre-built level; engine assembles | Output passes §5.8; per-level PSNR equals a direct encode within 0.01 dB; overhead ≤ 0.2 s/level; flags confirmed via `nvcompress -h` | Medium |
| **S8** | Does CS complex detection behave as the source says on our output? | `_m` variants with decoded 1×1 alpha 245, 248, 250, 252, 255, plus a black-RGB height mask | Complex shading/parallax on for 245–250, off for 252–255; margin set from the boundary | Medium |

---

## 13. Future extensions

**PBR JSON generator ("truepbr.json").** **[Requirement]** A generator in the spirit of jiayev's TruePBR-Manager, credited [D, scope]. **[Finding]** TruePBR-Manager writes no `truepbr.json`. It exports a PGPatcher JSON array to `<mod>/PBRNIFPatcher/<project>.json`: entries keyed by `texture` (vanilla match path) with `emissive`, `parallax`, `subsurface`, `subsurface_foliage`, `specular_level`, `roughness_scale`, `displacement_scale`, `subsurface_color`, `subsurface_opacity`, and conditionally `emissive_scale`, `emissive_color`, `match_normal`, `rename`, `slotN` paths, `lock_*` flags and coat, `fuzz`, `glint`, `hair` objects; landscape sets get `PBRTextureSets/<edid>.json` with material parameters only [TPM SPEC §3.10, §7–8]. **[Proposal]** The extension turns `sdc` texture sets (§6) into one entry each: slot paths under `textures/pbr/…`, features inferred from members (`_g` → emissive, `_p` → parallax, `_s` → subsurface, `_f` → fuzz), defaults such as `specular_level` 0.04 [SC §3], user parameters, and optional landscape EDID files. It is labelled "PGPatcher PBR JSON (TruePBR)" and built from PGPatcher's documented keys and TruePBR-Manager's published output shape, with no TruePBR-Manager code (§7.2).

**Linux.** **[Requirement]** Not a goal [D#12]; nothing is planned unless the owner reverses it.

**RDO.** **[Proposal]** bc7enc_rdo's `m_rdo_lambda` could shrink BSA archive size [EI §6]; it needs a size-versus-PSNR bench round first.

**Reserved-header hash.** **[Requirement]** Deferred behind the sidecar [D#15]; reviving it needs evidence that the Creation Kit, NifSkope, CAO and PGPatcher accept non-zero reserved fields.

**Others.** **[Proposal]** Cubemap assembly, NIF-aware alpha, BSA input, MO2/Synthesis integration.

---

## 14. Open items

| # | Item | Shipped default | Trigger to change | Owner |
|---|---|---|---|---|
| 1 | BC1 decoder model | Decided: `cBC1Ideal` batch default (owner 2026-10-08) | — | Closed |
| 2 | Alpha-test comparison | `gt`, as Castaño, DirectXTex, NVTT [MIP] | NIF survey or in-game test shows `>=` | S8 extension / owner |
| 3 | Coverage for `cutout_blend` | On (owner decision 2026-10-08); per-image switch | Blended edge fade at distance looks wrong in spot checks | Phase 6 |
| 4 | Variance-bake clamp *t* | 0.25 (Unity HDRP value, owner-approved) | Over-rough distance or residual shimmer in spot checks → retune per preset | Phase 6 |
| 5 | Exact opaque alpha, global default | Decided: on for `_m` / env masks only, off elsewhere (owner, Phase 0 exit; S5 cost −0.515 dB `rgb_real`, −1.020 dB `rgb_grad` exceeds 0.05 dB) | — | Closed |
| 6 | Exact-opaque mechanism | Decided: native patch 0001 (alpha == 255 on 18/18 images, flag off bit-identical on 39/39 on the host ISA); byte patch last resort, NVTT second fallback (M5) | — | Closed |
| 7 | Complex `_m` 1×1 margin | 4 code values below 251/255 (`complex_m_warn_alpha` 247); **closed by S8**: the gate is exactly `alpha < 251/255` | S8 boundary | S8 |
| 8 | BC6H peak threshold | Warn below 0.8 | Spot checks of highlight-heavy BC6H | Phase 6 |
| 9 | Turning `exact_opaque` off for an `_m` | **Closed by S6:** blocking (`ALPHA-EXACT-OFF-M`; 7 of 28 real `_m` at ≥ 40%, max 57.95%) | S6 finds a file ≥ 40% → blocking | S6 |
| 10 | NVTT per-mip feeding | Decided: one `-nomips` process per level, always `-nocuda` (S7 passed) | — | Closed |
| 11 | Scalar bc7e fallback | Not built (S4: not adopted, 0.0759 dB on 1 of 11 images, 3.8–4.8× slower) | ISPC objects fail on a needed configuration and the owner accepts a looser tolerance | Owner |
| 12 | Isolated batch workers | Off | Crash rate in Phases 3–4 [D#18] | Phases 3–4 |
| 13 | 16K preview | 8192, 16384 setting, smaller-mip fallback | Adapter limits on target hardware | GUI phase |
| 14 | Working name | `sdc` | Packaging decision | Owner, Phase 7 |
| 15 | Code signing and antivirus | Unsigned | False positives in release testing [D#16] | Phase 7 |
| 16 | Roughness default when a packed `_rmaos`/`_rfaos` has no roughness source | Decided: 0.5, preset field `packing.roughness_default`, confirmed by the owner 2026-10-09; per-channel global fill values override it (§3.2, §6.4) [D-H] | — | Closed |
| 17 | Output name of colour vocabulary files | `<base>.dds`; `_d` keeps `<base>_d.dds` (§4.4) | Owner prefers `<base>_d.dds` or another rule | Owner |
| 18 | Env-mask source of a packed `cpm`/`vanilla` `_m`; `cpm` `_m` height without a source | Decided: metallic, else specular; height 0.5 (§6.4) | Owner rule; the constant height passed S8 (128 < 251; RGB must not be grey) | Closed (DECISIONS Note M1) |
| 19 | Raw maps without a recipe | Decided: `_opacity`, specular-only and fuzz-only sets, height under `vanilla`, RGB `_sheen`: reported, not packed (§4.4, §6.4) | Owner names a recipe (e.g. opacity into colour alpha, specular into `_n` alpha) | Closed (DECISIONS Note M2) |
| 20 | Raw specular scale | Decided: copied unchanged into `_rmaos`/`_rfaos` A | Exporter "specular level" maps (0.5 = F0 0.04) shown to need a remap to TruePBR's A | Closed (DECISIONS Note M3) |
| 21 | Raw packing inputs and the exit code before M2.5 | Decided: not counted as skipped (exit 1) | Owner wants exit 1 when a set's packed map is missing | Closed (DECISIONS Note M4) |
| 22 | Global fill values for missing pack maps | Decided: per-role 8-bit codes via `packing.fill.<role>` and `--fill <role>=<0..255>`; provenance `fill:<v>` (§6.4) | M2.5 plan finalises the field path, CLI spelling, GUI control and whether a filled channel is still reported | Closed (DECISIONS Note M5) |

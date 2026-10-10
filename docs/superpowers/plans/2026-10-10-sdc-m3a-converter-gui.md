# M3a Converter GUI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `sdc-gui.exe`, a converter GUI over the existing engine: open a folder, see every texture and texture set, decide and edit per-texture settings (saved as `[files]` entries in the folder's `sdc.toml`), preview source vs encoded on the GPU, and export with a batch report.

**Architecture:** Engine work first (Tasks 2–8): an exact-path override scope, a `[files]` table in `sdc.toml` that is its own precedence layer, per-file `allow`, the single event-schema bump, pack-source roles in the manifest, an `engine::session::Session` that caches each file's `Analysis` (so edits and Mode changes re-resolve without reloading) and an in-memory `Session::preview` whose mips are byte-identical to `run`'s. Then a new crate `crates/gui` (Tasks 9–19): a plain-Rust view-model (`gui::model`, no egui types, tested headless), a format-preserving `sdc.toml` editor (`gui::project`, `toml_edit`), a wgpu preview renderer (`gui::render`) and the egui drawing (`gui::ui`). Worker threads do all engine work and report through channels the UI drains once per frame.

**Tech Stack:** Rust stable (`x86_64-pc-windows-msvc`, `+crt-static`), the `F:/ai/dds/converter` workspace (`engine`, `cli`, new `gui`); `eframe`/`egui` 0.36.2 with the wgpu renderer (wgpu through the `eframe::egui_wgpu::wgpu` re-export only), `rfd` 0.17, `toml_edit` 0.25; dev-only `egui_kittest` 0.36.2 (no `snapshot`/`wgpu` features).

**Spec:** `docs/superpowers/specs/2026-10-10-sdc-m3a-converter-gui-design.md` (owner-approved 2026-10-10; supersedes main spec §9 for what M3a builds). Background: main spec `docs/superpowers/specs/2026-10-08-skyrim-dds-converter-design.md` §3, §5, §6.4, §8, §9, §10; `docs/DECISIONS.md` Notes A–M; the M3 section ("Carried items", "Carried from M2.5") of `docs/superpowers/plans/2026-10-08-skyrim-dds-converter-phase0-2.md`.

## Global Constraints

Every task's requirements implicitly include these lines.

- Licence **GPL-3.0-or-later**. No TruePBR-Manager (CC BY-NC 4.0) or Skyking (reference only) code, assets or names are copied. Every new dependency must have a GPL-compatible licence (MIT, Apache-2.0, BSD, Zlib, MPL-2.0); record it in `THIRD_PARTY_NOTICES.md`.
- Platform **Windows x64 only**; Rust stable on **`x86_64-pc-windows-msvc`**; `+crt-static`; **no ISPC at build time**. In Git Bash run `export PATH="$HOME/.cargo/bin:$PATH"` before `cargo`. The `gui` crate declares `rust-version = "1.95"` (eframe 0.36.2's floor); the other crates keep 1.88.
- **Pinned GUI crates:** `eframe = 0.36.2` (default features: the wgpu renderer, accesskit, default fonts; never the `glow` renderer), `egui_kittest = 0.36.2` (dev, default features only), `rfd = 0.17.2`, `toml_edit = 0.25`. **Never add `wgpu` as a direct dependency**: use `eframe::egui_wgpu::wgpu`, so there is one `wgpu::Device` type. **API drift:** this plan's egui/wgpu code targets 0.36.2 from memory of the API; if a call does not compile against the pinned version, adapt the call, keep the behaviour, and say so in your report.
- **Never auto-decide** (spec §1): the GUI runs the engine with `accept = AcceptSuggestions::None`; a decision is written explicitly (a field value or an `allow` code). "Proceed anyway" exports only decided textures and skips the rest.
- **`[files]` precedence** (spec §5.3): built-in → preset chain → user config → project config (`defaults`, `subtypes`) → preset and config rules → analysis facts → **`[files]` entries** → command-line `--set`. `[files]` is its own layer, applied before any command-line override; scope rank orders overrides only within one layer.
- **`[files]` key rule:** a key is a `/`-separated path relative to the folder holding `sdc.toml` (the *project root*). It matches a file when, compared case-insensitively, `rel == key` or `rel-without-its-last-extension == key`, where `rel` is the file's path relative to the project root (for a packed output: the folder of its first source plus the virtual file name). The GUI writes real files with their extension (`landscape/rock01_n.png`) and packed outputs without (`landscape/rock01_rmaos`). `[files]` is read only from the project config; a `[files]` table in the user config is `EngineError::Config`.
- **The GUI owns only** `target`, `[files.*]` and `defaults.packing.fill.*` in `sdc.toml`; every other byte (comments, ordering, rules, other tables) survives a GUI save unchanged.
- **Preview:** encoding is **off by default** (a **Live encode** toggle and an **[Encode now]** button). The preview size limit is its own setting, **4096 by default** (values 1024, 2048, 4096, 8192, 16384; 16384 only as an explicit setting). Only mips whose larger side is ≤ the limit are encoded, and each is **byte-identical** to the same mip of the DDS `run` writes.
- **Not in M3a:** the isolated-worker mode, per-stage progress events (M7), switching the BC1 decoder model inside a running session (read at launch; a change shows "restart sdc-gui to apply"), undo/redo and set-membership editing (M3b), image edits (M3c).
- Event and manifest schemas: **`EVENT_SCHEMA` becomes 2 once** (Task 4) and Tasks 5 extends schema 2 without another bump; **`MANIFEST_SCHEMA` becomes 2** (Task 6). Every published schema in `schema/` is regenerated in the same commit as the type change: `cargo run -q -p sdc -- presets schema all --out schema`; `crates/engine/tests/schemas.rs` must stay green.
- CLI behaviour is unchanged except where a task says so: human output on stderr, `--json` stdout carries only JSON Lines, exit codes 3 > 2 > 1 > 4 > 0.
- Threads: the UI thread never blocks on engine work. A fresh `CancelToken` per engine call (tokens cannot be reset). The GUI's `EventSink` only pushes into a channel.
- Process rules (owner): never use `python3 -`/`python -` heredoc probes (they hang on this machine; write a script file or use the Edit tool); commit only the paths you changed, by name; `G:/` is read-only; never touch `F:/ai/dds/bench` (`results.csv` and `results2.csv` are frozen); downloads beyond the pinned crates need the owner's permission.
- Before each commit run `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace`; all must pass. GUI tests must not need a GPU: rendering smoke tests use `egui_kittest` without a renderer.

## Review Focus

Inputs the spec implies but no section tests directly. Each has a test in the task named.

1. **Mode change while a preview encode is in flight** (cs `_rmaos` becomes cpm `_m`, so the previewed job key no longer exists): the encode is cancelled, its late result is dropped, selection falls back to the set (or clears), and nothing panics. Test: Task 15 `mode_change_cancels_and_drops_a_preview_of_a_vanished_job`.
2. **Export folder equal to the opened folder, or an ancestor of it** (discovery skips everything under the output root, so every source would vanish next time): Export refuses with a message; a subfolder of the opened folder is allowed. Test: Task 14 `export_refuses_the_input_folder_or_an_ancestor`.
3. **Export when the save that precedes it fails** (`sdc.toml` read-only): Export stops before running, the edits stay unsaved, and the error is shown; otherwise `sdc convert <folder>` would not reproduce the export. Test: Task 14 `export_stops_when_the_save_fails`.
4. **A `[files]` key for a packed output whose recipe does not run under the current target** (`landscape/rock01_rmaos` after switching to `cpm`): the key raises one `CFG-FILE-UNMATCHED` info event, stays in the file, and is listed as unmatched in the Advanced drawer. Test: Task 3 `an_unmatched_files_key_is_reported_once_and_kept` and Task 18 `unmatched_entries_are_listed_and_removable`.
5. **HDR and 16-bit sources in thumbnails** (an `.exr`-like float source or a 16-bit PNG): thumbnails are tone-mapped or scaled into 8-bit, never black or all-white. Test: Task 7 `thumbnails_of_float_and_16_bit_sources_are_visible`.

## Decisions taken in this plan

Recorded by Task 1 as DECISIONS **Note N**.

- **N1 Preview size default 4096** (owner, 2026-10-10), replacing DECISIONS #19's 8192 for the preview only; export size is unaffected.
- **N2 `[files]` layer** as in Global Constraints (precedence and key rule). Provenance of a `[files]` value is `project_file` (new `Provenance::ProjectFile`, between `suggestion` and `user`).
- **N3 Proceed anyway** skips undecided textures (`unresolved`, `blocked`); the GUI never decides for the user.
- **N4 In-memory project config:** `BatchRequest::project_config_inline: Option<ConfigFile>` takes the place of reading `sdc.toml` from disk, so unsaved GUI edits re-resolve before Ctrl+S. The project root for `[files]` keys is then the first input folder.
- **N5 Skip kinds:** `up_to_date`, `on_existing_skip`, `unresolved`, `blocked`, `duplicate`, `dds_input`, `packing_input`, `unknown_existing`, `pack_conflict` (a pre-packed file replaced under `prefer = raw`) and `packed_source` (a raw map consumed by a recipe; its terminal event). The spec's list plus the last two, which M2.5's recipes need.
- **N6 Job key** = `manifest::output_key(output_root, output)` ('/'-separated output path relative to the output root), on every per-file event as `EngineEvent::job`.
- **N7 Manifest sources carry role and invert;** a schema-1 record has none, so its output re-encodes once under `if-changed`.
- **N8 `Session::preview(&ResolvedJob, max_size, &CancelToken)`** takes the job from the latest plan (the spec's "job key" form is the GUI's lookup, not the API).
- **N9 Typed decisions:** `JobStatus::Unresolved` gains `needs: Vec<Need>` (`subtype`, `alpha_meaning`, `green_convention`) so the GUI shows confirm controls without parsing reason text.

## File map

| File | Task | Responsibility |
|---|---|---|
| `crates/engine/src/overrides.rs` | 2, 3 | `Scope::Path`, `path_key_matches`, `FileScopes` |
| `crates/engine/src/policy.rs` | 2, 3, 4 | `ResolveCtx::files`, `allowed`, `[files]` layer, `Need` |
| `crates/engine/src/presets.rs` | 3, 4 | `ConfigFile::files`, `Provenance::ProjectFile`, `Deserialize for Provenance` |
| `crates/engine/src/api.rs` | 2–7 | `BatchRequest` fields, project root, events with job keys, `plan_cached` |
| `crates/engine/src/events.rs` | 4, 5 | `EVENT_SCHEMA = 2`, typed `FilePlanned`, `SkipKind`, `EngineEvent::job`, `Reporter::emit_job` |
| `crates/engine/src/manifest.rs` | 6 | `SourceRef::role`/`invert`, `MANIFEST_SCHEMA = 2` |
| `crates/engine/src/session.rs` (new) | 7, 8 | `AnalysisCache`, `Thumbnail`, `Session`, `PreviewResult` |
| `crates/engine/src/pipeline.rs` | 8 | `prepare` / `encode_levels` shared by `process_file` and `Session::preview` |
| `crates/cli/src/{verbs,human}.rs` | 4, 5 | read the typed job and the skip kind |
| `crates/gui/` (new) | 9–19 | `sdc-gui.exe` |
| `crates/gui/src/settings.rs` | 9 | `GuiSettings` in `%APPDATA%\sdc\gui.toml` |
| `crates/gui/src/project.rs` | 10 | `ProjectDoc`: format-preserving `sdc.toml` |
| `crates/gui/src/model/*.rs` | 11, 13–15, 18, 19 | view-model, no egui types |
| `crates/gui/src/worker.rs` | 11 | worker threads, `WorkerMsg` |
| `crates/gui/src/ui/*.rs` | 9, 12–14, 17, 18 | egui drawing only |
| `crates/gui/src/render/*.rs` | 16 | wgpu textures, WGSL, paint callback, CPU fallback |

---

### Task 1: Branch, decisions and spec wording

**Files:**
- Modify: `docs/DECISIONS.md` (new section after Note M)
- Modify: `docs/superpowers/specs/2026-10-10-sdc-m3a-converter-gui-design.md` (§3 table row, §4.5 heading and body)
- Commit (already written, uncommitted): the spec above, the §9 pointer line in `docs/superpowers/specs/2026-10-08-skyrim-dds-converter-design.md`, and this plan.

**Interfaces:** none (documents only).

- [ ] **Step 1: Create the branch**

```bash
cd F:/ai/dds/converter
git checkout main
git checkout -b feat/m3a-gui
```

- [ ] **Step 2: Add DECISIONS Note N** after the `## Note M — …` section (before `## Phase 2 exit`):

```markdown
## Note N — M3a converter GUI decisions (owner and M3a plan, 2026-10-10)

The owner split M3 into M3a (converter GUI), M3b (set editor and PGPatcher JSON) and M3c (texture edits) and approved the M3a design section by section (spec `docs/superpowers/specs/2026-10-10-sdc-m3a-converter-gui-design.md`).

- **N1 Preview size limit 4096 by default** (owner): replaces the 8192 of #19 for the GUI preview only. Values 1024–16384; 16384 only as an explicit setting. The preview encodes only mips at or below the limit; each is byte-identical to the exported mip.
- **N2 `[files]` in `sdc.toml`** (owner): per-texture GUI edits are saved as `[files."<path relative to sdc.toml>"]` entries (any profile field plus `allow = [codes]`). They are their own layer: above preset and config rules and analysis facts, below command-line `--set`. A key matches a file when `rel == key` or `rel` without its last extension `== key`, case-insensitively; packed outputs are keyed without extension. Provenance `project_file`. Only the project config may hold `[files]`.
- **N3 Proceed anyway** (owner): exporting with undecided textures asks "Proceed anyway / Cancel"; proceeding skips the undecided textures and never decides for the user.
- **N4 In-memory project config** (plan): `BatchRequest::project_config_inline` lets the GUI re-resolve unsaved edits; Export saves `sdc.toml` first, so `sdc convert <folder>` reproduces the last export.
- **N5 Skip kinds** (plan): `file_skipped.kind` is one of `up_to_date`, `on_existing_skip`, `unresolved`, `blocked`, `duplicate`, `dds_input`, `packing_input`, `unknown_existing`, `pack_conflict`, `packed_source`.
- **N6 Job key** (plan): every per-file event carries `job`, the output path relative to the output root (`manifest::output_key`).
- **N7 Manifest sources record role and invert** (plan, M2.5 carry): replaces the `pack.*` provenance comparison in `if-changed`; schema-1 records re-encode once.
- **N8 Preview API** (plan): `Session::preview(&ResolvedJob, max_size, &CancelToken)`.
- **N9 Typed unresolved needs** (plan): `JobStatus::Unresolved.needs` lists `subtype`, `alpha_meaning` and `green_convention`.
```

- [ ] **Step 3: Fix the preview API name in the M3a spec.** In §3's table, change the row `| \`engine::preview\` (new) | One job, in memory, to encoded mips plus readouts (§6) | existing pipeline stages |` to `| \`engine::session\` preview (new) | \`Session::preview\`: one job, in memory, to encoded mips plus readouts (§6) | existing pipeline stages |`. In §4.5 change the heading `### 4.5 \`Engine::preview\`` to `### 4.5 \`Session::preview\``, and in its body change `Session::preview(job_key, max_size, &CancelToken)` to `Session::preview(&ResolvedJob, max_size, &CancelToken)` and add after that sentence: `The GUI finds the job in its latest plan by job key (§4.3).` In §5.1 replace the sentence "The session plans it on a worker thread; thumbnails fill in as files are analysed, and a status line shows \"Analysing 34 / 110\"." with "The session plans it on a worker thread; the status line shows \"Analysing 110 files…\" and the thumbnails appear when planning finishes. (The engine emits no per-file event while it loads and analyses — per-stage events are M7 — so a running count is not available in M3a; plan ruling, reported to the owner at plan review.)"

- [ ] **Step 4: Commit**

```bash
git add docs/DECISIONS.md docs/superpowers/specs/2026-10-10-sdc-m3a-converter-gui-design.md docs/superpowers/specs/2026-10-08-skyrim-dds-converter-design.md docs/superpowers/plans/2026-10-10-sdc-m3a-converter-gui.md
git commit -m "docs: M3a converter GUI spec, plan and DECISIONS Note N"
```

---

### Task 2: Exact-path scope and per-file allow

**Files:**
- Modify: `crates/engine/src/overrides.rs` (add `Scope::Path`, `path_key_matches`, `FileScopes`, `NO_FILE_SCOPES`)
- Modify: `crates/engine/src/policy.rs:30-42` (`ResolveCtx::files`), `:185-203` (`override_applies`, `ranked_overrides`), `:461-480` (reach for `Scope::Path`), `:1101`, `:1202`, `:1213` (`allowed`)
- Modify: `crates/engine/src/api.rs:52-103` (`BatchRequest::file_allow`), `:623-632` (build `FileScopes`)
- Modify: every `ResolveCtx { … }` literal: `grep -rn "ResolveCtx {" crates` (api.rs and about ten in `crates/engine/tests/policy_checks.rs`; also check `crates/engine/src/pack/`)
- Test: `crates/engine/tests/overrides.rs`, `crates/engine/tests/file_scopes.rs` (new)

**Interfaces:**
- Produces (`engine::overrides`):
  - `Scope::Path(String)` — a project-relative key (Global Constraints key rule).
  - `pub fn path_key_matches(key: &str, rel: &str) -> bool`
  - `pub fn project_rel(root: &Path, input: &InputFile) -> Option<String>` — `'/'`-separated path of `input` relative to `root`: the folder of `input.path` relative to `root`, joined with the file name of `input.rel` (so a packed output's virtual name is used).
  - `#[derive(Debug, Clone, Default, PartialEq)] pub struct FileScopes { pub root: Option<PathBuf>, pub overrides: Vec<SetOverride>, pub allow: BTreeMap<String, BTreeSet<Code>> }` and `pub static NO_FILE_SCOPES: FileScopes`.
- Produces (`engine::policy`): `ResolveCtx::files: &'a FileScopes`; `pub fn allowed(ctx: &ResolveCtx, f: &FileFacts, code: Code) -> bool`.
- Produces (`engine::api`): `BatchRequest::file_allow: BTreeMap<String, BTreeSet<Code>>` (keys as `[files]` keys, root = the project root of Task 3; until Task 3 lands, the first input folder when it is a directory).

- [ ] **Step 1: Write the failing tests** in `crates/engine/tests/overrides.rs`:

```rust
use engine::overrides::{path_key_matches, project_rel};
use engine::discover::{InputFile, InputKind};
use std::path::{Path, PathBuf};

#[test]
fn path_keys_match_exactly_or_without_the_last_extension() {
    assert!(path_key_matches("landscape/rock01_n.png", "landscape/rock01_n.png"));
    assert!(path_key_matches("Landscape/Rock01_N.PNG", "landscape/rock01_n.png"));
    // A packed output is keyed without extension; its virtual name takes its first source's extension.
    assert!(path_key_matches("landscape/rock01_rmaos", "landscape/rock01_rmaos.tga"));
    // Glob syntax is literal.
    assert!(path_key_matches("a/[old]{1}*.png", "a/[old]{1}*.png"));
    assert!(!path_key_matches("a/[old]{1}*.png", "a/o1x.png"));
    // No basename matching across folders.
    assert!(!path_key_matches("rock01_n.png", "landscape/rock01_n.png"));
    assert!(!path_key_matches("landscape/rock01_n.png", "other/landscape/rock01_n.png"));
    // Only the last extension is dropped, and only from the file.
    assert!(path_key_matches("a/rock.01_rmaos", "a/rock.01_rmaos.png"));
    assert!(!path_key_matches("a/rock", "a/rock.01_rmaos.png"));
}

#[test]
fn project_rel_uses_the_virtual_file_name() {
    let root = Path::new("C:/mod/textures");
    let real = InputFile {
        path: PathBuf::from("C:/mod/textures/landscape/rock01_roughness.png"),
        rel: PathBuf::from("landscape/rock01_roughness.png"),
        game_path: "textures/landscape/rock01_roughness.png".into(),
        output: PathBuf::from("C:/out/landscape/rock01_roughness.dds"),
        kind: InputKind::Image,
    };
    assert_eq!(project_rel(root, &real).as_deref(), Some("landscape/rock01_roughness.png"));
    let virt = InputFile {
        rel: PathBuf::from("landscape/rock01_rmaos.png"),
        ..real.clone()
    };
    assert_eq!(project_rel(root, &virt).as_deref(), Some("landscape/rock01_rmaos.png"));
    assert_eq!(project_rel(Path::new("D:/elsewhere"), &real), None);
}
```

Create `crates/engine/tests/file_scopes.rs` (the `Scope::Path` precedence test comes in Task 3, where `[files]` can produce path overrides):

```rust
use engine::api::*;
use engine::codes::Code;
use engine::config::Target;
use engine::events::*;
use engine::overrides::parse_set;
use engine::policy::{JobStatus, ResolvedJob};
use image::{ImageBuffer, Rgb};
use std::path::Path;
use std::sync::Arc;

/// A `w`×`w` RGB PNG with no alpha at `dir/rel`.
fn png(dir: &Path, rel: &str, w: u32) {
    let p = dir.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    ImageBuffer::from_fn(w, w, |x, y| Rgb([(x * 4) as u8, (y * 4) as u8, 90]))
        .save(p)
        .unwrap();
}

fn plan(req: BatchRequest) -> Plan {
    let rep = Reporter::new(Arc::new(VecSink::default()), "t");
    Engine::new().unwrap().plan(req, &rep, &CancelToken::default()).unwrap()
}

fn job<'a>(p: &'a Plan, rel: &str) -> &'a ResolvedJob {
    p.jobs
        .iter()
        .find(|j| j.input.to_string_lossy().replace('\\', "/").ends_with(rel))
        .unwrap_or_else(|| panic!("no job for {rel}"))
}

/// DIM-NPOT is blocking; acknowledging it for `a/x.png` must leave `b/x.png` blocked.
#[test]
fn a_per_file_allow_acknowledges_one_file_only() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("in");
    png(&root, "a/x.png", 48);
    png(&root, "b/x.png", 48);
    let mut req = BatchRequest::new(vec![root.clone()], d.path().join("out"), Target::Cs);
    req.recursive = true;
    req.overrides = vec![parse_set("file:**/x.png.subtype=diffuse").unwrap()];
    req.file_allow
        .insert("a/x.png".into(), [Code::DimNpot].into_iter().collect());
    let p = plan(req);
    assert_eq!(job(&p, "a/x.png").status, JobStatus::Ready);
    assert!(matches!(
        &job(&p, "b/x.png").status,
        JobStatus::Blocked { codes } if codes.contains(&Code::DimNpot)
    ));
}
```

They must fail to compile now (no `path_key_matches`, `project_rel`, `file_allow`).

- [ ] **Step 2: Run the tests to see them fail**

Run: `cargo test -p engine --test overrides --test file_scopes`
Expected: compile errors naming `path_key_matches`, `project_rel`, `file_allow`.

- [ ] **Step 3: Implement in `crates/engine/src/overrides.rs`**

```rust
use crate::codes::Code;
use crate::discover::InputFile;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    All,
    Subtype(Subtype),
    File(String),
    /// A `[files]` key (DECISIONS N2): '/'-separated, relative to the project root, matched by `path_key_matches`.
    Path(String),
}

/// Whether a `[files]` key names the file at project-relative path `rel` (DECISIONS N2): case-insensitively,
/// `rel == key`, or `rel` without the last extension of its file name `== key` (a packed output's key).
pub fn path_key_matches(key: &str, rel: &str) -> bool {
    let (k, r) = (key.to_lowercase(), rel.to_lowercase());
    if r == k {
        return true;
    }
    let (dir, name) = r.rsplit_once('/').map_or(("", r.as_str()), |(d, n)| (d, n));
    match name.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => {
            let without = if dir.is_empty() { stem.to_string() } else { format!("{dir}/{stem}") };
            without == k
        }
        _ => false,
    }
}

/// `input`'s path relative to `root`, '/'-separated: the folder of `input.path` relative to `root`, then the file
/// name of `input.rel` (for a packed output, its virtual name). `None` when `input` is not under `root`.
pub fn project_rel(root: &Path, input: &InputFile) -> Option<String> {
    let lower = |p: &Path| p.to_string_lossy().replace('\\', "/").to_lowercase();
    let dir = input.path.parent()?;
    let (root_s, dir_s) = (lower(root), lower(dir));
    let root_s = root_s.trim_end_matches('/');
    let rest = if dir_s == root_s {
        ""
    } else {
        dir_s.strip_prefix(&format!("{root_s}/"))?
    };
    // Keep the original case of the folder part: take the same number of bytes from the original string.
    let dir_orig = dir.to_string_lossy().replace('\\', "/");
    let rest_orig = &dir_orig[dir_orig.len() - rest.len()..];
    let name = input.rel.file_name()?.to_string_lossy();
    Some(if rest_orig.is_empty() { name.into_owned() } else { format!("{rest_orig}/{name}") })
}

/// The `[files]` layer (DECISIONS N2) and the per-file acknowledgements, resolved against one project root.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FileScopes {
    /// The folder `[files]` keys are relative to; `None` disables the layer.
    pub root: Option<PathBuf>,
    /// `[files]` field values, every one `Scope::Path`, in key order then field order.
    pub overrides: Vec<SetOverride>,
    /// Key → codes acknowledged for that file only.
    pub allow: BTreeMap<String, BTreeSet<Code>>,
}

impl FileScopes {
    pub const NONE: FileScopes = FileScopes {
        root: None,
        overrides: Vec::new(),
        allow: BTreeMap::new(),
    };
    /// The project-relative path of `input` when the layer is active.
    pub fn rel_of(&self, input: &InputFile) -> Option<String> {
        project_rel(self.root.as_deref()?, input)
    }
}

/// For `ResolveCtx` literals that have no `[files]` layer (tests, `inspect` on a single file).
pub static NO_FILE_SCOPES: FileScopes = FileScopes::NONE;
```

(`project_rel`: the lower-cased comparison handles NTFS case; slicing the original by the lower-cased length is safe only when lower-casing kept byte lengths, so if `dir_orig.len() != dir_s.len()` fall back to returning `rest` as computed from `dir_s`.)

- [ ] **Step 4: Wire `policy.rs`**

```rust
pub struct ResolveCtx<'a> {
    // … existing fields unchanged …
    /// Codes allowed with `--allow`.
    pub allow: &'a BTreeSet<Code>,
    /// The `[files]` layer and per-file acknowledgements (DECISIONS N2).
    pub files: &'a FileScopes,
}

/// Whether `code` is acknowledged for this file: batch-wide `--allow`, or the file's own `allow`.
pub fn allowed(ctx: &ResolveCtx, f: &FileFacts, code: Code) -> bool {
    ctx.allow.contains(&code)
        || ctx.files.rel_of(f.input).is_some_and(|rel| {
            ctx.files
                .allow
                .iter()
                .any(|(k, codes)| codes.contains(&code) && path_key_matches(k, &rel))
        })
}

fn override_applies(ctx: &ResolveCtx, o: &SetOverride, st: Option<Subtype>, f: &FileFacts) -> bool {
    match &o.scope {
        Scope::All => true,
        Scope::Subtype(s) => Some(*s) == st,
        Scope::File(g) => glob_match(g, f),
        Scope::Path(k) => ctx.files.rel_of(f.input).is_some_and(|rel| path_key_matches(k, &rel)),
    }
}

/// Overrides, lowest precedence first: the `[files]` layer, then the command line (unscoped, sub-type, file glob,
/// path), each group in its own order (the sort is stable). DECISIONS N2: the command line always wins.
fn ranked_overrides<'a>(ctx: &ResolveCtx<'a>) -> Vec<&'a SetOverride> {
    let rank = |o: &SetOverride| match o.scope {
        Scope::All => 0,
        Scope::Subtype(_) => 1,
        Scope::File(_) => 2,
        Scope::Path(_) => 3,
    };
    let mut cli: Vec<&SetOverride> = ctx.overrides.iter().collect();
    cli.sort_by_key(|o| rank(o));
    ctx.files.overrides.iter().chain(cli).collect()
}
```

Update both callers of `override_applies` to pass `ctx`. In `layered_profile_with` (`policy.rs:466-470`) map `Scope::Path(_) => FILE` as for `Scope::File`. Replace the three `ctx.allow.contains(&X)` reads at `:1101`, `:1202`, `:1213` with `allowed(ctx, f, X)` (at `:1202` the code is `w.code`). `grep -rn "allow.contains" crates/engine/src` afterwards must print nothing outside `allowed`.

- [ ] **Step 5: Wire `api.rs`.** Add to `BatchRequest`:

```rust
    /// Per-file acknowledgements (DECISIONS N2): `[files]` key → codes. Merged with the project config's
    /// `[files.*].allow` (Task 3).
    pub file_allow: BTreeMap<String, BTreeSet<Code>>,
```

with `file_allow: BTreeMap::new()` in `BatchRequest::new`. In `plan`, before `let ctx = ResolveCtx {`, build:

```rust
        let file_scopes = FileScopes {
            root: req.inputs.first().filter(|p| p.is_dir()).cloned(),
            overrides: vec![],
            allow: req.file_allow.clone(),
        };
```

and pass `files: &file_scopes`. Add `files: &NO_FILE_SCOPES` to every other `ResolveCtx { … }` literal (tests included). `check_up_front` needs no change (it filters `Scope::All`).

- [ ] **Step 6: Run the tests**

Run: `cargo test -p engine --test overrides --test file_scopes` then `cargo test --workspace`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings
git add crates/engine/src/overrides.rs crates/engine/src/policy.rs crates/engine/src/api.rs crates/engine/tests/overrides.rs crates/engine/tests/file_scopes.rs crates/engine/tests/policy_checks.rs
git commit -m "engine: exact-path override scope and per-file allow (M3a)"
```

(Add any other file the `ResolveCtx` grep made you touch, by name.)

---

### Task 3: The `[files]` table and the in-memory project config

**Files:**
- Modify: `crates/engine/src/presets.rs:108-130` (`ConfigFile::files`, `ConfigFile::file_entries`), `:131-200` (`Provenance::ProjectFile`, `Display`, `FromStr`)
- Modify: `crates/engine/src/codes.rs` (`CfgFileUnmatched`)
- Modify: `crates/engine/src/api.rs` (`BatchRequest::project_config_inline`, `setup`, project root, `[files]` into `FileScopes`, `check_up_front`, unmatched keys)
- Modify: `crates/engine/src/policy.rs` (`[files]` values get `Provenance::ProjectFile`; `resolve_file` treats it like `User`/`Rule`)
- Modify: `crates/engine/src/schemas.rs` (partial `Profile` schema for `defaults`, `subtypes`, `overrides`, `files`)
- Modify: `schema/config.schema.json`, `schema/preset.schema.json` (regenerated)
- Modify: `docs/superpowers/specs/2026-10-08-skyrim-dds-converter-design.md` §10 (add `CFG-FILE-UNMATCHED`)
- Test: `crates/engine/tests/project_files.rs` (new)

**Interfaces:**
- Consumes: Task 2's `Scope::Path`, `FileScopes`, `path_key_matches`, `project_rel`.
- Produces:
  - `ConfigFile::files: BTreeMap<String, serde_json::Value>` — each value a partial profile plus optional `allow: [code strings]`.
  - `ConfigFile::file_entries(&self) -> Result<(Vec<SetOverride>, BTreeMap<String, BTreeSet<Code>>), PresetError>` — flattens each entry to `Scope::Path(key)` overrides (fields in `FIELD_PATHS` order? no: in `leaf_paths` order) and the `allow` codes; an unknown field or code is `PresetError::Parse` naming the key.
  - `BatchRequest::project_config_inline: Option<ConfigFile>` — when `Some`, used as the project config instead of any file; the project root is then the first input folder.
  - `Provenance::ProjectFile` (`"project_file"`).
  - `Code::CfgFileUnmatched` (`"CFG-FILE-UNMATCHED"`, `Info`), emitted once per unmatched key, path `None`, message `"[files] entry '<key>' matches no file"`, data `{ "key": "<key>" }`.
  - `pub fn api::project_root(req: &BatchRequest) -> Option<PathBuf>`: inline → first input when it is a directory; else the parent of `req.project_config`, else the first input when it is a directory and holds `sdc.toml`.

- [ ] **Step 1: Write the failing tests** in `crates/engine/tests/project_files.rs`:

```rust
//! M3a Task 3: the `[files]` table of `sdc.toml` (DECISIONS N2) and the in-memory project config (N4).
use engine::api::*;
use engine::codes::Code;
use engine::config::Target;
use engine::events::*;
use engine::overrides::parse_set;
use engine::policy::{JobStatus, ResolvedJob};
use engine::presets::ConfigFile;
use image::{ImageBuffer, Luma, Rgb};
use std::path::Path;
use std::sync::Arc;
use tempfile::TempDir;

fn rgb(dir: &Path, rel: &str, w: u32, c: [u8; 3]) {
    let p = dir.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    ImageBuffer::from_fn(w, w, |x, _| Rgb([c[0].wrapping_add(x as u8), c[1], c[2]]))
        .save(p)
        .unwrap();
}

fn gray(dir: &Path, rel: &str, v: u8) {
    let p = dir.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    ImageBuffer::from_fn(64, 64, |x, _| Luma([v.wrapping_add(x as u8)]))
        .save(p)
        .unwrap();
}

/// `in/` with two `rock01_n.png` in two folders, a diffuse, two 48 px (NPOT) files, and roughness + AO so cs packs
/// `landscape/rock01_rmaos`. `toml` is written to `in/sdc.toml`.
fn fixture(toml: &str) -> TempDir {
    let d = tempfile::tempdir().unwrap();
    let r = d.path().join("in");
    rgb(&r, "landscape/rock01_n.png", 64, [128, 128, 255]);
    rgb(&r, "other/rock01_n.png", 64, [128, 128, 255]);
    rgb(&r, "landscape/rock01.png", 64, [90, 60, 30]);
    rgb(&r, "landscape/odd.png", 48, [90, 60, 30]);
    rgb(&r, "other/odd.png", 48, [90, 60, 30]);
    gray(&r, "landscape/rock01_roughness.png", 120);
    gray(&r, "landscape/rock01_ao.png", 200);
    std::fs::write(r.join("sdc.toml"), toml).unwrap();
    d
}

fn req(d: &TempDir) -> BatchRequest {
    let mut r = BatchRequest::new(vec![d.path().join("in")], d.path().join("out"), Target::Cs);
    r.recursive = true;
    r
}

fn plan_events(req: BatchRequest) -> (Result<Plan, EngineError>, Vec<EngineEvent>) {
    let sink = Arc::new(VecSink::default());
    let rep = Reporter::new(sink.clone(), "t");
    let p = Engine::new().unwrap().plan(req, &rep, &CancelToken::default());
    (p, sink.take())
}

fn plan(req: BatchRequest) -> Plan {
    plan_events(req).0.unwrap()
}

fn job<'a>(p: &'a Plan, tail: &str) -> &'a ResolvedJob {
    p.jobs
        .iter()
        .find(|j| j.output.to_string_lossy().replace('\\', "/").ends_with(tail))
        .unwrap_or_else(|| panic!("no job writing {tail}"))
}

fn prov(j: &ResolvedJob, field: &str) -> String {
    j.provenance[field].to_string()
}

#[test]
fn files_entries_set_one_file_and_lose_to_the_command_line() {
    let d = fixture("[files.\"landscape/rock01_n.png\"]\nalpha.threshold = 100\n");
    let p = plan(req(&d));
    let j = job(&p, "landscape/rock01_n.dds");
    assert_eq!(j.profile.alpha.threshold, 100);
    assert_eq!(prov(j, "alpha.threshold"), "project_file");
    assert_ne!(job(&p, "other/rock01_n.dds").profile.alpha.threshold, 100);
    let mut r = req(&d);
    r.overrides = vec![parse_set("file:**/rock01_n.png.alpha.threshold=150").unwrap()];
    let p = plan(r);
    let j = job(&p, "landscape/rock01_n.dds");
    assert_eq!(j.profile.alpha.threshold, 150);
    assert_eq!(prov(j, "alpha.threshold"), "user");
}

#[test]
fn a_files_subtype_decides_the_file() {
    let d = fixture("[files.\"landscape/rock01.png\"]\nsubtype = \"diffuse\"\n");
    let p = plan(req(&d));
    let j = job(&p, "landscape/rock01.dds");
    assert_eq!(prov(j, "subtype"), "project_file");
    assert_eq!(j.status, JobStatus::Ready);
}

#[test]
fn files_allow_acknowledges_only_that_file() {
    let d = fixture(concat!(
        "[files.\"landscape/odd.png\"]\nsubtype = \"diffuse\"\nallow = [\"DIM-NPOT\"]\n",
        "[files.\"other/odd.png\"]\nsubtype = \"diffuse\"\n"
    ));
    let p = plan(req(&d));
    assert_eq!(job(&p, "landscape/odd.dds").status, JobStatus::Ready);
    assert!(matches!(
        &job(&p, "other/odd.dds").status,
        JobStatus::Blocked { codes } if codes.contains(&Code::DimNpot)
    ));
}

#[test]
fn a_packed_output_is_keyed_without_extension() {
    let d = fixture("[files.\"landscape/rock01_rmaos\".packing.fill]\nmetallic = 255\n");
    let p = plan(req(&d));
    assert_eq!(prov(job(&p, "landscape/rock01_rmaos.dds"), "pack.g"), "fill:255");
}

#[test]
fn an_unmatched_files_key_is_reported_once_and_kept() {
    let d = fixture("target = \"cpm\"\n[files.\"landscape/rock01_rmaos\"]\nalpha.threshold = 90\n");
    let mut r = req(&d);
    r.target = None;
    let (p, ev) = plan_events(r);
    assert!(p.is_ok());
    let hits: Vec<&EngineEvent> = ev
        .iter()
        .filter(|e| matches!(&e.kind, EventKind::Warning(w) if w.code == Code::CfgFileUnmatched))
        .collect();
    assert_eq!(hits.len(), 1);
    let EventKind::Warning(w) = &hits[0].kind else { unreachable!() };
    assert_eq!(w.data["key"], "landscape/rock01_rmaos");
    // The file is not rewritten by planning.
    let text = std::fs::read_to_string(d.path().join("in/sdc.toml")).unwrap();
    assert!(text.contains("landscape/rock01_rmaos"));
}

#[test]
fn the_inline_config_replaces_the_file_on_disk() {
    let d = fixture("[files.\"landscape/rock01_n.png\"]\nalpha.threshold = 100\n");
    let mut r = req(&d);
    let mut cfg = ConfigFile::default();
    cfg.files.insert(
        "landscape/rock01_n.png".into(),
        serde_json::json!({ "alpha": { "threshold": 150 } }),
    );
    r.project_config_inline = Some(cfg);
    let p = plan(r);
    assert_eq!(job(&p, "landscape/rock01_n.dds").profile.alpha.threshold, 150);
}

#[test]
fn bad_files_entries_are_config_errors_and_user_config_files_are_rejected() {
    let config_err = |toml: &str| match plan_events(req(&fixture(toml))).0 {
        Err(EngineError::Config(m)) => m,
        other => panic!("expected a config error, got {:?}", other.map(|_| ())),
    };
    assert!(config_err("[files.\"x.png\"]\ncolour = \"srgb\"\n").contains("x.png"));
    assert!(config_err("[files.\"x.png\"]\nallow = [\"NOT-A-CODE\"]\n").contains("NOT-A-CODE"));
    let d = fixture("");
    let user = d.path().join("user.toml");
    std::fs::write(&user, "[files.\"a.png\"]\nalpha.threshold = 90\n").unwrap();
    let mut r = req(&d);
    r.user_config = Some(user);
    match plan_events(r).0 {
        Err(EngineError::Config(m)) => assert!(m.contains("[files] is only valid in a project sdc.toml")),
        other => panic!("expected a config error, got {:?}", other.map(|_| ())),
    }
}
```

`Warning::data` is the `serde_json::Value` payload set by `with_data`.

- [ ] **Step 2: Run to see them fail**

Run: `cargo test -p engine --test project_files`
Expected: FAIL (compile: no `files` field, no `project_config_inline`).

- [ ] **Step 3: `ConfigFile` and `Provenance`** in `presets.rs`:

```rust
pub struct ConfigFile {
    // … existing fields …
    /// Per-file edits (DECISIONS N2): key relative to the folder holding this `sdc.toml` → partial profile, plus
    /// `allow = [codes]`. Only a project config may hold it.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    #[schemars(schema_with = "crate::schemas::files_table_schema")]
    pub files: BTreeMap<String, Value>,
}

impl ConfigFile {
    /// The `[files]` table as `Scope::Path` overrides (key order, then leaf order) and per-file `allow` codes.
    pub fn file_entries(
        &self,
    ) -> Result<(Vec<SetOverride>, BTreeMap<String, BTreeSet<Code>>), PresetError> {
        let mut overrides = vec![];
        let mut allow = BTreeMap::new();
        for (key, entry) in &self.files {
            let bad = |m: String| PresetError::Parse(format!("[files.\"{key}\"]: {m}"));
            let mut entry = entry.clone();
            let obj = entry
                .as_object_mut()
                .ok_or_else(|| bad("expected a table".into()))?;
            if let Some(a) = obj.remove("allow") {
                let list = a.as_array().ok_or_else(|| bad("allow must be a list of codes".into()))?;
                let mut codes = BTreeSet::new();
                for c in list {
                    let s = c.as_str().unwrap_or_default();
                    codes.insert(Code::parse(s).ok_or_else(|| bad(format!("unknown code '{s}'")))?);
                }
                allow.insert(key.clone(), codes);
            }
            let mut paths = vec![];
            leaf_paths(&entry, "", &mut paths);
            for field in paths {
                if !crate::overrides::FIELD_PATHS.contains(&field.as_str()) {
                    return Err(bad(format!("unknown field '{field}'")));
                }
                let value = field
                    .split('.')
                    .fold(Some(&entry), |v, part| v.and_then(|v| v.get(part)))
                    .cloned()
                    .unwrap_or(Value::Null);
                overrides.push(SetOverride {
                    scope: Scope::Path(key.clone()),
                    field,
                    value,
                });
            }
        }
        Ok((overrides, allow))
    }
}
```

Add `ProjectFile` to `Provenance` between `Suggestion` and `User` (declaration order is precedence), `Display` → `"project_file"`, and `FromStr` `"project_file" => ProjectFile`.

- [ ] **Step 4: `api.rs`.** Add to `BatchRequest`:

```rust
    /// The project config as edited in memory (DECISIONS N4). When `Some`, it is used instead of
    /// `project_config` or the first input folder's `sdc.toml`, and `[files]` keys are relative to the first input.
    pub project_config_inline: Option<ConfigFile>,
```

(`None` in `new`). In `setup`, take `project_config` from `req.project_config_inline.clone()` when it is `Some`, else as today. Reject a user config whose `files` is not empty: `EngineError::Config("[files] is only valid in a project sdc.toml, not in <path>".into())`. Add:

```rust
/// The folder `[files]` keys are relative to (DECISIONS N2, N4).
pub fn project_root(req: &BatchRequest) -> Option<PathBuf> {
    let first_dir = || req.inputs.first().filter(|p| p.is_dir()).cloned();
    if req.project_config_inline.is_some() {
        return first_dir();
    }
    match &req.project_config {
        Some(p) => p.parent().map(Path::to_path_buf),
        None => first_dir().filter(|d| d.join("sdc.toml").exists()),
    }
}
```

Keep `Setup` holding the parsed project config. In `plan`, build `FileScopes` from it:

```rust
        let (file_overrides, mut file_allow) = match &project_config {
            Some(c) => c.file_entries().map_err(config_err)?,
            None => (vec![], BTreeMap::new()),
        };
        for (k, codes) in &req.file_allow {
            file_allow.entry(k.clone()).or_default().extend(codes.iter().copied());
        }
        let file_scopes = FileScopes {
            root: project_root(&req).or_else(|| req.inputs.first().filter(|p| p.is_dir()).cloned()),
            overrides: file_overrides,
            allow: file_allow,
        };
```

In `check_up_front`, after the `--set` loop, validate the `[files]` overrides the same way, but as config errors naming the key: `EngineError::Config(format!("[files.\"{key}\"] {}={}: {e}", o.field, o.value))`. Parse `file_entries()` there too, so a bad entry fails in `preflight`.

After `plan` has every job (just before emitting `set_resolved`), report each `[files]` key (from overrides and allow) that matched no job, skipped file or failed file:

```rust
        // DECISIONS N2: a [files] key that names nothing planned is reported once (it stays in the file).
        let mut rels: Vec<String> = vec![];
        if let Some(root) = &file_scopes.root {
            let rel_of_path = |p: &Path| -> Option<String> {
                let f = InputFile {
                    path: p.to_path_buf(),
                    rel: PathBuf::from(p.file_name()?),
                    game_path: String::new(),
                    output: PathBuf::new(),
                    kind: InputKind::Image,
                };
                project_rel(root, &f)
            };
            for j in &jobs {
                match &j.pack {
                    // A packed output: the folder of its first source plus the output's stem (the virtual stem,
                    // e.g. `rock01_rmaos`), compared with the extension-less key. Its raw sources count too.
                    Some(pk) => {
                        let stem = j.output.file_stem().map(|s| s.to_string_lossy().into_owned());
                        if let (Some(r), Some(stem)) = (rel_of_path(&j.input), stem) {
                            let dir = r.rsplit_once('/').map_or(String::new(), |(d, _)| format!("{d}/"));
                            rels.push(format!("{dir}{stem}"));
                        }
                        rels.extend(pk.sources.iter().filter_map(|s| rel_of_path(&s.path)));
                    }
                    None => rels.extend(rel_of_path(&j.input)),
                }
            }
            rels.extend(skipped.iter().filter_map(|s| rel_of_path(&s.path)));
            rels.extend(failed.iter().filter_map(|f| rel_of_path(&f.path)));
        }
        let mut keys: BTreeSet<&String> = file_scopes.allow.keys().collect();
        keys.extend(file_scopes.overrides.iter().filter_map(|o| match &o.scope {
            Scope::Path(k) => Some(k),
            _ => None,
        }));
        for key in keys {
            if !rels.iter().any(|r| path_key_matches(key, r)) {
                rep.emit(
                    None,
                    None,
                    EventKind::Warning(
                        Warning::new(Code::CfgFileUnmatched, format!("[files] entry '{key}' matches no file"))
                            .with_data(serde_json::json!({ "key": key })),
                    ),
                );
            }
        }
```

- [ ] **Step 5: `policy.rs`.** In `layered_profile_with`'s override loop and in `decide_subtype`, the provenance of an override from `ctx.files.overrides` is `Provenance::ProjectFile`, else `Provenance::User`. Do this by comparing scope: `Scope::Path(_) => Provenance::ProjectFile`. In `resolve_file`, extend `matches!(d.provenance, Provenance::User | Provenance::Rule(_))` with `| Provenance::ProjectFile`.

- [ ] **Step 6: Code and schemas.** Add to `codes!` in `codes.rs`: `CfgFileUnmatched = "CFG-FILE-UNMATCHED" => Info,` and mention it in the module comment's list of added codes. In `schemas.rs` add:

```rust
/// A partial `Profile`: the `Profile` schema with every `required` list removed, recursively (plan M3 carried
/// "partial-Profile schema").
pub fn partial_profile_schema(gen: &mut schemars::SchemaGenerator) -> schemars::Schema {
    let mut s = gen.subschema_for::<crate::config::Profile>();
    fn strip(v: &mut serde_json::Value) {
        match v {
            serde_json::Value::Object(m) => {
                m.remove("required");
                m.values_mut().for_each(strip);
            }
            serde_json::Value::Array(a) => a.iter_mut().for_each(strip),
            _ => {}
        }
    }
    strip(s.as_value_mut());
    s
}

/// `[files]`: key → partial profile plus `allow`.
pub fn files_table_schema(gen: &mut schemars::SchemaGenerator) -> schemars::Schema {
    let entry = partial_profile_schema(gen);
    schemars::json_schema!({
        "type": "object",
        "additionalProperties": {
            "allOf": [entry],
            "properties": { "allow": { "type": "array", "items": { "type": "string" } } }
        }
    })
}
```

Use `#[schemars(schema_with = "crate::schemas::partial_profile_schema")]` on `ConfigFile::defaults` and `PresetFile::defaults`, and a map of it for `subtypes` and `Rule::overrides` (write `subtypes_schema` the same way: `{"type":"object","additionalProperties": <partial>}`). If `subschema_for` returns a `$ref` (it will for a named type), resolve it: call `gen.definitions()` to fetch the referenced schema, clone it, strip it, and return the stripped copy inline. Regenerate: `cargo run -q -p sdc -- presets schema all --out schema`.

Add to main spec §10's code table a row: `CFG-FILE-UNMATCHED | Info | A [files] key in sdc.toml matches no file (M3a, DECISIONS N2)`.

- [ ] **Step 7: Run the tests**

Run: `cargo test -p engine --test project_files` then `cargo test --workspace`
Expected: PASS (schemas test green after regeneration).

- [ ] **Step 8: Commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings
git add crates/engine/src/presets.rs crates/engine/src/codes.rs crates/engine/src/api.rs crates/engine/src/policy.rs crates/engine/src/schemas.rs crates/engine/tests/project_files.rs schema/ docs/superpowers/specs/2026-10-08-skyrim-dds-converter-design.md
git commit -m "engine: [files] table in sdc.toml, inline project config, partial-profile schema (M3a)"
```

---

### Task 4: Typed `file_planned.job` and unresolved needs (event schema 2)

**Files:**
- Modify: `crates/engine/src/events.rs` (`EVENT_SCHEMA = 2`, `FilePlanned { job: Box<ResolvedJob> }`)
- Modify: `crates/engine/src/policy.rs` (`Deserialize` on `ResolvedJob`, `JobStatus`, `AliasUse`; `Need`; `JobStatus::Unresolved::needs`)
- Modify: `crates/engine/src/presets.rs` (`Deserialize for Provenance` through `FromStr`)
- Modify: `crates/engine/src/suggest.rs` (`Deserialize` on `Suggestion` and `Confidence` if missing), `crates/engine/src/pack/mod.rs` (`Deserialize` on `PackJob`, `PackSource`, `ChannelSource`, `RecipeKind`, `GraySource` if missing)
- Modify: `crates/engine/src/api.rs:871-886` (emit the typed job)
- Modify: `crates/cli/src/verbs.rs:175-216` (`print_planned` takes `&ResolvedJob`)
- Modify: tests that read `FilePlanned { job }` as a `Value`: `crates/engine/tests/{golden,packing,pack_plan,events_manifest}.rs` (convert with `serde_json::to_value(&**job).unwrap()` where a `Value` is needed)
- Modify: `schema/event.schema.json` (regenerated)
- Test: `crates/engine/tests/typed_events.rs` (new)

**Interfaces:**
- Produces: `EventKind::FilePlanned { job: Box<ResolvedJob> }`; `ResolvedJob: Deserialize + PartialEq`; `#[serde(rename_all = "snake_case")] pub enum Need { Subtype, AlphaMeaning, GreenConvention }`; `JobStatus::Unresolved { reasons: Vec<String>, #[serde(default)] needs: Vec<Need> }`.

- [ ] **Step 1: Write the failing tests** in `crates/engine/tests/typed_events.rs`:

```rust
//! M3a Task 4: `file_planned.job` is a typed `ResolvedJob` (event schema 2) and unresolved jobs name their needs.
use engine::api::*;
use engine::config::Target;
use engine::events::*;
use engine::overrides::parse_set;
use engine::policy::{JobStatus, Need, ResolvedJob};
use image::{ImageBuffer, Rgb, Rgba};
use std::path::Path;
use std::sync::Arc;

fn fixture(dir: &Path) {
    let r = dir.join("in");
    std::fs::create_dir_all(&r).unwrap();
    ImageBuffer::from_fn(64, 64, |x, y| Rgb([x as u8 * 4, y as u8 * 4, 90]))
        .save(r.join("iron.png"))
        .unwrap();
    // Varying alpha: a diffuse with a mesh-dependent alpha meaning.
    ImageBuffer::from_fn(64, 64, |x, _| Rgba([200u8, 30, 30, if x < 32 { 255 } else { 0 }]))
        .save(r.join("leaf.png"))
        .unwrap();
    // `_normal` is a vocabulary term with no stated green convention.
    ImageBuffer::from_fn(64, 64, |_, _| Rgb([128u8, 128, 255]))
        .save(r.join("foo_normal.png"))
        .unwrap();
}

fn events(dir: &Path, overrides: &[&str]) -> Vec<EngineEvent> {
    let mut req = BatchRequest::new(vec![dir.join("in")], dir.join("out"), Target::Cs);
    req.overrides = overrides.iter().map(|s| parse_set(s).unwrap()).collect();
    let sink = Arc::new(VecSink::default());
    let rep = Reporter::new(sink.clone(), "t");
    Engine::new().unwrap().plan(req, &rep, &CancelToken::default()).unwrap();
    sink.take()
}

fn planned(ev: &[EngineEvent], name: &str) -> ResolvedJob {
    ev.iter()
        .find_map(|e| match &e.kind {
            EventKind::FilePlanned { job } if job.input.ends_with(name) => Some((**job).clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no file_planned for {name}"))
}

fn needs(j: &ResolvedJob) -> Vec<Need> {
    match &j.status {
        JobStatus::Unresolved { needs, .. } => needs.clone(),
        _ => vec![],
    }
}

#[test]
fn file_planned_round_trips_as_a_typed_job() {
    let d = tempfile::tempdir().unwrap();
    fixture(d.path());
    let ev = events(d.path(), &[]);
    let mut seen = 0;
    for e in ev.iter().filter(|e| matches!(e.kind, EventKind::FilePlanned { .. })) {
        assert_eq!(e.schema, 2);
        let text = serde_json::to_string(e).unwrap();
        let back: EngineEvent = serde_json::from_str(&text).unwrap();
        assert_eq!(&back, e);
        seen += 1;
    }
    assert_eq!(seen, 3);
}

#[test]
fn unresolved_jobs_name_what_they_need() {
    let d = tempfile::tempdir().unwrap();
    fixture(d.path());
    let ev = events(d.path(), &[]);
    assert!(needs(&planned(&ev, "leaf.png")).contains(&Need::Subtype));
    assert!(needs(&planned(&ev, "foo_normal.png")).contains(&Need::GreenConvention));
    let ev = events(d.path(), &["file:leaf.png.subtype=diffuse"]);
    assert_eq!(needs(&planned(&ev, "leaf.png")), vec![Need::AlphaMeaning]);
}
```

- [ ] **Step 2: Run to see them fail**

Run: `cargo test -p engine --test typed_events`
Expected: compile errors (`Need`, typed `job`).

- [ ] **Step 3: Implement.** In `policy.rs`:

```rust
/// What an unresolved job is waiting for (DECISIONS N9), so a GUI can offer the right control.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Need {
    /// The sub-type is a suggestion nobody confirmed.
    Subtype,
    /// The alpha meaning (policy) depends on the mesh and nobody chose it.
    AlphaMeaning,
    /// A plain `_normal`/`_nrm` source with no stated green convention.
    GreenConvention,
}
```

In `JobStatus::Unresolved` add `#[serde(default)] needs: Vec<Need>`. In `resolve_decided` push `Need::Subtype` with the `!d.decided` reason, `Need::AlphaMeaning` with the alpha reason, `Need::GreenConvention` with `green_reason`, and build `JobStatus::Unresolved { reasons, needs }`. Fix every other constructor/pattern of `Unresolved` (`grep -rn "Unresolved" crates`).

Derive `serde::Deserialize` on `ResolvedJob`, `JobStatus`, `AliasUse` and every type they contain that lacks it, and `PartialEq` on `ResolvedJob` (`EventKind` derives `PartialEq`, so the boxed job needs it). `Provenance` already has `FromStr` and a string `Serialize`; add:

```rust
impl<'de> serde::Deserialize<'de> for Provenance {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}
```

`JobStatus` uses `#[serde(tag = "state")]`; internally tagged enums deserialise fine. In `events.rs` set `pub const EVENT_SCHEMA: u32 = 2;` and `FilePlanned { job: Box<ResolvedJob> }` (import `crate::policy::ResolvedJob`). In `api.rs` emit `EventKind::FilePlanned { job: Box::new(job.clone()) }`.

In `crates/cli/src/verbs.rs` change `print_planned(job: &Value)` to `print_planned(job: &ResolvedJob)` printing the same lines from typed fields (`job.input.display()`, `job.suggestions.first()`, `job.profile.subtype.id()`, format via `serde_json::to_value(job.profile.format)`, `job.profile.color.tag`, `job.profile.alpha.policy`, `job.output.display()`, the status state and its `codes`/`reasons`, and each warning). The CLI's `inspect` output text must not change: `crates/cli/tests/cli_inspect_validate.rs` stays green unchanged.

- [ ] **Step 4: Regenerate schemas and run everything**

Run: `cargo run -q -p sdc -- presets schema all --out schema` then `cargo test --workspace`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings
git add crates/engine/src crates/engine/tests crates/cli/src/verbs.rs schema/
git commit -m "engine: typed file_planned job and unresolved needs; event schema 2 (M3a)"
```

(List the changed files by name in `git add`; the directory form above is shorthand for "the files you changed there".)

---

### Task 5: Job keys, skip kinds and pack-source events

**Files:**
- Modify: `crates/engine/src/events.rs` (`SkipKind`, `FileSkipped::kind`, `EngineEvent::job`, `Reporter::emit_job`)
- Modify: `crates/engine/src/api.rs` (`SkippedFile::output`, `FailedFile::output`; job keys on every per-file event; skip kinds; pack-source events)
- Modify: `crates/engine/src/pack/plan.rs:371` (the `PACK-CONFLICT` `SkippedFile` gets its `output`)
- Modify: `crates/engine/src/pipeline.rs` (progress and `IN-NONFINITE` carry the job key)
- Modify: `crates/cli/src/human.rs:33`, `crates/cli/src/verbs.rs:217,368` (patterns gain `..`; the human log prints nothing for `packed_source`)
- Modify: `schema/event.schema.json` (regenerated; schema stays 2)
- Test: `crates/engine/tests/job_keys.rs` (new)

**Interfaces:**
- Consumes: Task 4's schema 2.
- Produces:
  - `#[serde(rename_all = "snake_case")] pub enum SkipKind { UpToDate, OnExistingSkip, Unresolved, Blocked, Duplicate, DdsInput, PackingInput, UnknownExisting, PackConflict, PackedSource }`
  - `EventKind::FileSkipped { kind: SkipKind, code: Option<Code>, reason: String }`
  - `EngineEvent::job: Option<String>` (`#[serde(default, skip_serializing_if = "Option::is_none")]`) — `manifest::output_key(output_root, output)`.
  - `Reporter::emit_job(&self, job: Option<&str>, set: Option<&str>, path: Option<&Path>, kind: EventKind)`; `emit` calls it with `None`.
  - `SkippedFile::output: PathBuf`, `FailedFile::output: PathBuf`.

- [ ] **Step 1: Write the failing tests** in `crates/engine/tests/job_keys.rs`:

```rust
//! M3a Task 5: every per-file event carries its job key; skips carry a kind; pack sources get a terminal event.
use engine::api::*;
use engine::config::Target;
use engine::events::*;
use engine::overrides::parse_set;
use image::{ImageBuffer, Luma, Rgb};
use std::path::Path;
use std::sync::Arc;

fn fixture(dir: &Path) {
    let r = dir.join("in");
    std::fs::create_dir_all(&r).unwrap();
    ImageBuffer::from_fn(64, 64, |x, y| Rgb([x as u8 * 4, y as u8 * 4, 90]))
        .save(r.join("iron.png"))
        .unwrap();
    for (name, v) in [("iron_roughness.png", 120u8), ("iron_ao.png", 200)] {
        ImageBuffer::from_fn(64, 64, |x, _| Luma([v.wrapping_add(x as u8)]))
            .save(r.join(name))
            .unwrap();
    }
}

fn convert(dir: &Path) -> Vec<EngineEvent> {
    let mut req = BatchRequest::new(vec![dir.join("in")], dir.join("out"), Target::Cs);
    req.overrides = vec![parse_set("file:iron.png.subtype=diffuse").unwrap()];
    let sink = Arc::new(VecSink::default());
    let rep = Reporter::new(sink.clone(), "t");
    let e = Engine::new().unwrap();
    let plan = e.plan(req, &rep, &CancelToken::default()).unwrap();
    e.run(plan, &rep, &CancelToken::default()).unwrap();
    sink.take()
}

fn kind(e: &EngineEvent) -> String {
    serde_json::to_value(&e.kind).unwrap()["kind"].as_str().unwrap().to_string()
}

#[test]
fn every_per_file_event_carries_its_job_key() {
    let d = tempfile::tempdir().unwrap();
    fixture(d.path());
    let ev = convert(d.path());
    let per_file = [
        "file_discovered", "file_planned", "file_started", "progress", "file_done", "file_skipped", "file_failed",
    ];
    for e in ev.iter().filter(|e| per_file.contains(&kind(e).as_str())) {
        assert!(e.job.is_some(), "{} without a job key: {:?}", kind(e), e.path);
    }
    let done: Vec<&str> = ev
        .iter()
        .filter(|e| kind(e) == "file_done")
        .map(|e| e.job.as_deref().unwrap())
        .collect();
    assert!(done.contains(&"iron.dds"));
    assert!(done.contains(&"iron_rmaos.dds"));
}

#[test]
fn skips_carry_their_kind_and_pack_sources_get_a_terminal_event() {
    let d = tempfile::tempdir().unwrap();
    fixture(d.path());
    let ev = convert(d.path());
    let packed: Vec<(&str, &str)> = ev
        .iter()
        .filter_map(|e| match &e.kind {
            EventKind::FileSkipped { kind: SkipKind::PackedSource, .. } => {
                Some((e.path.as_deref().unwrap(), e.job.as_deref().unwrap()))
            }
            _ => None,
        })
        .collect();
    assert_eq!(packed.len(), 2, "{packed:?}");
    assert!(packed.iter().all(|(_, job)| *job == "iron_rmaos.dds"));
    assert!(packed.iter().any(|(p, _)| p.ends_with("iron_roughness.png")));
    assert!(packed.iter().any(|(p, _)| p.ends_with("iron_ao.png")));
    // A second run: both outputs are up to date; a copied DDS input is a dds_input skip.
    std::fs::copy(d.path().join("out/iron.dds"), d.path().join("in/extra.dds")).unwrap();
    let ev = convert(d.path());
    let kinds: Vec<SkipKind> = ev
        .iter()
        .filter_map(|e| match &e.kind {
            EventKind::FileSkipped { kind, .. } => Some(*kind),
            _ => None,
        })
        .collect();
    assert_eq!(kinds.iter().filter(|k| **k == SkipKind::UpToDate).count(), 2);
    assert!(kinds.contains(&SkipKind::DdsInput));
}
```

- [ ] **Step 2: Run to see them fail**

Run: `cargo test -p engine --test job_keys`
Expected: compile errors (`SkipKind`, `job`).

- [ ] **Step 3: Implement `events.rs`**

```rust
/// Why a file produced no output in this batch (DECISIONS N5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SkipKind {
    UpToDate,
    /// `--on-existing skip` kept an existing output.
    OnExistingSkip,
    /// A sub-type, alpha meaning or green convention still needs a decision.
    Unresolved,
    /// A blocking code was not acknowledged.
    Blocked,
    /// `IN-DUPLICATE`: two inputs map to one output.
    Duplicate,
    /// `IN-DDS-SKIPPED`.
    DdsInput,
    /// A raw map not packed (`IN-PACKING-INPUT`, `PACK-UNUSED-INPUT`).
    PackingInput,
    /// `OUT-UNKNOWN-EXISTING`.
    UnknownExisting,
    /// `PACK-CONFLICT`: a pre-packed file replaced under `packing.prefer = raw`.
    PackConflict,
    /// A raw map consumed by a recipe; `job` names the packed output.
    PackedSource,
}
```

`FileSkipped { kind: SkipKind, code: Option<Code>, reason: String }`. On `EngineEvent` add, after `path`:

```rust
    /// The job this event belongs to: its output path relative to the output root (DECISIONS N6). Absent on
    /// batch-level events.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job: Option<String>,
```

`Reporter::emit_job` builds the event with `job: job.map(str::to_string)`; `emit` becomes `self.emit_job(None, set, path, kind)`.

- [ ] **Step 4: Implement `api.rs` and `pipeline.rs`.** Add `pub output: PathBuf` to `SkippedFile` and `FailedFile` and fill it everywhere they are built (`files[i].output`, `item.1.output`, the recipe's `c.virtual_input.output` for pass-2 failures, the pre-packed file's output in `pack/plan.rs`). Add a local helper `let key = |out: &Path| output_key(&req.output_root, out);` and switch every per-file emit to `emit_job(Some(&key(..)), …)`:

| Event | Key from |
|---|---|
| `file_discovered` | `f.output` |
| `file_planned` and its warnings | `job.output` |
| pass-1 and pass-2 load-failure warnings, `file_failed` of `plan.failed` | the failed file's `output` |
| `file_skipped` of `plan.skipped` | `s.output` |
| `file_skipped` / `file_failed` of job statuses, `--on-existing`, up to date, `OUT-UNKNOWN-EXISTING` | `job.output` |
| `file_started`, `progress`, `warning`, `file_done`, `file_failed` in `run_one` | `job.output` |
| `progress` and `IN-NONFINITE` in `process_file` | `output_key(&req.output_root, &job.output)` |

Skip kinds: `plan.skipped` maps its code (`InDuplicate` → `Duplicate`, `InDdsSkipped` → `DdsInput`, `PackConflict` → `PackConflict`, anything else → `Duplicate`); `Unresolved` → `Unresolved`; `PackingInput` → `PackingInput`; `Blocked` → `Blocked`; `--on-existing skip` → `OnExistingSkip`; `OUT-UNKNOWN-EXISTING` → `UnknownExisting`; up to date → `UpToDate`.

Pack-source events: in `run`, while walking `plan.jobs` (before the status match), for every job with `pack: Some(pk)` emit for each source:

```rust
            if let Some(pk) = &job.pack {
                for s in &pk.sources {
                    rep.emit_job(
                        Some(&key(&job.output)),
                        Some(&job.set_key),
                        Some(&s.path),
                        EventKind::FileSkipped {
                            kind: SkipKind::PackedSource,
                            code: None,
                            reason: format!("packed into {}", slash(&job.output)),
                        },
                    );
                }
            }
```

These are not counted in `BatchSummary` (like packing inputs).

- [ ] **Step 5: CLI.** Add `..` to the `FileSkipped` patterns in `crates/cli/src/human.rs` and `verbs.rs`. **Both printers** ignore `FileSkipped { kind: SkipKind::PackedSource, .. }`: `HumanSink::emit` returns `None` for it, and `verbs.rs`'s `print_event` (line ~217) and the other `FileSkipped` printer (line ~368) skip it, so neither the human log nor `inspect` output changes. `cargo test -p sdc` must pass unchanged.

- [ ] **Step 6: Regenerate and run**

Run: `cargo run -q -p sdc -- presets schema all --out schema` then `cargo test --workspace`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings
git add crates/engine/src/events.rs crates/engine/src/api.rs crates/engine/src/pack/plan.rs crates/engine/src/pipeline.rs crates/cli/src/human.rs crates/cli/src/verbs.rs crates/engine/tests/job_keys.rs schema/event.schema.json
git commit -m "engine: job keys, skip kinds and pack-source events (M3a, event schema 2)"
```

---

### Task 6: Manifest sources record role and invert

**Files:**
- Modify: `crates/engine/src/manifest.rs:19` (`MANIFEST_SCHEMA = 2`), `:110-126` (`SourceRef::role`, `invert`, `of_pack`)
- Modify: `crates/engine/src/pack/mod.rs` (`invert_of`)
- Modify: `crates/engine/src/pipeline.rs:171-180` (packed sources use `SourceRef::of_pack`)
- Modify: `crates/engine/src/api.rs:385-433` (`pack_provenance_matches` replaced by `pack_roles_match`)
- Modify: `schema/manifest.schema.json` (regenerated)
- Test: `crates/engine/tests/pack_run.rs`

**Interfaces:**
- Produces: `SourceRef { path, sha256, role: Option<RawMap>, invert: bool }`; `SourceRef::of_pack(path: &Path, role: RawMap, invert: bool) -> io::Result<SourceRef>`; `pub fn pack::invert_of(pk: &PackJob, path: &Path) -> bool` (true when the channel this source feeds is stored inverted).

- [ ] **Step 1: Write the failing test** in `crates/engine/tests/pack_run.rs` (it reuses that file's `gray` helper, which writes under `dir/in/`):

```rust
/// DECISIONS N7: a schema-1 record has no source roles, so its packed output re-encodes once, then is up to date.
#[test]
fn a_schema_1_record_re_encodes_its_packed_output_once() {
    let d = tempfile::tempdir().unwrap();
    gray(d.path(), "iron_roughness.png", 64, |x, _| x as u8 * 3);
    gray(d.path(), "iron_ao.png", 64, |_, y| 255 - y as u8);
    let run = || {
        let req = BatchRequest::new(vec![d.path().join("in")], d.path().join("out"), Target::Cs);
        let rep = Reporter::new(Arc::new(NullSink), "t");
        let e = Engine::new().unwrap();
        let plan = e.plan(req, &rep, &CancelToken::default()).unwrap();
        e.run(plan, &rep, &CancelToken::default()).unwrap()
    };
    assert_eq!(run().converted, 1);
    let path = Manifest::at(&d.path().join("out")).path().to_path_buf();
    let text = std::fs::read_to_string(&path).unwrap();
    let last: serde_json::Value = serde_json::from_str(text.lines().last().unwrap()).unwrap();
    assert_eq!(last["sources"][0]["role"], "roughness");
    assert_eq!(last["sources"][1]["role"], "ao");
    // Rewrite every record as schema 1 without roles.
    let mut out = String::new();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let mut v: serde_json::Value = serde_json::from_str(line).unwrap();
        v["schema"] = 1.into();
        for s in v["sources"].as_array_mut().unwrap() {
            let s = s.as_object_mut().unwrap();
            s.remove("role");
            s.remove("invert");
        }
        out.push_str(&serde_json::to_string(&v).unwrap());
        out.push('\n');
    }
    std::fs::write(&path, out).unwrap();
    let s = run();
    assert_eq!((s.converted, s.up_to_date), (1, 0));
    let s = run();
    assert_eq!((s.converted, s.up_to_date), (0, 1));
}
```

(If the recipe orders its sources differently, assert the order the recipe uses and say so; the roles must be present.)

- [ ] **Step 2: Run to see it fail**

Run: `cargo test -p engine --test pack_run a_schema_1`
Expected: FAIL (no `role` in the record).

- [ ] **Step 3: Implement.** In `manifest.rs`:

```rust
pub const MANIFEST_SCHEMA: u32 = 2;

pub struct SourceRef {
    pub path: String,
    pub sha256: String,
    /// The raw-map role this source fills in a packed output (DECISIONS N7). Absent in schema-1 records.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<RawMap>,
    /// The source is stored inverted (a gloss map in a roughness channel).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub invert: bool,
}

impl SourceRef {
    /// `path` ('/'-separated, as the record's `source`) and its SHA-256; no role.
    pub fn of(path: &Path) -> io::Result<SourceRef> {
        Ok(SourceRef {
            path: path.to_string_lossy().replace('\\', "/"),
            sha256: sha256_file(path)?,
            role: None,
            invert: false,
        })
    }
    /// A packed output's source with the role it fills (DECISIONS N7).
    pub fn of_pack(path: &Path, role: RawMap, invert: bool) -> io::Result<SourceRef> {
        Ok(SourceRef { role: Some(role), invert, ..SourceRef::of(path)? })
    }
}
```

In `pack/mod.rs`:

```rust
/// Whether the channel `path` feeds is stored inverted (DECISIONS N7).
pub fn invert_of(pk: &PackJob, path: &Path) -> bool {
    pk.channels
        .iter()
        .any(|c| matches!(c, ChannelSource::Source { path: p, invert: true, .. } if p == path))
}
```

In `pipeline.rs` build packed sources with `SourceRef::of_pack(&s.path, s.role, invert_of(pk, &s.path))`. In `api.rs` replace `pack_provenance_matches` with:

```rust
/// A packed job's sources fill the same roles, inverted the same way, as the record's (DECISIONS N7). A schema-1
/// record has no roles, so it never matches and the output re-encodes once.
fn pack_roles_match(e: &ManifestEntry, job: &ResolvedJob) -> bool {
    match &job.pack {
        None => true,
        Some(pk) => e
            .sources
            .iter()
            .map(|r| (r.role, r.invert))
            .eq(pk.sources.iter().map(|s| (Some(s.role), invert_of(pk, &s.path)))),
    }
}
```

and use it in `is_up_to_date`. Fix the `SourceRef { path, sha256 }` literals (`grep -rn "SourceRef {" crates`). The existing `if_changed_follows_a_source_renamed_to_another_role` test (`packing.rs`) must still pass.

- [ ] **Step 4: Regenerate and run**

Run: `cargo run -q -p sdc -- presets schema all --out schema` then `cargo test --workspace`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings
git add crates/engine/src/manifest.rs crates/engine/src/pack/mod.rs crates/engine/src/pipeline.rs crates/engine/src/api.rs crates/engine/tests/pack_run.rs schema/manifest.schema.json
git commit -m "engine: manifest sources record role and invert; manifest schema 2 (M3a)"
```

---

### Task 7: Analysis cache, thumbnails and `Session`

**Files:**
- Create: `crates/engine/src/session.rs`
- Modify: `crates/engine/src/lib.rs` (`pub mod session;`)
- Modify: `crates/engine/src/api.rs:533-898` (`plan` → `plan_cached`; cache hits in pass 1 and pass 2)
- Modify: `crates/engine/src/analyse.rs` (`code8` public if it is not)
- Test: `crates/engine/tests/session.rs` (new)

**Interfaces:**
- Produces (`engine::session`):
  - `pub struct Thumbnail { pub width: u32, pub height: u32, pub rgba: Vec<u8> }` (RGBA8, straight alpha, display-ready) and `pub const THUMB_MAX: u32 = 256;`
  - `pub fn make_thumbnail(img: &Rgba32F, info: &SourceInfo) -> Thumbnail`
  - `#[derive(Default)] pub struct AnalysisCache` with `pub thumbnails: bool` (false by default, so the throwaway cache `Engine::plan` uses never builds thumbnails; `Session::new` sets it true), `pub fn loads(&self) -> u64`, `pub fn thumbnail(&self, job: &ResolvedJob) -> Option<Thumbnail>`, `pub fn thumbnail_of(&self, path: &Path) -> Option<Thumbnail>`.
  - `pub fn pack_key(pk: &PackJob) -> String`
  - `pub struct Session` with `new(bc1_model: Bc1Model) -> Result<Session, EngineError>`, `bc1_model()`, `engine()`, `cache()`, `plan(&self, req: BatchRequest, rep: &Reporter, cancel: &CancelToken) -> Result<Plan, EngineError>`, `run(&self, plan: Plan, rep: &Reporter, cancel: &CancelToken) -> Result<BatchSummary, EngineError>`, `thumbnail(&self, job: &ResolvedJob) -> Option<Thumbnail>`. `Session: Send + Sync`.
- Produces (`engine::api`): `Engine::plan_cached(&self, req: BatchRequest, rep: &Reporter, cancel: &CancelToken, cache: &AnalysisCache) -> Result<Plan, EngineError>`; `Engine::plan` calls it with a fresh cache.

- [ ] **Step 1: Write the failing tests** in `crates/engine/tests/session.rs`:

```rust
//! M3a Task 7: the session cache re-resolves without reloading (spec §4.4) and makes thumbnails.
use encoders_sys::Bc1Model;
use engine::api::*;
use engine::config::Target;
use engine::events::*;
use engine::image::{Rgba32F, SourceInfo};
use engine::overrides::parse_set;
use engine::session::*;
use image::{ImageBuffer, Luma, Rgb};
use std::path::Path;
use std::sync::Arc;

fn fixture(dir: &Path) {
    let r = dir.join("in");
    std::fs::create_dir_all(&r).unwrap();
    ImageBuffer::from_fn(64, 64, |x, y| Rgb([x as u8 * 4, y as u8 * 4, 90]))
        .save(r.join("iron.png"))
        .unwrap();
    for (name, v) in [("iron_roughness.png", 120u8), ("iron_ao.png", 200)] {
        ImageBuffer::from_fn(64, 64, |x, _| Luma([v.wrapping_add(x as u8)]))
            .save(r.join(name))
            .unwrap();
    }
}

fn req(dir: &Path, target: Target) -> BatchRequest {
    let mut r = BatchRequest::new(vec![dir.join("in")], dir.join("out"), target);
    r.overrides = vec![parse_set("file:iron.png.subtype=diffuse").unwrap()];
    r
}

fn plan(s: &Session, r: BatchRequest) -> Plan {
    s.plan(r, &Reporter::new(Arc::new(NullSink), "t"), &CancelToken::default())
        .unwrap()
}

#[test]
fn replanning_reuses_analyses_and_a_target_change_reloads_only_pack_sources() {
    let d = tempfile::tempdir().unwrap();
    fixture(d.path());
    let s = Session::new(Bc1Model::Ideal).unwrap();
    plan(&s, req(d.path(), Target::Cs));
    // Pass 1 loads the three files; pass 2 assembles _rmaos from its two sources.
    assert_eq!(s.cache().loads(), 5);
    plan(&s, req(d.path(), Target::Cs));
    assert_eq!(s.cache().loads(), 5, "an identical re-plan loads nothing");
    plan(&s, req(d.path(), Target::Cpm));
    assert_eq!(s.cache().loads(), 7, "cpm packs a complex _m: only its two sources load");
}

#[test]
fn a_changed_file_reloads_once() {
    let d = tempfile::tempdir().unwrap();
    fixture(d.path());
    let s = Session::new(Bc1Model::Ideal).unwrap();
    plan(&s, req(d.path(), Target::Cs));
    let before = s.cache().loads();
    // A different size is a different stamp, whatever the timestamp resolution.
    ImageBuffer::from_fn(32, 32, |_, _| Rgb([1u8, 2, 3]))
        .save(d.path().join("in/iron.png"))
        .unwrap();
    let p = plan(&s, req(d.path(), Target::Cs));
    assert_eq!(s.cache().loads(), before + 1);
    let iron = p.jobs.iter().find(|j| j.input.ends_with("iron.png")).unwrap();
    assert_eq!(s.thumbnail(iron).unwrap().width, 32);
}

/// `pack_key` must change when a pack source changes, or the packed analysis would be stale.
#[test]
fn a_changed_pack_source_reassembles_its_recipe() {
    let d = tempfile::tempdir().unwrap();
    fixture(d.path());
    let s = Session::new(Bc1Model::Ideal).unwrap();
    let p = plan(&s, req(d.path(), Target::Cs));
    let stamp = |p: &Plan| {
        p.jobs.iter().find_map(|j| j.pack.as_ref()).unwrap().sources[0].stamp
    };
    let before_stamp = stamp(&p);
    let before = s.cache().loads();
    ImageBuffer::from_fn(64, 64, |x, y| Luma([((x + y) % 256) as u8]))
        .save(d.path().join("in/iron_roughness.png"))
        .unwrap();
    // A different byte size makes a different stamp whatever the timestamp resolution; if the PNG encoder
    // happens to produce the same size, write 32x32 sources for both maps instead.
    let p = plan(&s, req(d.path(), Target::Cs));
    assert_eq!(s.cache().loads(), before + 1 + 2, "pass 1 reloads the map, pass 2 reassembles from both sources");
    assert_ne!(stamp(&p), before_stamp);
}

#[test]
fn engine_plan_builds_no_thumbnails() {
    let cache = AnalysisCache::default();
    assert!(!cache.thumbnails);
}

#[test]
fn thumbnails_of_float_and_16_bit_sources_are_visible() {
    // Float: linear radiance 4.0 tone-maps below white and well above black.
    let img = Rgba32F::new(512, 256, [4.0, 0.5, 0.05, 1.0]);
    let info = SourceInfo { bit_depth: 32, channels: 4, has_alpha: false, is_float: true };
    let t = make_thumbnail(&img, &info);
    assert_eq!((t.width, t.height), (256, 128));
    let px = &t.rgba[..4];
    assert!(px[0] > 150 && px[0] < 255, "{px:?}");
    assert!(px[2] > 0, "{px:?}");
    // 16-bit PNG through the planner: mid-grey stays mid-grey.
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(d.path().join("in")).unwrap();
    ImageBuffer::from_fn(64, 64, |_, _| Rgb([30000u16, 30000, 30000]))
        .save(d.path().join("in/deep.png"))
        .unwrap();
    let s = Session::new(Bc1Model::Ideal).unwrap();
    let mut r = BatchRequest::new(vec![d.path().join("in")], d.path().join("out"), Target::Cs);
    r.overrides = vec![parse_set("file:deep.png.subtype=diffuse").unwrap()];
    let p = plan(&s, r);
    let t = s.thumbnail(&p.jobs[0]).unwrap();
    assert!((100..=140).contains(&t.rgba[0]), "{:?}", &t.rgba[..4]);
}
```

(`encoders_sys` must be a dev-dependency of `engine` for this import; add `encoders-sys.workspace = true` under `[dev-dependencies]` if `Bc1Model` is not re-exported by `engine`.)

- [ ] **Step 2: Run to see them fail**

Run: `cargo test -p engine --test session`
Expected: compile errors (`engine::session`).

- [ ] **Step 3: Implement `session.rs`**

```rust
//! A GUI session over the engine (M3a spec §4.4, §4.5): an `Engine`, the encoders initialised once with the
//! session's BC1 model, and an `AnalysisCache` so re-planning after an edit or a target change reloads only what
//! changed.
use crate::analyse::{code8, Analysis};
use crate::api::{BatchRequest, BatchSummary, CancelToken, Engine, EngineError, Plan};
use crate::events::Reporter;
use crate::image::{Rgba32F, SourceInfo};
use crate::manifest::sha256_json;
use crate::pack::PackJob;
use crate::policy::{ResolvedJob, SourceStamp};
use encoders_sys::Bc1Model;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

/// Largest side of a thumbnail.
pub const THUMB_MAX: u32 = 256;

/// A display-ready RGBA8 thumbnail (straight alpha).
#[derive(Debug, Clone, PartialEq)]
pub struct Thumbnail {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

fn linear_to_srgb(v: f32) -> f32 {
    if v <= 0.003_130_8 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

/// Box-downscale `img` so its larger side is at most `THUMB_MAX`. A float source holds linear radiance, so its
/// RGB is tone-mapped (`x / (1 + x)`) and sRGB-encoded; any other source is shown as stored, clamped to [0, 1].
pub fn make_thumbnail(img: &Rgba32F, info: &SourceInfo) -> Thumbnail {
    let step = img.width.max(img.height).div_ceil(THUMB_MAX).max(1);
    let (w, h) = (img.width.div_ceil(step), img.height.div_ceil(step));
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for ty in 0..h {
        for tx in 0..w {
            let mut acc = [0f32; 4];
            let mut n = 0f32;
            for y in ty * step..((ty + 1) * step).min(img.height) {
                for x in tx * step..((tx + 1) * step).min(img.width) {
                    let p = img.px[(y * img.width + x) as usize];
                    for c in 0..4 {
                        acc[c] += if p[c].is_finite() { p[c] } else { 0.0 };
                    }
                    n += 1.0;
                }
            }
            for (c, a) in acc.iter().enumerate() {
                let v = a / n;
                let v = if info.is_float && c < 3 {
                    linear_to_srgb(v.max(0.0) / (1.0 + v.max(0.0)))
                } else {
                    v
                };
                rgba.push(code8(v));
            }
        }
    }
    Thumbnail { width: w, height: h, rgba }
}

#[derive(Clone)]
struct FileEntry {
    stamp: SourceStamp,
    analysis: Analysis,
    info: SourceInfo,
    thumb: Thumbnail,
}

#[derive(Clone)]
pub(crate) struct PackEntry {
    pub analysis: Analysis,
    pub info: SourceInfo,
    pub non_gray: Vec<PathBuf>,
    pub thumb: Thumbnail,
}

/// Each source's analysis (keyed by path, valid while its size and mtime match) and each packed image's analysis
/// (keyed by `pack_key`, which includes every source's stamp).
#[derive(Default)]
pub struct AnalysisCache {
    files: Mutex<HashMap<PathBuf, FileEntry>>,
    packs: Mutex<HashMap<String, PackEntry>>,
    loads: AtomicU64,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// The cache key of a packed image: everything that shapes it, including each source's stamp.
pub fn pack_key(pk: &PackJob) -> String {
    sha256_json(pk).unwrap_or_default()
}

impl AnalysisCache {
    /// Images decoded so far (a packed image counts each of its sources).
    pub fn loads(&self) -> u64 {
        self.loads.load(Ordering::Relaxed)
    }
    pub(crate) fn count_loads(&self, n: u64) {
        self.loads.fetch_add(n, Ordering::Relaxed);
    }
    pub(crate) fn file(&self, path: &Path, stamp: SourceStamp) -> Option<(Analysis, SourceInfo)> {
        lock(&self.files)
            .get(path)
            .filter(|e| e.stamp == stamp)
            .map(|e| (e.analysis.clone(), e.info))
    }
    pub(crate) fn put_file(
        &self,
        path: &Path,
        stamp: SourceStamp,
        analysis: Analysis,
        info: SourceInfo,
        thumb: Thumbnail,
    ) {
        lock(&self.files).insert(path.to_path_buf(), FileEntry { stamp, analysis, info, thumb });
    }
    pub(crate) fn pack(&self, key: &str) -> Option<PackEntry> {
        lock(&self.packs).get(key).cloned()
    }
    pub(crate) fn put_pack(&self, key: String, e: PackEntry) {
        lock(&self.packs).insert(key, e);
    }
    pub fn thumbnail_of(&self, path: &Path) -> Option<Thumbnail> {
        lock(&self.files).get(path).map(|e| e.thumb.clone())
    }
    /// The job's thumbnail: its packed image's for a packed job, else its source's.
    pub fn thumbnail(&self, job: &ResolvedJob) -> Option<Thumbnail> {
        match &job.pack {
            Some(pk) => lock(&self.packs).get(&pack_key(pk)).map(|e| e.thumb.clone()),
            None => self.thumbnail_of(&job.input),
        }
    }
}

/// An engine for one GUI window: encoders initialised once (the BC1 model is process-global, spec §4.6).
pub struct Session {
    engine: Engine,
    cache: AnalysisCache,
    bc1_model: Bc1Model,
}

impl Session {
    pub fn new(bc1_model: Bc1Model) -> Result<Session, EngineError> {
        encoders_sys::init(bc1_model).map_err(|e| EngineError::Fatal(e.to_string()))?;
        Ok(Session { engine: Engine::new()?, cache: AnalysisCache::default(), bc1_model })
    }
    pub fn bc1_model(&self) -> Bc1Model {
        self.bc1_model
    }
    pub fn engine(&self) -> &Engine {
        &self.engine
    }
    pub fn cache(&self) -> &AnalysisCache {
        &self.cache
    }
    /// `Engine::plan` through the cache; the request's BC1 model is the session's.
    pub fn plan(&self, mut req: BatchRequest, rep: &Reporter, cancel: &CancelToken) -> Result<Plan, EngineError> {
        req.bc1_model = self.bc1_model;
        self.engine.plan_cached(req, rep, cancel, &self.cache)
    }
    pub fn run(&self, plan: Plan, rep: &Reporter, cancel: &CancelToken) -> Result<BatchSummary, EngineError> {
        self.engine.run(plan, rep, cancel)
    }
    pub fn thumbnail(&self, job: &ResolvedJob) -> Option<Thumbnail> {
        self.cache.thumbnail(job)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn session_is_send_and_sync() {
        fn check<T: Send + Sync>() {}
        check::<super::Session>();
    }
}
```

(If `Engine` is not `Sync`, find the field that is not and make it so.)

Thumbnails only for sessions: add `pub thumbnails: bool` to `AnalysisCache` (the derived `Default` makes it false), set it true in `Session::new` (`AnalysisCache { thumbnails: true, ..Default::default() }`), and make `FileEntry::thumb` / `PackEntry::thumb` `Option<Thumbnail>`, filled only when `cache.thumbnails` (so `Engine::plan`, which the CLI uses with a throwaway cache, never pays for a downscale). `thumbnail` / `thumbnail_of` return `None` when none was built.

- [ ] **Step 4: `plan_cached` in `api.rs`.** Rename `plan`'s body to `plan_cached(&self, req, rep, cancel, cache: &AnalysisCache)` and make `plan` call `self.plan_cached(req, rep, cancel, &AnalysisCache::default())`. In `resolve_one` (`api.rs:645-651`) replace the load/analyse lines with:

```rust
            enter(Stage::Load);
            // Taken before the read, so a change made while it is loaded is seen as a change too.
            let source = SourceStamp::of(&f.path);
            let (a, info) = match source.and_then(|s| cache.file(&f.path, s)) {
                Some(hit) => {
                    drop(admitted);
                    hit
                }
                None => {
                    let (img, info) = load_image(&f.path)?;
                    cache.count_loads(1);
                    enter(Stage::Analyse);
                    let a = analyse(&img, &info);
                    if let Some(s) = source {
                        cache.put_file(&f.path, s, a.clone(), info, make_thumbnail(&img, &info));
                    }
                    drop(img);
                    drop(admitted);
                    (a, info)
                }
            };
```

and in pass 2 (`api.rs:777-794`) look the packed image up first:

```rust
                        let key = pack_key(&c.job);
                        let entry = match cache.pack(&key) {
                            Some(e) => {
                                drop(admitted);
                                e
                            }
                            None => {
                                let asm = pack::assemble::assemble(&c.job, c.job.gray).map_err(|e| match e {
                                    pack::assemble::PackError::Load(e) => PlanFailure::Load(e),
                                    e => PlanFailure::Pack(e.to_string()),
                                })?;
                                cache.count_loads(c.job.sources.len() as u64);
                                stage.set(Stage::Analyse);
                                let e = PackEntry {
                                    analysis: analyse(&asm.image, &asm.info),
                                    info: asm.info,
                                    non_gray: asm.non_gray,
                                    thumb: make_thumbnail(&asm.image, &asm.info),
                                };
                                drop(asm.image);
                                drop(admitted);
                                cache.put_pack(key, e.clone());
                                e
                            }
                        };
                        stage.set(Stage::Resolve);
                        let set = sets
                            .iter()
                            .find(|s| s.key == c.set_key)
                            .expect("a candidate's set is planned");
                        Ok(pack::plan::finish_recipe(ctx, c, set, &entry.analysis, &entry.info, &entry.non_gray))
```

(`pack_key` includes each source's stamp, so an edited source misses the cache.) The test hooks still run through `enter`.

- [ ] **Step 5: Run the tests**

Run: `cargo test -p engine --test session` then `cargo test --workspace`
Expected: PASS. If the first count differs because of how pass 1 treats raw packing inputs, set the expected numbers to what the code does, explain the count in the test's comment, and keep both delta assertions (an identical re-plan loads 0; the cpm re-plan loads only the new recipe's sources).

- [ ] **Step 6: Commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings
git add crates/engine/src/session.rs crates/engine/src/lib.rs crates/engine/src/api.rs crates/engine/src/analyse.rs crates/engine/Cargo.toml crates/engine/tests/session.rs
git commit -m "engine: Session with analysis cache and thumbnails (M3a)"
```

---

### Task 8: `Session::preview` (byte-identical, size-limited)

**Files:**
- Modify: `crates/engine/src/pipeline.rs` (split `process_file` into `prepare` + `encode_levels`; `process_file` keeps its behaviour)
- Modify: `crates/engine/src/session.rs` (`SourcePreview`, `PreviewResult`, `NormalLenStats`, `first_level_within`, `Session::preview_source`, `Session::preview`)
- Test: `crates/engine/tests/preview.rs` (new)

**Interfaces:**
- Consumes: Task 7's `Session`.
- Produces (`engine::session`):
  - `pub fn first_level_within(dims: &[(u32, u32)], max_size: u32) -> usize` — the first mip whose larger side is ≤ `max_size` (the last mip when none is).
  - `pub struct SourcePreview { pub first_level: usize, pub dims: Vec<(u32, u32)>, pub levels: Vec<Rgba32F> }` — `dims` of every mip; `levels` from `first_level` on: the chain after Preprocess, Mipmaps and coverage, the values the encoder receives.
  - `pub struct NormalLenStats { pub mean_err: f64, pub max_err: f64 }`
  - `pub struct PreviewResult { pub source: SourcePreview, pub format: Format, pub srgb: bool, pub levels: Vec<Vec<u8>>, pub validation: ValidationSummary, pub warnings: Vec<Warning>, pub normal_len: Vec<NormalLenStats> }` — `levels[i]` is mip `first_level + i`'s block data.
  - `Session::preview_source(&self, job: &ResolvedJob, max_size: u32, cancel: &CancelToken) -> Result<SourcePreview, FileError>`
  - `Session::preview(&self, job: &ResolvedJob, max_size: u32, cancel: &CancelToken) -> Result<PreviewResult, FileError>`
- Produces (`engine::pipeline`, crate-private): `pub(crate) enum Keep { Encode, Preview { max_size: u32 }, SourceOnly { max_size: u32 } }`; `pub(crate) struct Prepared { pub first_level: usize, pub dims: Vec<(u32, u32)>, pub chain: Vec<Rgba32F>, pub inputs: Vec<EncoderInput>, pub base_coverage: Option<f64>, pub source_peak: Option<f32> }` (`chain` and `inputs` start at `first_level`; `chain` is empty for `Keep::Encode`, `inputs` empty for `Keep::SourceOnly`); `pub(crate) fn prepare(job: &ResolvedJob, cancel: &CancelToken, rep: &Reporter, job_key: &str, tracker: &FileTracker, keep: Keep) -> Result<Prepared, FileError>`; `pub(crate) fn encode_levels(prep: &Prepared, p: &Profile, cancel: &CancelToken, rep: &Reporter, job_key: &str, set: Option<&str>, path: Option<&Path>) -> Result<Vec<Vec<u8>>, FileError>`.

- [ ] **Step 1: Write the failing tests** in `crates/engine/tests/preview.rs`:

```rust
//! M3a Task 8: `Session::preview` mips equal the same mips of the DDS `run` writes, byte for byte.
use encoders_sys::Bc1Model;
use engine::api::*;
use engine::config::Target;
use engine::dds::read_dds;
use engine::events::*;
use engine::overrides::parse_set;
use engine::policy::{JobStatus, ResolvedJob};
use engine::session::*;
use image::{ImageBuffer, Luma, Rgb, Rgba};
use std::path::Path;
use std::sync::Arc;

/// Sources and the overrides that decide each one. They cover BC7, BC1, BC3, BC5 or BC7 normals, BC4, a cutout with
/// coverage on and a complex `_m` with exact opaque alpha on (the paths that look at more than one block or mip).
/// Every job must be Ready; if one is not, change its overrides (not the assertions) and say so in the report.
fn fixture(dir: &Path) -> Vec<&'static str> {
    let r = dir.join("in");
    std::fs::create_dir_all(&r).unwrap();
    let grad = |x: u32, y: u32| [(x * 4) as u8, (y * 4) as u8, ((x + y) * 2) as u8];
    ImageBuffer::from_fn(64, 64, |x, y| Rgb(grad(x, y))).save(r.join("c7.png")).unwrap();
    ImageBuffer::from_fn(64, 64, |x, y| Rgb(grad(x, y))).save(r.join("c1.png")).unwrap();
    ImageBuffer::from_fn(64, 64, |x, y| {
        let g = grad(x, y);
        Rgba([g[0], g[1], g[2], (x * 4) as u8])
    })
    .save(r.join("c3.png"))
    .unwrap();
    ImageBuffer::from_fn(64, 64, |x, y| {
        let g = grad(x, y);
        Rgba([g[0], g[1], g[2], if (x / 8 + y / 8) % 2 == 0 { 255 } else { 0 }])
    })
    .save(r.join("cut.png"))
    .unwrap();
    ImageBuffer::from_fn(64, 64, |x, y| Rgb([128 + (x % 16) as u8, 128 - (y % 16) as u8, 250]))
        .save(r.join("rock_n.png"))
        .unwrap();
    ImageBuffer::from_fn(64, 64, |x, y| {
        let g = grad(x, y);
        Rgba([g[0], g[1], g[2], if x < 32 { 255 } else { (y * 4) as u8 }])
    })
    .save(r.join("armor_m.png"))
    .unwrap();
    ImageBuffer::from_fn(64, 64, |x, y| Luma([((x + y) * 2) as u8]))
        .save(r.join("h_p.png"))
        .unwrap();
    vec![
        "file:c7.png.subtype=diffuse",
        "file:c1.png.subtype=diffuse",
        "file:c1.png.format=bc1",
        "file:c3.png.subtype=diffuse",
        "file:c3.png.alpha.policy=blend",
        "file:c3.png.format=bc3",
        "file:cut.png.subtype=diffuse",
        "file:cut.png.alpha.policy=cutout",
        "file:rock_n.png.subtype=normal",
        "file:rock_n.png.channels.green_convention=directx",
        "file:armor_m.png.subtype=complex_material",
        "file:h_p.png.subtype=height",
    ]
}

fn request(dir: &Path, sets: &[&str]) -> BatchRequest {
    let mut r = BatchRequest::new(vec![dir.join("in")], dir.join("out"), Target::Cs);
    r.overrides = sets.iter().map(|s| parse_set(s).unwrap()).collect();
    r.on_existing = OnExisting::Overwrite;
    r
}

fn check(max_size: u32, expect_first: usize) {
    let d = tempfile::tempdir().unwrap();
    let sets = fixture(d.path());
    let s = Session::new(Bc1Model::Ideal).unwrap();
    let rep = Reporter::new(Arc::new(NullSink), "t");
    let plan = s.plan(request(d.path(), &sets), &rep, &CancelToken::default()).unwrap();
    let jobs: Vec<ResolvedJob> = plan.jobs.clone();
    assert_eq!(jobs.len(), 7);
    for j in &jobs {
        assert_eq!(j.status, JobStatus::Ready, "{} is {:?}", j.input.display(), j.status);
    }
    s.run(plan, &rep, &CancelToken::default()).unwrap();
    for j in &jobs {
        let p = s.preview(j, max_size, &CancelToken::default()).unwrap();
        assert_eq!(p.source.first_level, expect_first, "{}", j.input.display());
        let dds = read_dds(&std::fs::read(&j.output).unwrap()).unwrap();
        assert_eq!(dds.levels.len(), p.source.dims.len());
        assert_eq!(p.levels.len(), dds.levels.len() - expect_first);
        for (i, enc) in p.levels.iter().enumerate() {
            let range = dds.levels[expect_first + i].2.clone();
            assert!(enc[..] == dds.data[range], "{} mip {}", j.input.display(), expect_first + i);
        }
        assert_eq!(p.source.levels.len(), p.levels.len());
        assert_eq!(p.source.levels[0].width, dds.levels[expect_first].0);
    }
}

#[test]
fn preview_mips_equal_the_exported_mips() {
    check(4096, 0);
}

#[test]
fn a_size_limit_skips_the_large_mips_and_keeps_the_rest_identical() {
    // 64 and 32 are above 16: the preview starts at mip 2 (16x16).
    check(16, 2);
}

#[test]
fn first_level_within_picks_the_first_mip_that_fits() {
    let dims = [(8192, 4096), (4096, 2048), (2048, 1024), (1, 1)];
    assert_eq!(first_level_within(&dims, 4096), 1);
    assert_eq!(first_level_within(&dims, 16384), 0);
    assert_eq!(first_level_within(&dims, 0), 3);
}
```

- [ ] **Step 2: Run to see them fail**

Run: `cargo test -p engine --test preview`
Expected: compile errors (`preview`, `first_level_within`).

- [ ] **Step 3: Split `process_file`.** Move the work from the stamp check (`pipeline.rs:146-169`) through quantise (`:229-235`) into `prepare`, keeping the hashing (`source_sha256`, `sources`) in `process_file` after `prepare`'s stamp check has passed (call order inside `process_file`: test hook → `prepare` → hashes → `encode_levels` → DDS bytes → validate → record → write; a hashing failure still writes nothing). `prepare`'s tail:

```rust
    enter(Stage::Mipmaps);
    let mut chain = build_chain(&work, p);
    let alpha_present = work.alpha_present;
    let base_coverage = base_coverage(&chain[0], p, alpha_present);
    if base_coverage.is_some() {
        preserve_coverage(&mut chain, p.alpha.threshold, p.alpha.compare);
    }
    let source_peak = source_peak(&chain[0], p);
    let dims: Vec<(u32, u32)> = chain.iter().map(|l| (l.width, l.height)).collect();
    let first_level = match keep {
        Keep::Encode => 0,
        Keep::Preview { max_size } | Keep::SourceOnly { max_size } => first_level_within(&dims, max_size),
    };
    let inputs: Vec<EncoderInput> = match keep {
        Keep::SourceOnly { .. } => vec![],
        _ => chain[first_level..].iter().map(|l| to_encoder_input(l, &work, p)).collect(),
    };
    let chain = match keep {
        Keep::Encode => vec![],
        _ => chain.split_off(first_level),
    };
    drop(work);
    stop()?;
    Ok(Prepared { first_level, dims, chain, inputs, base_coverage, source_peak })
```

`encode_levels` encodes `prep.inputs[i]` at `prep.dims[prep.first_level + i]` with the same `EncodeCtx` and `Progress` events as today (`done` from 1 to `inputs.len()`, carrying the job key). `process_file` then uses `prep.dims[0]` for the DDS size and `prep.inputs` for validation, exactly as before. `first_level_within` lives in `session.rs` (public API):

```rust
/// The first mip whose larger side is at most `max_size`; the last mip when none is (M3a spec §6.5).
pub fn first_level_within(dims: &[(u32, u32)], max_size: u32) -> usize {
    dims.iter()
        .position(|&(w, h)| w.max(h) <= max_size)
        .unwrap_or(dims.len().saturating_sub(1))
}
```

- [ ] **Step 4: `Session::preview_source` and `Session::preview`** in `session.rs`:

```rust
#[derive(Debug, Clone)]
pub struct SourcePreview {
    pub first_level: usize,
    pub dims: Vec<(u32, u32)>,
    pub levels: Vec<Rgba32F>,
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct NormalLenStats {
    pub mean_err: f64,
    pub max_err: f64,
}

#[derive(Debug, Clone)]
pub struct PreviewResult {
    pub source: SourcePreview,
    pub format: Format,
    pub srgb: bool,
    pub levels: Vec<Vec<u8>>,
    pub validation: ValidationSummary,
    pub warnings: Vec<Warning>,
    pub normal_len: Vec<NormalLenStats>,
}

impl Session {
    /// The source side of the preview (M3a spec §6.2): the chain the encoder would receive, from the first mip
    /// within `max_size`. Nothing is encoded or written.
    pub fn preview_source(
        &self,
        job: &ResolvedJob,
        max_size: u32,
        cancel: &CancelToken,
    ) -> Result<SourcePreview, FileError> {
        let rep = Reporter::new(Arc::new(NullSink), "preview");
        let prep = prepare(job, cancel, &rep, "", &FileTracker::default(), Keep::SourceOnly { max_size })?;
        Ok(SourcePreview { first_level: prep.first_level, dims: prep.dims, levels: prep.chain })
    }

    /// Source and encoded sides (M3a spec §4.5): every mip within `max_size`, byte-identical to the same mips of
    /// the DDS `run` writes, with the validation readouts of those mips. Nothing is written.
    pub fn preview(&self, job: &ResolvedJob, max_size: u32, cancel: &CancelToken) -> Result<PreviewResult, FileError> {
        let rep = Reporter::new(Arc::new(NullSink), "preview");
        let p = &job.profile;
        let prep = prepare(job, cancel, &rep, "", &FileTracker::default(), Keep::Preview { max_size })?;
        let levels = encode_levels(&prep, p, cancel, &rep, "", None, None)?;
        let srgb = p.srgb_tag();
        let (width, height) = prep.dims[prep.first_level];
        let desc = DdsDesc { width, height, format: p.format, srgb, header: resolve_header(p.format, srgb, p.header) };
        let bytes = dds_bytes(&desc, &levels).map_err(|e| FileError::Failed(Stage::Write, e.to_string()))?;
        let report = validate(&ValidationInput {
            bytes: &bytes,
            profile: p,
            encoder_input: Some(&prep.inputs),
            base_coverage: prep.base_coverage,
            source_peak: prep.source_peak,
            bc1_model: self.bc1_model,
            expected_levels: Some(levels.len() as u32),
        });
        let normal_len = if p.subtype.category() == Category::Normal {
            normal_lengths(&bytes, self.bc1_model)
        } else {
            vec![]
        };
        Ok(PreviewResult {
            source: SourcePreview { first_level: prep.first_level, dims: prep.dims, levels: prep.chain },
            format: p.format,
            srgb,
            levels,
            validation: report.summary,
            warnings: report.warnings,
            normal_len,
        })
    }
}

/// Per mip: decode with the engine's decoder and measure |length − 1| of RGB read as `2v − 1` (for BC5, whose blue
/// is reconstructed, the excess `max(0, x² + y² − 1)`).
fn normal_lengths(dds_bytes: &[u8], bc1_model: Bc1Model) -> Vec<NormalLenStats> {
    let Ok(f) = read_dds(dds_bytes) else {
        return vec![];
    };
    let bc5 = matches!(f.format.as_format(), Some((Format::Bc5, _)));
    f.levels
        .iter()
        .filter_map(|(w, h, r)| decode_level(f.format, *w, *h, &f.data[r.clone()], Some(bc1_model)).ok())
        .map(|d| {
            let Decoded::Ldr(px) = d else {
                return NormalLenStats { mean_err: 0.0, max_err: 0.0 };
            };
            let (mut sum, mut max) = (0f64, 0f64);
            for q in &px {
                let v = |i: usize| q[i] as f64 / 255.0 * 2.0 - 1.0;
                let e = if bc5 {
                    (v(0) * v(0) + v(1) * v(1) - 1.0).max(0.0)
                } else {
                    ((v(0) * v(0) + v(1) * v(1) + v(2) * v(2)).sqrt() - 1.0).abs()
                };
                sum += e;
                max = max.max(e);
            }
            NormalLenStats { mean_err: sum / px.len().max(1) as f64, max_err: max }
        })
        .collect()
}
```

Imports this needs: `crate::codes::Warning`, `crate::config::{Category, Format}`, `crate::dds::{dds_bytes, read_dds, resolve_header, DdsDesc}`, `crate::decode::{decode_level, Decoded}`, `crate::events::{NullSink, Stage}`, `crate::pipeline::{encode_levels, prepare, FileError, FileTracker, Keep}`, `crate::validate::{validate, ValidationInput, ValidationSummary}`, `std::sync::Arc`. (Adjust `Format::Bc5` to the enum's real variant name.)

- [ ] **Step 5: Run the tests**

Run: `cargo test -p engine --test preview` then `cargo test --workspace` (the golden and pack tests prove `process_file` is unchanged)
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings
git add crates/engine/src/pipeline.rs crates/engine/src/session.rs crates/engine/tests/preview.rs
git commit -m "engine: Session::preview, byte-identical and size-limited (M3a)"
```

---

### Task 9: The `gui` crate, settings and the window skeleton

**Files:**
- Modify: `Cargo.toml` (workspace member `crates/gui`)
- Create: `crates/gui/Cargo.toml`, `crates/gui/src/main.rs`, `crates/gui/src/lib.rs`, `crates/gui/src/settings.rs`, `crates/gui/src/app.rs`, `crates/gui/src/ui/mod.rs`
- Modify: `THIRD_PARTY_NOTICES.md`
- Test: `crates/gui/tests/settings.rs`, `crates/gui/tests/ui_smoke.rs`

**Interfaces:**
- Produces (`sdc_gui::settings`):
  - `#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)] #[serde(default)] pub struct GuiSettings { pub export_folder: Option<PathBuf>, pub recent: Vec<PathBuf>, pub preview_max: u32, pub allow_16k_preview: bool, pub live_encode: bool, pub bc1_decoder: Bc1Choice, pub jobs: usize, pub mem_budget_mb: u64, pub columns_collapsed: bool, pub advanced_open: bool }` with `Default` = `{ None, [], 4096, false, false, Ideal, 0, 0, false, false }`.
  - `#[serde(rename_all = "kebab-case")] pub enum Bc1Choice { Ideal, IdealRound4, Nvidia, Amd }` with `pub fn model(self) -> Bc1Model` and `pub fn cli(self) -> &'static str` (`ideal`, `ideal-round4`, `nvidia`, `amd`).
  - `pub const PREVIEW_SIZES: [u32; 5] = [1024, 2048, 4096, 8192, 16384];`
  - `pub fn settings_path() -> Option<PathBuf>` (`%APPDATA%\sdc\gui.toml`), `GuiSettings::load(path: &Path) -> (GuiSettings, Option<String>)` (a missing file gives defaults and `None`; an unreadable or invalid one gives defaults and the error text), `GuiSettings::save(&self, path: &Path) -> std::io::Result<()>`, `GuiSettings::push_recent(&mut self, p: PathBuf)` (most recent first, no duplicates, at most 10).
- Produces (`sdc_gui::ui`): `pub fn draw(ctx: &egui::Context, model: &mut AppModel)` — Task 9 draws the empty skeleton from a placeholder `AppModel` defined in `model/mod.rs` with only `settings: GuiSettings` and `root: Option<PathBuf>`; Task 11 replaces the placeholder.

- [ ] **Step 1: Crate manifest.** Add `"crates/gui"` to the workspace `members`. Create `crates/gui/Cargo.toml`:

```toml
[package]
name = "sdc-gui"
version.workspace = true
edition.workspace = true
license.workspace = true
publish.workspace = true
# eframe 0.36.2's floor.
rust-version = "1.95"

[lib]
name = "sdc_gui"
path = "src/lib.rs"

[[bin]]
name = "sdc-gui"
path = "src/main.rs"

[dependencies]
engine.workspace = true
encoders-sys.workspace = true
serde.workspace = true
serde_json.workspace = true
toml.workspace = true
half.workspace = true
anyhow.workspace = true
# Default features: the wgpu renderer, accesskit, default fonts. wgpu is used only through eframe::egui_wgpu::wgpu.
eframe = "=0.36.2"
rfd = "=0.17.2"
toml_edit = "0.25"

[dev-dependencies]
egui_kittest = "=0.36.2"
tempfile.workspace = true
image.workspace = true
```

(The first `cargo build` downloads these crates from crates.io; the owner approved that at plan review.) If eframe 0.36.2's default features include `glow`, set `default-features = false, features = ["wgpu", "accesskit", "default_fonts", "x11", "wayland"]` instead, so only the wgpu renderer is built.

- [ ] **Step 2: Write the failing tests.** `crates/gui/tests/settings.rs`:

```rust
use sdc_gui::settings::*;

#[test]
fn defaults_follow_the_spec() {
    let s = GuiSettings::default();
    assert_eq!(s.preview_max, 4096);
    assert!(!s.live_encode, "preview encoding is off by default");
    assert!(!s.allow_16k_preview);
    assert_eq!(s.bc1_decoder, Bc1Choice::Ideal);
}

#[test]
fn settings_round_trip_and_bad_files_fall_back_to_defaults() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("sdc/gui.toml");
    let (s, err) = GuiSettings::load(&p);
    assert_eq!((s, err), (GuiSettings::default(), None));
    let mut s = GuiSettings::default();
    s.live_encode = true;
    s.preview_max = 2048;
    s.push_recent("C:/a".into());
    s.push_recent("C:/b".into());
    s.push_recent("C:/a".into());
    assert_eq!(s.recent, vec![std::path::PathBuf::from("C:/a"), "C:/b".into()]);
    s.save(&p).unwrap();
    assert_eq!(GuiSettings::load(&p).0, s);
    std::fs::write(&p, "preview_max = \"big\"").unwrap();
    let (s, err) = GuiSettings::load(&p);
    assert_eq!(s, GuiSettings::default());
    assert!(err.is_some());
}

#[test]
fn recent_keeps_ten() {
    let mut s = GuiSettings::default();
    for i in 0..12 {
        s.push_recent(format!("C:/f{i}").into());
    }
    assert_eq!(s.recent.len(), 10);
    assert_eq!(s.recent[0], std::path::PathBuf::from("C:/f11"));
}
```

`crates/gui/tests/ui_smoke.rs`:

```rust
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use sdc_gui::model::AppModel;
use sdc_gui::settings::GuiSettings;

#[test]
fn the_empty_window_shows_its_regions() {
    let mut h = Harness::new_state(
        |ctx, model: &mut AppModel| sdc_gui::ui::draw(ctx, model),
        AppModel::placeholder(GuiSettings::default()),
    );
    h.run();
    for label in ["Open folder…", "Mode", "Export", "Textures", "Sets", "Advanced"] {
        h.get_by_label(label);
    }
    h.get_by_label_contains("Open a folder");
}
```

- [ ] **Step 3: Run to see them fail**

Run: `cargo test -p sdc-gui`
Expected: compile errors (`sdc_gui` modules missing).

- [ ] **Step 4: Implement `settings.rs`**

```rust
//! GUI-only settings (M3a spec §5.5): `%APPDATA%\sdc\gui.toml`. Never part of `sdc.toml`.
use encoders_sys::Bc1Model;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Preview size limits offered (M3a spec §6.5); 16384 only when `allow_16k_preview` is set.
pub const PREVIEW_SIZES: [u32; 5] = [1024, 2048, 4096, 8192, 16384];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Bc1Choice {
    #[default]
    Ideal,
    IdealRound4,
    Nvidia,
    Amd,
}

impl Bc1Choice {
    pub const ALL: [Bc1Choice; 4] = [Bc1Choice::Ideal, Bc1Choice::IdealRound4, Bc1Choice::Nvidia, Bc1Choice::Amd];
    pub fn model(self) -> Bc1Model {
        match self {
            Bc1Choice::Ideal => Bc1Model::Ideal,
            Bc1Choice::IdealRound4 => Bc1Model::IdealRound4,
            Bc1Choice::Nvidia => Bc1Model::Nvidia,
            Bc1Choice::Amd => Bc1Model::Amd,
        }
    }
    /// The `sdc convert --bc1-decoder` value.
    pub fn cli(self) -> &'static str {
        match self {
            Bc1Choice::Ideal => "ideal",
            Bc1Choice::IdealRound4 => "ideal-round4",
            Bc1Choice::Nvidia => "nvidia",
            Bc1Choice::Amd => "amd",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GuiSettings {
    pub export_folder: Option<PathBuf>,
    /// Most recent first, at most 10.
    pub recent: Vec<PathBuf>,
    /// Preview size limit (M3a spec §6.5), 4096 by default.
    pub preview_max: u32,
    pub allow_16k_preview: bool,
    /// Live preview encoding; off on a fresh install (M3a spec §6.3).
    pub live_encode: bool,
    /// Read at launch; a change applies after a restart (spec §4.6).
    pub bc1_decoder: Bc1Choice,
    /// Worker threads; 0 = one per core.
    pub jobs: usize,
    /// Memory budget in MiB; 0 = half of physical memory.
    pub mem_budget_mb: u64,
    pub columns_collapsed: bool,
    pub advanced_open: bool,
}

impl Default for GuiSettings {
    fn default() -> Self {
        GuiSettings {
            export_folder: None,
            recent: vec![],
            preview_max: 4096,
            allow_16k_preview: false,
            live_encode: false,
            bc1_decoder: Bc1Choice::Ideal,
            jobs: 0,
            mem_budget_mb: 0,
            columns_collapsed: false,
            advanced_open: false,
        }
    }
}

/// `%APPDATA%\sdc\gui.toml`.
pub fn settings_path() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("sdc").join("gui.toml"))
}

impl GuiSettings {
    /// Defaults when the file is missing; defaults and the error text when it cannot be read or parsed.
    pub fn load(path: &Path) -> (GuiSettings, Option<String>) {
        match std::fs::read_to_string(path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (GuiSettings::default(), None),
            Err(e) => (GuiSettings::default(), Some(format!("{}: {e}", path.display()))),
            Ok(text) => match toml::from_str(&text) {
                Ok(s) => (s, None),
                Err(e) => (GuiSettings::default(), Some(format!("{}: {e}", path.display()))),
            },
        }
    }
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = toml::to_string_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(path, text)
    }
    pub fn push_recent(&mut self, p: PathBuf) {
        self.recent.retain(|r| r != &p);
        self.recent.insert(0, p);
        self.recent.truncate(10);
    }
}
```

- [ ] **Step 5: Skeleton.** `lib.rs`:

```rust
//! `sdc-gui` (M3a spec): a converter GUI over the engine. `model` holds all state and logic and has no egui types;
//! `ui` only draws it; `render` owns the GPU preview; `worker` runs engine calls off the UI thread.
pub mod app;
pub mod model;
pub mod settings;
pub mod ui;
```

`model/mod.rs` (placeholder until Task 11):

```rust
//! The view-model (M3a spec §3). Task 11 fills it in.
use crate::settings::GuiSettings;
use std::path::PathBuf;

pub struct AppModel {
    pub settings: GuiSettings,
    pub root: Option<PathBuf>,
}

impl AppModel {
    /// A model with no session, for drawing tests.
    pub fn placeholder(settings: GuiSettings) -> AppModel {
        AppModel { settings, root: None }
    }
}
```

`ui/mod.rs`:

```rust
//! egui drawing of the model (M3a spec §2). No logic beyond mapping input to model calls.
use crate::model::AppModel;

pub fn draw(ctx: &egui::Context, model: &mut AppModel) {
    egui::TopBottomPanel::top("header").show(ctx, |ui| {
        ui.horizontal(|ui| {
            let _ = ui.button("Open folder…");
            ui.label("Mode");
            ui.separator();
            ui.checkbox(&mut model.settings.columns_collapsed, "Collapse columns");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_enabled(false, egui::Button::new("Export"));
                ui.label("Export folder: not set");
            });
        });
    });
    egui::SidePanel::right("advanced").resizable(false).show(ctx, |ui| {
        ui.toggle_value(&mut model.settings.advanced_open, "Advanced");
    });
    egui::SidePanel::left("textures").show(ctx, |ui| {
        ui.heading("Textures");
    });
    egui::SidePanel::left("sets").show(ctx, |ui| {
        ui.heading("Sets");
    });
    egui::CentralPanel::default().show(ctx, |ui| {
        ui.centered_and_justified(|ui| ui.label("Open a folder of source textures to begin."));
    });
}
```

`use eframe::egui;` at the top of every `ui`/`app`/`render` file (egui is reached through eframe; do not add `egui` as a separate dependency).

`app.rs`:

```rust
//! The eframe application: owns the model, draws it each frame.
use crate::model::AppModel;
use eframe::egui;

pub struct SdcApp {
    pub model: AppModel,
}

impl eframe::App for SdcApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        crate::ui::draw(ctx, &mut self.model);
    }
}
```

`main.rs`:

```rust
#![windows_subsystem = "windows"]
//! `sdc-gui.exe`.
use sdc_gui::{app::SdcApp, model::AppModel, settings};

fn main() -> eframe::Result {
    let path = settings::settings_path();
    let (s, _err) = path.as_deref().map(settings::GuiSettings::load).unwrap_or_default();
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("sdc")
            .with_inner_size([1400.0, 900.0]),
        ..Default::default()
    };
    eframe::run_native("sdc", options, Box::new(|_cc| Ok(Box::new(SdcApp { model: AppModel::placeholder(s) }))))
}
```

- [ ] **Step 6: Notices.** Append to `THIRD_PARTY_NOTICES.md` a line: `- **egui / eframe / egui-wgpu / egui_kittest** (0.36.2), **wgpu**, **rfd** (0.17), **toml_edit** (0.25): MIT or Apache-2.0 — the GUI (sdc-gui.exe) only.` Run `cargo tree -p sdc-gui -e normal --prefix none | sort -u` and check every crate's licence with `cargo metadata --format-version 1` (field `license`); any licence that is not MIT, Apache-2.0, BSD-2/3-Clause, ISC, Zlib, MPL-2.0, Unicode-3.0, CC0-1.0, BSL-1.0 or Unlicense stops the task (report it).

- [ ] **Step 7: Run the tests**

Run: `cargo test -p sdc-gui` then `cargo build -p sdc-gui --release` (proves the exe links with `+crt-static`) then `cargo test --workspace`
Expected: PASS.

- [ ] **Step 8: Commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings
git add Cargo.toml Cargo.lock crates/gui THIRD_PARTY_NOTICES.md
git commit -m "gui: sdc-gui crate, settings and window skeleton (M3a)"
```

---

### Task 10: `ProjectDoc`, the format-preserving `sdc.toml`

**Files:**
- Create: `crates/gui/src/project.rs`
- Modify: `crates/gui/src/lib.rs` (`pub mod project;`)
- Test: `crates/gui/tests/project.rs`

**Interfaces:**
- Consumes: Task 3's `ConfigFile::files` shape and key rule.
- Produces (`sdc_gui::project`):
  - `pub struct ProjectDoc` with:
    - `pub fn open(root: &Path) -> ProjectDoc` — `<root>/sdc.toml`; a missing file is an empty document; a file that does not parse leaves the document empty and **read-only** with `error()` set.
    - `pub fn path(&self) -> &Path`, `pub fn is_dirty(&self) -> bool`, `pub fn read_only(&self) -> bool`, `pub fn error(&self) -> Option<&str>`
    - `pub fn config(&self) -> Result<ConfigFile, String>` — the document parsed as the engine sees it.
    - `pub fn target(&self) -> Option<Target>`, `pub fn set_target(&mut self, t: Target)`
    - `pub fn file_value(&self, key: &str, field: &str) -> Option<serde_json::Value>`, `pub fn set_file_value(&mut self, key: &str, field: &str, value: &serde_json::Value)`, `pub fn clear_file_value(&mut self, key: &str, field: &str)`
    - `pub fn allow(&self, key: &str) -> BTreeSet<Code>`, `pub fn set_allow(&mut self, key: &str, codes: &BTreeSet<Code>)`
    - `pub fn fill(&self, role: RawMap) -> Option<u8>`, `pub fn set_fill(&mut self, role: RawMap, value: Option<u8>)`
    - `pub fn file_keys(&self) -> Vec<String>`, `pub fn remove_file_entry(&mut self, key: &str)`
    - `pub fn changed_on_disk(&self) -> bool`, `pub fn save(&mut self, force: bool) -> Result<(), SaveError>`, `pub fn reload(&mut self)`
  - `#[derive(Debug, Clone, PartialEq)] pub enum SaveError { ReadOnly, ChangedOnDisk, Io(String) }` with `Display`.

Every setter is a no-op when `read_only()`; every setter that changes the text sets `is_dirty()`.

- [ ] **Step 1: Write the failing tests** in `crates/gui/tests/project.rs`:

```rust
use engine::codes::Code;
use engine::config::{RawMap, Target};
use sdc_gui::project::*;
use serde_json::json;

const HAND_WRITTEN: &str = r#"# My mod's settings — keep this comment.
preset = "cs-quality"

[defaults]
mips.levels = "full"   # trailing comment

[[rules]]
match = { glob = "**/foliage/**" }
overrides = { alpha = { policy = "cutout" } }
"#;

fn doc_with(text: &str) -> (tempfile::TempDir, ProjectDoc) {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("sdc.toml"), text).unwrap();
    let doc = ProjectDoc::open(d.path());
    (d, doc)
}

#[test]
fn edits_keep_every_hand_written_byte() {
    let (d, mut doc) = doc_with(HAND_WRITTEN);
    doc.set_target(Target::Cpm);
    doc.set_file_value("landscape/rock01_n.png", "format", &json!("bc7"));
    doc.set_file_value("landscape/rock01_d.png", "alpha.policy", &json!("blend"));
    doc.set_allow("landscape/rock01_n.png", &[Code::FmtBc5Normal].into_iter().collect());
    doc.set_fill(RawMap::Metallic, Some(255));
    assert!(doc.is_dirty());
    doc.save(false).unwrap();
    assert!(!doc.is_dirty());
    let text = std::fs::read_to_string(d.path().join("sdc.toml")).unwrap();
    for line in HAND_WRITTEN.lines() {
        assert!(text.contains(line), "lost line: {line}");
    }
    let cfg = ProjectDoc::open(d.path()).config().unwrap();
    assert_eq!(cfg.target, Some(Target::Cpm));
    assert_eq!(cfg.files["landscape/rock01_n.png"]["format"], "bc7");
    assert_eq!(cfg.files["landscape/rock01_n.png"]["allow"], json!(["FMT-BC5-NORMAL"]));
    assert_eq!(cfg.files["landscape/rock01_d.png"]["alpha"]["policy"], "blend");
    assert_eq!(cfg.defaults["packing"]["fill"]["metallic"], 255);
    assert_eq!(cfg.rules.len(), 1);
}

#[test]
fn awkward_keys_are_quoted_and_read_back() {
    let (d, mut doc) = doc_with("");
    let keys = ["a b/c d.png", "ü/naïve_n.png", "x.y/rock.01_rmaos", "[old]{1}*.png"];
    for k in keys {
        doc.set_file_value(k, "alpha.threshold", &json!(90));
    }
    doc.save(false).unwrap();
    let cfg = ProjectDoc::open(d.path()).config().unwrap();
    for k in keys {
        assert_eq!(cfg.files[k]["alpha"]["threshold"], 90, "{k}");
    }
}

#[test]
fn clearing_removes_empty_tables_and_entries() {
    let (_d, mut doc) = doc_with("");
    doc.set_file_value("a.png", "alpha.policy", &json!("blend"));
    doc.set_file_value("a.png", "format", &json!("bc7"));
    doc.clear_file_value("a.png", "alpha.policy");
    assert_eq!(doc.file_value("a.png", "alpha.policy"), None);
    assert_eq!(doc.file_value("a.png", "format"), Some(json!("bc7")));
    assert!(!doc.config().unwrap().files["a.png"].as_object().unwrap().contains_key("alpha"));
    doc.clear_file_value("a.png", "format");
    assert!(doc.file_keys().is_empty());
    doc.set_fill(RawMap::Ao, Some(10));
    doc.set_fill(RawMap::Ao, None);
    assert_eq!(doc.fill(RawMap::Ao), None);
}

#[test]
fn an_external_change_needs_force_and_an_invalid_file_is_read_only() {
    let (d, mut doc) = doc_with(HAND_WRITTEN);
    doc.set_target(Target::Vanilla);
    std::fs::write(d.path().join("sdc.toml"), "preset = \"other\"\n# edited elsewhere, longer text\n").unwrap();
    assert!(doc.changed_on_disk());
    assert_eq!(doc.save(false), Err(SaveError::ChangedOnDisk));
    doc.save(true).unwrap();
    assert!(!doc.changed_on_disk());

    let (_d, mut bad) = doc_with("target = [\n");
    assert!(bad.read_only());
    assert!(bad.error().unwrap().contains("sdc.toml"));
    bad.set_target(Target::Cs);
    assert!(!bad.is_dirty());
    assert_eq!(bad.save(true), Err(SaveError::ReadOnly));
}

#[test]
fn a_missing_file_is_created_on_save() {
    let d = tempfile::tempdir().unwrap();
    let mut doc = ProjectDoc::open(d.path());
    assert!(!doc.read_only());
    doc.set_file_value("a.png", "subtype", &json!("diffuse"));
    doc.save(false).unwrap();
    assert!(d.path().join("sdc.toml").exists());
}
```

- [ ] **Step 2: Run to see them fail**

Run: `cargo test -p sdc-gui --test project`
Expected: compile errors.

- [ ] **Step 3: Implement `project.rs`**

```rust
//! `sdc.toml` as the GUI edits it (M3a spec §5.3–5.4): format-preserving (`toml_edit`), so comments, ordering and
//! hand-written tables survive. The GUI owns only `target`, `[files.*]` and `defaults.packing.fill.*`.
use engine::codes::Code;
use engine::config::{RawMap, Target};
use engine::policy::SourceStamp;
use engine::presets::ConfigFile;
use serde_json::Value as Json;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use toml_edit::{Array, DocumentMut, Item, Table, Value};

#[derive(Debug, Clone, PartialEq)]
pub enum SaveError {
    /// The file did not parse when it was opened; the GUI does not write it.
    ReadOnly,
    /// The file changed on disk since it was loaded or saved; save again with `force` to overwrite.
    ChangedOnDisk,
    Io(String),
}

impl std::fmt::Display for SaveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SaveError::ReadOnly => f.write_str("sdc.toml is read-only until its error is fixed and it is reloaded"),
            SaveError::ChangedOnDisk => f.write_str("sdc.toml changed on disk since it was loaded"),
            SaveError::Io(e) => write!(f, "could not save sdc.toml: {e}"),
        }
    }
}

pub struct ProjectDoc {
    path: PathBuf,
    doc: DocumentMut,
    disk: Option<SourceStamp>,
    dirty: bool,
    error: Option<String>,
}

fn json_to_toml(v: &Json) -> Option<Value> {
    Some(match v {
        Json::Bool(b) => Value::from(*b),
        Json::Number(n) => match n.as_i64() {
            Some(i) => Value::from(i),
            None => Value::from(n.as_f64()?),
        },
        Json::String(s) => Value::from(s.as_str()),
        Json::Array(a) => {
            let mut arr = Array::new();
            for x in a {
                arr.push(json_to_toml(x)?);
            }
            Value::Array(arr)
        }
        Json::Null | Json::Object(_) => return None,
    })
}

fn toml_to_json(v: &Item) -> Option<Json> {
    let text = match v {
        Item::Value(v) => format!("v = {v}"),
        _ => return None,
    };
    let t: toml::Table = toml::from_str(&text).ok()?;
    serde_json::to_value(&t["v"]).ok()
}

impl ProjectDoc {
    pub fn open(root: &Path) -> ProjectDoc {
        let path = root.join("sdc.toml");
        let mut d = ProjectDoc { path, doc: DocumentMut::new(), disk: None, dirty: false, error: None };
        d.reload();
        d
    }

    /// Re-read the file, dropping unsaved edits.
    pub fn reload(&mut self) {
        self.dirty = false;
        self.error = None;
        self.doc = DocumentMut::new();
        self.disk = SourceStamp::of(&self.path);
        match std::fs::read_to_string(&self.path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => self.error = Some(format!("{}: {e}", self.path.display())),
            Ok(text) => match text.parse::<DocumentMut>() {
                Ok(doc) => {
                    self.doc = doc;
                    if let Err(e) = self.config() {
                        self.error = Some(format!("{}: {e}", self.path.display()));
                        self.doc = DocumentMut::new();
                    }
                }
                Err(e) => self.error = Some(format!("{}: {e}", self.path.display())),
            },
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }
    pub fn read_only(&self) -> bool {
        self.error.is_some()
    }
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// The document as the engine parses it.
    pub fn config(&self) -> Result<ConfigFile, String> {
        toml::from_str(&self.doc.to_string()).map_err(|e| e.to_string())
    }

    pub fn target(&self) -> Option<Target> {
        self.config().ok()?.target
    }
    pub fn set_target(&mut self, t: Target) {
        if self.read_only() {
            return;
        }
        let s = serde_json::to_value(t).ok();
        if let Some(Json::String(s)) = s {
            self.doc["target"] = toml_edit::value(s);
            self.dirty = true;
        }
    }

    /// The `[files]` table, created implicit (no bare `[files]` header) when missing.
    fn files_mut(&mut self) -> &mut Table {
        let item = self.doc.entry("files").or_insert_with(|| {
            let mut t = Table::new();
            t.set_implicit(true);
            Item::Table(t)
        });
        item.as_table_mut().expect("[files] is a table")
    }

    /// The entry for `key`: `[files."<key>"]`, with dotted keys inside for nested fields.
    fn entry_mut(&mut self, key: &str) -> &mut Table {
        let files = self.files_mut();
        if !files.contains_key(key) {
            files.insert(key, Item::Table(Table::new()));
        }
        files[key].as_table_mut().expect("a [files] entry is a table")
    }

    fn entry(&self, key: &str) -> Option<&Table> {
        self.doc.get("files")?.as_table()?.get(key)?.as_table()
    }

    pub fn file_value(&self, key: &str, field: &str) -> Option<Json> {
        let mut item: &Item = self.entry(key)?.get(field.split('.').next()?)?;
        for part in field.split('.').skip(1) {
            item = item.get(part)?;
        }
        toml_to_json(item)
    }

    pub fn set_file_value(&mut self, key: &str, field: &str, value: &Json) {
        if self.read_only() {
            return;
        }
        let Some(v) = json_to_toml(value) else { return };
        let parts: Vec<&str> = field.split('.').collect();
        let mut t = self.entry_mut(key);
        for part in &parts[..parts.len() - 1] {
            if !t.contains_key(part) {
                let mut sub = Table::new();
                sub.set_dotted(true);
                t.insert(part, Item::Table(sub));
            }
            t = t[*part].as_table_mut().expect("a field path runs through tables");
        }
        t[parts[parts.len() - 1]] = Item::Value(v);
        self.dirty = true;
    }

    pub fn clear_file_value(&mut self, key: &str, field: &str) {
        if self.read_only() || self.entry(key).is_none() {
            return;
        }
        fn remove(t: &mut Table, parts: &[&str]) -> bool {
            if parts.len() == 1 {
                return t.remove(parts[0]).is_some();
            }
            let Some(sub) = t.get_mut(parts[0]).and_then(Item::as_table_mut) else { return false };
            let hit = remove(sub, &parts[1..]);
            if sub.is_empty() {
                t.remove(parts[0]);
            }
            hit
        }
        let parts: Vec<&str> = field.split('.').collect();
        let entry = self.entry_mut(key);
        if remove(entry, &parts) {
            self.dirty = true;
        }
        if self.entry(key).is_some_and(Table::is_empty) {
            self.remove_file_entry(key);
        }
    }

    pub fn allow(&self, key: &str) -> BTreeSet<Code> {
        self.file_value(key, "allow")
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default()
            .iter()
            .filter_map(|c| c.as_str().and_then(Code::parse))
            .collect()
    }

    pub fn set_allow(&mut self, key: &str, codes: &BTreeSet<Code>) {
        if codes.is_empty() {
            self.clear_file_value(key, "allow");
        } else {
            let list: Vec<Json> = codes.iter().map(|c| Json::from(c.as_str())).collect();
            self.set_file_value(key, "allow", &Json::Array(list));
        }
    }

    pub fn fill(&self, role: RawMap) -> Option<u8> {
        let v = self.doc.get("defaults")?.get("packing")?.get("fill")?.get(role_id(role))?;
        toml_to_json(v)?.as_u64().and_then(|n| u8::try_from(n).ok())
    }

    pub fn set_fill(&mut self, role: RawMap, value: Option<u8>) {
        if self.read_only() {
            return;
        }
        let defaults = self.doc.entry("defaults").or_insert_with(|| Item::Table(Table::new()));
        let Some(defaults) = defaults.as_table_mut() else { return };
        match value {
            Some(v) => {
                let packing = defaults.entry("packing").or_insert_with(|| {
                    let mut t = Table::new();
                    t.set_dotted(true);
                    Item::Table(t)
                });
                let Some(packing) = packing.as_table_mut() else { return };
                let fill = packing.entry("fill").or_insert_with(|| {
                    let mut t = Table::new();
                    t.set_dotted(true);
                    Item::Table(t)
                });
                let Some(fill) = fill.as_table_mut() else { return };
                fill[role_id(role)] = toml_edit::value(i64::from(v));
            }
            None => {
                if let Some(fill) = defaults
                    .get_mut("packing")
                    .and_then(|p| p.get_mut("fill"))
                    .and_then(Item::as_table_mut)
                {
                    fill.remove(role_id(role));
                }
            }
        }
        self.dirty = true;
    }

    pub fn file_keys(&self) -> Vec<String> {
        self.doc
            .get("files")
            .and_then(Item::as_table)
            .map(|t| t.iter().map(|(k, _)| k.to_string()).collect())
            .unwrap_or_default()
    }

    pub fn remove_file_entry(&mut self, key: &str) {
        if self.read_only() {
            return;
        }
        if self.files_mut().remove(key).is_some() {
            self.dirty = true;
        }
    }

    pub fn changed_on_disk(&self) -> bool {
        SourceStamp::of(&self.path) != self.disk
    }

    /// Write the document (atomically: a temporary file, then rename).
    pub fn save(&mut self, force: bool) -> Result<(), SaveError> {
        if self.read_only() {
            return Err(SaveError::ReadOnly);
        }
        if !force && self.changed_on_disk() {
            return Err(SaveError::ChangedOnDisk);
        }
        let tmp = self.path.with_extension("toml.sdc-tmp");
        std::fs::write(&tmp, self.doc.to_string())
            .and_then(|_| std::fs::rename(&tmp, &self.path))
            .map_err(|e| SaveError::Io(e.to_string()))?;
        self.disk = SourceStamp::of(&self.path);
        self.dirty = false;
        Ok(())
    }
}

fn role_id(role: RawMap) -> &'static str {
    role.id()
}
```

(If `RawMap::id` does not exist, use `serde_json::to_value(role)` and take the string. `toml_edit` quotes a key that is not a bare key when it formats the document; the `awkward_keys` test proves it. If it does not, build the key with `toml_edit::Key::new(key)` and insert with `insert_formatted`.)

- [ ] **Step 4: Run the tests**

Run: `cargo test -p sdc-gui --test project`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings
git add crates/gui/src/project.rs crates/gui/src/lib.rs crates/gui/tests/project.rs
git commit -m "gui: ProjectDoc, format-preserving sdc.toml editing (M3a)"
```

---

### Task 11: The view-model and the worker

**Files:**
- Create: `crates/gui/src/model/mod.rs` (replaces the placeholder), `crates/gui/src/model/items.rs`, `crates/gui/src/worker.rs`
- Modify: `crates/gui/src/lib.rs` (`pub mod worker;`), `crates/gui/src/main.rs` (build the real model), `crates/gui/tests/ui_smoke.rs` (keeps compiling)
- Test: `crates/gui/tests/model.rs`, `crates/gui/tests/support/mod.rs` (shared fixture)

**Interfaces:**
- Consumes: `engine::session::Session` (Task 7), `ProjectDoc` (Task 10), `JobStatus::Unresolved::needs` (Task 4), `SkipKind`/job keys (Task 5), `BatchRequest::project_config_inline` (Task 3), `engine::overrides::project_rel` (Task 2).
- Produces (`sdc_gui::model::items`):

```rust
pub enum ItemKind { Image, Packed, Dds }
pub enum ItemStatus { Ready, NeedsDecision, Error(Vec<Code>), Failed(String), Skipped(String), PackedInto(String) }
pub enum Decision { Subtype { suggested: Option<Subtype> }, AlphaMeaning, GreenConvention, Acknowledge(Code) }
pub struct TextureItem {
    pub key: String,            // [files] key: project-relative, packed outputs without extension
    pub job_key: String,        // output key ("" when the item has no output)
    pub input: PathBuf,
    pub name: String,           // file name shown under the thumbnail (packed: "<stem>.<recipe>")
    pub set_key: Option<String>,
    pub kind: ItemKind,
    pub job: Option<ResolvedJob>,
    pub status: ItemStatus,
    pub decisions: Vec<Decision>,
    pub warnings: Vec<Warning>,
    pub thumb: Option<Thumbnail>,
    pub changed_on_disk: bool,
}
pub struct SetItem { pub key: String, pub members: Vec<String> /* item keys */, pub recipes: Vec<RecipeReport>, pub pbr: bool }
pub struct PlanView { pub target: Target, pub items: Vec<TextureItem>, pub sets: Vec<SetItem>, pub unmatched: Vec<String> }
impl PlanView { pub fn build(plan: &Plan, events: &[EngineEvent], session: &Session, root: &Path) -> PlanView }
```

- Produces (`sdc_gui::worker`):

```rust
pub enum WorkerMsg {
    Planned { generation: u64, result: Result<PlanView, String> },
    Event(EngineEvent),                                   // export progress (Task 14)
    ExportDone(Result<BatchSummary, String>),             // Task 14
    Source { token: u64, result: Result<Arc<SourcePreview>, String> },   // Task 15
    Encoded { token: u64, result: Result<Arc<PreviewOut>, String> },     // Task 15
    Panicked { what: &'static str, message: String },
}
pub struct ChannelSink(pub Sender<WorkerMsg>);           // EventSink: forwards every event
pub fn spawn_guarded(tx: Sender<WorkerMsg>, what: &'static str, f: impl FnOnce(&Sender<WorkerMsg>) + Send + 'static);
pub fn spawn_plan(tx: Sender<WorkerMsg>, session: Arc<Session>, req: BatchRequest, cancel: CancelToken, generation: u64, root: PathBuf);
```

- Produces (`sdc_gui::model::AppModel`, the parts later tasks extend):

```rust
pub enum Selection { None, Item(String), Set(String) }
pub struct Filter { pub needs_decision: bool, pub warnings: bool, pub failed: bool, pub not_in_set: bool }
pub enum Dialog { ProceedAnyway(usize), ExternalChange, CloseUnsaved, Error(String) }
pub enum Answer { Subtype(Subtype), Alpha(AlphaPolicy), Green(GreenConvention), Acknowledge(Code) }
impl AppModel {
    pub fn new(settings: GuiSettings, settings_path: Option<PathBuf>, session: Arc<Session>) -> AppModel;
    pub fn placeholder(settings: GuiSettings) -> AppModel;          // no session: draws, never plans
    pub fn open_folder(&mut self, root: PathBuf);
    pub fn poll(&mut self) -> bool;                                 // drain worker messages; true when state changed
    pub fn busy(&self) -> bool;
    pub fn wait_idle(&mut self, timeout: Duration) -> bool;         // tests: poll until !busy()
    pub fn visible_items(&self) -> Vec<usize>;
    pub fn undecided(&self) -> usize;
    pub fn item(&self, key: &str) -> Option<&TextureItem>;
    pub fn select_item(&mut self, key: &str); pub fn select_set(&mut self, key: &str);
    pub fn set_field(&mut self, item_key: &str, field: &str, value: serde_json::Value);
    pub fn clear_field(&mut self, item_key: &str, field: &str);
    pub fn decide(&mut self, item_key: &str, answer: Answer);
    pub fn set_target(&mut self, t: Target);
    pub fn set_fill(&mut self, role: RawMap, value: Option<u8>);
    pub fn save(&mut self) -> Result<(), SaveError>;                // ChangedOnDisk opens Dialog::ExternalChange
    pub fn resolve_external_change(&mut self, overwrite: bool);     // true: save(force); false: reload, dropping edits
    pub fn request_close(&mut self) -> bool;                        // false: Dialog::CloseUnsaved opened
    pub fn expand(&mut self, key: &str); pub fn collapse(&mut self); // Task 15 hangs the preview off these
    pub fn export_root(&self) -> PathBuf;                           // settings.export_folder or <root>/sdc-out
    pub fn request(&self) -> Option<BatchRequest>;                  // what the next plan uses
}
```

Fields later tasks read: `settings`, `root`, `project: Option<ProjectDoc>`, `target: Target`, `items`, `sets`, `unmatched_keys`, `selection`, `filter`, `search: String`, `status_line: String`, `banner: Option<String>`, `dialog: Option<Dialog>`, `expanded: Option<String>`, `session: Option<Arc<Session>>`.

- [ ] **Step 1: Shared fixture** `crates/gui/tests/support/mod.rs`:

```rust
#![allow(dead_code)]
//! A folder like a small mod: a diffuse with varying alpha (needs an alpha decision), a normal with no stated green
//! convention, a 48 px NPOT diffuse (blocked by DIM-NPOT), a plain diffuse, and roughness + AO that pack `_rmaos`.
use image::{ImageBuffer, Luma, Rgb, Rgba};
use sdc_gui::model::AppModel;
use sdc_gui::settings::GuiSettings;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

pub fn mod_folder(dir: &Path) -> PathBuf {
    let r = dir.join("textures/landscape");
    std::fs::create_dir_all(&r).unwrap();
    let grad = |x: u32, y: u32| [(x * 4) as u8, (y * 4) as u8, 90u8];
    ImageBuffer::from_fn(64, 64, |x, y| Rgb(grad(x, y))).save(r.join("rock01.png")).unwrap();
    ImageBuffer::from_fn(64, 64, |x, y| {
        let g = grad(x, y);
        Rgba([g[0], g[1], g[2], if x < 32 { 255 } else { 0 }])
    })
    .save(r.join("leaf.png"))
    .unwrap();
    ImageBuffer::from_fn(64, 64, |_, _| Rgb([128u8, 128, 255])).save(r.join("rock01_normal.png")).unwrap();
    ImageBuffer::from_fn(48, 48, |x, y| Rgb(grad(x, y))).save(r.join("odd.png")).unwrap();
    for (n, v) in [("rock01_roughness.png", 120u8), ("rock01_ao.png", 200)] {
        ImageBuffer::from_fn(64, 64, |x, _| Luma([v.wrapping_add(x as u8)])).save(r.join(n)).unwrap();
    }
    dir.join("textures")
}

/// One process-wide session: the BC1 model is process-global (spec §4.6).
pub fn session() -> Arc<engine::session::Session> {
    static S: OnceLock<Arc<engine::session::Session>> = OnceLock::new();
    S.get_or_init(|| Arc::new(engine::session::Session::new(encoders_sys::Bc1Model::Ideal).unwrap())).clone()
}

pub fn opened(dir: &Path) -> AppModel {
    let root = mod_folder(dir);
    let mut settings = GuiSettings::default();
    settings.export_folder = Some(dir.join("export"));
    let mut m = AppModel::new(settings, None, session());
    m.open_folder(root);
    assert!(m.wait_idle(Duration::from_secs(60)), "planning did not finish");
    m
}
```

(`encoders-sys` is already a dependency of `sdc-gui`; tests reach it as `encoders_sys`.)

- [ ] **Step 2: Write the failing tests** in `crates/gui/tests/model.rs`:

```rust
mod support;
use engine::codes::Code;
use engine::config::{AlphaPolicy, GreenConvention, Subtype, Target};
use sdc_gui::model::items::*;
use sdc_gui::model::*;
use std::time::Duration;
use support::*;

fn status(m: &AppModel, key: &str) -> ItemStatus {
    m.item(key).unwrap_or_else(|| panic!("no item {key}")).status.clone()
}

#[test]
fn opening_a_folder_lists_items_sets_and_decisions() {
    let d = tempfile::tempdir().unwrap();
    let m = opened(d.path());
    let keys: Vec<&str> = m.items.iter().map(|i| i.key.as_str()).collect();
    for k in [
        "landscape/rock01.png",
        "landscape/leaf.png",
        "landscape/rock01_normal.png",
        "landscape/odd.png",
        "landscape/rock01_rmaos",
        "landscape/rock01_roughness.png",
        "landscape/rock01_ao.png",
    ] {
        assert!(keys.contains(&k), "missing {k} in {keys:?}");
    }
    assert!(matches!(status(&m, "landscape/rock01_roughness.png"), ItemStatus::PackedInto(j) if j == "landscape/rock01_rmaos.dds"));
    let leaf = m.item("landscape/leaf.png").unwrap();
    assert!(leaf.decisions.iter().any(|d| matches!(d, Decision::Subtype { .. })));
    let n = m.item("landscape/rock01_normal.png").unwrap();
    assert!(n.decisions.contains(&Decision::GreenConvention));
    assert!(n.thumb.is_some());
    assert!(m.undecided() >= 3);
    let set = m.sets.iter().find(|s| s.key.ends_with("rock01")).unwrap();
    assert!(set.recipes.iter().any(|r| r.output.ends_with("rock01_rmaos.dds")));
    assert_eq!(m.target, Target::Cs);
}

#[test]
fn decisions_re_resolve_before_saving_and_save_writes_them() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.decide("landscape/rock01.png", Answer::Subtype(Subtype::Diffuse));
    m.decide("landscape/odd.png", Answer::Subtype(Subtype::Diffuse));
    m.decide("landscape/odd.png", Answer::Acknowledge(Code::DimNpot));
    m.decide("landscape/leaf.png", Answer::Subtype(Subtype::Diffuse));
    m.decide("landscape/leaf.png", Answer::Alpha(AlphaPolicy::Cutout));
    m.decide("landscape/rock01_normal.png", Answer::Subtype(Subtype::Normal));
    m.decide("landscape/rock01_normal.png", Answer::Green(GreenConvention::Directx));
    assert!(m.wait_idle(Duration::from_secs(60)));
    for k in ["landscape/rock01.png", "landscape/odd.png", "landscape/leaf.png", "landscape/rock01_normal.png"] {
        assert_eq!(status(&m, k), ItemStatus::Ready, "{k}");
    }
    let toml_path = m.root.clone().unwrap().join("sdc.toml");
    assert!(!toml_path.exists(), "nothing is written before a save");
    assert!(m.project.as_ref().unwrap().is_dirty());
    m.save().unwrap();
    let text = std::fs::read_to_string(&toml_path).unwrap();
    assert!(text.contains("landscape/odd.png") && text.contains("DIM-NPOT"));
    assert!(text.contains("green_convention"));
}

#[test]
fn filters_search_and_selection_survive_a_replan() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.filter.needs_decision = true;
    let shown: Vec<String> = m.visible_items().iter().map(|&i| m.items[i].key.clone()).collect();
    assert!(shown.contains(&"landscape/leaf.png".to_string()));
    assert!(!shown.contains(&"landscape/rock01_roughness.png".to_string()));
    m.filter.needs_decision = false;
    m.search = "LEAF".into();
    assert_eq!(m.visible_items().len(), 1);
    m.search.clear();
    m.select_item("landscape/leaf.png");
    m.set_field("landscape/leaf.png", "alpha.threshold", serde_json::json!(90));
    assert!(m.wait_idle(Duration::from_secs(60)));
    assert_eq!(m.selection, Selection::Item("landscape/leaf.png".into()));
    let job = m.item("landscape/leaf.png").unwrap().job.as_ref().unwrap();
    assert_eq!(job.profile.alpha.threshold, 90);
    assert_eq!(job.provenance["alpha.threshold"].to_string(), "project_file");
    m.clear_field("landscape/leaf.png", "alpha.threshold");
    assert!(m.wait_idle(Duration::from_secs(60)));
    assert_ne!(m.item("landscape/leaf.png").unwrap().job.as_ref().unwrap().profile.alpha.threshold, 90);
}

#[test]
fn the_mode_changes_target_and_recipes() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.set_target(Target::Cpm);
    assert!(m.wait_idle(Duration::from_secs(60)));
    assert_eq!(m.target, Target::Cpm);
    assert!(m.item("landscape/rock01_rmaos").is_none());
    assert!(m.item("landscape/rock01_m").is_some());
}

#[test]
fn an_invalid_sdc_toml_opens_read_only() {
    let d = tempfile::tempdir().unwrap();
    let root = mod_folder(d.path());
    std::fs::write(root.join("sdc.toml"), "target = [\n").unwrap();
    let mut m = sdc_gui::model::AppModel::new(Default::default(), None, session());
    m.open_folder(root.clone());
    assert!(m.wait_idle(Duration::from_secs(60)));
    assert!(m.banner.as_deref().unwrap().contains("sdc.toml"));
    assert!(!m.items.is_empty(), "the folder still plans without sdc.toml");
    m.set_field("landscape/leaf.png", "alpha.threshold", serde_json::json!(90));
    assert!(!m.project.as_ref().unwrap().is_dirty());
    assert_eq!(std::fs::read_to_string(root.join("sdc.toml")).unwrap(), "target = [\n");
}

#[test]
fn closing_with_unsaved_edits_asks_first() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    assert!(m.request_close());
    m.set_target(Target::Vanilla);
    assert!(!m.request_close());
    assert_eq!(m.dialog, Some(Dialog::CloseUnsaved));
}

#[test]
fn an_external_edit_is_not_overwritten_silently() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.set_target(Target::Vanilla);
    let p = m.root.clone().unwrap().join("sdc.toml");
    std::fs::write(&p, "# written by hand meanwhile\n").unwrap();
    assert!(m.save().is_err());
    assert_eq!(m.dialog, Some(Dialog::ExternalChange));
    m.resolve_external_change(false);
    assert!(m.wait_idle(Duration::from_secs(60)));
    assert_eq!(std::fs::read_to_string(&p).unwrap(), "# written by hand meanwhile\n");
    assert_eq!(m.target, Target::Cs);
}
```

- [ ] **Step 3: Run to see them fail**

Run: `cargo test -p sdc-gui --test model`
Expected: compile errors.

- [ ] **Step 4: Implement `worker.rs`**

```rust
//! Engine work off the UI thread (M3a spec §3): every call runs on its own thread with a fresh `CancelToken`,
//! catches panics at the thread boundary (spec §7.6) and reports through one channel the UI drains each frame.
use crate::model::items::PlanView;
use crate::model::preview::PreviewOut;
use engine::api::{BatchRequest, BatchSummary, CancelToken};
use engine::events::{EngineEvent, EventSink, Reporter, VecSink};
use engine::session::{Session, SourcePreview};
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::Arc;

pub enum WorkerMsg {
    Planned { generation: u64, result: Result<PlanView, String> },
    Event(EngineEvent),
    ExportDone(Result<BatchSummary, String>),
    Source { token: u64, result: Result<Arc<SourcePreview>, String> },
    Encoded { token: u64, result: Result<Arc<PreviewOut>, String> },
    Panicked { what: &'static str, message: String },
}

/// Forwards every engine event into the channel and nothing else, so it never slows the encoders (plan M3 carry).
pub struct ChannelSink(pub Sender<WorkerMsg>);

impl EventSink for ChannelSink {
    fn emit(&self, ev: &EngineEvent) {
        let _ = self.0.send(WorkerMsg::Event(ev.clone()));
    }
}

fn panic_text(p: &(dyn std::any::Any + Send)) -> String {
    p.downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| p.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "panic".into())
}

/// Run `f` on a new thread; a panic becomes `WorkerMsg::Panicked` and the window keeps running.
pub fn spawn_guarded(tx: Sender<WorkerMsg>, what: &'static str, f: impl FnOnce(&Sender<WorkerMsg>) + Send + 'static) {
    std::thread::Builder::new()
        .name(format!("sdc-gui-{what}"))
        .spawn(move || {
            if let Err(p) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(&tx))) {
                let _ = tx.send(WorkerMsg::Panicked { what, message: panic_text(&*p) });
            }
        })
        .expect("spawn a worker thread");
}

pub fn spawn_plan(
    tx: Sender<WorkerMsg>,
    session: Arc<Session>,
    req: BatchRequest,
    cancel: CancelToken,
    generation: u64,
    root: PathBuf,
) {
    spawn_guarded(tx, "plan", move |tx| {
        let sink = Arc::new(VecSink::default());
        let rep = Reporter::new(sink.clone(), engine::events::new_batch_id());
        let result = session
            .plan(req, &rep, &cancel)
            .map(|plan| PlanView::build(&plan, &sink.take(), &session, &root))
            .map_err(|e| e.to_string());
        let _ = tx.send(WorkerMsg::Planned { generation, result });
    });
}
```

The status line during planning (M3a spec §5.1): the engine emits no per-file event while it loads and analyses (per-stage events are M7), so the line reads `Analysing {n} files…` with `n` the number of `file_discovered` events, and thumbnails appear when the plan finishes. To send `n`, `spawn_plan` wraps its `VecSink` in a sink that also counts `file_discovered` and, on `batch_started` (which carries `inputs`), sends `WorkerMsg::PlanProgress { generation, total: inputs }`; `AppModel::handle` sets `status_line = format!("Analysing {total} files…")` for the current generation. Add the variant `PlanProgress { generation: u64, total: usize }` to `WorkerMsg` (both listings above).

`crate::model::preview::PreviewOut` arrives in Task 15; until then declare in `model/preview.rs`:

```rust
//! Preview state (Task 15 fills it in).
pub struct PreviewOut {
    pub result: engine::session::PreviewResult,
}
```

- [ ] **Step 5: Implement `model/items.rs`**

```rust
//! What the columns show, built from a plan and its events (M3a spec §2.2–2.3).
use engine::api::Plan;
use engine::codes::{Code, Severity, Warning};
use engine::config::{Subtype, Target};
use engine::discover::{InputFile, InputKind};
use engine::events::{EngineEvent, EventKind, RecipeReport};
use engine::manifest::output_key;
use engine::overrides::project_rel;
use engine::policy::{JobStatus, Need, ResolvedJob};
use engine::session::{Session, Thumbnail};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    Image,
    Packed,
    Dds,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ItemStatus {
    Ready,
    /// Unresolved or blocked: the amber border (M3a spec §2.2).
    NeedsDecision,
    Error(Vec<Code>),
    Failed(String),
    Skipped(String),
    /// A raw map consumed by a recipe: the packed output's job key.
    PackedInto(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Subtype { suggested: Option<Subtype> },
    AlphaMeaning,
    GreenConvention,
    Acknowledge(Code),
}

#[derive(Debug, Clone)]
pub struct TextureItem {
    pub key: String,
    pub job_key: String,
    pub input: PathBuf,
    pub name: String,
    pub set_key: Option<String>,
    pub kind: ItemKind,
    pub job: Option<ResolvedJob>,
    pub status: ItemStatus,
    pub decisions: Vec<Decision>,
    pub warnings: Vec<Warning>,
    pub thumb: Option<Thumbnail>,
    pub changed_on_disk: bool,
}

#[derive(Debug, Clone)]
pub struct SetItem {
    pub key: String,
    pub members: Vec<String>,
    pub recipes: Vec<RecipeReport>,
    pub pbr: bool,
}

#[derive(Debug, Clone)]
pub struct PlanView {
    pub target: Target,
    pub items: Vec<TextureItem>,
    pub sets: Vec<SetItem>,
    pub unmatched: Vec<String>,
}

/// The `[files]` key of a path under `root` (DECISIONS N2).
fn key_of(root: &Path, path: &Path) -> String {
    let f = InputFile {
        path: path.to_path_buf(),
        rel: PathBuf::from(path.file_name().unwrap_or_default()),
        game_path: String::new(),
        output: PathBuf::new(),
        kind: InputKind::Image,
    };
    project_rel(root, &f).unwrap_or_else(|| path.to_string_lossy().replace('\\', "/"))
}

/// A packed job's key: its first source's folder plus the output stem, without extension.
fn packed_key(root: &Path, job: &ResolvedJob) -> String {
    let src = key_of(root, &job.input);
    let dir = src.rsplit_once('/').map_or(String::new(), |(d, _)| format!("{d}/"));
    format!("{dir}{}", job.output.file_stem().unwrap_or_default().to_string_lossy())
}

pub fn decisions_of(job: &ResolvedJob) -> Vec<Decision> {
    let mut out = vec![];
    if let JobStatus::Unresolved { needs, .. } = &job.status {
        for n in needs {
            out.push(match n {
                Need::Subtype => Decision::Subtype { suggested: job.suggestions.first().map(|s| s.subtype) },
                Need::AlphaMeaning => Decision::AlphaMeaning,
                Need::GreenConvention => Decision::GreenConvention,
            });
        }
    }
    for w in &job.warnings {
        if w.severity == Severity::Blocking && !matches!(w.code, Code::SugLowConfidence | Code::InGreenUnstated) {
            out.push(Decision::Acknowledge(w.code));
        }
    }
    out
}

impl PlanView {
    pub fn build(plan: &Plan, events: &[EngineEvent], session: &Session, root: &Path) -> PlanView {
        let out_root = &plan.request.output_root;
        let mut items = vec![];
        for job in &plan.jobs {
            let packed = job.pack.is_some();
            let key = if packed { packed_key(root, job) } else { key_of(root, &job.input) };
            let status = match &job.status {
                JobStatus::Ready => ItemStatus::Ready,
                JobStatus::Unresolved { .. } | JobStatus::Blocked { .. } => ItemStatus::NeedsDecision,
                JobStatus::Error { codes } => ItemStatus::Error(codes.clone()),
                JobStatus::PackingInput { role } => ItemStatus::Skipped(format!("raw {} map, not packed", role.id())),
            };
            let name = if packed {
                format!("{}", job.output.file_stem().unwrap_or_default().to_string_lossy())
            } else {
                job.input.file_name().unwrap_or_default().to_string_lossy().into_owned()
            };
            items.push(TextureItem {
                key,
                job_key: output_key(out_root, &job.output),
                input: job.input.clone(),
                name,
                set_key: Some(job.set_key.clone()),
                kind: if packed { ItemKind::Packed } else { ItemKind::Image },
                decisions: decisions_of(job),
                warnings: job.warnings.clone(),
                thumb: session.thumbnail(job),
                job: Some(job.clone()),
                status,
                changed_on_disk: false,
            });
            if let Some(pk) = &job.pack {
                for s in &pk.sources {
                    items.push(TextureItem {
                        key: key_of(root, &s.path),
                        job_key: String::new(),
                        input: s.path.clone(),
                        name: s.path.file_name().unwrap_or_default().to_string_lossy().into_owned(),
                        set_key: Some(job.set_key.clone()),
                        kind: ItemKind::Image,
                        job: None,
                        status: ItemStatus::PackedInto(output_key(out_root, &job.output)),
                        decisions: vec![],
                        warnings: vec![],
                        thumb: session.cache().thumbnail_of(&s.path),
                        changed_on_disk: false,
                    });
                }
            }
        }
        for s in &plan.skipped {
            let dds = s.code == Some(Code::InDdsSkipped);
            items.push(TextureItem {
                key: key_of(root, &s.path),
                job_key: output_key(out_root, &s.output),
                input: s.path.clone(),
                name: s.path.file_name().unwrap_or_default().to_string_lossy().into_owned(),
                set_key: s.set.clone(),
                kind: if dds { ItemKind::Dds } else { ItemKind::Image },
                job: None,
                status: ItemStatus::Skipped(s.reason.clone()),
                decisions: vec![],
                warnings: vec![],
                thumb: None,
                changed_on_disk: false,
            });
        }
        for f in &plan.failed {
            items.push(TextureItem {
                key: key_of(root, &f.path),
                job_key: output_key(out_root, &f.output),
                input: f.path.clone(),
                name: f.path.file_name().unwrap_or_default().to_string_lossy().into_owned(),
                set_key: None,
                kind: ItemKind::Image,
                job: None,
                status: ItemStatus::Failed(f.error.clone()),
                decisions: vec![],
                warnings: vec![],
                thumb: None,
                changed_on_disk: false,
            });
        }
        // De-duplicate by key (a raw source can feed two recipes): keep the first.
        let mut seen = std::collections::BTreeSet::new();
        items.retain(|i| seen.insert(i.key.clone()));
        items.sort_by(|a, b| a.key.to_lowercase().cmp(&b.key.to_lowercase()));

        let by_input: BTreeMap<String, Vec<String>> = items.iter().fold(BTreeMap::new(), |mut m, i| {
            m.entry(i.input.to_string_lossy().replace('\\', "/")).or_default().push(i.key.clone());
            m
        });
        let mut sets = vec![];
        let mut unmatched = vec![];
        for e in events {
            match &e.kind {
                EventKind::SetResolved { members, pbr, recipes, .. } => {
                    let key = e.set.clone().unwrap_or_default();
                    let mut keys: Vec<String> =
                        members.iter().flat_map(|m| by_input.get(m).cloned().unwrap_or_default()).collect();
                    keys.extend(items.iter().filter(|i| i.kind == ItemKind::Packed && i.set_key.as_deref() == Some(&key)).map(|i| i.key.clone()));
                    keys.sort();
                    keys.dedup();
                    sets.push(SetItem { key, members: keys, recipes: recipes.clone(), pbr: *pbr });
                }
                EventKind::Warning(w) if w.code == Code::CfgFileUnmatched => {
                    if let Some(k) = w.data["key"].as_str() {
                        unmatched.push(k.to_string());
                    }
                }
                _ => {}
            }
        }
        PlanView { target: plan.target, items, sets, unmatched }
    }
}
```

(`RawMap::id()` exists in `config.rs`; `SetResolved`'s `members` are '/'-separated input paths.)

- [ ] **Step 6: Implement `model/mod.rs`**

```rust
//! The view-model (M3a spec §3): all GUI state and logic, no egui types, testable without a window.
pub mod items;
pub mod preview;

use crate::project::{ProjectDoc, SaveError};
use crate::settings::GuiSettings;
use crate::worker::{spawn_plan, WorkerMsg};
use engine::api::{BatchRequest, CancelToken};
use engine::codes::Code;
use engine::config::{AlphaPolicy, GreenConvention, RawMap, Subtype, Target};
use engine::policy::AcceptSuggestions;
use engine::presets::ConfigFile;
use engine::session::Session;
use items::*;
use serde_json::json;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Selection {
    None,
    Item(String),
    Set(String),
}

#[derive(Debug, Clone, Default)]
pub struct Filter {
    pub needs_decision: bool,
    pub warnings: bool,
    pub failed: bool,
    pub not_in_set: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Dialog {
    ProceedAnyway(usize),
    ExternalChange,
    CloseUnsaved,
    Error(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    Subtype(Subtype),
    Alpha(AlphaPolicy),
    Green(GreenConvention),
    Acknowledge(Code),
}

pub struct AppModel {
    pub settings: GuiSettings,
    pub settings_path: Option<PathBuf>,
    /// `%APPDATA%\sdc\config.toml`, set by `main`; `None` in tests so a developer's own config cannot leak in.
    pub user_config: Option<PathBuf>,
    pub session: Option<Arc<Session>>,
    pub root: Option<PathBuf>,
    pub project: Option<ProjectDoc>,
    pub target: Target,
    pub items: Vec<TextureItem>,
    pub sets: Vec<SetItem>,
    pub unmatched_keys: Vec<String>,
    pub selection: Selection,
    pub filter: Filter,
    pub search: String,
    pub status_line: String,
    pub banner: Option<String>,
    pub dialog: Option<Dialog>,
    pub expanded: Option<String>,
    generation: u64,
    planning: Option<CancelToken>,
    tx: Sender<WorkerMsg>,
    rx: Receiver<WorkerMsg>,
}

impl AppModel {
    pub fn new(settings: GuiSettings, settings_path: Option<PathBuf>, session: Arc<Session>) -> AppModel {
        let mut m = AppModel::placeholder(settings);
        m.settings_path = settings_path;
        m.session = Some(session);
        m
    }

    pub fn placeholder(settings: GuiSettings) -> AppModel {
        let (tx, rx) = channel();
        AppModel {
            settings,
            settings_path: None,
            user_config: None,
            session: None,
            root: None,
            project: None,
            target: Target::Cs,
            items: vec![],
            sets: vec![],
            unmatched_keys: vec![],
            selection: Selection::None,
            filter: Filter::default(),
            search: String::new(),
            status_line: String::new(),
            banner: None,
            dialog: None,
            expanded: None,
            generation: 0,
            planning: None,
            tx,
            rx,
        }
    }

    pub fn open_folder(&mut self, root: PathBuf) {
        let project = ProjectDoc::open(&root);
        self.banner = project.error().map(|e| format!("{e} — sdc.toml is ignored and the GUI is read-only until it is fixed and reloaded"));
        self.project = Some(project);
        self.settings.push_recent(root.clone());
        self.save_settings();
        self.root = Some(root);
        self.items.clear();
        self.sets.clear();
        self.selection = Selection::None;
        self.expanded = None;
        self.replan();
    }

    pub fn save_settings(&self) {
        if let Some(p) = &self.settings_path {
            let _ = self.settings.save(p);
        }
    }

    /// `<export folder>` from the settings, else `<root>/sdc-out`.
    pub fn export_root(&self) -> PathBuf {
        self.settings.export_folder.clone().unwrap_or_else(|| {
            self.root.clone().unwrap_or_default().join("sdc-out")
        })
    }

    /// What the next plan uses: the folder, recursive, nothing accepted, the in-memory `sdc.toml` (DECISIONS N4).
    pub fn request(&self) -> Option<BatchRequest> {
        let root = self.root.clone()?;
        let project = self.project.as_ref()?;
        let mut req = BatchRequest::new(vec![root], self.export_root(), Target::Cs);
        req.target = None;
        req.recursive = true;
        req.accept = AcceptSuggestions::None;
        req.project_config_inline = Some(if project.read_only() {
            ConfigFile::default()
        } else {
            project.config().unwrap_or_default()
        });
        req.user_config = self.user_config.clone();
        req.jobs = self.settings.jobs;
        req.mem_budget = self.settings.mem_budget_mb * 1024 * 1024;
        Some(req)
    }

    /// Plan again on a worker; a newer plan supersedes (cancels) an older one.
    pub fn replan(&mut self) {
        let (Some(session), Some(req), Some(root)) = (self.session.clone(), self.request(), self.root.clone()) else {
            return;
        };
        if let Some(c) = self.planning.take() {
            c.cancel();
        }
        self.generation += 1;
        let cancel = CancelToken::default();
        self.planning = Some(cancel.clone());
        self.status_line = "Planning…".into();
        spawn_plan(self.tx.clone(), session, req, cancel, self.generation, root);
    }

    pub fn busy(&self) -> bool {
        self.planning.is_some()
    }

    /// Drain worker messages. True when anything changed (the UI repaints).
    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        while let Ok(msg) = self.rx.try_recv() {
            changed = true;
            self.handle(msg);
        }
        changed
    }

    fn handle(&mut self, msg: WorkerMsg) {
        match msg {
            WorkerMsg::Planned { generation, result } if generation == self.generation => {
                self.planning = None;
                match result {
                    Ok(view) => self.apply_plan(view),
                    Err(e) => {
                        self.status_line = format!("Planning failed: {e}");
                        self.dialog = Some(Dialog::Error(e));
                    }
                }
            }
            WorkerMsg::Planned { .. } => {}
            WorkerMsg::Panicked { what, message } => {
                if what == "plan" {
                    self.planning = None;
                }
                self.dialog = Some(Dialog::Error(format!("internal error in {what}: {message}")));
            }
            other => self.handle_more(other),
        }
    }

    /// Messages later tasks handle (export, preview).
    fn handle_more(&mut self, _msg: WorkerMsg) {}

    fn apply_plan(&mut self, view: PlanView) {
        self.target = view.target;
        self.items = view.items;
        self.sets = view.sets;
        self.unmatched_keys = view.unmatched;
        self.status_line = format!(
            "{} textures, {} sets, {} need a decision",
            self.items.len(),
            self.sets.len(),
            self.undecided()
        );
        // Keep the selection when its item still exists; else fall back to its set, else nothing.
        if let Selection::Item(k) = &self.selection {
            if self.item(k).is_none() {
                self.selection = Selection::None;
            }
        }
        if let Some(k) = self.expanded.clone() {
            if self.item(&k).is_none() {
                self.expanded = None;
            }
        }
    }

    pub fn wait_idle(&mut self, timeout: Duration) -> bool {
        let end = Instant::now() + timeout;
        loop {
            self.poll();
            if !self.busy() {
                return true;
            }
            if Instant::now() > end {
                return false;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    pub fn item(&self, key: &str) -> Option<&TextureItem> {
        self.items.iter().find(|i| i.key == key)
    }

    pub fn undecided(&self) -> usize {
        self.items.iter().filter(|i| i.status == ItemStatus::NeedsDecision).count()
    }

    pub fn visible_items(&self) -> Vec<usize> {
        let q = self.search.to_lowercase();
        let in_set = |i: &TextureItem| {
            i.set_key.as_ref().is_some_and(|k| self.sets.iter().any(|s| &s.key == k && s.members.len() > 1))
        };
        self.items
            .iter()
            .enumerate()
            .filter(|(_, i)| q.is_empty() || i.key.to_lowercase().contains(&q))
            .filter(|(_, i)| !self.filter.needs_decision || i.status == ItemStatus::NeedsDecision)
            .filter(|(_, i)| !self.filter.warnings || !i.warnings.is_empty())
            .filter(|(_, i)| !self.filter.failed || matches!(i.status, ItemStatus::Failed(_) | ItemStatus::Error(_)))
            .filter(|(_, i)| !self.filter.not_in_set || !in_set(i))
            .map(|(n, _)| n)
            .collect()
    }

    pub fn select_item(&mut self, key: &str) {
        self.selection = Selection::Item(key.to_string());
    }
    pub fn select_set(&mut self, key: &str) {
        self.selection = Selection::Set(key.to_string());
    }
    pub fn expand(&mut self, key: &str) {
        self.select_item(key);
        self.expanded = Some(key.to_string());
    }
    pub fn collapse(&mut self) {
        self.expanded = None;
    }

    fn edit(&mut self, f: impl FnOnce(&mut ProjectDoc)) {
        let Some(p) = self.project.as_mut() else { return };
        if p.read_only() {
            return;
        }
        f(p);
        self.replan();
    }

    pub fn set_field(&mut self, item_key: &str, field: &str, value: serde_json::Value) {
        let k = item_key.to_string();
        let f = field.to_string();
        self.edit(move |p| p.set_file_value(&k, &f, &value));
    }

    pub fn clear_field(&mut self, item_key: &str, field: &str) {
        let k = item_key.to_string();
        let f = field.to_string();
        self.edit(move |p| p.clear_file_value(&k, &f));
    }

    /// A confirmation writes the value explicitly, or adds the code to the file's `allow` (M3a spec §5.2).
    pub fn decide(&mut self, item_key: &str, answer: Answer) {
        let k = item_key.to_string();
        self.edit(move |p| match answer {
            Answer::Subtype(st) => p.set_file_value(&k, "subtype", &json!(st)),
            Answer::Alpha(a) => p.set_file_value(&k, "alpha.policy", &json!(a)),
            Answer::Green(g) => p.set_file_value(&k, "channels.green_convention", &json!(g)),
            Answer::Acknowledge(c) => {
                let mut codes = p.allow(&k);
                codes.insert(c);
                p.set_allow(&k, &codes);
            }
        });
    }

    pub fn set_target(&mut self, t: Target) {
        self.edit(move |p| p.set_target(t));
    }

    pub fn set_fill(&mut self, role: RawMap, value: Option<u8>) {
        self.edit(move |p| p.set_fill(role, value));
    }

    pub fn is_dirty(&self) -> bool {
        self.project.as_ref().is_some_and(ProjectDoc::is_dirty)
    }

    pub fn save(&mut self) -> Result<(), SaveError> {
        let Some(p) = self.project.as_mut() else { return Ok(()) };
        let r = p.save(false);
        match &r {
            Err(SaveError::ChangedOnDisk) => self.dialog = Some(Dialog::ExternalChange),
            Err(e) => self.dialog = Some(Dialog::Error(e.to_string())),
            Ok(()) => {}
        }
        r
    }

    pub fn resolve_external_change(&mut self, overwrite: bool) {
        self.dialog = None;
        let Some(p) = self.project.as_mut() else { return };
        if overwrite {
            if let Err(e) = p.save(true) {
                self.dialog = Some(Dialog::Error(e.to_string()));
            }
        } else {
            p.reload();
            self.banner = p.error().map(str::to_string);
            self.replan();
        }
    }

    pub fn request_close(&mut self) -> bool {
        if self.is_dirty() {
            self.dialog = Some(Dialog::CloseUnsaved);
            false
        } else {
            true
        }
    }
}
```

In `main.rs`, build the real model: `Session::new(settings.bc1_decoder.model())`, then `AppModel::new(settings, settings_path, Arc::new(session))` and set `model.user_config` to `%APPDATA%\sdc\config.toml`; on a session error show the message in the window (`AppModel::placeholder` plus `dialog = Some(Dialog::Error(..))`).

- [ ] **Step 7: Run the tests**

Run: `cargo test -p sdc-gui`
Expected: PASS (`ui_smoke` still builds against `AppModel::placeholder`).

- [ ] **Step 8: Commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings
git add crates/gui/src crates/gui/tests/model.rs crates/gui/tests/support/mod.rs crates/gui/tests/ui_smoke.rs
git commit -m "gui: view-model, plan view and worker (M3a)"
```

---

### Task 12: Header and the two columns

**Files:**
- Create: `crates/gui/src/ui/header.rs`, `crates/gui/src/ui/columns.rs`, `crates/gui/src/ui/thumbs.rs`, `crates/gui/src/ui/dialogs.rs`
- Modify: `crates/gui/src/ui/mod.rs` (lay out the panels with them), `crates/gui/src/app.rs` (poll, Ctrl+S, close prompt, focus), `crates/gui/src/main.rs`
- Test: `crates/gui/tests/ui_smoke.rs`

**Interfaces:**
- Consumes: Task 11's `AppModel`.
- Produces: `ui::header::draw(ui, model)`, `ui::columns::textures(ui, model, thumbs)`, `ui::columns::sets(ui, model, thumbs)`, `ui::thumbs::ThumbCache { pub fn get(&mut self, ctx: &egui::Context, item: &TextureItem) -> Option<egui::TextureHandle> }` (keyed by item key, refreshed when the thumbnail bytes change), `ui::dialogs::draw(ctx, model)`; `ui::State { pub thumbs: ThumbCache }` and `pub fn draw_with(ctx, model, state: &mut ui::State)` (`draw` keeps its signature by using a thread-local `State` for tests).

Drawing rules (M3a spec §2):
- Header, left to right: **Open folder…** (`rfd::FileDialog::new().pick_folder()`), a **Recent** menu, **Mode** combo (`cs`, `vanilla`, `cpm`; calls `model.set_target`), **Collapse columns** toggle, then right-aligned: the **Export folder** button showing the folder (`pick_folder`, saved in settings), **Save** (enabled when dirty), **Export**. The window title is `sdc — <folder>` plus ` ●` when `model.is_dirty()` (`ctx.send_viewport_cmd(egui::ViewportCommand::Title(..))`).
- Below the header, the status line, and the banner in a warning colour when `model.banner` is set, with a **Reload** button (`model.resolve_external_change(false)`).
- Textures column: filter toggles **Needs decision**, **Has warnings**, **Failed**, **Not in a set**, a search box, then a wrapped grid of 96 px thumbnails with the file name under each. Border: amber (`egui::Color32::from_rgb(230, 160, 30)`) for `NeedsDecision`, red for `Failed`/`Error`, default otherwise; a ⚠ badge when there are warnings; "packed into <name>" small text for `PackedInto`; "changed on disk" for `changed_on_disk`. Click selects; double-click calls `model.expand(key)`. A selected item has a highlighted frame.
- Sets column: one framed "pill" per set (`egui::Frame::group` with rounding 12): the set key, member thumbnails at 32 px, and a line per recipe with its status (`run`, `conflict`, `failed`, `disabled`). Clicking the pill selects the set; clicking a member thumbnail selects that item; double-click expands it.
- Collapsed columns are 24 px strips with the column name drawn vertically.
- `app.rs`: each frame `model.poll()`; repaint after 50 ms while `model.busy()`; Ctrl+S → `model.save()`; on `close_requested` with `!model.request_close()` send `ViewportCommand::CancelClose`; on focus regained call `model.check_sources()` (Task 19 adds it; until then nothing).
- `dialogs.rs` draws `model.dialog` as an `egui::Modal`: `CloseUnsaved` (Save / Discard / Cancel: Save → `save()` then close, Discard → close without saving), `ExternalChange` (Reload / Overwrite / Cancel), `Error(msg)` (OK). `ProceedAnyway` is drawn by Task 14.

- [ ] **Step 1: Write the failing tests** — replace `crates/gui/tests/ui_smoke.rs`:

```rust
mod support;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use sdc_gui::model::AppModel;
use sdc_gui::settings::GuiSettings;
use support::*;

#[test]
fn the_empty_window_shows_its_regions() {
    let mut h = Harness::new_state(
        |ctx, model: &mut AppModel| sdc_gui::ui::draw(ctx, model),
        AppModel::placeholder(GuiSettings::default()),
    );
    h.run();
    for label in ["Open folder…", "Mode", "Export", "Save", "Textures", "Sets", "Advanced"] {
        h.get_by_label(label);
    }
    h.get_by_label_contains("Open a folder");
}

#[test]
fn an_opened_folder_shows_items_filters_and_the_undecided_count() {
    let d = tempfile::tempdir().unwrap();
    let model = opened(d.path());
    let mut h = Harness::new_state(|ctx, model: &mut AppModel| sdc_gui::ui::draw(ctx, model), model);
    h.run();
    for label in ["leaf.png", "rock01_normal.png", "rock01_rmaos", "Needs decision", "Not in a set"] {
        h.get_by_label_contains(label);
    }
    h.get_by_label_contains("need a decision");
}

#[test]
fn collapsing_the_columns_hides_the_thumbnails() {
    let d = tempfile::tempdir().unwrap();
    let mut model = opened(d.path());
    model.settings.columns_collapsed = true;
    let mut h = Harness::new_state(|ctx, model: &mut AppModel| sdc_gui::ui::draw(ctx, model), model);
    h.run();
    assert!(h.query_by_label_contains("leaf.png").is_none());
}
```

- [ ] **Step 2: Run to see them fail**

Run: `cargo test -p sdc-gui --test ui_smoke`
Expected: FAIL (labels missing).

- [ ] **Step 3: Implement** `header.rs`, `columns.rs`, `thumbs.rs`, `dialogs.rs` and the new `ui::draw` layout following the drawing rules above. `thumbs.rs`:

```rust
//! Thumbnails as egui textures, created once per item and refreshed when the bytes change.
use crate::model::items::TextureItem;
use eframe::egui;
use std::collections::HashMap;

#[derive(Default)]
pub struct ThumbCache {
    map: HashMap<String, (u64, egui::TextureHandle)>,
}

fn hash(bytes: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut h);
    h.finish()
}

impl ThumbCache {
    pub fn get(&mut self, ctx: &egui::Context, item: &TextureItem) -> Option<egui::TextureHandle> {
        let t = item.thumb.as_ref()?;
        let h = hash(&t.rgba);
        if let Some((old, tex)) = self.map.get(&item.key) {
            if *old == h {
                return Some(tex.clone());
            }
        }
        let img = egui::ColorImage::from_rgba_unmultiplied([t.width as usize, t.height as usize], &t.rgba);
        let tex = ctx.load_texture(format!("thumb:{}", item.key), img, egui::TextureOptions::LINEAR);
        self.map.insert(item.key.clone(), (h, tex.clone()));
        Some(tex)
    }
}
```

A thumbnail tile in `columns.rs`:

```rust
fn tile(ui: &mut egui::Ui, model: &mut AppModel, thumbs: &mut ThumbCache, idx: usize, size: f32) {
    let item = &model.items[idx];
    let border = match item.status {
        ItemStatus::NeedsDecision => egui::Color32::from_rgb(230, 160, 30),
        ItemStatus::Failed(_) | ItemStatus::Error(_) => egui::Color32::from_rgb(210, 60, 60),
        _ => ui.visuals().widgets.noninteractive.bg_stroke.color,
    };
    let selected = model.selection == Selection::Item(item.key.clone());
    let key = item.key.clone();
    let resp = egui::Frame::default()
        .stroke(egui::Stroke::new(if selected { 3.0 } else { 2.0 }, border))
        .inner_margin(4.0)
        .show(ui, |ui| {
            ui.set_width(size);
            ui.vertical_centered(|ui| {
                match thumbs.get(ui.ctx(), item) {
                    Some(t) => {
                        ui.add(egui::Image::new(&t).fit_to_exact_size(egui::vec2(size, size)));
                    }
                    None => {
                        ui.allocate_space(egui::vec2(size, size));
                    }
                }
                ui.label(egui::RichText::new(&item.name).small());
                if !item.warnings.is_empty() {
                    ui.label("⚠");
                }
                if let ItemStatus::PackedInto(j) = &item.status {
                    ui.label(egui::RichText::new(format!("packed into {j}")).small().weak());
                }
                if item.changed_on_disk {
                    ui.label(egui::RichText::new("changed on disk").small());
                }
            });
        })
        .response
        .interact(egui::Sense::click());
    if resp.double_clicked() {
        model.expand(&key);
    } else if resp.clicked() {
        model.select_item(&key);
    }
}
```

The undecided count shows in the Textures column header as `"{n} need a decision"` (amber) when `n > 0`.

- [ ] **Step 4: Run the tests and look at the window**

Run: `cargo test -p sdc-gui` then `cargo run -p sdc-gui` and open `F:/ai/dds/mvp-test/src` (read-only use; nothing is written until you save or export). Check the three regions and the amber borders, then close the window.
Expected: tests PASS; the window shows the textures with borders.

- [ ] **Step 5: Commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings
git add crates/gui/src/ui crates/gui/src/app.rs crates/gui/src/main.rs crates/gui/tests/ui_smoke.rs
git commit -m "gui: header, texture and set columns, dialogs (M3a)"
```

---

### Task 13: The centre — texture settings, confirm controls and the set view

**Files:**
- Create: `crates/gui/src/model/fields.rs` (pure field logic), `crates/gui/src/ui/settings_panel.rs`, `crates/gui/src/ui/set_view.rs`
- Modify: `crates/gui/src/model/mod.rs` (`pub mod fields;`), `crates/gui/src/ui/mod.rs` (centre panel)
- Test: `crates/gui/tests/fields.rs`, `crates/gui/tests/ui_smoke.rs`

**Interfaces:**
- Consumes: Task 11's `AppModel::{set_field, clear_field, decide}`, `TextureItem::{job, decisions}`.
- Produces (`sdc_gui::model::fields`):
  - `pub fn format_label(format: Format, tag: ColorTag) -> String` — `"BC7 · sRGB"`, `"BC7 · linear"`, `"BC1 · sRGB"`, `"BC3 · linear"`, `"BC4 · linear (one channel)"`, `"BC5 · linear (two channels)"`, `"BC6H · HDR linear"`; the tag `n/a` reads as linear.
  - `pub fn format_options(target: Target) -> Vec<Format>` — every format, BC6H only under `cs`.
  - `pub fn field_lock(target: Target, p: &Profile, field: &str) -> Option<&'static str>` — why a field cannot be changed, or `None`:
    - `"color.tag"`: `"_SRGB is only written under cs"` when `target != Cs`; `"BC6H has no _SRGB form"` when the format is BC6H.
    - `"encoder.perceptual"`: `"perceptual BC7 is only for colour without data alpha"` unless the category is `Color`, the alpha policy is not `data` and the format is BC7.
    - `"alpha.threshold"` and `"alpha.compare"`: `"used only by cutout and cutout + blend"` unless the policy is `cutout` or `cutout_blend`.
    - `"mips.normal"`: `"normal maps only"` unless the category is `Normal`.
  - `pub struct FieldView { pub field: &'static str, pub label: &'static str, pub value: serde_json::Value, pub provenance: String, pub edited: bool, pub lock: Option<&'static str> }` and `pub fn field_views(target: Target, job: &ResolvedJob) -> Vec<FieldView>` — one per editable field, grouped in this order: Type (`subtype`), Colour (`color.content`, `color.tag`), Alpha (`alpha.policy`, `alpha.threshold`, `alpha.compare`, `alpha.exact_opaque`), Mipmaps (`mips.levels`, `mips.normal`, `mips.coverage`), Compression (`format`, `encoder.effort`, `encoder.perceptual`); `edited` is true when the provenance is `project_file`.
  - `pub fn group_of(field: &str) -> &'static str` (`"Type"`, `"Colour"`, `"Alpha"`, `"Mipmaps"`, `"Compression"`).
  - Field labels (M3a spec §2.4, main spec §9 #2): `color.content` → **"Content"** with options **"sRGB-encoded colour"** / **"Linear data"**; `color.tag` → **"DDS tag"** with options **"_SRGB"** / **"UNORM (linear)"**; alpha policies **Remove / Cutout / Blend / Cutout + Blend / Data**.

Drawing rules:
- Texture selected: the item name and its job key; then, if it has decisions, an amber box **"Needs a decision"** with one control per decision: `Subtype` → a combo of `Subtype::ALL` (available ones under the target) with the suggestion preselected and a **Confirm** button (`Answer::Subtype`); `AlphaMeaning` → five buttons Remove / Cutout / Blend / Cutout + Blend / Data (`Answer::Alpha`), with the job's `SUG-LOW-CONFIDENCE` message shown above them (it carries the proposal); `GreenConvention` → **OpenGL** / **DirectX** buttons (`Answer::Green`); `Acknowledge(code)` → the code, its message, and **Acknowledge for this texture** (`Answer::Acknowledge`).
- Then the groups as `egui::CollapsingHeader`s, open by default. Each field row: label, an editor (combo for enums, slider 5–200 for `alpha.threshold`, checkbox for booleans), the provenance in small text (`from preset cs-quality`, `from this texture's settings`, …), and a **↺** button when `edited` (calls `clear_field`). A locked field is disabled and shows its lock reason on hover and as small text.
- The format combo shows `format_label` for every option of `format_options(target)`, with the tag the texture resolves now.
- Packed items (`ItemKind::Packed`) also show their channels: one line per channel from the job's `pack.r` … `pack.a` provenance.
- Items without a job (`PackedInto`, `Skipped`, `Failed`, `Dds`) show their status text only (a `PackedInto` item has a **Show packed output** button that selects it).
- Set selected (`set_view.rs`): the set key; member thumbnails at 64 px with their suffix label (the item name), each clickable; each recipe with its status, reason and channel provenance list; the set's warnings (the union of its members').

- [ ] **Step 1: Write the failing tests** `crates/gui/tests/fields.rs`:

```rust
mod support;
use engine::config::*;
use engine::presets::PresetRegistry;
use sdc_gui::model::fields::*;
use support::*;

#[test]
fn format_labels_state_bc_format_and_colour_space() {
    assert_eq!(format_label(Format::Bc7, ColorTag::Srgb), "BC7 · sRGB");
    assert_eq!(format_label(Format::Bc7, ColorTag::Unorm), "BC7 · linear");
    assert_eq!(format_label(Format::Bc5, ColorTag::Unorm), "BC5 · linear (two channels)");
    assert_eq!(format_label(Format::Bc6hUf16, ColorTag::NotApplicable), "BC6H · HDR linear");
    assert!(format_options(Target::Cs).contains(&Format::Bc6hUf16));
    assert!(!format_options(Target::Vanilla).contains(&Format::Bc6hUf16));
}

#[test]
fn locks_say_why() {
    let reg = PresetRegistry::builtin().unwrap();
    let mut p = reg.base_profile(Target::Vanilla, Subtype::Diffuse);
    assert_eq!(field_lock(Target::Vanilla, &p, "color.tag"), Some("_SRGB is only written under cs"));
    p.alpha.policy = AlphaPolicy::Blend;
    assert_eq!(field_lock(Target::Vanilla, &p, "alpha.threshold"), Some("used only by cutout and cutout + blend"));
    p.alpha.policy = AlphaPolicy::Cutout;
    assert_eq!(field_lock(Target::Vanilla, &p, "alpha.threshold"), None);
    let n = reg.base_profile(Target::Cs, Subtype::Normal);
    assert_eq!(field_lock(Target::Cs, &n, "mips.normal"), None);
    assert_eq!(field_lock(Target::Cs, &p, "mips.normal"), Some("normal maps only"));
}

#[test]
fn field_views_mark_edited_fields() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.set_field("landscape/leaf.png", "alpha.threshold", serde_json::json!(90));
    assert!(m.wait_idle(std::time::Duration::from_secs(60)));
    let job = m.item("landscape/leaf.png").unwrap().job.clone().unwrap();
    let views = field_views(m.target, &job);
    let t = views.iter().find(|v| v.field == "alpha.threshold").unwrap();
    assert!(t.edited);
    assert_eq!(t.value, serde_json::json!(90));
    let groups: Vec<&str> = views.iter().map(|v| group_of(v.field)).collect();
    let mut sorted = groups.clone();
    sorted.dedup();
    assert_eq!(sorted, vec!["Type", "Colour", "Alpha", "Mipmaps", "Compression"]);
}
```

Add to `ui_smoke.rs`:

```rust
#[test]
fn selecting_a_texture_shows_its_decisions_and_groups() {
    let d = tempfile::tempdir().unwrap();
    let mut model = opened(d.path());
    model.select_item("landscape/rock01_normal.png");
    let mut h = Harness::new_state(|ctx, model: &mut AppModel| sdc_gui::ui::draw(ctx, model), model);
    h.run();
    for label in ["Needs a decision", "OpenGL", "DirectX", "Colour", "Content", "DDS tag", "Compression"] {
        h.get_by_label_contains(label);
    }
}

#[test]
fn selecting_a_set_shows_its_recipe() {
    let d = tempfile::tempdir().unwrap();
    let mut model = opened(d.path());
    let key = model.sets.iter().find(|s| s.key.ends_with("rock01")).unwrap().key.clone();
    model.select_set(&key);
    let mut h = Harness::new_state(|ctx, model: &mut AppModel| sdc_gui::ui::draw(ctx, model), model);
    h.run();
    h.get_by_label_contains("rock01_rmaos");
    h.get_by_label_contains("run");
}
```

- [ ] **Step 2: Run to see them fail**

Run: `cargo test -p sdc-gui --test fields --test ui_smoke`
Expected: FAIL.

- [ ] **Step 3: Implement `model/fields.rs`**

```rust
//! Pure field logic for the settings panel (M3a spec §2.4): labels, options, locks, provenance.
use engine::config::*;
use engine::policy::ResolvedJob;
use serde_json::Value;

pub const FIELDS: [(&str, &str); 13] = [
    ("subtype", "Sub-type"),
    ("color.content", "Content"),
    ("color.tag", "DDS tag"),
    ("alpha.policy", "Alpha"),
    ("alpha.threshold", "Threshold"),
    ("alpha.compare", "Comparison"),
    ("alpha.exact_opaque", "Exact opaque alpha"),
    ("mips.levels", "Levels"),
    ("mips.normal", "Normal handling"),
    ("mips.coverage", "Coverage"),
    ("format", "Format"),
    ("encoder.effort", "Effort"),
    ("encoder.perceptual", "Perceptual"),
];

pub fn group_of(field: &str) -> &'static str {
    match field.split('.').next().unwrap_or("") {
        "subtype" => "Type",
        "color" => "Colour",
        "alpha" => "Alpha",
        "mips" => "Mipmaps",
        _ => "Compression",
    }
}

pub fn format_label(format: Format, tag: ColorTag) -> String {
    let space = if tag == ColorTag::Srgb { "sRGB" } else { "linear" };
    match format {
        Format::Bc1 => format!("BC1 · {space}"),
        Format::Bc3 => format!("BC3 · {space}"),
        Format::Bc7 => format!("BC7 · {space}"),
        Format::Bc4 => "BC4 · linear (one channel)".into(),
        Format::Bc5 => "BC5 · linear (two channels)".into(),
        Format::Bc6hUf16 => "BC6H · HDR linear".into(),
    }
}

pub fn format_options(target: Target) -> Vec<Format> {
    [Format::Bc1, Format::Bc3, Format::Bc4, Format::Bc5, Format::Bc6hUf16, Format::Bc7]
        .into_iter()
        .filter(|f| *f != Format::Bc6hUf16 || target == Target::Cs)
        .collect()
}

pub fn field_lock(target: Target, p: &Profile, field: &str) -> Option<&'static str> {
    let cutout = matches!(p.alpha.policy, AlphaPolicy::Cutout | AlphaPolicy::CutoutBlend);
    match field {
        "color.tag" if p.format == Format::Bc6hUf16 => Some("BC6H has no _SRGB form"),
        "color.tag" if target != Target::Cs => Some("_SRGB is only written under cs"),
        "encoder.perceptual"
            if !(p.subtype.category() == Category::Color
                && p.alpha.policy != AlphaPolicy::Data
                && p.format == Format::Bc7) =>
        {
            Some("perceptual BC7 is only for colour without data alpha")
        }
        "alpha.threshold" | "alpha.compare" if !cutout => Some("used only by cutout and cutout + blend"),
        "mips.normal" if p.subtype.category() != Category::Normal => Some("normal maps only"),
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct FieldView {
    pub field: &'static str,
    pub label: &'static str,
    pub value: Value,
    pub provenance: String,
    pub edited: bool,
    pub lock: Option<&'static str>,
}

pub fn field_views(target: Target, job: &ResolvedJob) -> Vec<FieldView> {
    let profile = serde_json::to_value(&job.profile).unwrap_or_default();
    FIELDS
        .iter()
        .map(|&(field, label)| {
            let value = field.split('.').fold(Some(&profile), |v, k| v.and_then(|v| v.get(k))).cloned().unwrap_or(Value::Null);
            let provenance = job.provenance.get(field).map(ToString::to_string).unwrap_or_else(|| "builtin".into());
            FieldView {
                field,
                label,
                value,
                edited: provenance == "project_file",
                provenance,
                lock: field_lock(target, &job.profile, field),
            }
        })
        .collect()
}

/// Provenance as a reader sees it.
pub fn provenance_text(p: &str) -> String {
    match p.split_once(':') {
        _ if p == "project_file" => "from this texture's settings".into(),
        _ if p == "user" => "from the command line".into(),
        _ if p == "builtin" => "built-in default".into(),
        _ if p == "project_config" => "from sdc.toml".into(),
        _ if p == "user_config" => "from your config.toml".into(),
        _ if p == "suggestion" => "suggested".into(),
        Some(("preset", id)) => format!("from preset {id}"),
        Some(("rule", r)) => format!("from rule {r}"),
        Some((k, v)) => format!("{k}: {v}"),
        None => p.into(),
    }
}
```

(If a field's path differs in `Profile`'s JSON, for example `mips.levels`, read the real key from `schema/profile.schema.json` and use it; `FIELD_PATHS` in `engine::overrides` is the list of valid paths.)

- [ ] **Step 4: Implement `settings_panel.rs` and `set_view.rs`** following the drawing rules, and route the centre panel: `model.expanded` → the preview (Task 17; until then a label "Preview"), else `Selection::Item` → settings panel, `Selection::Set` → set view, else the "Open a folder" or "Select a texture or a set" hint. Enum editors list their serde names through `serde_json::to_value` and write the chosen value back with `model.set_field(key, field, value)`. The panel is a function `pub fn draw(ui: &mut egui::Ui, model: &mut AppModel, key: &str, compact: bool)`; `compact = true` (used by the preview's settings strip in Task 17) shows only the decision box and the **Alpha**, **Mipmaps** and **Compression** groups, without provenance text.

- [ ] **Step 5: Run the tests**

Run: `cargo test -p sdc-gui`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings
git add crates/gui/src/model/fields.rs crates/gui/src/model/mod.rs crates/gui/src/ui crates/gui/tests/fields.rs crates/gui/tests/ui_smoke.rs
git commit -m "gui: texture settings, confirm controls and set view (M3a)"
```

---

### Task 14: Export, the bottom drawer and the report

**Files:**
- Create: `crates/gui/src/model/export.rs`, `crates/gui/src/ui/drawer.rs`
- Modify: `crates/gui/src/model/mod.rs` (`pub mod export;`, `export: ExportModel` field, `handle_more` for `Event` and `ExportDone`), `crates/gui/src/worker.rs` (`spawn_export`), `crates/gui/src/ui/header.rs` (Export button), `crates/gui/src/ui/dialogs.rs` (`ProceedAnyway`), `crates/gui/src/ui/mod.rs` (bottom panel)
- Test: `crates/gui/tests/export.rs`

**Interfaces:**
- Consumes: Task 5's job keys and `SkipKind`; Task 11's model, `save`, `request`.
- Produces (`sdc_gui::model::export`):

```rust
pub enum RowState { Queued, Running, Done { passed: bool }, UpToDate, Skipped(SkipKind, String), Failed(String), Cancelled }
pub struct Row { pub job_key: String, pub path: String, pub state: RowState, pub warnings: Vec<Warning> }
pub struct ExportModel {
    pub open: bool,                          // the drawer
    pub running: Option<CancelToken>,
    pub rows: Vec<Row>,                      // in first-seen order
    pub events: Vec<EngineEvent>,            // the whole stream, for JSON Lines export
    pub summary: Option<BatchSummary>,
    pub error: Option<String>,
}
impl ExportModel {
    pub fn by_code(&self) -> BTreeMap<Code, Vec<String>>;   // code → job keys
    pub fn to_jsonl(&self) -> String;
    pub fn to_markdown(&self) -> String;
}
pub fn cli_command(root: &Path, export: &Path, bc1: Bc1Choice, jobs: usize) -> String;
pub fn export_folder_problem(root: &Path, export: &Path) -> Option<String>;
```

- Produces (`AppModel`): `pub export: ExportModel`, `pub fn export_clicked(&mut self)`, `pub fn proceed_anyway(&mut self)`, `pub fn cancel_export(&mut self)`, `pub fn select_job(&mut self, job_key: &str)`; `busy()` also counts a running export.
- Produces (`worker`): `pub fn spawn_export(tx: Sender<WorkerMsg>, session: Arc<Session>, req: BatchRequest, cancel: CancelToken)` — plans (through the cache) and runs with a `ChannelSink`, then sends `ExportDone`.

Behaviour (M3a spec §2.5, §2.6, §5.4, §5.5; Review Focus 2 and 3):
1. `export_clicked`: if `export_folder_problem` returns a message, show it as `Dialog::Error` and stop. If `undecided() > 0`, open `Dialog::ProceedAnyway(n)` and stop. Else start.
2. Start (`proceed_anyway` too): `save()` first. On any error, stop there (the dialog shows it; for `ChangedOnDisk` the External-change dialog). Then clear `export`, open the drawer, and `spawn_export` with `request()` and `on_existing = IfChanged`.
3. Events update rows by `job`: `file_discovered` → `Queued`; `file_started` → `Running`; `file_done` → `Done { passed }`; `file_skipped` with `UpToDate` → `UpToDate`, with `PackedSource` → no row, else `Skipped(kind, reason)` (shown as "skipped: needs decision" for `Unresolved` and `Blocked`); `file_failed` → `Failed`; `warning` → appended to the row's warnings. `batch_done` cancelled paths mark their rows `Cancelled`.
4. `select_job(job_key)` selects the item whose `job_key` matches (the drawer's row and warning clicks call it); the centre is not replaced.
5. `export_folder_problem`: the export folder equals the opened folder, or the opened folder is inside it → `"The export folder must not be the opened folder or contain it: sdc would not find the sources next time. Choose another folder."`; a subfolder of the opened folder is fine (discovery skips it).
6. `cli_command`: `sdc convert "<root>" --recursive -o "<export>" --bc1-decoder <cli>` plus ` --jobs <n>` when `jobs > 0`. The target and every edit come from the saved `sdc.toml`.
7. While `export.running`, live preview encoding pauses (Task 15 reads `export.running.is_some()`).

Drawer: header line with counts (converted, up to date, skipped, failed, validation failed), **Cancel** while running, **Close**, **Save report…** (`rfd` save dialog; `.jsonl` writes `to_jsonl`, `.md` writes `to_markdown`), **Copy CLI command** (`ctx.copy_text(cli_command(..))`). Then two tabs: **Files** (one row per job: state, path, warnings count; click selects) and **Warnings** (grouped by code: the code, its severity, the count, and the job keys, each clickable).

- [ ] **Step 1: Write the failing tests** `crates/gui/tests/export.rs`:

```rust
mod support;
use engine::codes::Code;
use engine::config::{AlphaPolicy, GreenConvention, Subtype};
use engine::events::SkipKind;
use sdc_gui::model::export::*;
use sdc_gui::model::*;
use sdc_gui::settings::Bc1Choice;
use std::path::Path;
use std::time::Duration;
use support::*;

fn decide_all(m: &mut AppModel) {
    m.decide("landscape/rock01.png", Answer::Subtype(Subtype::Diffuse));
    m.decide("landscape/odd.png", Answer::Subtype(Subtype::Diffuse));
    m.decide("landscape/odd.png", Answer::Acknowledge(Code::DimNpot));
    m.decide("landscape/leaf.png", Answer::Subtype(Subtype::Diffuse));
    m.decide("landscape/leaf.png", Answer::Alpha(AlphaPolicy::Cutout));
    m.decide("landscape/rock01_normal.png", Answer::Subtype(Subtype::Normal));
    m.decide("landscape/rock01_normal.png", Answer::Green(GreenConvention::Directx));
    assert!(m.wait_idle(Duration::from_secs(60)));
}

#[test]
fn undecided_textures_ask_first_and_proceed_anyway_skips_them() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    let n = m.undecided();
    assert!(n > 0);
    m.export_clicked();
    assert_eq!(m.dialog, Some(Dialog::ProceedAnyway(n)));
    assert!(m.export.running.is_none(), "nothing runs before the answer");
    m.proceed_anyway();
    assert!(m.wait_idle(Duration::from_secs(120)));
    let s = m.export.summary.clone().unwrap();
    assert!(s.converted >= 1, "the packed _rmaos is decided and converts");
    let skipped: Vec<&Row> = m
        .export
        .rows
        .iter()
        .filter(|r| matches!(r.state, RowState::Skipped(SkipKind::Unresolved | SkipKind::Blocked, _)))
        .collect();
    assert_eq!(skipped.len(), n);
    assert!(d.path().join("export/landscape/rock01_rmaos.dds").exists());
    assert!(!d.path().join("export/landscape/leaf.dds").exists());
}

#[test]
fn a_decided_folder_exports_and_the_report_groups_warnings() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    decide_all(&mut m);
    m.export_clicked();
    assert_eq!(m.dialog, None);
    assert!(m.export.open);
    assert!(m.wait_idle(Duration::from_secs(120)));
    let s = m.export.summary.clone().unwrap();
    assert_eq!(s.failed, 0);
    assert!(m.root.clone().unwrap().join("sdc.toml").exists(), "export saves first");
    assert!(m.export.rows.iter().all(|r| !r.job_key.is_empty()));
    let codes = m.export.by_code();
    assert!(codes.get(&Code::DimNpot).is_some_and(|keys| keys.contains(&"landscape/odd.dds".to_string())));
    let md = m.export.to_markdown();
    assert!(md.contains("DIM-NPOT") && md.contains("landscape/odd.dds"));
    assert_eq!(m.export.to_jsonl().lines().count(), m.export.events.len());
    m.select_job("landscape/odd.dds");
    assert_eq!(m.selection, Selection::Item("landscape/odd.png".into()));
}

#[test]
fn export_refuses_the_input_folder_or_an_ancestor() {
    let root = Path::new("C:/mods/x/textures");
    assert!(export_folder_problem(root, root).is_some());
    assert!(export_folder_problem(root, Path::new("C:/mods/x")).is_some());
    assert!(export_folder_problem(root, Path::new("C:/mods/X/Textures")).is_some(), "case-insensitive");
    assert!(export_folder_problem(root, Path::new("C:/mods/x/textures/sdc-out")).is_none());
    assert!(export_folder_problem(root, Path::new("D:/out")).is_none());
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.settings.export_folder = m.root.clone();
    m.export_clicked();
    assert!(matches!(m.dialog, Some(Dialog::Error(ref e)) if e.contains("export folder")));
    assert!(m.export.running.is_none());
}

#[test]
fn export_stops_when_the_save_fails() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    decide_all(&mut m);
    // A directory where sdc.toml should be makes the save fail.
    let toml = m.root.clone().unwrap().join("sdc.toml");
    std::fs::create_dir(&toml).unwrap();
    m.export_clicked();
    assert!(matches!(m.dialog, Some(Dialog::Error(_)) | Some(Dialog::ExternalChange)));
    assert!(m.export.running.is_none());
    assert!(m.export.summary.is_none());
    assert!(m.is_dirty());
}

#[test]
fn the_cli_command_reproduces_the_export() {
    let c = cli_command(Path::new("C:/mods/x/textures"), Path::new("D:/out dir"), Bc1Choice::Nvidia, 0);
    assert_eq!(c, r#"sdc convert "C:/mods/x/textures" --recursive -o "D:/out dir" --bc1-decoder nvidia"#);
    let c = cli_command(Path::new("C:/a"), Path::new("C:/b"), Bc1Choice::Ideal, 6);
    assert!(c.ends_with("--bc1-decoder ideal --jobs 6"));
}
```

- [ ] **Step 2: Run to see them fail**

Run: `cargo test -p sdc-gui --test export`
Expected: compile errors.

- [ ] **Step 3: Implement `model/export.rs`**

```rust
//! Export and its report (M3a spec §2.5, §2.6, §5.5).
use crate::settings::Bc1Choice;
use engine::api::{BatchSummary, CancelToken};
use engine::codes::{Code, Warning};
use engine::events::{EngineEvent, EventKind, SkipKind};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub enum RowState {
    Queued,
    Running,
    Done { passed: bool },
    UpToDate,
    Skipped(SkipKind, String),
    Failed(String),
    Cancelled,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub job_key: String,
    pub path: String,
    pub state: RowState,
    pub warnings: Vec<Warning>,
}

#[derive(Default)]
pub struct ExportModel {
    pub open: bool,
    pub running: Option<CancelToken>,
    pub rows: Vec<Row>,
    pub events: Vec<EngineEvent>,
    pub summary: Option<BatchSummary>,
    pub error: Option<String>,
}

impl ExportModel {
    fn row(&mut self, ev: &EngineEvent) -> Option<&mut Row> {
        let key = ev.job.clone()?;
        let i = match self.rows.iter().position(|r| r.job_key == key) {
            Some(i) => i,
            None => {
                self.rows.push(Row {
                    job_key: key,
                    path: ev.path.clone().unwrap_or_default(),
                    state: RowState::Queued,
                    warnings: vec![],
                });
                self.rows.len() - 1
            }
        };
        Some(&mut self.rows[i])
    }

    pub fn on_event(&mut self, ev: EngineEvent) {
        match &ev.kind {
            EventKind::FileSkipped { kind: SkipKind::PackedSource, .. } => {}
            EventKind::FileDiscovered { .. } => {
                self.row(&ev);
            }
            EventKind::FileStarted => {
                if let Some(r) = self.row(&ev) {
                    r.state = RowState::Running;
                }
            }
            EventKind::FileDone { validation, .. } => {
                let passed = validation.passed;
                if let Some(r) = self.row(&ev) {
                    r.state = RowState::Done { passed };
                }
            }
            EventKind::FileSkipped { kind, reason, .. } => {
                let state = match kind {
                    SkipKind::UpToDate => RowState::UpToDate,
                    SkipKind::Unresolved | SkipKind::Blocked => {
                        RowState::Skipped(*kind, format!("skipped: needs decision ({reason})"))
                    }
                    k => RowState::Skipped(*k, reason.clone()),
                };
                if let Some(r) = self.row(&ev) {
                    r.state = state;
                }
            }
            EventKind::FileFailed { error, .. } => {
                let e = error.clone();
                if let Some(r) = self.row(&ev) {
                    r.state = RowState::Failed(e);
                }
            }
            EventKind::Warning(w) => {
                let w = w.clone();
                if let Some(r) = self.row(&ev) {
                    if !r.warnings.contains(&w) {
                        r.warnings.push(w);
                    }
                }
            }
            EventKind::BatchDone { cancelled, .. } => {
                for c in cancelled {
                    for r in self.rows.iter_mut().filter(|r| &r.path == c) {
                        r.state = RowState::Cancelled;
                    }
                }
            }
            _ => {}
        }
        self.events.push(ev);
    }

    /// Warnings grouped by code: code → job keys (M3a spec §2.5).
    pub fn by_code(&self) -> BTreeMap<Code, Vec<String>> {
        let mut m: BTreeMap<Code, Vec<String>> = BTreeMap::new();
        for r in &self.rows {
            for w in &r.warnings {
                let keys = m.entry(w.code).or_default();
                if !keys.contains(&r.job_key) {
                    keys.push(r.job_key.clone());
                }
            }
        }
        m
    }

    pub fn to_jsonl(&self) -> String {
        self.events
            .iter()
            .filter_map(|e| serde_json::to_string(e).ok())
            .map(|l| l + "\n")
            .collect()
    }

    pub fn to_markdown(&self) -> String {
        let mut s = String::from("# sdc export report\n\n");
        if let Some(b) = &self.summary {
            s += &format!(
                "Converted {}, up to date {}, skipped {}, failed {}, validation failed {}.\n\n",
                b.converted, b.up_to_date, b.skipped, b.failed, b.validation_failed
            );
        }
        s += "## Warnings by code\n\n";
        for (code, keys) in self.by_code() {
            s += &format!("- **{}** ({}): {}\n", code.as_str(), keys.len(), keys.join(", "));
        }
        s += "\n## Files\n\n| Output | State |\n|---|---|\n";
        for r in &self.rows {
            s += &format!("| {} | {:?} |\n", r.job_key, r.state);
        }
        s
    }
}

fn norm(p: &Path) -> String {
    std::path::absolute(p)
        .unwrap_or_else(|_| p.to_path_buf())
        .to_string_lossy()
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_lowercase()
}

/// Review Focus 2: discovery skips everything under the output root, so the export folder must not be the opened
/// folder or contain it. A subfolder of the opened folder is fine.
pub fn export_folder_problem(root: &Path, export: &Path) -> Option<String> {
    let (r, e) = (norm(root), norm(export));
    (r == e || r.starts_with(&format!("{e}/"))).then(|| {
        "The export folder must not be the opened folder or contain it: sdc would not find the sources next time. \
         Choose another folder."
            .to_string()
    })
}

/// The `sdc convert` command that reproduces the export from the saved `sdc.toml` (M3a spec §2.5).
pub fn cli_command(root: &Path, export: &Path, bc1: Bc1Choice, jobs: usize) -> String {
    let q = |p: &Path| format!("\"{}\"", p.to_string_lossy().replace('\\', "/"));
    let mut c = format!("sdc convert {} --recursive -o {} --bc1-decoder {}", q(root), q(export), bc1.cli());
    if jobs > 0 {
        c += &format!(" --jobs {jobs}");
    }
    c
}
```

In `model/mod.rs`: add `pub export: export::ExportModel` (default), `busy()` → `self.planning.is_some() || self.export.running.is_some()`, and:

```rust
    pub fn export_clicked(&mut self) {
        let Some(root) = self.root.clone() else { return };
        if let Some(problem) = export::export_folder_problem(&root, &self.export_root()) {
            self.dialog = Some(Dialog::Error(problem));
            return;
        }
        let n = self.undecided();
        if n > 0 {
            self.dialog = Some(Dialog::ProceedAnyway(n));
            return;
        }
        self.start_export();
    }

    /// DECISIONS N3: undecided textures are skipped, never decided here.
    pub fn proceed_anyway(&mut self) {
        self.dialog = None;
        self.start_export();
    }

    fn start_export(&mut self) {
        if self.save().is_err() {
            return;
        }
        let (Some(session), Some(mut req)) = (self.session.clone(), self.request()) else { return };
        req.on_existing = engine::api::OnExisting::IfChanged;
        self.export = export::ExportModel { open: true, ..Default::default() };
        let cancel = CancelToken::default();
        self.export.running = Some(cancel.clone());
        crate::worker::spawn_export(self.tx.clone(), session, req, cancel);
    }

    pub fn cancel_export(&mut self) {
        if let Some(c) = &self.export.running {
            c.cancel();
        }
    }

    pub fn select_job(&mut self, job_key: &str) {
        if let Some(k) = self.items.iter().find(|i| i.job_key == job_key).map(|i| i.key.clone()) {
            self.select_item(&k);
        }
    }
```

`handle_more`: `WorkerMsg::Event(e)` → `self.export.on_event(e)`; `WorkerMsg::ExportDone(r)` → `self.export.running = None`, store `summary` or `error` (and `Dialog::Error` on error). `WorkerMsg::Panicked { what: "export", .. }` also clears `export.running`.

`worker.rs`:

```rust
pub fn spawn_export(tx: Sender<WorkerMsg>, session: Arc<Session>, req: BatchRequest, cancel: CancelToken) {
    spawn_guarded(tx, "export", move |tx| {
        let rep = Reporter::new(Arc::new(ChannelSink(tx.clone())), engine::events::new_batch_id());
        let result = session
            .plan(req, &rep, &cancel)
            .and_then(|plan| session.run(plan, &rep, &cancel))
            .map_err(|e| e.to_string());
        let _ = tx.send(WorkerMsg::ExportDone(result));
    });
}
```

- [ ] **Step 4: Drawer, dialog and header.** `ui/drawer.rs` draws `model.export` in an `egui::TopBottomPanel::bottom("report")` shown while `export.open`, following the drawer rules above. `dialogs.rs` draws `ProceedAnyway(n)`: text `"{n} textures still need a decision."`, a second line `"Proceed anyway exports the decided textures and skips these; nothing is decided for you."`, buttons **Proceed anyway** (`model.proceed_anyway()`) and **Cancel** (closes; undecided stay highlighted). The header's **Export** button calls `model.export_clicked()` and is disabled while an export runs or no folder is open.

- [ ] **Step 5: Run the tests**

Run: `cargo test -p sdc-gui`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings
git add crates/gui/src/model crates/gui/src/worker.rs crates/gui/src/ui crates/gui/tests/export.rs
git commit -m "gui: export with proceed-anyway, bottom drawer and report (M3a)"
```

---

### Task 15: The preview state machine

**Files:**
- Modify: `crates/gui/src/model/preview.rs` (full), `crates/gui/src/model/mod.rs` (`preview: PreviewModel` field; `expand`/`collapse`/`apply_plan`/`handle_more` drive it; `tick(now)`), `crates/gui/src/worker.rs` (`spawn_source`, `spawn_encode`)
- Test: `crates/gui/tests/preview_model.rs`

**Interfaces:**
- Consumes: Task 8's `Session::{preview_source, preview}`; Task 11's model; Task 14's `export.running`.
- Produces (`sdc_gui::model::preview`):

```rust
pub struct PreviewOut {
    pub result: PreviewResult,
    /// CPU decode of every encoded mip (RGBA f32), for the heatmap and the no-BC-support fallback.
    pub decoded: Vec<Vec<[f32; 4]>>,
}
pub enum SourceState { Loading, Ready(Arc<SourcePreview>), Failed(String) }
pub enum EncodedState { NotEncoded, Encoding { previous: Option<Arc<PreviewOut>> }, Done(Arc<PreviewOut>), Stale(Arc<PreviewOut>), Failed(String) }
pub struct PreviewModel {
    pub item: Option<String>,
    pub source: SourceState,
    pub encoded: EncodedState,
    pub live: bool,
    pub notice: Option<String>,
    // private: pending_since: Option<Instant>, source_pending: bool, inflight_source/encode: Option<(u64, CancelToken)>,
    //          next_token: u64, profile_sig: Option<String>
}
pub const DEBOUNCE: Duration = Duration::from_millis(300);
pub fn decode_levels(r: &PreviewResult, bc1: Bc1Model) -> Vec<Vec<[f32; 4]>>;
```

- Produces (`AppModel`): `pub preview: PreviewModel`; `pub fn tick(&mut self, now: Instant)`; `pub fn encode_now(&mut self)`; `pub fn set_live(&mut self, on: bool)` (also saved in settings); `busy()` also counts in-flight preview work.

Behaviour (M3a spec §6.3; Review Focus 1):
1. `expand(key)` sets `preview.item`, clears both states (`Loading`, `NotEncoded`), records the job's profile signature (`sha256_json(&job.profile)` plus the pack key), and requests the source at once. When live is on it also schedules an encode (`pending_since = now`).
2. After every plan (`apply_plan`): if the expanded item vanished, cancel both in-flight jobs, clear the preview, collapse, and select the item's set when it still exists (else nothing). If its profile signature changed: `Done(r)` → `Stale(r)`; request the source again (debounced) and, when live, schedule an encode (debounced).
3. `tick(now)`: when an encode is scheduled, `now - pending_since >= DEBOUNCE`, live is on and no export runs, start it (cancelling any in-flight encode; the state becomes `Encoding { previous }` where `previous` is the last `Done`/`Stale` result). A debounced source request starts the same way (also during export).
4. `encode_now()` starts an encode at once (also when live is off), unless an export runs (then `notice = "Preview encoding waits for the export to finish"`).
5. Results carry the token they were started with; a result whose token is not the latest of its kind is dropped. `Encoded` → `Done` (or `Failed(msg)`); `Source` → `Ready` (or `Failed`).
6. A job with status `Error` cannot be previewed: `notice = "This texture has an error; fix its settings to preview it"`. An item without a job (packed source, skipped, failed) previews nothing: `notice = "Nothing to preview"`. Unresolved and blocked jobs preview their current settings with `notice = "Preview of the suggested settings"`.
6a. A DDS input (`ItemKind::Dds`, M3a spec §2.2) shows only the encoded side, as stored: `expand` starts `spawn_dds(tx, path, max, token)`, which reads the file with `engine::dds::read_dds`, maps its format with `DdsPixelFormat::as_format()`, keeps the mips within the size limit, and sends `WorkerMsg::Encoded` with a `PreviewOut` whose `result.source` has no levels (`dims` from the file, `first_level` from `first_level_within`), `validation` = `ValidationSummary::default()` (derive `Default` on it in the engine if it lacks it) and `decoded` from `decode_levels`. The source side reads "DDS input: no source image". A format the GPU path cannot show (an uncompressed DDS) uses the decoded path. Add a test `a_dds_input_previews_its_own_bytes` (copy an exported `.dds` into the opened folder, re-open, expand it, settle, assert `Done` with `levels.len() > 0` and the source side empty).
7. `set_live(true)` while `Stale`/`NotEncoded` schedules an encode.

Worker:

```rust
pub fn spawn_source(tx: Sender<WorkerMsg>, session: Arc<Session>, job: ResolvedJob, max: u32, cancel: CancelToken, token: u64) {
    spawn_guarded(tx, "preview", move |tx| {
        let result = session.preview_source(&job, max, &cancel).map(Arc::new).map_err(|e| format!("{e:?}"));
        let _ = tx.send(WorkerMsg::Source { token, result });
    });
}

pub fn spawn_encode(tx: Sender<WorkerMsg>, session: Arc<Session>, job: ResolvedJob, max: u32, cancel: CancelToken, token: u64) {
    spawn_guarded(tx, "preview", move |tx| {
        let bc1 = session.bc1_model();
        let result = session
            .preview(&job, max, &cancel)
            .map(|r| Arc::new(PreviewOut { decoded: decode_levels(&r, bc1), result: r }))
            .map_err(|e| format!("{e:?}"));
        let _ = tx.send(WorkerMsg::Encoded { token, result });
    });
}
```

`decode_levels` builds a `DdsPixelFormat` from `(format, srgb)` (use the engine's mapping: `engine::dds` has the format ↔ pixel-format conversion used by `read_dds`; if none is public, write the encoded mips into `dds_bytes` and parse them with `read_dds`) and calls `engine::decode::decode_level` per mip, converting `Decoded::Ldr` to `[r/255, g/255, b/255, a/255]` and `Decoded::Hdr` to `[r, g, b, 1.0]`.

The preview size limit is `settings.preview_max`, clamped to 8192 unless `settings.allow_16k_preview`.

- [ ] **Step 1: Write the failing tests** `crates/gui/tests/preview_model.rs`:

```rust
mod support;
use engine::config::Target;
use sdc_gui::model::preview::*;
use sdc_gui::model::*;
use std::time::{Duration, Instant};
use support::*;

fn settle(m: &mut AppModel) {
    assert!(m.wait_idle(Duration::from_secs(120)));
}

#[test]
fn encoding_is_off_by_default_and_encode_now_encodes_once() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    assert!(!m.preview.live);
    m.expand("landscape/rock01_rmaos");
    settle(&mut m);
    assert!(matches!(m.preview.source, SourceState::Ready(_)));
    assert!(matches!(m.preview.encoded, EncodedState::NotEncoded));
    m.tick(Instant::now() + Duration::from_secs(5));
    settle(&mut m);
    assert!(matches!(m.preview.encoded, EncodedState::NotEncoded), "live off: nothing encodes by itself");
    m.encode_now();
    settle(&mut m);
    let EncodedState::Done(out) = &m.preview.encoded else { panic!("{:?}", "not done") };
    assert_eq!(out.result.source.first_level, 0);
    assert_eq!(out.decoded.len(), out.result.levels.len());
}

#[test]
fn an_edit_makes_the_result_stale_and_live_re_encodes_after_the_debounce() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.expand("landscape/rock01_rmaos");
    m.encode_now();
    settle(&mut m);
    m.set_field("landscape/rock01_rmaos", "encoder.effort", serde_json::json!("fast"));
    settle(&mut m);
    assert!(matches!(m.preview.encoded, EncodedState::Stale(_)));
    m.set_live(true);
    let t0 = Instant::now();
    m.tick(t0);
    assert!(!matches!(m.preview.encoded, EncodedState::Encoding { .. }), "debounced");
    m.tick(t0 + DEBOUNCE + Duration::from_millis(1));
    assert!(matches!(m.preview.encoded, EncodedState::Encoding { previous: Some(_) }));
    settle(&mut m);
    assert!(matches!(m.preview.encoded, EncodedState::Done(_)));
}

#[test]
fn mode_change_cancels_and_drops_a_preview_of_a_vanished_job() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    let set = m.item("landscape/rock01_rmaos").unwrap().set_key.clone().unwrap();
    m.expand("landscape/rock01_rmaos");
    m.encode_now();
    // Switch target while the encode may still be running: cpm packs rock01_m instead.
    m.set_target(Target::Cpm);
    settle(&mut m);
    assert!(m.item("landscape/rock01_rmaos").is_none());
    assert_eq!(m.expanded, None);
    assert_eq!(m.preview.item, None);
    assert!(matches!(m.preview.encoded, EncodedState::NotEncoded));
    assert_eq!(m.selection, Selection::Set(set));
}

#[test]
fn the_size_limit_skips_large_mips() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.settings.preview_max = 16;
    m.expand("landscape/rock01_rmaos");
    m.encode_now();
    settle(&mut m);
    let EncodedState::Done(out) = &m.preview.encoded else { panic!("not done") };
    assert_eq!(out.result.source.first_level, 2, "64 and 32 are above 16");
}

#[test]
fn items_without_a_job_preview_nothing() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.expand("landscape/rock01_roughness.png");
    assert_eq!(m.preview.notice.as_deref(), Some("Nothing to preview"));
    assert!(!m.busy(), "nothing was started");
    // An unresolved job previews its current settings.
    m.expand("landscape/leaf.png");
    assert_eq!(m.preview.notice.as_deref(), Some("Preview of the suggested settings"));
    settle(&mut m);
    assert!(matches!(m.preview.source, SourceState::Ready(_)));
}
```

(`encoder.effort = "fast"`: use any valid effort value from `schema/profile.schema.json` that differs from the default.)

- [ ] **Step 2: Run to see them fail**

Run: `cargo test -p sdc-gui --test preview_model`
Expected: compile errors.

- [ ] **Step 3: Implement** `model/preview.rs`, the worker functions and the `AppModel` hooks per the behaviour list. Keep all timing in `tick(now)` (the UI calls `model.tick(Instant::now())` each frame and requests a repaint after `DEBOUNCE` while something is scheduled), so the tests drive time explicitly. In `apply_plan`, look up the expanded item's set **before** replacing `self.items`, so the fallback selection is known.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p sdc-gui`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings
git add crates/gui/src/model crates/gui/src/worker.rs crates/gui/tests/preview_model.rs
git commit -m "gui: preview state machine, off by default with live and encode-now (M3a)"
```

---

### Task 16: The GPU preview renderer

**Files:**
- Create: `crates/gui/src/render/mod.rs`, `crates/gui/src/render/upload.rs`, `crates/gui/src/render/preview.wgsl`
- Modify: `crates/gui/src/lib.rs` (`pub mod render;`), `crates/gui/src/main.rs` (request BC compression on the device), `crates/gui/src/app.rs` (create the renderer from `CreationContext::wgpu_render_state`)
- Test: `crates/gui/tests/render.rs`

**Interfaces:**
- Consumes: Task 15's `PreviewOut`, `SourcePreview`.
- Produces (`sdc_gui::render`):
  - `pub fn wgpu_format(format: Format) -> wgpu::TextureFormat` — `Bc1RgbaUnorm`, `Bc3RgbaUnorm`, `Bc4RUnorm`, `Bc5RgUnorm`, `Bc6hRgbUfloat`, `Bc7RgbaUnorm` (always UNORM: the shader decides how to show the bytes, §6.1).
  - `pub fn block_layout(format: Format, w: u32, h: u32) -> (u32 /* bytes per row */, u32 /* rows */)` — `ceil(w/4) * block_bytes`, `ceil(h/4)`.
  - `pub fn can_upload_bc(supports_bc: bool, max_dim: u32, first: (u32, u32)) -> bool` — BC upload needs the feature, a first mip whose sides are multiples of 4, and sides within `max_dim`.
  - `pub fn to_rgba16f(px: &[[f32; 4]]) -> Vec<u8>` — little-endian `f16` RGBA.
  - `#[derive(Clone, Copy, PartialEq, Eq)] pub enum Slot { Source, Encoded, Heatmap }`
  - `#[repr(C)] #[derive(Clone, Copy)] pub struct Params { pub uv_offset: [f32; 2], pub uv_scale: [f32; 2], pub mip: f32, pub channel: u32, pub view_linear: u32, pub tonemap: u32 }` — `channel`: 0 RGBA, 1 R, 2 G, 3 B, 4 A, 5 alpha as mask over a checkerboard; `view_linear`: show the bytes as linear data (re-encoded to sRGB for display); `tonemap`: Reinhard for BC6H.
  - `pub struct PreviewRenderer` with `pub fn new(rs: &egui_wgpu::RenderState) -> PreviewRenderer`, `pub fn supports_bc(&self) -> bool`, `pub fn max_dim(&self) -> u32`, `pub fn set_source(&mut self, rs, src: &SourcePreview)`, `pub fn set_encoded(&mut self, rs, out: &PreviewOut) -> UploadPath`, `pub fn clear(&mut self, slot: Slot)`; stored in `egui_wgpu::RenderState::renderer.write().callback_resources`.
  - `pub enum UploadPath { Bc, CpuDecoded(&'static str) }` — the reason text is shown as the notice (M3a spec §6.4): `"This GPU has no BC texture support: showing a CPU decode"`, `"Mip size not a multiple of 4: showing a CPU decode"`.
  - `pub fn paint(ui: &mut egui::Ui, rect: egui::Rect, slot: Slot, params: Params)` — adds an `egui_wgpu::Callback` that draws the slot's texture into `rect`.

GPU rules:
- `main.rs` requests `wgpu::Features::TEXTURE_COMPRESSION_BC` only when the adapter has it, and the adapter's own limits, through `eframe::egui_wgpu::WgpuConfiguration`'s device-descriptor hook (`WgpuSetup::CreateNew` with a `device_descriptor` closure taking the adapter). If the 0.36.2 API names differ, adapt them (Global Constraints).
- Source textures are `Rgba16Float` with the full mip chain from `first_level`; encoded textures use `wgpu_format` with `write_texture` per mip (`block_layout`); the CPU path uploads `decoded` as `Rgba16Float`.
- The shader samples one explicit mip (`textureSampleLevel`) with a nearest sampler when zoomed in (≥ 1 texel per pixel) and a linear one otherwise; for BC5 it reconstructs blue as `sqrt(1 − x² − y²)` with x, y = `2v − 1`, shown back in `[0, 1]`.
- Display: egui's target expects gamma-space colour. Show stored bytes as stored (`view_linear = 0`); with `view_linear = 1` apply the linear → sRGB encode, which is how a texture read as UNORM data looks when the game treats its values as linear light.
- `CreationContext::wgpu_render_state` is `None` when there is no usable adapter: the model's `preview.notice` becomes `"No usable GPU: the preview is off; everything else works"` (Task 19 adds the flag; here leave `render` unused in that case).

- [ ] **Step 1: Write the failing tests** `crates/gui/tests/render.rs`:

```rust
use engine::config::Format;
use sdc_gui::render::*;
use eframe::egui_wgpu::wgpu;

#[test]
fn formats_map_to_unorm_block_formats() {
    assert_eq!(wgpu_format(Format::Bc7), wgpu::TextureFormat::Bc7RgbaUnorm);
    assert_eq!(wgpu_format(Format::Bc5), wgpu::TextureFormat::Bc5RgUnorm);
    assert_eq!(wgpu_format(Format::Bc4), wgpu::TextureFormat::Bc4RUnorm);
    assert_eq!(wgpu_format(Format::Bc6hUf16), wgpu::TextureFormat::Bc6hRgbUfloat);
}

#[test]
fn block_layout_rounds_up_to_whole_blocks() {
    assert_eq!(block_layout(Format::Bc7, 64, 32), (256, 8));
    assert_eq!(block_layout(Format::Bc1, 2, 2), (8, 1));
    assert_eq!(block_layout(Format::Bc4, 1, 1), (8, 1));
}

#[test]
fn bc_upload_needs_support_whole_blocks_and_limits() {
    assert!(can_upload_bc(true, 16384, (4096, 4096)));
    assert!(!can_upload_bc(false, 16384, (4096, 4096)));
    assert!(!can_upload_bc(true, 16384, (2, 2)));
    assert!(!can_upload_bc(true, 4096, (8192, 8192)));
}

#[test]
fn rgba16f_packs_little_endian_halves() {
    let b = to_rgba16f(&[[1.0, 0.5, 0.0, 1.0]]);
    assert_eq!(b, vec![0x00, 0x3c, 0x00, 0x38, 0x00, 0x00, 0x00, 0x3c]);
}

/// Compiles the shader on a real adapter when one exists; skips (passes) without one.
#[test]
fn the_shader_compiles_on_any_available_adapter() {
    let instance = wgpu::Instance::default();
    let Some(adapter) = pollster_block(instance.request_adapter(&wgpu::RequestAdapterOptions::default())) else {
        return;
    };
    let (device, _queue) = pollster_block(adapter.request_device(&wgpu::DeviceDescriptor::default())).unwrap();
    device.push_error_scope(wgpu::ErrorFilter::Validation);
    let _ = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("preview"),
        source: wgpu::ShaderSource::Wgsl(include_str!("../src/render/preview.wgsl").into()),
    });
    assert!(pollster_block(device.pop_error_scope()).is_none());
}

fn pollster_block<F: std::future::Future>(f: F) -> F::Output {
    // A minimal executor: wgpu's native futures complete on first poll after the backend call.
    use std::task::{Context, Poll, Waker};
    let mut f = std::pin::pin!(f);
    let mut cx = Context::from_waker(Waker::noop());
    loop {
        if let Poll::Ready(v) = f.as_mut().poll(&mut cx) {
            return v;
        }
        std::thread::yield_now();
    }
}
```

(`request_adapter`'s return type changed across wgpu versions — `Option` or `Result`; adapt the `let … else` to the pinned one. Adding `pollster` as a dev-dependency instead of the helper is fine.)

- [ ] **Step 2: Run to see them fail**

Run: `cargo test -p sdc-gui --test render`
Expected: compile errors.

- [ ] **Step 3: The shader** `render/preview.wgsl`:

```wgsl
struct Params {
    uv_offset: vec2<f32>,
    uv_scale: vec2<f32>,
    mip: f32,
    channel: u32,
    view_linear: u32,
    tonemap: u32,
};

@group(0) @binding(0) var tex: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var<uniform> params: Params;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

// One triangle covering the viewport egui sets to the callback rect.
@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> VsOut {
    let xy = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var o: VsOut;
    o.pos = vec4<f32>(xy * 2.0 - 1.0, 0.0, 1.0);
    o.uv = vec2<f32>(xy.x, 1.0 - xy.y) * params.uv_scale + params.uv_offset;
    return o;
}

fn linear_to_srgb(c: vec3<f32>) -> vec3<f32> {
    let lo = c * 12.92;
    let hi = 1.055 * pow(max(c, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(hi, lo, c <= vec3<f32>(0.0031308));
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    if (any(in.uv < vec2<f32>(0.0)) || any(in.uv > vec2<f32>(1.0))) {
        return vec4<f32>(0.0, 0.0, 0.0, 0.0);
    }
    var c = textureSampleLevel(tex, samp, in.uv, params.mip);
    if (params.tonemap == 1u) {
        c = vec4<f32>(linear_to_srgb(c.rgb / (vec3<f32>(1.0) + c.rgb)), 1.0);
    }
    var rgb = c.rgb;
    switch params.channel {
        case 1u: { rgb = vec3<f32>(c.r); }
        case 2u: { rgb = vec3<f32>(c.g); }
        case 3u: { rgb = vec3<f32>(c.b); }
        case 4u: { rgb = vec3<f32>(c.a); }
        case 5u: {
            let cell = (u32(in.pos.x / 8.0) + u32(in.pos.y / 8.0)) % 2u;
            let checker = select(vec3<f32>(0.35), vec3<f32>(0.65), cell == 1u);
            rgb = mix(checker, c.rgb, c.a);
        }
        default: {}
    }
    if (params.view_linear == 1u) {
        rgb = linear_to_srgb(rgb);
    }
    return vec4<f32>(rgb, 1.0);
}
```

(BC5 blue reconstruction happens on the CPU side for the fallback and in the shader when `channel == 0` for BC5: add a `bc5: u32` field to `Params` if you implement it in the shader; either place is fine as long as the displayed blue is reconstructed.)

- [ ] **Step 4: Implement `render/mod.rs` and `upload.rs`.** The pure functions as specified. `PreviewRenderer` holds the pipeline (one bind-group layout: texture, sampler, uniform buffer), two samplers, and per `Slot` an optional `(wgpu::Texture, wgpu::Buffer /* Params */, wgpu::BindGroup, mip_count)`. `paint` adds:

```rust
struct PaintSlot {
    slot: Slot,
    params: Params,
}

impl egui_wgpu::CallbackTrait for PaintSlot {
    fn prepare(
        &self,
        _device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen: &egui_wgpu::ScreenDescriptor,
        _encoder: &mut wgpu::CommandEncoder,
        resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        if let Some(r) = resources.get::<PreviewRenderer>() {
            r.write_params(queue, self.slot, &self.params);
        }
        vec![]
    }

    fn paint(&self, _info: egui::PaintCallbackInfo, pass: &mut wgpu::RenderPass<'static>, resources: &egui_wgpu::CallbackResources) {
        if let Some(r) = resources.get::<PreviewRenderer>() {
            r.draw(pass, self.slot);
        }
    }
}
```

with `ui.painter().add(egui_wgpu::Callback::new_paint_callback(rect, PaintSlot { slot, params }))`. `Params` is uploaded with `bytemuck`-free code: write its fields into a 32-byte little-endian array by hand (`f32::to_le_bytes`, `u32::to_le_bytes`; pad to the uniform's 16-byte alignment). `set_encoded` chooses `UploadPath::Bc` when `can_upload_bc(self.supports_bc(), self.max_dim(), out.result.source.dims[first])`, else uploads `out.decoded` as `Rgba16Float`. The heatmap slot is filled by Task 17 through `set_heatmap(rs, w, h, rgba8: &[u8])` (an `Rgba8Unorm` texture).

- [ ] **Step 5: Run the tests and look at it**

Run: `cargo test -p sdc-gui --test render`, then `cargo run -p sdc-gui` (Task 17 wires the view; here only check that the window starts with the device descriptor change).
Expected: PASS; the window opens.

- [ ] **Step 6: Commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings
git add crates/gui/src/render crates/gui/src/lib.rs crates/gui/src/main.rs crates/gui/src/app.rs crates/gui/tests/render.rs
git commit -m "gui: wgpu preview renderer with BC upload and CPU fallback (M3a)"
```

---

### Task 17: The expanded preview

**Files:**
- Create: `crates/gui/src/model/view.rs` (pure view maths: pan/zoom, heatmap, readouts), `crates/gui/src/ui/preview.rs`
- Modify: `crates/gui/src/model/mod.rs` (`pub mod view;`, `view: ViewState` field), `crates/gui/src/ui/mod.rs` (centre routes to the preview when `expanded`), `crates/gui/src/ui/columns.rs` (Esc), `crates/gui/src/app.rs` (upload new results to the renderer once per change)
- Test: `crates/gui/tests/view.rs`, `crates/gui/tests/ui_smoke.rs`

**Interfaces:**
- Consumes: Task 15's `PreviewModel`, Task 16's `render::{paint, Params, Slot, PreviewRenderer}`, Task 13's compact settings panel.
- Produces (`sdc_gui::model::view`):

```rust
#[derive(Clone, Copy, PartialEq, Eq)] pub enum Channel { Rgba, R, G, B, A, AlphaMask }
#[derive(Clone, Copy, PartialEq, Eq)] pub enum Layout { SideBySide, Split }
pub struct ViewState {
    pub channel: Channel, pub layout: Layout, pub split: f32 /* 0..1 */, pub view_linear: bool,
    pub heatmap: bool, pub mip: usize /* absolute mip index */, pub zoom: f32 /* texels per pixel⁻¹: 1.0 = 1:1 */,
    pub center: [f32; 2] /* uv of the view centre */,
}
impl ViewState {
    pub fn uv_rect(&self, viewport: [f32; 2], mip_size: (u32, u32)) -> ([f32; 2], [f32; 2]); // (uv_offset, uv_scale)
    pub fn zoom_at(&mut self, factor: f32, cursor_uv: [f32; 2]);   // keeps the texel under the cursor fixed
    pub fn pan_pixels(&mut self, delta: [f32; 2], viewport: [f32; 2], mip_size: (u32, u32));
    pub fn fit(&mut self, viewport: [f32; 2], mip_size: (u32, u32));
    pub fn clamp_mip(&mut self, first: usize, count: usize);
}
pub fn heatmap(source: &Rgba32F, decoded: &[[f32; 4]], channel: Channel) -> (u32, u32, Vec<u8>); // RGBA8 ramp
pub struct Readouts { pub lines: Vec<(String, String)> }
pub fn readouts(out: &PreviewOut, mip: usize) -> Readouts;
pub fn skipped_note(dims: &[(u32, u32)], first: usize) -> Option<String>;
```

Behaviour (M3a spec §6):
- Toolbar: channel buttons **RGBA R G B A α-mask**; **Mip** slider over the previewable mips (`first_level ..= last`), labelled `mip k (w×h)`; **sRGB / UNORM** toggle (`view_linear`); **Side by side / Split** (split shows a draggable divider); **Heatmap**; **Fit** and **1:1**; **Live encode** checkbox (`model.set_live`); **Encode now** (`model.encode_now`).
- Two linked viewports (source left, encoded right, or one viewport cut by the split): scroll zooms at the cursor, drag pans, both sides share `ViewState`.
- The encoded side shows, by `EncodedState`: `NotEncoded` → "Not encoded" with an **Encode now** button; `Encoding { previous }` → the previous result dimmed (multiply the painted rect by a 50 % black overlay) with "encoding…"; `Stale(r)` → `r` with a "stale" badge; `Done(r)` → `r`; `Failed(msg)` → the message.
- Under the viewports: `skipped_note` (`"mip 0 (8192×8192) not previewed: above the preview limit"`, one line listing every skipped mip), the renderer's `UploadPath` notice if any, `preview.notice`, and the readouts for the selected mip: smallest-mip alpha (min alpha of the last mip, from `validation.alpha`), share of α ≠ 255, coverage at this mip, normal length (mean / max error) for Normal sub-types, BC6H peak ratio, minimum mip PSNR.
- The settings strip: the compact settings panel (Task 13) in a 280 px right side panel inside the centre.
- GPU size limit (M3a spec §6.5): `app.rs` copies `PreviewRenderer::max_dim()` into a new `AppModel::gpu_max_dim: u32` (default 16384). The effective preview limit is `min(settings limit, gpu_max_dim)`; when the GPU lowers it, `preview.notice` reads `"This GPU shows at most {n} px: previewing from the first mip that fits"`. Task 15's limit computation uses this effective value; add a model test `the_gpu_limit_lowers_the_preview_limit` (`gpu_max_dim = 32`, preview limit 4096 → `first_level == 1` for a 64 px texture, and the notice is set).
- Double-click inside a viewport, or Esc, calls `model.collapse()`.
- `app.rs` keeps the uploaded `Arc` pointers and calls `set_source` / `set_encoded` / `set_heatmap` only when the model's `Arc` changes (`Arc::ptr_eq`).

- [ ] **Step 1: Write the failing tests** `crates/gui/tests/view.rs`:

```rust
use engine::image::Rgba32F;
use sdc_gui::model::view::*;

#[test]
fn zooming_keeps_the_texel_under_the_cursor() {
    let mut v = ViewState::default();
    v.fit([512.0, 512.0], (1024, 1024));
    let cursor = [0.25, 0.75];
    let before = v.uv_rect([512.0, 512.0], (1024, 1024));
    v.zoom_at(2.0, cursor);
    let after = v.uv_rect([512.0, 512.0], (1024, 1024));
    // The uv at the cursor's screen position is unchanged.
    let at = |(off, scale): ([f32; 2], [f32; 2]), sx: f32, sy: f32| [off[0] + sx * scale[0], off[1] + sy * scale[1]];
    let sx = (cursor[0] - before.0[0]) / before.1[0];
    let sy = (cursor[1] - before.0[1]) / before.1[1];
    let a = at(after, sx, sy);
    assert!((a[0] - cursor[0]).abs() < 1e-4 && (a[1] - cursor[1]).abs() < 1e-4, "{a:?}");
    assert!(after.1[0] < before.1[0], "zoomed in shows less of the texture");
}

#[test]
fn the_mip_is_clamped_to_what_was_previewed() {
    let mut v = ViewState { mip: 0, ..Default::default() };
    v.clamp_mip(2, 7);
    assert_eq!(v.mip, 2);
    v.mip = 99;
    v.clamp_mip(2, 7);
    assert_eq!(v.mip, 6);
}

#[test]
fn the_heatmap_is_black_where_equal_and_bright_where_different() {
    let src = Rgba32F::new(2, 1, [0.5, 0.5, 0.5, 1.0]);
    let dec = vec![[0.5, 0.5, 0.5, 1.0], [1.0, 0.5, 0.5, 1.0]];
    let (w, h, px) = heatmap(&src, &dec, Channel::R);
    assert_eq!((w, h), (2, 1));
    assert_eq!(&px[..3], &[0, 0, 0]);
    assert!(px[4] > 100 || px[5] > 100 || px[6] > 100);
}

#[test]
fn skipped_mips_are_named() {
    let dims = [(8192, 8192), (4096, 4096), (2048, 2048)];
    assert_eq!(
        skipped_note(&dims, 1).as_deref(),
        Some("mip 0 (8192×8192) not previewed: above the preview limit")
    );
    assert_eq!(skipped_note(&dims, 0), None);
}
```

In `ui_smoke.rs`:

```rust
#[test]
fn the_expanded_preview_offers_encode_now_and_collapses_on_escape() {
    let d = tempfile::tempdir().unwrap();
    let mut model = opened(d.path());
    model.expand("landscape/rock01_rmaos");
    assert!(model.wait_idle(std::time::Duration::from_secs(60)));
    let mut h = Harness::new_state(|ctx, model: &mut AppModel| sdc_gui::ui::draw(ctx, model), model);
    h.run();
    h.get_by_label_contains("Not encoded");
    h.get_by_label("Encode now");
    h.get_by_label("Live encode");
    h.get_by_label_contains("mip 0");
    h.key_press(egui::Key::Escape);
    h.run();
    assert!(h.state().expanded.is_none());
}
```

(Kittest `Harness::key_press` and `state()` are the 0.36 names; adapt if they differ. The headless harness has no wgpu render state, so the viewports draw a placeholder rectangle and the labels still exist.)

- [ ] **Step 2: Run to see them fail**

Run: `cargo test -p sdc-gui --test view --test ui_smoke`
Expected: FAIL.

- [ ] **Step 3: Implement `model/view.rs`**

```rust
//! Pure maths for the expanded preview (M3a spec §6.1): pan/zoom in uv space, the difference heatmap, readouts.
use crate::model::preview::PreviewOut;
use engine::image::Rgba32F;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Channel {
    #[default]
    Rgba,
    R,
    G,
    B,
    A,
    AlphaMask,
}

impl Channel {
    pub fn shader_id(self) -> u32 {
        self as u32
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Layout {
    #[default]
    SideBySide,
    Split,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ViewState {
    pub channel: Channel,
    pub layout: Layout,
    pub split: f32,
    pub view_linear: bool,
    pub heatmap: bool,
    pub mip: usize,
    /// Screen pixels per texel of the shown mip.
    pub zoom: f32,
    /// The uv at the viewport centre.
    pub center: [f32; 2],
}

impl Default for ViewState {
    fn default() -> Self {
        ViewState {
            channel: Channel::Rgba,
            layout: Layout::SideBySide,
            split: 0.5,
            view_linear: false,
            heatmap: false,
            mip: 0,
            zoom: 1.0,
            center: [0.5, 0.5],
        }
    }
}

impl ViewState {
    /// `(uv_offset, uv_scale)` mapping viewport [0, 1]² to texture uv.
    pub fn uv_rect(&self, viewport: [f32; 2], mip: (u32, u32)) -> ([f32; 2], [f32; 2]) {
        let scale = [
            viewport[0] / (self.zoom * mip.0 as f32),
            viewport[1] / (self.zoom * mip.1 as f32),
        ];
        let offset = [self.center[0] - scale[0] / 2.0, self.center[1] - scale[1] / 2.0];
        (offset, scale)
    }

    pub fn zoom_at(&mut self, factor: f32, cursor_uv: [f32; 2]) {
        let z = (self.zoom * factor).clamp(1.0 / 64.0, 64.0);
        let f = self.zoom / z;
        for i in 0..2 {
            self.center[i] = cursor_uv[i] + (self.center[i] - cursor_uv[i]) * f;
        }
        self.zoom = z;
    }

    pub fn pan_pixels(&mut self, delta: [f32; 2], _viewport: [f32; 2], mip: (u32, u32)) {
        self.center[0] -= delta[0] / (self.zoom * mip.0 as f32);
        self.center[1] -= delta[1] / (self.zoom * mip.1 as f32);
    }

    pub fn fit(&mut self, viewport: [f32; 2], mip: (u32, u32)) {
        self.zoom = (viewport[0] / mip.0 as f32).min(viewport[1] / mip.1 as f32);
        self.center = [0.5, 0.5];
    }

    pub fn clamp_mip(&mut self, first: usize, count: usize) {
        self.mip = self.mip.clamp(first, count.saturating_sub(1).max(first));
    }
}

/// |source − decoded| of `channel` (RGB: the largest of the three), as a black → red → yellow → white ramp.
pub fn heatmap(source: &Rgba32F, decoded: &[[f32; 4]], channel: Channel) -> (u32, u32, Vec<u8>) {
    let mut out = Vec::with_capacity(decoded.len() * 4);
    for (s, d) in source.px.iter().zip(decoded) {
        let e = |c: usize| (s[c] - d[c]).abs();
        let err = match channel {
            Channel::R => e(0),
            Channel::G => e(1),
            Channel::B => e(2),
            Channel::A | Channel::AlphaMask => e(3),
            Channel::Rgba => e(0).max(e(1)).max(e(2)),
        };
        // 0.1 (about 25 codes) is full scale.
        let t = (err / 0.1).clamp(0.0, 1.0);
        let r = (t * 3.0).min(1.0);
        let g = (t * 3.0 - 1.0).clamp(0.0, 1.0);
        let b = (t * 3.0 - 2.0).clamp(0.0, 1.0);
        out.extend([(r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8, 255]);
    }
    (source.width, source.height, out)
}

pub fn skipped_note(dims: &[(u32, u32)], first: usize) -> Option<String> {
    (first > 0).then(|| {
        let names: Vec<String> = dims[..first]
            .iter()
            .enumerate()
            .map(|(i, (w, h))| format!("mip {i} ({w}×{h})"))
            .collect();
        format!("{} not previewed: above the preview limit", names.join(", "))
    })
}

pub struct Readouts {
    pub lines: Vec<(String, String)>,
}

/// The readouts of M3a spec §6.4 for absolute mip `mip`.
pub fn readouts(out: &PreviewOut, mip: usize) -> Readouts {
    let r = &out.result;
    let v = &r.validation;
    let i = mip.saturating_sub(r.source.first_level);
    let mut lines = vec![];
    if let Some(last) = v.alpha.last() {
        lines.push(("Smallest-mip alpha".into(), format!("{}", last.min)));
    }
    if let Some(a) = v.alpha.get(i) {
        let share = 1.0 - a.eq255 as f64 / a.pixels.max(1) as f64;
        lines.push(("α ≠ 255".into(), format!("{:.1} %", share * 100.0)));
    }
    if let Some(c) = v.coverage.get(i) {
        lines.push(("Coverage".into(), format!("{:.1} %", c * 100.0)));
    }
    if let Some(n) = r.normal_len.get(i) {
        lines.push(("Normal length error".into(), format!("mean {:.4}, max {:.4}", n.mean_err, n.max_err)));
    }
    if let Some(p) = v.bc6h_peak_ratio {
        lines.push(("BC6H peak ratio".into(), format!("{p:.3}")));
    }
    if let Some(p) = v.min_mip_psnr_db {
        lines.push(("Minimum mip PSNR".into(), format!("{p:.1} dB")));
    }
    Readouts { lines }
}
```

(Check `ValidationSummary`'s `alpha`/`coverage` indexing: they are per encoded mip in mip order; if `coverage` is empty when coverage does not apply, the line is simply absent.)

- [ ] **Step 4: Implement `ui/preview.rs`** per the behaviour list: the toolbar, the viewports (`render::paint` with `Params { uv_offset, uv_scale, mip: (view.mip - first) as f32, channel: view.channel.shader_id(), view_linear, tonemap: (format == Bc6hUf16) as u32 }` when a wgpu render state exists; a grey placeholder rectangle with the state text otherwise), the dimming overlay, the badges, the notes and readouts, the settings strip, and Esc / double-click to collapse. Heatmap mode paints `Slot::Heatmap`, whose texture `app.rs` builds with `view::heatmap(&src.levels[i], &out.decoded[i], view.channel)` when the mip, the channel or the result changes.

- [ ] **Step 5: Run the tests, then look**

Run: `cargo test -p sdc-gui`, then `cargo run -p sdc-gui --release`: open `F:/ai/dds/mvp-test/src`, double-click a texture, press **Encode now**, step through the mips and channels, toggle Split and Heatmap, press Esc.
Expected: tests PASS; the encoded side shows the texture after the encode.

- [ ] **Step 6: Commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings
git add crates/gui/src crates/gui/tests/view.rs crates/gui/tests/ui_smoke.rs
git commit -m "gui: expanded preview with channels, mips, split, heatmap and readouts (M3a)"
```

---

### Task 18: The Advanced drawer

**Files:**
- Create: `crates/gui/src/model/inspector.rs`, `crates/gui/src/ui/advanced.rs`
- Modify: `crates/gui/src/model/mod.rs` (`pub mod inspector;`, `remove_unmatched`, `set_preview_max`, `set_bc1_choice`), `crates/gui/src/ui/mod.rs` (the right drawer)
- Test: `crates/gui/tests/advanced.rs`

**Interfaces:**
- Consumes: Task 11's model and `ProjectDoc`; `PresetRegistry::base_profile` and `Session::engine().registry()`.
- Produces (`sdc_gui::model::inspector`):

```rust
pub struct InspectorRow { pub field: String, pub value: serde_json::Value, pub provenance: String, pub differs_from_base: bool }
pub fn inspector_rows(job: &ResolvedJob, registry: &PresetRegistry) -> Vec<InspectorRow>;  // every leaf of the profile JSON
```

- Produces (`AppModel`): `pub fn remove_unmatched(&mut self, key: &str)` (removes the `[files]` entry and re-plans), `pub fn set_preview_max(&mut self, size: u32)` (16384 only when `allow_16k_preview`; saved), `pub fn set_bc1_choice(&mut self, c: Bc1Choice)` (saved; sets `restart_needed = true` when it differs from the session's model), `pub restart_needed: bool`.

Drawer content (M3a spec §2.5), each a collapsing section:
1. **Preset inspector** for the selected texture: a table of `inspector_rows` (field, value, provenance text from `fields::provenance_text`), rows that differ from the base preset in bold, a **Show only differences** toggle, and the batch preset id. Built-in presets are read-only (no editing here).
2. **Batch**: worker threads (0 = one per core), memory budget (MiB, 0 = half of RAM), **BC1 decoder** combo (`Bc1Choice::ALL`) with `"restart sdc-gui to apply"` when `restart_needed`.
3. **Packing fill values**: one row per `RawMap::ALL` role: a checkbox **Fill** and a 0–255 slider; unchecked → `model.set_fill(role, None)`, checked → `Some(value)`. Help text: "A fill replaces the neutral default for a packed channel that has no source map."
4. **Preview**: the size combo (1024, 2048, 4096, 8192, and 16384 when **Allow 16384 previews** is ticked), **Live encode** default.
5. **Advanced texture fields** for the selected texture: `encoder.perceptual`, `encoder.bc1_transparent_black`, `encoder.bc45_search_radius`, `encoder.rgbcx_level`, `header` — the same row editor as the settings panel (`ui::settings_panel::field_row`), with their locks.
6. **Unmatched `[files]` entries** (`model.unmatched_keys`): each key with **Remove**.

- [ ] **Step 1: Write the failing tests** `crates/gui/tests/advanced.rs`:

```rust
mod support;
use engine::config::RawMap;
use sdc_gui::model::inspector::*;
use sdc_gui::settings::Bc1Choice;
use std::time::Duration;
use support::*;

#[test]
fn the_inspector_shows_provenance_and_differences_from_the_base() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.set_field("landscape/leaf.png", "alpha.threshold", serde_json::json!(90));
    assert!(m.wait_idle(Duration::from_secs(60)));
    let job = m.item("landscape/leaf.png").unwrap().job.clone().unwrap();
    let reg = m.session.as_ref().unwrap().engine().registry().clone();
    let rows = inspector_rows(&job, &reg);
    let t = rows.iter().find(|r| r.field == "alpha.threshold").unwrap();
    assert_eq!(t.provenance, "project_file");
    assert!(t.differs_from_base);
    assert!(rows.iter().any(|r| !r.differs_from_base));
}

#[test]
fn fill_values_are_written_to_defaults() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.set_fill(RawMap::Metallic, Some(255));
    assert!(m.wait_idle(Duration::from_secs(60)));
    let job = m.item("landscape/rock01_rmaos").unwrap().job.clone().unwrap();
    assert_eq!(job.provenance["pack.g"].to_string(), "fill:255");
    assert_eq!(m.project.as_ref().unwrap().fill(RawMap::Metallic), Some(255));
}

#[test]
fn unmatched_entries_are_listed_and_removable() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.set_field("landscape/rock01_rmaos", "alpha.threshold", serde_json::json!(90));
    m.set_target(engine::config::Target::Cpm);
    assert!(m.wait_idle(Duration::from_secs(60)));
    assert_eq!(m.unmatched_keys, vec!["landscape/rock01_rmaos".to_string()]);
    m.remove_unmatched("landscape/rock01_rmaos");
    assert!(m.wait_idle(Duration::from_secs(60)));
    assert!(m.unmatched_keys.is_empty());
    assert!(!m.project.as_ref().unwrap().file_keys().contains(&"landscape/rock01_rmaos".to_string()));
}

#[test]
fn preview_size_and_bc1_choice_follow_their_rules() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.set_preview_max(16384);
    assert_eq!(m.settings.preview_max, 4096, "16384 needs the explicit setting");
    m.settings.allow_16k_preview = true;
    m.set_preview_max(16384);
    assert_eq!(m.settings.preview_max, 16384);
    m.set_bc1_choice(Bc1Choice::Nvidia);
    assert!(m.restart_needed);
    m.set_bc1_choice(Bc1Choice::Ideal);
    assert!(!m.restart_needed);
}
```

- [ ] **Step 2: Run to see them fail**

Run: `cargo test -p sdc-gui --test advanced`
Expected: compile errors.

- [ ] **Step 3: Implement `model/inspector.rs`**

```rust
//! The preset inspector (main spec §9 #3): every resolved field, where it came from, and whether it differs from
//! the base preset's value for the same target and sub-type.
use engine::policy::ResolvedJob;
use engine::presets::{leaf_paths, PresetRegistry};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub struct InspectorRow {
    pub field: String,
    pub value: Value,
    pub provenance: String,
    pub differs_from_base: bool,
}

fn at<'a>(v: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.').try_fold(v, |v, k| v.get(k))
}

pub fn inspector_rows(job: &ResolvedJob, registry: &PresetRegistry) -> Vec<InspectorRow> {
    let now = serde_json::to_value(&job.profile).unwrap_or_default();
    let base = serde_json::to_value(registry.base_profile(job.profile.target, job.profile.subtype)).unwrap_or_default();
    let mut paths = vec![];
    leaf_paths(&now, "", &mut paths);
    paths
        .into_iter()
        .map(|field| {
            let value = at(&now, &field).cloned().unwrap_or(Value::Null);
            InspectorRow {
                differs_from_base: at(&base, &field) != Some(&value),
                provenance: job.provenance.get(&field).map(ToString::to_string).unwrap_or_else(|| "builtin".into()),
                value,
                field,
            }
        })
        .collect()
}
```

(If `PresetRegistry` is not `Clone`, borrow it from the session in the test instead.) The model functions:

```rust
    pub fn remove_unmatched(&mut self, key: &str) {
        let k = key.to_string();
        self.edit(move |p| p.remove_file_entry(&k));
    }

    pub fn set_preview_max(&mut self, size: u32) {
        if size == 16384 && !self.settings.allow_16k_preview {
            return;
        }
        if crate::settings::PREVIEW_SIZES.contains(&size) {
            self.settings.preview_max = size;
            self.save_settings();
        }
    }

    pub fn set_bc1_choice(&mut self, c: crate::settings::Bc1Choice) {
        self.settings.bc1_decoder = c;
        self.save_settings();
        self.restart_needed = self.session.as_ref().is_some_and(|s| s.bc1_model() != c.model());
    }
```

(`Bc1Model` must be `PartialEq`; it is a fieldless `#[repr]` enum — derive it in `encoders-sys` if missing.)

- [ ] **Step 4: Implement `ui/advanced.rs`** per the drawer content list; the right drawer is collapsed to a 24 px strip reading "Advanced" (vertical) until `settings.advanced_open`.

- [ ] **Step 5: Run the tests**

Run: `cargo test -p sdc-gui`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings
git add crates/gui/src crates/gui/tests/advanced.rs crates/encoders-sys/src/lib.rs
git commit -m "gui: Advanced drawer with preset inspector, batch, fill and preview settings (M3a)"
```

---

### Task 19: Edge cases, CLI reproducibility and documents

**Files:**
- Modify: `crates/gui/src/model/mod.rs` (`check_sources`, `reload_changed`, `gpu_available`), `crates/gui/src/app.rs` (focus → `check_sources`; no render state → `gpu_available = false`), `crates/gui/src/ui/columns.rs` (**Reload** on changed items)
- Create: `crates/cli/tests/gui_repro.rs`; modify `crates/cli/Cargo.toml` (dev-dependency `sdc-gui = { path = "../gui" }`)
- Create: `docs/gui.md` (user guide)
- Modify: `docs/superpowers/specs/2026-10-08-skyrim-dds-converter-design.md` (status line), `docs/superpowers/plans/2026-10-08-skyrim-dds-converter-phase0-2.md` (M3 carried items consumed by M3a), `docs/superpowers/specs/2026-10-10-sdc-m3a-converter-gui-design.md` (status line)
- Test: `crates/gui/tests/edges.rs`, `crates/cli/tests/gui_repro.rs`

**Interfaces:**
- Produces (`AppModel`): `pub gpu_available: bool` (default true), `pub fn check_sources(&mut self) -> usize` (marks `changed_on_disk` on every item whose source's `SourceStamp` differs from its job's `source` stamp, or from each pack source's stamp; returns the count), `pub fn reload_changed(&mut self)` (re-plans; the session cache reloads only changed files).

Behaviour (M3a spec §7):
1. Changed on disk: `check_sources` runs on window focus and before Export. A changed item shows "changed on disk" with **Reload**; Export is still allowed (the engine refuses a changed file: "source changed since planning; re-run"), and `export_clicked` runs `check_sources` first and re-plans when any changed, so the export uses fresh plans.
2. A worker panic shows `Dialog::Error("internal error in <what>: …")` and the window keeps running (Task 11's `spawn_guarded`); test it with a panicking closure.
3. No usable GPU (`wgpu_render_state` is `None`): `gpu_available = false`; the preview's viewports show `"No usable GPU: the preview is off; everything else works"`; Encode now is disabled; everything else works.

- [ ] **Step 1: Write the failing tests** `crates/gui/tests/edges.rs`:

```rust
mod support;
use image::{ImageBuffer, Rgb};
use sdc_gui::model::*;
use sdc_gui::worker::*;
use std::time::Duration;
use support::*;

#[test]
fn a_source_changed_on_disk_is_flagged_and_reloads_alone() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    assert_eq!(m.check_sources(), 0);
    let loads = m.session.as_ref().unwrap().cache().loads();
    let p = m.root.clone().unwrap().join("landscape/rock01.png");
    ImageBuffer::from_fn(32, 32, |_, _| Rgb([9u8, 9, 9])).save(&p).unwrap();
    assert_eq!(m.check_sources(), 1);
    assert!(m.item("landscape/rock01.png").unwrap().changed_on_disk);
    m.reload_changed();
    assert!(m.wait_idle(Duration::from_secs(60)));
    assert!(!m.item("landscape/rock01.png").unwrap().changed_on_disk);
    assert_eq!(m.session.as_ref().unwrap().cache().loads(), loads + 1);
}

#[test]
fn a_worker_panic_is_reported_and_the_model_keeps_working() {
    let (tx, rx) = std::sync::mpsc::channel();
    spawn_guarded(tx, "test", |_| panic!("boom"));
    match rx.recv_timeout(Duration::from_secs(10)).unwrap() {
        WorkerMsg::Panicked { what, message } => {
            assert_eq!(what, "test");
            assert!(message.contains("boom"));
        }
        _ => panic!("expected a panic message"),
    }
}

#[test]
fn without_a_gpu_the_preview_says_so() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.gpu_available = false;
    m.expand("landscape/rock01_rmaos");
    m.encode_now();
    assert_eq!(
        m.preview.notice.as_deref(),
        Some("No usable GPU: the preview is off; everything else works")
    );
}
```

`crates/cli/tests/gui_repro.rs` (the spec's key test, §8.2):

```rust
//! M3a spec §8: a folder decided and exported in the GUI, run through `sdc convert`, gives the same manifest
//! records and the same DDS bytes.
use assert_cmd::Command;
use engine::codes::Code;
use engine::config::{AlphaPolicy, GreenConvention, Subtype};
use image::{ImageBuffer, Luma, Rgb, Rgba};
use sdc_gui::model::{Answer, AppModel};
use sdc_gui::settings::GuiSettings;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

fn folder(dir: &Path) -> std::path::PathBuf {
    let r = dir.join("textures/landscape");
    std::fs::create_dir_all(&r).unwrap();
    let grad = |x: u32, y: u32| [(x * 4) as u8, (y * 4) as u8, 90u8];
    ImageBuffer::from_fn(64, 64, |x, y| Rgb(grad(x, y))).save(r.join("rock01.png")).unwrap();
    ImageBuffer::from_fn(64, 64, |x, y| {
        let g = grad(x, y);
        Rgba([g[0], g[1], g[2], if x < 32 { 255 } else { 0 }])
    })
    .save(r.join("leaf.png"))
    .unwrap();
    ImageBuffer::from_fn(64, 64, |_, _| Rgb([128u8, 128, 255])).save(r.join("rock01_normal.png")).unwrap();
    ImageBuffer::from_fn(48, 48, |x, y| Rgb(grad(x, y))).save(r.join("odd.png")).unwrap();
    for (n, v) in [("rock01_roughness.png", 120u8), ("rock01_ao.png", 200)] {
        ImageBuffer::from_fn(64, 64, |x, _| Luma([v.wrapping_add(x as u8)])).save(r.join(n)).unwrap();
    }
    dir.join("textures")
}

/// output → (profile hash, source hash, DDS bytes).
fn outputs(out: &Path) -> BTreeMap<String, (String, String, Vec<u8>)> {
    let text = std::fs::read_to_string(engine::manifest::Manifest::at(out).path()).unwrap();
    let mut m = BTreeMap::new();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let v: serde_json::Value = serde_json::from_str(line).unwrap();
        let o = v["output"].as_str().unwrap().to_string();
        let bytes = std::fs::read(out.join(&o)).unwrap();
        m.insert(o, (v["profile_sha256"].as_str().unwrap().into(), v["source_sha256"].as_str().unwrap().into(), bytes));
    }
    m
}

#[test]
fn a_gui_export_and_sdc_convert_agree() {
    let d = tempfile::tempdir().unwrap();
    let root = folder(d.path());
    let mut settings = GuiSettings::default();
    settings.export_folder = Some(d.path().join("gui-out"));
    let session = Arc::new(engine::session::Session::new(encoders_sys::Bc1Model::Ideal).unwrap());
    let mut m = AppModel::new(settings, None, session);
    m.open_folder(root.clone());
    assert!(m.wait_idle(Duration::from_secs(60)));
    m.decide("landscape/rock01.png", Answer::Subtype(Subtype::Diffuse));
    m.decide("landscape/odd.png", Answer::Subtype(Subtype::Diffuse));
    m.decide("landscape/odd.png", Answer::Acknowledge(Code::DimNpot));
    m.decide("landscape/leaf.png", Answer::Subtype(Subtype::Diffuse));
    m.decide("landscape/leaf.png", Answer::Alpha(AlphaPolicy::Cutout));
    m.decide("landscape/rock01_normal.png", Answer::Subtype(Subtype::Normal));
    m.decide("landscape/rock01_normal.png", Answer::Green(GreenConvention::Directx));
    m.set_field("landscape/leaf.png", "alpha.threshold", serde_json::json!(90));
    assert!(m.wait_idle(Duration::from_secs(60)));
    m.export_clicked();
    assert!(m.wait_idle(Duration::from_secs(120)));
    assert_eq!(m.export.summary.as_ref().unwrap().failed, 0);

    Command::cargo_bin("sdc")
        .unwrap()
        .args(["convert"])
        .arg(&root)
        .args(["--recursive", "-o"])
        .arg(d.path().join("cli-out"))
        .args(["--bc1-decoder", "ideal"])
        .env("APPDATA", d.path().join("no-appdata"))
        .assert()
        .success();
    let (gui, cli) = (outputs(&d.path().join("gui-out")), outputs(&d.path().join("cli-out")));
    assert_eq!(gui.keys().collect::<Vec<_>>(), cli.keys().collect::<Vec<_>>());
    for (k, g) in &gui {
        assert_eq!(g, &cli[k], "{k} differs");
    }
}
```

(Add to `crates/cli/Cargo.toml` `[dev-dependencies]`: `sdc-gui = { path = "../gui" }`, `encoders-sys.workspace = true`, `engine.workspace = true` if missing, `image.workspace = true`, `serde_json.workspace = true`. The `APPDATA` override keeps the developer's own `config.toml` out of the CLI run; the GUI model in the test has `user_config: None` by construction.)

- [ ] **Step 2: Run to see them fail**

Run: `cargo test -p sdc-gui --test edges` and `cargo test -p sdc --test gui_repro`
Expected: FAIL (`check_sources`, `gpu_available` missing); `gui_repro` may already pass — if it fails, the difference it prints is a real GUI/CLI divergence to fix, not a test to loosen.

- [ ] **Step 3: Implement** `check_sources`, `reload_changed`, `gpu_available` (and the preview notice when it is false: `encode_now` and the source request set the notice and start nothing), the focus hook in `app.rs` (`ctx.input(|i| i.focused)` turning true), and the **Reload** button. In `export_clicked`, call `check_sources()` first; when it returns more than 0, re-plan and wait for the plan before starting the export (keep the export pending in a field `export_after_plan: bool` that `apply_plan` acts on).

- [ ] **Step 4: Documents.**
  - `docs/gui.md`: a short user guide: opening a folder, the three columns, deciding (amber borders, confirm controls), editing (saved to `sdc.toml` as `[files]`, Ctrl+S, ●), the preview (double-click, Esc, Live encode off by default, Encode now, the 4096 limit), Export (Proceed anyway, the drawer, Copy CLI command), and where settings live (`%APPDATA%\sdc\gui.toml`).
  - Main spec status line: add "M3a (converter GUI) built on `feat/m3a-gui`; its design is `2026-10-10-sdc-m3a-converter-gui-design.md`."
  - M3a spec status line: "Built on `feat/m3a-gui` (plan `docs/superpowers/plans/2026-10-10-sdc-m3a-converter-gui.md`)."
  - Phase 0–2 plan, M3 section: mark as done by M3a the carried items it consumed — partial-Profile schema (Task 3), I5 event bump (Tasks 4–5), I6 per-file allow and exact-path scope (Tasks 2–3), `CancelToken` per batch (Tasks 11–15), Analysis kept for re-resolving (Task 7), channel-only GUI sink (Task 11), `manifest::append` caller (unchanged: the GUI calls `run`), and from "Carried from M2.5" the event-identity items (Task 5), `SourceRef` roles (Task 6) and the `file:` scope on packed outputs (Task 2's key rule). List what stays open for M3b/M7 unchanged.

- [ ] **Step 5: Run everything**

Run: `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo build --release -p sdc -p sdc-gui`
Expected: all PASS; both exes build.

- [ ] **Step 6: Commit**

```bash
git add crates/gui/src crates/gui/tests/edges.rs crates/cli/Cargo.toml crates/cli/tests/gui_repro.rs Cargo.lock docs/gui.md docs/superpowers/specs/2026-10-08-skyrim-dds-converter-design.md docs/superpowers/specs/2026-10-10-sdc-m3a-converter-gui-design.md docs/superpowers/plans/2026-10-08-skyrim-dds-converter-phase0-2.md
git commit -m "gui: changed-on-disk, panic and no-GPU handling; GUI/CLI reproducibility test; docs (M3a)"
```

---

## M3a exit (owner)

Not a task for an agent. After the final review, the owner:

1. Runs `target/release/sdc-gui.exe`, opens `F:/ai/dds/mvp-test/src`, decides the 13 `pbr_albedo` alpha cases in the GUI (amber borders → Alpha buttons), and exports to a new folder.
2. Double-clicks a few textures, encodes them, and checks the mips, channels and split view.
3. Notes anything awkward; the controller records the usability findings in DECISIONS and opens M3b.

The crash rate seen during this pass is recorded for the isolated-worker decision (DECISIONS #18).

# sdc M3a: converter GUI, design specification

| Field | Value |
|---|---|
| Date | 2026-10-10 |
| Status | Design approved by the owner in conversation, section by section (2026-10-10). Written spec awaiting owner review |
| Milestone | M3a, the first of three GUI parts (M3a → M3b → M3c, owner 2026-10-10) |
| Supersedes | Spec `2026-10-08-skyrim-dds-converter-design.md` §9 for everything M3a builds; §9 stays the reference for what M3a leaves out |
| Inputs | `F:/ai/dds/gui reference/Rough.jpg` (owner layout), `v1 instruction.txt`, `v1-gap-review.md`; plan `2026-10-08-skyrim-dds-converter-phase0-2.md` M3 carried items and "Carried from M2.5" |

**Tags.** As in the main spec: **[Requirement]** decided by the owner (here, in the 2026-10-10 design conversation unless cited otherwise); **[Finding]** backed by the code or a source; **[Proposal]** a design choice made here; **[Open]** undecided, with the shipped default stated.

---

## 1. Purpose and scope

**[Requirement]** `sdc-gui` is primarily a converter GUI: a source list, settings, a preset inspector, a 2D GPU preview and a batch report (spec §9). The owner widened the GUI's role to cover material authoring as well, so M3 is split into three parts, each with its own spec, plan and build:

| Part | Delivers | Depends on |
|---|---|---|
| **M3a: Converter GUI** (this spec) | Window layout, texture browser, read-only texture-set column, per-texture encoding settings, 2D preview, Export through the engine, batch report | Engine only |
| **M3b: Set editor and PGPatcher JSON** | Material-slot editor, drag-and-drop into sets, PGPatcher PBR JSON writer (three-state keys, `{default, entries}`, match modes, `textures\pbr\` export paths), GUI project file, undo/redo | M3a |
| **M3c: Texture edits** | Resize, channel flip and invert, basic filters, duplicating a shared source, protection against conflicting bakes on shared textures | New engine features, M3a |

**Out of scope for M3a.** **[Requirement]** TruePBR-Manager's 3D renderer, and Skyking's legacy→current and CPM⇄PBR conversions, are out of GUI v1 (`v1-gap-review.md`). **[Proposal]** Also left out of M3a: the isolated-worker mode (ships off as spec §9 says, and is not built in M3a); per-stage progress (per-stage events stay M7); switching the BC1 decoder model inside a running session (§4.6); undo/redo and anything that edits set membership (M3b).

---

## 2. Window layout and interaction

```
┌ Mode [cs▾]   Collapse columns          Export folder [...]  [Export] ┐
├────────┬────────┬──────────────────────────────────────────────┬────┤
│Textures│ Sets   │ CENTRE                                       │ A  │
│ filter │ (auto, │  · set selected     → set view               │ d  │
│ search │  read- │  · texture selected → its encoding settings  │ v  │
│ thumbs │  only) │  · double-click thumb → full 2D preview      │    │
│        │        │      + slim settings strip on one edge       │    │
├────────┴────────┴──────────────────────────────────────────────┴────┤
│ Bottom drawer: Export progress → report (warnings grouped by code)  │
└─────────────────────────────────────────────────────────────────────┘
```

### 2.1 Header

- **[Requirement]** **Mode** is the batch target: `cs`, `vanilla` or `cpm`. Changing it sets `target` (saved to `sdc.toml` with Ctrl+S or on Export, §5.4) and re-resolves every texture through the session cache (§4.4), without reloading images.
- **[Requirement]** **Collapse columns** folds the Textures and Sets columns to thin strips so the centre gets the room.
- **[Requirement]** **Export folder** (a folder picker, `rfd`) and the **Export** button sit on the right.
- **[Proposal]** A ● after the window title marks unsaved `sdc.toml` changes (§5.4).

### 2.2 Textures column

- **[Requirement]** Every source texture is a small-to-medium thumbnail with a visible border (`v1 instruction.txt`). Filter and search sit at the top.
- **[Proposal]** Filters: *Needs decision*, *Has warnings*, *Failed*, *Not in a set* (the "unmatched" filter of `v1 instruction.txt`: a file whose stem groups with no other file). Search matches the path relative to the opened folder, case-insensitively.
- **[Requirement]** A texture that needs a decision has an **amber border**. A decision is either a suggestion the engine will not apply without confirmation (low-confidence sub-type, an alpha policy such as the 13 `pbr_albedo` files of `F:/ai/dds/mvp-test`) or a blocking warning that needs acknowledgement. Its settings show a confirm control (§5.2).
- **[Proposal]** Other badges: red ✗ for a file that failed to load or plan, with the reason on hover; ⚠ for warnings; "packed into `<name>`" for a raw map consumed by a recipe (spec §6.4); "changed on disk" (§7.3).
- **[Proposal]** A DDS input (`IN-DDS-SKIPPED`) is listed and previewable, with only the encoded side shown (its own bytes); it is not converted (spec §1).

### 2.3 Sets column

- **[Requirement]** Read-only in M3a. It shows the engine's grouping (spec §6.1) and each set's packed outputs with their recipe status (`run`, `conflict`, `failed`, `disabled`; `RecipeReport`). Drag-and-drop into sets is M3b.
- **[Proposal]** A set is a pill holding its members' small thumbnails. Clicking a set opens the set view (§2.4); clicking a member thumbnail selects that texture.

### 2.4 Centre

- **Set selected.** **[Proposal]** The set's members as labelled slot thumbnails (diffuse, `_n`, `_rmaos`, …), each packed output with its per-channel sources and their provenance (`Source`, `Default`, `Fill`, as `sdc inspect` prints), and the set's warnings. No editing beyond selecting a member.
- **Texture selected.** **[Requirement]** That texture's encoding settings, in the collapsible groups of spec §9 #2: *Type* (target, category, sub-type, each with the reason for its default), *Colour* (the two explicit fields "Content: sRGB-encoded colour / Linear data" and "DDS tag: `_SRGB` / UNORM (linear)", each with its reason), *Alpha* (policy, threshold 5–200, comparison, exact opaque alpha), *Mipmaps*, *Compression*. Locked fields say why. **[Requirement]** Every format or preset label states its BC format and whether it is sRGB or linear (`v1 instruction.txt`).
- **Expanded preview.** **[Requirement]** Double-clicking any thumbnail, in either column or in the set view, expands the 2D preview over the whole centre. A **slim settings strip** stays visible on one edge, holding the texture's format, mipmap and alpha controls, so a setting can be changed while watching the encoded side. Double-clicking again, or Esc, collapses back to the view it came from. The preview is specified in §6.

### 2.5 Drawers

- **Advanced (right edge).** **[Requirement]** The preset inspector (spec §9 #3): every resolved field of the selected texture with its provenance, and the diff against the base preset. Built-in presets are read-only. **[Proposal]** It also holds the batch-level settings: worker threads, memory budget, BC1 decoder model (§4.6), the packing fill sliders (one per raw-map role, written to `sdc.toml` as `defaults.packing.fill.<role>`, spec §3.2), the preview size limit (§6.5), the 16384 preview setting, and the spec §9 #4 advanced fields that exist in the engine today.
- **Bottom drawer.** **[Requirement]** It slides up when Export starts and shows per-file progress, then the batch report with warnings grouped by code. It stays open until the user closes it. Clicking a warning or a file row selects that texture; the centre is not replaced. **[Proposal]** Per-file states are queued, running, done, up to date, skipped (with the skip kind of §4.3) and failed. The report can be exported as JSON Lines or Markdown (spec §9 #6), and a **Copy CLI command** button copies the `sdc convert` command that reproduces the export (§5.5).

### 2.6 Export with undecided textures

**[Requirement]** Export is never silently disabled. When textures still need a decision, Export opens a dialog: *"N textures still need a decision. [Proceed anyway] [Cancel]"*.

- **Proceed anyway** exports every decided texture. The undecided ones are **skipped, never auto-decided**, and the report lists them as "skipped: needs decision". This follows the spec §1 rule that the user always decides; the GUI runs the engine with `accept = none` and with only the per-file allows the user gave.
- **Cancel** returns with the undecided textures still highlighted.

---

## 3. Architecture

**[Proposal]** A new crate `crates/gui` builds `sdc-gui.exe`, linking `engine` in-process (spec §2).

| Unit | What it does | Depends on |
|---|---|---|
| `engine::session` (new) | Holds a planned folder: per-file `Analysis`, headers and resolved jobs; re-resolves one file or the whole folder without reloading (§4.4); runs Export and preview | existing engine |
| `engine::session` preview (new) | `Session::preview`: one job, in memory, to encoded mips plus readouts (§6) | existing pipeline stages |
| `gui::model` | Plain-Rust view-model: selection, filters, edits, the undecided count, the Export dialog decision, the preview state machine, drawer state. No egui types | `engine` |
| `gui::project` | Reads and writes `sdc.toml` with format preservation (§5) | `toml_edit` |
| `gui::render` | GPU textures and the preview paint callback (§6.4) | `egui_wgpu`, `wgpu` |
| `gui::ui` | egui drawing of the model; no logic beyond mapping input to model calls | `eframe`, `egui`, `rfd` |

**[Proposal]** Threads. The UI thread never blocks: planning, re-resolving, preview encodes and Export each run on worker threads and report through channels the UI drains once per frame. Every engine call gets a fresh `CancelToken` (a token cannot be reset). The GUI's `EventSink` only pushes into a channel, because `Reporter` holds its ordering lock across `emit` and a slow sink would stall every worker (plan M3 carried item).

**[Proposal]** New dependencies, versions pinned in the plan: `eframe`, `egui`, `egui_wgpu`, `wgpu` (feature `TEXTURE_COMPRESSION_BC`), `rfd`, `toml_edit`; dev-only `egui_kittest`. All must pass `cargo deny` (GPL-compatible licences).

---

## 4. Engine prerequisites

These are engine tasks at the start of the M3a plan. They consume the plan's M3 carried items as noted.

### 4.1 Exact-path override scope

**[Requirement]** A per-texture edit must hit exactly one file. **[Finding]** `Scope::File` takes a glob, so `[`, `]`, `{`, `}` and `*` in a filename are syntax, and a glob without `/` matches the basename (plan M3 carried I6). **[Proposal]** Add `Scope::Path(PathBuf)`, matched against the canonical input path. For a packed output, whose input is virtual, it matches the virtual path **without its extension**, which resolves the M2.5 carried item where the virtual name takes the first source's extension. Scope rank: `Path` is 3, above `File` (2), so an exact-path edit beats a glob that also matches the file.

### 4.2 Per-file allow

**[Proposal]** `BatchRequest` gains a per-file allow (`BTreeMap<PathBuf, BTreeSet<Code>>`, same path matching as §4.1) beside the batch-wide `allow`, so acknowledging `FMT-BC5-NORMAL` on one file does not acknowledge it on any other (plan M3 carried I6).

### 4.3 The event-schema bump

**[Proposal]** One `EVENT_SCHEMA` bump (plan M3 carried I5, an M3 entry condition):

- every per-file event carries a stable job key (`manifest::output_key`), so the GUI can tie an event to its thumbnail;
- `file_planned.job` is typed (`ResolvedJob` derives `Deserialize`, and its schema is embedded so the drift test sees it);
- skips carry an enum kind: `up_to_date`, `on_existing_skip`, `unresolved`, `blocked`, `duplicate`, `dds_input`, `packing_input`, `unknown_existing`. The textures skipped by "Proceed anyway" (§2.6) are `unresolved` or `blocked`; the GUI labels both "skipped: needs decision", so no new kind is needed;
- from "Carried from M2.5": every pack source gets its own terminal event, and `SourceRef` records each pack source's role and invert flag. That record replaces the `pack.*` provenance comparison in `is_up_to_date`.

The committed schemas are regenerated.

### 4.4 Session cache

**[Finding]** `Plan` keeps resolved jobs but not the decoded images or `Analysis`, and `run` consumes it (`Plan` is not `Clone`), so every Mode change or edit would reload every image (plan M3 carried item). **[Proposal]** `engine::session::Session` keeps, for every file, its `Analysis`, its header and its identity (path, size, mtime). Re-resolving needs only these, so a Mode change or an edit re-resolves with **no reload**. Decoded pixels are held in an LRU bounded by the memory budget and decoded again on demand (preview, packing pass 2, Export). A file whose size or mtime changed is reloaded and re-analysed. This replaces the carried option "keep `Analysis` in `Plan`".

### 4.5 `Session::preview`

**[Requirement]** Approach A: an in-memory preview whose bytes are exactly what Export writes. **[Proposal]** `Session::preview(&ResolvedJob, max_size, &CancelToken) -> Result<PreviewResult, FileError>` runs the same stages as `process_file` for one job, writes nothing to disk and touches no manifest. The GUI finds the job in its latest plan by job key (§4.3). `PreviewResult` holds:

- the source mip chain after Preprocess and Mipmaps, in the values the encoder receives, available as soon as the mips are built. For a packed output this is the assembled packed image (spec §6.4);
- the encoded block data of every mip whose larger side is ≤ `max_size` (§6.5), with the DXGI format;
- the `ValidationSummary` readouts for those mips, plus per-mip normal-length statistics for Normal-category jobs (new; `ValidationSummary` has none today).

**[Requirement]** For every mip it returns, the block data is byte-identical to the same mip of the DDS that `run` writes. BC encodes each mip independently from the same full mip chain, so skipping the mips above `max_size` keeps this exact.

### 4.6 Fixed per session

**[Finding]** `rgbcx::init` sets the BC1 decoder model as process-global state (spec §3.2, plan M3 known constraint). **[Proposal]** The GUI reads the model at launch; changing it in the Advanced drawer saves it and shows "restart sdc-gui to apply".

---

## 5. Data flow and saving

### 5.1 Opening a folder

**[Proposal]** Opening a folder (dialog, drag-and-drop or a recent folder) makes it the input root: `recursive = true`, project config `<root>/sdc.toml` (spec §8 rule 6), user config `%APPDATA%\sdc\config.toml`. The session plans it on a worker thread; the status line shows "Analysing 110 files…" and the thumbnails appear when planning finishes. (The engine emits no per-file event while it loads and analyses — per-stage events are M7 — so a running count is not available in M3a; plan ruling, reported to the owner at plan review.)

### 5.2 Editing and confirming

**[Requirement]** A settings edit becomes an exact-path override for that texture (§4.1); the session re-resolves that file only. **[Requirement]** Confirming a suggestion writes its value explicitly (for example `alpha.policy = "blend"` or `subtype = "pbr_albedo"`), so the engine and the CLI see a decision, not a guess. Acknowledging a blocking warning adds its code to that file's `allow`.

### 5.3 The `[files]` table in `sdc.toml`

**[Requirement]** Per-texture edits are saved as per-file overrides in the folder's `sdc.toml`, so `sdc convert <folder>` reproduces the GUI's export. **[Proposal]** `ConfigFile` gains a `files` table keyed by the path relative to the folder holding `sdc.toml`, with `/` separators and the exact file name; a packed output is keyed by its virtual path without extension (§4.1):

```toml
target = "cs"

[files."landscape/rock01_n.png"]
format = "bc5"
allow  = ["FMT-BC5-NORMAL"]

[files."landscape/rock01_d.png".alpha]
policy = "blend"

[files."landscape/rock01_rmaos".packing.fill]
metallic = 255
```

- Each entry holds any `FIELD_PATHS` field plus `allow`, a list of codes.
- **Precedence**, lowest to highest (spec §8): built-in → preset chain → user config → project config (`defaults`, `subtypes`) → preset rules → accepted suggestions → **`[files]` entries** → command-line `--set`. A CLI flag can therefore still override a GUI edit. The `[files]` entries are **their own layer**, applied before any command-line override; scope rank (§4.1) orders overrides only within one layer, so a GUI exact-path edit beats a hand-written `file:` glob in the same config, but never a command-line `--set`.
- An entry for a path that no longer exists stays in the file and raises one info event; the GUI lists such entries in the Advanced drawer so they can be removed.
- `preset.schema.json` and `config.schema.json` are regenerated; the `[files]` entries are typed as a partial `Profile`, which also delivers the carried "partial-Profile schema" item for `defaults`, `subtypes` and `overrides`.

### 5.4 Saving

- **[Requirement]** Ctrl+S saves. The ● marks unsaved changes, and closing the window with unsaved changes prompts Save / Discard / Cancel.
- **[Requirement]** Export saves first, so the folder's `sdc.toml` always matches the last export.
- **[Requirement]** Writes are format-preserving (`toml_edit`): comments, ordering and hand-written rules outside the keys the GUI owns survive. The GUI owns `target`, `[files.*]` and `defaults.packing.fill.*`.
- **[Requirement]** If `sdc.toml` changed on disk since the GUI loaded it, saving asks: reload (discarding GUI edits), overwrite, or cancel.

### 5.5 Export

**[Proposal]** Export calls `Session::run` on a worker thread with `on_existing = if-changed` (the CLI default), `accept = none`, the saved `sdc.toml`, and the export folder as output root. The bottom drawer opens (§2.5). While Export runs, live preview encoding pauses so the two do not compete for cores. The export folder, recent folders, window layout and the preview settings live in `%APPDATA%\sdc\gui.toml`, not in `sdc.toml`; **Copy CLI command** therefore includes `-o <export folder>`.

---

## 6. The 2D preview

### 6.1 Layout

**[Requirement]** Source and encoded, side by side or with a split slider; zoom and pan are linked. R / G / B / A isolation and alpha shown as a mask; a mip slider; a toggle that views the same bytes as `_SRGB` or as UNORM. The settings strip stays on one edge (§2.4).

### 6.2 Source side

**[Proposal]** Always available: the preprocessed source with its mip chain, from `PreviewResult` (§4.5). This step does no compression and is fast. It is uploaded as RGBA16F.

### 6.3 Encoded side: off by default

**[Requirement]** Preview encoding is **off by default**.

| State | What the encoded side shows |
|---|---|
| Live encode off, nothing encoded | "Not encoded" with an **[Encode now]** button (one-shot) |
| Encoded, then a setting changed (live off) | The last result, labelled **stale** |
| Live encode on | 300 ms after the last edit it re-encodes; a newer edit cancels the encode in flight. The previous result stays visible, dimmed, with "encoding…" |
| Done | The result, with its readouts |
| Failed or cancelled | The error message; nothing else is affected |

**[Proposal]** The **Live encode** toggle sits in the settings strip and in the preview toolbar; its last state is remembered in `gui.toml`, and a fresh install starts with it off.

### 6.4 GPU display and readouts

**[Proposal]** Encoded mips are uploaded to the GPU as **real BC blocks** (wgpu `TEXTURE_COMPRESSION_BC`) and drawn through an `egui_wgpu` paint callback (spec §9). If the adapter lacks BC support, `bcdec_rs` decodes on the CPU and a notice says so. **[Proposal]** Readouts come from `PreviewResult`: decoded smallest-mip alpha, share of α ≠ 255, coverage per mip, normal length (Normal category), BC6H peak ratio, and the minimum mip PSNR. A **difference heatmap** (source vs CPU-decoded encode, per selected channel) is computed on the CPU.

### 6.5 Size limits

- **[Requirement]** The preview has its own size limit, independent of the export size: **4096 by default**, because 8K encodes take a long time. This replaces the 8192 default of main spec §9 and DECISIONS #19 (owner, 2026-10-10); the plan's first task records it in DECISIONS. The preview encodes only the mips whose larger side is ≤ the limit (§4.5); the mip slider starts at the first such mip and notes "mip 0 (8192) not previewed: above the preview limit".
- **[Proposal]** The limit is a setting with the values 1024, 2048, 4096, 8192 and 16384. **[Requirement]** 16384 is offered only as an explicit setting (main spec §9).
- **[Proposal]** If the adapter's texture limit is below the chosen size, the preview starts at the first mip that fits and says so (spec §14 #13 stays open for the 16384 case).
- **[Proposal]** Thumbnails are CPU downscales of the source, cached in memory.

---

## 7. Errors and edge cases

1. **A file fails to load or plan.** **[Proposal]** A red ✗ thumbnail with the reason; the rest of the folder works; the report lists it.
2. **A preview encode fails or is cancelled.** The encoded side shows the message (§6.3). Export is unaffected.
3. **A source changed on disk.** Detected by size or mtime when the session next touches the file, and on window focus. The texture is flagged "changed on disk" with a **Reload** button that re-plans that file only. Export refuses a changed file (spec §8 rule 7).
4. **`sdc.toml` problems.** Unreadable or invalid on open: **[Finding]** the engine treats an invalid project config as a usage error before any file is read (spec §8 rule 5). **[Proposal]** The GUI therefore plans the folder **without** `sdc.toml`, shows a banner with the error and its line, and runs **read-only** (edits, saving and Export disabled) until the file is fixed and **Reload** succeeds. Nothing is overwritten. Unwritable on save: an error dialog; the edits stay in memory, marked unsaved.
5. **GPU limits.** No BC support: CPU decode with a notice. Preview size above the adapter limit: first mip that fits, with a notice. No usable adapter: the preview is disabled with a notice; everything else works.
6. **Engine panic in a worker.** **[Finding]** The engine already isolates per-file panics during planning and running. **[Proposal]** A panic in the GUI's own worker threads is caught at the thread boundary, shown as an error, and leaves the window running.

---

## 8. Testing

1. **Engine tasks, TDD as in M2.5.**
   - `Session::preview`: for each output format, the returned mips equal the same mips of `run`'s DDS, byte for byte, with and without the 4096 limit. Include BC7 with `exact_opaque` on, and a `cutout` source with coverage on, because those paths look at more than one block or mip.
   - `Scope::Path`: filenames containing `[`, `{` and `*`; a basename-only collision in two folders; packed outputs keyed without extension.
   - Per-file allow: allowing a code on one file leaves another file blocked.
   - Session cache: a Mode change re-resolves with zero loads (counted) for every file not consumed by a recipe; pack sources decode on demand for packing pass 2, because a target change swaps recipes. An mtime change forces one reload.
   - Event-schema bump: regenerated schemas, drift test, the new skip kinds and pack-source events.
2. **`sdc.toml` round trip.** Comments and hand-written rules survive a GUI save; `[files]` precedence sits above rules and below `--set`; **a GUI-saved folder run through `sdc convert` gives the same manifest as the GUI Export.**
3. **View-model tests (headless).** Selection, filters, edits, the undecided count, the Export dialog decision (§2.6), the preview state machine (§6.3) and the drawer, with no window.
4. **Rendering smoke tests** (`egui_kittest`). The main screens, the expanded preview and both drawers draw without panics, and key widgets exist. No pixel-exact screenshots.
5. **Exit: owner usability pass.** **[Requirement]** The owner opens `F:/ai/dds/mvp-test/src`, decides the 13 `pbr_albedo` alpha cases in the GUI, and exports.

---

## 9. Open items

1. **[Open]** Whether per-stage progress is pulled forward from M7. Shipped default: M3a shows per-file states only (§2.5). Trigger: the owner asks for a stage column after the usability pass.
2. **[Open]** The 16384 preview on adapters below that limit (main spec §14 #13). Shipped default: first mip that fits, with a notice.
3. **[Open]** Isolated-worker mode default (main spec §9, DECISIONS #18). Not built in M3a; the crash rate is recorded during the usability pass.

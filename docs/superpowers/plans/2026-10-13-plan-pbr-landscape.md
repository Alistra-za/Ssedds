# PBR Texture Sets and PBR Material Objects Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** sdc writes Community Shaders' `PBRTextureSets\<EDID>.json` and `PBRMaterialObjects\<EDID>.json` next to its PGPatcher export, from per-set blocks started from a built-in template library (Faultier's PBR landscape values), and the same texture-set block drives the set's PGPatcher entry when statics use its diffuse.

**Architecture:** Two optional blocks per set (`[sets.cs_txst]`, `[sets.cs_mato]`) live in `sdc-project.toml`; `SetsDoc` reads them, overlays the texture-set block's mapped values onto the set's PGPatcher settings and redirects PGPatcher edits of those keys into the block (one source, no drift). A new writer (`model/cs_pbr_export.rs`) turns each block into one strict JSON file per EDID at the end of a clean export, reusing `pbr_export.rs`'s atomic write and SHA-256 hand-edit rule with a per-file hash record. Scan reads (statics, EDID checks, the MATO picker) are cached per session in the model and read on demand; the 3D preview locks a texture-set set to the plane with a preview-only tiling factor applied after the pgpsim patch.

**Tech Stack:** Rust 2021 workspace `sdc` (egui 0.36 / eframe, toml_edit 0.25, serde_json, rusqlite via `modscan`), `egui_kittest` UI tests, Python 3 (stdlib) for the one-off template generator.

**Spec:** `F:\ai\dds\nifparseref\plan\pbr-landscape\2026-10-13-pbr-landscape-design.md` (APPROVED 2026-10-13). Evidence: `F:\ai\dds\nifparseref\plan\pbr-landscape\01-research.md`.

## Global Constraints

- Work on a new branch `nif/j-cspbr` cut from `nif/main` at `54a7a1c`, in its own worktree `F:\ai\dds\converter-nif-cspbr` (created in Task 1, Step 0). Every command below runs in that folder.
- Never push. Never merge to `main`. Never touch `F:\ai\dds\converter` (no checkout, cargo or git commands run there).
- No new crates or dependencies: `Cargo.lock` must stay unchanged. Workspace `rust-version = "1.88"` (sdc-gui keeps its own `1.95`, eframe's floor).
- Owner-approved edits to otherwise off-limits files: `crates/gui/src/sets_doc.rs`, `crates/gui/src/model/pbr_export.rs`, `crates/gui/src/pgp/writer.rs` (this plan ends up not needing `writer.rs`; see Task 3). Still off-limits: `crates/engine`, `crates/cli`, `crates/encoders-sys`, every other `crates/gui/src/pgp/*` file, `crates/gui/src/model/sets.rs`, `schema/`, golden and `gui_repro` tests.
- `docs/DECISIONS.md`: one new note, **Note AD** (two headings already say "Note AC"; AD is the next free letter), appended at the end in one commit.
- Shared files are add-only (no reordering or reformatting), each in its own commit: root `Cargo.toml`, `Cargo.lock`, `crates/gui/Cargo.toml`, `crates/gui/src/ui/mod.rs`, `crates/gui/src/model/mod.rs`, `crates/gui/src/app.rs`, `crates/gui/src/settings.rs`, and the set editor mount in `crates/gui/src/ui/set_editor.rs`.
- Tests never read real owner data except under `#[ignore]` plus the `SDC_ACHERUS` env var, and never open the real `%LOCALAPPDATA%\sdc\scan.db`. The resource-folder equality test is `#[ignore]` too (env var `SDC_PBR_RESOURCE`).
- A scan-DB schema change bumps `modscan::store::SCHEMA_VERSION` (6 → 7), so cached databases rebuild.
- Credit, exact text, in `docs/CREDITS.md` and `docs/gui.md`: "PBR landscape template values by Faultier (https://www.nexusmods.com/profile/Faultierfan?gameId=1704), from Faultier's PBR landscape modders resource." No credit inside exported JSON.
- Build and test: `cargo test -p <crate> --release` (the target dir is `target\x86_64-pc-windows-msvc` via `.cargo/config.toml`). Full gate before the last commit of every task: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --release -- -D warnings`, `cargo test --workspace --release`.
- Commit only explicit paths (`git add <path>…`, never `-A` or `.`). Commit messages end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- Never run `python -` with stdin; write a script file and run it with `python -I`.

## Review Focus

- A block added to the first of several `[[sets]]` must stay under that set after save and reload (toml_edit table placement). Owner: Task 3, test `a_block_added_to_the_first_set_stays_with_it`.
- An EDID whose case the user corrects (W2 fix) must rename sdc's own file to the new case (CS keys on the exact stem) and must not be reported as an orphan. Owner: Task 5, test `an_edid_whose_case_changed_renames_sdcs_own_file`.
- A file already in the export folder that sdc never wrote (for example Faultier's own JSON copied in) must never be overwritten. Owner: Task 5, test `a_hand_edited_or_foreign_file_is_kept_and_reported`.
- Two sets binding EDIDs that differ only in case collide on Windows: both must be refused, naming both sets. Owner: Task 5, test `duplicate_and_invalid_edids_are_refused_per_file`.
- An unreadable or old-schema scan database must not crash or write a statics entry: the set says why and no PGPatcher entry is written (unless `statics = "always"`). Owner: Task 6, test `an_unusable_scan_db_writes_no_statics_entry_unless_always`.
- A set that already has PGPatcher values (roughness, glint …) must not lose them when a texture-set block is added: they move into the block. Owner: Task 3, test `adding_a_texture_set_block_moves_the_sets_own_mapped_values_into_it`.

## Rulings made by this plan (record in Note AD, Task 10)

- **R1 Plane lock (deviation from spec §7, flag to owner).** "Landscape" for the preview = a set with a `cs_txst` block only. The spec's second rule ("or a set whose path starts with `landscape\`") is not implemented: the shared GUI test fixture's only PBR set is `landscape/rock01`, so the path rule would turn about ten existing set-preview tests (NIF candidates, Sphere fallback, Cube pick, LE notice, skip badges) into failures, and a landscape texture used by statics (rocks01: 278 meshes) is better previewed on those meshes until the user adds a texture-set block.
- **R2 Template names** = the first EDID of the group alphabetically. The spec's "most common EDID prefix" gives `Landscape` or an empty string for 8 of the 34 groups (dedup run on the resource folder, 2026-10-13: 31 texture-set and 3 material-object groups, matching O4); where the common prefix is itself an EDID of the group it is that same first EDID.
- **R3 Key order** in the CS JSON is alphabetical: CS's own saver (nlohmann::json) sorts object keys, as Faultier's 9 editor dumps and all 31 material-object files show. Lines end in `\n`, no final newline, 4-space indent.
- **R4 Hand-edit guard per file:** hashes in `[project] cs_pbr_sha256 = { "PBRTextureSets/<EDID>.json" = "<sha>" }`. A file edited outside sdc (or present without a record and different) is kept and reported in the export notes, never asked about: the app has one dialog slot and the PGPatcher JSON may already be using it. The record's keys that no block binds any more are the orphan report.
- **R5 Refusals are per file:** an invalid or duplicate EDID skips that file with a note (and a red line in the editor); `export_blocks` is not changed. Duplicates compare case-insensitively (Windows file names).
- **R6 Statics query** = the existing `Store::nifs_using_texture(set_key(v), …)` (PGPatcher's own match rule on winning copies, derived winners read through their source) with `nif::path::is_lod_path` removing LOD meshes, distinct paths. No new modscan join.
- **R7 PGPatcher mapping** is done in `SetsDoc` (overlay on read, redirect on write), so `pgp/writer.rs` is unchanged. `glint_on` follows `glintParameters.enabled`; the `subsurface` and `multilayer` switches stay the user's (they need `_s`/`_cnr` textures and NIF flags a texture set cannot know; turning them on automatically would block export for a set without those textures).
- **R8 Template EDIDs:** applying a template to a block with no EDIDs adds the template's EDIDs in the same undo step; a block with EDIDs keeps them untouched.
- **R9 Material-object `specularLevel`** when the block has none is written as `0.04` (the material reset value).
- **R10 Scan reads** run on the UI thread on demand (section open, export start), cached per session; "Refresh scan" forgets them. `shortcut:` noted in code.

---

## File Structure

| File | Status | Responsibility |
|---|---|---|
| `crates/esp/src/lib.rs`, `crates/esp/src/records.rs` | modify | `Payload::mato_flags` from MATO `DATA` |
| `crates/modscan/src/store.rs` | modify | `rec.mato_flags` column, `SCHEMA_VERSION = 7` |
| `crates/modscan/src/plugin_store.rs` | modify | write `mato_flags`; `WinRec`, `Store::winning_records(sig)` |
| `crates/modscan/tests/mato.rs` | create | MATO query tests (+ `#[ignore]` owner test, Task 10) |
| `crates/modscan/tests/store.rs` | modify | schema version 6 → 7 in two asserts |
| `tools/gen-cs-pbr-templates.py` | create | one-off generator of the template library |
| `crates/gui/assets/cs_pbr_templates.json` | create (generated) | 34 templates |
| `crates/gui/src/model/cs_pbr.rs` | create | kinds, field tables, templates, blocks, PGPatcher mapping, model edit API, scan cache, statics rule, EDID checks |
| `crates/gui/src/model/cs_pbr_export.rs` | create | JSON text, refusals, pending files, finish (write, guard, orphans) |
| `crates/gui/src/ui/cs_pbr.rs` | create | set-editor section |
| `crates/gui/src/sets_doc.rs` | modify (owner OK) | reserved keys, block read/overlay, block writes, edit redirect, hash record |
| `crates/gui/src/model/pbr_export.rs` | modify (owner OK) | `file_name_problem`, `hand_edited`, `write_atomic` `pub(crate)`; statics filter in `prepare` |
| `crates/gui/src/model/mod.rs` | modify (shared, add-only) | module lines, `cs_pbr` + `pending_cs` fields, export hooks |
| `crates/gui/src/ui/mod.rs`, `crates/gui/src/ui/set_editor.rs` | modify (shared, add-only) | mount the section |
| `crates/gui/src/model/set_preview.rs`, `crates/gui/src/ui/set_preview.rs` | modify | plane lock, tiling |
| `crates/gui/tests/cs_pbr.rs`, `cs_pbr_export.rs`, `cs_pbr_ui.rs` | create | tests |
| `crates/gui/tests/set_preview_model.rs`, `set_preview_ui.rs` | modify | `landscape` field in a literal; new preview tests |
| `docs/CREDITS.md`, `docs/gui.md`, `docs/DECISIONS.md` | modify | credit, user guide, Note AD |

---

### Task 1: MATO single-pass flags in the scan database

**Files:**
- Modify: `crates/esp/src/lib.rs:55-67` (struct `Payload`)
- Modify: `crates/esp/src/records.rs:59-74` (match in `read`)
- Modify: `crates/modscan/src/store.rs:11` and `:60-61` (schema)
- Modify: `crates/modscan/src/plugin_store.rs:150-161` (insert), end of the second `impl Store` block (after `unused_textures`, ~line 709)
- Modify: `crates/modscan/tests/store.rs:226`, `:584`
- Test: `crates/esp/tests/records.rs` (append), `crates/modscan/tests/mato.rs` (create)

**Interfaces:**
- Consumes: nothing.
- Produces: `esp::Payload::mato_flags: Option<u32>`; `modscan::plugin_store::WinRec { formid: u32, edid: String, plugin: String, mato_flags: Option<u32> }`; `modscan::store::Store::winning_records(&self, sig: &str) -> Result<Vec<WinRec>, ScanError>` (winning copies with an EDID, ordered by EDID then FormID).

- [ ] **Step 0: Create the worktree and branch**

```bash
git -C F:/ai/dds/converter-nif-p1 worktree add F:/ai/dds/converter-nif-cspbr -b nif/j-cspbr 54a7a1c
cd F:/ai/dds/converter-nif-cspbr
git log --oneline -1
```
Expected: `54a7a1c Merge pull request #8 …`.

- [ ] **Step 1: Write the failing esp test** (append to `crates/esp/tests/records.rs`)

```rust
/// Design §6 W3: a 52-byte MATO DATA ends in the flags u32 (bit 0 = single pass); a 48-byte one has none.
#[test]
fn mato_data_flags_are_read_when_present() {
    let data = |n: usize, flags: u32| {
        let mut d = vec![0u8; n];
        if n >= 52 {
            d[48..52].copy_from_slice(&flags.to_le_bytes());
        }
        d
    };
    let r = one(
        b"MATO",
        rec(b"MATO", 0x0100_0A00, &[edid("SnowMat1P"), sub(b"DATA", &data(52, 1))]),
    );
    assert_eq!(r.payload.mato_flags, Some(1));
    let r = one(
        b"MATO",
        rec(b"MATO", 0x0100_0A01, &[edid("Ash1P"), sub(b"DATA", &data(48, 0))]),
    );
    assert_eq!(r.payload.mato_flags, None);
    let r = one(b"MATO", rec(b"MATO", 0x0100_0A02, &[edid("NoData")]));
    assert_eq!(r.payload.mato_flags, None);
}
```

- [ ] **Step 2: Run it to see it fail**

Run: `cargo test -p esp --release --test records mato_data_flags`
Expected: compile error `no field mato_flags on type Payload`.

- [ ] **Step 3: Implement**

`crates/esp/src/lib.rs`, in `pub struct Payload` after `hdpt_flags`:

```rust
    /// MATO DATA flags (the u32 after the single-pass colour; bit 0 = Single Pass); `None` when DATA is absent or
    /// 48 bytes long (older records stop before the flags).
    pub mato_flags: Option<u32>,
```

`crates/esp/src/records.rs`, in `read`, right after the arm `b"HDPT" if &sig == b"DATA" => p.hdpt_flags = d.first().copied(),`:

```rust
            b"MATO" if &sig == b"DATA" => p.mato_flags = (d.len() >= 52).then(|| u32_at(d, 48)),
```

- [ ] **Step 4: Run it to see it pass**

Run: `cargo test -p esp --release`
Expected: all esp tests PASS.

- [ ] **Step 5: Write the failing modscan test** (create `crates/modscan/tests/mato.rs`)

```rust
//! Design §6: winning MATO records with the single-pass flags of the winning copy.
#[path = "../../esp/tests/common/mod.rs"]
mod b;
mod common;
use b::{edid, plugin, rec, sub, top};
use common::{instance_fixture, put};
use modscan::mo2::Instance;
use modscan::scan::{run, Source};
use modscan::store::Store;
use std::sync::atomic::AtomicBool;

fn data(n: usize, flags: u32) -> Vec<u8> {
    let mut d = vec![0u8; n];
    if n >= 52 {
        d[48..52].copy_from_slice(&flags.to_le_bytes());
    }
    d
}

fn mato(id: u32, name: &str, n: usize, flags: u32) -> Vec<u8> {
    rec(b"MATO", id, &[edid(name), sub(b"DATA", &data(n, flags))])
}

#[test]
fn winning_matos_carry_the_winners_data_flags() {
    let d = instance_fixture("+A\n");
    let r = d.path();
    put(
        r,
        "Game/Data/Skyrim.esm",
        &plugin(
            esp::FLAG_ESM,
            &[],
            &[top(
                b"MATO",
                &[
                    mato(0xA00, "SnowMat1P", 52, 1),
                    mato(0xA01, "AshMat1P", 48, 0),
                    mato(0xA02, "PlainMat", 52, 0),
                ],
            )],
        ),
    );
    // A.esp makes PlainMat single-pass: the winning copy's flags count.
    put(
        r,
        "mods/A/A.esp",
        &plugin(0, &["Skyrim.esm"], &[top(b"MATO", &[mato(0xA02, "PlainMat", 52, 1)])]),
    );
    put(r, "profiles/Default/plugins.txt", b"*A.esp\n");
    let mut s = Store::open(&r.join("scan.db")).unwrap();
    let src = Source::Mo2(Instance::load(r, None).unwrap());
    run(&src, &mut s, &AtomicBool::new(false), &|_| {}, &|| vec![]).unwrap();
    let got: Vec<(String, String, Option<u32>)> = s
        .winning_records("MATO")
        .unwrap()
        .into_iter()
        .map(|w| (w.edid, w.plugin, w.mato_flags))
        .collect();
    assert_eq!(
        got,
        vec![
            ("AshMat1P".into(), "Skyrim.esm".into(), None),
            ("PlainMat".into(), "A.esp".into(), Some(1)),
            ("SnowMat1P".into(), "Skyrim.esm".into(), Some(1)),
        ]
    );
    assert!(s.winning_records("TXST").unwrap().is_empty());
}
```

- [ ] **Step 6: Run it to see it fail**

Run: `cargo test -p modscan --release --test mato`
Expected: compile error `no method named winning_records`.

- [ ] **Step 7: Implement the schema, insert and query**

`crates/modscan/src/store.rs`: change line 11 to `pub const SCHEMA_VERSION: i64 = 7;` and the `rec` table (lines 60-61) to:

```sql
-- rec.mato_flags: the MATO DATA flags of that copy (bit 0 single pass), NULL when its DATA has no flags field.
CREATE TABLE rec (formid INTEGER, local INTEGER NOT NULL, plugin_id INTEGER NOT NULL, sig TEXT NOT NULL, edid TEXT,
                     owner_ok INTEGER NOT NULL DEFAULT 0, txst_flags INTEGER, decal INTEGER, hdpt_flags INTEGER,
                     mato_flags INTEGER);
```

`crates/modscan/src/plugin_store.rs`, the `rec` insert in `write_plugin`:

```rust
            tx.prepare_cached(
                "INSERT INTO rec(local, plugin_id, sig, edid, txst_flags, decal, hdpt_flags, mato_flags) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            )?
            .execute(params![
                local,
                id,
                sig_text(&r.sig),
                r.edid,
                t.map(|t| i64::from(t.flags)),
                t.map(|t| t.decal),
                r.payload.hdpt_flags.map(i64::from),
                r.payload.mato_flags.map(i64::from)
            ])?;
```

Next to `pub struct MissingRef` add:

```rust
/// A winning record of one type that has an EDID (design §6): the material-object picker and the EDID checks.
#[derive(Debug, Clone, PartialEq)]
pub struct WinRec {
    pub formid: u32,
    pub edid: String,
    /// The winning plugin.
    pub plugin: String,
    /// MATO `DATA` flags of the winning copy (bit 0 = single pass); `None` when its DATA has no flags field.
    pub mato_flags: Option<u32>,
}
```

At the end of the second `impl Store` block (after `unused_textures`):

```rust
    /// Winning records of type `sig` (`"TXST"`, `"MATO"`) that have an EDID, by EDID then FormID.
    pub fn winning_records(&self, sig: &str) -> Result<Vec<WinRec>, ScanError> {
        let mut st = self.conn.prepare(
            "SELECT w.formid, r.edid, pl.name, r.mato_flags FROM rec r
             JOIN rec_winner w ON w.formid = r.formid AND w.plugin_id = r.plugin_id
             JOIN plugin pl ON pl.id = r.plugin_id
             WHERE r.sig = ?1 AND r.owner_ok = 1 AND r.edid IS NOT NULL
             ORDER BY r.edid, w.formid",
        )?;
        let v = st
            .query_map([sig], |r| {
                Ok(WinRec {
                    formid: r.get::<_, i64>(0)? as u32,
                    edid: r.get(1)?,
                    plugin: r.get(2)?,
                    mato_flags: r.get::<_, Option<i64>>(3)?.map(|f| f as u32),
                })
            })?
            .collect::<Result<_, _>>()?;
        Ok(v)
    }
```

`crates/modscan/tests/store.rs`: line 226 `assert_eq!(v, 6);` → `assert_eq!(v, 7);`; line 584 `want: 6` → `want: 7`.

- [ ] **Step 8: Run the tests**

Run: `cargo test -p modscan --release` and `cargo test -p sdc-gui --release --test set_plugin_refs --test complex_grass`
Expected: PASS (the gui seeds insert `rec` by named columns; the new column is nullable).

- [ ] **Step 9: Gate and commit**

```bash
cargo fmt --all --check && cargo clippy --workspace --all-targets --release -- -D warnings && cargo test --workspace --release
git add crates/esp/src/lib.rs crates/esp/src/records.rs crates/esp/tests/records.rs crates/modscan/src/store.rs crates/modscan/src/plugin_store.rs crates/modscan/tests/store.rs crates/modscan/tests/mato.rs
git commit -m "feat(modscan): MATO single-pass flags and winning-record query (schema 7)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: Built-in template library

**Files:**
- Create: `tools/gen-cs-pbr-templates.py`
- Create (generated): `crates/gui/assets/cs_pbr_templates.json` (`.gitattributes` already forces LF for `crates/gui/assets/*.json`)
- Create: `crates/gui/src/model/cs_pbr.rs`
- Modify (shared, add-only, own commit): `crates/gui/src/model/mod.rs:2` (module list)
- Test: `crates/gui/tests/cs_pbr.rs` (create)

**Interfaces:**
- Consumes: nothing.
- Produces (in `sdc_gui::model::cs_pbr`): `enum Kind { Txst, Mato }` with `key() -> &'static str` (`"cs_txst"`/`"cs_mato"`), `folder()` (`"PBRTextureSets"`/`"PBRMaterialObjects"`), `label()` (`"PBR texture set"`/`"PBR material object"`); `enum FieldKind { Num, Rgb, Glint }`; `struct Field { name, kind, range: (f64, f64), default: f64 }`; `TXST_FIELDS: [Field; 13]`, `MATO_FIELDS: [Field; 4]`, `GLINT_FIELDS: [Field; 4]`; `fn fields(Kind) -> &'static [Field]`; `fn field(Kind, &str) -> Option<&'static Field>`; `fn valid_value(FieldKind, &Json) -> bool`; `struct Template { name: String, kind: Kind, edids: Vec<String>, values: Map<String, Json> }`; `fn templates() -> &'static [Template]`; `fn template(Kind, &str) -> Option<&'static Template>`; `fn floats(&Json) -> Json`.

- [ ] **Step 1: Write the generator** (`tools/gen-cs-pbr-templates.py`)

```python
"""Writes crates/gui/assets/cs_pbr_templates.json from Faultier's PBR landscape modders resource (design §4).

Run once by hand (not part of the build):
    python -I tools/gen-cs-pbr-templates.py "F:\\ai\\dds\\PBR landscape modders resource"
Files whose values are equal after parsing (ints read as floats) become one template; it keeps every EDID it was
used for. Its name is the group's first EDID alphabetically (plan ruling R2).
"""
import json
import os
import sys
from collections import defaultdict

KINDS = (("txst", "PBRTextureSets"), ("mato", "PBRMaterialObjects"))


def floats(v):
    if isinstance(v, bool):
        return v
    if isinstance(v, (int, float)):
        return float(v)
    if isinstance(v, list):
        return [floats(x) for x in v]
    if isinstance(v, dict):
        return {k: floats(x) for k, x in v.items()}
    return v


def main(res, out):
    templates = []
    for kind, folder in KINDS:
        groups = defaultdict(list)
        values = {}
        for f in sorted(os.listdir(os.path.join(res, folder))):
            if not f.lower().endswith(".json"):
                continue
            with open(os.path.join(res, folder, f), encoding="utf-8") as h:
                v = floats(json.load(h))
            key = json.dumps(v, sort_keys=True)
            groups[key].append(f[:-5])
            values[key] = v
        for key, edids in groups.items():
            edids.sort()
            templates.append({"name": edids[0], "kind": kind, "edids": edids, "values": values[key]})
    templates.sort(key=lambda t: (t["kind"] != "txst", t["name"]))
    names = [t["name"] for t in templates]
    assert len(names) == len(set(names)), "template names must be unique"
    with open(out, "w", encoding="utf-8", newline="\n") as h:
        json.dump(templates, h, indent=2, sort_keys=True)
        h.write("\n")
    print({k: sum(t["kind"] == k for t in templates) for k, _ in KINDS})


if __name__ == "__main__":
    here = os.path.dirname(os.path.abspath(__file__))
    main(sys.argv[1], os.path.join(here, "..", "crates", "gui", "assets", "cs_pbr_templates.json"))
```

- [ ] **Step 2: Generate the library**

Run: `python -I tools/gen-cs-pbr-templates.py "F:\ai\dds\PBR landscape modders resource"`
Expected output: `{'txst': 31, 'mato': 3}`; `crates/gui/assets/cs_pbr_templates.json` exists (the resource folder is only read).

- [ ] **Step 3: Write the failing test** (create `crates/gui/tests/cs_pbr.rs`)

```rust
//! Community Shaders PBR blocks (design 2026-10-13): the template library, blocks in sdc-project.toml, the
//! PGPatcher mapping, the statics rule and the EDID checks.
mod support;
use sdc_gui::model::cs_pbr::*;
use serde_json::{json, Value as Json};
use std::path::PathBuf;

#[test]
fn the_library_parses_with_31_texture_sets_and_3_material_objects() {
    let t = templates();
    assert_eq!(t.iter().filter(|t| t.kind == Kind::Txst).count(), 31);
    assert_eq!(t.iter().filter(|t| t.kind == Kind::Mato).count(), 3);
    assert_eq!(t.iter().map(|t| t.edids.len()).sum::<usize>(), 73 + 31);
    let names: std::collections::BTreeSet<_> = t.iter().map(|t| (t.kind, &t.name)).collect();
    assert_eq!(names.len(), t.len(), "names are unique per kind");
    // Applying any template writes only known fields with usable values.
    for x in t {
        assert!(x.edids.contains(&x.name), "{}", x.name);
        for (k, v) in &x.values {
            let f = field(x.kind, k).unwrap_or_else(|| panic!("{}: unknown field {k}", x.name));
            assert!(valid_value(f.kind, v), "{}: {k} = {v}", x.name);
        }
    }
    let snow = template(Kind::Txst, "LandscapeSnow01").unwrap();
    assert_eq!(snow.edids, ["LandscapeSnow01", "LandscapeSnow01Landscape"]);
    assert_eq!(snow.values["glintParameters"]["enabled"], json!(true));
}

#[test]
fn floats_turn_every_number_into_a_float() {
    assert_eq!(
        floats(&json!({"a": [1, 0.5], "b": {"c": 16}, "d": true})).to_string(),
        r#"{"a":[1.0,0.5],"b":{"c":16.0},"d":true}"#
    );
}

/// Every resource JSON's values equal its template's values (design §9). Reads the resource folder, read-only.
/// `$env:SDC_PBR_RESOURCE='F:\ai\dds\PBR landscape modders resource'; cargo test -p sdc-gui --release --test cs_pbr -- --ignored`
#[test]
#[ignore]
fn every_resource_file_equals_its_template() {
    let root = PathBuf::from(
        std::env::var("SDC_PBR_RESOURCE").expect("set SDC_PBR_RESOURCE to the resource folder"),
    );
    let mut n = 0;
    for kind in [Kind::Txst, Kind::Mato] {
        for e in std::fs::read_dir(root.join(kind.folder())).unwrap() {
            let p = e.unwrap().path();
            let edid = p.file_stem().unwrap().to_str().unwrap().to_string();
            let v: Json = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
            let t = templates()
                .iter()
                .find(|t| t.kind == kind && t.edids.contains(&edid))
                .unwrap_or_else(|| panic!("no template lists {edid}"));
            assert_eq!(floats(&v), floats(&Json::Object(t.values.clone())), "{edid}");
            n += 1;
        }
    }
    assert_eq!(n, 104);
}
```

- [ ] **Step 4: Run it to see it fail**

Run: `cargo test -p sdc-gui --release --test cs_pbr`
Expected: compile error `could not find cs_pbr in model`.

- [ ] **Step 5: Implement** (create `crates/gui/src/model/cs_pbr.rs`)

```rust
//! Community Shaders PBR texture sets and material objects (design 2026-10-13): the field tables, the built-in
//! template library, the per-set blocks, the PGPatcher mapping, the scan reads and the EDID checks. No egui types.
use serde::Deserialize;
use serde_json::{Map, Value as Json};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Txst,
    Mato,
}

impl Kind {
    /// The block's key in a `[[sets]]` table.
    pub fn key(self) -> &'static str {
        match self {
            Kind::Txst => "cs_txst",
            Kind::Mato => "cs_mato",
        }
    }
    /// The folder CS reads under `Data` (sdc writes it under the export root).
    pub fn folder(self) -> &'static str {
        match self {
            Kind::Txst => "PBRTextureSets",
            Kind::Mato => "PBRMaterialObjects",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Kind::Txst => "PBR texture set",
            Kind::Mato => "PBR material object",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    Num,
    /// Three floats.
    Rgb,
    /// `glintParameters { enabled, screenSpaceScale, logMicrofacetDensity, microfacetRoughness, densityRandomization }`.
    Glint,
}

/// One CS field (`TruePBR.h`): its in-game slider range (the editor's drag range; JSON is not clamped) and CS's
/// default (a colour's default is each channel's).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Field {
    pub name: &'static str,
    pub kind: FieldKind,
    pub range: (f64, f64),
    pub default: f64,
}

const fn f(name: &'static str, kind: FieldKind, lo: f64, hi: f64, default: f64) -> Field {
    Field {
        name,
        kind,
        range: (lo, hi),
        default,
    }
}

use FieldKind::{Glint, Num, Rgb};

/// Texture-set fields, in CS's struct order (research §2).
pub const TXST_FIELDS: [Field; 13] = [
    f("roughnessScale", Num, 0.0, 3.0, 1.0),
    f("displacementScale", Num, 0.0, 3.0, 1.0),
    f("specularLevel", Num, 0.0, 3.0, 0.04),
    f("subsurfaceColor", Rgb, 0.0, 1.0, 0.0),
    f("subsurfaceOpacity", Num, 0.0, 1.0, 0.0),
    f("coatColor", Rgb, 0.0, 1.0, 1.0),
    f("coatStrength", Num, 0.0, 1.0, 1.0),
    f("coatRoughness", Num, 0.0, 1.0, 1.0),
    f("coatSpecularLevel", Num, 0.0, 1.0, 0.04),
    f("innerLayerDisplacementOffset", Num, 0.0, 3.0, 0.0),
    f("fuzzColor", Rgb, 0.0, 1.0, 0.0),
    f("fuzzWeight", Num, 0.0, 1.0, 0.0),
    f("glintParameters", Glint, 0.0, 0.0, 0.0),
];

/// Material-object fields. `roughness` is absolute, not a scale; CS's `specularLevel` default is 1.0.
pub const MATO_FIELDS: [Field; 4] = [
    f("baseColorScale", Rgb, 0.0, 10.0, 1.0),
    f("roughness", Num, 0.0, 1.0, 1.0),
    f("specularLevel", Num, 0.0, 1.0, 1.0),
    f("glintParameters", Glint, 0.0, 0.0, 0.0),
];

/// The numeric glint fields (`enabled`, a bool defaulting to false, is not listed).
pub const GLINT_FIELDS: [Field; 4] = [
    f("screenSpaceScale", Num, 0.0, 3.0, 1.5),
    f("logMicrofacetDensity", Num, 0.0, 40.0, 40.0),
    f("microfacetRoughness", Num, 0.0, 1.0, 0.015),
    f("densityRandomization", Num, 0.0, 5.0, 2.0),
];

pub fn fields(kind: Kind) -> &'static [Field] {
    match kind {
        Kind::Txst => &TXST_FIELDS,
        Kind::Mato => &MATO_FIELDS,
    }
}

pub fn field(kind: Kind, name: &str) -> Option<&'static Field> {
    fields(kind).iter().find(|f| f.name == name)
}

/// Whether `v` has the type CS reads for a field of kind `fk` (unknown glint sub-keys are allowed: never written).
pub fn valid_value(fk: FieldKind, v: &Json) -> bool {
    match fk {
        FieldKind::Num => v.is_number(),
        FieldKind::Rgb => v
            .as_array()
            .is_some_and(|a| a.len() == 3 && a.iter().all(Json::is_number)),
        FieldKind::Glint => v.as_object().is_some_and(|o| {
            o.iter().all(|(k, x)| match k.as_str() {
                "enabled" => x.is_boolean(),
                k if GLINT_FIELDS.iter().any(|g| g.name == k) => x.is_number(),
                _ => true,
            })
        }),
    }
}

/// One built-in template (design §4).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Template {
    pub name: String,
    pub kind: Kind,
    /// The EDIDs Faultier used these values for.
    pub edids: Vec<String>,
    pub values: Map<String, Json>,
}

/// The library: Faultier's PBR landscape modders resource, deduplicated by `tools/gen-cs-pbr-templates.py`. A
/// parse failure is a test failure (`tests/cs_pbr.rs`), never a runtime path.
pub fn templates() -> &'static [Template] {
    static LIB: std::sync::OnceLock<Vec<Template>> = std::sync::OnceLock::new();
    LIB.get_or_init(|| {
        serde_json::from_str(include_str!("../../assets/cs_pbr_templates.json"))
            .expect("cs_pbr_templates.json parses")
    })
}

pub fn template(kind: Kind, name: &str) -> Option<&'static Template> {
    templates().iter().find(|t| t.kind == kind && t.name == name)
}

/// Every number as a float (`16` → `16.0`): CS silently drops a colour array of int literals (research §2).
pub fn floats(v: &Json) -> Json {
    match v {
        Json::Number(n) => n.as_f64().map_or(Json::Null, |x| serde_json::json!(x)),
        Json::Array(a) => Json::Array(a.iter().map(floats).collect()),
        Json::Object(o) => Json::Object(o.iter().map(|(k, x)| (k.clone(), floats(x))).collect()),
        _ => v.clone(),
    }
}
```

- [ ] **Step 6: Commit the new files** (the module is not compiled yet, so this commit builds)

```bash
git add tools/gen-cs-pbr-templates.py crates/gui/assets/cs_pbr_templates.json crates/gui/src/model/cs_pbr.rs crates/gui/tests/cs_pbr.rs
git commit -m "feat(gui): CS PBR template library from Faultier's resource

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

- [ ] **Step 7: Mount the module** (shared file, add-only). In `crates/gui/src/model/mod.rs` add after `pub mod bulk;`:

```rust
pub mod cs_pbr;
```

- [ ] **Step 8: Run the tests**

Run: `cargo test -p sdc-gui --release --test cs_pbr`
Expected: 2 PASS, 1 ignored. Then, once: `$env:SDC_PBR_RESOURCE='F:\ai\dds\PBR landscape modders resource'; cargo test -p sdc-gui --release --test cs_pbr -- --ignored` → PASS.

- [ ] **Step 9: Gate and commit the shared line**

```bash
cargo fmt --all --check && cargo clippy --workspace --all-targets --release -- -D warnings && cargo test --workspace --release
git add crates/gui/src/model/mod.rs
git commit -m "chore(gui): mount model::cs_pbr

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: Blocks in sdc-project.toml and the PGPatcher mapping

**Files:**
- Modify: `crates/gui/src/model/cs_pbr.rs` (append)
- Modify (owner OK): `crates/gui/src/sets_doc.rs:4-12` (imports), `:18-28` (`RESERVED`), `:30-47` (`SetEntry`), `:94-150` (`entry_of`), `:706-716` (`set_value`), new methods and helper
- Test: `crates/gui/tests/cs_pbr.rs` (append)

**Interfaces:**
- Consumes: Task 2 `Kind`, `field`, `fields`, `valid_value`, `floats`, `Template`, `template`.
- Produces (in `model::cs_pbr`): `enum Statics { Auto, Always, Never }` with `parse(Option<&str>)`, `as_str() -> Option<&'static str>`; `struct CsBlock { edids: Vec<String>, template: Option<String>, statics: Statics, values: Map<String, Json>, bad: Vec<String> }`; `pub(crate) fn block_of(&dyn TableLike, Kind) -> CsBlock`; `PGP_MAP: [(&str, &str); 14]`; `fn cs_to_pgp(&Map<String, Json>) -> Settings`; `fn pgp_to_cs(&str, Option<&Json>) -> Option<(&'static str, Option<Json>)>`; `fn overlay(&mut Settings, &CsBlock)`; `impl AppModel { cs_block(&self, &str, Kind) -> Option<CsBlock>; add_cs_block(&mut self, &str, Kind); remove_cs_block(&mut self, &str, Kind); set_cs_value(&mut self, &str, Kind, &str, Option<Json>); set_cs_edids(&mut self, &str, Kind, &[String]); apply_cs_template(&mut self, &str, Kind, &str) -> Result<(), String>; set_cs_statics(&mut self, &str, Statics) }`.
- Produces (in `sets_doc`): `SetEntry::cs_txst`, `SetEntry::cs_mato: Option<CsBlock>`; `SetsDoc::set_cs_value(&mut self, id: &str, kind: Kind, key: &str, v: Option<Json>)`, `add_cs_block(&mut self, &str, Kind)` (moves the set's mapped flat keys into a new texture-set block), `remove_cs_block(&mut self, &str, Kind)`, `apply_cs_template(&mut self, &str, &Template)`.

- [ ] **Step 1: Write the failing tests** (append to `crates/gui/tests/cs_pbr.rs`)

```rust
use sdc_gui::sets_doc::SetsDoc;

fn doc(text: &str) -> (tempfile::TempDir, SetsDoc) {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("sdc-project.toml"), text).unwrap();
    let s = SetsDoc::open(d.path());
    (d, s)
}

const TWO_SETS: &str = "# notes\n[[sets]]\nid = \"landscape/snow\"\nparallax = true\nroughness_scale = 0.3\n\n\
[sets.cs_txst] # texture set\nedids = [\"LandscapeSnow01\", \"LandscapeSnow01Landscape\"]\nroughnessScale = 1\n\
specularLevel = 0.04\nmyNote = \"keep\"\nglintParameters = { enabled = true, screenSpaceScale = 1.5 }\n\
subsurfaceColor = [1, 0, 0.2]\n\n[sets.cs_mato]\nedids = [\"SnowMaterialObject1P\"]\nbaseColorScale = [0.98, 0.98, 1.0]\n\
roughness = \"rough\"\n\n[[sets]]\nid = \"b/two\"\nhair = true\n";

#[test]
fn blocks_read_back_with_unknown_and_wrong_typed_keys_and_overlay_the_pgpatcher_keys() {
    let (_d, s) = doc(TWO_SETS);
    let e = &s.entries()[0];
    let t = e.cs_txst.as_ref().unwrap();
    assert_eq!(t.edids, ["LandscapeSnow01", "LandscapeSnow01Landscape"]);
    assert_eq!(t.values["roughnessScale"].as_f64(), Some(1.0));
    assert!(!t.values.contains_key("myNote") && t.bad.is_empty());
    let m = e.cs_mato.as_ref().unwrap();
    assert_eq!(m.bad, ["roughness"]);
    assert!(!m.values.contains_key("roughness"));
    // The block is the single source of the keys it maps; unmapped keys stay the set's own.
    assert_eq!(e.settings["roughness_scale"], json!(1.0));
    assert_eq!(e.settings["specular_level"], json!(0.04));
    assert_eq!(e.settings["glint_on"], json!(true));
    assert_eq!(e.settings["glint.screen_space_scale"], json!(1.5));
    assert_eq!(e.settings["subsurface_color"], json!([255, 0, 51]));
    assert_eq!(e.settings["parallax"], json!(true));
    assert!(e.settings.keys().all(|k| !k.starts_with("cs_") && !k.contains("edids")));
    assert!(s.entries()[1].cs_txst.is_none());
}

#[test]
fn block_edits_keep_comments_and_unknown_keys() {
    let (d, mut s) = doc(TWO_SETS);
    s.set_cs_value("landscape/snow", Kind::Txst, "displacementScale", Some(json!(0.2)));
    s.set_cs_value("landscape/snow", Kind::Txst, "glintParameters.enabled", Some(json!(false)));
    s.set_cs_value("landscape/snow", Kind::Txst, "specularLevel", None);
    s.save(false).unwrap();
    let text = std::fs::read_to_string(d.path().join("sdc-project.toml")).unwrap();
    for keep in ["# notes", "# texture set", "myNote = \"keep\"", "roughness = \"rough\""] {
        assert!(text.contains(keep), "{keep} in {text}");
    }
    let s = SetsDoc::open(d.path());
    let t = s.entries()[0].cs_txst.clone().unwrap();
    assert_eq!(t.values["displacementScale"], json!(0.2));
    assert_eq!(t.values["glintParameters"]["enabled"], json!(false));
    assert!(!t.values.contains_key("specularLevel"));
}

/// Review focus: a new `[sets.cs_txst]` is printed under its own set, not after the last one.
#[test]
fn a_block_added_to_the_first_set_stays_with_it() {
    let (d, mut s) = doc("[[sets]]\nid = \"a/one\"\nhair = true\n\n[[sets]]\nid = \"b/two\"\nhair = true\n");
    s.set_cs_value("a/one", Kind::Txst, "edids", Some(json!(["A"])));
    s.save(false).unwrap();
    let s = SetsDoc::open(d.path());
    let e = s.entries();
    assert_eq!(e[0].cs_txst.as_ref().unwrap().edids, ["A"]);
    assert!(e[1].cs_txst.is_none());
}

/// Review focus: adding a texture-set block never drops the set's own PGPatcher values of the keys it maps.
#[test]
fn adding_a_texture_set_block_moves_the_sets_own_mapped_values_into_it() {
    let (_d, mut s) = doc(
        "[[sets]]\nid = \"a/one\"\nroughness_scale = 0.3\nglint_on = true\nglint.screen_space_scale = 2.0\nhair = true\n",
    );
    s.add_cs_block("a/one", Kind::Txst);
    let e = &s.entries()[0];
    let b = e.cs_txst.as_ref().unwrap();
    assert!(b.edids.is_empty());
    assert_eq!(b.values["roughnessScale"], json!(0.3));
    assert_eq!(b.values["glintParameters"], json!({"enabled": true, "screenSpaceScale": 2.0}));
    assert_eq!(e.settings["roughness_scale"], json!(0.3));
    assert_eq!(e.settings["hair"], json!(true));
    assert!(!s.text().contains("roughness_scale"), "moved, not copied");
    s.add_cs_block("a/one", Kind::Txst);
    assert_eq!(s.entries()[0].cs_txst.as_ref().unwrap().values["roughnessScale"], json!(0.3), "a second add changes nothing");
}

#[test]
fn a_pgpatcher_edit_of_a_mapped_key_lands_in_the_block() {
    let (_d, mut s) = doc(TWO_SETS);
    s.set_value("landscape/snow", "roughness_scale", Some(json!(0.5)));
    s.set_value("landscape/snow", "subsurface_color", Some(json!([255, 0, 51])));
    s.set_value("landscape/snow", "glint_on", Some(json!(false)));
    s.set_value("landscape/snow", "hair", Some(json!(true)));
    let t = s.entries()[0].cs_txst.clone().unwrap();
    assert_eq!(t.values["roughnessScale"], json!(0.5));
    assert_eq!(t.values["subsurfaceColor"], json!([1.0, 0.0, 0.2]));
    assert_eq!(t.values["glintParameters"]["enabled"], json!(false));
    assert!(!s.text().contains("roughness_scale"), "the stale flat copy is gone");
    assert_eq!(s.entries()[0].settings["hair"], json!(true), "unmapped keys stay flat");
}

#[test]
fn a_template_replaces_values_keeps_unknown_keys_and_the_users_edids() {
    let (_d, mut s) = doc(TWO_SETS);
    let slab = template(Kind::Txst, "LandscapeMountainSlab").unwrap();
    s.apply_cs_template("landscape/snow", slab);
    let t = s.entries()[0].cs_txst.clone().unwrap();
    assert_eq!(t.edids, ["LandscapeSnow01", "LandscapeSnow01Landscape"]);
    assert_eq!(floats(&Json::Object(t.values)), floats(&Json::Object(slab.values.clone())));
    assert_eq!(t.template.as_deref(), Some("LandscapeMountainSlab"));
    assert!(s.text().contains("myNote"));
    // A block with no EDIDs takes the template's (ruling R8).
    s.set_cs_value("b/two", Kind::Txst, "edids", Some(json!([])));
    s.apply_cs_template("b/two", slab);
    assert_eq!(s.entries()[1].cs_txst.as_ref().unwrap().edids, slab.edids);
}

#[test]
fn model_edits_are_undoable_steps_and_cs_keys_never_reach_the_pgpatcher_json() {
    use sdc_gui::model::pbr_export::json_entries;
    use sdc_gui::pgp::writer::{document, to_text};
    let d = tempfile::tempdir().unwrap();
    let mut m = support::pbr_opened(d.path());
    let id = "landscape/rock01";
    m.add_cs_block(id, Kind::Txst);
    m.apply_cs_template(id, Kind::Txst, "LandscapeMountainSlab").unwrap();
    let b = m.cs_block(id, Kind::Txst).unwrap();
    assert_eq!(b.template.as_deref(), Some("LandscapeMountainSlab"));
    let entries: Vec<_> = json_entries(&m).into_iter().map(|(_, e)| e).collect();
    let text = to_text(&document(&entries));
    for leak in ["cs_txst", "edids", "glintParameters", "roughnessScale"] {
        assert!(!text.contains(leak), "{leak} in {text}");
    }
    assert!(text.contains("\"density_randomization\": 300.0"), "{text}");
    m.undo();
    assert!(m.wait_idle(std::time::Duration::from_secs(60)));
    assert_eq!(m.cs_block(id, Kind::Txst).unwrap().template, None, "one step undone");
    assert!(m.apply_cs_template(id, Kind::Txst, "NoSuchTemplate").is_err());
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p sdc-gui --release --test cs_pbr`
Expected: compile errors (`no field cs_txst`, `no method set_cs_value`, `Statics` missing).

- [ ] **Step 3: Implement the block model** (append to `crates/gui/src/model/cs_pbr.rs`)

```rust
use super::AppModel;
use crate::pgp::settings::Settings;
use crate::sets_doc::SetsDoc;
use engine::sets::same_id;
use serde_json::json;
use toml_edit::{Item, TableLike};

/// Texture sets: when the set's PGPatcher entry is written (design §5.1). `statics = "auto" | "always" | "never"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Statics {
    #[default]
    Auto,
    Always,
    Never,
}

impl Statics {
    pub fn parse(s: Option<&str>) -> Statics {
        match s {
            Some("always") => Statics::Always,
            Some("never") => Statics::Never,
            _ => Statics::Auto,
        }
    }
    /// The value written; `None` for the default.
    pub fn as_str(self) -> Option<&'static str> {
        match self {
            Statics::Auto => None,
            Statics::Always => Some("always"),
            Statics::Never => Some("never"),
        }
    }
}

/// One block as read from `sdc-project.toml` (design §3).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CsBlock {
    pub edids: Vec<String>,
    /// The last applied template (informational).
    pub template: Option<String>,
    pub statics: Statics,
    /// Known fields with a usable value, as read (`glintParameters` an object).
    pub values: Map<String, Json>,
    /// Known fields whose value has the wrong type: kept in the file, warned about, not exported (design §8).
    pub bad: Vec<String>,
}

/// A TOML item as JSON; a `[table]` (or dotted keys) becomes an object.
fn item_json(item: &Item) -> Option<Json> {
    match item {
        Item::Table(t) => Some(Json::Object(
            t.iter()
                .filter_map(|(k, i)| Some((k.to_string(), item_json(i)?)))
                .collect(),
        )),
        _ => crate::project::toml_to_json(item),
    }
}

/// Reads a block table. Unknown keys are left alone: they stay in the file and are never exported.
pub(crate) fn block_of(t: &dyn TableLike, kind: Kind) -> CsBlock {
    let mut b = CsBlock::default();
    for (k, item) in t.iter() {
        match k {
            "edids" => {
                b.edids = item
                    .as_array()
                    .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
                    .unwrap_or_default()
            }
            "template" => b.template = item.as_str().map(str::to_string),
            "statics" => b.statics = Statics::parse(item.as_str()),
            _ => {
                if let Some(f) = field(kind, k) {
                    match item_json(item).filter(|v| valid_value(f.kind, v)) {
                        Some(v) => {
                            b.values.insert(k.to_string(), v);
                        }
                        None => b.bad.push(k.to_string()),
                    }
                }
            }
        }
    }
    b
}

/// CS texture-set field → PGPatcher settings key (design §5.1, research §1.5). The `subsurface` and `multilayer`
/// switches stay the user's: they need textures and NIF flags a texture set cannot know (plan ruling R7).
pub const PGP_MAP: [(&str, &str); 14] = [
    ("roughnessScale", "roughness_scale"),
    ("specularLevel", "specular_level"),
    ("displacementScale", "displacement_scale"),
    ("subsurfaceColor", "subsurface_color"),
    ("subsurfaceOpacity", "subsurface_opacity"),
    ("coatColor", "coat_color"),
    ("coatStrength", "coat_strength"),
    ("coatRoughness", "coat_roughness"),
    ("coatSpecularLevel", "coat_specular_level"),
    ("glintParameters.enabled", "glint_on"),
    ("glintParameters.screenSpaceScale", "glint.screen_space_scale"),
    ("glintParameters.logMicrofacetDensity", "glint.log_microfacet_density"),
    ("glintParameters.microfacetRoughness", "glint.microfacet_roughness"),
    ("glintParameters.densityRandomization", "glint.density_randomization"),
];

/// The PGPatcher settings a texture-set block stands for: colours as 0–255 ints (DECISIONS P10), numbers as floats.
pub fn cs_to_pgp(values: &Map<String, Json>) -> Settings {
    let mut out = Settings::new();
    for (cs, pgp) in PGP_MAP {
        let v = match cs.split_once('.') {
            Some((o, k)) => values.get(o).and_then(|g| g.get(k)),
            None => values.get(cs),
        };
        let Some(v) = v else { continue };
        let v = if cs.ends_with("Color") {
            Json::Array(
                v.as_array()
                    .into_iter()
                    .flatten()
                    .map(|c| json!((c.as_f64().unwrap_or(0.0) * 255.0).round().clamp(0.0, 255.0) as i64))
                    .collect(),
            )
        } else {
            floats(v)
        };
        out.insert(pgp.to_string(), v);
    }
    out
}

/// A PGPatcher edit of a mapped key in CS form: (dotted block key, value); `None` when `key` is not mapped.
pub fn pgp_to_cs(key: &str, v: Option<&Json>) -> Option<(&'static str, Option<Json>)> {
    let (cs, _) = PGP_MAP.iter().find(|(_, p)| *p == key)?;
    let v = v.map(|v| {
        if cs.ends_with("Color") {
            Json::Array(
                v.as_array()
                    .into_iter()
                    .flatten()
                    .map(|c| json!((c.as_f64().unwrap_or(0.0) / 255.0 * 10_000.0).round() / 10_000.0))
                    .collect(),
            )
        } else {
            floats(v)
        }
    });
    Some((cs, v))
}

/// Design §5.1: a texture-set block is the single source of the keys it maps; the set's own copies are dropped.
pub fn overlay(settings: &mut Settings, b: &CsBlock) {
    for (_, pgp) in PGP_MAP {
        settings.remove(pgp);
    }
    settings.extend(cs_to_pgp(&b.values));
}

impl AppModel {
    /// Set `set`'s block of `kind`, as the document holds it.
    pub fn cs_block(&self, set: &str, kind: Kind) -> Option<CsBlock> {
        let e = self
            .sets_doc
            .as_ref()?
            .entries()
            .into_iter()
            .find(|e| same_id(&e.id, set))?;
        match kind {
            Kind::Txst => e.cs_txst,
            Kind::Mato => e.cs_mato,
        }
    }

    /// One undoable, JSON-only edit of the sets document (design §3; nothing is planned, DECISIONS P7).
    fn cs_edit(&mut self, set: &str, label: &str, merge: Option<String>, f: impl FnOnce(&mut SetsDoc, &str)) {
        if !self.sets_editable() {
            return;
        }
        let s = set.to_string();
        self.edit_docs(label, merge, false, move |_, d| f(d, &s));
    }

    pub fn add_cs_block(&mut self, set: &str, kind: Kind) {
        self.cs_edit(set, &format!("Add {}", kind.label()), None, |d, s| {
            d.add_cs_block(s, kind)
        });
    }

    pub fn remove_cs_block(&mut self, set: &str, kind: Kind) {
        self.cs_edit(set, &format!("Remove {}", kind.label()), None, |d, s| {
            d.remove_cs_block(s, kind)
        });
    }

    /// Set (or with `None` remove) block key `key` (dotted: `glintParameters.enabled`). Drags merge into one step.
    pub fn set_cs_value(&mut self, set: &str, kind: Kind, key: &str, v: Option<Json>) {
        let merge = Some(format!("{set}:{}:{key}", kind.key()));
        let k = key.to_string();
        self.cs_edit(set, &format!("Change {key}"), merge, move |d, s| {
            d.set_cs_value(s, kind, &k, v)
        });
    }

    pub fn set_cs_edids(&mut self, set: &str, kind: Kind, edids: &[String]) {
        let v = json!(edids);
        self.cs_edit(set, "Change EDIDs", None, move |d, s| {
            d.set_cs_value(s, kind, "edids", Some(v))
        });
    }

    /// Design §4: one undo step; see `SetsDoc::apply_cs_template`.
    pub fn apply_cs_template(&mut self, set: &str, kind: Kind, name: &str) -> Result<(), String> {
        let t = template(kind, name).ok_or_else(|| format!("no {} template {name}", kind.label()))?;
        self.cs_edit(set, &format!("Apply template {name}"), None, move |d, s| {
            d.apply_cs_template(s, t)
        });
        Ok(())
    }

    pub fn set_cs_statics(&mut self, set: &str, statics: Statics) {
        self.set_cs_value(set, Kind::Txst, "statics", statics.as_str().map(Json::from));
    }
}
```

- [ ] **Step 4: Implement the document side** (`crates/gui/src/sets_doc.rs`)

Imports (top of file), add:

```rust
use crate::model::cs_pbr::{self, block_of, field, fields, CsBlock, Kind, Template};
```

`RESERVED` becomes:

```rust
/// Keys of a `[[sets]]` table that are not JSON settings.
const RESERVED: [&str; 11] = [
    "id",
    "created",
    "renamed_from",
    "add",
    "remove",
    "borrow",
    "shader",
    "lock",
    "output_name",
    "cs_txst",
    "cs_mato",
];
```

`SetEntry`, after `output_name`:

```rust
    /// Design §3: the Community Shaders PBR texture-set and material-object blocks (`[sets.cs_txst]`, `[sets.cs_mato]`).
    pub cs_txst: Option<CsBlock>,
    pub cs_mato: Option<CsBlock>,
```

`entry_of`: after the settings loop (before `let borrow = …`) add:

```rust
    let block = |k: Kind| t.get(k.key()).and_then(Item::as_table_like).map(|b| block_of(b, k));
    let (cs_txst, cs_mato) = (block(Kind::Txst), block(Kind::Mato));
    if let Some(b) = &cs_txst {
        cs_pbr::overlay(&mut settings, b);
    }
```

and in the `Some(SetEntry { … })` literal add `cs_txst, cs_mato,` before `settings,`.

Replace `set_value` (lines 706-716) with:

```rust
    /// Set (or with `None` remove) JSON setting `key` (a dotted schema path) of set `id`. On a set with a texture-set
    /// block a mapped key lands in the block (design §5.1: one source, so the two files cannot drift).
    pub fn set_value(&mut self, id: &str, key: &str, v: Option<Json>) {
        self.rev = next_rev();
        if self.read_only() {
            return;
        }
        let t = self.table_mut(id);
        if t.get(Kind::Txst.key()).is_some_and(Item::is_table_like) {
            if let Some((cs, cv)) = cs_pbr::pgp_to_cs(key, v.as_ref()) {
                write_path(t, key, None);
                if let Some(b) = sub_table(t, Kind::Txst.key(), || Item::Table(Table::new())) {
                    match &cv {
                        Some(x) => write_json(b, cs, x),
                        None => write_path(b, cs, None),
                    };
                }
                self.dirty = true;
                return;
            }
        }
        if write_path(t, key, v) {
            self.dirty = true;
        }
    }

    /// Design §3: set (or with `None` remove) key `key` (dotted, `glintParameters.enabled`) of set `id`'s `kind`
    /// block, creating the block (`[sets.cs_txst]`) when missing.
    pub fn set_cs_value(&mut self, id: &str, kind: Kind, key: &str, v: Option<Json>) {
        self.rev = next_rev();
        if self.read_only() {
            return;
        }
        let t = self.table_mut(id);
        let Some(b) = sub_table(t, kind.key(), || Item::Table(Table::new())) else {
            return;
        };
        let hit = match &v {
            Some(v) => write_json(b, key, v),
            None => write_path(b, key, None),
        };
        if hit {
            self.dirty = true;
        }
    }

    /// Design §3: start set `id`'s `kind` block (`edids = []`); nothing when it exists. A texture-set block takes
    /// over the set's own values of the keys it maps (moved, so adding the block never drops them, design §5.1).
    pub fn add_cs_block(&mut self, id: &str, kind: Kind) {
        self.rev = next_rev();
        if self.read_only() {
            return;
        }
        let own = self
            .entries()
            .into_iter()
            .find(|e| same_id(&e.id, id))
            .map(|e| e.settings)
            .unwrap_or_default();
        let t = self.table_mut(id);
        if t.contains_key(kind.key()) {
            return;
        }
        let Some(b) = sub_table(t, kind.key(), || Item::Table(Table::new())) else {
            return;
        };
        set_value(b, "edids", Value::Array(Array::new()));
        if kind == Kind::Txst {
            for (_, pgp) in cs_pbr::PGP_MAP {
                if let Some((cs, Some(v))) = cs_pbr::pgp_to_cs(pgp, own.get(pgp)) {
                    write_json(b, cs, &v);
                }
            }
            for (_, pgp) in cs_pbr::PGP_MAP {
                write_path(t, pgp, None);
            }
        }
        self.dirty = true;
    }

    pub fn remove_cs_block(&mut self, id: &str, kind: Kind) {
        self.rev = next_rev();
        if self.read_only() {
            return;
        }
        if self.table_mut(id).remove(kind.key()).is_some() {
            self.dirty = true;
        }
    }

    /// Design §4: the template's values replace the block's known fields (unknown keys stay) and it is recorded as
    /// `template`; a block with no EDIDs takes the template's (plan ruling R8).
    pub fn apply_cs_template(&mut self, id: &str, t: &Template) {
        self.rev = next_rev();
        if self.read_only() {
            return;
        }
        let tbl = self.table_mut(id);
        let Some(b) = sub_table(tbl, t.kind.key(), || Item::Table(Table::new())) else {
            return;
        };
        for f in fields(t.kind) {
            b.remove(f.name);
        }
        for (k, v) in &t.values {
            if field(t.kind, k).is_some() {
                write_json(b, k, v);
            }
        }
        set_value(b, "template", Value::from(t.name.as_str()));
        if strings(b.get("edids")).is_empty() {
            let mut arr = Array::new();
            for e in &t.edids {
                arr.push(e.as_str());
            }
            set_value(b, "edids", Value::Array(arr));
        }
        self.dirty = true;
    }
```

After `fn write_path` add:

```rust
/// `write_path` for a value that may be a JSON object (a block's `glintParameters`): its keys go one level down.
fn write_json(t: &mut dyn TableLike, path: &str, v: &Json) -> bool {
    match v {
        Json::Object(o) => o
            .iter()
            .fold(false, |hit, (k, x)| write_json(t, &format!("{path}.{k}"), x) | hit),
        _ => write_path(t, path, Some(v.clone())),
    }
}
```

- [ ] **Step 5: Run the tests**

Run: `cargo test -p sdc-gui --release --test cs_pbr --test sets_doc --test sets_model --test pbr_export`
Expected: PASS.

- [ ] **Step 6: Gate and commit**

```bash
cargo fmt --all --check && cargo clippy --workspace --all-targets --release -- -D warnings && cargo test --workspace --release
git add crates/gui/src/model/cs_pbr.rs crates/gui/src/sets_doc.rs crates/gui/tests/cs_pbr.rs
git commit -m "feat(gui): cs_txst/cs_mato blocks in sdc-project.toml, one source for the PGPatcher keys

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: The CS JSON writer

**Files:**
- Create: `crates/gui/src/model/cs_pbr_export.rs`
- Modify (shared, add-only, own commit): `crates/gui/src/model/mod.rs` (module list)
- Test: `crates/gui/tests/cs_pbr_export.rs` (create)

**Interfaces:**
- Consumes: Task 2 `Kind`, `field`, `floats`, `GLINT_FIELDS`.
- Produces: `cs_pbr_export::MATO_SPECULAR: f64 = 0.04`; `cs_pbr_export::to_text(kind: Kind, values: &Map<String, Json>) -> String`; `cs_pbr_export::rel_path(kind: Kind, edid: &str) -> String` (`"PBRTextureSets/<EDID>.json"`).

- [ ] **Step 1: Write the failing tests** (create `crates/gui/tests/cs_pbr_export.rs`)

```rust
//! The CS runtime JSON (design §5): text, files per EDID, the hand-edit guard, refusals and orphans.
mod support;
use sdc_gui::model::cs_pbr::Kind;
use sdc_gui::model::cs_pbr_export::*;
use serde_json::{json, Map, Value as Json};

fn obj(v: Json) -> Map<String, Json> {
    v.as_object().unwrap().clone()
}

fn lines(l: &[&str]) -> String {
    l.join("\n")
}

/// Faultier's LandscapeMountainSlab.json, written from int literals: CS's own layout, every number a float.
#[test]
fn a_texture_set_is_written_as_cs_saves_it_with_decimal_points() {
    let v = obj(json!({
        "displacementScale": 0.2,
        "glintParameters": {"densityRandomization": 300, "enabled": true, "logMicrofacetDensity": 24,
                            "microfacetRoughness": 0.7, "screenSpaceScale": 2},
        "roughnessScale": 1, "specularLevel": 0.04, "subsurfaceColor": [1, 1, 1], "subsurfaceOpacity": 1,
        "notACsField": 5
    }));
    assert_eq!(
        to_text(Kind::Txst, &v),
        lines(&[
            "{",
            "    \"displacementScale\": 0.2,",
            "    \"glintParameters\": {",
            "        \"densityRandomization\": 300.0,",
            "        \"enabled\": true,",
            "        \"logMicrofacetDensity\": 24.0,",
            "        \"microfacetRoughness\": 0.7,",
            "        \"screenSpaceScale\": 2.0",
            "    },",
            "    \"roughnessScale\": 1.0,",
            "    \"specularLevel\": 0.04,",
            "    \"subsurfaceColor\": [",
            "        1.0,",
            "        1.0,",
            "        1.0",
            "    ],",
            "    \"subsurfaceOpacity\": 1.0",
            "}",
        ])
    );
}

#[test]
fn a_material_object_is_written_as_cs_saves_it() {
    let v = obj(json!({
        "baseColorScale": [0.98, 0.98, 1], "roughness": 0.7, "specularLevel": 0.02,
        "glintParameters": {"densityRandomization": 5, "enabled": true, "logMicrofacetDensity": 13,
                            "microfacetRoughness": 1, "screenSpaceScale": 1, "extra": 1}
    }));
    assert_eq!(
        to_text(Kind::Mato, &v),
        lines(&[
            "{",
            "    \"baseColorScale\": [",
            "        0.98,",
            "        0.98,",
            "        1.0",
            "    ],",
            "    \"glintParameters\": {",
            "        \"densityRandomization\": 5.0,",
            "        \"enabled\": true,",
            "        \"logMicrofacetDensity\": 13.0,",
            "        \"microfacetRoughness\": 1.0,",
            "        \"screenSpaceScale\": 1.0",
            "    },",
            "    \"roughness\": 0.7,",
            "    \"specularLevel\": 0.02",
            "}",
        ])
    );
}

#[test]
fn material_specular_is_always_written_and_absent_fields_are_omitted() {
    assert_eq!(
        to_text(Kind::Mato, &obj(json!({"roughness": 0.9}))),
        lines(&["{", "    \"roughness\": 0.9,", "    \"specularLevel\": 0.04", "}"])
    );
    assert_eq!(
        to_text(Kind::Txst, &obj(json!({"specularLevel": 0.04}))),
        lines(&["{", "    \"specularLevel\": 0.04", "}"])
    );
    assert_eq!(rel_path(Kind::Mato, "SnowMat1P"), "PBRMaterialObjects/SnowMat1P.json");
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p sdc-gui --release --test cs_pbr_export`
Expected: compile error `could not find cs_pbr_export in model`.

- [ ] **Step 3: Implement** (create `crates/gui/src/model/cs_pbr_export.rs`)

```rust
//! The Community Shaders runtime JSON (design §5): `PBRTextureSets\<EDID>.json` and `PBRMaterialObjects\<EDID>.json`,
//! one file per EDID per block, written after a clean export. No egui types.
use super::cs_pbr::{field, floats, Kind, GLINT_FIELDS};
use serde::Serialize;
use serde_json::{json, Map, Value as Json};

/// The material-object `specularLevel` written when the block has none: the material reset value. CS's own default
/// (1.0) is 25× that (research §2), so the key is always written (plan ruling R9).
pub const MATO_SPECULAR: f64 = 0.04;

/// `<folder>/<EDID>.json`, the path under the export root (and the hash record's key).
pub fn rel_path(kind: Kind, edid: &str) -> String {
    format!("{}/{edid}.json", kind.folder())
}

/// One file's text: the block's known fields with every number a float (CS drops int colour arrays), 4-space
/// indent, keys sorted as CS's own saver (nlohmann::json) writes them, `\n` line ends, no final newline (ruling R3).
/// serde_json's map is sorted (no `preserve_order` in Cargo.lock); the golden tests pin it.
pub fn to_text(kind: Kind, values: &Map<String, Json>) -> String {
    let mut obj = Map::new();
    for (k, v) in values {
        if field(kind, k).is_none() {
            continue;
        }
        let v = match v.as_object() {
            Some(g) => Json::Object(
                g.iter()
                    .filter(|(gk, _)| *gk == "enabled" || GLINT_FIELDS.iter().any(|f| f.name == gk.as_str()))
                    .map(|(gk, gv)| (gk.clone(), gv.clone()))
                    .collect(),
            ),
            None => v.clone(),
        };
        obj.insert(k.clone(), floats(&v));
    }
    if kind == Kind::Mato {
        obj.entry("specularLevel").or_insert(json!(MATO_SPECULAR));
    }
    let mut out = Vec::new();
    let mut ser = serde_json::Serializer::with_formatter(
        &mut out,
        serde_json::ser::PrettyFormatter::with_indent(b"    "),
    );
    Json::Object(obj)
        .serialize(&mut ser)
        .expect("JSON values serialise");
    String::from_utf8(out).expect("serde_json writes UTF-8")
}
```

- [ ] **Step 4: Commit the new files**

```bash
git add crates/gui/src/model/cs_pbr_export.rs crates/gui/tests/cs_pbr_export.rs
git commit -m "feat(gui): CS PBR JSON writer

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

- [ ] **Step 5: Mount the module** (shared, add-only). In `crates/gui/src/model/mod.rs` add after `pub mod cs_pbr;`:

```rust
pub mod cs_pbr_export;
```

- [ ] **Step 6: Run the tests**

Run: `cargo test -p sdc-gui --release --test cs_pbr_export`
Expected: 3 PASS.

- [ ] **Step 7: Gate and commit**

```bash
cargo fmt --all --check && cargo clippy --workspace --all-targets --release -- -D warnings && cargo test --workspace --release
git add crates/gui/src/model/mod.rs
git commit -m "chore(gui): mount model::cs_pbr_export

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: Export integration, refusals, hand-edit guard and orphan report

**Files:**
- Modify (owner OK): `crates/gui/src/model/pbr_export.rs:135-168` (`json_name_problem`), `:538-547` (`write_atomic`), `:569-575` (guard in `finish`)
- Modify (owner OK): `crates/gui/src/sets_doc.rs` (`[project] cs_pbr_sha256`, `restore_keeping_json_sha`)
- Modify: `crates/gui/src/model/cs_pbr_export.rs` (append)
- Modify (shared, add-only, own commit): `crates/gui/src/model/mod.rs` (field `pending_cs` at `:283`, init at `:381`, reset in `open_folder` at `:451`, `start_export` at `:1080`, after `self.finish_json();` at `:745`)
- Test: `crates/gui/tests/cs_pbr_export.rs` (append)

**Interfaces:**
- Consumes: Task 3 `SetEntry::cs_txst/cs_mato`, `CsBlock`, model edit API; Task 4 `to_text`, `rel_path`.
- Produces: `pbr_export::file_name_problem(what: &str, name: &str) -> Option<String>` (pub(crate)); `pbr_export::hand_edited(path: &Path, text: &str, last_sha: Option<&str>) -> bool` (pub(crate)); `pbr_export::write_atomic` (pub(crate)); `SetsDoc::cs_pbr_shas(&self) -> BTreeMap<String, String>`, `SetsDoc::set_cs_pbr_shas(&mut self, &BTreeMap<String, String>)`; `cs_pbr_export::edid_problem(&str) -> Option<String>`; `cs_pbr_export::refusals(&[SetEntry]) -> BTreeMap<(String, Kind, String), String>` (key: set id, kind, EDID as typed); `struct PendingCs { root: PathBuf, files: Vec<(String, String)>, notes: Vec<String> }`; `cs_pbr_export::prepare(&AppModel) -> Option<PendingCs>`; `cs_pbr_export::finish(&PendingCs, &BTreeMap<String, String>, &mut Vec<String>) -> BTreeMap<String, String>`; `AppModel::finish_cs_pbr(&mut self, p: Option<PendingCs>)` (pub(crate)).

- [ ] **Step 1: Write the failing tests** (append to `crates/gui/tests/cs_pbr_export.rs`)

```rust
use sdc_gui::model::{AppModel, Dialog};
use std::time::Duration;
use support::*;

const ROCK: &str = "landscape/rock01";

fn export_and_wait(m: &mut AppModel) {
    m.export_clicked();
    if matches!(m.dialog, Some(Dialog::ProceedAnyway(_))) {
        m.proceed_anyway();
    }
    assert!(m.wait_idle(Duration::from_secs(120)), "export did not finish");
}

fn txst_file(m: &AppModel, edid: &str) -> std::path::PathBuf {
    m.export_root().join("PBRTextureSets").join(format!("{edid}.json"))
}

fn names(dir: &std::path::Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

fn with_blocks(d: &std::path::Path) -> AppModel {
    let mut m = pbr_opened(d);
    m.add_cs_block(ROCK, Kind::Txst);
    m.set_cs_edids(ROCK, Kind::Txst, &["LandscapeRock01".into(), "LandscapeRock01Landscape".into()]);
    m.set_cs_value(ROCK, Kind::Txst, "specularLevel", Some(json!(0.02)));
    m.add_cs_block(ROCK, Kind::Mato);
    m.set_cs_edids(ROCK, Kind::Mato, &["SnowMaterialObject1P".into()]);
    m
}

#[test]
fn export_writes_one_file_per_edid_named_exactly_and_records_hashes() {
    let d = tempfile::tempdir().unwrap();
    let mut m = with_blocks(d.path());
    export_and_wait(&mut m);
    let root = m.export_root();
    assert_eq!(
        names(&root.join("PBRTextureSets")),
        ["LandscapeRock01.json", "LandscapeRock01Landscape.json"],
        "notes {:?}",
        m.export.notes
    );
    let b = m.cs_block(ROCK, Kind::Txst).unwrap();
    assert_eq!(
        std::fs::read_to_string(txst_file(&m, "LandscapeRock01")).unwrap(),
        to_text(Kind::Txst, &b.values)
    );
    let mato = std::fs::read_to_string(root.join("PBRMaterialObjects/SnowMaterialObject1P.json")).unwrap();
    assert!(mato.contains("\"specularLevel\": 0.04"), "{mato}");
    let shas = m.sets_doc.as_ref().unwrap().cs_pbr_shas();
    assert_eq!(shas.len(), 3);
    assert_eq!(
        shas.get("PBRTextureSets/LandscapeRock01.json").cloned(),
        sdc_gui::model::history::sha256_of(&txst_file(&m, "LandscapeRock01"))
    );
    assert!(!m.is_dirty(), "the record was saved");
    // Undo restores the document but keeps the record of what is on disk.
    m.undo();
    assert!(m.wait_idle(Duration::from_secs(60)));
    assert_eq!(m.sets_doc.as_ref().unwrap().cs_pbr_shas(), shas);
}

/// Review focus: a file sdc did not write last (edited, or never recorded) is never overwritten.
#[test]
fn a_hand_edited_or_foreign_file_is_kept_and_reported() {
    let d = tempfile::tempdir().unwrap();
    let mut m = with_blocks(d.path());
    let foreign = txst_file(&m, "LandscapeRock01Landscape");
    std::fs::create_dir_all(foreign.parent().unwrap()).unwrap();
    std::fs::write(&foreign, "{\"specularLevel\": 0.5}").unwrap();
    export_and_wait(&mut m);
    assert_eq!(std::fs::read_to_string(&foreign).unwrap(), "{\"specularLevel\": 0.5}");
    let mine = txst_file(&m, "LandscapeRock01");
    std::fs::write(&mine, "{}").unwrap();
    export_and_wait(&mut m);
    assert_eq!(std::fs::read_to_string(&mine).unwrap(), "{}");
    let kept: Vec<_> = m.export.notes.iter().filter(|n| n.contains("kept as edited outside sdc")).collect();
    assert_eq!(kept.len(), 2, "{:?}", m.export.notes);
}

#[test]
fn an_unbound_file_is_reported_not_deleted() {
    let d = tempfile::tempdir().unwrap();
    let mut m = with_blocks(d.path());
    export_and_wait(&mut m);
    m.set_cs_edids(ROCK, Kind::Txst, &["LandscapeRock01".into()]);
    export_and_wait(&mut m);
    let gone = txst_file(&m, "LandscapeRock01Landscape");
    assert!(gone.is_file(), "sdc never deletes it");
    assert!(
        m.export.notes.iter().any(|n| n.contains("LandscapeRock01Landscape.json: no set binds this EDID any more")),
        "{:?}",
        m.export.notes
    );
    assert!(m.sets_doc.as_ref().unwrap().cs_pbr_shas().contains_key("PBRTextureSets/LandscapeRock01Landscape.json"));
}

/// Review focus (O6): CS keys on the exact stem, so a corrected case renames sdc's own file.
#[test]
fn an_edid_whose_case_changed_renames_sdcs_own_file() {
    let d = tempfile::tempdir().unwrap();
    let mut m = with_blocks(d.path());
    m.set_cs_edids(ROCK, Kind::Txst, &["LandscapeRock01".into()]);
    export_and_wait(&mut m);
    m.set_cs_edids(ROCK, Kind::Txst, &["landscapeRock01".into()]);
    export_and_wait(&mut m);
    assert_eq!(names(&m.export_root().join("PBRTextureSets")), ["landscapeRock01.json"]);
    assert!(!m.export.notes.iter().any(|n| n.contains("no set binds")), "{:?}", m.export.notes);
    let shas = m.sets_doc.as_ref().unwrap().cs_pbr_shas();
    let txst: Vec<&String> = shas.keys().filter(|k| k.starts_with("PBRTextureSets")).collect();
    assert_eq!(txst, ["PBRTextureSets/landscapeRock01.json"]);
}

/// Review focus: refusals are per file; EDIDs differing only in case collide on Windows.
#[test]
fn duplicate_and_invalid_edids_are_refused_per_file() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("sdc-project.toml"),
        "[[sets]]\nid = \"a/one\"\n[sets.cs_txst]\nedids = [\"Dirt\", \"Bad:Name\", \"CON\", \"Fine\"]\n\n\
         [[sets]]\nid = \"b/two\"\n[sets.cs_txst]\nedids = [\"dirt\"]\n[sets.cs_mato]\nedids = [\"Dirt\"]\n",
    )
    .unwrap();
    let doc = sdc_gui::sets_doc::SetsDoc::open(d.path());
    let r = refusals(&doc.entries());
    let why = |id: &str, k: Kind, e: &str| r.get(&(id.to_string(), k, e.to_string())).cloned();
    assert_eq!(why("a/one", Kind::Txst, "Dirt").as_deref(), Some("PBR texture set Dirt is bound by a/one and b/two"));
    assert!(why("b/two", Kind::Txst, "dirt").is_some());
    assert!(why("a/one", Kind::Txst, "Bad:Name").unwrap().contains("Windows does not allow"));
    assert!(why("a/one", Kind::Txst, "CON").unwrap().contains("reserves for a device"));
    assert_eq!(why("a/one", Kind::Txst, "Fine"), None);
    assert_eq!(why("b/two", Kind::Mato, "Dirt"), None, "kinds do not collide");
    assert_eq!(edid_problem(""), Some("EDID: empty (\"\")".into()));
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p sdc-gui --release --test cs_pbr_export`
Expected: compile errors (`refusals`, `edid_problem`, `cs_pbr_shas` missing).

- [ ] **Step 3: Share the guard in `pbr_export.rs`** (owner OK; no behaviour change)

Replace `fn json_name_problem(name: &str) -> Option<String> {` and its first line `let bad = |why: &str| Some(format!("JSON name: {why} ({name:?})"));` with (the existing doc comment above stays):

```rust
fn json_name_problem(name: &str) -> Option<String> {
    file_name_problem("JSON name", name)
}

/// Why `name` cannot be a single Windows file name (without its extension); `what` starts the reason.
pub(crate) fn file_name_problem(what: &str, name: &str) -> Option<String> {
    let bad = |why: &str| Some(format!("{what}: {why} ({name:?})"));
```

(the rest of the old body stays as the body of `file_name_problem`). Change `fn write_atomic` to `pub(crate) fn write_atomic`. Add, after `write_atomic`:

```rust
/// The file was edited outside sdc since sdc last wrote it (DECISIONS Note P21): it exists, its hash is not the
/// recorded one, and its bytes are not `text` (identical bytes need no question, even with no record).
pub(crate) fn hand_edited(path: &std::path::Path, text: &str, last_sha: Option<&str>) -> bool {
    let on_disk = sha256_of(path);
    on_disk.is_some()
        && on_disk.as_deref() != last_sha
        && !std::fs::read_to_string(path).is_ok_and(|t| t == text)
}
```

In `finish`, replace

```rust
    let on_disk = sha256_of(&p.path);
    // Identical bytes need no question, even when no hash was recorded.
    let same = std::fs::read_to_string(&p.path).is_ok_and(|t| t == text);
    if on_disk.is_some() && on_disk.as_deref() != last_sha && !same {
        return Finish::Overwrite(text);
    }
```

with

```rust
    if hand_edited(&p.path, &text, last_sha) {
        return Finish::Overwrite(text);
    }
```

- [ ] **Step 4: Record per-file hashes in `sets_doc.rs`** (owner OK)

Below `const SECTIONS` add:

```rust
/// Design §5: `[project] cs_pbr_sha256`, path under the export root → hash of the CS JSON sdc last wrote there.
const CS_SHA: &str = "cs_pbr_sha256";
```

After `set_last_json_sha256` add:

```rust
    pub fn cs_pbr_shas(&self) -> BTreeMap<String, String> {
        self.doc
            .get("project")
            .and_then(|p| p.get(CS_SHA))
            .and_then(Item::as_table_like)
            .map(|t| {
                t.iter()
                    .filter_map(|(k, v)| Some((k.to_string(), v.as_str()?.to_string())))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn set_cs_pbr_shas(&mut self, shas: &BTreeMap<String, String>) {
        self.rev = next_rev();
        let mut t = toml_edit::InlineTable::new();
        for (k, v) in shas {
            t.insert(k, Value::from(v.as_str()));
        }
        self.set_project(CS_SHA, (!shas.is_empty()).then_some(Value::InlineTable(t)));
    }
```

In `restore_keeping_json_sha` replace

```rust
        let (doc, dirty, sha) = (self.doc.clone(), self.dirty, self.last_json_sha256());
        let old = self.text();
        self.restore(text);
        if self.last_json_sha256() != sha {
            self.set_project("last_json_sha256", sha.map(Value::from));
        }
```

with

```rust
        let (doc, dirty, sha) = (self.doc.clone(), self.dirty, self.last_json_sha256());
        let cs = self.cs_pbr_shas();
        let old = self.text();
        self.restore(text);
        if self.last_json_sha256() != sha {
            self.set_project("last_json_sha256", sha.map(Value::from));
        }
        if self.cs_pbr_shas() != cs {
            self.set_cs_pbr_shas(&cs);
        }
```

and update its doc comment's first sentence to "keeps the live `last_json_sha256` and `cs_pbr_sha256`".

- [ ] **Step 5: Implement prepare and finish** (append to `crates/gui/src/model/cs_pbr_export.rs`)

```rust
use super::cs_pbr::CsBlock;
use super::history::sha256_of;
use super::pbr_export::{file_name_problem, hand_edited, write_atomic};
use super::{AppModel, Dialog};
use crate::sets_doc::SetEntry;
use engine::config::Target;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

/// Why an EDID cannot be a file name (design §5): empty, a character Windows refuses, a trailing dot or space,
/// a device name.
pub fn edid_problem(edid: &str) -> Option<String> {
    file_name_problem("EDID", edid)
}

fn blocks(e: &SetEntry) -> impl Iterator<Item = (Kind, &CsBlock)> {
    [(Kind::Txst, e.cs_txst.as_ref()), (Kind::Mato, e.cs_mato.as_ref())]
        .into_iter()
        .filter_map(|(k, b)| Some((k, b?)))
}

/// Per (set id, kind, EDID as typed): why that file is not written (design §5, ruling R5). Two sets binding one
/// EDID of one kind both refuse it; EDIDs compare case-insensitively because Windows file names do.
pub fn refusals(entries: &[SetEntry]) -> BTreeMap<(String, Kind, String), String> {
    let mut owners: BTreeMap<(Kind, String), Vec<&str>> = BTreeMap::new();
    for e in entries {
        for (kind, b) in blocks(e) {
            for edid in &b.edids {
                let o = owners.entry((kind, edid.to_lowercase())).or_default();
                if !o.contains(&e.id.as_str()) {
                    o.push(&e.id);
                }
            }
        }
    }
    let mut out = BTreeMap::new();
    for e in entries {
        for (kind, b) in blocks(e) {
            for edid in &b.edids {
                let o = &owners[&(kind, edid.to_lowercase())];
                let why = edid_problem(edid).or_else(|| {
                    (o.len() > 1).then(|| format!("{} {edid} is bound by {}", kind.label(), o.join(" and ")))
                });
                if let Some(w) = why {
                    out.insert((e.id.clone(), kind, edid.clone()), w);
                }
            }
        }
    }
    out
}

/// What an export about to start writes once it has ended (computed when it starts, like the PGPatcher JSON).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PendingCs {
    pub root: PathBuf,
    /// (path under the root, `/`-separated; text).
    pub files: Vec<(String, String)>,
    /// Refused files and left-out keys, for the export report.
    pub notes: Vec<String>,
}

/// Design §5: one file per EDID per block (CS only). `Some` on CS even without blocks: the orphan report needs it.
pub fn prepare(m: &AppModel) -> Option<PendingCs> {
    if m.target != Target::Cs {
        return None;
    }
    let entries = m.sets_doc.as_ref()?.entries();
    let refused = refusals(&entries);
    let mut p = PendingCs {
        root: m.export_root(),
        ..Default::default()
    };
    let mut done = BTreeSet::new();
    for e in &entries {
        for (kind, b) in blocks(e) {
            for k in &b.bad {
                p.notes.push(format!("{}: {}.{k} has the wrong type and was left out", e.id, kind.key()));
            }
            for edid in &b.edids {
                match refused.get(&(e.id.clone(), kind, edid.clone())) {
                    Some(why) => {
                        let n = format!("{} {edid} not written: {why}", kind.label());
                        if !p.notes.contains(&n) {
                            p.notes.push(n);
                        }
                    }
                    None => {
                        if done.insert((kind, edid.to_lowercase())) {
                            p.files.push((rel_path(kind, edid), to_text(kind, &b.values)));
                        }
                    }
                }
            }
        }
    }
    Some(p)
}

/// After a clean export (design §5, ruling R4): write each file unless it was edited outside sdc, then report the
/// files sdc wrote before that no block binds any more (never deleted). `shas` = the record (path → hash of what
/// sdc last wrote there); returns the new record.
pub fn finish(p: &PendingCs, shas: &BTreeMap<String, String>, notes: &mut Vec<String>) -> BTreeMap<String, String> {
    notes.extend(p.notes.iter().cloned());
    let mut out = BTreeMap::new();
    let mut written = 0;
    for (rel, text) in &p.files {
        let path = p.root.join(rel);
        // O6: an EDID whose case changed renames sdc's own (unchanged) file; a replace may keep the old name's case.
        if let Some((old, old_sha)) = shas
            .iter()
            .find(|(r, _)| r.as_str() != rel.as_str() && r.eq_ignore_ascii_case(rel))
        {
            if sha256_of(&p.root.join(old)).as_deref() == Some(old_sha.as_str()) {
                let _ = std::fs::remove_file(p.root.join(old));
            }
        }
        let last = shas.get(rel).map(String::as_str);
        if hand_edited(&path, text, last) {
            notes.push(format!(
                "{} kept as edited outside sdc (delete it to let sdc write it again)",
                path.display()
            ));
            if let Some(s) = last {
                out.insert(rel.clone(), s.to_string());
            }
            continue;
        }
        match write_atomic(&path, text) {
            Ok(()) => {
                written += 1;
                if let Some(s) = sha256_of(&path) {
                    out.insert(rel.clone(), s);
                }
            }
            Err(e) => {
                notes.push(format!("{} not written: {e}", path.display()));
                if let Some(s) = last {
                    out.insert(rel.clone(), s.to_string());
                }
            }
        }
    }
    if written > 0 {
        notes.push(format!("CS PBR JSON written: {written} files under {}", p.root.display()));
    }
    for (rel, sha) in shas {
        if p.files.iter().any(|(r, _)| r.eq_ignore_ascii_case(rel)) {
            continue;
        }
        let path = p.root.join(rel);
        if path.is_file() {
            notes.push(format!(
                "{}: no set binds this EDID any more; the file was left in place",
                path.display()
            ));
            out.insert(rel.clone(), sha.clone());
        }
    }
    out
}

impl AppModel {
    /// The export ended: after a clean export write the CS files of `p` (taken from `pending_cs` by the caller) and
    /// record their hashes (saved, like the PGPatcher JSON's); otherwise only a note.
    pub(crate) fn finish_cs_pbr(&mut self, p: Option<PendingCs>) {
        let Some(p) = p else {
            return;
        };
        let ok = self.export.error.is_none()
            && self
                .export
                .summary
                .as_ref()
                .is_some_and(|s| s.cancelled.is_empty());
        if !ok {
            if !p.files.is_empty() {
                self.export
                    .notes
                    .push("CS PBR JSON not written: the export did not finish".into());
            }
            return;
        }
        let recorded = match self.sets_doc.as_ref() {
            Some(d) => d.cs_pbr_shas(),
            None => return,
        };
        let shas = finish(&p, &recorded, &mut self.export.notes);
        if shas == recorded {
            return;
        }
        let Some(d) = self.sets_doc.as_mut() else {
            return;
        };
        if d.read_only() {
            self.export
                .notes
                .push("the CS PBR JSON hashes were not recorded: sdc-project.toml is read-only".into());
            return;
        }
        d.set_cs_pbr_shas(&shas);
        if let Err(e) = d.save(false) {
            self.dialog = Some(Dialog::Error(format!("sdc-project.toml: {e}")));
        }
    }
}
```

- [ ] **Step 6: Build, then commit the non-shared files**

Run: `cargo build -p sdc-gui --release` (builds: `finish_cs_pbr` takes its pending files as an argument, so it needs no field yet; the only new warning is that it is unused until Step 7).

```bash
git add crates/gui/src/model/pbr_export.rs crates/gui/src/sets_doc.rs crates/gui/src/model/cs_pbr_export.rs crates/gui/tests/cs_pbr_export.rs
git commit -m "feat(gui): CS PBR export files with per-file hand-edit guard, refusals and orphan report

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

- [ ] **Step 7: Hook the export** (shared `crates/gui/src/model/mod.rs`, add-only)

In `pub struct AppModel`, after `pending_text: Option<String>,`:

```rust
    /// Design §5: the CS PBR files the running export writes afterwards.
    pending_cs: Option<cs_pbr_export::PendingCs>,
```

In `placeholder`, after `pending_text: None,`: `pending_cs: None,`
In `open_folder`, after `self.pending_text = None;`: `self.pending_cs = None;`
In `start_export`, after `self.pending_json = pbr_export::prepare(self);`: `self.pending_cs = cs_pbr_export::prepare(self);`
After `self.finish_json();` (line 745):

```rust
                let cs = self.pending_cs.take();
                self.finish_cs_pbr(cs);
```

- [ ] **Step 8: Run the tests**

Run: `cargo test -p sdc-gui --release --test cs_pbr_export --test pbr_export --test export`
Expected: PASS.

- [ ] **Step 9: Gate and commit the shared file**

```bash
cargo fmt --all --check && cargo clippy --workspace --all-targets --release -- -D warnings && cargo test --workspace --release
git add crates/gui/src/model/mod.rs
git commit -m "feat(gui): write the CS PBR files at the end of an export

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 6: Statics rule and scan reads

**Files:**
- Modify: `crates/gui/src/model/cs_pbr.rs` (append)
- Modify (owner OK): `crates/gui/src/model/pbr_export.rs` (`prepare`, the `json_entries(m)` chain)
- Modify (shared, add-only, own commit): `crates/gui/src/model/mod.rs` (field `cs_pbr`, init)
- Test: `crates/gui/tests/cs_pbr.rs` (append)

**Interfaces:**
- Consumes: Task 1 `Store::winning_records`, `WinRec`; existing `Store::txsts_matching`, `Store::nifs_using_texture`, `MatchRow`, `plugin_refs::{set_key, LIMIT, NO_SCAN}`, `nif::path::is_lod_path`; Task 3 `AppModel::cs_block`, `Statics`.
- Produces (in `model::cs_pbr`): `STATICS_ROWS: usize`; `struct SetFacts { txst_edids: Vec<String>, statics: Vec<String> }`; `struct ScanRecords { txst: Vec<WinRec>, mato: Vec<WinRec> }` with `of(&self, Kind) -> &[WinRec]`; `struct CsPbrScan { facts: RefCell<HashMap<TexKey, Result<SetFacts, String>>>, records: RefCell<Option<Result<Arc<ScanRecords>, String>>> }` with `facts(&self, Option<&Path>, &TexKey) -> Result<SetFacts, String>`, `records(&self, Option<&Path>) -> Result<Arc<ScanRecords>, String>`, `refresh(&self)`; `fn non_lod_nifs(&[MatchRow]) -> Vec<String>`; `enum StaticsStatus { Never, Always, Used(Vec<String>), Unused, NoScan(String) }`; `fn statics_text(&StaticsStatus) -> String`; `AppModel::statics_status(&self, &SetView) -> Option<StaticsStatus>`; `AppModel::pgp_entry_wanted(&self, &SetView) -> bool`; field `AppModel::cs_pbr: CsPbrScan`.

- [ ] **Step 1: Write the failing tests** (append to `crates/gui/tests/cs_pbr.rs`)

```rust
use sdc_gui::model::plugin_refs::set_key;
use sdc_gui::model::sets::{set_views, SetView};
use sdc_gui::model::{AppModel, Dialog};

fn row(path: &str) -> modscan::store::MatchRow {
    modscan::store::MatchRow {
        copy_id: 1,
        nif_path: path.into(),
        origin: "A".into(),
        shape_block: 3,
        shape_name: None,
        le_mesh: false,
        skinned: false,
        derived: false,
        load_copy_id: 1,
    }
}

#[test]
fn lod_meshes_do_not_count_as_statics() {
    let rows = [
        row("meshes/landscape/rocks/rock01.nif"),
        row("meshes/landscape/rocks/rock01.nif"),
        row("meshes/lod/rocks/rock01_lod_0.nif"),
        row("meshes/dyndolod/x.nif"),
        row("meshes/rocks/rock02_lod.nif"),
    ];
    assert_eq!(non_lod_nifs(&rows), ["meshes/landscape/rocks/rock01.nif"]);
}

const ROCK: &str = "landscape/rock01";

fn rock(m: &AppModel) -> SetView {
    set_views(m).into_iter().find(|v| v.id == ROCK).unwrap()
}

/// The fixture's rock set with a texture-set block (LandscapeMountainSlab values) and `facts` as the scan's answer.
fn landscape(d: &std::path::Path, statics: &[&str]) -> AppModel {
    let mut m = support::pbr_opened(d);
    m.add_cs_block(ROCK, Kind::Txst);
    m.apply_cs_template(ROCK, Kind::Txst, "LandscapeMountainSlab").unwrap();
    m.set_cs_edids(ROCK, Kind::Txst, &["LandscapeRock01".into()]);
    let key = set_key(&rock(&m)).unwrap();
    m.cs_pbr.facts.borrow_mut().insert(
        key,
        Ok(SetFacts {
            txst_edids: vec![],
            statics: statics.iter().map(|s| s.to_string()).collect(),
        }),
    );
    m
}

fn export_and_wait(m: &mut AppModel) {
    m.export_clicked();
    if matches!(m.dialog, Some(Dialog::ProceedAnyway(_))) {
        m.proceed_anyway();
    }
    assert!(m.wait_idle(std::time::Duration::from_secs(120)), "export did not finish");
}

fn pgp_json(m: &AppModel) -> Option<Json> {
    let p = m
        .export_root()
        .join("PBRNIFPatcher")
        .join(format!("{}.json", sdc_gui::model::pbr_export::json_name(m)));
    std::fs::read_to_string(p).ok().map(|t| serde_json::from_str(&t).unwrap())
}

#[test]
fn statics_using_the_diffuse_get_the_entry_with_the_blocks_values() {
    let d = tempfile::tempdir().unwrap();
    let mut m = landscape(d.path(), &["meshes/landscape/rocks/rock01.nif"]);
    m.set_cs_value(ROCK, Kind::Txst, "specularLevel", Some(json!(0.02)));
    m.set_cs_value(ROCK, Kind::Txst, "glintParameters.logMicrofacetDensity", Some(json!(16)));
    let v = rock(&m);
    assert_eq!(m.statics_status(&v), Some(StaticsStatus::Used(vec!["meshes/landscape/rocks/rock01.nif".into()])));
    assert_eq!(statics_text(&m.statics_status(&v).unwrap()), "Used by 1 meshes: PGPatcher entry written");
    export_and_wait(&mut m);
    let pgp = pgp_json(&m).unwrap_or_else(|| panic!("no PGPatcher JSON; notes {:?}", m.export.notes));
    let e = &pgp["entries"][0];
    let cs: Json = serde_json::from_str(
        &std::fs::read_to_string(m.export_root().join("PBRTextureSets/LandscapeRock01.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(e["specular_level"], json!(0.02));
    assert_eq!(cs["specularLevel"], json!(0.02));
    let g = &cs["glintParameters"];
    assert_eq!(
        e["glint"],
        json!({"screen_space_scale": g["screenSpaceScale"], "log_microfacet_density": 16.0,
               "microfacet_roughness": g["microfacetRoughness"], "density_randomization": g["densityRandomization"]})
    );
    assert_eq!(g["logMicrofacetDensity"], json!(16.0));
}

#[test]
fn only_lod_users_or_never_write_the_texture_set_only() {
    for never in [false, true] {
        let d = tempfile::tempdir().unwrap();
        let statics: &[&str] = if never { &["meshes/landscape/rocks/rock01.nif"] } else { &[] };
        let mut m = landscape(d.path(), statics);
        if never {
            m.set_cs_statics(ROCK, Statics::Never);
        }
        assert!(!m.pgp_entry_wanted(&rock(&m)));
        export_and_wait(&mut m);
        assert!(pgp_json(&m).is_none(), "no entry, so no PGPatcher JSON");
        assert!(m.export_root().join("PBRTextureSets/LandscapeRock01.json").is_file());
    }
}

#[test]
fn a_disabled_glint_writes_no_glint() {
    let d = tempfile::tempdir().unwrap();
    let mut m = landscape(d.path(), &["meshes/landscape/rocks/rock01.nif"]);
    m.set_cs_value(ROCK, Kind::Txst, "glintParameters.enabled", Some(json!(false)));
    export_and_wait(&mut m);
    assert!(pgp_json(&m).unwrap()["entries"][0].get("glint").is_none());
}

/// Review focus: a scan database that cannot be read is "no scan", never a crash and never an entry.
#[test]
fn an_unusable_scan_db_writes_no_statics_entry_unless_always() {
    let d = tempfile::tempdir().unwrap();
    let mut m = support::pbr_opened(d.path());
    m.add_cs_block(ROCK, Kind::Txst);
    let bad = d.path().join("scan.db");
    std::fs::write(&bad, "not a database").unwrap();
    m.scan_db = Some(bad);
    let v = rock(&m);
    assert!(matches!(m.statics_status(&v), Some(StaticsStatus::NoScan(_))));
    assert!(!m.pgp_entry_wanted(&v));
    m.set_cs_statics(ROCK, Statics::Always);
    assert_eq!(m.statics_status(&rock(&m)), Some(StaticsStatus::Always));
    assert!(m.pgp_entry_wanted(&rock(&m)));
    // A set without a texture-set block is unchanged.
    m.remove_cs_block(ROCK, Kind::Txst);
    assert_eq!(m.statics_status(&rock(&m)), None);
    assert!(m.pgp_entry_wanted(&rock(&m)));
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p sdc-gui --release --test cs_pbr`
Expected: compile errors (`non_lod_nifs`, `SetFacts`, field `cs_pbr` missing).

- [ ] **Step 3: Implement the scan reads and the statics rule** (append to `crates/gui/src/model/cs_pbr.rs`)

```rust
use super::plugin_refs::{set_key, LIMIT, NO_SCAN};
use super::sets::SetView;
use modscan::plugin_store::WinRec;
use modscan::store::{MatchRow, Store, TexKey};
use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};
use std::path::Path;
use std::sync::Arc;

/// Shape rows read for the statics count; more only lowers a count that is already far above 0.
pub const STATICS_ROWS: usize = 20_000;

/// What the scan says about one set's diffuse (design §5.1, §6).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SetFacts {
    /// EDIDs of the winning TXSTs whose slot matches the set (`txsts_matching`): suggestions; W4 when empty.
    pub txst_edids: Vec<String>,
    /// Non-LOD winning NIFs whose shapes the set's PGPatcher entry matches (ruling R6).
    pub statics: Vec<String>,
}

/// The scan's winning TXST and MATO records with an EDID (W1–W3, the material-object picker).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ScanRecords {
    pub txst: Vec<WinRec>,
    pub mato: Vec<WinRec>,
}

impl ScanRecords {
    pub fn of(&self, kind: Kind) -> &[WinRec] {
        match kind {
            Kind::Txst => &self.txst,
            Kind::Mato => &self.mato,
        }
    }
}

/// The scan reads of the CS PBR section and the statics rule, for the session. shortcut: read on the UI thread on
/// demand (section open, export start) like "Used by plugins"; move to a worker if a modlist makes them slow.
/// Errors are not cached (a scan may be one click away); "Refresh scan" forgets everything.
#[derive(Debug, Default)]
pub struct CsPbrScan {
    pub facts: RefCell<HashMap<TexKey, Result<SetFacts, String>>>,
    pub records: RefCell<Option<Result<Arc<ScanRecords>, String>>>,
}

fn open(db: Option<&Path>) -> Result<Store, String> {
    let p = db.filter(|p| p.is_file()).ok_or_else(|| NO_SCAN.to_string())?;
    Store::open_read_only(p).map_err(|e| e.to_string())
}

/// Distinct mesh paths of `rows`, LOD meshes left out (`is_lod_path`, PGPatcher's own blocklist).
pub fn non_lod_nifs(rows: &[MatchRow]) -> Vec<String> {
    rows.iter()
        .map(|r| r.nif_path.as_str())
        .filter(|p| !nif::path::is_lod_path(p))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(str::to_string)
        .collect()
}

fn read_facts(db: Option<&Path>, key: &TexKey) -> Result<SetFacts, String> {
    let s = open(db)?;
    let mut txst_edids: Vec<String> = s
        .txsts_matching(key, LIMIT)
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter_map(|t| t.edid)
        .collect();
    txst_edids.sort();
    txst_edids.dedup();
    let rows = s
        .nifs_using_texture(key, STATICS_ROWS)
        .map_err(|e| e.to_string())?;
    Ok(SetFacts {
        txst_edids,
        statics: non_lod_nifs(&rows),
    })
}

fn read_records(db: Option<&Path>) -> Result<Arc<ScanRecords>, String> {
    let s = open(db)?;
    let w = |sig: &str| s.winning_records(sig).map_err(|e| e.to_string());
    Ok(Arc::new(ScanRecords {
        txst: w("TXST")?,
        mato: w("MATO")?,
    }))
}

impl CsPbrScan {
    pub fn facts(&self, db: Option<&Path>, key: &TexKey) -> Result<SetFacts, String> {
        if let Some(Ok(f)) = self.facts.borrow().get(key) {
            return Ok(f.clone());
        }
        let r = read_facts(db, key);
        self.facts.borrow_mut().insert(key.clone(), r.clone());
        r
    }

    pub fn records(&self, db: Option<&Path>) -> Result<Arc<ScanRecords>, String> {
        if let Some(Ok(r)) = &*self.records.borrow() {
            return Ok(r.clone());
        }
        let r = read_records(db);
        *self.records.borrow_mut() = Some(r.clone());
        r
    }

    pub fn refresh(&self) {
        self.facts.borrow_mut().clear();
        *self.records.borrow_mut() = None;
    }
}

/// Whether and why a texture-set set gets its PGPatcher entry (design §5.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StaticsStatus {
    Never,
    Always,
    /// The non-LOD meshes of the latest scan whose shapes the entry matches.
    Used(Vec<String>),
    Unused,
    NoScan(String),
}

pub fn statics_text(s: &StaticsStatus) -> String {
    match s {
        StaticsStatus::Used(p) => format!("Used by {} meshes: PGPatcher entry written", p.len()),
        StaticsStatus::Unused => {
            "No mesh of the latest scan uses this diffuse (LOD meshes do not count): no PGPatcher entry".into()
        }
        StaticsStatus::NoScan(e) => {
            format!("{e} No PGPatcher entry is written until a scan finds meshes using this diffuse.")
        }
        StaticsStatus::Always => "statics = always: PGPatcher entry written".into(),
        StaticsStatus::Never => "statics = never: no PGPatcher entry".into(),
    }
}
```

- [ ] **Step 4: Build, then commit the types** (nothing uses `CsPbrScan` yet, so this builds)

Run: `cargo build -p sdc-gui --release`

```bash
git add crates/gui/src/model/cs_pbr.rs
git commit -m "feat(gui): CS PBR scan reads (statics, winning records)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

- [ ] **Step 5: Add the field** (shared `crates/gui/src/model/mod.rs`, add-only)

In `pub struct AppModel`, after `pub plugin_refs: plugin_refs::SetPluginRefs,`:

```rust
    /// Design §5.1, §6: the CS PBR scan reads (statics, EDID checks, the material-object picker).
    pub cs_pbr: cs_pbr::CsPbrScan,
```

In `placeholder`, after `plugin_refs: Default::default(),`: `cs_pbr: Default::default(),`

```bash
cargo build -p sdc-gui --release
git add crates/gui/src/model/mod.rs
git commit -m "chore(gui): AppModel::cs_pbr scan reads

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

- [ ] **Step 6: Implement the statics rule** (append to `crates/gui/src/model/cs_pbr.rs`)

```rust
impl AppModel {
    /// `None` without a texture-set block (the set's entry is written as before).
    pub fn statics_status(&self, v: &SetView) -> Option<StaticsStatus> {
        let b = self.cs_block(&v.id, Kind::Txst)?;
        Some(match b.statics {
            Statics::Never => StaticsStatus::Never,
            Statics::Always => StaticsStatus::Always,
            Statics::Auto => match set_key(v) {
                None => StaticsStatus::Unused,
                Some(k) => match self.cs_pbr.facts(self.scan_db.as_deref(), &k) {
                    Err(e) => StaticsStatus::NoScan(e),
                    Ok(f) if f.statics.is_empty() => StaticsStatus::Unused,
                    Ok(f) => StaticsStatus::Used(f.statics),
                },
            },
        })
    }

    pub fn pgp_entry_wanted(&self, v: &SetView) -> bool {
        matches!(
            self.statics_status(v),
            None | Some(StaticsStatus::Always | StaticsStatus::Used(_))
        )
    }
}
```

- [ ] **Step 7: Filter the written entries** (`crates/gui/src/model/pbr_export.rs`, `prepare`)

Replace

```rust
    let mut entries: Vec<_> = json_entries(m)
        .into_iter()
        .map(|(v, e)| (v.id.clone(), deps(&v), e))
        .collect();
```

with

```rust
    let mut entries: Vec<_> = json_entries(m)
        .into_iter()
        // Design §5.1: a set with a texture-set block gets its entry only for the statics that use its diffuse.
        .filter(|(v, _)| m.pgp_entry_wanted(v))
        .map(|(v, e)| (v.id.clone(), deps(&v), e))
        .collect();
```

- [ ] **Step 8: Run the tests**

Run: `cargo test -p sdc-gui --release --test cs_pbr --test pbr_export`
Expected: PASS.

- [ ] **Step 9: Gate and commit**

```bash
cargo fmt --all --check && cargo clippy --workspace --all-targets --release -- -D warnings && cargo test --workspace --release
git add crates/gui/src/model/cs_pbr.rs crates/gui/src/model/pbr_export.rs crates/gui/tests/cs_pbr.rs
git commit -m "feat(gui): statics rule for PBR landscape sets (non-LOD meshes from the scan)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 7: EDID checks W1–W4

**Files:**
- Modify: `crates/gui/src/model/cs_pbr.rs` (append)
- Test: `crates/gui/tests/cs_pbr.rs` (append)

**Interfaces:**
- Consumes: Task 1 `WinRec`; Task 6 `ScanRecords`, `SetFacts`.
- Produces: `enum EdidCheck { Found, Case(String), NotFound, NotSinglePass, SinglePassUnknown }`; `fn check_edid(kind: Kind, edid: &str, recs: &[WinRec]) -> EdidCheck`; `fn check_text(&EdidCheck) -> Option<&'static str>`; `fn single_pass_text(Option<u32>) -> &'static str`; consts `NOT_CHECKED`, `NO_TXST_USES`.

- [ ] **Step 1: Write the failing test** (append to `crates/gui/tests/cs_pbr.rs`)

```rust
fn win(edid: &str, flags: Option<u32>) -> modscan::plugin_store::WinRec {
    modscan::plugin_store::WinRec {
        formid: 1,
        edid: edid.into(),
        plugin: "Skyrim.esm".into(),
        mato_flags: flags,
    }
}

#[test]
fn edid_checks_find_case_mismatches_and_single_pass_status() {
    let txst = [win("LandscapeSnow01", None)];
    assert_eq!(check_edid(Kind::Txst, "LandscapeSnow01", &txst), EdidCheck::Found);
    assert_eq!(check_edid(Kind::Txst, "landscapesnow01", &txst), EdidCheck::Case("LandscapeSnow01".into()));
    assert_eq!(check_edid(Kind::Txst, "Nope", &txst), EdidCheck::NotFound);
    let mato = [win("Snow1P", Some(1)), win("Plain", Some(0)), win("Ash1P", None)];
    assert_eq!(check_edid(Kind::Mato, "Snow1P", &mato), EdidCheck::Found);
    assert_eq!(check_edid(Kind::Mato, "Plain", &mato), EdidCheck::NotSinglePass);
    assert_eq!(check_edid(Kind::Mato, "Ash1P", &mato), EdidCheck::SinglePassUnknown);
    assert_eq!(check_text(&EdidCheck::Found), None);
    assert_eq!(
        check_text(&EdidCheck::NotSinglePass),
        Some("CS ignores this JSON unless the record is single-pass")
    );
    assert_eq!(single_pass_text(Some(1)), "single-pass");
}
```

- [ ] **Step 2: Run it to see it fail**

Run: `cargo test -p sdc-gui --release --test cs_pbr edid_checks`
Expected: compile error `cannot find function check_edid`.

- [ ] **Step 3: Implement** (append to `crates/gui/src/model/cs_pbr.rs`)

```rust
pub const NOT_CHECKED: &str = "EDIDs not checked: no plugin scan yet";
/// W4.
pub const NO_TXST_USES: &str = "No texture set of the latest scan uses this texture";

/// Design §6 W1–W3 for one EDID against the scan's winning records of its kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EdidCheck {
    Found,
    /// W2: only a case-insensitive match; the scanned record's exact spelling (CS is case-sensitive).
    Case(String),
    /// W1.
    NotFound,
    /// W3.
    NotSinglePass,
    /// W3, a 48-byte DATA.
    SinglePassUnknown,
}

pub fn check_edid(kind: Kind, edid: &str, recs: &[WinRec]) -> EdidCheck {
    match recs.iter().find(|r| r.edid == edid) {
        Some(r) if kind == Kind::Mato => match r.mato_flags {
            None => EdidCheck::SinglePassUnknown,
            Some(f) if f & 1 == 0 => EdidCheck::NotSinglePass,
            Some(_) => EdidCheck::Found,
        },
        Some(_) => EdidCheck::Found,
        None => match recs.iter().find(|r| r.edid.eq_ignore_ascii_case(edid)) {
            Some(r) => EdidCheck::Case(r.edid.clone()),
            None => EdidCheck::NotFound,
        },
    }
}

/// The warning shown next to the EDID; `None` when it is fine. W2's exact spelling is the fix button's label.
pub fn check_text(c: &EdidCheck) -> Option<&'static str> {
    Some(match c {
        EdidCheck::Found => return None,
        EdidCheck::Case(_) => "the scanned record differs in case only (CS matches the exact case)",
        EdidCheck::NotFound => "not found among the scanned winning records",
        EdidCheck::NotSinglePass => "CS ignores this JSON unless the record is single-pass",
        EdidCheck::SinglePassUnknown => "single-pass unknown (the record's DATA has no flags)",
    })
}

pub fn single_pass_text(flags: Option<u32>) -> &'static str {
    match flags {
        None => "single-pass unknown",
        Some(f) if f & 1 == 0 => "not single-pass",
        Some(_) => "single-pass",
    }
}
```

- [ ] **Step 4: Run it**

Run: `cargo test -p sdc-gui --release --test cs_pbr`
Expected: PASS.

- [ ] **Step 5: Gate and commit**

```bash
cargo fmt --all --check && cargo clippy --workspace --all-targets --release -- -D warnings && cargo test --workspace --release
git add crates/gui/src/model/cs_pbr.rs crates/gui/tests/cs_pbr.rs
git commit -m "feat(gui): EDID checks W1-W3 for CS PBR blocks

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 8: Set editor section

**Files:**
- Create: `crates/gui/src/ui/cs_pbr.rs`
- Modify (shared, add-only, own commit): `crates/gui/src/ui/mod.rs:3` (module list), `crates/gui/src/ui/set_editor.rs:131` and `:153` (mount)
- Test: `crates/gui/tests/cs_pbr_ui.rs` (create)

**Interfaces:**
- Consumes: Tasks 2–7 (`templates`, `fields`, `Field`, `FieldKind`, `GLINT_FIELDS`, `CsBlock`, `Statics`, `AppModel::{cs_block, add_cs_block, remove_cs_block, set_cs_value, set_cs_edids, apply_cs_template, set_cs_statics, statics_status}`, `CsPbrScan::{facts, records, refresh}`, `check_edid`, `check_text`, `single_pass_text`, `statics_text`, `NOT_CHECKED`, `NO_TXST_USES`, `cs_pbr_export::refusals`, `MATO_SPECULAR`).
- Produces: `ui::cs_pbr::section(ui: &mut egui::Ui, model: &mut AppModel, v: &SetView)`; header label `SECTION = "Community Shaders PBR records"`.

- [ ] **Step 1: Write the failing test** (create `crates/gui/tests/cs_pbr_ui.rs`)

```rust
//! The set editor's CS PBR section (design §4, §6).
mod support;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use sdc_gui::model::cs_pbr::{Kind, NOT_CHECKED};
use sdc_gui::model::sets::set_views;
use sdc_gui::model::AppModel;
use support::*;

fn tall(model: AppModel) -> Harness<'static, AppModel> {
    let h = Harness::builder()
        .with_size(eframe::egui::vec2(1400.0, 2400.0))
        .build_ui_state(|ui, model: &mut AppModel| sdc_gui::ui::draw(ui, model), model);
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h
}

fn has(h: &Harness<'static, AppModel>, s: &str) -> bool {
    h.query_all_by_label_contains(s).next().is_some()
}

#[test]
fn the_section_shows_the_block_template_refusals_and_checks() {
    let d = tempfile::tempdir().unwrap();
    let mut model = pbr_opened(d.path());
    let id = "landscape/rock01";
    model.add_cs_block(id, Kind::Txst);
    model.apply_cs_template(id, Kind::Txst, "LandscapeMountainSlab").unwrap();
    model.set_cs_edids(id, Kind::Txst, &["Bad:Name".into()]);
    let key = set_views(&model).into_iter().find(|v| v.id == id).unwrap().key;
    model.select_set(&key);
    let mut h = tall(model);
    h.run();
    h.get_by_label(sdc_gui::ui::cs_pbr::SECTION).click();
    h.run();
    assert!(has(&h, "PBR texture set"));
    assert!(has(&h, "Add PBR material object"));
    assert!(has(&h, "LandscapeMountainSlab"), "the applied template");
    assert!(has(&h, "Bad:Name"));
    assert!(has(&h, "not written: EDID: contains a character"));
    assert!(has(&h, NOT_CHECKED));
    assert!(has(&h, "No scan yet"), "the statics line says why there is no entry");
    h.get_by_label("Add PBR material object").click();
    h.run();
    assert!(h.state().cs_block(id, Kind::Mato).is_some());
}
```

- [ ] **Step 2: Run it to see it fail**

Run: `cargo test -p sdc-gui --release --test cs_pbr_ui`
Expected: compile error `could not find cs_pbr in ui`.

- [ ] **Step 3: Implement** (create `crates/gui/src/ui/cs_pbr.rs`)

```rust
//! The set editor's Community Shaders PBR section (design §4, §6): per kind a block with its template picker,
//! fields, EDID list, warnings and (texture sets) the statics line. Drawing only: every change is a model call made
//! after drawing.
use super::columns::{AMBER, RED};
use crate::model::cs_pbr::{
    check_edid, check_text, fields, single_pass_text, statics_text, templates, CsBlock, EdidCheck, Field, FieldKind,
    Kind, Statics, StaticsStatus, GLINT_FIELDS, NOT_CHECKED, NO_TXST_USES,
};
use crate::model::cs_pbr_export::{refusals, MATO_SPECULAR};
use crate::model::plugin_refs::set_key;
use crate::model::sets::SetView;
use crate::model::AppModel;
use eframe::egui;
use serde_json::{json, Value as Json};

pub const SECTION: &str = "Community Shaders PBR records";

enum Act {
    Add(Kind),
    Remove(Kind),
    Value(Kind, String, Option<Json>),
    Edids(Kind, Vec<String>),
    Template(Kind, String),
    Statics(Statics),
    Refresh,
}

pub fn section(ui: &mut egui::Ui, model: &mut AppModel, v: &SetView) {
    if model.target != engine::config::Target::Cs {
        return;
    }
    let mut acts = vec![];
    let m: &AppModel = model;
    egui::CollapsingHeader::new(SECTION)
        .id_salt(("cs-pbr", v.id.as_str()))
        .show(ui, |ui| {
            if ui.small_button("Refresh scan").clicked() {
                acts.push(Act::Refresh);
            }
            for kind in [Kind::Txst, Kind::Mato] {
                ui.separator();
                match m.cs_block(&v.id, kind) {
                    None => {
                        if ui.button(format!("Add {}", kind.label())).clicked() {
                            acts.push(Act::Add(kind));
                        }
                    }
                    Some(b) => block(ui, m, v, kind, &b, &mut acts),
                }
            }
        });
    for a in acts {
        match a {
            Act::Add(k) => model.add_cs_block(&v.id, k),
            Act::Remove(k) => model.remove_cs_block(&v.id, k),
            Act::Value(k, key, x) => model.set_cs_value(&v.id, k, &key, x),
            Act::Edids(k, l) => model.set_cs_edids(&v.id, k, &l),
            Act::Template(k, name) => {
                let _ = model.apply_cs_template(&v.id, k, &name);
            }
            Act::Statics(s) => model.set_cs_statics(&v.id, s),
            Act::Refresh => model.cs_pbr.refresh(),
        }
    }
}

fn block(ui: &mut egui::Ui, m: &AppModel, v: &SetView, kind: Kind, b: &CsBlock, acts: &mut Vec<Act>) {
    ui.horizontal(|ui| {
        ui.strong(kind.label());
        egui::ComboBox::from_id_salt(("cs-template", kind.key(), v.id.as_str()))
            .selected_text(b.template.clone().unwrap_or_else(|| "Template…".into()))
            .show_ui(ui, |ui| {
                for t in templates().iter().filter(|t| t.kind == kind) {
                    let sel = b.template.as_deref() == Some(t.name.as_str());
                    if ui
                        .selectable_label(sel, format!("{} ({})", t.name, t.edids.join(", ")))
                        .clicked()
                    {
                        acts.push(Act::Template(kind, t.name.clone()));
                    }
                }
            });
        if ui.small_button("Remove").clicked() {
            acts.push(Act::Remove(kind));
        }
    });
    for f in fields(kind) {
        field_row(ui, kind, f, b.values.get(f.name), acts);
    }
    for k in &b.bad {
        ui.colored_label(AMBER, format!("{k}: wrong type in sdc-project.toml; not exported"));
    }
    edids(ui, m, v, kind, b, acts);
    if kind == Kind::Txst {
        statics(ui, m, v, b, acts);
    }
}

fn default_of(f: &Field) -> Json {
    match f.kind {
        FieldKind::Num => json!(f.default),
        FieldKind::Rgb => json!([f.default; 3]),
        FieldKind::Glint => json!({"enabled": true}),
    }
}

fn default_text(kind: Kind, f: &Field) -> String {
    match f.kind {
        _ if kind == Kind::Mato && f.name == "specularLevel" => {
            format!("written as {MATO_SPECULAR} (CS default {})", f.default)
        }
        FieldKind::Glint => "CS default: off".into(),
        FieldKind::Rgb => format!("CS default: {0}, {0}, {0}", f.default),
        FieldKind::Num => format!("CS default: {}", f.default),
    }
}

fn drag(ui: &mut egui::Ui, x: &mut f64, f: &Field) -> bool {
    ui.add(egui::DragValue::new(x).range(f.range.0..=f.range.1).speed(0.01))
        .changed()
}

fn field_row(ui: &mut egui::Ui, kind: Kind, f: &Field, cur: Option<&Json>, acts: &mut Vec<Act>) {
    ui.horizontal_wrapped(|ui| {
        let mut on = cur.is_some();
        if ui.checkbox(&mut on, f.name).changed() {
            acts.push(Act::Value(kind, f.name.into(), on.then(|| default_of(f))));
        }
        let Some(cur) = cur else {
            ui.weak(default_text(kind, f));
            return;
        };
        match f.kind {
            FieldKind::Num => {
                let mut x = cur.as_f64().unwrap_or(f.default);
                if drag(ui, &mut x, f) {
                    acts.push(Act::Value(kind, f.name.into(), Some(json!(x))));
                }
            }
            FieldKind::Rgb => {
                let mut c: Vec<f64> = cur
                    .as_array()
                    .map(|a| a.iter().map(|x| x.as_f64().unwrap_or(0.0)).collect())
                    .unwrap_or_default();
                let mut changed = false;
                for x in c.iter_mut() {
                    changed |= drag(ui, x, f);
                }
                if changed {
                    acts.push(Act::Value(kind, f.name.into(), Some(json!(c))));
                }
            }
            FieldKind::Glint => {
                let mut en = cur.get("enabled").and_then(Json::as_bool).unwrap_or(false);
                if ui.checkbox(&mut en, "enabled").changed() {
                    acts.push(Act::Value(kind, format!("{}.enabled", f.name), Some(json!(en))));
                }
                for g in &GLINT_FIELDS {
                    let mut x = cur.get(g.name).and_then(Json::as_f64).unwrap_or(g.default);
                    ui.label(g.name);
                    if drag(ui, &mut x, g) {
                        acts.push(Act::Value(kind, format!("{}.{}", f.name, g.name), Some(json!(x))));
                    }
                }
            }
        }
    });
}

fn edids(ui: &mut egui::Ui, m: &AppModel, v: &SetView, kind: Kind, b: &CsBlock, acts: &mut Vec<Act>) {
    let refused = refusals(&m.sets_doc.as_ref().map(|d| d.entries()).unwrap_or_default());
    let recs = m.cs_pbr.records(m.scan_db.as_deref());
    let scanned = recs.as_ref().ok().filter(|r| !r.of(kind).is_empty());
    ui.label("EDIDs (the file names, exact case):");
    for (i, e) in b.edids.iter().enumerate() {
        ui.horizontal_wrapped(|ui| {
            ui.monospace(e);
            if ui.small_button("×").on_hover_text("Remove this EDID").clicked() {
                let mut l = b.edids.clone();
                l.remove(i);
                acts.push(Act::Edids(kind, l));
            }
            if let Some(why) = refused.get(&(v.id.clone(), kind, e.clone())) {
                ui.colored_label(RED, format!("not written: {why}"));
            }
            if let Some(r) = scanned {
                let c = check_edid(kind, e, r.of(kind));
                if let Some(t) = check_text(&c) {
                    ui.colored_label(AMBER, t);
                }
                if let EdidCheck::Case(exact) = &c {
                    if ui.small_button(format!("Use {exact}")).clicked() {
                        let mut l = b.edids.clone();
                        l[i] = exact.clone();
                        acts.push(Act::Edids(kind, l));
                    }
                }
            }
        });
    }
    if scanned.is_none() {
        ui.weak(NOT_CHECKED);
    }
    let id = ui.make_persistent_id(("cs-edid-text", kind.key(), v.id.as_str()));
    let mut text: String = ui.data_mut(|d| d.get_temp(id).unwrap_or_default());
    ui.horizontal(|ui| {
        ui.text_edit_singleline(&mut text);
        if ui.button("Add EDID").clicked() && !text.trim().is_empty() {
            let mut l = b.edids.clone();
            l.push(text.trim().to_string());
            acts.push(Act::Edids(kind, l));
            text.clear();
        }
    });
    let suggestions: Vec<(String, String)> = match kind {
        Kind::Txst => match set_key(v).map(|k| m.cs_pbr.facts(m.scan_db.as_deref(), &k)) {
            Some(Ok(f)) => {
                if f.txst_edids.is_empty() {
                    ui.colored_label(AMBER, NO_TXST_USES);
                }
                f.txst_edids.into_iter().map(|e| (e.clone(), e)).collect()
            }
            _ => vec![],
        },
        Kind::Mato => scanned
            .map(|r| {
                let q = text.to_lowercase();
                r.mato
                    .iter()
                    .filter(|w| w.edid.to_lowercase().contains(&q))
                    .take(50)
                    .map(|w| {
                        let label = format!("{} · {} · {}", w.edid, w.plugin, single_pass_text(w.mato_flags));
                        (w.edid.clone(), label)
                    })
                    .collect()
            })
            .unwrap_or_default(),
    };
    ui.data_mut(|d| d.insert_temp(id, text));
    let new: Vec<_> = suggestions.into_iter().filter(|(e, _)| !b.edids.contains(e)).collect();
    if !new.is_empty() {
        ui.horizontal_wrapped(|ui| {
            ui.weak("From the scan:");
            for (e, label) in new {
                if ui.small_button(format!("+ {label}")).clicked() {
                    let mut l = b.edids.clone();
                    l.push(e);
                    acts.push(Act::Edids(kind, l));
                }
            }
        });
    }
}

fn statics(ui: &mut egui::Ui, m: &AppModel, v: &SetView, b: &CsBlock, acts: &mut Vec<Act>) {
    ui.horizontal(|ui| {
        ui.label("PGPatcher entry for statics:");
        for (s, name) in [(Statics::Auto, "auto"), (Statics::Always, "always"), (Statics::Never, "never")] {
            if ui.selectable_label(b.statics == s, name).clicked() && b.statics != s {
                acts.push(Act::Statics(s));
            }
        }
    });
    if let Some(st) = m.statics_status(v) {
        ui.label(statics_text(&st));
        if let StaticsStatus::Used(paths) = &st {
            egui::CollapsingHeader::new("Meshes")
                .id_salt(("cs-statics", v.id.as_str()))
                .show(ui, |ui| {
                    for p in paths {
                        ui.monospace(p);
                    }
                });
        }
    }
}
```

- [ ] **Step 4: Commit the new files**

```bash
git add crates/gui/src/ui/cs_pbr.rs crates/gui/tests/cs_pbr_ui.rs
git commit -m "feat(gui): set editor section for CS PBR blocks

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

- [ ] **Step 5: Mount it** (shared, add-only). `crates/gui/src/ui/mod.rs`, after `pub mod columns;`:

```rust
pub mod cs_pbr;
```

`crates/gui/src/ui/set_editor.rs`: in the 3D branch, after `under_grid(ui, model, &v);` (inside the `ScrollArea` closure) add `super::cs_pbr::section(ui, model, &v);`; in the 2D branch, after `super::plugin_refs::set_section(ui, model, &v);` add:

```rust
                super::cs_pbr::section(ui, model, &v);
```

- [ ] **Step 6: Run the tests**

Run: `cargo test -p sdc-gui --release --test cs_pbr_ui --test set_editor_ui --test set_plugin_refs --test ui_smoke`
Expected: PASS.

- [ ] **Step 7: Gate and commit**

```bash
cargo fmt --all --check && cargo clippy --workspace --all-targets --release -- -D warnings && cargo test --workspace --release
git add crates/gui/src/ui/mod.rs crates/gui/src/ui/set_editor.rs
git commit -m "chore(gui): mount the CS PBR section in the set editor

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 9: Preview plane lock and tiling

**Files:**
- Modify: `crates/gui/src/model/set_preview.rs:1100-1123` (`SetFingerprint`, `fingerprint`), `:1849-1885` (`check`), `:1939-1946` (`pick`), `:2056-2090` (`builtin_scene`), `:2111-2119` (LE notice in `push_scene`), `:2320-2340` (`apply_materials`), `SetPreview` struct and `new`
- Modify: `crates/gui/src/ui/set_preview.rs:92` (`draw`)
- Modify: `crates/gui/tests/set_preview_model.rs:388-404` (`fp_of` literal), append a test
- Test: `crates/gui/tests/set_preview_ui.rs` (append)

**Interfaces:**
- Consumes: Task 3 `AppModel::{cs_block, add_cs_block}`, `Kind`.
- Produces: `SetFingerprint::landscape: bool` (a `cs_txst` block, ruling R1); `SetPreview::tiling: f32` (1.0 default); `SetPreview::set_tiling(&mut self, t: f32)` (clamped 1.0–3.0, steps of 0.5).

- [ ] **Step 1: Write the failing tests**

Append to `crates/gui/tests/set_preview_model.rs`:

```rust
/// Design §7 (ruling R1): a texture-set block locks the plane; tiling multiplies the pgpsim `uv_scale` (2 at 2× = 4).
#[test]
fn a_texture_set_block_locks_the_plane_and_tiling_multiplies_after_pgpsim() {
    use sdc_gui::model::cs_pbr::Kind;
    let dir = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(dir.path());
    let key = rock(&m).0.key;
    m.select_set(&key);
    m.set_json_value("landscape/rock01", "uv_scale", Some(json!(2.0)));
    m.add_cs_block("landscape/rock01", Kind::Txst);
    let mut s = SetPreview::new(None, None, None, DEFAULT_CAP);
    run(&mut s, &m);
    let uv = |s: &SetPreview| match s.v3.content() {
        Some(Content::Builtin {
            shape: Builtin::Plane,
            material,
            ..
        }) => material.params.uv_scale,
        c => panic!("{c:?}"),
    };
    assert_eq!(uv(&s), [2.0, 2.0]);
    s.choose(MeshChoice::Builtin(Builtin::Cube));
    run(&mut s, &m);
    assert_eq!(uv(&s), [2.0, 2.0], "the plane stays");
    s.set_tiling(2.2);
    assert_eq!(s.tiling, 2.0, "steps of 0.5");
    assert_eq!(s.rows[0].material.params.uv_scale, [4.0, 4.0]);
    run(&mut s, &m);
    assert_eq!(uv(&s), [4.0, 4.0]);
}
```

In the same file's `fp_of` literal, add `landscape: false,` after `label: "x/rock".into(),`.

Append to `crates/gui/tests/set_preview_ui.rs`:

```rust
#[test]
fn a_texture_set_block_shows_tiling_instead_of_the_mesh_picker() {
    let d = tempfile::tempdir().unwrap();
    let mut m = rock_selected(d.path());
    m.add_cs_block("landscape/rock01", sdc_gui::model::cs_pbr::Kind::Txst);
    let mut h = harness(m, SetPreview::new(None, None, None, DEFAULT_CAP));
    settle(&mut h);
    h.get_by_label_contains(TILING);
    assert!(h.query_by_role(eframe::egui::accesskit::Role::ComboBox).is_none());
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p sdc-gui --release --test set_preview_model --test set_preview_ui`
Expected: compile errors (`no field landscape`, `set_tiling`, `TILING` missing).

- [ ] **Step 3: Implement the model** (`crates/gui/src/model/set_preview.rs`)

Near the other text constants (`NO_BUILTIN_MATCH`, `LE_ONLY`) add:

```rust
/// Design §7: the landscape plane's preview-only tiling slider.
pub const TILING: &str = "Tiling (preview only)";
```

`SetFingerprint`, after `label`:

```rust
    /// Design §7 (plan ruling R1): the set has a texture-set block, so it is shown on the plane only.
    pub landscape: bool,
```

`fingerprint`, in the literal after `label: v.label.clone(),`:

```rust
        landscape: m.cs_block(&v.id, crate::model::cs_pbr::Kind::Txst).is_some(),
```

`SetPreview` struct, after `pub choice: HashMap<String, MeshChoice>,`:

```rust
    /// Design §7: the landscape plane's tiling, 1.0–3.0 in steps of 0.5; this session only.
    pub tiling: f32,
```

and in `new`, after `choice: HashMap::new(),`: `tiling: 1.0,`.

`check`: replace

```rust
        let d = classify(self.fp.as_ref(), &fp, &self.default);
        if d == Delta::default() {
            return;
        }
```

with

```rust
        let d = classify(self.fp.as_ref(), &fp, &self.default);
        let relock = self.fp.as_ref().is_some_and(|f| f.landscape != fp.landscape);
        if d == Delta::default() && !relock {
            return;
        }
```

and `if d.set || d.encode || d.reload {` with `if d.set || d.encode || d.reload || relock {`.

`pick`, right after `let Some(fp) = &self.fp else { … };`:

```rust
        if fp.landscape {
            return Pick::Builtin(Builtin::Plane);
        }
```

`push_scene`, the LE notice: replace `if auto && !l.is_empty() && l.iter().all(|c| c.le_mesh) {` with `if auto && !fp.landscape && !l.is_empty() && l.iter().all(|c| c.le_mesh) {`.

`builtin_scene`: in the simulated branch replace

```rust
                let row = rows.remove(0);
                return (row.material.clone(), row);
```

with

```rust
                let mut row = rows.remove(0);
                row.material = self.tiled(row.material);
                return (row.material.clone(), row);
```

and in the plain branch replace `let m = plain_material(&members);` with `let m = self.tiled(plain_material(&members));`.

Add to `impl SetPreview` (next to `choose`):

```rust
    /// Design §7: a landscape set's tiling. A constant-buffer change only (`SetMaterial`; samplers already WRAP).
    pub fn set_tiling(&mut self, t: f32) {
        let t = (t.clamp(1.0, 3.0) * 2.0).round() / 2.0;
        if t != self.tiling {
            self.tiling = t;
            self.apply_materials();
        }
    }

    /// The tiling on a landscape set's material, applied after the pgpsim patch (pgpsim overwrites `uv_scale`).
    fn tiled(&self, mut m: MaterialState) -> MaterialState {
        if self.fp.as_ref().is_some_and(|f| f.landscape) {
            let [u, v] = m.params.uv_scale;
            m.params.uv_scale = [u * self.tiling, v * self.tiling];
        }
        m
    }
```

`apply_materials`: a built-in shape needs no entry (a plain landscape set still re-tiles). Replace its start, from `let Some(entry) = self.entry.clone() else {` down to the end of the `let rows = match self.on_screen { … };` statement, with:

```rust
        let rows = match self.on_screen {
            Some(Shown::Builtin(_)) => vec![self.builtin_scene().1],
            Some(Shown::Nif(id)) => {
                let Some(entry) = self.entry.clone() else {
                    return;
                };
                match self.nif.clone().filter(|n| n.load_copy_id == id) {
                    Some(nif) => self.nif_rows(&nif, &entry),
                    None => {
                        self.want_scene = true;
                        return;
                    }
                }
            }
            None => {
                self.want_scene = true;
                return;
            }
        };
```

(The only caller before this change, `check`, reaches here with an entry, so its behaviour is unchanged.)

- [ ] **Step 4: Implement the panel** (`crates/gui/src/ui/set_preview.rs`)

Add `TILING` to the `use crate::model::set_preview::{…}` list, then replace `mesh_picker(ui, s, fp.entry.is_some());` with:

```rust
    if fp.landscape {
        ui.label("Mesh: Plane (a PBR texture set previews on the plane)");
        let mut t = s.tiling;
        if ui
            .add(egui::Slider::new(&mut t, 1.0..=3.0).step_by(0.5).text(TILING))
            .changed()
        {
            s.set_tiling(t);
        }
    } else {
        mesh_picker(ui, s, fp.entry.is_some());
    }
```

- [ ] **Step 5: Run the tests**

Run: `cargo test -p sdc-gui --release --test set_preview_model --test set_preview_ui --test set_preview_layout --test viewport3d`
Expected: PASS (the fixture set has no block, so the existing tests keep the sphere, NIF and Cube behaviour).

- [ ] **Step 6: Gate and commit**

```bash
cargo fmt --all --check && cargo clippy --workspace --all-targets --release -- -D warnings && cargo test --workspace --release
git add crates/gui/src/model/set_preview.rs crates/gui/src/ui/set_preview.rs crates/gui/tests/set_preview_model.rs crates/gui/tests/set_preview_ui.rs
git commit -m "feat(gui): plane lock and preview tiling for PBR texture-set sets

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 10: Docs, credit, Note AD and the owner-data check

**Files:**
- Modify: `docs/CREDITS.md` (append a bullet), `docs/gui.md` (new section after "### Check by hand", before "## RNM normal layers"), `docs/DECISIONS.md` (append Note AD)
- Modify: `crates/modscan/tests/mato.rs` (append the `#[ignore]` owner test)

**Interfaces:**
- Consumes: everything above (names and texts only).
- Produces: nothing for code.

- [ ] **Step 1: Owner-data test** (append to `crates/modscan/tests/mato.rs`)

```rust
/// Owner data (design §9): the MATO query and single-pass flags on the owner's load order. Read-only; the store is
/// a temp file. `$env:SDC_ACHERUS='C:\_mount\moddingvol\Acherus'; cargo test -p modscan --release --test mato -- --ignored --nocapture`
#[test]
#[ignore]
fn owner_matos_and_single_pass_flags() {
    use modscan::mo2::loose_origins;
    use modscan::plugins::scan_plugins;
    let root = std::path::PathBuf::from(
        std::env::var("SDC_ACHERUS").expect("set SDC_ACHERUS to the MO2 instance root"),
    );
    let db = tempfile::tempdir().unwrap();
    let inst = Instance::load(&root, None).unwrap();
    let (loose, _) = loose_origins(&inst).unwrap();
    let mut store = Store::open(&db.path().join("scan.db")).unwrap();
    scan_plugins(&inst, &loose, &mut store, &AtomicBool::new(false), &|_| {}).unwrap();
    let m = store.winning_records("MATO").unwrap();
    let single = m.iter().filter(|w| w.mato_flags.is_some_and(|f| f & 1 == 1)).count();
    let unknown = m.iter().filter(|w| w.mato_flags.is_none()).count();
    println!("MATO winners: {} (single-pass {single}, no flags field {unknown})", m.len());
    for w in m.iter().filter(|w| w.edid.starts_with("SnowMaterialObject") || w.edid.starts_with("AshMaterialSolstheim")) {
        println!("{w:?}");
    }
    assert!(!m.is_empty() && single > 0);
}
```

Run once on the owner's machine: `$env:SDC_ACHERUS='C:\_mount\moddingvol\Acherus'; cargo test -p modscan --release --test mato -- --ignored --nocapture` → PASS, and paste the printed counts into the commit message body. Plain `cargo test -p modscan --release --test mato` → 1 PASS, 1 ignored.

- [ ] **Step 2: Credit** (`docs/CREDITS.md`, append)

```markdown
- **Faultier**: PBR landscape template values by Faultier (https://www.nexusmods.com/profile/Faultierfan?gameId=1704), from Faultier's PBR landscape modders resource.
```

- [ ] **Step 3: User guide** (`docs/gui.md`, new section after "### Check by hand")

```markdown
### Community Shaders PBR records

A set can also write Community Shaders' own runtime files: a **PBR texture set** (`<export>\PBRTextureSets\<EDID>.json`) and a **PBR material object** (`<export>\PBRMaterialObjects\<EDID>.json`). Open **Community Shaders PBR records** in the set editor and add either block.

- **Template** fills the block's values from the built-in library. PBR landscape template values by Faultier (https://www.nexusmods.com/profile/Faultierfan?gameId=1704), from Faultier's PBR landscape modders resource. A block with no EDIDs takes the template's EDIDs; EDIDs you added are never removed.
- **EDIDs** are the records the file is for, and the file name is the EDID exactly, case included (Community Shaders matches the exact case). After a plugin scan in the Meshes view, texture-set EDIDs are suggested from the texture sets that use the set's diffuse, and material objects can be picked from the scanned records. A warning says when an EDID is not in the scan, differs only in case (one click fixes it), or names a material object that is not single-pass (Community Shaders ignores the file then). An EDID that cannot be a file name, or that two sets bind, is not written.
- A texture set also drives the set's PGPatcher entry: roughness, specular, displacement, subsurface and coat values and glint come from the block, so the two files cannot drift. The entry is written only when the latest scan finds meshes (not LOD) that use the set's diffuse; **PGPatcher entry for statics** can force it on or off.
- Every number is written with a decimal point. A file you edited outside sdc is never overwritten (the export report says so; delete it to let sdc write it again), and a file sdc wrote for an EDID no set binds any more is reported, not deleted.
- In the 3D preview a set with a texture-set block is shown on the plane, with a **Tiling** slider (1× to 3×) that only changes the preview.
```

- [ ] **Step 4: Note AD** (`docs/DECISIONS.md`, append at the end)

```markdown
## Note AD — Community Shaders PBR texture sets and material objects (plan rulings, 2026-10-13)

Spec `F:\ai\dds\nifparseref\plan\pbr-landscape\2026-10-13-pbr-landscape-design.md` (owner-approved, including edits to `sets_doc.rs`, `model/pbr_export.rs` and `pgp/writer.rs`); plan `…\2026-10-13-plan-pbr-landscape.md`.

- **AD1 Blocks.** `[sets.cs_txst]` and `[sets.cs_mato]` are reserved keys of a `[[sets]]` table, with CS's own field names; unknown keys stay in the file and are never exported; a wrong-typed known key is warned about and left out.
- **AD2 One source.** A texture-set block's mapped values replace the set's own PGPatcher keys when read (`SetsDoc` overlay), and a PGPatcher edit of a mapped key is written into the block. `glint_on` follows `glintParameters.enabled`; `subsurface` and `multilayer` stay the user's switches. `pgp/writer.rs` is unchanged.
- **AD3 Library.** 31 texture-set and 3 material-object templates from Faultier's resource, deduplicated by value; each is named after its first EDID alphabetically and keeps every EDID it was used for. Generated once by `tools/gen-cs-pbr-templates.py`.
- **AD4 Files.** One file per EDID; keys sorted as CS's own saver writes them, 4-space indent, every number a float, `\n` line ends, no final newline. A material object always has `specularLevel` (0.04 when unset).
- **AD5 Guard and orphans.** `[project] cs_pbr_sha256` records each file's hash. A file edited outside sdc, or present without a record, is kept and reported, never asked about. A recorded file no block binds is reported, never deleted. An EDID whose case changed renames sdc's own unchanged file.
- **AD6 Refusals per file.** An EDID that cannot be a Windows file name, or that two sets bind in one kind (case-insensitive), is not written, with a note; export is not blocked.
- **AD7 Statics.** The PGPatcher entry of a set with a texture-set block is written when `Store::nifs_using_texture` finds non-LOD winning meshes for the set's key (`statics = "auto"`), or always/never by override. No scan writes no entry.
- **AD8 Scan database schema 7.** `rec.mato_flags` holds the MATO DATA flags; `Store::winning_records(sig)` lists winning records with an EDID.
- **AD9 Preview.** Deviation from spec §7, flagged to the owner: only a set with a texture-set block is locked to the plane (the path rule `landscape\` is not used); tiling 1×–3× multiplies `uv_scale` after the pgpsim patch, preview only.
- **AD10 Version** stays 0.7.0: nothing changes until a set gets a block.
```

- [ ] **Step 5: Final gate and commit**

```bash
cargo fmt --all --check && cargo clippy --workspace --all-targets --release -- -D warnings && cargo test --workspace --release
git add docs/CREDITS.md docs/gui.md docs/DECISIONS.md crates/modscan/tests/mato.rs
git commit -m "docs: CS PBR records guide, Faultier credit, Note AD; owner-data MATO check

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

## Self-review (done while writing)

- **Spec coverage.** §1 files and folders: Tasks 4–5. §2 O1/O4 library: Task 2; O2 credit: Task 10; O3 statics: Task 6; O5 preview: Task 9; O6 exact stem: Tasks 4–5 (`rel_path`, case rename). §3 data model, reserved keys, absent fields omitted: Tasks 3–4. §4 library file, script, picker, one undo step, EDID offer, credit: Tasks 2, 3, 8, 10. §5 export, format, guard reuse, refusals, orphans: Tasks 4–5. §5.1 statics, mapping, override, "Used by N meshes": Tasks 3, 6, 8. §6 suggestions, MATO picker, W1–W4: Tasks 1, 6, 7, 8. §7 plane lock and tiling: Task 9 (ruling R1 narrows the landscape rule). §8 malformed blocks, per-file write errors: Tasks 3, 5. §9 tests: writer goldens (T4), round trip and no leak (T3), library and `#[ignore]` equality (T2), EDID validation (T5, T7), statics (T6), export (T5), preview (T9), owner data (T10). §10 files: all listed in File Structure.
- **Placeholders.** None: every code step carries its code; every run step has a command and an expected result.
- **Type consistency.** `Kind`, `CsBlock`, `Statics`, `SetFacts`, `ScanRecords`, `WinRec`, `StaticsStatus`, `EdidCheck`, `PendingCs` and every function name are used with the signatures their producing task defines.
- **Review Focus.** Each of the six lines has its test in the owning task (T3 ×2, T5 ×3, T6).
- **Known gaps, left as they are:** the TruePBR JSON editor window still lists an entry the statics rule drops at write time; a non-PBR set that gets a texture-set block shows the TruePBR panel (its settings are no longer empty).

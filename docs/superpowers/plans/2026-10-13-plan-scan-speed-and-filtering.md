# Scan Speed + Inert-Record Filtering Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the modscan cold scan of the owner's load order faster (target ≤ 20 s, warm rescan no slower and < 3 s if the walk allows), then hide plugin records that nothing reaches ("inert") from Missing, Unused, Plugin overrides and Used by plugins, behind a "Show inert (N)" toggle.

**Architecture:** Speed work is seven independent, measured commits in `crates/vfs` (BSA positional reads) and `crates/modscan` (writer thread, cold-scan index deferral, in-memory interning, pragmas, plugin-pass overlap, derived direct write), each guarded by a table-dump test that pins today's results. Filtering adds a placed-reference walk and three subrecord reads to `crates/esp`, schema 5 tables (`placed`, `rec_inert`, `rec.hdpt_flags`), a reachability pass in SQL inside `resolve_plugins`, and GUI toggles that dim inert rows with their reason.

**Tech Stack:** Rust workspace, rusqlite 0.40.2 (bundled SQLite), rayon, std threads/mpsc, egui + egui_kittest.

**Spec:** `F:\ai\dds\nifparseref\plan\esp\2026-10-13-scan-speed-and-filtering-design.md` (approved 2026-10-13, binding). Speed brief: `F:\ai\dds\modscan-scan-speed-prompt.md`. Executors read both.

**Worktree (once, before Task 1):**
```bash
git -C /f/ai/dds/converter-nif-p1 worktree add /f/ai/dds/converter-nif-speed -b nif/i-speed 9cb9954
cd /f/ai/dds/converter-nif-speed && git status --short   # expected: empty
```
All paths below are relative to `F:\ai\dds\converter-nif-speed`. Every commit ends with the session's attribution trailer. Commit explicit paths only; never push.

## Global Constraints

Copied verbatim from spec §5:

> Same as before: the work goes on a new nif/ branch in its own worktree from nif/main; never push, never merge into main, and merge into nif/main only with the owner's OK. Shared files are add-only, in their own commits. Off-limits: crates/engine, cli, encoders-sys, gui/src/pgp, sets_doc.rs, model/sets.rs, model/pbr_export.rs, schema/, docs/DECISIONS.md and the golden/gui_repro tests. crates/vfs and crates/modscan are editable. Owner data is read-only. Tests read owner data only when `#[ignore]` and `SDC_ACHERUS` are both set, and never the real scan.db. The schema goes from 4 to 5 for the base-reference table, so the store rebuilds once.

Also binding (spec §2 / brief): results must not change (Task 1 dump test stays green through Task 8); `cargo test -p modscan` and the GUI store tests stay green; no new crate, no new database engine; `Cargo.lock` does not change (`git diff --exit-code 9cb9954 -- Cargo.lock`); keep error handling and cancellation intact, in particular `discard_pending`; one speed change per commit with before/after timings.

## Review Focus

1. **A store error on the writer thread mid-scan** (disk full, constraint) must end the scan with that error, not hang the parser blocked on a full channel. Test: Task 3 `a_writer_failure_ends_the_scan_with_its_error`.
2. **Another connection interns a path while a scan holds its in-memory map** (the GUI opens its own `Store` in `activate`) must still get one id per path. Test: Task 5 `path_ids_are_stable_inside_and_outside_a_scan`.
3. **A store file SQLite cannot read** (power loss while `synchronous = OFF`, a stray file) must be rebuilt, since the store is a cache, not refuse to open. Test: Task 6 `a_corrupt_store_file_is_rebuilt`.
4. **Malformed or absurdly nested CELL/WRLD groups** must report an error and go on with the next group: no panic, no stack overflow, records outside that group are kept. Tests: Task 10 `a_bad_nested_cell_group_is_reported_and_later_groups_are_read`, `groups_nested_too_deep_are_an_error_not_a_stack_overflow`.
5. **An override that changes reachability, and a placement whose master is not loaded,** must use the winning copy and must not count. Tests: Task 12 `reachability_follows_the_winning_copy`, `only_proven_unreached_records_are_inert` (Ghost.esp).

## Rulings (where the spec needed interpreting)

- **R1 Phases with the writer thread.** `phase_ms` "parse" = wall time of the overlapped parse+write loop; "write" = waiting for the writer to drain after the last chunk plus `commit_pending`. The sum stays ≈ wall time (the GUI's "in X s" sums `phase_ms`). Writer busy time is the `sub_ms` entry "write_chunk".
- **R2 Sub-timers.** The spec calls them temporary. They go in `Report.sub_ms` and `PluginStats.sub_ms` (not shown in the GUI) and stay on the branch so every task can be measured. Removing them is one follow-up commit if the owner wants it.
- **R3 Cold scan.** "Cold" = `nif_copy` is empty when `begin_scan` runs. A store rebuilt by the GUI's own `Store::open` is still cold for the worker's connection. `Store::end_scan` runs after **every** `run_with` outcome (ok, error, cancel) and always runs `CREATE INDEX IF NOT EXISTS`, so a cancelled cold scan is finished. A crashed one is finished by the next run.
- **R4 `synchronous = OFF` and crashes.** `user_version` keeps being set at schema creation; no sentinel. A sentinel would make a concurrent read-write `Store::open` drop the running scan's tables, and readers would show "scan again" for the whole cold scan. Verified claims instead: (a) a process crash during a cold scan loses nothing committed and the next scan finishes it with identical tables (Task 6 dump test); (b) a power loss can corrupt the file, so `Store::open` deletes `scan.db`, `-wal` and `-shm` and rebuilds on `SQLITE_CORRUPT`/`SQLITE_NOTADB` from its opening statements. Deep corruption that only shows in later queries is not caught; the GUI shows the error.
- **R5 BSA handle.** One `File` per archive, positional reads (spec §2.2). Rust opens Windows files as synchronous handles, and NT serializes I/O on one file object, so parallel reads of one archive may queue. Task 2 keeps the change only if the timing improves; otherwise it is reverted and reported (no improvised handle pool).
- **R6 Interning.** The map lives on `Store` for one scan (`begin_scan` loads it, `end_scan` drops it). A miss falls back to SQL (`INSERT OR IGNORE`; the new id comes from `last_insert_rowid` only when the insert changed 1 row, else `SELECT`). This is correct with a second connection, and a failed transaction cannot leave stale ids past the run.
- **R7 HNAM and GNAM edges** (HDPT → extra-part HDPT, LTEX → GRAS) are stored in `txst_ref` with roles `HNAM`/`GNAM`. `resolve_plugins` already remaps `txst_formid`. Their targets are not TXSTs, which the schema comment says, and the TXST reachability query excludes those roles.
- **R8 ARMA is never inert.** Spec §3 lists ARMO/ARMA among items. No ARMO → ARMA edge is read. *Finding, out of scope:* xEdit defines ARMO `MODL` as an ARMA FormID array (`wbDefinitionsTES5.pas` dev-4.1.6 l.4084), but `records.rs::model()` reads it as a path, and `every_model_record_type_reads_modl_and_mods` pins that behaviour. The bogus `meshes/…` rows never match `%.nif` in Missing.
- **R9 HDPT.** `DATA` is missing → `hdpt_flags` NULL → treated as Playable (conservative). A non-playable head part is reached only when the **winning copy** of a **playable** HDPT names it in HNAM. This is non-recursive: a playable parent is reached by definition. HDPT TNAM reaches a TXST only when that HDPT is not inert.
- **R10 Placements.** Every REFR/ACHR copy in any active plugin counts, including references that are overridden, disabled or deleted later (conservative). A base whose master is not loaded (`base_formid` NULL) does not count: it names another plugin's form. PGRE/PMIS/PHZD and the other placed-projectile types are not read: their bases are not in the placeable list.
- **R11 IPCT** joins the wanted groups. Its `MODL` is recorded in `model_ref` like every other model record (xEdit `wbGenericModel`); its DNAM/ENAM are TXST references.
- **R12 What filtering shows.** *Missing*: a path is listed normally when at least one live record names it, and then only live records are listed. Paths that only inert records name go to the inert list, each label ending in ` (reason)`. *Unused*: there is no toggle; a texture that only inert records name is plain Unused. *Plugin overrides*, *Used by plugins* and *Missing* get a "Show inert (N)" checkbox, shown only when N > 0. Inert rows are drawn `weak` (dimmed), with ` · reason` (overrides, used-by) or ` (reason)` (missing). Counts in headers ("Plugin overrides (N)", "Missing (N)") are live-only.
- **R13 Reason strings** (verbatim): code 1 `no reference`, 2 `not placed`, 3 `head part not playable`. GRAS that no LTEX lists → `no reference`.
- **R14 Shared files** = files outside the crates this branch owns, plus test helper modules used by more than one test file: `crates/modscan/tests/common/mod.rs`, `crates/esp/tests/common/mod.rs`, root `Cargo.toml`/`Cargo.lock`. Only `crates/modscan/tests/common/mod.rs` is touched (Task 1, add-only, own commit). GUI view/model files are owned by the nif/ line and are edited in place, kept local (see Task 14 about nif/h-refs-window).
- **R15 Dump test.** A golden file is written once on the baseline code (`SDC_BLESS=1`) and committed in Task 1. The dump uses explicit schema-4 column lists, so the schema-5 additions (Tasks 9–13) do not change it. Ids are replaced by natural keys and mtimes by "present".
- **R16** The esp test `unwanted_groups_are_skipped_without_reading_them` puts garbage in a `CELL` group. CELL is now walked, so the test's unwanted label becomes `QUST` (Task 10).
- **R17 Derived double copy (Task 8).** It applies only to cold runs: derived copies go straight into the live tables, because no previous rows exist to protect. A tainted or cancelled cold run then deletes every `is_derived = 1` copy. Warm runs keep the staging.
- **R18 Checkpoint.** The code has no explicit WAL checkpoint (SQLite auto-checkpoints inside commits). The owner timing test reports the WAL size and the time of one explicit `wal_checkpoint(TRUNCATE)` after the run.

## Layout sources (pinned)

UESP (`en.uesp.net/wiki/Skyrim_Mod:Mod_File_Format/…`) returned HTTP 403 on 2026-10-13 and could not be checked. Layouts come from xEdit dev-4.1.6, fetched from `raw.githubusercontent.com/TES5Edit/TES5Edit/dev-4.1.6/Core/`:
- `wbDefinitionsTES5.pas` l.6238-6276 **IPCT**: EDID, Model, DATA struct, DODT, `DNAM` FormID [TXST] "Texture Set", `ENAM` FormID [TXST] "Secondary Texture Set", SNAM/NAM1 sounds, NAM2 hazard.
- l.5279-5309 **HDPT**: `DATA` itU8 flags bit0 Playable (0x01), bit1 Male, bit2 Female, bit3 Is Extra Part, bit4 Use Solid Tint; `PNAM` type; `HNAM` repeated FormID [HDPT] "Extra Parts"; TNAM TXST (existing read).
- l.8002/8009 **LTEX**: `TNAM` [TXST]; `GNAM` repeated FormID [GRAS] "Grasses".
- l.9623 **REFR** `NAME` FormID "Base"; l.3119 **ACHR** `NAME` FormID [NPC_] "Base".
- l.4084 **ARMO** `MODL` = FormID array [ARMA] (R8 finding).
- `wbImplementation.pas` l.18451-18487 `TwbGroupRecord.GetName`: group type 0 Top (label = signature), 1 World Children, 2 Interior Cell Block, 3 Interior Cell Sub-Block, 4 Exterior Cell Block, 5 Exterior Cell Sub-Block, 6 Cell Children, 7 Topic Children, 8 Cell Persistent Children, 9 Cell Temporary Children, 10 Cell Visible Distant / Quest Children. The 24-byte GRUP header (`GRUP`, u32 size including header, 4-byte label, u32 type, 8 bytes stamp/version) matches `crates/esp/src/read.rs`.

Conservative choices where a source could not be checked: the walk recurses into **any** nested group type and reads only REFR/ACHR, so a type the table misnames cannot hide a placement. Depth is capped at 8: valid files nest at most 5 levels under a top group.

## Timing protocol and skip rule (Tasks 2–8, 10)

Run, with no other heavy disk work:
```bash
SDC_ACHERUS='C:\_mount\moddingvol\Acherus' cargo test -p modscan --release --test scan owner_timing -- --ignored --nocapture
```
Run it **twice**. Record the **second** run (the OS file cache is warm, so only the store is cold) in the Verification table: cold elapsed and warm elapsed, cold `phase_ms` and `sub_ms`, warm `phase_ms`.

**Skip rule.** Before a speed task, read the row of the last kept task. If the cost that task targets is < 0.5 s on cold **and** < 0.2 s on warm, skip it: add a row `skipped: <timer> = X ms` and make no commit. The targeted timers are: Task 2 `read_bsa_cpu`; Task 3 `write_chunk`; Task 4 `commit_pending` plus the `write` phase; Task 5 `intern` plus `write_chunk`; Task 6 the `write` phase; Task 7 `plugin_write` (`plugin_read` for the overlap); Task 8 `commit_pending` and the derived count.

Phase semantics to read the rows: `parse` includes the `intern` time (path interning + reading existing copies happen before the first parse lap, as at baseline); read Task 5's effect from `intern` and `write_chunk`, not from `parse`. A row from a run that printed `tainted=true` is not comparable: rerun.

**Formatting:** run `cargo fmt -p <crate>` for every crate a task touched before its commit, so Task 16's `--check` passes without a format-only commit.

**Shared infrastructure survives a skip or revert.** Task 4's `cold` flag, `Store::end_scan` and the `run_with`/`scan_body` wrapper are needed by Tasks 5, 6 and 8. If Task 4's index deferral is skipped or reverted, still commit them with only the two `DROP INDEX` lines left out (message `refactor(modscan): end_scan after every run`). Task 7(a) needs Task 5's `intern_path`/`Store.intern`; if Task 5 was reverted, skip 7(a). Struct literals in later tasks (`Store { conn, cold, intern }`) list only the fields that exist.

**Keep rule.** After a task, keep its commit only if cold or warm elapsed improves by ≥ 0.3 s, or its targeted timer by ≥ 20 %, with no regression > 0.3 s on the other. Otherwise run `git revert --no-edit HEAD`, add a row `reverted: <numbers>` and go on. Do not improvise an alternative; report it.

---

### Task 1: Measurement and the results-unchanged test

**Files:**
- Modify: `crates/modscan/src/scan.rs` (Report field, sub-timers)
- Modify: `crates/modscan/src/plugins.rs` (PluginStats field, sub-timers)
- Modify: `crates/gui/tests/meshes_view.rs:947-959` (`stats()` literal gains `sub_ms`)
- Modify (shared, add-only, own commit): `crates/modscan/tests/common/mod.rs`
- Create: `crates/modscan/tests/scan_dump.rs`, `crates/modscan/tests/fixtures/scan_dump.txt` (blessed)
- Test: `crates/modscan/tests/scan.rs` (append `owner_timing`)

**Interfaces:**
- Produces: `modscan::scan::Report.sub_ms: Vec<(&'static str, u64)>`; `modscan::plugins::PluginStats.sub_ms: Vec<(&'static str, u64)>`; `fn ns(t: Instant) -> u64` (private, scan.rs); test helpers `common::bsa(entries: &[(&str, &str, &[u8])]) -> Vec<u8>` and, in `scan_dump.rs`, `fn fixture() -> tempfile::TempDir`, `fn scan_and_dump(d: &tempfile::TempDir) -> String`, `fn golden() -> String`.

- [ ] **Step 1: Add the uncompressed BSA builder (shared helper, add-only)**

Append to `crates/modscan/tests/common/mod.rs`:
```rust
/// An uncompressed v105 BSA (archive flags 0x1|0x2) of (folder, file, bytes) entries; the entries of one folder must be
/// adjacent. Uncompressed so modscan needs no lz4 dev-dependency (Cargo.lock unchanged).
#[allow(dead_code)]
pub fn bsa(entries: &[(&str, &str, &[u8])]) -> Vec<u8> {
    let mut folders: Vec<&str> = entries.iter().map(|e| e.0).collect();
    folders.dedup();
    let total_folder_len: usize = folders.iter().map(|f| f.len() + 1).sum();
    let total_file_len: usize = entries.iter().map(|e| e.1.len() + 1).sum();
    let folder_recs = 24 * folders.len();
    let file_rec_block: usize =
        folders.iter().map(|f| f.len() + 2).sum::<usize>() + 16 * entries.len();
    let mut data_off = 36 + folder_recs + file_rec_block + total_file_len;
    let mut out = b"BSA\0".to_vec();
    for v in [
        105u32,
        36,
        0x3,
        folders.len() as u32,
        entries.len() as u32,
        total_folder_len as u32,
        total_file_len as u32,
    ] {
        out.extend(v.to_le_bytes());
    }
    out.extend(0u32.to_le_bytes()); // file flags + padding
    let mut recs_off = 36 + folder_recs;
    for f in &folders {
        let count = entries.iter().filter(|e| e.0 == *f).count();
        out.extend(0u64.to_le_bytes());
        out.extend((count as u32).to_le_bytes());
        out.extend(0u32.to_le_bytes());
        out.extend(((recs_off + total_file_len) as u64).to_le_bytes());
        recs_off += f.len() + 2 + 16 * count;
    }
    for f in &folders {
        out.push((f.len() + 1) as u8);
        out.extend(f.as_bytes());
        out.push(0);
        for (_, _, bytes) in entries.iter().filter(|e| e.0 == *f) {
            out.extend(0u64.to_le_bytes());
            out.extend((bytes.len() as u32).to_le_bytes());
            out.extend((data_off as u32).to_le_bytes());
            data_off += bytes.len();
        }
    }
    for e in entries {
        out.extend(e.1.as_bytes());
        out.push(0);
    }
    for e in entries {
        out.extend(e.2);
    }
    out
}
```
Run: `cargo test -p modscan --no-run` → Expected: builds.
Commit: `git add crates/modscan/tests/common/mod.rs && git commit -m "test(modscan): uncompressed BSA builder for scan fixtures"`

- [ ] **Step 2: Write the dump test (it fails: no golden yet)**

Create `crates/modscan/tests/scan_dump.rs`:
```rust
//! Spec §2 "Results must not change": a fixture modlist scanned cold, then warm, dumps every schema-4 table (ids
//! replaced by natural keys, mtimes by presence) identical to `fixtures/scan_dump.txt`, written once on the baseline
//! code (nif/main 9cb9954) with `SDC_BLESS=1`.
mod common;
#[path = "../../esp/tests/common/mod.rs"]
mod b;
use b::{alt, edid, form, plugin, rec, sub, top, zstr};
use common::{bsa, instance_fixture, put};
use modscan::mo2::Instance;
use modscan::scan::{run, Source};
use modscan::store::Store;
use rusqlite::types::ValueRef;
use std::sync::atomic::AtomicBool;

const GOLDEN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/scan_dump.txt");

fn nif() -> Vec<u8> {
    std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../nif/tests/fixtures/TestNifFile_Skinned_SE.nif"
    ))
    .unwrap()
}

/// Loose NIFs in two mods (one path in both), a parse failure, a PGPatcher mod deriving one path, loose and BSA
/// textures, an active plugin-named BSA, and a plugin with a TXST, an alternate texture and a landscape texture.
fn fixture() -> tempfile::TempDir {
    let d = instance_fixture("+PGP\n+B\n+A\n");
    let r = d.path();
    let n = nif();
    put(r, "mods/A/meshes/x.nif", &n);
    put(r, "mods/A/meshes/a_only.nif", &n);
    put(r, "mods/A/meshes/bad.nif", b"not a nif");
    put(r, "mods/B/meshes/x.nif", &n);
    put(r, "mods/PGP/meshes/x.nif", &n);
    put(r, "mods/PGP/PGPatcher.esp", b"e");
    let crc = crc32fast::hash(&n);
    put(
        r,
        "mods/PGP/ParallaxGen_Diff.json",
        format!(r#"{{"meshes\\x.nif":{{"crc32original":{crc},"crc32patched":1}}}}"#).as_bytes(),
    );
    put(r, "mods/A/textures/a/a_d.dds", b"DDS ");
    put(r, "mods/B/textures/a/a_d.dds", b"DDS ");
    put(
        r,
        "mods/A/Foo.bsa",
        &bsa(&[
            ("meshes\\bsa", "inbsa.nif", &n),
            ("textures\\bsa", "b_d.dds", b"DDS "),
        ]),
    );
    put(
        r,
        "mods/A/Foo.esp",
        &plugin(
            0,
            &[],
            &[
                top(b"TXST", &[rec(b"TXST", 0x800, &[edid("ATxst"), sub(b"TX00", &zstr("a\\a_d.dds"))])]),
                top(
                    b"STAT",
                    &[rec(
                        b"STAT",
                        0x801,
                        &[
                            edid("AStat"),
                            sub(b"MODL", &zstr("x.nif")),
                            sub(b"MODS", &alt(&[("Shape0", 0x800, 0)])),
                        ],
                    )],
                ),
                top(b"LTEX", &[rec(b"LTEX", 0x802, &[edid("ALtex"), form(b"TNAM", 0x800)])]),
            ],
        ),
    );
    put(r, "profiles/Default/plugins.txt", b"*Foo.esp\n");
    d
}

/// `texset` ids rendered as `n:slot0,…,slot8` (slot paths, `-` for empty).
fn texset_cte() -> String {
    let slots: Vec<String> = (0..9)
        .map(|i| format!("ifnull((SELECT p FROM path WHERE id = t.t{i}), '-')"))
        .collect();
    format!(
        "WITH ts(id, k) AS (SELECT t.id, t.n || ':' || {} FROM texset t) ",
        slots.join(" || ',' || ")
    )
}

/// Every schema-4 table, ids replaced by natural keys, as sorted `table|col|col…` lines.
fn dump(s: &Store, root: &std::path::Path) -> String {
    let ts = texset_cte();
    let copy = |k: &str| {
        format!(
            "JOIN nif_copy c ON c.id = {k} JOIN path cp ON cp.id = c.path_id JOIN origin co ON co.id = c.origin_id"
        )
    };
    let queries: Vec<(&str, String)> = vec![
        ("path", "SELECT p FROM path".into()),
        ("origin", "SELECT o.kind, o.name, o.root, (SELECT x.root FROM origin x WHERE x.id = o.parent_origin), o.priority, o.enabled, o.derived_by, o.derived_state FROM origin o".into()),
        ("nif_copy", "SELECT p.p, o.root, c.size, c.mtime IS NOT NULL, c.crc32, c.parse_status, c.bsver, c.le_mesh, c.is_derived, sp.p || '@' || so.root, c.derive_check, c.problem FROM nif_copy c JOIN path p ON p.id = c.path_id JOIN origin o ON o.id = c.origin_id LEFT JOIN nif_copy s ON s.id = c.source_copy_id LEFT JOIN path sp ON sp.id = s.path_id LEFT JOIN origin so ON so.id = s.origin_id".into()),
        ("shape", format!("{ts}SELECT cp.p, co.root, s.block, s.name, s.kind, s.shader_kind, s.shader_type, s.slsf1, s.slsf2, ts.k, s.alpha_flags, s.alpha_threshold, s.params, s.skinned, s.vertex_desc, s.num_vertices, s.num_triangles FROM shape s {} LEFT JOIN ts ON ts.id = s.texset_id", copy("s.copy_id"))),
        ("tex_raw", format!("SELECT cp.p, co.root, r.block, r.slot, r.raw, r.flags FROM tex_raw r {}", copy("r.copy_id"))),
        ("texset", format!("{ts}SELECT k FROM ts")),
        ("tex_use", format!("{ts}SELECT p.p, ts.k, u.slot FROM tex_use u JOIN path p ON p.id = u.tex_path_id JOIN ts ON ts.id = u.texset_id")),
        ("nif_winner", format!("SELECT p.p, cp.p, co.root, w.n_copies FROM nif_winner w JOIN path p ON p.id = w.path_id {}", copy("w.copy_id"))),
        ("nif_winner_src", format!("SELECT p.p, cp.p, co.root FROM nif_winner_src w JOIN path p ON p.id = w.path_id {}", copy("w.copy_id"))),
        ("pgp_diff", "SELECT p.p, d.crc_orig, d.crc_patched FROM pgp_diff d JOIN path p ON p.id = d.path_id".into()),
        ("tex_copy", "SELECT p.p, o.root, t.size, t.mtime IS NOT NULL FROM tex_copy t JOIN path p ON p.id = t.path_id JOIN origin o ON o.id = t.origin_id".into()),
        ("tex_winner", "SELECT p.p, o.root, w.n_copies FROM tex_winner w JOIN path p ON p.id = w.path_id JOIN origin o ON o.id = w.origin_id".into()),
        ("plugin", "SELECT name, path, size, lo_index, light, flags, masters_json, error_text FROM plugin".into()),
        ("rec", "SELECT pl.name, r.local, r.formid, r.sig, r.edid, r.owner_ok, r.txst_flags, r.decal FROM rec r JOIN plugin pl ON pl.id = r.plugin_id".into()),
        ("rec_winner", "SELECT w.formid, pl.name, w.n_overrides FROM rec_winner w JOIN plugin pl ON pl.id = w.plugin_id".into()),
        ("txst_slot", "SELECT pl.name, t.local, t.formid, t.slot, p.p FROM txst_slot t JOIN plugin pl ON pl.id = t.plugin_id JOIN path p ON p.id = t.path_id".into()),
        ("model_ref", "SELECT pl.name, m.local, m.formid, m.slot, p.p FROM model_ref m JOIN plugin pl ON pl.id = m.plugin_id JOIN path p ON p.id = m.path_id".into()),
        ("alt_tex", "SELECT pl.name, a.local, a.formid, a.model_slot, a.shape_name, a.txst_formid, a.txst_local, a.idx FROM alt_tex a JOIN plugin pl ON pl.id = a.plugin_id".into()),
        ("txst_ref", "SELECT pl.name, t.local, t.formid, t.role, t.txst_formid, t.txst_local FROM txst_ref t JOIN plugin pl ON pl.id = t.plugin_id".into()),
        ("tex_ref", "SELECT pl.name, t.local, t.formid, t.role, p.p FROM tex_ref t JOIN plugin pl ON pl.id = t.plugin_id JOIN path p ON p.id = t.path_id".into()),
    ];
    let roots = [
        root.display().to_string(),
        std::fs::canonicalize(root).unwrap().display().to_string(),
    ];
    let mut lines = vec![];
    for (name, sql) in &queries {
        let mut st = s.conn().prepare(sql).unwrap();
        let n = st.column_count();
        let mut rows = st.query([]).unwrap();
        while let Some(r) = rows.next().unwrap() {
            let cols: Vec<String> = (0..n)
                .map(|i| match r.get_ref(i).unwrap() {
                    ValueRef::Null => "NULL".into(),
                    ValueRef::Integer(v) => v.to_string(),
                    ValueRef::Real(v) => v.to_string(),
                    ValueRef::Text(t) => String::from_utf8_lossy(t).into_owned(),
                    ValueRef::Blob(b) => format!("blob{}", b.len()),
                })
                .collect();
            let mut line = format!("{name}|{}", cols.join("|"));
            for r in &roots {
                line = line.replace(r.as_str(), "<root>");
            }
            lines.push(line.replace('\\', "/"));
        }
    }
    lines.sort();
    lines.join("\n") + "\n"
}

fn scan_and_dump(d: &tempfile::TempDir) -> String {
    let inst = Instance::load(d.path(), None).unwrap();
    let mut store = Store::open(&d.path().join("scan.db")).unwrap();
    run(&Source::Mo2(inst), &mut store, &AtomicBool::new(false), &|_| {}, &|| vec![]).unwrap();
    dump(&store, d.path())
}

fn golden() -> String {
    std::fs::read_to_string(GOLDEN)
        .expect("golden dump missing: run once with SDC_BLESS=1 on the baseline code")
        .replace("\r\n", "\n")
}

#[test]
fn cold_and_warm_scans_match_the_baseline_dump() {
    let d = fixture();
    let cold = scan_and_dump(&d);
    let warm = scan_and_dump(&d);
    if std::env::var_os("SDC_BLESS").is_some() {
        std::fs::create_dir_all(std::path::Path::new(GOLDEN).parent().unwrap()).unwrap();
        std::fs::write(GOLDEN, &cold).unwrap();
    }
    let want = golden();
    assert_eq!(cold, want, "cold dump differs from the baseline");
    assert_eq!(warm, want, "warm dump differs from the baseline");
}
```

- [ ] **Step 3: Run it; expect a failure for the missing golden file**

Run: `cargo test -p modscan --test scan_dump`
Expected: FAIL, panic "golden dump missing: run once with SDC_BLESS=1 on the baseline code".

- [ ] **Step 4: Bless on the baseline code (no source change made yet) and check determinism**

Run: `SDC_BLESS=1 cargo test -p modscan --test scan_dump` → PASS.
Then twice without the flag: `cargo test -p modscan --test scan_dump` → PASS both times (a fresh temp dir each run proves the dump does not depend on the root or on mtimes).
Check by eye that `crates/modscan/tests/fixtures/scan_dump.txt` has `nif_copy|meshes/bsa/inbsa.nif|<root>/mods/A/Foo.bsa|…`, a derived `meshes/x.nif` row from `mods/PGP`, a `bad.nif` row with parse_status 3, and `rec_winner` lines for 0x800-0x802 (2048-2050). If any is missing, fix the fixture (not the code) and re-bless.

- [ ] **Step 5: Add the sub-timers**

`crates/modscan/src/scan.rs`:
- imports: `use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};`
- under `fn lap`, add:
```rust
/// Nanoseconds since `t` (sub-timers summed across threads).
fn ns(t: Instant) -> u64 {
    t.elapsed().as_nanos() as u64
}
```
- `Report`: after `phase_ms` add:
```rust
    /// Sub-timers in milliseconds (spec §2 step 1; ruling R2). `*_cpu` entries are summed over the parse threads.
    pub sub_ms: Vec<(&'static str, u64)>,
```
- Step 4 "Parse or reuse": wrap the two interning reads:
```rust
    let ti = Instant::now();
    let path_ids = store.path_ids(&copies.iter().map(|c| c.path.as_str()).collect::<Vec<_>>())?;
    let existing = store.existing_copies()?;
    let intern_ms = ti.elapsed().as_millis() as u64;
```
- Replace the parse loop (`let total = work.len();` through the `progress(Progress { phase: "parse", done: total, total })` after it) with:
```rust
    let total = work.len();
    let (read_loose_ns, read_bsa_ns, nif_parse_ns) =
        (AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0));
    let mut write_chunk_ns = 0u64;
    for (n, chunk) in work.chunks(CHUNK).enumerate() {
        if cancel.load(Ordering::Relaxed) {
            store.discard_pending()?;
            return Err(ScanError::Cancelled);
        }
        progress(Progress {
            phase: "parse",
            done: n * CHUNK,
            total,
        });
        let results: Vec<(usize, CopyResult, Option<u32>)> = chunk
            .par_iter()
            .map(|&i| {
                let t0 = Instant::now();
                let (bytes, read_ns) = match &locs[i] {
                    Loc::Loose(p) => (std::fs::read(p).map_err(|e| e.to_string()), &read_loose_ns),
                    Loc::Bsa(b, e) => {
                        let bsa = &opened[*b];
                        (
                            bsa.read(&bsa.entries[*e]).map_err(|e| e.to_string()),
                            &read_bsa_ns,
                        )
                    }
                };
                read_ns.fetch_add(ns(t0), Ordering::Relaxed);
                match bytes {
                    Ok(b) => {
                        let t1 = Instant::now();
                        let crc = candidate[i].then(|| crc32fast::hash(&b));
                        let r = scan_guarded(&b);
                        nif_parse_ns.fetch_add(ns(t1), Ordering::Relaxed);
                        (i, r, crc)
                    }
                    Err(e) => (i, CopyResult::ReadFailed(e), None),
                }
            })
            .collect();
        parse_ms += lap(&mut t);
        let mut items = Vec::with_capacity(results.len());
        for (i, res, crc) in results {
            let c = &copies[i];
            report.parsed += 1;
            match &res {
                CopyResult::Scanned(s) if s.le_mesh => report.le_meshes += 1,
                CopyResult::NotSkyrim { .. } => report.not_skyrim += 1,
                CopyResult::Failed(m) | CopyResult::ReadFailed(m) => report
                    .problems
                    .push((format!("{} ({})", c.path, origin_name(c.origin)), m.clone())),
                _ => {}
            }
            if let Some(crc) = crc {
                crc_known.insert(i, crc);
            }
            items.push(ChunkItem {
                path_id: path_ids[i],
                origin_id: origin_id(c.origin),
                size: c.size,
                mtime: c.mtime,
                crc32: crc,
                derived: derived[i],
                result: res,
            });
        }
        let tw = Instant::now();
        store.write_chunk(&items)?;
        write_chunk_ns += ns(tw);
        write_ms += lap(&mut t);
    }
    progress(Progress {
        phase: "parse",
        done: total,
        total,
    });
```
- Replace `store.commit_pending()?;` with
```rust
    let tc = Instant::now();
    store.commit_pending()?;
    let commit_ms = tc.elapsed().as_millis() as u64;
```
and after `report.phase_ms.push(("write", write_ms));` add:
```rust
    report.sub_ms.extend([
        ("intern", intern_ms),
        ("read_loose_cpu", read_loose_ns.into_inner() / 1_000_000),
        ("read_bsa_cpu", read_bsa_ns.into_inner() / 1_000_000),
        ("nif_parse_cpu", nif_parse_ns.into_inner() / 1_000_000),
        ("write_chunk", write_chunk_ns / 1_000_000),
        ("commit_pending", commit_ms),
    ]);
```
- Step 6: put `let tr = Instant::now();` before `let ids_by_key = store.copy_ids()?;` and `report.sub_ms.push(("winners", tr.elapsed().as_millis() as u64));` right after `store.set_winners(&game_rows, &src_rows)?;`. Put `let tdl = Instant::now();` before `let seen: HashSet<i64> = …` and `report.sub_ms.push(("delete_unseen", tdl.elapsed().as_millis() as u64));` after `store.delete_unseen(&seen)?;`.
- Step 8, the `Ok(p)` arm: add `report.sub_ms.extend(p.sub_ms.iter().copied());` before `report.plugins = Some(p);`.

`crates/modscan/src/plugins.rs`:
- `PluginStats`: add after `problems`:
```rust
    /// Sub-timers in milliseconds (scan speed spec §2 step 1): plugin_read, plugin_write, plugin_resolve.
    pub sub_ms: Vec<(&'static str, u64)>,
```
- In `scan_plugins`, before `for (k, chunk) in work.chunks(CHUNK).enumerate() {` add `let (mut read_ns, mut write_ns) = (0u64, 0u64);`. Wrap the `let read: Vec<…> = chunk.par_iter()…collect();` as `let tr = Instant::now();` … `read_ns += tr.elapsed().as_nanos() as u64;`. Wrap the `for (i, r) in read { … }` write loop as `let tw = Instant::now();` … `write_ns += tw.elapsed().as_nanos() as u64;`.
- Before `let names: Vec<String> = active.iter()…` add `let tres = Instant::now();`. Before `st.records = store.plugin_record_count()?;` add:
```rust
    st.sub_ms = vec![
        ("plugin_read", read_ns / 1_000_000),
        ("plugin_write", write_ns / 1_000_000),
        ("plugin_resolve", tres.elapsed().as_millis() as u64),
    ];
```

`crates/gui/tests/meshes_view.rs` `fn stats(…)`: add `sub_ms: vec![],` after the `problems` field of the `PluginStats` literal.

- [ ] **Step 6: Add the owner timing test**

Append to `crates/modscan/tests/scan.rs`:
```rust
/// Scan speed spec §2 step 1: cold then warm scan of the owner instance, with phase and sub-timers. Read-only on the
/// instance; the store is a temp file, never the real scan.db.
/// `SDC_ACHERUS='C:\_mount\moddingvol\Acherus' cargo test -p modscan --release --test scan owner_timing -- --ignored --nocapture`
#[test]
#[ignore]
fn owner_timing() {
    let root = std::path::PathBuf::from(
        std::env::var("SDC_ACHERUS").expect("set SDC_ACHERUS to the MO2 instance root"),
    );
    let db = tempfile::tempdir().unwrap();
    let path = db.path().join("scan.db");
    let mut store = Store::open(&path).unwrap();
    #[cfg(windows)]
    let procs = modscan::win::process_names;
    #[cfg(not(windows))]
    let procs = || -> Vec<String> { vec![] };
    for pass in ["cold", "warm"] {
        let inst = Instance::load(&root, None).unwrap();
        let t = std::time::Instant::now();
        let r = run(
            &Source::Mo2(inst),
            &mut store,
            &AtomicBool::new(false),
            &|_| {},
            &procs,
        )
        .unwrap();
        // A tainted run skipped resolve: its numbers are not comparable; rerun when PGPatcher is closed.
        println!(
            "{pass}: nifs={} parsed={} reused={} tainted={} elapsed {:.1} s",
            r.nifs,
            r.parsed,
            r.reused,
            r.tainted,
            t.elapsed().as_secs_f64()
        );
        println!("{pass}: phase_ms {:?}", r.phase_ms);
        println!("{pass}: sub_ms {:?}", r.sub_ms);
    }
    let q = |sql: &str| -> i64 { store.conn().query_row(sql, [], |r| r.get(0)).unwrap() };
    println!(
        "derived copies {}, BSA copies {}, shapes {}, paths {}, texsets {}",
        q("SELECT count(*) FROM nif_copy WHERE is_derived = 1"),
        q("SELECT count(*) FROM nif_copy c JOIN origin o ON o.id = c.origin_id WHERE o.kind = 4"),
        q("SELECT count(*) FROM shape"),
        q("SELECT count(*) FROM path"),
        q("SELECT count(*) FROM texset")
    );
    let wal = std::fs::metadata(format!("{}-wal", path.display())).map_or(0, |m| m.len());
    let t = std::time::Instant::now();
    store
        .conn()
        .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))
        .unwrap();
    println!(
        "wal before checkpoint {} MB; checkpoint {} ms",
        wal >> 20,
        t.elapsed().as_millis()
    );
}
```

- [ ] **Step 7: Run everything, then measure the baseline**

Run: `cargo test -p modscan` → Expected: PASS (including `report_has_phase_timings` and `scan_dump`).
Run: `cargo test -p sdc-gui --test meshes_view` → PASS.
Run the timing protocol, and fill the Verification row "Task 1 baseline" with cold/warm elapsed, `phase_ms`, `sub_ms` and the counts line. Note whether the owner's ~20 s figure (brief) matches a cold or a warm run.

- [ ] **Step 8: Commit**
```bash
git add crates/modscan/src/scan.rs crates/modscan/src/plugins.rs crates/gui/tests/meshes_view.rs \
  crates/modscan/tests/scan.rs crates/modscan/tests/scan_dump.rs crates/modscan/tests/fixtures/scan_dump.txt
git commit -m "perf(modscan): scan sub-timers, owner timing test, results-unchanged dump test"
```
(The plan file lives outside the repo, in `F:\ai\dds\nifparseref\plan\esp\`: save the Verification-table edits there, with no commit.)

---

### Task 2: BSA positional reads

**Files:**
- Modify: `crates/vfs/src/bsa.rs:24-29, 41-44, 120-139`
- Test: `crates/vfs/tests/bsa.rs` (append)

**Interfaces:**
- Consumes: nothing new. Produces: `Bsa` keeps the same public fields and the same `read(&self, &BsaEntry)` signature (used by `modscan::scan` and `gui::model::viewport3d`). Two private fields are added, so `Bsa` stays `Send + Sync`.

- [ ] **Step 1: Write the failing test**

Append to `crates/vfs/tests/bsa.rs`:
```rust
/// Scan speed spec §2 step 2: entries are read through the handle `Bsa::open` keeps (one per archive, positional
/// reads), never by reopening the path; reads from many threads at once stay correct.
#[test]
fn entries_are_read_through_the_open_handle() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("a.bsa");
    std::fs::write(
        &p,
        build(
            &[
                ("meshes\\a", "x.nif", b"NIFDATA-1", true),
                ("meshes\\a", "y.nif", b"plain", false),
            ],
            false,
        ),
    )
    .unwrap();
    let bsa = Bsa::open(&p).unwrap();
    std::fs::rename(&p, dir.path().join("moved.bsa")).unwrap();
    std::thread::scope(|s| {
        for _ in 0..8 {
            s.spawn(|| {
                for _ in 0..50 {
                    assert_eq!(bsa.read(&bsa.entries[0]).unwrap(), b"NIFDATA-1");
                    assert_eq!(bsa.read(&bsa.entries[1]).unwrap(), b"plain");
                }
            });
        }
    });
}
```
- [ ] **Step 2: Run it.** `cargo test -p vfs --test bsa entries_are_read_through_the_open_handle` → Expected: FAIL (the read after the rename reopens the old path: "cannot find the file" / NotFound).

- [ ] **Step 3: Implement.** In `crates/vfs/src/bsa.rs`:
- imports: `use std::io::{Read, Seek};` (`SeekFrom` is no longer used).
- `Bsa` gains two private fields after `entries`:
```rust
    /// Kept open for positional reads (spec §2 step 2); its length is checked once, at open.
    file: std::fs::File,
    total: u64,
```
- `open`: replace `let mut f = std::io::BufReader::new(file);` with `let mut f = std::io::BufReader::new(&file);` (`&File` is `Read + Seek`; the rest of `open` is unchanged). Change the final constructor to:
```rust
        Ok(Bsa {
            path: path.to_path_buf(),
            version,
            flags,
            entries,
            file,
            total: file_total,
        })
```
(If the borrow checker objects that `f` still borrows `file`, add `drop(f);` right after `f.read_exact(&mut names)?;`.)
- `read`: replace its first lines (through `f.read_exact(&mut data)?;`) with:
```rust
    pub fn read(&self, e: &BsaEntry) -> Result<Vec<u8>, VfsError> {
        if e.offset as u64 + e.size as u64 > self.total {
            return Err(VfsError::Corrupt(format!(
                "{}: data past end of file",
                e.norm
            )));
        }
        let mut data = vec![0u8; e.size as usize];
        read_at(&self.file, &mut data, e.offset as u64)?;
```
(the rest of `read` is unchanged).
- Add at the end of the file:
```rust
/// Fills `buf` from `off` without moving a shared cursor, so threads can read one handle at once.
fn read_at(f: &std::fs::File, buf: &mut [u8], off: u64) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::FileExt;
        let (mut buf, mut off) = (buf, off);
        while !buf.is_empty() {
            let n = f.seek_read(buf, off)?;
            if n == 0 {
                return Err(std::io::ErrorKind::UnexpectedEof.into());
            }
            buf = &mut buf[n..];
            off += n as u64;
        }
        Ok(())
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileExt;
        f.read_exact_at(buf, off)
    }
}
```
- [ ] **Step 4: Run.** `cargo test -p vfs` → PASS. `cargo test -p modscan --test scan_dump` → PASS. `cargo test -p modscan` → PASS. `cargo build -p sdc-gui` → builds.
- [ ] **Step 5: Measure** with the timing protocol and fill row "Task 2". Apply the keep rule (R5).
- [ ] **Step 6: Commit** `git add crates/vfs/src/bsa.rs crates/vfs/tests/bsa.rs && git commit -m "perf(vfs): BSA entries read positionally from one handle per archive"`

---

### Task 3: Writer thread (parse chunk N+1 while chunk N is written)

**Files:**
- Modify: `crates/modscan/src/scan.rs` (the parse loop from Task 1)
- Test: `crates/modscan/tests/scan.rs` (append)

**Interfaces:**
- Consumes: `Store::write_chunk(&mut self, &[ChunkItem])`, `Store::discard_pending`, `ns()`, Task 1 sub-timers. Produces: the same `run`/`run_with` signatures; `sub_ms` "write_chunk" now means writer busy time (R1).

- [ ] **Step 1: Write the review-focus test (pins behaviour; passes before and after)**

Append to `crates/modscan/tests/scan.rs`:
```rust
/// Review focus 1: a store error on the writer thread ends the scan with that error instead of hanging the parser.
#[test]
fn a_writer_failure_ends_the_scan_with_its_error() {
    let d = instance_fixture("+A\n");
    put(d.path(), "mods/A/meshes/x.nif", None);
    let inst = Instance::load(d.path(), None).unwrap();
    let mut store = Store::open(&d.path().join("scan.db")).unwrap();
    store
        .conn()
        .execute_batch("CREATE TRIGGER boom BEFORE INSERT ON nif_copy BEGIN SELECT RAISE(ABORT, 'boom'); END")
        .unwrap();
    let r = run(&Source::Mo2(inst), &mut store, &AtomicBool::new(false), &|_| {}, &|| vec![]);
    assert!(
        matches!(&r, Err(modscan::ScanError::Db(e)) if e.to_string().contains("boom")),
        "{r:?}"
    );
}
```
Run: `cargo test -p modscan --test scan a_writer_failure` → PASS (baseline behaviour).

- [ ] **Step 2: Replace the parse loop** (Task 1's version, from `let total = work.len();` through the `progress(… done: total …)` call) with:
```rust
    let total = work.len();
    let (read_loose_ns, read_bsa_ns, nif_parse_ns, write_ns) = (
        AtomicU64::new(0),
        AtomicU64::new(0),
        AtomicU64::new(0),
        AtomicU64::new(0),
    );
    // Spec §2 step 3: a writer thread stores chunk N while chunk N+1 is parsed; the Store stays on that one thread.
    let (tx, rx) = std::sync::mpsc::sync_channel::<Vec<ChunkItem>>(2);
    let writer_store = &mut *store;
    let wns = &write_ns;
    let looped: Result<(), ScanError> = std::thread::scope(|s| {
        let writer = s.spawn(move || -> Result<(), ScanError> {
            for items in rx {
                let tw = Instant::now();
                writer_store.write_chunk(&items)?;
                wns.fetch_add(ns(tw), Ordering::Relaxed);
            }
            Ok(())
        });
        let mut out = Ok(());
        for (n, chunk) in work.chunks(CHUNK).enumerate() {
            if cancel.load(Ordering::Relaxed) {
                out = Err(ScanError::Cancelled);
                break;
            }
            progress(Progress {
                phase: "parse",
                done: n * CHUNK,
                total,
            });
            let results: Vec<(usize, CopyResult, Option<u32>)> = chunk
                .par_iter()
                .map(|&i| {
                    let t0 = Instant::now();
                    let (bytes, read_ns) = match &locs[i] {
                        Loc::Loose(p) => {
                            (std::fs::read(p).map_err(|e| e.to_string()), &read_loose_ns)
                        }
                        Loc::Bsa(b, e) => {
                            let bsa = &opened[*b];
                            (
                                bsa.read(&bsa.entries[*e]).map_err(|e| e.to_string()),
                                &read_bsa_ns,
                            )
                        }
                    };
                    read_ns.fetch_add(ns(t0), Ordering::Relaxed);
                    match bytes {
                        Ok(b) => {
                            let t1 = Instant::now();
                            let crc = candidate[i].then(|| crc32fast::hash(&b));
                            let r = scan_guarded(&b);
                            nif_parse_ns.fetch_add(ns(t1), Ordering::Relaxed);
                            (i, r, crc)
                        }
                        Err(e) => (i, CopyResult::ReadFailed(e), None),
                    }
                })
                .collect();
            let mut items = Vec::with_capacity(results.len());
            for (i, res, crc) in results {
                let c = &copies[i];
                report.parsed += 1;
                match &res {
                    CopyResult::Scanned(s) if s.le_mesh => report.le_meshes += 1,
                    CopyResult::NotSkyrim { .. } => report.not_skyrim += 1,
                    CopyResult::Failed(m) | CopyResult::ReadFailed(m) => report
                        .problems
                        .push((format!("{} ({})", c.path, origin_name(c.origin)), m.clone())),
                    _ => {}
                }
                if let Some(crc) = crc {
                    crc_known.insert(i, crc);
                }
                items.push(ChunkItem {
                    path_id: path_ids[i],
                    origin_id: origin_id(c.origin),
                    size: c.size,
                    mtime: c.mtime,
                    crc32: crc,
                    derived: derived[i],
                    result: res,
                });
            }
            // A closed channel means the writer stopped on an error; `join` returns it.
            if tx.send(items).is_err() {
                break;
            }
        }
        drop(tx);
        parse_ms = lap(&mut t);
        let w = writer
            .join()
            .unwrap_or_else(|p| std::panic::resume_unwind(p));
        w.and(out)
    });
    if let Err(e) = looped {
        if matches!(e, ScanError::Cancelled) {
            store.discard_pending()?;
        }
        return Err(e);
    }
    progress(Progress {
        phase: "parse",
        done: total,
        total,
    });
```
and in the `sub_ms` block replace `("write_chunk", write_chunk_ns / 1_000_000),` with `("write_chunk", write_ns.into_inner() / 1_000_000),`. Delete the now-unused `let mut write_chunk_ns = 0u64;`. `parse_ms`/`write_ms` stay declared as `let (mut parse_ms, mut write_ms) = (0u64, 0u64);`; `write_ms += lap(&mut t);` after `commit_pending` now covers the writer drain plus the commit (R1).

- [ ] **Step 3: Run.** `cargo test -p modscan` → PASS (the dump test, `removed_files_are_deleted_and_cancel_stops`, `pgp_started_mid_scan_taints_result`, `report_has_phase_timings`, `a_writer_failure_ends_the_scan_with_its_error`). `cargo clippy -p modscan --all-targets -- -D warnings` → clean.
- [ ] **Step 4: Measure** and fill row "Task 3"; apply the keep rule.
- [ ] **Step 5: Commit** `git add crates/modscan/src/scan.rs crates/modscan/tests/scan.rs && git commit -m "perf(modscan): overlap NIF parsing with a dedicated store writer thread"`

---

### Task 4: Cold-build deferred indexes (+ `end_scan` on every outcome)

**Files:**
- Modify: `crates/modscan/src/store.rs` (SCHEMA, `Store` struct, `open`, `open_read_only`, `begin_scan`, new `end_scan`)
- Modify: `crates/modscan/src/scan.rs` (`run_with` wrapper)
- Test: `crates/modscan/tests/store.rs`, `crates/modscan/tests/scan.rs` (append)

**Interfaces:**
- Produces: `Store.cold: bool` (private field; read inside store.rs and by Task 8); `pub fn end_scan(&mut self) -> Result<(), ScanError>`; `const COLD_INDEXES: &str`. `run_with` always calls `end_scan` after its body (`fn scan_body` = the former `run_with` body).

- [ ] **Step 1: Write the failing tests**

Append to `crates/modscan/tests/store.rs`:
```rust
fn secondary_indexes(s: &Store) -> i64 {
    s.conn()
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type = 'index' AND name IN ('nif_copy_origin', 'shape_texset')",
            [],
            |r| r.get(0),
        )
        .unwrap()
}

/// Scan speed spec §2 step 4: a scan of an empty store builds its secondary indexes once, at the end.
#[test]
fn begin_scan_on_an_empty_store_defers_the_secondary_indexes() {
    let d = tempfile::tempdir().unwrap();
    let mut s = Store::open(&d.path().join("s.db")).unwrap();
    assert_eq!(secondary_indexes(&s), 2);
    s.begin_scan().unwrap();
    assert_eq!(secondary_indexes(&s), 0);
    s.end_scan().unwrap();
    assert_eq!(secondary_indexes(&s), 2);
    s.upsert_copy("meshes/a.nif", 1, 1, 1, CopyResult::Failed("x".into()), None)
        .unwrap();
    s.begin_scan().unwrap();
    assert_eq!(secondary_indexes(&s), 2, "a warm scan keeps them");
    s.end_scan().unwrap();
}
```
Append to `crates/modscan/tests/scan.rs`:
```rust
/// Ruling R3: a cancelled cold scan still ends with its indexes.
#[test]
fn a_cancelled_cold_scan_still_ends_with_its_indexes() {
    let d = instance_fixture("+A\n");
    put(d.path(), "mods/A/meshes/x.nif", None);
    let inst = Instance::load(d.path(), None).unwrap();
    let mut store = Store::open(&d.path().join("scan.db")).unwrap();
    let r = run(&Source::Mo2(inst), &mut store, &AtomicBool::new(true), &|_| {}, &|| vec![]);
    assert!(matches!(r, Err(modscan::ScanError::Cancelled)), "{r:?}");
    let n: i64 = store
        .conn()
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type = 'index' AND name IN ('nif_copy_origin', 'shape_texset')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 2);
}
```
- [ ] **Step 2: Run.** `cargo test -p modscan --test store begin_scan_on_an_empty_store` → Expected: FAIL to compile (`no method named end_scan`).

- [ ] **Step 3: Implement** in `crates/modscan/src/store.rs`:
- Delete `CREATE INDEX nif_copy_origin ON nif_copy(origin_id);` and `CREATE INDEX shape_texset ON shape(texset_id);` from `SCHEMA` and add after `SCHEMA`:
```rust
/// Secondary NIF indexes no upsert relies on (every UNIQUE stays in `SCHEMA`): dropped when a scan starts on an empty
/// store and built once by `end_scan` (spec §2 step 4).
const COLD_INDEXES: &str = "CREATE INDEX IF NOT EXISTS nif_copy_origin ON nif_copy(origin_id);
CREATE INDEX IF NOT EXISTS shape_texset ON shape(texset_id);";
```
- In `open`, the rebuild branch: after `tx.execute_batch(SCHEMA)?;` add `tx.execute_batch(COLD_INDEXES)?;`.
- `pub struct Store` becomes:
```rust
pub struct Store {
    pub(crate) conn: Connection,
    /// This scan started on an empty store (ruling R3): secondary indexes are deferred to `end_scan`.
    cold: bool,
}
```
and both constructors return `Store { conn, cold: false }`.
- Replace `begin_scan` and add `end_scan`:
```rust
    /// Start of a scan: staging tables created and emptied; every origin disabled until this run re-upserts it.
    /// `origin.enabled = 1` therefore means "part of the latest run" and is the filter of every read query.
    /// Rows of disabled origins stay cached (a disabled mod or a Folder scan in between costs no re-parse).
    /// On an empty store (a cold scan) the secondary indexes are dropped until `end_scan`.
    pub fn begin_scan(&mut self) -> Result<(), ScanError> {
        self.conn.execute_batch(TEMP)?;
        self.conn.execute("UPDATE origin SET enabled = 0", [])?;
        self.cold = !self
            .conn
            .query_row("SELECT EXISTS(SELECT 1 FROM nif_copy)", [], |r| r.get::<_, bool>(0))?;
        if self.cold {
            self.conn
                .execute_batch("DROP INDEX IF EXISTS nif_copy_origin; DROP INDEX IF EXISTS shape_texset;")?;
        }
        Ok(())
    }

    /// End of every run, whatever its outcome (ruling R3): the deferred indexes exist again, also after a scan that
    /// crashed before reaching here.
    pub fn end_scan(&mut self) -> Result<(), ScanError> {
        self.conn.execute_batch(COLD_INDEXES)?;
        self.cold = false;
        Ok(())
    }
```
In `crates/modscan/src/scan.rs`, rename the existing `pub fn run_with(` line to `fn scan_body(` (body and doc comment unchanged) and add above it:
```rust
/// `run`, optionally without the PGPatcher output ("Scan without PGPatcher output", Task 12 / ruling R6). With
/// `skip_derived`, a running PGPatcher does not block or taint the scan, mods carrying the PGPatcher signature are
/// left out of the run (as if disabled), and in other origins every derived path (dups, diff-listed) is skipped.
pub fn run_with(
    src: &Source,
    store: &mut Store,
    cancel: &AtomicBool,
    progress: &dyn Fn(Progress),
    procs: &dyn Fn() -> Vec<String>,
    skip_derived: bool,
) -> Result<Report, ScanError> {
    let r = scan_body(src, store, cancel, progress, procs, skip_derived);
    // Every outcome, cancel and errors included, ends the scan (deferred indexes, ruling R3).
    let end = store.end_scan();
    let report = r?;
    end?;
    Ok(report)
}
```
(The old doc comment on `scan_body` becomes `/// The scan itself; `run_with` adds `Store::end_scan` on every outcome.`)
- [ ] **Step 4: Run.** `cargo test -p modscan` → PASS. `cargo test -p sdc-gui --test meshes_view` → PASS.
- [ ] **Step 5: Measure**, fill row "Task 4", apply the keep rule.
- [ ] **Step 6: Commit** `git add crates/modscan/src/store.rs crates/modscan/src/scan.rs crates/modscan/tests/store.rs crates/modscan/tests/scan.rs && git commit -m "perf(modscan): build secondary NIF indexes after a cold scan's bulk load"`

---

### Task 5: Path/texset interning map

**Files:**
- Modify: `crates/modscan/src/store.rs` (`path_id`, new `Interner` + `intern_path`, `texset_id`, `tex_slots`, `write_shapes`, `upsert_copy_row`, `stage_derived`, `Store` struct/constructors, `begin_scan`, `end_scan`, `path_ids`, `write_chunk`, `upsert_copy`, `replace_shapes`, `set_pgp_diff`)
- Test: `crates/modscan/tests/store.rs` (append)

**Interfaces:**
- Consumes: `begin_scan`/`end_scan` (Task 4). Produces: `pub(crate) struct Interner`; `pub(crate) fn intern_path(c: &Connection, i: &mut Option<Interner>, p: &str) -> rusqlite::Result<i64>`; `Store.intern: Option<Interner>` (pub(crate) field, used by Task 7). Public signatures are unchanged.

- [ ] **Step 1: Write the test (review focus 2; pins behaviour)**

Append to `crates/modscan/tests/store.rs`:
```rust
/// Spec §2 step 5 + review focus 2: one id per path with and without the scan's in-memory map, also for a path
/// another connection added after the map was loaded.
#[test]
fn path_ids_are_stable_inside_and_outside_a_scan() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("s.db");
    let mut s = Store::open(&p).unwrap();
    let a = s.path_ids(&["meshes/a.nif", "meshes/b.nif"]).unwrap();
    s.begin_scan().unwrap();
    let mut other = Store::open(&p).unwrap();
    let x = other.path_ids(&["meshes/x.nif"]).unwrap()[0];
    let b = s
        .path_ids(&["meshes/b.nif", "meshes/a.nif", "meshes/c.nif", "meshes/c.nif", "meshes/x.nif"])
        .unwrap();
    s.end_scan().unwrap();
    assert_eq!((b[0], b[1]), (a[1], a[0]));
    assert_eq!(b[2], b[3]);
    assert_eq!(b[4], x, "a path another connection interned");
    assert_eq!(s.path_ids(&["meshes/c.nif"]).unwrap()[0], b[2]);
}
```
Run: `cargo test -p modscan --test store path_ids_are_stable` → PASS on the current code (pins behaviour).

- [ ] **Step 2: Implement** in `crates/modscan/src/store.rs`:
- Replace `path_id` and add the interner after it:
```rust
pub(crate) fn path_id(c: &Connection, p: &str) -> rusqlite::Result<i64> {
    // `last_insert_rowid` is the new id only when the insert changed a row; otherwise the path already existed.
    if c.prepare_cached("INSERT OR IGNORE INTO path(p) VALUES (?1)")?
        .execute([p])?
        == 1
    {
        return Ok(c.last_insert_rowid());
    }
    c.prepare_cached("SELECT id FROM path WHERE p = ?1")?
        .query_row([p], |r| r.get(0))
}

/// Path and texset ids of the running scan (spec §2 step 5, ruling R6): loaded by `begin_scan`, dropped by `end_scan`.
/// A miss goes to SQL, so ids added by another connection stay correct.
#[derive(Default)]
pub(crate) struct Interner {
    paths: HashMap<String, i64>,
    /// `[n, t0..t8]` with empty slots as 0, as the `texset_key` index folds them.
    texsets: HashMap<[i64; MAX_SLOTS + 1], i64>,
}

pub(crate) fn intern_path(
    c: &Connection,
    i: &mut Option<Interner>,
    p: &str,
) -> rusqlite::Result<i64> {
    if let Some(id) = i.as_ref().and_then(|i| i.paths.get(p)) {
        return Ok(*id);
    }
    let id = path_id(c, p)?;
    if let Some(i) = i {
        i.paths.insert(p.to_string(), id);
    }
    Ok(id)
}
```
- Replace `texset_id` and `tex_slots`:
```rust
fn texset_id(
    c: &Connection,
    i: &mut Option<Interner>,
    slots: &[Option<i64>],
) -> rusqlite::Result<i64> {
    let n = slots.len().min(MAX_SLOTS);
    let mut t = [None::<i64>; MAX_SLOTS];
    t[..n].copy_from_slice(&slots[..n]);
    let mut key = [0i64; MAX_SLOTS + 1];
    key[0] = n as i64;
    for (j, x) in t.iter().enumerate() {
        key[j + 1] = x.unwrap_or(0);
    }
    if let Some(id) = i.as_ref().and_then(|i| i.texsets.get(&key)) {
        return Ok(*id);
    }
    let inserted = c
        .prepare_cached(
            "INSERT OR IGNORE INTO texset(n,t0,t1,t2,t3,t4,t5,t6,t7,t8) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
        )?
        .execute(params![n as i64, t[0], t[1], t[2], t[3], t[4], t[5], t[6], t[7], t[8]])?;
    let id: i64 = if inserted == 1 {
        let id = c.last_insert_rowid();
        // A new set gets its tex_use rows; an existing one got them with its insert (tex_use is never deleted).
        let mut tu = c.prepare_cached(
            "INSERT OR IGNORE INTO tex_use(tex_path_id, texset_id, slot) VALUES (?1,?2,?3)",
        )?;
        for (slot, p) in t[..n].iter().enumerate() {
            if let Some(p) = p {
                tu.execute(params![p, id, slot as i64])?;
            }
        }
        id
    } else {
        let k = &key[1..];
        c.prepare_cached(
            "SELECT id FROM texset WHERE n = ?1 AND ifnull(t0,0) = ?2 AND ifnull(t1,0) = ?3 AND ifnull(t2,0) = ?4 \
             AND ifnull(t3,0) = ?5 AND ifnull(t4,0) = ?6 AND ifnull(t5,0) = ?7 AND ifnull(t6,0) = ?8 \
             AND ifnull(t7,0) = ?9 AND ifnull(t8,0) = ?10",
        )?
        .query_row(
            params![n as i64, k[0], k[1], k[2], k[3], k[4], k[5], k[6], k[7], k[8]],
            |r| r.get(0),
        )?
    };
    if let Some(i) = i {
        i.texsets.insert(key, id);
    }
    Ok(id)
}

fn tex_slots(
    c: &Connection,
    i: &mut Option<Interner>,
    paths: &[&TexPath],
) -> rusqlite::Result<Option<i64>> {
    let mut ids = vec![];
    for p in paths {
        ids.push(if p.flags.empty || p.norm.is_empty() {
            None
        } else {
            Some(intern_path(c, i, &p.norm)?)
        });
    }
    if ids.is_empty() {
        return Ok(None);
    }
    texset_id(c, i, &ids).map(Some)
}
```
- `write_shapes(c: &Connection, i: &mut Option<Interner>, t: Tables, key: i64, scan: &NifScan)`: the two `tex_slots(c, &slots)?` calls become `tex_slots(c, i, &slots)?`.
- `upsert_copy_row(c: &Connection, i: &mut Option<Interner>, it: &ChunkItem)` → `write_shapes(c, i, LIVE, id, s)?`; `stage_derived(c: &Connection, i: &mut Option<Interner>, it: &ChunkItem)` → `write_shapes(c, i, PENDING, pid, s)?`.
- `Store` gains `pub(crate) intern: Option<Interner>,` (doc: `/// The running scan's interning map (ruling R6).`); both constructors add `intern: None`.
- `begin_scan`: before `Ok(())` add:
```rust
        let mut i = Interner::default();
        {
            let mut st = self.conn.prepare("SELECT p, id FROM path")?;
            for r in st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))? {
                let (p, id) = r?;
                i.paths.insert(p, id);
            }
            let mut st = self.conn.prepare(
                "SELECT id, n, ifnull(t0,0), ifnull(t1,0), ifnull(t2,0), ifnull(t3,0), ifnull(t4,0), ifnull(t5,0), \
                 ifnull(t6,0), ifnull(t7,0), ifnull(t8,0) FROM texset",
            )?;
            for r in st.query_map([], |r| {
                let mut k = [0i64; MAX_SLOTS + 1];
                for (j, v) in k.iter_mut().enumerate() {
                    *v = r.get(j + 1)?;
                }
                Ok((k, r.get::<_, i64>(0)?))
            })? {
                let (k, id) = r?;
                i.texsets.insert(k, id);
            }
        }
        self.intern = Some(i);
```
- `end_scan`: before `Ok(())` add `self.intern = None;`.
- `path_ids`: `out.push(intern_path(&tx, &mut self.intern, p)?);`.
- `write_chunk`: `stage_derived(&tx, &mut self.intern, it)?` / `upsert_copy_row(&tx, &mut self.intern, it)?`.
- `upsert_copy`: `path_id: intern_path(&tx, &mut self.intern, path)?,` and `upsert_copy_row(&tx, &mut self.intern, &it)?`.
- `replace_shapes`: `write_shapes(&tx, &mut self.intern, LIVE, copy_id, scan)?`.
- `set_pgp_diff`: `let id = intern_path(&tx, &mut self.intern, p)?;`.
(`self.conn.transaction()` borrows only the `conn` field, so `&mut self.intern` next to `&tx` compiles.)
- [ ] **Step 3: Run.** `cargo test -p modscan` → PASS (dump test included). `cargo clippy -p modscan --all-targets -- -D warnings` → clean.
- [ ] **Step 4: Measure**, fill row "Task 5" (watch the `intern` warm cost: loading the map), apply the keep rule.
- [ ] **Step 5: Commit** `git add crates/modscan/src/store.rs crates/modscan/tests/store.rs && git commit -m "perf(modscan): intern paths and texture sets through an in-memory map per scan"`

---

### Task 6: Pragmas, `synchronous = OFF` on cold scans, corrupt-store rebuild

**Files:**
- Modify: `crates/modscan/src/store.rs` (`open` split into `open` + `open_rw`, `begin_scan`, `end_scan`)
- Test: `crates/modscan/tests/store.rs`, `crates/modscan/tests/scan_dump.rs` (append)

**Interfaces:**
- Consumes: `Store.cold`, `begin_scan`/`end_scan`. Produces: the same `Store::open` signature with the R4 behaviour.

- [ ] **Step 1: Write the tests**

Append to `crates/modscan/tests/store.rs`:
```rust
/// Spec §2 step 6: cache pragmas on every connection; synchronous OFF only while a cold scan runs.
#[test]
fn a_cold_scan_runs_with_synchronous_off_and_ends_normal() {
    let d = tempfile::tempdir().unwrap();
    let mut s = Store::open(&d.path().join("s.db")).unwrap();
    let pragma = |s: &Store, p: &str| -> i64 {
        s.conn().query_row(&format!("PRAGMA {p}"), [], |r| r.get(0)).unwrap()
    };
    assert_eq!(pragma(&s, "cache_size"), -65536);
    assert_eq!(pragma(&s, "temp_store"), 2, "MEMORY");
    assert_eq!(pragma(&s, "synchronous"), 1, "NORMAL");
    s.begin_scan().unwrap();
    assert_eq!(pragma(&s, "synchronous"), 0, "OFF during a cold scan");
    s.end_scan().unwrap();
    assert_eq!(pragma(&s, "synchronous"), 1);
    s.upsert_copy("meshes/a.nif", 1, 1, 1, CopyResult::Failed("x".into()), None)
        .unwrap();
    s.begin_scan().unwrap();
    assert_eq!(pragma(&s, "synchronous"), 1, "a warm scan keeps NORMAL");
    s.end_scan().unwrap();
}

/// Review focus 3 / ruling R4: a file SQLite cannot read is rebuilt (the store is a cache).
#[test]
fn a_corrupt_store_file_is_rebuilt() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("scan.db");
    std::fs::write(&p, vec![0xAB; 8192]).unwrap();
    let s = Store::open(&p).unwrap();
    let v: i64 = s
        .conn()
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(v, modscan::store::SCHEMA_VERSION);
}
```
Append to `crates/modscan/tests/scan_dump.rs`:
```rust
/// Ruling R4: a cold scan that dies part-way (here after its NIF writes; no `end_scan`, connection never closed, as
/// in a killed process) is finished by the next scan with the same tables as a clean one.
#[test]
fn a_scan_that_died_mid_way_is_finished_by_the_next_one() {
    let d = fixture();
    let mut s = Store::open(&d.path().join("scan.db")).unwrap();
    let inst = Instance::load(d.path(), None).unwrap();
    let died = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run(
            &Source::Mo2(inst),
            &mut s,
            &AtomicBool::new(false),
            &|p| {
                if p.phase == "resolve" {
                    panic!("simulated crash")
                }
            },
            &|| vec![],
        )
    }));
    assert!(died.is_err());
    // Deliberate leak: a killed process never closes its connection (on Windows the TempDir then stays behind).
    std::mem::forget(s);
    assert_eq!(scan_and_dump(&d), golden());
}
```
- [ ] **Step 2: Run.** `cargo test -p modscan --test store a_c` → Expected: `a_cold_scan_runs_with_synchronous_off_and_ends_normal` FAILS (cache_size is the default -2000) and `a_corrupt_store_file_is_rebuilt` FAILS ("file is not a database"). `cargo test -p modscan --test scan_dump a_scan_that_died` → PASS (pins the resume claim before the pragma change).

- [ ] **Step 3: Implement** in `crates/modscan/src/store.rs`: replace `pub fn open` with:
```rust
    pub fn open(path: &Path) -> Result<Store, ScanError> {
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir)?;
        }
        match Self::open_rw(path) {
            // Ruling R4: the store is a cache; a file SQLite cannot read (power loss during synchronous = OFF) is rebuilt.
            Err(ScanError::Db(e))
                if matches!(
                    e.sqlite_error_code(),
                    Some(rusqlite::ErrorCode::DatabaseCorrupt | rusqlite::ErrorCode::NotADatabase)
                ) =>
            {
                for ext in ["", "-wal", "-shm"] {
                    match std::fs::remove_file(format!("{}{ext}", path.display())) {
                        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
                        _ => {}
                    }
                }
                Self::open_rw(path)
            }
            r => r,
        }
    }

    fn open_rw(path: &Path) -> Result<Store, ScanError> {
        let conn = Connection::open(path)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        let _: String = conn.query_row("PRAGMA journal_mode = WAL", [], |r| r.get(0))?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        // Spec §2 step 6: temp tables in memory, a 64 MB page cache, 256 MB of memory-mapped reads.
        conn.execute_batch(
            "PRAGMA temp_store = MEMORY; PRAGMA cache_size = -65536; PRAGMA mmap_size = 268435456;",
        )?;
        let v: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if v != SCHEMA_VERSION {
            // The store is a cache: any other version is dropped and rebuilt.
            let tx = conn.unchecked_transaction()?;
            let tables: Vec<String> = {
                let mut st = tx.prepare(
                    "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
                )?;
                let v = st.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
                v
            };
            for t in tables {
                tx.execute_batch(&format!("DROP TABLE IF EXISTS \"{t}\""))?;
            }
            tx.execute_batch(SCHEMA)?;
            tx.execute_batch(COLD_INDEXES)?;
            tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
            tx.commit()?;
        }
        Ok(Store {
            conn,
            cold: false,
            intern: None,
        })
    }
```
In `begin_scan`, inside `if self.cold { … }` add after the DROP INDEX batch:
```rust
            // Ruling R4: a cold store has nothing to lose to a crash; a power loss is caught by `open`.
            self.conn.pragma_update(None, "synchronous", "OFF")?;
```
In `end_scan`, before `self.cold = false;` add `self.conn.pragma_update(None, "synchronous", "NORMAL")?;`.
- [ ] **Step 4: Run.** `cargo test -p modscan` → PASS. `cargo test -p sdc-gui --test meshes_view --test set_plugin_refs` → PASS.
- [ ] **Step 5: Measure**, fill row "Task 6", apply the keep rule.
- [ ] **Step 6: Commit** `git add crates/modscan/src/store.rs crates/modscan/tests/store.rs crates/modscan/tests/scan_dump.rs && git commit -m "perf(modscan): cache pragmas, synchronous OFF during cold scans, rebuild unreadable stores"`

---

### Task 7: Plugin pass — interning map and parse/write overlap

**Files:**
- Modify: `crates/modscan/src/plugin_store.rs` (`write_plugin` uses `intern_path`)
- Modify: `crates/modscan/src/plugins.rs` (`scan_plugins` chunk loop, new `write_found`)

**Interfaces:**
- Consumes: `crate::store::intern_path`, `Store.intern` (Task 5). Produces: the same `scan_plugins` signature and `PluginStats`. `plugin_write` now means writer busy time.

Skip rule specifics: do (a) unless Task 1's cold `plugin_write` < 0.5 s; do (b) unless `plugin_write` < 1 s. With both skipped, record the row and stop.

- [ ] **Step 1: (a) Interning.** In `crates/modscan/src/plugin_store.rs`: change the import to `use crate::store::{intern_path, tex_query, Store, TexKey};`. In `write_plugin`, replace the three `path_id(&tx, p)?` / `path_id(&tx, &m.path)?` calls with `intern_path(&tx, &mut self.intern, p)?` / `intern_path(&tx, &mut self.intern, &m.path)?`. If `path_id` is now unused in this file, the import list above already omits it.
- [ ] **Step 2: (b) Overlap.** In `crates/modscan/src/plugins.rs`: imports gain `use std::sync::atomic::AtomicU64;` and `use crate::plugin_store::PluginFile` (already imported). Add above `scan_plugins`:
```rust
/// Stores one plugin's facts; a failed read is stored with its error and no size, so it is read again next scan.
fn write_found(store: &mut Store, a: &Found, r: Result<PluginFacts, String>) -> Result<(), ScanError> {
    let (facts, size) = match r {
        Ok(f) => (f, Some(a.size)),
        Err(e) => (
            PluginFacts {
                name: a.name.clone(),
                errors: vec![e],
                ..Default::default()
            },
            None,
        ),
    };
    let path = a.path.display().to_string();
    store.write_plugin(
        &PluginFile {
            name: &a.name,
            path: &path,
            size,
            mtime: a.mtime,
        },
        &facts,
    )?;
    Ok(())
}
```
Replace the whole chunk loop (`let (mut read_ns, mut write_ns) = …` through its closing `}`) with:
```rust
    let (read_ns, write_ns) = (AtomicU64::new(0), AtomicU64::new(0));
    // Spec §2 step 7: a writer thread stores chunk k while chunk k+1 is parsed; the Store stays on that thread.
    let (tx, rx) = std::sync::mpsc::sync_channel::<Vec<(usize, Result<PluginFacts, String>)>>(2);
    let (writer_store, found, wns) = (&mut *store, &active, &write_ns);
    std::thread::scope(|s| -> Result<(), ScanError> {
        let writer = s.spawn(move || -> Result<(), ScanError> {
            for chunk in rx {
                let tw = Instant::now();
                for (i, r) in chunk {
                    write_found(writer_store, &found[i], r)?;
                }
                wns.fetch_add(tw.elapsed().as_nanos() as u64, Ordering::Relaxed);
            }
            Ok(())
        });
        let mut out = Ok(());
        for (k, chunk) in work.chunks(CHUNK).enumerate() {
            if cancel.load(Ordering::Relaxed) {
                out = Err(ScanError::Cancelled);
                break;
            }
            progress(Progress {
                phase: "plugins",
                done: k * CHUNK,
                total: work.len(),
            });
            let tr = Instant::now();
            let read: Vec<(usize, Result<PluginFacts, String>)> = chunk
                .par_iter()
                .map(|&i| (i, read_plugin(&active[i], cancel)))
                .collect();
            read_ns.fetch_add(tr.elapsed().as_nanos() as u64, Ordering::Relaxed);
            if cancel.load(Ordering::Relaxed) {
                out = Err(ScanError::Cancelled);
                break;
            }
            // A closed channel means the writer stopped on an error; `join` returns it.
            if tx.send(read).is_err() {
                break;
            }
        }
        drop(tx);
        let w = writer
            .join()
            .unwrap_or_else(|p| std::panic::resume_unwind(p));
        w.and(out)
    })?;
```
and in the `st.sub_ms` block use `read_ns.into_inner() / 1_000_000` and `write_ns.into_inner() / 1_000_000`.
- [ ] **Step 3: Run.** `cargo test -p modscan` → PASS (in particular `cancel_stops_the_plugin_pass`, `cancel_set_during_progress_stops_before_resolution`, `cancel_applies_to_an_all_cached_pass`, `a_failed_resolution_after_a_removal_is_redone`, the dump test). `cargo clippy -p modscan --all-targets -- -D warnings` → clean.
- [ ] **Step 4: Measure** (also run `SDC_ACHERUS=… cargo test -p modscan --release --test plugins owner_load_order -- --ignored --nocapture` → PASS), fill row "Task 7", apply the keep rule (a and b may be kept or reverted separately: commit them separately).
- [ ] **Step 5: Commit** `git add crates/modscan/src/plugin_store.rs && git commit -m "perf(modscan): plugin rows use the scan's path interning map"` then `git add crates/modscan/src/plugins.rs && git commit -m "perf(modscan): overlap plugin parsing with a store writer thread"`

---

### Task 8: Derived copies written once on cold scans (conditional)

Do this task only if Task 1 (or the latest row) shows `commit_pending` ≥ 0.5 s cold **or** derived copies ≥ 10 % of `nifs`. Otherwise record `skipped: commit_pending = X ms, derived = N` and go to Task 9.

**Files:**
- Modify: `crates/modscan/src/store.rs` (`write_chunk`, `discard_pending`)

**Interfaces:**
- Consumes: `Store.cold` (Task 4). Behaviour (R17): cold → derived copies go straight to `nif_copy`/`shape`/`tex_raw`; taint/cancel on cold deletes every `is_derived = 1` copy.

- [ ] **Step 1: Confirm the guarding tests pass** on the current code: `cargo test -p modscan --test scan pgp_started_mid_scan_taints_result winner_and_source_with_pgpatcher_output` and `cargo test -p modscan --test scan_dump` → PASS. (`pgp_started_mid_scan_taints_result` starts on an empty store, so it is the cold-taint test.)
- [ ] **Step 2: Implement.** `write_chunk`:
```rust
    /// Writes one chunk in one transaction: ordinary copies into `nif_copy`/`shape`. Derived ones are staged until the
    /// post-walk PGPatcher check, except on a cold scan (ruling R17): no previous rows to keep, so they go straight in.
    pub fn write_chunk(&mut self, items: &[ChunkItem]) -> Result<(), ScanError> {
        let tx = self.conn.transaction()?;
        for it in items {
            if it.derived && !self.cold {
                stage_derived(&tx, &mut self.intern, it)?;
            } else {
                upsert_copy_row(&tx, &mut self.intern, it)?;
            }
        }
        tx.commit()?;
        Ok(())
    }
```
`discard_pending`:
```rust
    /// Tainted or cancelled run: drop the staged derived copies and keep the previous rows. A cold run wrote its
    /// derived copies directly (ruling R17); there were no previous rows, so they are deleted.
    pub fn discard_pending(&mut self) -> Result<(), ScanError> {
        self.conn.execute_batch(
            "DELETE FROM pending_tex_raw; DELETE FROM pending_shape; DELETE FROM pending_derived;",
        )?;
        if self.cold {
            self.conn.execute_batch(
                "DELETE FROM shape WHERE copy_id IN (SELECT id FROM nif_copy WHERE is_derived = 1);
                 DELETE FROM tex_raw WHERE copy_id IN (SELECT id FROM nif_copy WHERE is_derived = 1);
                 DELETE FROM nif_copy WHERE is_derived = 1;",
            )?;
        }
        Ok(())
    }
```
- [ ] **Step 3: Run.** `cargo test -p modscan` → PASS (the dump test proves cold results are unchanged; `pgp_started_mid_scan_taints_result` proves the cold taint).
- [ ] **Step 4: Measure**, fill row "Task 8", apply the keep rule.
- [ ] **Step 5: Commit** `git add crates/modscan/src/store.rs && git commit -m "perf(modscan): cold scans write PGPatcher-derived copies once"`

---

### Task 9: esp — IPCT texture sets, HDPT flags and extra parts, LTEX grasses

**Files:**
- Modify: `crates/esp/src/lib.rs` (`Payload.hdpt_flags`, docs)
- Modify: `crates/esp/src/records.rs` (`WANTED` + IPCT, `read` arms)
- Test: `crates/esp/tests/records.rs` (append)

**Interfaces:**
- Produces: `esp::Payload.hdpt_flags: Option<u8>`. `Payload.txst_refs` also carries `(*b"DNAM"|*b"ENAM", txst)` for IPCT, `(*b"HNAM", hdpt)` for HDPT and `(*b"GNAM", gras)` for LTEX (R7, R11).

- [ ] **Step 1: Write the failing tests** (append to `crates/esp/tests/records.rs`):
```rust
/// Filtering spec §3, xEdit dev-4.1.6 `wbDefinitionsTES5.pas` l.6238-6276: IPCT DNAM/ENAM are TXSTs; its model is read
/// like every model record (ruling R11); sounds are not read.
#[test]
fn impact_texture_sets() {
    let r = one(
        b"IPCT",
        rec(
            b"IPCT",
            1,
            &[
                edid("Imp"),
                sub(b"MODL", &zstr("Effects\\Imp.nif")),
                sub(b"DATA", &[0u8; 24]),
                form(b"DNAM", 0xA1),
                form(b"ENAM", 0xA2),
                form(b"SNAM", 0xFF),
            ],
        ),
    );
    assert_eq!(r.payload.txst_refs, vec![(*b"DNAM", 0xA1), (*b"ENAM", 0xA2)]);
    assert_eq!(r.payload.models[0].path, "meshes/effects/imp.nif");
}

/// l.5279-5309: HDPT DATA is a u8 flag set (0x01 Playable); HNAM lists extra-part HDPTs (ruling R7).
#[test]
fn head_part_flags_and_extra_parts() {
    let h = one(
        b"HDPT",
        rec(
            b"HDPT",
            2,
            &[
                edid("Hair"),
                sub(b"DATA", &[0x03]),
                sub(b"PNAM", &3u32.to_le_bytes()),
                form(b"HNAM", 0xB1),
                form(b"HNAM", 0xB2),
                form(b"TNAM", 0xB3),
            ],
        ),
    );
    assert_eq!(h.payload.hdpt_flags, Some(0x03));
    assert_eq!(
        h.payload.txst_refs,
        vec![(*b"HNAM", 0xB1), (*b"HNAM", 0xB2), (*b"TNAM", 0xB3)]
    );
    assert_eq!(one(b"HDPT", rec(b"HDPT", 3, &[edid("NoData")])).payload.hdpt_flags, None);
    let s = one(b"STAT", rec(b"STAT", 4, &[edid("S"), sub(b"DATA", &[0x01])]));
    assert_eq!(s.payload.hdpt_flags, None, "DATA is read on HDPT only");
}

/// l.8009: LTEX GNAM lists the grasses of the landscape texture.
#[test]
fn landscape_texture_grasses() {
    let l = one(
        b"LTEX",
        rec(
            b"LTEX",
            5,
            &[edid("Dirt"), form(b"TNAM", 0xC1), form(b"GNAM", 0xC2), form(b"GNAM", 0xC3)],
        ),
    );
    assert_eq!(
        l.payload.txst_refs,
        vec![(*b"TNAM", 0xC1), (*b"GNAM", 0xC2), (*b"GNAM", 0xC3)]
    );
}
```
- [ ] **Step 2: Run.** `cargo test -p esp --test records` → Expected: FAIL to compile (`no field hdpt_flags`).
- [ ] **Step 3: Implement.**
`crates/esp/src/lib.rs`, `Payload`: change the `txst_refs` doc to `/// (role = subrecord signature, local FormID). The target is a TXST, except role HNAM (an extra-part HDPT) and GNAM (a GRAS); FormID 0 is never stored.` and add:
```rust
    /// HDPT DATA flags (xEdit: 0x01 Playable, 0x02 Male, 0x04 Female, 0x08 Is Extra Part, 0x10 Use Solid Tint);
    /// `None` on other records and when DATA is absent.
    pub hdpt_flags: Option<u8>,
```
`crates/esp/src/records.rs`: `const WANTED: [[u8; 4]; 29]` with `*b"IPCT",` appended after `*b"WTHR",`. In `read`, extend the `match &kind` (before `_ => {}`):
```rust
            b"HDPT" if &sig == b"DATA" => p.hdpt_flags = d.first().copied(),
            b"HDPT" if &sig == b"HNAM" => form_ref(p, sig, d),
            b"LTEX" if &sig == b"GNAM" => form_ref(p, sig, d),
            b"IPCT" if matches!(&sig, b"DNAM" | b"ENAM") => form_ref(p, sig, d),
```
Update the doc comment of `form_ref` to `/// A u32 FormID subrecord (a TXST, or an HDPT/GRAS for HNAM/GNAM); 0 (none) is dropped.`
- [ ] **Step 4: Run.** `cargo test -p esp` → PASS. `cargo test -p modscan --test scan_dump` → PASS (the fixture has no IPCT/HDPT/GNAM).
- [ ] **Step 5: Commit** `git add crates/esp/src/lib.rs crates/esp/src/records.rs crates/esp/tests/records.rs && git commit -m "feat(esp): read IPCT texture sets, HDPT flags and extra parts, LTEX grasses"`

---

### Task 10: esp — placed-reference pass (CELL/WRLD)

**Files:**
- Modify: `crates/esp/src/lib.rs` (`PluginFacts.placed`)
- Modify: `crates/esp/src/read.rs` (`parse` top-level loop, new `walk_placed`)
- Test: `crates/esp/tests/parse.rs` (one label change, R16; append tests)

**Interfaces:**
- Produces: `esp::PluginFacts.placed: Vec<u32>`, the distinct raw base FormIDs (as stored) of REFR/ACHR records in CELL/WRLD groups, ascending.

- [ ] **Step 1: Write the failing tests.** In `crates/esp/tests/parse.rs` change `let cell = grup(b"CELL", 0, &[vec![0xFF; 100]]);` to `let cell = grup(b"QUST", 0, &[vec![0xFF; 100]]);` (R16: CELL is now walked). Append:
```rust
fn placed(sig: &[u8; 4], id: u32, base: u32) -> Vec<u8> {
    rec(sig, id, &[edid("R"), form(b"NAME", base), sub(b"DATA", &[0u8; 24])])
}

/// Filtering spec §3; group types per xEdit dev-4.1.6 `wbImplementation.pas` `TwbGroupRecord.GetName` (1-10). The base
/// FormID of every REFR/ACHR under CELL and WRLD, once each, ascending; other records there are skipped unread.
#[test]
fn placed_references_in_interior_and_exterior_cells() {
    let cell = 0x0000_1000u32.to_le_bytes();
    let interior = grup(
        b"CELL",
        0,
        &[grup(&0u32.to_le_bytes(), 2, &[grup(&0u32.to_le_bytes(), 3, &[
            rec(b"CELL", 0x1000, &[edid("Room")]),
            grup(&cell, 6, &[
                grup(&cell, 8, &[placed(b"REFR", 0x2001, 0x0100_0800), placed(b"ACHR", 0x2002, 0x7777)]),
                grup(&cell, 9, &[placed(b"REFR", 0x2003, 0x0100_0800), rec(b"NAVM", 0x2004, &[vec![0xFF; 32]])]),
            ]),
        ])])],
    );
    let (wrld, ext) = (0x0000_3000u32.to_le_bytes(), 0x0000_3001u32.to_le_bytes());
    let packed = [form(b"NAME", 0x0ABC), sub(b"DATA", &[0u8; 24])].concat();
    let exterior = grup(
        b"WRLD",
        0,
        &[
            rec(b"WRLD", 0x3000, &[edid("Tamriel")]),
            grup(&wrld, 1, &[grup(&[0u8; 4], 4, &[grup(&[0u8; 4], 5, &[
                rec(b"CELL", 0x3001, &[]),
                grup(&ext, 6, &[grup(&ext, 9, &[rec_raw(b"REFR", 0x3002, FLAG_COMPRESSED, &compressed(&packed))])]),
            ])])]),
        ],
    );
    let f = parse(
        "x.esp",
        &plugin(0, &[], &[interior, exterior, top(b"STAT", &[rec(b"STAT", 0x0100_0800, &[edid("Rock")])])]),
    );
    assert!(f.errors.is_empty(), "{:?}", f.errors);
    assert_eq!(f.placed, vec![0x0ABC, 0x7777, 0x0100_0800]);
    assert_eq!(f.records.len(), 1, "placed references are not records");
}

/// Review focus 4: a bad nested group is reported and ends only its own group.
#[test]
fn a_bad_nested_cell_group_is_reported_and_later_groups_are_read() {
    let mut bad = grup(&[0u8; 4], 6, &[placed(b"REFR", 1, 0x10)]);
    bad[4..8].copy_from_slice(&10_000u32.to_le_bytes()); // runs past its parent
    let f = parse(
        "x.esp",
        &plugin(0, &[], &[grup(b"CELL", 0, &[bad]), top(b"STAT", &[rec(b"STAT", 0x0100_0800, &[edid("Rock")])])]),
    );
    assert_eq!(f.errors.len(), 1, "{:?}", f.errors);
    assert!(f.errors[0].contains("nested group size"), "{:?}", f.errors);
    assert_eq!(f.records.len(), 1);
}

/// Review focus 4: nesting is capped, so a crafted file cannot overflow the stack.
#[test]
fn groups_nested_too_deep_are_an_error_not_a_stack_overflow() {
    let mut g = grup(&[0u8; 4], 9, &[placed(b"REFR", 1, 0x10)]);
    for _ in 0..20 {
        g = grup(&[0u8; 4], 6, &[g]);
    }
    let f = parse("x.esp", &plugin(0, &[], &[grup(b"CELL", 0, &[g])]));
    assert_eq!(f.errors.len(), 1, "{:?}", f.errors);
    assert!(f.errors[0].contains("nested deeper"), "{:?}", f.errors);
    assert!(f.placed.is_empty());
}
```
- [ ] **Step 2: Run.** `cargo test -p esp --test parse` → Expected: FAIL to compile (`no field placed`).
- [ ] **Step 3: Implement.**
`crates/esp/src/lib.rs`, `PluginFacts`, after `records`:
```rust
    /// Base FormIDs (as stored) placed by REFR/ACHR records in CELL and WRLD groups, once each, ascending
    /// (filtering spec §3). The references themselves are not kept.
    pub placed: Vec<u32>,
```
`crates/esp/src/read.rs`: add `use std::collections::BTreeSet;` and `const MAX_DEPTH: usize = 8;` with the doc `/// Nested groups entered under a CELL/WRLD top group; valid files use at most 5.` In `parse`, before `let mut off = end;` add `let mut bases = BTreeSet::new();`. After `let wanted = …;` add:
```rust
        let placed = u32_at(b, off + 12) == 0 && (&label == b"CELL" || &label == b"WRLD");
```
In the `gend > b.len()` branch, replace `if wanted { walk_group(b, off + HEADER, b.len(), &mut f); }` with:
```rust
            if wanted {
                walk_group(b, off + HEADER, b.len(), &mut f);
            } else if placed {
                walk_placed(b, off + HEADER, b.len(), 0, &mut bases, &mut f.errors);
            }
```
and the normal `if wanted { walk_group(b, off + HEADER, gend, &mut f); }` with:
```rust
        if wanted {
            walk_group(b, off + HEADER, gend, &mut f);
        } else if placed {
            walk_placed(b, off + HEADER, gend, 0, &mut bases, &mut f.errors);
        }
```
Before the final `f` of `parse` add `f.placed = bases.into_iter().collect();`. Add after `walk_group`:
```rust
/// A CELL/WRLD group, `start..end` (filtering spec §3): every nested group is entered; of the records only REFR and
/// ACHR are read, for their base FormID (NAME); every other record is skipped by its size. A fault ends this group.
fn walk_placed(
    b: &[u8],
    start: usize,
    end: usize,
    depth: usize,
    bases: &mut BTreeSet<u32>,
    errors: &mut Vec<String>,
) {
    let mut p = start;
    while p < end {
        if p + HEADER > end {
            errors.push(format!("offset {p:#x}: truncated header runs past its group"));
            return;
        }
        let sig = sig_at(b, p);
        let size = u32_at(b, p + 4) as usize;
        if &sig == b"GRUP" {
            if size < HEADER || p + size > end {
                errors.push(format!("offset {p:#x}: nested group size {size} runs past its group"));
                return;
            }
            if depth >= MAX_DEPTH {
                errors.push(format!("offset {p:#x}: groups nested deeper than {MAX_DEPTH}"));
                return;
            }
            walk_placed(b, p + HEADER, p + size, depth + 1, bases, errors);
            p += size;
            continue;
        }
        let rend = p + HEADER + size;
        if rend > end {
            errors.push(format!(
                "offset {p:#x}: {} record size {size} runs past its group",
                sig_str(&sig)
            ));
            return;
        }
        if &sig == b"REFR" || &sig == b"ACHR" {
            let formid = u32_at(b, p + 12);
            let raw = &b[p + HEADER..rend];
            let inflated;
            let data: &[u8] = if u32_at(b, p + 8) & FLAG_COMPRESSED != 0 {
                match inflate(raw) {
                    Ok(v) => {
                        inflated = v;
                        &inflated
                    }
                    Err(e) => {
                        errors.push(format!("offset {p:#x}: {} {formid:08X}: {e}", sig_str(&sig)));
                        p = rend;
                        continue;
                    }
                }
            } else {
                raw
            };
            let mut base = None;
            if let Err(at) = each_sub(data, |s, d| {
                if &s == b"NAME" && d.len() >= 4 && base.is_none() {
                    base = Some(u32_at(d, 0));
                }
            }) {
                errors.push(format!(
                    "offset {p:#x}: {} {formid:08X}: subrecord at +{at:#x} runs past the record",
                    sig_str(&sig)
                ));
            }
            if let Some(id) = base.filter(|&id| id != 0) {
                bases.insert(id);
            }
        }
        p = rend;
    }
}
```
- [ ] **Step 4: Run.** `cargo test -p esp` → PASS. `cargo test -p modscan` → PASS (the dump test is unchanged: its plugin has no CELL).
- [ ] **Step 5: Measure the cold plugin cost.** Run the timing protocol and `SDC_ACHERUS=… cargo test -p modscan --release --test plugins owner_load_order -- --ignored --nocapture` (→ PASS; its `< 10 s` cold assert must hold). Fill row "Task 10" with `plugin_read` before/after. If cold elapsed now exceeds 20 s, or `plugin_read` grew by more than 3 s, stop and report with the numbers. The fallback is to inflate a compressed REFR only up to its NAME subrecord; it is not to be done unasked.
- [ ] **Step 6: Commit** `git add crates/esp/src/lib.rs crates/esp/src/read.rs crates/esp/tests/parse.rs && git commit -m "feat(esp): collect placed base FormIDs from CELL and WRLD groups"`

---

### Task 11: Store schema 5 — placed bases, head-part flags, inert table

**Files:**
- Modify: `crates/modscan/src/store.rs` (`SCHEMA_VERSION`, `SCHEMA`)
- Modify: `crates/modscan/src/plugin_store.rs` (`PLUGIN_TABLES`, `REMAP`, fallback rule, `write_plugin`)
- Modify: `crates/modscan/tests/store.rs:226` (`assert_eq!(v, 4)` → `5`)
- Create: `crates/modscan/tests/plugin_inert.rs`

**Interfaces:**
- Produces tables: `placed(plugin_id, base_local, base_formid)`, `rec.hdpt_flags`, `rec_inert(formid PK, reason)` (empty until Task 12). Test helpers in `plugin_inert.rs`: `fixture(plugins_txt: &str) -> TempDir`, `scan(&TempDir) -> (Store, PluginStats)`, `one_i64(&Store, &str) -> i64`, `ORDER`, `OVER`.

- [ ] **Step 1: Write the fixture and the failing test.** Create `crates/modscan/tests/plugin_inert.rs`:
```rust
//! Filtering spec §3: placed bases, head-part flags, and which winning records are inert (`rec_inert`).
#[path = "../../esp/tests/common/mod.rs"]
mod b;
mod common;
use b::{alt, edid, form, grup, plugin, rec, sub, top, zstr};
use common::{instance_fixture, put};
use modscan::mo2::{loose_origins, Instance};
use modscan::plugins::{scan_plugins, PluginStats};
use modscan::store::Store;
use std::sync::atomic::AtomicBool;

const ORDER: &str = "*Mod.esp\n*Ghost.esp\n";
const OVER: &str = "*Mod.esp\n*Ghost.esp\n*Over.esp\n";

fn txst(id: u32, name: &str, tx00: Option<&str>) -> Vec<u8> {
    let mut s = vec![edid(name)];
    if let Some(p) = tx00 {
        s.push(sub(b"TX00", &zstr(p)));
    }
    rec(b"TXST", id, &s)
}

fn model(sig: &[u8; 4], id: u32, name: &str, nif: &str, swap: u32) -> Vec<u8> {
    rec(
        sig,
        id,
        &[edid(name), sub(b"MODL", &zstr(nif)), sub(b"MODS", &alt(&[("Shape0", swap, 0)]))],
    )
}

fn hdpt(id: u32, name: &str, data: Option<u8>, extra: &[u32], tnam: Option<u32>) -> Vec<u8> {
    let mut s = vec![edid(name)];
    if let Some(f) = data {
        s.push(sub(b"DATA", &[f]));
    }
    s.extend(extra.iter().map(|&h| form(b"HNAM", h)));
    if let Some(t) = tnam {
        s.push(form(b"TNAM", t));
    }
    rec(b"HDPT", id, &s)
}

/// One interior cell (block 0, sub-block 0) whose temporary children are `(ref id, base, is ACHR)`.
fn cell(cell_id: u32, refs: &[(u32, u32, bool)]) -> Vec<u8> {
    let children: Vec<Vec<u8>> = refs
        .iter()
        .map(|&(id, base, achr)| rec(if achr { b"ACHR" } else { b"REFR" }, id, &[form(b"NAME", base)]))
        .collect();
    let c = cell_id.to_le_bytes();
    grup(b"CELL", 0, &[grup(&0u32.to_le_bytes(), 2, &[grup(&0u32.to_le_bytes(), 3, &[
        rec(b"CELL", cell_id, &[edid("Cell")]),
        grup(&c, 6, &[grup(&c, 9, &children)]),
    ])])])
}

/// Skyrim.esm (Data) defines everything; Mod.esp places PlacedStat, the Decal TXST and the Guard NPC; Ghost.esp
/// (master Missing.esm, not loaded) places a form of Missing.esm with LonelyStat's local id; Over.esp makes
/// PlayableHair non-playable.
fn fixture(plugins_txt: &str) -> tempfile::TempDir {
    let d = instance_fixture("+B\n+A\n");
    let r = d.path();
    put(
        r,
        "Game/Data/Skyrim.esm",
        &plugin(
            esp::FLAG_ESM,
            &[],
            &[
                top(b"TXST", &[
                    txst(0xA01, "UsedByPlaced", Some("live\\p_d.dds")),
                    txst(0xA02, "UsedByUnplaced", Some("inert\\u_d.dds")),
                    txst(0xA03, "Decal", None),
                    txst(0xA04, "Impact", None),
                    txst(0xA05, "Orphan", Some("inert\\o_d.dds")),
                    txst(0xA06, "HairTex", None),
                    txst(0xA07, "NpcHairTex", None),
                    txst(0xA08, "WeapTex", None),
                ]),
                top(b"STAT", &[
                    model(b"STAT", 0xB01, "PlacedStat", "placed.nif", 0xA01),
                    model(b"STAT", 0xB02, "LonelyStat", "lonely.nif", 0xA02),
                ]),
                top(b"WEAP", &[model(b"WEAP", 0xB03, "Sword", "sword.nif", 0xA08)]),
                top(b"IPCT", &[rec(b"IPCT", 0xB04, &[edid("Imp"), form(b"DNAM", 0xA04)])]),
                top(b"HDPT", &[
                    hdpt(0xC01, "PlayableHair", Some(0x01), &[0xC02], Some(0xA06)),
                    hdpt(0xC02, "ExtraPart", Some(0x08), &[], None),
                    hdpt(0xC03, "NpcOnly", Some(0x02), &[0xC04], Some(0xA07)),
                    hdpt(0xC04, "NpcExtra", Some(0x00), &[], None),
                    hdpt(0xC05, "NoData", None, &[], None),
                ]),
                top(b"GRAS", &[rec(b"GRAS", 0xD01, &[edid("Grass")]), rec(b"GRAS", 0xD02, &[edid("LonelyGrass")])]),
                top(b"LTEX", &[rec(b"LTEX", 0xD03, &[edid("Dirt"), form(b"GNAM", 0xD01)])]),
                top(b"ARMA", &[rec(b"ARMA", 0xE01, &[edid("Arma")])]),
                top(b"NPC_", &[rec(b"NPC_", 0xE02, &[edid("Guard")])]),
            ],
        ),
    );
    put(
        r,
        "mods/A/Mod.esp",
        &plugin(0, &["Skyrim.esm"], &[cell(0x0100_0800, &[
            (0x0100_0801, 0x0000_0B01, false),
            (0x0100_0802, 0x0000_0A03, false),
            (0x0100_0803, 0x0000_0E02, true),
        ])]),
    );
    put(
        r,
        "mods/A/Ghost.esp",
        &plugin(0, &["Missing.esm"], &[cell(0x0100_0800, &[(0x0100_0801, 0x0000_0B02, false)])]),
    );
    put(
        r,
        "mods/B/Over.esp",
        &plugin(0, &["Skyrim.esm"], &[top(b"HDPT", &[hdpt(0xC01, "PlayableHair", Some(0x00), &[0xC02], Some(0xA06))])]),
    );
    put(r, "profiles/Default/plugins.txt", plugins_txt.as_bytes());
    d
}

fn scan(d: &tempfile::TempDir) -> (Store, PluginStats) {
    let inst = Instance::load(d.path(), None).unwrap();
    let (loose, _) = loose_origins(&inst).unwrap();
    let mut store = Store::open(&d.path().join("scan.db")).unwrap();
    let st = scan_plugins(&inst, &loose, &mut store, &AtomicBool::new(false), &|_| {}).unwrap();
    (store, st)
}

fn one_i64(s: &Store, sql: &str) -> i64 {
    s.conn().query_row(sql, [], |r| r.get(0)).unwrap()
}

/// Spec §3 + ruling R7/R10: placed bases resolve through their plugin's masters (NULL when the master is not loaded);
/// HDPT flags are stored (NULL without DATA); HNAM/GNAM edges get load-order targets.
#[test]
fn placed_bases_head_part_flags_and_edges_are_stored() {
    let d = fixture(ORDER);
    let (s, _) = scan(&d);
    let placed: Vec<(String, Option<i64>)> = s
        .conn()
        .prepare("SELECT pl.name, p.base_formid FROM placed p JOIN plugin pl ON pl.id = p.plugin_id ORDER BY 1, 2")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(
        placed,
        vec![
            ("Ghost.esp".to_string(), None),
            ("Mod.esp".to_string(), Some(0xA03)),
            ("Mod.esp".to_string(), Some(0xB01)),
            ("Mod.esp".to_string(), Some(0xE02)),
        ]
    );
    assert_eq!(one_i64(&s, "SELECT hdpt_flags FROM rec WHERE edid = 'PlayableHair'"), 1);
    assert_eq!(one_i64(&s, "SELECT count(*) FROM rec WHERE edid = 'NoData' AND hdpt_flags IS NULL"), 1);
    assert_eq!(one_i64(&s, "SELECT txst_formid FROM txst_ref WHERE role = 'GNAM'"), 0xD01);
    assert_eq!(one_i64(&s, "SELECT count(*) FROM txst_ref WHERE role = 'HNAM'"), 2);
    assert_eq!(one_i64(&s, "SELECT count(*) FROM txst_ref WHERE role = 'DNAM'"), 1, "IPCT");
}
```
In `crates/modscan/tests/store.rs` change `assert_eq!(v, 4);` to `assert_eq!(v, 5);`.
- [ ] **Step 2: Run.** `cargo test -p modscan --test plugin_inert` → Expected: FAIL ("no such table: placed"). Checked at writing time: the only raw `INSERT INTO rec` in the tests (`crates/gui/tests/set_plugin_refs.rs:158`) names its columns, so the new `hdpt_flags` column breaks nothing; re-run `grep -rn "INTO rec[ (]" crates/gui crates/modscan` and convert any positional insert to a column list.
- [ ] **Step 3: Implement.** `crates/modscan/src/store.rs`: `pub const SCHEMA_VERSION: i64 = 5;`. In `SCHEMA`, add `, hdpt_flags INTEGER` to `rec` (after `decal INTEGER`). Change the comment above `txst_ref` to `-- txst_ref rows with role HNAM (HDPT -> extra-part HDPT) and GNAM (LTEX -> GRAS) point at those types, not TXSTs.` and add before `CREATE INDEX rec_formid`:
```sql
-- Base FormIDs placed by REFR/ACHR in a plugin's CELL/WRLD groups, once per plugin (filtering spec §3). `base_local`
-- clamped like `local`; `base_formid` set by `resolve_plugins`, NULL when its master is not loaded.
CREATE TABLE placed (plugin_id INTEGER NOT NULL, base_local INTEGER NOT NULL, base_formid INTEGER);
-- Winning records nothing reaches (filtering spec §3), rebuilt with rec_winner. reason: 1 no reference, 2 not
-- placed, 3 head part not playable.
CREATE TABLE rec_inert (formid INTEGER PRIMARY KEY, reason INTEGER NOT NULL);
CREATE INDEX placed_base ON placed(base_formid);
CREATE INDEX placed_plugin ON placed(plugin_id);
```
`crates/modscan/src/plugin_store.rs`:
- `const PLUGIN_TABLES: [&str; 7] = ["rec", "txst_slot", "model_ref", "alt_tex", "txst_ref", "tex_ref", "placed"];` (the `delete_rows` index `placed_plugin` covers it).
- `const REMAP: [(&str, &str, &str); 9]` with `("placed", "base_formid", "base_local"),` appended.
- In `resolve_plugins` replace the fallback:
```rust
            // A target whose master is missing stays NULL: its raw local id could equal a real winner's FormID.
            let fallback = if out == "formid" {
                format!("{t}.{src}")
            } else {
                "NULL".to_string()
            };
```
- `write_plugin`: the rec insert becomes
```rust
            tx.prepare_cached(
                "INSERT INTO rec(local, plugin_id, sig, edid, txst_flags, decal, hdpt_flags) VALUES (?1,?2,?3,?4,?5,?6,?7)",
            )?
            .execute(params![
                local,
                id,
                sig_text(&r.sig),
                r.edid,
                t.map(|t| i64::from(t.flags)),
                t.map(|t| t.decal),
                r.payload.hdpt_flags.map(i64::from)
            ])?;
```
and after the records loop (before `tx.commit()?;`):
```rust
        for &base in &facts.placed {
            tx.prepare_cached("INSERT INTO placed(plugin_id, base_local) VALUES (?1,?2)")?
                .execute(params![id, i64::from(clamp_local(base, nm))])?;
        }
```
- [ ] **Step 4: Run.** `cargo test -p modscan` → PASS (the dump test is unchanged: explicit schema-4 columns). `cargo test -p sdc-gui --test meshes_view --test set_plugin_refs` → PASS.
- [ ] **Step 5: Commit** `git add crates/modscan/src/store.rs crates/modscan/src/plugin_store.rs crates/modscan/tests/store.rs crates/modscan/tests/plugin_inert.rs && git commit -m "feat(modscan): schema 5 stores placed bases, head-part flags and an inert table"`

---

### Task 12: Reachability in SQL (`rec_inert`)

**Files:**
- Modify: `crates/modscan/src/plugin_store.rs` (`INERT_SQL`, `resolve_plugins`, `retain_plugins`, `unload_plugins`)
- Test: `crates/modscan/tests/plugin_inert.rs` (append)

**Interfaces:**
- Consumes: `placed`, `rec.hdpt_flags`, `txst_ref` roles HNAM/GNAM (Task 11). Produces: filled `rec_inert` after every `resolve_plugins`; emptied wherever `rec_winner` is emptied.

- [ ] **Step 1: Write the failing tests** (append to `plugin_inert.rs`):
```rust
/// (EDID of the winning copy, reason) of every inert record.
fn inert(s: &Store) -> Vec<(String, i64)> {
    s.conn()
        .prepare(
            "SELECT r.edid, i.reason FROM rec_inert i JOIN rec_winner w ON w.formid = i.formid
             JOIN rec r ON r.plugin_id = w.plugin_id AND r.formid = w.formid ORDER BY r.edid",
        )
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

/// Spec §3 (rulings R8-R10, R13): placeables need a placement, TXSTs a live user or a placement, GRAS an LTEX, HDPT
/// Playable or a playable parent; items, IPCT, NPC_, ARMA and records without DATA are never inert.
#[test]
fn only_proven_unreached_records_are_inert() {
    let d = fixture(ORDER);
    let (s, _) = scan(&d);
    let want: Vec<(String, i64)> = [
        ("LonelyGrass", 1),
        ("LonelyStat", 2),
        ("NpcExtra", 3),
        ("NpcHairTex", 1),
        ("NpcOnly", 3),
        ("Orphan", 1),
        ("UsedByUnplaced", 1),
    ]
    .into_iter()
    .map(|(e, r)| (e.to_string(), r))
    .collect();
    assert_eq!(inert(&s), want);
}

/// Review focus 5: the winning copy decides. Over.esp makes PlayableHair non-playable, so it, its extra part and its
/// texture set become inert.
#[test]
fn reachability_follows_the_winning_copy() {
    let d = fixture(OVER);
    let (s, _) = scan(&d);
    let i = inert(&s);
    for (e, r) in [("PlayableHair", 3), ("ExtraPart", 3), ("HairTex", 1)] {
        assert!(i.contains(&(e.to_string(), r)), "{e}: {i:?}");
    }
}

#[test]
fn inert_rows_go_with_the_winners() {
    let d = fixture(ORDER);
    let (mut s, _) = scan(&d);
    assert!(one_i64(&s, "SELECT count(*) FROM rec_inert") > 0);
    s.unload_plugins().unwrap();
    assert_eq!(one_i64(&s, "SELECT count(*) FROM rec_inert"), 0);
}
```
- [ ] **Step 2: Run.** `cargo test -p modscan --test plugin_inert` → Expected: the three new tests FAIL (`rec_inert` is empty).
- [ ] **Step 3: Implement** in `crates/modscan/src/plugin_store.rs`. Add after `REMAP`:
```rust
/// Filtering spec §3: which winning records nothing reaches, conservatively. Runs after `rec_winner` is built, in the
/// same transaction. `win` = each winning copy; edges and alternate textures count only from winning copies.
const INERT_SQL: &str = "
DELETE FROM rec_inert;
CREATE TEMP TABLE IF NOT EXISTS win (formid INTEGER PRIMARY KEY, plugin_id INTEGER NOT NULL, sig TEXT NOT NULL,
  hdpt_flags INTEGER);
DELETE FROM win;
INSERT OR IGNORE INTO win SELECT w.formid, w.plugin_id, r.sig, r.hdpt_flags FROM rec_winner w
  JOIN rec r ON r.plugin_id = w.plugin_id AND r.formid = w.formid AND r.owner_ok = 1;
CREATE TEMP TABLE IF NOT EXISTS placed_base (formid INTEGER PRIMARY KEY);
DELETE FROM placed_base;
INSERT OR IGNORE INTO placed_base SELECT p.base_formid FROM placed p JOIN plugin pl ON pl.id = p.plugin_id
  WHERE pl.lo_index IS NOT NULL AND p.base_formid IS NOT NULL;
-- 2 not placed: placeable bases no REFR/ACHR of the load order places (ruling R10).
INSERT INTO rec_inert SELECT formid, 2 FROM win
  WHERE sig IN ('STAT', 'MSTT', 'FURN', 'DOOR', 'CONT', 'LIGH', 'ACTI', 'TREE', 'FLOR')
    AND formid NOT IN (SELECT formid FROM placed_base);
-- 3 head part not playable: DATA without Playable (0x01; no DATA = playable) and no playable winning HDPT lists it
-- in HNAM (ruling R9).
INSERT INTO rec_inert SELECT h.formid, 3 FROM win h
  WHERE h.sig = 'HDPT' AND h.hdpt_flags & 1 = 0
    AND NOT EXISTS (SELECT 1 FROM txst_ref e JOIN win p ON p.formid = e.formid AND p.plugin_id = e.plugin_id
      WHERE e.role = 'HNAM' AND e.txst_formid = h.formid AND p.sig = 'HDPT' AND ifnull(p.hdpt_flags, 1) & 1 = 1);
-- 1 no reference: grass no winning landscape texture lists (GNAM).
INSERT INTO rec_inert SELECT g.formid, 1 FROM win g
  WHERE g.sig = 'GRAS'
    AND NOT EXISTS (SELECT 1 FROM txst_ref e JOIN win l ON l.formid = e.formid AND l.plugin_id = e.plugin_id
      WHERE e.role = 'GNAM' AND e.txst_formid = g.formid);
-- 1 no reference: TXSTs that no reference places and no winning, non-inert record uses (alternate textures; LTEX/HDPT
-- TNAM, NPC_ FTST, RACE face sets, ARMA NAM0/NAM1, IPCT DNAM/ENAM).
INSERT INTO rec_inert SELECT t.formid, 1 FROM win t
  WHERE t.sig = 'TXST'
    AND t.formid NOT IN (SELECT formid FROM placed_base)
    AND NOT EXISTS (SELECT 1 FROM alt_tex a JOIN win u ON u.formid = a.formid AND u.plugin_id = a.plugin_id
      WHERE a.txst_formid = t.formid AND u.formid NOT IN (SELECT formid FROM rec_inert))
    AND NOT EXISTS (SELECT 1 FROM txst_ref e JOIN win u ON u.formid = e.formid AND u.plugin_id = e.plugin_id
      WHERE e.txst_formid = t.formid AND e.role NOT IN ('HNAM', 'GNAM')
        AND u.formid NOT IN (SELECT formid FROM rec_inert));";
```
In `resolve_plugins`, after the `tx.execute_batch("UPDATE rec SET owner_ok … GROUP BY r.formid);")?;` call, add `tx.execute_batch(INERT_SQL)?;`. In `retain_plugins` change the batch to `"UPDATE plugin SET lo_index = NULL; DELETE FROM rec_winner; DELETE FROM rec_inert;"`. Make the same change in `unload_plugins`.
- [ ] **Step 4: Run.** `cargo test -p modscan` → PASS (`unload_plugins_rolls_back_as_one` and the other trigger tests still pass: one transaction).
- [ ] **Step 5: Measure** `owner_load_order` (cold must stay < 10 s, warm < 1 s; the asserts hold). Print the inert counts once by hand: `SELECT reason, count(*) FROM rec_inert GROUP BY 1`, added temporarily to the test and **not** committed. Record them in the Verification notes.
- [ ] **Step 6: Commit** `git add crates/modscan/src/plugin_store.rs crates/modscan/tests/plugin_inert.rs && git commit -m "feat(modscan): mark winning records nothing reaches as inert"`

---

### Task 13: Inert records ignored in Missing and Unused; `inert_of` for the GUI

**Files:**
- Modify: `crates/modscan/src/plugin_store.rs` (`MISSING_SQL`, `UNUSED_SQL`, `plugin_missing_files`, new `plugin_missing_with_inert`, `inert_of`, `inert_reason`)
- Test: `crates/modscan/tests/plugin_inert.rs` (append); `crates/modscan/tests/plugin_joins.rs` (fixtures place their records, Step 4)

**Interfaces:**
- Produces: `pub fn inert_reason(code: i64) -> &'static str`; `Store::inert_of(&self, formids: &[u32]) -> Result<HashMap<u32, &'static str>, ScanError>`; `Store::plugin_missing_with_inert(&self) -> Result<(Vec<MissingRef>, Vec<MissingRef>), ScanError>` (live, inert). `plugin_missing_files()` keeps its signature and returns the live list. `unused_textures()` keeps its signature; a texture that only inert records name is now in it (R12).

- [ ] **Step 1: Write the failing test** (append to `plugin_inert.rs`; add `use modscan::plugin_store::MissingRef;` to the imports):
```rust
/// Registers `path` as a winning texture of a fresh enabled origin.
fn provide(s: &Store, path: &str) {
    let c = s.conn();
    let n: i64 = c.query_row("SELECT count(*) FROM origin", [], |r| r.get(0)).unwrap();
    c.execute(
        "INSERT INTO origin(kind, name, root, priority, enabled) VALUES (9, 'x', ?1, 0, 1)",
        [format!("fake{n}")],
    )
    .unwrap();
    let o = c.last_insert_rowid();
    c.execute("INSERT OR IGNORE INTO path(p) VALUES (?1)", [path]).unwrap();
    let p: i64 = c.query_row("SELECT id FROM path WHERE p = ?1", [path], |r| r.get(0)).unwrap();
    c.execute("INSERT INTO tex_winner(path_id, origin_id, n_copies) VALUES (?1, ?2, 1)", [p, o])
        .unwrap();
}

/// Spec §3 (ruling R12): inert records name no Missing path and use no texture; paths only they name are listed apart,
/// labels ending in the reason.
#[test]
fn missing_and_unused_ignore_inert_records() {
    let d = fixture(ORDER);
    let (s, _) = scan(&d);
    let (live, inert) = s.plugin_missing_with_inert().unwrap();
    let paths = |v: &[MissingRef]| v.iter().map(|m| m.path.clone()).collect::<Vec<_>>();
    assert!(paths(&live).contains(&"textures/live/p_d.dds".to_string()), "{live:?}");
    assert!(paths(&live).contains(&"meshes/placed.nif".to_string()), "{live:?}");
    assert!(!paths(&live).contains(&"meshes/lonely.nif".to_string()), "{live:?}");
    let lonely = inert.iter().find(|m| m.path == "meshes/lonely.nif").unwrap();
    assert_eq!(lonely.records, ["LonelyStat [00000B02] STAT — Skyrim.esm (not placed)"]);
    let u = inert.iter().find(|m| m.path == "textures/inert/u_d.dds").unwrap();
    assert_eq!(u.records, ["UsedByUnplaced [00000A02] TXST — Skyrim.esm (no reference)"]);
    assert_eq!(s.plugin_missing_files().unwrap(), live);
    provide(&s, "textures/inert/u_d.dds");
    provide(&s, "textures/live/p_d.dds");
    let unused = s.unused_textures().unwrap();
    assert!(unused.contains(&"textures/inert/u_d.dds".to_string()), "{unused:?}");
    assert!(!unused.contains(&"textures/live/p_d.dds".to_string()), "{unused:?}");
    let r = s.inert_of(&[0xB01, 0xB02, 0xC03]).unwrap();
    assert_eq!(r.get(&0xB02), Some(&"not placed"));
    assert_eq!(r.get(&0xC03), Some(&"head part not playable"));
    assert!(!r.contains_key(&0xB01));
}
```
- [ ] **Step 2: Run.** `cargo test -p modscan --test plugin_inert missing_and_unused` → Expected: FAIL to compile (`no method plugin_missing_with_inert`).
- [ ] **Step 3: Implement** in `crates/modscan/src/plugin_store.rs`:
- `MISSING_SQL`: the select line becomes `SELECT DISTINCT p.p, r.edid, x.formid, r.sig, pl.name, i.reason FROM refs x` and add `LEFT JOIN rec_inert i ON i.formid = x.formid` after `JOIN plugin pl ON pl.id = x.plugin_id`. Doc comment: append ` Column 6 is the record's inert reason (NULL = live).`
- `UNUSED_SQL`: in both the `txst_slot` and the `tex_ref` sub-queries, add `AND NOT EXISTS (SELECT 1 FROM rec_inert i WHERE i.formid = t.formid)` after `WHERE t.path_id = tw.path_id`. Doc: append ` A record that is inert (filtering spec §3) is no use.`
- Add after `MISSING_RECORDS`:
```rust
/// The text of a `rec_inert.reason` (ruling R13).
pub fn inert_reason(code: i64) -> &'static str {
    match code {
        2 => "not placed",
        3 => "head part not playable",
        _ => "no reference",
    }
}

/// One path's rows gathered by `plugin_missing_with_inert`: live labels, inert labels.
type MissingAcc = (String, Vec<String>, Vec<String>);

fn push_missing(c: Option<MissingAcc>, live: &mut Vec<MissingRef>, inert: &mut Vec<MissingRef>) {
    let Some((path, l, i)) = c else {
        return;
    };
    let (to, labels) = if l.is_empty() { (inert, i) } else { (live, l) };
    to.push(MissingRef {
        path,
        n_records: labels.len(),
        records: labels.into_iter().take(MISSING_RECORDS).collect(),
    });
}
```
- Replace `plugin_missing_files` with:
```rust
    /// Missing files (spec §4.4, ruling R8) that at least one live record names, by path (filtering ruling R12).
    pub fn plugin_missing_files(&self) -> Result<Vec<MissingRef>, ScanError> {
        Ok(self.plugin_missing_with_inert()?.0)
    }

    /// Missing files split by who names them (filtering ruling R12): `.0` paths a live record names (only live records
    /// listed), `.1` paths only inert records name, each label ending in ` (reason)`.
    pub fn plugin_missing_with_inert(
        &self,
    ) -> Result<(Vec<MissingRef>, Vec<MissingRef>), ScanError> {
        let mut st = self.conn.prepare(MISSING_SQL)?;
        let mut rows = st.query([])?;
        let (mut live, mut inert) = (vec![], vec![]);
        let mut cur: Option<MissingAcc> = None;
        while let Some(r) = rows.next()? {
            let path: String = r.get(0)?;
            let edid: Option<String> = r.get(1)?;
            let label = record_label(
                edid.as_deref(),
                r.get::<_, i64>(2)? as u32,
                &r.get::<_, String>(3)?,
                &r.get::<_, String>(4)?,
            );
            let reason: Option<i64> = r.get(5)?;
            if cur.as_ref().is_some_and(|c| c.0 != path) {
                push_missing(cur.take(), &mut live, &mut inert);
            }
            let c = cur.get_or_insert_with(|| (path, vec![], vec![]));
            match reason {
                None => c.1.push(label),
                Some(x) => c.2.push(format!("{label} ({})", inert_reason(x))),
            }
        }
        push_missing(cur, &mut live, &mut inert);
        Ok((live, inert))
    }

    /// The inert reason (filtering spec §3) of each of `formids` that is inert.
    pub fn inert_of(&self, formids: &[u32]) -> Result<HashMap<u32, &'static str>, ScanError> {
        let ids = serde_json::to_string(formids).map_err(json_err)?;
        let mut st = self.conn.prepare(
            "SELECT formid, reason FROM rec_inert WHERE formid IN (SELECT value FROM json_each(?1))",
        )?;
        let v = st
            .query_map([ids], |r| {
                Ok((r.get::<_, i64>(0)? as u32, inert_reason(r.get(1)?)))
            })?
            .collect::<Result<_, _>>()?;
        Ok(v)
    }
```
- [ ] **Step 4: Keep the existing join tests about the Missing/Unused machinery, not about reachability.** Their fixtures never place anything, so under spec §3 GhostTXST, GhostStat, the `Lot*` TXSTs and the others are now inert, and `missing_and_unused_lists` / `missing_respects_archives_inactive_origins_facegen_and_caps_records` would see empty live lists. Place them instead of changing the assertions. In `crates/modscan/tests/plugin_joins.rs`, change the builder import to `use b::{alt, edid, form, grup, plugin, rec, sub, top, zstr};` and add:
```rust
/// A CELL group placing `bases` (filtering spec §3), so the fixture's TXSTs and STATs are live, not inert.
fn placing(bases: &[u32]) -> Vec<u8> {
    let refs: Vec<Vec<u8>> = bases
        .iter()
        .enumerate()
        .map(|(i, &b)| rec(b"REFR", 0x0F00 + i as u32, &[form(b"NAME", b)]))
        .collect();
    let cell = 0x0F80u32.to_le_bytes();
    grup(b"CELL", 0, &[grup(&0u32.to_le_bytes(), 2, &[grup(&0u32.to_le_bytes(), 3, &[grup(&cell, 9, &refs)])])])
}
```
In `fixture()`, add `placing(&[DIRT, GHOST, 0xDEF, 0xD00]),` as the last group of `Game/Data/Skyrim.esm`. In `fixture2()`, add `placing(&[0x0100_0101, 0x0100_0102, 0x0100_0103, 0x0100_0104, 0x0100_0200, 0x0100_0201, 0x0100_0202, 0x0100_0203, 0x0100_0204, 0x0100_0205, 0x0100_0206, 0x0100_0ABC]),` as the last group of its `Skyrim.esm` (its own records carry master index 1; unresolved `Orphan` stays unplaced on purpose).
Run: `cargo test -p modscan` → PASS, with no assertion changed in `plugin_joins.rs`. If another existing test fails, it must be because a record is now inert under spec §3. Fix its fixture the same way (place it) and name the test in the commit message.
- [ ] **Step 5: Commit** `git add crates/modscan/src/plugin_store.rs crates/modscan/tests/plugin_inert.rs crates/modscan/tests/plugin_joins.rs && git commit -m "feat(modscan): Missing and Unused ignore inert plugin records"`

---

### Task 14: GUI — Meshes view: Plugin overrides and Plugin file references

**Rebase note:** before starting, run `git log --oneline nif/main -5` and `git branch --contains` for `nif/h-refs-window`. If nif/h-refs-window has merged into nif/main, rebase this branch onto nif/main (`git rebase nif/main`; resolve keeping both changes) and put the Missing-tab toggle in that window's body instead of the `CollapsingHeader` described here. The model changes are the same. Keep edits inside `plugin_overrides`, `file_refs`, `load_shapes`, `load_file_refs`, `clear_nif` and the struct.

**Files:**
- Modify: `crates/gui/src/model/meshes.rs` (`MeshesModel` fields + `new`, `clear_nif`, `load_shapes`, `load_file_refs`, new `refs_inert_lines`)
- Modify: `crates/gui/src/ui/meshes.rs` (`detail` call site, `plugin_overrides`, `file_refs`)
- Test: `crates/gui/tests/meshes_view.rs` (append)

**Interfaces:**
- Consumes: `Store::inert_of`, `Store::plugin_missing_with_inert` (Task 13). Produces: `MeshesModel.plugin_inert: HashMap<u32, &'static str>`, `.show_inert_uses: bool`, `.missing_inert: Vec<MissingRef>`, `.show_inert_refs: bool`, `fn refs_inert_lines(&self) -> Vec<String>`.

- [ ] **Step 1: Write the failing tests** (append to `crates/gui/tests/meshes_view.rs`):
```rust
/// Filtering spec §3 / ruling R12: inert records are left out of "Plugin overrides" and its count, shown dimmed with
/// their reason behind "Show inert (N)".
#[test]
fn plugin_overrides_hide_inert_records_until_asked() {
    let mut m = model_with(None);
    m.nifs = vec![nif(1, "meshes/x.nif")];
    m.selected_nif = Some(1);
    let used = |formid: u32, edid: &str| MeshUse {
        formid,
        edid: Some(edid.into()),
        sig: "STAT".into(),
        plugin: "Skyrim.esm".into(),
        model_slot: 0,
        n_overrides: 0,
        swaps: vec![],
    };
    m.plugin_uses = vec![used(1, "Placed"), used(2, "Lonely")];
    m.plugin_inert = [(2u32, "not placed")].into_iter().collect();
    let mut h = harness(m);
    h.run();
    h.get_by_label("Plugin overrides (1)").click();
    h.run();
    h.get_by_label("Placed [00000001] STAT — Skyrim.esm");
    assert!(h.query_by_label_contains("Lonely").is_none());
    h.get_by_label("Show inert (1)").click();
    h.run();
    h.get_by_label("Lonely [00000002] STAT — Skyrim.esm · not placed");
}

/// Ruling R12: Missing lists live paths; paths only inert records name come dimmed behind "Show inert (N)".
#[test]
fn missing_files_of_inert_records_show_on_request() {
    let mut m = model_with(None);
    m.file_refs = Some(FileRefs {
        missing: vec![MissingRef {
            path: "textures/live.dds".into(),
            n_records: 1,
            records: vec!["A [00000001] TXST — S.esm".into()],
        }],
        unused: vec![],
    });
    m.missing_inert = vec![MissingRef {
        path: "meshes/lonely.nif".into(),
        n_records: 1,
        records: vec!["Lonely [00000002] STAT — S.esm (not placed)".into()],
    }];
    let mut h = harness(m);
    h.run();
    h.get_by_label("Plugin file references").click();
    h.run();
    h.get_by_label("Missing (1)");
    assert!(h.query_by_label_contains("lonely.nif").is_none());
    h.get_by_label("Show inert (1)").click();
    h.run();
    h.get_by_label("meshes/lonely.nif ← Lonely [00000002] STAT — S.esm (not placed)");
}
```
- [ ] **Step 2: Run.** `cargo test -p sdc-gui --test meshes_view inert` → Expected: FAIL to compile (`no field plugin_inert`).
- [ ] **Step 3: Implement the model** (`crates/gui/src/model/meshes.rs`):
- Struct, after `plugin_uses`:
```rust
    /// Inert reasons (filtering spec §3) of the `plugin_uses` records that are inert.
    pub plugin_inert: HashMap<u32, &'static str>,
    /// "Show inert" in Plugin overrides.
    pub show_inert_uses: bool,
```
after `refs_filter`:
```rust
    /// Missing paths that only inert records name (filtering ruling R12), read with `file_refs`.
    pub missing_inert: Vec<MissingRef>,
    /// "Show inert" on the Missing tab.
    pub show_inert_refs: bool,
```
and in `new`: `plugin_inert: HashMap::new(), show_inert_uses: false,` and `missing_inert: vec![], show_inert_refs: false,`.
- `clear_nif`: add `self.plugin_inert.clear();` after `self.plugin_uses.clear();`. Do the same in `load_shapes`'s `Err(e)` arm.
- `load_shapes`, `Ok(s)` arm: replace the `let uses = …;` and `self.plugin_uses = match uses {…};` with:
```rust
                let uses = store
                    .copy_location(copy)
                    .and_then(|l| match l {
                        Some((path, ..)) => store.plugin_uses_of_mesh(&path),
                        None => Ok(vec![]),
                    })
                    .and_then(|u| {
                        let ids: Vec<u32> = u.iter().map(|x| x.formid).collect();
                        Ok((store.inert_of(&ids)?, u))
                    });
                (self.plugin_inert, self.plugin_uses) = match uses {
                    Ok(x) => x,
                    Err(e) => {
                        self.error = Some(db_err(e));
                        (HashMap::new(), vec![])
                    }
                };
```
- `load_file_refs`: replace from `let r = store.plugin_missing_files()…` to the end of the `self.file_refs = …;` statement with:
```rust
        let r = store
            .plugin_missing_with_inert()
            .and_then(|(missing, inert)| {
                Ok((
                    FileRefs {
                        missing,
                        unused: store.unused_textures()?,
                    },
                    inert,
                ))
            });
        (self.file_refs, self.missing_inert) = match r {
            Ok((r, i)) => (Some(r), i),
            Err(e) => {
                self.error = Some(db_err(e));
                (Some(FileRefs::default()), vec![])
            }
        };
```
- Add after `refs_lines`:
```rust
    /// Missing lines of the paths only inert records name, for "Show inert" (filtering ruling R12).
    pub fn refs_inert_lines(&self) -> Vec<String> {
        // shortcut: rebuilt each frame while shown; cache like `refs_lines` if inert lists get large
        let r = FileRefs {
            missing: self.missing_inert.clone(),
            unused: vec![],
        };
        file_refs_lines(&r, false, &self.refs_filter)
    }
```
- [ ] **Step 4: Implement the view** (`crates/gui/src/ui/meshes.rs`):
- In `detail`, change `plugin_overrides(ui, &m.plugin_uses);` to `plugin_overrides(ui, m);`.
- Replace `fn plugin_overrides` with:
```rust
/// Winning plugin records that use this mesh and the shapes they re-texture (esp spec §4.2). Inert records (filtering
/// spec §3) only behind "Show inert", dimmed, with their reason. Information only.
fn plugin_overrides(ui: &mut egui::Ui, m: &mut MeshesModel) {
    let (uses, inert, show) = (&m.plugin_uses, &m.plugin_inert, &mut m.show_inert_uses);
    let n_inert = uses.iter().filter(|u| inert.contains_key(&u.formid)).count();
    egui::CollapsingHeader::new(format!("Plugin overrides ({})", uses.len() - n_inert))
        .id_salt("meshes-plugin-overrides")
        .show(ui, |ui| {
            if n_inert > 0 {
                ui.checkbox(&mut *show, format!("Show inert ({n_inert})"));
            }
            let shown: Vec<&MeshUse> = uses
                .iter()
                .filter(|u| *show || !inert.contains_key(&u.formid))
                .collect();
            egui::ScrollArea::vertical()
                .id_salt("meshes-plugin-overrides-scroll")
                .max_height(240.0)
                .show(ui, |ui| {
                    if shown.is_empty() {
                        ui.label("No plugin record of the latest scan uses this mesh.");
                    }
                    for (i, u) in shown.iter().take(MAX_LISTED).enumerate() {
                        let text = format!(
                            "{}{}",
                            record_label(u.edid.as_deref(), u.formid, &u.sig, &u.plugin),
                            overridden_text(u.n_overrides)
                        );
                        match inert.get(&u.formid) {
                            Some(why) => {
                                ui.weak(format!("{text} · {why}"));
                            }
                            None => {
                                ui.label(text);
                            }
                        }
                        ui.indent(("plugin-swaps", i), |ui| {
                            for s in &u.swaps {
                                // formid 0 = the TXST's master is not in the load order
                                let target = if s.txst_formid == 0 {
                                    "unresolved TXST".to_string()
                                } else {
                                    format!(
                                        "{} [{:08X}]",
                                        s.txst_edid.as_deref().unwrap_or("(no EDID)"),
                                        s.txst_formid
                                    )
                                };
                                ui.label(format!("{} → {target}", s.shape_name));
                            }
                        });
                    }
                    if shown.len() > MAX_LISTED {
                        ui.label(format!("… and {} more", shown.len() - MAX_LISTED));
                    }
                });
        });
}
```
- In `fn file_refs`: at the top, after the `let (missing, unused) = …;` statement, add:
```rust
    let inert: Vec<String> = if !m.refs_unused && m.show_inert_refs {
        m.refs_inert_lines()
    } else {
        vec![]
    };
```
change the Copy handler to:
```rust
        if ui.button("Copy").clicked() {
            let mut text = m.refs_lines().join("\n");
            if !inert.is_empty() {
                text.push('\n');
                text.push_str(&inert.join("\n"));
            }
            ui.ctx().copy_text(text);
        }
```
after the `PLUGINS_NOT_READ_NOTE` block add:
```rust
    if !m.refs_unused && !m.missing_inert.is_empty() {
        let n = m.missing_inert.len();
        ui.checkbox(&mut m.show_inert_refs, format!("Show inert ({n})"));
    }
```
change `if n == 0 {` to `if n == 0 && inert.is_empty() {`, and inside the scroll closure, after the `if n > MAX_REF_ROWS { … }` block, add:
```rust
            for l in inert.iter().take(MAX_REF_ROWS) {
                ui.add(egui::Label::new(egui::RichText::new(l.as_str()).weak()).wrap());
            }
```
- [ ] **Step 5: Run.** `cargo test -p sdc-gui --test meshes_view` → PASS (old tests unchanged: "Plugin overrides (1)/(0)/(205)", the file-refs tests). `cargo clippy -p sdc-gui --all-targets -- -D warnings` → clean.
- [ ] **Step 6: Commit** `git add crates/gui/src/model/meshes.rs crates/gui/src/ui/meshes.rs crates/gui/tests/meshes_view.rs && git commit -m "feat(gui): hide inert plugin records in Plugin overrides and Missing, with a Show inert toggle"`

---

### Task 15: GUI — set editor "Used by plugins"

**Files:**
- Modify: `crates/gui/src/model/plugin_refs.rs` (`SetPluginUse.inert`, `SetPluginRefs.show_inert`, `query`)
- Modify: `crates/gui/src/ui/plugin_refs.rs` (`set_section`)
- Modify: `crates/gui/tests/set_plugin_refs.rs` (the existing `SetPluginUse` literal gains `inert`; append a test)

**Interfaces:**
- Consumes: `Store::inert_of`. Produces: `SetPluginUse.inert: HashMap<u32, &'static str>` (TXST and alt-user FormIDs) and `SetPluginRefs.show_inert: bool`.

- [ ] **Step 1: Write the failing test.** In `used_by_plugins_lists_txsts_and_alternate_texture_users`, add `inert: Default::default(),` after `truncated: false,`. Append:
```rust
/// Filtering spec §3 / ruling R12: an inert alternate-texture user is hidden until "Show inert (N)", then dimmed with
/// its reason.
#[test]
fn used_by_plugins_hides_inert_records_until_asked() {
    use modscan::plugin_store::{AltUse, ShapeSwap, TxstUse};
    use sdc_gui::model::plugin_refs::{set_key, SetPluginUse};
    use sdc_gui::model::sets::set_views;
    let d = tempfile::tempdir().unwrap();
    let mut model = pbr_opened(d.path());
    let v = set_views(&model)
        .into_iter()
        .find(|v| set_key(v).is_some())
        .unwrap();
    model.plugin_refs.cached = Some((
        set_key(&v).unwrap(),
        Ok(SetPluginUse {
            txsts: vec![TxstUse {
                formid: 0x0001_2345,
                edid: Some("RockTXST".into()),
                plugin: "Skyrim.esm".into(),
                slot: 0,
                path: "textures/landscape/rock01.dds".into(),
                n_overrides: 1,
            }],
            alts: vec![AltUse {
                formid: 0x0000_0DEF,
                edid: Some("RockStat".into()),
                sig: "STAT".into(),
                plugin: "Skyrim.esm".into(),
                model_slot: 0,
                swap: ShapeSwap {
                    shape_name: "Shape0".into(),
                    txst_formid: 0x0001_2345,
                    txst_edid: Some("RockTXST".into()),
                    index: 0,
                },
            }],
            truncated: false,
            inert: [(0x0000_0DEFu32, "not placed")].into_iter().collect(),
        }),
    ));
    model.select_set(&v.key);
    let mut h = tall(model);
    h.run();
    h.get_by_label("Used by plugins").click();
    h.run();
    h.get_by_label("RockTXST [00012345] TXST — Skyrim.esm · slot 0: textures/landscape/rock01.dds · overridden 1×");
    assert!(h.query_by_label_contains("RockStat").is_none());
    h.get_by_label("Show inert (1)").click();
    h.run();
    h.get_by_label("RockStat [00000DEF] STAT — Skyrim.esm · Shape0 → RockTXST · not placed");
}
```
- [ ] **Step 2: Run.** `cargo test -p sdc-gui --test set_plugin_refs` → Expected: FAIL to compile (`no field inert`).
- [ ] **Step 3: Implement.** `crates/gui/src/model/plugin_refs.rs`: add `use std::collections::HashMap;`. Add to `SetPluginUse` `/// Inert reasons (filtering spec §3) of the listed TXSTs and alternate-texture users that are inert.` `pub inert: HashMap<u32, &'static str>,`. Add to `SetPluginRefs` `/// "Show inert" in the section.` `pub show_inert: bool,`. In `query`, replace the tail from `let ids: Vec<u32> = …` with:
```rust
    let mut ids: Vec<u32> = txsts.iter().map(|t| t.formid).collect();
    let alts = store.alt_users_of(&ids).map_err(|e| e.to_string())?;
    ids.extend(alts.iter().map(|a| a.formid));
    let inert = store.inert_of(&ids).map_err(|e| e.to_string())?;
    Ok(SetPluginUse {
        txsts,
        alts,
        truncated,
        inert,
    })
```
`crates/gui/src/ui/plugin_refs.rs`, `set_section`: before `match model.plugin_refs.get(…)` add `let mut show = model.plugin_refs.show_inert;`; after the `match` (still inside the closure) add `model.plugin_refs.show_inert = show;`. Replace the `Ok(u) => { … }` arm with:
```rust
                Ok(u) => {
                    let n_inert = u.txsts.iter().filter(|t| u.inert.contains_key(&t.formid)).count()
                        + u.alts.iter().filter(|a| u.inert.contains_key(&a.formid)).count();
                    if n_inert > 0 {
                        ui.checkbox(&mut show, format!("Show inert ({n_inert})"));
                    }
                    if u.txsts.is_empty() {
                        ui.label("No texture set of the latest scan matches this set.");
                    }
                    for t in &u.txsts {
                        let text = format!(
                            "{} · slot {}: {}{}",
                            record_label(t.edid.as_deref(), t.formid, "TXST", &t.plugin),
                            t.slot,
                            t.path,
                            overridden_text(t.n_overrides)
                        );
                        match u.inert.get(&t.formid) {
                            Some(why) if show => {
                                ui.weak(format!("{text} · {why}"));
                            }
                            Some(_) => {}
                            None => {
                                ui.label(text);
                            }
                        }
                    }
                    if u.truncated {
                        ui.weak(format!("… only the first {LIMIT} are listed"));
                    }
                    let alts: Vec<_> = u
                        .alts
                        .iter()
                        .filter(|a| show || !u.inert.contains_key(&a.formid))
                        .collect();
                    if !alts.is_empty() {
                        ui.label(egui::RichText::new("Alternate textures using them").strong());
                        for a in alts {
                            let text = format!(
                                "{} · {} → {}",
                                record_label(a.edid.as_deref(), a.formid, &a.sig, &a.plugin),
                                a.swap.shape_name,
                                a.swap.txst_edid.as_deref().unwrap_or("(no EDID)")
                            );
                            match u.inert.get(&a.formid) {
                                Some(why) => {
                                    ui.weak(format!("{text} · {why}"));
                                }
                                None => {
                                    ui.label(text);
                                }
                            }
                        }
                    }
                }
```
- [ ] **Step 4: Run.** `cargo test -p sdc-gui --test set_plugin_refs` → PASS. `cargo clippy -p sdc-gui --all-targets -- -D warnings` → clean.
- [ ] **Step 5: Commit** `git add crates/gui/src/model/plugin_refs.rs crates/gui/src/ui/plugin_refs.rs crates/gui/tests/set_plugin_refs.rs && git commit -m "feat(gui): hide inert records in the set editor's Used by plugins, with a Show inert toggle"`

---

### Task 16: Final verification

- [ ] **Step 1:** `cargo test -p vfs -p esp -p modscan -p sdc-gui` → PASS. `cargo clippy -p vfs -p esp -p modscan -p sdc-gui --all-targets -- -D warnings` → clean. `cargo fmt -p vfs -p esp -p modscan -p sdc-gui -- --check` → clean.
- [ ] **Step 2:** `git diff --exit-code 9cb9954 -- Cargo.lock Cargo.toml crates/engine crates/cli crates/encoders-sys crates/gui/src/pgp crates/gui/src/model/sets.rs crates/gui/src/model/pbr_export.rs schema docs/DECISIONS.md` → no output (Global Constraints). `git diff --stat 9cb9954 -- crates/modscan/tests/common/mod.rs` → only additions.
- [ ] **Step 3:** The timing protocol plus `owner_load_order`; fill row "Final". Compare with the targets: cold ≤ 20 s, warm no slower than the baseline and < 3 s if the walk allows. If a target is missed, say so in the report, with the dominant phase.
- [ ] **Step 4:** Manual GUI look (owner, optional): Meshes view → a mesh used by an unplaced STAT shows "Show inert (1)"; Plugin file references → Missing shows the toggle when inert-only paths exist.
- [ ] **Step 5:** Hand off for the Opus final branch review; merging into nif/main needs the owner's OK.

## Verification (filled during execution)

| Row | Cold elapsed | Warm elapsed | Cold phase_ms (origins/walk/parse/write/resolve/textures/plugins) | Cold sub_ms | Warm phase_ms | Kept? |
|---|---|---|---|---|---|---|
| Spec baseline (9cb9954, 2026-10-13) | 43.4 s | not measured | 481/2311/21881/12107/556/700/4981 | — | — | — |
| Task 1 baseline | | | | | | n/a |
| Task 2 BSA reads | | | | | | |
| Task 3 writer thread | | | | | | |
| Task 4 cold indexes | | | | | | |
| Task 5 interning | | | | | | |
| Task 6 pragmas | | | | | | |
| Task 7a plugin interning | | | | | | |
| Task 7b plugin overlap | | | | | | |
| Task 8 derived once | | | | | | |
| Task 10 placed refs | | | | | | |
| Task 12 inert (owner_load_order cold/warm) | | | | | | |
| Final | | | | | | |

Notes (counts from Task 1: derived copies, BSA copies, shapes, paths, texsets; WAL size and checkpoint ms; inert counts by reason from Task 12):

## Self-review (done at writing time)

- **Spec coverage:** §2.1 → Task 1; §2.2-2.8 → Tasks 2-8 in spec order; "results must not change" → Task 1 dump test, re-run in every speed task; §3 reached rules: TXST (alt textures, LTEX/HDPT TNAM, NPC_ FTST, RACE, ARMA NAM0/1, IPCT, placed) → Tasks 9, 12; placeables → Tasks 10-12; GRAS via LTEX GNAM → Tasks 9, 12; items never inert, ARMA → R8, Task 12 test (Sword, Arma); HDPT Playable and extra parts → Tasks 9, 12; other types reached → Task 12 (Guard, Imp, Dirt not inert); placed-reference pass cached per plugin, cold cost measured → Tasks 10, 11 (the `placed` table is in `PLUGIN_TABLES`, so it is cached and dropped with its plugin); "ignored everywhere" → Missing/Unused Task 13, Plugin overrides Task 14, Used by plugins Task 15; GUI toggle, dimmed, reasons → Tasks 14-15; §4 out of scope respected; §5 → Global Constraints, Task 16 Step 2; schema 4→5 → Task 11.
- **Placeholders:** none. Every code step carries its code. Measurement rows are data to fill, not plan gaps.
- **Type consistency:** `end_scan`/`begin_scan`/`cold` (T4) are used by T5, T6, T8; `Interner`/`intern_path`/`Store.intern` (T5) by T7; `Payload.hdpt_flags` (T9) by T11; `PluginFacts.placed` (T10) by T11; `rec_inert` (T11) by T12-13; `inert_of`, `plugin_missing_with_inert`, `inert_reason` (T13) by T14-15; `Report.sub_ms` and `PluginStats.sub_ms` (T1) by T3, T7.
- **Review Focus:** five lines, each with its test placed in the owning task (T3, T5, T6, T10, T12).

## Controller rulings after drafting (2026-10-13)

- Base moves to nif/main ee36005 (adds only a Cargo.lock refresh: esp 0.7.0 after main's version bump). Cargo.lock must otherwise stay unchanged.
- R8 finding is fixed in this round, inside Task 9: in Skyrim the ARMO MODL subrecord is an ARMA FormID (one per armature, after INDX), not a mesh path; ARMO world models are MOD2/MOD4. Read ARMO MODL as ARMA references (role MODL in txst_ref-style link table or a dedicated column), drop the bogus model_ref rows, update the existing test that pins the old reading. ARMA stays never-inert (R8 stands).


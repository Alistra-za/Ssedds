# Plugin scanner (esp) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** sdc reads the active MO2 load order's plugins quickly. It lists which plugin records use a texture set or a mesh, and which files plugins name that are missing or used by nothing. It shows these in the set editor and on the Meshes page.

**Architecture:** A new pure crate `crates/esp` turns one plugin's bytes into `PluginFacts`. It walks the TES4 header and only the wanted top-level groups, inflates compressed records with `flate2`, and canonicalises paths with the `nif::path` rules so they join the NIF scan's `path` table. `crates/modscan` finds each active plugin through the MO2 origins and parses changed plugins in parallel. It stores their rows with *local* FormIDs (the cache). Whenever the load order or a plugin changed, it recomputes load-order FormIDs and winners in SQL. All of this runs inside the existing scan worker after the NIF/texture pass. `crates/gui` reads the store into four information-only sections.

**Tech Stack:** Rust workspace; `flate2` (the only new third-party edge, already in `Cargo.lock`); `rusqlite` (bundled SQLite, JSON1 for `json_each`); `rayon`; egui + egui_kittest.

**Spec:** `F:\ai\dds\nifparseref\plan\esp\2026-10-12-esp-scanner-design.md` (approved 2026-10-12). Read it with this plan. Code base: `nif/main @ d45facf`.

**Layout sources.** UESP (`en.uesp.net`, `en.m.uesp.net`) answered HTTP 403 to every fetch on 2026-10-12. Every subrecord layout below is therefore cited from xEdit `TES5Edit/TES5Edit` branch `dev-4.1.6`: `Core/wbDefinitionsTES5.pas` (TES5) and `Core/wbDefinitionsCommon.pas` (shared). One item is not verified there: the `wbArrayS(…, -1)` count prefix width. Task 2 says so, and its fixture pins the reading chosen.

## Global Constraints

- Branch `nif/f-esp`, created from `nif/main` (`d45facf`), in its own worktree `F:\ai\dds\converter-nif-esp`. Read `F:\ai\dds\nifparseref\plan\CONCURRENCY.md` first. Never touch `F:\ai\dds\converter`. Never push. Never merge into `main`.
- Shared files are add-only, in small separate commits: `Cargo.toml`, `Cargo.lock`, `crates/gui/Cargo.toml`, `crates/gui/src/ui/mod.rs`, `crates/gui/src/model/mod.rs`, `crates/gui/src/app.rs`, `crates/gui/src/settings.rs`. This plan touches `Cargo.toml` and `Cargo.lock` (Tasks 1 and 3) and `ui/mod.rs`, `model/mod.rs` and `app.rs` (Task 8). It does not touch `crates/gui/Cargo.toml` or `settings.rs`.
- Off-limits (read pub items only): `crates/engine/`, `crates/cli/`, `crates/encoders-sys/`, `crates/gui/src/pgp/`, `crates/gui/src/sets_doc.rs`, `crates/gui/src/model/sets.rs`, `crates/gui/src/model/pbr_export.rs`, `schema/`, `docs/DECISIONS.md`, golden and `gui_repro` tests. The set editor reads `model::sets::SetView` (pub) only. New GUI model code goes in the new file `crates/gui/src/model/plugin_refs.rs`.
- Owner data (`C:\_mount\moddingvol\Acherus`, `G:\`) is read-only. Nothing is written next to plugins. Tests never read owner data, except `#[ignore]` tests that take the instance root from `SDC_ACHERUS`. Tests never read the real `%LOCALAPPDATA%\sdc\scan.db`: every test store is in a `tempfile` dir, and the GUI tests use a `None` store path.
- Dependencies: the only new third-party edge is `flate2` (workspace dep `flate2 = "1"`, default features, already locked as 1.1.10 with `miniz_oxide 0.9.1`). `esp` also depends on the in-workspace crate `nif`, and `modscan` on `esp`. After each `Cargo.toml` change, `git diff Cargo.lock` may show only the new `esp` package block and the new dependency-name lines (`"flate2"`, `"nif"` under `esp`; `"esp"` under `modscan`). If any other line changes (a version, a new third-party package), stop and report.
- Gates before every commit: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`. Commit messages end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. `git add` explicit paths only.
- Paths in this plan are relative to the worktree root. Edits are anchored by quoted text. Line numbers are those at `d45facf`; they move as earlier tasks land.
- Information only, everywhere: there is no delete/move action, and no PGPatcher warning for alternate-texture swaps (spec §1).
- Never run `python -` with stdin. No Python is needed in this plan.
- Review routing (owner): per-task reviews run on Codex gpt-6-astra (low/medium/high by risk). Opus is used only for the final whole-branch review.

## Review Focus

1. **A full plugin that references a record of a light (ESL) master must map it into FE space, not `LL xxxxxx`.** For example, `F.esp`'s LTEX `TNAM` points at `L.esp`'s record: the mapping must use the *target* plugin's light index and the 0xFFF mask. Pinned by Task 4 `formids_follow_the_load_order_full_and_light` (`FTex` → `FE000801`).
2. **Reordering plugins without changing any file shifts the FormIDs and re-parses nothing.** Pinned by Task 4 `unchanged_plugins_are_cached_and_a_reorder_remaps` (`cached == n`, `ZStat`/`LStat` swap light indices).
3. **A plugin that could not be read (locked by another program) is read again on the next scan.** It must not be cached as "no records". Pinned by Task 4 `a_plugin_whose_read_failed_is_read_again` (`size` stored as NULL never matches).
4. **A Folder scan between two MO2 scans hides the plugin sections, and the next MO2 scan restores them from cache without re-parsing.** Pinned by Task 6 `a_folder_scan_hides_plugin_rows_and_the_next_mo2_scan_restores_them`.
5. **No false Missing/Unused entries from overridden records or PGPatcher output.** A path that only an overridden record copy names is not "missing". `textures/pbr/…` files (PGPatcher output, referenced by no NIF or plugin by design) are not "unused". Pinned by Task 6 `missing_and_unused_lists`.

Accepted limits (not tested; listed for the reviewer):
- **Unused** still lists textures that only unread sources name: LOD, terrain, interface, sky cubemaps, engine-hardcoded paths, and paths in records outside spec §3 (ARMO `ICON`, NPC_ tint layers, etc.). It also lists vanilla BSA textures. The panel's note says so.
- ARMA `NAM2`/`NAM3` are FormLists (`FLST`), not TXSTs (xEdit `wbFormIDCK(NAM2, 'Male Skin Texture Swap List', [FLST, NULL])`). They are not read, and FLST contents are not followed.
- Records whose master is missing keep their raw FormIDs (spec §5). They are left out of `rec_winner` (`owner_ok = 0`), so they never win over a real record that has the same number.
- Load order = `plugins.txt` order as MO2 writes it, after the base masters and `Skyrim.ccc` entries; there is no ESM-first re-sort. A master that loads after its dependent is still mapped.
- WTHR cloud layers: all 32 (`00TX`…`@0TX`, `A0TX`…`O0TX`) are read, which is more than the spec's "00TX–L0TX".
- The set editor's "Used by plugins" reads once per key and on "Refresh", not after every scan.
- Schema 3 → 4 rebuilds every existing `scan.db`: the first scan after this branch is cold for NIFs as well as plugins.
- The cold < 10 s / warm < 1 s targets are asserted only by the `#[ignore]` owner test (Task 5, run in Task 9); no CI test pins them.

## Rulings on the spec (the plan argues from these)

- **R1 Cache vs FormID.** The rows keep each record's *local* FormID (`local`; the master index is clamped to the plugin). That is the per-plugin cache of spec §2.2. The load-order `formid` columns and `rec_winner` are recomputed in SQL by `resolve_plugins` only when a plugin was parsed or removed, or when the load order changed. The warm path is stat + compare.
- **R2 Light vs full index.** A FormID is mapped through the master it names: full → `LL << 24 | x & 0xFFFFFF`, light → `0xFE000000 | LLL << 12 | x & 0xFFF`. LL counts the preceding *full* plugins and LLL the preceding *light* ones. This applies to every FormID-valued field (alt-texture TXST, `TNAM`, `FTST`, `NAM0`/`NAM1`, `DFTM`/`DFTF`/`FTSM`/`FTSF`). FormID 0 is "none" and is dropped.
- **R3 ARMA:** only `NAM0`/`NAM1` → TXST (see the accepted limits).
- **R4 RACE:** `DFTM`/`DFTF`/`FTSM`/`FTSF` → TXST. Every `ANAM`/`MODL` string: `.dds` → texture reference, anything else → model reference (slot 0). `TINT` → texture reference. `WNAM` (→ ARMO) is not read. `model_ref` has no primary key (repeated `MODL`).
- **R5** `DMDL` + `DMDS` are read as model slot 6. Alternate-texture lists attach to the most recent model with the *same slot* (`MODS`→`MODL`, `MO2S`→`MOD2` …).
- **R6 "Scan log"** = `Report.phase_ms` gets `("plugins", ms)`, and `Report.plugins` holds `PluginStats { n, cached, records, ms, problems }`. The GUI also emits a `perf_log` event `plugins` (`n`, `cached`, `records`, `ms`) when a scan finishes.
- **R7 Folder source:** no plugin pass. `Store::unload_plugins` clears `plugin.lo_index` and empties `rec_winner` (rows stay as cache). The next MO2 scan re-resolves without re-parsing.
- **R8 Unused** leaves out `textures/pbr/`. Missing and Unused both check only `textures/**.dds` and `meshes/**.nif`, skipping FaceGen, which is the shape of what the NIF walk indexes. Missing uses winning records only. Unused is conservative: any NIF copy of the run and any record copy of an active plugin counts as a use.
- **R9 Set editor key:** "TXSTs whose slot paths match the set's files" means PGPatcher's own match rule for the set. The entry is the set's settings with `match_diffuse` defaulting to the derived match, and it goes through `set_preview::tex_key` (pub, this team's file). `Store::txsts_matching` reuses the `nifs_using_texture` prefilter (refactored into `tex_query`).
- **R10 Panel 4 location:** Meshes page bar, a collapsible "Plugin file references" under "Scan details". It loads when opened.
- **R11 Normalisation** happens in `esp` (spec §2.1), with `nif::path::canon_texture` and a new `nif::path::canon_model` (adds `meshes/`). The path form is the store's: lower case, `/`, `textures/…` / `meshes/…`. The spec's "`\` separators" is read as "the NIF scan's form", which uses `/`.
- **R12 `Payload`** is a struct with four parts (`txst`, `models`, `txst_refs`, `textures`), not an enum: spec §2.1 allows "combinations". Roles are the subrecord signatures (`TNAM`, `FTST`, `ICON`, `00TX`…).
- **R14 Fixture builder location:** spec §6 names `crates/esp/tests/fixtures/`. The builder lives in `crates/esp/tests/common/mod.rs` (the repo's convention for shared test code, and modscan tests `#[path]`-include it). The fixtures are built in code, so there is no fixtures folder of blobs.
- **R13 Cancel during the plugin pass:** the NIF results are complete, so `run` returns `Ok` with a note. The plugins written so far keep `lo_index = NULL`, which forces a re-resolve on the next scan.

## Task order and dependencies

**1 → 2 → 3 → 4 → 5 → 6 → 7 → 8 → 9.**
- Tasks 1–2: the `esp` crate, testable alone.
- Task 3: load order and the pure FormID helpers.
- Task 4: schema v4, the cache and `scan_plugins`.
- Task 5: wiring into `scan::run_with` (the `Report`, the Folder case, the owner timing test).
- Task 6: the store joins.
- Task 7: the Meshes page sections.
- Task 8: the set editor section.
- Task 9: the owner checklist.

The byte builder `crates/esp/tests/common/mod.rs` (Task 1) is used by the tests of Tasks 2, 4, 5 and 6. modscan tests include it with `#[path = "../../esp/tests/common/mod.rs"] mod b;`.

---

### Task 1: `esp` crate core — header, groups, records, compression, `XXXX`, errors

**Files**
- Create: `crates/esp/Cargo.toml`, `crates/esp/src/lib.rs`, `crates/esp/src/read.rs`, `crates/esp/src/records.rs`
- Create: `crates/esp/tests/common/mod.rs` (the byte builder), `crates/esp/tests/parse.rs`
- Modify (shared, own commit): `Cargo.toml` (`members`, `[workspace.dependencies]`), `Cargo.lock` (cargo writes it)

**Interfaces**
- Produces (crate `esp`):
  - `pub fn parse(name: &str, bytes: &[u8]) -> PluginFacts`
  - `pub const FLAG_ESM: u32 = 0x1; FLAG_LOCALIZED: u32 = 0x80; FLAG_ESL: u32 = 0x200; FLAG_COMPRESSED: u32 = 0x0004_0000;`
  - `pub struct PluginFacts { pub name: String, pub flags: u32, pub masters: Vec<String>, pub records: Vec<Rec>, pub errors: Vec<String> }`, with `fn esm(&self) -> bool`, `fn esl(&self) -> bool` and `fn localized(&self) -> bool`
  - `pub struct Rec { pub local_formid: u32, pub sig: [u8; 4], pub edid: Option<String>, pub payload: Payload }`
  - `pub struct Payload { pub txst: Option<Txst>, pub models: Vec<ModelRef>, pub txst_refs: Vec<([u8; 4], u32)>, pub textures: Vec<([u8; 4], String)> }` (filled in Task 2)
  - `pub struct Txst { pub slots: [Option<String>; 8], pub decal: bool, pub flags: u16 }`, `pub struct ModelRef { pub slot: u8, pub path: String, pub alt: Vec<AltTex> }`, `pub struct AltTex { pub shape_name: String, pub txst_local_formid: u32, pub index: i32 }`
  - Error strings: `"offset 0x…: <message>"`. The messages tests rely on are `not a plugin (no TES4 header)`, `runs past the end of the file`, `runs past its group`, `<SIG> <FORMID:08X>: bad zlib stream: …`, and `subrecord at +0x… runs past the record`.
  - Test builder (`crates/esp/tests/common/mod.rs`): `sub`, `zstr`, `edid`, `form`, `alt`, `rec_raw`, `rec`, `grup`, `top` and `plugin`, with the signatures shown in Step 1.

- [ ] **Step 1: Write the builder and the failing tests**

`crates/esp/tests/common/mod.rs`:

```rust
//! Plugin byte builder for tests (esp spec §6: fixtures are built in code, never binary blobs). The modscan tests
//! include this file with `#[path]`.
#![allow(dead_code)]

/// A subrecord. Data over 0xFFFF bytes is preceded by an `XXXX` subrecord carrying its u32 size, and its own u16
/// size is 0.
pub fn sub(sig: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut v = vec![];
    if data.len() > 0xFFFF {
        v.extend_from_slice(b"XXXX");
        v.extend_from_slice(&4u16.to_le_bytes());
        v.extend_from_slice(&(data.len() as u32).to_le_bytes());
        v.extend_from_slice(sig);
        v.extend_from_slice(&0u16.to_le_bytes());
    } else {
        v.extend_from_slice(sig);
        v.extend_from_slice(&(data.len() as u16).to_le_bytes());
    }
    v.extend_from_slice(data);
    v
}

/// A NUL-terminated string.
pub fn zstr(s: &str) -> Vec<u8> {
    let mut v = s.as_bytes().to_vec();
    v.push(0);
    v
}

pub fn edid(s: &str) -> Vec<u8> {
    sub(b"EDID", &zstr(s))
}

/// A FormID subrecord.
pub fn form(sig: &[u8; 4], id: u32) -> Vec<u8> {
    sub(sig, &id.to_le_bytes())
}

/// Alternate-texture data (`MODS`, `MO2S`…): u32 count, then per entry a u32 name length, the name (no terminator),
/// the TXST FormID (u32) and the 3D index (s32).
pub fn alt(entries: &[(&str, u32, i32)]) -> Vec<u8> {
    let mut v = (entries.len() as u32).to_le_bytes().to_vec();
    for (name, txst, index) in entries {
        v.extend_from_slice(&(name.len() as u32).to_le_bytes());
        v.extend_from_slice(name.as_bytes());
        v.extend_from_slice(&txst.to_le_bytes());
        v.extend_from_slice(&index.to_le_bytes());
    }
    v
}

/// A record with `body` as stored (the caller compresses it when `flags` has 0x40000).
pub fn rec_raw(sig: &[u8; 4], formid: u32, flags: u32, body: &[u8]) -> Vec<u8> {
    let mut v = sig.to_vec();
    v.extend_from_slice(&(body.len() as u32).to_le_bytes());
    v.extend_from_slice(&flags.to_le_bytes());
    v.extend_from_slice(&formid.to_le_bytes());
    v.extend_from_slice(&[0u8; 4]); // version control info
    v.extend_from_slice(&44u16.to_le_bytes()); // form version (SSE)
    v.extend_from_slice(&[0u8; 2]);
    v.extend_from_slice(body);
    v
}

pub fn rec(sig: &[u8; 4], formid: u32, subs: &[Vec<u8>]) -> Vec<u8> {
    rec_raw(sig, formid, 0, &subs.concat())
}

/// A group: 24-byte header (size includes the header), then `children` as given.
pub fn grup(label: &[u8; 4], ty: u32, children: &[Vec<u8>]) -> Vec<u8> {
    let body = children.concat();
    let mut v = b"GRUP".to_vec();
    v.extend_from_slice(&((24 + body.len()) as u32).to_le_bytes());
    v.extend_from_slice(label);
    v.extend_from_slice(&ty.to_le_bytes());
    v.extend_from_slice(&[0u8; 8]);
    v.extend_from_slice(&body);
    v
}

/// A top-level group of record type `label`.
pub fn top(label: &[u8; 4], records: &[Vec<u8>]) -> Vec<u8> {
    grup(label, 0, records)
}

/// TES4 header (`flags`, one `MAST` + `DATA` per master), then `groups`.
pub fn plugin(flags: u32, masters: &[&str], groups: &[Vec<u8>]) -> Vec<u8> {
    let mut subs = vec![sub(b"HEDR", &[0u8; 12])];
    for m in masters {
        subs.push(sub(b"MAST", &zstr(m)));
        subs.push(sub(b"DATA", &[0u8; 8]));
    }
    let mut v = rec_raw(b"TES4", 0, flags, &subs.concat());
    for g in groups {
        v.extend_from_slice(g);
    }
    v
}
```

`crates/esp/tests/parse.rs`:

```rust
//! esp spec §6: the reader core on plugins built in code (`tests/common`).
mod common;
use common::*;
use esp::{parse, FLAG_COMPRESSED, FLAG_ESL, FLAG_ESM, FLAG_LOCALIZED};
use std::io::Write;

/// A compressed record body as stored: u32 decompressed size, then the zlib stream.
fn compressed(body: &[u8]) -> Vec<u8> {
    let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    e.write_all(body).unwrap();
    let mut out = (body.len() as u32).to_le_bytes().to_vec();
    out.extend(e.finish().unwrap());
    out
}

#[test]
fn tes4_header_flags_and_masters() {
    let f = parse(
        "X.esp",
        &plugin(FLAG_ESM | FLAG_ESL | FLAG_LOCALIZED, &["Skyrim.esm", "Update.esm"], &[]),
    );
    assert_eq!(f.name, "X.esp");
    assert_eq!(f.masters, ["Skyrim.esm", "Update.esm"]);
    assert!(f.esm() && f.esl() && f.localized());
    assert!(f.records.is_empty() && f.errors.is_empty(), "{:?}", f.errors);
    let plain = parse("Y.esp", &plugin(0, &[], &[]));
    assert!(!plain.esm() && !plain.esl() && !plain.localized());
}

#[test]
fn not_a_plugin_is_an_error_not_a_panic() {
    let f = parse("x.esp", b"hello");
    assert_eq!(f.errors, ["offset 0x0: not a plugin (no TES4 header)"]);
    assert!(f.records.is_empty());
}

/// A top-level group of an unwanted type, and a nested group inside a wanted one, are skipped by their sizes: the
/// garbage inside them is never read.
#[test]
fn unwanted_groups_are_skipped_without_reading_them() {
    let cell = grup(b"CELL", 0, &[vec![0xFF; 100]]);
    let nested = grup(&[0u8; 4], 1, &[vec![0xEE; 40]]);
    let f = parse(
        "x.esp",
        &plugin(
            0,
            &[],
            &[cell, top(b"STAT", &[rec(b"STAT", 0x0100_0800, &[edid("Rock")]), nested])],
        ),
    );
    assert!(f.errors.is_empty(), "{:?}", f.errors);
    assert_eq!(f.records.len(), 1);
    assert_eq!(&f.records[0].sig, b"STAT");
    assert_eq!(f.records[0].local_formid, 0x0100_0800, "the FormID as the plugin stores it");
    assert_eq!(f.records[0].edid.as_deref(), Some("Rock"));
}

#[test]
fn compressed_records_are_inflated() {
    let body = edid("Packed");
    let f = parse(
        "x.esp",
        &plugin(
            0,
            &[],
            &[top(b"STAT", &[rec_raw(b"STAT", 0x0100_0801, FLAG_COMPRESSED, &compressed(&body))])],
        ),
    );
    assert!(f.errors.is_empty(), "{:?}", f.errors);
    assert_eq!(f.records[0].edid.as_deref(), Some("Packed"));
}

#[test]
fn a_bad_zlib_stream_is_reported_and_the_next_record_is_read() {
    let mut bad = 100u32.to_le_bytes().to_vec();
    bad.extend_from_slice(b"not zlib at all");
    let f = parse(
        "x.esp",
        &plugin(
            0,
            &[],
            &[top(
                b"STAT",
                &[
                    rec_raw(b"STAT", 0x0100_0801, FLAG_COMPRESSED, &bad),
                    rec(b"STAT", 0x0100_0802, &[edid("After")]),
                ],
            )],
        ),
    );
    assert_eq!(f.errors.len(), 1, "{:?}", f.errors);
    assert!(f.errors[0].contains("STAT 01000801: bad zlib stream"), "{:?}", f.errors);
    assert_eq!(f.records.len(), 1);
    assert_eq!(f.records[0].edid.as_deref(), Some("After"));
}

#[test]
fn xxxx_sets_the_next_subrecord_size() {
    let long = "a".repeat(70_000);
    let f = parse(
        "x.esp",
        &plugin(
            0,
            &[],
            &[top(b"STAT", &[rec(b"STAT", 1, &[edid(&long)]), rec(b"STAT", 2, &[edid("Next")])])],
        ),
    );
    assert!(f.errors.is_empty(), "{:?}", f.errors);
    assert_eq!(f.records[0].edid.as_deref().map(str::len), Some(70_000));
    assert_eq!(f.records[1].edid.as_deref(), Some("Next"));
}

/// Spec §2.1: "the plugin's remaining groups are still walked when the group sizes allow it".
#[test]
fn a_record_running_past_its_group_is_reported_and_the_next_group_is_read() {
    let mut broken = rec(b"STAT", 0x0100_0802, &[edid("Broken")]);
    broken[4..8].copy_from_slice(&1000u32.to_le_bytes());
    let f = parse(
        "x.esp",
        &plugin(
            0,
            &[],
            &[
                top(b"STAT", &[rec(b"STAT", 0x0100_0801, &[edid("Before")]), broken]),
                top(b"TXST", &[rec(b"TXST", 0x0100_0900, &[edid("NextGroup")])]),
            ],
        ),
    );
    assert_eq!(f.errors.len(), 1, "{:?}", f.errors);
    assert!(f.errors[0].contains("runs past its group"), "{:?}", f.errors);
    let edids: Vec<&str> = f.records.iter().map(|r| r.edid.as_deref().unwrap()).collect();
    assert_eq!(edids, ["Before", "NextGroup"]);
}

/// Spec §5: a truncated plugin contributes the records read before the fault.
#[test]
fn a_truncated_file_keeps_the_records_before_the_fault() {
    let mut bytes = plugin(
        0,
        &[],
        &[top(b"STAT", &[rec(b"STAT", 1, &[edid("First")]), rec(b"STAT", 2, &[edid("Second")])])],
    );
    bytes.truncate(bytes.len() - 3);
    let f = parse("x.esp", &bytes);
    assert_eq!(f.records.len(), 1);
    assert_eq!(f.records[0].edid.as_deref(), Some("First"));
    assert!(
        f.errors.iter().any(|e| e.contains("runs past the end of the file")),
        "{:?}",
        f.errors
    );
}

#[test]
fn a_subrecord_running_past_its_record_keeps_what_came_before() {
    let mut body = edid("Kept"); // 6 + 5 bytes: the next subrecord starts at +0xb
    body.extend_from_slice(b"MODL");
    body.extend_from_slice(&50u16.to_le_bytes());
    body.extend_from_slice(b"short");
    let f = parse("x.esp", &plugin(0, &[], &[top(b"STAT", &[rec_raw(b"STAT", 7, 0, &body)])]));
    assert_eq!(f.records[0].edid.as_deref(), Some("Kept"));
    assert_eq!(f.errors.len(), 1, "{:?}", f.errors);
    assert!(f.errors[0].contains("subrecord at +0xb runs past the record"), "{:?}", f.errors);
}
```

- [ ] **Step 2: Add the crate to the workspace**

`Cargo.toml`: in `members`, change `"crates/pgpsim"]` → `"crates/pgpsim", "crates/esp"]`. At the end of `[workspace.dependencies]`, after `crc32fast = "1.5.2"`, add:

```toml
flate2 = "1"
```

`crates/esp/Cargo.toml`:

```toml
[package]
name = "esp"
version.workspace = true
edition.workspace = true
license.workspace = true
publish.workspace = true
rust-version.workspace = true

[dependencies]
flate2.workspace = true
nif = { path = "../nif" }
```

`crates/esp/src/lib.rs` (types only for now, so the tests compile and fail):

```rust
//! Targeted Skyrim SE plugin reader (esp scanner spec §2.1): one plugin's bytes → the facts sdc needs (TXSTs, model
//! paths with alternate textures, TXST references, direct texture paths). Unwanted top-level groups are skipped by
//! size; nothing is ever written. Layouts: xEdit `wbDefinitionsTES5.pas` / `wbDefinitionsCommon.pas` (dev-4.1.6).
mod read;
mod records;

pub use read::parse;

/// TES4 header flags.
pub const FLAG_ESM: u32 = 0x1;
pub const FLAG_LOCALIZED: u32 = 0x80;
pub const FLAG_ESL: u32 = 0x200;
/// Record header flag: the data is a u32 decompressed size followed by a zlib stream.
pub const FLAG_COMPRESSED: u32 = 0x0004_0000;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PluginFacts {
    pub name: String,
    /// TES4 record flags as stored.
    pub flags: u32,
    pub masters: Vec<String>,
    pub records: Vec<Rec>,
    /// `"offset 0x…: message"`, in file order. Records read before a fault are kept.
    pub errors: Vec<String>,
}

impl PluginFacts {
    pub fn esm(&self) -> bool {
        self.flags & FLAG_ESM != 0
    }
    pub fn esl(&self) -> bool {
        self.flags & FLAG_ESL != 0
    }
    pub fn localized(&self) -> bool {
        self.flags & FLAG_LOCALIZED != 0
    }
}

/// One record of a wanted type, as this plugin stores it.
#[derive(Debug, Clone, PartialEq)]
pub struct Rec {
    /// High byte = index into the plugin's masters (== masters.len(): the plugin itself).
    pub local_formid: u32,
    pub sig: [u8; 4],
    pub edid: Option<String>,
    pub payload: Payload,
}

/// What a record carries for sdc (spec §2.1). A record can carry several parts (ARMA: models and TXST references).
/// Paths are canonical (`nif::path`): lower case, `/`, `textures/…` or `meshes/…`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Payload {
    pub txst: Option<Txst>,
    pub models: Vec<ModelRef>,
    /// (role = subrecord signature, local FormID of the TXST); FormID 0 is never stored.
    pub txst_refs: Vec<([u8; 4], u32)>,
    /// (role = subrecord signature, canonical texture path).
    pub textures: Vec<([u8; 4], String)>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Txst {
    /// TX00..TX07.
    pub slots: [Option<String>; 8],
    /// A DODT subrecord is present.
    pub decal: bool,
    /// DNAM (xEdit: itU16 — 0x1 No Specular Map, 0x2 Facegen Textures, 0x4 Has Model Space Normal Map).
    pub flags: u16,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModelRef {
    /// MODL 0, MOD2..MOD5 2..5, DMDL 6.
    pub slot: u8,
    pub path: String,
    pub alt: Vec<AltTex>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AltTex {
    /// The NIF shape name ("3D Name").
    pub shape_name: String,
    pub txst_local_formid: u32,
    /// "3D Index" (informational; shapes are matched by name).
    pub index: i32,
}
```

`crates/esp/src/read.rs` and `crates/esp/src/records.rs`: empty files for now.

Run: `cargo test -p esp`
Expected: compile errors (`unresolved import read::parse`).

- [ ] **Step 3: Implement the walk**

`crates/esp/src/read.rs`:

```rust
//! The walk (spec §2.1): TES4, then top-level groups; a group whose label is not a wanted type is skipped by its
//! size, and so is every nested group inside a wanted one.
use crate::{records, PluginFacts, Rec, FLAG_COMPRESSED};
use std::io::Read;

/// Record and group headers are both 24 bytes.
const HEADER: usize = 24;

pub(crate) fn u16_at(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}

pub(crate) fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

fn sig_at(b: &[u8], o: usize) -> [u8; 4] {
    [b[o], b[o + 1], b[o + 2], b[o + 3]]
}

fn sig_str(s: &[u8; 4]) -> String {
    String::from_utf8_lossy(s).into_owned()
}

/// Calls `f` for every subrecord of a record body (6-byte header; `XXXX` sets the next subrecord's u32 size).
/// `Err(offset)`: the subrecord at that offset (in `data`) runs past the end; the ones before it were delivered.
pub(crate) fn each_sub<'a>(data: &'a [u8], mut f: impl FnMut([u8; 4], &'a [u8])) -> Result<(), usize> {
    let mut p = 0;
    let mut big: Option<usize> = None;
    while p + 6 <= data.len() {
        let sig = sig_at(data, p);
        let n = big.take().unwrap_or(u16_at(data, p + 4) as usize);
        let start = p + 6;
        if start + n > data.len() {
            return Err(p);
        }
        if &sig == b"XXXX" {
            if n < 4 {
                return Err(p);
            }
            big = Some(u32_at(data, start) as usize);
        } else {
            f(sig, &data[start..start + n]);
        }
        p = start + n;
    }
    if p != data.len() {
        return Err(p);
    }
    Ok(())
}

/// One plugin's facts. Never panics on bad input; faults go to `errors` with their file offset.
pub fn parse(name: &str, b: &[u8]) -> PluginFacts {
    let mut f = PluginFacts {
        name: name.to_string(),
        ..Default::default()
    };
    if b.len() < HEADER || &b[0..4] != b"TES4" {
        f.errors.push("offset 0x0: not a plugin (no TES4 header)".into());
        return f;
    }
    let size = u32_at(b, 4) as usize;
    f.flags = u32_at(b, 8);
    let end = HEADER + size;
    if end > b.len() {
        f.errors
            .push(format!("offset 0x0: TES4 header size {size} runs past the end of the file"));
        return f;
    }
    let mut masters = vec![];
    if let Err(at) = each_sub(&b[HEADER..end], |sig, d| {
        if &sig == b"MAST" {
            if let Some(m) = records::zstring(d) {
                masters.push(m);
            }
        }
    }) {
        f.errors.push(format!(
            "offset {:#x}: TES4 subrecord runs past the header",
            HEADER + at
        ));
    }
    f.masters = masters;
    let mut off = end;
    while off < b.len() {
        if off + HEADER > b.len() {
            f.errors
                .push(format!("offset {off:#x}: {} trailing bytes", b.len() - off));
            break;
        }
        let sig = sig_at(b, off);
        let size = u32_at(b, off + 4) as usize;
        if &sig != b"GRUP" {
            f.errors
                .push(format!("offset {off:#x}: {} record outside a group", sig_str(&sig)));
            off = off.saturating_add(HEADER + size);
            continue;
        }
        if size < HEADER {
            f.errors.push(format!(
                "offset {off:#x}: group size {size} is smaller than its header"
            ));
            break;
        }
        let label = sig_at(b, off + 8);
        let wanted = u32_at(b, off + 12) == 0 && records::wanted(&label);
        let gend = off + size;
        if gend > b.len() {
            f.errors.push(format!(
                "offset {off:#x}: group {} runs past the end of the file",
                sig_str(&label)
            ));
            if wanted {
                walk_group(b, off + HEADER, b.len(), &mut f);
            }
            break;
        }
        if wanted {
            walk_group(b, off + HEADER, gend, &mut f);
        }
        off = gend;
    }
    f
}

/// The records of one wanted top-level group, `start..end`. A fault ends this group, never the plugin.
fn walk_group(b: &[u8], start: usize, end: usize, f: &mut PluginFacts) {
    let mut p = start;
    while p < end {
        if p + HEADER > end {
            f.errors
                .push(format!("offset {p:#x}: truncated header runs past its group"));
            return;
        }
        let sig = sig_at(b, p);
        let size = u32_at(b, p + 4) as usize;
        if &sig == b"GRUP" {
            // CELL/WRLD children and any other nested group are not wanted (spec §2.1).
            if size < HEADER || p + size > end {
                f.errors.push(format!(
                    "offset {p:#x}: nested group size {size} runs past its group"
                ));
                return;
            }
            p += size;
            continue;
        }
        let rend = p + HEADER + size;
        if rend > end {
            f.errors.push(format!(
                "offset {p:#x}: {} record size {size} runs past its group",
                sig_str(&sig)
            ));
            return;
        }
        let flags = u32_at(b, p + 8);
        let formid = u32_at(b, p + 12);
        let raw = &b[p + HEADER..rend];
        let inflated;
        let data: &[u8] = if flags & FLAG_COMPRESSED != 0 {
            match inflate(raw) {
                Ok(v) => {
                    inflated = v;
                    &inflated
                }
                Err(e) => {
                    f.errors
                        .push(format!("offset {p:#x}: {} {formid:08X}: {e}", sig_str(&sig)));
                    p = rend;
                    continue;
                }
            }
        } else {
            raw
        };
        let mut rec = Rec {
            local_formid: formid,
            sig,
            edid: None,
            payload: Default::default(),
        };
        if let Err(at) = records::read(&mut rec, data) {
            f.errors.push(format!(
                "offset {p:#x}: {} {formid:08X}: subrecord at +{at:#x} runs past the record",
                sig_str(&sig)
            ));
        }
        f.records.push(rec);
        p = rend;
    }
}

/// u32 decompressed size + zlib stream → the record data.
fn inflate(raw: &[u8]) -> Result<Vec<u8>, String> {
    if raw.len() < 4 {
        return Err("compressed record shorter than its size field".into());
    }
    let want = u32_at(raw, 0) as usize;
    // Cap the reservation: the size field comes from the file.
    let mut out = Vec::with_capacity(want.min(64 << 20));
    flate2::read::ZlibDecoder::new(&raw[4..])
        .read_to_end(&mut out)
        .map_err(|e| format!("bad zlib stream: {e}"))?;
    if out.len() != want {
        return Err(format!(
            "bad zlib stream: {} bytes, header says {want}",
            out.len()
        ));
    }
    Ok(out)
}
```

`crates/esp/src/records.rs` (only the EDID for now; Task 2 adds the payloads):

```rust
//! Which groups are read, and what each record type contributes (spec §3).
use crate::read::each_sub;
use crate::Rec;

/// Top-level groups read (spec §3); every other group is skipped by its size.
const WANTED: [[u8; 4]; 28] = [
    *b"TXST", *b"STAT", *b"MSTT", *b"ACTI", *b"FURN", *b"DOOR", *b"CONT", *b"LIGH", *b"MISC", *b"TREE",
    *b"FLOR", *b"GRAS", *b"ARMA", *b"ARMO", *b"WEAP", *b"AMMO", *b"BOOK", *b"INGR", *b"ALCH", *b"KEYM",
    *b"SLGM", *b"SCRL", *b"LTEX", *b"HDPT", *b"NPC_", *b"RACE", *b"EFSH", *b"WTHR",
];

pub(crate) fn wanted(label: &[u8; 4]) -> bool {
    WANTED.contains(label)
}

/// The bytes up to the first NUL.
pub(crate) fn until_nul(d: &[u8]) -> &[u8] {
    &d[..d.iter().position(|&c| c == 0).unwrap_or(d.len())]
}

/// A zstring, Windows-1252 (EDID and paths are never localized); `None` when empty.
pub(crate) fn zstring(d: &[u8]) -> Option<String> {
    let s = until_nul(d);
    (!s.is_empty()).then(|| nif::text::lossy(s))
}

/// Fills `rec` from its subrecords; subrecords not listed in spec §3 are skipped without allocating.
pub(crate) fn read(rec: &mut Rec, data: &[u8]) -> Result<(), usize> {
    let edid = &mut rec.edid;
    each_sub(data, |sig, d| {
        if &sig == b"EDID" {
            *edid = zstring(d);
        }
    })
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p esp`
Expected: 9 passed (`parse.rs`).

- [ ] **Step 5: Check the lock, gates, commit (two commits)**

Run: `git diff Cargo.lock`
Expected: only a new `[[package]] name = "esp"` block whose `dependencies` are `"flate2"` and `"nif"`. If anything else changed, stop and report.

```powershell
cargo fmt --all; cargo clippy --workspace --all-targets -- -D warnings; cargo test --workspace
git add crates/esp
git commit -m "esp: plugin reader core (TES4, wanted groups by size, zlib, XXXX, fault offsets)" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
git add Cargo.toml Cargo.lock
git commit -m "workspace: add crates/esp and flate2 (already locked via image)" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: record payloads (spec §3) and `canon_model`

**Files**
- Modify: `crates/nif/src/path.rs` (add `canon_model` after `canon_mesh`)
- Modify: `crates/esp/src/records.rs` (payload readers)
- Test: `crates/nif/tests/canon_model.rs` (new), `crates/esp/tests/records.rs` (new)

**Interfaces**
- Consumes: Task 1 `Rec`, `Payload`, `Txst`, `ModelRef`, `AltTex`, `each_sub`, `u32_at`, `until_nul`, `zstring`, and the test builder.
- Produces: `pub fn nif::path::canon_model(raw: &[u8]) -> String`. Filled payloads, per type:

| Record | Subrecord → payload | Source (xEdit dev-4.1.6) |
|---|---|---|
| TXST | `TX00`..`TX07` → `txst.slots[0..8]` (canonical texture); `DODT` present → `decal`; `DNAM` u16 → `flags` | TES5 `wbRecord(TXST…)`: `wbString(TX00,'Diffuse')`…`wbString(TX07,'Backlight Mask/Specular')`, `wbDODT`, `wbInteger(DNAM, 'Flags', itU16, …)` |
| every wanted type except RACE | `MODL`/`MOD2`/`MOD3`/`MOD4`/`MOD5`/`DMDL` → `models` (slot 0/2/3/4/5/6); `MODS`/`MO2S`/`MO3S`/`MO4S`/`MO5S`/`DMDS` → `alt` of the latest model with that slot | Common `wbGenericModel`: `wbString(MODL, 'Model FileName')`, `wbMODS`; TES5 `wbMODS := wbArrayS(MODS, 'Alternate Textures', wbAlternateTexture, -1)` (same for MO2S..MO5S, `wbDMDSs`) |
| alt-texture entry | u32 count; per entry u32 name length + name, u32 TXST FormID, s32 3D index | Common `wbAlternateTexture`: `wbLenString('3D Name')`, `wbFormIDCk('New Texture', [TXST])`, `wbInteger('3D Index', itS32)`. **Not verified:** that `wbArrayS(…, -1)` means a u32 count prefix and that `wbLenString` has a u32 length prefix (`wbInterface.pas` was not readable in full). The `alt` builder and the tests pin this reading. |
| LTEX, HDPT | `TNAM` → `txst_refs` | TES5 `wbFormIDCk(TNAM, 'Texture Set', [TXST]…)` (both) |
| NPC_ | `FTST` → `txst_refs` | TES5 `wbFormIDCk(FTST, 'Head texture', [TXST]…)` |
| ARMA | `NAM0`, `NAM1` → `txst_refs` (`NAM2`/`NAM3` are FLST: not read, R3) | TES5 `wbFormIDCK(NAM0, 'Male Skin Texture', [TXST, NULL])`, `NAM1` female; `NAM2`/`NAM3` `… Swap List', [FLST, NULL]` |
| RACE | `DFTM`/`DFTF`/`FTSM`/`FTSF` → `txst_refs`; `ANAM`/`MODL` → model (slot 0), or texture when `.dds`; `TINT` → texture (R4) | TES5 `wbFormIDCk(DFTM, 'Default Face Texture Male', [TXST, NULL])`, `FTSM`/`FTSF` `'Face Details Texture Set List …'`, `wbString(ANAM, 'Male Skeletal Model')`, `wbString(TINT, 'File Name')` |
| EFSH | `ICON`, `ICO2`, `NAM7`, `NAM8`, `NAM9` → `textures` | TES5 `wbString(ICON, 'Fill Texture')`, `ICO2 'Particle Shader Texture'`, `NAM7 'Holes Texture'`, `NAM8 'Membrane Palette Texture'`, `NAM9 'Particle Palette Texture'` |
| WTHR | the 32 cloud layers → `textures` | Common `wbWeatherCloudTextures`: `wbString(_30_0TX, 'Layer #0')` … `_40_0TX` (`@0TX`, layer 16), `A0TX` … `O0TX` (layer 31): first byte 0x30..=0x4F, then `0TX` |

- [ ] **Step 1: Write the failing tests**

`crates/nif/tests/canon_model.rs`:

```rust
//! esp spec §2.1: plugin model paths join the NIF scan's `path` rows (`meshes/…`, lower case, `/`).
use nif::path::canon_model;

#[test]
fn plugin_model_paths_get_the_meshes_prefix() {
    assert_eq!(canon_model(b"Clutter\\Book.NIF"), "meshes/clutter/book.nif");
    assert_eq!(canon_model(b"meshes\\clutter\\book.nif"), "meshes/clutter/book.nif");
    assert_eq!(canon_model(b"C:\\Games\\Skyrim\\Data\\Meshes\\x.nif"), "meshes/x.nif");
    assert_eq!(canon_model(b"  \\Clutter\\x.nif"), "meshes/clutter/x.nif");
    assert_eq!(canon_model(b""), "");
}
```

`crates/esp/tests/records.rs`:

```rust
//! esp spec §3: one fixture per record type, layouts as cited in the plan (xEdit dev-4.1.6).
mod common;
use common::*;
use esp::{parse, AltTex, ModelRef, Rec};

fn one(group: &[u8; 4], r: Vec<u8>) -> Rec {
    let f = parse("x.esp", &plugin(0, &[], &[top(group, &[r])]));
    assert!(f.errors.is_empty(), "{:?}", f.errors);
    assert_eq!(f.records.len(), 1);
    f.records.into_iter().next().unwrap()
}

#[test]
fn txst_slots_decal_and_flags() {
    let r = one(
        b"TXST",
        rec(
            b"TXST",
            0x0100_0ABC,
            &[
                edid("DirtTXST"),
                sub(b"OBND", &[0; 12]),
                sub(b"TX00", &zstr("Landscape\\Dirt02.dds")),
                sub(b"TX01", &zstr("textures\\Landscape\\Dirt02_N.dds")),
                sub(b"TX07", &zstr("Data\\Textures\\landscape\\dirt02_s.dds")),
                sub(b"DODT", &[0; 36]),
                sub(b"DNAM", &3u16.to_le_bytes()),
            ],
        ),
    );
    assert_eq!(r.edid.as_deref(), Some("DirtTXST"));
    let t = r.payload.txst.unwrap();
    assert_eq!(t.slots[0].as_deref(), Some("textures/landscape/dirt02.dds"));
    assert_eq!(t.slots[1].as_deref(), Some("textures/landscape/dirt02_n.dds"));
    assert_eq!(t.slots[7].as_deref(), Some("textures/landscape/dirt02_s.dds"));
    assert!(t.slots[2..7].iter().all(Option::is_none));
    assert!(t.decal);
    assert_eq!(t.flags, 3);
}

#[test]
fn a_txst_without_dodt_is_not_a_decal() {
    let r = one(b"TXST", rec(b"TXST", 1, &[edid("Plain"), sub(b"TX00", &zstr("a.dds"))]));
    let t = r.payload.txst.unwrap();
    assert!(!t.decal);
    assert_eq!(t.flags, 0);
}

/// Each alternate-texture list belongs to the model with its slot (MO2S → MOD2, MO4S → MOD4).
#[test]
fn models_carry_their_own_alternate_textures() {
    let r = one(
        b"ARMA",
        rec(
            b"ARMA",
            0x0100_0900,
            &[
                edid("BootsAA"),
                sub(b"MOD2", &zstr("Armor\\Boots\\M.nif")),
                sub(b"MO2T", &[0; 12]),
                sub(b"MO2S", &alt(&[("BootsShape", 0x0000_0ABC, 0), ("Strap", 0x0100_0ABD, 2)])),
                sub(b"MOD3", &zstr("Armor\\Boots\\F.nif")),
                sub(b"MOD4", &zstr("meshes\\Armor\\Boots\\M_1st.NIF")),
                sub(b"MO4S", &alt(&[("FirstPerson", 0x0000_0ABE, 1), ("NoTexture", 0, 3)])),
                form(b"NAM0", 0x0000_0D00),
                form(b"NAM1", 0),
                form(b"NAM2", 0x0000_0F00),
            ],
        ),
    );
    assert_eq!(
        r.payload.models,
        vec![
            ModelRef {
                slot: 2,
                path: "meshes/armor/boots/m.nif".into(),
                alt: vec![
                    AltTex { shape_name: "BootsShape".into(), txst_local_formid: 0xABC, index: 0 },
                    AltTex { shape_name: "Strap".into(), txst_local_formid: 0x0100_0ABD, index: 2 },
                ],
            },
            ModelRef { slot: 3, path: "meshes/armor/boots/f.nif".into(), alt: vec![] },
            ModelRef {
                slot: 4,
                path: "meshes/armor/boots/m_1st.nif".into(),
                alt: vec![AltTex { shape_name: "FirstPerson".into(), txst_local_formid: 0xABE, index: 1 }],
            },
        ],
        "an entry with TXST 0 is no swap"
    );
    // NAM1 = 0 is no reference; NAM2/NAM3 are FormLists (xEdit), not read.
    assert_eq!(r.payload.txst_refs, vec![(*b"NAM0", 0x0000_0D00)]);
}

#[test]
fn every_model_record_type_reads_modl_and_mods() {
    for sig in [
        b"STAT", b"MSTT", b"ACTI", b"FURN", b"DOOR", b"CONT", b"LIGH", b"MISC", b"TREE", b"FLOR", b"GRAS",
        b"ARMA", b"ARMO", b"WEAP", b"AMMO", b"BOOK", b"INGR", b"ALCH", b"KEYM", b"SLGM", b"SCRL", b"HDPT",
    ] {
        let r = one(
            sig,
            rec(
                sig,
                0x0100_0800,
                &[edid("M"), sub(b"MODL", &zstr("Clutter\\Thing.nif")), sub(b"MODS", &alt(&[("S", 0xABC, 0)]))],
            ),
        );
        let name = String::from_utf8_lossy(sig);
        assert_eq!(r.payload.models.len(), 1, "{name}");
        assert_eq!(r.payload.models[0].path, "meshes/clutter/thing.nif", "{name}");
        assert_eq!(r.payload.models[0].alt.len(), 1, "{name}");
    }
}

#[test]
fn weapon_scope_and_destruction_models_have_their_own_slots() {
    let r = one(
        b"WEAP",
        rec(
            b"WEAP",
            1,
            &[
                sub(b"MODL", &zstr("Weapons\\Bow.nif")),
                sub(b"MOD3", &zstr("Weapons\\Scope.nif")),
                sub(b"MO3S", &alt(&[("Lens", 0xA0, 0)])),
                sub(b"DMDL", &zstr("Weapons\\Broken.nif")),
                sub(b"DMDS", &alt(&[("Shard", 0xA1, 0)])),
            ],
        ),
    );
    let slots: Vec<(u8, usize)> = r.payload.models.iter().map(|m| (m.slot, m.alt.len())).collect();
    assert_eq!(slots, [(0, 0), (3, 1), (6, 1)]);
}

/// A list with no model of its slot before it has nothing to attach to; ARMO's ICON (inventory art) is not in §3.
#[test]
fn unlisted_subrecords_and_orphan_lists_are_skipped() {
    let r = one(
        b"ARMO",
        rec(
            b"ARMO",
            1,
            &[sub(b"MO2S", &alt(&[("X", 0xA0, 0)])), sub(b"ICON", &zstr("Armor\\Icon.dds"))],
        ),
    );
    assert!(r.payload.models.is_empty() && r.payload.textures.is_empty() && r.payload.txst.is_none());
}

#[test]
fn txst_references_of_ltex_hdpt_and_npc() {
    let l = one(b"LTEX", rec(b"LTEX", 1, &[edid("LDirt"), form(b"TNAM", 0xABC)]));
    assert_eq!(l.payload.txst_refs, vec![(*b"TNAM", 0xABC)]);
    let h = one(
        b"HDPT",
        rec(
            b"HDPT",
            2,
            &[
                edid("Brows"),
                sub(b"MODL", &zstr("Actors\\Character\\Brows.nif")),
                form(b"TNAM", 0xABD),
                sub(b"NAM1", &zstr("Actors\\Character\\Brows.tri")),
            ],
        ),
    );
    assert_eq!(h.payload.txst_refs, vec![(*b"TNAM", 0xABD)]);
    assert_eq!(h.payload.models.len(), 1, "NAM1 .tri parts are not models");
    assert_eq!(h.payload.models[0].path, "meshes/actors/character/brows.nif");
    let n = one(b"NPC_", rec(b"NPC_", 3, &[edid("Guard"), form(b"FTST", 0xABE), form(b"TNAM", 0xFFF)]));
    assert_eq!(n.payload.txst_refs, vec![(*b"FTST", 0xABE)], "TNAM is read on LTEX/HDPT only");
}

#[test]
fn race_face_texture_sets_models_and_tint_masks() {
    let r = one(
        b"RACE",
        rec(
            b"RACE",
            4,
            &[
                edid("NordRace"),
                form(b"WNAM", 0x0000_0D64),
                sub(b"ANAM", &zstr("Actors\\Character\\Character Assets\\skeleton.nif")),
                sub(b"MODL", &zstr("Actors\\Character\\Character Assets\\MaleBody.dds")),
                sub(b"MODL", &zstr("Actors\\Character\\Character Assets\\Head.nif")),
                form(b"DFTM", 0xA01),
                form(b"DFTF", 0xA02),
                form(b"FTSM", 0xA03),
                form(b"FTSF", 0xA04),
                sub(b"TINT", &zstr("Actors\\Character\\Character Assets\\TintMasks\\Cheek.dds")),
            ],
        ),
    );
    assert_eq!(
        r.payload.txst_refs,
        vec![(*b"DFTM", 0xA01), (*b"DFTF", 0xA02), (*b"FTSM", 0xA03), (*b"FTSF", 0xA04)],
        "WNAM is a skin ARMO, not a TXST"
    );
    let models: Vec<&str> = r.payload.models.iter().map(|m| m.path.as_str()).collect();
    assert_eq!(
        models,
        [
            "meshes/actors/character/character assets/skeleton.nif",
            "meshes/actors/character/character assets/head.nif"
        ]
    );
    assert_eq!(
        r.payload.textures,
        vec![
            (*b"MODL", "textures/actors/character/character assets/malebody.dds".to_string()),
            (*b"TINT", "textures/actors/character/character assets/tintmasks/cheek.dds".to_string()),
        ]
    );
}

#[test]
fn effect_shader_texture_paths() {
    let r = one(
        b"EFSH",
        rec(
            b"EFSH",
            5,
            &[
                edid("FireFX"),
                sub(b"ICON", &zstr("Effects\\Fire01.dds")),
                sub(b"ICO2", &zstr("Effects\\Particle.dds")),
                sub(b"NAM7", &zstr("Effects\\Holes.dds")),
                sub(b"NAM8", &zstr("Effects\\Membrane.dds")),
                sub(b"NAM9", &zstr("Effects\\ParticlePal.dds")),
                sub(b"NAM0", &zstr("")),
            ],
        ),
    );
    let roles: Vec<String> = r
        .payload
        .textures
        .iter()
        .map(|(s, _)| String::from_utf8_lossy(s).into_owned())
        .collect();
    assert_eq!(roles, ["ICON", "ICO2", "NAM7", "NAM8", "NAM9"]);
    assert_eq!(r.payload.textures[0].1, "textures/effects/fire01.dds");
}

/// xEdit `wbWeatherCloudTextures`: `_30_0TX` (= "00TX") … `_40_0TX` ("@0TX"), then `A0TX` … `O0TX`: 32 layers.
#[test]
fn weather_cloud_layers() {
    let mut subs = vec![edid("SkyrimClear")];
    let layers: Vec<[u8; 4]> = (0x30u8..=0x4F).map(|c| [c, b'0', b'T', b'X']).collect();
    for (i, s) in layers.iter().enumerate() {
        subs.push(sub(s, &zstr(&format!("Sky\\Cloud{i:02}.dds"))));
    }
    subs.push(sub(b"DNAM", &zstr("Sky\\OldStyleLayer.dds")));
    subs.push(sub(b"MODL", &zstr("Sky\\Aurora.nif")));
    let r = one(b"WTHR", rec(b"WTHR", 6, &subs));
    assert_eq!(r.payload.textures.len(), 32);
    assert_eq!(r.payload.textures[0], (*b"00TX", "textures/sky/cloud00.dds".to_string()));
    assert_eq!(r.payload.textures[16], (*b"@0TX", "textures/sky/cloud16.dds".to_string()));
    assert_eq!(r.payload.textures[31], (*b"O0TX", "textures/sky/cloud31.dds".to_string()));
    assert_eq!(r.payload.models[0].path, "meshes/sky/aurora.nif");
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p esp --test records; cargo test -p nif --test canon_model`
Expected: `canon_model` not found. The `records` tests fail on empty payloads.

- [ ] **Step 3: Implement**

`crates/nif/src/path.rs`, after `pub fn canon_mesh`:

```rust
/// A plugin model path (`MODL`…, relative to `meshes\`) in canonical form: lower case, `/`, starting `meshes/`. An
/// absolute or `data\`-prefixed path is cut back to its last `meshes` component (esp scanner spec §2.1).
pub fn canon_model(raw: &[u8]) -> String {
    let s = slashes_lower(&lossy(raw));
    if s.is_empty() || s.starts_with("meshes/") {
        return s;
    }
    let comps: Vec<&str> = s.split('/').collect();
    match comps.iter().rposition(|c| *c == "meshes") {
        Some(i) => comps[i..].join("/"),
        None => format!("meshes/{s}"),
    }
}
```

`crates/esp/src/records.rs`: change the `use` lines to

```rust
use crate::read::{each_sub, u32_at};
use crate::{AltTex, ModelRef, Payload, Rec, Txst};
use nif::path::{canon_model, canon_texture};
```

and replace `fn read` with:

```rust
/// Model subrecord, its alternate-texture subrecord, and the slot both use (xEdit `wbGenericModel`, `wbMODS`,
/// `wbMO2S`..`wbMO5S`, `wbDMDSs`; DMDL → slot 6, ruling R5).
const MODELS: [([u8; 4], [u8; 4], u8); 6] = [
    (*b"MODL", *b"MODS", 0),
    (*b"MOD2", *b"MO2S", 2),
    (*b"MOD3", *b"MO3S", 3),
    (*b"MOD4", *b"MO4S", 4),
    (*b"MOD5", *b"MO5S", 5),
    (*b"DMDL", *b"DMDS", 6),
];

/// Fills `rec` from its subrecords; subrecords not listed in spec §3 are skipped without allocating.
pub(crate) fn read(rec: &mut Rec, data: &[u8]) -> Result<(), usize> {
    let kind = rec.sig;
    let edid = &mut rec.edid;
    let p = &mut rec.payload;
    each_sub(data, |sig, d| {
        if &sig == b"EDID" {
            *edid = zstring(d);
            return;
        }
        match &kind {
            b"TXST" => txst(p, &sig, d),
            b"LTEX" | b"HDPT" if &sig == b"TNAM" => form_ref(p, sig, d),
            b"NPC_" if &sig == b"FTST" => form_ref(p, sig, d),
            b"ARMA" if matches!(&sig, b"NAM0" | b"NAM1") => form_ref(p, sig, d),
            b"RACE" => race(p, sig, d),
            b"EFSH" if matches!(&sig, b"ICON" | b"ICO2" | b"NAM7" | b"NAM8" | b"NAM9") => texture(p, sig, d),
            b"WTHR" if is_cloud_layer(&sig) => texture(p, sig, d),
            _ => {}
        }
        if &kind != b"RACE" {
            model(p, &sig, d);
        }
    })
}

fn tex_path(d: &[u8]) -> Option<String> {
    let t = canon_texture(until_nul(d));
    (!t.norm.is_empty()).then_some(t.norm)
}

fn model_path(d: &[u8]) -> Option<String> {
    let m = canon_model(until_nul(d));
    (!m.is_empty()).then_some(m)
}

/// TXST (xEdit `wbRecord(TXST…)`): TX00..TX07, DODT presence, DNAM itU16 flags.
fn txst(p: &mut Payload, sig: &[u8; 4], d: &[u8]) {
    let t = p.txst.get_or_insert_with(Txst::default);
    match sig {
        [b'T', b'X', b'0', n @ b'0'..=b'7'] => t.slots[usize::from(*n - b'0')] = tex_path(d),
        b"DODT" => t.decal = true,
        b"DNAM" if d.len() >= 2 => t.flags = u16::from_le_bytes([d[0], d[1]]),
        _ => {}
    }
}

/// A u32 FormID subrecord pointing at a TXST; 0 (none) is dropped.
fn form_ref(p: &mut Payload, sig: [u8; 4], d: &[u8]) {
    if d.len() >= 4 {
        let id = u32_at(d, 0);
        if id != 0 {
            p.txst_refs.push((sig, id));
        }
    }
}

fn texture(p: &mut Payload, sig: [u8; 4], d: &[u8]) {
    if let Some(t) = tex_path(d) {
        p.textures.push((sig, t));
    }
}

/// xEdit `wbWeatherCloudTextures`: `_30_0TX`..`_40_0TX`, `A0TX`..`O0TX` = first byte 0x30..=0x4F, then "0TX".
fn is_cloud_layer(sig: &[u8; 4]) -> bool {
    &sig[1..] == b"0TX" && (0x30..=0x4F).contains(&sig[0])
}

fn model(p: &mut Payload, sig: &[u8; 4], d: &[u8]) {
    for (m, s, slot) in MODELS {
        if sig == &m {
            if let Some(path) = model_path(d) {
                p.models.push(ModelRef { slot, path, alt: vec![] });
            }
            return;
        }
        if sig == &s {
            let alt = alt_textures(d);
            if let Some(r) = p.models.iter_mut().rev().find(|r| r.slot == slot) {
                r.alt.extend(alt);
            }
            return;
        }
    }
}

/// xEdit `wbAlternateTexture` in a `wbArrayS(…, -1)`: u32 count, then per entry a `wbLenString` 3D name (u32
/// length, no terminator), the TXST FormID (u32) and the 3D index (s32). The prefix widths are not verified against
/// `wbInterface.pas` (plan Task 2); the builder and these tests pin them. A short entry ends the list; TXST 0 is no swap.
fn alt_textures(d: &[u8]) -> Vec<AltTex> {
    let mut out = vec![];
    if d.len() < 4 {
        return out;
    }
    let n = u32_at(d, 0) as usize;
    let mut p = 4;
    for _ in 0..n {
        if p + 4 > d.len() {
            break;
        }
        let name_end = p + 4 + u32_at(d, p) as usize;
        if name_end + 8 > d.len() {
            break;
        }
        let txst = u32_at(d, name_end);
        if txst != 0 {
            out.push(AltTex {
                shape_name: nif::text::lossy(until_nul(&d[p + 4..name_end])),
                txst_local_formid: txst,
                index: u32_at(d, name_end + 4) as i32,
            });
        }
        p = name_end + 8;
    }
    out
}

/// RACE (ruling R4; xEdit `wbRecord(RACE…)`): DFTM/DFTF/FTSM/FTSF → TXST; ANAM (skeletal models) and MODL (body and
/// head data) → texture when `.dds`, else model (slot 0); TINT (tint mask file) → texture.
fn race(p: &mut Payload, sig: [u8; 4], d: &[u8]) {
    match &sig {
        b"DFTM" | b"DFTF" | b"FTSM" | b"FTSF" => form_ref(p, sig, d),
        b"TINT" => texture(p, sig, d),
        b"ANAM" | b"MODL" => {
            if until_nul(d).to_ascii_lowercase().ends_with(b".dds") {
                texture(p, sig, d);
            } else if let Some(path) = model_path(d) {
                p.models.push(ModelRef { slot: 0, path, alt: vec![] });
            }
        }
        _ => {}
    }
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p esp; cargo test -p nif --test canon_model`
Expected: all pass (9 `parse.rs` + 10 `records.rs` + 1 `canon_model`).

- [ ] **Step 5: Gates and commit**

`git diff Cargo.lock` must be empty here.

```powershell
cargo fmt --all; cargo clippy --workspace --all-targets -- -D warnings; cargo test --workspace
git add crates/nif/src/path.rs crates/nif/tests/canon_model.rs crates/esp/src/records.rs crates/esp/tests/records.rs
git commit -m "esp: spec §3 record payloads (TXST, models + alternate textures, TXST refs, EFSH/WTHR/RACE paths); nif: canon_model" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: active load order, plugin files by MO2 priority, FormID prefixes

**Files**
- Modify: `crates/modscan/src/archives.rs` (extract `active_plugins`, used by `active_bsas_with`)
- Create: `crates/modscan/src/plugins.rs` (pure helpers here; Task 4 adds `scan_plugins`)
- Modify: `crates/modscan/src/lib.rs` (`pub mod plugins;`), `crates/modscan/Cargo.toml` (`esp` dep)
- Modify (shared, own commit): `Cargo.lock`
- Test: `crates/modscan/tests/plugins.rs` (new)

**Interfaces**
- Consumes: `esp::FLAG_ESL`; `mo2::{Instance, LooseOrigin}`.
- Produces:
  - `archives::ActivePlugin { pub name: String /* lowercase */, pub optional: bool /* Skyrim.ccc entry */ }`
  - `pub fn archives::active_plugins(inst: &Instance) -> Vec<ActivePlugin>`. The order is: base masters, then `Skyrim.ccc` entries absent from plugins.txt, then plugins.txt `*` entries. Each name appears once.
  - `pub fn plugins::plugin_files(loose: &[LooseOrigin]) -> HashMap<String, (PathBuf, String)>` (lowercase name → (path, on-disk name), highest priority)
  - `pub fn plugins::prefixes(light: &[bool]) -> Vec<Option<(u32, u32)>>` ((base, mask) per plugin in order; `None` past 254 full / 4096 light)
  - `pub fn plugins::clamp_local(local: u32, n_masters: usize) -> u32`
  - `pub fn plugins::is_light(name: &str, flags: u32) -> bool`

- [ ] **Step 1: Write the failing tests**

`crates/modscan/tests/plugins.rs`:

```rust
//! esp scanner spec §2.2: the active load order, MO2 resolution of plugin files, and load-order FormIDs.
mod common;
use common::instance_fixture;
use modscan::archives::{active_plugins, ActivePlugin};
use modscan::mo2::{loose_origins, Instance};
use modscan::plugins::{clamp_local, is_light, plugin_files, prefixes};
use std::fs;
use std::path::Path;

fn put(root: &Path, rel: &str, bytes: &[u8]) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, bytes).unwrap();
}

fn ap(name: &str, optional: bool) -> ActivePlugin {
    ActivePlugin { name: name.into(), optional }
}

/// Base masters, then Skyrim.ccc entries not in plugins.txt (optional), then plugins.txt `*` entries; disabled and
/// repeated names are left out.
#[test]
fn active_plugins_in_game_order() {
    let d = instance_fixture("+A\n");
    let r = d.path();
    put(r, "Game/Skyrim.ccc", b"ccQSSSE001-Fish.esm\nccBGSSSE025-AdvDSGS.esm\n");
    put(
        r,
        "profiles/Default/plugins.txt",
        b"# comment\n*ccBGSSSE025-AdvDSGS.esm\n*A.esp\nB.esp\n*a.esp\n",
    );
    let inst = Instance::load(r, None).unwrap();
    assert_eq!(
        active_plugins(&inst),
        [
            ap("skyrim.esm", false),
            ap("update.esm", false),
            ap("dawnguard.esm", false),
            ap("hearthfires.esm", false),
            ap("dragonborn.esm", false),
            ap("ccqsssse001-fish.esm", true),
            ap("ccbgssse025-advdsgs.esm", false),
            ap("a.esp", false),
        ]
    );
}

/// modlist.txt is highest priority first: B beats A; the game Data folder is lowest. Only top-level plugin files count.
#[test]
fn plugin_files_take_the_highest_priority_origin() {
    let d = instance_fixture("+B\n+A\n");
    let r = d.path();
    put(r, "mods/A/X.esp", b"a");
    put(r, "mods/B/X.esp", b"b");
    put(r, "mods/A/Only.ESM", b"a");
    put(r, "Game/Data/Skyrim.esm", b"s");
    put(r, "mods/A/readme.txt", b"t");
    put(r, "mods/A/sub/Deep.esp", b"d");
    let inst = Instance::load(r, None).unwrap();
    let (loose, _) = loose_origins(&inst).unwrap();
    let f = plugin_files(&loose);
    assert_eq!(fs::read(&f["x.esp"].0).unwrap(), b"b");
    assert_eq!(f["only.esm"].1, "Only.ESM", "the on-disk name is kept for display");
    assert!(f["skyrim.esm"].0.starts_with(r.join("Game/Data")));
    assert!(!f.contains_key("readme.txt") && !f.contains_key("deep.esp"));
}

/// Full plugins count their own `LL`; light ones their own `LLL` in FE space (ruling R2).
#[test]
fn prefixes_count_full_and_light_separately() {
    assert_eq!(
        prefixes(&[false, true, false, true]),
        [
            Some((0x0000_0000, 0x00FF_FFFF)),
            Some((0xFE00_0000, 0xFFF)),
            Some((0x0100_0000, 0x00FF_FFFF)),
            Some((0xFE00_1000, 0xFFF)),
        ]
    );
    let full = prefixes(&[false; 255]);
    assert_eq!(full[253], Some((0xFD00_0000, 0x00FF_FFFF)));
    assert_eq!(full[254], None, "0xFE is the light space");
    let light = prefixes(&[true; 4097]);
    assert_eq!(light[4095], Some((0xFEFF_F000, 0xFFF)));
    assert_eq!(light[4096], None);
}

#[test]
fn a_master_index_past_the_list_is_the_plugin_itself() {
    assert_eq!(clamp_local(0x0500_0123, 2), 0x0200_0123);
    assert_eq!(clamp_local(0x0100_0123, 2), 0x0100_0123);
    assert_eq!(clamp_local(0x0200_0123, 2), 0x0200_0123);
}

#[test]
fn light_by_flag_or_extension() {
    assert!(is_light("X.esl", 0));
    assert!(is_light("X.esp", esp::FLAG_ESL));
    assert!(!is_light("X.esm", esp::FLAG_ESM));
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p modscan --test plugins`
Expected: compile errors (`active_plugins`, `modscan::plugins` not found).

- [ ] **Step 3: Implement**

`crates/modscan/Cargo.toml`, in `[dependencies]` after `crc32fast.workspace = true`:

```toml
esp = { path = "../esp" }
```

`crates/modscan/src/lib.rs`: after `pub mod pgp;` add `pub mod plugins;`.

`crates/modscan/src/archives.rs`, after the `IMPLICIT` constant, add:

```rust
/// One plugin of the active load order (lowercase file name). `optional`: a `Skyrim.ccc` entry, which the game loads
/// only when the file exists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivePlugin {
    pub name: String,
    pub optional: bool,
}

/// The active load order (esp spec §2.2): the five base masters, `Skyrim.ccc` entries absent from plugins.txt, then
/// plugins.txt `*` entries; each name once, at its first place.
pub fn active_plugins(inst: &Instance) -> Vec<ActivePlugin> {
    let game = game_dir(inst);
    let plugins = std::fs::read_to_string(inst.profiles.join(&inst.profile).join("plugins.txt"))
        .unwrap_or_default();
    let listed: std::collections::HashSet<String> = plugins
        .lines()
        .map(|l| l.trim().trim_start_matches('*').trim().to_ascii_lowercase())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect();
    let ccc: Vec<String> = std::fs::read_to_string(game.join("Skyrim.ccc"))
        .unwrap_or_default()
        .lines()
        .map(|l| l.trim().to_ascii_lowercase())
        .filter(|l| !l.is_empty() && !listed.contains(l))
        .collect();
    let mut seen = std::collections::HashSet::new();
    IMPLICIT
        .iter()
        .map(|s| (s.to_string(), false))
        .chain(ccc.into_iter().map(|s| (s, true)))
        .chain(
            plugins
                .lines()
                .filter_map(|l| l.strip_prefix('*'))
                .map(|s| (s.trim().to_ascii_lowercase(), false)),
        )
        .filter(|(n, _)| !n.is_empty() && seen.insert(n.clone()))
        .map(|(name, optional)| ActivePlugin { name, optional })
        .collect()
}
```

In `active_bsas_with`, replace everything from `    // 3. Plugin-attached archives: base masters, Skyrim.ccc entries absent from plugins.txt, plugins.txt.` up to the end of the `for p in active { … }` loop with:

```rust
    // 3. Plugin-attached archives: base masters, Skyrim.ccc entries absent from plugins.txt, plugins.txt.
    for p in active_plugins(inst) {
        let stem = p
            .name
            .rsplit_once('.')
            .map_or(p.name.as_str(), |x| x.0)
            .to_string();
        push(&format!("{stem}.bsa"), &mut out);
        push(&format!("{stem} - textures.bsa"), &mut out);
    }
```

(`let game = game_dir(inst);` above stays: step 2 still uses it.)

`crates/modscan/src/plugins.rs`:

```rust
//! The plugin pass (esp scanner spec §2.2): active plugins found through the MO2 origins, load-order FormIDs.
use crate::mo2::LooseOrigin;
use std::collections::HashMap;
use std::path::PathBuf;

/// Top-level `.esm/.esp/.esl` files of every loose origin, by lowercase name → (path, file name as on disk); the
/// highest-priority origin wins (overwrite > mods by priority > game Data). One `read_dir` per origin.
pub fn plugin_files(loose: &[LooseOrigin]) -> HashMap<String, (PathBuf, String)> {
    let mut best: HashMap<String, (PathBuf, String, i32)> = HashMap::new();
    for o in loose {
        let Ok(rd) = std::fs::read_dir(&o.root) else {
            continue;
        };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let low = name.to_ascii_lowercase();
            let plugin = [".esm", ".esp", ".esl"].iter().any(|x| low.ends_with(x));
            if !plugin || !e.file_type().is_ok_and(|t| t.is_file()) {
                continue;
            }
            if best.get(&low).is_none_or(|b| o.priority > b.2) {
                best.insert(low, (e.path(), name, o.priority));
            }
        }
    }
    best.into_iter().map(|(k, (p, n, _))| (k, (p, n))).collect()
}

/// (base, mask) of each plugin's own FormIDs, in load order (ruling R2): full `LL << 24` / 0xFFFFFF with LL = the
/// count of earlier full plugins; light `0xFE000000 | LLL << 12` / 0xFFF with LLL = the count of earlier light ones.
/// `None` past the game's limits (254 full, 4096 light).
pub fn prefixes(light: &[bool]) -> Vec<Option<(u32, u32)>> {
    let (mut full, mut lite) = (0u32, 0u32);
    light
        .iter()
        .map(|&l| {
            if l {
                let i = lite;
                lite += 1;
                (i < 0x1000).then_some((0xFE00_0000 | (i << 12), 0xFFF))
            } else {
                let i = full;
                full += 1;
                (i < 0xFE).then_some((i << 24, 0x00FF_FFFF))
            }
        })
        .collect()
}

/// The local FormID with a master index past the master list read as the plugin itself, as the game reads it.
pub fn clamp_local(local: u32, n_masters: usize) -> u32 {
    let idx = (local >> 24) as usize;
    if idx > n_masters {
        ((n_masters.min(0xFF) as u32) << 24) | (local & 0x00FF_FFFF)
    } else {
        local
    }
}

/// A light plugin: the ESL header flag, or the `.esl` extension.
pub fn is_light(name: &str, flags: u32) -> bool {
    flags & esp::FLAG_ESL != 0 || name.to_ascii_lowercase().ends_with(".esl")
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p modscan`
Expected: all pass. That includes the 5 new tests and the archive-order tests in `tests/resolve.rs` (F8 `Skyrim.ccc` etc.), which show the refactor kept the BSA order.

- [ ] **Step 5: Check the lock, gates, commit (two commits)**

Run: `git diff Cargo.lock`
Expected: one line `"esp",` added to the `modscan` package's `dependencies`. Anything else: stop and report.

```powershell
cargo fmt --all; cargo clippy --workspace --all-targets -- -D warnings; cargo test --workspace
git add crates/modscan/Cargo.toml crates/modscan/src/archives.rs crates/modscan/src/plugins.rs crates/modscan/src/lib.rs crates/modscan/tests/plugins.rs
git commit -m "modscan: active load order (shared with the BSA order), plugin files by MO2 priority, FormID prefixes" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
git add Cargo.lock
git commit -m "Cargo.lock: modscan depends on esp" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: store schema v4, plugin cache, `scan_plugins` (FormIDs, winners)

**Files**
- Modify: `crates/modscan/src/store.rs` (`SCHEMA_VERSION` 4, plugin tables, `conn` and `path_id` `pub(crate)`)
- Create: `crates/modscan/src/plugin_store.rs` (`impl Store` for plugins)
- Modify: `crates/modscan/src/lib.rs` (`pub mod plugin_store;`), `crates/modscan/src/plugins.rs` (`PluginStats`, `scan_plugins`), `crates/modscan/src/scan.rs` (`file_mtime` → `pub(crate)`)
- Test: `crates/modscan/tests/plugins.rs`

**Interfaces**
- Consumes: Task 1 `esp::{parse, PluginFacts}`; Task 3 `active_plugins`, `plugin_files`, `prefixes`, `clamp_local`, `is_light`; `scan::Progress`.
- Produces:
  - `pub struct plugins::PluginStats { pub n: usize, pub cached: usize, pub records: usize, pub ms: u64, pub problems: Vec<(String, String)> }` (`Debug, Clone, Default, PartialEq`)
  - `pub fn plugins::scan_plugins(inst: &Instance, loose: &[LooseOrigin], store: &mut Store, cancel: &AtomicBool, progress: &dyn Fn(Progress)) -> Result<PluginStats, ScanError>`. Progress phase is `"plugins"`. Cancel is checked per chunk of 64 plugins and returns `Err(ScanError::Cancelled)`.
  - `plugin_store`: `PluginFile<'a> { name, path: &'a str, size: Option<u64>, mtime: i64 }`, `PluginRow { id, name, path, size: Option<u64>, mtime, lo_index: Option<i64>, light, masters: Vec<String>, error_text: Option<String> }`
  - `Store::write_plugin(&mut self, f: &PluginFile, facts: &PluginFacts) -> Result<i64, ScanError>`, `retain_plugins(&mut self, active: &[String]) -> Result<usize, _>`, `plugin_rows(&self) -> Result<HashMap<String /*lowercase*/, PluginRow>, _>`, `resolve_plugins(&mut self, lo: &[(i64, i64)], map: &[[i64; 4]]) -> Result<(), _>`, `unload_plugins(&mut self) -> Result<(), _>`, `plugin_record_count(&self) -> Result<usize, _>`
  - Schema v4 tables (below). `formid` columns are NULL until resolved. `rec.owner_ok = 1` iff the record's master is loaded.

- [ ] **Step 1: Write the failing tests**

Append to `crates/modscan/tests/plugins.rs`. Add these `use` lines at the top, after the existing ones:

```rust
#[path = "../../esp/tests/common/mod.rs"]
mod b;
use b::{alt, edid, form, plugin, rec, sub, top, zstr};
use modscan::plugins::{scan_plugins, PluginStats};
use modscan::store::Store;
use modscan::ScanError;
use std::sync::atomic::AtomicBool;
```

and append:

```rust
const TXST_DIRT: u32 = 0x0000_0ABC;

fn txst(id: u32, name: &str, tx00: &str) -> Vec<u8> {
    rec(b"TXST", id, &[edid(name), sub(b"TX00", &zstr(tx00))])
}

fn stat(id: u32, name: &str, model: &str) -> Vec<u8> {
    rec(b"STAT", id, &[edid(name), sub(b"MODL", &zstr(model))])
}

/// Skyrim.esm in Data; A.esp (full) and L.esp (ESL flag) in mod A; Z.esl (light by extension) and F.esp (full,
/// master L.esp) in mod B. With `*A.esp *L.esp *Z.esl *F.esp`: Skyrim 00, A 01, L FE000, Z FE001, F 02.
fn load_order(plugins_txt: &str) -> tempfile::TempDir {
    let d = instance_fixture("+B\n+A\n");
    let r = d.path();
    put(
        r,
        "Game/Data/Skyrim.esm",
        &plugin(esp::FLAG_ESM, &[], &[top(b"TXST", &[txst(TXST_DIRT, "DirtTXST", "Landscape\\Dirt02.dds")])]),
    );
    put(
        r,
        "mods/A/A.esp",
        &plugin(
            0,
            &["Skyrim.esm"],
            &[
                top(b"TXST", &[txst(TXST_DIRT, "DirtTXST", "Landscape\\Dirt02.dds")]),
                top(b"STAT", &[stat(0x0100_0800, "AStat", "a.nif")]),
            ],
        ),
    );
    put(
        r,
        "mods/A/L.esp",
        &plugin(
            esp::FLAG_ESL,
            &["Skyrim.esm", "A.esp"],
            &[
                top(
                    b"STAT",
                    &[rec(
                        b"STAT",
                        0x0200_0801,
                        &[edid("LStat"), sub(b"MODL", &zstr("l.nif")), sub(b"MODS", &alt(&[("Shape0", TXST_DIRT, 0)]))],
                    )],
                ),
                top(b"LTEX", &[rec(b"LTEX", 0x0200_0802, &[edid("LTex"), form(b"TNAM", 0x0100_0800)])]),
            ],
        ),
    );
    put(r, "mods/B/Z.esl", &plugin(0, &["Skyrim.esm"], &[top(b"STAT", &[stat(0x0100_0803, "ZStat", "z.nif")])]));
    put(
        r,
        "mods/B/F.esp",
        &plugin(
            0,
            &["Skyrim.esm", "L.esp"],
            &[top(b"LTEX", &[rec(b"LTEX", 0x0200_0900, &[edid("FTex"), form(b"TNAM", 0x0100_0801)])])],
        ),
    );
    put(r, "profiles/Default/plugins.txt", plugins_txt.as_bytes());
    d
}

const ORDER: &str = "*A.esp\n*L.esp\n*Z.esl\n*F.esp\n";

fn try_scan(d: &tempfile::TempDir, cancel: bool) -> (Store, Result<PluginStats, ScanError>) {
    let inst = Instance::load(d.path(), None).unwrap();
    let (loose, _) = loose_origins(&inst).unwrap();
    let mut store = Store::open(&d.path().join("scan.db")).unwrap();
    let r = scan_plugins(&inst, &loose, &mut store, &AtomicBool::new(cancel), &|_| {});
    (store, r)
}

fn scan(d: &tempfile::TempDir) -> (Store, PluginStats) {
    let (s, r) = try_scan(d, false);
    (s, r.unwrap())
}

fn one_i64(s: &Store, sql: &str, p: impl rusqlite::Params) -> i64 {
    s.conn().query_row(sql, p, |r| r.get(0)).unwrap()
}

fn formid(s: &Store, edid: &str) -> u32 {
    one_i64(s, "SELECT formid FROM rec WHERE edid = ?1", [edid]) as u32
}

/// (winning plugin, n_overrides) of a load-order FormID.
fn winner(s: &Store, id: u32) -> (String, i64) {
    s.conn()
        .query_row(
            "SELECT p.name, w.n_overrides FROM rec_winner w JOIN plugin p ON p.id = w.plugin_id WHERE w.formid = ?1",
            [i64::from(id)],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap()
}

#[test]
fn formids_follow_the_load_order_full_and_light() {
    let d = load_order(ORDER);
    let (s, st) = scan(&d);
    assert_eq!((st.n, st.cached), (5, 0));
    assert_eq!(formid(&s, "AStat"), 0x0100_0800);
    assert_eq!(formid(&s, "LStat"), 0xFE00_0801, "light plugin: FE 000 801");
    assert_eq!(formid(&s, "ZStat"), 0xFE00_1803, ".esl without the flag is light too");
    assert_eq!(formid(&s, "FTex"), 0x0200_0900);
    let txst_of = |e: &str| {
        one_i64(&s, "SELECT t.txst_formid FROM txst_ref t JOIN rec r ON r.plugin_id = t.plugin_id AND r.local = t.local WHERE r.edid = ?1", [e]) as u32
    };
    assert_eq!(txst_of("LTex"), 0x0100_0800, "a light plugin's reference into a full master");
    assert_eq!(txst_of("FTex"), 0xFE00_0801, "Review Focus 1: a full plugin's reference into a light master");
    assert_eq!(
        one_i64(&s, "SELECT txst_formid FROM alt_tex WHERE shape_name = 'Shape0'", []) as u32,
        TXST_DIRT
    );
    assert!(st.problems.iter().any(|(p, m)| p == "update.esm" && m.contains("not found")), "{:?}", st.problems);
    assert_eq!(st.records, 7, "Skyrim 1 + A 2 + L 2 + Z 1 + F 1");
}

#[test]
fn last_plugin_wins_and_overrides_are_counted() {
    let d = load_order(ORDER);
    let (s, _) = scan(&d);
    assert_eq!(winner(&s, TXST_DIRT), ("A.esp".to_string(), 1));
    assert_eq!(winner(&s, 0xFE00_0801), ("L.esp".to_string(), 0));
}

/// Review Focus 2: nothing is re-parsed when only the order changes, and the FormIDs follow it.
#[test]
fn unchanged_plugins_are_cached_and_a_reorder_remaps() {
    let d = load_order(ORDER);
    let (s, _) = scan(&d);
    drop(s);
    let (s, st) = scan(&d);
    assert_eq!((st.n, st.cached), (5, 5));
    assert_eq!(formid(&s, "ZStat"), 0xFE00_1803);
    drop(s);
    fs::write(d.path().join("profiles/Default/plugins.txt"), "*Z.esl\n*A.esp\n*L.esp\n*F.esp\n").unwrap();
    let (s, st) = scan(&d);
    assert_eq!(st.cached, 5);
    assert_eq!(formid(&s, "ZStat"), 0xFE00_0803);
    assert_eq!(formid(&s, "LStat"), 0xFE00_1801);
    assert_eq!(formid(&s, "AStat"), 0x0100_0800, "full indices count full plugins only");
    drop(s);
    put(
        d.path(),
        "mods/B/Z.esl",
        &plugin(0, &["Skyrim.esm"], &[top(b"STAT", &[stat(0x0100_0803, "ZStat", "z.nif"), stat(0x0100_0804, "Z2", "z2.nif")])]),
    );
    let (s, st) = scan(&d);
    assert_eq!(st.cached, 4);
    assert_eq!(formid(&s, "Z2"), 0xFE00_0804);
}

/// Review Focus 3: a row stored after a failed read (size NULL) never counts as cached.
#[test]
fn a_plugin_whose_read_failed_is_read_again() {
    let d = load_order(ORDER);
    let (s, _) = scan(&d);
    s.conn().execute("UPDATE plugin SET size = NULL WHERE name = 'Z.esl'", []).unwrap();
    drop(s);
    let (_, st) = scan(&d);
    assert_eq!(st.cached, 4);
}

/// Spec §5: the record keeps its raw FormID and the plugin gets a note; it never wins.
#[test]
fn a_missing_master_is_noted_and_keeps_the_raw_formid() {
    let d = load_order("*A.esp\n*M.esp\n");
    put(
        d.path(),
        "mods/A/M.esp",
        &plugin(0, &["Missing.esm"], &[top(b"STAT", &[stat(0x0000_0900, "MStat", "m.nif")])]),
    );
    let (s, st) = scan(&d);
    assert!(st.problems.contains(&("M.esp".to_string(), "missing master Missing.esm".to_string())), "{:?}", st.problems);
    assert_eq!(formid(&s, "MStat"), 0x0000_0900);
    assert_eq!(one_i64(&s, "SELECT owner_ok FROM rec WHERE edid = 'MStat'", []), 0);
    assert_eq!(one_i64(&s, "SELECT count(*) FROM rec_winner WHERE formid = 2304", []), 0);
}

#[test]
fn not_found_disabled_and_dropped_plugins() {
    let d = load_order(ORDER);
    let (s, _) = scan(&d);
    drop(s);
    fs::write(d.path().join("profiles/Default/plugins.txt"), "*A.esp\nZ.esl\n*Gone.esp\n").unwrap();
    let (s, st) = scan(&d);
    assert_eq!(st.n, 2, "Skyrim.esm and A.esp");
    assert!(st.problems.iter().any(|(p, m)| p == "gone.esp" && m.contains("not found")), "{:?}", st.problems);
    assert_eq!(one_i64(&s, "SELECT count(*) FROM plugin", []), 2, "plugins that left the order are dropped");
    assert_eq!(one_i64(&s, "SELECT count(*) FROM rec WHERE edid = 'ZStat'", []), 0);
}

/// Spec §5: parse errors are reported with the plugin name on every scan, cached or not.
#[test]
fn parse_errors_are_reported_on_every_scan() {
    let d = load_order("*Bad.esp\n");
    put(d.path(), "mods/A/Bad.esp", b"junk");
    let want = ("Bad.esp".to_string(), "offset 0x0: not a plugin (no TES4 header)".to_string());
    let (s, st) = scan(&d);
    assert!(st.problems.contains(&want), "{:?}", st.problems);
    drop(s);
    let (_, st) = scan(&d);
    assert_eq!(st.cached, st.n);
    assert!(st.problems.contains(&want), "{:?}", st.problems);
}

#[test]
fn cancel_stops_the_plugin_pass() {
    let d = load_order(ORDER);
    let (_, r) = try_scan(&d, true);
    assert!(matches!(r, Err(ScanError::Cancelled)), "{r:?}");
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p modscan --test plugins`
Expected: compile errors (`scan_plugins`, `PluginStats` not found).

- [ ] **Step 3: Implement the schema**

`crates/modscan/src/store.rs`:
1. `pub const SCHEMA_VERSION: i64 = 3;` → `pub const SCHEMA_VERSION: i64 = 4;`
2. In `SCHEMA`, after the line `CREATE INDEX shape_texset ON shape(texset_id);` add:

```sql
-- Plugins (esp scanner spec §2.3). `local` = the FormID as the plugin stores it (master index clamped to the
-- plugin): the cache. `formid`/`txst_formid` = load-order FormIDs, set by `resolve_plugins` (NULL until then; the raw
-- local id when the master is not loaded, `rec.owner_ok` = 0). `plugin.lo_index` = load-order position, NULL when
-- not resolved for the current order (written only by `resolve_plugins`).
CREATE TABLE plugin (id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE COLLATE NOCASE, path TEXT NOT NULL,
                     size INTEGER, mtime INTEGER NOT NULL, lo_index INTEGER, light INTEGER NOT NULL,
                     flags INTEGER NOT NULL, masters_json TEXT NOT NULL, error_text TEXT);
CREATE TABLE rec (formid INTEGER, local INTEGER NOT NULL, plugin_id INTEGER NOT NULL, sig TEXT NOT NULL, edid TEXT,
                     owner_ok INTEGER NOT NULL DEFAULT 0, txst_flags INTEGER, decal INTEGER);
CREATE TABLE rec_winner (formid INTEGER PRIMARY KEY, plugin_id INTEGER NOT NULL, n_overrides INTEGER NOT NULL);
CREATE TABLE txst_slot (formid INTEGER, local INTEGER NOT NULL, plugin_id INTEGER NOT NULL, slot INTEGER NOT NULL,
                     path_id INTEGER NOT NULL);
CREATE TABLE model_ref (formid INTEGER, local INTEGER NOT NULL, plugin_id INTEGER NOT NULL, slot INTEGER NOT NULL,
                     path_id INTEGER NOT NULL);
CREATE TABLE alt_tex (formid INTEGER, local INTEGER NOT NULL, plugin_id INTEGER NOT NULL, model_slot INTEGER NOT NULL,
                     shape_name TEXT NOT NULL, txst_formid INTEGER, txst_local INTEGER NOT NULL, idx INTEGER NOT NULL);
CREATE TABLE txst_ref (formid INTEGER, local INTEGER NOT NULL, plugin_id INTEGER NOT NULL, role TEXT NOT NULL,
                     txst_formid INTEGER, txst_local INTEGER NOT NULL);
CREATE TABLE tex_ref (formid INTEGER, local INTEGER NOT NULL, plugin_id INTEGER NOT NULL, role TEXT NOT NULL,
                     path_id INTEGER NOT NULL);
CREATE INDEX rec_formid ON rec(formid);
CREATE INDEX rec_plugin ON rec(plugin_id, local);
CREATE INDEX txst_slot_path ON txst_slot(path_id);
CREATE INDEX txst_slot_plugin ON txst_slot(plugin_id, local);
CREATE INDEX model_ref_path ON model_ref(path_id);
CREATE INDEX model_ref_plugin ON model_ref(plugin_id, local);
CREATE INDEX alt_tex_txst ON alt_tex(txst_formid);
CREATE INDEX alt_tex_plugin ON alt_tex(plugin_id, local);
CREATE INDEX txst_ref_txst ON txst_ref(txst_formid);
CREATE INDEX txst_ref_plugin ON txst_ref(plugin_id, local);
CREATE INDEX tex_ref_path ON tex_ref(path_id);
CREATE INDEX tex_ref_plugin ON tex_ref(plugin_id, local);
```

3. `pub struct Store {` / `    conn: Connection,` → `    pub(crate) conn: Connection,`
4. `fn path_id(c: &Connection, p: &str) -> rusqlite::Result<i64> {` → `pub(crate) fn path_id(c: &Connection, p: &str) -> rusqlite::Result<i64> {`

`crates/modscan/src/scan.rs`: `fn file_mtime(p: &std::path::Path) -> i64 {` → `pub(crate) fn file_mtime(p: &std::path::Path) -> i64 {`

`crates/modscan/src/lib.rs`: after `pub mod pgp;` add `pub mod plugin_store;`.

- [ ] **Step 4: Implement `plugin_store.rs`**

```rust
//! Plugin tables of the scan store (esp scanner spec §2.3): the per-plugin cache rows and the load-order resolution.
use crate::plugins::{clamp_local, is_light};
use crate::store::{path_id, Store};
use crate::ScanError;
use esp::PluginFacts;
use rusqlite::{params, Connection};
use std::collections::{HashMap, HashSet};

/// Where a plugin came from. `size` is `None` after a failed read, so the row is never reused (read again next scan).
#[derive(Debug, Clone)]
pub struct PluginFile<'a> {
    pub name: &'a str,
    pub path: &'a str,
    pub size: Option<u64>,
    pub mtime: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PluginRow {
    pub id: i64,
    /// As on disk.
    pub name: String,
    pub path: String,
    pub size: Option<u64>,
    pub mtime: i64,
    pub lo_index: Option<i64>,
    pub light: bool,
    pub masters: Vec<String>,
    /// The parser's errors, one per line.
    pub error_text: Option<String>,
}

/// Tables with plugin rows, for deletes.
const PLUGIN_TABLES: [&str; 6] = ["rec", "txst_slot", "model_ref", "alt_tex", "txst_ref", "tex_ref"];

/// (table, load-order column, local column) recomputed by `resolve_plugins`.
const REMAP: [(&str, &str, &str); 8] = [
    ("rec", "formid", "local"),
    ("txst_slot", "formid", "local"),
    ("model_ref", "formid", "local"),
    ("tex_ref", "formid", "local"),
    ("alt_tex", "formid", "local"),
    ("alt_tex", "txst_formid", "txst_local"),
    ("txst_ref", "formid", "local"),
    ("txst_ref", "txst_formid", "txst_local"),
];

fn sig_text(s: &[u8; 4]) -> String {
    String::from_utf8_lossy(s).into_owned()
}

fn json_err(e: serde_json::Error) -> ScanError {
    ScanError::Io(std::io::Error::other(e))
}

fn delete_rows(c: &Connection, plugin_id: i64) -> rusqlite::Result<()> {
    for t in PLUGIN_TABLES {
        c.prepare_cached(&format!("DELETE FROM {t} WHERE plugin_id = ?1"))?
            .execute([plugin_id])?;
    }
    Ok(())
}

impl Store {
    /// Replaces one plugin's rows with `facts` (local FormIDs, `lo_index` NULL until `resolve_plugins`).
    pub fn write_plugin(&mut self, f: &PluginFile, facts: &PluginFacts) -> Result<i64, ScanError> {
        let tx = self.conn.transaction()?;
        let masters = serde_json::to_string(&facts.masters).map_err(json_err)?;
        let errors = (!facts.errors.is_empty()).then(|| facts.errors.join("\n"));
        let id: i64 = tx.query_row(
            "INSERT INTO plugin(name, path, size, mtime, lo_index, light, flags, masters_json, error_text)
             VALUES (?1,?2,?3,?4,NULL,?5,?6,?7,?8)
             ON CONFLICT(name) DO UPDATE SET name = excluded.name, path = excluded.path, size = excluded.size,
               mtime = excluded.mtime, lo_index = NULL, light = excluded.light, flags = excluded.flags,
               masters_json = excluded.masters_json, error_text = excluded.error_text
             RETURNING id",
            params![
                f.name,
                f.path,
                f.size.map(|s| s as i64),
                f.mtime,
                is_light(f.name, facts.flags),
                i64::from(facts.flags),
                masters,
                errors
            ],
            |r| r.get(0),
        )?;
        delete_rows(&tx, id)?;
        let nm = facts.masters.len();
        for r in &facts.records {
            let local = i64::from(clamp_local(r.local_formid, nm));
            let t = r.payload.txst.as_ref();
            tx.prepare_cached(
                "INSERT INTO rec(local, plugin_id, sig, edid, txst_flags, decal) VALUES (?1,?2,?3,?4,?5,?6)",
            )?
            .execute(params![
                local,
                id,
                sig_text(&r.sig),
                r.edid,
                t.map(|t| i64::from(t.flags)),
                t.map(|t| t.decal)
            ])?;
            if let Some(t) = t {
                for (slot, p) in t.slots.iter().enumerate() {
                    if let Some(p) = p {
                        let pid = path_id(&tx, p)?;
                        tx.prepare_cached(
                            "INSERT INTO txst_slot(local, plugin_id, slot, path_id) VALUES (?1,?2,?3,?4)",
                        )?
                        .execute(params![local, id, slot as i64, pid])?;
                    }
                }
            }
            for m in &r.payload.models {
                let pid = path_id(&tx, &m.path)?;
                tx.prepare_cached("INSERT INTO model_ref(local, plugin_id, slot, path_id) VALUES (?1,?2,?3,?4)")?
                    .execute(params![local, id, i64::from(m.slot), pid])?;
                for a in &m.alt {
                    tx.prepare_cached(
                        "INSERT INTO alt_tex(local, plugin_id, model_slot, shape_name, txst_local, idx)
                         VALUES (?1,?2,?3,?4,?5,?6)",
                    )?
                    .execute(params![
                        local,
                        id,
                        i64::from(m.slot),
                        a.shape_name,
                        i64::from(clamp_local(a.txst_local_formid, nm)),
                        i64::from(a.index)
                    ])?;
                }
            }
            for (role, t) in &r.payload.txst_refs {
                tx.prepare_cached("INSERT INTO txst_ref(local, plugin_id, role, txst_local) VALUES (?1,?2,?3,?4)")?
                    .execute(params![local, id, sig_text(role), i64::from(clamp_local(*t, nm))])?;
            }
            for (role, p) in &r.payload.textures {
                let pid = path_id(&tx, p)?;
                tx.prepare_cached("INSERT INTO tex_ref(local, plugin_id, role, path_id) VALUES (?1,?2,?3,?4)")?
                    .execute(params![local, id, sig_text(role), pid])?;
            }
        }
        tx.commit()?;
        Ok(id)
    }

    /// Deletes every plugin (with its rows) whose name is not in `active`; returns how many.
    pub fn retain_plugins(&mut self, active: &[String]) -> Result<usize, ScanError> {
        let keep: HashSet<String> = active.iter().map(|n| n.to_ascii_lowercase()).collect();
        let tx = self.conn.transaction()?;
        let gone: Vec<i64> = {
            let mut st = tx.prepare("SELECT id, name FROM plugin")?;
            let all = st
                .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?
                .collect::<Result<Vec<_>, _>>()?;
            all.into_iter()
                .filter(|(_, n)| !keep.contains(&n.to_ascii_lowercase()))
                .map(|(id, _)| id)
                .collect()
        };
        for id in &gone {
            delete_rows(&tx, *id)?;
            tx.execute("DELETE FROM plugin WHERE id = ?1", [id])?;
        }
        tx.commit()?;
        Ok(gone.len())
    }

    /// Every stored plugin by lowercase name.
    pub fn plugin_rows(&self) -> Result<HashMap<String, PluginRow>, ScanError> {
        let mut st = self.conn.prepare(
            "SELECT id, name, path, size, mtime, lo_index, light, masters_json, error_text FROM plugin",
        )?;
        let rows = st.query_map([], |r| {
            Ok(PluginRow {
                id: r.get(0)?,
                name: r.get(1)?,
                path: r.get(2)?,
                size: r.get::<_, Option<i64>>(3)?.map(|s| s as u64),
                mtime: r.get(4)?,
                lo_index: r.get(5)?,
                light: r.get(6)?,
                masters: serde_json::from_str(&r.get::<_, String>(7)?).unwrap_or_default(),
                error_text: r.get(8)?,
            })
        })?;
        let mut out = HashMap::new();
        for row in rows {
            let row = row?;
            out.insert(row.name.to_ascii_lowercase(), row);
        }
        Ok(out)
    }

    /// Load-order FormIDs and winners (ruling R1/R2). `lo`: (plugin id, load-order position) of every active plugin.
    /// `map`: [plugin id, master index, base, mask] for every master index that is loaded, the plugin itself
    /// included (index = its master count). An unmapped index keeps the raw local id with `owner_ok` = 0.
    pub fn resolve_plugins(&mut self, lo: &[(i64, i64)], map: &[[i64; 4]]) -> Result<(), ScanError> {
        let tx = self.conn.transaction()?;
        tx.execute_batch(
            "CREATE TEMP TABLE IF NOT EXISTS lo_map (plugin_id INTEGER NOT NULL, idx INTEGER NOT NULL,
               base INTEGER NOT NULL, mask INTEGER NOT NULL, PRIMARY KEY(plugin_id, idx)) WITHOUT ROWID;
             DELETE FROM lo_map;
             UPDATE plugin SET lo_index = NULL;",
        )?;
        {
            let mut m = tx.prepare("INSERT OR REPLACE INTO lo_map VALUES (?1,?2,?3,?4)")?;
            for r in map {
                m.execute(params![r[0], r[1], r[2], r[3]])?;
            }
            let mut l = tx.prepare("UPDATE plugin SET lo_index = ?2 WHERE id = ?1")?;
            for (id, i) in lo {
                l.execute(params![id, i])?;
            }
        }
        for (t, out, src) in REMAP {
            tx.execute(
                &format!(
                    "UPDATE {t} SET {out} = coalesce((SELECT m.base | ({t}.{src} & m.mask) FROM lo_map m
                       WHERE m.plugin_id = {t}.plugin_id AND m.idx = ({t}.{src} >> 24)), {t}.{src})"
                ),
                [],
            )?;
        }
        // SQLite: with one max() aggregate, the bare columns come from the row holding the max (the last plugin).
        tx.execute_batch(
            "UPDATE rec SET owner_ok = EXISTS(SELECT 1 FROM lo_map m WHERE m.plugin_id = rec.plugin_id
               AND m.idx = (rec.local >> 24));
             DELETE FROM rec_winner;
             INSERT INTO rec_winner(formid, plugin_id, n_overrides)
               SELECT formid, plugin_id, n FROM (
                 SELECT r.formid AS formid, r.plugin_id AS plugin_id, max(p.lo_index) AS lo, count(*) - 1 AS n
                 FROM rec r JOIN plugin p ON p.id = r.plugin_id
                 WHERE r.owner_ok = 1 AND p.lo_index IS NOT NULL GROUP BY r.formid);",
        )?;
        tx.commit()?;
        Ok(())
    }

    /// A Folder scan (ruling R7): no load order, so no winners; the rows stay as cache and the next MO2 scan
    /// re-resolves them (`lo_index` NULL never matches).
    pub fn unload_plugins(&mut self) -> Result<(), ScanError> {
        self.conn
            .execute_batch("UPDATE plugin SET lo_index = NULL; DELETE FROM rec_winner;")?;
        Ok(())
    }

    pub fn plugin_record_count(&self) -> Result<usize, ScanError> {
        let n: i64 = self.conn.query_row("SELECT count(*) FROM rec", [], |r| r.get(0))?;
        Ok(n as usize)
    }
}
```

- [ ] **Step 5: Implement `scan_plugins`**

`crates/modscan/src/plugins.rs`: replace the `use` lines with

```rust
use crate::archives::active_plugins;
use crate::mo2::{Instance, LooseOrigin};
use crate::plugin_store::{PluginFile, PluginRow};
use crate::scan::{file_mtime, Progress};
use crate::store::Store;
use crate::ScanError;
use esp::PluginFacts;
use rayon::prelude::*;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;
```

and append:

```rust
/// Plugins parsed per progress step and cancel check.
const CHUNK: usize = 64;

/// The plugin pass's numbers for the scan details line and the timing entry (ruling R6).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PluginStats {
    /// Active plugins found on disk.
    pub n: usize,
    /// Of those, kept from the store (same path, size, mtime).
    pub cached: usize,
    /// Record copies stored for the active plugins.
    pub records: usize,
    pub ms: u64,
    /// (plugin, message): not found, parse errors, missing masters, load-order limits. Every scan, cached or not.
    pub problems: Vec<(String, String)>,
}

/// An active plugin found on disk.
struct Found {
    name: String,
    path: PathBuf,
    size: u64,
    mtime: i64,
}

fn read_plugin(a: &Found) -> Result<PluginFacts, String> {
    let bytes = std::fs::read(&a.path).map_err(|e| format!("read failed: {e}"))?;
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| esp::parse(&a.name, &bytes)))
        .map_err(|_| "plugin parser panic".to_string())
}

/// The plugin pass (spec §2.2): find the active plugins through the MO2 origins, parse the changed ones in parallel
/// (one plugin per rayon task), drop the ones that left, then resolve load-order FormIDs and winners when anything
/// changed (ruling R1).
pub fn scan_plugins(
    inst: &Instance,
    loose: &[LooseOrigin],
    store: &mut Store,
    cancel: &AtomicBool,
    progress: &dyn Fn(Progress),
) -> Result<PluginStats, ScanError> {
    let t = Instant::now();
    let mut st = PluginStats::default();
    let files = plugin_files(loose);
    let mut active: Vec<Found> = vec![];
    for p in active_plugins(inst) {
        let Some((path, name)) = files.get(&p.name) else {
            if !p.optional {
                st.problems.push((
                    p.name.clone(),
                    "not found in the game Data folder or any enabled mod".into(),
                ));
            }
            continue;
        };
        match std::fs::metadata(path) {
            Ok(md) => active.push(Found {
                name: name.clone(),
                path: path.clone(),
                size: md.len(),
                mtime: file_mtime(path),
            }),
            Err(e) => st.problems.push((name.clone(), e.to_string())),
        }
    }
    st.n = active.len();

    let before = store.plugin_rows()?;
    let work: Vec<usize> = (0..active.len())
        .filter(|&i| {
            let a = &active[i];
            before.get(&a.name.to_ascii_lowercase()).is_none_or(|r| {
                r.path != a.path.display().to_string() || r.size != Some(a.size) || r.mtime != a.mtime
            })
        })
        .collect();
    st.cached = active.len() - work.len();
    for (k, chunk) in work.chunks(CHUNK).enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return Err(ScanError::Cancelled);
        }
        progress(Progress {
            phase: "plugins",
            done: k * CHUNK,
            total: work.len(),
        });
        let read: Vec<(usize, Result<PluginFacts, String>)> =
            chunk.par_iter().map(|&i| (i, read_plugin(&active[i]))).collect();
        for (i, r) in read {
            let a = &active[i];
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
        }
    }
    progress(Progress {
        phase: "plugins",
        done: work.len(),
        total: work.len(),
    });

    let names: Vec<String> = active.iter().map(|a| a.name.clone()).collect();
    let removed = store.retain_plugins(&names)?;
    let rows = store.plugin_rows()?;
    let order: Vec<&PluginRow> = names
        .iter()
        .filter_map(|n| rows.get(&n.to_ascii_lowercase()))
        .collect();
    let pre = prefixes(&order.iter().map(|r| r.light).collect::<Vec<_>>());
    let index: HashMap<String, usize> = order
        .iter()
        .enumerate()
        .map(|(i, r)| (r.name.to_ascii_lowercase(), i))
        .collect();
    let mut map: Vec<[i64; 4]> = vec![];
    for (i, r) in order.iter().enumerate() {
        for e in r.error_text.iter().flat_map(|t| t.lines()) {
            st.problems.push((r.name.clone(), e.to_string()));
        }
        let Some((base, mask)) = pre[i] else {
            st.problems.push((
                r.name.clone(),
                "past the load-order limit (254 full / 4096 light plugins): its records are not mapped".into(),
            ));
            continue;
        };
        for (idx, m) in r.masters.iter().enumerate() {
            match index.get(&m.to_ascii_lowercase()).and_then(|&j| pre[j]) {
                Some((b, k)) => map.push([r.id, idx as i64, i64::from(b), i64::from(k)]),
                None => st.problems.push((r.name.clone(), format!("missing master {m}"))),
            }
        }
        map.push([r.id, r.masters.len() as i64, i64::from(base), i64::from(mask)]);
    }
    let same = work.is_empty()
        && removed == 0
        && order
            .iter()
            .enumerate()
            .all(|(i, r)| r.lo_index == Some(i as i64));
    if !same {
        let lo: Vec<(i64, i64)> = order.iter().enumerate().map(|(i, r)| (r.id, i as i64)).collect();
        store.resolve_plugins(&lo, &map)?;
    }
    st.records = store.plugin_record_count()?;
    st.ms = t.elapsed().as_millis() as u64;
    Ok(st)
}
```

- [ ] **Step 6: Run the tests**

Run: `cargo test -p modscan`
Expected: all pass. That includes the 8 new tests and `tests/store.rs`/`tests/textures.rs` schema tests (old versions still rebuild to `SCHEMA_VERSION`).

- [ ] **Step 7: Gates and commit**

```powershell
cargo fmt --all; cargo clippy --workspace --all-targets -- -D warnings; cargo test --workspace
git add crates/modscan/src/store.rs crates/modscan/src/plugin_store.rs crates/modscan/src/plugins.rs crates/modscan/src/scan.rs crates/modscan/src/lib.rs crates/modscan/tests/plugins.rs
git commit -m "modscan: plugin tables (schema 4), per-plugin cache, load-order FormIDs (full + FE), winners" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: the plugin pass inside the scan worker (`Report.plugins`, timing, Folder, owner timing test)

**Files**
- Modify: `crates/modscan/src/scan.rs` (`Report.plugins`, step 8 in `run_with`)
- Test: `crates/modscan/tests/scan.rs` (`report_has_phase_timings`), `crates/modscan/tests/plugins.rs`

**Interfaces**
- Consumes: Task 4 `scan_plugins`, `PluginStats`, `Store::unload_plugins`.
- Produces: `Report.plugins: Option<PluginStats>` (`Some` after an MO2 run whose plugin pass finished). `phase_ms` gets `("plugins", ms)` last. A cancel during the plugin pass returns `Ok` with the note `PLUGINS_CANCELLED` (`pub const` in `scan.rs`).

- [ ] **Step 1: Write the failing tests**

`crates/modscan/tests/scan.rs`, in `report_has_phase_timings`, change

```rust
    assert_eq!(
        names,
        ["origins", "walk", "parse", "write", "resolve", "textures"]
    );
```

to

```rust
    assert_eq!(
        names,
        ["origins", "walk", "parse", "write", "resolve", "textures", "plugins"]
    );
```

Append to `crates/modscan/tests/plugins.rs` (add `use modscan::scan::{run, Source};` to the `use` lines):

```rust
fn full_run(store: &mut Store, src: Source) -> modscan::scan::Report {
    run(&src, store, &AtomicBool::new(false), &|_| {}, &|| vec![]).unwrap()
}

/// Spec §2.2: the plugin pass runs in the scan worker after the NIF/texture pass, MO2 sources only.
#[test]
fn the_scan_runs_the_plugin_pass_for_mo2_only() {
    let d = load_order(ORDER);
    let mut store = Store::open(&d.path().join("scan.db")).unwrap();
    let r = full_run(&mut store, Source::Mo2(Instance::load(d.path(), None).unwrap()));
    let p = r.plugins.expect("plugin stats");
    assert_eq!((p.n, p.cached, p.records), (5, 0, 7));
    assert_eq!(r.phase_ms.last().map(|x| x.0), Some("plugins"));
    let other = tempfile::tempdir().unwrap();
    fs::create_dir_all(other.path().join("Loose")).unwrap();
    let r = full_run(&mut store, Source::Folder(other.path().join("Loose")));
    assert!(r.plugins.is_none());
    assert!(r.phase_ms.iter().all(|x| x.0 != "plugins"));
    assert_eq!(one_i64(&store, "SELECT count(*) FROM rec_winner", []), 0, "no load order after a Folder scan");
}

/// The owner's full load order (spec §1, §6). Read-only; the store is a temp file.
/// `$env:SDC_ACHERUS='C:\_mount\moddingvol\Acherus'; cargo test -p modscan --release --test plugins owner_load_order -- --ignored --nocapture`
#[test]
#[ignore]
fn owner_load_order() {
    let root = std::path::PathBuf::from(
        std::env::var("SDC_ACHERUS").expect("set SDC_ACHERUS to the MO2 instance root"),
    );
    let db = tempfile::tempdir().unwrap();
    let inst = Instance::load(&root, None).unwrap();
    let (loose, _) = loose_origins(&inst).unwrap();
    let mut store = Store::open(&db.path().join("scan.db")).unwrap();
    let go = |store: &mut Store| {
        let t = std::time::Instant::now();
        let s = scan_plugins(&inst, &loose, store, &AtomicBool::new(false), &|_| {}).unwrap();
        (s, t.elapsed())
    };
    let (cold, cold_t) = go(&mut store);
    let (warm, warm_t) = go(&mut store);
    println!(
        "cold: n={} records={} ms={} ({:.2} s); warm: cached={}/{} ({:.2} s)",
        cold.n, cold.records, cold.ms, cold_t.as_secs_f64(), warm.cached, warm.n, warm_t.as_secs_f64()
    );
    for p in cold.problems.iter().take(40) {
        println!("problem: {p:?}");
    }
    let (edid, id, slots): (String, i64, i64) = store
        .conn()
        .query_row(
            "SELECT r.edid, r.formid, count(s.slot) FROM rec r JOIN plugin p ON p.id = r.plugin_id
             JOIN txst_slot s ON s.plugin_id = r.plugin_id AND s.local = r.local
             WHERE p.name = 'Skyrim.esm' AND r.sig = 'TXST' AND r.edid IS NOT NULL GROUP BY r.plugin_id, r.local LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    println!("Skyrim.esm TXST: {edid} [{id:08X}], {slots} slot paths");
    assert_eq!(id >> 24, 0, "a Skyrim.esm record maps to load-order index 00");
    assert!(slots >= 1);
    for base in ["skyrim.esm", "update.esm", "dawnguard.esm", "hearthfires.esm", "dragonborn.esm"] {
        assert!(
            !cold.problems.iter().any(|(p, _)| p.eq_ignore_ascii_case(base)),
            "{base}: {:?}",
            cold.problems
        );
    }
    assert_eq!(warm.cached, warm.n);
    assert!(cold_t.as_secs_f64() < 10.0, "cold {cold_t:?}");
    assert!(warm_t.as_secs_f64() < 1.0, "warm {warm_t:?}");
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p modscan --test plugins --test scan`
Expected: compile error `no field plugins on Report`.

- [ ] **Step 3: Implement**

`crates/modscan/src/scan.rs`:
1. In `pub struct Report`, after `pub phase_ms: Vec<(&'static str, u64)>,` add:

```rust
    /// The plugin pass (esp spec §2.2): `Some` after an MO2 run whose plugin pass finished.
    pub plugins: Option<crate::plugins::PluginStats>,
```

and change the `phase_ms` doc's list to `origins, walk, parse, write, resolve, textures, plugins.`
2. After `const CHUNK: usize = 512;` add:

```rust
/// Note of a run cancelled during the plugin pass (ruling R13): the NIF results are complete.
pub const PLUGINS_CANCELLED: &str =
    "Plugin scan cancelled: plugin results are incomplete until the next scan.";
```

3. At the end of `run_with`, replace

```rust
    report.phase_ms.push(("textures", textures_ms));
    progress(Progress {
        phase: "resolve",
        done: 1,
        total: 1,
    });
    Ok(report)
}
```

with

```rust
    report.phase_ms.push(("textures", textures_ms));
    progress(Progress {
        phase: "resolve",
        done: 1,
        total: 1,
    });

    // 8. Plugins (esp spec §2.2), after the NIF/texture pass, same cancel and progress. A Folder has no load order.
    match src {
        Source::Mo2(inst) => match crate::plugins::scan_plugins(inst, &loose, store, cancel, progress) {
            Ok(p) => {
                report.phase_ms.push(("plugins", p.ms));
                report.plugins = Some(p);
            }
            Err(ScanError::Cancelled) => report.notes.push(PLUGINS_CANCELLED.into()),
            Err(e) => return Err(e),
        },
        Source::Folder(_) => store.unload_plugins()?,
    }
    Ok(report)
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p modscan`
Expected: all pass (`owner_load_order` ignored).

- [ ] **Step 5: Gates and commit**

```powershell
cargo fmt --all; cargo clippy --workspace --all-targets -- -D warnings; cargo test --workspace
git add crates/modscan/src/scan.rs crates/modscan/tests/scan.rs crates/modscan/tests/plugins.rs
git commit -m "modscan: plugin pass in the scan worker (Report.plugins, plugins timing, Folder unloads, owner timing test)" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 6: store joins — set → TXST, TXST → alternate-texture users, NIF → plugin overrides, missing / unused

**Files**
- Modify: `crates/modscan/src/store.rs` (extract `tex_query` from `nifs_using_texture`)
- Modify: `crates/modscan/src/plugin_store.rs` (row types and queries)
- Test: `crates/modscan/tests/plugin_joins.rs` (new)

**Interfaces**
- Consumes: Tasks 4–5; `TexKey`; the `tex_winner`, `nif_winner`, `tex_use`, `shape` tables.
- Produces (all in `modscan::plugin_store`, `Debug, Clone, PartialEq`):
  - `pub fn record_label(edid: Option<&str>, formid: u32, sig: &str, plugin: &str) -> String` (`"EDID [FORMID] SIG — Plugin"`, `(no EDID)` when none)
  - `TxstUse { formid: u32, edid: Option<String>, plugin: String, slot: u32, path: String, n_overrides: u32 }`
  - `ShapeSwap { shape_name: String, txst_formid: u32, txst_edid: Option<String>, index: i32 }`
  - `AltUse { formid: u32, edid: Option<String>, sig: String, plugin: String, model_slot: u32, swap: ShapeSwap }`
  - `MeshUse { formid: u32, edid: Option<String>, sig: String, plugin: String, model_slot: u32, n_overrides: u32, swaps: Vec<ShapeSwap> }`
  - `MissingRef { path: String, n_records: usize, records: Vec<String> /* ≤ 5 record_label */ }`
  - `Store::txsts_matching(&self, key: &TexKey, limit: usize) -> Result<Vec<TxstUse>, ScanError>` (winning TXSTs only)
  - `Store::alt_users_of(&self, txsts: &[u32]) -> Result<Vec<AltUse>, ScanError>` (winning records only)
  - `Store::plugin_uses_of_mesh(&self, norm: &str) -> Result<Vec<MeshUse>, ScanError>`
  - `Store::plugin_missing_files(&self) -> Result<Vec<MissingRef>, ScanError>`, `Store::unused_textures(&self) -> Result<Vec<String>, ScanError>` (ruling R8)
  - `store.rs`: `pub(crate) fn tex_query(key: &TexKey, eff: &str) -> Option<TexQuery>` (`TexQuery` becomes `pub(crate) type`)

- [ ] **Step 1: Write the failing tests**

`crates/modscan/tests/plugin_joins.rs`:

```rust
//! esp scanner spec §6 "Store joins": set → TXST, NIF → alternate texture, missing / unused.
mod common;
#[path = "../../esp/tests/common/mod.rs"]
mod b;
use b::{alt, edid, plugin, rec, sub, top, zstr};
use common::instance_fixture;
use modscan::mo2::Instance;
use modscan::plugin_store::{ShapeSwap, TxstUse};
use modscan::scan::{run, Report, Source};
use modscan::store::{Store, TexKey};
use std::path::Path;
use std::sync::atomic::AtomicBool;

const DIRT: u32 = 0x0000_0ABC;
const GHOST: u32 = 0x0000_0AB0;

fn put(root: &Path, rel: &str, bytes: &[u8]) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, bytes).unwrap();
}

/// Skyrim.esm: DirtTXST (TX00 dirt02, TX01 dirt02_n, TX02 old/gone), GhostTXST (missing texture), RockStat
/// (meshes/x.nif with Shape0 → DirtTXST), GhostStat (missing mesh), FireFX (EFSH, present texture). A.esp overrides
/// DirtTXST without TX02. Loose textures: dirt02, dirt02_n, effects/fire, unused/lonely, pbr/landscape/dirt02.
fn fixture() -> tempfile::TempDir {
    let d = instance_fixture("+B\n+A\n");
    let r = d.path();
    let nif = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../nif/tests/fixtures/TestNifFile_Skinned_SE.nif")).unwrap();
    put(r, "mods/A/meshes/x.nif", &nif);
    for t in [
        "landscape/dirt02.dds",
        "landscape/dirt02_n.dds",
        "effects/fire.dds",
        "unused/lonely.dds",
        "pbr/landscape/dirt02.dds",
    ] {
        put(r, &format!("mods/A/textures/{t}"), b"dds");
    }
    let dirt = |old: bool| {
        let mut s = vec![
            edid("DirtTXST"),
            sub(b"TX00", &zstr("Landscape\\Dirt02.dds")),
            sub(b"TX01", &zstr("Landscape\\Dirt02_n.dds")),
        ];
        if old {
            s.push(sub(b"TX02", &zstr("Old\\Gone.dds")));
        }
        rec(b"TXST", DIRT, &s)
    };
    put(
        r,
        "Game/Data/Skyrim.esm",
        &plugin(
            esp::FLAG_ESM,
            &[],
            &[
                top(b"TXST", &[dirt(true), rec(b"TXST", GHOST, &[edid("GhostTXST"), sub(b"TX00", &zstr("Missing\\NotHere.dds"))])]),
                top(
                    b"STAT",
                    &[
                        rec(b"STAT", 0xDEF, &[edid("RockStat"), sub(b"MODL", &zstr("x.nif")), sub(b"MODS", &alt(&[("Shape0", DIRT, 0)]))]),
                        rec(b"STAT", 0xD00, &[edid("GhostStat"), sub(b"MODL", &zstr("Missing\\Ghost.nif"))]),
                    ],
                ),
                top(b"EFSH", &[rec(b"EFSH", 0xE00, &[edid("FireFX"), sub(b"ICON", &zstr("Effects\\Fire.dds"))])]),
            ],
        ),
    );
    put(r, "mods/A/A.esp", &plugin(0, &["Skyrim.esm"], &[top(b"TXST", &[dirt(false)])]));
    put(r, "profiles/Default/plugins.txt", b"*A.esp\n");
    d
}

fn mo2(d: &tempfile::TempDir, store: &mut Store) -> Report {
    let src = Source::Mo2(Instance::load(d.path(), None).unwrap());
    run(&src, store, &AtomicBool::new(false), &|_| {}, &|| vec![]).unwrap()
}

fn dirt_key() -> TexKey {
    TexKey::Suffix { slot: 0, stem: "landscape\\dirt02".into() }
}

#[test]
fn set_key_finds_the_winning_txst() {
    let d = fixture();
    let mut s = Store::open(&d.path().join("scan.db")).unwrap();
    mo2(&d, &mut s);
    let want = TxstUse {
        formid: DIRT,
        edid: Some("DirtTXST".into()),
        plugin: "A.esp".into(),
        slot: 0,
        path: "textures/landscape/dirt02.dds".into(),
        n_overrides: 1,
    };
    assert_eq!(s.txsts_matching(&dirt_key(), 50).unwrap(), vec![want]);
    let normal = s
        .txsts_matching(&TexKey::Exact { slot: 1, path: "textures\\Landscape\\Dirt02_n.dds".into() }, 50)
        .unwrap();
    assert_eq!(normal.len(), 1);
    assert_eq!(normal[0].slot, 1);
    assert!(s.txsts_matching(&TexKey::Suffix { slot: 0, stem: "irt02".into() }, 50).unwrap().is_empty(), "whole components");
}

#[test]
fn txst_users_and_mesh_overrides() {
    let d = fixture();
    let mut s = Store::open(&d.path().join("scan.db")).unwrap();
    mo2(&d, &mut s);
    let swap = ShapeSwap { shape_name: "Shape0".into(), txst_formid: DIRT, txst_edid: Some("DirtTXST".into()), index: 0 };
    let users = s.alt_users_of(&[DIRT]).unwrap();
    assert_eq!(users.len(), 1);
    assert_eq!((users[0].edid.as_deref(), users[0].sig.as_str(), users[0].plugin.as_str()), (Some("RockStat"), "STAT", "Skyrim.esm"));
    assert_eq!(users[0].swap, swap);
    let uses = s.plugin_uses_of_mesh("meshes/x.nif").unwrap();
    assert_eq!(uses.len(), 1);
    assert_eq!((uses[0].formid, uses[0].model_slot, uses[0].n_overrides), (0xDEF, 0, 0));
    assert_eq!(uses[0].swaps, vec![swap]);
    assert!(s.plugin_uses_of_mesh("meshes/other.nif").unwrap().is_empty());
}

/// Review Focus 5: a path only an overridden copy names is not missing; PGPatcher output is not unused.
#[test]
fn missing_and_unused_lists() {
    let d = fixture();
    let mut s = Store::open(&d.path().join("scan.db")).unwrap();
    mo2(&d, &mut s);
    let missing = s.plugin_missing_files().unwrap();
    let paths: Vec<&str> = missing.iter().map(|m| m.path.as_str()).collect();
    assert_eq!(paths, ["meshes/missing/ghost.nif", "textures/missing/nothere.dds"]);
    assert_eq!(missing[1].records, ["GhostTXST [00000AB0] TXST — Skyrim.esm"]);
    assert_eq!(missing[1].n_records, 1);
    let unused = s.unused_textures().unwrap();
    assert!(unused.contains(&"textures/unused/lonely.dds".to_string()), "{unused:?}");
    for used in ["textures/landscape/dirt02.dds", "textures/landscape/dirt02_n.dds", "textures/effects/fire.dds", "textures/pbr/landscape/dirt02.dds"] {
        assert!(!unused.contains(&used.to_string()), "{used} listed: {unused:?}");
    }
}

/// Review Focus 4.
#[test]
fn a_folder_scan_hides_plugin_rows_and_the_next_mo2_scan_restores_them() {
    let d = fixture();
    let mut s = Store::open(&d.path().join("scan.db")).unwrap();
    mo2(&d, &mut s);
    let other = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(other.path().join("Loose")).unwrap();
    run(&Source::Folder(other.path().join("Loose")), &mut s, &AtomicBool::new(false), &|_| {}, &|| vec![]).unwrap();
    assert!(s.txsts_matching(&dirt_key(), 50).unwrap().is_empty());
    assert!(s.plugin_missing_files().unwrap().is_empty());
    let r = mo2(&d, &mut s);
    let p = r.plugins.unwrap();
    assert_eq!(p.cached, p.n, "nothing re-parsed");
    assert_eq!(s.txsts_matching(&dirt_key(), 50).unwrap().len(), 1);
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p modscan --test plugin_joins`
Expected: compile errors (`ShapeSwap`, `TxstUse`, `txsts_matching` not found).

- [ ] **Step 3: Extract `tex_query` in `store.rs`**

Change `type TexQuery = (u32, String, String, Box<dyn Fn(&str) -> bool>);` → `pub(crate) type TexQuery = (u32, String, String, Box<dyn Fn(&str) -> bool>);`

Add before `impl Store {`:

```rust
/// The slot, the SQL condition on `eff` (the stored slot string as an SQL expression, argument `?2`), its argument
/// and the exact post-filter for `key` (spec §2.4 of the PGPatcher plan); `None` when the key is empty (no match).
pub(crate) fn tex_query(key: &TexKey, eff: &str) -> Option<TexQuery> {
    // `pgp_form` minus the extension and `_n`/`_msn` cuts, which only shorten the tail: a superset prefilter.
    let pgp = format!(
        "(CASE WHEN {eff} LIKE 'textures/pbr/%' THEN 'textures/' || substr({eff}, 14) ELSE {eff} END)"
    );
    Some(match key {
        TexKey::Suffix { slot, stem } => {
            let stem = key_form(stem);
            if stem.is_empty() {
                return None;
            }
            let pattern = format!("%/{}%", like_escape(&stem));
            let want = format!("/{stem}");
            let normal = *slot == 1;
            (
                *slot,
                format!("{pgp} LIKE ?2 ESCAPE '\\'"),
                pattern,
                Box::new(move |p: &str| pgp_form(p, normal).ends_with(&want)),
            )
        }
        TexKey::Contains { slot, frag } => {
            let frag = key_form(frag);
            if frag.is_empty() {
                return None;
            }
            let pattern = format!("%{}%", like_escape(&frag));
            let normal = *slot == 1;
            (
                *slot,
                format!("{pgp} LIKE ?2 ESCAPE '\\'"),
                pattern,
                Box::new(move |p: &str| pgp_form(p, normal).contains(&frag)),
            )
        }
        TexKey::Exact { slot, path } => {
            let p = key_form(path);
            let p = if p.starts_with("textures/") {
                p
            } else {
                format!("textures/{p}")
            };
            (*slot, format!("{eff} = ?2"), p, Box::new(|_: &str| true))
        }
    })
}
```

In `nifs_using_texture`, delete the lines from `        // \`pgp_form\` minus the extension and \`_n\`/\`_msn\` cuts, which only shorten the tail: a superset prefilter.` through the end of the statement `let (slot, cond, arg, check): TexQuery = match key { … };`, keeping the `let eff = …;` line above them. In their place put:

```rust
        let Some((slot, cond, arg, check)) = tex_query(key, eff) else {
            return Ok(vec![]);
        };
```

The existing `nifs_using_texture` tests show the refactor changed nothing.

- [ ] **Step 4: Implement the queries in `plugin_store.rs`**

Change `use crate::store::{path_id, Store};` → `use crate::store::{path_id, tex_query, Store, TexKey};` and append:

```rust
/// Records listed per missing path (spec §4.4).
pub const MISSING_RECORDS: usize = 5;

/// One record as listed everywhere: `EDID [FORMID] SIG — Plugin`.
pub fn record_label(edid: Option<&str>, formid: u32, sig: &str, plugin: &str) -> String {
    format!("{} [{formid:08X}] {sig} — {plugin}", edid.unwrap_or("(no EDID)"))
}

/// A winning TXST whose slot matches a set's key.
#[derive(Debug, Clone, PartialEq)]
pub struct TxstUse {
    pub formid: u32,
    pub edid: Option<String>,
    /// The winning plugin.
    pub plugin: String,
    pub slot: u32,
    pub path: String,
    pub n_overrides: u32,
}

/// One alternate-texture entry: a NIF shape re-textured with a TXST.
#[derive(Debug, Clone, PartialEq)]
pub struct ShapeSwap {
    pub shape_name: String,
    pub txst_formid: u32,
    /// The EDID of the TXST's winning copy.
    pub txst_edid: Option<String>,
    pub index: i32,
}

/// A winning record whose alternate textures use a given TXST.
#[derive(Debug, Clone, PartialEq)]
pub struct AltUse {
    pub formid: u32,
    pub edid: Option<String>,
    pub sig: String,
    pub plugin: String,
    pub model_slot: u32,
    pub swap: ShapeSwap,
}

/// A winning record that uses a mesh, with its swaps for that model slot.
#[derive(Debug, Clone, PartialEq)]
pub struct MeshUse {
    pub formid: u32,
    pub edid: Option<String>,
    pub sig: String,
    pub plugin: String,
    pub model_slot: u32,
    pub n_overrides: u32,
    pub swaps: Vec<ShapeSwap>,
}

/// A path winning plugin records name that nothing provides (spec §4.4).
#[derive(Debug, Clone, PartialEq)]
pub struct MissingRef {
    pub path: String,
    pub n_records: usize,
    /// The first [`MISSING_RECORDS`], as [`record_label`].
    pub records: Vec<String>,
}

const SWAPS: &str = "SELECT a.shape_name, a.txst_formid, tr.edid, a.idx FROM alt_tex a
     LEFT JOIN rec_winner tw ON tw.formid = a.txst_formid
     LEFT JOIN rec tr ON tr.plugin_id = tw.plugin_id AND tr.formid = tw.formid";

fn swap_row(r: &rusqlite::Row, at: usize) -> rusqlite::Result<ShapeSwap> {
    Ok(ShapeSwap {
        shape_name: r.get(at)?,
        txst_formid: r.get::<_, i64>(at + 1)? as u32,
        txst_edid: r.get(at + 2)?,
        index: r.get::<_, i64>(at + 3)? as i32,
    })
}

/// Paths of winning records (`txst_slot`, `tex_ref`, `model_ref`) that are `textures/**.dds` or `meshes/**.nif`, not
/// FaceGen, and that no enabled origin's winner provides (ruling R8).
const MISSING_SQL: &str = "
WITH refs(path_id, formid, plugin_id, local) AS (
  SELECT t.path_id, t.formid, t.plugin_id, t.local FROM txst_slot t
    JOIN rec_winner w ON w.formid = t.formid AND w.plugin_id = t.plugin_id
  UNION ALL SELECT t.path_id, t.formid, t.plugin_id, t.local FROM tex_ref t
    JOIN rec_winner w ON w.formid = t.formid AND w.plugin_id = t.plugin_id
  UNION ALL SELECT m.path_id, m.formid, m.plugin_id, m.local FROM model_ref m
    JOIN rec_winner w ON w.formid = m.formid AND w.plugin_id = m.plugin_id)
SELECT DISTINCT p.p, r.edid, x.formid, r.sig, pl.name FROM refs x
JOIN path p ON p.id = x.path_id
JOIN rec r ON r.plugin_id = x.plugin_id AND r.local = x.local
JOIN plugin pl ON pl.id = x.plugin_id
WHERE p.p NOT LIKE 'textures/actors/character/facegendata/%'
  AND p.p NOT LIKE 'meshes/actors/character/facegendata/%'
  AND ((p.p LIKE 'textures/%.dds' AND NOT EXISTS (SELECT 1 FROM tex_winner tw
          JOIN origin o ON o.id = tw.origin_id AND o.enabled = 1 WHERE tw.path_id = x.path_id))
    OR (p.p LIKE 'meshes/%.nif' AND NOT EXISTS (SELECT 1 FROM nif_winner nw JOIN nif_copy c ON c.id = nw.copy_id
          JOIN origin o ON o.id = c.origin_id AND o.enabled = 1 WHERE nw.path_id = x.path_id)))
ORDER BY p.p, pl.name, x.formid";

/// Winning textures of the run that no NIF copy of the run and no record copy of an active plugin names, outside
/// `textures/pbr/` (ruling R8).
const UNUSED_SQL: &str = "
SELECT p.p FROM tex_winner tw JOIN origin o ON o.id = tw.origin_id AND o.enabled = 1
JOIN path p ON p.id = tw.path_id
WHERE p.p NOT LIKE 'textures/pbr/%'
  AND NOT EXISTS (SELECT 1 FROM tex_use tu JOIN shape s ON s.texset_id = tu.texset_id
        JOIN nif_copy c ON c.id = s.copy_id JOIN origin co ON co.id = c.origin_id AND co.enabled = 1
        WHERE tu.tex_path_id = tw.path_id)
  AND NOT EXISTS (SELECT 1 FROM txst_slot t JOIN plugin pl ON pl.id = t.plugin_id AND pl.lo_index IS NOT NULL
        WHERE t.path_id = tw.path_id)
  AND NOT EXISTS (SELECT 1 FROM tex_ref t JOIN plugin pl ON pl.id = t.plugin_id AND pl.lo_index IS NOT NULL
        WHERE t.path_id = tw.path_id)
ORDER BY p.p";

impl Store {
    /// Winning TXSTs whose slot matches `key` with PGPatcher's rule (ruling R9), by EDID; at most `limit`.
    pub fn txsts_matching(&self, key: &TexKey, limit: usize) -> Result<Vec<TxstUse>, ScanError> {
        let Some((slot, cond, arg, check)) = tex_query(key, "tp.p") else {
            return Ok(vec![]);
        };
        let mut st = self.conn.prepare(&format!(
            "SELECT ts.formid, r.edid, pl.name, ts.slot, tp.p, w.n_overrides FROM path tp
             JOIN txst_slot ts ON ts.path_id = tp.id AND ts.slot = ?1
             JOIN rec_winner w ON w.formid = ts.formid AND w.plugin_id = ts.plugin_id
             JOIN rec r ON r.plugin_id = ts.plugin_id AND r.local = ts.local
             JOIN plugin pl ON pl.id = ts.plugin_id
             WHERE {cond} ORDER BY r.edid, ts.formid"
        ))?;
        let mut rows = st.query(params![i64::from(slot), arg])?;
        let mut out = vec![];
        while let Some(r) = rows.next()? {
            let path: String = r.get(4)?;
            if !check(&path) {
                continue;
            }
            out.push(TxstUse {
                formid: r.get::<_, i64>(0)? as u32,
                edid: r.get(1)?,
                plugin: r.get(2)?,
                slot: r.get::<_, i64>(3)? as u32,
                path,
                n_overrides: r.get::<_, i64>(5)? as u32,
            });
            if out.len() >= limit {
                break;
            }
        }
        Ok(out)
    }

    /// Winning records whose alternate textures use one of `txsts` (load-order FormIDs).
    pub fn alt_users_of(&self, txsts: &[u32]) -> Result<Vec<AltUse>, ScanError> {
        let ids = serde_json::to_string(txsts).map_err(json_err)?;
        let mut st = self.conn.prepare(
            "SELECT a.formid, r.edid, r.sig, pl.name, a.model_slot, a.shape_name, a.txst_formid, tr.edid, a.idx
             FROM alt_tex a
             JOIN rec_winner w ON w.formid = a.formid AND w.plugin_id = a.plugin_id
             JOIN rec r ON r.plugin_id = a.plugin_id AND r.local = a.local
             JOIN plugin pl ON pl.id = a.plugin_id
             LEFT JOIN rec_winner tw ON tw.formid = a.txst_formid
             LEFT JOIN rec tr ON tr.plugin_id = tw.plugin_id AND tr.formid = tw.formid
             WHERE a.txst_formid IN (SELECT value FROM json_each(?1))
             ORDER BY r.edid, a.formid, a.model_slot, a.idx",
        )?;
        let v = st
            .query_map([ids], |r| {
                Ok(AltUse {
                    formid: r.get::<_, i64>(0)? as u32,
                    edid: r.get(1)?,
                    sig: r.get(2)?,
                    plugin: r.get(3)?,
                    model_slot: r.get::<_, i64>(4)? as u32,
                    swap: swap_row(r, 5)?,
                })
            })?
            .collect::<Result<_, _>>()?;
        Ok(v)
    }

    /// Winning records whose model (any slot) is `norm`, with the swaps of that model slot (spec §4.2).
    pub fn plugin_uses_of_mesh(&self, norm: &str) -> Result<Vec<MeshUse>, ScanError> {
        let mut st = self.conn.prepare(
            "SELECT m.formid, r.edid, r.sig, pl.name, m.slot, w.n_overrides, m.plugin_id, m.local
             FROM path p JOIN model_ref m ON m.path_id = p.id
             JOIN rec_winner w ON w.formid = m.formid AND w.plugin_id = m.plugin_id
             JOIN rec r ON r.plugin_id = m.plugin_id AND r.local = m.local
             JOIN plugin pl ON pl.id = m.plugin_id
             WHERE p.p = ?1 ORDER BY r.sig, r.edid, m.formid, m.slot",
        )?;
        let rows: Vec<(MeshUse, i64, i64)> = st
            .query_map([norm], |r| {
                Ok((
                    MeshUse {
                        formid: r.get::<_, i64>(0)? as u32,
                        edid: r.get(1)?,
                        sig: r.get(2)?,
                        plugin: r.get(3)?,
                        model_slot: r.get::<_, i64>(4)? as u32,
                        n_overrides: r.get::<_, i64>(5)? as u32,
                        swaps: vec![],
                    },
                    r.get(6)?,
                    r.get(7)?,
                ))
            })?
            .collect::<Result<_, _>>()?;
        let mut sw = self.conn.prepare(&format!(
            "{SWAPS} WHERE a.plugin_id = ?1 AND a.local = ?2 AND a.model_slot = ?3 ORDER BY a.idx, a.shape_name"
        ))?;
        let mut out = Vec::with_capacity(rows.len());
        for (mut u, plugin_id, local) in rows {
            u.swaps = sw
                .query_map(params![plugin_id, local, i64::from(u.model_slot)], |r| swap_row(r, 0))?
                .collect::<Result<_, _>>()?;
            out.push(u);
        }
        Ok(out)
    }

    /// Missing files (spec §4.4, ruling R8), by path, each with up to [`MISSING_RECORDS`] referencing records.
    pub fn plugin_missing_files(&self) -> Result<Vec<MissingRef>, ScanError> {
        let mut st = self.conn.prepare(MISSING_SQL)?;
        let mut rows = st.query([])?;
        let mut out: Vec<MissingRef> = vec![];
        while let Some(r) = rows.next()? {
            let path: String = r.get(0)?;
            let edid: Option<String> = r.get(1)?;
            let label = record_label(
                edid.as_deref(),
                r.get::<_, i64>(2)? as u32,
                &r.get::<_, String>(3)?,
                &r.get::<_, String>(4)?,
            );
            match out.last_mut() {
                Some(m) if m.path == path => {
                    m.n_records += 1;
                    if m.records.len() < MISSING_RECORDS {
                        m.records.push(label);
                    }
                }
                _ => out.push(MissingRef {
                    path,
                    n_records: 1,
                    records: vec![label],
                }),
            }
        }
        Ok(out)
    }

    /// Unused textures (spec §4.4, ruling R8), by path.
    pub fn unused_textures(&self) -> Result<Vec<String>, ScanError> {
        let mut st = self.conn.prepare(UNUSED_SQL)?;
        let v = st
            .query_map([], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        Ok(v)
    }
}
```

- [ ] **Step 5: Run the tests**

Run: `cargo test -p modscan`
Expected: all pass. That includes the 4 new `plugin_joins` tests and every existing `nifs_using_texture` test (the refactor is behaviour-neutral).

- [ ] **Step 6: Gates and commit**

```powershell
cargo fmt --all; cargo clippy --workspace --all-targets -- -D warnings; cargo test --workspace
git add crates/modscan/src/store.rs crates/modscan/src/plugin_store.rs crates/modscan/tests/plugin_joins.rs
git commit -m "modscan: plugin joins (set key -> TXST, TXST -> alternate textures, mesh -> plugin overrides, missing/unused)" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 7: Meshes page — "Scan details" plugin line, NIF "Plugin overrides", "Plugin file references"

**Files**
- Modify: `crates/gui/src/model/meshes.rs` (fields, `load_shapes`, `finished`, `load_file_refs`, `refs_lines`, helpers, `log_plugins`)
- Modify: `crates/gui/src/ui/meshes.rs` (`bar`, `detail`, `plugin_overrides`, `file_refs`)
- Test: `crates/gui/tests/meshes_view.rs`

**Interfaces**
- Consumes: Task 5 `Report.plugins`, `PluginStats`; Task 6 `MeshUse`, `ShapeSwap`, `MissingRef`, `record_label`, `Store::plugin_uses_of_mesh`, `plugin_missing_files`, `unused_textures`.
- Produces (in `sdc_gui::model::meshes`):
  - `MeshesModel` pub fields `plugin_uses: Vec<MeshUse>`, `file_refs: Option<FileRefs>`, `refs_unused: bool`, `refs_filter: String`; `pub fn has_store(&self) -> bool`, `pub fn load_file_refs(&mut self)`, `pub fn refs_lines(&mut self) -> &[String]`
  - `#[derive(Debug, Clone, Default, PartialEq)] pub struct FileRefs { pub missing: Vec<MissingRef>, pub unused: Vec<String> }`
  - `pub fn plugins_line(p: &PluginStats) -> String` (`"Plugins: N (C cached), R records, T ms"`), `pub fn overridden_text(n: u32) -> String` (`""` or `" · overridden N×"`), `pub fn file_refs_lines(r: &FileRefs, unused: bool, filter: &str) -> Vec<String>`, `pub fn log_plugins(r: &Report)` (perf_log event `plugins`)
  - `pub const MISSING_NOTE: &str`, `pub const UNUSED_NOTE: &str`

- [ ] **Step 1: Write the failing tests**

Append to `crates/gui/tests/meshes_view.rs` (add to the `use` lines: `use modscan::plugin_store::{MeshUse, MissingRef, ShapeSwap};`, `use modscan::plugins::PluginStats;`, `use modscan::scan::Report;`, and add `FileRefs` to the `sdc_gui::model::meshes::{…}` import):

```rust
fn stats(n: usize, cached: usize, records: usize, ms: u64, problems: &[(&str, &str)]) -> PluginStats {
    PluginStats {
        n,
        cached,
        records,
        ms,
        problems: problems.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect(),
    }
}

/// esp spec §4.3: one plugin line, then parse errors and missing masters by plugin.
#[test]
fn scan_details_show_the_plugin_line_and_problems() {
    let mut m = model_with(None);
    m.report = Some(Report {
        plugins: Some(stats(3, 2, 120, 45, &[("M.esp", "missing master Missing.esm")])),
        ..Default::default()
    });
    let mut h = harness(m);
    h.run();
    h.get_by_label("Scan details").click();
    h.run();
    h.get_by_label("Plugins: 3 (2 cached), 120 records, 45 ms");
    h.get_by_label("Plugin M.esp: missing master Missing.esm");
}

#[test]
fn scan_details_show_an_empty_plugin_pass() {
    let mut m = model_with(None);
    m.report = Some(Report {
        plugins: Some(PluginStats::default()),
        ..Default::default()
    });
    let mut h = harness(m);
    h.run();
    h.get_by_label("Scan details").click();
    h.run();
    h.get_by_label("Plugins: 0 (0 cached), 0 records, 0 ms");
}

/// esp spec §4.2: records that use the mesh, and per re-textured shape `shape → TXST EDID`. No PGPatcher warning.
#[test]
fn plugin_overrides_list_records_and_shape_swaps() {
    let mut m = model_with(None);
    m.nifs = vec![nif(1, "meshes/x.nif")];
    m.selected_nif = Some(1);
    m.plugin_uses = vec![MeshUse {
        formid: 0x0001_2345,
        edid: Some("RockStat".into()),
        sig: "STAT".into(),
        plugin: "Skyrim.esm".into(),
        model_slot: 0,
        n_overrides: 2,
        swaps: vec![ShapeSwap {
            shape_name: "Shape0".into(),
            txst_formid: 0xABC,
            txst_edid: Some("DirtTXST".into()),
            index: 0,
        }],
    }];
    let mut h = harness(m);
    h.run();
    h.get_by_label("Plugin overrides (1)").click();
    h.run();
    h.get_by_label("RockStat [00012345] STAT — Skyrim.esm · overridden 2×");
    h.get_by_label("Shape0 → DirtTXST [00000ABC]");
}

#[test]
fn plugin_overrides_say_when_no_record_uses_the_mesh() {
    let mut m = model_with(None);
    m.nifs = vec![nif(1, "meshes/x.nif")];
    m.selected_nif = Some(1);
    let mut h = harness(m);
    h.run();
    h.get_by_label("Plugin overrides (0)").click();
    h.run();
    h.get_by_label("No plugin record of the latest scan uses this mesh.");
}

/// esp spec §4.4: counts, filter, copy; no actions.
#[test]
fn plugin_file_references_list_missing_and_unused_with_a_filter() {
    let mut m = model_with(None);
    m.file_refs = Some(FileRefs {
        missing: vec![MissingRef {
            path: "textures/missing/nothere.dds".into(),
            n_records: 6,
            records: vec!["GhostTXST [00000AB0] TXST — Skyrim.esm".into()],
        }],
        unused: vec!["textures/unused/lonely.dds".into(), "textures/other/x.dds".into()],
    });
    let mut h = harness(m);
    h.run();
    h.get_by_label("Plugin file references").click();
    h.run();
    h.get_by_label("textures/missing/nothere.dds ← GhostTXST [00000AB0] TXST — Skyrim.esm; … 5 more");
    h.get_by_label("Copy");
    h.get_by_label("Unused (2)").click();
    h.run();
    h.get_by_label("textures/unused/lonely.dds");
    h.state_mut().refs_filter = "LONELY".into();
    h.run();
    assert!(h.query_by_label("textures/other/x.dds").is_none());
    assert!(h.query_by_label_contains("Delete").is_none() && h.query_by_label_contains("Move").is_none());
}

#[test]
fn plugin_file_references_when_nothing_is_listed() {
    let mut m = model_with(None);
    m.file_refs = Some(FileRefs::default());
    let mut h = harness(m);
    h.run();
    h.get_by_label("Plugin file references").click();
    h.run();
    h.get_by_label("Missing (0)");
    h.get_by_label("Nothing listed.");
}

/// Ruling R6: the timing log gets one `plugins` line per finished scan.
#[test]
fn a_finished_scan_logs_the_plugin_timing() {
    use sdc_gui::model::meshes::log_plugins;
    let r = Report {
        plugins: Some(stats(3, 2, 120, 45, &[])),
        ..Default::default()
    };
    let ((), ev) = render3d::perf_log::capture(|| log_plugins(&r));
    assert_eq!(ev.len(), 1, "{ev:?}");
    assert_eq!(ev[0]["event"], "plugins");
    assert_eq!((ev[0]["n"].as_u64(), ev[0]["cached"].as_u64(), ev[0]["records"].as_u64(), ev[0]["ms"].as_u64()), (Some(3), Some(2), Some(120), Some(45)));
    let ((), none) = render3d::perf_log::capture(|| log_plugins(&Report::default()));
    assert!(none.is_empty());
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p sdc-gui --test meshes_view`
Expected: compile errors (`plugin_uses`, `file_refs`, `FileRefs`, `log_plugins` not found).

- [ ] **Step 3: Implement the model**

`crates/gui/src/model/meshes.rs`:
1. After `use modscan::ScanError;` add:

```rust
use modscan::plugin_store::{MeshUse, MissingRef};
use modscan::plugins::PluginStats;
use render3d::perf_log;
use serde_json::json;
```

2. Before `pub struct MeshesModel {` add:

```rust
/// "Plugin file references" (esp spec §4.4): information only.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FileRefs {
    pub missing: Vec<MissingRef>,
    pub unused: Vec<String>,
}

pub const MISSING_NOTE: &str = "Paths that winning plugin records name and that no enabled mod, the game Data folder or an active BSA provides. Information only.";
pub const UNUSED_NOTE: &str = "Textures that no NIF and no plugin record names. textures\\pbr\\ is left out (PGPatcher output); LOD, interface and engine textures are listed too. Information only.";

/// The scan details line of the plugin pass (esp spec §4.3).
pub fn plugins_line(p: &PluginStats) -> String {
    format!(
        "Plugins: {} ({} cached), {} records, {} ms",
        p.n, p.cached, p.records, p.ms
    )
}

/// " · overridden N×" after a record that later plugins override; empty otherwise.
pub fn overridden_text(n: u32) -> String {
    if n == 0 {
        String::new()
    } else {
        format!(" · overridden {n}×")
    }
}

/// The listed lines of one tab. `filter`: case-insensitive substring of the path, `\` read as `/`.
pub fn file_refs_lines(r: &FileRefs, unused: bool, filter: &str) -> Vec<String> {
    let needle = filter.trim().replace('\\', "/").to_lowercase();
    let keep = |p: &str| needle.is_empty() || p.contains(&needle);
    if unused {
        return r.unused.iter().filter(|p| keep(p)).cloned().collect();
    }
    r.missing
        .iter()
        .filter(|m| keep(&m.path))
        .map(|m| {
            let more = m.n_records.saturating_sub(m.records.len());
            let tail = if more > 0 {
                format!("; … {more} more")
            } else {
                String::new()
            };
            format!("{} ← {}{tail}", m.path, m.records.join("; "))
        })
        .collect()
}

/// Ruling R6: one timing log line per finished plugin pass.
pub fn log_plugins(r: &Report) {
    if let Some(p) = &r.plugins {
        perf_log::emit(
            "plugins",
            json!({"n": p.n, "cached": p.cached, "records": p.records, "ms": p.ms}),
        );
    }
}
```

3. In `pub struct MeshesModel`, after `pub tex_users: Option<(String, Vec<TexUser>)>,` add:

```rust
    /// Winning plugin records that use the shown mesh (esp spec §4.2).
    pub plugin_uses: Vec<MeshUse>,
    /// "Plugin file references", read when the section is first opened after a scan.
    pub file_refs: Option<FileRefs>,
    /// That section's tab (false: Missing) and filter.
    pub refs_unused: bool,
    pub refs_filter: String,
    /// The lines for (tab, filter), kept until either changes.
    refs_cache: Option<((bool, String), Vec<String>)>,
```

and in `MeshesModel::new`'s literal, after `tex_users: None,` add:

```rust
            plugin_uses: vec![],
            file_refs: None,
            refs_unused: false,
            refs_filter: String::new(),
            refs_cache: None,
```

4. In `finished`, replace `                self.status = report_summary(&report);` with

```rust
                self.status = report_summary(&report);
                log_plugins(&report);
                self.file_refs = None;
                self.refs_cache = None;
```

5. In `clear_nif`, after `self.tex_users = None;` add `self.plugin_uses.clear();`.
6. In `load_shapes`, replace

```rust
            Ok(s) => {
                let spec = mesh_spec(store, copy, &s);
```

with

```rust
            Ok(s) => {
                let uses = store.copy_location(copy).and_then(|l| match l {
                    Some((path, ..)) => store.plugin_uses_of_mesh(&path),
                    None => Ok(vec![]),
                });
                self.plugin_uses = match uses {
                    Ok(u) => u,
                    Err(e) => {
                        self.error = Some(db_err(e));
                        vec![]
                    }
                };
                let spec = mesh_spec(store, copy, &s);
```

7. After `pub fn show_stale_list(&mut self) { … }` add:

```rust
    pub fn has_store(&self) -> bool {
        self.store.is_some()
    }

    /// "Plugin file references" opened: the Missing and Unused lists (esp spec §4.4). A failed read is shown and
    /// leaves empty lists, so it is not retried every frame.
    pub fn load_file_refs(&mut self) {
        let Some(store) = &self.store else {
            return;
        };
        let r = store
            .plugin_missing_files()
            .and_then(|missing| Ok(FileRefs { missing, unused: store.unused_textures()? }));
        self.file_refs = Some(match r {
            Ok(r) => r,
            Err(e) => {
                self.error = Some(db_err(e));
                FileRefs::default()
            }
        });
        self.refs_cache = None;
    }

    /// The listed lines for the current tab and filter.
    pub fn refs_lines(&mut self) -> &[String] {
        let key = (self.refs_unused, self.refs_filter.clone());
        if self.refs_cache.as_ref().map(|c| &c.0) != Some(&key) {
            let lines = self
                .file_refs
                .as_ref()
                .map(|r| file_refs_lines(r, key.0, &key.1))
                .unwrap_or_default();
            self.refs_cache = Some((key, lines));
        }
        self.refs_cache.as_ref().map_or(&[][..], |c| c.1.as_slice())
    }
```

- [ ] **Step 4: Implement the UI**

`crates/gui/src/ui/meshes.rs`:
1. In the first `use` (from `crate::model::meshes`), replace `display_path, shader_type_name, slot_name, Banner, MeshesModel, ModLine, ScanTarget,` with `display_path, overridden_text, plugins_line, shader_type_name, slot_name, Banner, MeshesModel, ModLine, ScanTarget, MISSING_NOTE, UNUSED_NOTE,`. After `use modscan::store::{…};` add `use modscan::plugin_store::{record_label, MeshUse};`.
2. In `bar`, replace

```rust
        if !r.notes.is_empty() || !r.missing_mods.is_empty() || !r.problems.is_empty() {
```

with

```rust
        if !r.notes.is_empty()
            || !r.missing_mods.is_empty()
            || !r.problems.is_empty()
            || r.plugins.is_some()
        {
```

and inside its scroll area replace `                            for n in &r.notes {` with

```rust
                            if let Some(p) = &r.plugins {
                                ui.label(plugins_line(p));
                                for (plugin, msg) in p.problems.iter().take(MAX_LISTED) {
                                    ui.label(format!("Plugin {plugin}: {msg}"));
                                }
                                if p.problems.len() > MAX_LISTED {
                                    ui.label(format!(
                                        "… and {} more plugin problems",
                                        p.problems.len() - MAX_LISTED
                                    ));
                                }
                            }
                            for n in &r.notes {
```

3. In `bar`, insert before `    if let Some(e) = m.error.clone() {`:

```rust
    if m.has_store() || m.file_refs.is_some() {
        egui::CollapsingHeader::new("Plugin file references")
            .id_salt("meshes-plugin-file-refs")
            .show(ui, |ui| {
                if m.file_refs.is_none() {
                    m.load_file_refs();
                }
                file_refs(ui, m);
            });
    }
```

4. In `detail`, after the block

```rust
        if let Some(c) = &n.derive_check {
            ui.label(format!("PGPatcher check: {c}"));
        }
```

add `        plugin_overrides(ui, &m.plugin_uses);`.
5. Before `/// One shape; returns the texture path that was clicked.` add:

```rust
/// Winning plugin records that use this mesh and the shapes they re-texture (esp spec §4.2). Information only.
fn plugin_overrides(ui: &mut egui::Ui, uses: &[MeshUse]) {
    egui::CollapsingHeader::new(format!("Plugin overrides ({})", uses.len()))
        .id_salt("meshes-plugin-overrides")
        .show(ui, |ui| {
            if uses.is_empty() {
                ui.label("No plugin record of the latest scan uses this mesh.");
            }
            for (i, u) in uses.iter().take(MAX_LISTED).enumerate() {
                ui.label(format!(
                    "{}{}",
                    record_label(u.edid.as_deref(), u.formid, &u.sig, &u.plugin),
                    overridden_text(u.n_overrides)
                ));
                ui.indent(("plugin-swaps", i), |ui| {
                    for s in &u.swaps {
                        ui.label(format!(
                            "{} → {} [{:08X}]",
                            s.shape_name,
                            s.txst_edid.as_deref().unwrap_or("(no EDID)"),
                            s.txst_formid
                        ));
                    }
                });
            }
        });
}

/// "Plugin file references" (esp spec §4.4): counts, filter, copy; no actions.
fn file_refs(ui: &mut egui::Ui, m: &mut MeshesModel) {
    let (missing, unused) = m
        .file_refs
        .as_ref()
        .map_or((0, 0), |r| (r.missing.len(), r.unused.len()));
    ui.horizontal(|ui| {
        if ui
            .selectable_label(!m.refs_unused, format!("Missing ({missing})"))
            .clicked()
        {
            m.refs_unused = false;
        }
        if ui
            .selectable_label(m.refs_unused, format!("Unused ({unused})"))
            .clicked()
        {
            m.refs_unused = true;
        }
        ui.add(
            egui::TextEdit::singleline(&mut m.refs_filter)
                .hint_text("filter")
                .desired_width(200.0),
        );
        if ui.button("Copy").clicked() {
            let text = m.refs_lines().join("\n");
            ui.ctx().copy_text(text);
        }
    });
    ui.weak(if m.refs_unused { UNUSED_NOTE } else { MISSING_NOTE });
    let n = m.refs_lines().len();
    if n == 0 {
        ui.label("Nothing listed.");
        return;
    }
    let row = ui.text_style_height(&egui::TextStyle::Body);
    egui::ScrollArea::vertical()
        .id_salt("meshes-file-refs-scroll")
        .max_height(240.0)
        .show_rows(ui, row, n, |ui, range| {
            for l in &m.refs_lines()[range] {
                ui.label(l.as_str());
            }
        });
}
```

- [ ] **Step 5: Run the tests**

Run: `cargo test -p sdc-gui --test meshes_view`
Expected: all pass, including the 7 new tests.

- [ ] **Step 6: Gates and commit**

```powershell
cargo fmt --all; cargo clippy --workspace --all-targets -- -D warnings; cargo test --workspace
git add crates/gui/src/model/meshes.rs crates/gui/src/ui/meshes.rs crates/gui/tests/meshes_view.rs
git commit -m "gui: Meshes page plugin sections (scan details line, NIF plugin overrides, plugin file references, plugins timing line)" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 8: set editor — collapsible "Used by plugins"

**Files**
- Create: `crates/gui/src/model/plugin_refs.rs`, `crates/gui/src/ui/plugin_refs.rs`
- Modify (shared, own commit, add-only): `crates/gui/src/model/mod.rs` (`pub mod plugin_refs;`, `AppModel.plugin_refs` field + init), `crates/gui/src/ui/mod.rs` (`pub mod plugin_refs;`), `crates/gui/src/app.rs` (store path)
- Modify: `crates/gui/src/ui/set_editor.rs` (one call)
- Test: `crates/gui/tests/set_editor_ui.rs`

**Interfaces**
- Consumes: Task 6 `Store::txsts_matching`, `alt_users_of`, `TxstUse`, `AltUse`, `record_label`; Task 7 `overridden_text`; `set_preview::tex_key` (pub); `sets::SetView` (pub, read-only).
- Produces:
  - `sdc_gui::model::plugin_refs::{SetPluginRefs, SetPluginUse, set_key, LIMIT, NO_SCAN}`
  - `pub struct SetPluginRefs { pub store_path: Option<PathBuf>, pub cached: Option<(TexKey, Result<SetPluginUse, String>)> }` with `new(Option<PathBuf>)`, `get(&mut self, &TexKey) -> &Result<SetPluginUse, String>`, `refresh(&mut self)`
  - `pub struct SetPluginUse { pub txsts: Vec<TxstUse>, pub alts: Vec<AltUse>, pub truncated: bool }`
  - `pub fn set_key(v: &SetView) -> Option<TexKey>`
  - `AppModel.plugin_refs: SetPluginRefs` (`Default` = no store path: tests never read a database)
  - `sdc_gui::ui::plugin_refs::set_section(ui, model: &mut AppModel, v: &SetView)`

- [ ] **Step 1: Write the failing tests**

Append to `crates/gui/tests/set_editor_ui.rs`:

```rust
/// A taller window: the section sits under the slot cards.
fn tall(model: AppModel) -> Harness<'static, AppModel> {
    let h = Harness::builder()
        .with_size(eframe::egui::vec2(1400.0, 1800.0))
        .build_ui_state(|ui, model: &mut AppModel| sdc_gui::ui::draw(ui, model), model);
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h
}

/// esp spec §4.1: TXSTs matching the set (EDID, load-order FormID, winning plugin, slot, overridden N×), then the
/// records whose alternate textures use them. Information only.
#[test]
fn used_by_plugins_lists_txsts_and_alternate_texture_users() {
    use modscan::plugin_store::{AltUse, ShapeSwap, TxstUse};
    use sdc_gui::model::plugin_refs::{set_key, SetPluginUse};
    use sdc_gui::model::sets::set_views;
    use sdc_gui::model::Selection;
    let d = tempfile::tempdir().unwrap();
    let mut model = pbr_opened(d.path());
    let v = set_views(&model)
        .into_iter()
        .find(|v| set_key(v).is_some())
        .expect("a set with a match key");
    let key = set_key(&v).unwrap();
    model.plugin_refs.cached = Some((
        key,
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
        }),
    ));
    model.selection = Selection::Set(v.key.clone());
    let mut h = tall(model);
    h.run();
    h.get_by_label("Used by plugins").click();
    h.run();
    h.get_by_label("RockTXST [00012345] TXST — Skyrim.esm · slot 0: textures/landscape/rock01.dds · overridden 1×");
    h.get_by_label("RockStat [00000DEF] STAT — Skyrim.esm · Shape0 → RockTXST");
}

#[test]
fn used_by_plugins_without_a_scan_says_so() {
    use sdc_gui::model::plugin_refs::{set_key, NO_SCAN};
    use sdc_gui::model::sets::set_views;
    use sdc_gui::model::Selection;
    let d = tempfile::tempdir().unwrap();
    let mut model = pbr_opened(d.path());
    let v = set_views(&model).into_iter().find(|v| set_key(v).is_some()).unwrap();
    model.selection = Selection::Set(v.key.clone());
    let mut h = tall(model);
    h.run();
    h.get_by_label("Used by plugins").click();
    h.run();
    h.get_by_label(NO_SCAN);
}

#[test]
fn used_by_plugins_says_when_nothing_matches() {
    use sdc_gui::model::plugin_refs::{set_key, SetPluginUse};
    use sdc_gui::model::sets::set_views;
    use sdc_gui::model::Selection;
    let d = tempfile::tempdir().unwrap();
    let mut model = pbr_opened(d.path());
    let v = set_views(&model).into_iter().find(|v| set_key(v).is_some()).unwrap();
    model.plugin_refs.cached = Some((set_key(&v).unwrap(), Ok(SetPluginUse::default())));
    model.selection = Selection::Set(v.key.clone());
    let mut h = tall(model);
    h.run();
    h.get_by_label("Used by plugins").click();
    h.run();
    h.get_by_label("No texture set of the latest scan matches this set.");
}

/// Ruling R9: the set's own match key, with the derived `match_diffuse` when it names none.
#[test]
fn the_set_key_is_pgpatchers_match_rule() {
    use modscan::store::TexKey;
    use sdc_gui::model::plugin_refs::set_key;
    use sdc_gui::model::sets::set_views;
    let d = tempfile::tempdir().unwrap();
    let model = pbr_opened(d.path());
    let mut v = set_views(&model).into_iter().next().unwrap();
    v.settings.clear();
    v.derived_match = Some("landscape\\rock01.dds".into());
    assert_eq!(set_key(&v), Some(TexKey::Suffix { slot: 0, stem: "landscape\\rock01".into() }));
    v.settings.insert("match_normal".into(), serde_json::json!("landscape\\rock01_n.dds"));
    assert_eq!(set_key(&v), Some(TexKey::Suffix { slot: 1, stem: "landscape\\rock01_n".into() }));
    v.settings.clear();
    v.derived_match = None;
    assert_eq!(set_key(&v), None);
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p sdc-gui --test set_editor_ui`
Expected: compile errors (`sdc_gui::model::plugin_refs` not found, no field `plugin_refs`).

- [ ] **Step 3: Implement the model and the hooks**

`crates/gui/src/model/plugin_refs.rs`:

```rust
//! "Used by plugins" in the set editor (esp scanner spec §4.1): the TXSTs whose slot paths match the set's match key
//! (PGPatcher's own rule, `set_preview::tex_key`, ruling R9) and the records whose alternate textures use them.
use crate::model::sets::SetView;
use modscan::plugin_store::{AltUse, TxstUse};
use modscan::store::{Store, TexKey};
use serde_json::{Map, Value};
use std::path::PathBuf;

/// TXSTs listed; one more is read to say "only the first … are listed".
pub const LIMIT: usize = 200;
pub const NO_SCAN: &str = "No scan yet: scan the MO2 instance in the Meshes view.";

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SetPluginUse {
    pub txsts: Vec<TxstUse>,
    pub alts: Vec<AltUse>,
    pub truncated: bool,
}

/// The section's store reads, one per key (and on Refresh).
#[derive(Debug, Default)]
pub struct SetPluginRefs {
    /// `None` (tests, no %LOCALAPPDATA%): nothing is read.
    pub store_path: Option<PathBuf>,
    /// The key the result answers, and the result.
    pub cached: Option<(TexKey, Result<SetPluginUse, String>)>,
}

impl SetPluginRefs {
    pub fn new(store_path: Option<PathBuf>) -> SetPluginRefs {
        SetPluginRefs {
            store_path,
            cached: None,
        }
    }

    /// The result for `key`; read from the scan database when the key changed or after `refresh`. Only called while
    /// the section is open.
    pub fn get(&mut self, key: &TexKey) -> &Result<SetPluginUse, String> {
        if self.cached.as_ref().is_none_or(|(k, _)| k != key) {
            let r = self.query(key);
            self.cached = Some((key.clone(), r));
        }
        match &self.cached {
            Some((_, r)) => r,
            None => unreachable!("set just above"),
        }
    }

    pub fn refresh(&mut self) {
        self.cached = None;
    }

    /// Its own read-only connection: a scan may be writing.
    fn query(&self, key: &TexKey) -> Result<SetPluginUse, String> {
        let path = self
            .store_path
            .as_ref()
            .filter(|p| p.is_file())
            .ok_or_else(|| NO_SCAN.to_string())?;
        let store = Store::open_read_only(path).map_err(|e| e.to_string())?;
        let mut txsts = store
            .txsts_matching(key, LIMIT + 1)
            .map_err(|e| e.to_string())?;
        let truncated = txsts.len() > LIMIT;
        txsts.truncate(LIMIT);
        let ids: Vec<u32> = txsts.iter().map(|t| t.formid).collect();
        let alts = store.alt_users_of(&ids).map_err(|e| e.to_string())?;
        Ok(SetPluginUse {
            txsts,
            alts,
            truncated,
        })
    }
}

/// The set's match key as PGPatcher reads it: the set's settings, `match_diffuse` defaulting to the derived match.
pub fn set_key(v: &SetView) -> Option<TexKey> {
    let mut entry: Map<String, Value> = v
        .settings
        .iter()
        .map(|(k, x)| (k.clone(), x.clone()))
        .collect();
    if let Some(d) = &v.derived_match {
        entry
            .entry("match_diffuse")
            .or_insert_with(|| Value::String(d.clone()));
    }
    crate::model::set_preview::tex_key(&Value::Object(entry), &Value::Null).map(|(k, _)| k)
}
```

`crates/gui/src/ui/plugin_refs.rs`:

```rust
//! "Used by plugins" in the set editor (esp scanner spec §4.1). Information only: no PGPatcher warning (spec §1).
use crate::model::meshes::overridden_text;
use crate::model::plugin_refs::{set_key, LIMIT};
use crate::model::sets::SetView;
use crate::model::AppModel;
use eframe::egui;
use modscan::plugin_store::record_label;

pub fn set_section(ui: &mut egui::Ui, model: &mut AppModel, v: &SetView) {
    egui::CollapsingHeader::new("Used by plugins")
        .id_salt(("set-used-by-plugins", v.id.as_str()))
        .show(ui, |ui| {
            let Some(key) = set_key(v) else {
                ui.weak("This set has no match key.");
                return;
            };
            if ui.small_button("Refresh").clicked() {
                model.plugin_refs.refresh();
            }
            match model.plugin_refs.get(&key) {
                Err(e) => {
                    ui.weak(e.as_str());
                }
                Ok(u) => {
                    if u.txsts.is_empty() {
                        ui.label("No texture set of the latest scan matches this set.");
                    }
                    for t in &u.txsts {
                        ui.label(format!(
                            "{} · slot {}: {}{}",
                            record_label(t.edid.as_deref(), t.formid, "TXST", &t.plugin),
                            t.slot,
                            t.path,
                            overridden_text(t.n_overrides)
                        ));
                    }
                    if u.truncated {
                        ui.weak(format!("… only the first {LIMIT} are listed"));
                    }
                    if !u.alts.is_empty() {
                        ui.label(egui::RichText::new("Alternate textures using them").strong());
                        for a in &u.alts {
                            ui.label(format!(
                                "{} · {} → {}",
                                record_label(a.edid.as_deref(), a.formid, &a.sig, &a.plugin),
                                a.swap.shape_name,
                                a.swap.txst_edid.as_deref().unwrap_or("(no EDID)")
                            ));
                        }
                    }
                }
            }
        });
}
```

Shared hooks (add-only lines):
- `crates/gui/src/model/mod.rs`: after `pub mod pbr_export;` add `pub mod plugin_refs;`. In `pub struct AppModel`, after `pub restart_needed: bool,` add:

```rust
    /// The set editor's "Used by plugins" reads (esp spec §4.1); no store path in tests.
    pub plugin_refs: plugin_refs::SetPluginRefs,
```

  In `AppModel::placeholder`'s literal, after `restart_needed: false,` add `plugin_refs: Default::default(),`.
- `crates/gui/src/ui/mod.rs`: after `pub mod pgp_fields;` add `pub mod plugin_refs;`.
- `crates/gui/src/app.rs`: in `SdcApp::new`, as the first two lines of the body, add:

```rust
        let mut model = model;
        model.plugin_refs = crate::model::plugin_refs::SetPluginRefs::new(crate::model::meshes::default_store_path());
```

`crates/gui/src/ui/set_editor.rs`: replace `                under_grid(ui, model, &v);` with

```rust
                under_grid(ui, model, &v);
                super::plugin_refs::set_section(ui, model, &v);
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p sdc-gui --test set_editor_ui`
Expected: all pass, including the 4 new tests.

- [ ] **Step 5: Gates and commit (two commits)**

The first commit holds the shared hooks with the two new modules they declare, so every commit compiles. The second holds the call site and the tests.

```powershell
cargo fmt --all; cargo clippy --workspace --all-targets -- -D warnings; cargo test --workspace
git add crates/gui/src/model/mod.rs crates/gui/src/ui/mod.rs crates/gui/src/app.rs crates/gui/src/model/plugin_refs.rs crates/gui/src/ui/plugin_refs.rs
git commit -m "gui: plugin_refs model/ui modules and their hooks (add-only)" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
git add crates/gui/src/ui/set_editor.rs crates/gui/tests/set_editor_ui.rs
git commit -m "gui: set editor 'Used by plugins' (TXSTs matching the set, alternate-texture users)" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 9: owner checklist (manual; numbers from the scan log and the owner test)

**Files**
- Modify: this plan file, section "esp verification" at the end (record the results). No code.

**Interfaces:** Consumes Tasks 1–8. The scan writes `Report.phase_ms` (`plugins`) and, in the GUI, the timing log line `{"event":"plugins","n":…,"cached":…,"records":…,"ms":…}` in `%LOCALAPPDATA%\sdc\logs\preview-timings-YYYYMMDD-HHMMSS.jsonl`. Owner data stays read-only; the owner does not time anything by hand.

- [ ] **Step 1: Owner timing test.** In `F:\ai\dds\converter-nif-esp`: `$env:SDC_ACHERUS='C:\_mount\moddingvol\Acherus'; cargo test -p modscan --release --test plugins owner_load_order -- --ignored --nocapture`. Expect: pass, cold < 10 s, warm < 1 s with `cached = n`, a `Skyrim.esm TXST: <EDID> [00xxxxxx], N slot paths` line, and no problem lines for the five base masters. Record the printed line and the first problem lines verbatim.
- [ ] **Step 2: Build and scan.** `cargo build -p sdc-gui --release`; start the exe from MO2 (Acherus); Meshes → Scan. Expect: "Scan details" shows `Plugins: N (0 cached), R records, T ms` on the first scan after this build (schema 4 rebuilds the store). Scan again: `(N cached)` and a small T. The log has one `plugins` line per scan with the same numbers. Record both lines.
- [ ] **Step 3: NIF detail.** Open a vanilla mesh used by a Skyrim.esm static, e.g. any `meshes\landscape\rocks\…` NIF from the "All meshes" search. Expect: "Plugin overrides (≥1)" lists `EDID [FORMID] STAT — <plugin>`. If a record re-textures shapes, the lines `shape → TXST EDID` appear. There is no PGPatcher warning. Record one example.
- [ ] **Step 4: Set editor.** Open a set whose diffuse matches a vanilla landscape texture. Expand "Used by plugins". Expect: a Skyrim.esm (or overriding plugin) TXST with its EDID, FormID, slot and path. If any, records with alternate textures using it follow. Record one example.
- [ ] **Step 5: Plugin file references.** Expand it on the Meshes page. Check five Missing entries in MO2's data view: the file must really be absent from every enabled mod, the game Data folder and the active BSAs. Note any false entry with its path and record (Review Focus 5). For Unused, note which kinds dominate (LOD, interface, …); these are accepted limits, but a large class that a §3 record should cover is a bug.
- [ ] **Step 6: Send** the numbers and notes back.

## esp verification

(Filled in by Task 9.)

### Step 1 (run by the controller, 2026-10-12, HEAD c2e30dc)

    cold: n=1983 records=128751 ms=4156 (4.16 s); warm: cached=1983/1983 (0.14 s)
    Skyrim.esm TXST: LandscapeGrassSnow01 [00000889], 2 slot paths
    alt_tex: 26042 rows, e.g. shape "MineOreBlackreach02:3" -> txst 000A9293
    test owner_load_order ... ok

Targets met (cold < 10 s, warm < 1 s). alt_tex > 0 confirms the alternate-texture layout on real data (pre-flight F-7).

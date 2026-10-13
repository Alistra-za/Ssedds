# Audit fixes (findings 5-13, 15, 18-21, ours) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the audit findings this team owns. The engine stops trusting replaced outputs and stops aborting on one unreadable folder. Re-runs hash on every core, `--sidecar` works on up-to-date files, RNM strengths above 1 stay above the surface, and `fast-preview` checks coverage. The GUI stops rebuilding the Sets column and copying thumbnails every frame, and undo is honest. The CLI fails loudly on `--nvtt`. The suite is green on a fresh clone, and dead code, duplicate rules and the S4 spike are gone.

**Architecture:**
- **Engine staleness (Tasks 1-2).** A manifest record gains an `output_stamp` (size + mtime of the written DDS, the existing `SourceStamp`). `is_up_to_date` requires it to match before it hashes anything. The on-existing decisions are computed on the rayon pool, then reported serially in plan order.
- **Engine planning and encode (Tasks 3-4).** `discover` skips unreadable entries and reports them with a new code. The RNM `decode` clamps z at 0. Coverage is validated on every level under `validation.mips = smallest`.
- **GUI (Tasks 5-8).** `AppModel` caches the set views and export blocks under a key: an items revision, a global `SetsDoc` revision, the folder, the export root and the target. Thumbnails are `Arc`-shared and evicted. Undo merges only within an uninterrupted run of edits. "Accept all" is one history step and one re-plan.
- **CLI, lean, repo (Tasks 9-11).** `convert --nvtt` is a usage error. `validate` and `patch-header` refuse a folder. The duplicated rules call one copy. Bench corpus tests skip without a corpus, and the S4 spike is deleted.

**Tech Stack:** Rust 2021 workspace (rust-version 1.88). Packages: `engine`, `encoders-sys`, `sdc` (cli), `sdc-bench`, `sdc-gui` (lib `sdc_gui`). Also used: rayon, serde/serde_json/schemars, globset, walkdir, toml_edit, egui 0.36.2 and egui_kittest.

**Spec:** `docs/research/audit-2026-10-13.md` (an independent audit at `672a26f`; its line numbers predate complex grass). Owner rulings for this plan are in the Global Constraints below.

## Global Constraints

- **Base.** Branch `fix/audit-2026-10-13` from `main` 7765cea (worktree `F:/ai/dds/converter/.worktrees/audit`). Every `file:line` below was read at 7765cea. If one has drifted, find the code by the quoted text.
- **Ownership.** Only engine, encoders-sys, cli, gui, bench and repo files. **Off-limits:** `crates/nif`, `crates/vfs`, `crates/modscan`, `crates/render3d`, `crates/pgpsim`, `crates/esp`, and `crates/gui/src/render/upload.rs`. Findings 1-4, 14, 16, 17, the `modscan/tests/resolve.rs` part of 18, and finding 20's modscan/nif/vfs/render3d items belong to another team. Do not touch them.
- **Gates.** Run each one unpiped before every commit. All must pass:
  - `cargo fmt --all --check`
  - `cargo clippy --workspace --all-targets --offline -- -D warnings`
  - `cargo clippy --workspace --offline -- -D warnings`
  - `cargo test --workspace --offline`
- **Golden tests.** `crates/engine/tests/golden.rs` is unchanged by every task. No task changes encoded bytes for an existing test input. Finding 12 changes output only for an RNM layer at strength > 1 on a low-blue texel, and no test or golden has one (checked: `rnm_run.rs` uses 0.9, and `bump_is_resolution_invariant` uses strength 2 but bump never calls `decode`). Behaviour deliberately changes in these existing non-golden tests, and only these:
  - `crates/engine/tests/validate.rs::smallest_mip_validation_checks_only_the_last_level` (Task 4, finding 13);
  - `crates/engine/src/api.rs::tests::up_to_date_needs_a_comparable_matching_passing_record` (Task 1, gains the output stamp);
  - `crates/engine/tests/api.rs::a_failed_manifest_append_removes_the_output` (Task 1, sidecar on);
  - `crates/encoders-sys/tests/init.rs` and `crates/engine/src/image.rs` tests (Task 10, assertions on deleted functions);
  - `crates/gui/tests/project.rs::edits_keep_every_hand_written_byte` (Task 8, new fixture line).
  - The GUI tests listed in Task 5 also gain one `invalidate_views()` call each.
- **Cargo.lock.** It changes only by removed dependency edges (Task 9: `sdc` loses `schemars` and `anyhow`, `sdc-gui` loses `anyhow`). Add no new crates.
- **Schemas.** `schema/*.json` regenerate only when a serialised type changes:
  - Task 1 (`ManifestRecord.output_stamp`, `manifest.schema.json`);
  - Task 3 (new `Code::InUnreadable`, `event.schema.json` and `dds-validation.schema.json`).
  - Regenerate with `cargo run -p sdc --offline -- presets schema all --out schema` and commit the changed `schema/*.json`. `crates/engine/tests/schemas.rs` fails otherwise.
- **Counts.** `Code::ALL.len()` goes 86 → **87** (Task 3). `MANIFEST_SCHEMA` stays 2: the new field is optional.
- **Version.** It stays 0.7.0 unless the owner rules otherwise (see Owner decisions).
- **Commits.**
  - Commit one finding at a time inside a task where that is natural.
  - Commit only explicit paths (`git add <path> …`, never `-A` or `.`).
  - End every message with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
  - Never push.
- **Shell.** Never run `python -` or `python3 -` (stdin heredocs).
- **Decisions.** Task 11 appends `## Note Y — audit fixes (plan rulings, 2026-10-13)` at the end of `docs/DECISIONS.md`, with bullets Y1… (below). Cite the audit as `audit N` and rulings as `DECISIONS Note Y<n>` in comments.
- **Comments.** Match the surrounding density. Every known limit gets `shortcut: <limit>, <when to upgrade>`.

### Plan rulings (Note Y, written by Task 11)

- **Y1 Output stamp (audit 5).** A record stores the output's `SourceStamp` (size, mtime) taken after the rename. `if-changed` keeps an output only when the stamp still matches, and this check runs before any hashing. A record without the field is stale once: the first run after upgrading re-encodes each recorded output one time, then never again. A same-size, same-mtime replacement is not detected (metadata only, by design).
- **Y2 Sidecar (audit 15).** Under `--sidecar`, an up-to-date output with no sidecar gets one, written from its stored record (no re-encode). If that write fails, the file re-encodes, and the encode reports the error.
- **Y3 Parallel decisions (audit 9).** The `if-changed` checks run on the batch pool. Skip events are still emitted in plan order. The source is not re-used by `process_file`: a file found stale by its hash is hashed once more, and only when it really changed.
- **Y4 Discovery (audit 6).** An unreadable entry under an input folder is skipped with `IN-UNREADABLE` (warning, batch-level), and the batch continues. `inspect` skips the same entries silently.
- **Y5 RNM strength (audit 12).** `decode` clamps z to ≥ 0. At strength ≤ 1 this is a no-op. Above 1 it deviates from rnm.py, which has the same formula with no clamp: a steep texel becomes horizontal instead of pointing into the surface.
- **Y6 Coverage under `smallest` (audit 13).** This amends T5. `validation.mips = smallest` checks PSNR on the last level only and coverage on every level, so `summary.coverage` has one entry per level again.
- **Y7 GUI caches (audit 7, 8).** Set views and export blocks are cached under `(items_rev, sets_doc rev, root, export_root, target)`. `SetsDoc` revisions come from one global counter, so a replaced document never reuses a revision. Code that mutates `items` or `sets` in place calls `AppModel::invalidate_views()`. Engine thumbnails are `Arc`-shared. A GUI plan evicts cached files outside its inputs, and packs not in the plan (uncancelled plans only). `ThumbCache` clears when `items_rev` changes.
- **Y8 Undo (audit 10, 11).** An undo or redo ends the merge run. "Accept all suggestions" is one history step and one re-plan, whatever the count.
- **Y9 `--nvtt` (owner).** `sdc convert --nvtt` is a usage error (exit 2) until M5. `sdc encoders --nvtt` (detection only) is unchanged.
- **Y10 S4 spike (owner).** `spikes/s4-scalar` is deleted. It can be rebuilt from the pinned commit and SHA-256 in Spike S4.

## Review Focus

1. **An output kept as up to date when it should not be, or a mass re-encode on every run.** Task 1 pins both ways: a replaced output is re-encoded, and a legacy record re-encodes once and then is up to date (`a_record_without_an_output_stamp_is_stale_once`).
2. **A stale GUI cache.** Every input of `set_views` must be in the cache key or bump `items_rev` or the `SetsDoc` revision. Task 5 lists every mutation site found at HEAD. A new site that bypasses them is the bug to look for.
3. **Event order under parallel decisions.** `events_follow_the_documented_order` must still pass unchanged (Task 2).
4. **Undo history.** After "Accept all", one Ctrl+Z restores every pick (Task 7).

## Owner decisions (flagged; the plan's default in bold)

1. **Stale once (audit 5).** The first `if-changed` run after this lands re-encodes every recorded output once. **Default: accept it.** Alternative: "adopt", where a legacy record that otherwise matches counts as current, and a stamped copy of it is appended. That avoids the re-encode, but trusts pre-upgrade outputs one last time. A version bump would re-encode everything anyway (`ENGINE_VERSION` is part of `is_up_to_date`), and would absorb this cost.
2. **Version bump.** Audit 12 changes output for strength > 1 RNM layers only. Audit 13 changes validation results under `fast-preview` only. **Default: no bump (stay 0.7.0).** Rule 0.7.1 if you want those outputs re-encoded and reported on the next run.
3. **Audit 13 amends DECISIONS T5** (coverage on every level under `smallest`). **Default: amend.**
4. **Audit 12: clamp vs cap.** **Default: clamp z ≥ 0 and keep the 0..=2 range** (Note W7). The alternative, capping strength at 1, would turn existing configs above 1 into `CFG-INVALID`.
5. **Audit 6 needs a new code, `IN-UNREADABLE`** (warning), giving 87 codes. **Default: add it.**
6. **`.gitattributes` scope (audit 18).** The fixed patterns cover `crates/*/tests/fixtures/**` and `crates/*/tests/data/**`. That includes the nif and render3d fixture folders, which belong to the other team: their text fixtures (READMEs, `cs_settings`) check out LF on a fresh clone. **Default: apply it, and tell the other team.**
7. **GUI cache contract (audit 7).** Tests that write `m.items`/`m.sets` in place must call `invalidate_views()`. **Default: accept.**
8. **Glob cache (left-out item).** It is a `thread_local` map, unbounded in principle (bounded in practice by the size of the configs), with a `shortcut:` comment. **Default: accept.**

## Audit claims found wrong or adjusted on inspection

- **5:** `process_file` is in `crates/engine/src/pipeline.rs:363`, not api.rs. The rename is `write_atomic` at `pipeline.rs:501`. The record and the sidecar text are built *before* the write (`pipeline.rs:475-499`), so the stamp needs that code reordered (Task 1).
- **7:** "invalidated in `apply_plan` and `edit_docs` (the only two places the inputs change)" is wrong. `sets_doc` also changes in these places:
  - `open_folder` (`model/mod.rs:373`);
  - `apply_snapshot`, i.e. undo/redo (`:1252`);
  - `record_json_sha` (`:948`);
  - `reload_documents` (`:1450`);
  - both changed-on-disk branches (`:1501`, `:1566`).
  - The export root also changes in `ui/header.rs:156`, and tests mutate `items`/`sets` in place.
  - Task 5 uses a revision key instead of invalidation calls.
- **8:** "retain the cache … at the end of `plan_cached`" is adjusted: `plan_cached` also serves the CLI's throwaway cache. Retention goes in `Session::plan`, and packs are only evicted after an uncancelled plan.
- **9:** "pass the computed source hash into `process_file`" is dropped. It saves one hash per *changed* file, at the cost of plumbing through `run_one`/`process_file`. The dominant cost, hashing unchanged files serially, is fixed without it.
- **12:** the strength range is `policy.rs:1304-1311` (not `:1290`). rnm.py (`F:\ai\bake\rnm\rnm.py:48-52`) has the same unclamped formula, so the fix deviates from the reference above strength 1 (Y5).
- **13:** "keep the PSNR shortcut, which is what Note T5 asked for" is half right. T5 (`DECISIONS.md:503`) says "PSNR and coverage on the last mip only", so the fix amends T5. "Can never fire" holds for every real output (chains end at 1×1). The existing test passes only because its chain ends at 16×16 = 256 texels.
- **18:**
  - There are **11** non-ignored corpus tests, not 10: 2 in `engine_gates.rs`, 8 in `gates.rs` and `reads_bench_results_csv` in `basics.rs`.
  - The audit names the helper `corpus()`; it is `corpus::bench_root()` (`corpus.rs:5-9`).
  - `crates/*/tests/** -text` would also strip text handling from every test `.rs` source, so the plan scopes it to `fixtures/**` and `data/**`.
  - `$Ispc = "ispc.exe"` alone fails with PowerShell's "not recognized" before the version check can explain anything, so the plan adds a one-line check.
- **20:** `level_count` and `full_chain_len` differ at size 0: `level_count` guards with `.max(1)`. The dedupe keeps the guard.
- **21:** "the two later reads of `work.img` are bit-identical on `out[0]`" is wrong for a roughness member with a variance bake. `base` is baked (R changes), while the level-1 squaring (`mips.rs:276-281`) and the bake closure (`mips.rs:265`) read the unbaked `work.img`. The plan frees `work.img` right after `build_chain_with` instead (Task 10). That removes the duplicate through quantise, the peak, with no change to `build_chain_with`.
- **Complex grass (merged after the audit):**
  - `grass::build` (`grass.rs:94-135`) returns a `Work` whose `img` is already empty, so finding 21 does not apply to it. Its own two-chain duplicate is already a `shortcut:` at `grass.rs:114`.
  - The `level_count` dedupe reaches grass only through `build_chain`, with identical results.

## Skipped (left-out list) with reasons

- **Manifest never compacted** (`manifest.rs:324-371`): needs a compaction design (when, how it interacts with concurrent appends). Task 2 removes the dominant re-run cost.
- **No test of sidecar removal after a failed manifest append**: not skipped. Task 1 turns the sidecar on in that test.
- **GUI: no test that a failed save leaves `sdc.toml` / the PGPatcher JSON untouched**: a test-only gap with no known bug (temp-then-rename is in place). Out of the approved list.
- **CLI: `inspect`/`validate` decode on one thread**: performance only, and it needs a per-file parallel report with ordered output. Out of the approved list.
- **Workspace: two `toml_edit`/`winnow` versions**: aligning them changes `Cargo.lock` beyond removals (forbidden here).
- **Workspace: `cargo test -p sdc` builds the GUI** (`gui_repro.rs` dev-dep): needs the test moved to another crate. That is a structural change, owner's call.
- **Engine: `test-hooks` reachable from `--all-targets` builds**: a release-process note, not code. Out of the approved list.
- **Not ours:** vfs BSA double copy, esp `inflate` message, parser hostile-count tests, modscan parse/write overlap, render3d `wait_idle`/bytecode/panic hook, and the gui `render/upload.rs` clamp.
- **Optional `Subtype::id`/`Format::id`/`RawMap::id` generic:** **done** (Task 10). The three 6-line bodies are identical, and one private helper replaces them with no caller change.

---

## Task 1: Output stamp and sidecar for up-to-date outputs (audit 5, 15)

**Files:**
- Modify: `crates/engine/src/manifest.rs:180-209` (`ManifestRecord`), `:226-238` (`ManifestEntry`), `:248-272` (`from_value`)
- Modify: `crates/engine/src/pipeline.rs:475-512` (record, write order)
- Modify: `crates/engine/src/api.rs:548-596` (`is_up_to_date`), `:1685-1697` (up-to-date arm), `:2273-2380` (unit test)
- Modify: `crates/engine/tests/api.rs:343-385` (sidecar on), add two tests
- Modify: `crates/engine/tests/events_manifest.rs:16-38` (`output_stamp: None` in the literal)
- Regenerate: `schema/manifest.schema.json`

**Interfaces:**
```rust
// manifest.rs, ManifestRecord (after `shader`)
/// Size and mtime of the output as written (audit 5, DECISIONS Note Y1); `None` in records written before it.
#[serde(default, skip_serializing_if = "Option::is_none")]
pub output_stamp: Option<crate::policy::SourceStamp>,
// manifest.rs, ManifestEntry
pub output_stamp: Option<crate::policy::SourceStamp>,
```

- [ ] **Step 1: failing tests** (append to `crates/engine/tests/api.rs`)

```rust
/// Audit 5 (DECISIONS Note Y1): an output replaced after it was recorded is re-encoded, not kept as up to date.
#[test]
fn a_replaced_output_is_reencoded() {
    let d = tempfile::tempdir().unwrap();
    fixtures(d.path());
    run(request(d.path()));
    let out = d.path().join("out/armor/iron.dds");
    let good = std::fs::read(&out).unwrap();
    std::fs::write(&out, &good[..good.len() / 2]).unwrap();
    let (s, _) = run(request(d.path()));
    assert_eq!((s.converted, s.up_to_date), (1, 1));
    assert_eq!(std::fs::read(&out).unwrap(), good);
}

/// Note Y1: a record from before the stamp re-encodes once, then is up to date.
#[test]
fn a_record_without_an_output_stamp_is_stale_once() {
    let d = tempfile::tempdir().unwrap();
    fixtures(d.path());
    run(request(d.path()));
    let m = Manifest::at(&d.path().join("out"));
    let text = std::fs::read_to_string(m.path()).unwrap();
    let legacy: String = text
        .lines()
        .map(|l| {
            let mut v: serde_json::Value = serde_json::from_str(l).unwrap();
            assert!(v.as_object_mut().unwrap().remove("output_stamp").is_some());
            format!("{v}\n")
        })
        .collect();
    std::fs::write(m.path(), legacy).unwrap();
    let (s, _) = run(request(d.path()));
    assert_eq!((s.converted, s.up_to_date), (2, 0));
    let (s, _) = run(request(d.path()));
    assert_eq!((s.converted, s.up_to_date), (0, 2));
}

/// Audit 15 (Note Y2): `--sidecar` on a re-run writes the missing sidecar of an up-to-date output from its record.
#[test]
fn sidecar_is_written_for_an_up_to_date_output() {
    let d = tempfile::tempdir().unwrap();
    fixtures(d.path());
    run(request(d.path()));
    let side = d.path().join("out/armor/iron.dds.json");
    assert!(!side.exists());
    let mut req = request(d.path());
    req.sidecar = true;
    let (s, _) = run(req);
    assert_eq!((s.converted, s.up_to_date), (0, 2));
    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&side).unwrap()).unwrap();
    assert_eq!(v["output"], "armor/iron.dds");
}
```

In `a_failed_manifest_append_removes_the_output` (`tests/api.rs:343`), replace `let (s, ev) = run(request(d.path()));` with:
```rust
    let mut req = request(d.path());
    req.sidecar = true;
    let (s, ev) = run(req);
```
and after the `.dds` emptiness assertion add:
```rust
    assert!(files_with_ext(&d.path().join("out"), "json").is_empty(), "sidecars are removed too");
```

- [ ] **Step 2:** `cargo test -p engine --offline --test api` fails. `a_replaced_output_is_reencoded` reports `(0, 2)`, and the stamp test panics at the `remove` assert.

- [ ] **Step 3: implement**

`manifest.rs`: add the two fields above. In `from_value` add `output_stamp: lenient(&raw, "output_stamp"),`.

`pipeline.rs`: build the record with `output_stamp: None`, then reorder the tail so the stamp is taken after the rename and the sidecar carries it:
```rust
    let mut record = ManifestRecord { /* … as now … */ output_stamp: None };
    stop()?;

    write_atomic(&job.output, &bytes).map_err(|e| fail(Stage::Write)(e.to_string()))?;
    tracker.written.set(true);
    drop(bytes);
    // Note Y1: taken after the rename, so `if-changed` notices a replaced output. Any failure from here on removes
    // the output (`run_one`, `tracker.written`).
    record.output_stamp = SourceStamp::of(&job.output);
    if req.sidecar {
        let text = serde_json::to_string_pretty(&record).map_err(|e| fail(Stage::Write)(e.to_string()))?;
        write_atomic(&job.output.with_extension("dds.json"), text.as_bytes())
            .map_err(|e| fail(Stage::Write)(format!("sidecar: {e}")))?;
    }
    manifest.append(&record) /* … as now … */
```
(Delete the old `let sidecar = req.sidecar.then(...)` block. `SourceStamp` is already imported at `pipeline.rs:27`.)

`api.rs` `is_up_to_date`: right after the encoder/schema early return (before the BC1 and hash checks), add
```rust
    // Note Y1: metadata only, and before any hashing; a legacy record (no stamp) is stale once.
    if e.output_stamp.is_none() || SourceStamp::of(&job.output) != e.output_stamp {
        return false;
    }
```
Up-to-date arm (`api.rs:1685`):
```rust
                            Some(e)
                                if is_up_to_date(e, job, &encoders, req.bc1_model)
                                    && (!req.sidecar || sidecar_present(&job.output, e)) =>
```
New helper next to `is_up_to_date`:
```rust
/// `--sidecar` on an up-to-date output (audit 15, DECISIONS Note Y2): write the stored record when the sidecar is
/// missing. `false` when it cannot be written, so the file re-encodes and the encode reports the error.
fn sidecar_present(output: &Path, e: &ManifestEntry) -> bool {
    let side = output.with_extension("dds.json");
    side.exists()
        || serde_json::to_string_pretty(&e.raw)
            .is_ok_and(|t| crate::dds::write_atomic(&side, t.as_bytes()).is_ok())
}
```
Unit test `up_to_date_needs_a_comparable_matching_passing_record` (`api.rs:2273`): before `entry_for`, add `std::fs::write(&job.output, b"dds").unwrap();`. In `entry_for` add `output_stamp: SourceStamp::of(&job.output),`. Grow `not_up` to `[_; 12]` with:
```rust
            ("no output stamp", with(&|e| e.output_stamp = None)),
            (
                "replaced output",
                with(&|e| e.output_stamp.as_mut().unwrap().size += 1),
            ),
```
`events_manifest.rs:16` literal: add `output_stamp: None,`.

- [ ] **Step 4:** `cargo run -p sdc --offline -- presets schema all --out schema`. Only `schema/manifest.schema.json` changes.
- [ ] **Step 5:** gates. Commit `engine: output stamp in the manifest; if-changed re-encodes a replaced output (audit 5)`, and separately `engine: --sidecar writes the sidecar of an up-to-date output (audit 15)`. Splitting is optional; if split, the sidecar arm and test go second.

## Task 2: Up-to-date decisions on the pool (audit 9)

**Files:**
- Modify: `crates/engine/src/api.rs:1630-1700` (decision loop), `:1702` (`thread_pool` moves up)

**Interfaces:** none public.

- [ ] **Step 1: test.** This is a pure scheduling change, so no new behaviour exists to fail first. The guards are the existing tests, which must pass unchanged:
  - `converts_skips_unresolved_and_reruns_as_up_to_date`;
  - `events_follow_the_documented_order`;
  - `a_cancel_is_recorded_when_every_ready_file_is_up_to_date`;
  - `changed_source_is_reencoded_and_sidecar_is_written`;
  - Task 1's three tests.
  - Record the before/after wall time of a no-op re-run on a big folder in the commit message, if one is at hand.
- [ ] **Step 2: implement.** Move `let pool = thread_pool(req.jobs)?;` above the loop, then:
```rust
        // Audit 9 (DECISIONS Note Y3): the if-changed checks hash every source; run them on the pool, then report
        // them below in plan order.
        let current: Vec<bool> = if req.on_existing == OnExisting::IfChanged {
            use rayon::prelude::*;
            pool.install(|| {
                ready
                    .par_iter()
                    .map(|job| {
                        !cancel.is_cancelled()
                            && job.output.exists()
                            && existing
                                .get(&output_key(&req.output_root, &job.output))
                                .is_some_and(|e| is_up_to_date(e, job, &encoders, req.bc1_model))
                    })
                    .collect()
            })
        } else {
            vec![false; ready.len()]
        };
        let mut todo: Vec<(&ResolvedJob, PresetRef)> = vec![];
        for (job, current) in ready.into_iter().zip(current) {
```
The up-to-date arm becomes `Some(e) if current && (!req.sidecar || sidecar_present(&job.output, e)) => {`. Everything else in the loop is unchanged (the cancel check, the `None` arm, `Skip`, `Overwrite`).
- [ ] **Step 3:** gates. Commit `engine: if-changed decisions hash on the batch pool (audit 9)`.

## Task 3: Discovery skips unreadable entries (audit 6) and rule globs compile once (left-out)

**Files:**
- Modify: `crates/engine/src/discover.rs:82-129`
- Modify: `crates/engine/src/api.rs:769-770` (emit warnings)
- Modify: `crates/engine/src/codes.rs:101` (new code), `:196` (86 → 87)
- Modify: `crates/engine/src/policy.rs:142-162` (glob cache)
- Modify: `docs/superpowers/specs/2026-10-08-skyrim-dds-converter-design.md` (code table, next to `IN-NONFINITE` at `:700`), `docs/gui.md` (only if it lists codes; grep `GRASS-VANILLA` there for the pattern)
- Test: `crates/engine/tests/discover_group.rs`
- Regenerate: `schema/event.schema.json`, `schema/dds-validation.schema.json`
- Unchanged: `crates/cli/src/verbs.rs:404` (`inspect` keeps calling `discover`, which now skips silently)

**Interfaces:**
```rust
/// `discover`, plus each folder entry it could not read (walkdir's message, which names the path).
pub fn discover_reporting(inputs: &[PathBuf], output_root: &Path, recursive: bool)
    -> std::io::Result<(Vec<InputFile>, Vec<String>)>;
pub fn discover(...) -> std::io::Result<Vec<InputFile>>; // unchanged signature: discover_reporting(..).map(|(f, _)| f)
// codes.rs
InUnreadable = "IN-UNREADABLE" => Warning,
```

- [ ] **Step 1: failing test** (append to `tests/discover_group.rs`, Windows-only like the engine):

```rust
/// Audit 6 (DECISIONS Note Y4): an unreadable folder is skipped and reported; the rest is still found.
#[test]
fn an_unreadable_folder_is_skipped_not_fatal() {
    let d = tempfile::tempdir().unwrap();
    let src = d.path().join("in");
    let locked = src.join("locked");
    std::fs::create_dir_all(&locked).unwrap();
    std::fs::write(src.join("a.png"), b"x").unwrap();
    std::fs::write(locked.join("b.png"), b"x").unwrap();
    // *S-1-1-0 is Everyone, whatever the system language.
    let icacls = |args: &[&str]| {
        let ok = std::process::Command::new("icacls").arg(&locked).args(args).status().unwrap();
        assert!(ok.success());
    };
    icacls(&["/deny", "*S-1-1-0:(OI)(CI)(RX)"]);
    let r = discover_reporting(std::slice::from_ref(&src), &d.path().join("out"), true);
    icacls(&["/remove:d", "*S-1-1-0"]);
    let (files, skipped) = r.unwrap();
    let rels: Vec<String> = files.iter().map(|f| f.rel.to_string_lossy().into_owned()).collect();
    assert_eq!(rels, ["a.png"]);
    assert_eq!(skipped.len(), 1, "{skipped:?}");
    assert!(skipped[0].contains("locked"), "{skipped:?}");
}
```
In `codes.rs` tests, change `assert_eq!(Code::ALL.len(), 86);` to `87`.

- [ ] **Step 2:** `cargo test -p engine --offline --test discover_group` fails to compile (`discover_reporting` is missing).
- [ ] **Step 3: implement.** In `discover.rs`, rename the body to `discover_reporting` and keep a `skipped: Vec<String>`. Replace `let e = e.map_err(std::io::Error::other)?;` with
```rust
                let e = match e {
                    Ok(e) => e,
                    // Audit 6: an access-denied folder, a dangling junction or a cloud placeholder skips, never aborts.
                    Err(err) => {
                        skipped.push(err.to_string());
                        continue;
                    }
                };
```
Return `Ok((files, skipped))`. Add `pub fn discover(..) -> io::Result<Vec<InputFile>> { discover_reporting(inputs, output_root, recursive).map(|(f, _)| f) }`. The remaining `?`s (`absolute`, `strip_prefix`) stay: they cannot fail for a path walkdir produced under `input`.

`api.rs:769`:
```rust
        let (mut files, unreadable) = discover_reporting(&req.inputs, &req.output_root, req.recursive)
            .map_err(|e| EngineError::Usage(format!("discover: {e}")))?;
```
After the `BatchStarted` emit (`api.rs:772-780`, so the warnings follow it as the event order requires), add
```rust
        for u in unreadable {
            rep.emit(None, None, EventKind::Warning(Warning::new(Code::InUnreadable, format!("skipped: {u}"))));
        }
```
Import `discover_reporting` at `api.rs:11` (drop `discover` there if it becomes unused).

`codes.rs`: add `InUnreadable = "IN-UNREADABLE" => Warning,` after `InNonfinite`, and a module-doc mention next to the other `IN-*` notes. Add a row to the spec's code table: `| \`IN-UNREADABLE\` | warning | A file or folder under an input folder could not be read (access denied, dangling junction, cloud placeholder); it was skipped | Fix the permissions, or ignore |`.

Glob cache, in `policy.rs`, `glob_match_input`:
```rust
fn glob_match_input(glob: &str, input: &InputFile) -> bool {
    thread_local! {
        // shortcut: never evicted; globs come from presets, configs and --set (dozens), so bound it if they ever come
        // from data.
        static GLOBS: std::cell::RefCell<std::collections::HashMap<String, Option<globset::GlobMatcher>>> =
            Default::default();
    }
    let rel = input.rel.to_string_lossy().replace('\\', "/").to_lowercase();
    let name = rel.rsplit('/').next().unwrap_or_default();
    let game = input.game_path.to_lowercase();
    GLOBS.with_borrow_mut(|c| {
        if !c.contains_key(glob) {
            let m = globset::GlobBuilder::new(&glob.to_lowercase())
                .literal_separator(true)
                .build()
                .ok()
                .map(|g| g.compile_matcher());
            c.insert(glob.to_owned(), m);
        }
        let Some(m) = &c[glob] else { return false };
        m.is_match(&game) || m.is_match(&rel) || (!glob.contains('/') && m.is_match(name))
    })
}
```
This is behaviour-preserving, so it has no new test. `policy_layers.rs`, `file_scopes.rs` and `overrides.rs` exercise every glob path.

- [ ] **Step 4:** regenerate the schemas (only `event.schema.json` and `dds-validation.schema.json` change).
- [ ] **Step 5:** gates. Commit `engine: an unreadable folder entry is skipped with IN-UNREADABLE (audit 6)`, then `engine: compile each rule glob once per thread (audit left-out)`.

## Task 4: RNM z clamp (audit 12) and coverage under `smallest` (audit 13)

**Files:**
- Modify: `crates/engine/src/rnm/blend.rs:22-32` (`decode`), tests at `:193`
- Modify: `crates/engine/src/validate/mod.rs:339-345` (coverage loop)
- Modify: `crates/engine/tests/validate.rs:819-865`

- [ ] **Step 1: failing tests.** In `rnm/blend.rs` `mod tests`:
```rust
    /// Audit 12 (DECISIONS Note Y5): above strength 1 a steep texel never points into the surface; at or below 1
    /// the clamp never acts.
    #[test]
    fn strength_above_one_never_gives_negative_z() {
        assert!(decode([0.99, 0.5, 0.6, 1.0], 1.5, false)[2] >= 0.0);
        for s in [0.5f32, 1.0, 1.5, 2.0] {
            for b in 0..=255u8 {
                let z = decode([0.99, 0.5, b as f32 / 255.0, 1.0], s, false)[2];
                assert!(z >= 0.0, "strength {s}, blue {b}: z {z}");
            }
        }
    }
```
In `tests/validate.rs`, change the end of `smallest_mip_validation_checks_only_the_last_level` and rename it `smallest_mip_validation_checks_psnr_on_the_last_level_and_coverage_on_all`:
```rust
    assert_eq!(cov_levels, [0, 1], "coverage is checked on every level (Note Y6, amends T5)");
    assert_eq!(r.summary.coverage.len(), 2);

    // A real chain ends at 1x1: under `smallest` the coverage drift on the big levels still fires.
    let (b, inputs) = dds(&p, &[a_flat(32), a_flat(16), a_flat(8), a_flat(4), a_flat(2), a_flat(1)]);
    let r = run(&b, &p, Some(&inputs), Some(1.0), None);
    assert!(codes(&r).contains(&Code::ValCoverage), "{:?}", r.warnings);
    assert_eq!(r.summary.coverage.len(), 6);
```
Update its doc comment to "PSNR on the last level only; coverage on every level (audit 13, DECISIONS Note Y6)". (`p` there is the cutout profile with `ValidationMips::Smallest`.)

- [ ] **Step 2:** `cargo test -p engine --offline rnm::blend` and `--test validate` fail.
- [ ] **Step 3: implement.** `blend.rs:31`:
```rust
    // Audit 12 (Note Y5): above strength 1 the DAZ z goes negative on steep texels; rnm.py does not clamp.
    unit([t[0], t[1], (t[2].abs() + 1.0 - s).max(0.0)])
```
and extend the doc comment with "z is clamped at 0 (Note Y5)". `validate/mod.rs`: delete the three lines
```rust
                    if v.mips == ValidationMips::Smallest && l + 1 != ldr.len() {
                        continue;
                    }
```
and update the comment at `:210` to "fast preview checks PSNR on the smallest mip only (coverage runs on every level, Note Y6)".
- [ ] **Step 4:** gates (golden unchanged). Commit `engine: RNM decode clamps z at 0 above strength 1 (audit 12)`, then `engine: coverage is validated on every level under validation.mips = smallest (audit 13)`.

## Task 5: GUI set-view and export-block cache, item index, export drawer index (audit 7, left-out)

**Files:**
- Modify: `crates/gui/src/sets_doc.rs:39-45` (`rev` field), every `&mut self` method (`reload :139`, `restore :175`, `restore_keeping_json_sha :188`, `set_json_name :250`, `set_last_json_sha256 :256`, `set_order :263`, `set_auto_pbr :276`, `create_set :323`, `delete_set :332`, `rename_set :364`, `add_member :429`, `remove_member :444`, `set_borrow :456`, `set_shader :483`, `set_value :516`, and the private `set_project`); add `pub fn rev(&self) -> u64`
- Modify: `crates/gui/src/model/mod.rs:153-237` (fields), `:255` (`placeholder` literal), `:378-380` (`open_folder`), `:1058-1059` (`apply_plan`), `:1131-1133` (`item`)
- Modify: `crates/gui/src/model/sets.rs:360` (`set_views` → cached)
- Modify: `crates/gui/src/model/pbr_export.rs:250` (`export_blocks` → cached)
- Modify: `crates/gui/src/ui/sets_column.rs:106-110`
- Modify: `crates/gui/src/model/export.rs:54-88, 104-112, 158-167` (row index)
- Modify tests (add `m.invalidate_views();` after the in-place writes): `crates/gui/tests/pbr_export.rs:110` (after `leaf.job_key = …`), `crates/gui/tests/set_editor_ui.rs:858` (after `member.warnings = …`), `:900` (after the `with_only_warnings` writes), `crates/gui/tests/set_preview_model.rs:254`, `crates/gui/tests/model.rs:274`. `set_editor_ui.rs:711` replaces `sets_doc` outright: the new doc gets a fresh global revision, so it needs no call.

**Interfaces:**
```rust
// sets_doc.rs
static NEXT_REV: AtomicU64 = AtomicU64::new(1);
fn next_rev() -> u64 { NEXT_REV.fetch_add(1, Ordering::Relaxed) }
pub fn rev(&self) -> u64;
// model/mod.rs
#[derive(Default)]
pub(crate) struct ViewCache {
    pub key: Option<ViewKey>,
    pub views: Option<Arc<Vec<sets::SetView>>>,
    pub blocks: Option<Arc<Vec<pbr_export::ExportIssue>>>,
}
#[derive(Clone, PartialEq)]
pub(crate) struct ViewKey { items: u64, doc: u64, root: Option<PathBuf>, export_root: PathBuf, target: Target }
// AppModel fields: items_rev: u64, item_index: HashMap<String, usize>, views: RefCell<ViewCache>
pub fn items_rev(&self) -> u64;
/// Call after writing `items` or `sets` in place (tests); a plan does it itself.
pub fn invalidate_views(&mut self);
// sets.rs
pub fn views(m: &AppModel) -> Arc<Vec<SetView>>;   // cached
pub fn set_views(m: &AppModel) -> Vec<SetView>;    // unchanged signature: (*views(m)).clone()
// pbr_export.rs
pub fn export_blocks(m: &AppModel) -> Vec<ExportIssue>; // unchanged signature, cached
```

- [ ] **Step 1: failing tests** (new `crates/gui/tests/view_cache.rs`):
```rust
//! Audit 7 (DECISIONS Note Y7): the set views are built once per change, and follow every change.
mod support;
use sdc_gui::model::sets::views;
use std::sync::Arc;
use std::time::Duration;
use support::*;

#[test]
fn set_views_are_cached_until_an_input_changes() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    let a = views(&m);
    assert!(Arc::ptr_eq(&a, &views(&m)), "no change, no rebuild");
    // A document edit.
    m.rename_set("landscape/rock01", "landscape/stone").unwrap();
    assert!(!Arc::ptr_eq(&a, &views(&m)));
    assert!(m.wait_idle(Duration::from_secs(60)));
    assert!(views(&m).iter().any(|v| v.id == "landscape/stone"));
    // Undo restores the document without an edit_docs call.
    m.undo();
    assert!(m.wait_idle(Duration::from_secs(60)));
    assert!(!views(&m).iter().any(|v| v.id == "landscape/stone"));
    // The export folder.
    let before = views(&m);
    m.settings.export_folder = Some(d.path().join("elsewhere"));
    assert!(!Arc::ptr_eq(&before, &views(&m)));
    // An in-place write, announced.
    let before = views(&m);
    m.invalidate_views();
    assert!(!Arc::ptr_eq(&before, &views(&m)));
}

#[test]
fn every_sets_doc_edit_changes_its_revision() {
    let d = tempfile::tempdir().unwrap();
    let mut s = sdc_gui::sets_doc::SetsDoc::open(d.path());
    let mut last = s.rev();
    let mut step = |s: &mut sdc_gui::sets_doc::SetsDoc, what: &str| {
        assert_ne!(s.rev(), last, "{what}");
        last = s.rev();
    };
    s.create_set("a/x");
    step(&mut s, "create_set");
    s.set_order(&["a/x".into()]);
    step(&mut s, "set_order");
    s.set_json_name(Some("n"));
    step(&mut s, "set_json_name");
    let t = s.text();
    s.restore(&t);
    step(&mut s, "restore");
    s.reload();
    step(&mut s, "reload");
    assert_ne!(sdc_gui::sets_doc::SetsDoc::open(d.path()).rev(), last, "a new document never reuses a revision");
}
```
(`rename_set` is `model/sets.rs:1089`; the fixture set id is `landscape/rock01`.)

Add a drawer test in `crates/gui/tests/export.rs`, using its `ev(seq, job, path, kind)` helper (`:146`):
```rust
/// Audit left-out (DECISIONS Note Y7): rows are found by key; a packed-source drop keeps the index right.
#[test]
fn rows_stay_keyed_after_a_packed_source_drop() {
    let mut x = ExportModel::default();
    let found = |o: &str| EventKind::FileDiscovered { output: o.into() };
    x.on_event(ev(0, "a/r.dds", "a/r_roughness.png", found("a/r.dds")));
    x.on_event(ev(1, "a/b.dds", "a/b.png", found("a/b.dds")));
    x.on_event(ev(2, "a/c.dds", "a/c.png", found("a/c.dds")));
    let packed = EventKind::FileSkipped { kind: SkipKind::PackedSource, code: None, reason: "packed".into() };
    x.on_event(ev(3, "", "a/r_roughness.png", packed));
    assert_eq!(x.rows.len(), 2);
    x.on_event(ev(4, "a/c.dds", "a/c.png", EventKind::FileStarted));
    let state = |k: &str| x.rows.iter().find(|r| r.job_key == k).unwrap().state.clone();
    assert_eq!(state("a/c.dds"), RowState::Running);
    assert_eq!(state("a/b.dds"), RowState::Queued);
    assert_eq!(x.rows.len(), 2, "no duplicate row from a stale index");
}
```
If the `PackedSource` event at HEAD carries no `job`, keep `""`. `row()` is not reached for that arm. This test passes before the change too: it is the guard that the index stays right, not a failing-first test (the change is performance only).

- [ ] **Step 2:** `cargo test -p sdc-gui --offline --test view_cache` fails to compile (`views`, `rev` and `invalidate_views` are missing).
- [ ] **Step 3: implement.**
  - `SetsDoc`: add the field `rev: u64`, set to `next_rev()` in `open`. Put `self.rev = next_rev();` as the first statement of every `&mut self` method listed under Files. A spurious bump only costs a rebuild; a missing one is a stale view, so err on bumping.
  - `AppModel`:
    - `items_rev` is bumped, and `item_index` rebuilt, in `apply_plan` right after `self.items = view.items;` and in `open_folder` after `self.items.clear()`.
    - `invalidate_views` bumps `items_rev` and rebuilds `item_index`.
    - `item()`: `self.item_index.get(key).and_then(|&i| self.items.get(i)).filter(|i| i.key == key)`. The `filter` keeps it right if a test pushed or reordered items without `invalidate_views`.
    - `view_key()` builds the `ViewKey` from `items_rev`, `sets_doc.as_ref().map_or(0, SetsDoc::rev)`, `root.clone()`, `export_root()` and `target`.
  - `sets.rs`: rename today's `set_views` body to `fn build_views(m: &AppModel) -> Vec<SetView>`, then:
```rust
/// Every set, in display order (audit 7, DECISIONS Note Y7): built once per change of its inputs.
pub fn views(m: &AppModel) -> Arc<Vec<SetView>> {
    let key = m.view_key();
    {
        let c = m.views.borrow();
        if c.key.as_ref() == Some(&key) {
            if let Some(v) = &c.views {
                return v.clone();
            }
        }
    }
    let v = Arc::new(build_views(m));
    let mut c = m.views.borrow_mut();
    if c.key.as_ref() != Some(&key) {
        *c = ViewCache { key: Some(key), ..Default::default() };
    }
    c.views = Some(v.clone());
    v
}
pub fn set_views(m: &AppModel) -> Vec<SetView> {
    // shortcut: a clone per call for the ~20 callers that take ownership; switch them to `views` if it shows in a profile.
    (*views(m)).clone()
}
```
  - `pbr_export.rs`: rename `export_blocks` to `fn build_blocks` and wrap it with the same pattern on `c.blocks`, returning `(*blocks).clone()`. Never hold a `borrow()` across `build_views` or `build_blocks`: `build_blocks` calls `views`.
  - `sets_column.rs:106`: `let views = crate::model::sets::views(model);`. Iterate `views.iter()`, and replace `views.is_empty()` and the `position` lookups with the same calls on the `Arc`.
  - `export.rs`: add `index: HashMap<String, usize>` to `ExportModel`. `row()` uses `self.index.get(&key)`, inserting on push. After the `PackedSource` `retain`, call `fn reindex(&mut self)` (rebuilds from `rows`). In `BatchDone`, build `let cancelled: HashSet<String> = cancelled.iter().map(|c| slash(c)).collect();` and test `cancelled.contains(&slash(&r.path))`.
- [ ] **Step 4:** gates. Commit `gui: cache set views and export blocks under a revision key (audit 7)`, then `gui: export drawer finds rows by key (audit left-out)`.

## Task 6: Shared, evicted thumbnails (audit 8)

**Depends on Task 5** (`AppModel::items_rev`).

**Files:**
- Modify: `crates/engine/src/session.rs:79-85, 89-94, 136-139, 146-183, 216-234`
- Modify: `crates/gui/src/model/items.rs:12, 58, 220, 239`
- Modify: `crates/gui/src/ui/thumbs.rs:1-71`
- Modify: `crates/gui/src/ui/mod.rs:46` (sweep), `crates/gui/src/ui/rnm_section.rs:28-42` (sweep, `&Arc`)
- Test: `crates/engine/tests/session.rs`, `crates/gui/src/ui/thumbs.rs` (`#[cfg(test)]`)

**Interfaces:**
```rust
// engine::session
pub fn thumbnail_of(&self, path: &Path) -> Option<Arc<Thumbnail>>;
pub fn thumbnail(&self, job: &ResolvedJob) -> Option<Arc<Thumbnail>>;       // AnalysisCache and Session
pub(crate) fn make_thumb(&self, img: &Rgba32F, info: &SourceInfo) -> Option<Arc<Thumbnail>>;
pub(crate) fn retain_plan(&self, plan: &Plan);
// gui TextureItem
pub thumb: Option<Arc<Thumbnail>>,
// gui ThumbCache
pub fn sweep(&mut self, items_rev: u64);
pub fn get_raw(&mut self, ctx: &egui::Context, key: &str, t: &Arc<Thumbnail>, invert: Option<usize>) -> egui::TextureHandle;
```

- [ ] **Step 1: failing tests.** `crates/engine/tests/session.rs`:
```rust
/// Audit 8 (DECISIONS Note Y7): thumbnails are shared, not copied, and a plan of another folder drops the first
/// folder's entries.
#[test]
fn a_plan_of_another_folder_drops_the_first_folders_entries() {
    let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    fixture(a.path());
    fixture(b.path());
    let s = Session::new(Bc1Model::Ideal).unwrap();
    plan(&s, req(a.path(), Target::Cs));
    let iron_a = a.path().join("in").join("iron.png");
    let t = s.cache().thumbnail_of(&iron_a).unwrap();
    assert!(Arc::ptr_eq(&t, &s.cache().thumbnail_of(&iron_a).unwrap()));
    plan(&s, req(b.path(), Target::Cs));
    assert!(s.cache().thumbnail_of(&iron_a).is_none());
    assert!(s.cache().thumbnail_of(&b.path().join("in").join("iron.png")).is_some());
}
```
Check `session.rs:130-165` first. One test there asserts `thumbnail(&job).is_none()` after a change; it must keep its meaning, since retention only drops paths outside the plan's inputs.

`crates/gui/src/ui/thumbs.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use engine::session::Thumbnail;
    use std::sync::Arc;

    #[test]
    fn same_thumb_reuses_the_texture_and_a_new_plan_frees_them() {
        let ctx = egui::Context::default();
        let mut c = ThumbCache::default();
        let t = Arc::new(Thumbnail { width: 1, height: 1, rgba: vec![1, 2, 3, 255] });
        let a = c.get_raw(&ctx, "k", &t, None);
        assert_eq!(a.id(), c.get_raw(&ctx, "k", &t, None).id());
        let t2 = Arc::new(Thumbnail { rgba: vec![9, 9, 9, 255], ..(*t).clone() });
        assert_ne!(a.id(), c.get_raw(&ctx, "k", &t2, None).id(), "a new thumbnail uploads again");
        c.sweep(1);
        assert!(c.map.is_empty());
        c.sweep(1);
        c.get_raw(&ctx, "k", &t2, None);
        c.sweep(1);
        assert_eq!(c.map.len(), 1, "the same revision keeps the textures");
    }
}
```
- [ ] **Step 2:** both fail to compile (the `Arc` types, `sweep` and `retain_plan` are missing).
- [ ] **Step 3: implement.**
  - Engine:
    - `FileEntry.thumb` and `PackEntry.thumb` become `Option<Arc<Thumbnail>>`. `make_thumb` is `self.thumbnails.then(|| Arc::new(make_thumbnail(img, info)))`. `put_file` takes `Option<Arc<Thumbnail>>`. The getters return `Option<Arc<Thumbnail>>`. `api.rs:982` and `:1210` compile unchanged.
    - Add `retain_plan`:
```rust
    /// Audit 8 (Note Y7): keep only what this plan can use. Files outside its inputs go; packs go only after an
    /// uncancelled plan, which saw every pack it needs.
    pub(crate) fn retain_plan(&self, plan: &Plan) {
        let roots = &plan.request.inputs;
        lock(&self.files).retain(|p, _| roots.iter().any(|r| p.starts_with(r)));
        if plan.cancelled.is_empty() {
            let keep: std::collections::HashSet<String> = plan
                .jobs
                .iter()
                .filter_map(|j| j.pack.as_ref())
                .filter_map(pack_cache_key)
                .collect();
            lock(&self.packs).retain(|k, _| keep.contains(k));
        }
    }
```
    - `Session::plan`: `let plan = self.engine.plan_cached(req, rep, cancel, &self.cache)?; self.cache.retain_plan(&plan); Ok(plan)`. Leave `Engine::plan_cached` itself alone: the CLI's throwaway cache goes through it.
  - GUI:
    - `TextureItem.thumb: Option<Arc<Thumbnail>>` (`items.rs:220,239` compile as is).
    - `ThumbCache` gets a `rev: u64` field, and `map: HashMap<String, (usize, egui::TextureHandle)>` keyed by `Arc::as_ptr(t) as usize` instead of a pixel hash. Delete `fn hash`. `get` and `get_raw` compare pointers. `get_raw` takes `&Arc<Thumbnail>`.
    - `pub fn sweep(&mut self, rev: u64) { if self.rev != rev { self.map.clear(); self.rev = rev; } }`. Dropping the handles frees the GPU textures.
    - Call `state.thumbs.sweep(model.items_rev())` at the top of `ui::draw_with`, and `cache.lock()…sweep(model.items_rev())` in `rnm_section::thumb` before `get_raw`.
- [ ] **Step 4:** gates. Commit `engine, gui: thumbnails are Arc-shared and evicted with the plan (audit 8)`.

## Task 7: Honest undo, one-step accept-all, plan panics carry their generation (audit 10, 11, left-out)

**Files:**
- Modify: `crates/gui/src/model/history.rs:29-83` (merge reset; delete `fold_last` and `pushed`)
- Modify: `crates/gui/src/model/mod.rs:735-768` (`accept_all_suggestions`), `:1206-1233` (`edit_docs` batch flag), `:530-544` (drop the `"plan"` arm)
- Modify: `crates/gui/src/worker.rs:132-158` (`spawn_plan`)
- Test: `crates/gui/tests/history.rs`, `crates/gui/tests/export_undecided.rs`, `crates/gui/src/worker.rs` (`#[cfg(test)]`)

**Interfaces:**
```rust
// AppModel private field
batch: bool, // while set, edit_docs changes the documents but neither records a step nor re-plans
// worker.rs
fn plan_result(f: impl FnOnce() -> Result<PlanView, PlanFailure>) -> Result<PlanView, PlanFailure>;
```

- [ ] **Step 1: failing tests.** `crates/gui/tests/history.rs`:
```rust
/// Audit 10 (DECISIONS Note Y8): an edit after an undo starts a new step, even with the merge key now on top.
#[test]
fn an_edit_after_undo_never_merges_into_the_step_below() {
    let mut h = History::default();
    h.push(cmd("name", snap("", "0"), snap("", "x"), Some("json_name")));
    h.push(cmd("shader", snap("", "x"), snap("", "xs"), None));
    h.push(cmd("name", snap("", "xs"), snap("", "xsy"), Some("json_name")));
    h.undo_step();
    h.undo_step();
    h.push(cmd("name", snap("", "x"), snap("", "xz"), Some("json_name")));
    assert_eq!(h.undo.len(), 2, "z is a step of its own");
    assert_eq!(h.undo[0].after.project, "x", "the x state is still a step");
    // Redo also ends a run.
    let mut h = History::default();
    h.push(cmd("name", snap("", "0"), snap("", "a"), Some("k")));
    h.undo_step();
    h.redo_step();
    h.push(cmd("name", snap("", "a"), snap("", "ab"), Some("k")));
    assert_eq!(h.undo.len(), 2);
}
```
`crates/gui/tests/export_undecided.rs`:
```rust
/// Audit 11 (Note Y8): more picks than the history keeps are still one step, undone at once.
#[test]
fn accept_all_over_the_history_limit_is_one_undo_step() {
    use sdc_gui::model::history::History;
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    answer_all_but_subtypes(&mut m);
    let rock = m.root.clone().unwrap().join(ROCK);
    for i in 0..History::LIMIT + 5 {
        std::fs::copy(&rock, rock.with_file_name(format!("rock{i:03}.png"))).unwrap();
    }
    m.replan();
    idle(&mut m);
    let undecided = m.undecided();
    assert!(undecided > History::LIMIT, "{undecided}");
    m.dialog = Some(Dialog::ProceedAnyway(m.undecided_keys()));
    let steps = m.history.undo.len();
    m.accept_all_suggestions();
    assert_eq!(m.history.undo.len(), steps + 1, "one step");
    idle(&mut m);
    assert_eq!(m.undecided(), 0);
    m.undo();
    idle(&mut m);
    assert_eq!(m.undecided(), undecided, "one undo restores every pick");
}
```
`worker.rs`:
```rust
#[cfg(test)]
mod tests {
    #[test]
    fn a_panicking_plan_is_a_plan_failure_of_its_generation() {
        let r = super::plan_result(|| panic!("boom"));
        let f = r.err().expect("a failure");
        assert!(f.message.contains("boom") && !f.config, "{}", f.message);
    }
}
```
- [ ] **Step 2:** the three fail: the merge, `undo.len() == steps + 200`, and `plan_result` is missing.
- [ ] **Step 3: implement.**
  - `history.rs`:
    - `undo_step`: after `let c = self.undo.pop()?;`, add `if let Some(top) = self.undo.last_mut() { top.merge = None; }` with the comment "audit 10: a merge only continues an uninterrupted run".
    - `redo_step`: push `Command { merge: None, ..c.clone() }` instead of `c.clone()`.
    - Delete `fold_last` and the `pushed` field (only `accept_all_suggestions` used them, `mod.rs:754,764`).
  - `edit_docs`: after `f(p, s);`, add `if self.batch { return; }`. The docs change; no step is recorded and nothing re-plans. Add `batch: false` to the `placeholder` literal.
  - `accept_all_suggestions`: replace the `pushed`/`fold_last` lines with
```rust
        // Audit 11 (Note Y8): one snapshot, every pick, one step, one plan (not one plan thread per pick).
        let before = Self::snapshot_of(self.project.as_ref(), self.sets_doc.as_ref());
        self.batch = true;
        for (k, st) in &picks { /* … unchanged … */ }
        self.batch = false;
        let after = Self::snapshot_of(self.project.as_ref(), self.sets_doc.as_ref());
        if after != before {
            self.history.push(history::Command {
                label: "Accept all suggestions".into(),
                before,
                after,
                merge: None,
                created_file: None,
            });
            self.auto_replans = 0;
            self.replan();
        }
        self.refresh_if_settled();
```
    The PBR follow-on still reads `self.items` from the last landed plan, exactly as before: the per-pick re-plans never landed inside this loop.
  - `worker.rs`:
```rust
/// A plan, with a panic turned into this plan's failure, so a superseded plan's panic is dropped by its
/// generation like any stale result (audit left-out).
fn plan_result(f: impl FnOnce() -> Result<PlanView, PlanFailure>) -> Result<PlanView, PlanFailure> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or_else(|p| {
        Err(PlanFailure { config: false, message: format!("internal error in plan: {}", panic_text(&*p)) })
    })
}
```
    In `spawn_plan`, wrap the `session.plan(...)…map_err(...)` expression in `plan_result(|| …)`. Delete the `"plan" => { … }` arm of `WorkerMsg::Panicked` in `model/mod.rs:532-536`: `plan_failed` already clears `planning`, `export_after_plan` and `continue_after_plan`, and raises the error dialog.
- [ ] **Step 4:** gates. Commit `gui: undo and redo end a merge run (audit 10)`, `gui: accept all suggestions is one step and one plan (audit 11)`, `gui: a plan panic is a failure of its own generation (audit left-out)`.

## Task 8: A same-line comment survives an edit of its key (audit 19)

**Files:**
- Modify: `crates/gui/src/project.rs:89-97` (`set_value`), `:197-207` (`set_target`)
- Test: `crates/gui/tests/project.rs:7-15, 23-56`

- [ ] **Step 1: failing test.**
  - Change the `HAND_WRITTEN` fixture's first key line to `target = "cs"  # my target`, and add `[files."landscape/rock01_n.png"]` with `format = "bc1"  # BC7 looks worse here` before `[[rules]]`.
  - In `edits_keep_every_hand_written_byte`, keep the per-line loop for lines the test does not edit, and assert the edited ones:
```rust
    assert!(text.contains("target = \"cpm\"  # my target"), "{text}");
    assert!(text.contains("format = \"bc7\"  # BC7 looks worse here"), "{text}");
```
    Filter the loop with `.filter(|l| !l.starts_with("target") && !l.starts_with("format"))`.
- [ ] **Step 2:** fails: the comments are gone.
- [ ] **Step 3: implement.**
```rust
/// Set `t[key] = v`, replacing an existing item in place so the key keeps its comments and position; a comment on
/// the value's own line (its decor) is carried over (audit 19).
pub(crate) fn set_value(t: &mut dyn TableLike, key: &str, mut v: Value) {
    match t.get_mut(key) {
        Some(slot) => {
            if let Some(old) = slot.as_value() {
                *v.decor_mut() = old.decor().clone();
            }
            *slot = Item::Value(v);
        }
        None => {
            t.insert(key, Item::Value(v));
        }
    }
}
```
`set_target`: replace `self.doc["target"] = toml_edit::value(s);` with `set_value(self.doc.as_table_mut(), "target", Value::from(s));`.
- [ ] **Step 4:** gates. `sets_doc.rs` callers of `set_value` gain the same behaviour; `tests/sets_doc.rs::edits_preserve_comments_and_round_trip` must still pass. Commit `gui: a comment on an edited key's line survives (audit 19)`.

## Task 9: CLI: `--nvtt` fails, folders refused, unused deps (audit 20, left-out)

**Files:**
- Modify: `crates/cli/src/main.rs:23, 41-43`; `crates/cli/src/args.rs:158-160` (help text only)
- Modify: `crates/cli/src/verbs.rs:603` (`validate_files`), `:1074` (`patch_header`)
- Modify: `crates/cli/Cargo.toml:19-20`, `crates/gui/Cargo.toml:25`, `Cargo.lock` (by `cargo` only)
- Test: `crates/cli/tests/cli_edges.rs`, `crates/cli/tests/cli_inspect_validate.rs`, `crates/cli/tests/cli_patch_header.rs`

- [ ] **Step 1: failing tests.** `cli_edges.rs`:
```rust
/// Owner ruling (DECISIONS Note Y9): `--nvtt` on convert is a usage error until the NVTT backend exists (M5).
#[test]
fn nvtt_on_convert_is_a_usage_error() {
    let d = tempfile::tempdir().unwrap();
    img(&d.path().join("in/a.png"));
    let out = d.path().join("out");
    let o = convert(&d.path().join("in"), &out, &["--nvtt", "nvcompress.exe"]).output().unwrap();
    assert_eq!(code(&o), 2, "{}", stderr(&o));
    assert!(stderr(&o).contains("the NVTT backend is not available yet (M5)"), "{}", stderr(&o));
    assert!(o.stdout.is_empty(), "nothing runs");
    assert!(!out.exists());
}
```
`cli_inspect_validate.rs` (use its `sdc()`, `code()` and `text()` helpers):
```rust
/// Audit left-out: a folder is a usage error naming it, not "Access is denied".
#[test]
fn validate_given_a_folder_is_a_usage_error() {
    let d = tempfile::tempdir().unwrap();
    let o = sdc().arg("validate").arg(d.path()).output().unwrap();
    assert_eq!(code(&o), 2, "{}", text(&o.stderr));
    assert!(text(&o.stderr).contains("is a folder"), "{}", text(&o.stderr));
}
```
The same test goes in `cli_patch_header.rs` with `sdc().arg("patch-header").arg("--dry-run").arg(d.path())` (check the verb's required flags in `args.rs`, `PatchHeaderArgs`).
- [ ] **Step 2:** all three fail (exit 0 / exit 1).
- [ ] **Step 3: implement.**
  - `main.rs` `convert`: as its first statement,
```rust
    if a.nvtt.is_some() {
        return Err(EngineError::Usage(
            "--nvtt: the NVTT backend is not available yet (M5)".into(),
        ));
    }
```
    Delete the `eprintln!` note at `:41-43`. `args.rs:158`: `/// Path to nvcompress.exe. Not available yet (M5): passing it is a usage error.` `sdc encoders --nvtt` (`args.rs:167-169`, `verbs.rs:1003`) is unchanged; `cli_presets.rs:131` still covers it.
  - `verbs.rs`: at the top of `validate_files`, and of `patch_header` over `a.dds`:
```rust
    if let Some(p) = a.dds.iter().find(|p| p.is_dir()) {
        return Err(EngineError::Usage(format!("{} is a folder; give .dds files", p.display())));
    }
```
  - Deps:
    - First, `grep -rn "schemars\|anyhow" crates/cli/src crates/cli/tests crates/gui/src crates/gui/tests crates/gui/examples crates/gui/build.rs` must print nothing (it prints nothing at 7765cea).
    - Then delete `schemars.workspace = true` and `anyhow.workspace = true` from `crates/cli/Cargo.toml`, and `anyhow.workspace = true` from `crates/gui/Cargo.toml`.
    - Run `cargo build --workspace --offline`. `git diff Cargo.lock` must show only those edges removed from the `sdc` and `sdc-gui` entries. The workspace `anyhow` and `schemars` stay: bench and engine use them.
- [ ] **Step 4:** gates. Commit `cli: --nvtt on convert is a usage error until M5 (owner)`, `cli: validate and patch-header refuse a folder (audit left-out)`, `cli, gui: drop unused schemars and anyhow (audit 20)`.

## Task 10: Engine lean: one copy of each rule, no dead code, base level freed (audit 20, 21)

**Files:**
- Modify: `crates/engine/src/image.rs:26-28, 49` (delete `est_bytes` and its assert)
- Modify: `crates/encoders-sys/src/lib.rs:51-53` (delete `active_bc1_model`), `crates/encoders-sys/tests/init.rs:1, 7`
- Modify: `crates/engine/src/mips.rs:28-36` (`level_count` uses `full_chain_len`)
- Modify: `crates/engine/src/rnm/blend.rs:13-20` (`unit` uses `normalize_vector`)
- Modify: `crates/engine/src/quantize.rs:30-35` (`clamp_half`), `crates/engine/src/pipeline.rs:84-90`
- Modify: `crates/engine/src/config.rs:330-336, 533-539, 543-549` (one helper)
- Modify: `crates/engine/src/pipeline.rs:266-270` (free `work.img`)

**Interfaces:**
```rust
// quantize.rs
/// `v` clamped to the half-float range `[0, 65504]`; -0.0 and NaN give 0. Quantise and the BC6H peak check share it.
pub(crate) fn clamp_half(v: f32) -> f32;
// config.rs (private)
fn serde_id<T: serde::Serialize>(v: T) -> String;
```

- [ ] **Step 1: tests.** These are refactors, so the existing tests guard them, and they must pass unchanged:
  - `mips::tests::level_counts` (includes 1×1);
  - `dds_write.rs::full_chain_len_counts_to_one_by_one`;
  - the `rnm::blend` and `quantize` tests;
  - `golden.rs` in full;
  - `bake/mod.rs` tests (roughness bake bit-identity, `:454-500`);
  - `grass.rs` tests (`the_chain_stops_at_one_by_two`, `:231`).
  - Add one test for the half clamp, in `quantize.rs` tests:
```rust
    #[test]
    fn clamp_half_matches_the_documented_range() {
        assert_eq!(clamp_half(-0.0).to_bits(), 0);
        assert_eq!(clamp_half(f32::NAN), 0.0);
        assert_eq!(clamp_half(1e9), 65504.0);
        assert_eq!(clamp_half(1.5), 1.5);
    }
```
- [ ] **Step 2: implement**, one commit per item:
  1. Delete `Rgba32F::est_bytes` and the assert at `image.rs:49`.
  2. `active_bc1_model`: it has no caller outside `encoders-sys/tests/init.rs:7` (checked with `grep -rn active_bc1_model crates`). Delete the function, the import at `init.rs:1` and the assert at `init.rs:7`. The `ModelMismatch { active: Ideal, .. }` assert right after it still proves the active model.
  3. `mips.rs`: `let full = crate::dds::full_chain_len(w.max(1), h.max(1)) as u32;` (the `.max(1)` keeps the size-0 guard). Update the doc: "full = `dds::full_chain_len`, the one chain-length rule the DDS header shares".
  4. `blend.rs`:
```rust
pub(crate) fn unit(v: V3) -> V3 {
    // The one degenerate-normal rule (audit 20): `mips::normalize_vector`, here on a bare vector.
    let mut p = [v[0], v[1], v[2], 0.0];
    crate::mips::normalize_vector(&mut p, crate::ops::Domain::Normal);
    [p[0], p[1], p[2]]
}
```
  (Same threshold, same `[0, 0, 1]` fallback, same division per component, so the result is bit-identical.)
  5. `quantize.rs`: add `clamp_half` (`if v > 0.0 { v.min(HALF_MAX) } else { 0.0 }`). `half_bits` becomes `f16::from_f32(clamp_half(v)).to_bits()`. `pipeline.rs::source_peak` uses `crate::quantize::clamp_half` in place of its closure `c`.
  6. `config.rs`: a private `fn serde_id<T: serde::Serialize>(v: T) -> String { serde_json::to_value(v).unwrap().as_str().unwrap().to_owned() }`. `RawMap::id`, `Subtype::id` and `Format::id` each become `serde_id(self)`. Callers are unchanged.
  7. Audit 21, `pipeline.rs` after `let mut chain = match grass_chain { … };`:
```rust
    // Audit 21: mip 0 is in `chain` now; nothing below reads `work.img` (`to_encoder_input` reads only
    // `alpha_present` and `domain`), so free the source-sized copy before quantise, the peak.
    work.img = Rgba32F::new(0, 0, [0.0; 4]);
```
  (`Rgba32F` is in scope at `pipeline.rs` via `crate::image`; import it if not. Grass already returns an empty `img`.)
- [ ] **Step 3:** gates after each item. Commit messages: `engine: delete unused Rgba32F::est_bytes (audit 20)`, `encoders-sys: delete test-only active_bc1_model (audit 20)`, `engine: level_count uses the DDS chain-length rule (audit 20)`, `engine: RNM unit is mips::normalize_vector (audit 20)`, `engine: one half-float clamp for quantise and the BC6H peak (audit 20)`, `engine: one serde id helper (audit 20)`, `engine: free the base image before quantise (audit 21)`.

## Task 11: Bench and repo: green without a corpus, one PSNR, S4 spike removed, Note Y (audit 18, 20, left-out)

**Files:**
- Modify: `crates/bench/src/corpus.rs:5-9` (add `corpus_root`)
- Modify: `crates/bench/tests/gates.rs` (8 non-ignored tests), `crates/bench/tests/engine_gates.rs` (2), `crates/bench/tests/basics.rs:49-55` (1)
- Modify: `crates/engine/src/validate/mod.rs:102-118` (`pub fn psnr`, `pub fn psnr_from_mse`), `crates/bench/src/metrics.rs:3-21`
- Modify: `.gitattributes:2`, `tools/regen-ispc.ps1:9, 19`
- Delete: `spikes/s4-scalar/` (8 tracked files, 5,513 lines); modify root `Cargo.toml:4` (drop `exclude = ["spikes"]`; `spikes/` holds nothing else)
- Modify: `docs/DECISIONS.md:128, 133-136` (Spike S4), append Note Y; `docs/DECISIONS.md:503` (T5 amendment pointer)

**Interfaces:**
```rust
// bench corpus.rs
/// The corpus root, or `None` when `SDC_BENCH_ROOT` is unset and the default folder is missing (a fresh clone).
pub fn corpus_root() -> Option<PathBuf>;
// engine validate
pub fn psnr_from_mse(mse: f64) -> f64;
pub fn psnr(a: &[[u8; 4]], b: &[[u8; 4]], ch: &[usize]) -> f64;
```

- [ ] **Step 1: tests.** Corpus tests: each of the 11 begins with
```rust
    let Some(root) = corpus_root() else {
        eprintln!("skipped: no bench corpus (set SDC_BENCH_ROOT)");
        return;
    };
```
and passes `&root` where it passed `&bench_root()`. `reads_bench_results_csv` uses `root` the same way. The ignored tests (`s5b…`, `s5c…flag_on…`, `s7_nvtt…`) keep `bench_root()`, since they run by hand. The proof: temporarily set `SDC_BENCH_ROOT=` to an empty folder name, and `cargo test -p sdc-bench --offline` is green. Then run it unset on the owner's machine: the corpus is found and the gates run.

For an explicitly set root that is missing, `corpus_root` returns `Some` and the test fails loudly. That is intended: a misconfiguration is not a skip.
```rust
pub fn corpus_root() -> Option<PathBuf> {
    match std::env::var_os("SDC_BENCH_ROOT") {
        Some(p) => Some(PathBuf::from(p)),
        None => {
            let d = bench_root();
            d.is_dir().then_some(d)
        }
    }
}
```
PSNR: `crates/bench/tests/basics.rs::psnr_matches_the_python_formula` must pass unchanged after delegation.
- [ ] **Step 2: implement.**
  1. Bench skip, as above. Commit `bench: corpus tests skip without a corpus (audit 18)`.
  2. `.gitattributes:2`: replace `tests/** -text` with
```
crates/*/tests/fixtures/** -text
crates/*/tests/data/** -text
```
     Verify that `git check-attr text -- crates/nif/tests/fixtures/README.md crates/bench/tests/data/pre-patch-bc7e.sha256` prints `unset` for both. `git ls-files --eol crates/*/tests/fixtures crates/*/tests/data` must show `i/` unchanged (no renormalisation is needed for `-text`). To see the fresh-clone state:
     - remove those tracked files from the working tree;
     - `git checkout -- crates/*/tests/fixtures crates/*/tests/data`;
     - run the full gates. This includes render3d's fixtures, which now check out LF. Report any failure to the owner rather than fixing another team's tests.
  3. `tools/regen-ispc.ps1:9`: `[string]$Ispc = "ispc.exe",`. Before `$ver = & $Ispc --version` (`:19`) add
```powershell
if (-not (Get-Command $Ispc -ErrorAction SilentlyContinue)) { throw "ISPC 1.31.0 not found as '$Ispc'; pass -Ispc <path to ispc.exe>" }
```
     Commit `repo: .gitattributes reaches the test fixtures; regen-ispc takes ispc from PATH (audit 18)`.
  4. PSNR: in `validate/mod.rs`, split today's private `psnr` into `pub fn psnr_from_mse(mse: f64) -> f64` (the `<= 1e-12 → 99`, else `10·log10(65025/mse)` part) and `pub fn psnr(...)`, which calls it. In `bench/src/metrics.rs`:
```rust
/// The engine's rule (validation and the engine-parity gate must agree, audit left-out).
pub use engine::validate::psnr_from_mse;

pub fn channel_psnr(src: &[[u8; 4]], dec: &[[u8; 4]], ch: &[usize]) -> f64 {
    assert_eq!(src.len(), dec.len());
    engine::validate::psnr(src, dec, ch)
}
```
     The values are identical for non-empty input. For empty input the engine gives 99 where the bench gave NaN; the corpus has no empty image. Commit `bench: PSNR is the engine's (audit left-out)`.
  5. S4 spike:
     - `git rm -r spikes/s4-scalar`, and drop `exclude = ["spikes"]` from the root `Cargo.toml`.
     - In `docs/DECISIONS.md` Spike S4, replace `Lives only in \`spikes/s4-scalar/\` (own \`[workspace]\`); not part of the shipped build.` with `It lived in \`spikes/s4-scalar/\` (own \`[workspace]\`), removed 2026-10-13 (Note Y10); rebuild it from the commit and SHA-256 above.`
     - Replace "so it remains a viable fallback if the ISPC objects ever fail on a needed configuration;" with "so it could be rebuilt as a fallback (from the pinned source above) if the ISPC objects ever fail on a needed configuration;".
     - Replace the first half of the Reproduce line with "Reproduce: rebuild the spike from the pinned source, then".
     - `Cargo.lock` is unchanged: the spike has its own workspace.
     - Commit `repo: remove the S4 scalar spike; DECISIONS records how to rebuild it (owner)`.
  6. DECISIONS:
     - Append `## Note Y — audit fixes (plan rulings, 2026-10-13)` with Y1-Y10 exactly as in this plan's Plan rulings.
     - Append to T5 (`DECISIONS.md:503`): "Amended by Note Y6: coverage now runs on every level."
     - Commit `docs: DECISIONS Note Y (audit fixes)`.
- [ ] **Step 3:** gates (bench tests green with and without a corpus). Final full gate run on the branch head.

# GUI data side (model, project files, worker, export writers)

Scope read: `crates/gui/src/{app,worker,project,settings,sets_doc,crash_log,main,lib}.rs`, `settings/layout.rs`, `model/{mod,history,export,pbr_export,cs_pbr_export,cm_json,sets,items,bulk,rename,input_name,dds_patch,presets,preview,heatmap,rnm_preview,used_by,out_size,messages,shell,known_paths,encoding_follow,pbr_follow,fields,pack_fills,auto_flags,texture_view,pin,plugin_refs,picker,sections,skin,rnm,inspector,cs_pbr}.rs`, `pgp/{prefix,writer}.rs`, the test lists of `tests/{history,settings,project,export,pbr_export,cs_pbr_export,cm_json,sets_doc,model,edges,crash_recovery,audit_gui_data}.rs`.

1. **Every CS/CM/skin lookup re-parses the whole `[[sets]]` array, and the export blocks do it once per set** (`crates/gui/src/sets_doc.rs:404-410`, `crates/gui/src/model/cs_pbr.rs:345-355`, `crates/gui/src/model/pbr_export.rs:310,357,367`, `crates/gui/src/model/cm_json.rs:111-118`, `crates/gui/src/model/skin.rs:77-86`, `crates/gui/src/model/sets.rs:1545-1548`)
   - **What this is:** `SetsDoc::entries()` walks every `[[sets]]` table and converts each value through `toml_to_json` (format a one-line TOML string, parse it again, convert to JSON). `cs_block`, `cm_block`, `skin_values`, `output_name_of` and `statics_status` each call `entries()` and then keep one set.
   - **Problem:** `build_blocks` (the export-blocks cache) calls `m.pgp_entry_wanted(v)` for every set view, and `written_json_entries` does it again, so one rebuild is O(set views × `[[sets]]` entries) full document parses. With 300 sets in the plan and 150 user-touched entries (each ~20 settings), one rebuild is ~300 × ~5 ms ≈ 1.5 s on the UI thread, and the rebuild runs on every edit and every plan (the key includes `inputs_rev`). The set editor also parses the document ~6 times per frame (`cs_block` ×2 at `ui/cs_pbr.rs:42`, `statics_status` at `ui/cs_pbr.rs:310`, `cm_block` at `ui/cm_card.rs:34`, `skin_values` at `ui/skin_card.rs:33`, `output_name_of` at `ui/set_editor.rs:108`), ~30 ms per frame at that size.
   - **Fix:** cache the parse on the document, keyed by its revision, which every mutator already bumps: `entries_cache: RefCell<Option<(u64, Arc<Vec<SetEntry>>)>>` in `SetsDoc`; `entries()` returns `Arc<Vec<SetEntry>>` (or keeps its signature and clones out of the cache). Nothing else changes: `rev` is bumped by all 30 mutators, `reload` and `restore` included.
   - **If we skip it:** the set editor and every edit get slower quadratically with the number of sets; a project with a few hundred edited sets stalls for a second per keystroke.
   - **Group:** Should fix. **Kind:** scale.
   - **Confidence:** high on the mechanism (traced every `entries()` caller: 29 sites; checked that all `SetsDoc` mutators bump `rev`); medium on the magnitude (no profile; cost estimated from `toml_to_json`'s format+parse per value).

2. **The post-export hash record writes `sdc-project.toml` with whatever unsaved edits were made during the export** (`crates/gui/src/model/mod.rs:1491-1507`, `crates/gui/src/model/cs_pbr_export.rs:293-302`, `crates/gui/src/model/cm_json.rs:486-495`)
   - **What this is:** after a clean export the model stores the SHA-256 of the JSON it wrote in `[project]` and calls `d.save(false)`, which writes the whole in-memory document.
   - **Problem:** set edits (`ui/set_editor.rs` gates on `sets_editable()`, not `busy()`) and Undo/Redo (`ui/header.rs:187`, `ui/actions.rs:252`, enabled by `can_undo` only) still run while an export runs. Case: Export (saves both files), then Ctrl+Z twice while it runs, export ends → `record_json_sha` saves `sdc-project.toml` with the two steps undone. The user never pressed Save; closing and choosing "Discard" cannot bring the file back, and `sdc.toml` on disk is still the exported state, so the two files disagree. Same path for `finish_cs_pbr` and `finish_cm_json`.
   - **Fix:** in all three, take `let was_dirty = d.is_dirty();` before `set_*_sha`; when `was_dirty`, skip `d.save(false)` and push a note ("hash recorded; written with the next Save"). The in-memory hash is still right, and a stale on-disk record only makes the next export ask before overwriting its own file (safe). Add the test: edit a set value after `export_clicked()`, `wait_idle`, assert the file on disk does not contain the edit and `is_dirty()`.
   - **If we skip it:** edits made during an export are committed to disk behind the user's back, and "Discard" lies.
   - **Group:** Should fix. **Kind:** bug.
   - **Confidence:** medium-high (traced undo → `apply_snapshot` → `restore` marks dirty → `ExportDone` → `record_json_sha` → `save(false)`; the existing tests only undo after the export has ended).

3. **`view_key()` stats the scan database on every set-view lookup** (`crates/gui/src/model/mod.rs:1764-1777`, `crates/gui/src/model/cs_pbr.rs:457-460,537-543`)
   - **What this is:** the set-view cache key includes `cs_pbr::stamp(scan_db)`, a `std::fs::metadata` call, so the key check itself is a syscall. `views()` has 46 call sites in the model and 6 in the UI; the set editor, slot menus, `locked_items`, `slot_locked` (per card), `fill_proposals` and `rename_keys` each go through it every frame. `CsPbrScan::sync` adds one more stat per `facts()`/`records()` call.
   - **Problem:** 20–100 `GetFileAttributesEx` calls per frame on the UI thread (~20–50 µs each on NTFS, far more when `%LOCALAPPDATA%` is on a slow or roaming drive): 1–5 ms per frame doing nothing, and with finding 1 one per set in `build_blocks`.
   - **Fix:** replace the stamp in `ViewKey` with a counter: add `scan_rev: u64` to `AppModel`, bump it where the app already reacts to a finished scan (`app.rs` after `scan_ended`, next to `refresh_mesh_votes(false)`), and in `CsPbrScan::sync` compare against that counter instead of stat'ing; or at least route the stat through the existing 1 s `StatCache` (`self.stats`).
   - **If we skip it:** a steady per-frame syscall cost that scales with how many panels are open, invisible on a fast SSD and painful on a network profile.
   - **Group:** Should fix. **Kind:** speed.
   - **Confidence:** medium-high (read `view_key`, `views`, `stamp`, `sync`; counted call sites; did not profile).

4. **Two export-end paths disagree with the normal one** (`crates/gui/src/model/mod.rs:980-984` and `1042-1072`)
   - **What this is:** an export ends through `ExportDone` (stops the pace clock, writes the JSON/CS/CM files or their "not written" notes, toasts the result) or through `Panicked { what: "export" }` (only `running = None` + `run_after_export`).
   - **Problem:** (a) after a worker panic the drawer's `pace` is never stopped, so the elapsed time keeps counting while the export shows as ended; `pending_json/cs/cm` stay set and no "JSON not written" note appears. (b) `ExportDone(Err(e))` with a queued close/open (`after_export` set) skips the error dialog on purpose, but also never logs it: `export.error` is set and then `open_folder` resets `self.export`, so a real failure (not a cancel — a cancel comes back as `Ok(summary)` with `cancelled` filled) is lost without a trace in Messages.
   - **Fix:** one `fn export_ended(&mut self, result: Result<BatchSummary, String>)` used by both arms; the panic arm calls it with `Err(format!("internal error: {message}"))`. In the `Err` branch always `self.messages.log(Severity::Error, …)` before deciding about the dialog.
   - **If we skip it:** a failed export can vanish without a message when the user had asked to leave; after an internal error the drawer shows a running clock on a dead export.
   - **Group:** Nice to have. **Kind:** bug.
   - **Confidence:** high (read both arms; the existing test `a_worker_panic_in_the_export_still_lets_the_pending_close_happen` checks only the close).

5. **Scan-database read failures are reported with `eprintln!` in a program that has no console** (`crates/gui/src/model/mod.rs:807-809`, `crates/gui/src/model/known_paths.rs:102`; `main.rs:1` is `windows_subsystem = "windows"`)
   - **What this is:** the mesh-votes read (`votes_landed` `Err`) and the known-paths read print to stderr, which Windows discards for a GUI subsystem binary.
   - **Problem:** an old-schema or locked scan database (a scan running in another instance, a DB from a newer build) makes every plan keep stale mesh evidence and stale path pickers with no visible sign; the comment says "keeping the previous mesh evidence (stderr)".
   - **Fix:** `self.messages.log(messages::Severity::Warning, format!("Scan database: {e}; keeping the previous mesh evidence"))` in both places (the log dedupes nothing, so guard with a `last_db_error: Option<String>` as `save_settings` does for gui.toml).
   - **If we skip it:** silent wrong shader/alpha decisions until the user happens to rescan.
   - **Group:** Nice to have. **Kind:** bug.
   - **Confidence:** high (grepped all three `eprintln!` in the GUI).

6. **The DDS header-patch state is not reset when another folder opens** (`crates/gui/src/model/mod.rs:603-668` never touches `self.dds_patch`; `crates/gui/src/model/dds_patch.rs:20-62`)
   - **What this is:** `DdsPatchState` holds a per-item-key cache (stamp, chosen tag, view), the key awaiting Patch/Cancel (`confirming`) and the last result line (`last`), all keyed by `[files]` key.
   - **Problem:** open folder A, patch `textures/armor/x.dds` (result line "DDS-HEADER-…: written"), open folder B that also has `textures/armor/x.dds`: its panel shows A's result line and A's tag choice; if "Patch header…" was pressed and not answered, B's item opens straight on the confirmation. Everything else in `open_folder` is reset (sizes, stats, used-by, bulk, dropped fills); this one was missed.
   - **Fix:** add `self.dds_patch = Default::default();` to `open_folder`.
   - **If we skip it:** a wrong "patched" claim on another folder's file, and a one-click patch confirmation carried across folders.
   - **Group:** Nice to have. **Kind:** bug.
   - **Confidence:** high (grepped every `dds_patch` reference in `mod.rs`).

7. **Undo history keeps two full copies of both documents per step** (`crates/gui/src/model/history.rs:20-37`, `crates/gui/src/model/mod.rs:1827-1839,1841-1870`)
   - **What this is:** DECISIONS P9 decides on text snapshots, 200 steps. Each `Command` stores `before` and `after`, each the whole `sdc.toml` plus the whole `sdc-project.toml`; step N's `before` is byte-identical to step N-1's `after`.
   - **Problem:** after a bulk edit on 3000 textures `sdc.toml` is ~150–300 KB; 200 further steps hold 200 × 2 × ~300 KB ≈ 120 MB of strings (plus the redo stack), all duplicates of each other, and every edit serialises both documents twice (`snapshot_of` before and after) just to compare them.
   - **Fix:** make `Snapshot`'s fields `Arc<str>`; in `History::push`, when `c.before == last.after` reuse `last.after.clone()` (halves memory at no cost); or keep only `after` per step and take `before` from the previous step / a base snapshot. P9 stays: the undo model is still text snapshots.
   - **If we skip it:** a long session on a big folder quietly holds ~100 MB of identical TOML.
   - **Group:** Nice to have. **Kind:** scale.
   - **Confidence:** medium (sizes estimated, not measured; the duplication itself is certain).

8. **Lean: the same small helpers are written in five model files** (`crates/gui/src/model/sets.rs:405-412` `dir_of`/`base_of`, `rename.rs:153-182` `base_of`/`split_path`/`lower`, `input_name.rs:20` `lower`, `bulk.rs:559` and `encoding_follow.rs:195` `textures(n)`, `pbr_export.rs:394` `path_lower`, `export.rs:85,296` `slash`/`norm`, `sections.rs:69` `norm`)
   - **What this is:** two `base_of` (one returns `String`, one `&str`), two identical `lower`, two identical `textures(n)` pluralisers, four "backslash to slash and lower-case" normalisers with slightly different trimming, plus one-line wrappers that only forward: `json_name_problem` (`pbr_export.rs:139`), `edid_problem` (`cs_pbr_export.rs:63`), `reload_changed` (`mod.rs`, forwards to `replan`, one UI caller).
   - **Problem:** `sets::base_of` and `rename::base_of` must stay equal (the duplicate-into name and the rename dialog both split the same stem) and nothing ties them together; a change to one silently changes which files `drop_new_set` names versus what the rename preview shows.
   - **Fix:** keep `rename::base_of` (the `&str` one) and delete `sets::base_of`; one `lower`, one `textures`, one `path_lower` in `sets.rs` (or a tiny `model/text.rs`); inline the three forwarding wrappers. ~60 lines go.
   - **If we skip it:** near-copies that drift.
   - **Group:** Nice to have. **Kind:** lean.
   - **Confidence:** high (grepped the whole tree for each name).

9. **Lean: PGPatcher's suffix→slot table and the Windows file-name rule each exist twice** (`crates/gui/src/pgp/prefix.rs:10-22` `slot_suffixes` vs `crates/pgpsim/src/text.rs:21-41` `SUFFIXES`; `crates/gui/src/pgp/prefix.rs:48-75` `check_file_name` vs `crates/gui/src/model/pbr_export.rs:144-176` `file_name_problem`)
   - **What this is:** the GUI already depends on `pgpsim` (Cargo.toml), yet carries its own copy of `getTexSuffixMap` (19 entries, same content, different shape). `file_name_problem` re-implements the device-name (`CON`, `COM1`…) and trailing-dot checks that `check_file_name` already has, then adds its own `..` rule.
   - **Problem:** a PGPatcher release that adds a suffix (as `_f`/`_cnr` were added) has to be mirrored in two crates; the two name checkers already differ (`file_name_problem` refuses any name containing `..`; `check_file_name` does not), so the JSON name and an EDID are judged by a different rule than an output name.
   - **Fix:** `pub` the pgpsim table and let `slot_suffixes(n)` filter it (or delete `slot_suffixes` and use `pgpsim::text::tex_base_slot`); make `file_name_problem` call `check_file_name` and only add the `..` line.
   - **If we skip it:** the two tables drift on the next PGPatcher update.
   - **Group:** Nice to have. **Kind:** lean.
   - **Confidence:** high (diffed the two tables entry by entry).

10. **The two-pass plan's loop bound has no test** (`crates/gui/src/model/mod.rs` `finish_plan`: `auto_replans < 2`)
    - **What this is:** after a plan lands, `finish_plan` compares the engine inputs with those the plan was requested with and re-plans; `auto_replans` stops this after two rounds and `plan_unsettled` then turns the next Export into an error dialog.
    - **Problem:** no test exercises an input set that never settles. Removing the `< 2` bound (or resetting `auto_replans` in the wrong place) would make such a project plan forever (each plan reads the scan DB and re-plans) and no test fails; the only test naming the error text (`an_owned_path_on_a_mirrored_output_settles_and_blocks_by_name`) checks a case that does settle.
    - **Fix:** one test that injects alternating `WorkerMsg::MeshVotes` results through `sender()` (a different `gras_diffuses` set each time, so `EngineInputs` never equal `last_inputs`), then asserts `generation()` advanced by at most 3 and `export_clicked()` ends in the "sets changed while planning" dialog.
    - **If we skip it:** the bound is one refactor away from an infinite plan loop.
    - **Group:** Nice to have. **Kind:** missing test.
    - **Confidence:** medium (grepped tests for `auto_replans` and the dialog text).

Smaller ones left out (7): `cs_pbr_export::finish` deletes sdc's own old-case file before writing the new one, so a write failure right after leaves no file and drops the record (regenerable); `SizeCache::clear_folder` keeps the in-flight job, whose old-folder results then stay in `dims` for the session; `settings::merge` does not re-apply `truncate(10)` to `recent`; `check_sources()` plus `export_clicked()` start two plans for one hand edit; `History::push` keeps a merged step whose `after` returned to its `before`; `hand_edited` reads the JSON file twice (`sha256_of` then `read_to_string`); `GuiSettings::save` re-reads and re-parses `gui.toml` on every write to decide about `.bad`.

Not read: `model/meshes.rs` beyond lines 1-1010 and 1120-1410 (list/search views), `model/set_preview.rs` and `model/viewport3d.rs` except the texture-cache sections, `model/view.rs`, `model/nav.rs` except `item_for_game_path`, `model/readout.rs` beyond the types, `pgp/{schema,settings,validate,alpha}.rs` beyond the function lists, `settings/layout.rs` read fully but not traced into `layout.rs`, and the UI test files.
Lean: ~60 lines of duplicated helpers and forwarding wrappers (finding 8), ~40 lines of a copied suffix table and a second file-name checker (finding 9); no dependency can go (regex, sha2, hex, toml_edit, rfd, arboard, image are all used).

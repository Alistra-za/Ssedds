# Readers and scan database audit (crates/nif, vfs, esp, modscan, pgpsim)

Overall: the parsers are already hardened. Every length/count/offset in `nif` goes through `Cursor::take` (checked add, bound to the slice), the three variable-size loops in `graph.rs`/`skin_partition.rs`/`vertex_desc.rs` check `n * size <= remaining` before any `reserve`, `Header` caps blocks and strings at 1M, `index3d` is iterative with a seen-set and a 256 depth cap, `read_graph` checks every parent insert for cycles, `encoding_rs` decoding is lossy, and `scan::scan_guarded` turns a parser panic into a `Failed` copy. `vfs::bsa` bounds the LZ4 output with `take(orig + 1)` and refuses `orig > 512 MiB`; `esp` bounds zlib the same way at 64 MiB and caps CELL/WRLD nesting at 8. The four `unsafe` blocks in `modscan/src/win.rs` set `dwSize`, close every handle on every path, pass the registry buffer length in bytes, and free the shell string; they hold. The schema rebuild, the `scan.lock`, chunk transactions, `commit_pending`/`discard_pending`, `delete_unseen` and the two-GUI-thread "Busy" case all have tests that fail when they break. The "which mod wins" rule (loose beats BSA, then MO2 priority / BSA rank, overwrite = i32::MAX, Data = -1) is one function (`resolve::winners`) used for meshes, source meshes and textures, and the path key is one function (`nif::path::canon_mesh` / `canon_texture` / `canon_model`) at every place a key is built (loose walk, BSA entries, plugin paths, diff JSON, store queries). What is left is below.

1. **Source CRCs of a cached NIF are read serially on the main thread, and a BSA source is never verified once cached** (`crates/modscan/src/scan.rs:395-420`, `:644-656`)
   - **What this is:** PGPatcher output is checked for staleness by comparing the CRC32 of the source NIF with the CRC PGPatcher wrote in `ParallaxGen_Diff.json`. The parallel parse pass computes that CRC (`candidate[i]`) only for copies it parses; a copy reused from the store by (size, mtime) keeps whatever `crc32` the store has, which is NULL when the earlier scan had no PGPatcher output. Step 6 then fills the gap with `crc_of`, which `std::fs::read`s each source file one at a time on the scan thread, and returns `None` for a BSA entry.
   - **Problem:** The common flow: scan, run PGPatcher (3,000–20,000 patched meshes), scan again. Every source was parsed in scan 1 without a CRC and is reused in scan 2, so `crc_of` reads thousands of NIFs serially after the parallel pass, with progress stuck at "resolve 0/1" (tens of seconds on SSD, minutes on HDD). Worse, for a source that lives in a BSA (every vanilla mesh PGPatcher patches, e.g. `Skyrim - Meshes0.bsa`) `crc_of` gives `None`, so the derived copy is `Unverified` and stays `Unverified` on every later scan, because the reuse path never reads the entry again. Spec 05 §5.1 says "unverified until the BSA bytes are read, then match"; the parse path already does read and hash BSA entries (`candidate` is Loc-agnostic), so the same copy gets `match` on a cold store and `unverified` on a warm one.
   - **Fix:** Do not reuse a candidate that has no CRC; let the parallel pass read and hash it (it hashes BSA entries too). In the reuse loop:
     ```rust
     Some(m) if m.size == Some(c.size) && m.mtime == Some(c.mtime)
         && !(candidate[i] && m.crc32.is_none()) => { /* reuse */ }
     _ => work.push(i),
     ```
     Every candidate then has a CRC in `crc_known`, so `crc_of` reduces to `crc_known.get(&i).copied()`; delete its `fs::read` fallback, `new_crcs`, and the `crcs` parameter of `Store::set_derived` (store.rs:1278-1290).
   - **If we skip it:** One long unexplained stall per "scan after PGPatcher" and a permanently wrong `unverified` badge on every vanilla-source patched mesh.
   - **Group:** Should fix.  **Kind:** bug / speed.
   - **Confidence:** high (traced `candidate`, the reuse loop, `crc_of`, `set_derived`, and the spec line; no test covers a second scan after the output appears, see 2).

2. **Missing test: a scan that reuses sources, then a PGPatcher output appears** (`crates/modscan/tests/scan.rs`)
   - **What this is:** All PGPatcher tests (`winner_and_source_with_pgpatcher_output`, `stale_when_source_changed_even_with_older_mtime`, `stale_reordered_when_diff_matches_a_deeper_copy`, `dups_are_dup_of_...`) put the output in place before the first scan, so the sources are parsed as candidates and the CRC fallback path of finding 1 is never run.
   - **Problem:** Finding 1 (and its fix) is invisible to the suite: a regression that makes `crc_of` return `None` for loose files, or a fix that stops computing CRCs on reuse, passes every test.
   - **Fix:** One test: scan with `+A` only; add `mods/PGP` (diff JSON with A's CRC, `PGPatcher.esp`, `meshes/x.nif`), enable it, scan again; assert `r.reused == 1` and A's row has `derive_check == Some("match")`. A second variant with the source in a BSA (the `common::bsa` builder) asserting `match`, not `unverified`.
   - **If we skip it:** The staleness verdict, the one thing the derived layer exists for, has no test on its warm-cache path.
   - **Group:** Should fix.  **Kind:** missing test.
   - **Confidence:** high (grepped every `ParallaxGen_Diff` and `go(&d` in the tests).

3. **`tex_copy` is rewritten every scan and never read** (`crates/modscan/src/store.rs:43`, `:1203-1225`; `crates/modscan/src/scan.rs:604-624`)
   - **What this is:** `set_textures` deletes `tex_copy` and inserts one row per DDS copy per origin, every scan. The only readers are tests (`scan_dump.rs`, `lod_skip.rs`, `plugin_joins.rs`); no production query, CLI or GUI touches the table (grepped the whole tree). `tex_winner` is what the GUI uses.
   - **Problem:** A big instance has 200k–400k texture copies: that is 300k deletes and 300k inserts per scan inside the "textures" phase for nothing, and the table and its `size`/`mtime` columns live in the schema forever. (Already listed in AUDIT-2026-10-14 as write-only; still present, no decision in DECISIONS.md/RULINGS.md keeps it.)
   - **Fix:** Drop the table from `SCHEMA`, the `copies` parameter and the first loop of `set_textures`, and the `tex_rows` vec in `scan.rs`; adjust the three tests; bump `SCHEMA_VERSION`. Same for the write-only `nif_copy.bsver` if wanted.
   - **If we skip it:** A second or two per scan and 20–30 lines that look load-bearing but are not.
   - **Group:** Should fix.  **Kind:** lean / speed.
   - **Confidence:** high (grepped `tex_copy` across crates, docs, tests).

4. **An origin that fails to open in one run loses its whole cache** (`crates/modscan/src/scan.rs:792-803`, `:709-712`; `crates/modscan/src/store.rs:1314-1350`)
   - **What this is:** `walk` records a `Bsa::open` failure as a problem and moves on; the BSA's origin row was already staged and is enabled with the others, and `delete_unseen` removes every copy of an enabled origin that this walk did not see. A loose origin whose `read_dir` fails behaves the same (`vfs::loose::walk` returns an empty list).
   - **Problem:** `Skyrim - Meshes0.bsa` is unreadable for one scan (antivirus holding it, a mod manager redeploying, a transient sharing violation): the scan reports one problem and silently deletes its ~30k cached copies and their shape rows; the next scan re-parses the whole archive (the largest single parse cost in a vanilla-plus-mods instance).
   - **Fix:** Collect the indices of BSAs that failed to open (and loose roots whose `read_dir` failed) in `walk`, and leave their ids out of the `enable_run` call, so `delete_unseen` keeps their rows as cache and the origin simply does not appear in that run's lists (the problem line still says why).
   - **If we skip it:** A one-off I/O hiccup costs a full re-parse of the affected archive on the next scan.
   - **Group:** Nice to have.  **Kind:** scale.
   - **Confidence:** high on the mechanism (traced `walk`, `enable_run`, `delete_unseen`); medium on how often a BSA open fails on a real PC.

5. **`winning_nifs` lower-cases with Unicode rules; every key in the store is ASCII-lowered** (`crates/modscan/src/store.rs:1389-1397`)
   - **What this is:** Path keys are built with `canon_mesh` (`to_ascii_lowercase`), and `nifs_using_texture` lowers with SQLite's ASCII `lower()`. `winning_nifs` alone lowers the user's filter with `str::to_lowercase()`.
   - **Problem:** A mesh at `meshes/Ärger/x.nif` is stored as `meshes/Ärger/x.nif`. Typing `Ärger` in the Meshes filter becomes `ärger`, `LIKE` is ASCII-only, no row matches; typing `ärger` for a stored `ärger` works. Same filter, different rule, in two places.
   - **Fix:** `nif::path::canon_mesh(path_contains)` in place of `.replace('\\', "/").to_lowercase()` (it also trims and fixes separators, as the other call sites do).
   - **If we skip it:** Non-ASCII upper-case path components are unfindable in the "all meshes" filter; rare in Skyrim paths.
   - **Group:** Nice to have.  **Kind:** bug.
   - **Confidence:** high (compared with `canon_mesh`, `tex_query`, `nifs_using_texture`).

6. **`winners`' `exclude` parameter is `|_| false` at every production call** (`crates/modscan/src/resolve.rs:31-36`; `crates/modscan/src/scan.rs:581`, `:584`, `:609`)
   - **What this is:** The source view is produced by filtering `copies` to `nd_copies` before calling `winners`, so the closure parameter is only exercised by `tests/resolve.rs::exclude_gives_source_view`.
   - **Problem:** Two ways to do the same thing; a reader of `winners` has to work out that the parameter is a test-only path. (Also listed in AUDIT-2026-10-14; still present.)
   - **Fix:** Delete the parameter and the `if exclude(...)` line; rewrite that one test on `nd_copies`.
   - **If we skip it:** Nothing breaks; 6 lines of dead branch.
   - **Group:** Nice to have.  **Kind:** lean.
   - **Confidence:** high (grepped `winners(`).

7. **`esp::inflate` commits the declared size before decoding; `vfs::bsa` deliberately does not** (`crates/esp/src/read.rs:423`; `crates/vfs/src/bsa.rs:207-209`)
   - **What this is:** Both decompressors cap the declared size and `take(want + 1)`. `bsa.rs` grows the output `Vec` ("a capacity of `orig` would commit up to MAX_ENTRY per reader on the word of the entry header, audit 2026-10-14 finding 1"); `read.rs` does `Vec::with_capacity(want)` with `want` up to 64 MiB from the file.
   - **Problem:** A plugin whose compressed records all declare 64 MiB makes each record allocate 64 MiB before one byte inflates; `scan_plugins` parses 64 plugins per chunk on every core, so a folder of such plugins commits `threads × 64 MiB` at once. Bounded, not an OOM, but the two readers made opposite calls on the same question.
   - **Fix:** `let mut out = Vec::new();` as in `bsa.rs` (one line), or `with_capacity(want.min(raw.len() * 8))`.
   - **If we skip it:** Up to ~1 GiB transient on a hostile plugin set; a genuine record never gets near the cap.
   - **Group:** Nice to have.  **Kind:** risk / lean.
   - **Confidence:** high (read both functions and the rayon chunking in `plugins.rs:164-178`).

Smaller ones left out (6): `Bsa::read` allocates `e.size` (up to the archive's own length) per entry while rayon reads 512 entries across all cores, fine for meshes, worth a cap if the BSA reader is ever used for textures; `tex` copies from a BSA with duplicate entry names are counted twice in `tex_winner.n_copies` (`copies` dedups through `seen`, `tex` does not); `scan.rs` walks origins sequentially (walkdir per mod) where a per-origin rayon map would use the disk queue; `existing_copies()` and `copy_ids()` both scan `nif_copy` fully when the second could come back from `write_chunk`; `nif_copy.bsver`, `rec.txst_flags`, `rec.decal`, `plugin.flags` are written and never read; `file_mtime` (scan.rs) and the mtime computation in `vfs::loose::walk` are the same six lines.

Not read: `crates/pgpsim/tests/*` and `crates/modscan/tests/plugin_inert.rs`, `plugin_joins.rs`, `plugins.rs`, `scan_dump.rs` were only grepped for names and coverage, not read line by line; `crates/nif/tests/builder/mod.rs` and `tests/geom.rs` skimmed; `INERT_SQL` in `plugin_store.rs` read but not re-derived against the filtering spec. Everything else in scope was read in full.

Lean: roughly 60–80 lines and no dependencies could go: `tex_copy` table and its writer (~25 lines + `tex_rows`), `winners`' `exclude` (~6), `crc_of`'s file fallback plus `new_crcs` and `set_derived`'s `crcs` parameter after fix 1 (~20), the four write-only columns, the duplicate mtime helper (~8). No dependency in these crates is replaceable by std (`lz4_flex`, `flate2`, `rusqlite`, `encoding_rs`, `half`, `crc32fast`, `walkdir`, `rayon`, `serde_json` each do real work).

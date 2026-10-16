# Engine input and conversion path — audit

Scope: `crates/engine/src/{api,pipeline,session,load,psd,archive,image,decode,dds,dds_patch,discover,group,analyse,mips,filter,resize,coverage,dilate,quantize,encode,nvtt,grass,sysmem,ops}.rs`, `pack/`, `rnm/`, `bake/`, and the matching tests. Traced: discover → group → load (PNG/TIFF/EXR/PSD/KRA/ORA/DDS) → analyse → resolve → prepare (preprocess, mips, coverage, quantise) → encode (bc7e/rgbcx/ISPC, NVTT subprocess) → DDS bytes → validate → atomic write → sidecar → manifest.

Overall: this path is in good shape. Hostile-input handling is thorough and tested (header-first MAX_DIM checks, checked level arithmetic, zip-bomb cap from the IHDR, PackBits bounds, PNG iCCP bound, DDS reader fuzz-style tests), writes are atomic with a correct write → stamp → sidecar → manifest order and cleanup on failure, memory admission runs on the calling thread so the pool cannot deadlock on it, and NVTT paths are engine-generated (no path injection). I found no Must-fix item. The findings below are ordered by how much they would hurt the one modder on their PC.

1. **Missing-companion check stats every file in the folder, once per set** (`crates/engine/src/bake/plan.rs:350-375`, callers `:395`, `:454`)
   - **What this is:** when a PBR set has a `_n` but no roughness member (or the reverse), `sibling_outside` lists the input folder with `read_dir` and calls `p.is_file()` (a metadata syscall) on every entry to find a companion the batch does not hold. It runs once per such set, on every plan.
   - **Problem:** a flat `textures/` with 4500 files where 1500 PBR normals have no `_rmaos` yet (normal pass authored first, the common state of a pack in progress) → 1500 × 4500 = 6.75 million `GetFileAttributes` calls per plan, plus 1500 directory listings. At 3–15 µs each on NTFS that is 20–100 s of planning, and the GUI replans after every set edit. The `batch.contains(p)` test that would reject almost every entry runs *after* the stat.
   - **Fix:** take the type from the entry and test membership before anything else: `.filter_map(Result::ok).filter(|e| e.file_type().is_ok_and(|t| t.is_file())).map(|e| e.path()).filter(|p| !batch.contains(p))` (Windows `DirEntry::file_type()` needs no extra syscall). To drop the repeated listings too, read each folder once: a `HashMap<PathBuf, Vec<(PathBuf, bool)>>` built lazily in `plan_bakes` and passed to `sibling_outside` (~10 lines).
   - **If we skip it:** planning a half-finished PBR pack takes tens of seconds instead of under a second, and the GUI stalls on every edit.
   - **Group:** Should fix. **Kind:** scale.
   - **Confidence:** high that the code does this (traced both call sites, `missing_normal` and `missing_roughness`, and the cs-only gate in `plan_bakes`); magnitude estimated, not measured.

2. **An output root equal to the input folder silently discovers nothing** (`crates/engine/src/discover.rs:111-139`)
   - **What this is:** discovery skips any directory or file that sits under the output root, so the tool never reads back what it wrote. The test is a component prefix (`is_under`), which is also true when the output root *is* the input folder.
   - **Problem:** `sdc convert textures -o textures` (write the `.dds` next to the `.png`, the most common in-place modding layout) → every file is under the output root → `BatchStarted { inputs: 0 }`, nothing converted, exit 0, no message saying why. Explicit file inputs are not subject to the skip, and the engine already guards the one real in-place hazard (DECISIONS S6 `same_file` for a re-encoded DDS), so folder inputs are the only case that goes dark.
   - **Fix:** in `discover_reporting`, skip only when the output root is *strictly inside* the input folder: compute `let in_abs = std::path::absolute(input)?;` and apply the two `is_under(.., &out_abs)` filters only `if out_abs != in_abs && is_under(&out_abs, &in_abs)`. Written `.dds` files are then re-discovered on the next run as `InputKind::Dds`, which planning skips by default (`IN-DDS-SKIPPED`), and `_sdc` stays skipped. If in-place folders are meant to be unsupported, return `EngineError::Usage("output root is the input folder; …")` from `plan` instead; either is one `if`, but 0 inputs with exit 0 is wrong.
   - **If we skip it:** users running in place conclude the tool is broken; the GUI, if it ever offers the source folder as the export folder, shows an empty plan.
   - **Group:** Should fix. **Kind:** bug.
   - **Confidence:** medium: traced `is_under` and both filters; did not run it, and did not find a decision saying folder-in-place is forbidden (grepped DECISIONS/specs for "in place", "output root").

3. **Complex grass compares coverage over the stacked image, so a correct file can get `VAL-COVERAGE`** (`crates/engine/src/pipeline.rs:283-285`, `crates/engine/src/validate/mod.rs:343-367`)
   - **What this is:** coverage preservation keeps the share of alpha-test-passing texels constant across mips. For complex grass (one image, colour on top, normal map with specular-in-alpha below) `grass::build` preserves coverage on the top half only, as designed. But `base_coverage` is then measured on `chain[0]`, the *stacked* level, and `validate` compares every stacked level with it.
   - **Problem:** the bottom half's alpha is specular data, box-averaged and never coverage-corrected. A grass normal whose specular alpha varies around the threshold (codes 100–160 with threshold 128): mip 0 bottom half passes ~50 %, by mip 3 the box average has converged toward the mean and passes ~100 % (or 0 %). Stacked coverage drifts by up to 25 points against the base while the top half is exactly right → `VAL-COVERAGE` on every low mip of a correct texture, and the GUI's per-mip coverage readout is meaningless for grass.
   - **Fix:** in `prepare_then`, `let base_coverage = if grass { None } else { base_coverage(&chain[0], p, alpha_present) };` (the top half was already preserved and checked by `grass::build`'s path). Add one test in `tests/complex_grass.rs`: a source whose bottom-half alpha straddles the threshold produces no `VAL-COVERAGE`.
   - **If we skip it:** spurious warnings on grass packs with non-constant specular; users learn to ignore `VAL-COVERAGE`.
   - **Group:** Should fix. **Kind:** bug.
   - **Confidence:** medium: traced `base_coverage` → `ValidationInput` → the check; `ValCoverage` is `Warning` severity so the file still passes.

4. **The zip-bomb cap and the per-image cap disagree on bytes per texel** (`crates/engine/src/archive.rs:404-418`)
   - **What this is:** a KRA/ORA `mergedimage.png` may inflate to at most `merged_cap(w, h)`, which allows 16-bit RGBA (`h·(1 + 8w)`), but is clamped to `MERGED_CAP = MAX_DIM² × 4 + 64 MiB` (1.14 GB), an 8-bit RGBA figure.
   - **Problem:** a 16-bit RGBA Krita document of 16384² (a size `MAX_DIM` says is supported) has 2.15 GB of raw PNG data; detailed paintings compress 1.3–2×, so the stored PNG is often 1.1–1.6 GB → refused as "larger than its size in the PNG header allows (…a corrupt or hostile archive)". The same for 16-bit documents from about 12k² up. 8-bit documents are fine at every size.
   - **Fix:** make the two agree: `pub(crate) const MERGED_CAP: u64 = MAX_DIM² * 8 + MAX_DIM + (64 << 20);` (raw 16-bit RGBA plus a filter byte per row), ≈ 2.2 GB. The bomb bound stays finite and still scales with the IHDR for everything smaller.
   - **If we skip it:** large 16-bit Krita/ORA files fail with a message that calls them hostile.
   - **Group:** Nice to have. **Kind:** bug.
   - **Confidence:** medium: the arithmetic is certain; whether Krita writes the merged PNG at 16 bits for 16-bit documents I did not verify, but `merged_cap` itself budgets for it.

5. **Bake and pack planning rescan every job per set; the RNM planner already indexes once** (`crates/engine/src/bake/plan.rs:594-668`, `crates/engine/src/pack/plan.rs:428-441`, `:186-208`, `:170-182`)
   - **What this is:** `plan_bakes` does three full passes over `jobs` per set (`idx`, `own_normal`, `roughness_members`); `plan_packing` does `jobs.iter().any(..)` per raw member, per pre-packed probe and per `retag`. `rnm/plan.rs:75-78` solves the same problem by building `by_set: BTreeMap<set_key, Vec<usize>>` once.
   - **Problem:** 6000 textures in 2000 sets → about 36 million set-key string comparisons in `plan_bakes` alone, every plan; the GUI replans after every edit. Roughly 0.1–0.5 s per plan today, growing quadratically.
   - **Fix:** build the `by_set` index once at the top of `plan_bakes` (copy the three lines from `plan_rnm`) and have `own_normal`/`roughness_members` take the set's index slice; in `plan_packing` build a `BTreeSet<&Path>` of waiting raw inputs once.
   - **If we skip it:** planning stays O(sets × jobs); noticeable at 5k+ files, invisible below 1k.
   - **Group:** Nice to have. **Kind:** scale.
   - **Confidence:** high on the code shape (read all four scans); medium on the time.

6. **An NVTT encode has no time bound, and one hung process stalls every NVTT file** (`crates/engine/src/nvtt.rs:283-285`, `:398-415`)
   - **What this is:** the probe kills a program that does not answer within 5 s, but the per-level encode polls `try_wait` forever and only stops on the cancel token. All NVTT levels in the process share one slot (`NVTT_SLOT`).
   - **Problem:** an `nvcompress.exe` that hangs (driver, AV scan holding the exe, a bad input it loops on) blocks its worker, and because the slot is held, every other file on `backend = nvtt` sleeps in the `try_lock` loop. An unattended CLI batch never ends and never reports the file; the GUI shows no progress until the user cancels.
   - **Fix:** a generous ceiling per level in the wait loop, scaled by size so a 16k Production BC7 is not cut off, e.g. `let limit = Duration::from_secs(60 + 30 * (w as u64 * h as u64 / (1 << 20)));` and on expiry `child.kill(); return Err(failed(format!("nvcompress did not finish within {limit:?}")))`. Report it as `ENC-NVTT-FAILED` like any other failure.
   - **If we skip it:** one stuck process turns a batch into a hang instead of one failed file.
   - **Group:** Nice to have. **Kind:** risk.
   - **Confidence:** high on behaviour (read the loop and the slot); the hang itself is hypothetical.

7. **A stale NVTT temp folder is silently reused** (`crates/engine/src/nvtt.rs:264-270`, `:365-368`, `:423`)
   - **What this is:** `TempDir::new` names the folder `sdc-nvtt-<pid>-<n>` and creates it with `create_dir_all`, which succeeds when the folder already exists. A crash leaves such folders behind (documented); pids are recycled on Windows.
   - **Problem:** a later run with a recycled pid lands in the old folder; `l0.out.dds` from the crashed run is still there. If `nvcompress` ever exits 0 without writing (or writes to a different name), `encode_level_nvtt` reads the stale file, the size check passes (same level size), and texels from another texture are written into this DDS with a passing validation. Low likelihood, silent wrong output when it happens.
   - **Fix:** `std::fs::create_dir(&dir)` instead of `create_dir_all`, looping to the next `n` on `AlreadyExists` (as `write_atomic` does for its temp name), and `let _ = std::fs::remove_file(&out);` before `cmd.spawn()`.
   - **If we skip it:** nothing until a crash plus a pid recycle plus an odd nvcompress exit coincide.
   - **Group:** Nice to have. **Kind:** risk.
   - **Confidence:** medium (traced; the trigger needs nvcompress to misbehave).

8. **Every input's header is parsed five times across plan and run** (`crates/engine/src/api.rs:395-456`, `crates/engine/src/bake/plan.rs:480-484`, `crates/engine/src/pack/plan.rs:466-469`, `:573`)
   - **What this is:** `estimated_bytes` and `job_bytes` each call `input_dims` (two header reads per job in `run`), plan admission calls `estimated_bytes` again, `post_resize_dims` reads the header per job, and `plan_packing` runs `check_sizes` twice per recipe. For PSD/KRA inputs each read opens the file and walks the header/ZIP directory.
   - **Problem:** 5000 files × 5 header parses = 25k extra opens per plan+run; for PSD it also seeks through the resources section each time. About 0.1–0.5 s on an SSD, more on a cold HDD; harmless but pure waste, and `job_bytes` computing `input_dims` for a path it just resolved inside `estimated_bytes` is the same rule done twice in one function.
   - **Fix:** have `estimated_bytes` return `(bytes, dims)` (or make `job_bytes` call `input_dims` once and pass it), and reuse `Analysis.width/height` (already in the plan) in `post_resize_dims` instead of re-reading the header.
   - **If we skip it:** a fraction of a second per plan.
   - **Group:** Nice to have. **Kind:** speed.
   - **Confidence:** high (counted the call sites).

9. **`write_atomic` fsyncs every output and every sidecar** (`crates/engine/src/dds.rs:351-353`)
   - **What this is:** each write does `file.sync_all()` before the rename, twice per file with `--sidecar`.
   - **Problem:** on an HDD a flush is 10–30 ms: 3000 outputs with sidecars → 1–3 minutes of pure flushing per batch. The durability it buys is already provided elsewhere: after a power loss a zero-length or torn output has a different `output_stamp` than its manifest record, so `if-changed` re-encodes it on the next run (Note Y1), and the manifest append itself is not fsynced anyway.
   - **Fix:** delete the `sync_all` line (and the doc sentence), or keep it only for the manifest if durability of the record matters more than the output.
   - **If we skip it:** HDD users wait a minute or two longer per large batch.
   - **Group:** Nice to have. **Kind:** speed.
   - **Confidence:** high on the code; the recovery argument traced through `is_up_to_date`'s stamp check.

10. **No test pins the in-place and stacked-coverage behaviours above** (`crates/engine/tests/discover_group.rs`, `crates/engine/tests/complex_grass.rs`)
    - **What this is:** `skips_sdc_and_output_root_and_unknown_extensions` covers an output root *inside* the input; nothing covers output root *equal to* the input folder. `complex_grass.rs` asserts `passed` but never a grass source whose bottom-half alpha crosses the threshold, so finding 3 is invisible to CI.
    - **Problem:** whichever way finding 2 is decided (allow or refuse), a later change to `is_under` can flip it back to "0 inputs, exit 0" unnoticed; finding 3's fix can regress silently.
    - **Fix:** one test each: `discover(&[dir], &dir, false)` yields the images (or `plan` returns the Usage error); a 16×32 grass source with bottom alpha alternating 100/160 produces no `VAL-COVERAGE` warning.
    - **If we skip it:** the two behaviours stay unpinned.
    - **Group:** Nice to have. **Kind:** missing test.
    - **Confidence:** high (grepped both test files).

Smaller items left out (7): `load::decode` holds three pixel buffers (decoded + `to_rgba32f` + collected `Vec`) where two would do; `make_thumbnail` tone-maps every float source, so an EXR normal or data map gets an sRGB-encoded thumbnail; `AnalysisCache::thumbnail_of` ignores the stamp, so a replaced file shows its old thumbnail until the next plan; the same folder given twice as an input reports every file as `IN-DUPLICATE` with itself; `current_sources` and `SourceRef::of_*` hash pack/bake sources twice for a stale packed job; `dds::level_size` (the free function) is used only by tests; `read_dds` copies the whole file (`bytes[off..].to_vec()`) so a DDS input is briefly held twice.

Not read: `crates/engine/src/mips.rs` tests from line 620 on, `crates/engine/src/coverage.rs` tests from line 420 on, `api.rs:1960-2060` (`explicit_shaders`, `map_members`) and `check_up_front` (`api.rs:2107-2250`), `pack/plan.rs` only skimmed for `chosen_recipe`/`set_is_advanced_skin`, test files other than the ones named (`golden.rs`, `pack_*.rs`, `rnm_*.rs`, `bake_*.rs`, `preview.rs`, `validate.rs` were grepped, not read).

Lean: little to remove here. Roughly 30 lines could go (`sync_all`, the free `dds::level_size`, the duplicated `input_dims` call, the second `check_sizes` per recipe); no dependency in this path is replaceable by std (`flate2` is the ZIP inflater, `walkdir` the recursive walk, `image` the decoders).

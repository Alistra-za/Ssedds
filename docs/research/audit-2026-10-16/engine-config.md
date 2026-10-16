# Engine audit: configuration, policy and reporting

Scope: `crates/engine/src/{config,presets,overrides,policy,migrate,schemas,sets,shader,suggest,codes,events,manifest,reports}.rs`, `crates/engine/src/validate/`, `crates/engine/tests`, `presets/*.toml`, `schema/*.json`. Repo at b5ad038.

What held up well (checked, no finding): preset inheritance cycles and self-cycles are caught in `chain()`; `add_dir` re-checks orphans after a skip; `merge_json` null handling; the manifest append path (one `write_all` under a process-wide lock, torn-line newline repair, lenient `load`) and its tests; the `if-changed` comparison in `api.rs::is_up_to_date` (I traced every input that changes output bytes — source bytes, every pack/bake/RNM source with role, profile hash, encoders, rgbcx patch, NVTT version, BC1 model, output stamp, validation pass — and found no uncovered one); the schema files are regenerated and compared byte-for-byte in `tests/schemas.rs`; every config option in `FIELD_PATHS` is read somewhere in the engine; 91 of 93 codes are emitted (see finding 7 for the two that are not).

1. **An invalid glob in a rule or `--set file:` silently matches nothing** (`crates/engine/src/policy.rs:148-175`, `crates/engine/src/overrides.rs:316-345`, `crates/engine/src/presets.rs:668-684`, `crates/engine/src/api.rs:2107-2230`)
   - **What this is:** Rules (`match.glob`) and file-scoped overrides (`--set file:<glob>.field=value`) pick files with a glob pattern. Globs are compiled lazily in `glob_match_input`; a pattern that does not compile is cached as `None` and treated as "no match".
   - **Problem:** `--set 'file:**/armor/[.format=bc1'` (an unclosed `[`), or a user preset with `match = { glob = "**/{armor,weapons/**" }`, passes `parse_set`, `check_preset` (`sdc presets validate`, the user folder loader, the GUI preset editor) and `check_up_front` — none of them compiles the glob (`globset` is used only in `policy.rs`). The batch runs, exit 0, and the override or rule applies to zero files with no message. The user believes armour is BC1; it is BC7.
   - **Fix:** validate once where patterns enter: in `parse_set` for `Scope::File` and in `check_preset`/`check_up_front` for each `r.matcher.glob`, `globset::Glob::new(g).map_err(|e| ... format!("glob '{g}': {e}"))?` → usage error / `PresetError::Invalid`. Five lines; the lazy cache stays.
   - **If we skip it:** a typo in a glob is undetectable except by inspecting every job's provenance.
   - **Group:** Should fix. **Kind:** bug.
   - **Confidence:** high (grepped the tree for `globset`/`GlobBuilder`: one use; read `parse_set`, `check_preset`, `check_up_front`; no test covers an invalid glob).

2. **The published config schema rejects `[files]` entries the loader accepts and the GUI writes** (`crates/engine/src/schemas.rs:216-240`, `schema/config.schema.json:251-260,943-960`, `crates/engine/src/presets.rs:180-230`)
   - **What this is:** `schema/config.schema.json` is the editor-facing JSON Schema for `sdc.toml`. Its `FileEntry` definition is a copy of `PartialProfile` plus `allow`, with `additionalProperties: false`. `ConfigFile::file_entries` additionally accepts the two plan-time selectors `reencode` and `rnm.target` (both in `FIELD_PATHS`), and `docs/gui.md:212` says the GUI writes `rnm.target` onto a layer file's `[files]` entry.
   - **Problem:** an `sdc.toml` containing `[files."armor/x_n.dds"] reencode = true` or `[files."armor/x_detail.png"] rnm = { target = "cnr" }` loads and works, but validates as invalid against the published schema (`FileEntry` has no `reencode`; `PartialRnmSpec` has `additionalProperties: false` and no `target` — `grep -c reencode schema/*.json` is 0 for every file). The schema drift test passes because the generator itself is wrong, not stale.
   - **Fix:** in `files_table_schema`, add `props.insert("reencode", json!({"type":"boolean"}))`, clone `PartialRnmSpec` into a `FileEntryRnm` definition with an extra `target: {"enum":["n","cnr","both"]}` property, point `FileEntry.properties.rnm.$ref` at it, then regenerate `schema/`. Add one test in `tests/schemas.rs`: for every path in `overrides::FIELD_PATHS` except `subtype`, walk the `FileEntry` definition (following `$ref`s) and assert the leaf property exists — it fails today on `reencode` and `rnm.target`.
   - **If we skip it:** anyone who wires the schema into an editor sees red squiggles on files the GUI produced; the schema cannot be trusted as documentation of `[files]`.
   - **Group:** Should fix. **Kind:** bug (+ missing test).
   - **Confidence:** high (read `files_table_schema`, `file_entries`, the committed schema; grepped for `reencode`).

3. **A non-finite number in `--set` or `[files]` is silently dropped** (`crates/engine/src/overrides.rs:277-283`, `crates/engine/src/presets.rs:338-356`, `crates/engine/src/api.rs:2189-2209`)
   - **What this is:** `parse_value` parses the right-hand side as TOML and converts it to JSON with `serde_json::to_value`. TOML accepts `inf`, `-inf`, `nan` as floats; `serde_json`'s `From<f64>` turns a non-finite float into `Value::Null` (serde_json 1.0.151 `value/from.rs:47`). `merge_json` ignores a null patch and `leaf_paths` skips null leaves, so no value and no provenance are recorded.
   - **Problem:** `--set validation.min_mip_psnr_db=-inf` (the obvious way to switch the PSNR warning off) passes `check_up_front` (the patch is null, so `finish_profile` sees the unchanged profile), the batch runs, and the threshold stays 30 dB with provenance `builtin`. The same happens for `[files."x.png"] validation = { min_mip_psnr_db = -inf }`, where TOML→`serde_json::Value` goes through the same conversion.
   - **Fix:** in `parse_value`, after conversion: `if v.is_null() { return Err(...) }` (make `parse_value` return `Result` and let `parse_set` report `--set '…': not a finite number`); in `file_entries`, after `leaf_paths`, also walk the entry for null leaves and reject them. Six lines.
   - **If we skip it:** a setting the user typed is ignored without a word.
   - **Group:** Should fix. **Kind:** bug.
   - **Confidence:** high (read serde_json's `From<f64>` in the registry; traced `parse_value` → `merge_json`/`leaf_paths`).

4. **`overrides.subtype` in a rule passes validation and does nothing** (`crates/engine/src/presets.rs:668-684`, `crates/engine/src/policy.rs:470-500,780`, `crates/engine/src/presets.rs:916-930`)
   - **What this is:** A rule decides a sub-type with `rules[].subtype`; its `overrides` table is a partial profile. `FIELD_PATHS` includes `"subtype"` (for `--set subtype=`), so `check_preset` accepts `overrides = { subtype = "pbr_normal" }`.
   - **Problem:** `[[rules]] match = { suffix = "n" } overrides = { subtype = "pbr_normal" }` validates. At resolve time `apply` merges `subtype` into the document and records provenance, then `finish_profile` overwrites `subtype`/`category`/`target` with the sub-type already decided, and `resolve_decided` overwrites the provenance. The rule has no effect; the file is still a suggestion-decided `normal`. Nothing reports it.
   - **Fix:** in `check_preset` (and the same loop in `check_up_front`), reject a rule override path of `subtype`, `target` or `category` with "set the sub-type with `subtype = …` on the rule, not in `overrides`". Three lines.
   - **If we skip it:** a plausible preset mistake is silent.
   - **Group:** Nice to have. **Kind:** bug.
   - **Confidence:** high (traced `apply` → `finish_profile` → `resolve_decided`).

5. **The manifest only grows, and every `run` parses all of it** (`crates/engine/src/manifest.rs:300-370`, `crates/engine/src/api.rs:1730-1733`)
   - **What this is:** `_sdc/manifest.jsonl` is append-only; `Manifest::load` reads every line into a `serde_json::Value` and keeps the last record per output, including its full `raw` value. There is no compaction anywhere (grepped `cli`, `gui`, `engine` for compact/truncate/rewrite).
   - **Problem:** one record is roughly 5 KB (profile ≈1.2 KB, ~50 provenance entries, per-mip alpha stats, encoder hashes). A 2000-texture mod re-encoded 30 times over its development appends ~300 MB; every `run`, whatever `--on-existing`, parses the whole file (`manifest.load()` is unconditional at `api.rs:1731`) and holds ~2000 `Value`s (tens of MB) for the batch. It is also an unbounded file on the user's disk that nothing ever trims.
   - **Fix:** in `Manifest::load`, count lines read; if `lines > 2 * map.len()` (or the file is above, say, 32 MB), rewrite the file with `dds::write_atomic` from the retained `raw` values (one `to_vec` + `\n` per entry). About 12 lines, no new types.
   - **If we skip it:** slower starts and a manifest that eventually dwarfs the textures it describes.
   - **Group:** Nice to have. **Kind:** scale.
   - **Confidence:** medium (record size is an estimate from the struct; growth and unconditional load are read from the code).

6. **`check_preset` and `check_up_front` are two copies of the same validation** (`crates/engine/src/presets.rs:668-712`, `crates/engine/src/api.rs:2107-2230`)
   - **What this is:** both build the layered document per sub-type, deserialise it, then re-deserialise it once per rule (and, in `api.rs`, per `--set` and per `[files]` entry). `check_up_front` repeats it a third time for alias chains.
   - **Problem:** they already disagree: `check_preset` rejects an unknown field path in `rules[].overrides` up front (`unknown field 'x'`), `check_up_front` only finds it through `finish_profile`'s error for the batch chain; and both miss the glob check (finding 1) and the `subtype` override (finding 4) — each fix has to be made twice.
   - **Fix:** one `pub fn check_chain(chain, configs, target, extra_patches) -> Result<(), PresetError>` in `presets.rs`; `check_up_front` calls it for the batch chain and each alias chain and keeps only the `--set`/`[files]` loop. Deletes ~60 lines.
   - **If we skip it:** validation rules drift between `sdc presets validate` and the batch.
   - **Group:** Nice to have. **Kind:** lean.
   - **Confidence:** high (read both).

7. **`ENC-NVTT-FAILED` is a code nothing emits as a code** (`crates/engine/src/codes.rs:107`, `crates/engine/src/pipeline.rs:331-347`)
   - **What this is:** `Code::EncNvttFailed` (Error) is in the table, so `--allow ENC-NVTT-FAILED` and `[files] allow` accept it. The only emission is the string `"ENC-NVTT-FAILED: …"` inside `FileError::Failed`'s message (`FileFailed.error`), which carries no `Code`.
   - **Problem:** the event stream never contains `"code":"ENC-NVTT-FAILED"`, so a GUI or script that filters warnings by code never sees it, while `--allow ENC-NVTT-FAILED` is accepted and meaningless (an Error cannot be allowed anyway). `Code::InFloatLdr` is the other never-emitted entry, kept on purpose per its comment.
   - **Fix:** either emit `Warning::new(Code::EncNvttFailed, m)` on the job before failing it, or drop the variant and keep the string. One line either way.
   - **If we skip it:** a dead table entry and one more thing `Code::ALL.len() == 93` pins for no reason.
   - **Group:** Nice to have. **Kind:** lean.
   - **Confidence:** high (grepped the whole tree for `EncNvttFailed` and `ENC-NVTT-FAILED`).

8. **The alpha-threshold range and CS's 251 gate are hard-coded in six places** (`crates/engine/src/policy.rs:1577`, `crates/engine/src/shader.rs:399`, `crates/engine/src/validate/mod.rs:27`, `crates/gui/src/model/used_by.rs:130-132`, `crates/gui/src/ui/presets.rs:699`, `crates/gui/src/ui/settings_panel.rs:357,375`, `crates/gui/src/model/cm_json.rs:549-551`)
   - **What this is:** `check_profile` rejects `alpha.threshold` outside `1..=254`; `mesh_alpha_policy` clamps to the same; three GUI widgets repeat `1..=254`. `CS_COMPLEX_GATE = 251.0` is private to `validate/mod.rs`; the GUI's PGPatcher advice matches `251..=254` and prints "alpha ≥ 251" by hand.
   - **Problem:** changing the engine range (say to allow 255 for `ge`) or the gate leaves the GUI clamping/explaining the old value; today the GUI's `settings_panel.rs:375` falls back to `128` while `sdc-base` uses `127`.
   - **Fix:** `pub const ALPHA_THRESHOLD: RangeInclusive<u8> = 1..=254;` in `config.rs`, `pub const CS_COMPLEX_GATE: u8 = 251;` in `validate`, and use them at every site. Net −4 lines.
   - **If we skip it:** two numbers that must change together, in two crates.
   - **Group:** Nice to have. **Kind:** lean.
   - **Confidence:** high (grepped `gui` for `254`, `251`, `1..=254`).

9. **`cs-compact.toml` and `vanilla-compact.toml` are the same file twice** (`presets/cs-compact.toml`, `presets/vanilla-compact.toml`)
   - **What this is:** 74 lines each; only `id`, `base`, `target` and `description` differ. The two rules and the 26 `[subtypes.*] resize = "half"` tables are byte-identical.
   - **Problem:** a change to one (a new sub-type to halve, an edit to the BC1 rule's sub-type list) has to be mirrored by hand. The copy is already blind: `vanilla-compact` carries `resize = "half"` tables for `pbr_albedo`, `pbr_normal`, `coat_normal`, `rmaos`, `skin_rfaos`, `flow`, `skin_albedo`, `skin_sk` and `skin_wet`, every one of which `policy::availability` says is unavailable under vanilla (harmless today, misleading to the next editor).
   - **Fix:** a hidden `sdc-compact` preset (no `target`, Note T1 already allows that) holding the shared body; `cs-compact` and `vanilla-compact` become six-line files with `base = "sdc-compact"`. `layers_of` already merges chains of any length. Net −60 lines of TOML.
   - **If we skip it:** two copies that drift.
   - **Group:** Nice to have. **Kind:** lean.
   - **Confidence:** high (diffed the files).

Smaller ones left out (6): `PresetRef.sha256` hashes only the leaf preset file, so editing a user base preset leaves the child's recorded hash unchanged (the stored profile still tells the truth); `presets export` copies `hidden = true` into the new id; `[subtypes.rfaos]` and `[subtypes.skin_rfaos]` in one file silently last-wins; `--set file:*.header=dx10` is parsed as glob `*` plus field `header` (correct, but a glob ending in a field name cannot be expressed); `rule_sources` labels config rules `user`/`project` while `check_up_front` labels them `config`; `Code::InFloatLdr` kept retired by decision.

Not read: `shader.rs` beyond `member_defaults`, `member_subtype` and `mesh_alpha_policy`; `suggest.rs` beyond its structure; `api.rs` beyond `setup`, `check_up_front`, `is_up_to_date` and the manifest load site; `pipeline.rs` beyond the record construction; most test files (read `schemas.rs`, the manifest tests in `events_manifest.rs`, and grepped `presets.rs`, `user_presets.rs`, `project_files.rs`, `overrides.rs` for coverage of the cases above); `schema/*.json` other than `config.schema.json`.

Lean: ~130 lines could go (the duplicated validation in `api.rs`, the duplicated compact preset body, the constants, one dead code variant). No dependency in this scope is removable (`globset`, `humantime`, `sha2`/`hex`, `schemars`, `toml` are each used).

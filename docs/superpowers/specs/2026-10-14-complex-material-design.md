# Complex material card: design spec (2026-10-14)
Status: APPROVED by owner 2026-10-14 ("approved, update the goldens"): incl. engine policy.rs/suggest.rs/preset edits, schema regen, and updating the two golden tests in §8; off-limits sets_doc.rs + pgp/validate.rs approved.

This is a spec only. No code, branch or commit was made.

**Inputs.** The main input is `01-research.md` in this folder. Section names below refer to it: §1–§6, "Owner answers", "Inputs from the PGPatcher 2.2 log research", "NIF check", "CORRECTION" and "Owner ruling". Live evidence is in `02-test-jsons.md`, `03-nif-check.md` (Rerun 2) and `..\pgpatcher-2.2-logs.md`.

**Code read.**
- sdc `main` @ `c6c9e9b`, through `git show main:`. Abbreviated **[main]**.
- `nif/main` HEAD @ `fa71346`, the committed tree, through `git show HEAD:`. Abbreviated **[nif]**. Its working tree has a merge of `main` staged and was not relied on.
- Line numbers are from the tree named in brackets.

## 1. Goal

The set editor gets a per-set card, "PGPatcher complex material". When the user ticks the card's export box, sdc writes `<texture>_m.json` next to every complex `_m` of the set when it exports. The file holds only the keys the user set, plus `$schema`. PGPatcher 2.2 reads the file and applies those keys to every shape it patches for complex material with that `_m`.

## 2. Owner decisions (binding)

| # | Decision | Source |
|---|---|---|
| D1 | The card shows only on complex-material sets | Owner answers ("other proposed defaults accepted"); §6 |
| D2 | Both targets, Vanilla and CS. Not tied to the CS target ("complex only works with ENB anyways") | Owner answers |
| D3 | A per-set "Export complex material JSON" checkbox, off by default | Owner request (header); Owner answers |
| D4 | One block per set, applied to every complex `_m` member | Owner answers (open question 3) |
| D5 | `specular_color` is three number fields, and values above 1 are allowed | Owner answers (open question 5) |
| D6 | Write `$schema` | Owner answers (open question 6) |
| D7 | Unticking the box leaves the old JSON on disk with a note. sdc never deletes it | Owner answers (open question 7) |
| D8 | No glossiness range and no preview | Owner answers (open questions 8, 9) |
| D9 | The file is `<texture>_m.json` in the `_m`'s folder: the `.dds` extension becomes `.json` | Owner request; §2 "File name" |
| D10 | Fields start **unset**. Each field shows PGPatcher's default only as a hint. sdc writes only the keys the user changes. No prefill from mesh values | Owner ruling (supersedes CORRECTION) |
| D11 | The card says that PGPatcher resets specular colour, specular strength and env map scale to 1 unless the JSON overrides them | Owner ruling; NIF check; `03` Rerun 2 finding 2 |
| D12 | A warning on the card when the `_m`'s smallest-mip alpha is 251–254 (PGPatcher reads complex material, CS reads an env mask) | Inputs from the 2.2 log research; `pgpatcher-2.2-logs.md` §c and rec. 4 |
| D13 | Bring the bundled `pbr.schema.json` up to 2.2.0 (the `alphaprop_*` keys) | Inputs from the 2.2 log research; `pgpatcher-2.2-logs.md` rec. 3 |
| D14 | A hand-edit guard, as in the CS PBR export | Owner request recap; §5 "Export hooks" |
| D15 | The owner approved edits to the off-limits files `sets_doc.rs` and `pgp/validate.rs` | Owner answers |
| D16 | **Vanilla keeps a complex `_m`.** An `_m` with varying alpha resolves to `complex_material` under Vanilla too, written in the same format as on CS (BC7 with alpha), so the JSON works there. A plain env-mask `_m` stays BC4 with no alpha. This is one plan with the card. Edits to `policy.rs`, `suggest.rs` and `presets/` are approved, and `schema/` may be regenerated. Golden tests stay off-limits: any golden change goes to the owner (Q1) | Owner answer 2026-10-14 #1 (answers the old Q1) |
| D17 | The 251–254 alpha warning shows only after an export | Owner answer 2026-10-14 #2 (answers the old Q2) |

**Evidence the spec relies on (`03` Rerun 2).**
- PGPatcher 2.2 applies every key present in the JSON.
- An omitted `glossiness` keeps the mesh's value.
- An omitted colour, strength or env scale goes to 1 (or 1,1,1), with or without a JSON.
- Defaults for `specular_enabled` and `dynamic_cubemap` were not told apart. They matched the meshes tested.

## 3. Data model: `sdc-project.toml`

A new sub-table in `[[sets]]`. Keys use the schema names exactly.

```toml
[[sets]]
id = "cuirass"
[sets.cm]
export = true
specular_color = [1.0, 0.85, 0.6]
glossiness = 80.0
```

- **Unset and set.** A key that is absent is unset: sdc writes nothing for it and PGPatcher's default applies. A key that is present is set and is written. There is no stored "default" value.
- **`export`.** Absent or `false` means nothing is written (D3). Unticking writes `export = false` and keeps the values, so re-ticking restores them.
- **Wrong types.** A known key with the wrong type stays in the file, is listed in `CmBlock::bad`, gets an amber warning on the card, and is not exported. This copies the CS `CsBlock.bad` pattern (`[nif] model/cs_pbr.rs:199-208`). A `specular_color` that is not exactly three numbers counts as a wrong type, because PGPatcher 1.2.0 crashes on non-numbers (§2).
- **Unknown keys** in `[sets.cm]` stay in the file and are ignored. They are never exported, because the schema has `additionalProperties: false`.
- **`RESERVED`** gains `"cm"`. It has 10 entries on [main] and 11 on [nif]. Without this, the block's keys would flatten into the TruePBR settings (`[nif] sets_doc.rs:21-33, 103-120`). They would then leak into the TruePBR JSON and make `shows_json_panel` true (`[main] model/sets.rs:255`).
- **`SetEntry.cm: Option<CmBlock>`.** `CmBlock { export: bool, values: Map<String, Json>, bad: Vec<String> }`. It is parsed in `entry_of`, as `cs_txst` is.
- **Setters**, modelled on `set_cs_value` (`[nif] sets_doc.rs:786-806`):
  - `set_cm_value(id, key, Option<Json>)`: `None` removes the key.
  - `set_cm_export(id, bool)`.
  - Both create `[sets.cm]` when it is missing, and both keep the file's formatting.
  - `set_value`'s TXST redirect (`:760-784`) only touches `cs_txst`, so it cannot reach `cm`.
- **Hash record.** `[project] cm_json_sha256 = { "<path under the export root>" = "<sha256>" }`, with `cm_json_shas()` and `set_cm_json_shas()`. These copy `cs_pbr_shas` / `set_cs_pbr_shas` (`[nif] :321-341`).
  - `restore_keeping_json_sha` (`[nif] :241-258`) must keep `cm_json_sha256` as well. Without that, an undo wipes the record and the hand-edit guard misfires.

## 4. When the card shows: `model/cm_json.rs`

**`cm_members(m, v) -> Vec<item key>`**, the set's complex `_m` members. A member counts when either of these holds:
- (a) its resolved sub-type is `complex_material`. This covers loose files and packed cpm `_m`.
- (b) its resolved sub-type is `envmask` and the set's shader (the choice, else the engine pick) is `Shader::ComplexMaterial`.

**The card shows when** `cm_members` is not empty, **or** the set already has a `[sets.cm]` block. The second condition keeps stored values visible after a shader change, which is the reason `shows_json_panel` checks `!settings.is_empty()`. The predicate is new code in `cm_json.rs`, so `model/sets.rs` is not edited.

**Targets.** Once the §8 engine change is in, (a) covers both targets: a varying-alpha `_m` resolves to `complex_material` under Vanilla as well, and the shader auto-pick then gives Complex Material (`[main] shader.rs:564`).

(b) remains for one case: a set whose shader is Complex Material but whose `_m` still resolves to `envmask`. That happens when the source alpha is constant or missing, since an `_m` is never re-typed (S-shader-5), or when the user forces `envmask`. Such members get the "written as an env mask" warning (§5).

## 5. Card UI: `ui/cm_card.rs::section(ui, model, v)`

**Placement and drawing**
- A collapsing section titled "PGPatcher complex material". It is drawn on both targets.
- In 2D it goes in the right column, after the TruePBR panel and the skin card (`[main] set_editor.rs:176-185`). In 3D it is a collapsing header beside "TruePBR settings" (`:136-158`). On [nif] it sits next to `cs_pbr::section`.
- It follows the `ui/cs_pbr.rs` pattern: acts are collected while drawing and applied afterwards. Each change is one `sets_doc` edit, which means one undo step.
- The card is greyed out with the read-only reason when `sdc-project.toml` is read-only.

**Rows, top to bottom**
1. **`[ ] Export complex material JSON`**. Hover text: "Writes <name>.json next to each complex _m when you export. PGPatcher applies it to every shape it patches as complex material with that _m."
2. **Targets.** One weak line per member, giving the output path with `.json`, for example `textures\armor\steel\cuirass_m.json`.
3. **Note**, always shown, in weak text: "PGPatcher sets specular colour, specular strength and environment map scale to 1 on every complex-material shape unless this file sets them. Glossiness keeps the mesh's value. Only the keys you tick are written." (D11)
4. **Six key rows**, in schema order. Each row is `[ ] key` followed by the value widget once the key is ticked. Unticking removes the key: that is the per-key clear. While a key is unticked, its row shows the hint below in weak text.

   | Key | Widget when set | Unset hint | Value when first ticked |
   |---|---|---|---|
   | `specular_enabled` | checkbox "on" | "PGPatcher: on if the _m has gloss (G), else the mesh's" | `true` |
   | `specular_color` | three drags R, G, B; ≥ 0, no upper bound (D5) | "PGPatcher: 1, 1, 1" | `[1.0, 1.0, 1.0]` |
   | `specular_strength` | drag, ≥ 0 | "PGPatcher: 1" | `1.0` |
   | `glossiness` | drag, ≥ 0, no range hint (D8) | "PGPatcher: keeps the mesh's value" | `80.0` (the schema and wiki example; not read from a mesh, D10) |
   | `environment_map_scale` | drag, ≥ 0 | "PGPatcher: 1" | `1.0` |
   | `dynamic_cubemap` | checkbox "on" | "PGPatcher: on (dynamic cubemap)" | `true` |

   - Hover text on `dynamic_cubemap`: "Off keeps the shape's own cubemap. Under CS an active complex material still uses the dynamic path (§4)."
   - Stored values are edited even while export is off, and they are kept.

5. **Warnings**, in amber, one line per case:
   - **Bad types.** "`<key>`: wrong type in sdc-project.toml; not exported".
   - **Smallest-mip alpha 251–254 (D12).** Read from the latest export row of the `_m`: the `VAL-M-COMPLEX-MARGIN` data `smallest_alpha` (`[main] engine/src/validate/mod.rs:387-409`). Text: "<name>: smallest-mip alpha N. PGPatcher treats it as complex material and applies <name>.json, but Community Shaders renders it as a plain env mask (alpha ≥ 251)."
     - This is **post-export only**. Plan-time item warnings are the job's policy warnings (`[main] model/items.rs:220`), and VAL-* codes appear only in export rows. Before an export, nothing is shown. The owner accepted this (D17).
     - 252 is measured (s8 `m_252_m`). 253 and 254 are inferred from the "not pure white" rule.
   - **Alpha 255 at the smallest mip.** "PGPatcher likely reads <name> as an env mask; the JSON will not be applied." PGPatcher actually classifies by the share of non-255 alpha (`VAL-PGPATCHER-50`), hence "likely".
   - **Written as an env mask.** This applies only to a member that counts through (b), meaning it resolved to `envmask` (forced by the user, or constant or missing alpha), or to a `complex_material` with an explicit `alpha.policy = remove`. Text: "<name> is written as an env mask without alpha, so PGPatcher ignores the JSON. Set its sub-type to complex material." `envmask` is BC4 with `alpha remove` on both targets (`[main] presets/sdc-base.toml:55-58`). Since D16, a complex `_m` on Vanilla no longer triggers this line.
   - **Export refusals** from `prepare`, the same text as the export notes in §6.

**Relation to Note AH2.** "Enable parallax" off writes alpha 0, which is always complex for both PGPatcher and CS. The 251–254 case arises only with parallax on and a near-white height. The card does not touch `alpha.parallax`.

## 6. Export: `model/cm_json.rs`

**Hooks.** These mirror the CS PBR export (`[nif] model/mod.rs:767-769, 1104-1105`; `model/cs_pbr_export.rs`).
- `start_export`: `self.pending_cm = cm_json::prepare(self)`.
- `ExportDone`: `self.finish_cm_json(self.pending_cm.take())`.
- `prepare` is **not** gated on the target (D2).

**`prepare(m) -> Option<PendingCm>`.** For each set whose `cm.export` is true:
1. If `cm_members` is empty, add the note "set X: no complex _m; complex material JSON not written".
2. Otherwise, for each member, take the path `item.job.output.with_extension("json")` (§5 "Where the sidecar goes").
   - This follows the output name and suffix as resolved, so `_em`, `_envmask`, `output_name` renames and packed cpm `_m` are all covered.
   - The record key is that path relative to `m.export_root()` (`[nif] model/mod.rs:516-521`), `/`-separated. An output outside the root is refused with a note.
3. Build the text:
   - `"$schema": "https://raw.githubusercontent.com/hakasapl/PGPatcher/main/schema/complex_material.schema.json"` (the schema's `$id`).
   - Then every set, well-typed key. Every number is written as an f64, because PGPatcher checks only `is_number()`, and a single form keeps the golden test stable. `specular_color` is always three numbers.
   - The output is serde_json pretty with 2-space indent. Keys come out sorted, so `$schema` is first. It ends with `\n`.
   - With zero keys set, the file is still written as `$schema` only. That restates PGPatcher's defaults, so the box means "this file exists".
4. Validate the text with `pgp::validate::validate_cm` (§7). A failure refuses that file with the validator's errors as a note. This is a safety net that should never fire.
5. Two members, in one set or in two, that map to the same path, compared case-insensitively, are both refused with a note. This copies `cs_pbr_export::refusals`.

**`finish(p, shas, notes)`.** This copies `cs_pbr_export::finish` (`[nif] :198-266`). It runs only after a clean export: no error and nothing cancelled.
- If the `_m` `.dds` is missing on disk, skip it with the note "<path>: <name>.dds was not exported; JSON not written".
- If `pbr_export::hand_edited(path, text, last)` is true, keep the file with the note "<path> kept as edited outside sdc (delete it to let sdc write it again)". This also protects a mod's own `_m.json` when the export root is a mod folder.
- Otherwise, `pbr_export::write_atomic`, then record the sha.
- **Untick or orphan (D7).** A recorded path that this run does not write, and that is still on disk, keeps its record and gets the note "<path>: complex material export is off for its set (or the _m left the set); the file was left in place. Delete it to drop the overrides." sdc never deletes it.
- The new record is saved through `set_cm_json_shas` and `save(false)`, with the read-only and save-error handling of `finish_cs_pbr`.

**No clash with `.dds.json`.** The engine's `--sidecar` record is `<name>.dds.json` (`engine/src/api.rs`, §5). The complex material JSON is `<name>.json`. `write_atomic`'s temporary file is `<name>.json.sdc-tmp`. None of these collide.

## 7. Schemas and validator

**`pbr.schema.json` → 2.2.0 (D13): already done on [main]. Verify only.**
- [main]'s vendored file is byte-identical, CR ignored, to upstream `origin/main` `0176567`. The sha256 of both is `4bdc7920…`.
- Upstream's last change to the file is `4d89733` (2026-10-11, "nialpha properties"). That is earlier than `8d4b05e` (2026-10-13), which the research shows is inside `2.2.0`. None of the later commits touch it.
- So the [main] copy equals 2.2.0's. This is settled by ancestry; the `2.2.0` tag itself was not fetched.
- DECISIONS AG27 / `e75d159` added the four `alphaprop_*` keys to the schema and to `pgp/schema.rs` `KEYS`.
- [nif] HEAD has none of them yet. They arrive with the `main` merge that is in progress.
- Work item: after that merge, confirm that the four keys are present and the KEYWORDS test is green. No new vendoring.

**Vendor `crates/gui/assets/complex_material.schema.json`.**
- Take upstream `origin/main` `0176567` byte for byte (sha256 `001f12ad…`, CR ignored). It is identical at `2.2.0` ([Schema], §1).
- It declares `$schema` as a property, so sdc's own file passes `additionalProperties: false`.

**`pgp/validate.rs`** (approved, D15).
- Add `include_str!` of the CM schema and a second `OnceLock`.
- Add `pub fn validate_cm(doc) -> Result<(), Vec<String>>`, built on the existing private `check`.
- The CM schema's keywords (`type`, `properties`, `additionalProperties`, `items`, `minItems`, `maxItems`, `$ref`, `allOf`) are a subset of `KEYWORDS`.
- `crates/gui/tests/pgp.rs:917-926` (the keyword and annotation guard) is extended to walk the CM schema too. It is not a golden or `gui_repro` test.

## 8. Engine: a complex `_m` under Vanilla (D16)

Today, under Vanilla, an `_m` with varying alpha goes through three steps:
- It is suggested as `envmask`, from "_m under vanilla" (`[main] engine/src/suggest.rs:108-110`).
- An explicit `complex_material` is unavailable under Vanilla and falls back to `envmask` (`policy.rs:1103`).
- `envmask` is written as BC4 with `alpha remove` (`presets/sdc-base.toml:55-58`). With varying alpha that is blocked by `LOSS-ALPHA-DATA` (`golden.rs` `vanilla_g_is_glow_and_m_with_alpha_is_envmask_with_loss`), and allowing it drops the alpha.

**Smallest change: two deletions, no format work.**

| File | Change | Why it is enough |
|---|---|---|
| `crates/engine/src/policy.rs:1103` | Delete the arm `(Target::Vanilla, ComplexMaterial) => un(Some(Envmask))`. `complex_material` then falls through to `Available` on every target, as it already does on Cpm | `offered(Vanilla, complex_material)` becomes true, so the GUI and CLI pickers offer it. No `CFG-UNAVAILABLE` or fallback path is left for it |
| `crates/engine/src/suggest.rs:109-110` | Delete the `if c.target == Target::Vanilla` branch, so Vanilla uses the existing rule: `Varying` alpha (Note L: min < 251) gives `complex_material` High and `envmask` Low; anything else gives `envmask` High | This is the same rule as CS. It matches PGPatcher's pixel rule closely enough, and a plain `_m` stays `envmask` |
| `presets/*` | **No format change.** `sdc-base` `[subtypes.complex_material]` (alpha `data`, `exact_opaque`, default BC7) already applies under every target. `vanilla-quality` inherits it, and `vanilla-compact` already halves it (`:43`). `envmask` stays BC4 with no alpha. Text only: drop "no complex _m" from the `vanilla-quality` description (`presets/vanilla-quality.toml:6`) | The Vanilla complex `_m` gets the same profile as CS: BC7 UNORM with alpha (BC3 option) |

**Consistency with Note AH ([main] DECISIONS:1056-1088).** None of the AH logic is gated on the target. It checks only the sub-type: `policy.rs:620-622` (parallax default), `:1269`, `:1765-1787`, `ops::complex_alpha_zero`, `bc7_fixup::force_zero_alpha` (`encode.rs:125`). Under Vanilla these rules therefore apply unchanged:
- Parallax defaults to on only for alpha that really varies.
- Otherwise alpha 0 is written, and decodes as exactly 0.
- An explicit `complex_material` with no source alpha writes 0 (AH1).
- `VAL-M-COMPLEX-MARGIN` and the 251–254 card warning work the same way.

**Side effects to record (DECISIONS note).**
- Under Vanilla, a set with a complex `_m` now auto-picks the Complex Material shader (`shader.rs:564`), as it does on CS.
- A Vanilla varying-alpha `_m` changes from skipped (`LOSS-ALPHA-DATA`), or BC4 with no alpha when that is allowed, to `complex_material` BC7. It gets a new `profile_sha256`, so `if-changed` re-encodes it. The engine version stays 0.7.0 (the AH rule).
- BC7 is twice the size of BC4. Without ENB, the vanilla shader ignores the alpha. The owner accepted this ("complex only works with ENB anyways").
- The packing recipes (the cpm `_m` recipe per target) are not touched.

**Docs and schema.**
- Main spec §4.2 table (`docs/superpowers/specs/2026-10-08-skyrim-dds-converter-design.md:252`): the Vanilla cell for `complex_material` changes from "— (→ `envmask`, `LOSS-ALPHA-DATA`)" to "BC7_UNORM (BC3 option)".
- No generated schema in `schema/` encodes availability or suggestions: a grep for vanilla and complex finds nothing. The plan regenerates `schema/` anyway (allowed) and expects no diff.

**Engine tests**
- Non-golden tests that change:
  - The `suggest.rs` unit test (`:350-360`, Vanilla `_m` `Varying` → `Envmask`) now expects `ComplexMaterial`.
  - `policy_checks.rs::availability_table` (`:296-301`) now expects `Available`.
- New tests:
  - Vanilla varying `_m` → `complex_material` BC7 with alpha, parallax on.
  - Vanilla constant-255 `_m` and Vanilla no-alpha `_m` → `envmask` BC4.
  - Explicit `complex_material` with no source alpha under Vanilla → alpha 0 (AH1).
  - `vanilla-compact` halves a complex `_m`.
- **Golden tests that must change (off-limits, Q1):**
  - `crates/engine/tests/golden.rs::vanilla_fallbacks_are_written_with_allow_cfg_unavailable` (`:494-~575`): the `iron_m` `complex_material` → `envmask` fallback row goes away. The blocked and converted counts go from 4 to 3 fallback files, and the `LOSS-ALPHA-DATA` and `envmask`/`bc4` asserts for `iron_m` are dropped.
  - `crates/engine/tests/golden.rs::vanilla_g_is_glow_and_m_with_alpha_is_envmask_with_loss` (`:661-~710`): the Vanilla varying `iron_m` is now converted as `complex_material` BC7 instead of being skipped with `LOSS-ALPHA-DATA`.
- Unaffected:
  - `cpm_and_vanilla_block_unavailable_subtypes` has no `_m`.
  - The CLI `validate` tests (`cli_inspect_validate.rs:767`, `:798-810`) use near-opaque or `cs` inputs.

## 9. Testing

**Unit tests (`cm_json.rs`)**
- The golden text for both schema `examples`.
- Only set keys are written, and an unset key is absent.
- `1` is written as `1.0`.
- `specular_color` is three floats, and values above 1 are kept.
- Zero keys gives a `$schema`-only file.
- Every written text passes `validate_cm`.

**Validator**
- `validate_cm` accepts both examples.
- It rejects an unknown key, a two-entry colour and a string colour entry.
- The keyword guard covers the CM schema.

**`sets_doc`**
- `[sets.cm]` parses into `SetEntry.cm`.
- **RESERVED leak test:** `cm` keys never appear in `settings`, in the TruePBR JSON, or in `shows_json_panel`.
- Bad types are kept in the file and listed.
- The setters round-trip and keep formatting.
- The `cm_json_sha256` record gets and sets correctly.
- Undo keeps the record.

**Predicate**
- On CS, a set with a `complex_material` member shows the card.
- On Vanilla, a varying-alpha `_m` resolves to `complex_material` and shows the card (D16).
- On either target, an `envmask` `_m` with the Complex Material shader shows the card with the env-mask warning. Without that shader it is hidden.
- A PBR set is hidden.
- A set with a stored `[sets.cm]` block but no complex member still shows the card.

**Export (temporary directory)**
- With export off, or with no block, nothing is written.
- With export on, `<x>_m.json` is written next to `<x>_m.dds`. Also tested for `_em`, an `output_name` rename and a packed cpm `_m`.
- With `--sidecar`-style `<x>_m.dds.json` present, both files coexist.
- A hand-edited file is kept.
- After an untick, the file is left in place with a note.
- A path clash is refused.
- A missing `.dds` gives a note.
- An unclean or cancelled export writes nothing.
- A Vanilla complex `_m` writes its JSON with no env-mask warning. A forced `envmask` member shows the warning.

**Warnings**
- With an export row whose `smallest_alpha` is 252, the 251–254 line shows.
- 250 shows nothing.
- 255 shows the env-mask line.

**Gates and owner check**
- Gates: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`. `gui_repro` tests stay unchanged. The golden tests change only in the two listed in §8, after the owner agrees (Q1).
- Owner check in game: export a test set on CS and on Vanilla and rerun PGPatcher 2.2 with trace. Expect `Failed to parse JSON` = 0, and the Vanilla `_m` logged as `Type: complex material`. Then do the NIF check from `03` on one key.

## 10. Files and ownership (CONCURRENCY.md §3)

Build on a `nif/` branch from `nif/main` **after** the `main` merge lands. That merge brings AH, AG27 and the [main] `set_editor.rs`.

| File | Status | Change |
|---|---|---|
| `crates/gui/src/sets_doc.rs` | Off-limits, **approved** (D15) | `RESERVED` += `cm`; `SetEntry.cm`, `CmBlock` parse; `set_cm_value`, `set_cm_export`; `cm_json_shas` get/set; `restore_keeping_json_sha` keeps it |
| `crates/gui/src/pgp/validate.rs` | Off-limits, **approved** (D15) | CM schema `include_str!`, `validate_cm` |
| `crates/gui/assets/complex_material.schema.json` | New | Vendored upstream file |
| `crates/gui/src/model/cm_json.rs` | New | `cm_members`, `prepare`, `finish`, `finish_cm_json`, warnings |
| `crates/gui/src/ui/cm_card.rs` | New | The card |
| `crates/gui/src/model/mod.rs` | Shared (small commit) | `mod cm_json`, a `pending_cm` field, two hook lines beside the CS ones |
| `crates/gui/src/ui/mod.rs` | Shared | `mod cm_card` |
| `crates/gui/src/ui/set_editor.rs` | Not listed | Two `cm_card::section` calls (2D and 3D) |
| `crates/gui/tests/pgp.rs` | Not listed | Keyword guard covers the CM schema |
| `crates/gui/tests/cm_json.rs` | New | §9 |
| `crates/engine/src/policy.rs` | Off-limits, **approved** (D16) | Delete the Vanilla `complex_material` arm (`:1103`) |
| `crates/engine/src/suggest.rs` | Off-limits, **approved** (D16) | Delete the Vanilla `_m` branch (`:109-110`), and update its unit test |
| `presets/vanilla-quality.toml` | **Approved** (D16) | Description text only |
| `crates/engine/tests/policy_checks.rs` | Engine tests (non-golden) | `availability_table` expectation, plus the new Vanilla tests |
| `crates/engine/tests/golden.rs` | Off-limits golden | Two tests change (§8). **Owner question Q1** |
| `schema/` | Regeneration allowed | Expected to have no diff |
| `docs/superpowers/specs/2026-10-08-skyrim-dds-converter-design.md` | Spec doc | §4.2 Vanilla `complex_material` cell |
| `crates/gui/src/model/pbr_export.rs`, `crates/gui/src/model/sets.rs` | Off-limits | **No change** |
| `docs/DECISIONS.md` | Off-limits | Append one new note (e.g. "AL"), as AD and AH were. Confirm with the owner when the plan is approved |

## 11. Open risks

- **Vanilla behaviour change (D16).** Every Vanilla varying-alpha `_m` now becomes a BC7 complex `_m`: twice the size of BC4, the Complex Material shader is auto-picked, and earlier exports are re-encoded once. Without ENB the vanilla shader ignores the alpha.
- **A forced `envmask` still ignores the JSON.** The card warns about this, but the export still writes the file.
- **The JSON is per texture path, load-order wide.** If the `_m` shares a path with vanilla or another mod's meshes, every complex-material shape that uses that path gets the overrides. A higher-priority mod's own `_m.json`, loose or in a BSA, beats sdc's file.
- **The 251–254 warning is approximate.** PGPatcher classifies by the share of non-255 alpha (50% is untested, `pgpatcher-2.2-logs.md` open question 4). The card reads the smallest mip, and only after an export.
- **`dynamic_cubemap = false` does little under CS** while complex material is active (§4). Under ENB it is untested.
- **`specular_strength` in CS** is not traced to a shader constant (§4).
- **PGPatcher 1.2.0** crashes on a non-number colour. sdc writes only numbers, and bad stored values are left out.
- **Merge timing.** `RESERVED` (10 on [main], 11 on [nif]) and `set_editor.rs` differ between trees. Build only after the `main` merge into `nif/main` completes.

## 12. Open questions for the owner

The earlier Vanilla and post-export questions are answered: see D16 and D17.

1. **Golden tests.** D16 changes the expected Vanilla `_m` result in two off-limits golden tests: `golden.rs::vanilla_fallbacks_are_written_with_allow_cfg_unavailable` and `golden.rs::vanilla_g_is_glow_and_m_with_alpha_is_envmask_with_loss`. See §8 for exactly what changes. May the plan update them as described?

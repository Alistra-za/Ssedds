# P1-C "Your CS Settings" Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The 3D preview reads the user's Community Shaders settings through MO2 (overwrite wins), compiles the six material-local CS features the user has loaded (each behind an installed-CS gate test), fills FeatureData from those settings instead of capture constants, and re-reads them when the window regains focus, so the preview matches the user's Linear Lighting, Fresnel, hair and skin settings.

**Architecture:** A new pure module `render3d::cs_settings` reads `SettingsUser.json` over `SettingsDefault.json`, decides which features are loaded (`ShaderCache/Info.ini`, else `Shaders/Features/*.ini` minus "Disable at Boot"), hashes what it read, and produces the preview defines and the FeatureData values. Reflection records each cbuffer variable's type (`VarKind`, `REFLECT_VERSION 3`) so values are written as float or uint by the shader's own declaration. `locate::find_file` finds the files in MO2 origin order; `Renderer::set_settings` rewrites every draw's static block (no compile) and routes a define change through the P1-B keep-old permutation path; the GUI reads the settings in the existing locate worker (start and focus, both viewports) and sends `SetSettings` only when the hash changes.

**Tech Stack:** Rust workspace; D3D11 via the `windows` crate (WARP in tests); D3DCompile reflection; serde_json, sha2, hex (already render3d deps); egui/eframe + egui_kittest. No new crates.

**Spec:** `F:\ai\dds\nifparseref\plan\p1\00-p1-spec.md` (approved 2026-10-11), §4 row "P1-C: your CS settings" and §2.2 rulings. Design: `03-renderer-p1-design.md` §3 and §8 T-S1..T-S6 (same folder). Binding contract: `plan-parts-c\00-contract.md` (rulings C1–C12, interface names). Code facts at `nif/main @ 19825cc`: `plan-parts-c\00-facts.md`. Owner CS data verified 2026-10-12: `plan-parts-c\01-owner-cs-data.md`.

## Global Constraints

- Branch `nif/c-p1c` from `nif/main @ 19825cc` (P1-B merged), own worktree `F:\ai\dds\converter-nif-p1c`. Read `F:\ai\dds\nifparseref\plan\CONCURRENCY.md` first. Never touch `F:\ai\dds\converter`. Never push. Never merge into `main`.
- Off-limits (read pub items only): `crates/engine/`, `crates/cli/`, `crates/encoders-sys/`, `crates/gui/src/pgp/`, `crates/gui/src/sets_doc.rs`, `crates/gui/src/model/sets.rs`, `crates/gui/src/model/pbr_export.rs`, `schema/`, `docs/DECISIONS.md`, golden and `gui_repro` tests.
- Shared files are add-only, in small separate commits: `Cargo.toml`, `Cargo.lock`, `crates/gui/Cargo.toml`, `crates/gui/src/ui/mod.rs`, `crates/gui/src/model/mod.rs`, `crates/gui/src/app.rs`, `crates/gui/src/settings.rs`. The one planned edit is a single added line in `app.rs` `focus_regained` (Task 5), in its own commit. `Cargo.lock` must stay unchanged; if it changes, stop and report.
- No new crate dependencies.
- Owner data (`C:\_mount\moddingvol\Acherus`, the CS install, `G:\`) is read-only. Tests never read the owner's files except `#[ignore]` + `SDC_ACHERUS` tests, which panic without it, and never the real `%LOCALAPPDATA%\sdc\scan.db`. The owner's settings are never edited by hand: Task 7 changes them only through CS's in-game menu, or in a `%TEMP%` copy.
- Spec §2.2: `pointLightMult` stays π while the preview has no point lights. Design §3.2: only material-local features compile; screen-space and world features are listed as "off in preview".
- Gates before every commit: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`. Commit messages end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- Tests run on WARP + the repo stand-in `crates/render3d/tests/fixtures/standin/Lighting.hlsl`.
- Paths: Tasks 1–4 give paths from the worktree root; Tasks 5–7 give them relative to `crates/`. Edits are anchored by quoted text; line numbers are those at `19825cc` and move as earlier tasks land.
- The P1-B timing log stays on; Task 6 adds a `settings` event. The owner does not time anything by hand.
- Review routing (owner): per-task reviews on Codex gpt-6-astra (low/medium/high by risk); Opus only for the final whole-branch review.

## Review Focus

1. **A missing, partial or half-written settings file never zeroes a multiplier or blanks the preview:** absent files/keys fall back to the capture constants (C7); a JSON that fails to parse keeps the previous settings and shows a note (C10). Pinned by Task 1's malformed-JSON test, Task 2's default-equals-constants test, Task 5 `a_malformed_settings_file_keeps_the_previous_settings_and_shows_a_note`.
2. **A uint/bool setting is written as a uint bit pattern, never as a float** (JSON enables are integers, floats are f32-widened). Pinned by Task 2 `feature_data_writes_reflected_kinds`.
3. **Overwrite wins over every mod and over game Data** when the same settings file exists in several places. Pinned by Task 3 `mo2_overwrite_wins`.
4. **A settings change that changes defines never blanks or freezes the preview; one that changes only values compiles nothing.** Pinned by Task 4 `settings_change_of_define_goes_through_swap` and `a_settings_change_without_define_change_compiles_nothing`.
5. **Focus re-reads do nothing when nothing changed** (no `SetSettings`, no reload, no repaint loop). Pinned by Task 5 `focus_reread_sends_set_settings_only_when_the_hash_changes`.

## Task order and dependencies

Tasks run in this order: **1 → 2 → 3 → 4 → 5 → 6 → 7**. 1 → 2 (Task 2 uses `CsSettings`); 1 → 3 (`SettingsPaths`); 2 → 4 (`feature_values`, the `rewrite_material` signature); 3, 4 → 5; 4 → 6; 7 (owner checklist) last. Where a task edits a file an earlier task changed, follow the quoted text anchors, not line numbers.

## Cross-part reconciliation (binding; the parts were drafted in parallel)

- **R1 (`rebuild_device_on`):** Task 2 adds the `feature_values` carry line; Task 4 adds `settings` and `features_on` only.
- **R2 (`set_features` vs `set_features_on`):** as Task 4 states: `set_features` replaces the whole define list (tests, T-S5); `set_settings`/`set_features_on` replace only the defines the settings own (`ENABLE_LL` + the `PREVIEW_FEATURES` defines) and keep the rest in order. `features()` returns the active list. A fresh `Renderer` has `features_on == true`.
- **R3 (sRGB encode timing, accepted):** `set_settings`/`set_features_on` switch `encode_srgb` at once, so while new programs compile after an LL toggle the kept old draws look briefly washed out (LL on) or dark (LL off). A transition like P1-B's keep-old, never a blank; Task 7 asks the owner to judge it. The fix if rejected (switch the encode when no shape is `Pending`) is a later, separate change.
- **R4 (skin packing):** Task 2's packing, read from the owner's installed `Lighting.hlsl`, supersedes design §3.1: `physicalParams.w = WetResponse`, `skinParams2.w = BaseColorMultiplier`, `skinParams.w = EnableSkin`, `skinParams2.y = ExtraSkinWetness`; `skinDetailParams.x = SkinDetailTiling × BodyTilingMultiplier` stays a marked `shortcut:` inference.
- **R5 (hash scope, accepted):** the `Shaders/Features/*.ini` folder is not hashed (C10 as written). Without an Info.ini, adding a feature ini takes effect at the next change to one of the three files.
- **R6 (fixtures):** Task 2 adds its own fixture (`feature_kinds.hlsl`); Task 4 adds an inert `STANDIN_FEATURE_DATA` hook to the stand-in `Lighting.hlsl`. No overlap.
- **R7 (T-S5 meshes):** a vanilla face-tint head (type 4), skin-tint body (type 5) and hair from the owner's `mods\SkyrimMeshesExtract` (loose files, read by direct path; render3d cannot read BSAs) replace design §8's Jordis/Arch_Hair. T-S5 is a compile/NaN gate; the look is Task 7.
- **R8 (Meshes view toggle):** the features toggle no longer reloads, so the Meshes view shows no spinner while it recompiles (Mesh content sends no `Shapes`); the picture updates through refresh frames. Accepted.

---

### Task 1: `cs_settings` — read, merge, hash, loaded features, preview defines

**Files**
- Create: `crates/render3d/src/cs_settings.rs`
- Modify: `crates/render3d/src/lib.rs` (one line: `pub mod cs_settings;`)
- Create: `crates/render3d/tests/fixtures/cs_settings/SettingsUser.json`
- Create: `crates/render3d/tests/fixtures/cs_settings/SettingsDefault.json`
- Create: `crates/render3d/tests/fixtures/cs_settings/Info.ini` (UTF-8 **with BOM**, created by command)
- Test: `crates/render3d/tests/cs_settings.rs` (new)

**Interfaces**
- Consumes: `serde_json::{Map, Value}`, `sha2::{Digest, Sha256}`, `hex::encode` (all existing deps).
- Produces (exact, contract):
  ```rust
  pub struct SettingsPaths { pub user: Option<PathBuf>, pub default: Option<PathBuf>, pub info_ini: Option<PathBuf>, pub features_dir: Option<PathBuf> } // + Debug, Clone, Default, PartialEq
  #[derive(Debug, Clone, PartialEq)]
  pub struct CsSettings { pub hash: String, pub loaded: BTreeSet<String>, pub linear_lighting: bool,
                          pub merged: serde_json::Map<String, Value>, pub notes: Vec<String>, pub parse_failed: bool }
  impl Default for CsSettings;            // hash of three absents, loaded empty, linear_lighting true, merged empty, no notes
  pub fn read_settings(p: &SettingsPaths) -> CsSettings;   // never errors
  pub const PREVIEW_FEATURES: &[(&str, &str)];
  pub fn preview_defines(s: &CsSettings, features_on: bool) -> Vec<String>;   // sorted
  ```
- Rules implemented (state them in review):
  - **Merge:** `SettingsDefault.json` first, then `SettingsUser.json` over it, per key, recursively for nested objects
    (`"Advanced Skin".DefaultProfile` merges per key too). A non-object value on either side (e.g. `"Version"`,
    `"Disable at Boot": null` under User's `{"UnifiedWater": true}`) is simply replaced by User's value when User has
    the key, else Default's stays.
  - **Numbers:** `num(v)` = JSON number, or bool as 1/0 (owner enables are integers, some flags bools).
  - **Linear Lighting:** `"Linear Lighting".enableLinearLighting != 0`; absent → true (P0 default).
  - **Loaded:** Info.ini when present and UTF-8: every `[Section]` except `[Cache]` whose `Enabled = true`
    (case-insensitive value; BOM stripped; CRLF or LF). Otherwise `features_dir/*.ini` stems minus every
    `"Disable at Boot"` key with a truthy value (`null` disables nothing), with a note.
  - **Parse failure:** a present JSON file that does not parse (or is not an object) contributes nothing, adds a note
    naming the file, sets `parse_failed`; the other file still merges; its bytes are still hashed. A non-UTF-8 Info.ini
    likewise (falls back to the features dir). A read error other than NotFound → note, treated as absent.
  - **Hash (C10):** sha256 over, in order `user`, `default`, `info_ini`: the role name, then either the byte length as
    u64 LE followed by the bytes, or `absent`. Hex. `read_settings` of nothing equals `CsSettings::default()` exactly.
  - **Defines (C8):** none if `!features_on`; `ENABLE_LL` iff `linear_lighting`; each `PREVIEW_FEATURES` row iff the
    Info.ini name is in `loaded` and its own master switch is not 0 (`"Vanilla Fresnel".Enable`,
    `"Hair Specular".Enabled`; Skin / ExtendedMaterials / ExtendedTranslucency have none). Sorted.

- [ ] **Step 1: Create the fixtures.**

`crates/render3d/tests/fixtures/cs_settings/SettingsUser.json` (owner's SettingsUser.json sections verbatim from
01 §3a, 1-space indent, plus `Version`):
```json
{
 "Advanced Skin": {
  "DefaultProfile": {
   "BaseColorMultiplier": 1.0,
   "BodyTilingMultiplier": 2.0,
   "EnableSkinDetail": false,
   "ExtraEdgeRoughness": 0.25,
   "F0": 0.027799999341368675,
   "FuzzF0": 0.04500000178813934,
   "FuzzRoughness": 0.3499999940395355,
   "FuzzStrength": 1.0,
   "PhysicalMainRoughnessMultiplier": 1.2999999523162842,
   "PhysicalSecondRoughnessMultiplier": 0.75,
   "PhysicalSpecularStrength": 1.0,
   "SSSAmount": 1.0,
   "SecondarySpecularStrength": 0.15000000596046448,
   "SkinDetailStrength": 0.25,
   "SkinDetailTiling": 10.0,
   "SkinMainRoughness": 0.699999988079071,
   "SkinSecondRoughness": 0.3499999940395355,
   "SkinSpecularTexMultiplier": 1.0,
   "Translucency": 0.10000000149011612,
   "UseSSS": true,
   "WetResponse": 1.0,
   "sssWidth": 0.20000000298023224
  },
  "DetailTexture": "Data/Shaders/Skin/skin_detail_n.dds",
  "EnableSkin": true,
  "ExtraSkinWetness": 0.0,
  "FullSweat": 0.15000000596046448,
  "StartSweat": 0.75,
  "WetFadeTime": 10.0,
  "WetParams": [
   512.0,
   0.699999988079071,
   10.0,
   4.0
  ]
 },
 "Disable at Boot": {
  "UnifiedWater": true
 },
 "Extended Materials": {
  "EnableComplexMaterial": 1,
  "EnableHeightBlending": 1,
  "EnableParallax": 1,
  "EnableParallaxWarpingFix": 1,
  "EnableShadows": 1,
  "EnableTerrain": 0
 },
 "Extended Translucency": {
  "AlphaMode": 3,
  "AlphaReduction": 0.15000000596046448,
  "AlphaSoftness": 0.0,
  "AlphaStrength": 0.0,
  "SkinnedOnly": true
 },
 "General": {
  "Enable Async": true,
  "Enable Disk Cache": true,
  "Enable Shaders": true,
  "Language": "en",
  "Skip Unchanged Shaders": true
 },
 "Hair Specular": {
  "BaseColorMult": 1.5,
  "DiffuseIndirectMult": 1.0,
  "DiffuseMult": 1.0,
  "EnableSelfShadow": 1,
  "EnableTangentShift": 1,
  "Enabled": 1,
  "HairGlossiness": 70.0,
  "HairMode": 1,
  "HairSaturation": 1.0,
  "PrimaryTangentShift": 0.5,
  "SecondaryTangentShift": -0.25,
  "SelfShadowExponent": 0.10000000149011612,
  "SelfShadowScale": 2.5,
  "SelfShadowStrength": 1.0,
  "SpecularIndirectMult": 1.0,
  "SpecularMult": 1.0,
  "Transmission": 1.0
 },
 "Linear Lighting": {
  "ambientMult": 1.0,
  "bloodEffectMult": 1.0,
  "deferredEffectMult": 1.0,
  "diffuseGamma": 2.200000047683716,
  "diffuseMidReflectance": 0.28717461228370667,
  "diffuseWhiteReflectance": 1.0,
  "directionalLightMult": 1.0,
  "effectLightingMult": 0.3199999928474426,
  "emitColorMult": 1.0,
  "enableACEScg": 0,
  "enableLinearLighting": 1,
  "fireEffectCurve": 1.0,
  "fireEffectMult": 1.0,
  "gameGamma": 1.600000023841858,
  "glowmapMult": 0.6600000262260437,
  "lightSpriteEffectMult": 1.0,
  "membraneEffectMult": 1.0,
  "otherEffectMult": 1.0,
  "particleEffectMult": 1.0,
  "pointLightMult": 1.0,
  "projectedEffectMult": 1.0
 },
 "True PBR": {
  "EnableMicroShadows": 1,
  "EnableVerboseJsonLogging": false,
  "MicroShadowStrength": 1.0,
  "VertexAOStrength": 1.0
 },
 "Vanilla Fresnel": {
  "BaseF0Multiplier": 0.3199999928474426,
  "ComplexMaterialF0Multiplier": 1.0,
  "CubemapToF0Multiplier": 1.0,
  "Enable": 1,
  "EnableDynamicCubemapsConversion": 0,
  "EnableEyeSpecialHandling": 1,
  "EnableGGX": 0,
  "EnableGGXOnGrass": 0,
  "MinF0": 0.019999999552965164,
  "RoughnessMultiplier": 1.0,
  "SpecularRoughnessBlend": 1.0
 },
 "Version": "1-9-0-0"
}
```

`crates/render3d/tests/fixtures/cs_settings/SettingsDefault.json` (owner-shaped, trimmed to three sections; carries
the two real User/Default differences: `EnableSkinDetail: true` and `"Disable at Boot": null`):
```json
{
 "Advanced Skin": {
  "DefaultProfile": {
   "BaseColorMultiplier": 1.0,
   "BodyTilingMultiplier": 2.0,
   "EnableSkinDetail": true,
   "ExtraEdgeRoughness": 0.25,
   "F0": 0.027799999341368675,
   "FuzzF0": 0.04500000178813934,
   "FuzzRoughness": 0.3499999940395355,
   "FuzzStrength": 1.0,
   "PhysicalMainRoughnessMultiplier": 1.2999999523162842,
   "PhysicalSecondRoughnessMultiplier": 0.75,
   "PhysicalSpecularStrength": 1.0,
   "SSSAmount": 1.0,
   "SecondarySpecularStrength": 0.15000000596046448,
   "SkinDetailStrength": 0.25,
   "SkinDetailTiling": 10.0,
   "SkinMainRoughness": 0.699999988079071,
   "SkinSecondRoughness": 0.3499999940395355,
   "SkinSpecularTexMultiplier": 1.0,
   "Translucency": 0.10000000149011612,
   "UseSSS": true,
   "WetResponse": 1.0,
   "sssWidth": 0.20000000298023224
  },
  "DetailTexture": "Data/Shaders/Skin/skin_detail_n.dds",
  "EnableSkin": true,
  "ExtraSkinWetness": 0.0,
  "FullSweat": 0.15000000596046448,
  "StartSweat": 0.75,
  "WetFadeTime": 10.0,
  "WetParams": [
   512.0,
   0.699999988079071,
   10.0,
   4.0
  ]
 },
 "Disable at Boot": null,
 "Linear Lighting": {
  "ambientMult": 1.0,
  "bloodEffectMult": 1.0,
  "deferredEffectMult": 1.0,
  "diffuseGamma": 2.200000047683716,
  "diffuseMidReflectance": 0.28717461228370667,
  "diffuseWhiteReflectance": 1.0,
  "directionalLightMult": 1.0,
  "effectLightingMult": 0.3199999928474426,
  "emitColorMult": 1.0,
  "enableACEScg": 0,
  "enableLinearLighting": 1,
  "fireEffectCurve": 1.0,
  "fireEffectMult": 1.0,
  "gameGamma": 1.600000023841858,
  "glowmapMult": 0.6600000262260437,
  "lightSpriteEffectMult": 1.0,
  "membraneEffectMult": 1.0,
  "otherEffectMult": 1.0,
  "particleEffectMult": 1.0,
  "pointLightMult": 1.0,
  "projectedEffectMult": 1.0
 },
 "Version": "1-9-0-0"
}
```

`Info.ini` cannot carry its BOM through Markdown: create it with this exact Git Bash command (the BOM test asserts the
first three bytes, so a lost BOM fails loudly):
```bash
cd /f/ai/dds/converter-nif-p1c/crates/render3d/tests/fixtures/cs_settings
printf '\xEF\xBB\xBF' > Info.ini
cat >> Info.ini <<'EOF'
[Cache]
PluginVersion = 1-9-0-0


[TruePBR]
Enabled = true
Version = 1-1-0


[ExtendedMaterials]
Enabled = true
Version = 1-4-0


[VanillaFresnel]
Enabled = true
Version = 1-0-0


[HairSpecular]
Enabled = true
Version = 1-1-2


[ExtendedTranslucency]
Enabled = true
Version = 1-0-0


[LinearLighting]
Enabled = true
Version = 1-3-0


[UnifiedWater]
Enabled = false
Version = 1-0-0


[Skin]
Enabled = true
Version = 0-3-0
EOF
head -c 3 Info.ini | od -An -tx1   # expect: ef bb bf
```

- [ ] **Step 2: Write the failing tests** — `crates/render3d/tests/cs_settings.rs`:
```rust
//! P1-C (03 §3, T-S1/T-S2): the user's CS settings files → merged settings, loaded features, change hash and
//! preview defines. File I/O on fixtures and tempdirs only: never the owner's real files, no GPU.
use render3d::cs_settings::{preview_defines, read_settings, CsSettings, SettingsPaths};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn fixtures() -> PathBuf {
    PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/cs_settings"
    ))
}

/// The owner-shaped fixture files (each optional) and a features folder.
fn paths(user: bool, default: bool, info: bool, features: Option<&Path>) -> SettingsPaths {
    let f = fixtures();
    SettingsPaths {
        user: user.then(|| f.join("SettingsUser.json")),
        default: default.then(|| f.join("SettingsDefault.json")),
        info_ini: info.then(|| f.join("Info.ini")),
        features_dir: features.map(Path::to_path_buf),
    }
}

/// A `Shaders/Features` folder with one CS feature ini per name.
fn features_dir(names: &[&str]) -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    for n in names {
        std::fs::write(
            d.path().join(format!("{n}.ini")),
            "[Info]\nVersion = 1-0-0\n",
        )
        .unwrap();
    }
    d
}

fn names_set(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|s| s.to_string()).collect()
}

/// Info.ini fixture sections with `Enabled = true` (UnifiedWater is false, `[Cache]` is not a feature).
const INFO_LOADED: &[&str] = &[
    "ExtendedMaterials",
    "ExtendedTranslucency",
    "HairSpecular",
    "LinearLighting",
    "Skin",
    "TruePBR",
    "VanillaFresnel",
];

/// T-S1: per key, User over Default; a key in neither is left out of `merged` (the built-in table fills it).
#[test]
fn settings_overlay_user_over_default_over_builtin() {
    let d = tempfile::tempdir().unwrap();
    let (user, default) = (
        d.path().join("SettingsUser.json"),
        d.path().join("SettingsDefault.json"),
    );
    std::fs::write(
        &default,
        r#"{"Linear Lighting": {"glowmapMult": 0.25, "ambientMult": 2.0}, "Version": "1-9-0-0"}"#,
    )
    .unwrap();
    std::fs::write(&user, r#"{"Linear Lighting": {"glowmapMult": 0.5}}"#).unwrap();
    let s = read_settings(&SettingsPaths {
        user: Some(user),
        default: Some(default),
        ..Default::default()
    });
    let ll = &s.merged["Linear Lighting"];
    assert_eq!(ll["glowmapMult"].as_f64(), Some(0.5), "User wins");
    assert_eq!(ll["ambientMult"].as_f64(), Some(2.0), "Default fills a key User lacks");
    assert!(ll.get("directionalLightMult").is_none(), "in neither file");
    assert_eq!(s.merged["Version"].as_str(), Some("1-9-0-0"));
    assert!(!s.parse_failed && s.notes.is_empty(), "T-S1 notes: {:?}", s.notes);
    // Owner shape: the nested DefaultProfile merges per key; User's "Disable at Boot" object replaces Default's null.
    let o = read_settings(&paths(true, true, false, None));
    let profile = &o.merged["Advanced Skin"]["DefaultProfile"];
    assert_eq!(profile["EnableSkinDetail"].as_bool(), Some(false));
    assert_eq!(profile["SkinDetailTiling"].as_f64(), Some(10.0));
    assert_eq!(o.merged["Disable at Boot"]["UnifiedWater"].as_bool(), Some(true));
    let only_default = read_settings(&paths(false, true, false, None));
    assert!(only_default.merged["Disable at Boot"].is_null());
    assert_eq!(
        only_default.merged["Advanced Skin"]["DefaultProfile"]["EnableSkinDetail"].as_bool(),
        Some(true)
    );
}

/// T-S2: Info.ini decides; feature inis it does not list (Upscaling) or disables (UnifiedWater) do not count.
#[test]
fn info_ini_wins() {
    let f = features_dir(&["HairSpecular", "Skin", "UnifiedWater", "Upscaling"]);
    let s = read_settings(&paths(true, true, true, Some(f.path())));
    assert_eq!(s.loaded, names_set(INFO_LOADED));
    assert!(!s.loaded.contains("Cache"));
    assert!(s.notes.is_empty(), "{:?}", s.notes);
}

/// T-S2: no Info.ini → `Features/*.ini` minus "Disable at Boot" (User's object; Default's null disables nothing).
#[test]
fn fallback_to_feature_inis_minus_disable_at_boot() {
    let f = features_dir(&["HairSpecular", "Skin", "UnifiedWater", "VanillaFresnel"]);
    let s = read_settings(&paths(true, true, false, Some(f.path())));
    assert_eq!(s.loaded, names_set(&["HairSpecular", "Skin", "VanillaFresnel"]));
    assert!(s.notes.iter().any(|n| n.contains("Info.ini")), "{:?}", s.notes);
    assert!(!s.parse_failed);
    let d = read_settings(&paths(false, true, false, Some(f.path())));
    assert_eq!(
        d.loaded,
        names_set(&["HairSpecular", "Skin", "UnifiedWater", "VanillaFresnel"])
    );
}

/// The owner's Info.ini starts with a UTF-8 BOM before `[Cache]`; a BOM before a feature section must not hide it.
#[test]
fn info_ini_bom_and_cache_section() {
    let bytes = std::fs::read(fixtures().join("Info.ini")).unwrap();
    assert_eq!(&bytes[..3], &[0xEF, 0xBB, 0xBF], "the fixture must keep its BOM");
    let s = read_settings(&paths(false, false, true, None));
    assert_eq!(s.loaded, names_set(INFO_LOADED));
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("Info.ini");
    std::fs::write(
        &p,
        "\u{feff}[TruePBR]\r\nEnabled = true\r\n\r\n[Skin]\r\nEnabled = false\r\n",
    )
    .unwrap();
    let s = read_settings(&SettingsPaths {
        info_ini: Some(p),
        ..Default::default()
    });
    assert_eq!(s.loaded, names_set(&["TruePBR"]));
}

/// C10: same bytes → same hash; any byte, or present-but-empty vs absent → a different hash; nothing found =
/// `CsSettings::default()` exactly (so the GUI sends nothing).
#[test]
fn hash_is_stable_and_changes_with_any_byte() {
    let d = tempfile::tempdir().unwrap();
    for n in ["SettingsUser.json", "SettingsDefault.json", "Info.ini"] {
        std::fs::copy(fixtures().join(n), d.path().join(n)).unwrap();
    }
    let info = d.path().join("Info.ini");
    let p = SettingsPaths {
        user: Some(d.path().join("SettingsUser.json")),
        default: Some(d.path().join("SettingsDefault.json")),
        info_ini: Some(info.clone()),
        features_dir: None,
    };
    let a = read_settings(&p);
    assert_eq!(a.hash.len(), 64);
    assert_eq!(read_settings(&p).hash, a.hash);
    let mut b = std::fs::read(&info).unwrap();
    *b.last_mut().unwrap() ^= 1;
    std::fs::write(&info, &b).unwrap();
    assert_ne!(read_settings(&p).hash, a.hash);
    std::fs::write(&info, b"").unwrap();
    let empty = read_settings(&p).hash;
    std::fs::remove_file(&info).unwrap();
    assert_ne!(read_settings(&p).hash, empty);
    assert_eq!(read_settings(&SettingsPaths::default()), CsSettings::default());
}

/// C10: a file CS is half-way through writing → a note naming it, `parse_failed`, and the bad file contributes
/// nothing (no value from a previous read either); the other file still applies; its bytes still change the hash.
#[test]
fn malformed_json_notes_and_falls_back() {
    let d = tempfile::tempdir().unwrap();
    let user = d.path().join("SettingsUser.json");
    std::fs::write(&user, "{\"Linear Lighting\": {\"enableLinearLighting\": 0,").unwrap();
    let s = read_settings(&SettingsPaths {
        user: Some(user),
        default: Some(fixtures().join("SettingsDefault.json")),
        ..Default::default()
    });
    assert!(s.parse_failed);
    assert!(
        s.notes.iter().any(|n| n.contains("SettingsUser.json")),
        "{:?}",
        s.notes
    );
    assert!(s.linear_lighting, "the bad file's enableLinearLighting 0 is not applied");
    assert_eq!(
        s.merged["Advanced Skin"]["DefaultProfile"]["EnableSkinDetail"].as_bool(),
        Some(true)
    );
    let good = read_settings(&paths(false, true, false, None));
    assert_ne!(s.hash, good.hash);
}

/// C8: defines from loaded features and their own switches; LL from the setting; off toggle → none.
#[test]
fn preview_defines_follow_loaded_and_switches() {
    let s = read_settings(&paths(true, true, true, None));
    assert_eq!(
        preview_defines(&s, true),
        [
            "CS_HAIR",
            "CS_SKIN",
            "ENABLE_LL",
            "EXTENDED_MATERIALS",
            "EXTENDED_TRANSLUCENCY",
            "VANILLA_FRESNEL"
        ]
    );
    assert!(preview_defines(&s, false).is_empty());
    // No files: exactly today's default (Part B's set_features_on relies on it).
    assert_eq!(
        preview_defines(&CsSettings::default(), true),
        render3d::DEFAULT_FEATURES
    );
    let mut off = s.clone();
    off.merged.get_mut("Hair Specular").unwrap()["Enabled"] = 0.into();
    off.merged.get_mut("Vanilla Fresnel").unwrap()["Enable"] = 0.into();
    off.linear_lighting = false;
    assert_eq!(
        preview_defines(&off, true),
        ["CS_SKIN", "EXTENDED_MATERIALS", "EXTENDED_TRANSLUCENCY"]
    );
    let d = tempfile::tempdir().unwrap();
    let user = d.path().join("SettingsUser.json");
    std::fs::write(&user, r#"{"Linear Lighting": {"enableLinearLighting": 0}}"#).unwrap();
    let ll_off = read_settings(&SettingsPaths {
        user: Some(user),
        ..Default::default()
    });
    assert!(!ll_off.linear_lighting);
    assert!(preview_defines(&ll_off, true).is_empty());
}
```

- [ ] **Step 3: Run — expect a compile failure.**
`cargo test -p render3d --test cs_settings`
Expected: `error[E0432]: unresolved import render3d::cs_settings` (the module does not exist yet).

- [ ] **Step 4: Implement.** Create `crates/render3d/src/cs_settings.rs`:
```rust
//! The user's Community Shaders settings (03 §3): `SettingsUser.json` over `SettingsDefault.json` per key, which
//! features CS loaded (`ShaderCache/Info.ini`, else `Shaders/Features/*.ini` minus "Disable at Boot"), a change
//! hash, and the feature defines the preview compiles. Never fails: problems become notes and fallbacks.
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Where the settings files and the feature ini folder are; `None` = not found.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SettingsPaths {
    pub user: Option<PathBuf>,
    pub default: Option<PathBuf>,
    pub info_ini: Option<PathBuf>,
    pub features_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CsSettings {
    /// sha256 over the bytes read (C10): an equal hash means nothing to apply.
    pub hash: String,
    /// Info.ini short names of the features CS loaded.
    pub loaded: BTreeSet<String>,
    /// `"Linear Lighting".enableLinearLighting != 0`; true when absent (the P0 default).
    pub linear_lighting: bool,
    /// Every section, User over Default per key (nested objects per key too).
    pub merged: Map<String, Value>,
    /// Problems and fallbacks taken, for the panel.
    pub notes: Vec<String>,
    /// A present file failed to parse: the GUI keeps its previous settings and shows the note (C10).
    pub parse_failed: bool,
}

impl Default for CsSettings {
    /// No files: built-in values, Linear Lighting on, no feature loaded.
    fn default() -> Self {
        CsSettings {
            hash: settings_hash(None, None, None),
            loaded: BTreeSet::new(),
            linear_lighting: true,
            merged: Map::new(),
            notes: vec![],
            parse_failed: false,
        }
    }
}

/// Material-local CS features the preview compiles when loaded and not switched off (03 §3.2, C8): Info.ini
/// name → define. A row whose installed-CS gate test (T-S5) fails is deleted here (C9).
pub const PREVIEW_FEATURES: &[(&str, &str)] = &[
    ("VanillaFresnel", "VANILLA_FRESNEL"),
    ("HairSpecular", "CS_HAIR"),
    ("Skin", "CS_SKIN"),
    ("ExtendedMaterials", "EXTENDED_MATERIALS"),
    ("ExtendedTranslucency", "EXTENDED_TRANSLUCENCY"),
];

/// Reads and merges the settings. Absent files are skipped; a file that does not parse is skipped with a note
/// and `parse_failed`, but its bytes still count in the hash.
pub fn read_settings(p: &SettingsPaths) -> CsSettings {
    let mut notes = vec![];
    let user = read(p.user.as_deref(), &mut notes);
    let default = read(p.default.as_deref(), &mut notes);
    let info = read(p.info_ini.as_deref(), &mut notes);
    let mut parse_failed = false;
    let mut merged = Map::new();
    for (file, bytes) in [("SettingsDefault.json", &default), ("SettingsUser.json", &user)] {
        let Some(b) = bytes else {
            continue;
        };
        match serde_json::from_slice::<Value>(strip_bom(b)) {
            Ok(Value::Object(m)) => overlay(&mut merged, m),
            Ok(_) => {
                parse_failed = true;
                notes.push(format!("{file} is not a JSON object: ignored"));
            }
            Err(e) => {
                parse_failed = true;
                notes.push(format!("{file} could not be parsed ({e}): ignored"));
            }
        }
    }
    let linear_lighting =
        setting(&merged, "Linear Lighting", "enableLinearLighting").is_none_or(|x| x != 0.0);
    let loaded = match info.as_deref().map(std::str::from_utf8) {
        Some(Ok(text)) => info_ini_features(text),
        Some(Err(e)) => {
            parse_failed = true;
            notes.push(format!("Info.ini is not UTF-8 ({e}): ignored"));
            feature_inis(p.features_dir.as_deref(), &merged, &mut notes)
        }
        None => feature_inis(p.features_dir.as_deref(), &merged, &mut notes),
    };
    CsSettings {
        hash: settings_hash(user.as_deref(), default.as_deref(), info.as_deref()),
        loaded,
        linear_lighting,
        merged,
        notes,
        parse_failed,
    }
}

/// The feature defines for these settings (C8), sorted, so equal lists mean "defines unchanged".
pub fn preview_defines(s: &CsSettings, features_on: bool) -> Vec<String> {
    if !features_on {
        return vec![];
    }
    let mut out: Vec<String> = PREVIEW_FEATURES
        .iter()
        .filter(|(name, _)| s.loaded.contains(*name) && !switched_off(s, name))
        .map(|(_, define)| define.to_string())
        .collect();
    if s.linear_lighting {
        out.push("ENABLE_LL".into());
    }
    out.sort();
    out
}

/// The feature's own master switch is 0. Skin, ExtendedMaterials and ExtendedTranslucency have none.
fn switched_off(s: &CsSettings, feature: &str) -> bool {
    let (section, key) = match feature {
        "VanillaFresnel" => ("Vanilla Fresnel", "Enable"),
        "HairSpecular" => ("Hair Specular", "Enabled"),
        _ => return false,
    };
    setting(&s.merged, section, key).is_some_and(|x| x == 0.0)
}

fn setting(m: &Map<String, Value>, section: &str, key: &str) -> Option<f64> {
    m.get(section).and_then(|s| s.get(key)).and_then(num)
}

/// A JSON number, or a bool as 1/0 (CS writes enables as integers and some flags as bools).
fn num(v: &Value) -> Option<f64> {
    v.as_f64()
        .or_else(|| v.as_bool().map(|b| f64::from(u8::from(b))))
}

/// `top` over `base` per key; two objects merge per key, anything else is replaced.
fn overlay(base: &mut Map<String, Value>, top: Map<String, Value>) {
    for (k, v) in top {
        match (base.get_mut(&k), v) {
            (Some(Value::Object(b)), Value::Object(t)) => overlay(b, t),
            (Some(slot), v) => *slot = v,
            (None, v) => {
                base.insert(k, v);
            }
        }
    }
}

fn read(p: Option<&Path>, notes: &mut Vec<String>) -> Option<Vec<u8>> {
    let p = p?;
    match std::fs::read(p) {
        Ok(b) => Some(b),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => {
            notes.push(format!("{}: {e}", p.display()));
            None
        }
    }
}

fn strip_bom(b: &[u8]) -> &[u8] {
    b.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(b)
}

/// Info.ini (BOM, CRLF or LF): every `[Section]` but `[Cache]` with `Enabled = true`.
fn info_ini_features(text: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut section = "";
    for line in text.trim_start_matches('\u{feff}').lines() {
        let line = line.trim();
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            section = name.trim();
        } else if let Some((k, v)) = line.split_once('=') {
            if !section.is_empty()
                && section != "Cache"
                && k.trim() == "Enabled"
                && v.trim().eq_ignore_ascii_case("true")
            {
                out.insert(section.to_string());
            }
        }
    }
    out
}

/// No usable Info.ini (disk cache off, or the game never ran): a feature is loaded iff `<dir>/<Name>.ini` exists
/// and "Disable at Boot" does not set it (03 §3.1).
fn feature_inis(
    dir: Option<&Path>,
    merged: &Map<String, Value>,
    notes: &mut Vec<String>,
) -> BTreeSet<String> {
    let Some(dir) = dir else {
        return BTreeSet::new();
    };
    notes.push(format!(
        "no usable Info.ini: loaded features taken from {}",
        dir.display()
    ));
    let off = |name: &str| setting(merged, "Disable at Boot", name).is_some_and(|x| x != 0.0);
    match std::fs::read_dir(dir) {
        Ok(entries) => entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("ini")))
            .filter_map(|p| p.file_stem().and_then(|s| s.to_str()).map(str::to_string))
            .filter(|n| !off(n))
            .collect(),
        Err(e) => {
            notes.push(format!("{}: {e}", dir.display()));
            BTreeSet::new()
        }
    }
}

/// C10: sha256 over (role, length, bytes) or (role, "absent") for user, default, info_ini, in that order.
fn settings_hash(user: Option<&[u8]>, default: Option<&[u8]>, info: Option<&[u8]>) -> String {
    let mut h = Sha256::new();
    for (role, b) in [("user", user), ("default", default), ("info_ini", info)] {
        h.update(role.as_bytes());
        match b {
            Some(b) => {
                h.update((b.len() as u64).to_le_bytes());
                h.update(b);
            }
            None => h.update(b"absent"),
        }
    }
    hex::encode(h.finalize())
}
```

In `crates/render3d/src/lib.rs`, anchor:
```rust
pub mod compile;
pub mod device;
```
becomes
```rust
pub mod compile;
pub mod cs_settings;
pub mod device;
```

- [ ] **Step 5: Run — expect pass.**
`cargo test -p render3d --test cs_settings`
Expected: 7 passed (`settings_overlay_user_over_default_over_builtin`, `info_ini_wins`,
`fallback_to_feature_inis_minus_disable_at_boot`, `info_ini_bom_and_cache_section`,
`hash_is_stable_and_changes_with_any_byte`, `malformed_json_notes_and_falls_back`,
`preview_defines_follow_loaded_and_switches`).

- [ ] **Step 6: Gates.**
`cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`

- [ ] **Step 7: Commit.**
```bash
git add crates/render3d/src/cs_settings.rs crates/render3d/src/lib.rs crates/render3d/tests/cs_settings.rs crates/render3d/tests/fixtures/cs_settings
git commit -m "render3d: read the user's CS settings, loaded features, change hash and preview defines (P1-C Task 1)" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: reflected `VarKind`, `REFLECT_VERSION 3`, FeatureData from the settings

Two commits: **2a** reflection types (every `CbLayout.vars` site), **2b** `feature_data` / `write_feature_data` /
`diffuse_curve` replacing the static writer sites.

**Files**
- Modify: `crates/render3d/src/compile.rs` (`VarKind`, `CbVar`, `CbLayout.vars`, `struct_members`, `reflect`, `REFLECT_VERSION`)
- Modify: `crates/render3d/src/cbuffer.rs` (`vars: Vec<CbVar>`, `find`, `resolve`, new `kind`)
- Modify: `crates/render3d/src/cs_settings.rs` (2b: `feature_data`, `write_feature_data`, `diffuse_curve`)
- Modify: `crates/render3d/src/scene.rs` (`Scene::write` and `write_static` take the values; FEATURE_* doc relabel; unit test layout)
- Modify: `crates/render3d/src/draw.rs` (`update_by_name`, `rewrite_material` take the values)
- Modify: `crates/render3d/src/lib.rs` (`Renderer.feature_values`, init, `rebuild_device_on` carry, four call sites)
- Create: `crates/render3d/tests/fixtures/feature_kinds.hlsl`
- Test: `crates/render3d/tests/compile.rs`, `crates/render3d/tests/cs_settings.rs`; update `crates/render3d/tests/frame.rs`, `crates/render3d/tests/material.rs`

Every construction / field-access site of `CbLayout.vars` (grep `vars` / `CbLayout` over `crates/`, nif/main 19825cc;
no other crate builds a `CbLayout`):

| Site | Change |
|---|---|
| `crates/render3d/src/compile.rs` `pub vars: Vec<(String, u32, u32)>` | `Vec<CbVar>` |
| `crates/render3d/src/compile.rs` `struct_members(.., out: &mut Vec<(String, u32, u32)>)` + `out.push((full.clone(), base + off, msize))` | `CbVar` with member kind |
| `crates/render3d/src/compile.rs` `reflect`: `vars.push((name.clone(), vd.StartOffset, vd.Size))` | `CbVar` with top-level kind |
| `crates/render3d/src/cbuffer.rs` `vars: Vec<(String, u32, u32)>`, `find` (`v.0`, `v.1`, `v.2`) | `Vec<CbVar>`, fields by name |
| `crates/render3d/src/cbuffer.rs` test `size_zero_layout_still_gets_one_register` (`vars: vec![]`) | unchanged (empty vec) |
| `crates/render3d/src/scene.rs` test `world_rows_pack_row_major_camera_relative` (`vars: vec![("World".into(), 0, 48), ...]`) | `CbVar` |
| `crates/render3d/tests/compile.rs` `tiny_shader_compiles_and_reflects` (`v.0 == "Tint" && v.1 == 16`) | `v.name`, `v.offset`, `v.kind` |
| `crates/render3d/tests/compile.rs` `cbuffer_writer_sets_by_name` (`vars: vec![("Scale".into(), 0, 4), ...]`) | `CbVar` |
| `crates/render3d/tests/frame.rs` `fn layout` (`.map(\|(i, n)\| (n.clone(), size * i as u32, size))`) | `CbVar` (FEATURE_UINTS names `Uint`, rest `Float`) |
| `crates/render3d/tests/frame.rs` `&l.vars[..2]` in a `{:?}` | unchanged (`CbVar: Debug`) |
| `crates/render3d/tests/material.rs` `fn writer` (`.map(\|(i, (n, s))\| (n.to_string(), 16 * i as u32, *s))`) | `CbVar` kind `Float` |

Static FeatureData writer sites replaced (facts §1): `Renderer::build_draw` (`scene::write_static(&mut cb.writer)`),
`ShapeDraw::rewrite_material` (`write_static(&mut cb.writer)`), `Scene::write` (the two `for (n, d) in FEATURE_…` loops),
and `write_static` itself (the same two loops). Callers that pass the values: `lib.rs` build_draw, the two
`d.rewrite_material(…, ll)` calls (`set_material` path and the texture-swap loop), `render`'s
`s.update_by_name(gpu, scene, &view)`; tests `scene.rs` unit test, `tests/frame.rs`.

**Interfaces**
- Consumes: Task 1 `CsSettings`, `setting`, `num` (private helpers in the same module); `scene::{FEATURE_FLOATS, FEATURE_UINTS}`.
- Produces:
  ```rust
  // compile.rs
  #[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
  pub enum VarKind { Float, Uint, Int, Bool, Other }
  #[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
  pub struct CbVar { pub name: String, pub offset: u32, pub size: u32, pub kind: VarKind }
  // CbLayout.vars: Vec<CbVar>; REFLECT_VERSION = 3
  // cbuffer.rs
  impl CbWriter { pub fn kind(&self, var: &str) -> Option<VarKind> }
  // cs_settings.rs
  pub fn feature_data(s: &CsSettings) -> Vec<(String, Vec<f64>)>;
  pub fn write_feature_data(w: &mut CbWriter, values: &[(String, Vec<f64>)]);
  pub fn diffuse_curve(gamma: f64, mid: f64, white: f64) -> f64;
  // scene.rs
  pub fn write_static(w: &mut CbWriter, feature_values: &[(String, Vec<f64>)]);
  impl Scene { pub fn write(&self, w: &mut CbWriter, v: &View, model: &Mat3x4, feature_values: &[(String, Vec<f64>)]) -> Vec<&'static str> }
  // draw.rs
  impl ShapeDraw {
      pub fn update_by_name(&mut self, gpu: &Gpu, scene: &Scene, view: &View, feature_values: &[(String, Vec<f64>)]) -> usize;
      pub fn rewrite_material(&mut self, m: &MaterialState, linear_lighting: bool, feature_values: &[(String, Vec<f64>)]);
  }
  // lib.rs (private field, Part B sets it in set_settings)
  Renderer.feature_values: Vec<(String, Vec<f64>)>   // = cs_settings::feature_data(&CsSettings::default()) in with_gpu
  ```
- Write rule (C6): reflected `Float | Other` → f32; `Uint | Bool` → `x as u32`; `Int` → `x as i32 as u32`; name not in
  the layout → skipped (`kind` is `None`).

#### 2a — reflection types

- [ ] **Step 1: Write the failing tests.**

Create `crates/render3d/tests/fixtures/feature_kinds.hlsl`:
```hlsl
// P1-C Task 2: a FeatureData-shaped cbuffer (namespaced like CS's SharedData.hlsli, struct members of each scalar
// type) for the reflected-kind tests in tests/compile.rs.
namespace SharedData
{
	struct HairSpecularSettings
	{
		uint Enabled;
		float HairGlossiness;
		int Signed;
		bool Flag;
	};

	cbuffer FeatureData : register(b6)
	{
		HairSpecularSettings hairSpecularSettings;
		float4 skinParams;
	};
}

float4 main(float4 pos : SV_Position) : SV_Target
{
	float g = SharedData::hairSpecularSettings.HairGlossiness + SharedData::hairSpecularSettings.Signed +
	          (SharedData::hairSpecularSettings.Flag ? 1.0 : 0.0);
	return SharedData::hairSpecularSettings.Enabled ? SharedData::skinParams * g : 0.0;
}
```

In `crates/render3d/tests/compile.rs`, replace
```rust
    assert!(cb.vars.iter().any(|v| v.0 == "Tint" && v.1 == 16));
```
with
```rust
    assert!(cb.vars.iter().any(|v| v.name == "Tint"
        && v.offset == 16
        && v.kind == render3d::compile::VarKind::Float));
```
replace
```rust
        vars: vec![("Scale".into(), 0, 4), ("Tint".into(), 16, 12)],
```
with
```rust
        vars: vec![
            render3d::compile::CbVar {
                name: "Scale".into(),
                offset: 0,
                size: 4,
                kind: render3d::compile::VarKind::Float,
            },
            render3d::compile::CbVar {
                name: "Tint".into(),
                offset: 16,
                size: 12,
                kind: render3d::compile::VarKind::Float,
            },
        ],
```
and append at the end of the file:
```rust
fn feature_kinds_layout(cache: &Path) -> render3d::compile::CbLayout {
    let dir = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures"));
    let c = compile(
        &dir.join("feature_kinds.hlsl"),
        dir,
        &Default::default(),
        "ps_5_0",
        cache,
        "h",
    )
    .unwrap();
    c.reflection
        .cbuffers
        .into_iter()
        .find(|l| l.name.ends_with("FeatureData"))
        .unwrap()
}

/// C6: every reflected variable carries its HLSL scalar type, struct members included; a struct is `Other`.
#[test]
fn reflection_reports_member_kinds() {
    use render3d::compile::VarKind;
    let cache = tempfile::tempdir().unwrap();
    let l = feature_kinds_layout(cache.path());
    assert_eq!(l.slot, 6);
    let kind = |n: &str| l.vars.iter().find(|v| v.name.ends_with(n)).map(|v| v.kind);
    assert_eq!(kind("hairSpecularSettings"), Some(VarKind::Other));
    assert_eq!(kind("hairSpecularSettings.Enabled"), Some(VarKind::Uint));
    assert_eq!(kind("hairSpecularSettings.HairGlossiness"), Some(VarKind::Float));
    assert_eq!(kind("hairSpecularSettings.Signed"), Some(VarKind::Int));
    assert_eq!(kind("hairSpecularSettings.Flag"), Some(VarKind::Bool));
    assert_eq!(kind("skinParams"), Some(VarKind::Float));
    let w = render3d::cbuffer::CbWriter::new(&l);
    assert_eq!(w.kind("hairSpecularSettings.Enabled"), Some(VarKind::Uint));
    assert_eq!(w.kind("notInThisBuild"), None);
}
```

- [ ] **Step 2: Run — expect a compile failure.**
`cargo test -p render3d --test compile -- reflection_reports_member_kinds`
Expected: `error[E0433]`/`E0412` — `render3d::compile::VarKind` / `CbVar` not found, `no field name on type (String, u32, u32)`.

- [ ] **Step 3: Implement.**

`crates/render3d/src/compile.rs` — the Direct3D import, anchor:
```rust
    D3D_SRV_DIMENSION_TEXTURE2DARRAY, D3D_SRV_DIMENSION_TEXTURECUBE, D3D_SVC_STRUCT,
};
```
becomes
```rust
    D3D_SRV_DIMENSION_TEXTURE2DARRAY, D3D_SRV_DIMENSION_TEXTURECUBE, D3D_SVC_STRUCT, D3D_SVT_BOOL,
    D3D_SVT_FLOAT, D3D_SVT_INT, D3D_SVT_UINT,
};
```

Current `CbLayout` (quoted):
```rust
    /// (name, offset, size) in bytes. Struct members are listed after their struct as `struct.member` (nested:
    /// `a.b.c`), with the absolute offset and the bytes up to the next member, so `CbWriter` can set them.
    pub vars: Vec<(String, u32, u32)>,
}
```
becomes
```rust
    /// Every variable: name, absolute offset and size in bytes, scalar type. Struct members are listed after
    /// their struct as `struct.member` (nested: `a.b.c`), sized up to the next member, so `CbWriter` can set them.
    pub vars: Vec<CbVar>,
}

/// The HLSL scalar type of a cbuffer variable (`D3D_SVT_*` of its type; vectors, matrices and arrays report their
/// element's). Structs and anything else are `Other`. Decides how `cs_settings::write_feature_data` encodes a
/// number (C6): the JSON shape (`1` vs `1.0`) is not trusted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum VarKind {
    Float,
    Uint,
    Int,
    Bool,
    Other,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CbVar {
    pub name: String,
    pub offset: u32,
    pub size: u32,
    pub kind: VarKind,
}
```

`REFLECT_VERSION`, current:
```rust
/// Bumped when the cached `Reflection` gains information (2: struct members flattened), so old entries miss.
const REFLECT_VERSION: u32 = 2;
```
becomes
```rust
/// Bumped when the cached `Reflection` gains information (2: struct members flattened; 3: variable types), so
/// old entries miss.
const REFLECT_VERSION: u32 = 3;
```

`struct_members`, current (quoted, whole):
```rust
unsafe fn struct_members(
    t: &ID3D11ShaderReflectionType,
    prefix: &str,
    base: u32,
    size: u32,
    out: &mut Vec<(String, u32, u32)>,
) {
    let mut td = D3D11_SHADER_TYPE_DESC::default();
    // SAFETY: (caller) live type; `td` is a local out-param; indices are bounded by `Members`.
    unsafe {
        if t.GetDesc(&mut td).is_err() || td.Class != D3D_SVC_STRUCT || td.Elements > 0 {
            return;
        }
        let mut members = vec![];
        for i in 0..td.Members {
            let (Some(mt), name) = (t.GetMemberTypeByIndex(i), pcstr(t.GetMemberTypeName(i)))
            else {
                continue;
            };
            let mut md = D3D11_SHADER_TYPE_DESC::default();
            if mt.GetDesc(&mut md).is_ok() {
                members.push((name, md.Offset, mt));
            }
        }
        for (i, (name, off, mt)) in members.iter().enumerate() {
            let end = members.get(i + 1).map_or(size, |m| m.1);
            let full = format!("{prefix}.{name}");
            let msize = end.saturating_sub(*off);
            out.push((full.clone(), base + off, msize));
            struct_members(mt, &full, base + off, msize, out);
        }
    }
}
```
becomes
```rust
unsafe fn struct_members(
    t: &ID3D11ShaderReflectionType,
    prefix: &str,
    base: u32,
    size: u32,
    out: &mut Vec<CbVar>,
) {
    let mut td = D3D11_SHADER_TYPE_DESC::default();
    // SAFETY: (caller) live type; `td` is a local out-param; indices are bounded by `Members`.
    unsafe {
        if t.GetDesc(&mut td).is_err() || td.Class != D3D_SVC_STRUCT || td.Elements > 0 {
            return;
        }
        let mut members = vec![];
        for i in 0..td.Members {
            let (Some(mt), name) = (t.GetMemberTypeByIndex(i), pcstr(t.GetMemberTypeName(i)))
            else {
                continue;
            };
            let mut md = D3D11_SHADER_TYPE_DESC::default();
            if mt.GetDesc(&mut md).is_ok() {
                members.push((name, md.Offset, var_kind(&md), mt));
            }
        }
        for (i, (name, off, kind, mt)) in members.iter().enumerate() {
            let end = members.get(i + 1).map_or(size, |m| m.1);
            let full = format!("{prefix}.{name}");
            let msize = end.saturating_sub(*off);
            out.push(CbVar {
                name: full.clone(),
                offset: base + off,
                size: msize,
                kind: *kind,
            });
            struct_members(mt, &full, base + off, msize, out);
        }
    }
}

/// The scalar type of a reflected type (see [`VarKind`]).
fn var_kind(t: &D3D11_SHADER_TYPE_DESC) -> VarKind {
    match t.Type {
        D3D_SVT_FLOAT => VarKind::Float,
        D3D_SVT_UINT => VarKind::Uint,
        D3D_SVT_INT => VarKind::Int,
        D3D_SVT_BOOL => VarKind::Bool,
        _ => VarKind::Other,
    }
}
```

In `reflect`, current:
```rust
                let name = pcstr(vd.Name);
                vars.push((name.clone(), vd.StartOffset, vd.Size));
                if let Some(t) = v.GetType() {
                    struct_members(&t, &name, vd.StartOffset, vd.Size, &mut vars);
                }
```
becomes
```rust
                let name = pcstr(vd.Name);
                let t = v.GetType();
                let mut td = D3D11_SHADER_TYPE_DESC::default();
                let kind = match &t {
                    Some(t) if t.GetDesc(&mut td).is_ok() => var_kind(&td),
                    _ => VarKind::Other,
                };
                vars.push(CbVar {
                    name: name.clone(),
                    offset: vd.StartOffset,
                    size: vd.Size,
                    kind,
                });
                if let Some(t) = t {
                    struct_members(&t, &name, vd.StartOffset, vd.Size, &mut vars);
                }
```
(If rustc rejects the `&mut td` borrow in the match guard, use instead:
`let mut kind = VarKind::Other; if let Some(t) = &t { if t.GetDesc(&mut td).is_ok() { kind = var_kind(&td); } }`.)

`crates/render3d/src/cbuffer.rs`:
- `use crate::compile::CbLayout;` → `use crate::compile::{CbLayout, CbVar, VarKind};`
- in `pub struct CbWriter`, `    vars: Vec<(String, u32, u32)>,` → `    vars: Vec<CbVar>,`
- current `find`:
  ```rust
      fn find(&self, var: &str) -> Option<(usize, usize)> {
          self.vars
              .iter()
              .find(|v| name_matches(&v.0, var))
              .map(|v| (v.1 as usize, v.2 as usize))
      }
  ```
  becomes
  ```rust
      fn find(&self, var: &str) -> Option<&CbVar> {
          self.vars.iter().find(|v| name_matches(&v.name, var))
      }
  ```
- current `resolve` body:
  ```rust
          self.find(var).map(|(off, size)| Field {
              off: off as u32,
              size: size as u32,
          })
      }
  ```
  becomes (and add `kind` right after `resolve`)
  ```rust
          self.find(var).map(|v| Field {
              off: v.offset,
              size: v.size,
          })
      }

      /// The reflected scalar type of `var` (namespace-tolerant), for writers that hold raw numbers.
      pub fn kind(&self, var: &str) -> Option<VarKind> {
          self.find(var).map(|v| v.kind)
      }
  ```

`crates/render3d/src/scene.rs` unit test `world_rows_pack_row_major_camera_relative`, current:
```rust
        use crate::compile::CbLayout;
        let layout = CbLayout {
            name: "PerGeometry".into(),
            slot: 2,
            size: 96,
            vars: vec![("World".into(), 0, 48), ("PreviousWorld".into(), 48, 48)],
        };
```
becomes
```rust
        use crate::compile::{CbLayout, CbVar, VarKind};
        let var = |name: &str, offset| CbVar {
            name: name.into(),
            offset,
            size: 48,
            kind: VarKind::Float,
        };
        let layout = CbLayout {
            name: "PerGeometry".into(),
            slot: 2,
            size: 96,
            vars: vec![var("World", 0), var("PreviousWorld", 48)],
        };
```

`crates/render3d/tests/frame.rs`:
- `use render3d::compile::CbLayout;` → `use render3d::compile::{CbLayout, CbVar, VarKind};`
- in `fn layout`, current:
  ```rust
          vars: names
              .iter()
              .enumerate()
              .map(|(i, n)| (n.clone(), size * i as u32, size))
              .collect(),
  ```
  becomes
  ```rust
          vars: names
              .iter()
              .enumerate()
              .map(|(i, n)| CbVar {
                  name: n.clone(),
                  offset: size * i as u32,
                  size,
                  kind: if FEATURE_UINTS.iter().any(|(u, _)| n.ends_with(u)) {
                      VarKind::Uint
                  } else {
                      VarKind::Float
                  },
              })
              .collect(),
  ```

`crates/render3d/tests/material.rs`:
- `use render3d::compile::CbLayout;` → `use render3d::compile::{CbLayout, CbVar, VarKind};`
- in `fn writer`, `        .map(|(i, (n, s))| (n.to_string(), 16 * i as u32, *s))` becomes
  ```rust
          .map(|(i, (n, s))| CbVar {
              name: n.to_string(),
              offset: 16 * i as u32,
              size: *s,
              kind: VarKind::Float,
          })
  ```

- [ ] **Step 4: Run — expect pass.**
`cargo test -p render3d --test compile` — expected: all pass, including `reflection_reports_member_kinds`,
`tiny_shader_compiles_and_reflects`, `cbuffer_writer_sets_by_name`.
`cargo test -p render3d --test frame --test material` and `cargo test -p render3d --lib` — expected: pass (layout shape only).

- [ ] **Step 5: Gates.**
`cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`

- [ ] **Step 6: Commit.**
```bash
git add crates/render3d/src/compile.rs crates/render3d/src/cbuffer.rs crates/render3d/src/scene.rs crates/render3d/tests/compile.rs crates/render3d/tests/frame.rs crates/render3d/tests/material.rs crates/render3d/tests/fixtures/feature_kinds.hlsl
git commit -m "render3d: reflect cbuffer variable types (VarKind, CbVar, REFLECT_VERSION 3) (P1-C Task 2a)" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

#### 2b — FeatureData from the settings

Skin packing (C5) — each component's source key from how the installed CS reads it
(`<CS mod>\Shaders\Lighting.hlsl`, `Skin\Skin.hlsli`, owner install 2026-10-03, read-only), checked against
FEATURE_FLOATS; this corrects the design's guess `physicalParams.w = BaseColorMultiplier`:

| Field | x | y | z | w |
|---|---|---|---|---|
| `skinParams` (0.7, 0.35, 1, 1) | `SkinMainRoughness` (:1969) | `SkinSecondRoughness` (:1970) | `SkinSpecularTexMultiplier` (:1969 `x − z·Glossiness`) | `"Advanced Skin".EnableSkin` (:1365 `skinParams.w > 0` = skin on) |
| `skinParams2` (0.15, 0, 0.0278, 1) | `SecondarySpecularStrength` (:1977) | `"Advanced Skin".ExtraSkinWetness` (:1666, :2190) | `F0` (:1979) | `BaseColorMultiplier` (:1555) |
| `skinDetailParams` (20, 2, 0.25, 0) | `SkinDetailTiling × BodyTilingMultiplier` (:1673; capture 20 = 10 × 2) | `BodyTilingMultiplier` (:1663 wet UV) | `SkinDetailStrength` (:1688) | `EnableSkinDetail` (:1682; User false → 0) |
| `sssParams` (0.1, 0.2, 1, 1) | `Translucency` (Skin.hlsli:166) | `sssWidth` (Skin.hlsli:162) | `SSSAmount` (:3159) | `UseSSS` (Skin.hlsli:166) |
| `fuzzParams` (1, 0.35, 0.045, 0.25) | `FuzzStrength` (:1982) | `FuzzRoughness` (:1983) | `FuzzF0` (:1984) | `ExtraEdgeRoughness` (:1968) |
| `physicalParams` (1.3, 0.75, 1, 1) | `PhysicalMainRoughnessMultiplier` (:1972) | `PhysicalSecondRoughnessMultiplier` (:1973) | `PhysicalSpecularStrength` (:1979) | `WetResponse` (:2216) |
| `wetParams` (512, 0.7, 10, 4) | `"Advanced Skin".WetParams` (4-float array) | | | |

A component whose key the files lack keeps its FEATURE_FLOATS value. C2: every `skinData.<f>` is emitted again as
`skinPerGeometryProfile.<f>` (the PS reads `SkinPerGeometry` b7, Skin.hlsli:21-31), plus `skinPerGeometry = 0` and
`skinMaterialFlags = 0`. Zero `skinPerGeometry` is neutral: `GetWetness` returns 0 when `.x == 0 && .y == 0`
(Skin.hlsli:254), so no neutral value is needed.

- [ ] **Step 1: Write the failing tests.**

In `crates/render3d/tests/cs_settings.rs`, the `use` line
```rust
use render3d::cs_settings::{preview_defines, read_settings, CsSettings, SettingsPaths};
```
becomes
```rust
use render3d::cs_settings::{
    diffuse_curve, feature_data, preview_defines, read_settings, CsSettings, SettingsPaths,
};
use render3d::scene::{FEATURE_FLOATS, FEATURE_UINTS};
```
In `settings_overlay_user_over_default_over_builtin`, after the line
```rust
    assert!(!s.parse_failed && s.notes.is_empty(), "T-S1 notes: {:?}", s.notes);
```
insert
```rust
    let fd = feature_data(&s);
    assert_eq!(value(&fd, "linearLightingSettings.glowmapMult"), [0.5]);
    assert_eq!(value(&fd, "linearLightingSettings.ambientMult"), [2.0]);
    assert_eq!(
        value(&fd, "linearLightingSettings.directionalLightMult"),
        [1.0],
        "built-in (capture 11178) when neither file has it"
    );
```
Append at the end of the file:
```rust
fn value<'a>(fd: &'a [(String, Vec<f64>)], name: &str) -> &'a [f64] {
    &fd.iter()
        .find(|(n, _)| n == name)
        .unwrap_or_else(|| panic!("{name} missing"))
        .1
}

fn bits(v: &[f64]) -> Vec<u32> {
    v.iter().map(|x| (*x as f32).to_bits()).collect()
}

/// Every FEATURE_FLOATS / FEATURE_UINTS value, bit-exact as written to the cbuffer.
fn assert_constants(fd: &[(String, Vec<f64>)]) {
    for (n, v) in FEATURE_FLOATS {
        let want: Vec<u32> = v.iter().map(|x| x.to_bits()).collect();
        assert_eq!(bits(value(fd, n)), want, "{n}");
    }
    for (n, u) in FEATURE_UINTS {
        assert_eq!(value(fd, n), [f64::from(*u)], "{n}");
    }
}

/// T-S3 (C4): the derived diffuseCurve from the owner's diffuseGamma / mid / white reflectance.
#[test]
fn diffuse_curve_derivation() {
    let c = diffuse_curve(2.200000047683716, 0.28717461228370667, 1.0);
    assert!((c - 1.448_227_7).abs() < 1e-6, "{c}");
}

/// T-S3: the owner-shaped files reproduce every capture-11178 value; pointLightMult stays π although the file says 1.
#[test]
fn owner_settings_reproduce_capture_values() {
    let s = read_settings(&paths(true, true, true, None));
    assert_eq!(
        s.merged["Linear Lighting"]["pointLightMult"].as_f64(),
        Some(1.0)
    );
    let fd = feature_data(&s);
    assert_constants(&fd);
    assert_eq!(
        bits(value(&fd, "linearLightingSettings.pointLightMult")),
        [std::f32::consts::PI.to_bits()]
    );
}

/// No files: exactly today's constants, plus the C2 per-geometry copy and zeros.
#[test]
fn default_settings_reproduce_todays_constants() {
    let fd = feature_data(&CsSettings::default());
    assert_constants(&fd);
    for f in [
        "skinParams",
        "skinParams2",
        "skinDetailParams",
        "sssParams",
        "fuzzParams",
        "physicalParams",
        "wetParams",
    ] {
        assert_eq!(
            value(&fd, &format!("skinPerGeometryProfile.{f}")),
            value(&fd, &format!("skinData.{f}")),
            "{f}"
        );
    }
    assert_eq!(value(&fd, "skinPerGeometry"), [0.0; 4]);
    assert_eq!(value(&fd, "skinMaterialFlags"), [0.0; 4]);
    assert_eq!(value(&fd, "linearLightingSettings.isMainOrLoadingMenu"), [0.0]);
}

/// C5: section → struct prefix with the renamed keys; skin packing from the profile.
#[test]
fn owner_section_keys_map_to_struct_fields() {
    let fd = feature_data(&read_settings(&paths(true, true, false, None)));
    assert_eq!(value(&fd, "extendedTranslucencySettings.MaterialModel"), [3.0]);
    assert_eq!(
        bits(value(&fd, "extendedTranslucencySettings.Reduction")),
        [0.15f32.to_bits()]
    );
    assert_eq!(value(&fd, "extendedTranslucencySettings.Strength"), [0.0]);
    assert_eq!(value(&fd, "extendedMaterialSettings.EnableTerrainParallax"), [0.0]);
    assert_eq!(value(&fd, "extendedMaterialSettings.EnableParallax"), [1.0]);
    assert!(!fd.iter().any(|(n, _)| n.ends_with("AlphaMode")));
    assert_eq!(value(&fd, "skinData.skinDetailParams"), [20.0, 2.0, 0.25, 0.0]);
    // Default alone has EnableSkinDetail true: detail on.
    let d = feature_data(&read_settings(&paths(false, true, false, None)));
    assert_eq!(value(&d, "skinData.skinDetailParams")[3], 1.0);
}
```
In `crates/render3d/tests/compile.rs` append:
```rust
/// C6, read back from the writer's bytes: a `uint` member gets the integer's bit pattern (1, not 0x3F800000), a
/// `float` member the f32's, `int` two's complement, `bool` 1; a name the layout lacks is skipped.
#[test]
fn feature_data_writes_reflected_kinds() {
    let cache = tempfile::tempdir().unwrap();
    let l = feature_kinds_layout(cache.path());
    let mut w = render3d::cbuffer::CbWriter::new(&l);
    render3d::cs_settings::write_feature_data(
        &mut w,
        &[
            ("hairSpecularSettings.Enabled".into(), vec![1.0]),
            ("hairSpecularSettings.HairGlossiness".into(), vec![70.0]),
            ("hairSpecularSettings.Signed".into(), vec![-2.0]),
            ("hairSpecularSettings.Flag".into(), vec![1.0]),
            ("skinParams".into(), vec![0.7, 0.35, 1.0, 1.0]),
            ("notInThisBuild".into(), vec![5.0]),
        ],
    );
    let word = |n: &str, i: usize| {
        let at = w.resolve(n).unwrap().off as usize + 4 * i;
        u32::from_le_bytes(w.bytes()[at..at + 4].try_into().unwrap())
    };
    assert_eq!(word("hairSpecularSettings.Enabled", 0), 1);
    assert_eq!(word("hairSpecularSettings.HairGlossiness", 0), 70.0f32.to_bits());
    assert_eq!(word("hairSpecularSettings.Signed", 0), (-2i32) as u32);
    assert_eq!(word("hairSpecularSettings.Flag", 0), 1);
    assert_eq!(word("skinParams", 0), 0.7f32.to_bits());
    assert_eq!(word("skinParams", 3), 1.0f32.to_bits());
}
```

- [ ] **Step 2: Run — expect a compile failure.**
`cargo test -p render3d --test cs_settings -- diffuse_curve_derivation`
Expected: `error[E0432]: unresolved imports render3d::cs_settings::diffuse_curve, render3d::cs_settings::feature_data`.
`cargo test -p render3d --test compile -- feature_data_writes_reflected_kinds`
Expected: `error[E0425]: cannot find function write_feature_data in module render3d::cs_settings`.

- [ ] **Step 3: Implement.**

`crates/render3d/src/cs_settings.rs` — anchor `use serde_json::{Map, Value};` becomes
```rust
use crate::cbuffer::CbWriter;
use crate::compile::VarKind;
use crate::scene::{FEATURE_FLOATS, FEATURE_UINTS};
use serde_json::{Map, Value};
```
Append at the end of the file:
```rust
/// JSON section → FeatureData struct prefix, with the keys whose struct field is named differently (C5).
const SECTIONS: &[(&str, &str, &[(&str, &str)])] = &[
    ("Linear Lighting", "linearLightingSettings", &[]),
    ("Hair Specular", "hairSpecularSettings", &[]),
    ("Vanilla Fresnel", "vanillaFresnelSettings", &[]),
    ("True PBR", "truePBRSettings", &[]),
    (
        "Extended Materials",
        "extendedMaterialSettings",
        &[("EnableTerrain", "EnableTerrainParallax")],
    ),
    (
        "Extended Translucency",
        "extendedTranslucencySettings",
        &[
            ("AlphaMode", "MaterialModel"),
            ("AlphaReduction", "Reduction"),
            ("AlphaSoftness", "Softness"),
            ("AlphaStrength", "Strength"),
        ],
    ),
];

/// Linear Lighting's `diffuseCurve` (C4, no settings key): the inverse of CS's DiffuseToWorking curve at mid
/// grey. `x = 0.5^gamma; mid·(1−x) / (x·(white−mid))`.
pub fn diffuse_curve(gamma: f64, mid: f64, white: f64) -> f64 {
    let x = 0.5f64.powf(gamma);
    mid * (1.0 - x) / (x * (white - mid))
}

/// FeatureData values by cbuffer name, raw numbers (the writer types them from the reflection): the
/// capture-11178 constants, overwritten by every number the settings have (C7: a missing file or key keeps the
/// constant). Names this CS build lacks are dropped at write time.
pub fn feature_data(s: &CsSettings) -> Vec<(String, Vec<f64>)> {
    let mut out: Vec<(String, Vec<f64>)> = FEATURE_FLOATS
        .iter()
        .map(|(n, v)| (n.to_string(), v.iter().map(|x| f64::from(*x)).collect()))
        .chain(
            FEATURE_UINTS
                .iter()
                .map(|(n, u)| (n.to_string(), vec![f64::from(*u)])),
        )
        .collect();
    for (section, prefix, renames) in SECTIONS {
        let Some(Value::Object(m)) = s.merged.get(*section) else {
            continue;
        };
        for (key, v) in m {
            let field = renames
                .iter()
                .find(|r| r.0 == key.as_str())
                .map_or(key.as_str(), |r| r.1);
            let vals: Vec<f64> = match v {
                Value::Array(a) => a.iter().filter_map(num).collect(),
                v => num(v).into_iter().collect(),
            };
            if !vals.is_empty() {
                put(&mut out, &format!("{prefix}.{field}"), vals);
            }
        }
    }
    let ll = |k: &str| setting(&s.merged, "Linear Lighting", k);
    if let (Some(gamma), Some(mid)) = (ll("diffuseGamma"), ll("diffuseMidReflectance")) {
        let white = ll("diffuseWhiteReflectance").unwrap_or(1.0);
        put(
            &mut out,
            "linearLightingSettings.diffuseCurve",
            vec![diffuse_curve(gamma, mid, white)],
        );
    }
    // C4 / spec §2.2: the installed CS uploads π whatever the setting says (capture 11178).
    put(
        &mut out,
        "linearLightingSettings.pointLightMult",
        vec![f64::from(std::f32::consts::PI)],
    );
    put(&mut out, "linearLightingSettings.isMainOrLoadingMenu", vec![0.0]);
    if let Some(Value::Object(skin)) = s.merged.get("Advanced Skin") {
        pack_skin(skin, &mut out);
    }
    // C2: the PS reads the profile from `SkinPerGeometry` (b7), not FeatureData (Skin.hlsli:21-31). No skin
    // extra / wetness / sk textures in the preview: flags 0; per-geometry wetness 0 (Skin.hlsli:254 returns 0).
    let profile: Vec<(String, Vec<f64>)> = out
        .iter()
        .filter_map(|(n, v)| {
            n.strip_prefix("skinData.")
                .map(|f| (format!("skinPerGeometryProfile.{f}"), v.clone()))
        })
        .collect();
    out.extend(profile);
    out.push(("skinPerGeometry".into(), vec![0.0; 4]));
    out.push(("skinMaterialFlags".into(), vec![0.0; 4]));
    out
}

fn put(out: &mut Vec<(String, Vec<f64>)>, name: &str, vals: Vec<f64>) {
    match out.iter_mut().find(|(n, _)| n == name) {
        Some(slot) => slot.1 = vals,
        None => out.push((name.to_string(), vals)),
    }
}

/// `"Advanced Skin"` → `skinData.*` (C5). Source keys follow how the installed CS Lighting.hlsl / Skin.hlsli read
/// each component (line refs: CS 2026-10-03); a key the files lack keeps the capture-11178 component.
fn pack_skin(skin: &Map<String, Value>, out: &mut [(String, Vec<f64>)]) {
    let profile = skin.get("DefaultProfile");
    let p = |k: &str| profile.and_then(|d| d.get(k)).and_then(num);
    let top = |k: &str| skin.get(k).and_then(num);
    // shortcut: tiling × body multiplier is inferred from the captured 20 = 10 × 2 (the shader only reads the
    // product, :1673); revisit if a capture with BodyTilingMultiplier ≠ 2 disagrees.
    let tiling = p("SkinDetailTiling")
        .zip(p("BodyTilingMultiplier"))
        .map(|(t, b)| t * b);
    let packed = [
        // x roughness, y secondary roughness (:1969-1970), z × glossiness (:1969), w skin on (:1365).
        (
            "skinParams",
            [
                p("SkinMainRoughness"),
                p("SkinSecondRoughness"),
                p("SkinSpecularTexMultiplier"),
                top("EnableSkin"),
            ],
        ),
        // x secondary specular (:1977), y extra wetness (:1666, :2190), z F0 (:1979), w base colour × (:1555).
        (
            "skinParams2",
            [
                p("SecondarySpecularStrength"),
                top("ExtraSkinWetness"),
                p("F0"),
                p("BaseColorMultiplier"),
            ],
        ),
        // x detail UV tiling (:1673), y wet UV tiling (:1663), z detail strength (:1688), w detail on (:1682).
        (
            "skinDetailParams",
            [
                tiling,
                p("BodyTilingMultiplier"),
                p("SkinDetailStrength"),
                p("EnableSkinDetail"),
            ],
        ),
        // x·w translucency (Skin.hlsli:166), y width (Skin.hlsli:162), z amount (:3159).
        (
            "sssParams",
            [p("Translucency"), p("sssWidth"), p("SSSAmount"), p("UseSSS")],
        ),
        // x weight (:1982), y roughness (:1983), z colour (:1984), w edge roughness (:1968).
        (
            "fuzzParams",
            [
                p("FuzzStrength"),
                p("FuzzRoughness"),
                p("FuzzF0"),
                p("ExtraEdgeRoughness"),
            ],
        ),
        // x, y roughness multipliers (:1972-1973), z specular with a skin-extra map (:1979), w wet response (:2216).
        (
            "physicalParams",
            [
                p("PhysicalMainRoughnessMultiplier"),
                p("PhysicalSecondRoughnessMultiplier"),
                p("PhysicalSpecularStrength"),
                p("WetResponse"),
            ],
        ),
    ];
    for (field, comps) in packed {
        let name = format!("skinData.{field}");
        if let Some((_, v)) = out.iter_mut().find(|(n, _)| *n == name) {
            for (dst, c) in v.iter_mut().zip(comps) {
                if let Some(c) = c {
                    *dst = c;
                }
            }
        }
    }
    if let Some(Value::Array(a)) = skin.get("WetParams") {
        let w: Vec<f64> = a.iter().filter_map(num).collect();
        if w.len() == 4 {
            if let Some(slot) = out.iter_mut().find(|(n, _)| n == "skinData.wetParams") {
                slot.1 = w;
            }
        }
    }
}

/// Writes `values` by name, each as its reflected type (C6): float (and unknown) fields as f32, uint and bool
/// fields as u32, int fields as i32 bits. Names this build's layout lacks are skipped.
pub fn write_feature_data(w: &mut CbWriter, values: &[(String, Vec<f64>)]) {
    for (name, v) in values {
        match w.kind(name) {
            None => {}
            Some(VarKind::Float | VarKind::Other) => {
                let f: Vec<f32> = v.iter().map(|x| *x as f32).collect();
                w.set(name, &f);
            }
            Some(VarKind::Uint | VarKind::Bool) => {
                let u: Vec<u32> = v.iter().map(|x| *x as u32).collect();
                w.set_u32(name, &u);
            }
            Some(VarKind::Int) => {
                let u: Vec<u32> = v.iter().map(|x| *x as i32 as u32).collect();
                w.set_u32(name, &u);
            }
        }
    }
}
```
(`pack_skin` takes `&mut [..]`: it only edits existing entries, so `wetParams` is set in place rather than via `put`.)

`crates/render3d/src/scene.rs`:
- after `use crate::cbuffer::{CbWriter, Field};` add `use crate::cs_settings::write_feature_data;`
- doc relabel (C7), replace
  ```rust
  /// These are the owner's CS SettingsUser.json / frame-11178 weather values; P1 reads them per user.
  ```
  with
  ```rust
  /// CS 1.9 values observed in capture 11178: the fallback for every setting the user's files lack (C7,
  /// [`crate::cs_settings::feature_data`]).
  ```
  and replace
  ```rust
  /// Owner's CS SettingsUser.json / frame-11178 values (see [`FEATURE_FLOATS`]); P1 reads them per user.
  ```
  with
  ```rust
  /// CS 1.9 values observed in capture 11178 (see [`FEATURE_FLOATS`]).
  ```
- `Scene::write`, current signature lines:
  ```rust
      /// Writes the scene and camera into one cbuffer (any stage, any slot); fields the layout lacks are
      /// skipped. `model` is the shape's `Mesh.transform`. Returns the names this writer accepted.
      pub fn write(&self, w: &mut CbWriter, v: &View, model: &Mat3x4) -> Vec<&'static str> {
  ```
  become
  ```rust
      /// Writes the scene and camera into one cbuffer (any stage, any slot); fields the layout lacks are
      /// skipped. `model` is the shape's `Mesh.transform`; `feature_values` the FeatureData values
      /// ([`crate::cs_settings::feature_data`]). Returns the names this writer accepted.
      pub fn write(
          &self,
          w: &mut CbWriter,
          v: &View,
          model: &Mat3x4,
          feature_values: &[(String, Vec<f64>)],
      ) -> Vec<&'static str> {
  ```
  and its tail, current:
  ```rust
          if w.set_u32("InInterior", &[1]) {
              ok.push("InInterior");
          }
          for (n, d) in FEATURE_FLOATS {
              w.set(n, d);
          }
          for (n, d) in FEATURE_UINTS {
              w.set_u32(n, &[*d]);
          }
          ok
  ```
  becomes
  ```rust
          if w.set_u32("InInterior", &[1]) {
              ok.push("InInterior");
          }
          write_feature_data(w, feature_values);
          ok
  ```
- `write_static`, current:
  ```rust
  pub fn write_static(w: &mut CbWriter) {
  ```
  becomes
  ```rust
  pub fn write_static(w: &mut CbWriter, feature_values: &[(String, Vec<f64>)]) {
  ```
  and its tail, current:
  ```rust
      w.set_u32("InInterior", &[1]);
      for (n, d) in FEATURE_FLOATS {
          w.set(n, d);
      }
      for (n, d) in FEATURE_UINTS {
          w.set_u32(n, &[*d]);
      }
  }
  ```
  becomes
  ```rust
      w.set_u32("InInterior", &[1]);
      write_feature_data(w, feature_values);
  }
  ```
- unit test `world_rows_pack_row_major_camera_relative`: `        Scene::neutral().write(&mut w, &v, &model);` →
  `        Scene::neutral().write(&mut w, &v, &model, &[]);`

`crates/render3d/src/draw.rs`:
- current
  ```rust
      pub fn update_by_name(&mut self, gpu: &Gpu, scene: &Scene, view: &View) -> usize {
          let mut uploads = 0;
          for cb in self.vs_cbs.iter_mut().chain(self.ps_cbs.iter_mut()) {
              scene.write(&mut cb.writer, view, &self.model);
  ```
  becomes
  ```rust
      pub fn update_by_name(
          &mut self,
          gpu: &Gpu,
          scene: &Scene,
          view: &View,
          feature_values: &[(String, Vec<f64>)],
      ) -> usize {
          let mut uploads = 0;
          for cb in self.vs_cbs.iter_mut().chain(self.ps_cbs.iter_mut()) {
              scene.write(&mut cb.writer, view, &self.model, feature_values);
  ```
- current
  ```rust
      pub fn rewrite_material(&mut self, m: &MaterialState, linear_lighting: bool) {
          for cb in self.vs_cbs.iter_mut().chain(self.ps_cbs.iter_mut()) {
              cb.writer.zero();
              write_material(&mut cb.writer, m, &self.bound, linear_lighting);
              write_static(&mut cb.writer);
  ```
  becomes
  ```rust
      pub fn rewrite_material(
          &mut self,
          m: &MaterialState,
          linear_lighting: bool,
          feature_values: &[(String, Vec<f64>)],
      ) {
          for cb in self.vs_cbs.iter_mut().chain(self.ps_cbs.iter_mut()) {
              cb.writer.zero();
              write_material(&mut cb.writer, m, &self.bound, linear_lighting);
              write_static(&mut cb.writer, feature_values);
  ```

`crates/render3d/src/lib.rs`:
- struct field, anchor:
  ```rust
      /// Test hook: draw with P0's by-name writer (every field, every cbuffer, every frame).
      reference_writer: bool,
  ```
  becomes
  ```rust
      /// Test hook: draw with P0's by-name writer (every field, every cbuffer, every frame).
      reference_writer: bool,
      /// FeatureData values every static block writes ([`cs_settings::feature_data`]); typed at write time from
      /// the reflection. Default: the capture-11178 constants.
      feature_values: Vec<(String, Vec<f64>)>,
  ```
- `with_gpu`, anchor `            reference_writer: false,` becomes
  ```rust
              reference_writer: false,
              feature_values: cs_settings::feature_data(&cs_settings::CsSettings::default()),
  ```
- `rebuild_device_on`, anchor `        n.reference_writer = self.reference_writer;` becomes
  ```rust
          n.reference_writer = self.reference_writer;
          n.feature_values = std::mem::take(&mut self.feature_values);
  ```
- `build_draw`, `            scene::write_static(&mut cb.writer);` → `            scene::write_static(&mut cb.writer, &self.feature_values);`
- `set_material` path, `        d.rewrite_material(m, ll);` → `        d.rewrite_material(m, ll, &self.feature_values);`
- texture-swap loop, `                d.rewrite_material(&material, ll);` → `                d.rewrite_material(&material, ll, &self.feature_values);`
- `render`, `                    s.update_by_name(gpu, scene, &view)` → `                    s.update_by_name(gpu, scene, &view, &self.feature_values)`
  (`d`/`s` borrow `self.shapes` only, so borrowing `self.feature_values` alongside is a disjoint field borrow.)

`crates/render3d/tests/frame.rs`:
- after `use render3d::cbuffer::CbWriter;` add `use render3d::cs_settings::{feature_data, CsSettings};`
- in `resolved_fields_write_same_bytes_as_by_name`, current:
  ```rust
      for l in [layout(&names, 400), layout(&ns, 32)] {
          let mut by_name = CbWriter::new(&l);
          write_material(&mut by_name, &m, &bound, true);
          scene.write(&mut by_name, &view, &model);
          let mut resolved = CbWriter::new(&l);
          write_material(&mut resolved, &m, &bound, true);
          write_static(&mut resolved);
  ```
  becomes
  ```rust
      let fv = feature_data(&CsSettings::default());
      for l in [layout(&names, 400), layout(&ns, 32)] {
          let mut by_name = CbWriter::new(&l);
          write_material(&mut by_name, &m, &bound, true);
          scene.write(&mut by_name, &view, &model, &fv);
          let mut resolved = CbWriter::new(&l);
          write_material(&mut resolved, &m, &bound, true);
          write_static(&mut resolved, &fv);
  ```

- [ ] **Step 4: Run — expect pass.**
`cargo test -p render3d --test cs_settings` — expected: 11 passed (Task 1's seven plus `diffuse_curve_derivation`,
`owner_settings_reproduce_capture_values`, `default_settings_reproduce_todays_constants`,
`owner_section_keys_map_to_struct_fields`).
`cargo test -p render3d --test compile -- feature_data_writes_reflected_kinds` — expected: 1 passed.
`cargo test -p render3d --test frame` — expected: pass (by-name and resolved writers still byte-identical; the
reference-writer pixel test unchanged).
`cargo test -p render3d` — expected: every WARP test passes (stand-in shader has no FeatureData: nothing new written).

- [ ] **Step 5: Gates.**
`cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`

- [ ] **Step 6: Commit.**
```bash
git add crates/render3d/src/cs_settings.rs crates/render3d/src/scene.rs crates/render3d/src/draw.rs crates/render3d/src/lib.rs crates/render3d/tests/cs_settings.rs crates/render3d/tests/compile.rs crates/render3d/tests/frame.rs
git commit -m "render3d: FeatureData from the user's CS settings, typed by reflection; three static writer sites use Renderer.feature_values (P1-C Task 2b)" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: `locate::find_file` and `locate::settings_paths` (C12, T-S4)

**Files**
- Modify: `crates/render3d/src/locate.rs` (add after `find_in_mods`, current lines 56-63)
- Modify: `crates/render3d/tests/locate.rs` (imports at the top, new tests at the end)

**Interfaces**
- Consumes (Part A, Task 1): `render3d::cs_settings::SettingsPaths { pub user: Option<PathBuf>, pub default: Option<PathBuf>, pub info_ini: Option<PathBuf>, pub features_dir: Option<PathBuf> }`
- Produces:
  - `pub fn find_file(origins: &[(String, PathBuf, i32)], rel: &str) -> Option<(String, PathBuf)>` returns (origin name, **full path of the file**). This differs from `find_in_mods`, which returns the origin root.
  - `pub fn settings_paths(origins: Option<&[(String, PathBuf, i32)]>, shaders_root: &Path) -> SettingsPaths`

**How the GUI gets origins today (Part C passes the same tuples).** `gui/src/model/viewport3d.rs`, `locate_shaders`
(lines 1361-1405), builds `origins: Vec<(String, PathBuf, i32)>` in two ways:
- MO2: `modscan::mo2::Instance::load(root, Some(profile)).ok().and_then(|i| modscan::mo2::loose_origins(&i).ok()).map(|(o, _)| o.into_iter().map(|o| (o.name, o.root, o.priority)).collect()).unwrap_or_default()`.
- Folder target: `vec![(name, f.clone(), 0)]`.

It then calls `locate::find_in_mods(&origins)`, and the Shaders root is `root.join("Shaders")`. Priorities come from
`modscan/src/mo2.rs` `loose_origins` (lines 83-124): game `Data` is -1, mods are 0..n (the first modlist line gets
the highest), and `overwrite` is `i32::MAX`. Part C calls:
- `settings_paths(Some(&origins), &shaders_root)` in that function, both for the find-in-mods branch and for the saved-folder fallback.
- `settings_paths(None, &root)` in the early "saved chosen folder" branch and in `choose_folder` (after `chosen_root`), where there are no origins.

An empty origins slice (MO2 failed to load) is treated as `None`.

- [ ] **Step 1: Write the failing tests.** In `crates/render3d/tests/locate.rs`, replace the two import lines
```rust
use render3d::locate::*;
use std::fs;
```
with
```rust
use render3d::locate::*;
use std::fs;
use std::path::{Path, PathBuf};
```
and append at the end of the file:
```rust
const USER: &str = "SKSE/Plugins/CommunityShaders/SettingsUser.json";
const DEFAULT: &str = "SKSE/Plugins/CommunityShaders/SettingsDefault.json";
const INFO: &str = "ShaderCache/Info.ini";

fn put(path: &Path) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, "{}").unwrap();
}

/// An MO2-shaped tree with origins as `modscan::mo2::loose_origins` reports them: game Data at -1, mods 0..,
/// overwrite at `i32::MAX`. SettingsUser.json is in Data, the CS mod and overwrite; SettingsDefault.json only
/// in the CS mod; no Info.ini anywhere. `Other` has no folder on disk.
fn mo2(d: &Path) -> Vec<(String, PathBuf, i32)> {
    let data = d.join("Data");
    let cs_mod = d.join("mods/CommunityShaders");
    let ow = d.join("overwrite");
    cs(&cs_mod, "a");
    for root in [&data, &cs_mod, &ow] {
        put(&root.join(USER));
    }
    put(&cs_mod.join(DEFAULT));
    vec![
        ("Data".into(), data, -1),
        ("CommunityShaders".into(), cs_mod, 0),
        ("Other".into(), d.join("mods/Other"), 1),
        ("overwrite".into(), ow, i32::MAX),
    ]
}

/// 03 T-S4
#[test]
fn mo2_overwrite_wins() {
    let d = tempfile::tempdir().unwrap();
    let o = mo2(d.path());
    assert_eq!(
        find_file(&o, USER),
        Some(("overwrite".into(), d.path().join("overwrite").join(USER)))
    );
    assert_eq!(
        find_file(&o, DEFAULT),
        Some((
            "CommunityShaders".into(),
            d.path().join("mods/CommunityShaders").join(DEFAULT)
        )),
        "a file only in a mod comes from that mod"
    );
    assert_eq!(find_file(&o, INFO), None);
    put(&d.path().join("Data").join(INFO));
    assert_eq!(
        find_file(&o, INFO),
        Some(("Data".into(), d.path().join("Data").join(INFO))),
        "game Data is the last resort"
    );
}

#[test]
fn settings_paths_with_origins_use_them() {
    let d = tempfile::tempdir().unwrap();
    let o = mo2(d.path());
    let shaders = d.path().join("mods/CommunityShaders/Shaders");
    fs::create_dir_all(shaders.join("Features")).unwrap();
    let p = settings_paths(Some(&o), &shaders);
    assert_eq!(p.user, Some(d.path().join("overwrite").join(USER)));
    assert_eq!(
        p.default,
        Some(d.path().join("mods/CommunityShaders").join(DEFAULT))
    );
    assert_eq!(p.info_ini, None);
    assert_eq!(p.features_dir, Some(shaders.join("Features")));
}

/// Folder target or "Choose folder…": the Data folder is the parent of the Shaders root.
#[test]
fn settings_paths_without_origins_look_in_the_parent_of_the_shaders_root() {
    let d = tempfile::tempdir().unwrap();
    let data = d.path().join("Data");
    cs(&data, "a");
    put(&data.join(USER));
    put(&data.join(INFO));
    let p = settings_paths(None, &data.join("Shaders"));
    assert_eq!(p.user, Some(data.join(USER)));
    assert_eq!(p.default, None);
    assert_eq!(p.info_ini, Some(data.join(INFO)));
    assert_eq!(p.features_dir, None, "no Shaders/Features folder");
    // An empty origin list (MO2 failed to load) is the same as none.
    assert_eq!(settings_paths(Some(&[]), &data.join("Shaders")).user, Some(data.join(USER)));
    // A chosen CS mod folder holds no settings (the owner's CS mod has none, 01 §1): nothing found, no error.
    let m = d.path().join("CS");
    cs(&m, "a");
    fs::create_dir_all(m.join("Shaders/Features")).unwrap();
    let p = settings_paths(None, &m.join("Shaders"));
    assert_eq!((p.user, p.default, p.info_ini), (None, None, None));
    assert_eq!(p.features_dir, Some(m.join("Shaders/Features")));
}
```

- [ ] **Step 2: Run the tests and watch them fail.**
`cargo test -p render3d --test locate`
Expected: a compile error, `cannot find function 'find_file' in this scope` (and the same for `settings_paths`).

- [ ] **Step 3: Implement.** In `crates/render3d/src/locate.rs`, replace
```rust
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
```
with
```rust
use crate::cs_settings::SettingsPaths;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
```
Then insert this block directly after the closing `}` of `find_in_mods` (the line after
`.map(|(n, r, _)| (n.clone(), r.clone()))`) and before `#[derive(Debug, Clone, PartialEq)]\npub enum Change`:
```rust

/// Highest-priority origin that has the file `rel` (`/`-separated, relative to the origin root), and that
/// file's full path. Same tuples and tie rule as [`find_in_mods`]: MO2's overwrite (`i32::MAX`) beats every
/// mod, and game Data (-1) loses to all of them.
pub fn find_file(origins: &[(String, PathBuf, i32)], rel: &str) -> Option<(String, PathBuf)> {
    origins
        .iter()
        .map(|(n, root, prio)| (n, root.join(rel), *prio))
        .filter(|(_, f, _)| f.is_file())
        .max_by_key(|(_, _, prio)| *prio)
        .map(|(n, f, _)| (n.clone(), f))
}

const SETTINGS_USER: &str = "SKSE/Plugins/CommunityShaders/SettingsUser.json";
const SETTINGS_DEFAULT: &str = "SKSE/Plugins/CommunityShaders/SettingsDefault.json";
const INFO_INI: &str = "ShaderCache/Info.ini";

/// Where the user's CS settings are (P1-C C12). Through the MO2 origins when there are any (overwrite wins);
/// otherwise in the Data folder, which is the parent of the `Shaders` root (folder target, "Choose folder…").
/// A file that is not there, or a missing `Shaders/Features` folder, is `None`.
pub fn settings_paths(
    origins: Option<&[(String, PathBuf, i32)]>,
    shaders_root: &Path,
) -> SettingsPaths {
    let find = |rel: &str| match origins.filter(|o| !o.is_empty()) {
        Some(o) => find_file(o, rel).map(|(_, f)| f),
        None => shaders_root
            .parent()
            .map(|data| data.join(rel))
            .filter(|f| f.is_file()),
    };
    SettingsPaths {
        user: find(SETTINGS_USER),
        default: find(SETTINGS_DEFAULT),
        info_ini: find(INFO_INI),
        features_dir: Some(shaders_root.join("Features")).filter(|d| d.is_dir()),
    }
}
```

- [ ] **Step 4: Run the tests and watch them pass.**
`cargo test -p render3d --test locate`
Expected: every test passes, including the 3 new ones and the 5 existing ones.

- [ ] **Step 5: Run the gates.**
`cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: clean. `cargo fmt` may rewrap the long `assert_eq!` line in the test, which is fine.

- [ ] **Step 6: Commit.**
```bash
git -C F:/ai/dds/converter-nif-p1c add crates/render3d/src/locate.rs crates/render3d/tests/locate.rs
git -C F:/ai/dds/converter-nif-p1c commit -m "$(cat <<'EOF'
render3d: locate CS settings files through MO2 origins (P1-C C12)

find_file picks the highest-priority origin holding a relative path (overwrite wins);
settings_paths resolves SettingsUser/SettingsDefault/Info.ini and Shaders/Features, falling back to
the parent of the Shaders root when there are no origins (folder target, Choose folder).

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 4: `Renderer::set_settings`, `settings`, `set_features_on`, `off_in_preview`, C3 defaults, rebuild carry (C3, C8, C11, T-S6)

**Files**
- Modify: `crates/render3d/src/lib.rs` (imports, `Renderer` fields, `with_gpu`, `set_features` doc, new methods, `rebuild_device_on`)
- Modify: `crates/render3d/src/textures.rs` (`default_for`, the `technique_defaults` unit test)
- Modify: `crates/render3d/tests/fixtures/standin/Lighting.hlsl` (a new `STANDIN_FEATURE_DATA` hook; without the define the output is unchanged)
- Create: `crates/render3d/tests/settings.rs`

**Interfaces**
- Consumes (Part A):
  - `render3d::cs_settings::{CsSettings (+Default, Clone, PartialEq, Debug), feature_data, preview_defines, PREVIEW_FEATURES}`
  - the field `Renderer.feature_values: Vec<(String, Vec<f64>)>`, initialised in `with_gpu`
  - **assumed** post-Task-2 signature: `ShapeDraw::rewrite_material(&mut self, m: &MaterialState, linear_lighting: bool, feature_values: &[(String, Vec<f64>)])`. If Part A names or orders that parameter differently, only the one call in `rewrite_static` below changes.
- Produces (contract):
  - `pub fn set_settings(&mut self, s: CsSettings)`
  - `pub fn settings(&self) -> &CsSettings`
  - `pub fn set_features_on(&mut self, on: bool)`
  - `pub fn off_in_preview(&self) -> Vec<String>`
  - private `fn begin_all(&mut self)`: the device-wide "every shape through the permutation path"

**How `set_features` and `set_features_on` interact (the rule).** `Renderer.features` stays the one list that
`wanted()` and `set_material`'s `classify` read. `set_features(v)` is unchanged and replaces the whole list; that is
how tests add the `STANDIN_*` hooks. `set_settings` and `set_features_on` replace only the defines the settings
manage, which are `ENABLE_LL` plus every define in `PREVIEW_FEATURES`:
- every other entry is kept, in order;
- then `preview_defines(&settings, features_on)` is appended.

"Defines changed" compares the sorted old list with the sorted new list. This is safe because `program_key`
sorts defines before hashing (`compile.rs` ~95-104), so order never changes a key. The defaults in `with_gpu` are
`features_on: true`, `settings: CsSettings::default()` (LL on) and `features: DEFAULT_FEATURES` (`["ENABLE_LL"]`).
That equals `preview_defines(&CsSettings::default(), true)`, which is P0. `encode_srgb` stays `false` in `with_gpu`,
as today. Only `set_settings` and `set_features_on` couple it (C11). The coupling is `encode_srgb = features contain
"ENABLE_LL"`, which equals `linear_lighting && features_on` whenever `set_features` did not itself add `ENABLE_LL`.

**Applying (C11).** The steps:
- If the defines are unchanged, the static block of every `Drawing` shape is rewritten in place with its current
  material (`rewrite_material`). That is a cbuffer edit, so it shows on the next frame with no compile.
- If the defines changed, every non-hidden shape goes through `begin()` (`begin_all`). New draws are built by
  `build_draw` with the new `feature_values`. A shape whose program is not at hand keeps its old draw **with its
  old constants**, as `Pending { showing_previous: true }`, per P1-B. That is why the frame stays byte-identical
  while pending.
- `Pending` and `Failed` shapes (old draw on screen) are not rewritten in place. The pending ones get the new
  values when their program lands; a failed one keeps the old values until the next edit of that shape.

`begin()` today (`lib.rs` 686-724) is the per-shape primitive the helper loops over:
```rust
    /// Starts building shape `id` for `m` (already stored as its material): swaps now when the program is at
    /// hand, else queues one job (deduplicated) and marks the shape `Pending`. The old draw, if any, stays.
    fn begin(
        &mut self,
        id: ShapeId,
        m: &MaterialState,
        textures: &dyn Fn(&str) -> Option<Vec<u8>>,
        rep: &mut LoadReport,
    ) {
        self.resolve_textures(m, textures, rep);
        let wanted = match self.wanted(m) {
```
`begin_all` passes the resolver `&|_| None`. `texture()` (lib.rs 450-478) returns from `tex_cache` first, and every
slot of a shape's current material went into `tex_cache` at its load or edit. Paths the resolver did not have are
stored too, as an empty entry. So the resolver is never consulted for a slot it was not already asked about.

**Stand-in defines used by the tests, and why.** The stand-in PS `#error`s unless `DO_ALPHA_TEST` or
`STANDIN_OPAQUE_OK` is defined (`Lighting.hlsl`: `#if !defined(DO_ALPHA_TEST) && !defined(STANDIN_OPAQUE_OK)`).
The tests therefore start from `common::FLAT = ["STANDIN_OPAQUE_OK", "STANDIN_FLAT"]` through `set_features`, which
the managed-subset rule keeps. `ENABLE_LL` is not read by the stand-in, so toggling it compiles fine and only
changes the program key. That is exactly what T-S6 needs: a define change whose new programs compile, with no
shader-side pixel change. The stand-in has no FeatureData cbuffer, so a "values only" change cannot show in pixels.
The new hook `STANDIN_FEATURE_DATA` declares one CS-named member, `linearLightingSettings.ambientMult` in
`cbuffer FeatureData : register(b6)`, and scales the flat colour by it. It sits behind its own define, so every
existing permutation compiles to the same code as before. The source hash, and with it every program key, changes,
but every test uses a fresh tempdir cache.

- [ ] **Step 1: Write the failing C3 unit test.** In `crates/render3d/src/textures.rs`, in `mod tests`, replace
```rust
        assert_eq!(default_for(14, 0, false), [255, 255, 255, 255]);
    }
```
with
```rust
        assert_eq!(default_for(14, 0, false), [255, 255, 255, 255]);
        // P1-C C3: CS_SKIN / CS_HAIR registers get neutral 1×1 defaults, whatever the technique.
        assert_eq!(default_for(72, 0, false), [128, 128, 255, 0]); // skin detail normal: flat, mask 0
        assert_eq!(default_for(73, 6, false), [128, 128, 128, 255]); // hair tangent shift: .x - 0.5 ≈ 0
        assert_eq!(default_for(71, 0, false), [0, 0, 0, 0]); // skin extra (gated off by skinMaterialFlags)
        assert_eq!(default_for(74, 0, true), [0, 0, 0, 0]); // skin wetness (gated off)
    }
```

- [ ] **Step 2: Add the stand-in hook.** This is fixture-only and changes nothing without the define. In
`crates/render3d/tests/fixtures/standin/Lighting.hlsl`, replace
```hlsl
//   light, camera-relative like World. Zero lights add exactly nothing.
```
with
```hlsl
//   light, camera-relative like World. Zero lights add exactly nothing.
// - STANDIN_FEATURE_DATA (with STANDIN_FLAT): the flat colour is scaled by CS's FeatureData
//   linearLightingSettings.ambientMult, so a settings change that only rewrites the static cbuffer block shows in
//   pixels (P1-C settings tests).
```
then replace
```hlsl
SamplerState DiffuseSampler : register(s0);
```
with
```hlsl
SamplerState DiffuseSampler : register(s0);

#if defined(STANDIN_FEATURE_DATA)
struct StandinLinearLightingSettings
{
    float ambientMult;
    float3 pad;
};

cbuffer FeatureData : register(b6)
{
    StandinLinearLightingSettings linearLightingSettings;
};
#endif
```
then replace
```hlsl
    return float4(0.25 * base.rgb * i.Color.rgb + EmitColor.rgb + emissive + float3(0.0, flagGreen, 0.0), base.a * 0.5);
```
with
```hlsl
    float3 col = 0.25 * base.rgb * i.Color.rgb + EmitColor.rgb + emissive + float3(0.0, flagGreen, 0.0);
#if defined(STANDIN_FEATURE_DATA)
    col *= linearLightingSettings.ambientMult;
#endif
    return float4(col, base.a * 0.5);
```
(The variable is `col`, not `flat`: `flat` is an HLSL interpolation keyword.)

- [ ] **Step 3: Write the failing integration tests.** Create `crates/render3d/tests/settings.rs`:
```rust
//! P1-C T-S6 and C11: the user's CS settings applied to a loaded model, on WARP with the stand-in shader. The
//! test is the pool, as in async_compile.rs: it takes the jobs and runs them with `run_job`.
mod common;
use common::{cache_files, channel, covered, fixture, renderer, FLAT};
use render3d::cs_settings::CsSettings;
use render3d::device::Gpu;
use render3d::scene::Scene;
use render3d::{parse, run_job, Renderer, ShapeStatus};
use std::path::Path;

/// FLAT plus the stand-in's FeatureData hook: the flat colour × `linearLightingSettings.ambientMult`.
const FEATURE_DATA: &[&str] = &["STANDIN_OPAQUE_OK", "STANDIN_FLAT", "STANDIN_FEATURE_DATA"];
const N: u32 = 64;

fn none(_: &str) -> Option<Vec<u8>> {
    None
}

/// Settings as `read_settings` would give them for a SettingsUser.json with only a "Linear Lighting" section.
/// Both `linear_lighting` and the merged key are set, so the test does not depend on which one
/// `preview_defines` reads.
fn settings(ll: bool, ambient: f64) -> CsSettings {
    let merged = serde_json::json!({
        "Linear Lighting": {"enableLinearLighting": i32::from(ll), "ambientMult": ambient}
    });
    CsSettings {
        hash: format!("test ll={ll} ambient={ambient}"),
        linear_lighting: ll,
        merged: merged.as_object().unwrap().clone(),
        ..CsSettings::default()
    }
}

fn statuses(r: &Renderer) -> Vec<ShapeStatus> {
    r.shapes().into_iter().map(|s| s.status).collect()
}

fn rgb(px: &[u8]) -> u64 {
    channel(px, 0) + channel(px, 1) + channel(px, 2)
}

/// Everything queued so far, compiled on this thread and applied. Returns how many jobs there were.
fn drain(r: &mut Renderer) -> usize {
    let jobs = r.take_jobs();
    let n = jobs.len();
    for j in jobs {
        r.apply_compiled(run_job(j));
    }
    n
}

/// The furniture fixture (2 Lighting shapes) with `features` and settings `s`, async, every program compiled.
fn furniture(cache: &Path, features: &[&str], s: CsSettings) -> Renderer {
    let mut r = renderer(cache, features);
    r.set_async_compile(true);
    r.set_settings(s);
    let rep = r.load_parsed(&parse(&fixture("TestNifFile_Furniture_Col_SE.nif")), &none);
    assert!(
        rep.errors.is_empty() && rep.compile_errors.is_empty(),
        "{rep:?}"
    );
    assert!(drain(&mut r) >= 1);
    assert_eq!(statuses(&r), vec![ShapeStatus::Drawing; 2]);
    r
}

/// 03 T-S6: LL off → on is a define change: every shape goes through the permutation path, old draws on screen.
#[test]
fn settings_change_of_define_goes_through_swap() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = furniture(cache.path(), FLAT, settings(false, 1.0));
    assert_eq!(r.features(), FLAT, "LL off: no ENABLE_LL");
    let scene = Scene::neutral();
    // C11 switches the display encode on with LL at once; what must not change meanwhile is the draws, so the
    // reference frame is the old draws read back with the encode on.
    r.set_encode_srgb(true);
    let before = r.render(&scene, N, N).unwrap();
    assert!(covered(&before) > 0);
    r.set_settings(settings(true, 1.0));
    assert!(r.features().iter().any(|f| f == "ENABLE_LL"));
    assert_eq!(
        statuses(&r),
        vec![
            ShapeStatus::Pending {
                showing_previous: true
            };
            2
        ]
    );
    let jobs = r.take_jobs();
    assert!(!jobs.is_empty(), "the new programs are queued, not compiled inline");
    assert_eq!(
        r.render(&scene, N, N).unwrap(),
        before,
        "the old draws, byte-identical, while the programs compile"
    );
    for j in jobs {
        r.apply_compiled(run_job(j));
    }
    assert_eq!(statuses(&r), vec![ShapeStatus::Drawing; 2]);
    assert!(covered(&r.render(&scene, N, N).unwrap()) > 0);
}

/// C11: same defines, new FeatureData values → a cbuffer edit only: no job, no cache entry, next frame differs.
#[test]
fn a_settings_change_without_define_change_compiles_nothing() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = furniture(cache.path(), FEATURE_DATA, settings(true, 1.0));
    let scene = Scene::neutral();
    let before = r.render(&scene, N, N).unwrap();
    assert_eq!(r.render(&scene, N, N).unwrap(), before);
    assert_eq!(r.last_uploads(), 0, "a steady frame uploads nothing");
    let features = r.features().to_vec();
    let files = cache_files(cache.path());
    r.set_settings(settings(true, 0.5));
    assert_eq!(r.features(), features, "the defines did not change");
    assert!(r.take_jobs().is_empty(), "no compile job");
    assert_eq!(statuses(&r), vec![ShapeStatus::Drawing; 2]);
    let after = r.render(&scene, N, N).unwrap();
    assert!(r.last_uploads() > 0, "the rewritten static block is uploaded");
    assert_eq!(cache_files(cache.path()), files, "nothing compiled");
    assert!(
        rgb(&after) < rgb(&before),
        "ambientMult 1 → 0.5 darkens: {} → {}",
        rgb(&before),
        rgb(&after)
    );
}

/// C8: loaded features (Info.ini) the preview does not compile; TruePBR (settings only) is never listed.
#[test]
fn off_in_preview_lists_loaded_but_uncompiled_features() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = renderer(cache.path(), FLAT);
    let mut s = settings(true, 1.0);
    s.loaded = ["LightLimitFix", "LinearLighting", "TruePBR"]
        .map(String::from)
        .into();
    r.set_features_on(false);
    r.set_settings(s.clone());
    assert_eq!(r.off_in_preview(), ["LightLimitFix", "LinearLighting"]);
    r.set_features_on(true);
    assert_eq!(r.off_in_preview(), ["LightLimitFix"]);
    assert_eq!(r.settings(), &s);
}

/// C11: a device rebuild keeps the settings, the toggle, the FeatureData values and the sRGB mode.
#[test]
fn rebuild_device_on_keeps_settings() {
    let cache = tempfile::tempdir().unwrap();
    let s = settings(true, 0.5);
    let mut r = furniture(cache.path(), FEATURE_DATA, s.clone());
    r.set_features_on(false); // drops ENABLE_LL: a define change, compiled here
    assert!(drain(&mut r) >= 1);
    assert_eq!(statuses(&r), vec![ShapeStatus::Drawing; 2]);
    let scene = Scene::neutral();
    let before = r.render(&scene, N, N).unwrap();
    let features = r.features().to_vec();
    r.rebuild_device_on(Gpu::new_warp().unwrap()).unwrap();
    assert_eq!(r.settings(), &s);
    assert_eq!(r.features(), features);
    let rep = r.load_parsed(&parse(&fixture("TestNifFile_Furniture_Col_SE.nif")), &none);
    assert_eq!((rep.shapes, rep.pending), (2, 0), "{rep:?}");
    assert_eq!(
        r.render(&scene, N, N).unwrap(),
        before,
        "same FeatureData values and sRGB mode"
    );
    // The toggle survived too: re-applying the same settings leaves ENABLE_LL off and compiles nothing.
    r.set_settings(s);
    assert_eq!(r.features(), features);
    assert!(r.take_jobs().is_empty());
}
```

- [ ] **Step 4: Run the tests and watch them fail.**
`cargo test -p render3d --lib technique_defaults`
Expected: FAIL at `assert_eq!(default_for(72, 0, false), [128, 128, 255, 0])` with left `[0, 0, 0, 255]`.
`cargo test -p render3d --test settings`
Expected: compile errors, `no method named 'set_settings' found for struct 'Renderer'` (and the same for
`set_features_on`, `off_in_preview`, `settings`).

- [ ] **Step 5: Implement C3.** In `crates/render3d/src/textures.rs`, replace
```rust
pub fn default_for(reg: u32, shader_type: u32, pbr: bool) -> [u8; 4] {
    match (reg, pbr, shader_type) {
        (5, true, _) => [255, 0, 255, 255],
```
with
```rust
///
/// CS feature registers (P1-C C3; reflected only with CS_SKIN / CS_HAIR): t72 skin detail normal = flat with
/// detail mask 0; t73 hair tangent shift (`Texture2D<float>`, read `.x − 0.5`) = mid grey, no shift; t71 skin
/// extra and t74 skin wetness = 0 (gated off by `skinMaterialFlags` = 0).
pub fn default_for(reg: u32, shader_type: u32, pbr: bool) -> [u8; 4] {
    match (reg, pbr, shader_type) {
        (72, _, _) => [128, 128, 255, 0],
        (73, _, _) => [128, 128, 128, 255],
        (71 | 74, _, _) => [0, 0, 0, 0],
        (5, true, _) => [255, 0, 255, 255],
```
This is the per-register choice that both `bind_textures` (lib.rs:525
`let rgba = textures::default_for(*reg, m.shader_type, pbr);`, the loop "a 1×1 default for every reflected
register left without one") and `rebind` (lib.rs:1182) use. `default_srv` keys its cache by `(kind, rgba)`, so the
new colours get their own 1×1 views.

- [ ] **Step 6: Implement the Renderer side.** In `crates/render3d/src/lib.rs`:

6a. Imports: replace
```rust
use compile::{cached, program_key, CompiledPair, ProgramKey, SrvKind};
```
with
```rust
use compile::{cached, program_key, CompiledPair, ProgramKey, SrvKind};
use cs_settings::CsSettings;
```
(Skip this if Part A already imports `CsSettings` in lib.rs.)

6b. Fields: in `pub struct Renderer`, replace
```rust
    cache_dir: PathBuf,
    features: Vec<String>,
```
with
```rust
    cache_dir: PathBuf,
    /// The CS feature defines every program is compiled with. `set_features` replaces it whole; `set_settings`
    /// and `set_features_on` replace only the settings' defines (`ENABLE_LL` and [`cs_settings::PREVIEW_FEATURES`]).
    features: Vec<String>,
    /// The user's CS settings last applied (P1-C C11); default = no settings files found.
    settings: CsSettings,
    /// The "Community Shaders features" toggle (C8): off, the settings add no define.
    features_on: bool,
```

6c. `with_gpu`: replace
```rust
            features: DEFAULT_FEATURES.iter().map(|s| s.to_string()).collect(),
```
with
```rust
            // = preview_defines(&CsSettings::default(), true): P0's Linear Lighting default.
            features: DEFAULT_FEATURES.iter().map(|s| s.to_string()).collect(),
            settings: CsSettings::default(),
            features_on: true,
```

6d. Replace the `set_features` doc and add the new public methods. Replace
```rust
    /// CS feature defines for the next load or rebuilt draw (default [`DEFAULT_FEATURES`]).
    pub fn set_features(&mut self, features: Vec<String>) {
        self.features = features;
    }

    pub fn features(&self) -> &[String] {
        &self.features
    }
```
with
```rust
    /// The whole CS feature define list for the next load or rebuilt draw (default [`DEFAULT_FEATURES`]): tests
    /// and P0 callers. A later [`Renderer::set_settings`] / [`Renderer::set_features_on`] replaces only the
    /// settings' defines in it and keeps the rest (the stand-in's `STANDIN_*` hooks) in order.
    pub fn set_features(&mut self, features: Vec<String>) {
        self.features = features;
    }

    pub fn features(&self) -> &[String] {
        &self.features
    }

    /// Applies the user's CS settings (P1-C C11). The FeatureData values are recomputed. If the settings'
    /// defines are unchanged, the static block of every drawing shape is rewritten (a cbuffer edit, next
    /// frame). If they changed, every shape goes through the permutation path, and the old draws keep drawing
    /// with their old constants until their programs land. The display sRGB encode follows `ENABLE_LL`.
    pub fn set_settings(&mut self, s: CsSettings) {
        self.feature_values = cs_settings::feature_data(&s);
        self.settings = s;
        self.apply_settings_defines();
    }

    pub fn settings(&self) -> &CsSettings {
        &self.settings
    }

    /// The "Community Shaders features" toggle (C8): off drops every settings define (LL included) and the sRGB
    /// encode; applied like [`Renderer::set_settings`].
    pub fn set_features_on(&mut self, on: bool) {
        self.features_on = on;
        self.apply_settings_defines();
    }

    /// Features CS loaded (Info.ini, or the Features inis) that this preview does not compile, in name order, for
    /// the panel's "off in preview" line. TruePBR is never listed: it has no define (TRUE_PBR is per material)
    /// and its settings are always written.
    pub fn off_in_preview(&self) -> Vec<String> {
        self.settings
            .loaded
            .iter()
            .filter(|n| n.as_str() != "TruePBR")
            .filter(|n| !define_of(n).is_some_and(|d| self.features.iter().any(|f| f == d)))
            .cloned()
            .collect()
    }

    /// `set_settings` / `set_features_on`: the defines the settings do not manage, in their order, then
    /// `preview_defines`. Unchanged as a set (`program_key` sorts defines) → the static block of every drawing
    /// shape is rewritten in place; changed → every shape through [`Renderer::begin`].
    fn apply_settings_defines(&mut self) {
        let mut f: Vec<String> = self
            .features
            .iter()
            .filter(|d| !settings_define(d))
            .cloned()
            .collect();
        f.extend(cs_settings::preview_defines(&self.settings, self.features_on));
        let (mut old, mut new) = (self.features.clone(), f.clone());
        old.sort();
        new.sort();
        // Linear Lighting draws in linear working space: encode for display (was the GUI's apply_features).
        self.encode_srgb = f.iter().any(|d| d == "ENABLE_LL");
        self.features = f;
        if old == new {
            self.rewrite_static();
        } else {
            self.begin_all();
        }
    }

    /// New FeatureData values on every `Drawing` shape: the cbuffer-edit path of `apply_in_place` with the
    /// shape's current material. Pending/failed shapes keep their old draw's constants (P1-B); a pending one gets
    /// the new values when its program lands.
    fn rewrite_static(&mut self) {
        let ll = self.linear_lighting();
        for s in self.shapes.values_mut() {
            if s.status != ShapeStatus::Drawing {
                continue;
            }
            if let Some(d) = s.draw.as_mut() {
                d.rewrite_material(&s.material, ll, &self.feature_values);
            }
        }
    }

    /// Every shape that is not hidden through [`Renderer::begin`] (a define-set change, C11). A program that is at
    /// hand swaps now; the others queue one job each and stay `Pending { showing_previous }` on their old draw.
    /// The resolver is never asked: every slot of a shape's material is in `tex_cache` since its load or edit.
    fn begin_all(&mut self) {
        let todo: Vec<(ShapeId, MaterialState)> = self
            .shapes
            .iter()
            .filter(|(_, s)| s.status != ShapeStatus::Hidden)
            .map(|(id, s)| (*id, s.material.clone()))
            .collect();
        let mut scratch = LoadReport::default();
        for (id, m) in todo {
            self.begin(id, &m, &|_| None, &mut scratch);
        }
        self.sort_draw_order();
        if !self.async_compile {
            self.settle();
        }
    }
```

6e. Add two free functions directly above `/// Flushes and spins on the renderer's event query` (before `fn wait_idle`):
```rust
/// A define the CS settings manage (C8): `ENABLE_LL` or a [`cs_settings::PREVIEW_FEATURES`] define.
fn settings_define(d: &str) -> bool {
    d == "ENABLE_LL" || cs_settings::PREVIEW_FEATURES.iter().any(|(_, x)| *x == d)
}

/// The define a loaded CS feature (Info.ini short name) compiles as, if any: Linear Lighting is setting-driven
/// (`ENABLE_LL`), the others come from [`cs_settings::PREVIEW_FEATURES`].
fn define_of(feature: &str) -> Option<&'static str> {
    if feature == "LinearLighting" {
        return Some("ENABLE_LL");
    }
    cs_settings::PREVIEW_FEATURES
        .iter()
        .find(|(n, _)| *n == feature)
        .map(|(_, d)| *d)
}

```

6f. `rebuild_device_on`: replace
```rust
    /// After a device loss: a fresh device (the caller makes it, WARP if need be) with the same shaders, cache
    /// dir, features, sRGB mode and `compiled` results. Everything that lived on the old device is gone,
```
with
```rust
    /// After a device loss: a fresh device (the caller makes it, WARP if need be) with the same shaders, cache
    /// dir, features, CS settings and toggle, sRGB mode and `compiled` results. Everything that lived on the old device is gone,
```
and replace
```rust
        n.features = std::mem::take(&mut self.features);
        n.encode_srgb = self.encode_srgb;
```
with
```rust
        n.features = std::mem::take(&mut self.features);
        n.settings = std::mem::take(&mut self.settings);
        n.features_on = self.features_on;
        n.encode_srgb = self.encode_srgb;
```
(Part A's Task 2 already added the `feature_values` carry line after `n.reference_writer = self.reference_writer;`; do not add a second one.)

- [ ] **Step 7: Run the tests and watch them pass.**
`cargo test -p render3d --lib technique_defaults`
`cargo test -p render3d --test settings`
Expected: 1 + 4 tests pass. Then `cargo test -p render3d --test async_compile --test standin --test frame --test edit --test hotswap`.
Expected: all still pass. The stand-in hook is inert without its define, and `set_features` is unchanged.

- [ ] **Step 8: Run the gates.**
`cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: clean. `sdc-gui` still compiles: it calls `set_features` and `set_encode_srgb`, which are unchanged. Part C
switches it to `set_features_on` / `set_settings`.

- [ ] **Step 9: Commit.**
```bash
git -C F:/ai/dds/converter-nif-p1c add crates/render3d/src/lib.rs crates/render3d/src/textures.rs crates/render3d/tests/fixtures/standin/Lighting.hlsl crates/render3d/tests/settings.rs
git -C F:/ai/dds/converter-nif-p1c commit -m "$(cat <<'EOF'
render3d: apply user CS settings to the loaded model (P1-C C3, C8, C11)

set_settings recomputes FeatureData and the settings' defines: unchanged defines rewrite the static
cbuffer block in place (no compile); changed defines route every shape through begin() with old draws
kept until their programs land. set_features_on toggles the settings' defines; the sRGB encode follows
ENABLE_LL. off_in_preview lists loaded features the preview does not compile. Neutral 1x1 defaults for
the CS skin/hair registers t71-t74. rebuild_device_on keeps settings, toggle and FeatureData values.
The stand-in shader gains an inert STANDIN_FEATURE_DATA hook for the tests.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 5: GUI: settings read on locate (start and focus) for both viewports, `SetSettings` without a reload, a features toggle that does not reload, the off-in-preview line (T-S4b)

**Files**
- Modify: `gui/src/model/viewport3d.rs`: `V3Cmd::SetSettings`, the `settings` field on `V3Reply::Located`/`Chosen`, `V3Reply::OffInPreview`, the locate/choose workers, `on_settings`, `set_features` without a reload, `ensure_thread`/`render_loop` carrying the settings, `apply_features` removed, `off_line`
- Modify: `gui/src/model/set_preview.rs`: `SetPreview::relocate`
- Modify: `gui/src/ui/viewport3d.rs`: the footer line from `off_line`
- Modify: `gui/src/ui/set_preview.rs`: the P0 hard-coded list is shown only until the settings are read
- Modify: `gui/tests/viewport3d.rs`: four new tests, three existing tests adjusted (they used the toggle as a reload trigger), one `V3Reply::Located` literal gains `settings: None`
- Modify (**shared file, add-only, separate commit, unavoidable**): `gui/src/app.rs`: one line in `focus_regained`. The set preview re-locates only once (`model/set_preview.rs:1743-1746`, `if !self.located`). `focus_regained` (`app.rs:147-150`) re-locates only the Meshes viewport (`self.meshes.relocate()`). Without this line the set preview never re-reads the settings on focus, which C12 requires.

**Interfaces (produced; Task 6 and later parts use these names)**

```rust
// gui/src/model/viewport3d.rs
pub enum V3Cmd { /* … */ SetFeatures(bool) /* now: no reload */, SetSettings(CsSettings), /* … */ }
pub enum V3Reply {
    /* … */
    Located { generation: u64, result: Result<Option<Fingerprint>, String>, settings: Option<CsSettings> },
    Chosen { generation: u64, result: Result<Fingerprint, String>, settings: Option<CsSettings> },
    /// Renderer::off_in_preview() after the thread starts and after each SetSettings / SetFeatures.
    OffInPreview(Vec<String>),
}
pub const SETTINGS_KEPT: &str;                 // notice prefix for a settings file that did not parse
pub fn off_line(off: &[String], fallback: &str) -> String;
pub fn read_cs_settings(target: Option<&ScanTarget>, found: &Fingerprint) -> CsSettings;
impl Viewport3d {
    pub off_in_preview: Vec<String>,           // field
    pub fn settings(&self) -> &CsSettings;     // last applied
    pub fn on_settings(&mut self, s: CsSettings);
}
// gui/src/model/set_preview.rs
impl SetPreview { pub fn relocate(&mut self); }
```

Reply shape decision (the contract left it to Part C): the read `CsSettings` is a **separate field** `settings: Option<CsSettings>` on `Located` and `Chosen`. It is `Some` whenever a `Shaders` root was found, and it is dropped with a stale generation exactly like `result`. `on_located(result)` keeps its signature, so its ≈26 test call sites stay unchanged. `handle` calls `on_located`/`on_chosen` and then `on_settings`. This means `Change::Same`, which never reaches `adopt`, still compares the hash.

- [ ] **Step 1: Write the failing tests.** Append this to the end of `gui/tests/viewport3d.rs`:

```rust
// ---- P1-C Task 5: the user's CS settings (T-S4b) ----

use render3d::cs_settings::CsSettings;
use sdc_gui::model::meshes::ScanTarget;

const SETTINGS_REL: &str = "SKSE/Plugins/CommunityShaders/SettingsUser.json";
const GOOD: &str = r#"{"Linear Lighting": {"enableLinearLighting": 1}}"#;
const CHANGED: &str = r#"{"Linear Lighting": {"enableLinearLighting": 1, "glowmapMult": 0.5}}"#;

/// A folder target holding a CS `Shaders/Lighting.hlsl` and `SettingsUser.json` (no real owner files).
fn cs_folder(json: &str) -> (tempfile::TempDir, ScanTarget, PathBuf) {
    let d = tempfile::tempdir().unwrap();
    let m = d.path().join("CS");
    std::fs::create_dir_all(m.join("Shaders")).unwrap();
    std::fs::write(m.join("Shaders/Lighting.hlsl"), "// l").unwrap();
    let file = m.join(SETTINGS_REL);
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, json).unwrap();
    (d, ScanTarget::Folder(m), file)
}

/// One locate (what start and window focus do), waited for.
fn relocated(v: &mut Viewport3d, target: &ScanTarget) {
    v.locate(Some(target.clone()));
    let t = Instant::now();
    while v.locating() {
        assert!(t.elapsed() < Duration::from_secs(20), "locate did not answer");
        v.poll();
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn settings_sent(cmds: &std::sync::mpsc::Receiver<v3::V3Cmd>) -> Vec<CsSettings> {
    cmds.try_iter()
        .filter_map(|c| match c {
            v3::V3Cmd::SetSettings(s) => Some(s),
            _ => None,
        })
        .collect()
}

#[test]
fn focus_reread_sends_set_settings_only_when_the_hash_changes() {
    let (_d, target, file) = cs_folder(GOOD);
    let mut v = Viewport3d::new(None);
    let cmds = v.attach_test_thread();
    relocated(&mut v, &target); // start
    let first = settings_sent(&cmds);
    assert_eq!(first.len(), 1, "the first read is applied");
    assert!(!first[0].parse_failed, "{:?}", first[0].notes);
    assert_eq!(v.settings().hash, first[0].hash);
    relocated(&mut v, &target); // focus, nothing changed on disk
    assert!(settings_sent(&cmds).is_empty(), "same hash: nothing sent");
    std::fs::write(&file, CHANGED).unwrap();
    relocated(&mut v, &target); // focus after CS saved its settings
    let next = settings_sent(&cmds);
    assert_eq!(next.len(), 1, "a new hash is sent once");
    assert_ne!(next[0].hash, first[0].hash);
    assert_eq!(v.settings().hash, next[0].hash);
}

#[test]
fn a_malformed_settings_file_keeps_the_previous_settings_and_shows_a_note() {
    let (_d, target, file) = cs_folder(GOOD);
    let mut v = Viewport3d::new(None);
    let cmds = v.attach_test_thread();
    relocated(&mut v, &target);
    assert_eq!(settings_sent(&cmds).len(), 1);
    let good = v.settings().clone();
    // CS is half-way through writing the file.
    std::fs::write(&file, r#"{"Linear Lighting": "#).unwrap();
    relocated(&mut v, &target);
    assert!(settings_sent(&cmds).is_empty(), "a malformed file sends nothing");
    assert_eq!(v.settings(), &good, "the previous settings stay");
    let note = v.notice.clone().expect("a notice");
    assert!(note.starts_with(v3::SETTINGS_KEPT), "{note}");
    // The panel shows it through the existing notice line.
    let mut h = harness(v);
    h.run();
    h.get_by_label_contains(v3::SETTINGS_KEPT);
    let mut v = h.into_state();
    // CS finished writing: the next focus applies the file and the note goes.
    std::fs::write(&file, CHANGED).unwrap();
    relocated(&mut v, &target);
    assert_eq!(settings_sent(&cmds).len(), 1);
    assert!(v.notice.is_none(), "{:?}", v.notice);
}

#[test]
fn the_features_toggle_sends_set_features_and_no_load() {
    let (_d, mut v, cmds) = confirmed();
    let nif = Arc::new(render3d::ParsedNif::default());
    v.set_content(Some(parsed(1, &nif, &[])), false);
    v.request_frame(64, 64);
    let Ok(v3::V3Cmd::LoadParsed { id, .. }) = cmds.try_recv() else {
        panic!("LoadParsed")
    };
    loaded(
        &mut v,
        id,
        render3d::LoadReport {
            shapes: 1,
            ..Default::default()
        },
    );
    v.request_frame(64, 64);
    let Ok(v3::V3Cmd::Render { id: rid, .. }) = cmds.try_recv() else {
        panic!("render")
    };
    frame_reply(&mut v, rid); // answered, so nothing is in flight before the toggle
    v.set_features(false);
    v.request_frame(64, 64);
    let sent: Vec<v3::V3Cmd> = cmds.try_iter().collect();
    assert_eq!(sent.len(), 1, "only the toggle: no load, no render");
    assert!(matches!(sent[0], v3::V3Cmd::SetFeatures(false)));
    assert_eq!(v.loaded_for_test(), Some(id));
    assert!(!v.busy(), "nothing pending in the model");
}

#[test]
fn the_off_line_lists_what_the_renderer_leaves_off() {
    assert_eq!(v3::off_line(&[], v3::CUBEMAPS_FOOTER), v3::CUBEMAPS_FOOTER);
    let off = ["DynamicCubemaps", "LODBlending", "ScreenSpaceGI", "NRD"].map(String::from);
    assert_eq!(
        v3::off_line(&off, "unused"),
        "Off in this preview: Dynamic Cubemaps, LOD Blending, Screen Space GI, NRD"
    );
    let mut v = Viewport3d::with_state(V3State::Ready);
    v.handle(v3::V3Reply::OffInPreview(off.to_vec()));
    let mut h = harness(v);
    h.run();
    h.get_by_label_contains("Off in this preview: Dynamic Cubemaps, LOD Blending");
}
```

- [ ] **Step 2: Adjust the three existing tests that used the toggle as a reload trigger, and the one `Located` literal.**

(a) `a_reload_with_compile_errors_keeps_the_last_frame` (`gui/tests/viewport3d.rs:608-610`). The toggle no longer reloads. A CS update (same folder, new hash) still does, and that is what the test's own comment describes. Replace

```rust
    // A CS update recompiles; both shapes now fail.
    v.set_features(false);
    assert!(matches!(cmds.try_recv(), Ok(v3::V3Cmd::SetFeatures(false))));
```

with

```rust
    // A CS update (same folder, new hash) reloads and recompiles; both shapes now fail.
    v.on_located(Ok(Some(fp(d.path(), "CS", "b"))));
    assert!(matches!(cmds.try_recv(), Ok(v3::V3Cmd::SetShaders(_))));
```

Expected: the rest of the test is unchanged and passes. `adopt` calls `reload_pending`, so `pending_load == Some(id)` and the `Loaded` that follows is taken as a reload.

(b) `a_texture_swap_reaches_the_render_thread_and_survives_a_reload` (`:1641-1644`). Replace

```rust
    let id1 = v.frame.as_ref().unwrap().id;
    v.set_features(false);
    // A new frame after the reload that is green again: the swap was applied again.
    wait_for(&mut v, 1, id1);
```

with

```rust
    let id1 = v.frame.as_ref().unwrap().id;
    // A CS update (same folder, new hash) reloads the content; the features toggle no longer does (P1-C).
    let mut updated = v.shaders.clone().unwrap();
    updated.hash.push('x');
    v.on_located(Ok(Some(updated)));
    // A new frame after the reload that is green again: the swap was applied again.
    wait_for(&mut v, 1, id1);
```

(c) The ignored owner render (`:351-362`, `#[ignore]`, run only by hand). The toggle now recompiles in the background and sends `Refresh` frames. For `Content::Mesh` the model gets no `Shapes`, so `busy()` cannot wait for them. Replace

```rust
    // Features off: no ENABLE_LL, gamma-space output, no display encode (for a by-eye check).
    v.set_features(false);
    v.request_frame(1600, 900);
    wait(&mut v);
    let before = v.frame.clone().unwrap().id;
    v.request_frame(1600, 900);
    wait(&mut v);
    assert_ne!(
        v.frame.as_ref().unwrap().id,
        before,
        "re-rendered after the reload"
    );
```

with

```rust
    // Features off: no ENABLE_LL, gamma-space output, no display encode (for a by-eye check). No reload (P1-C):
    // the thread recompiles in the background and sends refresh frames; take the picture once none came for 5 s.
    let before = v.frame.clone().unwrap().id;
    v.set_features(false);
    let t = std::time::Instant::now();
    let mut last = (before, std::time::Instant::now());
    while (last.0 == before || last.1.elapsed().as_secs() < 5) && t.elapsed().as_secs() < 300 {
        v.poll();
        let id = v.frame.as_ref().unwrap().id;
        if id != last.0 {
            last = (id, std::time::Instant::now());
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert_ne!(
        v.frame.as_ref().unwrap().id,
        before,
        "re-rendered after the toggle"
    );
```

(d) `a_stale_locate_after_choose_folder_is_ignored` (`:503-506`). Replace

```rust
    v.handle(v3::V3Reply::Located {
        generation: stale,
        result: Ok(None),
    });
```

with

```rust
    v.handle(v3::V3Reply::Located {
        generation: stale,
        result: Ok(None),
        settings: None,
    });
```

- [ ] **Step 3: Run them to see them fail.**

```powershell
cargo test -p sdc-gui --test viewport3d
```

Expected: compile errors only: no `V3Cmd::SetSettings`, no `settings` field on `Located`, no `V3Reply::OffInPreview`, no `Viewport3d::settings`, `SETTINGS_KEPT` or `off_line`.

- [ ] **Step 4: Model: constants, commands, replies.** In `gui/src/model/viewport3d.rs`:

Imports. Replace

```rust
use render3d::locate::{self, Change, Fingerprint};
```

with

```rust
use render3d::cs_settings::{self, CsSettings};
use render3d::locate::{self, Change, Fingerprint};
```

Constants. Replace

```rust
pub const CUBEMAPS_FOOTER: &str = "Dynamic Cubemaps: off in preview";
```

with

```rust
/// The footer's off-in-preview line until the user's CS settings have been read (P0 wording).
pub const CUBEMAPS_FOOTER: &str = "Dynamic Cubemaps: off in preview";
/// Notice prefix when a CS settings file did not parse (CS may be writing it): the previous settings stay (C10).
pub const SETTINGS_KEPT: &str = "Community Shaders settings not read, keeping the previous ones";

/// `DynamicCubemaps` → `Dynamic Cubemaps`, `LODBlending` → `LOD Blending` (Info.ini short names as CS's menu
/// spells them).
fn spaced(name: &str) -> String {
    let c: Vec<char> = name.chars().collect();
    let mut out = String::new();
    for (i, &ch) in c.iter().enumerate() {
        let after_lower = i > 0 && c[i - 1].is_lowercase();
        let acronym_end =
            i > 0 && c[i - 1].is_uppercase() && c.get(i + 1).is_some_and(|n| n.is_lowercase());
        if ch.is_uppercase() && (after_lower || acronym_end) {
            out.push(' ');
        }
        out.push(ch);
    }
    out
}

/// The panel's off-in-preview line: the user's loaded CS features this preview does not compile
/// (`Renderer::off_in_preview`); `fallback` until the thread has reported them (no settings read yet).
pub fn off_line(off: &[String], fallback: &str) -> String {
    if off.is_empty() {
        return fallback.to_string();
    }
    let names: Vec<String> = off.iter().map(|n| spaced(n)).collect();
    format!("Off in this preview: {}", names.join(", "))
}
```

`V3Cmd`. Replace

```rust
pub enum V3Cmd {
    SetShaders(Fingerprint),
    SetFeatures(bool),
```

with

```rust
pub enum V3Cmd {
    SetShaders(Fingerprint),
    /// The "Community Shaders features" toggle (`Renderer::set_features_on`): no reload, changed shapes recompile.
    SetFeatures(bool),
    /// New user CS settings (`Renderer::set_settings`, C11): a cbuffer rewrite, recompiles only when the defines
    /// change, never a reload.
    SetSettings(CsSettings),
```

`V3Reply`. Replace

```rust
    /// A locate answer; `generation` tells a stale one (started before a later locate or "Choose folder…").
    Located {
        generation: u64,
        result: Result<Option<Fingerprint>, String>,
    },
    Chosen {
        generation: u64,
        result: Result<Fingerprint, String>,
    },
}
```

with

```rust
    /// A locate answer; `generation` tells a stale one (started before a later locate or "Choose folder…").
    /// `settings`: the user's CS settings read in the same worker, whenever a `Shaders` folder was found.
    Located {
        generation: u64,
        result: Result<Option<Fingerprint>, String>,
        settings: Option<CsSettings>,
    },
    Chosen {
        generation: u64,
        result: Result<Fingerprint, String>,
        settings: Option<CsSettings>,
    },
    /// `Renderer::off_in_preview()`, after the thread starts and after each settings or features change.
    OffInPreview(Vec<String>),
}
```

- [ ] **Step 5: Model: fields, accessor, `on_settings`, workers, toggle.**

Fields. Replace

```rust
    pub features_on: bool,
    pub notice: Option<String>,
```

with

```rust
    pub features_on: bool,
    /// The user's loaded CS features this preview leaves off (`V3Reply::OffInPreview`), for the panel line.
    pub off_in_preview: Vec<String>,
    pub notice: Option<String>,
```

and replace

```rust
    confirmed: bool,
    thread: Option<Sender<V3Cmd>>,
```

with

```rust
    confirmed: bool,
    /// The CS settings last applied: sent to the render thread, or given to it when it starts.
    settings: CsSettings,
    thread: Option<Sender<V3Cmd>>,
```

Constructor (`Viewport3d::new`). Replace `            features_on: true,` with

```rust
            features_on: true,
            off_in_preview: vec![],
```

and replace `            confirmed: false,` with

```rust
            confirmed: false,
            settings: CsSettings::default(),
```

Accessor. Insert it after `locate_generation`. Replace

```rust
    pub fn locate_generation(&self) -> u64 {
        self.locate_gen
    }
```

with

```rust
    pub fn locate_generation(&self) -> u64 {
        self.locate_gen
    }

    /// The CS settings last applied.
    pub fn settings(&self) -> &CsSettings {
        &self.settings
    }
```

Locate worker. In `pub fn locate`, replace

```rust
        spawn_guarded(self.tx.clone(), "locate", move |tx| {
            let result = locate_shaders(target.as_ref(), saved.as_ref());
            let _ = tx.send(WorkerMsg::Viewport3d(V3Reply::Located {
                generation,
                result,
            }));
        });
```

with

```rust
        spawn_guarded(self.tx.clone(), "locate", move |tx| {
            let result = locate_shaders(target.as_ref(), saved.as_ref());
            // Small files (~60 KB): a full re-read and hash on every locate (03 §3.4), same generation.
            let settings = match &result {
                Ok(Some(f)) => Some(read_cs_settings(target.as_ref(), f)),
                _ => None,
            };
            let _ = tx.send(WorkerMsg::Viewport3d(V3Reply::Located {
                generation,
                result,
                settings,
            }));
        });
```

Choose worker. In `pub fn choose_folder`, replace

```rust
            let result = chosen_root(&folder)
                .and_then(|root| locate::fingerprint(&root, None).map_err(|e| e.to_string()));
            let _ = tx.send(WorkerMsg::Viewport3d(V3Reply::Chosen {
                generation,
                result,
            }));
```

with

```rust
            let result = chosen_root(&folder)
                .and_then(|root| locate::fingerprint(&root, None).map_err(|e| e.to_string()));
            let settings = result.as_ref().ok().map(|f| read_cs_settings(None, f));
            let _ = tx.send(WorkerMsg::Viewport3d(V3Reply::Chosen {
                generation,
                result,
                settings,
            }));
```

`on_settings`. Insert it before `fn on_chosen`. Replace

```rust
    fn on_chosen(&mut self, r: Result<Fingerprint, String>) {
```

with

```rust
    /// The CS settings read with a locate answer (C10). If a file did not parse, the previous settings stay and
    /// the notice says why. A new hash goes to the render thread; with no thread yet, it starts with these.
    pub fn on_settings(&mut self, s: CsSettings) {
        if s.parse_failed {
            self.notice = Some(format!("{SETTINGS_KEPT}: {}", s.notes.join("; ")));
            return;
        }
        if self
            .notice
            .as_deref()
            .is_some_and(|n| n.starts_with(SETTINGS_KEPT))
        {
            self.notice = None;
        }
        if s.hash == self.settings.hash {
            return;
        }
        self.settings = s.clone();
        if let Some(t) = &self.thread {
            let _ = t.send(V3Cmd::SetSettings(s));
        }
    }

    fn on_chosen(&mut self, r: Result<Fingerprint, String>) {
```

Toggle. Replace

```rust
    /// The "Community Shaders features" toggle.
    pub fn set_features(&mut self, on: bool) {
        if self.features_on == on {
            return;
        }
        self.features_on = on;
        if let Some(t) = &self.thread {
            let _ = t.send(V3Cmd::SetFeatures(on));
            self.reload_pending();
        }
    }
```

with

```rust
    /// The "Community Shaders features" toggle (C8). The render thread recomputes the defines. Shapes whose
    /// permutation changes recompile in the background while their old draw stays. Nothing reloads.
    pub fn set_features(&mut self, on: bool) {
        if self.features_on == on {
            return;
        }
        self.features_on = on;
        if let Some(t) = &self.thread {
            let _ = t.send(V3Cmd::SetFeatures(on));
        }
    }
```

`ensure_thread`. Replace

```rust
        let features = self.features_on;
        let me = ctx.clone();
        spawn_guarded(self.tx.clone(), "viewport3d", move |tx| {
            render_loop(crx, me, tx, shaders, cache, features)
        });
```

with

```rust
        let features = self.features_on;
        let settings = self.settings.clone();
        let me = ctx.clone();
        spawn_guarded(self.tx.clone(), "viewport3d", move |tx| {
            render_loop(crx, me, tx, shaders, cache, features, settings)
        });
```

`handle`. Replace

```rust
            V3Reply::Located { generation, result } if generation == self.locate_gen => {
                self.on_located(result)
            }
            V3Reply::Chosen { generation, result } if generation == self.locate_gen => {
                self.on_chosen(result)
            }
            V3Reply::Located { .. } | V3Reply::Chosen { .. } => {}
```

with

```rust
            V3Reply::Located {
                generation,
                result,
                settings,
            } if generation == self.locate_gen => {
                self.on_located(result);
                if let Some(s) = settings {
                    self.on_settings(s);
                }
            }
            V3Reply::Chosen {
                generation,
                result,
                settings,
            } if generation == self.locate_gen => {
                self.on_chosen(result);
                if let Some(s) = settings {
                    self.on_settings(s);
                }
            }
            V3Reply::Located { .. } | V3Reply::Chosen { .. } => {}
            V3Reply::OffInPreview(off) => self.off_in_preview = off,
```

- [ ] **Step 6: Model: the origins helper and `read_cs_settings`.** This is a pure refactor of `locate_shaders` (same behaviour, and its existing tests keep passing). In `locate_shaders`, replace

```rust
    let origins: Vec<(String, PathBuf, i32)> = match target {
        Some(ScanTarget::Mo2 { root, profile }) => {
            modscan::mo2::Instance::load(root, Some(profile))
                .ok()
                .and_then(|i| modscan::mo2::loose_origins(&i).ok())
                .map(|(o, _)| {
                    o.into_iter()
                        .map(|o| (o.name, o.root, o.priority))
                        .collect()
                })
                .unwrap_or_default()
        }
        Some(ScanTarget::Folder(f)) => {
            let name = f
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            vec![(name, f.clone(), 0)]
        }
        None => vec![],
    };
    if let Some((name, root)) = locate::find_in_mods(&origins) {
```

with

```rust
    let origins = target_origins(target);
    if let Some((name, root)) = locate::find_in_mods(&origins) {
```

and insert these two functions before `pub fn locate_shaders`. Replace

```rust
/// Finds the CS `Shaders` folder: a folder the user chose (no owning mod) while it still holds `Lighting.hlsl`;
```

with

```rust
/// The target's loose origins as `(name, root, priority)`: MO2 = game Data −1, mods by modlist order, overwrite
/// `i32::MAX`; a folder target is one origin.
fn target_origins(target: Option<&ScanTarget>) -> Vec<(String, PathBuf, i32)> {
    match target {
        Some(ScanTarget::Mo2 { root, profile }) => {
            modscan::mo2::Instance::load(root, Some(profile))
                .ok()
                .and_then(|i| modscan::mo2::loose_origins(&i).ok())
                .map(|(o, _)| {
                    o.into_iter()
                        .map(|o| (o.name, o.root, o.priority))
                        .collect()
                })
                .unwrap_or_default()
        }
        Some(ScanTarget::Folder(f)) => {
            let name = f
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            vec![(name, f.clone(), 0)]
        }
        None => vec![],
    }
}

/// The user's CS settings for the located `Shaders` folder (03 §3.4, C12). They come through the target's
/// origins when CS was found there (overwrite wins). For a chosen folder, or a saved one with no origins, they come
/// from the Data folder above `Shaders`. Never fails: problems become notes.
pub fn read_cs_settings(target: Option<&ScanTarget>, found: &Fingerprint) -> CsSettings {
    let origins = if found.owner_mod.is_some() {
        target_origins(target)
    } else {
        vec![]
    };
    let origins = (!origins.is_empty()).then_some(origins.as_slice());
    cs_settings::read_settings(&locate::settings_paths(origins, &found.root))
}

/// Finds the CS `Shaders` folder: a folder the user chose (no owning mod) while it still holds `Lighting.hlsl`;
```

(For an MO2 target this reads `modlist.txt` and stats the mod folders a second time per locate, on the worker thread. That is accepted: see Open points.)

- [ ] **Step 7: Render thread.** Remove `apply_features`. Delete

```rust
fn apply_features(r: &mut Renderer, on: bool) {
    let f = if on {
        render3d::DEFAULT_FEATURES
            .iter()
            .map(|s| s.to_string())
            .collect()
    } else {
        vec![]
    };
    // Linear Lighting draws in linear space: encode for display. Without it the output is gamma-space already.
    let ll = f.iter().any(|s: &String| s == "ENABLE_LL");
    r.set_features(f);
    r.set_encode_srgb(ll);
}

```

(the sRGB-encode coupling moves into `Renderer::set_settings` / `set_features_on`, C11).

Signature and `make`. Replace

```rust
    shaders: Fingerprint,
    cache: PathBuf,
    features: bool,
) {
    let send = |r: V3Reply| {
        let _ = tx.send(WorkerMsg::Viewport3d(r));
    };
    let make = |shaders: Fingerprint, features: bool| -> Result<Renderer, String> {
        let mut r = Renderer::new(shaders, cache.clone())?;
        r.set_async_compile(true);
        apply_features(&mut r, features);
        Ok(r)
    };
```

with

```rust
    shaders: Fingerprint,
    cache: PathBuf,
    features: bool,
    settings: CsSettings,
) {
    let send = |r: V3Reply| {
        let _ = tx.send(WorkerMsg::Viewport3d(r));
    };
    let make = |shaders: Fingerprint, features: bool| -> Result<Renderer, String> {
        let mut r = Renderer::new(shaders, cache.clone())?;
        r.set_async_compile(true);
        r.set_features_on(features);
        r.set_settings(settings);
        Ok(r)
    };
```

Startup reply. Replace

```rust
    send(V3Reply::Ready {
        adapter: r.gpu().adapter.clone(),
        warp: r.gpu().warp,
    });
    let pool = CompilePool::new(render3d::pool::WORKERS, move |d| {
```

with

```rust
    send(V3Reply::Ready {
        adapter: r.gpu().adapter.clone(),
        warp: r.gpu().warp,
    });
    send(V3Reply::OffInPreview(r.off_in_preview()));
    let pool = CompilePool::new(render3d::pool::WORKERS, move |d| {
```

(`rebuild` keeps the settings in the renderer, C11, so it sends nothing new.)

Batch flag. Replace

```rust
        let mut compiled_changed = false;
        let mut pending: Vec<(u64, ShapeId, MaterialState)> = vec![];
```

with

```rust
        let mut compiled_changed = false;
        // A settings or features change: cbuffers rewritten, programs maybe swapped or queued; no reload.
        let mut restyled = false;
        let mut pending: Vec<(u64, ShapeId, MaterialState)> = vec![];
```

Arms. Replace

```rust
                V3Cmd::SetFeatures(on) => {
                    apply_features(&mut r, on);
                    reload = true;
                    None
                }
```

with

```rust
                V3Cmd::SetFeatures(on) => {
                    r.set_features_on(on);
                    restyled = true;
                    None
                }
                V3Cmd::SetSettings(s) => {
                    r.set_settings(s);
                    restyled = true;
                    None
                }
```

After the batch. Replace

```rust
        pump(&mut r);
        // A landed program changed a shape and nothing above reported it: tell the panel.
```

with

```rust
        pump(&mut r);
        if restyled {
            send(V3Reply::OffInPreview(r.off_in_preview()));
        }
        // Redraw from the last camera, and report the shapes (now pending or swapped) like a landed program.
        compiled_changed |= restyled;
        // A landed program changed a shape and nothing above reported it: tell the panel.
```

- [ ] **Step 8: Set preview re-locates on focus.** In `gui/src/model/set_preview.rs`, insert after `poll_at`'s closing brace. The anchor is the start of `poll_at`'s doc comment. Replace

```rust
    /// `poll` at a given time (tests drive the debounce and the throttle). Call once per frame.
```

with

```rust
    /// Window focus (`app.rs`): look for CS and re-read its settings again, once this panel has located it.
    pub fn relocate(&mut self) {
        if self.located {
            self.v3.locate(self.target.clone());
        }
    }

    /// `poll` at a given time (tests drive the debounce and the throttle). Call once per frame.
```

- [ ] **Step 9: Panel lines.** `gui/src/ui/viewport3d.rs`. Replace the import

```rust
use crate::model::viewport3d::{
    moved_text, Look, V3State, Viewport3d, CHOOSE_FOLDER, CUBEMAPS_FOOTER, FEATURES_LABEL,
    FRAME_LABEL, KEEP_OLD, NOT_FOUND, USE_NEW,
};
```

with

```rust
use crate::model::viewport3d::{
    moved_text, off_line, Look, V3State, Viewport3d, CHOOSE_FOLDER, CUBEMAPS_FOOTER,
    FEATURES_LABEL, FRAME_LABEL, KEEP_OLD, NOT_FOUND, USE_NEW,
};
```

and in `fn footer` replace `        ui.weak(CUBEMAPS_FOOTER);` with

```rust
        ui.weak(off_line(&v.off_in_preview, CUBEMAPS_FOOTER));
```

`gui/src/ui/set_preview.rs`. The shared `viewport3d::show` (and its footer) is drawn under this panel, so once the list arrives it would appear twice. Replace

```rust
    ui.label(egui::RichText::new(OFF_FEATURES).small());
```

with

```rust
    // Until the CS settings are read the shared footer names only Dynamic Cubemaps: keep the P0 list here.
    if s.v3.off_in_preview.is_empty() {
        ui.label(egui::RichText::new(OFF_FEATURES).small());
    }
```

Do not edit `OFF_FEATURES` itself (`model/set_preview.rs:1034-1036`). It stays as this fallback, and `tests/set_preview_ui.rs:77` still finds it (no thread in that test, so the list is empty).

- [ ] **Step 10: Run the tests.**

```powershell
cargo test -p sdc-gui --test viewport3d
cargo test -p sdc-gui --test set_preview_ui
```

Expected: all pass. That includes the four new tests, the three adjusted ones and the unchanged `:105` footer check ("Dynamic Cubemaps: off in preview", empty list → fallback). If `a_malformed_settings_file_…` fails because `read_settings` does not set `parse_failed` for a truncated `SettingsUser.json`, that is a Part A bug against C10. Report it; do not work around it here.

- [ ] **Step 11: Gates, then commit (gui files only).**

```powershell
cargo fmt --all; cargo clippy --workspace --all-targets -- -D warnings; cargo test --workspace
git add crates/gui/src/model/viewport3d.rs crates/gui/src/model/set_preview.rs crates/gui/src/ui/viewport3d.rs crates/gui/src/ui/set_preview.rs crates/gui/tests/viewport3d.rs
git commit -m "feat(gui): CS settings read on locate, SetSettings and the features toggle without a reload, off-in-preview line (T-S4b)" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

- [ ] **Step 12: Shared file, add-only, separate commit.** In `gui/src/app.rs`, replace

```rust
    fn focus_regained(&mut self) {
        self.model.check_sources();
        self.meshes.relocate();
    }
```

with

```rust
    fn focus_regained(&mut self) {
        self.model.check_sources();
        self.meshes.relocate();
        self.state.set3d.relocate();
    }
```

```powershell
cargo fmt --all; cargo clippy --workspace --all-targets -- -D warnings; cargo test --workspace
git add crates/gui/src/app.rs
git commit -m "feat(gui): the set preview re-reads CS (and its settings) on window focus (shared app.rs, add-only)" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 6: installed-CS gate tests per preview feature (T-S5) and the `settings` timing-log line

**Files**
- Create: `render3d/tests/cs_features.rs` (ignored, `SDC_ACHERUS`)
- Modify: `gui/src/model/viewport3d.rs`: `apply_settings` (applies the settings and emits `settings`), used by `make` and the `SetSettings` arm
- Modify: `gui/tests/viewport3d.rs`: one capture test
- Possibly modify: `render3d/src/cs_settings.rs` `PREVIEW_FEATURES`: delete the rows whose gate fails (C9), in a separate commit

**Interfaces**

```rust
// gui/src/model/viewport3d.rs
/// Applies user CS settings on the render thread and writes the `settings` log line
/// {"hash": <first 12 hex>, "defines": [..], "ms": f64, "notes": usize}.
#[doc(hidden)]
pub fn apply_settings(r: &mut Renderer, s: CsSettings);
```

`perf_log::capture` sees only the calling thread (`perf_log.rs:290`). The emit therefore sits in a function the test calls on its own thread with a WARP stand-in renderer. The render thread calls the same function.

Mesh choice for T-S5. The existing ignored render3d tests (`lights.rs:137-165`) draw a builtin sphere and read loose files by direct path; render3d has no BSA reader (no `vfs` dependency). The meshes are therefore loose vanilla files in the owner's MO2 extract mod. Checked present on 2026-10-12, read-only:
- head: `{SDC_ACHERUS}\mods\SkyrimMeshesExtract\meshes\actors\character\character assets\femalehead.nif` (87,175 B). FaceGen, shader type 4 → `FACEGEN` → `SKIN` (installed `Lighting.hlsl:17-18`), so `CS_SKIN` takes effect.
- body: `{SDC_ACHERUS}\mods\SkyrimMeshesExtract\meshes\actors\character\character assets\femalebody_1.nif` (193,604 B). SkinTint, shader type 5 → `FACEGEN_RGB_TINT` → `SKIN`, so `CS_SKIN` takes effect (owner ruling 2026-10-12: Advanced Skin is assumed for both skin types, face tint and skin tint).
- hair: `{SDC_ACHERUS}\mods\SkyrimMeshesExtract\meshes\actors\character\character assets\hair\femaledarkelfhair01.nif`. HairTint, shader type 6 → `HAIR`, so `CS_HAIR` takes effect.
- `SkyrimMeshesExtract` is **disabled** in the modlist (`-SkyrimMeshesExtract`). That does not matter for a direct path read. Fallbacks in the same folder if a file is missing: `malehead.nif`; `malebody_1.nif`; `hair/femaledarkelfhair02.nif`, `hair/femalehumanoldhair01.nif`. The test asserts the shader type, so a wrong pick fails loudly.

"No NaN pixels" proxy. The readback maps half → u8 and turns NaN into 0 (`device.rs:277-288`: `srgb8` returns 0, `unorm8`'s clamp-then-cast gives 0). Infinity clamps to 255. A NaN pixel is therefore exactly `(0,0,0)`. The test renders on a grey 0.5 background and requires that a feature run has **no more exact-black pixels than the `ENABLE_LL`-only baseline** of the same mesh and camera. It also requires that the mesh drew: pixels that differ from the background corner exist, and their RGB sum is greater than 0. See Open points: a true float NaN count would need a renderer seam.

- [ ] **Step 1: Write the gate test.** Create `render3d/tests/cs_features.rs`:

```rust
//! T-S5 (03 §8, C9): each preview feature define (`PREVIEW_FEATURES`) alone with `ENABLE_LL`, and all of them as
//! the owner's settings turn them on, on the owner's installed Community Shaders. A face-tint head and a skin-tint body (both SKIN)
//! and a hair (HAIR) mesh must compile, draw, and add no NaN pixels. A row that fails is deleted from
//! `PREVIEW_FEATURES` and the plan records it. Ignored: reads the MO2 instance read-only; panics without it.
//! `$env:SDC_ACHERUS='C:\_mount\moddingvol\Acherus'; cargo test -p render3d --release --test cs_features -- --ignored --nocapture`
use render3d::cs_settings::{preview_defines, read_settings, CsSettings, PREVIEW_FEATURES};
use render3d::device::Gpu;
use render3d::locate::{fingerprint, settings_paths, Fingerprint};
use render3d::scene::{Background, Scene};
use render3d::{parse, Renderer};
use std::path::{Path, PathBuf};

const CS_MOD: &str = "CommunityShaders_AIO-2026-10-03T06-46Z";
/// Vanilla meshes, loose in the owner's (disabled) extract mod: read by path, no BSA, no scan.
const ASSETS: &str = "mods/SkyrimMeshesExtract/meshes/actors/character/character assets";
/// (file under ASSETS, shader type that must be present: 4 FaceGen (face tint) and 5 FaceGen RGB tint
/// (skin tint) → SKIN, 6 HairTint → HAIR)
const MESHES: [(&str, u32); 3] = [
    ("femalehead.nif", 4),
    ("femalebody_1.nif", 5),
    ("hair/femaledarkelfhair01.nif", 6),
];
const N: u32 = 128;

struct Shot {
    drawn: usize,
    sum: u64,
    black: usize,
}

fn acherus() -> PathBuf {
    PathBuf::from(std::env::var("SDC_ACHERUS").expect("set SDC_ACHERUS to the MO2 instance root"))
}

/// The owner's settings the way the GUI finds them: overwrite over the CS mod (no scan, C12 tuples).
fn owner_settings(root: &Path) -> CsSettings {
    let origins = vec![
        (CS_MOD.to_string(), root.join("mods").join(CS_MOD), 0),
        ("overwrite".to_string(), root.join("overwrite"), i32::MAX),
    ];
    let shaders = root.join("mods").join(CS_MOD).join("Shaders");
    let s = read_settings(&settings_paths(Some(&origins), &shaders));
    assert!(!s.parse_failed, "{:?}", s.notes);
    s
}

/// One mesh, one define set, one frame on a grey background, in a fresh renderer.
fn shoot(
    fp: &Fingerprint,
    cache: &Path,
    s: &CsSettings,
    defines: &[String],
    nif: &[u8],
) -> Result<Shot, String> {
    let mut r = Renderer::with_gpu(Gpu::new_warp()?, fp.clone(), cache.to_path_buf())?;
    r.set_settings(s.clone()); // the owner's FeatureData values (and LL encode)
    r.set_features(defines.to_vec()); // then exactly this define set
    let rep = r.load(nif, &|_| None);
    if rep.shapes == 0 || rep.pending > 0 || !rep.errors.is_empty() || !rep.compile_errors.is_empty() {
        return Err(format!("load: {rep:?}"));
    }
    let mut scene = Scene::neutral();
    scene.background = Background::Solid(0.5);
    let px = r.render(&scene, N, N)?;
    let bg = [px[0], px[1], px[2]]; // a corner: the camera frames the bounds with a margin
    let (mut drawn, mut sum, mut black) = (0, 0u64, 0);
    for p in px.chunks(4) {
        if p[..3] == [0, 0, 0] {
            black += 1; // NaN reads back as 0 (device.rs srgb8 / unorm8)
        }
        if p[..3] != bg {
            drawn += 1;
            sum += p[..3].iter().map(|&c| c as u64).sum::<u64>();
        }
    }
    Ok(Shot { drawn, sum, black })
}

#[test]
#[ignore = "needs SDC_ACHERUS (MO2 instance root) with the CS 2026-10-03 mod"]
fn each_preview_feature_compiles_and_draws_a_head_and_hair_on_the_installed_cs() {
    let root = acherus();
    let s = owner_settings(&root);
    let fp = fingerprint(&root.join("mods").join(CS_MOD).join("Shaders"), None).unwrap();
    let cache = tempfile::tempdir().unwrap(); // shared by every run below: each permutation compiles once
    let ll = "ENABLE_LL".to_string();
    let all = preview_defines(&s, true);
    assert_eq!(
        all.len(),
        PREVIEW_FEATURES.len() + 1,
        "the owner's settings turn every preview feature and LL on: {all:?}"
    );
    let mut cases: Vec<(String, Vec<String>)> = vec![("ENABLE_LL only".into(), vec![ll.clone()])];
    for (name, define) in PREVIEW_FEATURES {
        cases.push((name.to_string(), vec![ll.clone(), define.to_string()]));
    }
    cases.push(("all (owner settings)".into(), all));
    let mut failures = vec![];
    for (file, shader_type) in MESHES {
        let path = root.join(ASSETS).join(file);
        let nif = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert!(
            parse(&nif).shapes.iter().any(|x| x.material.shader_type == shader_type),
            "{}: no shape of shader type {shader_type}; pick a fallback mesh (plan Task 6)",
            path.display()
        );
        let mut baseline_black = None;
        for (label, defines) in &cases {
            let t0 = std::time::Instant::now();
            match shoot(&fp, cache.path(), &s, defines, &nif) {
                Ok(shot) => {
                    println!(
                        "{file} / {label}: drawn {} px, rgb sum {}, black {} ({:.1} s)",
                        shot.drawn,
                        shot.sum,
                        shot.black,
                        t0.elapsed().as_secs_f64()
                    );
                    if label == "ENABLE_LL only" {
                        baseline_black = Some(shot.black);
                    }
                    // No baseline = the LL-only run failed: that is a P0 regression, already listed above.
                    let base = baseline_black.unwrap_or(0);
                    if shot.drawn == 0 || shot.sum == 0 {
                        failures.push(format!("{file} / {label}: nothing drawn (black frame)"));
                    } else if shot.black > base {
                        failures.push(format!(
                            "{file} / {label}: {} exact-black (NaN) pixels, baseline {base}",
                            shot.black
                        ));
                    }
                }
                Err(e) => failures.push(format!("{file} / {label}: {e}")),
            }
        }
    }
    assert!(
        failures.is_empty(),
        "delete the failing rows from PREVIEW_FEATURES (C9):\n{}",
        failures.join("\n")
    );
}
```

- [ ] **Step 2: Check that it compiles and stays ignored in the gate run.**

```powershell
cargo test -p render3d --test cs_features
```

Expected: `1 ignored`, `0 passed`.

- [ ] **Step 3: Write the failing log-line test.** Append to `gui/tests/viewport3d.rs`:

```rust
// ---- P1-C Task 6: the `settings` timing-log line ----

#[test]
fn applying_settings_writes_a_settings_line() {
    let standin = PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../render3d/tests/fixtures/standin"
    ));
    let fp = render3d::locate::fingerprint(&standin, None).unwrap();
    let cache = tempfile::tempdir().unwrap();
    let mut r = render3d::Renderer::with_gpu(
        render3d::device::Gpu::new_warp().unwrap(),
        fp,
        cache.path().to_path_buf(),
    )
    .unwrap();
    let s = CsSettings {
        hash: "0123456789abcdef0123".into(),
        notes: vec!["Info.ini missing".into()],
        ..Default::default()
    };
    let ((), ev) = perf_log::capture(|| v3::apply_settings(&mut r, s));
    let e = named(&ev, "settings");
    assert_eq!(e.len(), 1, "{ev:?}");
    assert_eq!(e[0]["hash"], "0123456789ab");
    assert_eq!(e[0]["defines"], serde_json::json!(["ENABLE_LL"]), "default settings: LL only");
    assert_eq!(e[0]["notes"], 1);
    assert!(e[0]["ms"].as_f64().unwrap() >= 0.0, "{}", e[0]);
}
```

```powershell
cargo test -p sdc-gui --test viewport3d applying_settings
```

Expected: compile error, no `v3::apply_settings`.

- [ ] **Step 4: Implement `apply_settings` and use it at both call sites.** In `gui/src/model/viewport3d.rs`, insert it before `fn resolver`. Replace

```rust
/// The texture resolver over `map` (canonical path → bytes); read errors are collected in `errors`.
```

with

```rust
/// Applies user CS settings (C11: a cbuffer rewrite, recompiles only when the defines change, never a reload) and
/// writes the `settings` log line. The render thread calls it; public so a test can capture the line.
#[doc(hidden)]
pub fn apply_settings(r: &mut Renderer, s: CsSettings) {
    let t0 = Instant::now();
    let hash: String = s.hash.chars().take(12).collect();
    let notes = s.notes.len();
    r.set_settings(s);
    perf_log::emit(
        "settings",
        json!({"hash": hash, "defines": r.features(), "ms": perf_log::ms_since(t0), "notes": notes}),
    );
}

/// The texture resolver over `map` (canonical path → bytes); read errors are collected in `errors`.
```

In `make`, replace `        r.set_settings(settings);` with `        apply_settings(&mut r, settings);`. This logs the settings the thread starts with too. In the arm, replace

```rust
                V3Cmd::SetSettings(s) => {
                    r.set_settings(s);
```

with

```rust
                V3Cmd::SetSettings(s) => {
                    apply_settings(&mut r, s);
```

- [ ] **Step 5: Run.**

```powershell
cargo test -p sdc-gui --test viewport3d
```

Expected: all pass, `applying_settings_writes_a_settings_line` included.

- [ ] **Step 6: Gates and commit.**

```powershell
cargo fmt --all; cargo clippy --workspace --all-targets -- -D warnings; cargo test --workspace
git add crates/render3d/tests/cs_features.rs crates/gui/src/model/viewport3d.rs crates/gui/tests/viewport3d.rs
git commit -m "test(render3d): installed-CS gate per preview feature (T-S5, ignored); feat(gui): settings timing-log line" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

- [ ] **Step 7: Run the gate on this machine (owner data, read-only)** and record the output in the ledger:

```powershell
$env:SDC_ACHERUS='C:\_mount\moddingvol\Acherus'; cargo test -p render3d --release --test cs_features -- --ignored --nocapture
```

Expected: `1 passed`, with one line per mesh × case (2 × 7 with 5 rows) printing drawn px, rgb sum, black count and seconds. A failing `ENABLE_LL only` line is a P0 regression (stop and report), not a C9 row; with no baseline every later row is compared against 0 exact-black pixels. A cold run compiles every permutation through fxc on WARP, which can take minutes; the shared temp cache makes the second mesh faster.

- [ ] **Step 8 (only if Step 7 lists failures): turn the failing rows off (C9).** Delete exactly the failing `(name, define)` rows from `const PREVIEW_FEATURES` in `render3d/src/cs_settings.rs` (one line each). Re-run Step 7 until it passes, then run the gates. If Part A's tests pin the row count or the owner-defines list, they must be updated in the same commit. Commit separately:

```powershell
git add crates/render3d/src/cs_settings.rs
git commit -m "fix(render3d): turn off <rows> in the preview: T-S5 fails on the installed CS (<one-line reason each>)" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

Record the removed rows and the failure text in the plan's "P1-C verification" section (Task 7, row "T-S5").

---

### Task 7: end-to-end verification on owner data (manual checklist; numbers come from the timing log)

**Files**
- Modify: the stitched P1-C plan file, section "P1-C verification" at the end (record the results)
- No code. Owner data (`C:\_mount\moddingvol\Acherus`, the CS install, the game) is read-only to sdc and to the person running the checklist: **never edit `overwrite\SKSE\Plugins\CommunityShaders\*.json` or `overwrite\ShaderCache\Info.ini` by hand**. Settings change only through CS's own in-game menu. The only things written by hand are scratch copies under `%TEMP%`. sdc itself writes only its shader cache (`%LOCALAPPDATA%\sdc\shader-cache`) and the timing log (`%LOCALAPPDATA%\sdc\logs`).

**Interfaces:** Consumes the whole P1-C build (Tasks 1–6). Produces pass/fail per check. **The owner does not time anything.** The app writes `%LOCALAPPDATA%\sdc\logs\preview-timings-YYYYMMDD-HHMMSS.jsonl` (P1-B Task 9). P1-C adds the `settings` line: `{"event":"settings","hash":"<12 hex>","defines":[…],"ms":…,"notes":n}`, written when a render thread starts and whenever new settings are applied. Pause about 5 seconds between steps and send the newest log file back; the controller reads the numbers with the table at the end. Screenshots are taken only where the check is about pixels.

- [ ] **Step 1: Prepare**
  - `cargo build -p sdc-gui --release` in `F:\ai\dds\converter-nif-p1c`.
  - Start the exe from MO2 (the Acherus instance), so the CS locate and the settings resolve through MO2 (overwrite wins). Do not set `SDC_PREVIEW_LOG=0`. In the set preview's "Timings" section check that it shows "Timing log: <path>"; if it says "off", stop and report.
  - Write down the `LastWriteTime` of `C:\_mount\moddingvol\Acherus\overwrite\SKSE\Plugins\CommunityShaders\SettingsUser.json` (read-only look: `(Get-Item <path>).LastWriteTime`).

- [ ] **Step 2: The settings are read at start (both viewports)**
  - Open a set (set preview) and the Meshes view's 3D panel once each, and let each draw.
  - Expect: under the frame, a line "Off in this preview: …" listing the user's loaded features that the preview leaves off, spelled with spaces ("Dynamic Cubemaps", "Light Limit Fix", "Screen Space Shadows" …). It must **not** list Unified Water (disabled at boot), Linear Lighting, Vanilla Fresnel, Hair Specular, Skin, Extended Materials or Extended Translucency, unless T-S5 turned a row off (Task 6 Step 8). In the set preview the long P0 list is gone, and the line appears once.
  - Log: one `settings` line per viewport thread (two in all), the same `hash`, `defines` = `["CS_HAIR","CS_SKIN","ENABLE_LL","EXTENDED_MATERIALS","EXTENDED_TRANSLUCENCY","VANILLA_FRESNEL"]` minus any row T-S5 removed, `notes: 0`.

- [ ] **Step 3: The preview matches the game (visual)**
  - Pick three subjects that exist both in sdc and in a game save you can reach: (a) a character head and body close up (Meshes view: `meshes\actors\character\character assets\femalehead.nif` (face tint) and `femalebody_1.nif` (skin tint), or a set whose NIF is a head or body), (b) a hair mesh (`…\character assets\hair\femaledarkelfhair01.nif` or the hair you see in game), (c) a metal or glossy armour piece with an envmap or complex material (Fresnel, Extended Materials).
  - In game: a well-lit interior, CS on, screenshot each subject from the front. In sdc: the same subject, default camera and light, screenshot. Then toggle "Community Shaders features" off and screenshot again (the P0-like look, for reference); toggle it back on.
  - Expect, comparing sdc to the game: overall diffuse brightness and gamma (Linear Lighting) close; grazing-angle specular on the armour (Vanilla Fresnel) visible in both; hair with the CS hair highlight (two lobes, shifted along the strands, `HairMode 1`, Marschner) rather than the vanilla single sheen; skin with soft, low specular and edge fuzz (Advanced Skin) rather than vanilla plastic. Known differences that are **not** failures: no SSS, GI, IBL, dynamic cubemaps, wetness or shadows in the preview (they are listed as off), and different light rigs.
  - Pass: for each of LL, Fresnel, hair, skin, "matches / close / differs" with one sentence and the screenshot pair. Visual only, no log check.

- [ ] **Step 4: An in-game settings change shows after alt-tab, with no reload**
  - Keep sdc open on the hair mesh of Step 3 (set preview or Meshes view) with the frame up. Wait 5 s.
  - **Cbuffer-only change:** in game open the CS menu, Hair Specular, change `HairGlossiness` (70 → 20), use the menu's save so CS writes `SettingsUser.json`, then alt-tab to sdc. Wait 5 s.
    - Expect: the hair highlight changes (broader) within a frame or two of focusing the window; the picture never blanks; the shape list does not flicker; no "Loading…".
    - Log: after the focus, one `settings` line per open viewport with a **new** `hash`, the **same** `defines` as Step 2 and `ms`; **no `load` line** after it; no `shape_pending`.
  - **Define change:** in game switch Hair Specular `Enabled` off, save, alt-tab to sdc, wait 5 s.
    - Expect: the hair goes back to the vanilla look after its programs land (old image kept meanwhile, never blank); the off line now also lists Hair Specular.
    - Log: a `settings` line whose `defines` lacks `CS_HAIR`; `shape_pending` / `shape_ready` (ok: true) lines for the hair shapes; **no `load` line**.
  - **Restore:** in game set `HairGlossiness` back to 70 and `Enabled` on, save, alt-tab. Log: a `settings` line with `defines` as in Step 2. Note whether its `hash` equals the Step 2 hash (same bytes written back; informational).
  - **Focus without a change:** alt-tab away and back without touching the game. Log: **no** new `settings` line (same hash).
  - If the game cannot be run, mark this step "n/a (game not run)". Do not edit the JSON instead.

- [ ] **Step 5: No `Info.ini`: features from `Shaders\Features\*.ini` (scratch copy only)**
  - Build a scratch Data folder (copies; the originals are only read):

```powershell
$s = "$env:TEMP\sdc-cs-noinfo"
robocopy "C:\_mount\moddingvol\Acherus\mods\CommunityShaders_AIO-2026-10-03T06-46Z\Shaders" "$s\Shaders" /E
robocopy "C:\_mount\moddingvol\Acherus\overwrite\SKSE\Plugins\CommunityShaders" "$s\SKSE\Plugins\CommunityShaders" SettingsUser.json SettingsDefault.json
# deliberately no ShaderCache\Info.ini
```

  - In a viewport panel, "Shader folder…" → pick `%TEMP%\sdc-cs-noinfo\Shaders`. With a chosen folder, sdc reads the settings from the folder above `Shaders` (C12).
  - Expect: the preview redraws (new shaders path). The off line still does **not** list Unified Water: the copied `SettingsUser.json` has `"Disable at Boot": {"UnifiedWater": true}`, and the 48 feature `.ini` files stand in for Info.ini. Log: a `settings` line with `notes` ≥ 1 (Info.ini absent), the same `defines` as Step 2, and a hash different from Step 2's.
  - Variant without JSON: delete `$s\SKSE`, alt-tab away and back. Expect: Unified Water **now appears** in the off line (nothing disables it at boot). Log: a `settings` line with more `notes`; `defines` still contains `ENABLE_LL` (no file → Linear Lighting default on).
  - Malformed file (scratch only): recreate `$s\SKSE\Plugins\CommunityShaders\SettingsUser.json` with the text `{"Linear Lighting": ` and alt-tab. Expect: an amber notice "Community Shaders settings not read, keeping the previous ones: …", the picture unchanged. Log: no `settings` line. Copy the real `SettingsUser.json` over it again, alt-tab: the notice goes and a `settings` line appears.
  - Restore: "Shader folder…" back to `C:\_mount\moddingvol\Acherus\mods\CommunityShaders_AIO-2026-10-03T06-46Z\Shaders`, check that the off line matches Step 2, then `Remove-Item -Recurse $env:TEMP\sdc-cs-noinfo`.

- [ ] **Step 6: The installed-CS gate (T-S5)**, in a terminal (read-only owner data, writes only a temp cache):

```powershell
cd F:\ai\dds\converter-nif-p1c
$env:SDC_ACHERUS='C:\_mount\moddingvol\Acherus'; cargo test -p render3d --release --test cs_features -- --ignored --nocapture
```

  - Expect `1 passed` and one line per mesh × case: `femalehead.nif / <case>`, `femalebody_1.nif / <case>` and `hair/femaledarkelfhair01.nif / <case>` with drawn px > 0, rgb sum > 0, black ≤ the `ENABLE_LL only` line. Send the whole output. A failure names the rows to turn off (Task 6 Step 8).

- [ ] **Step 7: Close the exe normally and send the log**
  - Close sdc-gui with the window's close button, so the log ends with `run_end`. Send the newest `%LOCALAPPDATA%\sdc\logs\preview-timings-*.jsonl` ("Copy log path" in the set preview gives its path). If several runs were needed, send each file and say which step it covers.
  - Also send: the Step 3 screenshot pairs with the four verdicts, the Step 6 output, and the `LastWriteTime` of the owner's `SettingsUser.json` from Step 1 and from now (it changes only through the in-game saves of Step 4).

- [ ] **Step 8: Record** in the "P1-C verification" section at the end of the plan file, filled in by the controller from the log (reading rules below):

```markdown
## P1-C verification (<date>, exe <git sha>, log file <name>)

| Check | Result | Value (from the log unless marked manual) |
|---|---|---|
| Step 2 settings read at start, both viewports | pass / fail | settings lines <n> (expect 2), hash <12 hex>, defines <list>, notes <n>, ms <ms>; off line correct yes/no (manual) |
| Step 3 LL / Fresnel / hair / skin vs game | — | <matches / close / differs> ×4 with one sentence each (manual, screenshots) |
| Step 4 cbuffer-only change after alt-tab | pass / fail / n/a | settings hash <old> → <new>, defines unchanged yes/no, ms <ms>; load lines after it: <0>; shape_pending: <0> |
| Step 4 define change (Hair Specular off) | pass / fail / n/a | defines without CS_HAIR yes/no; shape_pending <k> / shape_ready ok <k>; load lines: <0>; never blank (manual) |
| Step 4 restore + focus without change | pass / fail / n/a | restore hash = Step 2 hash yes/no (informational); settings lines on the no-change focus: <0> |
| Step 5 no Info.ini (JSON copied) | pass / fail | notes <n ≥ 1>, defines = Step 2 yes/no; Unified Water absent from the off line (manual) |
| Step 5 no JSON | pass / fail | notes <n>; Unified Water listed (manual); ENABLE_LL in defines yes/no |
| Step 5 malformed JSON | pass / fail | notice shown, picture unchanged (manual); settings lines while malformed: <0>; after the fix: <1> |
| T-S5 gate (Step 6 / Task 6 Step 7) | pass / fail | per mesh × case drawn / sum / black; rows turned off (C9): <none or list with reason> |
| Whole run | — | ui_gap lines around the alt-tabs: <count, max gap_ms>; dropped events (run_end): <n>; device_rebuild lines: <n> |

Notes / surprises:
```

- [ ] **Step 9: Commit the record**

```bash
git add <the stitched P1-C plan file>.md
git commit -m "docs: P1-C verification results (user CS settings, alt-tab re-read, Info.ini fallback, T-S5 gate)" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

### How the controller reads the log

One JSON object per line; `t_ms` = milliseconds since the app started its log. Each window is cut at the `settings` line that follows an alt-tab and ends 5 s later (the pauses keep them apart). Quick filter: `Get-Content <file> | ConvertFrom-Json | Where-Object event -eq settings | Format-Table t_ms,hash,ms,notes,defines`.

| Checklist line | Event(s) | Pass threshold |
|---|---|---|
| Step 2: read at start | `settings` (first per thread), `gpu` | one `settings` per viewport thread; `defines` = the 6 (minus T-S5 removals), sorted; `notes` 0; `ms` informational |
| Step 4: cbuffer-only change | `settings`, then `load`, `shape_pending`, `frame_stats` in the window | new `hash`, `defines` identical to the previous `settings` line; **no `load`** and **no `shape_pending`** in the window; `ms` < 50 (a cbuffer rewrite, no compile) |
| Step 4: define change | `settings`, `shape_pending`, `shape_ready`, `compile`, `load` | `defines` lacks `CS_HAIR`; `shape_pending` and `shape_ready` with `ok: true` for the hair shapes; **no `load`**; `compile` `ms` informational |
| Step 4: focus without a change | `settings` | no `settings` line after that alt-tab |
| Step 5: Info.ini absent | `settings` | `notes` ≥ 1; `defines` as Step 2 (JSON copied); with no JSON: `defines` still contains `ENABLE_LL` |
| Step 5: malformed | `settings` | none while the file is malformed; exactly one after the fix |
| Always | `ui_gap`, `run_end.dropped`, `device_rebuild`, `gpu.warp` | no `ui_gap` caused by the re-read (the read runs on the locate worker); `dropped` 0; `warp: false`; `device_rebuild` absent |

---


## P1-C verification (2026-10-12, exe 9b0c5d5 built 15:44 (fix round 3 58c8fd2 is tests only), log preview-timings-20261012-224435.jsonl)

Run 1 (exe f43e3cd) found the head mirror-glossy and the scalp flow map tinting colour. Root cause (Task 8 diagnosis, 6c3fcb6): the owner's settings were never applied (saved shaders path is a chosen folder → MO2 overwrite not searched; features-only read hashed as default → dropped), so every run compiled only ENABLE_LL. Fixed in 9b0c5d5; Task 8 also added the pre-October CS skin RFAOS lookup (owner rule: slot 7 `_s` → `_rfaos`, else slot 1 `_n`/`_msn` → `_rfaos`), per-shape `skinMaterialFlags`, linear t71.

| Check | Result | Value (from the log unless marked manual) |
|---|---|---|
| Step 2 settings read at start | pass | `settings` defines `[CS_HAIR, CS_SKIN, ENABLE_LL, EXTENDED_MATERIALS, EXTENDED_TRANSLUCENCY, VANILLA_FRESNEL]`, hash `0f9846645d7b`, notes 0; off line lists loaded-but-uncompiled features, no Unified Water (manual, screenshot); also lists non-visual features (cosmetic) |
| Step 3 LL / Fresnel / hair / skin vs game | pass (manual) | owner: head, body, hair and armour look correct (Arch head uses `light/head_rfaos.dds`) |
| Step 4 value-only change | pass | 509.25 s: new hash `139f130f4c51`, same defines, no `load`, no `shape_pending` (owner edited HairGlossiness by hand with a backup, game closed) |
| Step 4 define change (Hair Specular off) | pass | 533.14 s: defines without CS_HAIR; shape 3 `shape_pending` showing_previous true → `shape_ready` ok in 283 ms; no `load`; off line lists Hair Specular (manual) |
| Step 4 restore | pass | 558.42 s: hash back to `0f9846645d7b` |
| Step 5 no Info.ini / no JSON / malformed | covered by tests | with an MO2 target the settings now always come from the MO2 origins, so the scratch-folder procedure cannot reach these cases; pinned by Task 1 (T-S2 fallback, malformed), Task 5 (malformed keeps previous), Task 8 fix rounds (fallback precedence) |
| Step 6 installed-CS gate (T-S5) | pass | Task 6: 3 meshes × 7 cases, 0 black px |
| Whole run | pass | `dropped` 0; no `panic`; ui_gap lines (4.8–16.8 s) each end at a "Shader folder…" pick (modal dialog) |

Follow-ups offered: a "settings coverage" log/notes line (fields no setting wrote, keys that matched no field) to catch CS key drift after updates; filter non-visual features from the off line.

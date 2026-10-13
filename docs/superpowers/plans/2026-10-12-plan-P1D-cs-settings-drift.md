# P1-D "CS settings drift" Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The preview writes correct FeatureData for every Community Shaders build the owner has (1.6 0528, 1.8 bottle, Nexus 1.9.1, installed AIO 1.9.0). Each exception is keyed on the fields the compiled shader reflects, never on a version number. Every settings apply logs which fields nothing wrote and which keys matched no field.

**Architecture:** `feature_data` gets the namespace-stripped set of reflected cbuffer names of the program a draw uses (`reflected_fields`, stored on `ShapeDraw.fields`). It applies three shape-keyed rules: new Linear Lighting (`diffuseCurve`), old Linear Lighting (`isDirLightLinear`), and new skin packing (`skinPerGeometryProfile`). Before any program exists, one decision is needed: whether `-D ENABLE_LL` is compiled. It comes from the same evidence in the expanded `Lighting.hlsl` text, memoised per shaders folder. A pure `coverage()` compares the reflected names with the written names and the settings keys. The renderer logs it per program (`settings_coverage`) and the panel shows one line only when that line is non-empty.

**Tech Stack:** Rust workspace; D3D11 through the `windows` crate (WARP in tests); serde_json; egui + egui_kittest. A one-off Python 3 (`python -I`) step extracts the fixtures. No new crates.

**Spec:** `F:\ai\dds\nifparseref\plan\p1\plan-parts-c\03-cs-settings-drift.md` (method (c)+(e)+(a), §6 recommendation; owner chose it 2026-10-12) together with the P1-C contract `F:\ai\dds\nifparseref\plan\p1\plan-parts-c\00-contract.md`. Read both. Code base: `nif/c-p1c @ db5ea8a` merged into `nif/main`.

**Supersedes in the P1-C contract:** C4's "pointLightMult stays π whatever the setting says". New LL now writes `π × pointLightMult`, which still gives π for the owner's setting of 1 (capture 11178). The C5 note "`skinDetailParams.x` = tiling × body is a `shortcut:` inference" is now sourced (advanced-skin `Renderer.cpp:263-264`), and the comment is replaced. The C8 rule "`ENABLE_LL` iff the setting" holds only for new-LL builds. Old-LL builds never get the define.

## Global Constraints

- Branch `nif/d-p1d`, created from `nif/main` after `nif/c-p1c` is merged; own worktree `F:\ai\dds\converter-nif-p1d`. Read `F:\ai\dds\nifparseref\plan\CONCURRENCY.md` first. Never touch `F:\ai\dds\converter`. Never push. Never merge into `main`.
- Off-limits (read pub items only): `crates/engine/`, `crates/cli/`, `crates/encoders-sys/`, `crates/gui/src/pgp/`, `crates/gui/src/sets_doc.rs`, `crates/gui/src/model/sets.rs`, `crates/gui/src/model/pbr_export.rs`, `schema/`, `docs/DECISIONS.md`, golden and `gui_repro` tests.
- Shared files are add-only, in small separate commits: `Cargo.toml`, `Cargo.lock`, `crates/gui/Cargo.toml`, `crates/gui/src/ui/mod.rs`, `crates/gui/src/model/mod.rs`, `crates/gui/src/app.rs`, `crates/gui/src/settings.rs`. None is planned. `Cargo.lock` must stay unchanged; if it changes, stop and report.
- No new crate dependencies.
- Owner data (`C:\_mount\moddingvol\Acherus`, the CS installs under `G:\`, `F:\ai\dds\nifparseref\Community Shaders nexus release latest`) is read-only. The Task 1 extraction only reads it. Tests never read the owner's files except `#[ignore]` + `SDC_ACHERUS` tests (none added here), and never the real `%LOCALAPPDATA%\sdc\scan.db`. The committed fixtures hold names and JSON types only, no CS source and no CS values (GPL-3).
- New LL (`linearLightingSettings.diffuseCurve` reflected): `pointLightMult = π × setting` (setting absent → 1). With LL inactive, every multiplier and `fireEffectCurve` = 1.0. That rule comes from dev source and has not been verified against the installed DLL; the code comment says so. Design §3.2: only material-local features compile; screen-space and world features are listed as "off in preview".
- Gates before every commit: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`. Commit messages end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- Tests run on WARP + the repo stand-in `crates/render3d/tests/fixtures/standin/Lighting.hlsl`.
- Paths are given from the worktree root. Edits are anchored by quoted text. Line numbers are those at `db5ea8a` and move as earlier tasks land.
- The P1-B timing log stays on. Task 4 adds a `settings_coverage` event. The owner does not time anything by hand.
- Review routing (owner): per-task reviews on Codex gpt-6-astra (low/medium/high by risk); Opus only for the final whole-branch review.

## Review Focus

1. **An old-LL build with no settings file (Nexus 1.9.1) must show LL off, never "LL on" with a zero flag.** `linear_lighting_on(&CsSettings::default(), true, true) == false`, `enableLinearLighting` is written 0, and no `-D ENABLE_LL` is passed. Pinned by Task 2 `old_linear_lighting_writes_raw_values_and_the_engine_fields` and `an_old_linear_lighting_build_compiles_without_the_enable_ll_define`.
2. **Turning the "Community Shaders features" toggle off on a new-LL build writes 1.0 multipliers**, because the 1.9 shader reads them ungated. Pinned by Task 2 `new_linear_lighting_off_writes_every_multiplier_as_one` (the `features_on = false` case).
3. **No CS skin section at all on an old build still un-doubles the tiling.** `x = 10`, not the capture's pre-multiplied 20, which the old shader would multiply again. Pinned by Task 3 `each_build_packs_skin_tiling_its_own_way` (the empty-settings case).
4. **The installed build stays silent.** Coverage for AIO 1.9.0 with its own key set is empty, so the panel shows no line and the log arrays are empty. Pinned by Task 4 `coverage_per_build`.
5. **The coverage event is emitted once per program per apply, not per frame or per draw.** Pinned by Task 4 `every_program_reports_its_settings_coverage` (equal counts across two applies).

Accepted limits (not tested here; listed for the reviewer):
- **Head detection is a proxy.** CS: a geometry with a `BSFaceGenNiNode` ancestor (advanced-skin `Renderer.cpp:133-134`). That node is created at runtime: Skyrim files have no such block (nif.xml has it only for `#STF#`). The preview uses FaceGen shader type 4, which is the old shader's own FACEGEN split (1.8 `Lighting.hlsl:2047`). Type-5 head parts such as the mouth (capture 11178 ev 4269 `Arch_Mouth`, head tiling 10/1) get body tiling. This is marked `shortcut:` in the code.
- `set_shaders` resets the old-LL memo and recomputes `encode_srgb` (f7bf9c6). Until the reload lands, the loaded draws keep the old folder's programs, so a frame or two can show the wrong gamma (final review M2).
- The per-geometry `skinParams.w = 0` for non-skin geometry (CS `Renderer.cpp:295`, capture: 140 draws) is out of scope. The owner did not list it.
- `coverage` (like `programs`) is never evicted within one shaders folder. After a define change the union still holds the earlier programs. They share the same structs, so the line stays right.
- On an old-LL build, an LL toggle still flips `ENABLE_LL` in `features`, so it takes `begin_all` (a draw rebuild per shape from programs at hand) instead of `rewrite_static`. The result is correct with keep-old semantics, just heavier.
- 1.8 bottle's `SettingsUser.json`/`SettingsDefault.json` sit at the mod root only because the owner moved them there when switching versions; CS itself writes them under `Data\SKSE\Plugins\CommunityShaders\` (MO2 overwrite). The fixture extraction reads them from where they are; the app needs no new path.
- `dev` after 10-02 also π-scales `directionalLightMult`. Capture 11178 (installed build) shows 1.0, so directional stays raw. Only a new capture catches that change (Task 5).

## Task order and dependencies

**1 → 2 → 3 → 4 → 5.** Task 1 creates the fixtures, `reflected_fields`, the new `feature_data` signature and the per-draw plumbing (no behaviour change). Task 2 adds the LL rules, Task 3 the skin rules, Task 4 coverage + log + panel, and Task 5 is the owner checklist. Tasks 2–4 append tests to `crates/render3d/tests/cs_versions.rs`, which Task 1 creates.

---

### Task 1: fixtures, `reflected_fields`, per-draw `feature_data` plumbing (no behaviour change)

**Files**
- Create (generated, Step 1): `crates/render3d/tests/fixtures/cs_versions/{cs16_0528,cs18_bottle,nexus191,aio190}/{fields.txt,keys.json}`
- Modify: `crates/render3d/src/cs_settings.rs` (`reflected_fields`, `feature_data` signature)
- Modify: `crates/render3d/src/draw.rs` (`ShapeDraw.fields`)
- Modify: `crates/render3d/src/lib.rs` (drop `Renderer.feature_values`, `program_fields`, `values_for`, `is_head`, 6 call sites)
- Modify: `crates/render3d/tests/cs_settings.rs`, `crates/render3d/tests/frame.rs` (call sites), `crates/render3d/tests/compile.rs` (one test)
- Create: `crates/render3d/tests/cs_versions.rs`

**Interfaces**
- Produces:
  - `pub fn reflected_fields<'a>(layouts: impl IntoIterator<Item = &'a CbLayout>) -> BTreeSet<String>`
  - `pub fn feature_data(s: &CsSettings, fields: &BTreeSet<String>, features_on: bool, head: bool) -> Vec<(String, Vec<f64>)>`. In this task the three new parameters are named `_fields`, `_features_on`, `_head` (unused). Tasks 2 and 3 drop the underscores.
  - `ShapeDraw.fields: BTreeSet<String>` (the program's reflected names, VS + PS)
  - lib.rs private: `fn program_fields(p: &Program) -> BTreeSet<String>`, `fn values_for(s: &CsSettings, features_on: bool, fields: &BTreeSet<String>, m: &MaterialState) -> Vec<(String, Vec<f64>)>`, `fn is_head(m: &MaterialState) -> bool`
  - Fixture layout: `fields.txt` = one reflected-style name per line, sorted (`linearLightingSettings.diffuseCurve`, `skinPerGeometryProfile.skinParams`, `emissiveMult` …). `keys.json` = the 7 `SettingsDefault.json` sections with values replaced by their type (`1`, `1.0`, `true`, `[1.0,…]`, `""`); Nexus 1.9.1 = `{}`.

- [ ] **Step 1: Extract the fixtures (one-off, read-only on owner data)**

Save this script **outside the repo** as `%TEMP%\sdc-p1d\extract_cs_versions.py` (it is not committed):

```python
"""One-off, read-only: CS build -> fields.txt (reflected-style names) + keys.json (the 7 SettingsDefault sections).
Usage: python -I extract_cs_versions.py <out_dir>"""
import json, pathlib, re, sys

BUILDS = {
    "cs16_0528": (r"G:\Acherus\mods\CommunityShaders_AIO 0528",
                  r"G:\Acherus\mods\CommunityShaders_AIO 0528\SKSE\Plugins\CommunityShaders\SettingsDefault.json"),
    "cs18_bottle": (r"G:\Acherus\mods\CommunityShaders_AIO_bottle",
                    r"G:\Acherus\mods\CommunityShaders_AIO_bottle\SettingsDefault.json"),
    "nexus191": (r"F:\ai\dds\nifparseref\Community Shaders nexus release latest", None),
    "aio190": (r"C:\_mount\moddingvol\Acherus\mods\CommunityShaders_AIO-2026-10-03T06-46Z",
               r"C:\_mount\moddingvol\Acherus\overwrite\SKSE\Plugins\CommunityShaders\SettingsDefault.json"),
}
PREFIXES = ["linearLightingSettings", "hairSpecularSettings", "vanillaFresnelSettings", "truePBRSettings",
            "extendedMaterialSettings", "extendedTranslucencySettings", "skinData"]
SECTIONS = ["Linear Lighting", "Hair Specular", "Vanilla Fresnel", "True PBR", "Extended Materials",
            "Extended Translucency", "Advanced Skin"]
MEMBER = re.compile(r"^\s*([\w:]+)\s+(\w+)\s*(?:\[\s*\d+\s*\])?\s*;", re.M)

def members(body):
    return MEMBER.findall(re.sub(r"//[^\n]*", "", body))

def read(p):
    p = pathlib.Path(p)
    return p.read_text(encoding="utf-8-sig", errors="replace") if p.exists() else ""

def block(text, head):
    m = re.search(head + r"\s*\{(.*?)\};", text, re.S)
    return members(m.group(1)) if m else []

def names(v):
    """Key names and JSON types only, no CS values: numbers -> 1 / 1.0, bools -> true, arrays keep their length."""
    if isinstance(v, dict):
        return {k: names(x) for k, x in v.items()}
    if isinstance(v, bool):
        return True
    if isinstance(v, int):
        return 1
    if isinstance(v, float):
        return 1.0
    if isinstance(v, list):
        return [1.0] * len(v)
    return ""

def main(out):
    for tag, (root, settings) in BUILDS.items():
        shaders = pathlib.Path(root) / "Shaders"
        shared = read(shaders / "Common" / "SharedData.hlsli")
        structs = {m.group(1): members(m.group(2))
                   for m in re.finditer(r"struct\s+(\w+)\s*\{(.*?)\};", shared, re.S)}
        def expand(var, ty):
            return [f"{var}.{n}" for _, n in structs[ty.split("::")[-1]]]
        fields = []
        for ty, var in block(shared, r"cbuffer\s+FeatureData\s*:\s*register\(b6\)"):
            if var in PREFIXES:
                fields += expand(var, ty)
        for ty, var in block(read(shaders / "Skin" / "Skin.hlsli"), r"cbuffer\s+SkinPerGeometry\s*:\s*register\(b7\)"):
            fields += expand(var, ty) if ty.split("::")[-1] in structs else [var]
        for _, var in block(read(shaders / "Common" / "Color.hlsli"), r"cbuffer\s+LLPerGeometry\s*:\s*register\(b8\)"):
            fields.append(var)
        d = pathlib.Path(out) / tag
        d.mkdir(parents=True, exist_ok=True)
        (d / "fields.txt").write_text("\n".join(sorted(fields)) + "\n", encoding="utf-8", newline="\n")
        keys = {}
        if settings:
            j = json.loads(read(settings))
            keys = {s: names(j[s]) for s in SECTIONS if s in j}
        (d / "keys.json").write_text(json.dumps(keys, indent=1, sort_keys=True) + "\n", encoding="utf-8", newline="\n")
        print(tag, len(fields), "fields,", sum(len(v) for v in keys.values()), "keys")

main(sys.argv[1])
```

Run (PowerShell, from any directory other than the script's):

```powershell
python -I "$env:TEMP\sdc-p1d\extract_cs_versions.py" F:\ai\dds\converter-nif-p1d\crates\render3d\tests\fixtures\cs_versions
```

Expected output, exactly (verified 2026-10-12):

```
cs16_0528 82 fields, 94 keys
cs18_bottle 82 fields, 93 keys
nexus191 68 fields, 0 keys
aio190 80 fields, 72 keys
```

Spot-check: `aio190\fields.txt` contains `linearLightingSettings.diffuseCurve`, `skinPerGeometryProfile.skinDetailParams` and no `emissiveMult`. `cs18_bottle\fields.txt` contains `linearLightingSettings.isDirLightLinear`, `emissiveMult`, `pad0`, `skinPerGeometry` and no `skinPerGeometryProfile.*`. `nexus191\fields.txt` has no `vanillaFresnelSettings.*` and no `skinPerGeometry`. `nexus191\keys.json` is `{}`. If a count differs, stop and report: the owner's folders changed.

- [ ] **Step 2: Write the failing tests**

Append to `crates/render3d/tests/compile.rs` (after `reflection_reports_member_kinds`):

```rust
/// P1-D: the reflected names `feature_data` keys its exceptions on, without fxc's namespace qualifier.
#[test]
fn reflected_fields_drop_the_namespace() {
    let cache = tempfile::tempdir().unwrap();
    let l = feature_kinds_layout(cache.path());
    let f = render3d::cs_settings::reflected_fields([&l]);
    assert!(f.contains("hairSpecularSettings.Enabled"), "{f:?}");
    assert!(f.contains("skinParams"), "{f:?}");
    assert!(f.iter().all(|n| !n.contains("::")), "{f:?}");
}
```

Create `crates/render3d/tests/cs_versions.rs`:

```rust
//! P1-D: shape-keyed CS settings rules and the coverage report over the four CS builds the owner has
//! (03-cs-settings-drift.md). Fixtures = name lists extracted by the P1-D plan Task 1 (no CS source, no CS values).
use render3d::cs_settings::{feature_data, read_settings, CsSettings, SettingsPaths};
use std::collections::BTreeSet;
use std::path::PathBuf;

const BUILDS: [&str; 4] = ["cs16_0528", "cs18_bottle", "nexus191", "aio190"];

fn dir(tag: &str) -> PathBuf {
    PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/cs_versions"
    ))
    .join(tag)
}

fn fields_of(tag: &str) -> BTreeSet<String> {
    std::fs::read_to_string(dir(tag).join("fields.txt"))
        .unwrap()
        .lines()
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect()
}

/// The build's reflected names and its SettingsDefault key set, read like a real settings file.
fn build(tag: &str) -> (BTreeSet<String>, CsSettings) {
    let s = read_settings(&SettingsPaths {
        default: Some(dir(tag).join("keys.json")),
        ..Default::default()
    });
    assert!(!s.parse_failed, "{tag}: {:?}", s.notes);
    (fields_of(tag), s)
}

fn value(fd: &[(String, Vec<f64>)], name: &str) -> Vec<f64> {
    fd.iter()
        .find(|(n, _)| n == name)
        .unwrap_or_else(|| panic!("{name} not written"))
        .1
        .clone()
}

/// The evidence each build carries (03 report §2): which LL generation, b7 profile, b8, Fresnel, skin JSON shape.
#[test]
fn fixtures_carry_each_builds_evidence() {
    for tag in BUILDS {
        let (f, s) = build(tag);
        let new = tag == "aio190";
        assert_eq!(f.contains("linearLightingSettings.diffuseCurve"), new, "{tag}");
        assert_eq!(f.contains("linearLightingSettings.isDirLightLinear"), !new, "{tag}");
        assert_eq!(f.contains("skinPerGeometryProfile.skinParams"), new, "{tag}");
        assert_eq!(f.contains("emissiveMult"), !new, "{tag}: b8 LLPerGeometry");
        assert_eq!(
            f.contains("vanillaFresnelSettings.Enable"),
            tag != "nexus191",
            "{tag}"
        );
        let skin = s.merged.get("Advanced Skin");
        match tag {
            "nexus191" => assert!(s.merged.is_empty(), "no JSON until CS first runs"),
            "aio190" => assert!(skin.unwrap().get("DefaultProfile").is_some()),
            _ => assert!(skin.unwrap().get("SkinDetailTiling").is_some(), "{tag}: flat skin"),
        }
    }
}

/// Task 1 changes no value: with the installed build's names, feature_data equals what P1-C wrote.
#[test]
fn the_installed_build_keeps_the_p1c_values() {
    let fd = feature_data(&CsSettings::default(), &fields_of("aio190"), true, false);
    assert_eq!(
        (value(&fd, "linearLightingSettings.pointLightMult")[0] as f32).to_bits(),
        std::f32::consts::PI.to_bits()
    );
    assert_eq!(value(&fd, "skinData.skinDetailParams"), [20.0, 2.0, 0.25, 0.0]);
}
```

Update every `feature_data` call in `crates/render3d/tests/cs_settings.rs` to the new signature, with the installed build's names. Add this helper after `names_set`:

```rust
/// The installed CS 1.9.0's reflected FeatureData/b7 names (P1-D fixture): the build these tests describe.
fn aio190() -> BTreeSet<String> {
    std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/cs_versions/aio190/fields.txt"
    ))
    .unwrap()
    .lines()
    .filter(|l| !l.is_empty())
    .map(String::from)
    .collect()
}
```

and change the five calls exactly:
- `let fd = feature_data(&s);` (two places) → `let fd = feature_data(&s, &aio190(), true, false);`
- `let fd = feature_data(&CsSettings::default());` → `let fd = feature_data(&CsSettings::default(), &aio190(), true, false);`
- `let fd = feature_data(&read_settings(&paths(true, true, false, None)));` → `let fd = feature_data(&read_settings(&paths(true, true, false, None)), &aio190(), true, false);`
- `let d = feature_data(&read_settings(&paths(false, true, false, None)));` → `let d = feature_data(&read_settings(&paths(false, true, false, None)), &aio190(), true, false);`

In `crates/render3d/tests/frame.rs` change `let fv = feature_data(&CsSettings::default());` → `let fv = feature_data(&CsSettings::default(), &Default::default(), true, false);`

- [ ] **Step 3: Run them to verify they fail**

Run: `cargo test -p render3d --test cs_versions --test compile reflected_fields`
Expected: compile errors `cannot find function reflected_fields` and `this function takes 1 argument but 4 arguments were supplied`.

- [ ] **Step 4: Implement**

`crates/render3d/src/cs_settings.rs`: change `use crate::compile::VarKind;` → `use crate::compile::{CbLayout, VarKind};`, and add after `diffuse_curve`:

```rust
/// Every cbuffer variable name of `layouts` without its namespace (`SharedData::linearLightingSettings.diffuseCurve`
/// → `linearLightingSettings.diffuseCurve`): the shape evidence [`feature_data`] keys its exceptions on (P1-D).
pub fn reflected_fields<'a>(layouts: impl IntoIterator<Item = &'a CbLayout>) -> BTreeSet<String> {
    layouts
        .into_iter()
        .flat_map(|l| &l.vars)
        .map(|v| unqualified(&v.name).to_string())
        .collect()
}

/// `ns::a.b` → `a.b`. Only the part before the first `.` can carry a namespace (struct members are dotted).
fn unqualified(name: &str) -> &str {
    let head = name.split('.').next().unwrap_or(name);
    head.rfind("::").map_or(name, |i| &name[i + 2..])
}
```

Change the `feature_data` doc and signature (body unchanged):

```rust
/// FeatureData values by cbuffer name, raw numbers (the writer types them from the reflection): the
/// capture-11178 constants, overwritten by every number the settings have (C7: a missing file or key keeps the
/// constant). Names this CS build lacks are dropped at write time. `fields` = the reflected names of the program
/// the values are for ([`reflected_fields`]); `head` = the draw is a head (per-geometry skin tiling, P1-D).
pub fn feature_data(
    s: &CsSettings,
    _fields: &BTreeSet<String>,
    _features_on: bool,
    _head: bool,
) -> Vec<(String, Vec<f64>)> {
```

`crates/render3d/src/draw.rs`, in `pub struct ShapeDraw`, after the `pub stale: bool,` field:

```rust
    /// The program's reflected cbuffer names, namespace-stripped (VS + PS): what `cs_settings::feature_data` keys
    /// its CS-build exceptions on (P1-D).
    pub fields: std::collections::BTreeSet<String>,
```

`crates/render3d/src/lib.rs`:
1. Change `use std::collections::{BTreeMap, HashMap, HashSet};` → `use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};`
2. Delete the field and its doc:

```rust
    /// FeatureData values every static block writes ([`cs_settings::feature_data`]); typed at write time from
    /// the reflection. Default: the capture-11178 constants.
    feature_values: Vec<(String, Vec<f64>)>,
```

3. In `with_gpu`'s `Ok(Renderer {` literal, delete `feature_values: cs_settings::feature_data(&cs_settings::CsSettings::default()),`.
4. In `set_settings`, delete `self.feature_values = cs_settings::feature_data(&s);` and change its doc's second sentence to "The FeatureData values are recomputed per draw."
5. In `rebuild_device_on`, delete `n.feature_values = std::mem::take(&mut self.feature_values);`.
6. In `rewrite_static`, replace `d.rewrite_material(&s.material, ll, &self.feature_values);` with:

```rust
                let fv = values_for(&self.settings, self.features_on, &d.fields, &s.material);
                d.rewrite_material(&s.material, ll, &fv);
```

7. In `build_draw`, replace

```rust
        let ll = self.linear_lighting();
        for cb in vs_cbs.iter_mut().chain(ps_cbs.iter_mut()) {
            write_material(&mut cb.writer, m, &bound, ll);
            scene::write_static(&mut cb.writer, &self.feature_values);
```

with

```rust
        let ll = self.linear_lighting();
        let fields = program_fields(program);
        let fv = values_for(&self.settings, self.features_on, &fields, m);
        for cb in vs_cbs.iter_mut().chain(ps_cbs.iter_mut()) {
            write_material(&mut cb.writer, m, &bound, ll);
            scene::write_static(&mut cb.writer, &fv);
```

and in the `Ok(ShapeDraw {` literal below it, after `stale: true,`, add `fields,`.
8. Replace `d.rewrite_material(m, ll, &self.feature_values);` with

```rust
        let fv = values_for(&self.settings, self.features_on, &d.fields, m);
        d.rewrite_material(m, ll, &fv);
```

9. Replace `d.rewrite_material(&material, ll, &self.feature_values);` with

```rust
                let fv = values_for(&self.settings, self.features_on, &d.fields, &material);
                d.rewrite_material(&material, ll, &fv);
```

10. Replace `s.update_by_name(gpu, scene, &view, &self.feature_values)` with

```rust
                    {
                        // Test hook only (P0's reference writer): recomputed every frame.
                        let fv = values_for(&self.settings, self.features_on, &s.fields, &s.built);
                        s.update_by_name(gpu, scene, &view, &fv)
                    }
```

11. Add the free functions after `fn rfaos_candidates` (top of file):

```rust
/// The reflected names of a program, both stages ([`cs_settings::reflected_fields`]).
fn program_fields(p: &Program) -> BTreeSet<String> {
    cs_settings::reflected_fields(p.vs.reflection.cbuffers.iter().chain(&p.ps.reflection.cbuffers))
}

/// One draw's FeatureData values: the settings against that draw's program (P1-D: exceptions keyed on its fields).
fn values_for(
    s: &CsSettings,
    features_on: bool,
    fields: &BTreeSet<String>,
    m: &MaterialState,
) -> Vec<(String, Vec<f64>)> {
    cs_settings::feature_data(s, fields, features_on, is_head(m))
}

/// CS's head (bodyTiling = 1): a geometry under a `BSFaceGenNiNode` (advanced-skin Renderer.cpp:133-134). That
/// node exists only at runtime, so the preview uses the FaceGen shader type, the old shader's own head split
/// (1.8 Lighting.hlsl:2047).
/// shortcut: type-5 head parts (mouth: capture 11178 ev 4269, tiling 10/1) count as body; upgrade with a
/// "loaded from a facegendata NIF / root named BSFaceGenNiNodeSkinned" flag at parse time if a user reports it.
fn is_head(m: &MaterialState) -> bool {
    m.shader_type == 4
}
```

`Program` (`use draw::{…, Program, ShapeDraw}`) and `MaterialState` are already imported in lib.rs.

- [ ] **Step 5: Run the tests**

Run: `cargo test -p render3d`
Expected: all pass, including the 3 new tests, `owner_settings_reproduce_capture_values` and the `settings.rs` WARP tests (no behaviour change).

- [ ] **Step 6: Gates and commit**

```powershell
cargo fmt --all; cargo clippy --workspace --all-targets -- -D warnings; cargo test --workspace
git add crates/render3d/src/cs_settings.rs crates/render3d/src/draw.rs crates/render3d/src/lib.rs crates/render3d/tests/cs_settings.rs crates/render3d/tests/frame.rs crates/render3d/tests/compile.rs crates/render3d/tests/cs_versions.rs crates/render3d/tests/fixtures/cs_versions
git commit -m "render3d: per-draw FeatureData from the program's reflected fields; CS build name fixtures (P1-D)" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: Linear Lighting rules keyed on `diffuseCurve` / `isDirLightLinear`

**Files**
- Modify: `crates/render3d/src/cs_settings.rs` (`NEW_LL`, `OLD_LL`, `linear_lighting_on`, LL block of `feature_data`)
- Modify: `crates/render3d/src/lib.rs` (`old_ll` memo, `linear_lighting`, `apply_settings_defines`, `wanted`, `set_shaders`)
- Test: `crates/render3d/tests/cs_versions.rs`, `crates/render3d/tests/settings.rs`

**Interfaces**
- Consumes: Task 1 `feature_data(s, fields, features_on, head)` and the cs_versions.rs helpers `fields_of`, `build`, `value`, `BUILDS` (this task adds `with`, `bits`, `PI`).
- Produces:
  - `pub const NEW_LL: &str = "linearLightingSettings.diffuseCurve";`
  - `pub const OLD_LL: &str = "linearLightingSettings.isDirLightLinear";`
  - `pub fn linear_lighting_on(s: &CsSettings, old_ll: bool, features_on: bool) -> bool`
  - Renderer private: `old_ll: std::cell::OnceCell<bool>`, `fn old_ll(&self) -> bool`

- [ ] **Step 1: Write the failing tests**

Append to `crates/render3d/tests/cs_versions.rs` (and change its first `use` line to `use render3d::cs_settings::{feature_data, linear_lighting_on, read_settings, CsSettings, SettingsPaths};`):

```rust
/// Settings as `read_settings` would give them for a file with exactly `json`.
fn with(json: serde_json::Value) -> CsSettings {
    let merged = json.as_object().unwrap().clone();
    let linear_lighting = merged
        .get("Linear Lighting")
        .and_then(|l| l.get("enableLinearLighting"))
        .and_then(|v| v.as_f64())
        .is_none_or(|x| x != 0.0);
    CsSettings {
        merged,
        linear_lighting,
        ..CsSettings::default()
    }
}

const PI: f64 = std::f32::consts::PI as f64;

/// New LL (diffuseCurve reflected): the CPU uploads π × pointLightMult (dev LinearLighting.cpp:228-229).
#[test]
fn new_linear_lighting_uploads_pi_times_the_setting() {
    let s = with(serde_json::json!({"Linear Lighting": {"enableLinearLighting": 1, "pointLightMult": 2.0}}));
    let fd = feature_data(&s, &fields_of("aio190"), true, false);
    assert_eq!(value(&fd, "linearLightingSettings.pointLightMult"), [PI * 2.0]);
    assert_eq!(
        value(&fd, "linearLightingSettings.directionalLightMult"),
        [1.0],
        "directional stays raw (capture 11178)"
    );
}

/// New LL inactive (setting off, or the features toggle off): every multiplier and fireEffectCurve = 1.0.
#[test]
fn new_linear_lighting_off_writes_every_multiplier_as_one() {
    let f = fields_of("aio190");
    let off = with(serde_json::json!({"Linear Lighting": {"enableLinearLighting": 0, "glowmapMult": 0.66}}));
    let on = with(serde_json::json!({"Linear Lighting": {"enableLinearLighting": 1, "glowmapMult": 0.66}}));
    for (s, features_on) in [(&off, true), (&on, false)] {
        let fd = feature_data(s, &f, features_on, false);
        let mults: Vec<&String> = f
            .iter()
            .filter(|n| n.starts_with("linearLightingSettings.") && (n.ends_with("Mult") || n.ends_with("fireEffectCurve")))
            .collect();
        assert_eq!(mults.len(), 14, "13 *Mult + fireEffectCurve: {mults:?}");
        for n in mults {
            assert_eq!(value(&fd, n), [1.0], "{n}");
        }
    }
    let fd = feature_data(&on, &f, true, false);
    assert_eq!(bits(&value(&fd, "linearLightingSettings.glowmapMult")), [0.66f32.to_bits()]);
}

fn bits(v: &[f64]) -> Vec<u32> {
    v.iter().map(|x| (*x as f32).to_bits()).collect()
}

/// Old LL (isDirLightLinear reflected): raw multipliers (the shader applies π), the engine fields and b8
/// emissiveMult set, and the LL flag = the setting; no JSON (Nexus) = off.
#[test]
fn old_linear_lighting_writes_raw_values_and_the_engine_fields() {
    let f = fields_of("cs18_bottle");
    let on = with(serde_json::json!({"Linear Lighting": {"enableLinearLighting": 1, "pointLightMult": 1.0}}));
    let fd = feature_data(&on, &f, true, false);
    assert_eq!(value(&fd, "linearLightingSettings.pointLightMult"), [1.0]);
    assert_eq!(value(&fd, "linearLightingSettings.enableLinearLighting"), [1.0]);
    assert_eq!(value(&fd, "linearLightingSettings.isDirLightLinear"), [0.0]);
    assert_eq!(value(&fd, "linearLightingSettings.dirLightMult"), [1.0]);
    assert_eq!(value(&fd, "emissiveMult"), [1.0]);
    let toggled_off = feature_data(&on, &f, false, false);
    assert_eq!(value(&toggled_off, "linearLightingSettings.enableLinearLighting"), [0.0]);
    let none = CsSettings::default();
    assert!(!linear_lighting_on(&none, true, true), "no JSON: the old shader's flag reads 0");
    assert!(linear_lighting_on(&none, false, true), "new LL keeps the P0 default");
    let fd = feature_data(&none, &f, true, false);
    assert_eq!(value(&fd, "linearLightingSettings.enableLinearLighting"), [0.0]);
    assert_eq!(value(&fd, "linearLightingSettings.pointLightMult"), [1.0], "not the capture's π");
}

/// Each build triggers exactly its own rule (fixtures: every pointLightMult key is 1).
#[test]
fn each_build_triggers_its_own_linear_lighting_rule() {
    for tag in BUILDS {
        let (f, s) = build(tag);
        let fd = feature_data(&s, &f, true, false);
        let point = value(&fd, "linearLightingSettings.pointLightMult")[0];
        if tag == "aio190" {
            assert_eq!(point, PI, "{tag}");
            assert!(!fd.iter().any(|(n, _)| n == "emissiveMult"), "{tag}");
        } else {
            assert_eq!(point, 1.0, "{tag}");
            assert_eq!(value(&fd, "linearLightingSettings.dirLightMult"), [1.0], "{tag}");
            assert_eq!(value(&fd, "emissiveMult"), [1.0], "{tag}");
        }
    }
}
```

Append to `crates/render3d/tests/settings.rs` (add `standin_root` to the `use common::{…}` list):

```rust
/// P1-D: an old Linear Lighting build (its source names `isDirLightLinear`) never gets `-D ENABLE_LL`: the shader
/// aliases ENABLE_LL to the cbuffer flag (old Color.hlsli:7), which feature_data writes from the setting.
#[test]
fn an_old_linear_lighting_build_compiles_without_the_enable_ll_define() {
    let cache = tempfile::tempdir().unwrap();
    let old = tempfile::tempdir().unwrap();
    let src = std::fs::read_to_string(standin_root().join("Lighting.hlsl")).unwrap();
    std::fs::write(
        old.path().join("Lighting.hlsl"),
        // A declaration, not a comment, so the marker survives any comment handling in the expander.
        src + "\nstatic const uint isDirLightLinear = 0; // P1-D old-LL marker\n",
    )
    .unwrap();
    let ll_defined = |root: &Path| -> Vec<bool> {
        let fp = render3d::locate::fingerprint(root, None).unwrap();
        let mut r = Renderer::with_gpu(Gpu::new_warp().unwrap(), fp, cache.path().to_path_buf()).unwrap();
        r.set_features(FLAT.iter().map(|s| s.to_string()).collect());
        r.set_settings(settings(true, 1.0));
        assert!(r.features().iter().any(|f| f == "ENABLE_LL"), "the settings still say on");
        let rep = r.load_parsed(&parse(&fixture("TestNifFile_Furniture_Col_SE.nif")), &none);
        assert!(rep.errors.is_empty() && rep.compile_errors.is_empty(), "{rep:?}");
        let ids: Vec<_> = r.shapes().into_iter().map(|s| s.id).collect();
        ids.into_iter()
            .map(|id| r.draw_diag(id).unwrap().0.iter().any(|(n, _)| n == "ENABLE_LL"))
            .collect()
    };
    assert_eq!(ll_defined(&standin_root()), [true, true], "new-LL stand-in: defined");
    assert_eq!(ll_defined(old.path()), [false, false], "old LL: never a -D");
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p render3d --test cs_versions --test settings`
Expected: compile error `no function linear_lighting_on`. After a stub exists, the failures are the π/1.0 assertions and `[false, false]`.

- [ ] **Step 3: Implement `cs_settings.rs`**

Add after `SECTIONS`:

```rust
/// Reflected only by the new Linear Lighting (AIO 1.9.0): π on the CPU, LL switched by a compile-time define.
pub const NEW_LL: &str = "linearLightingSettings.diffuseCurve";
/// Reflected only by the old Linear Lighting (1.6, 1.8, Nexus 1.9.1): π in the shader, LL switched by a cbuffer flag.
pub const OLD_LL: &str = "linearLightingSettings.isDirLightLinear";

/// Whether Linear Lighting is on in the preview. Old LL: the setting, with no key = off (the shader's flag reads the
/// written 0, Nexus before CS's first run). New LL: `CsSettings::linear_lighting` (no key = the P0 default, on).
pub fn linear_lighting_on(s: &CsSettings, old_ll: bool, features_on: bool) -> bool {
    features_on
        && if old_ll {
            setting(&s.merged, "Linear Lighting", "enableLinearLighting").is_some_and(|x| x != 0.0)
        } else {
            s.linear_lighting
        }
}
```

In `feature_data`, rename `_fields` → `fields` and `_features_on` → `features_on`. Replace

```rust
    // C4 / spec §2.2: the installed CS uploads π whatever the setting says (capture 11178).
    put(
        &mut out,
        "linearLightingSettings.pointLightMult",
        vec![f64::from(std::f32::consts::PI)],
    );
```

with

```rust
    let ll_on = linear_lighting_on(s, fields.contains(OLD_LL), features_on);
    if fields.contains(NEW_LL) {
        // New LL: the CPU uploads π × pointLightMult and the shader no longer multiplies (dev LinearLighting.cpp:
        // 228-229; capture 11178: setting 1 → π). Directional stays raw: the capture shows 1.0.
        let point = f64::from(std::f32::consts::PI) * ll("pointLightMult").unwrap_or(1.0);
        put(&mut out, "linearLightingSettings.pointLightMult", vec![point]);
        if !ll_on {
            // From dev source only (LinearLighting.cpp:207-225, 2026-10-09), not verified against the installed
            // DLL: with LL inactive CS uploads every multiplier and fireEffectCurve as 1.0; the 1.9 shader reads
            // them ungated (ColorManagement.hlsli:61-137).
            for f in fields.iter().filter(|f| {
                f.strip_prefix("linearLightingSettings.")
                    .is_some_and(|m| m.ends_with("Mult") || m == "fireEffectCurve")
            }) {
                put(&mut out, f, vec![1.0]);
            }
        }
    } else if fields.contains(OLD_LL) {
        // Old LL: raw multipliers, the shader applies π itself (Color.hlsli:270-278). isDirLightLinear and
        // dirLightMult are engine-derived (advanced-skin LinearLighting.cpp:125-133); 0 would black out the sun
        // (1.8 Lighting.hlsl:2730). The flag is the LL switch (Color.hlsli:7).
        put(&mut out, "linearLightingSettings.pointLightMult", vec![ll("pointLightMult").unwrap_or(1.0)]);
        put(
            &mut out,
            "linearLightingSettings.enableLinearLighting",
            vec![f64::from(u8::from(ll_on))],
        );
        put(&mut out, "linearLightingSettings.isDirLightLinear", vec![0.0]);
        put(&mut out, "linearLightingSettings.dirLightMult", vec![1.0]);
        // b8 LLPerGeometry (Color.hlsli:10-14): 0 turns emissive black (Color.hlsli:283).
        put(&mut out, "emissiveMult", vec![1.0]);
    }
```

The `ll` closure is already defined above this block (`let ll = |k: &str| setting(&s.merged, "Linear Lighting", k);`).

- [ ] **Step 4: Implement `lib.rs`**

1. In `pub struct Renderer`, after `features_on: bool,`:

```rust
    /// The shaders are an old Linear Lighting build (P1-D); memoised from the expanded source, reset by `set_shaders`.
    old_ll: std::cell::OnceCell<bool>,
```

and in the `Ok(Renderer {` literal, after `features_on: true,` add `old_ll: std::cell::OnceCell::new(),`. In `rebuild_device_on` nothing changes: the fresh renderer re-reads it.
2. Replace

```rust
    fn linear_lighting(&self) -> bool {
        self.features.iter().any(|f| f == "ENABLE_LL")
    }
```

with

```rust
    /// Linear Lighting on in the preview: the `ENABLE_LL` define, or on an old-LL build the cbuffer flag that
    /// `feature_data` writes (that build never gets the define).
    fn linear_lighting(&self) -> bool {
        if self.old_ll() {
            cs_settings::linear_lighting_on(&self.settings, true, self.features_on)
        } else {
            self.features.iter().any(|f| f == "ENABLE_LL")
        }
    }

    /// The shaders name `isDirLightLinear` (1.6, 1.8, Nexus 1.9.1). Read from the expanded `Lighting.hlsl` because
    /// the define list is decided before any program (and its reflection) exists. An unreadable source is not
    /// memoised.
    fn old_ll(&self) -> bool {
        if let Some(v) = self.old_ll.get() {
            return *v;
        }
        match self.sources.get(Path::new("Lighting.hlsl")) {
            Ok(src) => *self.old_ll.get_or_init(|| src.text.contains("isDirLightLinear")),
            Err(_) => false,
        }
    }
```

3. In `apply_settings_defines`, replace

```rust
        self.encode_srgb = f.iter().any(|d| d == "ENABLE_LL");
        self.features = f;
```

with

```rust
        self.features = f;
        self.encode_srgb = self.linear_lighting();
```

(keep the comment line above it).
4. In `wanted`, replace `let f: Vec<&str> = self.features.iter().map(String::as_str).collect();` with

```rust
        let old_ll = self.old_ll();
        // Old LL: ENABLE_LL is the shader's own alias of the cbuffer flag (Color.hlsli:7), never a -D (P1-D).
        let f: Vec<&str> = self
            .features
            .iter()
            .map(String::as_str)
            .filter(|d| !(old_ll && *d == "ENABLE_LL"))
            .collect();
```

5. In `set_shaders`, after `self.programs.clear();` add `self.old_ll = std::cell::OnceCell::new();`.

- [ ] **Step 5: Run the tests**

Run: `cargo test -p render3d`
Expected: all pass, including `owner_settings_reproduce_capture_values` (π × 1) and the 5 new tests.

- [ ] **Step 6: Gates and commit**

```powershell
cargo fmt --all; cargo clippy --workspace --all-targets -- -D warnings; cargo test --workspace
git add crates/render3d/src/cs_settings.rs crates/render3d/src/lib.rs crates/render3d/tests/cs_versions.rs crates/render3d/tests/settings.rs
git commit -m "render3d: Linear Lighting rules keyed on the reflected generation (P1-D)" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: skin — flat or `DefaultProfile`, old or new packing, head tiling

**Files**
- Modify: `crates/render3d/src/cs_settings.rs` (`pack_skin`, skin part of `feature_data`)
- Test: `crates/render3d/tests/cs_versions.rs`

**Interfaces**
- Consumes: Task 1 `feature_data(…, head)`, `is_head` (already wired in lib.rs), `fields_of`, `value`; Task 2 `with`.
- Produces: `fn pack_skin(skin: &Map<String, Value>, new: bool, out: &mut [(String, Vec<f64>)]) -> f64` (private; returns SkinDetailTiling).

- [ ] **Step 1: Write the failing tests**

Append to `crates/render3d/tests/cs_versions.rs`:

```rust
/// 1.6/1.8 write the skin keys flat; CS reads the section root when there is no DefaultProfile (advanced-skin
/// Skin.cpp:22). Old packing: x = tiling (the shader multiplies by y, 1.8 Lighting.hlsl:2049), w = detail && skin,
/// sssParams.z and physicalParams.w unused (0).
#[test]
fn flat_skin_settings_are_read_like_cs_does() {
    let s = with(serde_json::json!({"Advanced Skin": {
        "EnableSkin": true, "SkinMainRoughness": 0.5, "EnableSkinDetail": true, "SkinDetailTiling": 10.0,
        "BodyTilingMultiplier": 2.0, "SkinDetailStrength": 0.3, "Translucency": 0.25
    }}));
    let fd = feature_data(&s, &fields_of("cs18_bottle"), true, false);
    assert_eq!(value(&fd, "skinData.skinParams")[0], 0.5);
    assert_eq!(value(&fd, "skinData.skinDetailParams"), [10.0, 2.0, 0.3, 1.0]);
    assert_eq!(value(&fd, "skinData.sssParams")[0], 0.25);
    assert_eq!(value(&fd, "skinData.sssParams")[2], 0.0);
    assert_eq!(value(&fd, "skinData.physicalParams")[3], 0.0);
    let skin_off = with(serde_json::json!({"Advanced Skin": {"EnableSkin": false, "EnableSkinDetail": true}}));
    let fd = feature_data(&skin_off, &fields_of("cs18_bottle"), true, false);
    assert_eq!(value(&fd, "skinData.skinDetailParams")[3], 0.0, "old w = EnableSkinDetail && EnableSkin");
}

/// Golden, capture 11178 (owner settings, installed build): body b7 skinDetailParams (20, 2, 0.25, 0) at ev 4386,
/// head (FaceGen, ev 4182) (10, 1, 0.25, 0); FeatureData b6 stays the body value for both (GetCommonBufferData).
#[test]
fn new_packing_premultiplies_and_heads_use_tiling_one() {
    let owner = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/cs_settings"));
    let s = read_settings(&SettingsPaths {
        user: Some(owner.join("SettingsUser.json")),
        default: Some(owner.join("SettingsDefault.json")),
        ..Default::default()
    });
    let f = fields_of("aio190");
    let body = feature_data(&s, &f, true, false);
    let head = feature_data(&s, &f, true, true);
    assert_eq!(value(&body, "skinPerGeometryProfile.skinDetailParams"), [20.0, 2.0, 0.25, 0.0]);
    assert_eq!(value(&head, "skinPerGeometryProfile.skinDetailParams"), [10.0, 1.0, 0.25, 0.0]);
    assert_eq!(value(&head, "skinData.skinDetailParams"), [20.0, 2.0, 0.25, 0.0]);
}

/// Each build packs tiling its own way, also with no skin section at all (old shaders would double the capture's
/// pre-multiplied 20).
#[test]
fn each_build_packs_skin_tiling_its_own_way() {
    for tag in BUILDS {
        let f = fields_of(tag);
        for s in [CsSettings::default(), with(serde_json::json!({"Advanced Skin": {"SkinDetailTiling": 10.0, "BodyTilingMultiplier": 2.0}}))] {
            let fd = feature_data(&s, &f, true, false);
            let want = if tag == "aio190" { 20.0 } else { 10.0 };
            assert_eq!(value(&fd, "skinData.skinDetailParams")[0], want, "{tag}");
        }
    }
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p render3d --test cs_versions`
Expected: FAIL. The flat test reads 0.7/20 (the capture constants), the head test reads `[20, 2, …]`, and the old builds read x = 20.

- [ ] **Step 3: Implement**

In `feature_data`, rename `_head` → `head`, and replace

```rust
    if let Some(Value::Object(skin)) = s.merged.get("Advanced Skin") {
        pack_skin(skin, &mut out);
    }
```

with

```rust
    // New packing iff the build reflects the per-geometry profile (AIO 1.9.0); always packed, so an old build with
    // no skin section still gets the un-multiplied tiling.
    let new_skin = fields.contains("skinPerGeometryProfile.skinParams");
    let empty = Map::new();
    let skin = match s.merged.get("Advanced Skin") {
        Some(Value::Object(m)) => m,
        _ => &empty,
    };
    let tiling = pack_skin(skin, new_skin, &mut out);
```

and after the line `out.extend(profile);` add

```rust
    if head && new_skin {
        // CS: bodyTiling = 1 on a head (advanced-skin Renderer.cpp:263-264); FeatureData keeps the body value
        // (GetCommonBufferData passes false).
        if let Some((_, v)) = out
            .iter_mut()
            .find(|(n, _)| n == "skinPerGeometryProfile.skinDetailParams")
        {
            v[0] = tiling;
            v[1] = 1.0;
        }
    }
```

Replace the head of `pack_skin` (doc, signature, the `profile`/`p`/`top` closures, the `shortcut:` comment and `let tiling = …;`) with:

```rust
/// `"Advanced Skin"` → `skinData.*` (C5). The profile is `DefaultProfile`, else the section itself (CS's own
/// migration, advanced-skin Skin.cpp:22: 1.6/1.8 files are flat with the same key names). `new` (the build reflects
/// `skinPerGeometryProfile`): advanced-skin Renderer.cpp:260-268, the CPU pre-multiplies the tiling. Otherwise dev
/// Skin.cpp:267-273: the shader multiplies x·y itself (1.8 Lighting.hlsl:2049). Returns SkinDetailTiling (for the
/// head copy). A key the files lack keeps the capture-11178 component.
fn pack_skin(skin: &Map<String, Value>, new: bool, out: &mut [(String, Vec<f64>)]) -> f64 {
    let profile = match skin.get("DefaultProfile") {
        Some(Value::Object(d)) => d,
        _ => skin,
    };
    let p = |k: &str| profile.get(k).and_then(num);
    let top = |k: &str| skin.get(k).and_then(num);
    // Capture 11178: x = 20 = 10 × 2.
    let tiling = p("SkinDetailTiling").unwrap_or(10.0);
    let body = p("BodyTilingMultiplier").unwrap_or(2.0);
    let (detail_x, detail_on, sss_amount, wet_response) = if new {
        (tiling * body, p("EnableSkinDetail"), p("SSSAmount"), p("WetResponse"))
    } else {
        let on = p("EnableSkinDetail")
            .zip(top("EnableSkin"))
            .map(|(d, e)| f64::from(u8::from(d != 0.0 && e != 0.0)));
        (tiling, on, Some(0.0), Some(0.0))
    };
```

In the `packed` array change these four entries:
- `skinDetailParams`: `[tiling, p("BodyTilingMultiplier"), p("SkinDetailStrength"), p("EnableSkinDetail")]` → `[Some(detail_x), Some(body), p("SkinDetailStrength"), detail_on]`
- `sssParams`: `p("SSSAmount"),` → `sss_amount,`
- `physicalParams`: `p("WetResponse"),` → `wet_response,`

Update their comments: `// x detail UV tiling (new: × body, :1673; old: the shader × y), y body tiling …` and `// … w wet response (new only, :2216; old 0)`. At the end of `pack_skin`, after the `WetParams` block, add `tiling` as the return value.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p render3d`
Expected: all pass, including `owner_section_keys_map_to_struct_fields` (`[20, 2, 0.25, 0]`), `default_settings_reproduce_todays_constants` (profile == skinData for a non-head) and the 3 new tests.

- [ ] **Step 5: Gates and commit**

```powershell
cargo fmt --all; cargo clippy --workspace --all-targets -- -D warnings; cargo test --workspace
git add crates/render3d/src/cs_settings.rs crates/render3d/tests/cs_versions.rs
git commit -m "render3d: skin settings flat or DefaultProfile, packing and head tiling by reflected shape (P1-D)" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: coverage report — `settings_coverage` log event and one panel line

**Files**
- Modify: `crates/render3d/src/cs_settings.rs` (`Coverage`, `coverage`, `NO_FIELD_KEYS`, `SKIN_KEYS`, `renamed`)
- Modify: `crates/render3d/src/lib.rs` (`coverage` map, `note_coverage`, `settings_coverage`)
- Modify: `crates/gui/src/model/viewport3d.rs` (`V3Reply::Coverage`, `Viewport3d.coverage`, render-thread send)
- Modify: `crates/gui/src/ui/viewport3d.rs` (footer line; the set preview shares this footer, so `ui/set_preview.rs` is unchanged)
- Test: `crates/render3d/tests/cs_versions.rs`, `crates/render3d/tests/settings.rs`, `crates/gui/tests/viewport3d.rs`

**Interfaces**
- Consumes: Tasks 1–3.
- Produces:
  - `#[derive(Debug, Clone, Default, PartialEq)] pub struct Coverage { pub unset: BTreeSet<String>, pub unmatched: BTreeSet<String> }` with `pub fn line(&self) -> Option<String>` = `"CS settings: {N} fields unset, {M} keys unmatched"` when either is non-empty.
  - `pub fn coverage(s: &CsSettings, fields: &BTreeSet<String>, written: &[(String, Vec<f64>)]) -> Coverage`
  - `Renderer::settings_coverage(&self) -> cs_settings::Coverage` (union over the programs at hand)
  - perf_log event `{"event":"settings_coverage","key":"<12 hex>","unset":[…],"unmatched":[…]}`, once per program when it is created and once per program on every `set_settings`
  - `V3Reply::Coverage(Option<String>)`, `Viewport3d.coverage: Option<String>`

- [ ] **Step 1: Write the failing tests**

Append to `crates/render3d/tests/cs_versions.rs` (add `coverage, Coverage` to the `use render3d::cs_settings::{…}` list):

```rust
/// 03 report §6.3: with the rules of Tasks 2-3 every build's own keys land; Nexus has no JSON, so the old-LL
/// gammas and the ExtMat/ExtTrans fields stay unset (zero), the honest answer.
#[test]
fn coverage_per_build() {
    for tag in BUILDS {
        let (f, s) = build(tag);
        let c = coverage(&s, &f, &feature_data(&s, &f, true, false));
        assert!(c.unmatched.is_empty(), "{tag}: {:?}", c.unmatched);
        if tag == "nexus191" {
            let want: BTreeSet<String> = [
                "extendedMaterialSettings.EnableComplexMaterial", "extendedMaterialSettings.EnableHeightBlending",
                "extendedMaterialSettings.EnableParallax", "extendedMaterialSettings.EnableParallaxWarpingFix",
                "extendedMaterialSettings.EnableShadows", "extendedMaterialSettings.EnableTerrainParallax",
                "extendedTranslucencySettings.MaterialModel", "extendedTranslucencySettings.Reduction",
                "extendedTranslucencySettings.Softness", "extendedTranslucencySettings.Strength",
                "linearLightingSettings.ambientGamma", "linearLightingSettings.colorGamma",
                "linearLightingSettings.effectAlphaGamma", "linearLightingSettings.effectGamma",
                "linearLightingSettings.emitColorGamma", "linearLightingSettings.emitColorMult",
                "linearLightingSettings.fogAlphaGamma", "linearLightingSettings.fogGamma",
                "linearLightingSettings.glowmapGamma", "linearLightingSettings.lightGamma",
                "linearLightingSettings.skyGamma", "linearLightingSettings.vanillaDiffuseColorMult",
                "linearLightingSettings.vlGamma", "linearLightingSettings.waterGamma",
            ]
            .map(String::from)
            .into();
            assert_eq!(c.unset, want);
        } else {
            assert_eq!(c, Coverage::default(), "{tag}");
            assert_eq!(c.line(), None);
        }
    }
}

/// Drift shows: an old-generation key on the new build is unmatched, a reflected field nothing writes is unset;
/// pads, struct-less sections and known JSON-only keys stay quiet.
#[test]
fn coverage_names_the_drift() {
    // The installed build's own (clean) key set plus one drifted field and two drifted keys.
    let (mut f, mut s) = build("aio190");
    f.insert("linearLightingSettings.newThing".into());
    let section = |s: &CsSettings, name: &str| -> serde_json::Map<String, serde_json::Value> {
        s.merged[name].as_object().unwrap().clone()
    };
    let mut ll = section(&s, "Linear Lighting");
    ll.insert("lightGamma".into(), 2.2.into());
    s.merged.insert("Linear Lighting".into(), ll.into());
    let mut skin = section(&s, "Advanced Skin");
    skin["DefaultProfile"]
        .as_object_mut()
        .unwrap()
        .insert("NewSkinKnob".into(), 1.0.into());
    s.merged.insert("Advanced Skin".into(), skin.into());
    let c = coverage(&s, &f, &feature_data(&s, &f, true, false));
    assert_eq!(c.unset, BTreeSet::from(["linearLightingSettings.newThing".to_string()]));
    assert_eq!(
        c.unmatched,
        BTreeSet::from(["Advanced Skin.NewSkinKnob".to_string(), "Linear Lighting.lightGamma".to_string()])
    );
    assert_eq!(c.line().as_deref(), Some("CS settings: 1 fields unset, 2 keys unmatched"));
}
```

Append to `crates/render3d/tests/settings.rs` (add `use render3d::perf_log;`):

```rust
/// P1-D: one `settings_coverage` line per program when it is created and per program on every apply; the renderer's
/// union feeds the panel line.
#[test]
fn every_program_reports_its_settings_coverage() {
    let cache = tempfile::tempdir().unwrap();
    let mut s = settings(true, 1.0);
    s.merged
        .get_mut("Linear Lighting")
        .unwrap()
        .as_object_mut()
        .unwrap()
        .insert("bogusKey".into(), 1.into());
    let (mut r, ev) = perf_log::capture(|| furniture(cache.path(), FEATURE_DATA, s));
    let lines = |ev: &[serde_json::Value]| -> Vec<serde_json::Value> {
        ev.iter().filter(|e| e["event"] == "settings_coverage").cloned().collect()
    };
    let first = lines(&ev);
    assert!(!first.is_empty(), "{ev:?}");
    for e in &first {
        assert_eq!(e["unmatched"], serde_json::json!(["Linear Lighting.bogusKey"]), "{e}");
        assert_eq!(e["unset"], serde_json::json!([]), "{e}");
        assert_eq!(e["key"].as_str().unwrap().len(), 12, "{e}");
    }
    assert_eq!(
        r.settings_coverage().line().as_deref(),
        Some("CS settings: 0 fields unset, 1 keys unmatched")
    );
    let ((), ev) = perf_log::capture(|| r.set_settings(settings(true, 0.5)));
    let again = lines(&ev);
    assert_eq!(again.len(), first.len(), "once per program per apply");
    assert!(again.iter().all(|e| e["unmatched"] == serde_json::json!([])));
    assert_eq!(r.settings_coverage().line(), None);
}
```

Append to `crates/gui/tests/viewport3d.rs`:

```rust
/// P1-D: the coverage line shows under the frame only when the renderer reports one.
#[test]
fn the_coverage_line_shows_only_when_reported() {
    let mut v = Viewport3d::with_state(V3State::Ready);
    v.handle(v3::V3Reply::Coverage(Some(
        "CS settings: 3 fields unset, 1 keys unmatched".into(),
    )));
    let mut h = harness(v);
    h.run();
    h.get_by_label_contains("CS settings: 3 fields unset, 1 keys unmatched");
    let mut h = harness(Viewport3d::with_state(V3State::Ready));
    h.run();
    assert!(h.query_by_label_contains("CS settings:").is_none());
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p render3d --test cs_versions --test settings` and `cargo test -p sdc-gui --test viewport3d the_coverage_line`
Expected: compile errors (`coverage`, `Coverage`, `settings_coverage`, `V3Reply::Coverage` not found).

- [ ] **Step 3: Implement `cs_settings.rs`**

Add after `SECTIONS`:

```rust
/// Settings keys with no FeatureData field by design (03 report §3, §6.3): CPU-only (`SkinnedOnly`, the sweat
/// timers, `DetailTexture`, `EnableVerboseJsonLogging`), consumed by a derivation (`diffuseMidReflectance` →
/// diffuseCurve, `enableLinearLighting` → the LL switch) or unused by the shader (`enableACEScg`, `gameGamma`,
/// 1.9 `emitColorMult`, `UseDynamicWetness`).
const NO_FIELD_KEYS: &[&str] = &[
    "SkinnedOnly",
    "enableACEScg",
    "gameGamma",
    "emitColorMult",
    "diffuseMidReflectance",
    "enableLinearLighting",
    "EnableVerboseJsonLogging",
    "DetailTexture",
    "FullSweat",
    "StartSweat",
    "WetFadeTime",
    "UseDynamicWetness",
];

/// Every `"Advanced Skin"` key [`pack_skin`] reads (profile or section root).
const SKIN_KEYS: &[&str] = &[
    "EnableSkin",
    "ExtraSkinWetness",
    "WetParams",
    "SkinMainRoughness",
    "SkinSecondRoughness",
    "SkinSpecularTexMultiplier",
    "SecondarySpecularStrength",
    "F0",
    "BaseColorMultiplier",
    "SkinDetailTiling",
    "BodyTilingMultiplier",
    "SkinDetailStrength",
    "EnableSkinDetail",
    "Translucency",
    "sssWidth",
    "SSSAmount",
    "UseSSS",
    "FuzzStrength",
    "FuzzRoughness",
    "FuzzF0",
    "ExtraEdgeRoughness",
    "PhysicalMainRoughnessMultiplier",
    "PhysicalSecondRoughnessMultiplier",
    "PhysicalSpecularStrength",
    "WetResponse",
];

/// The struct field a section key lands in (C5 renames).
fn renamed<'a>(renames: &[(&str, &'a str)], key: &'a str) -> &'a str {
    renames.iter().find(|r| r.0 == key).map_or(key, |r| r.1)
}

/// What one program's settings write missed (03 report §6.3): reflected fields nothing wrote, and settings keys
/// that matched no field.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Coverage {
    pub unset: BTreeSet<String>,
    pub unmatched: BTreeSet<String>,
}

impl Coverage {
    /// The panel line; `None` when there is nothing to report.
    pub fn line(&self) -> Option<String> {
        (!self.unset.is_empty() || !self.unmatched.is_empty()).then(|| {
            format!(
                "CS settings: {} fields unset, {} keys unmatched",
                self.unset.len(),
                self.unmatched.len()
            )
        })
    }
}

/// `fields` (one program's [`reflected_fields`]) against `written` (its [`feature_data`]) and the settings. Checked
/// fields: the 7 sections' structs, `skinData`, b7 and b8, minus `pad*`. Checked keys: the 7 sections', minus
/// [`NO_FIELD_KEYS`], strings and objects; a section whose struct this build lacks is skipped.
pub fn coverage(s: &CsSettings, fields: &BTreeSet<String>, written: &[(String, Vec<f64>)]) -> Coverage {
    let under = |f: &str, p: &str| f.strip_prefix(p).is_some_and(|r| r.starts_with('.'));
    let has = |p: &str| fields.iter().any(|f| under(f.as_str(), p));
    let in_scope = |f: &str| {
        SECTIONS
            .iter()
            .map(|sec| sec.1)
            .chain(["skinData", "skinPerGeometryProfile"])
            .any(|p| under(f, p))
            || matches!(f, "skinPerGeometry" | "skinMaterialFlags" | "emissiveMult")
    };
    let is_pad = |f: &str| f.rsplit('.').next().is_some_and(|l| l.starts_with("pad"));
    let wrote: BTreeSet<&str> = written.iter().map(|(n, _)| n.as_str()).collect();
    let unset = fields
        .iter()
        .filter(|f| in_scope(f.as_str()) && !is_pad(f.as_str()) && !wrote.contains(&f.as_str()))
        .cloned()
        .collect();
    let counts = |k: &str, v: &Value| {
        matches!(v, Value::Number(_) | Value::Bool(_) | Value::Array(_)) && !NO_FIELD_KEYS.contains(&k)
    };
    let mut unmatched = BTreeSet::new();
    for (section, prefix, renames) in SECTIONS {
        let Some(Value::Object(m)) = s.merged.get(*section) else {
            continue;
        };
        if !has(prefix) {
            continue;
        }
        for (key, v) in m {
            if counts(key.as_str(), v) && !fields.contains(&format!("{prefix}.{}", renamed(renames, key))) {
                unmatched.insert(format!("{section}.{key}"));
            }
        }
    }
    if let (Some(Value::Object(skin)), true) = (s.merged.get("Advanced Skin"), has("skinData")) {
        let profile = match skin.get("DefaultProfile") {
            Some(Value::Object(d)) => Some(d),
            _ => None,
        };
        for (key, v) in skin.iter().chain(profile.into_iter().flatten()) {
            if counts(key.as_str(), v) && !SKIN_KEYS.contains(&key.as_str()) {
                unmatched.insert(format!("Advanced Skin.{key}"));
            }
        }
    }
    Coverage { unset, unmatched }
}
```

In `feature_data`'s section loop, replace

```rust
            let field = renames
                .iter()
                .find(|r| r.0 == key.as_str())
                .map_or(key.as_str(), |r| r.1);
```

with `let field = renamed(renames, key);`.

- [ ] **Step 4: Implement `lib.rs`**

1. In `pub struct Renderer`, after the `old_ll` field:

```rust
    /// The last coverage of every program at hand (P1-D), for the panel line; dropped with the programs.
    coverage: HashMap<ProgramKey, cs_settings::Coverage>,
```

and in the `Ok(Renderer {` literal add `coverage: HashMap::new(),` after `old_ll: std::cell::OnceCell::new(),`.
2. In `set_settings`, after `self.apply_settings_defines();` add:

```rust
        let at_hand: Vec<(ProgramKey, BTreeSet<String>)> = self
            .programs
            .iter()
            .filter_map(|(k, p)| p.as_ref().ok().map(|p| (k.clone(), program_fields(p))))
            .collect();
        for (k, f) in at_hand {
            self.note_coverage(k, f);
        }
```

3. After `set_settings`, add:

```rust
    /// The union of every program's coverage (P1-D): the panel shows `line()` when it is `Some`.
    pub fn settings_coverage(&self) -> cs_settings::Coverage {
        let mut all = cs_settings::Coverage::default();
        for c in self.coverage.values() {
            all.unset.extend(c.unset.iter().cloned());
            all.unmatched.extend(c.unmatched.iter().cloned());
        }
        all
    }

    /// Logs and keeps one program's coverage (`settings_coverage` line). The coverage depends only on names, so
    /// a non-head draw stands for the program.
    fn note_coverage(&mut self, key: ProgramKey, fields: BTreeSet<String>) {
        let values = cs_settings::feature_data(&self.settings, &fields, self.features_on, false);
        let c = cs_settings::coverage(&self.settings, &fields, &values);
        perf_log::emit(
            "settings_coverage",
            serde_json::json!({"key": perf_log::short(&key.0), "unset": c.unset, "unmatched": c.unmatched}),
        );
        self.coverage.insert(key, c);
    }
```

4. In `lookup`, replace

```rust
                self.programs.insert(w.key.clone(), built.clone());
```

with

```rust
                self.programs.insert(w.key.clone(), built.clone());
                if let Ok(p) = &built {
                    self.note_coverage(w.key.clone(), program_fields(p));
                }
```

5. In `set_shaders`, after `self.old_ll = std::cell::OnceCell::new();` add `self.coverage.clear();`.

- [ ] **Step 5: Implement the GUI**

`crates/gui/src/model/viewport3d.rs`:
1. In `pub enum V3Reply`, after

```rust
    /// `Renderer::off_in_preview()`, after the thread starts and after each settings or features change.
    OffInPreview(Vec<String>),
```

add

```rust
    /// `Renderer::settings_coverage().line()` whenever it changes (P1-D); `None` = nothing to report.
    Coverage(Option<String>),
```

2. In `pub struct Viewport3d`, after `pub off_in_preview: Vec<String>,` add

```rust
    /// The CS settings coverage line (`V3Reply::Coverage`), shown under the frame when `Some`.
    pub coverage: Option<String>,
```

and in its literal, after `off_in_preview: vec![],` add `coverage: None,`.
3. In `handle`, after `V3Reply::OffInPreview(off) => self.off_in_preview = off,` add `V3Reply::Coverage(c) => self.coverage = c,`.
4. In the render thread, replace

```rust
    let mut stats = FrameStats::default();
    while let Ok(first) = rx.recv() {
```

with

```rust
    let mut stats = FrameStats::default();
    let mut last_coverage: Option<String> = None;
    while let Ok(first) = rx.recv() {
```

and replace

```rust
        pump(&mut r);
        if restyled {
```

with

```rust
        pump(&mut r);
        // A settings apply or a new program can change it; sent only on change.
        let coverage = r.settings_coverage().line();
        if coverage != last_coverage {
            send(V3Reply::Coverage(coverage.clone()));
            last_coverage = coverage;
        }
        if restyled {
```

`crates/gui/src/ui/viewport3d.rs`, in `fn footer`, after `ui.weak(off_line(&v.off_in_preview, CUBEMAPS_FOOTER));` add:

```rust
        if let Some(c) = &v.coverage {
            ui.weak(format!("· {c}"));
        }
```

- [ ] **Step 6: Run the tests**

Run: `cargo test -p render3d` and `cargo test -p sdc-gui --test viewport3d`
Expected: all pass, including the 4 new tests.

- [ ] **Step 7: Gates and commit**

```powershell
cargo fmt --all; cargo clippy --workspace --all-targets -- -D warnings; cargo test --workspace
git add crates/render3d/src/cs_settings.rs crates/render3d/src/lib.rs crates/render3d/tests/cs_versions.rs crates/render3d/tests/settings.rs crates/gui/src/model/viewport3d.rs crates/gui/src/ui/viewport3d.rs crates/gui/tests/viewport3d.rs
git commit -m "render3d, gui: CS settings coverage report (settings_coverage log line, panel note) (P1-D)" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: owner checklist (manual; numbers from the timing log)

**Files**
- Modify: this plan file, section "P1-D verification" at the end (record the results). No code.

**Interfaces:** Consumes Tasks 1–4. The app writes `%LOCALAPPDATA%\sdc\logs\preview-timings-YYYYMMDD-HHMMSS.jsonl`. P1-D adds `{"event":"settings_coverage","key":"<12 hex>","unset":[…],"unmatched":[…]}` once per program on creation and on every settings apply. The owner does not time anything. Owner data stays read-only: settings change only through CS's in-game menu, never by hand.

**Golden capture:** a capture exists (`F:\ai\dds\tools\rdc-out\p04\Skyrim_AE_1.6.1170_frame11178\state_*.json`, b6 + b7 per draw). Its values are already in the repo as `FEATURE_FLOATS` (b6, checked by `owner_settings_reproduce_capture_values`) and as Task 3's head/body literals (b7, ev 4182/4386). No capture-reading test is added (the dumps are outside the repo). Re-capturing after a CS update is Step 4.

- [ ] **Step 1: Build and start.** `cargo build -p sdc-gui --release` in `F:\ai\dds\converter-nif-p1d`; start the exe from MO2 (Acherus). The set preview's "Timings" section shows "Timing log: <path>".
- [ ] **Step 2: Installed 1.9.0 is silent.** Open a set and the Meshes 3D panel, and let them draw. Expect: no "CS settings:" text under the frame. Log: every `settings_coverage` line has `"unset":[]` and `"unmatched":[]`. Count the lines: about one per distinct program (a settings apply that changes defines can log a new program twice, final review M4), and none per frame. The `settings` line `defines` are unchanged from the P1-C verification Step 2.
- [ ] **Step 3: The old builds' shaders, read-only.** With an MO2 scan target the settings always come from the MO2 origins (overwrite wins, P1-C 9b0c5d5/8340fe7), so a chosen old `Shaders` folder is paired with the owner's current 1.9.0 settings, as a real install of that build would be after CS rewrites them. The old builds' own settings are covered by the Task 1–4 fixture tests. In a viewport panel use "Shader folder…" → `G:\Acherus\mods\CommunityShaders_AIO 0528\Shaders`. Load `meshes\actors\character\character assets\femalehead.nif` and `femalebody_1.nif`. Expect: the `settings` line `defines` lack `ENABLE_LL` (old LL never gets the define); with LL on, the picture is linear-lit with no black sun and no black emissive, and point lights are not about 10× too bright; the `settings_coverage` lines list only keys the 1.6 shader lacks (record them verbatim; expected: 1.9-only keys such as Vanilla Fresnel's). Repeat with `G:\Acherus\mods\CommunityShaders_AIO_bottle\Shaders` (1.8). Restore the folder to the installed 1.9.0 `Shaders`.
- [ ] **Step 4: After each CS update (standing step, not only now).** Capture one frame in game with RenderDoc that shows a head and a body (as frame 11178). Dump it with `F:\ai\dds\tools\rdc-scripts\p04_stageB_state.py` as for p04. Then compare, for one head draw and one body draw: b6 `linearLightingSettings.pointLightMult` / `directionalLightMult` / the `*Mult` values against `crates/render3d/src/scene.rs` `FEATURE_FLOATS`, and b7 `skinPerGeometryProfile.skinDetailParams` against `[10,1,…]` (head) and `[20,2,…]` (body). Also open sdc once on the new CS and check that the `settings_coverage` lines are still empty. Any difference (for example `directionalLightMult` becoming π, as in `dev` after 10-02) means a new rule is needed: report the numbers, do not edit constants by hand.
- [ ] **Step 5: Send** the newest log file and the Step 3 notes back.

## P1-D verification

(results recorded here by the person running Task 5)

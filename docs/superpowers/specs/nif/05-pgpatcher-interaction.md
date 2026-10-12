# 05 — PGPatcher (ParallaxGen) output and sdc's MO2 resolution: is a failsafe needed?

Date: 2026-10-11. Status: research done, nothing built.
Owner request: "ensure a fail[safe] is built in for an active PGPatcher when a user launches [sdc] with MO2 to build their priority list, if that is needed or not."
Inputs: [00-decision.md](00-decision.md) §3.2–3.3, [02-mo2-resolution.md](02-mo2-resolution.md) §2, §6–7.

Tags: **[Source master]** = PGPatcher `master` at `fd2fd4c` (2026-10-10). **[Source 1.2.0]** = tag `1.2.0` (`3c43295`), the build the owner has installed (`tools\ParallaxGen\log\PGPatcher.log` line 1: "Welcome to PGPatcher version 1.2.0!"). **[Doc]** = PGPatcher wiki (cloned). **[Measured]** = read-only inspection of the owner's instance `C:\_mount\moddingvol\Acherus`, profile `Nexus`, on 2026-10-11. **[Inference]** = my reasoning.
Clones: session scratchpad `pgp\src` (source + tag 1.2.0) and `pgp\wiki`. Scripts: scratchpad `scripts\pgp_scan.py` and `pgp_mismatch2.py`. Output: `pgp_scan_out.txt`.

---

## TL;DR

1. **A failsafe is needed. The main reason is correct attribution, not crash safety.** On the owner's setup the PGPatcher output mod `ParallaxGen_Output` supplies the winning copy for **32,285 of 52,262 loose NIF paths (61.8 %)**, or 25,517 non-FaceGen paths [Measured]. Without the failsafe, sdc's "Wins / Overridden by" view says PGPatcher "owns" most of the setup, and every texture-set row describes a generated copy rather than the mod the user installed.
2. **sdc should treat PGPatcher output as a *derived layer*.** It stays an ordinary origin at its real MO2 priority, because that is what the game loads. It is flagged `derived_by = pgpatcher`, and every derived copy is linked to its **source copy**. The link is precise: `ParallaxGen_Diff.json` stores `crc32original` for every patched base mesh. 12,312 of the 12,324 entries whose source is a loose file matched the CRC32 of the next-lower loose copy byte for byte [Measured].
3. **The brief's premise is wrong for this owner.** The output mod is **not** at the top. It is line 54 of 1,989 in modlist.txt, rank 37 among 1,542 enabled mods (overwrite = 0). Two mods above it override 16 of its paths [Measured]. So "derived" must be a flag on an origin, not "the top layer".
4. **Detection must use marker files, not PGPatcher's own rule.** PGPatcher recognises its output by comparing the mod folder with *its configured output dir* (`std::filesystem::equivalent`) [Source]. sdc does not know that config. Use these markers at an origin root: `ParallaxGen_Diff.json`, `PGPatcher.esp`, `PG_<n>.esp`, and `meshes\_pgpatcher_dups\`. PGPatcher's `cfg\user.json` → `output.dir` can confirm, but only by file identity. On this machine the stored path is `G:\Acherus\…` and the instance is at `C:\_mount\moddingvol\Acherus`, which is the **same volume** (same file ID) [Measured].
5. **Live PGPatcher process → block the scan.** PGPatcher deletes `meshes\` wholesale and writes NIFs with a plain `ofstream` (no temp + rename), so a scan during a run would see empty or partly written files [Source]. **Stale output → warn per mesh, using CRC32 rather than mtime.** Of 12 stale meshes, only 3 had a newer source mtime. 4 came from a mod inserted between source and output after the run [Measured]. Output in overwrite, or a disabled output mod → informational handling, no block.

---

## 1. Where PGPatcher writes, and what

### 1.1 Output location
- **There is no default output dir.** An empty value is a validation error, `launcher.validation.outputLocationRequired` → "Output Location is required" [Source master PGPatcher/src/PGConfig.cpp:341-342; translations/en.json:166]. The tooltip reads "This folder should be used EXCLUSIVELY for PGPatcher, recommended to be a mod folder" [Source master translations/en.json, `launcher.output.location.tooltip`].
- A relative output dir is resolved against the PGPatcher exe folder [Source master PGConfig.cpp:68-86].
- The wiki recommends an empty mod folder such as `PGPatcher_Output` with zip unchecked, and says "the mod must be disabled in your mod manager prior to running PGPatcher" [Doc Basic-Usage.md].
- Rejected targets:
  - the Data folder itself, and any subfolder of the real `gameDataPath` [Source master PGPatcher/src/main.cpp:428-438; Source 1.2.0 main.cpp:319, 326];
  - an **enabled** mod folder that `equivalent()`s the output dir. The critical message is "If outputting to MO2 you must disable the mod {} first to prevent issues with MO2 VFS" [Source master PGLib/src/PGModManager.cpp:404-416; Source 1.2.0 PGModManager.cpp:408-409].
- **Does output go to overwrite without an output dir?** No. PGPatcher writes to an explicit real path, never through the VFS. Output reaches `overwrite` only if the user **types the overwrite folder as the output dir**. Nothing in PGPatcher blocks that. `overwrite` is not a modlist.txt line, so the `equivalent` check never sees it, and it is neither Data nor under Data [Inference from the two guards above]. A previous output already in overwrite would trip the `Data\ParallaxGen_Diff.json` check below on the next run.
- Owner's config: `output.dir = "G:\\Acherus\\mods\\ParallaxGen_Output"`, `zip=false`; `modmanager.type = 2` (MO2), `mo2instancedir = "G:\\Acherus"`, `mo2useloosefileorder = false`, so PGPatcher's own mod order is *not* locked to MO2's loose order [Measured `tools\ParallaxGen\cfg\user.json`, `settings.json`].

### 1.2 Files written (exact names)
| Item | Name / location | Source |
|---|---|---|
| Patched base meshes | `meshes\<original relative path>` (full copy of the NIF) | [Doc Output.md]; [Source master PGMeshPermutationTracker.cpp:240-265] |
| Permutation duplicates | `meshes\_pgpatcher_dups\<n>\<rest of path>`: `_pgpatcher_dups\<index>` is inserted after the first path component | [Source master PGMeshPermutationTracker.hpp:326-327, .cpp:625; Source 1.2.0 .cpp:707] |
| Generated textures | `textures\...`; always `textures\cubemaps\dynamic1pxcubemap_black.dds` when CM is on; CM conversions (`_m`) and the SSS-fix delighted diffuse | [Doc Output.md]; [Source master PatcherMeshShaderComplexMaterial.hpp:33; main.cpp:156-170] |
| Light Placer JSONs | `lightplacer\...` | [Doc Output.md] |
| Plugins | `PGPatcher.esp` (new TXST only; ESM, ESL when possible); `PG_<n>.esp` (record overrides, split at 255 masters) | [Doc Output.md]; [Source master PGMutagen.cs:183, 1309] |
| **Diff manifest** | **`ParallaxGen_Diff.json`**: `{ "meshes\\…\\x.nif": {"crc32original": u32, "crc32patched": u32} }`, base meshes only, written only if non-empty | [Source master main.cpp:924-932; PGPatcher.cpp:641-650; Source 1.2.0 main.cpp:713-716] |
| Update cache (master only) | `PGPatcher_UpdateCache.bin` (+ `.tmp` while saving): private binary per-mesh dependency record | [Source master PGRunCache.hpp:50; PGPatcher.cpp:307-310]. **Absent in 1.2.0** (`git grep` finds no `PGRunCache`/`UpdateCache`) |
| Zip (optional) | `PGPatcher_Output.zip` | [Source master main.cpp:960] |
| Other folders it owns | `calientetools`, `pbrnifpatcher`, `pbrtexturesets` (deleted on cleanup) | [Source master PGPatcher.cpp:304-305] |
| Logs | `<exe dir>\log\PGPatcher.log` (not in the output) | [Measured] |
| NIF markers | `PG_STOCK` NiStringExtraData (stock material diff, master only, mainly BodySlide ShapeData); `PG_IGNORE` NiBooleanExtraData (opt-out) | [Source master PatcherMeshPreStockMarker.hpp:17-39; PGNIFUtil.cpp:587]. **`PG_STOCK` is absent in 1.2.0** |

There is **no** `PGPatcher_output.json`. The only cross-version manifest is `ParallaxGen_Diff.json`. PGPatcher's own whitelist for its output root (`deleteOutputDir`, which refuses any other file with "Output directory has non-PGPatcher related files") is: folders `meshes textures calientetools pbrnifpatcher lightplacer pbrtexturesets`; files `pgpatcher.esp parallaxgen_diff.json pg_*.esp meta.ini pgpatcher_output.zip` (+ cache files in master) [Source master PGPatcher.cpp:302-363; Source 1.2.0 PGPatcher.cpp:281-288]. **sdc can use the same whitelist as a strong signature.**

### 1.3 Write order during a run (matters for "active PGPatcher")
1. `deleteOutputDir()` removes `meshes`, `textures`, `lightplacer`, …, `PGPatcher.esp`, `PG_*.esp` **and `ParallaxGen_Diff.json`** before patching starts [Source 1.2.0 main.cpp:619; master main.cpp:794-801 (master's "Update Output" mode keeps meshes/textures and prunes per mesh)].
2. Meshes are written by a background file saver, `std::ofstream file(meshFilename)` → write → close, straight to the final path [Source 1.2.0 PGMeshPermutationTracker.cpp:258; master .cpp:259-264].
3. Wait for the saver, then save plugins, deploy the cubemap, and **write `ParallaxGen_Diff.json` near the end** [Source master main.cpp:855-932]. In master, the update cache is written last via temp + rename [Source master PGRunCache.cpp:1259; util/BinaryIO.cpp:58].

[Inference] So "`meshes\` present but `ParallaxGen_Diff.json` absent", or "any NIF/ESP mtime newer than the diff JSON by more than a few minutes", means a run is in progress, crashed, or was cancelled.

---

## 2. How PGPatcher uses MO2 (and what sdc should copy)

- **Requires launch through MO2** when MO2 mode is set. It checks that `usvfs_x64.dll` is loaded in its own process, else "Please verify that you are launching PGPatcher from MO2, VFS not detected." Override: `--ignore-mo2vfscheck` [Source master main.cpp:530-535; PGModManager.cpp:672-708; PGHandlers.hpp:120]. It reads all asset bytes through the VFS Data view (`pgd->file()`), including BSAs.
- **Reads modlist.txt itself** for "which mod owns each file", to drive its conflict manager [Source master PGModManager.cpp:337-470]:
  - it reads `modorganizer.ini` → profiles/mods dirs and `selected_profile` → `modlist.txt`;
  - it skips `-` disabled, `*` foreign, `#` comments and `*_separator` lines;
  - the first listed mod wins per file (`if (m_modFileMap.contains(...)) return true`);
  - it maps only `meshes textures pbrnifpatcher lightplacer calientetools` [Source master PGGlobals.hpp:28];
  - it skips hidden entries and `meta.ini`.
  - **It does not handle overwrite.** Overwrite files are visible through the VFS but unowned.
- **Its own mod order** is `cfg\modrules.json` (`mod_order` in `user.json`), editable in the Conflict Manager. It is optionally locked to MO2's loose order. The wiki says it "should be a subset of your loose file order" [Doc Conflict-Manager.md].
- **Self-exclusion (how it avoids reading its own output):**
  1. The output mod must be **disabled**: an enabled mod folder `equivalent` to the output dir is a critical stop [Source above].
  2. `Data\ParallaxGen_Diff.json` visible through the VFS means an output is active somewhere (overwrite, an enabled mod under a different name, or the real Data). The message is "PGPatcher meshes exist in your data directory, please delete before re-running." [Source master main.cpp:598-605; 1.2.0 main.cpp:448].
  3. `vramroutput.tmp` visible → "Please disable VRAMr output mod" [Source master main.cpp:608-611].
- **What sdc should copy:**
  - (2), the marker-file check, generalised to per-origin, because sdc resolves statically and knows each origin;
  - the **file-identity comparison** of (1) whenever it compares folders (`GetFileInformationByHandle` volume serial + file index, not strings);
  - the root whitelist of §1.2 as a signature.
  - sdc should **not** copy the "must be disabled" rule. For sdc an *enabled* output is the normal, correct state, because that is what the player sees.

---

## 3. Consequence for sdc's per-mod view and texture sets

### 3.1 Measured on the owner's instance (profile `Nexus`)
| Quantity | Value |
|---|---|
| `ParallaxGen_Output` position | modlist.txt line 54 of 1,989, `+` enabled; rank 37 of 1,542 enabled (0 = overwrite). Disabled in the 3 other profiles. `PGPatcher.esp` = plugins.txt line 130, `PG_1.esp` line 1968 |
| Files | 32,373 total: **32,301 NIF** (incl. **2,270** under `_pgpatcher_dups`), 56 DDS, `lightplacer\`, `PGPatcher.esp` 112 KB, `PG_1.esp` 1.86 MB, `ParallaxGen_Diff.json` 4.25 MB, `meta.ini`. No update cache, so it was built by 1.2.0 |
| Run time | log 2026-08-21 20:03:46 → diff JSON / meshes 20:05. `PG_1.esp` mtime 20:23, written after the run by something else (unexplained) |
| Loose NIF paths (all enabled mods + overwrite) | 52,262 unique |
| **Winning paths from the output mod** | **32,285 (61.8 %)**; non-FaceGen 25,517 of 45,433. Next biggest winner: `DynDOLOD Resources SE` with 4,289 |
| Output paths overridden by higher mods | 16 (`[Predator] Simple But Elegant…` 12, `Arch_Solitude` 4) |
| Diff JSON entries | 30,031: keys are lowercase, backslash, `meshes\…`. Every key exists in the output, and every non-dup output NIF has a key |
| Source of the patched base meshes | **12,324 have a loose copy below the output mod; 17,707 come only from a BSA or Data** (vanilla/CC/mod BSAs) |
| `crc32original` vs next-lower loose copy | **12,312 match**, **12 mismatch**, 17,707 not verifiable without a BSA reader |
| Output integrity | 601 sampled outputs: CRC32 of the file == `crc32patched` in 601/601 |
| Process now | `tools\ParallaxGen\PGPatcher.exe` (+ `pgtools.exe`) installed in the instance; not running |

The 12 mismatches [Measured, `pgp_mismatch2.py`]:
- **4 have a CRC that matches a *deeper* copy** (`Zenithar's Anvil - A Forge Replacer`). A mod (`…Forge Replacer Meshes`) was enabled or moved in between after the run, so the patched mesh now hides a different source than the one it was built from.
- **3 have a source mtime newer than the diff JSON** (`Zenithar's Anvil - Embers XD Patch`): the source was updated in place.
- **5 have an older mtime and match no copy** (`Remodeled Armor SE - CBBE 3BA` ×3, `Embers XD Patch` ×1, `DD2Hair` ×1). These are probably reinstalls that kept archive mtimes, or a source that came from overwrite/BodySlide at run time. **So mtime alone misses 9 of 12.**

### 3.2 What changes between the source copy and the patched copy
PGPatcher rewrites the fields sdc stores per shape (`NifShapeRow`, 00 §2):
- **Parallax:** shader type → `BSLSP_PARALLAX` (3) + `SLSF1_PARALLAX`; slot 4 (index 3) = `<prefix>_p.dds` [Source master PatcherMeshShaderVanillaParallax.cpp:189-194].
- **Complex material:** shader type → `BSLSP_ENVMAP` (1) + `SLSF1_ENVIRONMENT_MAPPING` (+ `SLSF1_SPECULAR`); env mask slot = `_m`, and the cubemap slot can become the generated 1 px black cube [Source master PatcherMeshShaderComplexMaterial.cpp:194-275].
- **True PBR:** shader type → `BSLSP_DEFAULT` + `SLSF2_UNUSED01` (the PBR bit). It clears envmap/parallax/multilayer/hair/facegen/glow flags and rewrites slots to `textures\pbr\…` paths, plus `_rmaos`, `_cnr`, `_s` etc. [Source master PatcherMeshShaderTruePBR.cpp:663-672, 1032-1040; conventions in `skyrim_conventions.md` §8].
- **Pre/post patchers:** texture-slot-count fix, mesh lighting fix, SSS fix (new delighted diffuse in slot 2), hair flow map (backlight slot) [Source master PGLib/src/patchers/].
- **Plugin side:** `PG_<n>.esp` overrides alternate textures / model records and points records at `_pgpatcher_dups` copies. What the game shows for a placed object can therefore also depend on plugin records that a NIF scan does not read [Doc Output.md; Source master PGPatcher.cpp:600-630]. Out of scope for Feature 1; note it in the UI.

**So slot paths and shader type differ by design.** The texture set the player sees is the patched one. The texture set the *mod author* shipped is the source one. sdc needs both.

### 3.3 Mapping patched → source precisely
- **Base meshes:** derived copy at vpath `P` → source = the highest-priority copy of `P` *below* the derived origin (loose, else BSA by game rank, else Data). Verify with `crc32(source bytes) == ParallaxGen_Diff.json[P].crc32original` (IEEE CRC-32, as `boost::crc_32_type` and Python `zlib.crc32`). If it does not match, scan the deeper copies; a deeper match means "built from X, now hidden by Y". If nothing matches, the copy is stale or has an unknown source.
- **Duplicates (`meshes\_pgpatcher_dups\<n>\…`):** not in the diff JSON. The source vpath is the path with the `_pgpatcher_dups\<n>` component removed; the source copy is resolved as above; CRC state is "dup_of" its base entry [Source master PGMeshPermutationTracker.hpp:326-327].
- **Meshes in the output and not in the diff JSON:** none on this setup apart from the dups. If any appear, show them as derived with source "unknown".
- **Not usable:** `PGPatcher_UpdateCache.bin` (private, versioned binary; master only) and `PG_STOCK` (master only, written mainly for BodySlide ShapeData). If `PG_STOCK` is present, sdc *may* show "stock" slot values from it later. That is optional.

---

## 4. Failure modes of an "active PGPatcher"

| Case | What happens without a failsafe | Detect | sdc action |
|---|---|---|---|
| **(a) PGPatcher running during an sdc scan** | `meshes\` is deleted at run start, then refilled by unsynchronised `ofstream` writes. sdc sees missing files, torn (half-written) NIFs, and (size, mtime) churn that poisons the incremental cache. **Mitigating:** PGPatcher only runs if the output mod is *disabled* in modlist.txt, so a static resolver already resolves without it. But on the next scan it reappears, and a scan of its folder during the run is garbage. Also MO2 locks its UI while a hooked tool runs, so launching sdc from MO2 at the same time needs the user to unlock it [Claim, not checked in MO2 source; see open question 5]. | 1. A process named `PGPatcher.exe` or `pgtools.exe` (`CreateToolhelp32Snapshot`; PGTools also calls `deleteOutputDir`, PGTools/src/main.cpp:360). If `QueryFullProcessImageNameW` works, note the exe folder. 2. Signature folder: `meshes\` present and `ParallaxGen_Diff.json` absent; or `PGPatcher_UpdateCache.bin.tmp` present (master). 3. Mtime of the newest file under the derived origin changes between the start and end of sdc's walk. | **Block** the scan (or just the derived origin's walk) with "PGPatcher is running. Wait for it to finish, then rescan." Buttons **Retry** / **Scan without PGPatcher output**. Never write cache rows for a derived origin read during (a). Re-check the process at the end of the scan; if it appeared mid-scan, mark the result `tainted` and rescan. |
| **(b) Output stale relative to sources** | The game shows the patched mesh, built from old source bytes or from a source that is now hidden. sdc's "winner" is correct (that is what loads), but the user may edit the *source* and wonder why nothing changes in game. | Per base mesh: `crc32original` vs the current source copy (§3.3). Cheap pre-filter: skip the CRC when the source copy's (size, mtime, origin) is unchanged since the last verified scan. Global: modlist.txt/plugins.txt hash or mtime newer than the diff JSON → "load order changed since last PGPatcher run". | **Warn**, never block. A banner: "PGPatcher output is out of date for N meshes (M sources changed, K now hidden by a newly added mod). Re-run PGPatcher ('Update Output')." A per-NIF badge **Stale PGPatcher copy**. On the owner's data N = 12. |
| **(c) Output sits in overwrite** | Overwrite is always on top and mixed with other tool output (owner's overwrite has 1,402 files, 250 NIFs). sdc would call it "Overwrite wins" for everything. The next PGPatcher run refuses ("PGPatcher meshes exist in your data directory"). | Same signature check on the overwrite root (`ParallaxGen_Diff.json` / `PGPatcher.esp` / `PG_*.esp` / `meshes\_pgpatcher_dups`). Only paths listed in the diff JSON or under `_pgpatcher_dups` count as derived; other overwrite files stay ordinary. | Treat those paths as a derived layer. **Warn once:** "PGPatcher output is in Overwrite. Move it to its own mod (right-click Overwrite → Create mod) — PGPatcher will refuse to run again until you do." |
| **(d) Output mod disabled** (other 3 profiles here) | Nothing loads it; it does not affect winners. | Disabled mod folder with the signature. | Ignore it for resolution. Show an info chip "PGPatcher output present but disabled in this profile" only if the profile has `PGPatcher.esp`/`PG_*.esp` active in plugins.txt (a mismatch → meshes and plugin disagree; **warn**). Note that disabling it is the correct state *while PGPatcher runs*. |
| (e) Multiple derived origins | Two folders with the signature enabled (old + new output, or a renamed copy). | Count of enabled signature origins > 1. | Warn: "Two PGPatcher outputs are enabled: A (higher) hides B." |
| (f) Higher mods override the output | 16 paths here: the user sees the higher mod's unpatched mesh. | Derived copy that is not the winner. | Badge on that NIF: "PGPatcher patch is hidden by X." The wiki says to use "Ignore Meshes" instead of placing meshes after PGPatcher [Doc Conflict-Manager.md]. Info only. |

**Circularity (sdc feeding PGPatcher later).** sdc might later write PBR textures + PGPatcher JSON (`PBRNifPatcher\*.json`) into its own mod for PGPatcher to consume. Then:
- sdc must decide "which texture set does this mesh have" from the **source copy**, not the derived copy. Otherwise sdc's next JSON would be written against already-PBR'd slots (`textures\pbr\…`), and PGPatcher would match nothing or double-patch.
- So the resolver needs a mode **`resolve(exclude_derived = true)`** that drops derived origins (same as PGPatcher running with its output disabled).
- sdc's own output mod must never carry the PGPatcher signature, and PGPatcher must see it as an ordinary enabled source mod placed *below* the PGPatcher output.
- When exporting for PGPatcher, sdc should refuse while a derived origin is stale **and** the user has picked "derived" view, and point to the source view instead [Inference].

---

## 5. Recommendation

**Is a failsafe needed? Yes.**
1. Correctness: PGPatcher output wins about 62 % of loose NIF paths on a real setup, so a naïve per-mod view is misleading for most of the list.
2. Safety: concurrent runs produce torn files and would corrupt sdc's incremental cache.
3. Circularity: any future sdc→PGPatcher feed must read source copies.

The design is small: a marker check, one JSON parse, CRC on demand, and a process check.

### 5.1 Detection rules (on every scan, before walking)
1. **Process gate:** enumerate processes. If `PGPatcher.exe` or `pgtools.exe` is running → state `pgp_running`. Repeat the check after the walk.
2. **Signature per origin root** (each enabled mod, overwrite, and Data):
   - `strong` = `ParallaxGen_Diff.json` present **and** (`PGPatcher.esp` or any `PG_<digits>.esp`);
   - `medium` = any one of those, or `meshes\_pgpatcher_dups\` exists;
   - `incomplete` = `meshes\` present, no `ParallaxGen_Diff.json`, and (an ESP present, or `PGPatcher_UpdateCache.bin*` present).
   - Optional confirmation: find `PGPatcher.exe` under the instance (`tools\**`, `mods\**`, depth ≤ 3), read `cfg\user.json` `params.output.dir`, and compare by **file ID**, not string (owner: `G:\Acherus\…` == `C:\_mount\moddingvol\Acherus\…`).
3. For a non-overwrite origin with `strong`/`medium`, the whole origin is `derived_by = pgpatcher`. For overwrite (and Data), only paths that are diff-JSON keys or under `_pgpatcher_dups` are derived.
4. Parse `ParallaxGen_Diff.json` once (4 MB here). Store `(path_id → crc32original, crc32patched)`. Re-parse when its (size, mtime) changes.
5. Staleness per derived base mesh, after resolution: pick the source copy (§3.3) → compute CRC32 if not cached for the copy's (size, mtime). States: `match | stale_changed | stale_reordered (matches a deeper copy) | unverified (source in BSA until the BSA reader decompresses) | unknown_source | dup_of`.

### 5.2 Data model (extends 02 §6)
```sql
ALTER TABLE origin   ADD COLUMN derived_by   INTEGER;      -- NULL | 1 = pgpatcher (room for vramr/bodyslide/texgen later)
ALTER TABLE origin   ADD COLUMN derived_state INTEGER;     -- ok | incomplete | running_at_scan | in_overwrite
ALTER TABLE nif_copy ADD COLUMN is_derived   INTEGER NOT NULL DEFAULT 0; -- per copy (needed for overwrite)
ALTER TABLE nif_copy ADD COLUMN source_copy_id INTEGER REFERENCES nif_copy(id); -- resolved source, NULL if unknown
ALTER TABLE nif_copy ADD COLUMN derive_check INTEGER;      -- match|stale_changed|stale_reordered|unverified|unknown|dup_of
CREATE TABLE pgp_diff (path_id INTEGER PRIMARY KEY, crc_orig INTEGER NOT NULL, crc_patched INTEGER NOT NULL);
ALTER TABLE nif_copy ADD COLUMN crc32 INTEGER;             -- computed lazily, only for source candidates of derived copies
-- second resolved view: the winner when derived origins are excluded
CREATE TABLE nif_winner_src (path_id INTEGER PRIMARY KEY, copy_id INTEGER NOT NULL);
```
`nif_winner` stays "what the game loads". `nif_winner_src` is the same resolution with derived origins removed; it is cheap, the same pass with one filter. `source_copy_id` for base meshes = `nif_winner_src` of the same path (for dups: of the stripped path).

### 5.3 UI
- **Global banner** (one line, highest-severity first):
  - running: "PGPatcher is running — scan paused." [Retry] [Scan without PGPatcher output]
  - incomplete: "PGPatcher output 'X' looks incomplete (no ParallaxGen_Diff.json). Re-run PGPatcher." Its paths are treated as derived and unverified.
  - in overwrite: see §4 (c).
  - stale: "PGPatcher output is out of date for N meshes." [Show list]
  - multiple outputs: see §4 (e).
- **Per-mod view:**
  - The derived mod gets a "Generated by PGPatcher" header chip and its list is grouped by source mod.
  - For a source mod, NIFs that are hidden only by a derived copy get the badge **"Patched by PGPatcher (in game)"** instead of "Overridden by ParallaxGen_Output", plus a stale marker when applicable.
  - "Overridden by" lists only non-derived overriders.
- **NIF detail:** two columns, **In game (PGPatcher copy)** and **Source (Mod X)**. Shader type, flags and slots are diffed and the changed fields highlighted. A toggle at the top: **View: In game / Source**. The default is In game, because the player sees it.
- **Texture reverse lookup:** count uses from both views separately, so "who uses `_p.dds`" lists PGPatcher-added slots as "added by PGPatcher".
- **Converter/export actions** (future sdc→PGPatcher feed) always use the Source view.

### 5.4 Test cases
1. Fixture instance: mod A (`meshes\x.nif`), output mod O above A with a patched `x.nif` + diff JSON with A's CRC → winner O, source A, check `match`; per-mod view of A shows "Patched by PGPatcher".
2. Same, but A's `x.nif` changed (bytes differ, newer mtime) → `stale_changed`, banner N=1.
3. Insert mod B between O and A carrying a different `x.nif` → `stale_reordered` (CRC matches A, a deeper copy). Mirrors the owner's Zenithar case.
4. Changed bytes with an *older* mtime → still `stale_changed`. This guards against mtime-only logic.
5. `meshes\_pgpatcher_dups\1\x.nif` only in O → derived, `dup_of`, source path `meshes\x.nif`.
6. Diff JSON key with no lower loose copy and the path present in a BSA → `unverified` (until the BSA bytes are read), then `match` once decompressed.
7. Output folder contents in `overwrite` mixed with unrelated files → only diff-JSON/dup paths are derived; others remain overwrite winners; overwrite warning shown.
8. O disabled in modlist.txt, `PGPatcher.esp` active in plugins.txt → resolution ignores O; mismatch warning.
9. Two enabled signature folders → multiple-outputs warning; higher one wins.
10. O has `meshes\` but no `ParallaxGen_Diff.json` → `incomplete` banner; all its NIFs derived + unknown_source.
11. Fake running process: a stub exe named `PGPatcher.exe` alive at scan start → scan blocked; started mid-scan → result `tainted`, cache rows for O not committed.
12. A mod higher than O overrides a derived path → badge "PGPatcher patch is hidden by X".
13. Path identity: config `output.dir` uses a different drive letter or mount point for the same folder → still matched (file ID compare).
14. `resolve(exclude_derived=true)` equals the resolution of the same fixture with O removed from modlist.txt.
15. Case and slashes: diff JSON keys `meshes\\…` lowercase join to sdc's `meshes/…` canonical form (00 §2 rule 5).

---

## 6. Open questions
1. BSA-sourced derived meshes (17,707 here) can only be CRC-verified once the BSA reader decompresses entries. Verify lazily on NIF selection, or in a background pass?
2. Should sdc also flag other generators the same way: BodySlide output (`CalienteTools` in overwrite), TexGen/DynDOLOD output, VRAMr (`vramroutput.tmp`)? `derived_by` is designed to take them. Owner preference?
3. `PG_1.esp` has mtime 20:23, 18 min after the run. Something edited it (xEdit/LOOT/MO2?). Does it matter for sdc? Probably not for NIF scanning.
4. Should plugin-level alternate textures from `PG_<n>.esp` be read later (needs a plugin reader), so the "in game" view is complete for placed objects that use `_pgpatcher_dups` meshes?
5. MO2 locks its UI while a hooked tool (PGPatcher) runs. Confirm on the owner's MO2 2.5.2 whether sdc can be started at all while PGPatcher runs. If not, case (a) mainly happens when sdc is started outside MO2 or the user unlocks.
6. Owner: default NIF detail view "In game" (recommended) or "Source"?

---

## Sources
**PGPatcher source** (https://github.com/hakasapl/PGPatcher, cloned to scratchpad `pgp\src`):
- master `fd2fd4c`:
  - PGPatcher/src/main.cpp:156-170, 380-470, 520-560, 598-611, 794-801, 855-975, 1152, 1162-1163
  - PGPatcher/src/PGConfig.cpp:68-86, 183-190, 341-342, 420-423; PGPatcher/include/PGHandlers.hpp:120
  - PGPatcher/translations/en.json (`launcher.output.*`, line 166)
  - PGLib/src/PGModManager.cpp:337-470, 672-708; PGLib/include/PGModManager.hpp:113-114, 280-293
  - PGLib/src/PGPatcher.cpp:302-418, 450-456, 600-650
  - PGLib/include/PGRunCache.hpp:28-140; PGLib/src/PGRunCache.cpp:1249-1279, 2080-2086; PGLib/src/util/BinaryIO.cpp:58
  - PGLib/include/pgutil/PGMeshPermutationTracker.hpp:27, 198, 326-327; PGLib/src/pgutil/PGMeshPermutationTracker.cpp:40-70, 240-275, 625
  - PGLib/include/patchers/PatcherMeshPreStockMarker.hpp:17-39
  - PGLib/include/patchers/PatcherMeshShaderComplexMaterial.hpp:33
  - PGLib/src/patchers/PatcherMeshShaderVanillaParallax.cpp:81-194; PatcherMeshShaderComplexMaterial.cpp:84-275; PatcherMeshShaderTruePBR.cpp:311-335, 663-672, 1032-1040
  - PGLib/include/PGGlobals.hpp:28; PGLib/src/pgutil/PGNIFUtil.cpp:587; PGMutagen/PGMutagen.cs:183, 1309
  - PGTools/src/main.cpp:360; PGPatcher/CMakeLists.txt:20; PGTools/CMakeLists.txt:20
- tag 1.2.0 `3c43295`: PGLib/src/PGModManager.cpp:408-409; PGLib/src/PGPatcher.cpp:281-288, 323; PGLib/src/pgutil/PGMeshPermutationTracker.cpp:246-258, 707; PGPatcher/src/main.cpp:319, 326, 401, 448, 619, 713-716, 734; PGTools/src/main.cpp:148. `PGRunCache`, `UpdateCache` and `PG_STOCK` do not occur in 1.2.0.

**Wiki** (https://github.com/hakasapl/PGPatcher/wiki, cloned): Output.md, Basic-Usage.md, Conflict-Manager.md.

**Local notes:** `F:\ai\dds\research_notes\Skyrim DDS converter project plan\skyrim_conventions.md` §8 (PGPatcher slot/suffix rules, lines 13-14, 171-245, 320-358, 523-534).

**Owner instance (read-only, 2026-10-11):**
- `C:\_mount\moddingvol\Acherus\ModOrganizer.ini` (lines 3, 5); `profiles\{Nexus, Nexus - Expansions, Nexus - Immersion, Skyrim Modding Essentials}\modlist.txt`; `profiles\Nexus\plugins.txt` (lines 130, 1968)
- `mods\ParallaxGen_Output\` (listing, `meta.ini`, `ParallaxGen_Diff.json`); `overwrite\`
- `tools\ParallaxGen\{PGPatcher.exe, pgtools.exe, cfg\user.json, cfg\settings.json, log\PGPatcher.log}`
- Same-volume check: `stat` inode 18014398509615881 for `G:\Acherus\mods\ParallaxGen_Output\ParallaxGen_Diff.json` and `C:\_mount\moddingvol\Acherus\…\ParallaxGen_Diff.json`.
- Scripts: scratchpad `scripts\pgp_scan.py`, `scripts\pgp_mismatch2.py`; output `pgp_scan_out.txt`.

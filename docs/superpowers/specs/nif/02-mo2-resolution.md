# 02 — MO2 overwrite resolution for the NIF texture-set scan

Feature 1 (part): scan NIFs for texture sets across a Mod Organizer 2 setup, load individual mod folders, and know for every overwritten NIF which copy the player actually sees. Requirement: sdc is launched through MO2 at most once, only to discover the setup.

Tags: **[Source]** = read in code (file:line). **[Doc]** = official docs or API headers. **[Claim]** = community or forum statement I could not verify in code. **[Inference]** = my own reasoning from the sources.
Local MO2 source: `F:\ai\dds\nifparseref\modorganizer-master\src` (shortened to `mo2/`). The 2.5.x branch has the same logic in every function cited here; the diffs are cosmetic or about `modDataDirectory` only (`diff` of profile.cpp, fileentry.cpp, directoryentry.cpp, directoryrefresher.cpp).
External repos were shallow-cloned into the session scratchpad `mo2refs\` (commit/date): uibase `edc09d3`/2026-10-06, plugin_python `baf0920`/2026-10-06, usvfs `57f1ea5`/2026-04-15, game_skyrimSE `af9b50b`/2025-05-29, game_gamebryo `8b1f5fe`/2025-05-29, bsa-rs `7b90ce1`/2025-01-01.
Real data comes from a read-only inspection of the user's portable instance `G:\Acherus` (MO2 2.5.2, profile `Nexus`, 1,543 enabled mods) and of the Steam install `G:\SteamLibrary\...\Skyrim Special Edition\Data`.

---

## TL;DR

1. **Primary path: read MO2's files directly, every time (option a).** The single MO2 launch is only used to find the instance directory, through the injected `usvfs_x64.dll`, the parent process `ModOrganizer.exe`, and MO2's directory appended to `PATH`. After that, sdc re-reads `ModOrganizer.ini` and `profiles\<p>\{modlist.txt, plugins.txt, settings.ini, skyrim*.ini}` on each scan. MO2's resolution rules are small and deterministic. In `modlist.txt`, line 1 is the highest priority, `overwrite` is always on top, the game `Data` folder is at the bottom, loose files beat every BSA, and BSA-vs-BSA is decided by plugin load order.
2. **Optional "exact" mode: an MO2 Python tool plugin (option b)** that dumps `virtualFileTree`/`getFileOrigins` for `meshes/**.nif` (plus `textures/**.dds`) to JSON. Use it to cross-check sdc's own resolver and to get MO2's own view of BSA contents. It needs MO2 running and a user click, so it is not the default.
3. **usvfs cannot tell you origins (option c, rejected as a resolver).** usvfs rewrites `NtQueryObject(ObjectNameInformation)` and `NtQueryInformationFile(FileName/FileNormalizedName/FileAll)` back to the *virtual* path, so `GetFinalPathNameByHandle` returns `...\Data\meshes\...`. File IDs are not rewritten, so the one MO2 launch can still serve as a self-test of the static resolver.
4. **Do your own BSA reading, not MO2's.** MO2 only parses BSAs inside mod folders, only with `archive_parsing_experimental=true` (off by default), and never the game `Data` folder's BSAs. It also ranks BSAs with no matching plugin *above* plugin BSAs, which is the opposite of the game. To read v105, hand-roll a small reader with `lz4_flex` (frame feature, MIT, pure Rust). The alternative `ba2` 3.0.1 (0BSD) works but always pulls in the C/C++ `directxtex` and `lzzzz` builds.
5. **Storage: SQLite (`rusqlite`, bundled), with interned paths, a deduplicated texture-set table and a reverse texture index.** Measured on the user's setup there are about 52k unique loose NIF paths, 21k NIFs inside mod BSAs and 25k vanilla/CC NIFs. FaceGen is 13–22% of NIFs and an estimated 25–35% of shape rows. The estimated database size is about 45–60 MB with FaceGen and about 30–40 MB without.

---

## 1. Detecting "launched through MO2" and finding the instance

### 1.1 What MO2 does when it spawns a tool
- Hooked processes are created with `usvfsCreateProcessHooked(...)`, and plain ones with `CreateProcess`. In both cases the creator is the MO2 process itself, with flags `CREATE_BREAKAWAY_FROM_JOB` [Source mo2/spawn.cpp:456-466]. **The parent of sdc is `ModOrganizer.exe`.** For 64-bit targets there is no proxy process; `usvfs_proxy_x86/x64.exe` only appear in cross-bitness injection [Inference from spawn.cpp and the files in G:\Acherus].
- **Environment:** MO2 sets no `MO_*` variables. It temporarily *appends its own exe directory to `PATH`* before `CreateProcess` and restores it afterwards, so the child inherits `PATH=...;<MO2 dir>` [Source mo2/spawn.cpp:447-448 and 471; env.cpp:390-395]. It also sets `SteamAPPId` when it does a Steam check [Source mo2/spawn.cpp:617-622]. Neither is an MO2-specific marker. The `PATH` tail is still a useful secondary signal and gives the MO2 directory directly.
- **usvfs:** a hooked child has `usvfs_x64.dll` (`AppConfig::vfs64DLLName`) loaded [Source mo2/shared/appconfig.inc]. MO2 uses a constant VFS instance name `"mod_organizer_instance"` [Source mo2/usvfsconnector.cpp:35,138]. That name carries no instance information.

### 1.2 Detection algorithm (sdc side)
1. `GetModuleHandleW(L"usvfs_x64.dll") != NULL` means sdc is running under the MO2 VFS. This is the strongest signal.
2. Parent process: get the parent PID with `CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS)` (`th32ParentProcessID`). Then call `OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION)` and `QueryFullProcessImageNameW`, and check that the basename is `ModOrganizer.exe`. Also check `GetProcessTimes(parent).creation < own.creation` to guard against PID reuse [Inference].
3. Fallback: the last `PATH` entry whose directory contains `ModOrganizer.exe`.
4. From the MO2 exe directory `D`, find the instance the same way MO2 does [Source mo2/instancemanager.cpp:551-573, 638-656]:
   - if `D\portable.txt` exists, the instance is portable and **forced** to `D` (`allowedToChangeInstance()==false`);
   - else, if the global setting `CurrentInstance` is non-empty, the instance is `%LOCALAPPDATA%\ModOrganizer\<CurrentInstance>`. `CurrentInstance` is stored in `QSettings("Mod Organizer Team","Mod Organizer")`, which is the registry key `HKCU\Software\Mod Organizer Team\Mod Organizer` [Source mo2/settings.cpp:2492-2508];
   - else, if `D\ModOrganizer.ini` exists, the instance is portable at `D`.
   - Global root: `QStandardPaths::AppLocalDataLocation` [Source mo2/instancemanager.cpp:592-596]. MO2 sets no organisation or application name, so Qt falls back to the exe basename (`OUTPUT_NAME "ModOrganizer"`, mo2/CMakeLists.txt:23), which gives `%LOCALAPPDATA%\ModOrganizer\<name>` [Inference; on this machine that folder exists and holds only `cache`/`nxmhandler.ini`]. Global instances are subfolders containing `ModOrganizer.ini` [Source mo2/instancemanager.cpp:603-620].
   - Caveat: `-i <instance>` and `moshortcut://<instance>:<exe>` override the instance for that MO2 session without changing the registry [Source mo2/commandline.cpp:407-424; main.cpp:89-93]. To be exact, read the parent's command line with `NtQueryInformationProcess(ProcessCommandLineInformation)` and parse `-i`/`moshortcut://`. On the user's machine the registry `CurrentInstance` is empty and `G:\Acherus\portable.txt` exists, so this case does not arise there.
5. Store `{instance_dir, mo2_exe_dir, detected_profile}` in sdc's own config (`%LOCALAPPDATA%\sdc\`). **Never write under the VFS-mapped `Data` while hooked**, because writes there are redirected to `overwrite` or a custom overwrite [Inference; profile `[custom_overwrites]` exists in G:\Acherus\profiles\Nexus\settings.ini].
6. Offer "Restart sdc outside MO2". Every file I/O sdc makes is hooked while it runs under usvfs [Source usvfs/src/usvfs_dll/hookmanager.cpp:210-312], and sdc does heavy DDS I/O.

Without any MO2 launch, sdc can still list global instances (`%LOCALAPPDATA%\ModOrganizer\*\ModOrganizer.ini`) and let the user browse to a portable `ModOrganizer.exe` or instance folder. The one launch only makes discovery effortless.

### 1.3 `ModOrganizer.ini` keys (Qt INI format)
| Key | Section | Storage | Default | Source |
|---|---|---|---|---|
| `gameName` | General | QString | — | settings.cpp:675-683 |
| `gamePath` | General | **QByteArray**: `@ByteArray(G:\\Acherus\\Stock Game)` | — | settings.cpp:661-673 |
| `game_edition` | General | QString (`Steam`/`GOG`/`Epic Games`) | — | settings.cpp:685-693 |
| `selected_profile` | General | **QByteArray** `@ByteArray(Nexus)`. Rewritten whenever the profile changes | — | settings.cpp:695-707; organizercore.cpp:610 |
| `base_directory` | Settings | QString | dir of ModOrganizer.ini | settings.cpp:1701-1707 |
| `mod_directory` | Settings | QString, may contain `%BASE_DIR%` | `%BASE_DIR%/mods` | settings.cpp:1721-1725 |
| `overwrite_directory` | Settings | 〃 | `%BASE_DIR%/overwrite` | settings.cpp:1733-1737 |
| `profiles_directory` | Settings | 〃 | `%BASE_DIR%/profiles` | settings.cpp:1727-1731 |
| `download_directory` | Settings | 〃 | `%BASE_DIR%/downloads` | settings.cpp:1709-1713 |
| `archive_parsing_experimental` | Settings | bool | false | settings.cpp:359-362 |

- `%BASE_DIR%` is a plain string replace with `base()` [Source settings.cpp:1626, 1681-1686]. Defaults are written as `%BASE_DIR%/<name>` [Source settings.cpp:1688-1691].
- The parser must handle these Qt-INI details: `@ByteArray(...)` wrapping; `\\` escapes inside values (seen in `gamePath`); `%20` and similar key escaping (seen `Skyrim%20Special%20Edition%20Support%20Plugin\enderal_downloads`); quoted values; `/` vs `\` separators. **Data folder = `<gamePath>\Data`** (here `G:\Acherus\Stock Game\Data`, a "Stock Game" copy, *not* the Steam install). Never use the Steam library path when an MO2 instance is known [Source, observed G:\Acherus\ModOrganizer.ini lines 2-5].

---

## 2. Static resolution (re-read on every scan)

### 2.1 `profiles\<p>\modlist.txt`
- **Written reversed:** `doWriteModlist` iterates `m_ModIndexByPriority` from highest to lowest. The comment in the source says "the priority order was inverted on load so it has to be inverted again" [Source mo2/profile.cpp:258-273]. On read, the k-th listed mod gets `numKnownMods - k - 1` [Source profile.cpp:523-525]. **So line 1 (after the header) is the highest priority, and the last line is the lowest.**
- Line grammar (UTF-8) [Source profile.cpp:446-463]:
  - empty lines and `#` lines are skipped;
  - `-name` means disabled;
  - `+name` means enabled;
  - `*name` means foreign or unmanaged (`DLC: Dawnguard`, `Creation Club: ...`) and is always enabled (`ModInfoForeign::alwaysEnabled`, modinfoforeign.h:81);
  - a bare `name` counts as enabled;
  - names are trimmed; duplicate names are ignored after the first occurrence (profile.cpp:473-477);
  - a line named `overwrite` triggers a warning and is ignored (profile.cpp:469-471, 572-578).
- Mod folders not listed in modlist.txt get priority above all listed mods but are **disabled**: `ModStatus` defaults to `m_Enabled=false` [Source profile.h:379; profile.cpp:536-538]. Foreign mods not listed go below everything (`--topInsert`, profile.cpp:533-535).
- **Separators** are folders whose name matches `.*_separator` [Source modinfo.cpp:65-70]. They are listed like mods (usually `-Foo_separator`) and contain only `meta.ini`, so skip them. The user's instance has 127 separators in 1,988 lines.
- **Backups** are folders matching `.*backup[0-9]*`, which is a case-sensitive regex [Source modinfo.cpp:73-77]. They are never written to modlist.txt (`hasAutomaticPriority`, modinfo.h:660; profile.cpp:263) and are `alwaysDisabled` (modinfobackup.h:22). **sdc must apply the same regex**, otherwise a folder named `...backup2` would be counted.
- **Overwrite:** always enabled (modinfooverwrite.h:38) and always at the top priority `N-1` [Source profile.cpp:518-521].
- **Game Data folder** is origin `"data"` at priority 0. Mods are added at `priority + 1` [Source directoryrefresher.cpp:471-480, 419]. Foreign (DLC/CC) mods only "steal" their plugin and BSA files from `data` and add no loose files [Source directoryrefresher.cpp:240-281, 416-421].
- Resulting order, lowest to highest: `data` < modlist.txt bottom … top (enabled `+` only) < `overwrite`.
- The profile's `settings.ini` `[General]` holds `LocalSaves`, `LocalSettings` (profile-local INIs), `AutomaticArchiveInvalidation` and `[custom_overwrites]` (per-executable redirect of overwrite into a named mod) [Source profile.cpp:160-191, 883-887; observed in G:\Acherus\profiles\Nexus\settings.ini]. Custom overwrites only change where *new* files are written. The target is an ordinary mod, so resolution is unaffected [Inference].
- **Multiple profiles:** list `profiles_directory\*` subfolders that contain `modlist.txt`. Store the chosen profile name in sdc's config. Default to "follow `selected_profile`", because MO2 rewrites it whenever the profile changes [Source organizercore.cpp:610].

### 2.2 How MO2 computes conflicts
- `DirectoryRefresher::refresh` builds a `DirectoryEntry` tree. It adds `data` at priority 0, then all enabled mods sorted ascending by priority (multi-threaded), then calls `FileRegister::sortOrigins()` [Source directoryrefresher.cpp:462-509, 380-460].
- **`FileEntry::sortOrigins` gives the final answer** [Source mo2/shared/fileentry.cpp:141-185]. `addOrigin` (fileentry.cpp:18-83) is only provisional. The comparator works like this:
  - loose vs loose: by mod priority (a negative priority counts as `INT_MAX`);
  - archive vs archive: by **plugin load-order index** of the BSA (`order`), regardless of mod priority (`-1` counts as `INT_MAX`);
  - loose vs archive: **the loose copy always ranks higher**.
  - The winner is the back of the sorted list. The remaining alternatives stay in *ascending* order.
- **Archive parsing:**
  - It only happens when `archive_parsing_experimental=true` (default false) [Source settings.cpp:359-362; directoryrefresher.cpp:319, 362, 398].
  - It only covers archives named in the profile's `archives.txt` [Source organizercore.cpp:1571-1590].
  - It only covers BSAs *inside mod folders*. The `data` origin is added with `addFromOrigin` only, and foreign mods take the steal path, so vanilla and Data-folder BSAs are never parsed [Source directoryrefresher.cpp:480, 416-421; only call sites of `addFromAllBSAs` are directoryrefresher.cpp:236, 363].
  - The BSA `order` is the load-order index of a plugin whose stem `P` satisfies `bsa.lower().starts_with(P+" - ")` or `starts_with(P+".")` [Source directoryentry.cpp:164-197].
  - **Quirk:** a BSA with no matching plugin gets `order=-1`, treated as `INT_MAX`, so it *wins* over all plugin BSAs in MO2's view. In game, INI-registered archives load *first* (lowest) [Claim, see §3]. sdc's resolver should follow the game, not this quirk [Inference].
  - Second caveat: `addFromBSA` returns early if `containsArchive(name)` is already true [Source directoryentry.cpp:200-210, 370-380]. That check only looks at root-level entries. Two enabled mods shipping a same-named BSA are parsed by parallel threads (directoryrefresher.cpp:422-450), so MO2's attribution of archived files can depend on which thread ran first [Inference]. This is one more reason option b is a cross-check, not ground truth, for archived files.
  - The user's instance has archive parsing **on** (`ModOrganizer.ini` line 57) and 352 lines in `archives.txt`.
- Plugin API: `IModList::priority(name)` returns the priority (higher wins, -1 if unknown), and `allModsByProfilePriority()` returns mods in priority order [Doc uibase imodlist.h:83-91, 158-163]. The plugin dump should include these so the cross-check covers the mod order too.
- `getFileOrigins(path)` returns `[winner, then alternatives]` [Source organizercore.cpp:998-1013]. The header says it is "sorted by their priority" [Doc uibase imoinfo.h:295-301], but only index 0 is reliable as "winner" because the tail is ascending. `findFileInfos` is **not recursive**; it only walks `dir->getFiles()` [Source organizercore.cpp:1015-1040]. Its `FileInfo.filePath` is the real path of the winner, and `archive` is the BSA name when the winner is archived [Doc imoinfo.h:73-80].

### 2.3 Plugins, load order, INIs
- `plugins.txt` (profile) is written by `CreationGamePlugins` in priority (load) order, using the **system ANSI code page** [Source game_gamebryo/src/creation/creationgameplugins.cpp:39-104]:
  - `*name` = active, a bare `name` = inactive;
  - primary plugins are **not** written: Skyrim, Update, Dawnguard, HearthFires and Dragonborn `.esm`, plus every line of `<game>\Skyrim.ccc` [Source game_skyrimSE/src/gameskyrimse.cpp:247-255, 286-307].
  - So the effective load order is primary plugins (in that order) followed by the plugins.txt lines [Source creationgameplugins.cpp:106-170]. `loadorder.txt` is also written.
- Do not key on the header line. In the observed plugins.txt the first line was the game's/LOOT's "# This file is used by Skyrim…" rather than MO2's header. Every `#` line is a comment whoever wrote it.
- `archives.txt` (profile) lists enabled archives (used only for MO2's own archive parsing).
- INI files: `skyrim.ini`, `skyrimprefs.ini`, `skyrimcustom.ini` [Source gameskyrimse.cpp:281-284].
  - If `LocalSettings=true` they live in the profile folder; otherwise in `Documents\My Games\<Skyrim Special Edition | ... GOG | ... EPIC>` [Source gameskyrimse.cpp:137-145; skyrimsedataarchives.cpp:21-24].
  - Resolve Documents with `SHGetKnownFolderPath(FOLDERID_Documents)` to handle OneDrive redirection [Inference].
  - MO2 reads `SResourceArchiveList` and `SResourceArchiveList2` **from skyrim.ini only** [Source skyrimsedataarchives.cpp:17-27]. The game also applies `SkyrimCustom.ini` overrides [Claim], so sdc should read both and let Custom win per key.
  - Real profile example: `sResourceArchiveList=Skyrim - Misc.bsa, ... Meshes0.bsa, Meshes1.bsa, Sounds.bsa` and `...List2=Voices_en0, Textures0..8, Skyrim - Patch.bsa` (profile skyrim.ini).

---

## 3. BSA ordering in SSE and Rust readers

- **Loose files beat BSAs:**
  - [Claim, STEP/forums] "loose files always override same files inside of registered AND plugin-loaded BSAs".
  - MO2 models the same rule [Source fileentry.cpp:173-177].
  - MO2's SkyrimSE plugin registers **no** `BSAInvalidation` feature, unlike older Gamebryo games [Source gameskyrimse.cpp:119-127 vs game_gamebryo/gamebryobsainvalidation.cpp]. That is consistent with SSE not needing timestamp invalidation [Inference]. The user's skyrim.ini still has `bInvalidateOlderFiles=1`, which is harmless.
  - I could not get primary engine documentation: UESP returns 403 to the fetch tool.
- **BSA vs BSA:** INI-registered archives load first, in list order (List, then List2). Plugin-loaded archives come next, in plugin load order; a later plugin wins [Claim, STEP forum thread]. SSE loads `<plugin>.bsa` and `<plugin> - Textures.bsa` for every *active* plugin, including `.esl` [Claim]. MO2 uses a broader `"<plugin> - *"` prefix [Source directoryentry.cpp:183-185].
  - sdc resolver rank, highest to lowest: loose (by mod priority) > plugin BSAs by descending load index (Textures and non-Textures BSAs of the same plugin share a rank, with Textures second [Inference]) > INI-registered BSAs in reverse list order.
  - **Which folder supplies a BSA follows loose-file resolution:** the BSA file is itself a loose file in the virtual Data folder, so the highest-priority mod providing `Foo.bsa` supplies it [Inference]. MO2 also adds each mod's BSAs under that mod's origin.
- **Vanilla counts** (my v105 header parser over the Steam `Data` folder; 89 BSAs, all v105): 25,433 NIFs, of which 3,736 are FaceGen (14.7%). The base game alone (Meshes0/1 and Animations) has 22,091 NIFs and 3,159 FaceGen (14.3%). There are 35,665 DDS, of which 3,723 are FaceGen tints.
- **Rust crates:**
  | Crate | Version / licence | v105 + LZ4 | Notes |
  |---|---|---|---|
  | `ba2` (Ryan-rsm-McKenzie/bsa-rs) | 3.0.1, **0BSD** (GPL-compatible), crate updated 2024-12-22 | Yes: `Version::v105`, x64 directory entries, LZ4 frame via `lzzzz::lz4f` [Source bsa-rs/src/tes4/archive.rs:248, 561-567, 881; file.rs:12] | Unconditional deps `directxtex` (C++ DirectXTex build) and `lzzzz` (C lz4), plus `flate2` with zlib [Source bsa-rs/Cargo.toml]. Mature, has tests, read+write, memmap. |
  | `bsa` (chisui/elderscrolls-rust) | 0.2.1, MIT OR Apache-2.0, 2021-10-09 | README lists v105 support; uses `lz4` (C) | Self-described WIP; stale [Doc docs.rs/crate/bsa/0.2.1, GitHub README]. |
  | Hand-rolled + `lz4_flex` | lz4_flex 0.14.0, MIT, pure Rust; `frame::FrameDecoder` behind the `frame` feature | — | The reader is about 250 lines: header (36 B), folder records (24 B in v105), bzstring folder names + 16 B file records, name block, the 0x40000000 compression-toggle bit, embedded-name flag. My 40-line Python prototype read all 89 vanilla v105 BSAs and 341 of 342 mod BSAs. **Scanning needs only the name tables (no decompression). Decompression is needed only to parse NIF bytes.** |
  - Recommendation: hand-roll the reader with `lz4_flex` (no C toolchain added, read-only, name-table-first). Use `ba2` only if write support or BA2 is ever needed.

---

## 4. Option matrix

| | a. One MO2 launch → static resolution | b. MO2 Python tool plugin dump | c. Run under usvfs, walk virtual Data | d. No MO2: user picks folder(s) |
|---|---|---|---|---|
| Exactness | Equals MO2 for loose files (same rules re-implemented). BSA layer follows *game* order, better than MO2's `-1` quirk. Risk: edge rules (backup regex, foreign mods) | Exact MO2 view. BSAs only if archive parsing is on and the BSA is listed in archives.txt; vanilla BSAs never | Content is exact, origins unknown: names are rewritten to virtual [Source usvfs ntdll.cpp:858-916, 966-1000] | Exact for one folder. A Data folder gives the winner only (or Vortex manifest, see below) |
| Needs MO2 running | Once (or never, with manual path pick) | Every refresh (user runs the tool) | Every session | Never |
| Effort | Medium: Qt-INI parser, modlist/plugins parsers, walker, BSA reader | Low-medium: ~150 lines of Python plus a JSON importer; must recurse via `listDirectories`/`virtualFileTree` | Low to walk, but useless for attribution; heavy I/O hooking cost | Low |
| Robust to MO2 updates | Good. Formats have been stable since 2.x (2.5.x and master identical here) | Good. Plugin API is versioned; Python proxy must be enabled | Fragile (usvfs internals) | n/a |
| Vortex / no-MO2 | No | No | No | Yes. Vortex writes `vortex.deployment.json` (deployed file → source mod) in the deployment target [Claim, Nexus forum], which could make (d) origin-aware. Schema unverified |

Other uses of (c): during the one hooked launch, open a sample of conflicted virtual paths and compare the volume serial + `FileIdInformation` with the file sdc's resolver predicts. Those information classes are not rewritten [Source usvfs ntdll.cpp:966-969 only touches name classes; `NtQueryVolumeInformationFile` not in the hook list, hookmanager.cpp:285-300]. This turns the one launch into a resolver self-test [Inference; needs a spike].

**Edge cases:**
- **Root Builder:** files under a mod's `Root\` deploy to the game root, not Data, so they are irrelevant to meshes. If a mod has `Root\Data\meshes`, treat it as out of scope or warn [Inference].
- Relocated `mod_directory`/`base_directory` on other drives are handled through `%BASE_DIR%`.
- Per-mod junctions/symlinks: the walker must follow reparse points (MO2's own walker does).
- `modDataDirectory` (master only) is empty for SSE [Source directoryrefresher.cpp:184-187].
- Stock-Game folders: use `gamePath`.
- Case-insensitive path keys throughout; normalise to lowercase with `/`.

---

## 5. Recommendation

1. **Default: option a.**
   - On first run, detect MO2 (§1.2) or let the user pick an instance or `ModOrganizer.exe`, then persist the instance and profile.
   - Each scan:
     1. Re-read `ModOrganizer.ini`, `modlist.txt`, `plugins.txt`, profile `settings.ini` and the relevant `skyrim*.ini`. Change detection uses (size, mtime) plus a content hash of these small files.
     2. Walk `meshes/` and `textures/` of every enabled mod, `overwrite` and `<gamePath>\Data`, with stat-only enumeration. Python `os.walk` took 2.2 s with a warm cache over 15k dirs and 113k files on the user's setup.
     3. Read the name tables of every BSA that is active per the plugins and the INI.
     4. Resolve winners in memory.
2. **Optional "Verify with MO2" (option b):** ship a small `sdc_export.py` tool plugin. It writes `%LOCALAPPDATA%\sdc\mo2_origins.json` with `{vpath, origins[], realPath, archive}` for meshes and textures, and sdc diffs it against its own resolution. It is also the path for users who want MO2's exact archive semantics.
3. **Option c:** only as a detector, plus an optional FileId self-test during the single launch.
4. **Option d:** always available ("Open mod folder…" / "Open Data folder…"). Read Vortex's `vortex.deployment.json` if present, after verifying its format.

---

## 6. Storage schema

**Choice:** rusqlite 0.40.2 (MIT) with the `bundled` feature, WAL, a single file at `%LOCALAPPDATA%\sdc\scan.db`.
- Pros: relational queries are exactly what the per-mod view needs (winner/overridden-by, reverse texture lookup), incremental upserts, crash safety.
- A flat file (`postcard`/`rkyv` plus a string pool) would be about 2–3× smaller and faster to load. However, incremental updates mean rewriting the whole file, and every view needs hand-built indexes. Consider it only as an in-memory snapshot format.
- `redb` 4.3.0 (pure Rust KV) is a middle ground without SQL.

```sql
-- interning
CREATE TABLE path   (id INTEGER PRIMARY KEY, p TEXT NOT NULL UNIQUE);            -- lowercase, '/' sep, relative to Data
CREATE TABLE origin (id INTEGER PRIMARY KEY, kind INTEGER NOT NULL,             -- 0=data 1=mod 2=overwrite 3=bsa
                     name TEXT NOT NULL, root TEXT NOT NULL,                    -- mod folder / bsa file path
                     parent_origin INTEGER REFERENCES origin(id),               -- bsa -> providing mod
                     priority INTEGER NOT NULL,                                  -- loose: mod prio; bsa: game rank
                     enabled INTEGER NOT NULL, sig_size INTEGER, sig_mtime INTEGER);
CREATE TABLE profile_state (key TEXT PRIMARY KEY, val BLOB);                    -- instance, profile, file hashes
-- every physical copy of a NIF (one row per origin that has it)
CREATE TABLE nif_copy (id INTEGER PRIMARY KEY, path_id INTEGER NOT NULL, origin_id INTEGER NOT NULL,
                       size INTEGER, mtime INTEGER, xxh3 INTEGER, parse_status INTEGER,
                       UNIQUE(path_id, origin_id));
-- resolved view (rebuilt cheaply after modlist/plugins change; no reparse)
CREATE TABLE nif_winner (path_id INTEGER PRIMARY KEY, copy_id INTEGER NOT NULL, n_copies INTEGER NOT NULL);
-- parse results per physical copy (xxh3-keyed dedup deferred, see below)
CREATE TABLE shape (copy_id INTEGER NOT NULL, block INTEGER NOT NULL, name TEXT,
                    shader_kind INTEGER, shader_type INTEGER, slsf1 INTEGER, slsf2 INTEGER,
                    texset_id INTEGER, PRIMARY KEY(copy_id, block)) WITHOUT ROWID;
CREATE TABLE texset (id INTEGER PRIMARY KEY, t0 INTEGER, t1 INTEGER, t2 INTEGER, t3 INTEGER, t4 INTEGER,
                     t5 INTEGER, t6 INTEGER, t7 INTEGER, t8 INTEGER,            -- path ids (textures/...), NULL=empty
                     UNIQUE(t0,t1,t2,t3,t4,t5,t6,t7,t8));
CREATE TABLE tex_use (tex_path_id INTEGER, texset_id INTEGER, slot INTEGER,
                      PRIMARY KEY(tex_path_id, texset_id, slot)) WITHOUT ROWID; -- reverse index
-- tex_copy: unused since audit 2026-10-16 (no longer written, nothing reads it; drop at the next schema bump).
CREATE TABLE tex_copy (path_id INTEGER, origin_id INTEGER, size INTEGER, mtime INTEGER,
                       PRIMARY KEY(path_id, origin_id)) WITHOUT ROWID;
CREATE TABLE tex_winner (path_id INTEGER PRIMARY KEY, origin_id INTEGER, n_copies INTEGER);
CREATE INDEX nif_copy_origin ON nif_copy(origin_id);
```
- Texture paths are normalised the way the engine does: lowercase, `\`→`/`, strip a leading `data/`, prefix `textures/` if missing [Claim/Inference; verify against nifly/CS sources in plan 01/03]. FaceGen is excluded by prefix: `meshes/actors/character/facegendata/` for NIFs and `textures/actors/character/facegendata/` for DDS.
- Shapes could instead be keyed by `xxh3` to deduplicate identical copies across mods. Keying by `copy_id` is simpler; I recommend xxh3 deduplication only if size becomes a problem.

**Measured inputs (user's instance, read-only scan):**
- Loose: 69,192 NIF files across 1,543 enabled mods and overwrite, giving 52,261 unique virtual paths. 13,562 paths conflict, with up to 11 copies of one path. Of the unique paths, 6,829 (13.1%) are FaceGen. Average virtual path length is 57 chars. There are 43,548 loose DDS.
- Mod BSAs: 342 BSAs in 247 mods hold 21,149 NIFs, of which 4,586 (21.7%) are FaceGen.
- Vanilla/CC BSAs: 25,433 NIFs, 3,736 FaceGen (14.7%).
- **About 115k NIF copies and about 90–100k unique virtual NIF paths; FaceGen is about 15–17% of NIFs.**

**Size estimate [Inference]:**
- `path`, NIF part: 100k × (57 B text + about 70 B unique index and row) ≈ 13 MB.
- `path`, texture part: about 60k × 130 B ≈ 8 MB.
- `nif_copy`: 115k × about 45 B ≈ 5 MB, plus about 3 MB of indexes.
- `shape`: with about 4 shapes per NIF (FaceGen heads carry about 8–12), 115k copies give about 500k rows × about 35 B ≈ 18 MB.
- `texset`: about 40k × 90 B ≈ 4 MB. `tex_use`: about 150k × 20 B ≈ 3 MB.
- **Total ≈ 50–60 MB with FaceGen.**
- **Skipping FaceGen** removes about 16% of paths and copies but an estimated 25–35% of shapes and many single-use texture sets (unique tint per NPC). That gives **≈ 35–40 MB**.
- Keying shapes by xxh3 instead of copy_id saves about 20% more where duplicated copies are common.

**Incremental rescan:**
1. Config files: hash ModOrganizer.ini, modlist.txt, plugins.txt, the profile settings.ini and skyrim*.ini. If only these changed, recompute `origin.priority`/`enabled` and rebuild `nif_winner`/`tex_winner` in a single SQL pass, with **no file walk**.
2. File walk: enumerate with `FindFirstFileExW(FindExInfoBasic, FIND_FIRST_EX_LARGE_FETCH)` or `std::fs::read_dir`, which on Windows returns size and mtime from the directory listing without an extra open [Claim]. A (size, mtime) match means the copy is unchanged. A mismatch means re-hash and re-parse. Rows not seen are deleted.
3. BSAs: (size, mtime) per archive. On change, re-read the name table and the per-file (size, offset) to detect changed entries.
4. A cheap "dirty" check: NTFS directory mtime changes when entries are added or removed (not on content edits), so it can skip *enumerating* unchanged dirs but not the stat of files [Inference].

---

## 7. "Load an individual mod folder" UX

1. The mod picker lists enabled mods from the active profile (by priority, with separators as group headers), plus "Overwrite", "Game Data", "Other folder…".
2. Selecting a mod shows its NIFs: `SELECT ... FROM nif_copy WHERE origin_id=?`, joined with `nif_winner`. Each NIF gets one badge:
   - **Wins** (`winner.copy_id = this copy`);
   - **Overridden by X** (winner origin name, plus "+N others" from `n_copies`);
   - **Overrides** (list lower copies);
   - for a BSA-provided copy: "Inside Foo.bsa, loses to loose file in Y".
   Filters: only winners / only overridden / only conflicted.
3. Selecting a NIF shows its shapes (shader kind/type, SLSF1/2 flags) and texture set slots. Each texture path is resolved through `tex_winner` to its winning origin (mod or BSA), with a warning when the texture is missing everywhere. Showing the winning texture's origin tells the user which mod's DDS the converter would actually need to touch.
4. Reverse lookup: selecting a texture lists every texture set, NIF and mod that uses it (through `tex_use`).
5. "Show winning copy" switches to the NIF's winner when the selected mod's copy is overridden, because that is what the player sees.
6. Folders outside MO2 (option d) use the same view with a single origin. Win/loss badges are hidden unless that folder is part of a known instance.

---

## 8. Open questions

1. Parent command line (`-i`/`moshortcut://`) parsing: is it worth implementing, or is "portable.txt → registry → portable ini" plus a confirmation dialog enough?
2. Should sdc honour MO2's `order=-1 → top` BSA quirk when in "match MO2" mode, or always use game order? (I recommend game order and flagging the difference.)
3. SSE plugin↔BSA naming: confirm that the engine accepts only `<plugin>.bsa` and `<plugin> - Textures.bsa`, versus MO2's broader prefix match. A test is needed with a BSA named `Foo - Meshes.bsa`.
4. SkyrimCustom.ini override of `sResourceArchiveList*`: confirm that the engine merges per key.
5. Vortex `vortex.deployment.json` schema and location (Data folder vs staging): verify from Vortex source before building the (d)+Vortex path.
6. FileId self-test under usvfs: spike to confirm `GetFileInformationByHandleEx(FileIdInfo)` returns the real ID inside a hooked process.
7. One mod BSA failed my quick parser. Identify it (BA2 renamed, or v104?) and decide how to handle it.
8. Shape count per NIF was estimated, not measured. Measure it once the NIF parser (plan 01) exists, to firm up the size estimate.

---

## Sources

**Local MO2 (modorganizer-master/src, same logic in 2_5_x):**
- profile.cpp:243-281, 392-583, 603-615, 880-945
- modinfo.cpp:65-108, 225-282
- modinfo.h:660-665; modinfobackup.h:22; modinfoforeign.h:81; modinfooverwrite.h:38
- profile.h:379
- directoryrefresher.cpp:174-192, 240-460, 462-509
- shared/directoryentry.cpp:164-241; shared/fileentry.cpp:18-83, 141-185
- organizercore.cpp:610, 998-1040, 1571-1606
- settings.cpp:359-362, 655-707, 1626-1770, 2492-2508
- instancemanager.cpp:551-656; commandline.cpp:395-424
- spawn.cpp:440-475 (PATH at 447-448/471), 611-622; env.cpp:390-407; usvfsconnector.cpp:35, 138
- shared/appconfig.inc; CMakeLists.txt:23

**GitHub (cloned):**
- https://github.com/ModOrganizer2/modorganizer-uibase: include/uibase/imoinfo.h:73-80, 255-318; imodlist.h:83-163
- https://github.com/ModOrganizer2/modorganizer-plugin_python: src/mobase/wrappers/basic_classes.cpp:607-716
- https://github.com/ModOrganizer2/usvfs: src/usvfs_dll/hookmanager.cpp:285-312; hooks/ntdll.cpp:832-1010
- https://github.com/ModOrganizer2/modorganizer-game_skyrimSE: src/gameskyrimse.cpp:113-307; skyrimsedataarchives.cpp; skyrimseunmanagedmods.cpp
- https://github.com/ModOrganizer2/modorganizer-game_gamebryo: src/creation/creationgameplugins.cpp:39-170; src/gamebryo/gamebryodataarchives.cpp; gamebryobsainvalidation.cpp
- https://github.com/Ryan-rsm-McKenzie/bsa-rs: Cargo.toml; src/tes4/archive.rs; src/tes4/file.rs

**Crates:**
- https://crates.io/crates/ba2 (3.0.1, 0BSD)
- https://crates.io/crates/bsa (0.2.1)
- https://github.com/chisui/elderscrolls-rust
- https://docs.rs/lz4_flex/latest/lz4_flex/frame/ (lz4_flex 0.14.0, MIT)
- rusqlite 0.40.2 MIT; redb 4.3.0 MIT/Apache-2.0; postcard 1.1.3; rkyv 0.8.18 (crates.io API, 2026-10-10)

**Community:**
- https://stepmodifications.org/forum/topic/5033-ramifications-of-bsa-extraction-in-mod-organizer/ (loose > plugin BSA > registered BSA)
- https://forums.nexusmods.com/topic/10087578-a-question-about-vortexdeploymentjson/ (Vortex manifest)
- UESP "Skyrim Mod:Archive File Format" returned 403 to the fetch tool and was not used.

**Local data:**
- G:\Acherus\ModOrganizer.ini; profiles\Nexus\{modlist.txt, plugins.txt, settings.ini, skyrim.ini, archives.txt}
- G:\SteamLibrary\steamapps\common\Skyrim Special Edition\Data\*.bsa
- Counting scripts: scratchpad\scripts\{bsacount.py, modscan.py, modbsa.py}

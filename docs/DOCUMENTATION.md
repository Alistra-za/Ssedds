# Documentation policy

These rules apply to every agent or person who writes documentation for Tessalith. They are owner rules (2026-10-31).

They cover these files:
- **User-facing docs:** README.md, docs/wiki/, docs/gui.md, and the plugin pages (docs/*-plugin.md).
- **The ruling log:** docs/DECISIONS.md.
- **History:** CHANGELOG.md (versions) and docs/presets-changelog.md (presets).

docs/research/ is a record of measurements. It may describe old results, but each page states the date and the build it measured.

## 1. Current state only
Documentation describes the build on `main` as it is now. History lives only in the changelog.
- Delete anything that is no longer true. Don't mark it "old" or "deprecated", and don't strike it through.
- **Banned in user-facing docs:**
  - "now", "no longer", "used to", "was renamed", "previously", "since 1.x";
  - "(new)" badges;
  - "in earlier versions…".
  
  A feature is described as what it is, not as what changed.
- **Where an upgrade note goes:** a user who upgrades needs to know what changed, for example "the Resize option was renamed". That goes in the version's CHANGELOG.md entry. A doc page may link to that entry ("Upgrading from 1.1: see CHANGELOG 1.2.0") but doesn't repeat it.
- **When a feature is removed:** delete its section, its screenshots, and every reference to it in the same commit.

## 2. One home per fact
Every fact has exactly one home: the one place where it is explained in full, with its reasons, its limits and an example.
- **A fact appears at most twice:** its home plus one short reference.
- **Three times is the cap,** only when a third mention is essential. For example, a warning that prevents data loss may also appear next to the button that causes it.
- **The same fact reworded still counts.** "BC7 is used for colour with alpha" and "alpha textures encode as BC7" are one statement.
- **A short reference is one line,** and it links to the home: "Tessalith never overwrites a file it didn't write ([why](wiki/Validation-and-Output.md#re-runs-and-if-changed))". Quick-reference tables, the FAQ, Getting-Started, tooltip lists and the sidebar are all short references.
- **Before you add a sentence,** grep docs/ and README.md for the fact. If it already has a home, link to it. If the new place should be the home, move the full text there and turn the old place into a link.
- **One home per topic family:**
  - GUI behaviour: the GUI pages in docs/wiki;
  - CLI behaviour: CLI-Reference;
  - settings keys: Project-Config;
  - texture types: Texture-Types;
  - encoder choice: Research, which links to docs/research.
  
  docs/gui.md and the plugin pages in docs/ either become the home for their topic, with the wiki linking to them, or shrink to a link. They never repeat the wiki.

## 3. Precise
- **Name things exactly as the UI and CLI do.**
  - Button and menu names are **bold** and copied character for character.
  - Settings keys, flags and file names are in `code`.
- **Give values, not adjectives.**
  - Write "at most 512 px", not "small".
  - Write "Slowest and Max use the peak-preserving BC6H mode", not "higher efforts handle highlights better".
- **Say what happens.** "Export skips the file and adds a Problems row" is precise. "Export may have trouble with it" is not.
- **Every claim can be checked against the code or a test.** If you can't find where the code does it, don't write it.
- **State defaults and limits in the home only.** A short reference never restates a number, because numbers drift. It links instead.

## 4. Page size and structure
- **Size budget:** a wiki page holds at most about 15 KB of text, around 2,500 words. Over that, split it. A page under about 2 KB merges into its parent.
- **Lookup tables:** reference pages that are mostly tables (CLI-Reference, Project-Config) may run to 30 KB. Past that, split by command or section group.
- **The GUI guide is split into one page per area of the window.** Each page is named after the area as the UI labels it, for example:
  - GUI-Main-Window;
  - GUI-Problems;
  - GUI-Texture-Panel;
  - GUI-Set-Editor;
  - GUI-Review-and-Export;
  - GUI-Meshes-and-NIF-Edit;
  - GUI-Bottom-Tabs, which covers Messages, Files and Presets;
  - GUI-Keyboard, which covers shortcuts and the command palette;
  - GUI-Settings.
  
  GUI-Guide.md becomes a one-screen overview: an annotated main-window picture that links to each area page. Split further along the same lines when a page outgrows the budget.
- **Every page opens with one to three lines** saying what the page covers and who it is for.
- **_Sidebar.md lists every page.** A split updates it in the same commit.
- **Links between pages use the `Page.md#anchor` form,** as publish-wiki.ps1 expects. Check the links after every split with `tools\publish-wiki.ps1 -Out <dir>`.

## 5. Screenshots
Every menu, context menu, panel, tab and dialog the guide documents has a screenshot on its page.
- **Generated, never hand-taken.** A headless kittest test renders each shot from a generated demo project. The demo uses made-up textures, never game or mod assets. The pattern is `crates/gui/tests/doc_shots.rs`.
- **Settings are fixed** so the same build renders the same pixels: one window size, one UI scale, the dark theme and the system fonts the app loads (hashes in shots.toml).
- **Layout:**
  - images go in `docs/img/gui/<page>/<shot>.png`;
  - a manifest, `docs/img/gui/shots.toml`, lists each shot with the test that makes it and the pages that use it.
- **Every image has alt text** that says what it shows. Captions don't repeat the text around them.
- **Before every push, refresh the shots and compare them** (the tool to build is `tools\doc-shots.ps1`):
  1. Render all shots into a temp folder.
  2. Compare each with the committed PNG. A pixel counts as changed when any channel differs by more than 8. A shot counts as changed when more than 0.1 % of its pixels changed. This ignores anti-aliasing noise.
  3. For each changed shot, write a difference overlay next to the new render: the old image dimmed, with the changed pixels in magenta. Also write a summary table: shot, changed %, and the pages that use it.
  4. Look at each overlay.
     - If the change is intended, replace the committed PNG and re-read the pages that use it, because the text may describe the old layout.
     - If it isn't intended, it's a GUI bug, so report it.
  5. Unchanged shots keep their committed bytes. Never re-commit identical-looking PNGs, which only churns the history.
  
  A shot that no test renders any more is deleted along with every reference to it.

## 6. The changelog
- CHANGELOG.md has one section per version, newest first, with one line per user-visible change.
- **The changelog is where history goes.** When a doc page loses an "earlier versions…" sentence, check that the change is in the changelog, and add it if it's missing.
- Unreleased changes collect under `## Unreleased` until a release commit names the version.

## 7. DECISIONS.md: live rulings only
DECISIONS.md is the ruling log: why the code behaves as it does. It keeps only rulings that are in force.
- **Fully reverted or superseded rulings move to `docs/DECISIONS-ARCHIVE.md`.** They move verbatim, under their original ID, headed `ARCHIVED (superseded by Note XX, <date>)`.
- **A partly superseded item is edited in place to the current rule.** The replaced wording goes to the archive under the same ID.
- **IDs are permanent.** Code comments and commits cite them ("Note BC10"), so a letter is never reused. An archived ID stays findable by grep in the archive.
- **When you archive a ruling, update the code comments that cite it** to cite the ruling that replaced it.
- **A live note keeps the rule and its reason.** Narratives of how a ruling was reached, and measurement tables, go to docs/research. The note links to them.
- **What agents read:**
  - Agents read DECISIONS.md.
  - They open DECISIONS-ARCHIVE.md only when a citation points there or someone asks about history.
  - Archived rulings are not in force. Never cite one as a reason for current behaviour.

**Why archive.** DECISIONS.md had grown to about 1.3 MB across 139 notes. That is too big to read whole, so agents search it. A search returns superseded rulings beside the live ones with no way to tell them apart. Agents have planned from stale rulings: the first drafts of the 2026-10-29 fix plans were rejected for exactly that.

## 8. Checklist for a documentation change
1. Find the fact's home. Edit it there.
2. Grep for other mentions. At most one short reference stays, two only if essential. Every reference links to the home.
3. Delete what is no longer true, everywhere, and make sure the change is in CHANGELOG.md.
4. Check page sizes and split any page over budget. Update _Sidebar.md.
5. If the GUI changed, run the screenshot refresh and compare (section 5).
6. If a ruling was reverted, move it to the archive (section 7).
7. Preview the wiki with `tools\publish-wiki.ps1 -Out <dir>`. Publish only when the owner asks.

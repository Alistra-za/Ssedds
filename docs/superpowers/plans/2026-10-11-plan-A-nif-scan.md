# NIF Texture-Set Scan (MO2 + PGPatcher aware) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

> **Not started. `F:\ai\dds\converter` is read-only until the owner says otherwise.** Every path below is where the code *will* go once the owner opens the converter for this work. Before Task 1, the executor asks the owner which milestone name to use and re-reads `converter\docs\superpowers\plans\` for the current milestone state (M3b was in progress on 2026-10-11).

**Goal:** Scan every NIF in a Mod Organizer 2 setup (or one plain folder), record one row per shape (shader type, flags, texture slots, material values), resolve which mod's copy the game loads, treat PGPatcher output as a derived layer linked to its source, and show it per mod in sdc-gui.

**Architecture:** Three new library crates plus one GUI module.
- `nif`: a pure-Rust, seek-only NIF reader over `&[u8]` (layer 0). Layer 1 geometry is added later by plan B.
- `vfs`: loose-file walk plus an SSE BSA v105 reader.
- `modscan`: MO2 instance and profile resolution, PGPatcher detection, the scan orchestrator and the SQLite store.

sdc-gui gets a "Meshes" view driven by a worker thread, following the existing `worker.rs` pattern.

**Tech Stack:** Rust 1.88+ (workspace `rust-version`; sdc-gui needs 1.95), `thiserror`, `rayon`, `walkdir` (already workspace deps). New deps:
- `lz4_flex` (MIT): pure Rust, frame decoder;
- `rusqlite` with `bundled` (MIT);
- `encoding_rs` (MIT/Apache-2.0): Windows-1252 decode;
- `crc32fast` (MIT/Apache-2.0);
- `windows` 0.62.2 (already in the lock): process list and file IDs.

**Spec:** `docs/superpowers/specs/nif/00-decision.md` §2, §3 and §6 (in-repo copies of `F:\ai\dds\nifparseref\plan\`; all spec files below live in the same folder). Paths written `converter/crates/...` below mean `crates/...` in the worktree `F:\ai\dds\converter-nif` (CONCURRENCY.md). Detail in:
- `01-nif-reader.md`: block layouts Q1–Q3, Q8;
- `02-mo2-resolution.md`: §1–§2, §6–§7;
- `05-pgpatcher-interaction.md`: §5;
- `06-third-party-nif-check.md`.

The executor reads 00 plus the cited sections before starting.

## Global Constraints

- Licence: GPL-3.0-or-later workspace. New deps must be MIT, Apache-2.0, 0BSD, BSD or zlib. No OFL/CC assets.
- Windows-only build (`#![cfg(windows)]` is not needed for `nif`/`vfs`; `modscan` uses `windows` only behind `cfg(windows)` modules).
- **Canonical path form** (00 §2 rule 5): ASCII-lowercase, `/` separators, relative to `Data`, starting `textures/` or `meshes/`. This must equal `engine::discover::game_path` output for the same file, ignoring the extension. Raw NIF strings are kept alongside.
- NIF gate: version `0x14020007`, endian 1, user version 12, BS version ∈ {83, 100}. Anything else → `NifError::NotSkyrim`. BSVER 83 rows carry `le_mesh = true` (the scan lists them; the renderer skips them).
- NIF strings are bytes. Decode with Windows-1252 lossily; never fail a file because of a string (06 §2).
- Texture-set slot count is read from the file, never assumed to be 9 (06: 535 sets have 1–6 slots).
- FaceGen skipped by prefix: `meshes/actors/character/facegendata/` and `textures/actors/character/facegendata/`.
- MO2 `modlist.txt` order: first line = highest priority. `overwrite` beats every mod; the game `Data` folder loses to every mod; loose beats BSA; BSA-vs-BSA by plugin load order (game order, tagged [Claim] in 00 §3.2).
- PGPatcher: a running `PGPatcher.exe` or `pgtools.exe` **blocks the scan** (re-checked after the walk). Its output is an ordinary origin at its real priority, flagged `derived_by = pgpatcher`, with each derived copy linked to a source copy (05 §5).
- Read-only on user data: sdc never writes inside the MO2 instance, mods, overwrite or the game folder. The store lives at `%LOCALAPPDATA%\sdc\scan.db`.
- `converter\crates\engine` and existing crates are not modified, except two lines: workspace `members` in the root `Cargo.toml`, and `sdc-gui`'s `Cargo.toml` depending on `modscan`.

## Review Focus

1. **A NIF from a mod that is not a Skyrim NIF** (FO4 BSVER 130, Morrowind/Oblivion header, a 0-byte file, a renamed `.kf`) must give a per-file "not a Skyrim SE/LE NIF" row, never a panic or an aborted scan. Pinned in Task 2 (`rejects_fo4_and_mw_headers`, `truncated_file_is_error_not_panic`).
2. **A path from the MO2 INI that uses `%BASE_DIR%`, `\\` escapes, `@ByteArray(...)` or a relative path** must resolve to the right absolute folder. Pinned in Task 6 (`ini_placeholders_and_bytearray`).
3. **A mod folder that is in `modlist.txt` but missing on disk, or present on disk but not in the list** must not crash; the missing one is reported, the unlisted one is disabled. Pinned in Task 7 (`missing_and_unlisted_mods`).
4. **PGPatcher started during a scan** must leave the cache for its output origin uncommitted and show "PGPatcher is running". Pinned in Task 10 (`pgp_started_mid_scan_taints_result`).
5. **Texture paths with odd forms** (`Data\Textures\...`, `C:\build\textures\...`, `.tga`, empty string, trailing spaces) must normalise to the canonical form or be flagged, never dropped silently. Pinned in Task 3 (`odd_paths_table`).

---

## File Structure

```
converter/Cargo.toml                               (modify: add 3 members + 4 workspace deps)
converter/crates/nif/
  Cargo.toml
  src/lib.rs            pub mod re-exports; NifError
  src/cursor.rs         bounds-checked LE reader
  src/text.rs           Windows-1252 lossy decode
  src/header.rs         Header parse + version gate + offsets + file-length identity
  src/props.rs          BSLightingShaderProperty / BSEffectShaderProperty / BSShaderTextureSet / NiAlphaProperty decoders
  src/shape_head.rs     shape heads up to shader/alpha refs (+ VertexDesc/counts for BSTriShape)
  src/scan.rs           LAYER 0: scan(&[u8]) -> NifScan (one ShapeRow per shape)
  src/path.rs           canonical texture path + flags
  tests/builder/mod.rs  test-only NIF byte builder
  tests/header.rs, tests/props.rs, tests/scan.rs, tests/path.rs, tests/fixtures.rs
  tests/fixtures/       copies of nifly-main/tests/expected/*.nif (GPLv3) + LICENSE note
converter/crates/vfs/
  Cargo.toml
  src/lib.rs            Origin, VfsEntry, read_entry()
  src/loose.rs          walk meshes/ and textures/ of one root (stat-only)
  src/bsa.rs            SSE BSA v105 reader (name table + extract, LZ4 frame)
  tests/bsa.rs          builder-made v105 archives; opt-in vanilla test
converter/crates/modscan/
  Cargo.toml
  src/lib.rs
  src/qtini.rs          Qt INI reader (ModOrganizer.ini, profile settings.ini)
  src/mo2.rs            instance detection, profile, modlist.txt/plugins.txt, origin list
  src/archives.rs       active BSAs per plugin + INI archive lists, game rank
  src/resolve.rs        winners (game view) + winners excluding derived (source view)
  src/pgp.rs            process gate, origin signatures, ParallaxGen_Diff.json, staleness
  src/store.rs          SQLite schema + upserts + queries
  src/scan.rs           orchestrator: walk → parse (rayon) → store → resolve
  tests/ (one file per module, tempdir fixtures)
converter/crates/gui/src/model/meshes.rs           (new) view model for the Meshes view
converter/crates/gui/src/ui/meshes.rs              (new) egui view
converter/crates/gui/src/worker.rs                 (modify: new WorkerMsg variants)
converter/crates/gui/tests/meshes_view.rs          (new) egui_kittest
```

---

### Task 1: `nif` crate skeleton, cursor and text decode

**Files:**
- Modify: `converter/Cargo.toml` (members + `[workspace.dependencies]` `encoding_rs = "0.8"`, `lz4_flex = { version = "0.11", default-features = false, features = ["frame", "std"] }`, `rusqlite = { version = "0.37", features = ["bundled"] }`, `crc32fast = "1"`; the executor runs `cargo search` and pins the current minor of each, then records the versions in the commit message; 02 measured rusqlite 0.40.2 on 2026-10-11, so the pins will be newer than the minors written here)
- Create: `converter/crates/nif/Cargo.toml`, `src/lib.rs`, `src/cursor.rs`, `src/text.rs`
- Test: inline `#[cfg(test)]` in `cursor.rs` and `text.rs`

**Interfaces:**
- Produces:
  - `nif::NifError` (enum, `thiserror`): `Truncated { at: usize, need: usize }`, `NotSkyrim { version: u32, user: u32, bsver: u32 }`, `BadEndian`, `FileLength { expected: usize, actual: usize }`, `BlockSize { block: u32, kind: String, declared: u32, consumed: u32 }`, `BadHeaderLine`.
  - `nif::cursor::Cursor<'a>`: `new(&'a [u8], usize)`, `pos()`, `skip(n)`, `u8/u16/u32/i32/u64/f32() -> Result<_, NifError>`, `f32s::<N>() -> Result<[f32; N]>`, `sized_bytes() -> Result<&'a [u8]>` (u32 length + bytes), `export_bytes() -> Result<&'a [u8]>` (u8 length incl. NUL, NUL stripped).
  - `nif::text::lossy(&[u8]) -> String`.

- [ ] **Step 1: Write the failing tests**

```rust
// crates/nif/src/cursor.rs (bottom)
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reads_le_values_and_strings() {
        let mut b = vec![];
        b.extend(7u32.to_le_bytes());
        b.extend((-1i32).to_le_bytes());
        b.extend(1.5f32.to_le_bytes());
        b.extend(3u32.to_le_bytes()); b.extend(b"abc");
        b.push(4); b.extend(b"xyz\0");
        let mut c = Cursor::new(&b, 0);
        assert_eq!(c.u32().unwrap(), 7);
        assert_eq!(c.i32().unwrap(), -1);
        assert_eq!(c.f32().unwrap(), 1.5);
        assert_eq!(c.sized_bytes().unwrap(), b"abc");
        assert_eq!(c.export_bytes().unwrap(), b"xyz");
        assert_eq!(c.pos(), b.len());
    }
    #[test]
    fn truncation_is_an_error_not_a_panic() {
        let b = [1u8, 2];
        let mut c = Cursor::new(&b, 0);
        assert!(matches!(c.u32(), Err(NifError::Truncated { at: 0, need: 4 })));
        let mut c = Cursor::new(&[255, 255, 255, 255], 0);
        assert!(matches!(c.sized_bytes(), Err(NifError::Truncated { .. })));
    }
}
// crates/nif/src/text.rs (bottom)
#[cfg(test)]
mod tests {
    #[test]
    fn cp1252_lossy() {
        assert_eq!(super::lossy(b"abc"), "abc");
        assert_eq!(super::lossy(&[0x93, b'x', 0x94]), "\u{201C}x\u{201D}");
        assert_eq!(super::lossy(&[0x0c, 0x90, b')']), "\u{000C}\u{0090})");
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p nif`
Expected: compile error (module items missing).

- [ ] **Step 3: Implement**

```rust
// crates/nif/Cargo.toml
[package]
name = "nif"
version.workspace = true
edition.workspace = true
license.workspace = true
publish.workspace = true
rust-version.workspace = true

[dependencies]
thiserror.workspace = true
encoding_rs.workspace = true
```

```rust
// crates/nif/src/lib.rs
//! Skyrim LE/SE NIF reader. Layer 0 (`scan`) reads only the header, shape heads and the small property blocks,
//! seeking by the header's block-size array (plan 01 Q1). Input is always `&[u8]`; loose files, BSA entries and
//! fixtures share one path.
pub mod cursor;
pub mod header;
pub mod path;
pub mod props;
pub mod scan;
pub mod shape_head;
pub mod text;

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum NifError {
    #[error("file ends early: needed {need} bytes at offset {at}")]
    Truncated { at: usize, need: usize },
    #[error("not a Skyrim LE/SE NIF (version {version:#x}, user {user}, BS version {bsver})")]
    NotSkyrim { version: u32, user: u32, bsver: u32 },
    #[error("big-endian NIF")]
    BadEndian,
    #[error("no Gamebryo header line")]
    BadHeaderLine,
    #[error("block sizes do not add up to the file length ({expected} vs {actual})")]
    FileLength { expected: usize, actual: usize },
    #[error("block {block} ({kind}) declared {declared} bytes but decodes as {consumed}")]
    BlockSize { block: u32, kind: String, declared: u32, consumed: u32 },
}
```

```rust
// crates/nif/src/cursor.rs
use crate::NifError;

pub struct Cursor<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    pub fn new(buf: &'a [u8], pos: usize) -> Self {
        Self { buf, pos }
    }
    pub fn pos(&self) -> usize {
        self.pos
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8], NifError> {
        let end = self.pos.checked_add(n).filter(|&e| e <= self.buf.len());
        match end {
            Some(e) => {
                let s = &self.buf[self.pos..e];
                self.pos = e;
                Ok(s)
            }
            None => Err(NifError::Truncated { at: self.pos, need: n }),
        }
    }
    pub fn skip(&mut self, n: usize) -> Result<(), NifError> {
        self.take(n).map(|_| ())
    }
    pub fn u8(&mut self) -> Result<u8, NifError> {
        Ok(self.take(1)?[0])
    }
    pub fn u16(&mut self) -> Result<u16, NifError> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    pub fn u32(&mut self) -> Result<u32, NifError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    pub fn i32(&mut self) -> Result<i32, NifError> {
        Ok(i32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    pub fn u64(&mut self) -> Result<u64, NifError> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    pub fn f32(&mut self) -> Result<f32, NifError> {
        Ok(f32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    pub fn f32s<const N: usize>(&mut self) -> Result<[f32; N], NifError> {
        let mut out = [0.0; N];
        for v in &mut out {
            *v = self.f32()?;
        }
        Ok(out)
    }
    /// u32 length + bytes (nif.xml SizedString).
    pub fn sized_bytes(&mut self) -> Result<&'a [u8], NifError> {
        let n = self.u32()? as usize;
        self.take(n)
    }
    /// u8 length including the NUL + bytes (nif.xml ExportString); the NUL is stripped.
    pub fn export_bytes(&mut self) -> Result<&'a [u8], NifError> {
        let n = self.u8()? as usize;
        let s = self.take(n)?;
        Ok(s.strip_suffix(&[0]).unwrap_or(s))
    }
}
```

```rust
// crates/nif/src/text.rs
//! NIF strings are bytes in the author's code page; Skyrim tools write Windows-1252. Decode lossily so a string
//! can never fail a file (plan 06 §2).
pub fn lossy(b: &[u8]) -> String {
    encoding_rs::WINDOWS_1252.decode_without_bom_handling(b).0.into_owned()
}
```

Create empty `header.rs`, `path.rs`, `props.rs`, `scan.rs`, `shape_head.rs` files (each `//! filled by Task N`) so `lib.rs` compiles.

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p nif`
Expected: 3 tests PASS.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock crates/nif
git commit -m "feat(nif): crate skeleton, bounds-checked cursor, cp1252 text"
```

---

### Task 2: Header, version gate, block offsets, file-length identity (+ test builder)

**Files:**
- Create: `crates/nif/src/header.rs`, `crates/nif/tests/builder/mod.rs`, `crates/nif/tests/header.rs`
- Create: `crates/nif/tests/fixtures/` with these files copied byte-for-byte from `F:\ai\dds\nifparseref\nifly-main\tests\expected\`:
  - `TestNifFile_Skinned_SE.nif`, `TestNifFile_Furniture_Col_SE.nif`, `TestNifFile_Animated_LE.nif`, `TestNifFile_Optimize_LE_to_SE.nif`, `TestNifFile_Skinned_FO4.nif`, `TestNifFile_Skinned_MW.nif`, `TestNifFile_DeepGraph_SE.nif`, `TestNifFile_Skinned_Dynamic_SE.nif`;
  - plus `fixtures/README.md` stating "from nifly (GPL-3.0), https://github.com/ousnius/nifly, tests/expected".

**Interfaces:**
- Consumes: `Cursor`, `NifError` (Task 1).
- Produces:
  - `nif::header::Header { pub bsver: u32, pub block_types: Vec<Vec<u8>>, pub type_index: Vec<u16>, pub sizes: Vec<u32>, pub strings: Vec<Vec<u8>>, pub data_start: usize }`;
  - `Header::parse(&[u8]) -> Result<Header, NifError>`;
  - `Header::offsets(&self) -> Vec<usize>`;
  - `Header::kind(&self, block: usize) -> &[u8]`;
  - `Header::string(&self, idx: i32) -> Option<&[u8]>`;
  - `Header::check_file_length(&self, file: &[u8]) -> Result<(), NifError>`;
  - test builder `builder::Nif` (used by Tasks 3–5 and plan B).

- [ ] **Step 1: Write the test builder (test-only code, needed by the failing tests)**

```rust
// crates/nif/tests/builder/mod.rs
//! Writes minimal Skyrim NIFs (20.2.0.7, user 12) for tests: header + raw blocks + footer.
#![allow(dead_code)]
pub struct Nif {
    pub bsver: u32,
    pub version: u32,
    pub user: u32,
    types: Vec<String>,
    blocks: Vec<(u16, Vec<u8>)>,
    pub strings: Vec<String>,
}

impl Nif {
    pub fn new(bsver: u32) -> Self {
        Self { bsver, version: 0x1402_0007, user: 12, types: vec![], blocks: vec![], strings: vec![] }
    }
    pub fn string(&mut self, s: &str) -> i32 {
        if let Some(i) = self.strings.iter().position(|x| x == s) {
            return i as i32;
        }
        self.strings.push(s.into());
        (self.strings.len() - 1) as i32
    }
    /// Appends a block and returns its index.
    pub fn block(&mut self, kind: &str, body: Vec<u8>) -> i32 {
        let t = match self.types.iter().position(|x| x == kind) {
            Some(i) => i,
            None => {
                self.types.push(kind.into());
                self.types.len() - 1
            }
        };
        self.blocks.push((t as u16, body));
        (self.blocks.len() - 1) as i32
    }
    pub fn bytes(&self) -> Vec<u8> {
        let mut b = b"Gamebryo File Format, Version 20.2.0.7\n".to_vec();
        b.extend(self.version.to_le_bytes());
        b.push(1);
        b.extend(self.user.to_le_bytes());
        b.extend((self.blocks.len() as u32).to_le_bytes());
        b.extend(self.bsver.to_le_bytes());
        for _ in 0..3 {
            b.extend([1u8, 0]); // author, process script, export script: empty ExportStrings
        }
        b.extend((self.types.len() as u16).to_le_bytes());
        for t in &self.types {
            b.extend((t.len() as u32).to_le_bytes());
            b.extend(t.as_bytes());
        }
        for (t, _) in &self.blocks {
            b.extend(t.to_le_bytes());
        }
        for (_, body) in &self.blocks {
            b.extend((body.len() as u32).to_le_bytes());
        }
        b.extend((self.strings.len() as u32).to_le_bytes());
        b.extend((self.strings.iter().map(|s| s.len()).max().unwrap_or(0) as u32).to_le_bytes());
        for s in &self.strings {
            b.extend((s.len() as u32).to_le_bytes());
            b.extend(s.as_bytes());
        }
        b.extend(0u32.to_le_bytes()); // groups
        for (_, body) in &self.blocks {
            b.extend(body);
        }
        b.extend(1u32.to_le_bytes()); // one root
        b.extend(0i32.to_le_bytes());
        b
    }
}

/// Little-endian byte writer for block bodies.
#[derive(Default)]
pub struct W(pub Vec<u8>);
impl W {
    pub fn u8(mut self, v: u8) -> Self { self.0.push(v); self }
    pub fn u16(mut self, v: u16) -> Self { self.0.extend(v.to_le_bytes()); self }
    pub fn u32(mut self, v: u32) -> Self { self.0.extend(v.to_le_bytes()); self }
    pub fn i32(mut self, v: i32) -> Self { self.0.extend(v.to_le_bytes()); self }
    pub fn u64(mut self, v: u64) -> Self { self.0.extend(v.to_le_bytes()); self }
    pub fn f(mut self, v: f32) -> Self { self.0.extend(v.to_le_bytes()); self }
    pub fn fs(mut self, v: &[f32]) -> Self { for x in v { self.0.extend(x.to_le_bytes()); } self }
    pub fn sized(mut self, s: &str) -> Self { self.0.extend((s.len() as u32).to_le_bytes()); self.0.extend(s.as_bytes()); self }
    pub fn raw(mut self, b: &[u8]) -> Self { self.0.extend(b); self }
}
```

- [ ] **Step 2: Write the failing tests**

```rust
// crates/nif/tests/header.rs
mod builder;
use builder::{Nif, W};
use nif::header::Header;
use nif::NifError;

#[test]
fn parses_block_table_and_offsets() {
    let mut n = Nif::new(100);
    n.string("root");
    n.block("NiNode", vec![0; 10]);
    n.block("BSShaderTextureSet", W::default().u32(0).0);
    let b = n.bytes();
    let h = Header::parse(&b).unwrap();
    assert_eq!(h.bsver, 100);
    assert_eq!(h.kind(1), b"BSShaderTextureSet");
    assert_eq!(h.sizes, vec![10, 4]);
    let off = h.offsets();
    assert_eq!(off[1], off[0] + 10);
    assert_eq!(h.string(0), Some(&b"root"[..]));
    assert_eq!(h.string(-1), None);
    h.check_file_length(&b).unwrap();
}

#[test]
fn block_type_index_top_bit_is_masked() {
    let mut n = Nif::new(100);
    n.block("NiNode", vec![]);
    let mut b = n.bytes();
    // the type-index array sits right after the one type name; set its top bit (PhysX flag, plan 01 Q1)
    let pos = b.windows(6).position(|w| w == b"NiNode").unwrap() + 6;
    b[pos + 1] |= 0x80;
    let h = Header::parse(&b).unwrap();
    assert_eq!(h.kind(0), b"NiNode");
}

#[test]
fn rejects_fo4_and_mw_headers() {
    let mut n = Nif::new(130);
    n.block("NiNode", vec![]);
    assert!(matches!(Header::parse(&n.bytes()), Err(NifError::NotSkyrim { bsver: 130, .. })));
    let mut n = Nif::new(100);
    n.user = 11;
    assert!(matches!(Header::parse(&n.bytes()), Err(NifError::NotSkyrim { user: 11, .. })));
    let mw = include_bytes!("fixtures/TestNifFile_Skinned_MW.nif");
    assert!(matches!(Header::parse(mw), Err(NifError::NotSkyrim { .. })));
    let fo4 = include_bytes!("fixtures/TestNifFile_Skinned_FO4.nif");
    assert!(matches!(Header::parse(fo4), Err(NifError::NotSkyrim { bsver: 130, .. })));
}

#[test]
fn truncated_file_is_error_not_panic() {
    let mut n = Nif::new(100);
    n.block("NiNode", vec![0; 10]);
    let b = n.bytes();
    for cut in [0, 5, 40, 60, b.len() - 3] {
        let r = Header::parse(&b[..cut]).and_then(|h| h.check_file_length(&b[..cut]));
        assert!(r.is_err(), "cut at {cut}");
    }
}

#[test]
fn file_length_identity_catches_wrong_sizes() {
    let mut n = Nif::new(100);
    n.block("NiNode", vec![0; 10]);
    let mut b = n.bytes();
    b.insert(b.len() - 8, 0); // one stray byte before the footer
    let h = Header::parse(&b).unwrap();
    assert!(matches!(h.check_file_length(&b), Err(NifError::FileLength { .. })));
}

#[test]
fn real_se_and_le_fixtures_pass_the_identity() {
    for f in ["TestNifFile_Skinned_SE.nif", "TestNifFile_Furniture_Col_SE.nif", "TestNifFile_Animated_LE.nif"] {
        let b = std::fs::read(format!("{}/tests/fixtures/{f}", env!("CARGO_MANIFEST_DIR"))).unwrap();
        let h = Header::parse(&b).unwrap();
        assert!(h.bsver == 100 || h.bsver == 83, "{f}");
        h.check_file_length(&b).unwrap();
    }
}
```

- [ ] **Step 3: Run to verify failure**

Run: `cargo test -p nif --test header`
Expected: compile error: `Header` not found.

- [ ] **Step 4: Implement**

```rust
// crates/nif/src/header.rs
//! NIF 20.2.0.7 header (nif.xml Header 1963-1983; plan 01 Q1). Only Skyrim LE (BSVER 83) and SE (100) pass.
use crate::cursor::Cursor;
use crate::NifError;

pub struct Header {
    pub bsver: u32,
    pub block_types: Vec<Vec<u8>>,
    pub type_index: Vec<u16>,
    pub sizes: Vec<u32>,
    pub strings: Vec<Vec<u8>>,
    /// Offset of block 0.
    pub data_start: usize,
}

const MAX_BLOCKS: u32 = 1 << 20; // nifly NIF_BLOCK_INDEX_LIMIT

impl Header {
    pub fn parse(b: &[u8]) -> Result<Header, NifError> {
        let nl = b.iter().take(128).position(|&c| c == b'\n').ok_or(NifError::BadHeaderLine)?;
        if !(b.starts_with(b"Gamebryo File Format") || b.starts_with(b"NetImmerse File Format")) {
            return Err(NifError::BadHeaderLine);
        }
        let mut c = Cursor::new(b, nl + 1);
        // MW/OB/FO3 headers end up here too and fail the version gate with NotSkyrim (better message than BadHeaderLine)
        let version = c.u32()?;
        if version != 0x1402_0007 {
            return Err(NifError::NotSkyrim { version, user: 0, bsver: 0 });
        }
        if c.u8()? != 1 {
            return Err(NifError::BadEndian);
        }
        let user = c.u32()?;
        let nblocks = c.u32()?;
        let bsver = c.u32()?;
        if user != 12 || !(bsver == 83 || bsver == 100) || nblocks > MAX_BLOCKS {
            return Err(NifError::NotSkyrim { version, user, bsver });
        }
        c.export_bytes()?; // author
        c.export_bytes()?; // process script (BSVER < 131)
        c.export_bytes()?; // export script; no Max Filepath below BSVER 103
        let ntypes = c.u16()? as usize;
        let block_types = (0..ntypes).map(|_| c.sized_bytes().map(<[u8]>::to_vec)).collect::<Result<Vec<_>, _>>()?;
        let type_index = (0..nblocks).map(|_| c.u16().map(|t| t & 0x7FFF)).collect::<Result<Vec<_>, _>>()?;
        if type_index.iter().any(|&t| t as usize >= ntypes) {
            return Err(NifError::Truncated { at: c.pos(), need: 0 });
        }
        let sizes = (0..nblocks).map(|_| c.u32()).collect::<Result<Vec<_>, _>>()?;
        let nstrings = c.u32()?;
        c.u32()?; // max string length
        let strings = (0..nstrings).map(|_| c.sized_bytes().map(<[u8]>::to_vec)).collect::<Result<Vec<_>, _>>()?;
        let ngroups = c.u32()? as usize;
        c.skip(4 * ngroups)?;
        Ok(Header { bsver, block_types, type_index, sizes, strings, data_start: c.pos() })
    }

    pub fn offsets(&self) -> Vec<usize> {
        let mut p = self.data_start;
        self.sizes
            .iter()
            .map(|&s| {
                let o = p;
                p += s as usize;
                o
            })
            .collect()
    }

    pub fn kind(&self, block: usize) -> &[u8] {
        &self.block_types[self.type_index[block] as usize]
    }

    pub fn string(&self, idx: i32) -> Option<&[u8]> {
        usize::try_from(idx).ok().and_then(|i| self.strings.get(i)).map(Vec::as_slice)
    }

    /// data_start + Σ sizes + 4 + 4·roots == file length (plan 01 Q1 scanner rule a).
    pub fn check_file_length(&self, b: &[u8]) -> Result<(), NifError> {
        let end: usize = self.data_start + self.sizes.iter().map(|&s| s as usize).sum::<usize>();
        let mut c = Cursor::new(b, end);
        let roots = c.u32().map_err(|_| NifError::FileLength { expected: end + 4, actual: b.len() })? as usize;
        let expected = end + 4 + 4 * roots;
        if expected != b.len() {
            return Err(NifError::FileLength { expected, actual: b.len() });
        }
        Ok(())
    }
}
```

- [ ] **Step 5: Run to verify pass**

Run: `cargo test -p nif --test header`
Expected: 6 PASS. If `real_se_and_le_fixtures_pass_the_identity` fails on a fixture, print `h.bsver` and the two lengths. Do not loosen the check. Report it to the reviewer, because 06 measured 87,265/87,265 real files passing.

- [ ] **Step 6: Commit**

```bash
git add crates/nif
git commit -m "feat(nif): header parse, Skyrim gate, block offsets, file-length identity"
```

---

### Task 3: Canonical texture paths

**Files:**
- Create: `crates/nif/src/path.rs`, `crates/nif/tests/path.rs`

**Interfaces:**
- Produces:
  - `nif::path::TexPath { pub raw: String, pub norm: String, pub flags: PathFlags }`;
  - `PathFlags { pub empty: bool, pub rebased: bool, pub prefixed: bool, pub non_dds: bool }`;
  - `nif::path::canon_texture(raw: &[u8]) -> TexPath`;
  - `nif::path::canon_mesh(rel: &str) -> String`;
  - `nif::path::is_facegen(norm: &str) -> bool`.

- [ ] **Step 1: Write the failing test**

```rust
// crates/nif/tests/path.rs
use nif::path::{canon_mesh, canon_texture, is_facegen};

#[test]
fn odd_paths_table() {
    // (raw, norm, empty, rebased, prefixed, non_dds)
    let cases: &[(&[u8], &str, bool, bool, bool, bool)] = &[
        (b"textures\\armor\\iron\\iron_d.dds", "textures/armor/iron/iron_d.dds", false, false, false, false),
        (b"Textures\\Armor\\Iron_N.DDS", "textures/armor/iron_n.dds", false, false, false, false),
        (b"Data\\Textures\\x\\a.dds", "textures/x/a.dds", false, true, false, false),
        (b"C:\\build\\Skyrim\\Data\\textures\\x\\a.dds", "textures/x/a.dds", false, true, false, false),
        (b"armor\\iron\\iron_d.dds", "textures/armor/iron/iron_d.dds", false, false, true, false),
        (b"textures/x/a.dds  ", "textures/x/a.dds", false, false, false, false),
        (b"textures\\x\\a.tga", "textures/x/a.tga", false, false, false, true),
        (b"", "", true, false, false, false),
        (b"   ", "", true, false, false, false),
    ];
    for &(raw, norm, empty, rebased, prefixed, non_dds) in cases {
        let t = canon_texture(raw);
        assert_eq!(t.norm, norm, "{:?}", String::from_utf8_lossy(raw));
        assert_eq!(
            (t.flags.empty, t.flags.rebased, t.flags.prefixed, t.flags.non_dds),
            (empty, rebased, prefixed, non_dds),
            "{:?}",
            String::from_utf8_lossy(raw)
        );
        assert_eq!(t.raw, nif::text::lossy(raw));
    }
}

#[test]
fn mesh_paths_and_facegen() {
    assert_eq!(canon_mesh("Meshes\\Armor\\Iron\\x.NIF"), "meshes/armor/iron/x.nif");
    assert!(is_facegen("meshes/actors/character/facegendata/facegeom/skyrim.esm/00013bbd.nif"));
    assert!(is_facegen("textures/actors/character/facegendata/facetint/skyrim.esm/00013bbd.dds"));
    assert!(!is_facegen("meshes/actors/character/character assets/malehead.nif"));
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p nif --test path`
Expected: compile error.

- [ ] **Step 3: Implement**

```rust
// crates/nif/src/path.rs
//! Canonical form (00-decision §2 rule 5): ASCII-lowercase, '/' separators, relative to Data, starting
//! `textures/` — equal to `engine::discover::game_path` for the same file.
use crate::text::lossy;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PathFlags {
    pub empty: bool,
    /// An absolute or `data/`-prefixed path was cut back to `textures/...`.
    pub rebased: bool,
    /// `textures/` was missing and was added.
    pub prefixed: bool,
    pub non_dds: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TexPath {
    pub raw: String,
    pub norm: String,
    pub flags: PathFlags,
}

fn slashes_lower(s: &str) -> String {
    s.trim().replace('\\', "/").to_ascii_lowercase().trim_start_matches('/').to_string()
}

pub fn canon_texture(raw: &[u8]) -> TexPath {
    let raw_s = lossy(raw);
    let mut flags = PathFlags::default();
    let mut s = slashes_lower(&raw_s);
    if s.is_empty() {
        flags.empty = true;
        return TexPath { raw: raw_s, norm: s, flags };
    }
    if !s.starts_with("textures/") {
        let comps: Vec<&str> = s.split('/').collect();
        if let Some(i) = comps.iter().rposition(|c| *c == "textures") {
            s = comps[i..].join("/");
            flags.rebased = true;
        } else {
            s = format!("textures/{s}");
            flags.prefixed = true;
        }
    }
    flags.non_dds = !s.ends_with(".dds");
    TexPath { raw: raw_s, norm: s, flags }
}

pub fn canon_mesh(rel: &str) -> String {
    slashes_lower(rel)
}

pub fn is_facegen(norm: &str) -> bool {
    norm.starts_with("meshes/actors/character/facegendata/") || norm.starts_with("textures/actors/character/facegendata/")
}
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p nif --test path`
Expected: 2 PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/nif
git commit -m "feat(nif): canonical texture/mesh paths matching engine game_path"
```

---

### Task 4: Property block decoders (lighting, effect, texture set, alpha) with size check

**Files:**
- Create: `crates/nif/src/props.rs`, `crates/nif/tests/props.rs`

**Interfaces:**
- Consumes: `Cursor`, `Header`, `TexPath`, `canon_texture`.
- Produces:
  - `LightingParams { uv_offset: [f32;2], uv_scale: [f32;2], emissive_color: [f32;3], emissive_multiple: f32, texture_clamp_mode: u32, alpha: f32, refraction_strength: f32, glossiness: f32, specular_color: [f32;3], specular_strength: f32, lighting_effect_1: f32, lighting_effect_2: f32, trailer: LightingTrailer }`.
  - `enum LightingTrailer`:
    - `None`
    - `EnvMap { scale: f32 }`
    - `SkinTint([f32;3])`
    - `HairTint([f32;3])`
    - `ParallaxOcc { max_passes: f32, scale: f32 }`
    - `MultiLayer { inner_thickness: f32, refraction_scale: f32, inner_texture_scale: [f32;2], envmap_strength: f32 }`
    - `SparkleSnow([f32;4])`
    - `Eye { cubemap_scale: f32, left_center: [f32;3], right_center: [f32;3] }`
  - `Lighting { name: Option<String>, shader_type: u32, slsf1: u32, slsf2: u32, texture_set: i32, params: LightingParams, field_offsets: FieldOffsets }`.
  - `FieldOffsets { block_offset: usize, slsf1: usize, uv_offset: usize, emissive_color: usize, glossiness: usize, trailer: usize }`: absolute file offsets for later in-place patching (01 Q8).
  - `EffectParams { uv_offset, uv_scale, texture_clamp_mode: u8, lighting_influence: u8, env_map_min_lod: u8, falloff: [f32;4], base_color: [f32;4], base_color_scale: f32, soft_falloff_depth: f32 }`.
  - `Effect { name, slsf1, slsf2, source: TexPath, greyscale: TexPath, params: EffectParams }`.
  - `TextureSet { slots: Vec<TexPath> }`.
  - `Alpha { flags: u16, threshold: u8 }` with methods `blend() -> bool` (bit 0) and `test() -> bool` (bit 9).
  - `pub fn decode_lighting(h: &Header, b: &[u8], block: usize) -> Result<Lighting, NifError>`, plus `decode_effect`, `decode_texture_set` and `decode_alpha` with the same signature. Each returns `NifError::BlockSize` when consumed ≠ declared.

- [ ] **Step 1: Write the failing tests**

```rust
// crates/nif/tests/props.rs
mod builder;
use builder::{Nif, W};
use nif::header::Header;
use nif::props::*;
use nif::NifError;

/// BSLightingShaderProperty body (plan 01 Q2 table), no extra data, type-specific trailer appended by caller.
fn lsp(shader_type: u32, name: i32, texset: i32) -> W {
    W::default()
        .u32(shader_type).i32(name).u32(0).i32(-1)
        .u32(0x8000_0001).u32(0x0000_0021)       // SLSF1, SLSF2
        .fs(&[0.5, 0.25]).fs(&[2.0, 2.0])         // uv offset, uv scale
        .i32(texset)
        .fs(&[0.1, 0.2, 0.3]).f(1.5)              // emissive colour, multiple
        .u32(3).f(1.0).f(0.0).f(80.0)             // clamp, alpha, refraction, glossiness
        .fs(&[1.0, 0.9, 0.8]).f(2.0)              // specular colour, strength
        .f(0.3).f(2.0)                            // lighting effect 1, 2
}

#[test]
fn lighting_default_and_trailers() {
    let cases: Vec<(u32, Vec<f32>, fn(&LightingTrailer) -> bool)> = vec![
        (0, vec![], |t| matches!(t, LightingTrailer::None)),
        (1, vec![0.7], |t| matches!(t, LightingTrailer::EnvMap { scale } if *scale == 0.7)),
        (5, vec![1.0, 0.5, 0.25], |t| matches!(t, LightingTrailer::SkinTint(c) if c[1] == 0.5)),
        (6, vec![1.0, 0.5, 0.25], |t| matches!(t, LightingTrailer::HairTint(_))),
        (7, vec![4.0, 0.1], |t| matches!(t, LightingTrailer::ParallaxOcc { .. })),
        (11, vec![1.0, 2.0, 3.0, 4.0, 5.0], |t| matches!(t, LightingTrailer::MultiLayer { envmap_strength, .. } if *envmap_strength == 5.0)),
        (14, vec![1.0, 2.0, 3.0, 4.0], |t| matches!(t, LightingTrailer::SparkleSnow(_))),
        (16, vec![1.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0], |t| matches!(t, LightingTrailer::Eye { right_center, .. } if right_center[2] == 1.0)),
        (2, vec![], |t| matches!(t, LightingTrailer::None)),
    ];
    for (st, trailer, check) in cases {
        let mut n = Nif::new(100);
        let name = n.string("mat");
        n.block("BSLightingShaderProperty", lsp(st, name, 1).fs(&trailer).0);
        let b = n.bytes();
        let h = Header::parse(&b).unwrap();
        let l = decode_lighting(&h, &b, 0).unwrap_or_else(|e| panic!("type {st}: {e}"));
        assert_eq!(l.shader_type, st);
        assert_eq!((l.slsf1, l.slsf2, l.texture_set), (0x8000_0001, 0x21, 1));
        assert_eq!(l.name.as_deref(), Some("mat"));
        assert_eq!(l.params.uv_scale, [2.0, 2.0]);
        assert_eq!(l.params.glossiness, 80.0);
        assert!(check(&l.params.trailer), "type {st}");
    }
}

#[test]
fn lighting_with_extra_data_refs() {
    let mut n = Nif::new(100);
    let body = W::default().u32(0).i32(-1).u32(2).i32(5).i32(6).i32(-1)
        .raw(&lsp(0, -1, 3).0[16..]); // reuse the tail after the NiObjectNET prefix
    n.block("BSLightingShaderProperty", body.0);
    let b = n.bytes();
    let h = Header::parse(&b).unwrap();
    let l = decode_lighting(&h, &b, 0).unwrap();
    assert_eq!(l.texture_set, 3);
    assert_eq!(l.name, None);
}

#[test]
fn wrong_declared_size_is_reported() {
    let mut n = Nif::new(100);
    n.block("BSLightingShaderProperty", lsp(0, -1, 1).u32(0).0); // 4 stray bytes
    let b = n.bytes();
    let h = Header::parse(&b).unwrap();
    assert!(matches!(decode_lighting(&h, &b, 0), Err(NifError::BlockSize { declared, consumed, .. }) if declared == consumed + 4));
}

#[test]
fn texture_set_reads_its_count() {
    for count in [9u32, 6, 1, 0] {
        let mut w = W::default().u32(count);
        for i in 0..count {
            w = w.sized(if i == 0 { "textures\\a_d.dds" } else { "" });
        }
        let mut n = Nif::new(100);
        n.block("BSShaderTextureSet", w.0);
        let b = n.bytes();
        let h = Header::parse(&b).unwrap();
        let t = decode_texture_set(&h, &b, 0).unwrap();
        assert_eq!(t.slots.len(), count as usize);
        if count > 0 {
            assert_eq!(t.slots[0].norm, "textures/a_d.dds");
        }
    }
}

#[test]
fn effect_has_two_inline_strings() {
    let body = W::default().i32(-1).u32(0).i32(-1)
        .u32(1).u32(2).fs(&[0.0, 0.0]).fs(&[1.0, 1.0])
        .sized("textures\\effects\\fx.dds")
        .u8(3).u8(255).u8(0).u8(0)
        .fs(&[90.0, 0.0, 1.0, 0.0])
        .fs(&[1.0, 1.0, 1.0, 1.0]).f(2.0).f(100.0)
        .sized("textures\\effects\\gradients\\g.dds");
    let mut n = Nif::new(100);
    n.block("BSEffectShaderProperty", body.0);
    let b = n.bytes();
    let h = Header::parse(&b).unwrap();
    let e = decode_effect(&h, &b, 0).unwrap();
    assert_eq!(e.source.norm, "textures/effects/fx.dds");
    assert_eq!(e.greyscale.norm, "textures/effects/gradients/g.dds");
    assert_eq!(e.params.base_color_scale, 2.0);
    assert_eq!(e.params.lighting_influence, 255);
}

#[test]
fn alpha_flags() {
    let mut n = Nif::new(100);
    n.block("NiAlphaProperty", W::default().i32(-1).u32(0).i32(-1).u16(0x12ED).u8(128).0);
    let b = n.bytes();
    let h = Header::parse(&b).unwrap();
    let a = decode_alpha(&h, &b, 0).unwrap();
    assert_eq!(a.threshold, 128);
    assert!(a.blend() && a.test());
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p nif --test props`
Expected: compile error.

- [ ] **Step 3: Implement**

```rust
// crates/nif/src/props.rs
//! Small property blocks, decoded fully and checked against the declared block size (plan 01 Q2, nif.xml
//! 6581-6685, 6307-6319, 3972-3978). BSVER 83 and 100 share these layouts.
use crate::cursor::Cursor;
use crate::header::Header;
use crate::path::{canon_texture, TexPath};
use crate::text::lossy;
use crate::NifError;

#[derive(Debug, Clone, PartialEq)]
pub enum LightingTrailer {
    None,
    EnvMap { scale: f32 },
    SkinTint([f32; 3]),
    HairTint([f32; 3]),
    ParallaxOcc { max_passes: f32, scale: f32 },
    MultiLayer { inner_thickness: f32, refraction_scale: f32, inner_texture_scale: [f32; 2], envmap_strength: f32 },
    SparkleSnow([f32; 4]),
    Eye { cubemap_scale: f32, left_center: [f32; 3], right_center: [f32; 3] },
}

#[derive(Debug, Clone, PartialEq)]
pub struct LightingParams {
    pub uv_offset: [f32; 2],
    pub uv_scale: [f32; 2],
    pub emissive_color: [f32; 3],
    pub emissive_multiple: f32,
    pub texture_clamp_mode: u32,
    pub alpha: f32,
    pub refraction_strength: f32,
    pub glossiness: f32,
    pub specular_color: [f32; 3],
    pub specular_strength: f32,
    pub lighting_effect_1: f32,
    pub lighting_effect_2: f32,
    pub trailer: LightingTrailer,
}

/// Absolute file offsets of editable fields, for a later in-place patcher (plan 01 Q8).
#[derive(Debug, Clone, PartialEq)]
pub struct FieldOffsets {
    pub block_offset: usize,
    pub slsf1: usize,
    pub uv_offset: usize,
    pub emissive_color: usize,
    pub glossiness: usize,
    pub trailer: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Lighting {
    pub name: Option<String>,
    pub shader_type: u32,
    pub slsf1: u32,
    pub slsf2: u32,
    pub texture_set: i32,
    pub params: LightingParams,
    pub field_offsets: FieldOffsets,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EffectParams {
    pub uv_offset: [f32; 2],
    pub uv_scale: [f32; 2],
    pub texture_clamp_mode: u8,
    pub lighting_influence: u8,
    pub env_map_min_lod: u8,
    /// start angle, stop angle, start opacity, stop opacity
    pub falloff: [f32; 4],
    pub base_color: [f32; 4],
    pub base_color_scale: f32,
    pub soft_falloff_depth: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Effect {
    pub name: Option<String>,
    pub slsf1: u32,
    pub slsf2: u32,
    pub source: TexPath,
    pub greyscale: TexPath,
    pub params: EffectParams,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TextureSet {
    pub slots: Vec<TexPath>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Alpha {
    pub flags: u16,
    pub threshold: u8,
}
impl Alpha {
    pub fn blend(&self) -> bool {
        self.flags & 1 != 0
    }
    pub fn test(&self) -> bool {
        self.flags & (1 << 9) != 0
    }
}

fn block_cursor<'a>(h: &Header, b: &'a [u8], block: usize) -> (Cursor<'a>, usize, usize) {
    let start = h.offsets()[block];
    (Cursor::new(b, start), start, start + h.sizes[block] as usize)
}

fn finish(h: &Header, block: usize, c: &Cursor, start: usize, end: usize) -> Result<(), NifError> {
    if c.pos() != end {
        return Err(NifError::BlockSize {
            block: block as u32,
            kind: lossy(h.kind(block)),
            declared: h.sizes[block],
            consumed: (c.pos() - start) as u32,
        });
    }
    Ok(())
}

/// NiObjectNET: Name (string index), NumExtraData + refs, Controller.
fn object_net(h: &Header, c: &mut Cursor) -> Result<Option<String>, NifError> {
    let name = h.string(c.i32()?).map(lossy).filter(|s| !s.is_empty());
    let n = c.u32()? as usize;
    c.skip(4 * n)?;
    c.i32()?;
    Ok(name)
}

pub fn decode_lighting(h: &Header, b: &[u8], block: usize) -> Result<Lighting, NifError> {
    let (mut c, start, end) = block_cursor(h, b, block);
    let shader_type = c.u32()?; // before the name at BSVER 83..139 (plan 01 Q2)
    let name = object_net(h, &mut c)?;
    let slsf1_off = c.pos();
    let slsf1 = c.u32()?;
    let slsf2 = c.u32()?;
    let uv_off = c.pos();
    let uv_offset = c.f32s::<2>()?;
    let uv_scale = c.f32s::<2>()?;
    let texture_set = c.i32()?;
    let em_off = c.pos();
    let emissive_color = c.f32s::<3>()?;
    let emissive_multiple = c.f32()?;
    let texture_clamp_mode = c.u32()?;
    let alpha = c.f32()?;
    let refraction_strength = c.f32()?;
    let gloss_off = c.pos();
    let glossiness = c.f32()?;
    let specular_color = c.f32s::<3>()?;
    let specular_strength = c.f32()?;
    let lighting_effect_1 = c.f32()?;
    let lighting_effect_2 = c.f32()?;
    let trailer_off = c.pos();
    let trailer = match shader_type {
        1 => LightingTrailer::EnvMap { scale: c.f32()? },
        5 => LightingTrailer::SkinTint(c.f32s::<3>()?),
        6 => LightingTrailer::HairTint(c.f32s::<3>()?),
        7 => LightingTrailer::ParallaxOcc { max_passes: c.f32()?, scale: c.f32()? },
        11 => LightingTrailer::MultiLayer {
            inner_thickness: c.f32()?,
            refraction_scale: c.f32()?,
            inner_texture_scale: c.f32s::<2>()?,
            envmap_strength: c.f32()?,
        },
        14 => LightingTrailer::SparkleSnow(c.f32s::<4>()?),
        16 => LightingTrailer::Eye { cubemap_scale: c.f32()?, left_center: c.f32s::<3>()?, right_center: c.f32s::<3>()? },
        _ => LightingTrailer::None,
    };
    finish(h, block, &c, start, end)?;
    Ok(Lighting {
        name,
        shader_type,
        slsf1,
        slsf2,
        texture_set,
        params: LightingParams {
            uv_offset, uv_scale, emissive_color, emissive_multiple, texture_clamp_mode, alpha,
            refraction_strength, glossiness, specular_color, specular_strength, lighting_effect_1,
            lighting_effect_2, trailer,
        },
        field_offsets: FieldOffsets {
            block_offset: start, slsf1: slsf1_off, uv_offset: uv_off, emissive_color: em_off,
            glossiness: gloss_off, trailer: trailer_off,
        },
    })
}

pub fn decode_effect(h: &Header, b: &[u8], block: usize) -> Result<Effect, NifError> {
    let (mut c, start, end) = block_cursor(h, b, block);
    let name = object_net(h, &mut c)?;
    let slsf1 = c.u32()?;
    let slsf2 = c.u32()?;
    let uv_offset = c.f32s::<2>()?;
    let uv_scale = c.f32s::<2>()?;
    let source = canon_texture(c.sized_bytes()?);
    let texture_clamp_mode = c.u8()?;
    let lighting_influence = c.u8()?;
    let env_map_min_lod = c.u8()?;
    c.u8()?;
    let falloff = c.f32s::<4>()?;
    let base_color = c.f32s::<4>()?;
    let base_color_scale = c.f32()?;
    let soft_falloff_depth = c.f32()?;
    let greyscale = canon_texture(c.sized_bytes()?);
    finish(h, block, &c, start, end)?;
    Ok(Effect {
        name, slsf1, slsf2, source, greyscale,
        params: EffectParams {
            uv_offset, uv_scale, texture_clamp_mode, lighting_influence, env_map_min_lod, falloff,
            base_color, base_color_scale, soft_falloff_depth,
        },
    })
}

pub fn decode_texture_set(h: &Header, b: &[u8], block: usize) -> Result<TextureSet, NifError> {
    let (mut c, start, end) = block_cursor(h, b, block);
    let n = c.u32()? as usize;
    let slots = (0..n).map(|_| c.sized_bytes().map(canon_texture)).collect::<Result<Vec<_>, _>>()?;
    finish(h, block, &c, start, end)?;
    Ok(TextureSet { slots })
}

pub fn decode_alpha(h: &Header, b: &[u8], block: usize) -> Result<Alpha, NifError> {
    let (mut c, start, end) = block_cursor(h, b, block);
    object_net(h, &mut c)?;
    let flags = c.u16()?;
    let threshold = c.u8()?;
    finish(h, block, &c, start, end)?;
    Ok(Alpha { flags, threshold })
}
```

Note: `block_cursor` calls `h.offsets()` per block. Task 5 replaces this with a precomputed slice: add `offsets: Vec<usize>` to `Header`, computed once in `parse`, and make `offsets()` return `&[usize]`. That refactor lands in Task 5 with its own test, so leave it.

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p nif --test props`
Expected: 6 PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/nif
git commit -m "feat(nif): decode lighting/effect/texture-set/alpha blocks with size check"
```

---

### Task 5: Shape heads and layer-0 `scan` (one row per shape)

**Files:**
- Create: `crates/nif/src/shape_head.rs`, `crates/nif/tests/scan.rs`, `crates/nif/tests/fixtures.rs`
- Modify: `crates/nif/src/scan.rs`; `crates/nif/src/header.rs` (cache offsets: `pub offsets: Vec<usize>` filled in `parse`, `offsets()` returns `&[usize]`); `crates/nif/src/props.rs` (`block_cursor` uses `h.offsets()[block]`, now a slice index)

**Interfaces:**
- Consumes: Tasks 2–4.
- Produces:
  - `nif::shape_head::ShapeKind` enum: `BSTriShape, BSSubIndexTriShape, BSDynamicTriShape, BSMeshLODTriShape, NiTriShape, NiTriStrips, BSLODTriShape, BSSegmentedTriShape, NiParticleSystem, BSStripParticleSystem`, with `ShapeKind::from_name(&[u8]) -> Option<ShapeKind>`.
  - `ShapeHead { kind, name: Option<String>, skinned: bool, shader: i32, alpha: i32, vertex_desc: Option<u64>, num_triangles: Option<u32>, num_vertices: Option<u32> }`.
  - `nif::scan::Shader` enum: `None`, `Lighting(Lighting, Vec<TexPath>)` (the slots come from its texture set; empty if ref −1), `Effect(Effect)`, `Other { kind: String }`.
  - `ShapeRow { block: u32, head: ShapeHead, shader: Shader, alpha: Option<Alpha> }`.
  - `NifScan { bsver: u32, le_mesh: bool, shapes: Vec<ShapeRow>, problems: Vec<String> }`.
  - `pub fn scan(bytes: &[u8]) -> Result<NifScan, NifError>`.
    - Hard errors are reserved for header failures and the file-length check.
    - A single bad property block goes into `problems` and that shape's shader becomes `Shader::None`.

- [ ] **Step 1: Write the failing tests**

```rust
// crates/nif/tests/scan.rs
mod builder;
use builder::{Nif, W};
use nif::scan::{scan, Shader};
use nif::shape_head::ShapeKind;

fn net(name: i32) -> W { W::default().i32(name).u32(0).i32(-1) }
fn avobj(w: W) -> W { w.u32(14).fs(&[0.0; 3]).fs(&[1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]).f(1.0).i32(-1) }
fn texset(paths: &[&str]) -> Vec<u8> {
    let mut w = W::default().u32(paths.len() as u32);
    for p in paths { w = w.sized(p); }
    w.0
}
fn lsp(st: u32, ts: i32) -> Vec<u8> {
    W::default().u32(st).i32(-1).u32(0).i32(-1).u32(0x8000_0000).u32(0x21)
        .fs(&[0.0, 0.0]).fs(&[1.0, 1.0]).i32(ts).fs(&[0.0; 3]).f(1.0).u32(3).f(1.0).f(0.0).f(80.0)
        .fs(&[1.0; 3]).f(1.0).f(0.3).f(2.0).0
}

#[test]
fn bstrishape_links_shader_texset_alpha() {
    let mut n = Nif::new(100);
    let nm = n.string("Torso");
    // block 0 shape, 1 lsp, 2 texset, 3 alpha
    let shape = avobj(net(nm)).fs(&[0.0, 0.0, 0.0, 10.0]).i32(-1).i32(1).i32(3)
        .u64(0x0000_B000_0000_0007).u16(12).u16(30).u32(0).raw(&[0u8; 16]);
    n.block("BSTriShape", shape.0);
    n.block("BSLightingShaderProperty", lsp(0, 2));
    n.block("BSShaderTextureSet", texset(&["textures\\a_d.dds", "textures\\a_n.dds", "", "", "", "", "", "", ""]));
    n.block("NiAlphaProperty", net(-1).u16(0x00ED).u8(128).0);
    let s = scan(&n.bytes()).unwrap();
    assert_eq!((s.bsver, s.le_mesh), (100, false));
    assert_eq!(s.shapes.len(), 1);
    let r = &s.shapes[0];
    assert_eq!(r.head.kind, ShapeKind::BSTriShape);
    assert_eq!(r.head.name.as_deref(), Some("Torso"));
    assert_eq!((r.head.num_triangles, r.head.num_vertices), (Some(12), Some(30)));
    match &r.shader {
        Shader::Lighting(l, slots) => {
            assert_eq!(l.shader_type, 0);
            assert_eq!(slots[1].norm, "textures/a_n.dds");
            assert_eq!(slots.len(), 9);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(r.alpha.unwrap().threshold, 128);
}

#[test]
fn nitrishape_le_layout_and_flag() {
    let mut n = Nif::new(83);
    let shape = avobj(net(-1)).i32(-1).i32(-1).u32(0).i32(-1).u8(0).i32(1).i32(-1);
    n.block("NiTriShape", shape.0);
    n.block("BSLightingShaderProperty", lsp(1, -1).into_iter().chain(0.5f32.to_le_bytes()).collect());
    let s = scan(&n.bytes()).unwrap();
    assert!(s.le_mesh);
    assert!(matches!(&s.shapes[0].shader, Shader::Lighting(l, slots) if l.shader_type == 1 && slots.is_empty()));
}

#[test]
fn particle_system_owns_effect_shader() {
    let mut n = Nif::new(100);
    let ps = avobj(net(-1)).fs(&[0.0; 4]).i32(-1).i32(1).i32(-1).raw(&[0u8; 20]);
    n.block("NiParticleSystem", ps.0);
    let esp = net(-1).u32(0).u32(0).fs(&[0.0, 0.0]).fs(&[1.0, 1.0]).sized("textures\\fx\\a.dds")
        .u8(3).u8(0).u8(0).u8(0).fs(&[0.0; 4]).fs(&[1.0; 4]).f(1.0).f(100.0).sized("");
    n.block("BSEffectShaderProperty", esp.0);
    let s = scan(&n.bytes()).unwrap();
    assert!(matches!(&s.shapes[0].shader, Shader::Effect(e) if e.source.norm == "textures/fx/a.dds" && e.greyscale.flags.empty));
}

#[test]
fn bad_property_block_is_a_problem_not_a_failure() {
    let mut n = Nif::new(100);
    let shape = avobj(net(-1)).fs(&[0.0; 4]).i32(-1).i32(1).i32(-1).u64(0).u16(0).u16(0).u32(0);
    n.block("BSTriShape", shape.0);
    n.block("BSLightingShaderProperty", { let mut v = lsp(0, -1); v.extend([0u8; 4]); v });
    let s = scan(&n.bytes()).unwrap();
    assert!(matches!(s.shapes[0].shader, Shader::None));
    assert_eq!(s.problems.len(), 1);
}

#[test]
fn ref_out_of_range_is_a_problem() {
    let mut n = Nif::new(100);
    let shape = avobj(net(-1)).fs(&[0.0; 4]).i32(-1).i32(42).i32(-1).u64(0).u16(0).u16(0).u32(0);
    n.block("BSTriShape", shape.0);
    let s = scan(&n.bytes()).unwrap();
    assert!(matches!(s.shapes[0].shader, Shader::None));
    assert_eq!(s.problems.len(), 1);
}
```

```rust
// crates/nif/tests/fixtures.rs — real files written by nifly (GPL-3.0 fixtures)
use nif::scan::{scan, Shader};

fn read(f: &str) -> Vec<u8> {
    std::fs::read(format!("{}/tests/fixtures/{f}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

#[test]
fn se_fixtures_scan_without_problems() {
    for f in ["TestNifFile_Skinned_SE.nif", "TestNifFile_Furniture_Col_SE.nif", "TestNifFile_DeepGraph_SE.nif",
              "TestNifFile_Skinned_Dynamic_SE.nif", "TestNifFile_Optimize_LE_to_SE.nif"] {
        let s = scan(&read(f)).unwrap_or_else(|e| panic!("{f}: {e}"));
        assert_eq!(s.bsver, 100, "{f}");
        assert!(s.problems.is_empty(), "{f}: {:?}", s.problems);
        assert!(!s.shapes.is_empty(), "{f}");
        assert!(s.shapes.iter().any(|r| !matches!(r.shader, Shader::None)), "{f}");
    }
}

#[test]
fn le_fixture_is_flagged() {
    let s = scan(&read("TestNifFile_Animated_LE.nif")).unwrap();
    assert!(s.le_mesh);
}

/// Opt-in: walks a real mesh folder. `SDC_NIF_CORPUS=C:\path\to\meshes cargo test -p nif -- --ignored corpus`
#[test]
#[ignore]
fn corpus() {
    let root = std::env::var("SDC_NIF_CORPUS").expect("set SDC_NIF_CORPUS");
    let (mut files, mut bad) = (0, vec![]);
    for e in walkdir::WalkDir::new(root).into_iter().filter_map(Result::ok) {
        if e.path().extension().is_some_and(|x| x.eq_ignore_ascii_case("nif")) {
            files += 1;
            match scan(&std::fs::read(e.path()).unwrap()) {
                Ok(s) if s.problems.is_empty() => {}
                Ok(s) => bad.push(format!("{}: {:?}", e.path().display(), s.problems)),
                Err(nif::NifError::NotSkyrim { .. }) => {}
                Err(err) => bad.push(format!("{}: {err}", e.path().display())),
            }
        }
    }
    println!("{files} files, {} with problems", bad.len());
    assert!(bad.is_empty(), "{:#?}", &bad[..bad.len().min(20)]);
}
```

Add `walkdir.workspace = true` under `[dev-dependencies]` in `crates/nif/Cargo.toml`.

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p nif --test scan --test fixtures`
Expected: compile error.

- [ ] **Step 3: Implement**

```rust
// crates/nif/src/shape_head.rs
//! Shape heads up to the shader/alpha refs (plan 01 Q3; nif.xml 3840-3870, 8228-8246).
use crate::cursor::Cursor;
use crate::header::Header;
use crate::text::lossy;
use crate::NifError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShapeKind {
    BSTriShape, BSSubIndexTriShape, BSDynamicTriShape, BSMeshLODTriShape,
    NiTriShape, NiTriStrips, BSLODTriShape, BSSegmentedTriShape,
    NiParticleSystem, BSStripParticleSystem,
}

impl ShapeKind {
    pub fn from_name(n: &[u8]) -> Option<ShapeKind> {
        use ShapeKind::*;
        Some(match n {
            b"BSTriShape" => BSTriShape,
            b"BSSubIndexTriShape" => BSSubIndexTriShape,
            b"BSDynamicTriShape" => BSDynamicTriShape,
            b"BSMeshLODTriShape" => BSMeshLODTriShape,
            b"NiTriShape" => NiTriShape,
            b"NiTriStrips" => NiTriStrips,
            b"BSLODTriShape" => BSLODTriShape,
            b"BSSegmentedTriShape" => BSSegmentedTriShape,
            b"NiParticleSystem" => NiParticleSystem,
            b"BSStripParticleSystem" => BSStripParticleSystem,
            _ => return None,
        })
    }
    fn bs_layout(self, bsver: u32) -> bool {
        use ShapeKind::*;
        match self {
            BSTriShape | BSSubIndexTriShape | BSDynamicTriShape | BSMeshLODTriShape => true,
            NiParticleSystem | BSStripParticleSystem => bsver >= 100,
            _ => false,
        }
    }
    fn has_vertex_head(self) -> bool {
        use ShapeKind::*;
        matches!(self, BSTriShape | BSSubIndexTriShape | BSDynamicTriShape | BSMeshLODTriShape)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ShapeHead {
    pub kind: ShapeKind,
    pub name: Option<String>,
    pub skinned: bool,
    pub shader: i32,
    pub alpha: i32,
    pub vertex_desc: Option<u64>,
    pub num_triangles: Option<u32>,
    pub num_vertices: Option<u32>,
}

pub fn decode_head(h: &Header, b: &[u8], block: usize, kind: ShapeKind) -> Result<ShapeHead, NifError> {
    let mut c = Cursor::new(b, h.offsets()[block]);
    let name = h.string(c.i32()?).map(lossy).filter(|s| !s.is_empty());
    let n = c.u32()? as usize;
    c.skip(4 * n)?;
    c.i32()?; // controller
    c.u32()?; // NiAVObject flags (u32 at BSVER > 26)
    c.skip(12 + 36 + 4)?; // translation, rotation, scale
    c.i32()?; // collision
    let (skinned, shader, alpha);
    let (mut vertex_desc, mut num_triangles, mut num_vertices) = (None, None, None);
    if kind.bs_layout(h.bsver) {
        c.skip(16)?; // bounding sphere
        skinned = c.i32()? != -1;
        shader = c.i32()?;
        alpha = c.i32()?;
        if kind.has_vertex_head() {
            vertex_desc = Some(c.u64()?);
            num_triangles = Some(c.u16()? as u32); // u16 at BSVER 100 (u32 only from 130)
            num_vertices = Some(c.u16()? as u32);
        }
    } else {
        c.i32()?; // data
        skinned = c.i32()? != -1;
        let m = c.u32()? as usize;
        c.skip(8 * m)?; // material names + extra data
        c.i32()?; // active material
        c.u8()?; // needs update
        shader = c.i32()?;
        alpha = c.i32()?;
    }
    Ok(ShapeHead { kind, name, skinned, shader, alpha, vertex_desc, num_triangles, num_vertices })
}
```

```rust
// crates/nif/src/scan.rs
//! LAYER 0: header → shape heads → follow shader/alpha/texture-set refs. One row per shape (00-decision §2 rule 1).
use crate::header::Header;
use crate::path::TexPath;
use crate::props::{decode_alpha, decode_effect, decode_lighting, decode_texture_set, Alpha, Effect, Lighting};
use crate::shape_head::{decode_head, ShapeHead, ShapeKind};
use crate::text::lossy;
use crate::NifError;

#[derive(Debug, Clone, PartialEq)]
pub enum Shader {
    None,
    Lighting(Lighting, Vec<TexPath>),
    Effect(Effect),
    Other { kind: String },
}

#[derive(Debug, Clone, PartialEq)]
pub struct ShapeRow {
    pub block: u32,
    pub head: ShapeHead,
    pub shader: Shader,
    pub alpha: Option<Alpha>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NifScan {
    pub bsver: u32,
    pub le_mesh: bool,
    pub shapes: Vec<ShapeRow>,
    pub problems: Vec<String>,
}

fn valid(h: &Header, r: i32) -> Option<usize> {
    usize::try_from(r).ok().filter(|&i| i < h.sizes.len())
}

pub fn scan(b: &[u8]) -> Result<NifScan, NifError> {
    let h = Header::parse(b)?;
    h.check_file_length(b)?;
    let mut out = NifScan { bsver: h.bsver, le_mesh: h.bsver == 83, shapes: vec![], problems: vec![] };
    for i in 0..h.sizes.len() {
        let Some(kind) = ShapeKind::from_name(h.kind(i)) else { continue };
        let head = match decode_head(&h, b, i, kind) {
            Ok(hd) => hd,
            Err(e) => {
                out.problems.push(format!("block {i}: {e}"));
                continue;
            }
        };
        let shader = match (head.shader, valid(&h, head.shader)) {
            (-1, _) => Shader::None,
            (r, None) => {
                out.problems.push(format!("block {i}: shader ref {r} out of range"));
                Shader::None
            }
            (_, Some(s)) => match h.kind(s) {
                b"BSLightingShaderProperty" => match decode_lighting(&h, b, s) {
                    Ok(l) => {
                        let slots = match valid(&h, l.texture_set) {
                            Some(t) if h.kind(t) == b"BSShaderTextureSet" => match decode_texture_set(&h, b, t) {
                                Ok(ts) => ts.slots,
                                Err(e) => {
                                    out.problems.push(format!("block {t}: {e}"));
                                    vec![]
                                }
                            },
                            _ => vec![],
                        };
                        Shader::Lighting(l, slots)
                    }
                    Err(e) => {
                        out.problems.push(format!("block {s}: {e}"));
                        Shader::None
                    }
                },
                b"BSEffectShaderProperty" => match decode_effect(&h, b, s) {
                    Ok(e) => Shader::Effect(e),
                    Err(e) => {
                        out.problems.push(format!("block {s}: {e}"));
                        Shader::None
                    }
                },
                other => Shader::Other { kind: lossy(other) },
            },
        };
        let alpha = valid(&h, head.alpha)
            .filter(|&a| h.kind(a) == b"NiAlphaProperty")
            .and_then(|a| match decode_alpha(&h, b, a) {
                Ok(x) => Some(x),
                Err(e) => {
                    out.problems.push(format!("block {a}: {e}"));
                    None
                }
            });
        out.shapes.push(ShapeRow { block: i as u32, head, shader, alpha });
    }
    Ok(out)
}
```

In `header.rs`, add `pub offsets: Vec<usize>` to `Header`, compute it at the end of `parse` (same loop as before), and change `pub fn offsets(&self) -> &[usize] { &self.offsets }`. Update the Task 2 test (`let off = h.offsets();` still works with a slice).

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p nif`
Expected: all PASS (header 6, path 2, props 6, scan 5, fixtures 2; `corpus` ignored).

- [ ] **Step 5: Run the corpus test on the owner's vanilla meshes (read-only)**

Run (after extracting nothing: use a loose mod folder): `$env:SDC_NIF_CORPUS='C:\_mount\moddingvol\Acherus\mods\Assorted mesh fixes\meshes'; cargo test -p nif --release -- --ignored corpus --nocapture`
Expected: PASS. This matches plan 06: 0 problems in 87k files.

- [ ] **Step 6: Commit**

```bash
git add crates/nif
git commit -m "feat(nif): layer-0 scan with shape heads, particle systems, per-shape rows"
```

---

### Task 6: `vfs` crate — loose walk and SSE BSA v105 reader

**Files:**
- Create: `crates/vfs/Cargo.toml` (deps: `thiserror`, `walkdir`, `lz4_flex`, `encoding_rs`, `nif` for path canon), `src/lib.rs`, `src/loose.rs`, `src/bsa.rs`, `tests/bsa.rs`, `tests/loose.rs`

**Interfaces:**
- Consumes: `nif::path::{canon_mesh, is_facegen}`, `nif::text::lossy`.
- Produces:
  - `vfs::VfsError` (`Io(std::io::Error)`, `NotBsa`, `UnsupportedBsaVersion(u32)`, `Corrupt(String)`).
  - `vfs::loose::LooseFile { pub norm: String, pub abs: PathBuf, pub size: u64, pub mtime: i64 }`.
  - `vfs::loose::walk(root: &Path, top: &[&str], ext: &[&str]) -> Vec<LooseFile>`. `top` = `["meshes","textures"]`, `ext` = `["nif"]` or `["dds"]`. The walk skips FaceGen and matches folder names case-insensitively.
  - `vfs::bsa::Bsa { pub path: PathBuf, pub version: u32, pub entries: Vec<BsaEntry> }` with `Bsa::open(&Path) -> Result<Bsa, VfsError>` (reads the name table only) and `Bsa::read(&self, e: &BsaEntry) -> Result<Vec<u8>, VfsError>`.
  - `BsaEntry { pub norm: String, pub offset: u32, pub size: u32, pub compressed: bool }`.

- [ ] **Step 1: Write the failing tests** (a v105 builder lives in the test file; layout per the 01/02 prototype `scratchpad\nifscan_scripts\scan.py:50-112`)

```rust
// crates/vfs/tests/bsa.rs
use std::io::Write;
use vfs::bsa::Bsa;

/// Builds a v105 BSA with the given (folder, file, bytes, compress) entries; flags 0x1|0x2 (+0x4 if all compressed).
fn build(entries: &[(&str, &str, &[u8], bool)], embed_names: bool) -> Vec<u8> {
    let mut folders: Vec<&str> = entries.iter().map(|e| e.0).collect();
    folders.dedup();
    let aflags: u32 = 0x1 | 0x2 | if embed_names { 0x100 } else { 0 };
    let total_folder_len: usize = folders.iter().map(|f| f.len() + 1).sum();
    let total_file_len: usize = entries.iter().map(|e| e.1.len() + 1).sum();
    let header = 36usize;
    let folder_recs = 24 * folders.len();
    let file_rec_block: usize = folders.iter().map(|f| 1 + f.len() + 1).sum::<usize>() + 16 * entries.len();
    let names_start = header + folder_recs + file_rec_block;
    let mut data_off = names_start + total_file_len;
    let mut out = vec![];
    out.extend(b"BSA\0");
    for v in [105u32, 36, aflags, folders.len() as u32, entries.len() as u32, total_folder_len as u32, total_file_len as u32] {
        out.extend(v.to_le_bytes());
    }
    out.extend(2u16.to_le_bytes()); // file flags: meshes
    out.extend(0u16.to_le_bytes());
    let mut file_recs_off = header + folder_recs;
    for f in &folders {
        let count = entries.iter().filter(|e| e.0 == *f).count() as u32;
        out.extend(0u64.to_le_bytes());
        out.extend(count.to_le_bytes());
        out.extend(0u32.to_le_bytes());
        out.extend(((file_recs_off + total_file_len) as u64).to_le_bytes());
        file_recs_off += 1 + f.len() + 1 + 16 * count as usize;
    }
    let mut blobs = vec![];
    for f in &folders {
        out.push((f.len() + 1) as u8);
        out.extend(f.as_bytes());
        out.push(0);
        for (_, name, bytes, comp) in entries.iter().filter(|e| e.0 == *f) {
            let mut blob = vec![];
            if embed_names {
                let full = format!("{f}\\{name}");
                blob.push(full.len() as u8);
                blob.extend(full.as_bytes());
            }
            if *comp {
                blob.extend((bytes.len() as u32).to_le_bytes());
                let mut enc = lz4_flex::frame::FrameEncoder::new(vec![]);
                enc.write_all(bytes).unwrap();
                blob.extend(enc.finish().unwrap());
            } else {
                blob.extend(*bytes);
            }
            let size = blob.len() as u32 | if *comp { 0x4000_0000 } else { 0 };
            out.extend(0u64.to_le_bytes());
            out.extend(size.to_le_bytes());
            out.extend((data_off as u32).to_le_bytes());
            data_off += blob.len();
            blobs.push(blob);
        }
    }
    for e in entries {
        out.extend(e.1.as_bytes());
        out.push(0);
    }
    for b in blobs {
        out.extend(b);
    }
    out
}

#[test]
fn reads_names_and_both_compression_states() {
    let dir = tempfile::tempdir().unwrap();
    for embed in [false, true] {
        let p = dir.path().join(format!("t{embed}.bsa"));
        let bytes = build(&[
            ("meshes\\armor\\iron", "cuirass.nif", b"NIFDATA-1", true),
            ("meshes\\armor\\iron", "boots.nif", b"plain", false),
            ("textures\\armor\\iron", "cuirass_d.dds", b"DDS ", true),
        ], embed);
        std::fs::write(&p, bytes).unwrap();
        let bsa = Bsa::open(&p).unwrap();
        assert_eq!(bsa.version, 105);
        let names: Vec<_> = bsa.entries.iter().map(|e| e.norm.as_str()).collect();
        assert_eq!(names, ["meshes/armor/iron/cuirass.nif", "meshes/armor/iron/boots.nif", "textures/armor/iron/cuirass_d.dds"]);
        assert_eq!(bsa.read(&bsa.entries[0]).unwrap(), b"NIFDATA-1");
        assert_eq!(bsa.read(&bsa.entries[1]).unwrap(), b"plain");
    }
}

#[test]
fn rejects_non_105_and_garbage() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("old.bsa");
    let mut b = build(&[("meshes", "a.nif", b"x", false)], false);
    b[4..8].copy_from_slice(&104u32.to_le_bytes());
    std::fs::write(&p, &b).unwrap();
    assert!(matches!(Bsa::open(&p), Err(vfs::VfsError::UnsupportedBsaVersion(104))));
    std::fs::write(&p, b"BTDX....").unwrap();
    assert!(matches!(Bsa::open(&p), Err(vfs::VfsError::NotBsa)));
    std::fs::write(&p, b"BSA\0\x69\0\0\0").unwrap();
    assert!(Bsa::open(&p).is_err());
}

/// Opt-in: `SDC_BSA=C:\...\Skyrim - Meshes0.bsa cargo test -p vfs -- --ignored real_bsa`
#[test]
#[ignore]
fn real_bsa() {
    let bsa = Bsa::open(std::path::Path::new(&std::env::var("SDC_BSA").unwrap())).unwrap();
    let nif = bsa.entries.iter().find(|e| e.norm.ends_with(".nif")).unwrap();
    let bytes = bsa.read(nif).unwrap();
    assert!(nif::scan::scan(&bytes).is_ok(), "{}", nif.norm);
}
```

```rust
// crates/vfs/tests/loose.rs
#[test]
fn walks_case_insensitively_and_skips_facegen() {
    let d = tempfile::tempdir().unwrap();
    let mk = |p: &str| {
        let f = d.path().join(p);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(f, b"x").unwrap();
    };
    mk("Meshes/Armor/A.NIF");
    mk("meshes/actors/character/FaceGenData/FaceGeom/skyrim.esm/0001.nif");
    mk("meshes/b.txt");
    mk("textures/a_d.dds");
    let got: Vec<_> = vfs::loose::walk(d.path(), &["meshes"], &["nif"]).into_iter().map(|f| f.norm).collect();
    assert_eq!(got, ["meshes/armor/a.nif"]);
}
```

Dev-deps: `tempfile.workspace = true`, `lz4_flex.workspace = true`, `nif = { path = "../nif" }`.

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p vfs`
Expected: compile error.

- [ ] **Step 3: Implement**

```rust
// crates/vfs/src/lib.rs
//! Virtual file sources for the NIF scan: loose folders and SSE BSA v105 archives. FaceGen is skipped here.
pub mod bsa;
pub mod loose;

#[derive(Debug, thiserror::Error)]
pub enum VfsError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("not a BSA archive")]
    NotBsa,
    #[error("BSA version {0} is not Skyrim SE (105)")]
    UnsupportedBsaVersion(u32),
    #[error("corrupt archive: {0}")]
    Corrupt(String),
}
```

```rust
// crates/vfs/src/loose.rs
use nif::path::{canon_mesh, is_facegen};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq)]
pub struct LooseFile {
    pub norm: String,
    pub abs: PathBuf,
    pub size: u64,
    /// Seconds since the Unix epoch.
    pub mtime: i64,
}

/// Walks `root/<top>/**` for files with one of `ext` (lowercase, no dot). Case-insensitive on folder names.
pub fn walk(root: &Path, top: &[&str], ext: &[&str]) -> Vec<LooseFile> {
    let Ok(rd) = std::fs::read_dir(root) else { return vec![] };
    let mut out = vec![];
    for d in rd.filter_map(Result::ok) {
        let name = d.file_name().to_string_lossy().to_ascii_lowercase();
        if !top.contains(&name.as_str()) || !d.path().is_dir() {
            continue;
        }
        let it = walkdir::WalkDir::new(d.path()).into_iter().filter_entry(|e| {
            !(e.file_type().is_dir() && e.file_name().eq_ignore_ascii_case("facegendata"))
        });
        for e in it.filter_map(Result::ok).filter(|e| e.file_type().is_file()) {
            let p = e.path();
            let ok = p.extension().map(|x| x.to_string_lossy().to_ascii_lowercase()).is_some_and(|x| ext.contains(&x.as_str()));
            if !ok {
                continue;
            }
            let rel = p.strip_prefix(root).unwrap().to_string_lossy();
            let norm = canon_mesh(&rel);
            if is_facegen(&norm) {
                continue;
            }
            let md = match e.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };
            let mtime = md
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_secs() as i64);
            out.push(LooseFile { norm, abs: p.to_path_buf(), size: md.len(), mtime });
        }
    }
    out.sort_by(|a, b| a.norm.cmp(&b.norm));
    out
}
```

```rust
// crates/vfs/src/bsa.rs
//! Skyrim SE BSA (version 105, LZ4 frame). Reads the folder/file records and the name table on open;
//! entry bytes on demand. Layout: header 36 B, folder records 24 B (v105), per-folder bzstring name + 16 B file
//! records, NUL-separated file names, data (optional embedded bstring full path; compressed = u32 size + LZ4 frame).
use crate::VfsError;
use nif::path::canon_mesh;
use nif::text::lossy;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

const INCLUDE_DIR_NAMES: u32 = 0x1;
const INCLUDE_FILE_NAMES: u32 = 0x2;
const COMPRESSED_DEFAULT: u32 = 0x4;
const EMBED_NAMES: u32 = 0x100;

#[derive(Debug, Clone, PartialEq)]
pub struct BsaEntry {
    pub norm: String,
    pub offset: u32,
    pub size: u32,
    pub compressed: bool,
}

pub struct Bsa {
    pub path: PathBuf,
    pub version: u32,
    pub flags: u32,
    pub entries: Vec<BsaEntry>,
}

fn rd<const N: usize>(f: &mut impl Read) -> Result<[u8; N], VfsError> {
    let mut b = [0u8; N];
    f.read_exact(&mut b)?;
    Ok(b)
}
fn u32le(f: &mut impl Read) -> Result<u32, VfsError> {
    Ok(u32::from_le_bytes(rd::<4>(f)?))
}

impl Bsa {
    pub fn open(path: &Path) -> Result<Bsa, VfsError> {
        let mut f = std::io::BufReader::new(std::fs::File::open(path)?);
        let magic = rd::<4>(&mut f).map_err(|_| VfsError::NotBsa)?;
        if &magic != b"BSA\0" {
            return Err(VfsError::NotBsa);
        }
        let version = u32le(&mut f)?;
        if version != 105 {
            return Err(VfsError::UnsupportedBsaVersion(version));
        }
        let folder_off = u32le(&mut f)?;
        let flags = u32le(&mut f)?;
        let nfolders = u32le(&mut f)? as usize;
        let nfiles = u32le(&mut f)? as usize;
        let _folder_len = u32le(&mut f)?;
        let file_len = u32le(&mut f)? as usize;
        rd::<4>(&mut f)?; // file flags + padding
        if folder_off != 36 || flags & (INCLUDE_DIR_NAMES | INCLUDE_FILE_NAMES) != (INCLUDE_DIR_NAMES | INCLUDE_FILE_NAMES) {
            return Err(VfsError::Corrupt("archive without directory or file names".into()));
        }
        if nfiles > 4_000_000 || nfolders > nfiles.max(1) * 2 {
            return Err(VfsError::Corrupt("implausible counts".into()));
        }
        let mut counts = Vec::with_capacity(nfolders);
        for _ in 0..nfolders {
            let r = rd::<24>(&mut f)?;
            counts.push(u32::from_le_bytes(r[8..12].try_into().unwrap()) as usize);
        }
        let mut recs = Vec::with_capacity(nfiles);
        for &count in &counts {
            let l = rd::<1>(&mut f)?[0] as usize;
            let mut name = vec![0u8; l];
            f.read_exact(&mut name)?;
            let folder = lossy(name.strip_suffix(&[0]).unwrap_or(&name));
            for _ in 0..count {
                let r = rd::<16>(&mut f)?;
                let size = u32::from_le_bytes(r[8..12].try_into().unwrap());
                let offset = u32::from_le_bytes(r[12..16].try_into().unwrap());
                let compressed = (flags & COMPRESSED_DEFAULT != 0) ^ (size & 0x4000_0000 != 0);
                recs.push((folder.clone(), size & 0x3FFF_FFFF, offset, compressed));
            }
        }
        if recs.len() != nfiles {
            return Err(VfsError::Corrupt("file count mismatch".into()));
        }
        let mut names = vec![0u8; file_len];
        f.read_exact(&mut names)?;
        let names: Vec<&[u8]> = names.split(|&c| c == 0).collect();
        let entries = recs
            .into_iter()
            .enumerate()
            .map(|(i, (folder, size, offset, compressed))| {
                let file = names.get(i).map(|n| lossy(n)).unwrap_or_default();
                BsaEntry { norm: canon_mesh(&format!("{folder}\\{file}")), offset, size, compressed }
            })
            .collect();
        Ok(Bsa { path: path.to_path_buf(), version, flags, entries })
    }

    pub fn read(&self, e: &BsaEntry) -> Result<Vec<u8>, VfsError> {
        let mut f = std::fs::File::open(&self.path)?;
        f.seek(SeekFrom::Start(e.offset as u64))?;
        let mut data = vec![0u8; e.size as usize];
        f.read_exact(&mut data)?;
        let mut p = 0usize;
        if self.flags & EMBED_NAMES != 0 {
            p += 1 + *data.first().ok_or_else(|| VfsError::Corrupt("empty entry".into()))? as usize;
        }
        if !e.compressed {
            return data.get(p..).map(<[u8]>::to_vec).ok_or_else(|| VfsError::Corrupt("entry too short".into()));
        }
        let orig = data.get(p..p + 4).ok_or_else(|| VfsError::Corrupt("entry too short".into()))?;
        let orig = u32::from_le_bytes(orig.try_into().unwrap()) as usize;
        let mut out = Vec::with_capacity(orig);
        lz4_flex::frame::FrameDecoder::new(&data[p + 4..]).read_to_end(&mut out).map_err(|x| VfsError::Corrupt(x.to_string()))?;
        if out.len() != orig {
            return Err(VfsError::Corrupt(format!("{}: {} bytes, header says {orig}", e.norm, out.len())));
        }
        Ok(out)
    }
}
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p vfs`
Expected: 3 PASS (1 ignored).
Then: `$env:SDC_BSA='C:\_mount\moddingvol\Acherus\Stock Game\Data\Skyrim - Meshes0.bsa'; cargo test -p vfs --release -- --ignored real_bsa` → PASS. The executor confirms the Stock Game path from `ModOrganizer.ini` `gamePath` first.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock crates/vfs
git commit -m "feat(vfs): loose walk and SSE BSA v105 reader"
```

---

### Task 7: MO2 instance, profile and origin order (`modscan::qtini`, `modscan::mo2`)

**Files:**
- Create: `crates/modscan/Cargo.toml` (deps: `thiserror`, `nif`, `vfs`, `rusqlite`, `rayon`, `serde`, `serde_json`, `crc32fast`; `windows` with features `Win32_System_Diagnostics_ToolHelp`, `Win32_Foundation`, `Win32_Storage_FileSystem`, `Win32_System_Registry`), `src/lib.rs`, `src/qtini.rs`, `src/mo2.rs`, `tests/mo2.rs`

**Interfaces:**
- Produces:
  - `modscan::ScanError` (enum: `Io`, `NoInstance(String)`, `Ini(String)`, `PgpRunning`, `Db(rusqlite::Error)`).
  - `qtini::Ini::parse(&str) -> Ini` and `Ini::get(&self, section: &str, key: &str) -> Option<String>`. Values are unescaped: `\\` → `\`, `@ByteArray(x)` → `x`, surrounding quotes removed.
  - `mo2::Instance { pub root: PathBuf, pub game_data: PathBuf, pub mods: PathBuf, pub overwrite: PathBuf, pub profiles: PathBuf, pub profile: String }`. `Instance::load(root: &Path, profile: Option<&str>) -> Result<Instance, ScanError>` resolves `%BASE_DIR%` and relative paths against `base_directory` (or `root`).
  - `mo2::OriginKind { GameData, Mod, Overwrite }`.
  - `mo2::LooseOrigin { pub kind: OriginKind, pub name: String, pub root: PathBuf, pub priority: i32 }`. Higher priority wins: GameData = −1, mods 0..n (bottom of `modlist.txt` = 0), Overwrite = i32::MAX.
  - `mo2::loose_origins(&Instance) -> Result<(Vec<LooseOrigin>, Vec<String> /*missing mod folders*/), ScanError>`.
  - `mo2::detect_launch() -> Option<PathBuf>`: when `usvfs_x64.dll` is loaded in this process or the parent is `ModOrganizer.exe`, returns the MO2 exe folder; otherwise `None`.
  - `mo2::find_instance(exe_dir: &Path) -> Option<PathBuf>`: `portable.txt` next to the exe → the exe folder; else the registry `HKCU\Software\Mod Organizer Team\Mod Organizer\CurrentInstance` → `%LOCALAPPDATA%\ModOrganizer\<name>`.

- [ ] **Step 1: Write the failing tests**

```rust
// crates/modscan/tests/mo2.rs
use modscan::mo2::{loose_origins, Instance, OriginKind};
use modscan::qtini::Ini;
use std::fs;

#[test]
fn ini_placeholders_and_bytearray() {
    let ini = Ini::parse(
        "[General]\ngamePath=@ByteArray(C:\\\\Games\\\\Skyrim Special Edition)\nselected_profile=@ByteArray(Nexus)\n\
         [Settings]\nbase_directory=D:/MO2\nmod_directory=%BASE_DIR%/mods\noverwrite_directory=\"%BASE_DIR%/overwrite\"\n",
    );
    assert_eq!(ini.get("General", "gamePath").unwrap(), "C:\\Games\\Skyrim Special Edition");
    assert_eq!(ini.get("General", "selected_profile").unwrap(), "Nexus");
    assert_eq!(ini.get("Settings", "mod_directory").unwrap(), "%BASE_DIR%/mods");
    assert_eq!(ini.get("Settings", "overwrite_directory").unwrap(), "%BASE_DIR%/overwrite");
    assert_eq!(ini.get("Settings", "nope"), None);
}

fn instance_fixture(modlist: &str) -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    let r = d.path();
    let game = r.join("Game");
    fs::create_dir_all(game.join("Data")).unwrap();
    fs::write(
        r.join("ModOrganizer.ini"),
        format!("[General]\ngamePath=@ByteArray({})\nselected_profile=@ByteArray(Default)\n", game.display().to_string().replace('\\', "\\\\")),
    )
    .unwrap();
    for m in ["A", "B", "C", "Sep_separator"] {
        fs::create_dir_all(r.join("mods").join(m)).unwrap();
    }
    fs::create_dir_all(r.join("overwrite")).unwrap();
    fs::create_dir_all(r.join("profiles/Default")).unwrap();
    fs::write(r.join("profiles/Default/modlist.txt"), modlist).unwrap();
    d
}

#[test]
fn modlist_first_line_is_highest_priority() {
    let d = instance_fixture("# comment\n+C\n-B\n+Sep_separator\n+A\n*DLC: Dawnguard\n");
    let inst = Instance::load(d.path(), None).unwrap();
    assert_eq!(inst.profile, "Default");
    let (o, missing) = loose_origins(&inst).unwrap();
    assert!(missing.is_empty());
    let order: Vec<_> = o.iter().map(|x| (x.kind, x.name.as_str())).collect();
    assert_eq!(order, [(OriginKind::GameData, "Data"), (OriginKind::Mod, "A"), (OriginKind::Mod, "C"), (OriginKind::Overwrite, "overwrite")]);
    assert!(o[1].priority < o[2].priority);
}

#[test]
fn missing_and_unlisted_mods() {
    let d = instance_fixture("+Ghost\n+A\n");
    let inst = Instance::load(d.path(), None).unwrap();
    let (o, missing) = loose_origins(&inst).unwrap();
    assert_eq!(missing, ["Ghost"]);
    assert!(o.iter().all(|x| x.name != "B" && x.name != "C"));
}

#[test]
fn bad_profile_is_an_error() {
    let d = instance_fixture("+A\n");
    assert!(Instance::load(d.path(), Some("Nope")).is_err());
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p modscan --test mo2`
Expected: compile error.

- [ ] **Step 3: Implement**

```rust
// crates/modscan/src/qtini.rs
//! Minimal reader for Qt QSettings INI files as MO2 writes them (plan 02 §1.3).
use std::collections::HashMap;

pub struct Ini(HashMap<(String, String), String>);

fn unquote(v: &str) -> String {
    let v = v.trim();
    let v = v.strip_prefix('"').and_then(|x| x.strip_suffix('"')).unwrap_or(v);
    let v = v.strip_prefix("@ByteArray(").and_then(|x| x.strip_suffix(')')).unwrap_or(v);
    v.replace("\\\\", "\\")
}

impl Ini {
    pub fn parse(text: &str) -> Ini {
        let mut map = HashMap::new();
        let mut section = String::from("General");
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
                continue;
            }
            if let Some(s) = line.strip_prefix('[').and_then(|x| x.strip_suffix(']')) {
                section = s.to_string();
            } else if let Some((k, v)) = line.split_once('=') {
                map.insert((section.to_ascii_lowercase(), k.trim().to_ascii_lowercase()), unquote(v));
            }
        }
        Ini(map)
    }
    pub fn get(&self, section: &str, key: &str) -> Option<String> {
        self.0.get(&(section.to_ascii_lowercase(), key.to_ascii_lowercase())).cloned()
    }
}
```

```rust
// crates/modscan/src/mo2.rs
//! MO2 instance discovery and the loose-file origin order (plan 02 §1–§2).
use crate::qtini::Ini;
use crate::ScanError;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Instance {
    pub root: PathBuf,
    pub game_data: PathBuf,
    pub mods: PathBuf,
    pub overwrite: PathBuf,
    pub profiles: PathBuf,
    pub profile: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OriginKind {
    GameData,
    Mod,
    Overwrite,
}

#[derive(Debug, Clone)]
pub struct LooseOrigin {
    pub kind: OriginKind,
    pub name: String,
    pub root: PathBuf,
    pub priority: i32,
}

fn resolve(base: &Path, v: Option<String>, default: &str) -> PathBuf {
    let v = v.unwrap_or_else(|| format!("%BASE_DIR%/{default}"));
    let v = v.replace("%BASE_DIR%", &base.to_string_lossy());
    let p = PathBuf::from(v);
    if p.is_absolute() { p } else { base.join(p) }
}

impl Instance {
    pub fn load(root: &Path, profile: Option<&str>) -> Result<Instance, ScanError> {
        let text = std::fs::read_to_string(root.join("ModOrganizer.ini"))
            .map_err(|e| ScanError::NoInstance(format!("{}: {e}", root.display())))?;
        let ini = Ini::parse(&text);
        let base = ini.get("Settings", "base_directory").map(PathBuf::from).unwrap_or_else(|| root.to_path_buf());
        let game = ini.get("General", "gamePath").ok_or_else(|| ScanError::Ini("gamePath missing".into()))?;
        let profiles = resolve(&base, ini.get("Settings", "profiles_directory"), "profiles");
        let profile = match profile {
            Some(p) => p.to_string(),
            None => ini.get("General", "selected_profile").ok_or_else(|| ScanError::Ini("selected_profile missing".into()))?,
        };
        if !profiles.join(&profile).join("modlist.txt").is_file() {
            return Err(ScanError::NoInstance(format!("profile '{profile}' has no modlist.txt")));
        }
        Ok(Instance {
            root: root.to_path_buf(),
            game_data: PathBuf::from(game).join("Data"),
            mods: resolve(&base, ini.get("Settings", "mod_directory"), "mods"),
            overwrite: resolve(&base, ini.get("Settings", "overwrite_directory"), "overwrite"),
            profiles,
            profile,
        })
    }
}

/// Origins lowest → highest priority. modlist.txt is stored highest-first (plan 02 §2.1).
pub fn loose_origins(inst: &Instance) -> Result<(Vec<LooseOrigin>, Vec<String>), ScanError> {
    let text = std::fs::read_to_string(inst.profiles.join(&inst.profile).join("modlist.txt")).map_err(ScanError::Io)?;
    let mut enabled = vec![];
    let mut missing = vec![];
    for line in text.lines() {
        let Some(name) = line.strip_prefix('+') else { continue }; // '-' disabled, '*' unmanaged, '#' comment
        let name = name.trim();
        if name.ends_with("_separator") {
            continue;
        }
        let root = inst.mods.join(name);
        if root.is_dir() {
            enabled.push((name.to_string(), root));
        } else {
            missing.push(name.to_string());
        }
    }
    let mut out = vec![LooseOrigin { kind: OriginKind::GameData, name: "Data".into(), root: inst.game_data.clone(), priority: -1 }];
    for (i, (name, root)) in enabled.into_iter().rev().enumerate() {
        out.push(LooseOrigin { kind: OriginKind::Mod, name, root, priority: i as i32 });
    }
    out.push(LooseOrigin { kind: OriginKind::Overwrite, name: "overwrite".into(), root: inst.overwrite.clone(), priority: i32::MAX });
    Ok((out, missing))
}

/// MO2 launch detection (plan 02 §1.2): usvfs loaded in this process, or parent process is ModOrganizer.exe.
#[cfg(windows)]
pub fn detect_launch() -> Option<PathBuf> {
    crate::win::mo2_exe_dir()
}

/// portable.txt next to ModOrganizer.exe → that folder; else the registry CurrentInstance (plan 02 §1.2).
#[cfg(windows)]
pub fn find_instance(exe_dir: &Path) -> Option<PathBuf> {
    if exe_dir.join("portable.txt").is_file() {
        return Some(exe_dir.to_path_buf());
    }
    let name = crate::win::current_instance_name()?;
    let local = std::env::var_os("LOCALAPPDATA")?;
    Some(PathBuf::from(local).join("ModOrganizer").join(name))
}
```

```rust
// crates/modscan/src/lib.rs
pub mod archives;
pub mod mo2;
pub mod pgp;
pub mod qtini;
pub mod resolve;
pub mod scan;
pub mod store;
#[cfg(windows)]
pub mod win;

#[derive(Debug, thiserror::Error)]
pub enum ScanError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("no MO2 instance: {0}")]
    NoInstance(String),
    #[error("MO2 settings: {0}")]
    Ini(String),
    #[error("PGPatcher is running")]
    PgpRunning,
    #[error(transparent)]
    Db(#[from] rusqlite::Error),
}
```

```rust
// crates/modscan/src/win.rs
//! Win32 helpers: process snapshot, loaded modules, registry, file identity.
use std::path::PathBuf;
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::Diagnostics::ToolHelp::*;

fn exe_name(e: &PROCESSENTRY32W) -> String {
    let n = e.szExeFile.iter().position(|&c| c == 0).unwrap_or(e.szExeFile.len());
    String::from_utf16_lossy(&e.szExeFile[..n])
}

/// All running process names, lowercase.
pub fn process_names() -> Vec<String> {
    let mut out = vec![];
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return out };
        let mut e = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        if Process32FirstW(snap, &mut e).is_ok() {
            loop {
                out.push(exe_name(&e).to_ascii_lowercase());
                if Process32NextW(snap, &mut e).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
    }
    out
}

/// Folder of ModOrganizer.exe if this process was started by MO2.
pub fn mo2_exe_dir() -> Option<PathBuf> {
    unsafe {
        let me = std::process::id();
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).ok()?;
        let mut e = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut parent = None;
        if Process32FirstW(snap, &mut e).is_ok() {
            loop {
                if e.th32ProcessID == me {
                    parent = Some(e.th32ParentProcessID);
                }
                if Process32NextW(snap, &mut e).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
        let parent = parent?;
        let msnap = CreateToolhelp32Snapshot(TH32CS_SNAPMODULE, parent).ok()?;
        let mut m = MODULEENTRY32W { dwSize: std::mem::size_of::<MODULEENTRY32W>() as u32, ..Default::default() };
        let mut found = None;
        if Module32FirstW(msnap, &mut m).is_ok() {
            let n = m.szExePath.iter().position(|&c| c == 0).unwrap_or(0);
            let p = PathBuf::from(String::from_utf16_lossy(&m.szExePath[..n]));
            if p.file_name().is_some_and(|f| f.eq_ignore_ascii_case("ModOrganizer.exe")) {
                found = p.parent().map(PathBuf::from);
            }
        }
        let _ = CloseHandle(msnap);
        found
    }
}

/// HKCU\Software\Mod Organizer Team\Mod Organizer\CurrentInstance.
pub fn current_instance_name() -> Option<String> {
    use windows::core::w;
    use windows::Win32::System::Registry::*;
    unsafe {
        let mut buf = [0u16; 260];
        let mut len = (buf.len() * 2) as u32;
        let r = RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Mod Organizer Team\\Mod Organizer"),
            w!("CurrentInstance"),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut len),
        );
        if r.is_err() {
            return None;
        }
        let n = buf.iter().position(|&c| c == 0).unwrap_or(0);
        Some(String::from_utf16_lossy(&buf[..n])).filter(|s| !s.is_empty())
    }
}
```

Create stub modules `archives.rs`, `pgp.rs`, `resolve.rs`, `scan.rs`, `store.rs` (`//! filled by Task N`) so the crate compiles. The usvfs-module check is folded into `mo2_exe_dir` via the parent check. 02 §1.2 ranks the usvfs DLL first. The executor adds `GetModuleHandleW(w!("usvfs_x64.dll")).is_ok()` as an early `Some` path that then still needs the parent exe for the folder. If the parent is not MO2 (for example a launcher in between), return `None` and let the GUI ask.

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p modscan --test mo2`
Expected: 4 PASS.

- [ ] **Step 5: Manual read-only check on the owner's instance**

Add a temporary test with `#[ignore]` named `owner_instance`. It loads `C:\_mount\moddingvol\Acherus` and asserts:
- 1,542 enabled mods (count from plan 05 §3.1);
- `ParallaxGen_Output` at rank 37 from the top.

Run it with `--ignored`. Delete the test before committing if the owner does not want machine-specific tests in the repo; otherwise keep it ignored.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock crates/modscan
git commit -m "feat(modscan): MO2 instance/profile load, modlist priority, launch detection"
```

---

### Task 8: Archive ranking and winner resolution (game view)

**Files:**
- Create: `crates/modscan/src/archives.rs`, `crates/modscan/src/resolve.rs`, `crates/modscan/tests/resolve.rs`

**Interfaces:**
- Consumes: `LooseOrigin`, `Instance`.
- Produces:
  - `archives::ActiveBsa { pub path: PathBuf, pub providing_mod: Option<String>, pub rank: i32 }`.
  - `archives::active_bsas(inst: &Instance, loose: &[LooseOrigin]) -> Vec<ActiveBsa>`, ordered lowest → highest rank:
    1. INI archive lists (`sResourceArchiveList` + `sResourceArchiveList2` from the profile `skyrim.ini`/`skyrimcustom.ini` if `LocalSettings=true` in the profile `settings.ini`, else `Documents\My Games\Skyrim Special Edition\`) rank first, in list order.
    2. Then for each active plugin in `plugins.txt` order (`*` prefix = active; implicit masters `Skyrim.esm, Update.esm, Dawnguard.esm, HearthFires.esm, Dragonborn.esm` first): `<stem>.bsa`, then `<stem> - Textures.bsa`, each looked up in the highest-priority loose origin that has it. [Claim tag 00 §3.2]
  - `resolve::Copy { pub path: String, pub origin: OriginRef, pub size: u64, pub mtime: i64 }`, where `OriginRef { Loose(usize /* index into loose origins */), Bsa(usize /* index into active bsas */) }`.
  - `resolve::winners(copies: &[Copy], loose: &[LooseOrigin], bsas: &[ActiveBsa], exclude: &dyn Fn(OriginRef) -> bool) -> HashMap<String, (usize /*copy idx*/, u32 /*n copies*/)>`. Rule: any loose copy beats any BSA copy; among loose copies higher `priority` wins; among BSA copies higher `rank` wins. `exclude` drops origins, which the source view uses.

- [ ] **Step 1: Write the failing tests**

```rust
// crates/modscan/tests/resolve.rs
use modscan::archives::ActiveBsa;
use modscan::mo2::{LooseOrigin, OriginKind};
use modscan::resolve::{winners, Copy, OriginRef};
use std::path::PathBuf;

fn lo(name: &str, prio: i32) -> LooseOrigin {
    LooseOrigin { kind: OriginKind::Mod, name: name.into(), root: PathBuf::from(name), priority: prio }
}
fn c(path: &str, o: OriginRef) -> Copy {
    Copy { path: path.into(), origin: o, size: 1, mtime: 0 }
}

#[test]
fn loose_beats_bsa_and_priority_orders_loose() {
    let loose = vec![lo("Data", -1), lo("A", 0), lo("B", 1), lo("overwrite", i32::MAX)];
    let bsas = vec![
        ActiveBsa { path: "Skyrim - Meshes0.bsa".into(), providing_mod: None, rank: 0 },
        ActiveBsa { path: "B.bsa".into(), providing_mod: Some("B".into()), rank: 10 },
    ];
    let copies = vec![
        c("meshes/x.nif", OriginRef::Bsa(1)),
        c("meshes/x.nif", OriginRef::Loose(1)),
        c("meshes/y.nif", OriginRef::Bsa(0)),
        c("meshes/y.nif", OriginRef::Bsa(1)),
        c("meshes/z.nif", OriginRef::Loose(1)),
        c("meshes/z.nif", OriginRef::Loose(2)),
        c("meshes/z.nif", OriginRef::Loose(3)),
    ];
    let w = winners(&copies, &loose, &bsas, &|_| false);
    assert_eq!(w["meshes/x.nif"], (1, 2));
    assert_eq!(w["meshes/y.nif"], (3, 2));
    assert_eq!(w["meshes/z.nif"], (6, 3));
}

#[test]
fn exclude_gives_source_view() {
    let loose = vec![lo("A", 0), lo("PGP", 5), lo("C", 9)];
    let copies = vec![c("meshes/x.nif", OriginRef::Loose(0)), c("meshes/x.nif", OriginRef::Loose(1))];
    let game = winners(&copies, &loose, &[], &|_| false);
    let source = winners(&copies, &loose, &[], &|o| o == OriginRef::Loose(1));
    assert_eq!(game["meshes/x.nif"].0, 1);
    assert_eq!(source["meshes/x.nif"].0, 0);
}
```

Add one archives test using a tempdir instance from Task 7's fixture shape:
- `plugins.txt`: `*Foo.esp`, `Bar.esp`, `*Baz.esp`;
- BSAs: `mods/A/Foo.bsa`, `mods/A/Bar.bsa`, `mods/B/Baz - Textures.bsa`;
- expected `active_bsas` order: `[Foo.bsa, Baz - Textures.bsa]`; Bar is inactive.

Name it `plugin_bsas_follow_load_order`.

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p modscan --test resolve`
Expected: compile error.

- [ ] **Step 3: Implement**

```rust
// crates/modscan/src/resolve.rs
//! Winner per virtual path: loose beats BSA, then MO2 priority / archive rank (plan 02 §2, 05 §5.2).
use crate::archives::ActiveBsa;
use crate::mo2::LooseOrigin;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OriginRef {
    Loose(usize),
    Bsa(usize),
}

#[derive(Debug, Clone)]
pub struct Copy {
    pub path: String,
    pub origin: OriginRef,
    pub size: u64,
    pub mtime: i64,
}

fn key(o: OriginRef, loose: &[LooseOrigin], bsas: &[ActiveBsa]) -> (u8, i64) {
    match o {
        OriginRef::Loose(i) => (1, loose[i].priority as i64),
        OriginRef::Bsa(i) => (0, bsas[i].rank as i64),
    }
}

pub fn winners(
    copies: &[Copy],
    loose: &[LooseOrigin],
    bsas: &[ActiveBsa],
    exclude: &dyn Fn(OriginRef) -> bool,
) -> HashMap<String, (usize, u32)> {
    let mut best: HashMap<String, (usize, u32)> = HashMap::new();
    for (i, c) in copies.iter().enumerate() {
        if exclude(c.origin) {
            continue;
        }
        let e = best.entry(c.path.clone()).or_insert((i, 0));
        e.1 += 1;
        if key(c.origin, loose, bsas) > key(copies[e.0].origin, loose, bsas) {
            e.0 = i;
        }
    }
    best
}
```

```rust
// crates/modscan/src/archives.rs
//! Active BSAs in game order (plan 02 §3; engine order tagged [Claim] in 00-decision §3.2).
use crate::mo2::{Instance, LooseOrigin};
use crate::qtini::Ini;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct ActiveBsa {
    pub path: PathBuf,
    pub providing_mod: Option<String>,
    pub rank: i32,
}

const IMPLICIT: [&str; 5] = ["skyrim.esm", "update.esm", "dawnguard.esm", "hearthfires.esm", "dragonborn.esm"];

fn find(loose: &[LooseOrigin], file: &str) -> Option<(PathBuf, String)> {
    let mut best: Option<&LooseOrigin> = None;
    for o in loose {
        if o.root.join(file).is_file() && best.is_none_or(|b| o.priority > b.priority) {
            best = Some(o);
        }
    }
    best.map(|o| (o.root.join(file), o.name.clone()))
}

fn ini_lists(inst: &Instance) -> Vec<String> {
    let prof = inst.profiles.join(&inst.profile);
    let local = std::fs::read_to_string(prof.join("settings.ini"))
        .map(|t| Ini::parse(&t).get("General", "LocalSettings").as_deref() == Some("true"))
        .unwrap_or(false);
    let dir = if local {
        prof
    } else {
        let docs = std::env::var_os("USERPROFILE").map(PathBuf::from).unwrap_or_default();
        docs.join("Documents").join("My Games").join("Skyrim Special Edition")
    };
    let mut out = vec![];
    for key in ["sResourceArchiveList", "sResourceArchiveList2"] {
        let mut val = None;
        for f in ["skyrim.ini", "skyrimcustom.ini"] {
            if let Ok(t) = std::fs::read_to_string(dir.join(f)) {
                if let Some(v) = Ini::parse(&t).get("Archive", key) {
                    val = Some(v);
                }
            }
        }
        out.extend(val.unwrap_or_default().split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()));
    }
    out
}

pub fn active_bsas(inst: &Instance, loose: &[LooseOrigin]) -> Vec<ActiveBsa> {
    let mut out: Vec<ActiveBsa> = vec![];
    let mut push = |file: &str, out: &mut Vec<ActiveBsa>| {
        if out.iter().any(|b| b.path.file_name().is_some_and(|n| n.eq_ignore_ascii_case(file))) {
            return;
        }
        if let Some((path, m)) = find(loose, file) {
            let rank = out.len() as i32;
            out.push(ActiveBsa { path, providing_mod: Some(m).filter(|m| m != "Data"), rank });
        }
    };
    for f in ini_lists(inst) {
        push(&f, &mut out);
    }
    let plugins = std::fs::read_to_string(inst.profiles.join(&inst.profile).join("plugins.txt")).unwrap_or_default();
    let active = IMPLICIT.iter().map(|s| s.to_string()).chain(
        plugins.lines().filter_map(|l| l.strip_prefix('*')).map(|s| s.trim().to_ascii_lowercase()),
    );
    for p in active {
        let stem = p.rsplit_once('.').map_or(p.as_str(), |x| x.0).to_string();
        push(&format!("{stem}.bsa"), &mut out);
        push(&format!("{stem} - textures.bsa"), &mut out);
    }
    out
}
```

`find` is case-sensitive on `join(file)`. NTFS is case-insensitive, so `is_file()` works for any case. `push` compares file names case-insensitively.

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p modscan --test resolve`
Expected: 3 PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/modscan
git commit -m "feat(modscan): active BSA ranking and winner resolution with exclusion"
```

---

### Task 9: PGPatcher failsafe (`modscan::pgp`)

**Files:**
- Create: `crates/modscan/src/pgp.rs`, `crates/modscan/tests/pgp.rs`

**Interfaces:**
- Consumes: `LooseOrigin`, `win::process_names`.
- Produces:
  - `pgp::running(names: &[String]) -> bool`: true for `pgpatcher.exe` or `pgtools.exe`.
  - `pgp::Signature { None, Incomplete, Medium, Strong }` and `pgp::signature(root: &Path) -> Signature`. Rules (05 §5.1):
    - `Strong`: `ParallaxGen_Diff.json` and (`PGPatcher.esp` or `PG_<digits>.esp`).
    - `Medium`: any one of those, or `meshes/_pgpatcher_dups/`.
    - `Incomplete`: `meshes/` present, no diff JSON, and (any ESP or a file starting `PGPatcher_UpdateCache.bin`).
  - `pgp::Diff { pub crc_orig: HashMap<String /*canonical path*/, u32>, pub crc_patched: HashMap<String, u32> }` and `pgp::read_diff(root: &Path) -> Option<Diff>`. Keys are `canon_mesh` of the JSON key.
  - `pgp::dup_source(norm: &str) -> Option<String>`: `meshes/_pgpatcher_dups/<n>/rest` → `meshes/rest`.
  - `pgp::DeriveCheck { Match, StaleChanged, StaleReordered, Unverified, UnknownSource, DupOf }`.
  - `pgp::check(diff_crc: Option<u32>, source_crc: Option<u32>, deeper_crcs: &[u32]) -> DeriveCheck`.
  - `pgp::is_derived_path(norm: &str, diff: Option<&Diff>) -> bool`: for overwrite and Data, only diff keys and dup paths are derived.

- [ ] **Step 1: Write the failing tests** (05 §5.4 cases 1–5, 7, 10, 11, 15)

```rust
// crates/modscan/tests/pgp.rs
use modscan::pgp::*;
use std::fs;

fn mk(root: &std::path::Path, rel: &str, bytes: &[u8]) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, bytes).unwrap();
}

#[test]
fn process_gate() {
    assert!(running(&["explorer.exe".into(), "pgpatcher.exe".into()]));
    assert!(running(&["pgtools.exe".into()]));
    assert!(!running(&["parallaxgen.exe.bak".into()]));
}

#[test]
fn signatures() {
    let d = tempfile::tempdir().unwrap();
    let r = d.path();
    assert_eq!(signature(r), Signature::None);
    mk(r, "meshes/x.nif", b"n");
    mk(r, "PGPatcher.esp", b"e");
    assert_eq!(signature(r), Signature::Incomplete);
    mk(r, "ParallaxGen_Diff.json", b"{}");
    assert_eq!(signature(r), Signature::Strong);
    let d2 = tempfile::tempdir().unwrap();
    mk(d2.path(), "meshes/_pgpatcher_dups/1/armor/x.nif", b"n");
    assert_eq!(signature(d2.path()), Signature::Medium);
    let d3 = tempfile::tempdir().unwrap();
    mk(d3.path(), "PG_12.esp", b"e");
    mk(d3.path(), "ParallaxGen_Diff.json", b"{}");
    assert_eq!(signature(d3.path()), Signature::Strong);
}

#[test]
fn diff_json_keys_are_canonical() {
    let d = tempfile::tempdir().unwrap();
    mk(d.path(), "ParallaxGen_Diff.json",
       br#"{"meshes\\Armor\\Iron\\Cuirass.nif":{"crc32original":305419896,"crc32patched":1}}"#);
    let diff = read_diff(d.path()).unwrap();
    assert_eq!(diff.crc_orig["meshes/armor/iron/cuirass.nif"], 0x1234_5678);
    assert!(is_derived_path("meshes/armor/iron/cuirass.nif", Some(&diff)));
    assert!(!is_derived_path("meshes/other.nif", Some(&diff)));
    assert!(is_derived_path("meshes/_pgpatcher_dups/3/other.nif", None));
}

#[test]
fn dup_paths_map_to_source() {
    assert_eq!(dup_source("meshes/_pgpatcher_dups/12/armor/x.nif").as_deref(), Some("meshes/armor/x.nif"));
    assert_eq!(dup_source("meshes/armor/x.nif"), None);
}

#[test]
fn staleness_uses_crc_not_mtime() {
    assert_eq!(check(Some(7), Some(7), &[]), DeriveCheck::Match);
    assert_eq!(check(Some(7), Some(8), &[]), DeriveCheck::StaleChanged);
    assert_eq!(check(Some(7), Some(8), &[9, 7]), DeriveCheck::StaleReordered);
    assert_eq!(check(Some(7), None, &[]), DeriveCheck::Unverified);
    assert_eq!(check(None, Some(8), &[]), DeriveCheck::UnknownSource);
}
```

The orchestrator owns the mid-scan taint test (`pgp_started_mid_scan_taints_result`). It lives in Task 10.

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p modscan --test pgp`
Expected: compile error.

- [ ] **Step 3: Implement**

```rust
// crates/modscan/src/pgp.rs
//! PGPatcher (ParallaxGen) output as a derived layer (plan 05 §5). Detection by marker files, staleness by CRC32.
use nif::path::canon_mesh;
use std::collections::HashMap;
use std::path::Path;

pub fn running(names: &[String]) -> bool {
    names.iter().any(|n| n == "pgpatcher.exe" || n == "pgtools.exe")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signature {
    None,
    Incomplete,
    Medium,
    Strong,
}

fn is_pg_esp(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n == "pgpatcher.esp"
        || n.strip_prefix("pg_").and_then(|r| r.strip_suffix(".esp")).is_some_and(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit()))
}

pub fn signature(root: &Path) -> Signature {
    let names: Vec<String> = std::fs::read_dir(root)
        .map(|rd| rd.filter_map(Result::ok).map(|e| e.file_name().to_string_lossy().into_owned()).collect())
        .unwrap_or_default();
    let diff = names.iter().any(|n| n.eq_ignore_ascii_case("ParallaxGen_Diff.json"));
    let esp = names.iter().any(|n| is_pg_esp(n));
    let any_esp = names.iter().any(|n| n.to_ascii_lowercase().ends_with(".esp"));
    let cache = names.iter().any(|n| n.to_ascii_lowercase().starts_with("pgpatcher_updatecache.bin"));
    let dups = root.join("meshes").join("_pgpatcher_dups").is_dir();
    let meshes = root.join("meshes").is_dir();
    if diff && esp {
        Signature::Strong
    } else if meshes && !diff && (any_esp || cache) && !dups {
        Signature::Incomplete
    } else if diff || esp || dups {
        Signature::Medium
    } else {
        Signature::None
    }
}

#[derive(Debug, Default)]
pub struct Diff {
    pub crc_orig: HashMap<String, u32>,
    pub crc_patched: HashMap<String, u32>,
}

pub fn read_diff(root: &Path) -> Option<Diff> {
    let text = std::fs::read_to_string(root.join("ParallaxGen_Diff.json")).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    let mut d = Diff::default();
    for (k, e) in v.as_object()? {
        let key = canon_mesh(k);
        if let Some(c) = e.get("crc32original").and_then(|x| x.as_u64()) {
            d.crc_orig.insert(key.clone(), c as u32);
        }
        if let Some(c) = e.get("crc32patched").and_then(|x| x.as_u64()) {
            d.crc_patched.insert(key, c as u32);
        }
    }
    Some(d)
}

pub fn dup_source(norm: &str) -> Option<String> {
    let rest = norm.strip_prefix("meshes/_pgpatcher_dups/")?;
    let (_, tail) = rest.split_once('/')?;
    Some(format!("meshes/{tail}"))
}

pub fn is_derived_path(norm: &str, diff: Option<&Diff>) -> bool {
    dup_source(norm).is_some() || diff.is_some_and(|d| d.crc_orig.contains_key(norm))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeriveCheck {
    Match,
    StaleChanged,
    StaleReordered,
    Unverified,
    UnknownSource,
    DupOf,
}

pub fn check(diff_crc: Option<u32>, source_crc: Option<u32>, deeper_crcs: &[u32]) -> DeriveCheck {
    match (diff_crc, source_crc) {
        (None, _) => DeriveCheck::UnknownSource,
        (Some(_), None) => DeriveCheck::Unverified,
        (Some(d), Some(s)) if d == s => DeriveCheck::Match,
        (Some(d), Some(_)) if deeper_crcs.contains(&d) => DeriveCheck::StaleReordered,
        _ => DeriveCheck::StaleChanged,
    }
}
```

The JSON shape (top-level object keyed by mesh path, with `crc32original`/`crc32patched`) comes from 05 §3.3. Before Step 4, the executor confirms it against the owner's `C:\_mount\moddingvol\Acherus\mods\ParallaxGen_Output\ParallaxGen_Diff.json` (read-only: open it, look at the first entry). If the shape differs, adjust `read_diff` and the test's fixture JSON to the real shape.

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p modscan --test pgp`
Expected: 5 PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/modscan
git commit -m "feat(modscan): PGPatcher process gate, output signatures, diff JSON, CRC staleness"
```

---

### Task 10: SQLite store and scan orchestrator

**Files:**
- Create: `crates/modscan/src/store.rs`, `crates/modscan/src/scan.rs`, `crates/modscan/tests/scan.rs`

**Interfaces:**
- Consumes: everything above.
- Produces:
  - `store::Store::open(path: &Path) -> Result<Store, ScanError>`. Creates the schema of 02 §6 plus the 05 §5.2 columns, with the `nif_copy.le_mesh` and `shape` columns below.
  - `Store` methods:
    - `Store::upsert_copy(&mut self, ...)`;
    - `Store::replace_shapes(copy_id, &NifScan)`;
    - `Store::set_winners(game: &HashMap<..>, source: &HashMap<..>)`;
    - `Store::mods() -> Vec<ModRow>`;
    - `Store::nifs_of(origin_id) -> Vec<NifRow>`;
    - `Store::shapes_of(copy_id) -> Vec<ShapeView>`;
    - `Store::users_of_texture(norm) -> Vec<(String /*nif*/, u32 /*slot*/)>`.
  - `scan::Source { Mo2(Instance), Folder(PathBuf) }`.
  - `scan::Progress { pub phase: &'static str, pub done: usize, pub total: usize }`.
  - `scan::Report { pub nifs: usize, pub parsed: usize, pub reused: usize, pub problems: Vec<(String, String)>, pub not_skyrim: usize, pub le_meshes: usize, pub pgp: Vec<PgpOrigin>, pub tainted: bool, pub missing_mods: Vec<String> }`.
  - `scan::run(src: &Source, store: &mut Store, cancel: &AtomicBool, progress: &dyn Fn(Progress), procs: &dyn Fn() -> Vec<String>) -> Result<Report, ScanError>`. `procs` is injected so tests can fake `PGPatcher.exe`; production passes `win::process_names`.

Schema (in `store.rs`, executed once with `PRAGMA user_version = 1`):

```sql
PRAGMA journal_mode = WAL;
CREATE TABLE IF NOT EXISTS path   (id INTEGER PRIMARY KEY, p TEXT NOT NULL UNIQUE);
CREATE TABLE IF NOT EXISTS origin (id INTEGER PRIMARY KEY, kind INTEGER NOT NULL, name TEXT NOT NULL, root TEXT NOT NULL,
                                   parent_origin INTEGER, priority INTEGER NOT NULL, enabled INTEGER NOT NULL,
                                   derived_by INTEGER, derived_state INTEGER, UNIQUE(kind, root));
CREATE TABLE IF NOT EXISTS nif_copy (id INTEGER PRIMARY KEY, path_id INTEGER NOT NULL, origin_id INTEGER NOT NULL,
                                   size INTEGER, mtime INTEGER, crc32 INTEGER, parse_status INTEGER NOT NULL,
                                   bsver INTEGER, le_mesh INTEGER NOT NULL DEFAULT 0, is_derived INTEGER NOT NULL DEFAULT 0,
                                   source_copy_id INTEGER, derive_check INTEGER, problem TEXT,
                                   UNIQUE(path_id, origin_id));
CREATE TABLE IF NOT EXISTS shape (copy_id INTEGER NOT NULL, block INTEGER NOT NULL, name TEXT, kind TEXT NOT NULL,
                                   shader_kind INTEGER NOT NULL, shader_type INTEGER, slsf1 INTEGER, slsf2 INTEGER,
                                   texset_id INTEGER, alpha_flags INTEGER, alpha_threshold INTEGER, params TEXT,
                                   PRIMARY KEY(copy_id, block)) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS texset (id INTEGER PRIMARY KEY, n INTEGER NOT NULL,
                                   t0 INTEGER, t1 INTEGER, t2 INTEGER, t3 INTEGER, t4 INTEGER, t5 INTEGER, t6 INTEGER,
                                   t7 INTEGER, t8 INTEGER, UNIQUE(n, t0, t1, t2, t3, t4, t5, t6, t7, t8));
CREATE TABLE IF NOT EXISTS tex_use (tex_path_id INTEGER, texset_id INTEGER, slot INTEGER,
                                   PRIMARY KEY(tex_path_id, texset_id, slot)) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS nif_winner     (path_id INTEGER PRIMARY KEY, copy_id INTEGER NOT NULL, n_copies INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS nif_winner_src (path_id INTEGER PRIMARY KEY, copy_id INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS pgp_diff (path_id INTEGER PRIMARY KEY, crc_orig INTEGER NOT NULL, crc_patched INTEGER NOT NULL);
CREATE INDEX IF NOT EXISTS nif_copy_origin ON nif_copy(origin_id);
```

`shape.params` holds `serde_json` of `LightingParams`/`EffectParams`. Add `#[derive(serde::Serialize, serde::Deserialize)]` to those structs, and to `LightingTrailer` and `TexPath`/`PathFlags`, in `nif` behind an optional `serde` feature that `modscan` enables. Texture tables stay texture-path-only for now. `tex_copy`/`tex_winner` arrive with the texture-resolution step in Task 11.

Orchestrator algorithm (`scan::run`):
1. `if pgp::running(&procs()) { return Err(ScanError::PgpRunning) }`.
2. Build origins:
   - `Mo2(inst)`: `loose_origins` + `active_bsas`.
   - `Folder(p)`: one loose origin, priority 0.
   - Upsert into `origin`. For each loose origin compute `pgp::signature(root)`. `Strong`/`Medium` on a `Mod` → `derived_by = 1`. `Incomplete` on a `Mod` → `derived_by = 1, derived_state = incomplete` (05 §5.1: treated as derived and unverified; its copies get `derive_check = unknown`). On Overwrite/GameData → `derived_by = 1` only per path via `is_derived_path`. Read the diff once per derived origin into `pgp_diff`.
3. Walk: `vfs::loose::walk(root, &["meshes"], &["nif"])` per loose origin, and `Bsa::open` name tables per BSA (NIF entries only). Collect `resolve::Copy`s.
4. For each copy whose `(path, origin)` row exists with the same `(size, mtime)` → reuse. Otherwise read the bytes (loose: `fs::read`; BSA: `Bsa::read`) and `nif::scan::scan`. Run this over `rayon` in chunks of 512; check `cancel` between chunks. Store results in one transaction per chunk.
5. `pgp::running(&procs())` again. If true → `report.tainted = true`, roll back the rows of derived origins written in this run (keep the previous rows), and do not update the winners.
6. Winners:
   - `game = winners(.., &|_| false)`;
   - `source = winners(.., &|o| derived(o))`;
   - write both tables;
   - for each derived copy, `source_copy_id = source[path or dup_source(path)]`. Compute CRC32 of the source copy (`crc32fast`, cached in `nif_copy.crc32` by `(size, mtime)`) and of the deeper loose copies; then `derive_check = pgp::check(...)` (`DupOf` for dups).
7. Delete `nif_copy` rows not seen in this walk (cascade their `shape` rows).

- [ ] **Step 1: Write the failing tests**

First move `instance_fixture` from `tests/mo2.rs` into `crates/modscan/tests/common/mod.rs` and use `mod common; use common::instance_fixture;` in both test files. With that, the scan tests are:

```rust
// crates/modscan/tests/scan.rs
mod common;
use common::instance_fixture;
use modscan::mo2::Instance;
use modscan::scan::{run, Source};
use modscan::store::Store;
use std::sync::atomic::AtomicBool;

fn put(root: &std::path::Path, rel: &str, bytes: Option<&[u8]>) {
    let fixture = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../nif/tests/fixtures/TestNifFile_Skinned_SE.nif")).unwrap();
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, bytes.unwrap_or(&fixture)).unwrap();
}

fn go(d: &tempfile::TempDir, procs: Vec<String>) -> (Store, modscan::scan::Report) {
    let inst = Instance::load(d.path(), None).unwrap();
    let mut store = Store::open(&d.path().join("scan.db")).unwrap();
    let r = run(&Source::Mo2(inst), &mut store, &AtomicBool::new(false), &|_| {}, &move || procs.clone()).unwrap();
    (store, r)
}

#[test]
fn winner_and_source_with_pgpatcher_output() {
    let d = instance_fixture("+PGP\n+A\n");
    std::fs::create_dir_all(d.path().join("mods/PGP")).unwrap();
    put(d.path(), "mods/A/meshes/x.nif", None);
    let a_bytes = std::fs::read(d.path().join("mods/A/meshes/x.nif")).unwrap();
    put(d.path(), "mods/PGP/meshes/x.nif", None);
    put(d.path(), "mods/PGP/PGPatcher.esp", Some(b"e"));
    let crc = crc32fast::hash(&a_bytes);
    put(d.path(), "mods/PGP/ParallaxGen_Diff.json",
        Some(format!(r#"{{"meshes\\x.nif":{{"crc32original":{crc},"crc32patched":1}}}}"#).as_bytes()));
    let (store, r) = go(&d, vec![]);
    assert!(!r.tainted);
    assert_eq!(r.pgp.len(), 1);
    let mods = store.mods();
    let a = mods.iter().find(|m| m.name == "A").unwrap();
    let nifs = store.nifs_of(a.id);
    assert_eq!(nifs[0].status_label(), "Patched by PGPatcher (in game)");
    assert_eq!(nifs[0].derive_check.as_deref(), Some("match"));
}

#[test]
fn stale_when_source_changed_even_with_older_mtime() {
    let d = instance_fixture("+PGP\n+A\n");
    put(d.path(), "mods/A/meshes/x.nif", None);
    put(d.path(), "mods/PGP/meshes/x.nif", None);
    put(d.path(), "mods/PGP/PGPatcher.esp", Some(b"e"));
    put(d.path(), "mods/PGP/ParallaxGen_Diff.json", Some(br#"{"meshes\\x.nif":{"crc32original":1,"crc32patched":2}}"#));
    let (store, _) = go(&d, vec![]);
    let a = store.mods().into_iter().find(|m| m.name == "A").unwrap();
    assert_eq!(store.nifs_of(a.id)[0].derive_check.as_deref(), Some("stale_changed"));
}

#[test]
fn pgp_running_blocks_scan() {
    let d = instance_fixture("+A\n");
    put(d.path(), "mods/A/meshes/x.nif", None);
    let inst = Instance::load(d.path(), None).unwrap();
    let mut store = Store::open(&d.path().join("scan.db")).unwrap();
    let r = run(&Source::Mo2(inst), &mut store, &AtomicBool::new(false), &|_| {}, &|| vec!["pgpatcher.exe".into()]);
    assert!(matches!(r, Err(modscan::ScanError::PgpRunning)));
}

#[test]
fn pgp_started_mid_scan_taints_result() {
    let d = instance_fixture("+PGP\n+A\n");
    put(d.path(), "mods/A/meshes/x.nif", None);
    put(d.path(), "mods/PGP/meshes/x.nif", None);
    put(d.path(), "mods/PGP/PGPatcher.esp", Some(b"e"));
    let calls = std::sync::atomic::AtomicUsize::new(0);
    let inst = Instance::load(d.path(), None).unwrap();
    let mut store = Store::open(&d.path().join("scan.db")).unwrap();
    let r = run(&Source::Mo2(inst), &mut store, &AtomicBool::new(false), &|_| {}, &|| {
        if calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 { vec![] } else { vec!["pgtools.exe".into()] }
    })
    .unwrap();
    assert!(r.tainted);
    let pgp = store.mods().into_iter().find(|m| m.name == "PGP").unwrap();
    assert!(store.nifs_of(pgp.id).is_empty(), "derived rows from a tainted run must not be committed");
}

#[test]
fn not_skyrim_and_le_are_counted_not_fatal() {
    let d = instance_fixture("+A\n");
    put(d.path(), "mods/A/meshes/fo4.nif",
        Some(&std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../nif/tests/fixtures/TestNifFile_Skinned_FO4.nif")).unwrap()));
    put(d.path(), "mods/A/meshes/le.nif",
        Some(&std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../nif/tests/fixtures/TestNifFile_Animated_LE.nif")).unwrap()));
    put(d.path(), "mods/A/meshes/empty.nif", Some(b""));
    let (_, r) = go(&d, vec![]);
    assert_eq!(r.not_skyrim, 1);
    assert_eq!(r.le_meshes, 1);
    assert_eq!(r.problems.len(), 1);
}

#[test]
fn rescan_reuses_unchanged_copies() {
    let d = instance_fixture("+A\n");
    put(d.path(), "mods/A/meshes/x.nif", None);
    let (_, r1) = go(&d, vec![]);
    let (_, r2) = go(&d, vec![]);
    assert_eq!((r1.parsed, r2.parsed, r2.reused), (1, 0, 1));
}
```

`ModRow { id: i64, name: String, kind: i32, priority: i32, derived: bool }`. `NifRow { copy_id: i64, path: String, wins: bool, overridden_by: Option<String>, n_copies: u32, derived: bool, derive_check: Option<String>, le_mesh: bool, problem: Option<String> }`, with `NifRow::status_label(&self) -> &'static str`:
- `"Wins"`;
- `"Patched by PGPatcher (in game)"`: the game winner is a derived copy whose source is this copy;
- `"Overridden"`: any other loser;
- `"Hidden by PGPatcher patch"`: never used for sources. It is reserved; don't emit it.

`overridden_by` names the non-derived overrider (05 §5.3).

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p modscan --test scan`
Expected: compile error.

- [ ] **Step 3: Implement `store.rs` and `scan.rs`**

Implement exactly the schema and algorithm above. Separators: Task 7's `loose_origins` drops `_separator` lines, so step 2 re-reads `modlist.txt` and upserts each `+<name>_separator` line as an `origin` row with `kind = 3`, `enabled = 1`, and `priority` equal to the mod below it (display only, never a file source). `ModRow` exposes them for Task 12's headers.

Constraints for the reviewer:
- One `rusqlite::Transaction` per 512-copy chunk.
- Copies of derived origins are written into a temp table `pending_derived` and moved into `nif_copy` only after step 5 passes.
- On a tainted run, drop `pending_derived` and keep the old rows. That is what `pgp_started_mid_scan_taints_result` checks.
- CRC32 only for source candidates of derived copies (`crc32fast::hash(&bytes)`), cached in `nif_copy.crc32`.
- The `params` column stores `serde_json::to_string` of the params.
- `texset` rows are deduplicated by `INSERT OR IGNORE` + `SELECT id`. Empty slots are `NULL`. `tex_use` gets one row per non-empty slot.
- `NotSkyrim` → `parse_status = 2`, counted in `not_skyrim`, no problem entry. Other `NifError`s → `parse_status = 3`, `problem` text, `problems` entry. `scan.problems` non-empty → `parse_status = 1` and the problems joined into `problem`.

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p modscan`
Expected: all PASS.

- [ ] **Step 5: Owner-instance smoke run (read-only; writes only `%TEMP%\sdc-scan-smoke.db`)**

Add an `#[ignore]` test `owner_smoke`. It runs `scan::run` on `C:\_mount\moddingvol\Acherus` with `win::process_names`, then prints:
- the `Report` and elapsed time;
- the count of game winners;
- the share of game winners from `ParallaxGen_Output`. Expect about 25.5k non-FaceGen winning paths (05 TL;DR 1).

Run: `cargo test -p modscan --release -- --ignored owner_smoke --nocapture`
Expected: completes. Record the time against the 10–30 s estimate (00 §1) in the commit message. If it exceeds 60 s, open a follow-up task; don't optimise here.

- [ ] **Step 6: Commit**

```bash
git add crates/nif crates/modscan
git commit -m "feat(modscan): SQLite store and scan orchestrator with PGPatcher derived layer"
```

---

### Task 11: Texture resolution for slot paths

**Files:**
- Modify: `crates/modscan/src/store.rs`, `crates/modscan/src/scan.rs`
- Test: `crates/modscan/tests/textures.rs`

**Interfaces:**
- Produces:
  - tables `tex_copy (path_id, origin_id, size, mtime, PRIMARY KEY(path_id, origin_id))` and `tex_winner (path_id PRIMARY KEY, origin_id, n_copies)`;
  - `Store::resolve_texture(norm: &str) -> TexResolution { Found { origin: String, n_copies: u32 }, Missing }`.

Algorithm: during the walk, also `vfs::loose::walk(root, &["textures"], &["dds"])` per loose origin and texture entries per BSA (`Bsa::open` name table, `.dds` only). Then run `winners` on them (game view only) and store the winners.

- [ ] **Step 1: Write the failing test**

```rust
// crates/modscan/tests/textures.rs
mod common;
use common::instance_fixture;

#[test]
fn slot_textures_resolve_to_winning_origin_or_missing() {
    let d = instance_fixture("+B\n+A\n");
    for (m, f) in [("A", "textures/a_d.dds"), ("B", "textures/a_d.dds"), ("A", "textures/only_a.dds")] {
        let p = d.path().join("mods").join(m).join(f);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, b"DDS ").unwrap();
    }
    let inst = modscan::mo2::Instance::load(d.path(), None).unwrap();
    let mut store = modscan::store::Store::open(&d.path().join("scan.db")).unwrap();
    modscan::scan::run(&modscan::scan::Source::Mo2(inst), &mut store, &Default::default(), &|_| {}, &|| vec![]).unwrap();
    use modscan::store::TexResolution::*;
    assert_eq!(store.resolve_texture("textures/a_d.dds"), Found { origin: "B".into(), n_copies: 2 });
    assert_eq!(store.resolve_texture("textures/only_a.dds"), Found { origin: "A".into(), n_copies: 1 });
    assert_eq!(store.resolve_texture("textures/nope.dds"), Missing);
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p modscan --test textures`
Expected: FAIL (`resolve_texture` missing).

- [ ] **Step 3: Implement** the tables, the walk addition and `resolve_texture` (`SELECT o.name, w.n_copies FROM tex_winner w JOIN path p ON p.id = w.path_id JOIN origin o ON o.id = w.origin_id WHERE p.p = ?1`).

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p modscan`
Expected: all PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/modscan
git commit -m "feat(modscan): resolve texture slot paths to their winning origin"
```

---

### Task 12: sdc-gui "Meshes" view

**Files:**
- Modify: `crates/gui/Cargo.toml` (add `modscan = { path = "../modscan" }`, `nif = { path = "../nif" }`)
- Modify: `crates/gui/src/worker.rs`: new `WorkerMsg::ScanProgress { done: usize, total: usize, phase: &'static str }` and `WorkerMsg::Scanned(Result<modscan::scan::Report, String>)`, plus `fn start_scan(source: modscan::scan::Source, db: PathBuf, tx: Sender<WorkerMsg>, cancel: Arc<AtomicBool>)`. It follows the existing per-call thread + panic catch pattern of `worker.rs`.
- Create: `crates/gui/src/model/meshes.rs` (`MeshesModel { store_path, mods: Vec<ModRow>, selected_mod, nifs: Vec<NifRow>, filter: NifFilter, selected_nif, shapes: Vec<ShapeView>, view_source: bool, banner: Option<Banner> }`)
- Create: `crates/gui/src/ui/meshes.rs`
- Modify: `crates/gui/src/ui/mod.rs`, `crates/gui/src/app.rs` (one top-level toggle "Textures | Meshes" in the header; read `app.rs` and `ui/header.rs` first and add the toggle next to the existing Mode menu)
- Test: `crates/gui/tests/meshes_view.rs` (egui_kittest, the harness already used by the GUI tests)

**Interfaces:**
- Consumes: `modscan::{scan, store, mo2}`.
- Produces:
  - `MeshesModel::empty() -> MeshesModel`; `enum Banner { PgpRunning, PgpIncomplete(String), PgpStale(usize) }`; `struct ScanRequest { pub skip_derived: bool }` and `MeshesModel::take_request(&mut self) -> Option<ScanRequest>` (set by the banner buttons and the Scan button, drained by `app.rs`, which calls `worker::start_scan`); `sdc_gui::ui::meshes::show(ui: &mut egui::Ui, m: &mut MeshesModel)`.
  - `MeshesModel::banner()` texts, verbatim from 05 §5.3:
    - "PGPatcher is running — scan paused." [Retry] [Scan without PGPatcher output];
    - "PGPatcher output '{name}' looks incomplete (no ParallaxGen_Diff.json). Re-run PGPatcher.";
    - "PGPatcher output is out of date for {n} meshes." [Show list];
    - "LE mesh: optimise it for SSE, e.g. with Cathedral Assets Optimizer." (per NIF, in the detail pane).
  - MO2 first-run dialog: "Opened from Mod Organizer 2 instance '{root}', profile '{profile}'. Use it?" [Use] [Choose instance…] [Open a folder instead]. The answer is saved in `%APPDATA%\sdc\gui.toml` under `[meshes] instance`, `profile`.

The "Scan without PGPatcher output" button on the running banner sets the scan's `exclude derived` flag. To support it:
- extend `scan::run` with `skip_derived: bool` (default false);
- when true, skip the process check and do not walk derived origins;
- add a test in Task 10's file: `skip_derived_ignores_pgp_and_running_process`.

- [ ] **Step 1: Write the failing UI test**

```rust
// crates/gui/tests/meshes_view.rs
use egui_kittest::{kittest::Queryable, Harness};
use sdc_gui::model::meshes::{Banner, MeshesModel};

fn model_with(banner: Option<Banner>) -> MeshesModel {
    let mut m = MeshesModel::empty();
    m.mods = vec![modscan::store::ModRow { id: 1, name: "A".into(), kind: 1, priority: 0, derived: false }];
    m.banner = banner;
    m
}

#[test]
fn running_banner_offers_retry_and_skip() {
    let mut m = model_with(Some(Banner::PgpRunning));
    let mut h = Harness::new_ui(|ui| sdc_gui::ui::meshes::show(ui, &mut m));
    h.run();
    h.get_by_label("PGPatcher is running — scan paused.");
    h.get_by_label("Retry");
    h.get_by_label("Scan without PGPatcher output").click();
    h.run();
    assert!(m.take_request().is_some_and(|r| r.skip_derived));
}

#[test]
fn mod_list_and_badges() {
    let mut m = model_with(None);
    m.nifs = vec![modscan::store::NifRow {
        copy_id: 1, path: "meshes/x.nif".into(), wins: false, overridden_by: None, n_copies: 2, derived: false,
        derive_check: Some("stale_changed".into()), le_mesh: false, problem: None, patched_in_game: true,
    }];
    let mut h = Harness::new_ui(|ui| sdc_gui::ui::meshes::show(ui, &mut m));
    h.run();
    h.get_by_label("A");
    h.get_by_label("Patched by PGPatcher (in game)");
    h.get_by_label("PGPatcher patch out of date");
}
```

(`NifRow` gains `patched_in_game: bool`. Set it in Task 10's `nifs_of`, and update Task 10 if this task runs after it, which it does.)

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p sdc-gui --test meshes_view`
Expected: compile error.

- [ ] **Step 3: Implement**

Three columns, matching the existing three-column layout:
- **Left:** mods by priority, with separator names from `modlist.txt` as headers (`ModRow.kind = 3` rows written by Task 10 step 2), plus a "Generated by PGPatcher" chip on derived mods.
- **Centre:** NIFs of the selected mod, with a badge from `status_label()`, a stale chip ("PGPatcher patch out of date") when `derive_check` starts `stale`, and an LE chip. Filters: All / Wins / Overridden / Conflicted / Problems.
- **Right:** shapes of the selected NIF.
  - Per shape: shape name, shader kind/type name (use the type-name table from `skyrim_conventions.md` §8), SLSF1/SLSF2 as hex, and slot rows "slot n: path → winning origin / missing".
  - An "In game | Source" toggle when the NIF has a derived copy.
  - The LE warning text for `le_mesh`.

Clicking a texture path lists `users_of_texture`.

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p sdc-gui`
Expected: all PASS, including the existing GUI tests (no regressions).

- [ ] **Step 5: Manual check on the owner's machine**

Launch `sdc-gui` from MO2 (add it as an MO2 executable once):
- the first-run dialog names `C:\_mount\moddingvol\Acherus`, profile `Nexus`;
- Meshes → scan completes;
- `ParallaxGen_Output` shows the chip;
- selecting "Assorted mesh fixes" shows "Patched by PGPatcher (in game)" badges.

Report the scan time.

- [ ] **Step 6: Commit**

```bash
git add crates/gui
git commit -m "feat(gui): Meshes view — per-mod NIFs, PGPatcher-aware badges, texture slots"
```

---

## Self-review notes (done while writing)

- **Spec coverage:**

  | Requirement | Task(s) |
  |---|---|
  | 00 §2 rules 1–5 | Tasks 3–5, 10 |
  | §3.1 crates | Tasks 1, 6, 7, 10 |
  | §3.2 MO2 steps 1–6 | 7, 8, 10, 12 |
  | §3.3 GUI | 12 |
  | 05 §5.1–5.4 | 9, 10, 12 |
  | 06 findings (count read, lossy strings, LE flag, types 3/14 recorded) | 1, 4, 5 |

  - Deferred, out of scope: the optional MO2 Python "Verify with MO2" plugin (00 §3.2 item 4), and the Vortex fallback.
- **Known follow-ups not in this plan:** BSA-sourced derived meshes are verified only on demand (05 open question 1). `derived_by` for BodySlide/VRAMr is waiting on the owner (05 open question 2).
- **Type names** used across tasks: `ShapeRow`, `Shader`, `NifScan`, `LooseOrigin`, `OriginRef`, `Copy`, `ActiveBsa`, `Signature`, `DeriveCheck`, `ModRow`, `NifRow`, `TexResolution`. Each is defined in the task that produces it.

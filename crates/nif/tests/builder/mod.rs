//! Writes minimal Skyrim NIFs (20.2.0.7, user 12) for tests: header + raw blocks + footer.
#![allow(dead_code)]
pub struct Nif {
    pub bsver: u32,
    pub version: u32,
    pub user: u32,
    types: Vec<String>,
    blocks: Vec<(u16, Vec<u8>)>,
    pub strings: Vec<String>,
    /// Footer root refs.
    pub roots: Vec<i32>,
    /// OR-ed into block i's stored type index (e.g. 0x8000).
    pub type_flags: Vec<u16>,
    /// Overrides the computed Max String Length.
    pub max_string_len: Option<u32>,
}

impl Nif {
    pub fn new(bsver: u32) -> Self {
        Self {
            bsver,
            version: 0x1402_0007,
            user: 12,
            types: vec![],
            blocks: vec![],
            strings: vec![],
            roots: vec![0],
            type_flags: vec![],
            max_string_len: None,
        }
    }
    /// Registers a block type without adding a block.
    pub fn add_type(&mut self, kind: &str) {
        if !self.types.iter().any(|x| x == kind) {
            self.types.push(kind.into());
        }
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
        for (i, (t, _)) in self.blocks.iter().enumerate() {
            let f = self.type_flags.get(i).copied().unwrap_or(0);
            b.extend((t | f).to_le_bytes());
        }
        for (_, body) in &self.blocks {
            b.extend((body.len() as u32).to_le_bytes());
        }
        b.extend((self.strings.len() as u32).to_le_bytes());
        let computed = self.strings.iter().map(|s| s.len()).max().unwrap_or(0) as u32;
        b.extend(self.max_string_len.unwrap_or(computed).to_le_bytes());
        for s in &self.strings {
            b.extend((s.len() as u32).to_le_bytes());
            b.extend(s.as_bytes());
        }
        b.extend(0u32.to_le_bytes()); // groups
        for (_, body) in &self.blocks {
            b.extend(body);
        }
        b.extend((self.roots.len() as u32).to_le_bytes());
        for r in &self.roots {
            b.extend(r.to_le_bytes());
        }
        b
    }
}

/// Little-endian byte writer for block bodies.
#[derive(Default)]
pub struct W(pub Vec<u8>);
impl W {
    pub fn u8(mut self, v: u8) -> Self {
        self.0.push(v);
        self
    }
    pub fn u16(mut self, v: u16) -> Self {
        self.0.extend(v.to_le_bytes());
        self
    }
    pub fn u32(mut self, v: u32) -> Self {
        self.0.extend(v.to_le_bytes());
        self
    }
    pub fn i32(mut self, v: i32) -> Self {
        self.0.extend(v.to_le_bytes());
        self
    }
    pub fn u64(mut self, v: u64) -> Self {
        self.0.extend(v.to_le_bytes());
        self
    }
    pub fn f(mut self, v: f32) -> Self {
        self.0.extend(v.to_le_bytes());
        self
    }
    pub fn fs(mut self, v: &[f32]) -> Self {
        for x in v {
            self.0.extend(x.to_le_bytes());
        }
        self
    }
    pub fn sized(mut self, s: &str) -> Self {
        self.0.extend((s.len() as u32).to_le_bytes());
        self.0.extend(s.as_bytes());
        self
    }
    pub fn raw(mut self, b: &[u8]) -> Self {
        self.0.extend(b);
        self
    }
}

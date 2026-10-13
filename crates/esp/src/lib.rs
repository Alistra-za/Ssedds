//! Targeted Skyrim SE plugin reader (esp scanner spec §2.1): one plugin's bytes → the facts sdc needs (TXSTs, model
//! paths with alternate textures, TXST references, direct texture paths). Unwanted top-level groups are skipped by
//! size; nothing is ever written. Layouts: xEdit `wbDefinitionsTES5.pas` / `wbDefinitionsCommon.pas` (dev-4.1.6).
mod read;
mod records;

pub use read::parse;

/// TES4 header flags.
pub const FLAG_ESM: u32 = 0x1;
pub const FLAG_LOCALIZED: u32 = 0x80;
pub const FLAG_ESL: u32 = 0x200;
/// Record header flag: the data is a u32 decompressed size followed by a zlib stream.
pub const FLAG_COMPRESSED: u32 = 0x0004_0000;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PluginFacts {
    pub name: String,
    /// TES4 record flags as stored.
    pub flags: u32,
    pub masters: Vec<String>,
    pub records: Vec<Rec>,
    /// Base FormIDs (as stored) placed by REFR/ACHR records in CELL and WRLD groups, once each, ascending
    /// (filtering spec §3). The references themselves are not kept.
    pub placed: Vec<u32>,
    /// `"offset 0x…: message"`, in file order. Records read before a fault are kept.
    pub errors: Vec<String>,
}

impl PluginFacts {
    pub fn esm(&self) -> bool {
        self.flags & FLAG_ESM != 0
    }
    pub fn esl(&self) -> bool {
        self.flags & FLAG_ESL != 0
    }
    pub fn localized(&self) -> bool {
        self.flags & FLAG_LOCALIZED != 0
    }
}

/// One record of a wanted type, as this plugin stores it.
#[derive(Debug, Clone, PartialEq)]
pub struct Rec {
    /// High byte = index into the plugin's masters (== masters.len(): the plugin itself).
    pub local_formid: u32,
    pub sig: [u8; 4],
    pub edid: Option<String>,
    pub payload: Payload,
}

/// What a record carries for sdc (spec §2.1). A record can carry several parts (ARMA: models and TXST references).
/// Paths are canonical (`nif::path`): lower case, `/`, `textures/…` or `meshes/…`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Payload {
    pub txst: Option<Txst>,
    pub models: Vec<ModelRef>,
    /// (role = subrecord signature, local FormID). The target is a TXST, except role HNAM (an extra-part HDPT), GNAM (a GRAS)
    /// and ARMO MODL (an ARMA); FormID 0 is never stored.
    pub txst_refs: Vec<([u8; 4], u32)>,
    /// HDPT DATA flags (xEdit: 0x01 Playable, 0x02 Male, 0x04 Female, 0x08 Is Extra Part, 0x10 Use Solid Tint);
    /// `None` on other records and when DATA is absent.
    pub hdpt_flags: Option<u8>,
    /// (role = subrecord signature, canonical texture path).
    pub textures: Vec<([u8; 4], String)>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Txst {
    /// TX00..TX07.
    pub slots: [Option<String>; 8],
    /// A DODT subrecord is present.
    pub decal: bool,
    /// DNAM (xEdit: itU16 — 0x1 No Specular Map, 0x2 Facegen Textures, 0x4 Has Model Space Normal Map).
    pub flags: u16,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModelRef {
    /// MODL 0, MOD2..MOD5 2..5, DMDL 6.
    pub slot: u8,
    pub path: String,
    pub alt: Vec<AltTex>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AltTex {
    /// The NIF shape name ("3D Name").
    pub shape_name: String,
    pub txst_local_formid: u32,
    /// "3D Index" (informational; shapes are matched by name).
    pub index: i32,
}

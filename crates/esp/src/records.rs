//! Which groups are read, and what each record type contributes (spec §3).
use crate::read::each_sub;
use crate::Rec;

/// Top-level groups read (spec §3); every other group is skipped by its size.
const WANTED: [[u8; 4]; 28] = [
    *b"TXST", *b"STAT", *b"MSTT", *b"ACTI", *b"FURN", *b"DOOR", *b"CONT", *b"LIGH", *b"MISC",
    *b"TREE", *b"FLOR", *b"GRAS", *b"ARMA", *b"ARMO", *b"WEAP", *b"AMMO", *b"BOOK", *b"INGR",
    *b"ALCH", *b"KEYM", *b"SLGM", *b"SCRL", *b"LTEX", *b"HDPT", *b"NPC_", *b"RACE", *b"EFSH",
    *b"WTHR",
];

pub(crate) fn wanted(label: &[u8; 4]) -> bool {
    WANTED.contains(label)
}

/// The bytes up to the first NUL.
pub(crate) fn until_nul(d: &[u8]) -> &[u8] {
    &d[..d.iter().position(|&c| c == 0).unwrap_or(d.len())]
}

/// A zstring, Windows-1252 (EDID and paths are never localized); `None` when empty.
pub(crate) fn zstring(d: &[u8]) -> Option<String> {
    let s = until_nul(d);
    (!s.is_empty()).then(|| nif::text::lossy(s))
}

/// Fills `rec` from its subrecords; subrecords not listed in spec §3 are skipped without allocating.
pub(crate) fn read(rec: &mut Rec, data: &[u8]) -> Result<(), usize> {
    let edid = &mut rec.edid;
    each_sub(data, |sig, d| {
        if &sig == b"EDID" {
            *edid = zstring(d);
        }
    })
}

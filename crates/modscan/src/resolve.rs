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
) -> HashMap<String, (usize, u32)> {
    let mut best: HashMap<String, (usize, u32)> = HashMap::new();
    for (i, c) in copies.iter().enumerate() {
        let e = best.entry(c.path.clone()).or_insert((i, 0));
        e.1 += 1;
        if key(c.origin, loose, bsas) > key(copies[e.0].origin, loose, bsas) {
            e.0 = i;
        }
    }
    best
}

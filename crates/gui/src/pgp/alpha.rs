//! NiAlphaProperty flags (Gamebryo / NifSkope layout; `nif::props::Alpha` agrees on bits 0 and 9): bit 0 blending,
//! bits 1-4 source mode, 5-8 destination mode, 9 testing, 10-12 test function, 13 no sorter, 14-15 kept as stored.
pub const BLEND_MODES: [&str; 11] = [
    "One",
    "Zero",
    "Src Color",
    "Inv Src Color",
    "Dest Color",
    "Inv Dest Color",
    "Src Alpha",
    "Inv Src Alpha",
    "Dest Alpha",
    "Inv Dest Alpha",
    "Src Alpha Saturate",
];
pub const TEST_FUNCS: [&str; 8] = [
    "Always",
    "Less",
    "Equal",
    "Less or Equal",
    "Greater",
    "Not Equal",
    "Greater or Equal",
    "Never",
];
/// Testing on, Greater, Src Alpha / Inv Src Alpha, blending off (DECISIONS AG28).
pub const DEFAULT_FLAGS: u16 = 4844;
pub const DEFAULT_THRESH: u8 = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AlphaFlags {
    pub blend: bool,
    pub src: u8,
    pub dst: u8,
    pub test: bool,
    pub func: u8,
    pub no_sorter: bool,
    /// Bits 14-15, never changed here.
    pub keep: u8,
}

impl AlphaFlags {
    pub fn from_raw(r: u16) -> Self {
        AlphaFlags {
            blend: r & 1 != 0,
            src: (r >> 1 & 0xF) as u8,
            dst: (r >> 5 & 0xF) as u8,
            test: r >> 9 & 1 != 0,
            func: (r >> 10 & 7) as u8,
            no_sorter: r >> 13 & 1 != 0,
            keep: (r >> 14) as u8,
        }
    }

    /// Fields are masked to their width, so one cannot spill into its neighbour.
    pub fn raw(&self) -> u16 {
        u16::from(self.blend)
            | u16::from(self.src & 0xF) << 1
            | u16::from(self.dst & 0xF) << 5
            | u16::from(self.test) << 9
            | u16::from(self.func & 7) << 10
            | u16::from(self.no_sorter) << 13
            | u16::from(self.keep & 3) << 14
    }
}

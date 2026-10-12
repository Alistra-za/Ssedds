//! Raw C ABI of shim/sdc_shim.cpp. Only this crate calls these.
extern "C" {
    pub fn sdc_init(bc1_mode: i32);
    pub fn sdc_bc7e_compress(
        n: u32,
        blocks: *mut u64,
        px: *const u32,
        level: i32,
        perceptual: i32,
        exact: i32,
    );
    pub fn sdc_bc1_encode(dst: *mut u8, px: *const u8, level: u32, allow3: i32, black: i32);
    pub fn sdc_bc3_encode(dst: *mut u8, px: *const u8, level: u32, rad: u32);
    pub fn sdc_bc4_encode(dst: *mut u8, px: *const u8, stride: u32, rad: u32);
    pub fn sdc_bc5_encode(dst: *mut u8, px: *const u8, rad: u32);
    pub fn sdc_bc1_decode(src: *const u8, px: *mut u8, mode: i32) -> i32;
    pub fn sdc_bc4_block_values(l: u32, h: u32, out8: *mut u8);
    pub fn sdc_bc6h_compress(
        rgba16f: *const u16,
        width: i32,
        rows: i32,
        stride_bytes: i32,
        dst: *mut u8,
        level: i32,
    );
    pub fn sdc_bc7e_patched() -> i32;
}

//! CS's glint noise map (`Glints::Glint2023NoiseMap`, t20; Glints2023.hlsli:7, 224-231), made on the CPU: a port of
//! CS's `Common/Glints/noisegen.cs.hlsl` for the 128×128 map the shader indexes. Every texel packs four
//! (uniform, gaussian) pairs, each as two f16 bit patterns in one f32: the cell, its +y, +x and +x+y neighbours.

/// Side of the map (Glints2023.hlsli:224 `uint2 size = 128`).
pub const SIZE: u32 = 128;

fn wang_hash(mut s: u32) -> u32 {
    s = (s ^ 61) ^ (s >> 16);
    s = s.wrapping_mul(9);
    s ^= s >> 4;
    s = s.wrapping_mul(0x27d4_eb2d);
    s ^ (s >> 15)
}

fn xorshift_float(s: &mut u32) -> f32 {
    *s ^= *s << 13;
    *s ^= *s >> 17;
    *s ^= *s << 5;
    *s as f32 * (1.0 / 4_294_967_296.0)
}

// Giles' single-precision erfinv, as noisegen.cs.hlsl writes it.
#[allow(clippy::excessive_precision)]
fn erf_inv(x: f32) -> f32 {
    let mut w = -((1.0 - x) * (1.0 + x)).ln();
    let p = if w < 5.0 {
        w -= 2.5;
        let mut q = 2.810_226_36e-08;
        q = 3.432_739_39e-07 + q * w;
        q = -3.523_387_7e-06 + q * w;
        q = -4.391_506_54e-06 + q * w;
        q = 0.000_218_580_87 + q * w;
        q = -0.001_253_725_03 + q * w;
        q = -0.004_177_681_64 + q * w;
        q = 0.246_640_727 + q * w;
        1.501_409_41 + q * w
    } else {
        w = w.sqrt() - 3.0;
        let mut q = -0.000_200_214_257;
        q = 0.000_100_950_558 + q * w;
        q = 0.001_349_343_22 + q * w;
        q = -0.003_673_428_44 + q * w;
        q = 0.005_739_507_73 + q * w;
        q = -0.007_622_461_3 + q * w;
        q = 0.009_438_870_47 + q * w;
        q = 1.001_674_06 + q * w;
        2.832_976_82 + q * w
    };
    p * x
}

/// The (uniform, gaussian) pair of cell (`x`, `y`): `WangHash(flat id of coord × 123 + w·h·0x69420)`, then two
/// xorshift draws, the second through the standard normal inverse CDF.
pub fn cell(x: u32, y: u32) -> (f32, f32) {
    // `int offset = size.x * size.y * 0x69420` wraps in 32 bits; added to the uint id it is the same bits.
    let offset = (SIZE * SIZE).wrapping_mul(0x69420);
    let mut s = wang_hash((y * 123 * SIZE + x * 123).wrapping_add(offset));
    let u = xorshift_float(&mut s);
    let g = std::f32::consts::SQRT_2 * erf_inv(2.0 * xorshift_float(&mut s) - 1.0);
    (u, g)
}

/// `PackFloats`: `f32tof16(a) << 16 | f32tof16(b)` read as a float (f16 rounding: nearest even).
fn pack((a, b): (f32, f32)) -> f32 {
    let h = |v: f32| u32::from(half::f16::from_f32(v).to_bits());
    f32::from_bits(h(a) << 16 | h(b))
}

/// The whole map, RGBA32F, row by row (`SIZE² × 4` floats).
pub fn noise_map() -> Vec<f32> {
    let n = |v: u32| (v + 1) % SIZE;
    let mut out = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for y in 0..SIZE {
        for x in 0..SIZE {
            out.extend([
                pack(cell(x, y)),
                pack(cell(x, n(y))),
                pack(cell(n(x), y)),
                pack(cell(n(x), n(y))),
            ]);
        }
    }
    out
}

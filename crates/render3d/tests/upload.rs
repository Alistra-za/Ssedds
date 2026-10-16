//! Audit REPORT 2026-10-16 #13 (Note BD): `load_dds_as` per-face/per-mip pointer and pitch arithmetic on BC1 files
//! with a full mip chain: square and non-square 2D, and a cube. Every block has its own colour; every subresource
//! is copied back from the GPU (WARP) and compared byte for byte with the file, so a wrong face offset, mip start,
//! row pitch (too small or too large) or a width/height swap fails here.
use render3d::device::Gpu;
use render3d::textures::load_dds;
use windows::core::Interface;
use windows::Win32::Graphics::Direct3D11::{
    ID3D11Resource, ID3D11ShaderResourceView, ID3D11Texture2D, D3D11_CPU_ACCESS_READ,
    D3D11_MAPPED_SUBRESOURCE, D3D11_MAP_READ, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
};

/// One flat BC1 block: colour0 = colour1 = `c` (RGB565), every index 0.
fn block(c: u16) -> [u8; 8] {
    let [a, b] = c.to_le_bytes();
    [a, b, a, b, 0, 0, 0, 0]
}

/// (blocks across, blocks down) of mip `m` of a `w`x`h` BC texture.
fn blocks(w: u32, h: u32, m: u32) -> (usize, usize) {
    let b = |s: u32| (s >> m).max(1).div_ceil(4) as usize;
    (b(w), b(h))
}

/// A `w`x`h` BC1 (DXT1) DDS with its full mip chain, legacy header (`cube` = six faces, `w` = `h`), and the
/// expected bytes of every subresource (face-major). Each block is a distinct colour.
fn dds_bc1_chain(w: u32, h: u32, cube: bool) -> (Vec<u8>, Vec<Vec<u8>>) {
    let mips = 32 - w.max(h).leading_zeros();
    let mut b = vec![0u8; 128];
    let mut put = |at: usize, v: u32| b[at..at + 4].copy_from_slice(&v.to_le_bytes());
    put(0, u32::from_le_bytes(*b"DDS "));
    put(4, 124);
    put(8, 0x1 | 0x2 | 0x4 | 0x1000 | 0x2_0000 | 0x8_0000); // CAPS|HEIGHT|WIDTH|PIXELFORMAT|MIPMAPCOUNT|LINEARSIZE
    put(12, h);
    put(16, w);
    let (bw, bh) = blocks(w, h, 0);
    put(20, (bw * bh * 8) as u32);
    put(28, mips);
    put(76, 32);
    put(80, 0x4); // DDPF_FOURCC
    put(84, u32::from_le_bytes(*b"DXT1"));
    put(108, 0x1000 | 0x8 | 0x40_0000); // TEXTURE|COMPLEX|MIPMAP
    if cube {
        put(112, 0x200 | 0xFC00); // CUBEMAP + all six faces
    }
    let mut subs = vec![];
    let mut n = 0u16;
    for _ in 0..if cube { 6 } else { 1 } {
        for m in 0..mips {
            let (bw, bh) = blocks(w, h, m);
            let mut s = vec![];
            for _ in 0..bw * bh {
                n += 1;
                s.extend(block(n));
            }
            b.extend(&s);
            subs.push(s);
        }
    }
    (b, subs)
}

/// Every subresource of the view's texture, rows of blocks packed, subresource order (face-major).
fn read_back(gpu: &Gpu, srv: &ID3D11ShaderResourceView, w: u32, h: u32) -> Vec<Vec<u8>> {
    // SAFETY: every object belongs to `gpu.device`; out-pointers are locals; each Map is paired with an Unmap.
    unsafe {
        let res: ID3D11Resource = srv.GetResource().unwrap();
        let tex: ID3D11Texture2D = res.cast().unwrap();
        let mut d = D3D11_TEXTURE2D_DESC::default();
        tex.GetDesc(&mut d);
        assert_eq!((d.Width, d.Height), (w, h));
        d.Usage = D3D11_USAGE_STAGING;
        d.BindFlags = 0;
        d.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
        let mut s = None;
        gpu.device.CreateTexture2D(&d, None, Some(&mut s)).unwrap();
        let s = s.unwrap();
        gpu.ctx.CopyResource(&s, &tex);
        let mut out = vec![];
        for sub in 0..d.MipLevels * d.ArraySize {
            let (bw, bh) = blocks(w, h, sub % d.MipLevels);
            let mut m = D3D11_MAPPED_SUBRESOURCE::default();
            gpu.ctx
                .Map(&s, sub, D3D11_MAP_READ, 0, Some(&mut m))
                .unwrap();
            let mut v = vec![];
            for r in 0..bh {
                let p = (m.pData as *const u8).add(r * m.RowPitch as usize);
                v.extend_from_slice(std::slice::from_raw_parts(p, bw * 8));
            }
            gpu.ctx.Unmap(&s, sub);
            out.push(v);
        }
        out
    }
}

fn check(w: u32, h: u32, cube: bool) {
    let gpu = Gpu::new_warp().unwrap();
    let (file, want) = dds_bc1_chain(w, h, cube);
    let t = load_dds(&gpu, &file).unwrap();
    let got = read_back(&gpu, &t.srv, w, h);
    assert_eq!(got.len(), want.len());
    let mips = want.len() / if cube { 6 } else { 1 };
    for (i, (g, e)) in got.iter().zip(&want).enumerate() {
        assert_eq!(g, e, "face {} mip {}", i / mips, i % mips);
    }
}

#[test]
fn a_bc1_mip_chain_uploads_every_level() {
    check(8, 8, false);
}

#[test]
fn a_non_square_bc1_mip_chain_uploads_every_level() {
    check(16, 8, false);
}

#[test]
fn a_bc1_cube_uploads_every_face_and_level() {
    check(8, 8, true);
}

//! Audit REPORT 2026-10-16 #13 (Note BD): `load_dds_as` per-face/per-mip pointer and pitch arithmetic on a BC1 file
//! with a full mip chain, 2D and cube. Every subresource is copied back from the GPU (WARP) and compared byte for
//! byte with the file, so a wrong face offset, a texel-instead-of-block pitch or a short read fails here.
use render3d::device::Gpu;
use render3d::textures::load_dds;
use windows::core::Interface;
use windows::Win32::Graphics::Direct3D11::{
    ID3D11Resource, ID3D11Texture2D, D3D11_CPU_ACCESS_READ, D3D11_MAPPED_SUBRESOURCE,
    D3D11_MAP_READ, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
};

/// One flat BC1 block: colour0 = colour1 = `c` (RGB565), every index 0.
fn block(c: u16) -> [u8; 8] {
    let [a, b] = c.to_le_bytes();
    [a, b, a, b, 0, 0, 0, 0]
}

/// The flat colour of (face, mip): distinct everywhere, so a face or mip read from the wrong offset shows.
fn colour(face: usize, mip: usize) -> u16 {
    0x0841 * (face * 4 + mip + 1) as u16
}

/// An 8×8 BC1 (DXT1) DDS with its four mips (8, 4, 2, 1), legacy header; `cube` = six faces, one chain each.
fn dds_bc1_chain(cube: bool) -> Vec<u8> {
    let mut b = vec![0u8; 128];
    let mut put = |at: usize, v: u32| b[at..at + 4].copy_from_slice(&v.to_le_bytes());
    put(0, u32::from_le_bytes(*b"DDS "));
    put(4, 124);
    put(8, 0x1 | 0x2 | 0x4 | 0x1000 | 0x2_0000 | 0x8_0000); // CAPS|HEIGHT|WIDTH|PIXELFORMAT|MIPMAPCOUNT|LINEARSIZE
    put(12, 8);
    put(16, 8);
    put(20, 32);
    put(28, 4);
    put(76, 32);
    put(80, 0x4); // DDPF_FOURCC
    put(84, u32::from_le_bytes(*b"DXT1"));
    put(108, 0x1000 | 0x8 | 0x40_0000); // TEXTURE|COMPLEX|MIPMAP
    if cube {
        put(112, 0x200 | 0xFC00); // CUBEMAP + all six faces
    }
    for face in 0..if cube { 6 } else { 1 } {
        for (mip, blocks) in [4, 1, 1, 1].into_iter().enumerate() {
            for _ in 0..blocks {
                b.extend(block(colour(face, mip)));
            }
        }
    }
    b
}

/// Every subresource of the view's texture as (rows of blocks) bytes, subresource order (face-major).
fn read_back(
    gpu: &Gpu,
    srv: &windows::Win32::Graphics::Direct3D11::ID3D11ShaderResourceView,
) -> Vec<Vec<u8>> {
    // SAFETY: every object belongs to `gpu.device`; out-pointers are locals; each Map is paired with an Unmap.
    unsafe {
        let res: ID3D11Resource = srv.GetResource().unwrap();
        let tex: ID3D11Texture2D = res.cast().unwrap();
        let mut d = D3D11_TEXTURE2D_DESC::default();
        tex.GetDesc(&mut d);
        assert_eq!((d.Width, d.Height, d.MipLevels), (8, 8, 4));
        d.Usage = D3D11_USAGE_STAGING;
        d.BindFlags = 0;
        d.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
        let mut s = None;
        gpu.device.CreateTexture2D(&d, None, Some(&mut s)).unwrap();
        let s = s.unwrap();
        gpu.ctx.CopyResource(&s, &tex);
        let mut out = vec![];
        for sub in 0..d.MipLevels * d.ArraySize {
            let size = (8u32 >> (sub % d.MipLevels)).max(1);
            let rows = size.div_ceil(4) as usize; // block rows; square, so also blocks per row
            let row_bytes = rows * 8;
            let mut m = D3D11_MAPPED_SUBRESOURCE::default();
            gpu.ctx
                .Map(&s, sub, D3D11_MAP_READ, 0, Some(&mut m))
                .unwrap();
            let mut v = vec![];
            for r in 0..rows {
                let p = (m.pData as *const u8).add(r * m.RowPitch as usize);
                v.extend_from_slice(std::slice::from_raw_parts(p, row_bytes));
            }
            gpu.ctx.Unmap(&s, sub);
            out.push(v);
        }
        out
    }
}

fn check(cube: bool) {
    let gpu = Gpu::new_warp().unwrap();
    let t = load_dds(&gpu, &dds_bc1_chain(cube)).unwrap();
    let subs = read_back(&gpu, &t.srv);
    assert_eq!(subs.len(), if cube { 24 } else { 4 });
    for (i, got) in subs.iter().enumerate() {
        let (face, mip) = (i / 4, i % 4);
        let want: Vec<u8> = std::iter::repeat_n(block(colour(face, mip)), [4, 1, 1, 1][mip])
            .flatten()
            .collect();
        assert_eq!(got, &want, "face {face} mip {mip}");
    }
}

#[test]
fn a_bc1_mip_chain_uploads_every_level() {
    check(false);
}

#[test]
fn a_bc1_cube_uploads_every_face_and_level() {
    check(true);
}

//! Environment cube from the HDRI (v2 Phase D, D5): CS's cube face convention (CaptureCommon.hlsli:48-76
//! GetSamplingVector), the yaw turn, the box mips, and the GPU's own cube addressing of the uploaded cube.
use render3d::composite::EnvMaps;
use render3d::hdri::{cube_dir, cube_faces, dir_to_uv, mip_chain, studio, Hdr};
use std::f32::consts::PI;

/// `w × h` image whose pixel (x, y) is (x, y, 0): the source pixel of any sample can be read back exactly.
fn index_image(w: usize, h: usize) -> Hdr {
    let mut rgb = Vec::with_capacity(w * h * 3);
    for y in 0..h {
        for x in 0..w {
            rgb.extend([x as f32, y as f32, 0.0]);
        }
    }
    Hdr {
        width: w,
        height: h,
        rgb,
    }
}

/// The environment turned by `yaw` seen along `d`: `Rz(-yaw)·d`, as `hdri::integrate` turns its axes.
fn turned(d: [f32; 3], yaw: f32) -> [f32; 3] {
    let (s, c) = (-yaw).sin_cos();
    [d[0] * c - d[1] * s, d[0] * s + d[1] * c, d[2]]
}

const AXES: [[f32; 3]; 6] = [
    [1.0, 0.0, 0.0],
    [-1.0, 0.0, 0.0],
    [0.0, 1.0, 0.0],
    [0.0, -1.0, 0.0],
    [0.0, 0.0, 1.0],
    [0.0, 0.0, -1.0],
];

#[test]
fn cube_faces_agree_with_dir_to_uv() {
    let img = index_image(16, 8);
    for yaw in [0.0f32, 1.0] {
        let faces = cube_faces(&img, 3, yaw);
        for (f, axis) in AXES.iter().enumerate() {
            let d = cube_dir(f, 1, 1, 3);
            assert_eq!(d, *axis, "face {f} centre");
            let (u, v) = dir_to_uv(turned(d, yaw));
            let (x, y) = (((u * 16.0) as usize).min(15), ((v * 8.0) as usize).min(7));
            let got = faces[f][4];
            if f >= 4 {
                // ±Z is a pole: every column is that direction, so only the row is defined.
                assert_eq!(got[1..], [y as f32, 0.0, 1.0], "face {f} yaw {yaw}");
            } else {
                assert_eq!(got, [x as f32, y as f32, 0.0, 1.0], "face {f} yaw {yaw}");
            }
        }
    }
}

/// A quarter turn: what face +X showed at texel (N-1-y, x) shows on +Y at (x, y), and +Z turns in place the same way
/// (Rz(-π/2) maps cube_dir(+Y, x, y) to cube_dir(+X, N-1-y, x)). Float rounding of the turn may move a sample to the
/// neighbouring source pixel: at most 1 pixel, counted.
#[test]
fn yaw_rotates_the_cube() {
    const N: u32 = 8;
    let img = index_image(studio().image.width, studio().image.height);
    let (a, b) = (cube_faces(&img, N, 0.0), cube_faces(&img, N, PI / 2.0));
    let w = img.width as f32;
    let mut moved = 0;
    for (from, to) in [(0usize, 2usize), (4, 4)] {
        for y in 0..N {
            for x in 0..N {
                let got = b[to][(y * N + x) as usize];
                let want = a[from][(x * N + (N - 1 - y)) as usize];
                let dx = (got[0] - want[0]).abs();
                let dx = dx.min(w - dx); // the azimuth wraps
                let dy = (got[1] - want[1]).abs();
                assert!(
                    dx <= 1.0 && dy <= 1.0,
                    "face {to} ({x}, {y}): {got:?} vs {want:?}"
                );
                moved += usize::from(got != want);
            }
        }
    }
    println!(
        "texels whose source moved by one pixel: {moved} of {}",
        2 * N * N
    );
}

#[test]
fn cube_dir_matches_cs_table() {
    const N: u32 = 4;
    let norm = |d: [f32; 3]| {
        let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        d.map(|c| c / l)
    };
    for (x, y) in [(0u32, 1u32), (3, 0), (2, 3), (1, 2)] {
        let st = ((x as f32 + 0.5) / N as f32, (y as f32 + 0.5) / N as f32);
        let uv = (2.0 * st.0 - 1.0, 2.0 * (1.0 - st.1) - 1.0);
        let table = [
            [1.0, uv.1, -uv.0],
            [-1.0, uv.1, uv.0],
            [uv.0, 1.0, -uv.1],
            [uv.0, -1.0, uv.1],
            [uv.0, uv.1, 1.0],
            [-uv.0, uv.1, -1.0],
        ];
        for (f, d) in table.iter().enumerate() {
            assert_eq!(cube_dir(f, x, y, N), norm(*d), "face {f} ({x}, {y})");
        }
    }
}

#[test]
fn mip_chain_halves_to_one_texel_and_keeps_the_mean() {
    let face: Vec<[f32; 4]> = (0..256 * 256)
        .map(|i| {
            let v = ((i * 7919) % 1000) as f32 / 1000.0;
            [v, 1.0 - v, v * v, 1.0]
        })
        .collect();
    let chain = mip_chain(&face, 256).unwrap();
    assert_eq!(chain.len(), 9);
    for (m, level) in chain.iter().enumerate() {
        assert_eq!(level.len(), (256 >> m) * (256 >> m), "mip {m}");
    }
    assert_eq!(chain[0], face);
    // Review M5: no panic in public API.
    assert!(mip_chain(&face[..100 * 100], 100).is_err());
    assert!(mip_chain(&face, 128).is_err(), "face is not 128²");
    assert!(mip_chain(&[], 0).is_err());
    let gpu = render3d::device::Gpu::new_warp().unwrap();
    assert!(EnvMaps::new(&gpu, &index_image(16, 8), 100, 0.0).is_err());
    assert!(EnvMaps::new(&gpu, &index_image(16, 8), 0, 0.0).is_err());
    for k in 0..4 {
        let mean = face.iter().map(|p| p[k] as f64).sum::<f64>() / face.len() as f64;
        let last = chain[8][0][k] as f64;
        assert!((last - mean).abs() < 1e-6, "ch {k}: {last} vs {mean}");
    }
}

/// `cube` sampled on the GPU (POINT, level 0) at the `cube_dir` of each (face, x, y) texel of an `n`² cube.
fn sample_cube(
    gpu: &render3d::device::Gpu,
    cube: &windows::Win32::Graphics::Direct3D11::ID3D11ShaderResourceView,
    texels: &[(usize, u32, u32)],
    n_size: u32,
) -> Vec<[f32; 4]> {
    use windows::Win32::Graphics::Direct3D11::*;
    let dirs: Vec<f32> = texels
        .iter()
        .flat_map(|&(f, x, y)| {
            let d = cube_dir(f, x, y, n_size);
            [d[0], d[1], d[2], 0.0]
        })
        .collect();
    let src = "TextureCube<float4> Env : register(t0);\nSamplerState Pt : register(s0);\n\
               StructuredBuffer<float4> Dirs : register(t1);\nRWStructuredBuffer<float4> Out : register(u0);\n\
               [numthreads(1, 1, 1)] void main(uint3 id : SV_DispatchThreadID) { Out[id.x] = Env.SampleLevel(Pt, Dirs[id.x].xyz, 0); }\n";
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("probe.hlsl"), src).unwrap();
    let s = render3d::source::SourceSet::new(dir.path().to_path_buf())
        .get(std::path::Path::new("probe.hlsl"))
        .unwrap();
    let c = render3d::compile::compile(
        &s,
        &render3d::permutation::Defines(vec![]),
        "cs_5_0",
        dir.path(),
    )
    .unwrap();
    let n = texels.len() as u32;
    let buf = |bind: D3D11_BIND_FLAG, usage: D3D11_USAGE, cpu: u32, init: Option<&[f32]>| {
        let d = D3D11_BUFFER_DESC {
            ByteWidth: n * 16,
            Usage: usage,
            BindFlags: bind.0 as u32,
            CPUAccessFlags: cpu,
            MiscFlags: if usage == D3D11_USAGE_STAGING {
                0
            } else {
                D3D11_RESOURCE_MISC_BUFFER_STRUCTURED.0 as u32
            },
            StructureByteStride: if usage == D3D11_USAGE_STAGING { 0 } else { 16 },
        };
        let data = init.map(|v| D3D11_SUBRESOURCE_DATA {
            pSysMem: v.as_ptr() as _,
            SysMemPitch: 0,
            SysMemSlicePitch: 0,
        });
        let mut b = None;
        unsafe {
            gpu.device
                .CreateBuffer(&d, data.as_ref().map(|x| x as *const _), Some(&mut b))
        }
        .unwrap();
        b.unwrap()
    };
    let dirs_buf = buf(
        D3D11_BIND_SHADER_RESOURCE,
        D3D11_USAGE_DEFAULT,
        0,
        Some(&dirs),
    );
    let out = buf(D3D11_BIND_UNORDERED_ACCESS, D3D11_USAGE_DEFAULT, 0, None);
    let staging = buf(
        D3D11_BIND_FLAG(0),
        D3D11_USAGE_STAGING,
        D3D11_CPU_ACCESS_READ.0 as u32,
        None,
    );
    let sd = D3D11_SAMPLER_DESC {
        Filter: D3D11_FILTER_MIN_MAG_MIP_POINT,
        AddressU: D3D11_TEXTURE_ADDRESS_CLAMP,
        AddressV: D3D11_TEXTURE_ADDRESS_CLAMP,
        AddressW: D3D11_TEXTURE_ADDRESS_CLAMP,
        MaxLOD: D3D11_FLOAT32_MAX,
        ..Default::default()
    };
    let (mut cs, mut smp, mut dsrv, mut uav) = (None, None, None, None);
    unsafe {
        gpu.device
            .CreateComputeShader(&c.bytecode, None, Some(&mut cs))
            .unwrap();
        gpu.device.CreateSamplerState(&sd, Some(&mut smp)).unwrap();
        gpu.device
            .CreateShaderResourceView(&dirs_buf, None, Some(&mut dsrv))
            .unwrap();
        gpu.device
            .CreateUnorderedAccessView(&out, None, Some(&mut uav))
            .unwrap();
        let ctx = &gpu.ctx;
        ctx.CSSetShader(cs.as_ref(), None);
        ctx.CSSetShaderResources(0, Some(&[Some(cube.clone()), dsrv.clone()]));
        ctx.CSSetSamplers(0, Some(&[smp.clone()]));
        ctx.CSSetUnorderedAccessViews(0, 1, Some(&uav), None);
        ctx.Dispatch(n, 1, 1);
        ctx.CopyResource(&staging, &out);
        let mut m = D3D11_MAPPED_SUBRESOURCE::default();
        ctx.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut m))
            .unwrap();
        let v = std::slice::from_raw_parts(m.pData as *const [f32; 4], n as usize).to_vec();
        ctx.Unmap(&staging, 0);
        ctx.ClearState();
        v
    }
}

/// The uploaded cube sampled by the GPU (POINT, level 0) at the `cube_dir` of off-centre texels of every face returns
/// that texel: catches face order and flips the CPU tests cannot see.
#[test]
fn env_cube_matches_d3d_cube_addressing() {
    const N: u32 = 4;
    let gpu = render3d::device::Gpu::new_warp().unwrap();
    let img = index_image(16, 8);
    let env = EnvMaps::new(&gpu, &img, N, 0.0).unwrap();
    let faces = cube_faces(&img, N, 0.0);
    let texels: Vec<(usize, u32, u32)> = (0..6)
        .flat_map(|f| [(f, 0, 1), (f, 3, 0), (f, 1, 3), (f, 2, 2)])
        .collect();
    let got = sample_cube(&gpu, &env.reflection, &texels, N);
    for (i, &(f, x, y)) in texels.iter().enumerate() {
        let want = faces[f][(y * N + x) as usize].map(|v| half::f16::from_f32(v).to_f32());
        assert_eq!(
            got[i].map(f32::to_bits),
            want.map(f32::to_bits),
            "face {f} ({x}, {y}): {:?} vs {want:?}",
            got[i]
        );
    }
}

/// Review M7: an R11G11B10 cube (the captured format) through `cube_srv`: 2², 2 mips, every mip-0 texel distinct
/// (R = 2^(f-2), G = 1 + i/64, B = 1); sampled back exactly.
#[test]
fn r11g11b10_cube_srv_samples_back_its_texels() {
    use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R11G11B10_FLOAT;
    let gpu = render3d::device::Gpu::new_warp().unwrap();
    let texel = |f: u32, i: u32| ((13 + f) << 6) | (((15 << 6) | i) << 11) | ((15 << 5) << 22);
    let mut data = vec![];
    for f in 0..6 {
        for i in 0..4 {
            data.extend(texel(f, i).to_le_bytes());
        }
        data.extend(0u32.to_le_bytes()); // mip 1
    }
    let srv =
        render3d::textures::cube_srv(&gpu, 2, 2, DXGI_FORMAT_R11G11B10_FLOAT, 4, &data).unwrap();
    let texels: Vec<(usize, u32, u32)> = (0..6)
        .flat_map(|f| [(f, 0, 0), (f, 1, 1), (f, 0, 1)])
        .collect();
    let got = sample_cube(&gpu, &srv, &texels, 2);
    for (k, &(f, x, y)) in texels.iter().enumerate() {
        let i = y * 2 + x;
        let want = [(f as f32 - 2.0).exp2(), 1.0 + i as f32 / 64.0, 1.0, 1.0];
        assert_eq!(got[k], want, "face {f} ({x}, {y})");
    }
}

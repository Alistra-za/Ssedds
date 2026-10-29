//! D3D11 device creation (each hardware adapter →
//! default hardware → WARP), an offscreen RGBA16F + D32 target, clear, and
//! RGBA8 readback.
//!
//! Review Focus #5: when no hardware device can be created (RDP session,
//! broken driver) [`Gpu::new`] falls back to WARP and sets [`Gpu::warp`] so
//! the GUI can show a "software renderer" notice. If WARP fails too, `new`
//! returns `Err` (the GUI shows "3D preview unavailable"); it never panics.
//! [`Target`] operations return `Err` rather than panicking too.
//!
//! **Device removal** (TDR, driver update, GPU unplugged): a failing
//! [`Target`] operation checks `GetDeviceRemovedReason`. When the device is
//! gone, the error starts with [`DEVICE_REMOVED`] and [`Gpu::is_removed`]
//! is true. On that error, drop the `Gpu` and everything made from it and
//! rebuild with [`Gpu::new`] (which may land on WARP this time).
//!
//! **Threading:** the immediate context is not thread-safe. Keep a `Gpu`
//! and its `Target`s on one thread (or serialise every use behind a lock).
//!
//! COM objects are released by scope (the `windows` crate's interface types
//! call `Release` on drop). Every `unsafe` block below is a plain D3D11/DXGI
//! call whose pointer arguments are locals that outlive the call.

use std::sync::OnceLock;
use windows::core::Interface;
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE, D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_UNKNOWN, D3D_DRIVER_TYPE_WARP,
    D3D_FEATURE_LEVEL_11_0,
};
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11DepthStencilView, ID3D11Device, ID3D11DeviceContext,
    ID3D11RenderTargetView, ID3D11ShaderResourceView, ID3D11Texture2D, ID3D11UnorderedAccessView,
    D3D11_BIND_DEPTH_STENCIL, D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE,
    D3D11_BIND_UNORDERED_ACCESS, D3D11_BOX, D3D11_CLEAR_DEPTH, D3D11_CPU_ACCESS_READ,
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_DEPTH_STENCIL_VIEW_DESC, D3D11_DSV_DIMENSION_TEXTURE2D,
    D3D11_MAPPED_SUBRESOURCE, D3D11_MAP_READ, D3D11_SDK_VERSION, D3D11_SHADER_RESOURCE_VIEW_DESC,
    D3D11_SHADER_RESOURCE_VIEW_DESC_0, D3D11_TEX2D_SRV, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT,
    D3D11_USAGE_STAGING,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT, DXGI_FORMAT_D32_FLOAT, DXGI_FORMAT_R10G10B10A2_UNORM, DXGI_FORMAT_R11G11B10_FLOAT,
    DXGI_FORMAT_R16G16B16A16_FLOAT, DXGI_FORMAT_R16G16_FLOAT, DXGI_FORMAT_R16_UNORM,
    DXGI_FORMAT_R32_FLOAT, DXGI_FORMAT_R32_TYPELESS, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, IDXGIAdapter, IDXGIAdapter1, IDXGIDevice, IDXGIFactory1,
    DXGI_ADAPTER_DESC1, DXGI_ADAPTER_FLAG_SOFTWARE,
};

/// Prefix of every error caused by a removed device; see the module docs.
pub const DEVICE_REMOVED: &str = "device removed";

/// A D3D11 device and its immediate context. Single-threaded: see the
/// module docs.
pub struct Gpu {
    pub device: ID3D11Device,
    pub ctx: ID3D11DeviceContext,
    /// True when rendering in software (WARP, or the Microsoft Basic Render
    /// Driver that RDP sessions hand out). The GUI shows a notice.
    pub warp: bool,
    /// Adapter description, e.g. "NVIDIA GeForce RTX 4090".
    pub adapter: String,
}

fn adapter_name(desc: &DXGI_ADAPTER_DESC1) -> String {
    let n = desc
        .Description
        .iter()
        .position(|&c| c == 0)
        .unwrap_or(desc.Description.len());
    String::from_utf16_lossy(&desc.Description[..n])
}

fn create(adapter: Option<&IDXGIAdapter>, driver: D3D_DRIVER_TYPE) -> Result<Gpu, String> {
    let mut device: Option<ID3D11Device> = None;
    let mut ctx: Option<ID3D11DeviceContext> = None;
    // SAFETY: out-pointers are locals that outlive the call; the feature
    // level slice is a temporary that lives for the statement.
    unsafe {
        D3D11CreateDevice(
            adapter,
            driver,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            Some(&[D3D_FEATURE_LEVEL_11_0]),
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            Some(&mut ctx),
        )
    }
    .map_err(|e| e.to_string())?;
    let device = device.ok_or("D3D11CreateDevice returned no device")?;
    let ctx = ctx.ok_or("D3D11CreateDevice returned no context")?;
    // Ask the device which adapter it actually landed on (HARDWARE with a
    // null adapter may give the Basic Render Driver under RDP).
    let desc = device
        .cast::<IDXGIDevice>()
        // SAFETY: plain COM getters on live interfaces.
        .and_then(|d| unsafe { d.GetAdapter() })
        .and_then(|a| a.cast::<IDXGIAdapter1>())
        .and_then(|a| unsafe { a.GetDesc1() })
        .ok();
    let software = desc
        .as_ref()
        .is_some_and(|d| d.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0);
    Ok(Gpu {
        device,
        ctx,
        warp: driver == D3D_DRIVER_TYPE_WARP || software,
        adapter: desc.as_ref().map(adapter_name).unwrap_or_default(),
    })
}

/// Hardware adapters (skipping software ones such as the Basic Render
/// Driver) from DXGI enumeration, in DXGI order.
fn hardware_adapters() -> Vec<(IDXGIAdapter1, DXGI_ADAPTER_DESC1)> {
    let mut out = Vec::new();
    // SAFETY: plain COM factory creation / enumeration.
    let Ok(factory) = (unsafe { CreateDXGIFactory1::<IDXGIFactory1>() }) else {
        return out;
    };
    for i in 0.. {
        // EnumAdapters1 fails with DXGI_ERROR_NOT_FOUND past the last one.
        let Ok(a) = (unsafe { factory.EnumAdapters1(i) }) else {
            break;
        };
        let Ok(desc) = (unsafe { a.GetDesc1() }) else {
            continue;
        };
        if desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 == 0 {
            out.push((a, desc));
        }
    }
    out
}

fn create_on(a: &IDXGIAdapter1) -> Result<Gpu, String> {
    let a = a.cast::<IDXGIAdapter>().map_err(|e| e.to_string())?;
    create(Some(&a), D3D_DRIVER_TYPE_UNKNOWN)
}

impl Gpu {
    /// Tries, in order:
    /// 1. every other enumerated hardware adapter, in DXGI order (so a real
    ///    GPU listed after adapter 0 is still used);
    /// 2. `D3D_DRIVER_TYPE_HARDWARE` on the default adapter;
    /// 3. WARP.
    ///
    /// The error lists every attempt's failure.
    pub fn new() -> Result<Gpu, String> {
        let mut errors = Vec::new();
        let adapters = hardware_adapters();
        for (a, d) in &adapters {
            match create_on(a) {
                Ok(g) => return Ok(g),
                Err(e) => errors.push(format!("adapter {}: {e}", adapter_name(d))),
            }
        }
        match create(None, D3D_DRIVER_TYPE_HARDWARE) {
            Ok(g) => return Ok(g),
            Err(e) => errors.push(format!("hardware: {e}")),
        }
        match Gpu::new_warp() {
            Ok(g) => Ok(g),
            Err(e) => {
                errors.push(e);
                Err(format!("3D preview unavailable: {}", errors.join("; ")))
            }
        }
    }

    /// Forces the WARP software rasteriser (ships with Windows).
    pub fn new_warp() -> Result<Gpu, String> {
        create(None, D3D_DRIVER_TYPE_WARP).map_err(|e| format!("WARP: {e}"))
    }

    /// `Some(reason)` once the device has been removed; rebuild the `Gpu`.
    pub fn removed_reason(&self) -> Option<String> {
        // SAFETY: plain COM getter on a live interface.
        unsafe { self.device.GetDeviceRemovedReason() }
            .err()
            .map(|e| e.to_string())
    }

    pub fn is_removed(&self) -> bool {
        self.removed_reason().is_some()
    }

    /// Error text for a failed `what`. It starts with [`DEVICE_REMOVED`]
    /// when the device is gone, so callers know to rebuild.
    pub(crate) fn fail(&self, what: &str, e: impl std::fmt::Display) -> String {
        fail_text(self.removed_reason(), what, e)
    }
}

/// [`Gpu::fail`]'s text for a device removed for `reason` (None: still live).
fn fail_text(reason: Option<String>, what: &str, e: impl std::fmt::Display) -> String {
    match reason {
        Some(reason) => format!("{DEVICE_REMOVED}: {reason} ({what}: {e})"),
        None => format!("{what}: {e}"),
    }
}

#[cfg(test)]
mod fail_tests {
    /// trace 2026-10-28 #37: a lost device's error starts with the prefix the GUI rebuilds on.
    #[test]
    fn a_removed_device_error_starts_with_the_prefix() {
        let lost = super::fail_text(Some("hung".into()), "CreateBlendState", "x");
        assert_eq!(lost, "device removed: hung (CreateBlendState: x)");
        assert!(lost.starts_with(super::DEVICE_REMOVED));
        assert_eq!(
            super::fail_text(None, "CreateBlendState", "x"),
            "CreateBlendState: x"
        );
    }
}

/// CS's deferred G-buffer, index = RT (Lighting.hlsl:290-301: Diffuse, MotionVectors, NormalGlossiness, Albedo,
/// Specular, Reflectance, Masks, Masks2). Formats as the golden captures' mrt.json lists them; names as there and
/// in [`crate::golden::format_tol`].
pub const MRT_FORMATS: [(DXGI_FORMAT, &str); 8] = [
    (DXGI_FORMAT_R16G16B16A16_FLOAT, "R16G16B16A16_FLOAT"),
    (DXGI_FORMAT_R16G16_FLOAT, "R16G16_FLOAT"),
    (DXGI_FORMAT_R10G10B10A2_UNORM, "R10G10B10A2_UNORM"),
    (DXGI_FORMAT_R10G10B10A2_UNORM, "R10G10B10A2_UNORM"),
    (DXGI_FORMAT_R11G11B10_FLOAT, "R11G11B10_FLOAT"),
    (DXGI_FORMAT_R11G11B10_FLOAT, "R11G11B10_FLOAT"),
    (DXGI_FORMAT_R11G11B10_FLOAT, "R11G11B10_FLOAT"),
    (DXGI_FORMAT_R16_UNORM, "R16_UNORM"),
];

/// Offscreen render target: RGBA16F colour + D32 depth. Forward writes `SV_Target0` only; a deferred target
/// ([`Target::new_deferred`]) adds RT1-7 and `color` is RT0.
pub struct Target {
    pub width: u32,
    pub height: u32,
    pub color: ID3D11Texture2D,
    pub rtv: ID3D11RenderTargetView,
    pub depth: ID3D11Texture2D,
    pub dsv: ID3D11DepthStencilView,
    /// CPU-readable copy target for the readback, created with the target (03 §7).
    pub staging: ID3D11Texture2D,
    /// Deferred only: RT0..RT7 (texture, RTV, SRV) in [`MRT_FORMATS`] order; mrt[0] is `color`. Empty = forward
    /// target.
    pub mrt: Vec<(
        ID3D11Texture2D,
        ID3D11RenderTargetView,
        ID3D11ShaderResourceView,
    )>,
    /// Deferred only: `color` as a UAV. CS's MainRW is RT0 itself (composite.json ResourceId::879).
    pub main_uav: Option<ID3D11UnorderedAccessView>,
    /// Deferred only: `depth` as an R32_FLOAT SRV, the composite's DepthTexture (D5; the depth is R32_TYPELESS there).
    pub depth_srv: Option<ID3D11ShaderResourceView>,
}

fn tex2d_desc(w: u32, h: u32, format: DXGI_FORMAT) -> D3D11_TEXTURE2D_DESC {
    D3D11_TEXTURE2D_DESC {
        Width: w,
        Height: h,
        MipLevels: 1,
        ArraySize: 1,
        Format: format,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: 0,
        CPUAccessFlags: 0,
        MiscFlags: 0,
    }
}

fn create_tex(gpu: &Gpu, desc: &D3D11_TEXTURE2D_DESC) -> Result<ID3D11Texture2D, String> {
    let mut t = None;
    // SAFETY: `desc` and the out-pointer are live locals.
    unsafe { gpu.device.CreateTexture2D(desc, None, Some(&mut t)) }
        .map_err(|e| gpu.fail("CreateTexture2D", e))?;
    t.ok_or_else(|| gpu.fail("CreateTexture2D", "no texture returned"))
}

/// half → u8: round-to-nearest of clamp(v, 0, 1) × 255 (0.5 → 128; NaN → 0).
fn unorm8(bits: u16) -> u8 {
    let v = half::f16::from_bits(bits).to_f32();
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// half (linear) → u8 sRGB: IEC 61966-2-1 encode of clamp(v, 0, 1), then round (NaN → 0). Encoding before the
/// 8-bit quantisation keeps the dark steps that a u8-linear → sRGB table would merge.
fn srgb8(bits: u16) -> u8 {
    let v = half::f16::from_bits(bits).to_f32();
    if v.is_nan() {
        return 0;
    }
    let l = v.clamp(0.0, 1.0);
    let s = engine::ops::linear_to_srgb(l);
    (s * 255.0).round() as u8
}

/// `f(bits)` for every half bit pattern, built once: the readback is one table lookup per channel.
fn table(cell: &'static OnceLock<Vec<u8>>, f: fn(u16) -> u8) -> &'static [u8] {
    cell.get_or_init(|| (0..=u16::MAX).map(f).collect())
}
static UNORM8: OnceLock<Vec<u8>> = OnceLock::new();
static SRGB8: OnceLock<Vec<u8>> = OnceLock::new();

/// Bytes per texel of an [`MRT_FORMATS`] format.
fn texel_bytes(f: DXGI_FORMAT) -> usize {
    match f {
        DXGI_FORMAT_R16G16B16A16_FLOAT => 8,
        DXGI_FORMAT_R16_UNORM => 2,
        _ => 4,
    }
}

/// One texel → RGBA f32 the way the golden extractors write `.f32`: missing channels 0, missing alpha 1.
fn decode(f: DXGI_FORMAT, t: &[u8]) -> [f32; 4] {
    let h = |i: usize| half::f16::from_le_bytes([t[i], t[i + 1]]).to_f32();
    let word = || u32::from_le_bytes([t[0], t[1], t[2], t[3]]);
    // 11- and 10-bit floats share half's exponent; their mantissa is half's top 6 (5) bits.
    let small = |bits: u32| half::f16::from_bits(bits as u16).to_f32();
    match f {
        DXGI_FORMAT_R16G16B16A16_FLOAT => [h(0), h(2), h(4), h(6)],
        DXGI_FORMAT_R16G16_FLOAT => [h(0), h(2), 0.0, 1.0],
        DXGI_FORMAT_R10G10B10A2_UNORM => {
            let v = word();
            let c = |s: u32| ((v >> s) & 0x3ff) as f32 / 1023.0;
            [c(0), c(10), c(20), (v >> 30) as f32 / 3.0]
        }
        DXGI_FORMAT_R11G11B10_FLOAT => {
            let v = word();
            [
                small((v & 0x7ff) << 4),
                small(((v >> 11) & 0x7ff) << 4),
                small(((v >> 22) & 0x3ff) << 5),
                1.0,
            ]
        }
        DXGI_FORMAT_R16_UNORM => [
            u16::from_le_bytes([t[0], t[1]]) as f32 / 65535.0,
            0.0,
            0.0,
            1.0,
        ],
        _ => [0.0; 4],
    }
}

/// RGBA f32 → one texel of `f`, appended to `out`; exact for any value [`decode`] produced from that format.
fn encode(f: DXGI_FORMAT, c: [f32; 4], out: &mut Vec<u8>) {
    let unorm = |x: f32, max: f32| (x.clamp(0.0, 1.0) * max).round() as u32;
    let small = |x: f32, shift: u32| u32::from(half::f16::from_f32(x.max(0.0)).to_bits()) >> shift;
    let halves = |n: usize| {
        c[..n]
            .iter()
            .flat_map(|x| half::f16::from_f32(*x).to_le_bytes())
    };
    match f {
        DXGI_FORMAT_R16G16B16A16_FLOAT => out.extend(halves(4)),
        DXGI_FORMAT_R16G16_FLOAT => out.extend(halves(2)),
        DXGI_FORMAT_R10G10B10A2_UNORM => out.extend(
            (unorm(c[0], 1023.0)
                | (unorm(c[1], 1023.0) << 10)
                | (unorm(c[2], 1023.0) << 20)
                | (unorm(c[3], 3.0) << 30))
                .to_le_bytes(),
        ),
        DXGI_FORMAT_R11G11B10_FLOAT => out.extend(
            ((small(c[0], 4) & 0x7ff)
                | ((small(c[1], 4) & 0x7ff) << 11)
                | ((small(c[2], 5) & 0x3ff) << 22))
                .to_le_bytes(),
        ),
        DXGI_FORMAT_R16_UNORM => out.extend((unorm(c[0], 65535.0) as u16).to_le_bytes()),
        _ => {}
    }
}

impl Target {
    /// Creates the colour and depth textures and their views. A removed
    /// device gives an error starting with [`DEVICE_REMOVED`].
    pub fn new(gpu: &Gpu, w: u32, h: u32) -> Result<Target, String> {
        Target::build(gpu, w, h, false)
    }

    /// A deferred target: [`Target::new`] plus RT1-7 in [`MRT_FORMATS`], an SRV on every RT, and `color` (RT0)
    /// also bound as a UAV. All textures are made together, so a resize or toggle never mixes sizes.
    pub fn new_deferred(gpu: &Gpu, w: u32, h: u32) -> Result<Target, String> {
        Target::build(gpu, w, h, true)
    }

    /// True for a [`Target::new_deferred`] target.
    pub fn deferred(&self) -> bool {
        !self.mrt.is_empty()
    }

    /// The colour RTVs to bind: RT0..RT7 when deferred, else RT0 alone.
    pub fn rtvs(&self) -> Vec<Option<ID3D11RenderTargetView>> {
        if self.deferred() {
            self.mrt.iter().map(|m| Some(m.1.clone())).collect()
        } else {
            vec![Some(self.rtv.clone())]
        }
    }

    fn build(gpu: &Gpu, w: u32, h: u32, deferred: bool) -> Result<Target, String> {
        let mut cd = tex2d_desc(w, h, DXGI_FORMAT_R16G16B16A16_FLOAT);
        cd.BindFlags = D3D11_BIND_RENDER_TARGET.0 as u32;
        if deferred {
            cd.BindFlags |= (D3D11_BIND_SHADER_RESOURCE.0 | D3D11_BIND_UNORDERED_ACCESS.0) as u32;
        }
        let color = create_tex(gpu, &cd)?;
        // Deferred: typeless, so the composite can read it as R32_FLOAT (D5 R6); the DSV is D32_FLOAT either way.
        let mut dd = tex2d_desc(
            w,
            h,
            if deferred {
                DXGI_FORMAT_R32_TYPELESS
            } else {
                DXGI_FORMAT_D32_FLOAT
            },
        );
        dd.BindFlags = D3D11_BIND_DEPTH_STENCIL.0 as u32;
        if deferred {
            dd.BindFlags |= D3D11_BIND_SHADER_RESOURCE.0 as u32;
        }
        let depth = create_tex(gpu, &dd)?;
        let dsv_desc = D3D11_DEPTH_STENCIL_VIEW_DESC {
            Format: DXGI_FORMAT_D32_FLOAT,
            ViewDimension: D3D11_DSV_DIMENSION_TEXTURE2D,
            ..Default::default()
        };
        let mut sd = tex2d_desc(w, h, DXGI_FORMAT_R16G16B16A16_FLOAT);
        sd.Usage = D3D11_USAGE_STAGING;
        sd.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
        let staging = create_tex(gpu, &sd)?;
        let (mut rtv, mut dsv) = (None, None);
        // SAFETY: live resources and local out-pointers.
        unsafe {
            gpu.device
                .CreateRenderTargetView(&color, None, Some(&mut rtv))
                .map_err(|e| gpu.fail("CreateRenderTargetView", e))?;
            gpu.device
                .CreateDepthStencilView(&depth, Some(&dsv_desc), Some(&mut dsv))
                .map_err(|e| gpu.fail("CreateDepthStencilView", e))?;
        }
        let rtv = rtv.ok_or_else(|| gpu.fail("CreateRenderTargetView", "no view returned"))?;
        let (mut mrt, mut main_uav, mut depth_srv) = (Vec::new(), None, None);
        if deferred {
            for (i, (format, _)) in MRT_FORMATS.iter().enumerate() {
                let (tex, view) = if i == 0 {
                    (color.clone(), rtv.clone())
                } else {
                    let mut d = tex2d_desc(w, h, *format);
                    d.BindFlags =
                        (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32;
                    let tex = create_tex(gpu, &d)?;
                    let mut v = None;
                    // SAFETY: live resource and local out-pointer.
                    unsafe { gpu.device.CreateRenderTargetView(&tex, None, Some(&mut v)) }
                        .map_err(|e| gpu.fail("CreateRenderTargetView", e))?;
                    let v =
                        v.ok_or_else(|| gpu.fail("CreateRenderTargetView", "no view returned"))?;
                    (tex, v)
                };
                let mut srv = None;
                // SAFETY: live resource and local out-pointer.
                unsafe {
                    gpu.device
                        .CreateShaderResourceView(&tex, None, Some(&mut srv))
                }
                .map_err(|e| gpu.fail("CreateShaderResourceView", e))?;
                let srv =
                    srv.ok_or_else(|| gpu.fail("CreateShaderResourceView", "no view returned"))?;
                mrt.push((tex, view, srv));
            }
            // SAFETY: live resource and local out-pointer.
            unsafe {
                gpu.device
                    .CreateUnorderedAccessView(&color, None, Some(&mut main_uav))
            }
            .map_err(|e| gpu.fail("CreateUnorderedAccessView", e))?;
            if main_uav.is_none() {
                return Err(gpu.fail("CreateUnorderedAccessView", "no view returned"));
            }
            let sd = D3D11_SHADER_RESOURCE_VIEW_DESC {
                Format: DXGI_FORMAT_R32_FLOAT,
                ViewDimension: windows::Win32::Graphics::Direct3D::D3D11_SRV_DIMENSION_TEXTURE2D,
                Anonymous: D3D11_SHADER_RESOURCE_VIEW_DESC_0 {
                    Texture2D: D3D11_TEX2D_SRV {
                        MostDetailedMip: 0,
                        MipLevels: 1,
                    },
                },
            };
            // SAFETY: live resource, descriptor and out-pointer are locals.
            unsafe {
                gpu.device
                    .CreateShaderResourceView(&depth, Some(&sd), Some(&mut depth_srv))
            }
            .map_err(|e| gpu.fail("CreateShaderResourceView (depth)", e))?;
            if depth_srv.is_none() {
                return Err(gpu.fail("CreateShaderResourceView (depth)", "no view returned"));
            }
        }
        Ok(Target {
            width: w,
            height: h,
            color,
            rtv,
            depth,
            dsv: dsv.ok_or_else(|| gpu.fail("CreateDepthStencilView", "no view returned"))?,
            staging,
            mrt,
            main_uav,
            depth_srv,
        })
    }

    /// Clears colour to `rgba`, RT1-7 to 0 (CS Masks2 0 = no occlusion) and depth to 1.0.
    pub fn clear(&self, gpu: &Gpu, rgba: [f32; 4]) {
        // SAFETY: views belong to `gpu.device`; `rgba` outlives the call.
        unsafe {
            gpu.ctx.ClearRenderTargetView(&self.rtv, &rgba);
            for m in self.mrt.iter().skip(1) {
                gpu.ctx.ClearRenderTargetView(&m.1, &[0.0; 4]);
            }
            gpu.ctx
                .ClearDepthStencilView(&self.dsv, D3D11_CLEAR_DEPTH.0, 1.0, 0);
        }
    }

    /// The frame for display: as the plain readback, but alpha is 255 everywhere. Opaque shapes write
    /// their diffuse alpha into the target (CS `psout.Diffuse.w = baseColor.w`); the frame is already drawn
    /// over the clear colour, so its alpha must not make the viewer see through an opaque mesh.
    pub fn readback_display(&self, gpu: &Gpu, srgb: bool) -> Result<Vec<u8>, String> {
        self.read(gpu, srgb, true)
    }

    fn read(&self, gpu: &Gpu, srgb: bool, opaque: bool) -> Result<Vec<u8>, String> {
        let colour = if srgb {
            table(&SRGB8, srgb8)
        } else {
            table(&UNORM8, unorm8)
        };
        let alpha = table(&UNORM8, unorm8);
        let alpha = |bits: usize| if opaque { 255 } else { alpha[bits] };
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        // SAFETY: both resources belong to `gpu.device`; `mapped` is a local.
        unsafe {
            gpu.ctx.CopyResource(&self.staging, &self.color);
            gpu.ctx
                .Map(&self.staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
                .map_err(|e| gpu.fail("Map", e))?;
        }
        let (w, h) = (self.width as usize, self.height as usize);
        let pitch = mapped.RowPitch as usize;
        let mut out = Vec::with_capacity(w * h * 4);
        if !mapped.pData.is_null() {
            for y in 0..h {
                // SAFETY: while mapped, `pData` points to `h` rows spaced
                // `RowPitch` bytes apart, each holding at least `w × 8` bytes
                // (4 × f16). Read as bytes, so no alignment assumption.
                let row = unsafe {
                    std::slice::from_raw_parts((mapped.pData as *const u8).add(y * pitch), w * 8)
                };
                out.extend(row.as_chunks::<2>().0.iter().enumerate().map(|(i, c)| {
                    let bits = u16::from_le_bytes(*c) as usize;
                    if i % 4 == 3 {
                        alpha(bits)
                    } else {
                        colour[bits]
                    }
                }));
            }
        }
        // SAFETY: paired with the successful Map above; nothing between
        // them can return early.
        unsafe { gpu.ctx.Unmap(&self.staging, 0) };
        if out.len() != w * h * 4 {
            return Err(gpu.fail("Map", "null pointer returned"));
        }
        Ok(out)
    }

    fn check_crop(&self, crop: [u32; 4]) -> Result<(), String> {
        let [x, y, w, h] = crop.map(u64::from);
        if x + w > self.width.into() || y + h > self.height.into() {
            return Err(format!(
                "crop {crop:?} outside the {}x{} target",
                self.width, self.height
            ));
        }
        Ok(())
    }

    /// RT `rt` (0 = `color`; 1-7 only on a deferred target) and its format.
    fn rt(&self, rt: usize) -> Result<(&ID3D11Texture2D, DXGI_FORMAT), String> {
        match rt {
            0 => Ok((&self.color, MRT_FORMATS[0].0)),
            _ if rt < self.mrt.len() => Ok((&self.mrt[rt].0, MRT_FORMATS[rt].0)),
            _ => Err(format!(
                "no RT{rt}: the target has {}",
                self.mrt.len().max(1)
            )),
        }
    }

    /// RT `rt` over `crop` ([x, y, w, h]) as row-major f32 RGBA, decoded like the golden extractors (half → f32
    /// unclamped; UNORM / small floats per [`MRT_FORMATS`]; missing channels 0, missing alpha 1): the layout of a
    /// golden fixture's `mrt<N>.f32`.
    pub fn read_f32(&self, gpu: &Gpu, rt: usize, crop: [u32; 4]) -> Result<Vec<f32>, String> {
        let (src, format) = self.rt(rt)?;
        self.check_crop(crop)?;
        let [x, y, w, h] = crop;
        let mut sd = tex2d_desc(w, h, format);
        sd.Usage = D3D11_USAGE_STAGING;
        sd.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
        let staging = create_tex(gpu, &sd)?;
        let b = D3D11_BOX {
            left: x,
            top: y,
            front: 0,
            right: x + w,
            bottom: y + h,
            back: 1,
        };
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        // SAFETY: both resources belong to `gpu.device`; `b` and `mapped` are locals; the box lies inside (checked).
        unsafe {
            gpu.ctx
                .CopySubresourceRegion(&staging, 0, 0, 0, 0, src, 0, Some(&b));
            gpu.ctx
                .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
                .map_err(|e| gpu.fail("Map", e))?;
        }
        let (w, h, tb) = (w as usize, h as usize, texel_bytes(format));
        let pitch = mapped.RowPitch as usize;
        let mut out = Vec::with_capacity(w * h * 4);
        if !mapped.pData.is_null() {
            for row in 0..h {
                // SAFETY: while mapped, `pData` holds `h` rows `RowPitch` bytes apart, each `w` texels of `tb`
                // bytes. Read as bytes, so no alignment assumption.
                let px = unsafe {
                    std::slice::from_raw_parts((mapped.pData as *const u8).add(row * pitch), w * tb)
                };
                out.extend(px.chunks_exact(tb).flat_map(|t| decode(format, t)));
            }
        }
        // SAFETY: paired with the successful Map above.
        unsafe { gpu.ctx.Unmap(&staging, 0) };
        if out.len() != w * h * 4 {
            return Err(gpu.fail("Map", "null pointer returned"));
        }
        Ok(out)
    }

    /// Writes `rgba` (row-major f32 RGBA, `crop` = [x, y, w, h]) into RT `rt`, encoded in its format: a golden
    /// fixture's `mrt<N>_before.f32` as the draw's starting contents.
    pub fn seed_f32(
        &self,
        gpu: &Gpu,
        rt: usize,
        crop: [u32; 4],
        rgba: &[f32],
    ) -> Result<(), String> {
        let (dst, format) = self.rt(rt)?;
        self.check_crop(crop)?;
        let [x, y, w, h] = crop;
        if rgba.len() != (w * h * 4) as usize {
            return Err(format!("seed: {} floats for a {w}x{h} crop", rgba.len()));
        }
        let tb = texel_bytes(format);
        let mut bytes = Vec::with_capacity((w * h) as usize * tb);
        for px in rgba.as_chunks::<4>().0 {
            encode(format, *px, &mut bytes);
        }
        let b = D3D11_BOX {
            left: x,
            top: y,
            front: 0,
            right: x + w,
            bottom: y + h,
            back: 1,
        };
        // SAFETY: `bytes` holds `h` rows of `w × tb` bytes and outlives the call; the box lies inside (checked).
        unsafe {
            gpu.ctx
                .UpdateSubresource(dst, 0, Some(&b), bytes.as_ptr() as _, w * tb as u32, 0)
        };
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::unorm8;
    use half::f16;

    #[test]
    fn half_to_srgb8_encodes_before_quantising() {
        use super::srgb8;
        let b = |v: f32| f16::from_f32(v).to_bits();
        assert_eq!(srgb8(b(0.0)), 0);
        assert_eq!(srgb8(b(1.0)), 255);
        assert_eq!(srgb8(b(4.0)), 255);
        assert_eq!(srgb8(b(-1.0)), 0);
        assert_eq!(srgb8(f16::NAN.to_bits()), 0);
        // 0.5 linear = 0.7354 sRGB.
        assert_eq!(srgb8(b(0.5)), 188);
        // Dark tones keep their steps (a u8-linear LUT would collapse 0.001 and 0.002 to 0).
        assert!(srgb8(b(0.002)) > srgb8(b(0.001)));
        assert!(srgb8(b(0.001)) > 0);
    }

    #[test]
    fn half_to_unorm8_rounds_and_clamps() {
        let b = |v: f32| f16::from_f32(v).to_bits();
        assert_eq!(unorm8(b(0.5)), 128);
        assert_eq!(unorm8(b(1.0)), 255);
        assert_eq!(unorm8(b(2.0)), 255);
        assert_eq!(unorm8(b(-1.0)), 0);
        assert_eq!(unorm8(f16::NAN.to_bits()), 0);
    }

    #[test]
    fn mrt_texels_decode_like_the_extractors() {
        use super::{decode, encode, texel_bytes};
        use windows::Win32::Graphics::Dxgi::Common::*;
        let h = |v: f32| f16::from_f32(v).to_le_bytes();
        let rg16 = [h(0.25), h(0.5)].concat();
        let samples: [(DXGI_FORMAT, Vec<u8>, [f32; 4]); 5] = [
            (
                DXGI_FORMAT_R10G10B10A2_UNORM,
                (1023u32 | 512 << 20 | 3 << 30).to_le_bytes().to_vec(),
                [1.0, 0.0, 512.0 / 1023.0, 1.0],
            ),
            (
                DXGI_FORMAT_R11G11B10_FLOAT,
                (0x3C0u32 | 0x3C0 << 11 | 0x1E0 << 22)
                    .to_le_bytes()
                    .to_vec(),
                [1.0; 4],
            ),
            (
                DXGI_FORMAT_R11G11B10_FLOAT,
                0x380u32.to_le_bytes().to_vec(),
                [0.5, 0.0, 0.0, 1.0],
            ),
            (
                DXGI_FORMAT_R16_UNORM,
                0xFFFFu16.to_le_bytes().to_vec(),
                [1.0, 0.0, 0.0, 1.0],
            ),
            (DXGI_FORMAT_R16G16_FLOAT, rg16, [0.25, 0.5, 0.0, 1.0]),
        ];
        for (f, texel, want) in samples {
            assert_eq!(texel.len(), texel_bytes(f), "{f:?}");
            let got = decode(f, &texel);
            assert_eq!(got, want, "{f:?}");
            let mut out = Vec::new();
            encode(f, got, &mut out);
            assert_eq!(out, texel, "{f:?} round trip");
        }
        // RGBA16F: four halves, exact.
        let px = [h(0.75), h(-2.0), h(4.0), h(1.0)].concat();
        assert_eq!(
            decode(DXGI_FORMAT_R16G16B16A16_FLOAT, &px),
            [0.75, -2.0, 4.0, 1.0]
        );
        let mut out = Vec::new();
        encode(
            DXGI_FORMAT_R16G16B16A16_FLOAT,
            [0.75, -2.0, 4.0, 1.0],
            &mut out,
        );
        assert_eq!(out, px);
    }
}

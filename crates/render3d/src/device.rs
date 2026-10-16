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
    ID3D11RenderTargetView, ID3D11Texture2D, D3D11_BIND_DEPTH_STENCIL, D3D11_BIND_RENDER_TARGET,
    D3D11_CLEAR_DEPTH, D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT,
    D3D11_MAPPED_SUBRESOURCE, D3D11_MAP_READ, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC,
    D3D11_USAGE_DEFAULT, D3D11_USAGE_STAGING,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT, DXGI_FORMAT_D32_FLOAT, DXGI_FORMAT_R16G16B16A16_FLOAT, DXGI_SAMPLE_DESC,
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
        match self.removed_reason() {
            Some(reason) => format!("{DEVICE_REMOVED}: {reason} ({what}: {e})"),
            None => format!("{what}: {e}"),
        }
    }
}

/// Offscreen render target: RGBA16F colour + D32 depth. P0 forward writes
/// `SV_Target0` only.
pub struct Target {
    pub width: u32,
    pub height: u32,
    pub color: ID3D11Texture2D,
    pub rtv: ID3D11RenderTargetView,
    pub depth: ID3D11Texture2D,
    pub dsv: ID3D11DepthStencilView,
    /// CPU-readable copy target for the readback, created with the target (03 §7).
    pub staging: ID3D11Texture2D,
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

impl Target {
    /// Creates the colour and depth textures and their views. A removed
    /// device gives an error starting with [`DEVICE_REMOVED`].
    pub fn new(gpu: &Gpu, w: u32, h: u32) -> Result<Target, String> {
        let mut cd = tex2d_desc(w, h, DXGI_FORMAT_R16G16B16A16_FLOAT);
        cd.BindFlags = D3D11_BIND_RENDER_TARGET.0 as u32;
        let color = create_tex(gpu, &cd)?;
        let mut dd = tex2d_desc(w, h, DXGI_FORMAT_D32_FLOAT);
        dd.BindFlags = D3D11_BIND_DEPTH_STENCIL.0 as u32;
        let depth = create_tex(gpu, &dd)?;
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
                .CreateDepthStencilView(&depth, None, Some(&mut dsv))
                .map_err(|e| gpu.fail("CreateDepthStencilView", e))?;
        }
        Ok(Target {
            width: w,
            height: h,
            color,
            rtv: rtv.ok_or_else(|| gpu.fail("CreateRenderTargetView", "no view returned"))?,
            depth,
            dsv: dsv.ok_or_else(|| gpu.fail("CreateDepthStencilView", "no view returned"))?,
            staging,
        })
    }

    /// Clears colour to `rgba` and depth to 1.0.
    pub fn clear(&self, gpu: &Gpu, rgba: [f32; 4]) {
        // SAFETY: views belong to `gpu.device`; `rgba` outlives the call.
        unsafe {
            gpu.ctx.ClearRenderTargetView(&self.rtv, &rgba);
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
}

//! D3D11 device creation (hardware adapter by LUID → any hardware → WARP),
//! an offscreen RGBA16F + D32 target, clear, and RGBA8 readback.
//!
//! Review Focus #5: when no hardware device can be created (RDP session,
//! broken driver) [`Gpu::new`] falls back to WARP and sets [`Gpu::warp`] so
//! the GUI can show a "software renderer" notice. If WARP fails too, `new`
//! returns `Err` (the GUI shows "3D preview unavailable"); it never panics.
//!
//! COM objects are released by scope (the `windows` crate's interface types
//! call `Release` on drop). Every `unsafe` block below is a plain D3D11/DXGI
//! call whose pointer arguments are locals that outlive the call.

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

/// A D3D11 device and its immediate context.
pub struct Gpu {
    pub device: ID3D11Device,
    pub ctx: ID3D11DeviceContext,
    /// True when rendering in software (WARP, or the Microsoft Basic Render
    /// Driver that RDP sessions hand out). The GUI shows a notice.
    pub warp: bool,
    /// Adapter description, e.g. "NVIDIA GeForce RTX 4090".
    pub adapter: String,
}

/// `LUID` as 8 bytes: `LowPart` little-endian, then `HighPart` little-endian.
/// Callers that pass `prefer_luid` (e.g. wgpu's adapter LUID) use this order.
pub fn luid_bytes(desc: &DXGI_ADAPTER_DESC1) -> [u8; 8] {
    let mut b = [0u8; 8];
    b[..4].copy_from_slice(&desc.AdapterLuid.LowPart.to_le_bytes());
    b[4..].copy_from_slice(&desc.AdapterLuid.HighPart.to_le_bytes());
    b
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

/// Hardware adapters (non-software) from DXGI enumeration.
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

/// Hardware adapters as `(description, LUID bytes)`, in DXGI order.
pub fn hardware_adapter_list() -> Vec<(String, [u8; 8])> {
    hardware_adapters()
        .iter()
        .map(|(_, d)| (adapter_name(d), luid_bytes(d)))
        .collect()
}

impl Gpu {
    /// Hardware adapter whose LUID matches `prefer_luid` (see [`luid_bytes`]
    /// for the byte order), then any hardware adapter, then WARP. The error
    /// lists every attempt's failure.
    pub fn new(prefer_luid: Option<[u8; 8]>) -> Result<Gpu, String> {
        let mut errors = Vec::new();
        if let Some(want) = prefer_luid {
            match hardware_adapters()
                .into_iter()
                .find(|(_, d)| luid_bytes(d) == want)
            {
                Some((a, d)) => match a.cast::<IDXGIAdapter>() {
                    Ok(a) => match create(Some(&a), D3D_DRIVER_TYPE_UNKNOWN) {
                        Ok(g) => return Ok(g),
                        Err(e) => errors.push(format!("adapter {}: {e}", adapter_name(&d))),
                    },
                    Err(e) => errors.push(format!("adapter {}: {e}", adapter_name(&d))),
                },
                None => errors.push(format!("no hardware adapter with LUID {want:02x?}")),
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
    unsafe { gpu.device.CreateTexture2D(desc, None, Some(&mut t)) }.map_err(|e| e.to_string())?;
    t.ok_or_else(|| "CreateTexture2D returned no texture".into())
}

/// half → u8: round-to-nearest of clamp(v, 0, 1) × 255 (0.5 → 128; NaN → 0).
fn unorm8(bits: u16) -> u8 {
    let v = half::f16::from_bits(bits).to_f32();
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

impl Target {
    /// Panics if the device cannot create the textures; see [`Target::try_new`].
    pub fn new(gpu: &Gpu, w: u32, h: u32) -> Target {
        Target::try_new(gpu, w, h).expect("create offscreen target")
    }

    pub fn try_new(gpu: &Gpu, w: u32, h: u32) -> Result<Target, String> {
        let mut cd = tex2d_desc(w, h, DXGI_FORMAT_R16G16B16A16_FLOAT);
        cd.BindFlags = D3D11_BIND_RENDER_TARGET.0 as u32;
        let color = create_tex(gpu, &cd)?;
        let mut dd = tex2d_desc(w, h, DXGI_FORMAT_D32_FLOAT);
        dd.BindFlags = D3D11_BIND_DEPTH_STENCIL.0 as u32;
        let depth = create_tex(gpu, &dd)?;
        let (mut rtv, mut dsv) = (None, None);
        // SAFETY: live resources and local out-pointers.
        unsafe {
            gpu.device
                .CreateRenderTargetView(&color, None, Some(&mut rtv))
                .map_err(|e| e.to_string())?;
            gpu.device
                .CreateDepthStencilView(&depth, None, Some(&mut dsv))
                .map_err(|e| e.to_string())?;
        }
        Ok(Target {
            width: w,
            height: h,
            color,
            rtv: rtv.ok_or("CreateRenderTargetView returned no view")?,
            depth,
            dsv: dsv.ok_or("CreateDepthStencilView returned no view")?,
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

    /// Tightly packed RGBA8, `width × height × 4` bytes, top row first.
    /// Panics on a device error; see [`Target::try_readback_rgba8`].
    pub fn readback_rgba8(&self, gpu: &Gpu) -> Vec<u8> {
        self.try_readback_rgba8(gpu)
            .expect("read back offscreen target")
    }

    /// Copies the RGBA16F colour into a staging texture, maps it (rows are
    /// `RowPitch` apart, which may exceed `width × 8`), and converts each
    /// half to u8 with the identity-clamp tonemap of P0.
    pub fn try_readback_rgba8(&self, gpu: &Gpu) -> Result<Vec<u8>, String> {
        let mut sd = tex2d_desc(self.width, self.height, DXGI_FORMAT_R16G16B16A16_FLOAT);
        sd.Usage = D3D11_USAGE_STAGING;
        sd.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
        let staging = create_tex(gpu, &sd)?;
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        // SAFETY: both resources belong to `gpu.device`; `mapped` is a local.
        unsafe {
            gpu.ctx.CopyResource(&staging, &self.color);
            gpu.ctx
                .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
                .map_err(|e| e.to_string())?;
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
                out.extend(
                    row.as_chunks::<2>()
                        .0
                        .iter()
                        .map(|c| unorm8(u16::from_le_bytes(*c))),
                );
            }
        }
        // SAFETY: paired with the successful Map above; nothing between
        // them can return early.
        unsafe { gpu.ctx.Unmap(&staging, 0) };
        if out.len() != w * h * 4 {
            return Err("Map returned a null pointer".into());
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::unorm8;
    use half::f16;

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

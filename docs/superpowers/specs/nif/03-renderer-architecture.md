# 03 — 3D NIF preview: renderer architecture

Feature 2 for sdc: render a NIF in 3D in the GUI. It has to be quick, match the shaders, and allow live edits (swap textures, change shader type, edit material values). Community Shaders (CS) is the reference for accuracy.

Tags: **[Source]** = read in a local file (path:line) · **[Test]** = run here on 2026-10-10 · **[Doc]** = vendor documentation · **[Claim]** = not verified here · **[Inference]** = my reasoning.

---

## TL;DR

1. **Recommendation: option c. Render natively in D3D11 through the `windows` crate. Compile CS's own `Lighting.hlsl` *unchanged* with fxc (`vs_5_0`/`ps_5_0`, CS's defines), render into a D3D11 shared texture, and import that texture into sdc's wgpu device (`texture_from_d3d11_shared_handle` on Vulkan, `OpenSharedHandle` + `texture_from_raw` on DX12). Show it with `register_native_texture`.** This uses the same bytecode path as the game: the same compiler, the same register layout and the same API as a RenderDoc capture of Skyrim.
2. **fxc compiles CS Lighting.hlsl unchanged.** I tested four permutations, from a vanilla skinned MSN to TRUE_PBR + LLF + IBL + Skylighting + DynamicCubemaps + DEFERRED. Each compiled in **131–659 ms**, 0 errors, 6.6–70 KB DXBC [Test]. Changing the shader type means a background recompile plus a cache, which is what CS does in game. Changing material values means a cbuffer write.
3. **The DXC / SPIR-V / naga route (option b) does not work as-is.** DXC 1.8.2502 rejects the same source with 80 front-end errors. 77 of them are "no member named X in namespace Y": CS puts `cbuffer`s inside `namespace`s, which fxc accepts and DXC does not. The Windows SDK's dxc also has no SPIR-V codegen [Test]. wgpu 30's HLSL passthrough on DX12 compiles against wgpu's own root signature and global sampler heap, so `register(s5)` etc. would not bind [Source].
4. **A hand port to WGSL (option a) is the fallback, not the main path.** It means about 2.8k–3.7k flattened lines per permutation [Test], 60+ `#if` permutation symbols, and re-porting on every CS update. It is the only option with zero native interop, so keep it in reserve in case the interop path fails on some driver.
5. **Phases:** P0, the forward (non-DEFERRED) permutation drawn to SV_Target0 with stubbed engine state (first pixel in about 1–2 weeks). P1, live edit UI, texture hot-swap from sdc's encoder and a cache keyed on the define set. P2, DEFERRED + `DeferredCompositeCS` with stubbed SSGI/env, checked pixel-by-pixel against RenderDoc captures. The perf target is 60 fps at 1600×900 with a 50k-tri armour and 8× 4K BC7. Every option except d clears it easily [Inference].

---

## 1. What sdc runs today (versions found)

| Item | Value | Where |
|---|---|---|
| eframe / egui / egui-wgpu | **0.36.2** (exact pin `=0.36.2`) | `converter/Cargo.lock`; `crates/gui/Cargo.toml:27` [Source] |
| wgpu / wgpu-core / wgpu-hal / wgpu-types | **30.0.1** | `Cargo.lock` [Source] |
| naga | **30.0.1** | `Cargo.lock` [Source] |
| windows | **0.62.2**. The lock has a single copy, the same one wgpu-hal uses. It has features `Win32_Graphics_Direct3D11`, `Direct3D11on12`, `Direct3D_Fxc` | `Cargo.lock`; `windows-0.62.2/Cargo.toml:426,427,432` [Source] |
| gpu-allocator | 0.28.0 | `Cargo.lock` [Source] |
| wgpu backends compiled in | dx12, vulkan, gles (+metal, webgpu: inert on Windows). eframe `wgpu` feature → `egui-wgpu/default` → `wgpu/default` + `wgpu/webgl` | `eframe-0.36.2/Cargo.toml:99-102`, `egui-wgpu-0.36.2/Cargo.toml [features]`, `wgpu-30.0.1/Cargo.toml:52-61` [Source] |
| wgpu shader-input features | `wgsl` only. `spirv` and `glsl` are **not** enabled; `hlsl` passthrough goes through `PASSTHROUGH_SHADERS` (feature bit 52) | `egui-wgpu-0.36.2/Cargo.toml:120-126`; `wgpu-types-30.0.1/src/features.rs:1343-1360` [Source] |
| Instance backends requested | `Backends::from_env().unwrap_or(PRIMARY | GL)` | `egui-wgpu-0.36.2/src/setup.rs:245-246` [Source] |
| Power preference | `HighPerformance` unless env overrides | `setup.rs:255-256` [Source] |
| **Backend actually selected (default)** | **Vulkan** on a machine with a Vulkan driver. wgpu-core adds hal instances in the order Vulkan, Metal, Dx12, Gles (`wgpu-core-30.0.1/src/instance.rs:84-92`), and `request_adapter` stable-sorts only by device type (`:573-600`). So the discrete Vulkan adapter comes before the discrete DX12 one. `WGPU_BACKEND=dx12` overrides this. | [Source] + [Inference: stable sort keeps insertion order] |
| sdc device setup | `render::wgpu_configuration()` adds `TEXTURE_COMPRESSION_BC` and takes the adapter's full limits | `crates/gui/src/render/mod.rs:457-476`, `main.rs:41-43` [Source] |
| How the 2D preview draws | `egui_wgpu::CallbackTrait` paint callback (`PaintSlot`). The renderer lives in `callback_resources`, uploads happen in `sync_renderer()` before the UI. | `render/mod.rs:3-4,415-452`; `app.rs:47-54,85-125,188-189` [Source] |

Consequences:
- sdc can pin a backend with one line in `wgpu_configuration()` (set `setup.instance_descriptor.backends`). Option c works on either backend, but the import call differs (section 3c).
- A 3D view fits the existing pattern: a `PreviewRenderer`-like resource in `callback_resources`, plus either a paint callback (option a) or a registered native texture (option c).

---

## 2. What CS needs from the engine (the reason "accurate" is hard)

Facts measured on the local CS tree (`F:\ai\dds\nifparseref\skyrim-community-shaders-dev`). Another agent is producing the full inventory; this is only what drives the architecture.

- CS compiles `Data/Shaders/<name>.hlsl` **at runtime** with `D3DCompileFromFile`, profiles `vs_5_0`/`ps_5_0`/`cs_5_0`, flags `ENABLE_STRICTNESS | OPTIMIZATION_LEVEL3` (`src/ShaderCache.cpp:111-113,115-118,1693,1720-1723`) [Source]. **The HLSL ships in the user's install.** sdc can compile whatever CS version the user has, so accuracy keeps up with their CS version without vendoring anything [Inference].
- Defines are `VSHADER`/`PSHADER`, then the **vanilla game's** `GetLightingShaderDefines(descriptor)` (called through a relocation into SkyrimSE.exe, `ShaderCache.cpp:138-141`), then `DEFERRED`, `TRUE_PBR`, `GLINT` and one define per loaded feature (`:143-158`) [Source]. **sdc must re-implement the vanilla descriptor→define table.** It is a small fixed table. Its exact output can be read from CS's debug log line `"Compiling {} ... to {defines}"` (`:1689`) or from a RenderDoc capture (section 7).
- Some lighting flags are read **at runtime** from cbuffer `Permutation::PerShader : register(b4)` (`Common/Permutation.hlsli:97-104`). `HasLightingFlag` masks `PixelShaderDescriptor` with `RuntimeLightingFlags` = Specular | SoftLighting | RimLighting | BackLighting for normal objects (`Lighting.hlsl:846-856`) [Source]. Those toggles are cbuffer edits; everything else is a define and needs a recompile.
- Bindings used by the compiled permutations [Test, `fxc /dumpbin`]:
  - VS skinned: cb0 PerTechnique, cb1 PerMaterial, cb9 PreviousBonesBuffer, cb10 BonesBuffer, cb12 VS_PerFrame. Input POSITION, TEXCOORD0, BLENDWEIGHT, BLENDINDICES (+NORMAL/BINORMAL without MSN, COLOR with VC; `Lighting.hlsl:32-46`).
  - PS skinned TRUE_PBR (forward): s0,s5,s6,s9,s11,s12,s14; t0,t1,t5,t6,t9,t11,t12,t14; cb0,1,2,4,5,6,8,12. Writes **3** targets.
  - PS + LLF + IBL + Skylighting + DynCubemaps + DEFERRED: the above plus t35–37 (LLF structured buffers), t50/t53 (skylighting 3D probes), t76/t77 (IBL), cb3 StrictLightData. Writes **8** targets (`Lighting.hlsl:290-300`) consumed by `DeferredCompositeCS.hlsl` (t0–t13 incl. env cubemaps and SSGI, u0–u2; `DeferredCompositeCS.hlsl:10-45`) [Source].
- Engine state that must be stubbed in every option: cb5 `SharedData` (dir light, sun, AmbientSH R/G/B, CameraData, BufferDim, Timer, InInterior…; `Common/SharedData.hlsli:9-35`), cb6 `FeatureData` (per-feature settings structs; `:451-473`), cb12 PerFrame, cb4 descriptors, cb8 LLPerGeometry, t14 shadow mask (white = unshadowed), t17 depth, LLF light lists (empty, or one light), skylighting probes (constant), IBL textures (from a user HDRI or a neutral grey) [Source for slots; Inference for stub values].

**The reflection blob produced by fxc (`D3DReflect`) lists exactly which slots a permutation uses.** Under option c the stub binder can be data-driven: for each reflected slot, bind a user resource or a default [Inference]. Options a and b need this table hand-maintained.

---

## 3. Options

### a. In-egui wgpu pass + hand-port of CS Lighting.hlsl → WGSL
- **How:** a new `CallbackTrait` (sdc already uses one) or render-to-texture plus `register_native_texture`. The WGSL is hand-translated.
- **Size of the port:** the flattened (preprocessed) PS is 2,771 non-blank lines for the vanilla skinned MSN permutation, 3,221 for skinned TRUE_PBR and 3,699 with the common features [Test, `fxc /P`]. `Lighting.hlsl` alone is 3,095 lines with about 60 distinct `defined(X)` symbols (TRUE_PBR ×52, LANDSCAPE ×44, SKINNED ×25 …) [Test]. WGSL has no preprocessor, so permutations become naga/WGSL override constants, uniform branches, or a home-made `#ifdef` pass.
- **Accuracy:** risks drift on every port. HLSL semantics hide in the details: `packoffset`, `row_major float3x4`, `precise`, fxc's `/O3` reassociation and `[branch]` behaviour, half-precision sampling of sRGB views. CS changes the shaders often, so each update is a manual diff and re-port.
- **Pros:** no unsafe interop, works on any wgpu backend, easiest to fit into eframe, single-process.

### b. CS HLSL → DXC → SPIR-V → naga → wgpu, or wgpu passthrough
- **DXC front end:** `dxc 1.8.2502.11 -T ps_6_0 -HV 2018` on skinned TRUE_PBR fails with **80 errors**: 48 + 29 "no member named 'X' in namespace 'Y'" (cbuffers declared inside `namespace SharedData {}`, `Permutation {}`, etc.), plus 3 redefinitions. HV 2016 gives the same result; HV 2021 adds 2 ternary errors. The VS compiles cleanly [Test]. Source edits are needed, so the shaders are no longer "unchanged".
- **SPIR-V codegen:** the Windows SDK dxc reports "SPIR-V CodeGen not available" [Test]. A Vulkan-SDK or GitHub-release DXC would be needed, but it shares the same front end and would hit the same 80 errors [Inference].
- **naga spv-in (30.0.1):** has an allowlist of supported capabilities (`naga-30.0.1/src/front/spv/mod.rs:54+`) and rejects non-tight matrix strides (`:2569-2578`) and runtime arrays outside StorageBuffer (`error.rs:55-56`) [Source]. Structured buffers like LLF's are SSBOs, which is fine. The other limits are unknown until something compiles. sdc does not enable wgpu's `spirv` feature today.
- **wgpu passthrough at 30.0.1:** `Device::create_shader_module_passthrough` is gated on `Features::PASSTHROUGH_SHADERS` (Vulkan, DX12, Metal, WebGPU; `wgpu-types features.rs:1343-1360`). The descriptor itself warns: "you shouldn't expect this to work with bindings except on SPIR-V" (`wgpu-types shader.rs:119-121`) [Source].
  - DX12 HLSL passthrough is compiled with wgpu's `shader_model` and its own fxc/dxc container (`wgpu-hal dx12/device.rs:397-401,420-427`). Registers come from wgpu's root signature: per-group register counters, increasing spaces (`:937-1160`). Samplers sit in a **global sampler heap at s0..s2047 / s2048.. space 0, indexed through a per-group index SRV** (`:1160-1196,1288-1300`) [Source]. CS's `SamplerState … : register(s5)` would read heap slot 5, whatever sampler that is. Every `register()` would have to be rewritten per bind-group layout, which is fragile across CS updates. (`fxc /T ps_5_1` on the TRUE_PBR permutation does compile [Test], so the shader model is not the blocker; the binding scheme is.)
  - SPIR-V passthrough: you write descriptor sets yourself, but you still need DXC's SPIR-V output, so you are back to the namespace errors.
- **Verdict:** dismissed for now. It has the costs of a port (source patching) and of native code (unsafe passthrough, backend-specific behaviour) without the accuracy of c.

### c. Native D3D11 via `windows` crate, CS HLSL unchanged, shared texture → wgpu  **(recommended)**
- **Compile:** `D3DCompile`/`D3DCompileFromFile` from `d3dcompiler_47.dll`, the same DLL wgpu-hal itself loads (`wgpu-hal dx12/shader_compilation.rs:163-169`). Use CS's include root (`Data/Shaders`) and its defines. Measured 131 ms (VS), 147 ms (vanilla PS), 394 ms (PBR PS), 659 ms (PBR + 5 features + DEFERRED) [Test]. Cache by `(source hash, define set)`; CS does the same (`ShaderCache.cpp:1538`).
- **Render:** an `ID3D11Device` on the **same adapter LUID** as wgpu's adapter. Bind cbuffers and SRVs to the exact registers from reflection, then draw the NIF using the game's input layout (section 2). Output goes to a `D3D11_RESOURCE_MISC_SHARED | D3D11_RESOURCE_MISC_SHARED_NTHANDLE` RGBA16F/RGBA8 texture (**no keyed mutex**: neither wgpu import path acquires one; wgpu's Vulkan import uses the plain `D3D11_TEXTURE` handle type, `vulkan/device.rs:468-472`) plus a depth buffer.
- **Import into wgpu (both paths exist at 30.0.1):**
  - Vulkan backend (sdc's default): `wgpu_hal::vulkan::Device::texture_from_d3d11_shared_handle(HANDLE, &desc)`. It requires `Features::VULKAN_EXTERNAL_MEMORY_WIN32`, which must be added in the `device_descriptor` closure, and uses `D3D11_TEXTURE` handle type + dedicated allocation (`wgpu-hal vulkan/device.rs:446-505`). Then `wgpu::Device::create_texture_from_hal::<Vulkan>` (`wgpu api/device.rs:341`) [Source]. The `HANDLE` type comes from the same `windows 0.62.2` crate sdc would use, so there is no version mismatch [Source].
  - DX12 backend: `ID3D12Device::OpenSharedHandle` on `dx12::Device::raw_device()` → `dx12::Device::texture_from_raw(resource, …)` (`wgpu-hal dx12/device.rs:455-486`) → `create_texture_from_hal::<Dx12>` [Source]. Alternative: pin DX12 and run **D3D11On12** on wgpu's own `ID3D12Device`/queue (`raw_queue()` `:459`). Then no shared handle or cross-device sync is needed [Claim: zero-copy holds; the `windows` crate binding exists, `Direct3D11on12`].
- **Sync:** wgpu has no *portable* external-fence import. On **Vulkan**, however, wgpu-hal 30.0.1 exposes `vulkan::Queue::add_wait_semaphore(semaphore, Some(value), stage)` and `add_signal_semaphore`, which stage a wait or signal on the next submit (`wgpu-hal vulkan/mod.rs:1519-1570`) [Source]. A shared `ID3D11Fence` (NT handle) could be imported as a timeline semaphore via `VK_KHR_external_semaphore_win32` using the raw `ash::Device` (`Queue::raw_device()`). That would give a GPU-side wait. Whether drivers accept a D3D11 fence handle (`D3D11_FENCE` handle type) is a [Claim] to test. P0 plan:
  1. D3D11 renders, then `Flush`, then waits on an `ID3D11Query(EVENT)` or an `ID3D11Fence` + event on a worker thread.
  2. Mark the frame "ready" before egui samples it. Double-buffer two shared textures so neither side stalls.
  3. The cost is at most one frame of latency.
  On DX12 the raw `ID3D12CommandQueue::Wait` on a shared fence is possible through `as_hal`. The GPU-side Vulkan semaphore path above is a P2 optimisation.
- **Fallback if import fails** (driver lacks `VK_KHR_external_memory_win32` D3D11_TEXTURE, or an LUID mismatch): `CopyResource` to a staging texture, `Map`, then `queue.write_texture`. 1600×900 RGBA8 is about 5.8 MB per frame, a few ms, and still interactive [Inference].
- **Present:** `egui_wgpu::Renderer::register_native_texture(device, view, filter)` (`egui-wgpu-0.36.2/src/renderer.rs:774`) → `ui.image(id)`. Resize recreates both sides. Input (orbit, zoom, pan) comes from the egui `Response`.
- **Licence:** `package/Shaders` is MIT (`package/Shaders/LICENSE`). `features/*` shaders have no own licence, so they fall under the repo default: GPL-3.0-or-later WITH Modding + Linking exceptions (`README.md:195-215`, `COPYING`, `EXCEPTIONS.md`) [Source]. Both are compatible with sdc's GPL-3.0-or-later. Loading the user's installed shaders at runtime avoids redistributing them at all.

### d. Sidecar process (C++ or Rust D3D11) over IPC + shared texture
- Same renderer as c, out of process. The shared-handle import into wgpu is identical; the handle is duplicated across processes (`DuplicateHandle`, or a named NT handle).
- **Gain:** a TDR or driver crash from a bad permutation does not take sdc down. A C++ sidecar could reuse D3D11 boilerplate easily.
- **Cost:** IPC protocol for meshes, textures, cbuffers and camera; process lifecycle; double the build systems if C++; harder debugging. Edits cost one IPC round-trip each, which is fine.
- **Verdict:** keep as a **refactor target** if c shows instability. Design c's renderer behind a message-shaped API (`SetMesh`, `SetTexture(slot, …)`, `SetCB(reg, bytes)`, `SetDefines`, `Render(camera)`) so it can move out of process later.

### e. Reuse BodySlide / NifSkope / TruePBR-Manager, or a game-engine crate
- **BodySlide/Outfit Studio:** OpenGL 3.3 GLSL (`res/shaders/sk_truepbr.frag:1`, 230 lines; `default.frag` 414). It is its own approximation of TruePBR, not CS code (`sk_truepbr.frag:10-19`). GPL-3.0 (`LICENSE`) [Source]. **Dismiss as a renderer; keep as a reference for NIF→material/slot mapping** (e.g. the slot-5 RMAOS rule).
- **NifSkope:** OpenGL 4.1 GLSL (`res/shaders/sk_default.frag:1`, 195 lines), vanilla-ish approximation. BSD-style NifSkope licence (`LICENSE`) [Source]. Dismiss as a renderer.
- **TruePBR-Manager:** a D3D12 renderer (`SPEC.md:82`) in a Qt child HWND (`CreateSwapChainForHwnd`, `D3D12Renderer.cpp:284`; `MaterialPreviewWidget.cpp:87`). Shaders are compiled with **DXC SM6 offline** (`src/CMakeLists.txt:337-412`) and it uses its *own* `PBRShader.hlsl` with `Common/PBR.hlsli` "Adapted from skyrim-community-shaders" (`src/renderer/Common/PBR.hlsli` header). It only draws primitive shapes, not NIFs (`SPEC.md:85`) [Source]. **Licence: CC BY-NC 4.0** (`LICENSE:1`) [Source]. The non-commercial term is an additional restriction that GPL-3.0 forbids, so **no code reuse**. It is a design reference only: IBL compute passes, GT7 tonemap, controls bar (`SPEC.md:88-101,645-667`). Its adaptation also confirms that CS's PBR include needs edits under DXC [Inference, consistent with the 80 errors above].
- **bevy / rend3 / three-d:** each brings its own material system and shader language, so none would run CS's HLSL. Embedding them in eframe means sharing or owning a wgpu device of a matching version. bevy pins its own wgpu; rend3 is unmaintained [Claim]. **Dismiss.**
- **Child HWND (variant of c/d):** possible (TruePBR-Manager does it with Qt), but it has airspace problems: egui popups and tooltips can't draw over it, DPI and resize jitter, focus stealing. **Dismiss in favour of shared texture**.

---

## 4. Options matrix

Scores: ●●● good · ●● ok · ● poor. All perf figures are [Inference] unless marked.

| Criterion | a. WGSL hand-port | b. DXC/SPIR-V/naga or passthrough | **c. Native D3D11 + shared tex** | d. Sidecar D3D11 | e. Reuse BS/NifSkope/TPM/engine |
|---|---|---|---|---|---|
| Accuracy vs CS | ●: hand-port drift; fxc-specific codegen differences | ●●: CS source would be patched; DXC ≠ fxc codegen | **●●●: same source, compiler, profile, API and register layout as the game** | ●●● (same as c) | ●: different shaders |
| Time to first pixel | ~2–3 wk (a reduced port of one permutation) | blocked (source patching first) | **~1–2 wk** (D3D11 boilerplate + fxc + one import path) | ~3 wk | n/a |
| Effort to "full" (DEFERRED + composite + features) | months, and repeated on every CS update | months | ~4–6 wk on top of P0 | c + ~2 wk IPC | n/a |
| Realtime edit = cbuffer update, not recompile | ●●● uniforms | ●●● | ●●● cb0/1/2/4/5/6 writes; runtime flags via cb4 | ●●● (IPC hop) | — |
| Permutation handling | manual (override consts / branches) | DXC defines, after patching | **CS defines → fxc 0.13–0.66 s [Test], background + cache** | same as c | — |
| Maintenance as CS updates | ● re-port | ● re-patch namespaces/registers | **●●● load the user's installed `Data/Shaders`**; only the define table and stub layouts change | ●●● | — |
| eframe integration | ●●● existing CallbackTrait pattern | ●● passthrough feature + unsafe | ●● unsafe `as_hal` import + `register_native_texture`; one backend-specific branch | ●● | ● |
| Licence | own WGSL derived from MIT/GPL source: OK | OK | **OK** (MIT + GPL-3.0+exc vs sdc GPL-3.0+) | OK | TPM **CC BY-NC: no**; BS GPL OK; engines OK |
| Perf @ target | ●●● | ●●● | ●●● (+≤1 frame latency, or ~3–5 ms if CPU-copy fallback) | ●● (+IPC) | — |
| Crash isolation | n/a | n/a | ● (D3D11 TDR shares the process) | ●●● | — |
| Platform | any wgpu backend | DX12/Vulkan | Windows only (sdc is Windows-only) | Windows | — |

---

## 5. Recommendation and phased path

**Option c,** structured with d's message API so it can become a sidecar later. Option a is a dormant fallback; b and e are dismissed.

### P0 — First pixel (≈1–2 weeks)
1. Crate `sdc-render3d` (Windows-only). hal types come through `eframe::egui_wgpu::wgpu::hal`, since wgpu re-exports wgpu-hal as `hal` (`wgpu-30.0.1/src/lib.rs:249`) [Source]; sdc does not need a direct wgpu-hal dependency. Use `windows 0.62.2` features `Win32_Graphics_Direct3D11`, `Win32_Graphics_Dxgi`, `Win32_Graphics_Direct3D_Fxc`, `Win32_Graphics_Direct3D`. This is the same `windows` version already in the lock.
2. Create the D3D11 device on the wgpu adapter's LUID:
   - DX12: the hal adapter's raw `IDXGIAdapter`.
   - Vulkan: `VkPhysicalDeviceIDProperties.deviceLUID` through `Adapter::as_hal::<Vulkan>`.
3. In `render::wgpu_configuration()`, request `VULKAN_EXTERNAL_MEMORY_WIN32` when the adapter has it. Keep the CPU-copy fallback.
4. Compile `Lighting.hlsl` VS+PS for **forward** (no `DEFERRED`), skinned, MSN or TRUE_PBR, from a user-pointed `Data/Shaders` (or a vendored snapshot for tests). Use reflection to build the slot table.
5. Stubs:
   - cb5: one dir light plus AmbientSH from a neutral preset.
   - cb12: camera.
   - cb9/cb10: identity bones (bind pose).
   - t14: a white 1×1.
   - Other reflected SRVs: 1×1 defaults.
6. Draw from the NIF's SSE packed vertex data as-is, through an input layout matched to the VS input signature. Show SV_Target0 with `register_native_texture`.

### P1 — Live editing (≈2 weeks)
- **Material edits:** cbuffer writes every frame (PerMaterial/PerGeometry, TruePBR settings in FeatureData, runtime flags in cb4). No recompile.
- **Shader type / define changes:** queue an fxc compile on a worker thread. Keep drawing the old permutation until the new one is ready. Disk cache keyed on `(hash(sources), defines, profile, flags)`.
- **Texture hot-swap from sdc:** sdc already produces BCn bytes for its encoded preview (`app.rs:102` `set_encoded`). Upload the same mip chain with `UpdateSubresource` into a D3D11 BCn texture, then rebind that SRV slot. A/B toggle: source texture vs encoded output on the mesh. Use `_SRGB` views where the game does (to be confirmed by RenderDoc, section 7).
- **Camera and lighting controls:** orbit/pan/zoom (egui drag/scroll), light direction/colour/intensity, ambient SH preset (exterior day / interior / custom), HDRI for IBL stubs, exposure, a debug view (single target / channel). TruePBR-Manager's control bar is a design reference only (`SPEC.md:645-667`).

### P2 — Accuracy parity (≈4–6 weeks)
- Compile with `DEFERRED` and the user's enabled feature defines. Add 8 RTs plus `DeferredCompositeCS.hlsl` (`cs_5_0`) with SSGI textures as neutral stubs and env cubemaps from the HDRI. Then add the tonemap/ISHDR chain if the comparison needs it.
- **Validation loop:**
  1. In game, take a RenderDoc capture of a reference armour on a neutral scene (CS has a RenderDoc feature, `features/RenderDoc`, and `tools/taa-renderdoc-ab.py`).
  2. Export the draw's cbuffers (cb0–cb12), SRV formats and sampler states, input layout, and the shader disassembly / defines.
  3. Load that exact state into sdc ("replay preset") and render the same camera.
  4. Diff the PS output targets per pixel (ΔE / max-abs per channel).
  5. Use the same RenderDoc on sdc's D3D11 device to compare draw state field by field.
  6. Keep the reference captures as regression fixtures.

### Perf target
- **Target:** 60 fps (16.6 ms) at a 1600×900 viewport with a ~50k-tri skinned armour, 8 × 4K BC7 textures with mips (≈ 8 × 21.3 MB ≈ 170 MB VRAM), forward and deferred.
  - High end (RTX 3070-class): < 2 ms GPU.
  - Mid range (GTX 1660 / RX 6600-class): < 6 ms GPU.
  - Shader edits apply in the next frame; permutation changes within 0.7 s.
- **How c meets it:** one draw of 50k tris is negligible. The PS is a game shader that already runs at 4K in game, and fxc output is at most 70 KB DXBC [Test]. The extra cost of the shared texture is a sample in egui's pass plus at most one frame of latency. The CPU-copy fallback adds roughly 3–5 ms [Inference]. Render only on change (camera/edit), not continuously, so the GUI stays cool.

---

## 6. Risks

| Risk | Mitigation |
|---|---|
| `VK_KHR_external_memory_win32` with `D3D11_TEXTURE` handles unsupported or buggy on some drivers (wgpu just logs and returns `Unexpected`, `vulkan/device.rs:458-465`) | Feature-probe at start-up, then fall back to DX12 `OpenSharedHandle`, or pin the DX12 backend, or the CPU-copy path |
| LUID mismatch on hybrid laptops (wgpu picks the dGPU while D3D11 defaults to the iGPU) | Create D3D11 explicitly on wgpu's adapter LUID |
| No portable cross-API GPU sync in wgpu 30 | P0: CPU-side fence/event wait + double buffering. Later: on Vulkan, import a shared D3D11 fence as a timeline semaphore and stage it with `vulkan::Queue::add_wait_semaphore` (`vulkan/mod.rs:1552`); on DX12, `ID3D12CommandQueue::Wait` via `raw_queue()` |
| Re-implementing vanilla `GetLightingShaderDefines` wrong | Read the real defines from the CS debug log (`ShaderCache.cpp:1689`) or RenderDoc; table-driven tests |
| CS changes the cbuffer layouts (`SharedData`, `FeatureData` grow often) | Build stub buffers from **reflection** (field names/offsets) rather than hard-coded structs; unknown fields are zeroed |
| Unsafe interop / TDR from a bad permutation kills sdc | Message-shaped API so it can move to a sidecar (d) |
| Accuracy hinges on stubbed engine state (SH ambient, fog, shadows, IBL), not on the shader | "Replay preset" from RenderDoc captures; expose every stub in the UI |
| Shipping CS shaders | Prefer loading from the user's install; if vendoring, keep the MIT notice and GPL source as required |

---

## 7. Open questions to settle with RenderDoc
(RenderDoc was not found at `C:\Program Files\RenderDoc` [Test]; a per-user install elsewhere was not checked.)

1. For an armour draw, a skin (facegen / CS_SKIN) draw and a hair draw: the actual cb4 `PixelShaderDescriptor` / `ExtraShaderDescriptor` and the define list CS compiled (does the shader name or debug info carry it?).
2. Under CS, does Lighting ever run **non-DEFERRED** for opaque objects? How much does `DeferredCompositeCS` change a static object's final pixel, i.e. is the forward P0 "close"?
3. Values of cb5 SharedData and cb12 PerFrame in a neutral exterior day vs interior. These become the stub presets.
4. SRV formats (`_SRGB` or not) and sampler states per slot (anisotropy, LOD bias / `MipBias` from cb5, address modes).
5. Input layout and vertex buffer format the game binds for SSE BSTriShape (half vs float positions, packed normals/tangents). Can sdc bind NIF vertex data byte-for-byte?
6. Render-target formats (Diffuse R16G16B16A16F? Masks?) and blend/depth state for alpha-tested and alpha-blended materials.
7. Which feature defines are present for a typical user setup (LLF, IBL, Skylighting, DynCubemaps, ExtendedMaterials, Wetness…), so the P2 stub set covers them.
8. Bone buffer layout (cb9/cb10) for a skinned draw, to support posed preview later.

---

## 8. Sources

Local (read-only):
- `F:\ai\dds\converter\Cargo.lock` (versions); `crates\gui\Cargo.toml:26-27`; `crates\gui\src\main.rs:39-46`; `crates\gui\src\render\mod.rs:1-4,108-212,415-476`; `crates\gui\src\app.rs:47-54,85-125,188-189`
- `~\.cargo\registry\src\index.crates.io-*\`:
  - `egui-wgpu-0.36.2\src\setup.rs:240-256`; `egui-wgpu-0.36.2\src\renderer.rs:774`; `egui-wgpu-0.36.2\Cargo.toml`; `eframe-0.36.2\Cargo.toml:99-107`
  - `wgpu-30.0.1\src\lib.rs:249`; `wgpu-hal-30.0.1\src\vulkan\mod.rs:1519-1570`; `wgpu-30.0.1\Cargo.toml:48-136`; `wgpu-30.0.1\src\api\device.rs:181-203,341`
  - `wgpu-core-30.0.1\src\instance.rs:84-92,480-600`
  - `wgpu-hal-30.0.1\src\vulkan\device.rs:296,446-505`; `wgpu-hal-30.0.1\src\dx12\device.rs:380-486,937-1300,1866-1900`; `wgpu-hal-30.0.1\src\dx12\shader_compilation.rs:140-222`
  - `wgpu-types-30.0.1\src\features.rs:1219,1343-1360`; `wgpu-types-30.0.1\src\shader.rs:104-145`
  - `naga-30.0.1\src\front\spv\mod.rs:54-84,2569-2578`; `naga-30.0.1\src\front\spv\error.rs:26-75`
  - `windows-0.62.2\Cargo.toml:426-432`
- `F:\ai\dds\nifparseref\skyrim-community-shaders-dev\`:
  - `package\Shaders\Lighting.hlsl:1-14,32-46,94-130,290-494,795-953,846-856`; `package\Shaders\Common\Permutation.hlsli:97-104`; `package\Shaders\Common\SharedData.hlsli:9-35,451-475`; `package\Shaders\DeferredCompositeCS.hlsl:10-45`
  - `src\ShaderCache.cpp:24-48,111-158,700-740,1538,1640-1723`
  - `package\Shaders\LICENSE` (MIT); `COPYING` (GPL-3.0); `EXCEPTIONS.md`; `README.md:195-215`
- `F:\ai\dds\nifparseref\BodySlide-and-Outfit-Studio-dev\res\shaders\sk_truepbr.frag:1-30`; `LICENSE` (GPL-3.0)
- `F:\ai\dds\nifparseref\nifskope-develop\res\shaders\sk_default.frag:1`; `LICENSE`
- `F:\ai\dds\gui reference\ref\TruePBR-Manager-master\SPEC.md:13,80-101,634-667`; `LICENSE:1` (CC BY-NC 4.0); `src\CMakeLists.txt:333-412`; `src\renderer\D3D12Renderer.cpp:284`; `src\ui\MaterialPreviewWidget.cpp:87`; `src\renderer\Common\PBR.hlsli` header

Tests run here (scratchpad `…\scratchpad\r3d\`, merged `package/Shaders` + `features/*/Shaders` tree, as installed in game):
- `fxc.exe 10.0.26100.0 /T vs_5_0|ps_5_0 /O3 /Ges /I <root>`: 4 permutations OK, 131/147/394/659 ms, cso 6.6/31/52/70 KB. `ps_5_1` also OK. `/P` line counts. `/dumpbin` binding tables.
- `dxc.exe 1.8.2502.11 -T ps_6_0 -HV 2016|2018|2021`: 80/80/82 errors (PS); VS OK. `-spirv`: "SPIR-V CodeGen not available".

Web (not fetched in this pass, so these remain [Claim] until checked):
- D3D11On12: https://learn.microsoft.com/en-us/windows/win32/direct3d12/direct3d-11-on-12
- Vulkan external memory guide (cited in wgpu-hal source): https://docs.vulkan.org/guide/latest/extensions/external.html
- wgpu passthrough discussion (linked from wgpu-types): https://github.com/gfx-rs/wgpu/issues/3103#issuecomment-2833058367

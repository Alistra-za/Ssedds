// C ABI over rgbcx, bc7e (ISPC) and ISPC Texture Compressor for the sdc encoders-sys crate.
// Parameter structs stay on the C++ side, so Rust never mirrors ISPC struct layouts.
#include <cstdint>
#include <cstring>
#include <mutex>
#include "rgbcx.h"
#include "bc7e_ispc.h"
#include "ispc_texcomp.h"

#ifndef SDC_BC7E_PATCHED
#define SDC_BC7E_PATCHED 0
#endif
#ifndef SDC_BC7E_PATCH2
#define SDC_BC7E_PATCH2 0
#endif

namespace {
ispc::bc7e_compress_block_params g_bc7[8][2][2];  // [level u0..u6, 7 = sdc_max][perceptual][exact_opaque]
bc6h_enc_settings g_bc6[6];                       // veryfast, fast, basic, slow, veryslow, veryslow_peak

void init_bc7_level(ispc::bc7e_compress_block_params* p, int level, bool perceptual) {
    switch (level) {
    case 0: ispc::bc7e_compress_block_params_init_ultrafast(p, perceptual); break;
    case 1: ispc::bc7e_compress_block_params_init_veryfast(p, perceptual); break;
    case 2: ispc::bc7e_compress_block_params_init_fast(p, perceptual); break;
    case 3: ispc::bc7e_compress_block_params_init_basic(p, perceptual); break;
    case 4: ispc::bc7e_compress_block_params_init_slow(p, perceptual); break;
    case 5: ispc::bc7e_compress_block_params_init_veryslow(p, perceptual); break;
    case 6: ispc::bc7e_compress_block_params_init_slowest(p, perceptual); break;
#if SDC_BC7E_PATCH2
    default: ispc::bc7e_compress_block_params_init_sdc_max(p, perceptual); break;
#else
    // Without patch 0002 level 7 does not exist; Rust refuses it before it gets here.
    default: ispc::bc7e_compress_block_params_init_slowest(p, perceptual); break;
#endif
    }
}
}  // namespace

extern "C" {

// ISPC's per-target entry points behind the dispatcher. Each target has its own lookup tables, which the
// dispatcher's bc7e_compress_block_init fills only for the target it picks.
#define SDC_BC7E_TARGET(t) \
    void bc7e_compress_block_init_##t(); \
    void bc7e_compress_blocks_##t(uint32_t, uint64_t*, const uint32_t*, const ispc::bc7e_compress_block_params*);
SDC_BC7E_TARGET(sse2)
SDC_BC7E_TARGET(sse4)
SDC_BC7E_TARGET(avx)
SDC_BC7E_TARGET(avx2)

// Must run exactly once (Rust guards it with OnceLock): rgbcx::init writes global state.
void sdc_init(int bc1_mode) {
    rgbcx::init(static_cast<rgbcx::bc1_approx_mode>(bc1_mode));
    ispc::bc7e_compress_block_init();
    for (int l = 0; l < 8; l++)
        for (int s = 0; s < 2; s++)
            for (int e = 0; e < 2; e++) {
                ispc::bc7e_compress_block_params* p = &g_bc7[l][s][e];
                std::memset(p, 0, sizeof(*p));  // rdo_bc_encoder.cpp does the same before init
                init_bc7_level(p, l, s != 0);
#if SDC_BC7E_PATCHED
                p->m_exact_opaque_alpha = (e != 0);
#endif
            }
    GetProfile_bc6h_veryfast(&g_bc6[0]);
    GetProfile_bc6h_fast(&g_bc6[1]);
    GetProfile_bc6h_basic(&g_bc6[2]);
    GetProfile_bc6h_slow(&g_bc6[3]);
    GetProfile_bc6h_veryslow(&g_bc6[4]);
    // Peak-preserving mode (kernel.ispc sdc patch 0001): veryslow with fast_mode also set, which turns it on.
    GetProfile_bc6h_veryslow(&g_bc6[5]);
    g_bc6[5].fast_mode = true;
}

// `target` < 0 lets ISPC dispatch on the CPU; 0..=3 forces the sse2, sse4, avx or avx2 code path (tests only).
// The caller checks the CPU supports the target and never forces the one the dispatcher uses, so the
// per-target init below never rewrites tables a dispatched encode is reading.
void sdc_bc7e_compress(uint32_t n, uint64_t* blocks, const uint32_t* px, int level, int perceptual, int exact,
                       int target) {
    static std::once_flag inited[4];
    const ispc::bc7e_compress_block_params* p = &g_bc7[level][perceptual ? 1 : 0][exact ? 1 : 0];
    switch (target) {
    case 0: std::call_once(inited[0], bc7e_compress_block_init_sse2); bc7e_compress_blocks_sse2(n, blocks, px, p); break;
    case 1: std::call_once(inited[1], bc7e_compress_block_init_sse4); bc7e_compress_blocks_sse4(n, blocks, px, p); break;
    case 2: std::call_once(inited[2], bc7e_compress_block_init_avx); bc7e_compress_blocks_avx(n, blocks, px, p); break;
    case 3: std::call_once(inited[3], bc7e_compress_block_init_avx2); bc7e_compress_blocks_avx2(n, blocks, px, p); break;
    default: ispc::bc7e_compress_blocks(n, blocks, px, p); break;
    }
}

// Measurement only (sdc-bench bc7 tuning): the level's parameters with some fields overridden. A negative
// override keeps the level's value. `t` is 14 ints: partitions_to_try (modes 0/1/3, 2 and 7 together),
// uber_level, refinement_passes, pbit_search, weights r g b a, max_partitions for modes 1/3/7, mode 0, mode 2,
// then the patch-0002 flags endpoint_search and refine_partitions (ignored without the patch), and one spare.
void sdc_bc7e_compress_tuned(uint32_t n, uint64_t* blocks, const uint32_t* px, int level, int perceptual, int exact,
                             const int32_t* t) {
    ispc::bc7e_compress_block_params p = g_bc7[level][perceptual ? 1 : 0][exact ? 1 : 0];
    if (t[0] >= 0) {
        p.m_opaque_settings.m_max_mode13_partitions_to_try = t[0];
        p.m_opaque_settings.m_max_mode0_partitions_to_try = t[0];
        p.m_opaque_settings.m_max_mode2_partitions_to_try = t[0];
        p.m_alpha_settings.m_max_mode7_partitions_to_try = t[0];
    }
    if (t[1] >= 0) p.m_uber_level = t[1];
    if (t[2] >= 0) p.m_refinement_passes = t[2];
    if (t[3] >= 0) p.m_pbit_search = t[3] != 0;
    for (int c = 0; c < 4; c++)
        if (t[4 + c] >= 0) p.m_weights[c] = t[4 + c];
    if (t[8] >= 0) {
        p.m_max_partitions_mode[1] = t[8];
        p.m_max_partitions_mode[3] = t[8];
        p.m_max_partitions_mode[7] = t[8];
    }
    if (t[9] >= 0) p.m_max_partitions_mode[0] = t[9];
    if (t[10] >= 0) p.m_max_partitions_mode[2] = t[10];
#if SDC_BC7E_PATCH2
    if (t[11] >= 0) p.m_endpoint_search = t[11] != 0;
    if (t[12] >= 0) p.m_refine_partitions = t[12] != 0;
#endif
    ispc::bc7e_compress_blocks(n, blocks, px, &p);
}

void sdc_bc1_encode(uint8_t* dst, const uint8_t* px, uint32_t level, int allow3, int black) {
    rgbcx::encode_bc1(level, dst, px, allow3 != 0, black != 0);
}

void sdc_bc3_encode(uint8_t* dst, const uint8_t* px, uint32_t level, uint32_t rad) {
    rgbcx::encode_bc3_hq(level, dst, px, rad, rgbcx::BC4_USE_ALL_MODES);
}

void sdc_bc4_encode(uint8_t* dst, const uint8_t* px, uint32_t stride, uint32_t rad) {
    rgbcx::encode_bc4_hq(dst, px, stride, rad, rgbcx::BC4_USE_ALL_MODES);
}

// `flags` goes to encode_bc4_hq verbatim (research probes and tests reach the upstream path with explicit modes).
void sdc_bc4_encode_flags(uint8_t* dst, const uint8_t* px, uint32_t stride, uint32_t rad, uint32_t flags) {
    rgbcx::encode_bc4_hq(dst, px, stride, rad, flags);
}

void sdc_bc5_encode(uint8_t* dst, const uint8_t* px, uint32_t rad) {
    rgbcx::encode_bc5_hq(dst, px, 0, 1, 4, rad, rgbcx::BC4_USE_ALL_MODES);
}

int sdc_bc1_decode(const uint8_t* src, uint8_t* px, int mode) {
    return rgbcx::unpack_bc1(src, px, true, static_cast<rgbcx::bc1_approx_mode>(mode)) ? 1 : 0;
}

// rgbcx's BC4 decode model, the one encode_bc4_hq searches with (tests and the bench probe compare it with bcdec).
void sdc_bc4_block_values(uint32_t l, uint32_t h, uint8_t* out8) {
    rgbcx::bc4_block::get_block_values(out8, l, h);
}

void sdc_bc6h_compress(const uint16_t* rgba16f, int32_t width, int32_t rows, int32_t stride_bytes, uint8_t* dst, int level) {
    rgba_surface s;
    s.ptr = reinterpret_cast<uint8_t*>(const_cast<uint16_t*>(rgba16f));
    s.width = width;
    s.height = rows;
    s.stride = stride_bytes;
    CompressBlocksBC6H(&s, dst, &g_bc6[level]);
}

int sdc_bc7e_patched(void) { return SDC_BC7E_PATCHED; }

}  // extern "C"

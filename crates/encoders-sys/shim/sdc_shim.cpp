// C ABI over rgbcx, bc7e (ISPC) and ISPC Texture Compressor for the sdc encoders-sys crate.
// Parameter structs stay on the C++ side, so Rust never mirrors ISPC struct layouts.
#include <cstdint>
#include <cstring>
#include "rgbcx.h"
#include "bc7e_ispc.h"
#include "ispc_texcomp.h"

#ifndef SDC_BC7E_PATCHED
#define SDC_BC7E_PATCHED 0
#endif

namespace {
ispc::bc7e_compress_block_params g_bc7[7][2][2];  // [level u0..u6][perceptual][exact_opaque]
bc6h_enc_settings g_bc6[5];                       // veryfast, fast, basic, slow, veryslow

void init_bc7_level(ispc::bc7e_compress_block_params* p, int level, bool perceptual) {
    switch (level) {
    case 0: ispc::bc7e_compress_block_params_init_ultrafast(p, perceptual); break;
    case 1: ispc::bc7e_compress_block_params_init_veryfast(p, perceptual); break;
    case 2: ispc::bc7e_compress_block_params_init_fast(p, perceptual); break;
    case 3: ispc::bc7e_compress_block_params_init_basic(p, perceptual); break;
    case 4: ispc::bc7e_compress_block_params_init_slow(p, perceptual); break;
    case 5: ispc::bc7e_compress_block_params_init_veryslow(p, perceptual); break;
    default: ispc::bc7e_compress_block_params_init_slowest(p, perceptual); break;
    }
}
}  // namespace

extern "C" {

// Must run exactly once (Rust guards it with OnceLock): rgbcx::init writes global state.
void sdc_init(int bc1_mode) {
    rgbcx::init(static_cast<rgbcx::bc1_approx_mode>(bc1_mode));
    ispc::bc7e_compress_block_init();
    for (int l = 0; l < 7; l++)
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
}

void sdc_bc7e_compress(uint32_t n, uint64_t* blocks, const uint32_t* px, int level, int perceptual, int exact) {
    ispc::bc7e_compress_blocks(n, blocks, px, &g_bc7[level][perceptual ? 1 : 0][exact ? 1 : 0]);
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

void sdc_bc5_encode(uint8_t* dst, const uint8_t* px, uint32_t rad) {
    rgbcx::encode_bc5_hq(dst, px, 0, 1, 4, rad, rgbcx::BC4_USE_ALL_MODES);
}

int sdc_bc1_decode(const uint8_t* src, uint8_t* px, int mode) {
    return rgbcx::unpack_bc1(src, px, true, static_cast<rgbcx::bc1_approx_mode>(mode)) ? 1 : 0;
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

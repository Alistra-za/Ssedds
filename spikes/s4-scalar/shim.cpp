#include "vendor/basisu_bc7e_scalar.cpp"
#define SCALAR_NS bc7e_scalar::
extern "C" void s4_init() { SCALAR_NS bc7e_compress_block_init(); }
extern "C" void s4_compress(uint32_t n, uint64_t* blocks, const uint32_t* px) {
    static SCALAR_NS bc7e_compress_block_params p;
    static bool once = false;
    if (!once) { memset(&p, 0, sizeof(p)); SCALAR_NS bc7e_compress_block_params_init_veryslow(&p, false); once = true; }
    SCALAR_NS bc7e_compress_blocks(n, blocks, px, &p);
}

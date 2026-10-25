// Tessalith DDS: Photoshop pixel samples to RGBA floats. MIT licence (see photoshop/LICENSE).
#pragma once

#include <stddef.h>
#include <stdint.h>

// Rows per Photoshop request so that one band (rows * row_bytes) stays within cap_bytes: at least one row,
// at most `height`.
int32_t band_rows(int64_t row_bytes, int32_t height, int64_t cap_bytes);

// Converts `pixels` interleaved pixels from Photoshop into RGBA floats in [0, 1] (Note DC6):
// 8-bit samples (uint8) scale by 1/255, 16-bit samples (uint16, Photoshop's 0..32768 range) by 1/32768.
// `channels` is 3 (RGB, alpha becomes 1) or 4 (RGB plus the alpha source). `out` receives pixels * 4 floats.
void to_rgba_floats(const void *src, size_t pixels, int channels, int depth, float *out);

// Copies `pixels` interleaved RGBA8 pixels into planar blocks of `pixels` bytes each: R, G, B and, when `planes`
// is 4, A (3 drops alpha). `out` receives pixels * planes bytes.
void deinterleave_rgba8_to_planes(const uint8_t *rgba, size_t pixels, int planes, uint8_t *out);

// The largest DDS Open accepts: 16384 x 16384 RGBA8 with a full mip chain, plus magic, header and DX10 header.
constexpr int64_t max_dds_file_bytes() {
    int64_t bytes = 4 + 124 + 20;
    for (int64_t side = 16384; side >= 1; side /= 2) bytes += side * side * 4;
    return bytes;
}
constexpr int64_t kMaxDdsFileBytes = max_dds_file_bytes();

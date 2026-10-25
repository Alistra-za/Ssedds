// Tessalith DDS: Photoshop pixel samples to RGBA floats. MIT licence (see photoshop/LICENSE).
#pragma once

#include <stddef.h>
#include <stdint.h>

// Rows per Photoshop request so that one band (rows * row_bytes) stays within cap_bytes: at least one row,
// at most `height`.
int32_t band_rows(int64_t row_bytes, int32_t height, int64_t cap_bytes);

// Converts `pixels` interleaved pixels from Photoshop into RGBA floats in [0, 1] (Note DC6):
// 8-bit samples (uint8) scale by 1/255, 16-bit samples (uint16, Photoshop's 0..32768 range) by 1/32768,
// 32-bit samples (float) are copied unchanged: no scale, no clamp, NaN kept (Note DE1).
// `channels` is 3 (RGB, alpha becomes 1) or 4 (RGB plus the alpha source). `out` receives pixels * 4 floats.
void to_rgba_floats(const void *src, size_t pixels, int channels, int depth, float *out);

// Writes one NUL-terminated trace line for a float image and returns its length (cap bounds the write, NUL included):
//   px (r,g,b,a) ... r[min,max] g[min,max] b[min,max] a[min,max] nan N above1 N below0 N
// The first 4 pixels use %.9g (NaN prints as nan); min and max are over finite values (0,0 when none); the three
// counts cover RGB only.
size_t describe_samples(const float *rgba, size_t pixels, char *out, size_t cap);

// Copies `pixels` interleaved RGBA8 pixels into planar blocks of `pixels` bytes each: R, G, B and, when `planes`
// is 4, A (3 drops alpha). `out` receives pixels * planes bytes.
void deinterleave_rgba8_to_planes(const uint8_t *rgba, size_t pixels, int planes, uint8_t *out);

// The float form of deinterleave_rgba8_to_planes (DE7): `rgba_f32` holds `pixels` RGBA pixels of native-endian f32, read
// with memcpy (no alignment assumed); `out` receives `planes` blocks of pixels * 4 bytes.
void deinterleave_rgba32f_to_planes(const uint8_t *rgba_f32, size_t pixels, int planes, uint8_t *out);

// The largest DDS Open accepts: 16384 x 16384 RGBA8 with a full mip chain, plus magic, header and DX10 header.
constexpr int64_t max_dds_file_bytes() {
    int64_t bytes = 4 + 124 + 20;
    for (int64_t side = 16384; side >= 1; side /= 2) bytes += side * side * 4;
    return bytes;
}
constexpr int64_t kMaxDdsFileBytes = max_dds_file_bytes();

// Open's alpha rule (DC23): transparency only when psdds reports alpha and some alpha byte is below 254. The engine
// guarantees only alpha >= 254 for opaque BC7 (DECISIONS Notes A/C/D/S5), so its own opaque files open opaque.
bool opens_with_transparency(const uint8_t *rgba, size_t pixels, uint32_t has_alpha);

// Save's alpha rule (DC31), Open's rule on the float image: true when some alpha is below 254/255 (8-bit: a byte
// below 254; 16-bit: below 254/255 of 32768). An image whose alpha is all 254 or more saves as opaque.
bool floats_have_transparency(const float *rgba, size_t pixels);

// Estimate (PIFormat.h minDataBytes / maxDataBytes) for a width x height save: the smallest file (BC1/BC4, no mips,
// legacy header) to the largest (RGBA8, full mip chain, DX10 header). Above 8 bits the largest is RGBA16F (8 bytes a pixel):
// the 32-bit estimate is the same two figures with that size (amends DC24, Note DE6).
void dds_size_range(int64_t width, int64_t height, int depth, int64_t *min_bytes, int64_t *max_bytes);

// FilterFile: false when the first len bytes of a file (up to 148: magic, header, DX10 header) show a DDS that Open
// will refuse, so another DDS plug-in can open it: other float formats (DXGI 94 and the like), cube maps, texture
// arrays. BC6H, RGBA16F, RGBA32F and FourCC 113/116 are claimed: they open at 32 bits (DE7). A file shorter than
// the headers is claimed, so the reader can say why it fails. Not a DDS (no magic): false.
bool dds_header_openable(const uint8_t *h, size_t len);

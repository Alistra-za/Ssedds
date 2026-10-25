// Tessalith DDS: Photoshop pixel samples to RGBA floats. MIT licence (see photoshop/LICENSE).
#include "pixels.h"

#include <stdint.h>

template <typename T>
static void convert(const T *src, size_t pixels, int channels, float scale, float *out) {
    for (size_t p = 0; p < pixels; p++, src += channels, out += 4) {
        out[0] = src[0] * scale;
        out[1] = src[1] * scale;
        out[2] = src[2] * scale;
        out[3] = channels == 4 ? src[3] * scale : 1.0f;
    }
}

int32_t band_rows(int64_t row_bytes, int32_t height, int64_t cap_bytes) {
    const int64_t rows = row_bytes > 0 ? cap_bytes / row_bytes : height;
    return rows < 1 ? 1 : rows > height ? height : int32_t(rows);
}

void to_rgba_floats(const void *src, size_t pixels, int channels, int depth, float *out) {
    if (depth == 16)
        convert(static_cast<const uint16_t *>(src), pixels, channels, 1.0f / 32768.0f, out);
    else
        convert(static_cast<const uint8_t *>(src), pixels, channels, 1.0f / 255.0f, out);
}

void deinterleave_rgba8_to_planes(const uint8_t *rgba, size_t pixels, int planes, uint8_t *out) {
    for (int c = 0; c < planes; c++, out += pixels)
        for (size_t p = 0; p < pixels; p++) out[p] = rgba[p * 4 + c];
}

static uint32_t le32(const uint8_t *h, size_t at) {
    return uint32_t(h[at]) | uint32_t(h[at + 1]) << 8 | uint32_t(h[at + 2]) << 16 | uint32_t(h[at + 3]) << 24;
}

bool dds_header_openable(const uint8_t *h, size_t len) {
    if (len < 4 || h[0] != 'D' || h[1] != 'D' || h[2] != 'S' || h[3] != ' ') return false;
    if (len < 128) return true;
    if (le32(h, 112) & 0x200) return false; // caps2 DDSCAPS2_CUBEMAP
    const uint32_t fourcc = le32(h, 84);
    if (fourcc >= 111 && fourcc <= 117) return false; // D3DFMT float formats (R16F .. A32B32G32R32F)
    if (fourcc != 0x30315844 /* "DX10" */ || len < 148) return true;
    const uint32_t dxgi = le32(h, 128);
    // DXGI float formats and BC6H: R32G32B32A32 2, R32G32B32 6, R16G16B16A16 10, R32G32 16, R11G11B10 26, R16G16 34,
    // R32 41, R16 54, R9G9B9E5 67, BC6H typeless/UF16/SF16 94..96.
    switch (dxgi) {
    case 2: case 6: case 10: case 16: case 26: case 34: case 41: case 54: case 67: case 94: case 95: case 96:
        return false;
    }
    if (le32(h, 136) & 0x4) return false; // miscFlag D3D11_RESOURCE_MISC_TEXTURECUBE
    return le32(h, 140) <= 1;             // arraySize
}

bool opens_with_transparency(const uint8_t *rgba, size_t pixels, uint32_t has_alpha) {
    if (!has_alpha) return false;
    for (size_t p = 0; p < pixels; p++)
        if (rgba[p * 4 + 3] < 254) return true;
    return false;
}

void dds_size_range(int64_t width, int64_t height, int64_t *min_bytes, int64_t *max_bytes) {
    // BC1/BC4 store a 4x4 block in 8 bytes.
    *min_bytes = ((width + 3) / 4) * ((height + 3) / 4) * 8 + 128;
    int64_t bytes = 148;
    for (int64_t w = width, h = height;; w = w > 1 ? w / 2 : 1, h = h > 1 ? h / 2 : 1) {
        bytes += w * h * 4;
        if (w == 1 && h == 1) break;
    }
    *max_bytes = bytes;
}

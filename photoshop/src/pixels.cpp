// Tessalith DDS: Photoshop pixel samples to RGBA floats. MIT licence (see photoshop/LICENSE).
#include "pixels.h"

#include "../psdds.h"

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

// The rules live in psdds (crates/psdds/src/helpers.rs); these keep the plug-in's names.
bool dds_header_openable(const uint8_t *h, size_t len) { return psdds_dds_openable(h, len) != 0; }

bool opens_with_transparency(const uint8_t *rgba, size_t pixels, uint32_t has_alpha) {
    return psdds_opens_with_alpha(rgba, pixels, has_alpha) != 0;
}

bool floats_have_transparency(const float *rgba, size_t pixels) { return psdds_alpha_used(rgba, pixels) != 0; }

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

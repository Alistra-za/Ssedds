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

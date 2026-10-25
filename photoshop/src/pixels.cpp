// Tessalith DDS: Photoshop pixel samples to RGBA floats. MIT licence (see photoshop/LICENSE).
#include "pixels.h"

#include "../psdds.h"

#include <stdint.h>
#include <stdio.h>

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
    if (depth == 32)
        convert(static_cast<const float *>(src), pixels, channels, 1.0f, out); // x * 1.0f keeps every float, NaN too
    else if (depth == 16)
        convert(static_cast<const uint16_t *>(src), pixels, channels, 1.0f / 32768.0f, out);
    else
        convert(static_cast<const uint8_t *>(src), pixels, channels, 1.0f / 255.0f, out);
}

static void put_float(char *&at, char *end, float v) {
    if (at >= end) return;
    at += v != v ? snprintf(at, size_t(end - at), "nan") : snprintf(at, size_t(end - at), "%.9g", double(v));
    if (at > end) at = end;
}

size_t describe_samples(const float *rgba, size_t pixels, char *out, size_t cap) {
    if (cap == 0) return 0;
    char *at = out, *end = out + cap - 1; // room for the NUL
    auto text = [&](const char *fmt, auto... args) {
        if (at < end) at += snprintf(at, size_t(end - at) + 1, fmt, args...);
        if (at > end) at = end;
    };
    text("px");
    for (size_t p = 0; p < pixels && p < 4; p++) {
        text(" (");
        for (int c = 0; c < 4; c++) {
            if (c) text(",");
            put_float(at, end, rgba[p * 4 + c]);
        }
        text(")");
    }
    float lo[4], hi[4];
    bool seen[4] = {};
    size_t nan = 0, above = 0, below = 0;
    for (size_t p = 0; p < pixels; p++)
        for (int c = 0; c < 4; c++) {
            const float v = rgba[p * 4 + c];
            if (v != v) {
                nan += c < 3;
                continue;
            }
            if (v - v != 0.0f) continue; // infinity: not finite
            if (!seen[c] || v < lo[c]) lo[c] = v;
            if (!seen[c] || v > hi[c]) hi[c] = v;
            seen[c] = true;
            if (c < 3) {
                above += v > 1.0f;
                below += v < 0.0f;
            }
        }
    for (int c = 0; c < 4; c++) {
        text(" %c[", "rgba"[c]);
        put_float(at, end, seen[c] ? lo[c] : 0.0f);
        text(",");
        put_float(at, end, seen[c] ? hi[c] : 0.0f);
        text("]");
    }
    text(" nan %zu above1 %zu below0 %zu", nan, above, below);
    *at = 0;
    return size_t(at - out);
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

void dds_size_range(int64_t width, int64_t height, int depth, int64_t *min_bytes, int64_t *max_bytes) {
    // BC1/BC4 store a 4x4 block in 8 bytes.
    *min_bytes = ((width + 3) / 4) * ((height + 3) / 4) * 8 + 128;
    const int64_t texel = depth > 8 ? 8 : 4; // RGBA16F above 8 bits
    int64_t bytes = 148;
    for (int64_t w = width, h = height;; w = w > 1 ? w / 2 : 1, h = h > 1 ? h / 2 : 1) {
        bytes += w * h * texel;
        if (w == 1 && h == 1) break;
    }
    *max_bytes = bytes;
}

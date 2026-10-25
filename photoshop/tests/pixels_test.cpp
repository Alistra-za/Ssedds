// Tessalith DDS: tests for pixels.cpp, built and run by `build.ps1 -Test`. MIT licence (see photoshop/LICENSE).
#include "../src/pixels.h"

#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

static int failures = 0;

static void check(bool ok, const char *test, const char *what) {
    if (!ok) {
        printf("  FAIL %s: %s\n", test, what);
        failures++;
    }
}

static bool near(float a, float b) { return a - b < 1e-6f && b - a < 1e-6f; }

static void sixteen_bit_white_is_white() {
    const uint16_t px[3] = {32768, 16384, 0};
    float out[4];
    to_rgba_floats(px, 1, 3, 16, out);
    check(near(out[0], 1.0f), __func__, "32768 -> 1.0");
    check(near(out[1], 0.5f), __func__, "16384 -> 0.5");
    check(near(out[2], 0.0f), __func__, "0 -> 0.0");
}

static void eight_bit_codes_scale_by_255() {
    const uint8_t px[3] = {255, 51, 0};
    float out[4];
    to_rgba_floats(px, 1, 3, 8, out);
    check(near(out[0], 1.0f), __func__, "255 -> 1.0");
    check(near(out[1], 0.2f), __func__, "51 -> 0.2");
    check(near(out[2], 0.0f), __func__, "0 -> 0.0");
}

static void thirty_two_bit_floats_pass_through() {
    const float px[4] = {0.5f, 4.0f, -0.25f, NAN};
    float out[4];
    to_rgba_floats(px, 1, 4, 32, out);
    check(out[0] == 0.5f && out[1] == 4.0f && out[2] == -0.25f, __func__, "RGB unchanged: no scale, no clamp");
    check(isnan(out[3]), __func__, "NaN kept");
    to_rgba_floats(px, 1, 3, 32, out);
    check(out[1] == 4.0f && out[3] == 1.0f, __func__, "3 channels: alpha becomes 1");
}

static void describe_samples_reports_range() {
    const float px[8] = {0.5f, 4.0f, -1.0f, 1.0f, NAN, 0.0f, 0.0f, 0.5f};
    char line[512];
    const size_t n = describe_samples(px, 2, line, sizeof line);
    check(n == strlen(line), __func__, "returns the length written");
    check(strcmp(line, "px (0.5,4,-1,1) (nan,0,0,0.5) r[0.5,0.5] g[0,4] b[-1,0] a[0.5,1] nan 1 above1 1 below0 1") == 0, __func__,
          line);
    char tiny[8];
    check(describe_samples(px, 2, tiny, sizeof tiny) == 7 && tiny[7] == 0, __func__, "a short buffer is cut and terminated");
}

static void interleaves_planes_with_alpha() {
    const uint8_t px[8] = {255, 0, 0, 51, 0, 255, 0, 255};
    float out[8];
    to_rgba_floats(px, 2, 4, 8, out);
    const float want[8] = {1.0f, 0.0f, 0.0f, 0.2f, 0.0f, 1.0f, 0.0f, 1.0f};
    for (int i = 0; i < 8; i++) check(near(out[i], want[i]), __func__, "RGBA order kept");
}

static void no_alpha_plane_gives_opaque() {
    const uint16_t px[6] = {0, 0, 0, 32768, 32768, 32768};
    float out[8];
    to_rgba_floats(px, 2, 3, 16, out);
    check(near(out[3], 1.0f), __func__, "pixel 0 alpha 1");
    check(near(out[7], 1.0f), __func__, "pixel 1 alpha 1");
    check(near(out[4], 1.0f), __func__, "pixel 1 red 1");
}

static void bands_cover_largest_image_within_cap() {
    const int64_t cap = int64_t(64) << 20;
    const int32_t height = 16384;
    const int64_t row_bytes = int64_t(16384) * 4 * 2;
    const int32_t rows = band_rows(row_bytes, height, cap);
    check(rows > 0 && rows * row_bytes <= cap, __func__, "one band fits the cap");
    static unsigned char seen[16384];
    for (int32_t top = 0; top < height; top += rows) {
        const int32_t n = top + rows > height ? height - top : rows;
        check(n * row_bytes <= cap, __func__, "every band fits the cap");
        for (int32_t y = top; y < top + n; y++) seen[y]++;
    }
    bool once = true;
    for (int32_t y = 0; y < height; y++) once = once && seen[y] == 1;
    check(once, __func__, "every row requested exactly once");
}

static void band_rows_edges() {
    check(band_rows(1000, 10, 64) == 1, __func__, "a row wider than the cap still gets one row");
    check(band_rows(300, 1000, 1000) == 3, __func__, "rows round down to fit");
    check(band_rows(300, 2, 1 << 20) == 2, __func__, "never more rows than the image");
}

static void bands_convert_into_their_rows() {
    // A 2x3 RGB 8-bit image delivered as bands of 2 rows and 1 row lands in the right rows of the float image.
    const uint8_t band0[12] = {255, 0, 0, 255, 0, 0, 0, 255, 0, 0, 255, 0};
    const uint8_t band1[6] = {0, 0, 255, 0, 0, 255};
    float image[2 * 3 * 4] = {};
    const size_t width = 2;
    to_rgba_floats(band0, width * 2, 3, 8, image + 0 * width * 4);
    to_rgba_floats(band1, width * 1, 3, 8, image + 2 * width * 4);
    check(near(image[0], 1.0f) && near(image[1 * 4 + 0], 1.0f), __func__, "row 0 red");
    check(near(image[2 * 4 + 1], 1.0f) && near(image[3 * 4 + 1], 1.0f), __func__, "row 1 green");
    check(near(image[4 * 4 + 2], 1.0f) && near(image[5 * 4 + 2], 1.0f), __func__, "row 2 blue");
    check(near(image[5 * 4 + 3], 1.0f), __func__, "row 2 opaque");
}

static void deinterleave_rgba8_to_planes_test() {
    // Two RGBA8 pixels become planar R, G, B (and A) blocks of `pixels` bytes each.
    const uint8_t px[8] = {10, 20, 30, 40, 50, 60, 70, 80};
    uint8_t out[8] = {};
    deinterleave_rgba8_to_planes(px, 2, 4, out);
    const uint8_t want4[8] = {10, 50, 20, 60, 30, 70, 40, 80};
    for (int i = 0; i < 8; i++) check(out[i] == want4[i], __func__, "4 planes: R, G, B, A blocks");
    uint8_t out3[8] = {0, 0, 0, 0, 0, 0, 99, 99};
    deinterleave_rgba8_to_planes(px, 2, 3, out3);
    const uint8_t want3[8] = {10, 50, 20, 60, 30, 70, 99, 99};
    for (int i = 0; i < 8; i++) check(out3[i] == want3[i], __func__, "3 planes: alpha dropped, nothing written past B");
}

static void deinterleave_rgba32f_to_planes_test() {
    const float px[8] = {1.5f, 2.5f, 3.5f, 4.5f, -5.0f, 6.0f, 70000.0f, 0.25f};
    uint8_t buf[1 + 32] = {}; // odd offset: no alignment assumed
    uint8_t *src = buf + 1;
    memcpy(src, px, sizeof px);
    uint8_t out[32];
    deinterleave_rgba32f_to_planes(src, 2, 4, out);
    const float want4[8] = {1.5f, -5.0f, 2.5f, 6.0f, 3.5f, 70000.0f, 4.5f, 0.25f};
    for (int i = 0; i < 8; i++) {
        float v;
        memcpy(&v, out + i * 4, 4);
        check(v == want4[i], __func__, "4 planes: R, G, B, A blocks");
    }
    uint8_t out3[32];
    memset(out3, 0x77, sizeof out3);
    deinterleave_rgba32f_to_planes(src, 2, 3, out3);
    float v;
    memcpy(&v, out3 + 5 * 4, 4);
    check(v == 70000.0f, __func__, "3 planes: B block");
    check(out3[24] == 0x77 && out3[31] == 0x77, __func__, "3 planes: nothing written past B");
}

static void largest_dds_is_16k_rgba8_full_chain() {
    // 16384^2 RGBA8 with 15 mip levels: 4 * (4^15 - 1) / 3 bytes of pixels, plus magic, header and DX10 header.
    check(kMaxDdsFileBytes == int64_t(4) * ((int64_t(1) << 30) - 1) / 3 + 4 + 124 + 20, __func__, "closed form");
    check(kMaxDdsFileBytes == 1431655912, __func__, "1,431,655,912 bytes");
}

static void header(uint8_t *h, size_t n, const char *fourcc, uint32_t caps2, uint32_t dxgi, uint32_t misc, uint32_t array) {
    memset(h, 0, n);
    memcpy(h, "DDS ", 4);
    if (fourcc) memcpy(h + 84, fourcc, 4);
    memcpy(h + 112, &caps2, 4);
    memcpy(h + 128, &dxgi, 4);
    memcpy(h + 136, &misc, 4);
    memcpy(h + 140, &array, 4);
}

static void filter_file_claims_only_openable_headers() {
    uint8_t h[148];
    header(h, sizeof h, "DXT5", 0, 0, 0, 0);
    check(dds_header_openable(h, 148), __func__, "legacy BC3 opens");
    header(h, sizeof h, "DX10", 0, 98, 0, 1);
    check(dds_header_openable(h, 148), __func__, "DX10 BC7, array size 1, opens");
    header(h, sizeof h, "DX10", 0, 95, 0, 1);
    check(dds_header_openable(h, 148), __func__, "BC6H UF16 claimed (DE7)");
    header(h, sizeof h, "DX10", 0, 96, 0, 1);
    check(dds_header_openable(h, 148), __func__, "BC6H SF16 claimed");
    header(h, sizeof h, "DX10", 0, 10, 0, 1);
    check(dds_header_openable(h, 148), __func__, "RGBA16F claimed");
    header(h, sizeof h, "DX10", 0, 2, 0, 1);
    check(dds_header_openable(h, 148), __func__, "RGBA32F claimed");
    const uint32_t others[3] = {6, 26, 94};
    for (uint32_t other : others) {
        header(h, sizeof h, "DX10", 0, other, 0, 1);
        check(!dds_header_openable(h, 148), __func__, "DXGI 6, 26 and 94 declined");
    }
    header(h, sizeof h, "DX10", 0, 98, 0x4, 1);
    check(!dds_header_openable(h, 148), __func__, "DX10 cube map declined");
    header(h, sizeof h, "DX10", 0, 98, 0, 6);
    check(!dds_header_openable(h, 148), __func__, "DX10 array declined");
    header(h, sizeof h, "DXT1", 0x200, 0, 0, 0);
    check(!dds_header_openable(h, 148), __func__, "legacy cube map (caps2 0x200) declined");
    header(h, sizeof h, nullptr, 0, 0, 0, 0);
    uint32_t f16 = 113;
    memcpy(h + 84, &f16, 4);
    check(dds_header_openable(h, 148), __func__, "legacy A16B16G16R16F (FourCC 113) claimed");
    header(h, sizeof h, "DXT5", 0, 0, 0, 0);
    check(dds_header_openable(h, 128), __func__, "128 bytes, no DX10 header needed");
    check(dds_header_openable(h, 40), __func__, "truncated file is claimed so the reader says why");
    check(!dds_header_openable(h, 3), __func__, "too short for the magic");
    h[0] = 'X';
    check(!dds_header_openable(h, 148), __func__, "wrong magic");
}

static void open_alpha_rule() {
    // Alpha >= 254 everywhere is the engine's opaque floor for BC7 (DECISIONS Notes A/C/D/S5): open opaque.
    const uint8_t opaque[8] = {1, 2, 3, 255, 4, 5, 6, 255};
    const uint8_t floor[8] = {1, 2, 3, 254, 4, 5, 6, 255};
    const uint8_t real[8] = {1, 2, 3, 255, 4, 5, 6, 253};
    const uint8_t cut[8] = {1, 2, 3, 0, 4, 5, 6, 255};
    check(!opens_with_transparency(opaque, 2, 1), __func__, "all 255: opaque");
    check(!opens_with_transparency(floor, 2, 1), __func__, "all >= 254 with some 254: opaque");
    check(opens_with_transparency(real, 2, 1), __func__, "a byte of 253: transparency");
    check(opens_with_transparency(cut, 2, 1), __func__, "a byte of 0: transparency");
    check(!opens_with_transparency(cut, 2, 0), __func__, "has_alpha 0: opaque whatever the bytes");
}

// Save decides alpha from the pixels with Open's threshold, after the host samples became floats.
static void save_alpha_rule() {
    const uint8_t floor8[8] = {1, 2, 3, 254, 4, 5, 6, 255};
    const uint8_t real8[8] = {1, 2, 3, 255, 4, 5, 6, 253};
    const uint16_t opaque16[8] = {1, 2, 3, 32768, 4, 5, 6, 32640}; // 32640 / 32768 is just above 254/255
    const uint16_t real16[8] = {1, 2, 3, 32768, 4, 5, 6, 32639};   // just below
    float f[8];
    to_rgba_floats(floor8, 2, 4, 8, f);
    check(!floats_have_transparency(f, 2), __func__, "8-bit alpha 254 and 255: opaque");
    to_rgba_floats(real8, 2, 4, 8, f);
    check(floats_have_transparency(f, 2), __func__, "8-bit alpha 253: transparency");
    to_rgba_floats(opaque16, 2, 4, 16, f);
    check(!floats_have_transparency(f, 2), __func__, "16-bit alpha >= 254/255: opaque");
    to_rgba_floats(real16, 2, 4, 16, f);
    check(floats_have_transparency(f, 2), __func__, "16-bit alpha below 254/255: transparency");
    to_rgba_floats(real8, 2, 3, 8, f);
    check(!floats_have_transparency(f, 2), __func__, "no alpha plane: opaque");
}

static void estimate_covers_every_format() {
    int64_t lo = 0, hi = 0;
    dds_size_range(1024, 1024, 8, &lo, &hi);
    // BC1/BC4 without mips is the smallest (half a byte per pixel); RGBA8 with a full chain the largest.
    check(lo == 1024 * 1024 / 2 + 128, __func__, "minimum: BC1, no mips, legacy header");
    check(hi == int64_t(4) * (int64_t(1024) * 1024 * 4 - 1) / 3 + 148, __func__, "maximum: RGBA8, full chain");
    dds_size_range(16384, 16384, 8, &lo, &hi);
    check(hi == kMaxDdsFileBytes, __func__, "16k maximum equals the Open cap");
    dds_size_range(16384, 16384, 32, &lo, &hi);
    check(hi == (kMaxDdsFileBytes - 148) * 2 + 148, __func__, "32-bit maximum: RGBA16F, full chain");
    dds_size_range(1024, 1024, 16, &lo, &hi);
    check(lo == 1024 * 1024 / 2 + 128 && hi == int64_t(8) * (int64_t(1024) * 1024 * 4 - 1) / 3 + 148, __func__, "16-bit: BC1 minimum, RGBA16F maximum");
}

int main() {
    struct {
        const char *name;
        void (*run)();
    } tests[] = {
        {"sixteen_bit_white_is_white", sixteen_bit_white_is_white},
        {"eight_bit_codes_scale_by_255", eight_bit_codes_scale_by_255},
        {"thirty_two_bit_floats_pass_through", thirty_two_bit_floats_pass_through},
        {"describe_samples_reports_range", describe_samples_reports_range},
        {"interleaves_planes_with_alpha", interleaves_planes_with_alpha},
        {"no_alpha_plane_gives_opaque", no_alpha_plane_gives_opaque},
        {"bands_cover_largest_image_within_cap", bands_cover_largest_image_within_cap},
        {"band_rows_edges", band_rows_edges},
        {"bands_convert_into_their_rows", bands_convert_into_their_rows},
        {"deinterleave_rgba8_to_planes", deinterleave_rgba8_to_planes_test},
        {"deinterleave_rgba32f_to_planes", deinterleave_rgba32f_to_planes_test},
        {"largest_dds_is_16k_rgba8_full_chain",largest_dds_is_16k_rgba8_full_chain},
        {"filter_file_claims_only_openable_headers", filter_file_claims_only_openable_headers},
        {"open_alpha_rule", open_alpha_rule},
        {"save_alpha_rule", save_alpha_rule},
        {"estimate_covers_every_format", estimate_covers_every_format},
    };
    for (auto &t : tests) {
        int before = failures;
        t.run();
        printf("%s %s\n", failures == before ? "PASS" : "FAIL", t.name);
    }
    printf("pixels_test: %s\n", failures == 0 ? "all passed" : "FAILED");
    return failures == 0 ? 0 : 1;
}

// Tessalith DDS: tests for pixels.cpp, built and run by `build.ps1 -Test`. MIT licence (see photoshop/LICENSE).
#include "../src/pixels.h"

#include <stdint.h>
#include <stdio.h>

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

int main() {
    struct {
        const char *name;
        void (*run)();
    } tests[] = {
        {"sixteen_bit_white_is_white", sixteen_bit_white_is_white},
        {"eight_bit_codes_scale_by_255", eight_bit_codes_scale_by_255},
        {"interleaves_planes_with_alpha", interleaves_planes_with_alpha},
        {"no_alpha_plane_gives_opaque", no_alpha_plane_gives_opaque},
        {"bands_cover_largest_image_within_cap", bands_cover_largest_image_within_cap},
        {"band_rows_edges", band_rows_edges},
        {"bands_convert_into_their_rows", bands_convert_into_their_rows},
    };
    for (auto &t : tests) {
        int before = failures;
        t.run();
        printf("%s %s\n", failures == before ? "PASS" : "FAIL", t.name);
    }
    printf("pixels_test: %s\n", failures == 0 ? "all passed" : "FAILED");
    return failures == 0 ? 0 : 1;
}

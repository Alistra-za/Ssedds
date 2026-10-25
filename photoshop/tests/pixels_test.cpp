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

int main() {
    struct {
        const char *name;
        void (*run)();
    } tests[] = {
        {"sixteen_bit_white_is_white", sixteen_bit_white_is_white},
        {"eight_bit_codes_scale_by_255", eight_bit_codes_scale_by_255},
        {"interleaves_planes_with_alpha", interleaves_planes_with_alpha},
        {"no_alpha_plane_gives_opaque", no_alpha_plane_gives_opaque},
    };
    for (auto &t : tests) {
        int before = failures;
        t.run();
        printf("%s %s\n", failures == before ? "PASS" : "FAIL", t.name);
    }
    printf("pixels_test: %s\n", failures == 0 ? "all passed" : "FAILED");
    return failures == 0 ? 0 : 1;
}

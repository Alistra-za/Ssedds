// Tessalith DDS: Photoshop pixel samples to RGBA floats. MIT licence (see photoshop/LICENSE).
#pragma once

#include <stddef.h>

// Converts `pixels` interleaved pixels from Photoshop into RGBA floats in [0, 1] (Note DC6):
// 8-bit samples (uint8) scale by 1/255, 16-bit samples (uint16, Photoshop's 0..32768 range) by 1/32768.
// `channels` is 3 (RGB, alpha becomes 1) or 4 (RGB plus the alpha source). `out` receives pixels * 4 floats.
void to_rgba_floats(const void *src, size_t pixels, int channels, int depth, float *out);

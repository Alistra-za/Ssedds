// Tessalith DDS: the Save path. Takes the whole image in one request, encodes it with psdds and writes the DDS
// file through the host's file handle. MIT licence (see photoshop/LICENSE).
#include "write.h"

#include "../psdds.h"
#include "pixels.h"

#include <stdlib.h>
#include <vector>

int16 write_start(FormatRecord *r) {
    const bool rgb = r->imageMode == plugInModeRGBColor || r->imageMode == plugInModeRGB48;
    if (!rgb || (r->depth != 8 && r->depth != 16))
        return report_error(r, "Tessalith DDS saves only 8-bit and 16-bit RGB documents.");
    if (!r->HostSupports32BitCoordinates)
        return report_error(r, "this Photoshop version is too old for Tessalith DDS.");

    const int32 width = r->imageSize32.h, height = r->imageSize32.v;
    if (width <= 0 || height <= 0) return report_error(r, "the image is empty.");

    // Spec "Save dialog": alpha from the transparency plane first, else the first alpha channel (plane 3).
    const bool has_alpha = r->planes > 3;
    const int16 channels = has_alpha ? 4 : 3;
    const int16 sample = r->depth / 8;
    void *buffer = malloc(size_t(width) * size_t(height) * size_t(channels) * size_t(sample));
    if (!buffer) return report_error(r, "there is not enough memory to save this image.");

    r->data = buffer;
    r->theRect32.top = 0;
    r->theRect32.left = 0;
    r->theRect32.bottom = height;
    r->theRect32.right = width;
    r->loPlane = 0;
    r->hiPlane = channels - 1;
    r->colBytes = channels * sample;
    r->rowBytes = width * channels * sample;
    r->planeBytes = sample;
    for (int16 i = 0; i < 3; i++) r->planeMap[i] = i;
    if (has_alpha) r->planeMap[3] = int16(r->transparencyPlane > 0 ? r->transparencyPlane : 3);
    return noErr;
}

static bool write_all(HANDLE file, const uint8_t *bytes, size_t len) {
    while (len > 0) {
        const DWORD chunk = len > 0x40000000 ? 0x40000000 : DWORD(len);
        DWORD written = 0;
        if (!WriteFile(file, bytes, chunk, &written, nullptr) || written == 0) return false;
        bytes += written;
        len -= written;
    }
    return true;
}

int16 write_continue(FormatRecord *r) {
    // Every exit ends the request loop: our buffer freed, data NULL and an empty rectangle.
    struct EndRequests {
        FormatRecord *r;
        ~EndRequests() {
            free(r->data);
            r->data = nullptr;
            r->theRect32 = VRect{};
        }
    } end{r};

    const uint32_t width = uint32_t(r->imageSize32.h), height = uint32_t(r->imageSize32.v);
    const int channels = r->hiPlane - r->loPlane + 1;
    const uint32_t has_alpha = channels == 4 ? 1 : 0;
    const size_t pixels = size_t(width) * height;

    std::vector<float> rgba(pixels * 4);
    to_rgba_floats(r->data, pixels, channels, r->depth, rgba.data());
    free(r->data);
    r->data = nullptr;

    const PsddsSettings defaults = psdds_defaults(PSDDS_COLOUR);
    PsddsSettings effective;
    PsddsControls controls;
    if (psdds_resolve(&defaults, has_alpha, &effective, &controls) != 0)
        return report_error(r, "internal error: the default settings were refused.");

    PsddsBuf dds = {};
    char err[256] = {};
    if (psdds_encode(rgba.data(), width, height, has_alpha, &effective, &dds, err, sizeof err) != 0)
        return report_error(r, err[0] ? err : "the DDS encoder failed.");

    // dataFork is the Windows file handle of the open, empty output file (PIFormat.h).
    const bool ok = write_all(reinterpret_cast<HANDLE>(r->dataFork), dds.data, dds.len);
    psdds_free(dds);
    return ok ? noErr : report_error(r, "the DDS file could not be written.");
}

int16 write_finish(FormatRecord *r) {
    // Reached after a cancel or an error, when write_continue may not have freed the buffer.
    free(r->data);
    r->data = nullptr;
    return noErr;
}

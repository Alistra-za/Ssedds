// Tessalith DDS: the Open path. Reads the whole file through the host's file handle, decodes the top mip with psdds
// (RGBA8, or RGBA f32 for BC6H and the float formats, DF7) and hands it to Photoshop as an 8-bit or 32-bit RGB
// document in planar bands of whole rows.
// MIT licence (see photoshop/LICENSE).
#include "write.h"

#include "pixels.h"

#include <vector>

struct ReadState {
    PsddsBuf pixels = {};      // psdds_decode's RGBA8 image; freed exactly once in free_read
    std::vector<uint8_t> band; // one planar band for the host
    int32 rows_per_band = 0;
    int32 next_top = 0;        // first row of the band after the one the host holds
    int16 planes = 3;
    uint32_t depth = 8;        // 8: RGBA8 pixels; 32: RGBA f32 pixels (DF7)
};

namespace {

void free_read(FormatRecord *r, intptr_t *data) {
    if (*data) {
        Globals *g = reinterpret_cast<Globals *>(*data);
        if (g->read) psdds_free(g->read->pixels);
        delete g->read;
        g->read = nullptr;
    }
    r->data = nullptr;
}

bool read_all(HANDLE file, uint8_t *bytes, size_t len) {
    LARGE_INTEGER start = {};
    if (!SetFilePointerEx(file, start, nullptr, FILE_BEGIN)) return false;
    while (len > 0) {
        const DWORD chunk = len > 0x40000000 ? 0x40000000 : DWORD(len);
        DWORD got = 0;
        if (!ReadFile(file, bytes, chunk, &got, nullptr) || got == 0) return false;
        bytes += got;
        len -= got;
    }
    return true;
}

// Describes rows [top, top + rows_per_band) to the host as planar 8-bit planes 0..planes-1.
void hand_band(FormatRecord *r, ReadState *s, int32 top) {
    const int32 width = r->imageSize32.h, height = r->imageSize32.v;
    const int32 bottom = top + s->rows_per_band > height ? height : top + s->rows_per_band;
    const size_t pixels = size_t(width) * size_t(bottom - top);
    const int16 sample = s->depth == 32 ? 4 : 1; // bytes per sample
    const uint8_t *from = s->pixels.data + size_t(top) * size_t(width) * 4 * size_t(sample);
    if (s->depth == 32)
        deinterleave_rgba32f_to_planes(from, pixels, s->planes, s->band.data());
    else
        deinterleave_rgba8_to_planes(from, pixels, s->planes, s->band.data());
    r->theRect32.top = top;
    r->theRect32.left = 0;
    r->theRect32.bottom = bottom;
    r->theRect32.right = width;
    r->loPlane = 0;
    r->hiPlane = s->planes - 1;
    r->colBytes = sample;
    r->rowBytes = width * sample;
    r->planeBytes = int32(pixels * size_t(sample));
    for (int16 i = 0; i < s->planes; i++) r->planeMap[i] = i;
    r->data = s->band.data();
    s->next_top = bottom;
}

// ReadStart's work once the state exists: read, decode, describe the document, hand the first band.
int16 decode_and_describe(FormatRecord *r, HANDLE file, size_t size, ReadState *s) {
    uint32_t width = 0, height = 0, has_alpha = 0;
    char err[256] = {};
    {
        std::vector<uint8_t> bytes(size); // freed before the image goes to the host
        if (!read_all(file, bytes.data(), bytes.size())) return report_error(r, "the DDS file could not be read.");
        if (psdds_decode_depth(bytes.data(), bytes.size(), &width, &height, &has_alpha, &s->depth, &s->pixels, err,
                               sizeof err) != 0)
            return report_error(r, err[0] ? as_reason(err) : "of an internal error in Tessalith DDS.");
    }
    if (width == 0 || height == 0) return report_error(r, "the DDS image has no pixels.");
    if (s->depth != 8 && s->depth != 32) return report_error(r, "of an internal error in Tessalith DDS.");
    // An opaque file (alpha >= 254, DC23) opens without transparency, so a re-save keeps the colour mip filter
    // instead of the alpha rule's Box. A float file's has_alpha already applies that rule (psdds, DF4).
    const bool translucent = s->depth == 32
                                 ? has_alpha != 0
                                 : opens_with_transparency(s->pixels.data, size_t(width) * size_t(height), has_alpha);
    s->planes = translucent ? 4 : 3;

    // RGB mode at depth 8 or 32; Photoshop derives the deep mode from depth (RGB96 on read is rejected, DF7). No ICC profile.
    r->imageMode = plugInModeRGBColor;
    r->depth = int16(s->depth);
    r->planes = s->planes;
    r->imageSize32.h = int32(width);
    r->imageSize32.v = int32(height);
    r->imageHRes = r->imageVRes = Fixed(72) << 16;
    // PIFormat.h: -1 means the host supports transparency and the plug-in names the plane. A host that passes 0
    // gets the fourth plane as an alpha channel instead.
    if (r->transparencyPlane == -1) r->transparencyPlane = translucent ? 3 : 0;

    const int32 sample = s->depth == 32 ? 4 : 1;
    s->rows_per_band = band_rows(int64_t(width) * s->planes * sample, int32(height), kBandCap);
    s->band.resize(size_t(width) * size_t(s->rows_per_band) * size_t(s->planes) * size_t(sample));
    hand_band(r, s, 0);
    return noErr;
}

} // namespace

int16 filter_file(FormatRecord *r) {
    // PIFormat.h FilterFile: claim only files this plug-in can open, so Photoshop offers the rest (other float
    // formats, cube maps, texture arrays, non-DDS) to other plug-ins or shows its own message. Only the headers are read.
    uint8_t head[148] = {};
    DWORD got = 0;
    const HANDLE file = reinterpret_cast<HANDLE>(r->dataFork);
    LARGE_INTEGER start = {};
    const bool read = SetFilePointerEx(file, start, nullptr, FILE_BEGIN) && ReadFile(file, head, sizeof head, &got, nullptr);
    return read && dds_header_openable(head, got) ? noErr : formatCannotRead;
}

int16 read_start(FormatRecord *r, intptr_t *data) {
    if (!r->HostSupports32BitCoordinates) return report_error(r, "this Photoshop version is too old for Tessalith DDS.");
    r->layerData = 0; // the PiPL claims layers (DC38); a DDS opens as one composite

    const HANDLE file = reinterpret_cast<HANDLE>(r->dataFork); // the open file's Windows handle (PIFormat.h dataFork)
    LARGE_INTEGER size = {};
    if (!GetFileSizeEx(file, &size)) return report_error(r, "the DDS file could not be read.");
    // Refused before allocating: nothing this plug-in opens is larger (pixels.h kMaxDdsFileBytes).
    if (size.QuadPart > kMaxDdsFileBytes)
        return report_error(r, "this file is too large to be a DDS this plugin can open.");

    if (size.QuadPart == 0) return report_error(r, "the DDS file is empty.");

    Globals *g = globals_of(data);
    free_read(r, data); // a stale state from an earlier open that never reached ReadFinish
    g->read = new ReadState;
    // ReadFinish follows only a successful ReadStart, so every failure frees the state here.
    try {
        const int16 result = decode_and_describe(r, file, size_t(size.QuadPart), g->read);
        if (result != noErr) free_read(r, data);
        return result;
    } catch (...) {
        free_read(r, data);
        throw;
    }
}

int16 read_continue(FormatRecord *r, intptr_t *data) {
    ReadState *s = *data ? reinterpret_cast<Globals *>(*data)->read : nullptr;
    if (!s) return report_error(r, "of an internal error in Tessalith DDS.");
    if (s->next_top >= r->imageSize32.v) { // the host has the last band: data = NULL ends the read
        free_read(r, data);
        return noErr;
    }
    hand_band(r, s, s->next_top);
    return noErr;
}

int16 read_finish(FormatRecord *r, intptr_t *data) {
    // Reached after a cancel or an error too, when read_continue has not freed the state.
    free_read(r, data);
    return noErr;
}

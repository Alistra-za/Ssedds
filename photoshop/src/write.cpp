// Tessalith DDS: the Save path. Requests the image in bands of whole rows, converts each band into one float
// image, encodes it with psdds and writes the DDS file through the host's file handle.
// MIT licence (see photoshop/LICENSE).
#include "write.h"

#include "dialog.h"
#include "pixels.h"
#include "settings_store.h"

#include <ctype.h>
#include <vector>

namespace {

// Largest single request: PIFormat.h asks for pieces no larger than maxData; we set maxData to 0 in WritePrepare
// and use our own memory, so this cap keeps each band small instead.
constexpr int64_t kBandCap = int64_t(64) << 20;
// psdds refuses larger sides; checked here too so the size message wins over a failed allocation.
constexpr int32 kMaxSide = 16384;

} // namespace

struct SaveState {
    std::vector<float> rgba;     // the whole image, width * height * 4
    std::vector<uint8_t> band;   // one band of host samples
    int32 rows_per_band = 0;
};

namespace {

Globals *globals_of(intptr_t *data) {
    if (!*data) *data = reinterpret_cast<intptr_t>(new Globals);
    return reinterpret_cast<Globals *>(*data);
}

SaveState *state_of(intptr_t *data) { return *data ? reinterpret_cast<Globals *>(*data)->save : nullptr; }

// Frees the image buffers; the globals (and the chosen settings) stay.
void free_state(FormatRecord *r, intptr_t *data) {
    if (*data) {
        Globals *g = reinterpret_cast<Globals *>(*data);
        delete g->save;
        g->save = nullptr;
    }
    r->data = nullptr;
    r->theRect32 = VRect{};
}

// The host plane holding the alpha source, or -1 for none. Spec "Save dialog": the transparency plane first, else
// the first alpha channel. A spot channel is never alpha (PIGeneral.h channel types): the document's channel list
// marks them ctSpotColor. If the host also sends spot channels as planes, the list order is the plane order after
// RGB; if it drops them (the PiPL does not claim spot-channel support), the first non-spot channel is plane 3.
int16 alpha_plane(const FormatRecord *r) {
    if (r->transparencyPlane > 0) return int16(r->transparencyPlane);
    if (r->planes <= 3) return -1;
    const ReadImageDocumentDesc *doc = r->documentInfo;
    if (!doc || !doc->alphaChannels) return 3;
    int listed = 0, first_alpha = -1;
    for (const ReadChannelDesc *c = doc->alphaChannels; c; c = c->next, listed++)
        if (first_alpha < 0 && c->channelType != ctSpotColor) first_alpha = listed;
    if (first_alpha < 0) return -1;
    return int16(r->planes - 3 == listed ? 3 + first_alpha : 3);
}

void request_band(FormatRecord *r, SaveState *s, int32 top) {
    const int32 height = r->imageSize32.v;
    r->theRect32.top = top;
    r->theRect32.left = 0;
    r->theRect32.bottom = top + s->rows_per_band > height ? height : top + s->rows_per_band;
    r->theRect32.right = r->imageSize32.h;
    r->data = s->band.data();
}

bool write_all(HANDLE file, const uint8_t *bytes, size_t len) {
    while (len > 0) {
        const DWORD chunk = len > 0x40000000 ? 0x40000000 : DWORD(len);
        DWORD written = 0;
        if (!WriteFile(file, bytes, chunk, &written, nullptr) || written == 0) return false;
        bytes += written;
        len -= written;
    }
    return true;
}

// psdds messages start with a capital; Photoshop puts "Cannot complete operation because " in front.
const char *as_reason(char *message) {
    if (isupper(static_cast<unsigned char>(message[0])) && islower(static_cast<unsigned char>(message[1])))
        message[0] = char(tolower(static_cast<unsigned char>(message[0])));
    return message;
}

int16 encode_and_write(FormatRecord *r, const PsddsSettings &chosen, SaveState *s, uint32_t has_alpha) {
    PsddsSettings effective;
    PsddsControls controls;
    if (psdds_resolve(&chosen, has_alpha, &effective, &controls) != 0)
        return report_error(r, "of an internal error in Tessalith DDS.");

    PsddsBuf dds = {};
    char err[256] = {};
    if (psdds_encode(s->rgba.data(), uint32_t(r->imageSize32.h), uint32_t(r->imageSize32.v), has_alpha,
                     &effective, &dds, err, sizeof err) != 0)
        return report_error(r, err[0] ? as_reason(err) : "of an internal error in Tessalith DDS.");

    // dataFork is the Windows file handle of the open, empty output file (PIFormat.h).
    const bool ok = write_all(reinterpret_cast<HANDLE>(r->dataFork), dds.data, dds.len);
    psdds_free(dds);
    return ok ? noErr : report_error(r, "the DDS file could not be written.");
}

} // namespace

int16 options_start(FormatRecord *r, intptr_t *data) {
    r->data = nullptr; // no OptionsContinue: the options do not depend on the pixels
    // shortcut: scripting parameters (descriptorParameters) are not read or written, so actions save with the
    // remembered settings; add them when recorded actions need their own settings.
    Globals *g = globals_of(data);
    PsddsSettings chosen = load_settings(r->sSPBasic);
    const PlatformData *platform = static_cast<const PlatformData *>(r->platformData);
    HWND owner = platform ? reinterpret_cast<HWND>(platform->hwnd) : nullptr;
    // The same has_alpha the write will pass to psdds (alpha_plane), so the dialog greys what the encoder does.
    if (!ShowSaveDialog(owner, alpha_plane(r) >= 0 ? 1 : 0, &chosen)) return userCanceledErr;
    save_settings(r->sSPBasic, chosen);
    g->settings = chosen;
    g->have_settings = true;
    return noErr;
}

int16 write_start(FormatRecord *r, intptr_t *data) {
    const bool rgb = r->imageMode == plugInModeRGBColor || r->imageMode == plugInModeRGB48;
    if (!rgb || (r->depth != 8 && r->depth != 16))
        return report_error(r, "Tessalith DDS saves only 8-bit and 16-bit RGB documents.");
    if (!r->HostSupports32BitCoordinates)
        return report_error(r, "this Photoshop version is too old for Tessalith DDS.");

    const int32 width = r->imageSize32.h, height = r->imageSize32.v;
    if (width <= 0 || height <= 0) return report_error(r, "the image is empty.");
    if (width > kMaxSide || height > kMaxSide)
        return report_error(r, "this image is larger than 16384 pixels on a side, the most Skyrim supports.");

    const int16 alpha = alpha_plane(r);
    const int16 channels = alpha >= 0 ? 4 : 3;
    const int16 sample = r->depth / 8;
    const int64_t row_bytes = int64_t(width) * channels * sample;

    Globals *g = globals_of(data);
    if (!g->have_settings) { // saved without OptionsStart (e.g. a script): the remembered settings
        g->settings = load_settings(r->sSPBasic);
        g->have_settings = true;
    }
    delete g->save; // a stale state from an earlier save that never reached WriteFinish
    g->save = nullptr;
    auto *s = new SaveState;
    g->save = s;
    s->rgba.resize(size_t(width) * size_t(height) * 4);
    s->rows_per_band = band_rows(row_bytes, height, kBandCap);
    s->band.resize(size_t(row_bytes) * size_t(s->rows_per_band));

    r->loPlane = 0;
    r->hiPlane = channels - 1;
    r->colBytes = channels * sample;
    r->rowBytes = int32(row_bytes);
    r->planeBytes = sample;
    for (int16 i = 0; i < 3; i++) r->planeMap[i] = i;
    if (alpha >= 0) r->planeMap[3] = alpha;
    request_band(r, s, 0);
    return noErr;
}

int16 write_continue(FormatRecord *r, intptr_t *data) {
    SaveState *s = state_of(data);
    if (!s) return report_error(r, "of an internal error in Tessalith DDS.");

    const size_t width = size_t(r->imageSize32.h);
    const int32 top = r->theRect32.top, bottom = r->theRect32.bottom;
    const int channels = r->hiPlane - r->loPlane + 1;
    to_rgba_floats(s->band.data(), width * size_t(bottom - top), channels, r->depth,
                   s->rgba.data() + size_t(top) * width * 4);

    if (bottom < r->imageSize32.v) {
        request_band(r, s, bottom);
        return noErr;
    }
    // Last band: the request loop ends here whatever the outcome.
    const int16 result = encode_and_write(r, reinterpret_cast<Globals *>(*data)->settings, s, channels == 4 ? 1 : 0);
    free_state(r, data);
    return result;
}

int16 write_finish(FormatRecord *r, intptr_t *data) {
    // Reached after a cancel or an error too, when write_continue has not freed the state.
    free_state(r, data);
    return noErr;
}

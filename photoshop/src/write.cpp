// Tessalith DDS: the Save path. Requests the image in bands of whole rows, converts each band into one float
// image, encodes it with psdds and writes the DDS file through the host's file handle.
// MIT licence (see photoshop/LICENSE).
#include "write.h"

#include "dialog.h"
#include "pixels.h"
#include "settings_store.h"

#include <ctype.h>
#include <stdio.h>
#include <vector>

namespace {

// psdds refuses larger sides; checked here too so the size message wins over a failed allocation.
constexpr int32 kMaxSide = 16384;

// A save that ends before the encoded bytes exist; the file was never written to (Note DC27).
const char kNothingSaved[] = "Tessalith DDS could not read the image from Photoshop, so nothing was saved.";

} // namespace

struct SaveState {
    std::vector<float> rgba;     // the whole image, width * height * 4
    std::vector<uint8_t> band;   // one band of host samples
    int32 rows_per_band = 0;
};

Globals *globals_of(intptr_t *data) {
    if (!*data) *data = reinterpret_cast<intptr_t>(new Globals);
    return reinterpret_cast<Globals *>(*data);
}

const char *as_reason(char *message) {
    if (isupper(static_cast<unsigned char>(message[0])) && islower(static_cast<unsigned char>(message[1])))
        message[0] = char(tolower(static_cast<unsigned char>(message[0])));
    return message;
}


namespace {

SaveState *state_of(intptr_t *data) { return *data ? reinterpret_cast<Globals *>(*data)->save : nullptr; }

// Frees the image buffers; the globals (and the chosen settings) stay.
void free_state(FormatRecord *r, intptr_t *data) {
    if (*data) {
        Globals *g = reinterpret_cast<Globals *>(*data);
        delete g->save;
        g->save = nullptr;
        g->have_settings = false; // the next save without OptionsStart loads the remembered settings again
        g->show_dialog = false;
    }
    r->data = nullptr;
    r->theRect32 = VRect{};
}

// The host plane holding the alpha source, or -1 for none (DC42): the first non-spot alpha channel, else the
// transparency plane, else none. A spot channel is never alpha (PIGeneral.h channel types): the document's channel
// list marks them ctSpotColor. The planes after RGB are the transparency plane (if any), then the channels in list
// order; if the host drops the spot channels (the PiPL does not claim spot-channel support), the first non-spot
// channel is the first plane after those.
int16 alpha_plane(const FormatRecord *r) {
    const int base = r->transparencyPlane > 0 ? int(r->transparencyPlane) + 1 : 3;
    const int16 fallback = r->transparencyPlane > 0 ? int16(r->transparencyPlane) : int16(-1);
    if (r->planes <= base) return fallback;
    const ReadImageDocumentDesc *doc = r->documentInfo;
    if (!doc || !doc->alphaChannels) return int16(base);
    int listed = 0, first_alpha = -1;
    for (const ReadChannelDesc *c = doc->alphaChannels; c; c = c->next, listed++)
        if (first_alpha < 0 && c->channelType != ctSpotColor) first_alpha = listed;
    if (first_alpha < 0) return fallback;
    return int16(r->planes - base == listed ? base + first_alpha : base);
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

int16 encode_and_write(FormatRecord *r, const PsddsSettings &chosen, SaveState *s, uint32_t has_alpha) {
    PsddsSettings effective;
    PsddsControls controls;
    if (psdds_resolve_depth(&chosen, has_alpha, uint32_t(r->depth), &effective, &controls, nullptr) != 0)
        return report_error(r, "of an internal error in Tessalith DDS.");

    PsddsBuf dds = {};
    char err[256] = {};
    if (psdds_encode_depth(s->rgba.data(), uint32_t(r->imageSize32.h), uint32_t(r->imageSize32.v), has_alpha, uint32_t(r->depth),
                     &effective, &dds, err, sizeof err) != 0)
        return report_error(r, err[0] ? as_reason(err) : "of an internal error in Tessalith DDS.");

    // dataFork is the Windows file handle of the open, empty output file (PIFormat.h).
    // The only write of a save, after the encode succeeded. SetEndOfFile drops any old tail if the host passed an
    // existing file without truncating it.
    const HANDLE file = reinterpret_cast<HANDLE>(r->dataFork);
    const bool ok = write_all(file, dds.data, dds.len) && SetEndOfFile(file);
    psdds_free(dds);
    return ok ? noErr : report_error(r, "the DDS file could not be written.");
}

// The last-band step shared by the band path and the merged path (DC38): alpha from the pixels, the dialog, the
// encode and the single write; frees the save state whatever the outcome.
// `path` is "band" or "merged", for the trace.
int16 finish_save(FormatRecord *r, intptr_t *data, SaveState *s, int channels, const char *path) {
    Globals *g = reinterpret_cast<Globals *>(*data);
    if (trace_on()) {
        char note[640];
        const int head = snprintf(note, sizeof note, "samples %s depth %d channels %d: ", path, int(r->depth), channels);
        describe_samples(s->rgba.data(), s->rgba.size() / 4, note + head, sizeof note - size_t(head));
        trace_note(note);
    }
    // Alpha from the pixels, not the planes (DC31): a normal layer always has a transparency plane.
    const uint32_t has_alpha = channels == 4 && floats_have_transparency(s->rgba.data(), s->rgba.size() / 4) ? 1 : 0;
    int16 result = noErr;
    if (g->show_dialog) { // modal UI inside the write is allowed without SupportsBackgroundSave (PiPL keys)
        const PlatformData *platform = static_cast<const PlatformData *>(r->platformData);
        HWND owner = platform ? reinterpret_cast<HWND>(platform->hwnd) : nullptr;
        const DialogResult shown = ShowSaveDialog(owner, has_alpha, uint32_t(r->depth), &g->settings);
        if (shown == DialogResult::Cancel) result = userCanceledErr; // nothing has been written
        else if (shown == DialogResult::Failed) result = report_error(r, "the Tessalith DDS options dialog could not be shown");
        else save_settings(r->sSPBasic, g->settings);
    }
    if (result == noErr) result = encode_and_write(r, g->settings, s, has_alpha);
    free_state(r, data);
    return result;
}

// A channel-port channel usable for this save: a port, the image's depth and the image's size (Note DC38).
bool usable_channel(const FormatRecord *r, const ReadChannelDesc *c) {
    return c && c->port && c->depth == r->depth && c->bounds.right - c->bounds.left == r->imageSize32.h &&
           c->bounds.bottom - c->bounds.top == r->imageSize32.v;
}

// Photoshop's merged composite for a layered save (DC38): R, G, B, then the alpha source, the same policy as
// alpha_plane (DC42): the first non-spot alpha channel, else the merged transparency, else none. Returns 3 or 4 and fills
// ch, or 0 with `why` set when something needed is missing or does not match the image.
int merged_source(const FormatRecord *r, const ReadChannelDesc *ch[4], const char **why) {
    const ReadImageDocumentDesc *doc = r->documentInfo;
    if (!doc) return *why = "no documentInfo", 0;
    if (!r->channelPortProcs || !r->channelPortProcs->readPixelsProc) return *why = "no channel ports", 0;
    const ReadChannelDesc *c = doc->mergedCompositeChannels;
    for (int i = 0; i < 3; i++, c = c ? c->next : nullptr) {
        if (!usable_channel(r, c)) return *why = "merged colour channels missing or not the image's depth and size", 0;
        ch[i] = c;
    }
    const ReadChannelDesc *alpha = nullptr;
    for (const ReadChannelDesc *a = doc->alphaChannels; a && !alpha; a = a->next)
        if (a->channelType != ctSpotColor) alpha = a;
    if (!alpha) alpha = doc->mergedTransparency;
    if (!alpha) return 3;
    if (!usable_channel(r, alpha)) return *why = "the alpha source is not the image's depth and size", 0;
    ch[3] = alpha;
    return 4;
}

// Reads the merged composite in bands through the channel ports, each channel written straight into its slot of
// the interleaved band (PixelMemoryDesc colBits), at the document depth: the same samples the band path gets
// (16-bit 0..32768), so to_rgba_floats converts them unchanged. Then finishes as the last band would. A failed or
// short read writes nothing.
int16 write_merged(FormatRecord *r, intptr_t *data, const ReadChannelDesc *const ch[4], int channels) {
    const int32 width = r->imageSize32.h, height = r->imageSize32.v;
    const int32 sample = r->depth / 8;
    const int64_t row_bytes = int64_t(width) * channels * sample;
    Globals *g = globals_of(data);
    delete g->save; // a stale state from an earlier save that never reached WriteFinish
    auto *s = new SaveState;
    g->save = s;
    // shortcut: the whole image as floats (about 4 GiB at 16384 x 16384) plus the engine's work, and a 32-bit save of that size writes an RGBA16F file over the Open cap (preflight B11, Note DE6); fine at sane sizes, stream or refuse early if it bites.
    s->rgba.resize(size_t(width) * size_t(height) * 4);
    s->rows_per_band = band_rows(row_bytes, height, kBandCap);
    s->band.resize(size_t(row_bytes) * size_t(s->rows_per_band));
    for (int32 top = 0; top < height; top += s->rows_per_band) {
        const int32 rows = top + s->rows_per_band > height ? height - top : s->rows_per_band;
        for (int c = 0; c < channels; c++) {
            const VRect b = ch[c]->bounds;
            const VRect rect = {b.top + top, b.left, b.top + top + rows, b.left + width};
            VRect wrote = {};
            const OSErr e = read_port(r, ch[c], rect, s->band.data() + size_t(c) * size_t(sample),
                                      int32(row_bytes * 8), channels * r->depth, &wrote);
            if (e != noErr || wrote.top != rect.top || wrote.left != rect.left || wrote.bottom != rect.bottom ||
                wrote.right != rect.right) {
                char note[128];
                snprintf(note, sizeof note, "merged read failed: channel %d rows %d..%d result %d", c, top, top + rows, e);
                trace_note(note);
                free_state(r, data); // WriteFinish does not follow a failed WriteStart
                return report_error(r, kNothingSaved);
            }
        }
        to_rgba_floats(s->band.data(), size_t(width) * size_t(rows), channels, r->depth,
                       s->rgba.data() + size_t(top) * size_t(width) * 4);
    }
    return finish_save(r, data, s, channels, "merged"); // frees the state and leaves theRect32 empty: no WriteContinue
}

} // namespace

OSErr read_port(const FormatRecord *r, const ReadChannelDesc *c, const VRect &rect, void *dst, int32 row_bits,
                int32 col_bits, VRect *wrote) {
    if (!r->channelPortProcs || !r->channelPortProcs->readPixelsProc || !c || !c->port) return -1;
    PSScaling scaling;
    scaling.sourceRect = rect;
    scaling.destinationRect = rect; // equal rectangles: an unscaled read (PIGeneral.h PSScaling)
    PixelMemoryDesc mem;
    mem.data = dst;
    mem.rowBits = row_bits;
    mem.colBits = col_bits;
    mem.bitOffset = 0;
    mem.depth = c->depth;
    VRect write_rect = rect;
    ReadPixelsProc read = r->channelPortProcs->readPixelsProc;
    PIChannelPort port = c->port;
    __try { // a fault inside the host's read fails this save instead of crashing Photoshop
        return read(port, &scaling, &write_rect, &mem, wrote);
    } __except (EXCEPTION_EXECUTE_HANDLER) {
        return -2;
    }
}
int16 options_start(FormatRecord *r, intptr_t *data) {
    static_assert(kPlayInfoDisplay == plugInDialogDisplay, "dialog.h mirrors PIActions.h");
    r->data = nullptr; // no OptionsContinue: the options do not depend on the pixels
    Globals *g = globals_of(data);
    // Only prepares the save (DC31): the dialog needs the pixels' alpha, so it opens after the last band arrives.
    // has_alpha here only validates the stored values; the greying is decided again from the pixels.
    g->settings = load_settings(r->sSPBasic, alpha_plane(r) >= 0 ? 1 : 0);
    g->have_settings = true;
    // shortcut: the descriptor's contents are neither read nor written; a script, action or batch saves with the
    // remembered settings. Add descriptor keys when recorded actions need their own settings.
    const PIDescriptorParameters *script = r->descriptorParameters;
    g->show_dialog = ShouldShowDialog(script != nullptr, script ? script->playInfo : 0);
    return noErr;
}


// A refusal before any pixel is read: the options chosen for this save must not leak into the next one (a script or
// batch that skips OptionsStart), so the flags are reset as on every other end of a save (Note DC40).
static int16 refuse(FormatRecord *r, intptr_t *data, const char *message) {
    free_state(r, data);
    return report_error(r, message);
}

int16 write_start(FormatRecord *r, intptr_t *data) {
    const bool rgb = r->imageMode == plugInModeRGBColor || r->imageMode == plugInModeRGB48 ||
                     r->imageMode == plugInModeRGB96;
    if (!rgb || (r->depth != 8 && r->depth != 16 && r->depth != 32))
        return refuse(r, data, "Tessalith DDS saves only 8-bit, 16-bit and 32-bit RGB documents.");
    if (!r->HostSupports32BitCoordinates)
        return refuse(r, data, "this Photoshop version is too old for Tessalith DDS.");

    const int32 width = r->imageSize32.h, height = r->imageSize32.v;
    if (width <= 0 || height <= 0) return refuse(r, data, "the image is empty.");
    if (width > kMaxSide || height > kMaxSide)
        return refuse(r, data, "this image is larger than 16384 pixels on a side, the most Skyrim supports.");

    const int16 alpha = alpha_plane(r);
    const int16 channels = alpha >= 0 ? 4 : 3;
    const int16 sample = r->depth / 8;
    const int64_t row_bytes = int64_t(width) * channels * sample;

    Globals *g = globals_of(data);
    if (!g->have_settings) { // the host skipped OptionsStart: the remembered settings
        g->settings = load_settings(r->sSPBasic, uint32_t(channels == 4));
        g->have_settings = true;
    }
    if (r->layerData > 0) { // a layered save: Photoshop sends layers, not the composite, so read its merged image (DC38)
        const ReadChannelDesc *ch[4] = {};
        const char *why = "";
        const int merged = merged_source(r, ch, &why);
        if (!merged) {
            char note[160];
            snprintf(note, sizeof note, "layered save without a usable merged composite: %s", why);
            trace_note(note);
            return refuse(r, data, kNothingSaved);
        }
        if (merged == 3) { // shortcut: no field proved reliable for "no Background layer" (DC40), so this saves opaque
            char note[160];
            snprintf(note, sizeof note, "layered save without an alpha source: transparencyPlane %d layers %d alphas %d",
                     int(r->transparencyPlane), r->documentInfo ? int(r->documentInfo->layerCount) : -1,
                     r->documentInfo ? int(r->documentInfo->alphaChannelCount) : -1);
            trace_note(note);
        }
        trace_note(merged == 4 ? "saving from the merged composite with alpha" : "saving from the merged composite");
        return write_merged(r, data, ch, merged);
    }
    delete g->save; // a stale state from an earlier save that never reached WriteFinish
    g->save = nullptr;
    auto *s = new SaveState;
    g->save = s;
    // shortcut: the whole image as floats (about 4 GiB at 16384 x 16384) plus the engine's work, and a 32-bit save of that size writes an RGBA16F file over the Open cap (preflight B11, Note DE6); fine at sane sizes, stream or refuse early if it bites.
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
    return finish_save(r, data, s, channels, "band");
}

int16 write_finish(FormatRecord *r, intptr_t *data) {
    // The state survives only when the last band never arrived (a cancel, or a host that stopped sending rows):
    // nothing was encoded or written. write_continue frees it on the last band and reports its own errors.
    const bool incomplete = state_of(data) != nullptr;
    free_state(r, data);
    return incomplete ? report_error(r, kNothingSaved) : noErr;
}


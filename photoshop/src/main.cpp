// Tessalith DDS: the format plug-in entry point and selector dispatch. MIT licence (see photoshop/LICENSE).
#include "write.h"

#include "pixels.h"

#include <new>
#include <string.h>

int16 report_error(FormatRecord *r, const char *message) {
    if (!r->errorString) return formatBadParameters;
    // Str255 is a Pascal string: byte 0 holds the length (at most 255).
    unsigned char *s = *r->errorString;
    const size_t len = strnlen(message, 255);
    s[0] = static_cast<unsigned char>(len);
    memcpy(s + 1, message, len);
    if (len < 255) s[len + 1] = 0;
    return errReportString;
}

static int16 dispatch(int16 selector, FormatRecord *r, intptr_t *data) {
    switch (selector) {
    case formatSelectorOptionsStart:
        return options_start(r, data);
    case formatSelectorEstimateStart: {
        // A range over every format the dialog offers, smallest to largest file (pixels.h dds_size_range).
        const bool wide = r->HostSupports32BitCoordinates != 0;
        int64_t lo = 0, hi = 0;
        dds_size_range(wide ? r->imageSize32.h : r->imageSize.h, wide ? r->imageSize32.v : r->imageSize.v, &lo, &hi);
        r->minDataBytes = int32(lo > INT32_MAX ? INT32_MAX : lo);
        r->maxDataBytes = int32(hi > INT32_MAX ? INT32_MAX : hi);
        r->data = nullptr;
        return noErr;
    }
    case formatSelectorReadPrepare:
    case formatSelectorWritePrepare:
        r->maxData = 0; // the plug-in uses its own memory for the image (PIFormat.h maxData)
        return noErr;
    case formatSelectorWriteStart:
        return write_start(r, data);
    case formatSelectorWriteContinue:
        return write_continue(r, data);
    case formatSelectorWriteFinish:
        return write_finish(r, data);
    case formatSelectorReadStart:
        r->layerData = 0; // no layers: the image comes as one composite (PiPL FormatLayerSupport, Note DC26)
        return read_start(r, data);
    case formatSelectorReadContinue:
        return read_continue(r, data);
    case formatSelectorReadFinish:
        return read_finish(r, data);
    case formatSelectorReadLayerStart:
    case formatSelectorReadLayerContinue:
    case formatSelectorReadLayerFinish:
    case formatSelectorWriteLayerStart:
    case formatSelectorWriteLayerContinue:
    case formatSelectorWriteLayerFinish:
        // Claimed only so Save As lists the format (DC26); DDS has no layers, so no layer pixels are requested.
        r->data = nullptr;
        r->theRect32 = VRect{};
        return noErr;
    case formatSelectorFilterFile:
        return filter_file(r);
    default:
        return noErr;
    }
}

extern "C" __declspec(dllexport) void PluginMain(const int16 selector, FormatRecordPtr record, intptr_t *data,
                                                 int16 *result) {
    // Pin the module (Note DC25): the engine's rayon workers live on after a save and sleep inside this module's
    // code, so if Photoshop unloaded it, their next wake-up would execute unmapped memory.
    static const bool pinned = [] {
        HMODULE self = nullptr;
        return GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_PIN | GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
                                  reinterpret_cast<LPCWSTR>(&PluginMain), &self) != 0;
    }();
    (void)pinned;
    // The About call passes an AboutRecord, not a FormatRecord; Tessalith DDS has no About box.
    if (selector == formatSelectorAbout || !record) {
        *result = noErr;
        return;
    }
    // Declared on every call, so the host reads theRect32 / imageSize32 whichever selector it checks the flag on.
    if (record->HostSupports32BitCoordinates) record->PluginUsing32BitCoordinates = 1;
    // Nothing may throw across the plug-in boundary.
    try {
        *result = dispatch(selector, record, data);
    } catch (const std::bad_alloc &) {
        const bool reading = selector >= formatSelectorReadPrepare && selector <= formatSelectorReadFinish;
        *result = report_error(record, reading ? "there is not enough memory to open this DDS."
                                               : "there is not enough memory to save this image.");
    } catch (...) {
        *result = report_error(record, "of an internal error in Tessalith DDS.");
    }
}

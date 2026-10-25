// Tessalith DDS: the format plug-in entry point and selector dispatch. MIT licence (see photoshop/LICENSE).
#include "write.h"

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
        // About a byte per pixel (BC7) plus a third for the mips and the header.
        const int64_t pixels = int64_t(r->imageSize32.h) * r->imageSize32.v;
        const int64_t bytes = pixels + pixels / 3 + 148;
        r->minDataBytes = r->maxDataBytes = int32(bytes > INT32_MAX ? INT32_MAX : bytes);
        r->data = nullptr;
        return noErr;
    }
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
        return report_error(r, "opening DDS files is not supported yet.");
    default:
        return noErr;
    }
}

extern "C" __declspec(dllexport) void PluginMain(const int16 selector, FormatRecordPtr record, intptr_t *data,
                                                 int16 *result) {
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
        *result = report_error(record, "there is not enough memory to save this image.");
    } catch (...) {
        *result = report_error(record, "of an internal error in Tessalith DDS.");
    }
}

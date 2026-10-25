// Tessalith DDS: the format plug-in entry point and selector dispatch. MIT licence (see photoshop/LICENSE).
#include "write.h"

#include "pixels.h"

#include <new>
#include <stdarg.h>
#include <stdio.h>
#include <string.h>

namespace {

// Selector trace (Note DC28): with TESSALITH_DDS_TRACE=1 in Photoshop's environment, every call is appended to
// %TEMP%\TessalithDDS-trace.txt, so the host's real selector sequence can be captured without a rebuild.
bool trace_enabled() {
    static const bool on = [] {
        wchar_t v[4] = {};
        return GetEnvironmentVariableW(L"TESSALITH_DDS_TRACE", v, 4) == 1 && v[0] == L'1';
    }();
    return on;
}

const char *selector_name(int16 s) {
    static const char *const names[] = {
        "About", "ReadPrepare", "ReadStart", "ReadContinue", "ReadFinish", "OptionsPrepare", "OptionsStart",
        "OptionsContinue", "OptionsFinish", "EstimatePrepare", "EstimateStart", "EstimateContinue", "EstimateFinish",
        "WritePrepare", "WriteStart", "WriteContinue", "WriteFinish", "FilterFile"};
    if (s >= 0 && s < int16(sizeof names / sizeof names[0])) return names[s];
    switch (s) {
    case formatSelectorReadLayerStart: return "ReadLayerStart";
    case formatSelectorReadLayerContinue: return "ReadLayerContinue";
    case formatSelectorReadLayerFinish: return "ReadLayerFinish";
    case formatSelectorWriteLayerStart: return "WriteLayerStart";
    case formatSelectorWriteLayerContinue: return "WriteLayerContinue";
    case formatSelectorWriteLayerFinish: return "WriteLayerFinish";
    default: return "other";
    }
}

void append_trace(const char *line, int n) {
    wchar_t path[MAX_PATH + 1] = {};
    const DWORD len = GetTempPathW(MAX_PATH, path);
    if (len == 0 || len > MAX_PATH - 24) return;
    wcscat_s(path, L"TessalithDDS-trace.txt");
    HANDLE f = CreateFileW(path, FILE_APPEND_DATA, FILE_SHARE_READ, nullptr, OPEN_ALWAYS, 0, nullptr);
    if (f == INVALID_HANDLE_VALUE) return;
    DWORD written = 0;
    WriteFile(f, line, DWORD(n), &written, nullptr);
    CloseHandle(f);
}

// One line per call: "in" with what the host set, "out" with the result and what the plug-in requested next.
void trace(const char *when, int16 selector, const FormatRecord *r, int16 result) {
    char line[640];
    int n = snprintf(line, sizeof line,
                     "%lu %s %s(%d) result %d mode %d depth %d planes %d tP %d size %dx%d layerData %u visible %d "
                     "blend %08x opacity %u divider %d groupStart %d text %d rect %d,%d,%d,%d planes %d..%d data %d "
                     "advanceState %d script %d",
                     GetTickCount(), when, selector_name(selector), selector, result, r->imageMode, r->depth,
                     r->planes, r->transparencyPlane, r->imageSize32.h, r->imageSize32.v, r->layerData, r->isVisible,
                     unsigned(r->blendMode), unsigned(r->opacity), r->isGroupDivider, r->isGroupStart, r->isTextLayer,
                     r->theRect32.top, r->theRect32.left, r->theRect32.bottom, r->theRect32.right, r->loPlane,
                     r->hiPlane, r->data != nullptr, r->advanceState != nullptr, r->descriptorParameters != nullptr);
    if (n < 0) return;
    const bool layer = selector >= formatSelectorReadLayerStart && selector <= formatSelectorWriteLayerFinish;
    if (layer && r->layerName && size_t(n) < sizeof line - 80) { // the layer name, UTF-16 to UTF-8, shortened
        wchar_t name[33] = {};
        for (int i = 0; i < 32 && r->layerName[i]; i++) name[i] = wchar_t(r->layerName[i]);
        n += snprintf(line + n, sizeof line - size_t(n), " name \"");
        n += WideCharToMultiByte(CP_UTF8, 0, name, -1, line + n, int(sizeof line) - n - 4, nullptr, nullptr) - 1;
        n += snprintf(line + n, sizeof line - size_t(n), "\"");
    }
    n += snprintf(line + n, sizeof line - size_t(n), "\r\n");
    append_trace(line, n);
}

// On WriteStart (DC38): what the host offers through documentInfo and the channel ports, the source of a layered
// save. Every pointer is checked before use.
struct Line {
    char text[4096];
    size_t n = 0;
    void add(const char *fmt, ...) {
        if (n >= sizeof text - 3) return;
        va_list args;
        va_start(args, fmt);
        const int m = vsnprintf(text + n, sizeof text - 3 - n, fmt, args);
        va_end(args);
        if (m > 0) n = n + size_t(m) < sizeof text - 3 ? n + size_t(m) : sizeof text - 4; // truncated: keep what fit
    }
};

void describe_channel(Line &out, const ReadChannelDesc *c) {
    const VRect b = c->bounds;
    char name[40] = {};
    if (c->name) strncpy_s(name, c->name, _TRUNCATE);
    out.add(" {\"%s\" type %d depth %d bounds %d,%d,%d,%d tile %dx%d port %d", name, c->channelType, c->depth, b.top,
            b.left, b.bottom, b.right, c->tileSize.h, c->tileSize.v, c->port != nullptr);
    out.add("}");
}

void describe_list(Line &out, const char *label, const ReadChannelDesc *c) {
    out.add(" %s", label);
    if (!c) out.add(" null");
    for (int i = 0; c && i < 6; c = c->next, i++) describe_channel(out, c);
}

void trace_document(const FormatRecord *r) {
    Line line;
    const ChannelPortProcs *procs = r->channelPortProcs;
    line.add("    doc channelPortProcs %d", procs != nullptr);
    if (procs)
        line.add(" (version %d count %d readPixels %d)", procs->channelPortProcsVersion, procs->numChannelPortProcs,
                 procs->readPixelsProc != nullptr);
    const ReadImageDocumentDesc *d = r->documentInfo;
    line.add(" documentInfo %d", d != nullptr);
    if (d) {
        line.add(" version %d..%d mode %d depth %d bounds %d,%d,%d,%d", d->minVersion, d->maxVersion, d->imageMode,
                 d->depth, d->bounds.top, d->bounds.left, d->bounds.bottom, d->bounds.right);
        if (d->maxVersion >= 3)
            line.add(" composites %d layers %d alphas %d", d->compositeChannelCount, d->layerCount, d->alphaChannelCount);
        line.add(" layersDescriptor %d", d->maxVersion >= 2 && d->layersDescriptor != nullptr);
        describe_list(line, "merged", d->mergedCompositeChannels);
        describe_list(line, "mergedTransparency", d->mergedTransparency);
        describe_list(line, "alpha", d->alphaChannels);
        // Per layer: name and whether it has a transparency channel (a Background layer should not). Evidence for
        // telling "no Background layer" apart later (DC40); first 8 layers only.
        if (d->maxVersion >= 2) {
            line.add(" layerlist");
            int shown = 0;
            for (const ReadLayerDesc *l = d->layersDescriptor; l && shown < 8; l = l->next, shown++)
                line.add(" {\"%.24s\" T%d}", l->name ? l->name : "", l->transparency != nullptr);
        }
    }
    memcpy(line.text + line.n, "\r\n", 3);
    append_trace(line.text, int(line.n + 2));
}
} // namespace

bool trace_on() { return trace_enabled(); }

void trace_note(const char *note) {
    if (!trace_enabled()) return;
    char line[768]; // the samples line (write.cpp finish_save) is about 400 characters
    const int n = snprintf(line, sizeof line, "%lu    %s\r\n", GetTickCount(), note);
    if (n > 0) append_trace(line, n < int(sizeof line) ? n : int(sizeof line) - 1);
}

int16 report_error(FormatRecord *r, const char *message) {
    if (!r->errorString) return formatBadParameters;
    // Str255 is a Pascal string: byte 0 holds the length (at most 255).
    unsigned char *s = *r->errorString;
    size_t len = strnlen(message, 255);
    while (len > 0 && message[len - 1] == '.') len--; // Photoshop ends the sentence itself (Note DC30)
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
        return read_start(r, data);
    case formatSelectorReadContinue:
        return read_continue(r, data);
    case formatSelectorReadFinish:
        return read_finish(r, data);

    case formatSelectorFilterFile:
        return filter_file(r);
    case formatSelectorReadLayerStart: // the PiPL claims layers (DC38); no layer pixels are exchanged
    case formatSelectorReadLayerContinue:
    case formatSelectorReadLayerFinish:
    case formatSelectorWriteLayerStart:
    case formatSelectorWriteLayerContinue:
    case formatSelectorWriteLayerFinish:
        r->data = nullptr;
        r->theRect32 = VRect{};
        return noErr;
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
    if (trace_enabled()) {
        trace("in ", selector, record, 0);
        if (selector == formatSelectorWriteStart) trace_document(record);
    }
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
    if (trace_enabled()) trace("out", selector, record, *result);
}

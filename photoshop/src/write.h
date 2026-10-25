// Tessalith DDS: format selector handlers shared by main.cpp, write.cpp and read.cpp. MIT licence (see photoshop/LICENSE).
#pragma once

// The SDK's FormatRecord layout trips C4121 (member alignment sensitive to packing) at /W4; it is Adobe's ABI.
#pragma warning(push)
#pragma warning(disable : 4121)
#include "PIFormat.h"
#pragma warning(pop)

#include "../psdds.h"

// Puts `message` in the host's error string and returns errReportString, so Photoshop shows
// "Cannot complete operation because <message>". Without an error string it returns formatBadParameters.
int16 report_error(FormatRecord *record, const char *message);

// psdds messages start with a capital; lower-cases it in place, since Photoshop puts "because " in front.
const char *as_reason(char *message);

// Largest single pixel request: PIFormat.h asks for pieces no larger than maxData; we set maxData to 0 in
// Read/WritePrepare and use our own memory, so this cap keeps each band small instead.
constexpr int64_t kBandCap = int64_t(64) << 20;

struct SaveState;
struct ReadState;

// What PluginMain's `data` points to. Photoshop keeps that value for the plug-in across calls (zero on the first),
// so it carries the settings chosen in OptionsStart to the write selectors. Allocated on first use and kept while
// the plug-in is loaded; `save` (the image buffers) lives only from WriteStart to the end of the write.
struct Globals {
    PsddsSettings settings = {};
    bool have_settings = false; // false: WriteStart loads the remembered settings (no OptionsStart this save)
    SaveState *save = nullptr;
    ReadState *read = nullptr; // the decoded image, from ReadStart to the end of the open
};

// The Globals behind `data`, allocated on first use.
Globals *globals_of(intptr_t *data);


// OptionsStart: shows the Save dialog with the remembered settings; OK stores them, Cancel returns userCanceledErr.
int16 options_start(FormatRecord *record, intptr_t *data);

// The Open selectors (read.cpp). filter_file: noErr for a file starting with "DDS ", else formatCannotRead.
int16 filter_file(FormatRecord *record);
int16 read_start(FormatRecord *record, intptr_t *data);
int16 read_continue(FormatRecord *record, intptr_t *data);
int16 read_finish(FormatRecord *record, intptr_t *data);

// `data` is the plug-in's globals pointer from PluginMain (see Globals).
int16 write_start(FormatRecord *record, intptr_t *data);
int16 write_continue(FormatRecord *record, intptr_t *data);
int16 write_finish(FormatRecord *record, intptr_t *data);

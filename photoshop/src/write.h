// Tessalith DDS: format selector handlers shared between main.cpp and write.cpp. MIT licence (see photoshop/LICENSE).
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

struct SaveState;

// What PluginMain's `data` points to. Photoshop keeps that value for the plug-in across calls (zero on the first),
// so it carries the settings chosen in OptionsStart to the write selectors. Allocated on first use and kept while
// the plug-in is loaded; `save` (the image buffers) lives only from WriteStart to the end of the write.
struct Globals {
    PsddsSettings settings = {};
    bool have_settings = false; // false: WriteStart loads the remembered settings (no OptionsStart this save)
    SaveState *save = nullptr;
};

// OptionsStart: shows the Save dialog with the remembered settings; OK stores them, Cancel returns userCanceledErr.
int16 options_start(FormatRecord *record, intptr_t *data);

// `data` is the plug-in's globals pointer from PluginMain (see Globals).
int16 write_start(FormatRecord *record, intptr_t *data);
int16 write_continue(FormatRecord *record, intptr_t *data);
int16 write_finish(FormatRecord *record, intptr_t *data);

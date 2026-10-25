// Tessalith DDS: format selector handlers shared between main.cpp and write.cpp. MIT licence (see photoshop/LICENSE).
#pragma once

// The SDK's FormatRecord layout trips C4121 (member alignment sensitive to packing) at /W4; it is Adobe's ABI.
#pragma warning(push)
#pragma warning(disable : 4121)
#include "PIFormat.h"
#pragma warning(pop)

// Puts `message` in the host's error string and returns errReportString, so Photoshop shows
// "Cannot complete operation because <message>". Without an error string it returns formatBadParameters.
int16 report_error(FormatRecord *record, const char *message);

// `data` is the plug-in's per-call globals pointer from PluginMain; the save state lives there between selectors.
int16 write_start(FormatRecord *record, intptr_t *data);
int16 write_continue(FormatRecord *record, intptr_t *data);
int16 write_finish(FormatRecord *record, intptr_t *data);

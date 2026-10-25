// Tessalith DDS: format selector handlers shared between main.cpp and write.cpp. MIT licence (see photoshop/LICENSE).
#pragma once

#include "PIFormat.h"

// Puts `message` in the host's error string and returns errReportString, so Photoshop shows
// "Cannot complete operation because <message>". Without an error string it returns formatBadParameters.
int16 report_error(FormatRecord *record, const char *message);

int16 write_start(FormatRecord *record);
int16 write_continue(FormatRecord *record);
int16 write_finish(FormatRecord *record);

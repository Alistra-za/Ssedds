// Tessalith DDS: the Save dialog. MIT licence (see photoshop/LICENSE).
#pragma once

// Control IDs, shared with dialog.rc (rc.exe reads only the #defines).
#define IDD_SAVE 100
#define IDC_FORMAT 1001
#define IDC_TYPE 1002
#define IDC_SRGB 1003
#define IDC_FILTER 1004
#define IDC_MIPS 1005
#define IDC_QUALITY 1006

#ifndef RC_INVOKED
#include <windows.h>

#include "../psdds.h"

// What each control shows and whether it is editable, straight from psdds_resolve.
struct ControlStates {
    bool ok;               // false: psdds_resolve refused the input (out-of-range value)
    PsddsSettings shown;   // the effective values
    bool srgb_enabled;     // the sRGB checkbox; checked = shown.srgb
    bool filter_enabled;   // false also with Mipmaps unchecked
    bool quality_enabled;
    bool formats[6];       // the Format list: psdds_format_allowed(shown.texture_type, PSDDS_BC1 .. PSDDS_RGBA8)
};

// Runs `input` through psdds_resolve with the document's has_alpha (0/1). Format, type and mips are always editable.
ControlStates FromResolve(const PsddsSettings &input, uint32_t has_alpha);

// PIActions.h plugInDialogDisplay (dialogPlayEnum); write.cpp checks it against the SDK's value.
constexpr int32_t kPlayInfoDisplay = 1;

// Whether the save shows the dialog (decided in OptionsStart, shown after the pixels are read, DC31). With descriptor parameters (a script, an action, a batch), the host's
// playInfo decides: only plugInDialogDisplay shows it; plugInDialogDontDisplay and plugInDialogSilent do not.
// Without them (an interactive Save or Save As) the dialog always shows (DC22).
bool ShouldShowDialog(bool has_descriptor, int32_t play_info);

enum class DialogResult { Ok, Cancel, Failed };

// Modal Save options. *inout holds the raw choices (what the user picked, including values of greyed controls) on
// entry and, after Ok, on return. Failed: the dialog could not be created.
DialogResult ShowSaveDialog(HWND owner, uint32_t has_alpha, PsddsSettings *inout);
#endif

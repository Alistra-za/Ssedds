// Tessalith DDS: tests for the Save dialog's control states, built and run by `build.ps1 -Test`.
// MIT licence (see photoshop/LICENSE).
#include "../src/dialog.h"

#include <stdio.h>

static int failures = 0;

static void check(bool ok, const char *test, const char *what) {
    if (!ok) {
        printf("  FAIL %s: %s\n", test, what);
        failures++;
    }
}

static PsddsSettings make(uint32_t format, uint32_t type, uint32_t srgb, uint32_t filter) {
    PsddsSettings s = psdds_defaults(type);
    s.format = format;
    s.srgb = srgb;
    s.filter = filter;
    return s;
}

static void dialog_state_follows_resolve() {
    const char *t = __func__;

    // Normal + BC7: sRGB greys showing Off, filter greys showing the Normal preset's filter.
    ControlStates n = FromResolve(make(PSDDS_BC7, PSDDS_NORMAL, 1, PSDDS_KAISER), 0);
    check(n.ok, t, "Normal+BC7 resolves");
    check(!n.srgb_enabled && n.shown.srgb == 0, t, "Normal greys sRGB showing Off");
    check(!n.filter_enabled && n.shown.filter == psdds_defaults(PSDDS_NORMAL).filter, t,
          "Normal greys filter showing the preset's");
    check(n.quality_enabled, t, "Normal+BC7 quality editable");
    check(n.shown.format == PSDDS_BC7 && n.shown.texture_type == PSDDS_NORMAL && n.shown.mips == 1, t,
          "Normal+BC7 shows format, type, mips");

    // Colour + BC5 with sRGB on: BC5 has no sRGB form, and mips with Box.
    ControlStates c5 = FromResolve(make(PSDDS_BC5, PSDDS_COLOUR, 1, PSDDS_KAISER), 0);
    check(c5.ok, t, "Colour+BC5 resolves");
    check(!c5.srgb_enabled && c5.shown.srgb == 0, t, "Colour+BC5 greys sRGB showing Off");
    check(!c5.filter_enabled && c5.shown.filter == PSDDS_BOX, t, "Colour+BC5 greys filter showing Box");
    check(c5.quality_enabled, t, "Colour+BC5 quality editable");

    // Colour + RGBA8, no alpha: sRGB and filter editable, quality greyed at 0.
    ControlStates r = FromResolve(make(PSDDS_RGBA8, PSDDS_COLOUR, 1, PSDDS_KAISER), 0);
    check(r.ok, t, "Colour+RGBA8 resolves");
    check(r.srgb_enabled && r.shown.srgb == 1, t, "Colour+RGBA8 sRGB editable, On");
    check(r.filter_enabled && r.shown.filter == PSDDS_KAISER, t, "Colour+RGBA8 filter editable, Kaiser");
    check(!r.quality_enabled && r.shown.quality == 0, t, "RGBA8 greys quality at 0");

    // Colour + BC7 with alpha: the filter greys showing Box.
    ControlStates a = FromResolve(make(PSDDS_BC7, PSDDS_COLOUR, 1, PSDDS_KAISER), 1);
    check(a.ok && !a.filter_enabled && a.shown.filter == PSDDS_BOX, t, "alpha greys filter showing Box");
    check(a.srgb_enabled && a.shown.srgb == 1, t, "alpha leaves sRGB editable");

    // An out-of-range value (a corrupt remembered setting) is refused, not shown.
    check(!FromResolve(make(9, PSDDS_COLOUR, 1, 0), 0).ok, t, "format 9 refused");
}

static void dialog_follows_play_info() {
    const char *t = __func__;
    check(ShouldShowDialog(false, 0, false), t, "no descriptor: interactive save shows the dialog");
    check(ShouldShowDialog(false, 2, false), t, "no descriptor: playInfo is not read");
    check(ShouldShowDialog(true, 1, false), t, "plugInDialogDisplay shows it");
    check(!ShouldShowDialog(true, 0, false), t, "plugInDialogDontDisplay does not");
    check(!ShouldShowDialog(true, 2, false), t, "plugInDialogSilent does not");
}

static void dialog_skipped_with_revert_info() {
    const char *t = __func__;
    check(!ShouldShowDialog(false, 0, true), t, "plain Save of a document with revertInfo: no dialog");
    check(ShouldShowDialog(true, 1, true), t, "a script asking for the dialog still gets it");
    check(!ShouldShowDialog(true, 2, true), t, "silent script: no dialog");
}
int main() {
    int before = failures;
    dialog_state_follows_resolve();
    printf("  %s dialog_state_follows_resolve\n", failures == before ? "PASS" : "FAIL");
    before = failures;
    dialog_follows_play_info();
    printf("  %s dialog_follows_play_info\n", failures == before ? "PASS" : "FAIL");
    before = failures;
    dialog_skipped_with_revert_info();
    printf("  %s dialog_skipped_with_revert_info\n", failures == before ? "PASS" : "FAIL");
    if (failures) {
        printf("dialog_test: FAILED\n");
        return 1;
    }
    printf("dialog_test: all passed\n");
    return 0;
}

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

    // Colour + BC5 (a remembered pick): Colour does not offer BC5, so it shows the Colour default, BC7 (DC32).
    ControlStates c5 = FromResolve(make(PSDDS_BC5, PSDDS_COLOUR, 1, PSDDS_KAISER), 0);
    check(c5.ok && c5.shown.format == PSDDS_BC7, t, "Colour+BC5 resolves to BC7");
    check(c5.srgb_enabled && c5.shown.srgb == 1, t, "Colour+BC5 -> BC7 keeps sRGB editable, On");

    // Linear mask + BC5 with sRGB on: no sRGB form, sRGB greys showing Off.
    ControlStates m5 = FromResolve(make(PSDDS_BC5, PSDDS_LINEAR_MASK, 1, PSDDS_KAISER), 0);
    check(m5.ok && m5.shown.format == PSDDS_BC5, t, "Mask+BC5 kept");
    check(!m5.srgb_enabled && m5.shown.srgb == 0, t, "Mask+BC5 greys sRGB showing Off");
    check(m5.quality_enabled, t, "Mask+BC5 quality editable");

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

// DC33: the Format list per type, the sRGB checkbox and the filter beside the Mipmaps checkbox.
static void dialog_controls_follow_type_and_mips() {
    const char *t = __func__;
    const bool colour[6] = {true, true, false, false, true, true};
    const bool normal[6] = {true, true, false, true, true, true};
    const bool mask[6] = {true, true, true, true, true, true};
    const bool *want[3] = {colour, normal, mask};
    for (uint32_t ty = 0; ty < 3; ty++) {
        ControlStates s = FromResolve(make(PSDDS_BC7, ty, 1, PSDDS_KAISER), 0);
        check(s.ok, t, "resolves");
        for (uint32_t f = 0; f < 6; f++) check(s.formats[f] == want[ty][f], t, "format list per type");
        check(s.formats[s.shown.format], t, "shown format is in the list");
    }

    // sRGB checkbox: editable and checked for Colour + BC7 On; editable unchecked for Off; greyed unchecked for Normal.
    ControlStates on = FromResolve(make(PSDDS_BC7, PSDDS_COLOUR, 1, PSDDS_KAISER), 0);
    check(on.srgb_enabled && on.shown.srgb == 1, t, "Colour sRGB On: enabled, checked");
    ControlStates off = FromResolve(make(PSDDS_BC7, PSDDS_COLOUR, 0, PSDDS_KAISER), 0);
    check(off.srgb_enabled && off.shown.srgb == 0, t, "Colour sRGB Off: enabled, unchecked");
    ControlStates nrm = FromResolve(make(PSDDS_BC7, PSDDS_NORMAL, 1, PSDDS_KAISER), 0);
    check(!nrm.srgb_enabled && nrm.shown.srgb == 0, t, "Normal: greyed, unchecked");

    // Filter: editable with mips on, greyed (keeping Kaiser) with mips off.
    check(on.filter_enabled && on.shown.mips == 1, t, "mips on: filter editable");
    PsddsSettings nomips = make(PSDDS_BC7, PSDDS_COLOUR, 1, PSDDS_KAISER);
    nomips.mips = 0;
    ControlStates nm = FromResolve(nomips, 0);
    check(nm.ok && !nm.filter_enabled && nm.shown.filter == PSDDS_KAISER && nm.shown.mips == 0, t,
          "mips off: filter greyed, Kaiser kept");
}

static void dialog_follows_play_info() {
    const char *t = __func__;
    check(ShouldShowDialog(false, 0), t, "no descriptor: interactive save shows the dialog");
    check(ShouldShowDialog(false, 2), t, "no descriptor: playInfo is not read");
    check(ShouldShowDialog(true, 1), t, "plugInDialogDisplay shows it");
    check(!ShouldShowDialog(true, 0), t, "plugInDialogDontDisplay does not");
    check(!ShouldShowDialog(true, 2), t, "plugInDialogSilent does not");
}

int main() {
    int before = failures;
    dialog_state_follows_resolve();
    printf("  %s dialog_state_follows_resolve\n", failures == before ? "PASS" : "FAIL");
    before = failures;
    dialog_controls_follow_type_and_mips();
    printf("  %s dialog_controls_follow_type_and_mips\n", failures == before ? "PASS" : "FAIL");
    before = failures;
    dialog_follows_play_info();
    printf("  %s dialog_follows_play_info\n", failures == before ? "PASS" : "FAIL");
    if (failures) {
        printf("dialog_test: FAILED\n");
        return 1;
    }
    printf("dialog_test: all passed\n");
    return 0;
}

// Tessalith DDS: tests for the Save dialog's control states, built and run by `build.ps1 -Test`.
// MIT licence (see photoshop/LICENSE).
#include "../src/dialog.h"
#include "../src/presets.h"

#include <stdio.h>
#include <string.h>

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

static std::wstring temp_dir() {
    wchar_t tmp[MAX_PATH] = {};
    GetTempPathW(MAX_PATH, tmp);
    return std::wstring(tmp) + L"tdds_presets_test_" + std::to_wstring(GetCurrentProcessId());
}

static void write_text(const std::wstring &path, const char *utf8) {
    HANDLE f = CreateFileW(path.c_str(), GENERIC_WRITE, 0, nullptr, CREATE_ALWAYS, FILE_ATTRIBUTE_NORMAL, nullptr);
    DWORD n = 0;
    WriteFile(f, utf8, DWORD(strlen(utf8)), &n, nullptr);
    CloseHandle(f);
}

static bool same(const PsddsSettings &a, const PsddsSettings &b) { return memcmp(&a, &b, sizeof a) == 0; }

static std::wstring widen(const char *ascii) {
    std::wstring w;
    for (; ascii && *ascii; ascii++) w += wchar_t(*ascii);
    return w;
}

// DC37: the preset file: seeded once, round trip, corrupt lines skipped, an empty file stays empty.
static void presets_store_round_trip() {
    const char *t = __func__;
    const std::wstring dir = temp_dir();
    const std::wstring file = dir + L"\\presets.txt";
    DeleteFileW(file.c_str());
    RemoveDirectoryW(dir.c_str());

    std::vector<Preset> seeded = LoadPresets(dir);
    check(seeded.size() == psdds_preset_count() && seeded.size() == 5, t, "absent file: the five seeds");
    check(GetFileAttributesW(file.c_str()) != INVALID_FILE_ATTRIBUTES, t, "absent file: seeds written");
    for (uint32_t i = 0; i < seeded.size(); i++) {
        PsddsSettings s = {};
        psdds_preset_settings(i, &s);
        check(same(seeded[i].settings, s), t, "seed settings");
    }
    check(seeded.size() == 5 && seeded[1].name == L"Colour BC1 (small, opaque)", t, "seed names");

    std::vector<Preset> list = seeded;
    PsddsSettings mine = psdds_defaults(PSDDS_LINEAR_MASK);
    mine.format = PSDDS_BC5;
    mine.discard_alpha = 1;
    list.push_back({L"Mine ü猫", mine});
    check(SavePresets(dir, list), t, "save");
    std::vector<Preset> back = LoadPresets(dir);
    bool equal = back.size() == list.size();
    for (size_t i = 0; equal && i < list.size(); i++)
        equal = back[i].name == list[i].name && same(back[i].settings, list[i].settings);
    check(equal, t, "round trip keeps order, names (UTF-8) and all seven values");

    check(SavePresets(dir, {}), t, "save empty");
    check(LoadPresets(dir).empty() && LoadPresets(dir).empty(), t, "an empty store stays empty (no reseed)");

    write_text(file, "# comment\r\nGood\t4\t0\t0\t0\t1\t4\t0\r\nShort\t4\t0\t0\r\nBadFormat\t9\t0\t0\t0\t1\t4\t0\n"
                     "Neg\t-1\t0\t0\t0\t1\t4\t0\nDisc\t4\t0\t0\t0\t1\t4\t2\n\t4\t0\t0\t0\t1\t4\t0\n");
    back = LoadPresets(dir);
    check(back.size() == 1 && back[0].name == L"Good", t, "only the valid line loads");

    DeleteFileW(file.c_str());
    RemoveDirectoryW(dir.c_str());
}

static void preset_names_and_matching() {
    const char *t = __func__;
    check(CleanPresetName(L"  Rock wall  ") == L"Rock wall", t, "trimmed");
    check(CleanPresetName(L"   ").empty() && CleanPresetName(L"").empty(), t, "empty refused");
    check(CleanPresetName(std::wstring(64, L'x')).size() == 64, t, "64 characters allowed");
    check(CleanPresetName(std::wstring(65, L'x')).empty(), t, "65 refused");
    check(CleanPresetName(L"a\tb").empty() && CleanPresetName(L"a\nb").empty(), t, "control characters refused");

    std::vector<Preset> list;
    for (uint32_t i = 0; i < psdds_preset_count(); i++) {
        Preset p;
        p.name = widen(psdds_preset_name(i));
        psdds_preset_settings(i, &p.settings);
        list.push_back(p);
    }
    check(FindPreset(list, L"colour bc7") == 0 && FindPreset(list, L"NORMAL BC7") == 2, t, "find ignores case");
    check(FindPreset(list, L"Colour") == -1, t, "find is whole-name");

    // Preset -> edit -> Custom -> edit back -> preset.
    PsddsSettings raw = list[1].settings;
    check(MatchPreset(list, raw, 0) == 1, t, "BC1 preset matches itself");
    raw.quality = 3;
    check(MatchPreset(list, raw, 0) == -1, t, "an edit shows Custom");
    raw.quality = list[1].settings.quality;
    check(MatchPreset(list, raw, 0) == 1, t, "editing back shows the preset again");
    raw.discard_alpha = 0;
    check(MatchPreset(list, raw, 0) == -1, t, "discard transparency is part of a preset");
    check(MatchPreset(list, psdds_defaults(PSDDS_COLOUR), 1) == 0, t, "the defaults are Colour BC7");
    // A greyed difference does not make it Custom: Normal with a remembered sRGB On still shows Normal BC7.
    PsddsSettings n = list[2].settings;
    n.srgb = 1;
    check(MatchPreset(list, n, 0) == 2, t, "matching uses effective values");
    check(MatchPreset({}, raw, 0) == -1, t, "an empty store: Custom");
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
    presets_store_round_trip();
    printf("  %s presets_store_round_trip\n", failures == before ? "PASS" : "FAIL");
    before = failures;
    preset_names_and_matching();
    printf("  %s preset_names_and_matching\n", failures == before ? "PASS" : "FAIL");
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

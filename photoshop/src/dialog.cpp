// Tessalith DDS: the Save dialog. Every control is always shown; after each change the dialog asks psdds_resolve
// what each control shows and whether it is editable, and psdds_format_allowed which formats the Format list offers,
// so the greying and format rules live only in psdds.
// MIT licence (see photoshop/LICENSE).
#include "dialog.h"
#include "presets.h"

#include <string>
#include <vector>

ControlStates FromResolve(const PsddsSettings &input, uint32_t has_alpha) {
    ControlStates s = {};
    PsddsControls c = {};
    s.ok = psdds_resolve(&input, has_alpha, &s.shown, &c) == 0;
    if (!s.ok) return s;
    s.srgb_enabled = c.srgb_enabled != 0;
    s.filter_enabled = c.filter_enabled != 0;
    s.quality_enabled = c.quality_enabled != 0;
    for (uint32_t f = 0; f < 6; f++) s.formats[f] = psdds_format_allowed(s.shown.texture_type, f) != 0;
    return s;
}

bool ShouldShowDialog(bool has_descriptor, int32_t play_info) {
    return !has_descriptor || play_info == kPlayInfoDisplay;
}

namespace {

struct Item {
    const wchar_t *label;
    uint32_t value;
};

const Item kFormats[] = {{L"BC1", PSDDS_BC1}, {L"BC3", PSDDS_BC3},  {L"BC4", PSDDS_BC4},
                         {L"BC5", PSDDS_BC5}, {L"BC7", PSDDS_BC7}, {L"RGBA8 (uncompressed)", PSDDS_RGBA8}};
const Item kTypes[] = {{L"Colour", PSDDS_COLOUR}, {L"Normal", PSDDS_NORMAL}, {L"Linear mask", PSDDS_LINEAR_MASK}};
const Item kFilters[] = {{L"Box", PSDDS_BOX}, {L"Tent", PSDDS_TENT}, {L"Mitchell", PSDDS_MITCHELL}, {L"Kaiser", PSDDS_KAISER}};

struct Context {
    PsddsSettings raw; // the user's picks; greyed controls keep theirs, so switching back restores them
    uint32_t has_alpha;
    std::wstring preset_dir;     // empty: presets cannot be stored (no %APPDATA%)
    std::vector<Preset> presets; // as in the preset file (DC37)
    bool writable = true;                  // false: the preset file could not be read in full, so it is never written (DC40)
    std::vector<std::string> unparsed;     // lines of the file that did not parse, written back unchanged
    int picked = -1;                       // the preset the user chose; shown while its settings still match (DC40)
};

const wchar_t kReadOnlyMessage[] = L"The preset file could not be read, so presets can't be changed.";

constexpr uint32_t kCustom = 0xFFFFFFFFu; // the preset list's "Custom" entry

void add(HWND combo, const wchar_t *label, uint32_t value) {
    const LRESULT i = SendMessageW(combo, CB_ADDSTRING, 0, reinterpret_cast<LPARAM>(label));
    if (i >= 0) SendMessageW(combo, CB_SETITEMDATA, WPARAM(i), LPARAM(value));
}

template <size_t N> void fill(HWND dlg, int id, const Item (&items)[N]) {
    for (const Item &it : items) add(GetDlgItem(dlg, id), it.label, it.value);
}

void select_value(HWND dlg, int id, uint32_t value) {
    HWND combo = GetDlgItem(dlg, id);
    const LRESULT n = SendMessageW(combo, CB_GETCOUNT, 0, 0);
    LRESULT pick = -1;
    for (LRESULT i = 0; i < n; i++)
        if (uint32_t(SendMessageW(combo, CB_GETITEMDATA, WPARAM(i), 0)) == value) pick = i;
    SendMessageW(combo, CB_SETCURSEL, WPARAM(pick), 0);
}

uint32_t selected_value(HWND dlg, int id) {
    HWND combo = GetDlgItem(dlg, id);
    const LRESULT i = SendMessageW(combo, CB_GETCURSEL, 0, 0);
    return i < 0 ? 0 : uint32_t(SendMessageW(combo, CB_GETITEMDATA, WPARAM(i), 0));
}

std::wstring widen(const char *utf8) {
    const int n = MultiByteToWideChar(CP_UTF8, 0, utf8, -1, nullptr, 0);
    if (n <= 1) return std::wstring();
    std::wstring w(size_t(n), L'\0');
    MultiByteToWideChar(CP_UTF8, 0, utf8, -1, &w[0], n);
    w.resize(size_t(n - 1));
    return w;
}

void fill_quality(HWND dlg, uint32_t format) {
    HWND combo = GetDlgItem(dlg, IDC_QUALITY);
    SendMessageW(combo, CB_RESETCONTENT, 0, 0);
    const uint32_t n = psdds_quality_count(format);
    for (uint32_t i = 0; i < n; i++)
        if (const char *label = psdds_quality_label(format, i)) add(combo, widen(label).c_str(), i);
}

// The preset list: "Custom", then the stored presets in file order (not sorted).
void fill_presets(HWND dlg, const Context &ctx) {
    HWND combo = GetDlgItem(dlg, IDC_PRESET);
    SendMessageW(combo, CB_RESETCONTENT, 0, 0);
    add(combo, L"Custom", kCustom);
    for (size_t i = 0; i < ctx.presets.size(); i++) add(combo, ctx.presets[i].name.c_str(), uint32_t(i));
}

// `changed`: the control the user just changed, or 0 at start (and after a preset pick). The Format list is rebuilt
// when the type changes (it offers only psdds_format_allowed formats), the Quality list when the shown format may have
// changed. The preset list shows the first preset whose effective values match, else Custom (DC37).
void refresh(HWND dlg, Context &ctx, int changed) {
    const ControlStates s = FromResolve(ctx.raw, ctx.has_alpha);
    if (!s.ok) return; // unreachable: every control offers only in-range values
    if (changed == 0 || changed == IDC_TYPE) {
        HWND combo = GetDlgItem(dlg, IDC_FORMAT);
        SendMessageW(combo, CB_RESETCONTENT, 0, 0);
        for (const Item &it : kFormats)
            if (s.formats[it.value]) add(combo, it.label, it.value);
    }
    if (changed == 0 || changed == IDC_TYPE || changed == IDC_FORMAT) fill_quality(dlg, s.shown.format);
    const int match = MatchPreset(ctx.presets, ctx.raw, ctx.has_alpha, ctx.picked);
    ctx.picked = match;
    select_value(dlg, IDC_PRESET, match < 0 ? kCustom : uint32_t(match));
    select_value(dlg, IDC_FORMAT, s.shown.format);
    select_value(dlg, IDC_TYPE, s.shown.texture_type);
    select_value(dlg, IDC_FILTER, s.shown.filter);
    select_value(dlg, IDC_QUALITY, s.shown.quality);
    CheckDlgButton(dlg, IDC_SRGB, s.shown.srgb ? BST_CHECKED : BST_UNCHECKED);
    CheckDlgButton(dlg, IDC_DISCARD, s.shown.discard_alpha ? BST_CHECKED : BST_UNCHECKED);
    CheckDlgButton(dlg, IDC_MIPS, s.shown.mips ? BST_CHECKED : BST_UNCHECKED);
    EnableWindow(GetDlgItem(dlg, IDC_SRGB), s.srgb_enabled);
    EnableWindow(GetDlgItem(dlg, IDC_FILTER), s.filter_enabled);
    EnableWindow(GetDlgItem(dlg, IDC_QUALITY), s.quality_enabled);
    EnableWindow(GetDlgItem(dlg, IDC_PRESET_DELETE), match >= 0);
}

void message(HWND dlg, const wchar_t *text) { MessageBoxW(dlg, text, L"Tessalith DDS", MB_OK | MB_ICONWARNING); }

bool ask(HWND dlg, const std::wstring &text) {
    return MessageBoxW(dlg, text.c_str(), L"Tessalith DDS", MB_YESNO | MB_ICONQUESTION | MB_DEFBUTTON2) == IDYES;
}

// The "Save preset" name prompt. lp: the std::wstring that holds the proposed name and, after OK, the clean name.
INT_PTR CALLBACK name_proc(HWND dlg, UINT msg, WPARAM wp, LPARAM lp) {
    std::wstring *name = reinterpret_cast<std::wstring *>(GetWindowLongPtrW(dlg, DWLP_USER));
    if (msg == WM_INITDIALOG) {
        SetWindowLongPtrW(dlg, DWLP_USER, lp);
        name = reinterpret_cast<std::wstring *>(lp);
        SendDlgItemMessageW(dlg, IDC_PRESET_NAME, EM_LIMITTEXT, 64, 0);
        SetDlgItemTextW(dlg, IDC_PRESET_NAME, name->c_str());
        SendDlgItemMessageW(dlg, IDC_PRESET_NAME, EM_SETSEL, 0, -1);
        return TRUE;
    }
    if (msg != WM_COMMAND || !name) return FALSE;
    if (LOWORD(wp) == IDCANCEL) {
        EndDialog(dlg, IDCANCEL);
        return TRUE;
    }
    if (LOWORD(wp) != IDOK) return FALSE;
    wchar_t buf[80] = {};
    GetDlgItemTextW(dlg, IDC_PRESET_NAME, buf, 80);
    const std::wstring clean = CleanPresetName(buf);
    if (clean.empty()) {
        message(dlg, L"Enter a preset name of 1 to 64 characters.");
        SetFocus(GetDlgItem(dlg, IDC_PRESET_NAME));
        return TRUE;
    }
    *name = clean;
    EndDialog(dlg, IDOK);
    return TRUE;
}

// Save preset: name it (an existing name, in any case, asks to replace that preset), then write the file.
void save_preset(HWND dlg, Context &ctx) {
    if (!ctx.writable) return message(dlg, kReadOnlyMessage);
    const int match = MatchPreset(ctx.presets, ctx.raw, ctx.has_alpha, ctx.picked);
    std::wstring name = match < 0 ? std::wstring() : ctx.presets[size_t(match)].name;
    HMODULE self = reinterpret_cast<HMODULE>(GetWindowLongPtrW(dlg, GWLP_HINSTANCE));
    if (DialogBoxParamW(self, MAKEINTRESOURCEW(IDD_PRESET_NAME), dlg, name_proc, LPARAM(&name)) != IDOK) return;
    std::vector<Preset> next = ctx.presets;
    int at = FindPreset(next, name);
    if (at >= 0) {
        if (!ask(dlg, L"A preset named \"" + next[size_t(at)].name + L"\" already exists. Replace it?")) return;
        next[size_t(at)] = {name, ctx.raw};
    } else {
        next.push_back({name, ctx.raw});
        at = int(next.size()) - 1;
    }
    if (!SavePresets(ctx.preset_dir, next, ctx.unparsed)) {
        message(dlg, L"The preset could not be saved.");
        return;
    }
    ctx.presets = next;
    ctx.picked = at;
    fill_presets(dlg, ctx);
}

void delete_preset(HWND dlg, Context &ctx) {
    if (!ctx.writable) return message(dlg, kReadOnlyMessage);
    const int match = MatchPreset(ctx.presets, ctx.raw, ctx.has_alpha, ctx.picked);
    if (match < 0) return;
    if (!ask(dlg, L"Delete the preset \"" + ctx.presets[size_t(match)].name + L"\"?")) return;
    std::vector<Preset> next = ctx.presets;
    next.erase(next.begin() + match);
    if (!SavePresets(ctx.preset_dir, next, ctx.unparsed)) {
        message(dlg, L"The preset could not be deleted.");
        return;
    }
    ctx.presets = next;
    ctx.picked = -1;
    fill_presets(dlg, ctx);
}

INT_PTR CALLBACK dialog_proc(HWND dlg, UINT msg, WPARAM wp, LPARAM lp) {
    Context *ctx = reinterpret_cast<Context *>(GetWindowLongPtrW(dlg, DWLP_USER));
    switch (msg) {
    case WM_INITDIALOG:
        SetWindowLongPtrW(dlg, DWLP_USER, lp);
        ctx = reinterpret_cast<Context *>(lp);
        fill(dlg, IDC_TYPE, kTypes);
        fill(dlg, IDC_FILTER, kFilters);
        fill_presets(dlg, *ctx);
        refresh(dlg, *ctx, 0);
        return TRUE; // focus goes to the first tab stop (Preset)
    case WM_COMMAND: {
        const int id = LOWORD(wp);
        if (id == IDOK || id == IDCANCEL) {
            EndDialog(dlg, id);
            return TRUE;
        }
        if (!ctx) return FALSE;
        if (id == IDC_PRESET_SAVE || id == IDC_PRESET_DELETE) {
            if (HIWORD(wp) != BN_CLICKED) return FALSE;
            if (id == IDC_PRESET_SAVE) save_preset(dlg, *ctx);
            else delete_preset(dlg, *ctx);
            refresh(dlg, *ctx, IDC_PRESET);
            return TRUE;
        }
        const bool checkbox = id == IDC_SRGB || id == IDC_MIPS || id == IDC_DISCARD;
        if (HIWORD(wp) != (checkbox ? BN_CLICKED : CBN_SELCHANGE)) return FALSE;
        const uint32_t v = checkbox ? uint32_t(IsDlgButtonChecked(dlg, id) == BST_CHECKED) : selected_value(dlg, id);
        int changed = id;
        switch (id) {
        case IDC_PRESET:
            if (v >= ctx->presets.size()) break; // Custom: nothing to apply; refresh shows the match again
            ctx->raw = ctx->presets[v].settings;
            ctx->picked = int(v);
            changed = 0; // type and format may both change
            break;
        case IDC_FORMAT: ctx->raw.format = v; break;
        case IDC_TYPE: ctx->raw.texture_type = v; break;
        case IDC_SRGB: ctx->raw.srgb = v; break;
        case IDC_DISCARD: ctx->raw.discard_alpha = v; break;
        case IDC_FILTER: ctx->raw.filter = v; break;
        case IDC_MIPS: ctx->raw.mips = v; break;
        case IDC_QUALITY: ctx->raw.quality = v; break;
        default: return FALSE;
        }
        refresh(dlg, *ctx, changed);
        return TRUE;
    }
    default:
        return FALSE;
    }
}
} // namespace

DialogResult ShowSaveDialog(HWND owner, uint32_t has_alpha, PsddsSettings *inout) {
    HMODULE self = nullptr;
    if (!GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                            reinterpret_cast<LPCWSTR>(&ShowSaveDialog), &self))
        return DialogResult::Failed;
    Context ctx = {*inout, has_alpha, PresetDir(), {}};
    PresetFileInfo file;
    ctx.presets = LoadPresets(ctx.preset_dir, &file);
    ctx.writable = file.complete;
    ctx.unparsed = file.unparsed;
    const INT_PTR r = DialogBoxParamW(self, MAKEINTRESOURCEW(IDD_SAVE), owner, dialog_proc, LPARAM(&ctx));
    if (r == -1 || r == 0) return DialogResult::Failed; // 0: the owner window was invalid
    if (r != IDOK) return DialogResult::Cancel;
    *inout = ctx.raw;
    return DialogResult::Ok;
}
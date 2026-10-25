// Tessalith DDS: the Save dialog. Every control is always shown; after each change the dialog asks psdds_resolve
// what each control shows and whether it is editable, so the greying rules live only in psdds.
// MIT licence (see photoshop/LICENSE).
#include "dialog.h"

#include <string>

ControlStates FromResolve(const PsddsSettings &input, uint32_t has_alpha) {
    ControlStates s = {};
    PsddsControls c = {};
    s.ok = psdds_resolve(&input, has_alpha, &s.shown, &c) == 0;
    if (!s.ok) return s;
    s.srgb_enabled = c.srgb_enabled != 0;
    s.filter_enabled = c.filter_enabled != 0;
    s.quality_enabled = c.quality_enabled != 0;
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
const Item kSrgb[] = {{L"On", 1}, {L"Off", 0}};
const Item kFilters[] = {{L"Box", PSDDS_BOX}, {L"Tent", PSDDS_TENT}, {L"Mitchell", PSDDS_MITCHELL}, {L"Kaiser", PSDDS_KAISER}};
const Item kMips[] = {{L"Full chain", 1}, {L"None", 0}};

struct Context {
    PsddsSettings raw; // the user's picks; greyed controls keep theirs, so switching back restores them
    uint32_t has_alpha;
};

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

void refresh(HWND dlg, const Context &ctx, bool format_changed) {
    const ControlStates s = FromResolve(ctx.raw, ctx.has_alpha);
    if (!s.ok) return; // unreachable: every list offers only in-range values
    if (format_changed) fill_quality(dlg, s.shown.format);
    select_value(dlg, IDC_FORMAT, s.shown.format);
    select_value(dlg, IDC_TYPE, s.shown.texture_type);
    select_value(dlg, IDC_SRGB, s.shown.srgb);
    select_value(dlg, IDC_FILTER, s.shown.filter);
    select_value(dlg, IDC_MIPS, s.shown.mips);
    select_value(dlg, IDC_QUALITY, s.shown.quality);
    EnableWindow(GetDlgItem(dlg, IDC_SRGB), s.srgb_enabled);
    EnableWindow(GetDlgItem(dlg, IDC_FILTER), s.filter_enabled);
    EnableWindow(GetDlgItem(dlg, IDC_QUALITY), s.quality_enabled);
}

INT_PTR CALLBACK dialog_proc(HWND dlg, UINT msg, WPARAM wp, LPARAM lp) {
    Context *ctx = reinterpret_cast<Context *>(GetWindowLongPtrW(dlg, DWLP_USER));
    switch (msg) {
    case WM_INITDIALOG:
        SetWindowLongPtrW(dlg, DWLP_USER, lp);
        ctx = reinterpret_cast<Context *>(lp);
        fill(dlg, IDC_FORMAT, kFormats);
        fill(dlg, IDC_TYPE, kTypes);
        fill(dlg, IDC_SRGB, kSrgb);
        fill(dlg, IDC_FILTER, kFilters);
        fill(dlg, IDC_MIPS, kMips);
        refresh(dlg, *ctx, true);
        return TRUE; // focus goes to the first tab stop (Format)
    case WM_COMMAND: {
        const int id = LOWORD(wp);
        if (id == IDOK || id == IDCANCEL) {
            EndDialog(dlg, id);
            return TRUE;
        }
        if (HIWORD(wp) != CBN_SELCHANGE || !ctx) return FALSE;
        const uint32_t v = selected_value(dlg, id);
        switch (id) {
        case IDC_FORMAT: ctx->raw.format = v; break;
        case IDC_TYPE: ctx->raw.texture_type = v; break;
        case IDC_SRGB: ctx->raw.srgb = v; break;
        case IDC_FILTER: ctx->raw.filter = v; break;
        case IDC_MIPS: ctx->raw.mips = v; break;
        case IDC_QUALITY: ctx->raw.quality = v; break;
        default: return FALSE;
        }
        refresh(dlg, *ctx, id == IDC_FORMAT);
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
    Context ctx = {*inout, has_alpha};
    const INT_PTR r = DialogBoxParamW(self, MAKEINTRESOURCEW(IDD_SAVE), owner, dialog_proc, LPARAM(&ctx));
    if (r == -1 || r == 0) return DialogResult::Failed; // 0: the owner window was invalid
    if (r != IDOK) return DialogResult::Cancel;
    *inout = ctx.raw;
    return DialogResult::Ok;
}

// Tessalith DDS: drives TessalithDDS.8bi through its selectors as Photoshop would. MIT licence (see photoshop/LICENSE).
//   save_without_image_fails  a save that ends before the last band (Escape, or a host that stops sending rows)
//                             reports an error and leaves the existing file untouched (Note DC27).
//   dialog_after_pixels       the Save dialog opens after the pixels are read, greys the mip filter only for real
//                             transparency, honours the filter picked, and Cancel writes nothing (Note DC31).
//   unload_after_save         the plug-in survives Photoshop unloading it after a save (Note DC25). The engine's rayon
//                             workers outlive the save; if the module's code were unmapped under them, their next
//                             wake-up would execute freed memory (the BEX64 crash at TessalithDDS.8bi_unloaded+0x9483).
//
//   host_test.exe <path to TessalithDDS.8bi>
#pragma warning(push)
#pragma warning(disable : 4121) // the SDK's FormatRecord layout (see write.h)
#include "PIFormat.h"
#pragma warning(pop)

#include "../src/dialog.h" // the Save dialog's control IDs

#include <tlhelp32.h>

#include <set>
#include <stdio.h>
#include <string>
#include <vector>

typedef void (*Entry)(const int16, FormatRecordPtr, intptr_t *, int16 *);

static std::set<DWORD> own_threads() {
    std::set<DWORD> ids;
    HANDLE snap = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
    if (snap == INVALID_HANDLE_VALUE) return ids;
    THREADENTRY32 t = {};
    t.dwSize = sizeof t;
    for (BOOL ok = Thread32First(snap, &t); ok; ok = Thread32Next(snap, &t))
        if (t.th32OwnerProcessID == GetCurrentProcessId()) ids.insert(t.th32ThreadID);
    CloseHandle(snap);
    return ids;
}

static bool running(DWORD id) {
    HANDLE h = OpenThread(SYNCHRONIZE, FALSE, id);
    if (!h) return false;
    const bool alive = WaitForSingleObject(h, 0) == WAIT_TIMEOUT;
    CloseHandle(h);
    return alive;
}

// A host's view of one 64 x 64 8-bit RGB document being saved into an already open file.
struct Host {
    Entry entry;
    std::vector<unsigned char> rec = std::vector<unsigned char>(sizeof(FormatRecord), 0);
    FormatRecord *r = reinterpret_cast<FormatRecord *>(rec.data());
    Str255 err = {};
    intptr_t data = 0;

    Host(Entry e, HANDLE file) : entry(e) {
        r->errorString = &err;
        r->imageMode = plugInModeRGBColor;
        r->depth = 8;
        r->planes = 3;
        r->HostSupports32BitCoordinates = 1;
        r->imageSize32.h = 64;
        r->imageSize32.v = 64;
        r->dataFork = reinterpret_cast<intptr_t>(file);
        for (int16 i = 0; i < 16; i++) r->planeMap[i] = i;
    }
    int16 call(int16 selector) {
        int16 result = 0;
        entry(selector, r, &data, &result);
        return result;
    }
    // Fills every band the plug-in requests until it stops asking or fails.
    int16 send_image() {
        int16 result = 0;
        while (result == 0 && r->data) {
            memset(r->data, 0x80, size_t(r->theRect32.bottom - r->theRect32.top) * size_t(r->rowBytes));
            result = call(formatSelectorWriteContinue);
        }
        return result;
    }
    // Fills every requested band with a gradient (8-bit) and alpha(x, y) on plane 3 when requested.
    int16 send_rgba(int (*alpha)(int x, int y)) {
        int16 result = 0;
        while (result == 0 && r->data) {
            const VRect q = r->theRect32;
            for (int y = q.top; y < q.bottom; y++)
                for (int x = q.left; x < q.right; x++) {
                    unsigned char *px = static_cast<unsigned char *>(r->data) + (y - q.top) * r->rowBytes + x * r->colBytes;
                    px[0] = (unsigned char)(x * 4);
                    px[1] = (unsigned char)(y * 4);
                    px[2] = (unsigned char)((x ^ y) * 4);
                    if (r->hiPlane >= 3) px[3] = (unsigned char)alpha(x, y);
                }
            result = call(formatSelectorWriteContinue);
        }
        return result;
    }
    std::string message() const { return std::string(reinterpret_cast<const char *>(err + 1), err[0]); }
};

static std::string contents(const wchar_t *path) {
    std::string s;
    HANDLE f = CreateFileW(path, GENERIC_READ, FILE_SHARE_READ, nullptr, OPEN_EXISTING, 0, nullptr);
    if (f == INVALID_HANDLE_VALUE) return s;
    char buf[4096];
    DWORD n = 0;
    while (ReadFile(f, buf, sizeof buf, &n, nullptr) && n) s.append(buf, n);
    CloseHandle(f);
    return s;
}

static HANDLE open_existing(const wchar_t *path) {
    return CreateFileW(path, GENERIC_READ | GENERIC_WRITE, 0, nullptr, OPEN_EXISTING, 0, nullptr);
}

static void write_file(const wchar_t *path, const std::string &bytes) {
    HANDLE f = CreateFileW(path, GENERIC_WRITE, 0, nullptr, CREATE_ALWAYS, 0, nullptr);
    DWORD n = 0;
    WriteFile(f, bytes.data(), DWORD(bytes.size()), &n, nullptr);
    CloseHandle(f);
}

// The existing file must survive a save that never got its image; the host opens it without truncating.
static int save_without_image_fails(Entry entry, const wchar_t *path) {
    const std::string original(20000, 'x'); // longer than the DDS written below, so a left-over tail shows
    const char *expected = "Tessalith DDS could not read the image from Photoshop, so nothing was saved"; // no full stop (DC30)
    int bad = 0;
    { // WriteStart, then WriteFinish before any band (as after Escape)
        write_file(path, original);
        HANDLE f = open_existing(path);
        Host h(entry, f);
        int16 result = h.call(formatSelectorWritePrepare);
        result |= h.call(formatSelectorWriteStart);
        if (!result) result = h.call(formatSelectorWriteFinish);
        CloseHandle(f);
        const bool ok = result == errReportString && h.message() == expected && contents(path) == original;
        printf("  %s WriteFinish before the image: result %d \"%s\", file %s\n", ok ? "ok  " : "FAIL", result,
               h.message().c_str(), contents(path) == original ? "untouched" : "CHANGED");
        bad += ok ? 0 : 1;
    }
    { // A full save over the same file replaces the old bytes exactly.
        HANDLE f = open_existing(path);
        Host h(entry, f);
        int16 result = h.call(formatSelectorWritePrepare);
        result |= h.call(formatSelectorWriteStart);
        if (!result) result = h.send_image();
        if (!result) result = h.call(formatSelectorWriteFinish);
        CloseHandle(f);
        const std::string out = contents(path);
        const bool ok = result == 0 && out.size() > 128 && out.compare(0, 4, "DDS ") == 0 && out.find("xxx") == out.npos;
        printf("  %s full save over a longer file: result %d, %zu bytes\n", ok ? "ok  " : "FAIL", result, out.size());
        bad += ok ? 0 : 1;
    }
    DeleteFileW(path);
    printf("%s save_without_image_fails\n", bad ? "FAIL" : "PASS");
    return bad;
}

// Plays the user at the Save dialog from another thread: reads the mip filter control, optionally picks a filter by
// its label, then presses OK or Cancel.
struct DialogUser {
    const wchar_t *pick = nullptr; // filter label to choose, or nullptr
    bool cancel = false;
    bool untick_mips = false; // click the Mipmaps checkbox (after any pick)
    bool seen = false, filter_enabled = false, filter_enabled_after = false;
    LRESULT formats = 0; // entries in the Format list
    wchar_t filter_shown[32] = {};
};

static BOOL CALLBACK find_dialog(HWND w, LPARAM out) {
    DWORD pid = 0;
    GetWindowThreadProcessId(w, &pid);
    wchar_t cls[16] = {};
    GetClassNameW(w, cls, 16);
    if (pid == GetCurrentProcessId() && IsWindowVisible(w) && wcscmp(cls, L"#32770") == 0) {
        *reinterpret_cast<HWND *>(out) = w;
        return FALSE;
    }
    return TRUE;
}

static DWORD WINAPI play_dialog(void *arg) {
    DialogUser *u = static_cast<DialogUser *>(arg);
    HWND dlg = nullptr;
    for (int i = 0; i < 500 && !dlg; i++) {
        Sleep(20);
        EnumWindows(find_dialog, reinterpret_cast<LPARAM>(&dlg));
    }
    if (!dlg) return 0;
    Sleep(100); // let WM_INITDIALOG finish filling the controls
    u->seen = true;
    HWND filter = GetDlgItem(dlg, IDC_FILTER);
    u->filter_enabled = IsWindowEnabled(filter) != 0;
    if (u->pick) {
        SendMessageW(filter, CB_SELECTSTRING, WPARAM(-1), reinterpret_cast<LPARAM>(u->pick));
        SendMessageW(dlg, WM_COMMAND, MAKEWPARAM(IDC_FILTER, CBN_SELCHANGE), reinterpret_cast<LPARAM>(filter));
    }
    u->formats = SendMessageW(GetDlgItem(dlg, IDC_FORMAT), CB_GETCOUNT, 0, 0);
    if (u->untick_mips) SendMessageW(GetDlgItem(dlg, IDC_MIPS), BM_CLICK, 0, 0);
    u->filter_enabled_after = IsWindowEnabled(filter) != 0;
    GetWindowTextW(filter, u->filter_shown, 32);
    PostMessageW(dlg, WM_COMMAND, u->cancel ? IDCANCEL : IDOK, 0);
    return 0;
}

static int opaque_alpha(int, int) { return 255; }
static int floor_alpha(int x, int) { return x % 2 ? 254 : 255; } // Tessalith's own opaque BC7 floor
static int real_alpha(int x, int) { return x < 32 ? 255 : 100; }

// One interactive save of a 64 x 64 layer document (planes 4, transparency plane 3) into `path`: OptionsStart, the
// write selectors, and the dialog played by `u` once the pixels are in. Returns the last selector's result.
static int16 interactive_save(Entry entry, const wchar_t *path, int (*alpha)(int, int), DialogUser *u) {
    HANDLE f = open_existing(path);
    Host h(entry, f);
    h.r->planes = 4;
    h.r->transparencyPlane = 3;
    int16 result = h.call(formatSelectorOptionsStart);
    if (!result) result = h.call(formatSelectorWritePrepare);
    if (!result) result = h.call(formatSelectorWriteStart);
    const bool started = result == 0; // WriteFinish follows if and only if WriteStart succeeded (PIFormat.h)
    HANDLE t = CreateThread(nullptr, 0, play_dialog, u, 0, nullptr);
    if (!result) result = h.send_rgba(alpha);
    WaitForSingleObject(t, 15000);
    CloseHandle(t);
    if (started) {
        const int16 finish = h.call(formatSelectorWriteFinish);
        if (!result) result = finish;
    }
    CloseHandle(f);
    return result;
}

// The dialog opens after the pixels are read and greys the filter from the pixels' alpha (DC31).
static int dialog_after_pixels(Entry entry, const wchar_t *path) {
    int bad = 0;
    auto expect = [&](bool ok, const char *what) {
        printf("  %s %s\n", ok ? "ok  " : "FAIL", what);
        bad += ok ? 0 : 1;
    };
    const std::string original(20000, 'x');

    // Opaque layer: filter editable; a Kaiser pick changes the mips compared with Box.
    std::string kaiser, box;
    for (int i = 0; i < 2; i++) {
        write_file(path, original);
        DialogUser u;
        u.pick = i == 0 ? L"Kaiser" : L"Box";
        const int16 result = interactive_save(entry, path, i == 0 ? opaque_alpha : floor_alpha, &u);
        (i == 0 ? kaiser : box) = contents(path);
        expect(result == 0 && u.seen && u.filter_enabled && wcscmp(u.filter_shown, u.pick) == 0,
               i == 0 ? "opaque layer: dialog after the pixels, filter editable, Kaiser picked"
                      : "alpha 254/255 counts as opaque: filter editable, Box picked");
    }
    expect(kaiser.compare(0, 4, "DDS ") == 0 && box.compare(0, 4, "DDS ") == 0 && kaiser != box,
           "Kaiser is honoured (its DDS differs from the Box one)");

    // Real transparency: the filter is greyed and shows Box.
    {
        write_file(path, original);
        DialogUser u;
        const int16 result = interactive_save(entry, path, real_alpha, &u);
        expect(result == 0 && u.seen && !u.filter_enabled && wcscmp(u.filter_shown, L"Box") == 0,
               "transparent layer: filter greyed, shows Box");
    }

    // DC33: Colour lists four formats; unticking Mipmaps greys the filter and saves a single level.
    {
        write_file(path, original);
        DialogUser u;
        u.untick_mips = true;
        const int16 result = interactive_save(entry, path, opaque_alpha, &u);
        const std::string dds = contents(path);
        const bool one_level = dds.size() > 32 && dds.compare(0, 4, "DDS ") == 0 &&
                               (unsigned char)dds[28] <= 1 && dds[29] == 0 && dds[30] == 0 && dds[31] == 0;
        expect(result == 0 && u.seen && u.formats == 4 && u.filter_enabled && !u.filter_enabled_after && one_level,
               "Colour: 4 formats; Mipmaps unticked greys the filter and saves one level");
    }

    // Cancel at the end: userCanceledErr and the file untouched.
    {
        write_file(path, original);
        DialogUser u;
        u.cancel = true;
        const int16 result = interactive_save(entry, path, opaque_alpha, &u);
        expect(result == userCanceledErr && u.seen && contents(path) == original, "Cancel writes nothing");
    }
    DeleteFileW(path);
    printf("%s dialog_after_pixels\n", bad ? "FAIL" : "PASS");
    return bad;
}

int wmain(int argc, wchar_t **argv) {
    if (argc != 2) {
        printf("usage: host_test <TessalithDDS.8bi>\n");
        return 2;
    }
    wchar_t out[MAX_PATH + 1] = {};
    GetTempPathW(MAX_PATH, out);
    wcscat_s(out, L"tessalith_host_test.dds");

    const std::set<DWORD> before = own_threads();
    HMODULE m = LoadLibraryW(argv[1]);
    Entry entry = m ? reinterpret_cast<Entry>(GetProcAddress(m, "PluginMain")) : nullptr;
    if (!entry) {
        printf("FAIL: could not load PluginMain from the plug-in\n");
        return 1;
    }
    const uintptr_t base = reinterpret_cast<uintptr_t>(m);
    int failed = save_without_image_fails(entry, out); // also runs one full save, which starts the workers
    failed += dialog_after_pixels(entry, out);

    std::vector<DWORD> workers;
    for (DWORD id : own_threads())
        if (!before.count(id)) workers.push_back(id);
    if (workers.empty()) { // nothing outlives the save, so the check below would prove nothing
        printf("FAIL unload_after_save: the save left no threads behind; the test no longer checks anything\n");
        return 1;
    }

    FreeLibrary(m); // Photoshop unloads a format plug-in it is done with
    Sleep(2000);

    int alive = 0;
    for (DWORD id : workers) alive += running(id) ? 1 : 0;
    MEMORY_BASIC_INFORMATION mbi = {};
    const bool mapped = VirtualQuery(reinterpret_cast<void *>(base), &mbi, sizeof mbi) && mbi.State != MEM_FREE;
    if (!mapped) {
        printf("FAIL unload_after_save: FreeLibrary unmapped the plug-in while %d of its %zu threads still run\n",
               alive, workers.size());
        return 1;
    }
    printf("PASS unload_after_save (%zu threads outlive the save, %d running; the module stays mapped)\n",
           workers.size(), alive);
    return failed ? 1 : 0;
}

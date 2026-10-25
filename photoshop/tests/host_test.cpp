// Tessalith DDS: drives TessalithDDS.8bi through its selectors as Photoshop would. MIT licence (see photoshop/LICENSE).
//   save_without_image_fails  a save that ends before the last band (Escape, or a host that stops sending rows)
//                             reports an error and leaves the existing file untouched (Note DC27).
//   unload_after_save         the plug-in survives Photoshop unloading it after a save (Note DC25). The engine's rayon
//                             workers outlive the save; if the module's code were unmapped under them, their next
//                             wake-up would execute freed memory (the BEX64 crash at TessalithDDS.8bi_unloaded+0x9483).
//
//   host_test.exe <path to TessalithDDS.8bi>
#pragma warning(push)
#pragma warning(disable : 4121) // the SDK's FormatRecord layout (see write.h)
#include "PIFormat.h"
#pragma warning(pop)

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
    const char *expected = "Tessalith DDS could not read the image from Photoshop, so nothing was saved.";
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

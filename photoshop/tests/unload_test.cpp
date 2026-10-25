// Tessalith DDS: the plug-in survives Photoshop unloading it after a save (Note DC25). Loads TessalithDDS.8bi,
// drives one save through the write selectors as a host would, calls FreeLibrary and waits. The engine's rayon
// workers outlive the save; if the module's code were unmapped under them, the next wake-up would execute freed
// memory (the BEX64 crash at TessalithDDS.8bi_unloaded+0x9483). MIT licence (see photoshop/LICENSE).
//
//   unload_test.exe <path to TessalithDDS.8bi>
#pragma warning(push)
#pragma warning(disable : 4121) // the SDK's FormatRecord layout (see write.h)
#include "PIFormat.h"
#pragma warning(pop)

#include <tlhelp32.h>

#include <set>
#include <stdio.h>
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

// One 64 x 64 8-bit RGB save with the remembered (here: default) settings, as Photoshop drives it.
static bool save(Entry entry, const wchar_t *path) {
    HANDLE f = CreateFileW(path, GENERIC_WRITE, 0, nullptr, CREATE_ALWAYS, FILE_ATTRIBUTE_TEMPORARY, nullptr);
    if (f == INVALID_HANDLE_VALUE) return false;
    std::vector<unsigned char> rec(sizeof(FormatRecord), 0);
    FormatRecord *r = reinterpret_cast<FormatRecord *>(rec.data());
    Str255 err = {};
    r->errorString = &err;
    r->imageMode = plugInModeRGBColor;
    r->depth = 8;
    r->planes = 3;
    r->HostSupports32BitCoordinates = 1;
    r->imageSize32.h = 64;
    r->imageSize32.v = 64;
    r->dataFork = reinterpret_cast<intptr_t>(f);
    for (int16 i = 0; i < 16; i++) r->planeMap[i] = i;
    intptr_t data = 0;
    int16 result = 0;
    entry(formatSelectorWritePrepare, r, &data, &result);
    entry(formatSelectorWriteStart, r, &data, &result);
    while (result == 0 && r->data) {
        memset(r->data, 0x80, size_t(r->theRect32.bottom - r->theRect32.top) * size_t(r->rowBytes));
        entry(formatSelectorWriteContinue, r, &data, &result);
    }
    int16 finish = 0;
    entry(formatSelectorWriteFinish, r, &data, &finish);
    LARGE_INTEGER size = {};
    GetFileSizeEx(f, &size);
    CloseHandle(f);
    DeleteFileW(path);
    if (result != 0) printf("  save failed: %d \"%.*s\"\n", result, err[0], reinterpret_cast<const char *>(err + 1));
    return result == 0 && size.QuadPart > 128;
}

int wmain(int argc, wchar_t **argv) {
    if (argc != 2) {
        printf("usage: unload_test <TessalithDDS.8bi>\n");
        return 2;
    }
    wchar_t out[MAX_PATH + 1] = {};
    GetTempPathW(MAX_PATH, out);
    wcscat_s(out, L"tessalith_unload_test.dds");

    const std::set<DWORD> before = own_threads();
    HMODULE m = LoadLibraryW(argv[1]);
    Entry entry = m ? reinterpret_cast<Entry>(GetProcAddress(m, "PluginMain")) : nullptr;
    if (!entry) {
        printf("FAIL unload_after_save: could not load PluginMain from the plug-in\n");
        return 1;
    }
    const uintptr_t base = reinterpret_cast<uintptr_t>(m);
    if (!save(entry, out)) {
        printf("FAIL unload_after_save: the save did not write a DDS\n");
        return 1;
    }
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
    return 0;
}

// Tessalith DDS: the Save dialog's presets in <dir>\presets.txt (DC37). One preset per line, UTF-8:
// name TAB format TAB texture_type TAB srgb TAB filter TAB mips TAB quality TAB discard_alpha TAB alpha_mode, after a
// header line without tabs. A line without alpha_mode (written before Note DH) loads with alpha_mode 0. A name may start with '#'. MIT licence (see photoshop/LICENSE).
#include "presets.h"

#include <windows.h>

#include <stdio.h>
#include <string.h>

namespace {

const wchar_t kFile[] = L"\\presets.txt";
const char kHeader[] =
    "# Tessalith DDS presets: name, format, type, sRGB, filter, mips, quality, discard transparency, alpha is transparency\r\n";
// The header written before Note DH: dropped on load like kHeader, so it is not kept as an unparsed line.
const char kOldHeader[] = "# Tessalith DDS presets: name, format, type, sRGB, filter, mips, quality, discard transparency\r\n";
constexpr int kFields = 8;    // PsddsSettings values on a line written now
constexpr int kOldFields = 7; // before Note DH (no alpha_mode)
// A preset line is under 300 bytes; this bounds what a corrupt or foreign file can make the dialog allocate.
const LONGLONG kMaxFileBytes = 1 << 20;

uint32_t *field(PsddsSettings &s, int i) {
    uint32_t *f[kFields] = {&s.format, &s.texture_type, &s.srgb,         &s.filter,
                            &s.mips,   &s.quality,      &s.discard_alpha, &s.alpha_mode};
    return f[i];
}

std::wstring from_utf8(const std::string &s) {
    if (s.empty()) return std::wstring();
    const int n = MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, s.data(), int(s.size()), nullptr, 0);
    if (n <= 0) return std::wstring();
    std::wstring w(size_t(n), L'\0');
    MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, s.data(), int(s.size()), &w[0], n);
    return w;
}

std::string to_utf8(const std::wstring &w) {
    if (w.empty()) return std::string();
    const int n = WideCharToMultiByte(CP_UTF8, 0, w.data(), int(w.size()), nullptr, 0, nullptr, nullptr);
    if (n <= 0) return std::string();
    std::string s(size_t(n), '\0');
    WideCharToMultiByte(CP_UTF8, 0, w.data(), int(w.size()), &s[0], n, nullptr, nullptr);
    return s;
}

// A decimal number below 2^32, nothing else.
bool parse_u32(const std::wstring &t, uint32_t *out) {
    if (t.empty() || t.size() > 10) return false;
    unsigned long long v = 0;
    for (wchar_t c : t) {
        if (c < L'0' || c > L'9') return false;
        v = v * 10 + unsigned(c - L'0');
    }
    if (v > 0xFFFFFFFFull) return false;
    *out = uint32_t(v);
    return true;
}

bool parse_line(const std::wstring &line, Preset *out) {
    std::vector<std::wstring> parts;
    size_t start = 0;
    for (;;) {
        const size_t tab = line.find(L'\t', start);
        parts.push_back(line.substr(start, tab == std::wstring::npos ? std::wstring::npos : tab - start));
        if (tab == std::wstring::npos) break;
        start = tab + 1;
    }
    const int values = int(parts.size()) - 1;
    if (values != kFields && values != kOldFields) return false;
    Preset p;
    p.settings.alpha_mode = 0; // a line from before Note DH
    p.name = CleanPresetName(parts[0]);
    if (p.name.empty() || p.name != parts[0]) return false;
    for (int i = 0; i < values; i++)
        if (!parse_u32(parts[size_t(i) + 1], field(p.settings, i))) return false;
    PsddsSettings effective;
    PsddsControls controls;
    if (psdds_resolve(&p.settings, 0, &effective, &controls) != 0) return false;
    *out = p;
    return true;
}

bool make_dirs(const std::wstring &dir) {
    // Create each missing folder from the left; an existing one is fine. A UNC path (a redirected %APPDATA%) starts
    // below \\server\share, which cannot be created.
    size_t first = 3;
    if (dir.compare(0, 2, L"\\\\") == 0) {
        const size_t server = dir.find(L'\\', 2), share = server == std::wstring::npos ? server : dir.find(L'\\', server + 1);
        if (share == std::wstring::npos) return true;
        first = share + 1;
    }
    for (size_t i = dir.find(L'\\', first); ; i = dir.find(L'\\', i + 1)) {
        const std::wstring part = dir.substr(0, i);
        if (!CreateDirectoryW(part.c_str(), nullptr) && GetLastError() != ERROR_ALREADY_EXISTS) return false;
        if (i == std::wstring::npos) return true;
    }
}

} // namespace

std::wstring PresetDir() {
    wchar_t buf[MAX_PATH] = {};
    DWORD n = GetEnvironmentVariableW(L"TESSALITH_DDS_PRESETS_DIR", buf, MAX_PATH);
    if (n > 0 && n < MAX_PATH) return std::wstring(buf, n);
    n = GetEnvironmentVariableW(L"APPDATA", buf, MAX_PATH);
    if (n == 0 || n >= MAX_PATH) return std::wstring();
    return std::wstring(buf, n) + L"\\Tessalith DDS\\presets";
}

std::vector<Preset> LoadPresets(const std::wstring &dir, PresetFileInfo *info) {
    std::vector<Preset> list;
    PresetFileInfo none;
    PresetFileInfo &out = info ? *info : none;
    out = PresetFileInfo{};
    if (dir.empty()) return list;
    const std::wstring path = dir + kFile;
    HANDLE f = CreateFileW(path.c_str(), GENERIC_READ, FILE_SHARE_READ, nullptr, OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL,
                           nullptr);
    if (f == INVALID_HANDLE_VALUE) {
        const DWORD e = GetLastError();
        if (e != ERROR_FILE_NOT_FOUND && e != ERROR_PATH_NOT_FOUND) { // there but unreadable: never written over (DC40)
            out.complete = false;
            return list;
        }
        for (uint32_t i = 0; i < psdds_preset_count(); i++) {
            Preset p;
            const char *name = psdds_preset_name(i);
            if (!name || psdds_preset_settings(i, &p.settings) != 0) continue;
            p.name = from_utf8(name);
            list.push_back(p);
        }
        SavePresets(dir, list); // a failure only means the seeds are offered again next time
        return list;
    }
    LARGE_INTEGER size = {};
    std::string bytes;
    out.complete = false;
    if (GetFileSizeEx(f, &size) && size.QuadPart <= kMaxFileBytes) {
        bytes.resize(size_t(size.QuadPart));
        DWORD got = 0;
        out.complete = bytes.empty() || (ReadFile(f, &bytes[0], DWORD(bytes.size()), &got, nullptr) && got == bytes.size());
        if (!out.complete) bytes.clear();
    }
    CloseHandle(f);
    size_t start = 0;
    while (start < bytes.size()) {
        size_t end = bytes.find('\n', start);
        if (end == std::string::npos) end = bytes.size();
        std::string line = bytes.substr(start, end - start);
        if (!line.empty() && line.back() == '\r') line.pop_back();
        start = end + 1;
        Preset p;
        if (parse_line(from_utf8(line), &p)) list.push_back(p);
        else if (!line.empty() && line + "\r\n" != kHeader && line + "\r\n" != kOldHeader) out.unparsed.push_back(line); // kept verbatim on the next write
    }
    return list;
}

bool SavePresets(const std::wstring &dir, const std::vector<Preset> &list, const std::vector<std::string> &unparsed) {
    if (dir.empty() || !make_dirs(dir)) return false;
    std::string text = kHeader;
    for (const Preset &p : list) {
        if (CleanPresetName(p.name) != p.name || p.name.empty()) return false;
        text += to_utf8(p.name);
        PsddsSettings s = p.settings;
        for (int i = 0; i < kFields; i++) {
            char num[16];
            snprintf(num, sizeof num, "\t%u", *field(s, i));
            text += num;
        }
        text += "\r\n";
    }
    for (const std::string &line : unparsed) text += line + "\r\n";
    const std::wstring path = dir + kFile;
    const std::wstring tmp = path + L".tmp";
    HANDLE f = CreateFileW(tmp.c_str(), GENERIC_WRITE, 0, nullptr, CREATE_ALWAYS, FILE_ATTRIBUTE_NORMAL, nullptr);
    if (f == INVALID_HANDLE_VALUE) return false;
    DWORD put = 0;
    const bool written =
        WriteFile(f, text.data(), DWORD(text.size()), &put, nullptr) && put == text.size() && FlushFileBuffers(f);
    CloseHandle(f);
    if (!written || !MoveFileExW(tmp.c_str(), path.c_str(), MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH)) {
        DeleteFileW(tmp.c_str());
        return false;
    }
    return true;
}

std::wstring CleanPresetName(const std::wstring &name) {
    const wchar_t *space = L" \t\r\n\v\f\u00A0\u3000";
    const size_t first = name.find_first_not_of(space);
    if (first == std::wstring::npos) return std::wstring();
    const std::wstring t = name.substr(first, name.find_last_not_of(space) - first + 1);
    if (t.size() > 64) return std::wstring();
    for (wchar_t c : t)
        if (c < 0x20 || c == 0x7f) return std::wstring();
    return t;
}

int FindPreset(const std::vector<Preset> &list, const std::wstring &name) {
    for (size_t i = 0; i < list.size(); i++)
        if (CompareStringOrdinal(list[i].name.c_str(), int(list[i].name.size()), name.c_str(), int(name.size()),
                                 TRUE) == CSTR_EQUAL)
            return int(i);
    return -1;
}

// The effective settings; Discard transparency has no effect without alpha, so it is zeroed there (DC40).
static bool effective_of(const PsddsSettings &raw, uint32_t has_alpha, uint32_t depth, PsddsSettings *out) {
    PsddsControls c;
    uint32_t discard_enabled = 0;
    if (psdds_resolve_depth(&raw, has_alpha, depth, out, &c, &discard_enabled) != 0) return false;
    if (!has_alpha) out->discard_alpha = 0;
    return true;
}

int MatchPreset(const std::vector<Preset> &list, const PsddsSettings &raw, uint32_t has_alpha, uint32_t depth, int preferred) {
    PsddsSettings want, got;
    if (!effective_of(raw, has_alpha, depth, &want)) return -1;
    if (preferred >= 0 && size_t(preferred) < list.size() &&
        effective_of(list[size_t(preferred)].settings, has_alpha, depth, &got) && memcmp(&want, &got, sizeof want) == 0)
        return preferred;
    for (size_t i = 0; i < list.size(); i++)
        if (effective_of(list[i].settings, has_alpha, depth, &got) && memcmp(&want, &got, sizeof want) == 0) return int(i);
    return -1;
}

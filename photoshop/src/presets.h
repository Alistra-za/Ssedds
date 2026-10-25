// Tessalith DDS: the Save dialog's presets, kept in one UTF-8 file (DC37). MIT licence (see photoshop/LICENSE).
#pragma once

#include <string>
#include <vector>

#include "../psdds.h"

struct Preset {
    std::wstring name;
    PsddsSettings settings; // raw, as the dialog holds them (DC19)
};

// %APPDATA%\Tessalith DDS\presets, or TESSALITH_DDS_PRESETS_DIR when set (tests). Empty when neither is set.
std::wstring PresetDir();

// What LoadPresets learned about the file (DC40).
struct PresetFileInfo {
    bool complete = true;                // false: the file exists but was not read in full; it must not be written
    std::vector<std::string> unparsed;   // lines that did not parse, verbatim, to be written back by SavePresets
};

// The presets in <dir>\presets.txt, in file order. When the file does not exist yet, it is created with psdds's seed
// presets (DC35) and those are returned. A line that does not parse, whose name is invalid, or whose values
// psdds_resolve refuses is skipped (kept in info->unparsed). A file that exists but cannot be read in full (open fails
// other than not-found, over 1 MiB, short read) gives an empty list and info->complete == false: callers must not
// call SavePresets then. A missing file counts as complete.
std::vector<Preset> LoadPresets(const std::wstring &dir, PresetFileInfo *info = nullptr);

// Writes the whole list to <dir>\presets.txt (creating the folders), through a temporary file that is flushed and
// then renamed over the old one. false when any step fails; the old file is then unchanged.
// `unparsed` lines are appended unchanged (PresetFileInfo::unparsed).
bool SavePresets(const std::wstring &dir, const std::vector<Preset> &list, const std::vector<std::string> &unparsed = {});

// The name trimmed of white space, or empty when it is then empty, longer than 64 characters, or holds a control
// character (tab, line break).
std::wstring CleanPresetName(const std::wstring &name);

// The index of the preset called `name`, compared ordinally ignoring case, or -1.
int FindPreset(const std::vector<Preset> &list, const std::wstring &name);

// The first preset whose effective settings (psdds_resolve_depth with the document's has_alpha and depth) equal those of `raw`, or
// -1 when none does ("Custom").
// Discard transparency is ignored when has_alpha is 0 (it has no effect). `preferred` (the preset the user picked) wins
// over an earlier preset with equal settings when it still matches (DC40).
int MatchPreset(const std::vector<Preset> &list, const PsddsSettings &raw, uint32_t has_alpha, uint32_t depth, int preferred = -1);

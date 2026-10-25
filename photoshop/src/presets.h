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

// The presets in <dir>\presets.txt, in file order. When the file does not exist yet, it is created with psdds's seed
// presets (DC35) and those are returned. A line that does not parse, whose name is invalid, or whose values
// psdds_resolve refuses is skipped. A file that exists but cannot be read gives an empty list and is left alone.
std::vector<Preset> LoadPresets(const std::wstring &dir);

// Writes the whole list to <dir>\presets.txt (creating the folders), through a temporary file that is flushed and
// then renamed over the old one. false when any step fails; the old file is then unchanged.
bool SavePresets(const std::wstring &dir, const std::vector<Preset> &list);

// The name trimmed of white space, or empty when it is then empty, longer than 64 characters, or holds a control
// character (tab, line break).
std::wstring CleanPresetName(const std::wstring &name);

// The index of the preset called `name`, compared ordinally ignoring case, or -1.
int FindPreset(const std::vector<Preset> &list, const std::wstring &name);

// The first preset whose effective settings (psdds_resolve with the document's has_alpha) equal those of `raw`, or
// -1 when none does ("Custom").
int MatchPreset(const std::vector<Preset> &list, const PsddsSettings &raw, uint32_t has_alpha);

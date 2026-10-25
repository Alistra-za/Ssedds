// Tessalith DDS: the remembered Save settings, kept in Photoshop's Descriptor Registry. MIT licence (see photoshop/LICENSE).
#pragma once

#include "../psdds.h"

struct SPBasicSuite;

// The last settings saved with OK, as stored (raw), after psdds_resolve has accepted them. psdds_defaults(Colour)
// on first run, when the registry suites are unavailable, or when a stored value is missing or out of range.
PsddsSettings load_settings(SPBasicSuite *basic);

// Stores `s` persistently (kept across Photoshop restarts). Silently does nothing without the suites.
void save_settings(SPBasicSuite *basic, const PsddsSettings &s);

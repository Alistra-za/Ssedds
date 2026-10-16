Tessalith 0.8.0-alpha.1
=======================

Tessalith converts a folder of source images (PNG, TGA, TIFF, EXR, PSD,
Krita, OpenRaster) to DDS for Skyrim SE/AE. It works out what each file is
from its name, groups files into material sets, packs loose maps into
Skyrim's layouts, builds the mips and picks the format and colour tag.

This is an ALPHA. Back up your mod folders first.

Files
-----
  tessalith.exe   the GUI
  sdc.exe         the command line (same engine, same sdc.toml)
  CHANGELOG.md, LICENSE, LICENSES\, THIRD_PARTY_NOTICES.md,
  THIRD_PARTY_LICENSES.md

Requirements
------------
  - Windows 10 or 11, x64.
  - A graphics card for the GUI's preview windows. The CLI needs none.
  - Optional: NVIDIA Texture Tools 3.2.5 (nvcompress.exe) as the BC7/BC6H
    encoder. It is not bundled; install it yourself if you want it.
  - Nothing to install: unzip anywhere and run.

Start
-----
  GUI:  double-click tessalith.exe, then Open folder, pick the Mode, settle
        any amber-bordered texture, Export.
  CLI:  open a terminal in this folder and run
          sdc inspect textures --recursive
          sdc convert textures --recursive -o out --accept-suggestions high
          sdc validate out\textures\armor\iron\iron_n.dds --target cs

SmartScreen
-----------
The exes are not code-signed, so Windows SmartScreen may warn on first run.
Click "More info", then "Run anyway".

Help
----
  Documentation, known issues and bug reports:
    https://github.com/Alistra-za/Tessalith
    Wiki pages: Known-Issues, Troubleshooting, GUI-Guide, CLI-Reference
  If the GUI crashes, attach the newest file in
    %LOCALAPPDATA%\sdc\logs\
  to your report.

Licence
-------
GPL-3.0-or-later. See LICENSE. Third-party licences: THIRD_PARTY_NOTICES.md
and LICENSES\.

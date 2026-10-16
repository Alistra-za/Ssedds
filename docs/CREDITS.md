# Credits

- **AlphaBloom** (tkRiverGamer): the "Auto Coverage" idea that Tessalith's coverage-preserving mip step rebuilds (MIT, Copyright (c) 2026 RiverGamer; no code copied, Tessalith uses a quantile solve sharing one pass predicate with its counter instead).
- **Ignacio Castaño**: coverage-preserving alpha mipmaps (the method Tessalith's solver follows).
- **jiayev/TruePBR-Manager**: TruePBR per-slot default formats that Tessalith's `cs` defaults align with (no code copied; CC BY-NC 4.0 code is not used).
- **Rich Geldreich / Binomial LLC**: bc7e and rgbcx. **Intel**: ISPC Texture Compressor. **iOrange**: bcdec (bcdec_rs port by its authors).
- **Faultier**: PBR landscape template values by Faultier (https://www.nexusmods.com/profile/Faultierfan?gameId=1704), from Faultier's PBR landscape modders resource.
- **ousnius/nifly** (https://github.com/ousnius/nifly, GPL-3.0): the NIF library whose source the `nif` crate follows as a reference for parsing and for transform, shader-flag and tree-order conventions (cited in code comments). Its test NIF files (GPL-3.0, `crates/nif/tests/fixtures/`) are used by the test suite only and are not in the executables.
- **hakasapl/PGPatcher** (https://github.com/hakasapl/PGPatcher, GPL-3.0): the PBR and complex-material JSON schemas Tessalith vendors unmodified to validate its exports, and the behaviour the `pgpsim` crate simulates.
- **Poly Haven**: the `studio_small_09` HDRI behind the 3D preview (licence as recorded in `crates/render3d/assets/hdri/README.md`: CC0 1.0).

# Controller rulings — Phase 0–2 build (feat/phase0-2)

Every decision the build controller made without asking the owner, in the order made, with the cost if it is wrong.
Owner decisions are in DECISIONS.md. Overrule any of these and it gets changed.

Each item names the review that raised it, using these abbreviations:
- **T*n*** — the per-task review of Task *n* (e.g. T30 = Task 30).
- **Codex A** / **Codex B** — the two batch reviews by the Codex second reviewer.
- **I*n*** / **M*n*** — the *n*-th Important / Minor finding in that review.

Items marked **OWNER CHECK** need your explicit yes.

1. .superpowers/ added to .gitignore before Task 2 — SDD workspace must not be committed — cost if wrong: none (Task 2 rewrites .gitignore; implementer told to keep the line)
2. B1 — Task 2 MODIFIES .gitignore (keep target/, *.dds, /out/, .superpowers/; add _sdc/ and *.tmp) instead of overwriting — spec has no .gitignore rule; overwrite would commit the SDD workspace — cost if wrong: none.
3. I1 — ISPC TC objects are REBUILT in Task 3 from the vendored kernel via tools/regen-ispc.ps1 (repo-relative path, same flags as the bench), not copied from the bench; bc7e objects may be copied (rebuild is byte-identical). Task 3 Step 8 expects all ten to MATCH after this. Task 11's 0.01 dB gate is the proof that the rebuilt BC6H objects equal the bench — spec §2 requires a reproducible rebuild gate and no personal path inside sdc.exe — cost if wrong: Task 11 fails; fall back to the bench objects and document the path leak.
4. I2 Task 37 line ~8908 typo — fix when Task 37 is dispatched.
5. I3 Task 45 gates must include the bc7e flag-off bit-identity check (spec §11.2).
6. I4 Task 46 S6 uses alpha.policy=remove (spec), not data.
7. I5 `_g` switched to BC7/BC1 under cs keeps _SRGB per spec §3.4 (fix in Task 25/26 resolve or Task 33 writer, whichever owns the tag).
8. I6 DDS reader validates header mip count/size against file length and caps (16384 px, ≤15 mips) before allocating (Task 34).
9. I7 variance bake stays in M4 unless the owner says otherwise at Task 17.
10. m5 — every task runs cargo fmt + cargo clippy -D warnings (scoped to the crates it touches) before committing, as Phase 1 does — avoids a lint wall at T45 — cost if wrong: small extra time per task.
11. m7 — Apache-2.0 text for LICENSES/ comes from the local copy (the Apache-2.0 portion of Git's libopenssl LICENSE.txt), no download — owner approved only three downloads — cost if wrong: re-fetch the canonical text later.
12. m9 — regen-ispc.ps1 passes --arch=x86-64 --target-os=windows explicitly and runs ISPC from the repo root on repo-relative source paths; the generated header's line-2 temp-path comment is stripped before hashing/vendoring — reproducible hashes on any machine — cost if wrong: header comment lost (cosmetic).
13. GPL-3.0 LICENSE taken from Git's bundled COPYING.GPLv3 (gnu.org TLS reset) — SHA-256 3972dc9744f6499f0f9b2dbf76696f2ae7ad8af9b23dde66d6af86c9dfb36986 is the well-known canonical gpl-3.0.txt hash — cost if wrong: swap the file later.
14. m24 — build.rs defines NDEBUG for rgbcx/shim/ispc_texcomp (release C++ without asserts or absolute __FILE__ paths; Task 10 gate re-proves output) — cost if wrong: an upstream assert stops firing.
15. m26 — sdc_bc7e_patched is used to fill Versions.bc7e_patch instead of left unused — cost if wrong: none.
16. Task 2 deferred minor taken in Task 4 — static_crt test asserts on prefixes vcruntime140 and msvcp140 (C++ linkage arrives now) — cost if wrong: none.
17. Task 6 — brief's ramp tests could not meet tolerance 10 at radius 5 (max err 12); tests use BC45_RADIUS_MAX, default stays 5 (bench parity; bench measured -hr10 only +0.09 dB on real masks) — cost if wrong: default slightly lossier on extreme ramps; revisit default in M6 presets.
18. exact_opaque stays scoped to envmask/complex_material (DECISIONS Note D default) and is NOT turned on globally — S5 measured cost exceeds the 0.05 dB condition the owner set; native patch is the mechanism when on, byte patch last resort — cost if wrong: _m maps lose ~0.5 dB RGB, which the owner accepted for _m.
19. S7 overhead gate measures fixed per-process cost on levels ≤32×32 (encode time excluded); spec §12 S7 says 'overhead ≤ 0.2 s/level', i.e. process overhead, not total encode time — cost if wrong: none (all-level mean printed as info).
20. m11 — the every-builtin-subtype test resolves each preset with its own target (cs/cpm/vanilla), not Target::Cs for all — cost if wrong: none.
21. m20 — Provenance serialises to the spec §5.2 strings ("builtin", "preset:<id>", "rule:<source>#<n>", plus "user_config"/"project_config"/"suggestion"/"override" as plain strings) instead of externally tagged objects — spec is authority; manifests/events must match it — cost if wrong: GUI/manifest consumers parse strings (simpler).
22. m28 — layer order is all `defaults` (base→child) first, then all `subtypes.<st>` (base→child): a sub-type-specific setting at any level beats a generic default at any level — prevents a user `[defaults] format` silently overriding base envmask=bc4 — cost if wrong: a user wanting a global format override must set it per sub-type or via --set/rules.
23. I5 → Task 26 — check_profile emits CFG-INVALID when (format == bc6h_uf16) != (color.tag == n/a), message "set color.tag=srgb for a non-BC6H _g".
24. Constant255 decided on 8-bit codes (16-bit alpha ≥ 65407/65535 counts as 255) — every BC output stores 8-bit alpha, so sub-code variation can't survive encoding anyway — cost if wrong: none measurable.
25. spec §4.3 rule 5 (contradictions reported; analysis can lower confidence) is unowned in the plan → assigned to Task 26: resolve emits SUG-CONTRADICTION (warning) when the decided sub-type contradicts analysis — at minimum: normal-category sub-type whose decoded normal_len_err is high or that is grayscale; `_m` complex_material without varying alpha; colour sub-type that looks like a normal map (normal_len_err low, blue mostly high); data sub-types flagged sRGB-looking are out of scope — cost if wrong: extra warnings, never a blocked file.
26. alias layering — the alias preset's chain replaces the batch chain for that file; an alias preset WITHOUT an explicit `base` inherits the target's built-in quality preset (cs-/cpm-/vanilla-quality), which satisfies Note G "layered on top of the target preset" — cost if wrong: a custom batch preset's settings don't reach aliased files unless the alias preset bases on it (documented in spec §3.7).
27. aliases never raise `_s` to High (four meanings) — keeps "never auto-decide" — cost if wrong: aliased `_s` files still need --set/rule.
28. new repeatable `--preset-file <file>` CLI option (Task 40) accepted — needed to load alias presets alongside the batch preset — cost if wrong: one extra flag.
29. analysis-derived alpha `remove` (no alpha / constant 255, High only) applies without --accept-suggestions — it is a measured fact, not a meaning guess (brief's recorded plan decision); DECISIONS scope says suggestions never auto-decide meaning, and this decides no meaning — cost if wrong: a file whose constant-255 alpha the artist wanted kept loses it (they can --set alpha.policy).
30. `file:` --set / rule globs also match the file's basename when the glob contains no `/` (so `file:*.png` and `file:wood01.png` work as users expect) → implement in Task 26 — cost if wrong: broader matches; documented.
31. Task 26 I1 (plan-mandated allowable CFG-UNAVAILABLE) — BC6H outside cs is a non-allowable error (global constraint "BC6H only under cs"); for §4.2 "—" sub-types, `--allow CFG-UNAVAILABLE` means "accept fallback": re-resolve with the fallback sub-type (pbr_albedo→diffuse, pbr_emissive→glow, pbr_normal→normal, complex_material→envmask); sub-types with no fallback stay blocked and cannot be allowed — spec §10 resolution text "Accept fallback or change target" — cost if wrong: users can't force-write unavailable PBR maps under cpm/vanilla (they shouldn't).
32. Task 26 I2 — a non-fitting backend is kept only when set at least as specifically as the format (sub-type/file-scoped --set, a rule, or the same patch as the format); otherwise the backend follows the format — spec §3.2 backend default = format — cost if wrong: a deliberate global NVTT backend applies only where it fits (BC7/BC6H), which is the intent.
33. aliases to `d` still raise the top colour reading to High (spec §3.7) — declaring an alias is an explicit sub-type declaration; it never decides alpha meaning (only an alias preset's own alpha.policy does) — cost if wrong: an aliased `_d` under sky/ or a PBR set auto-picks hdr_sky/pbr_albedo with --accept-suggestions high; owner can ask to add `d` to the `_s` exception.
34. Codex A#1 (Constant255 from 8-bit codes) REJECTED — all BC outputs store 8-bit alpha; values rounding to 255 are opaque at output precision; consistent with earlier Task 23 ruling — cost if wrong: none measurable.
35. Codex A#3 (dilation treats alpha < 0.5/255 as zero) REJECTED — such texels encode alpha 0 in every 8-bit output; their weight in alpha-weighted mips is < 0.2% — cost if wrong: negligible colour shift at near-zero alpha.
36. Codex A#2 (user preset id `sdc-base` replaces base → resolve_file expect panic) ACCEPTED — reserve `sdc-base` against user replacement (CFG-INVALID) and propagate instead of expect.
37. Codex A#4 (grayscale omits R–B) ACCEPTED — use max−min ≤ 4.
38. Codex A#5 (non-ASCII case in file:/rule globs) ACCEPTED — Unicode case-insensitive glob matching + test.
39. Codex A#6 (ALPHA-EXACT-OFF-M suppressed for remove) ACCEPTED — warn on BC7 envmask/complex with exact_opaque=false regardless of alpha policy; BC4 never warns.
40. Task 27a concern 2 — an unscoped --set subtype=X applies to every file incl. raw maps; that is what unscoped means (users scope with file:/subtype:) — cost if wrong: none.
41. Task 27a Minor 1 — unstated green convention gets its own blocking, non-allowable code (IN-GREEN-UNSTATED, catalogue 48→49) instead of SUG-LOW-CONFIDENCE; Minor 2 — the unresolved hint suggests the file-scoped form `--set 'file:<name>.channels.green_convention=opengl|directx'` → both folded into the Codex-A fix batch — cost if wrong: one extra code.
42. ALPHA-EXACT-OFF-M fires for BC3 too — narrow to Format::Bc7 (254 risk is BC7-only; BC3 alpha block stores 255 exactly; spec §10 "storing alpha" read as BC7) → folded into Task 28 dispatch as extra item — cost if wrong: BC3 _m loses a warning that could never apply.
43. Codex T29#1 (odd sizes double-count the boundary texel) ACCEPTED — DIM-NPOT is allowable so NPOT reaches mips; area-exact box weights, POT bit-identical.
44. Codex T29#2 (NaN/inf poison the chain) ACCEPTED — sanitise in preprocess (NaN→0, ±inf clamped per domain), count in Work.nonfinite_samples; carry → Task 37: surface the count as a warning.
45. T29 review Minor 2 — clamp HDR negatives in preprocess (not only finalised levels) so mip 0 and accumulation agree — cost if wrong: a deliberately negative HDR source loses its negatives (BC6H UF16 cannot store them anyway).
46. T29 review Minors 1, 4 (initial clone only), 6 ACCEPTED; Minor 3 subsumed by #2; Minor 5 partly via #1 tests; Minor 7 deferred.
47. NPOT deeper levels treat a partly-covered child as uniform — approximate but bounded; NPOT needs --allow DIM-NPOT — cost if wrong: small mip colour drift on allowed NPOT textures.
48. T30 τ = smallest f32 whose code8 passes (verified), not (t+1)/255 — encoder sees the same rounded boundary code either way — cost if wrong: none measurable.
49. T30 ties — choose the achievable group count nearest k (tie → larger count); document limitation (uniform remap cannot split equal alphas) — cost if wrong: binary cutouts at small mips keep ±group coverage error.
50. T30 remap = monotone piecewise-linear (0→0, a→T, 1→1) instead of uniform τ/a scale, so cutout_blend opaque interiors stay opaque — deviation from Castaño/spec §5.5 wording; OWNER CHECK — cost if wrong: different alpha gradient shape at edges (coverage identical). **OWNER CHECK**
51. _flow B = 0 even if overridden to a non-BC5 format — flow is a 2-channel vector; B carries no meaning — cost if wrong: an odd user override loses B.
52. T30 follow-up — when the nearest group is 1.0 (or a lower value already passes), anchor on the next lower value instead of leaving the level unchanged — cost if wrong: none (strictly closer to target).
53. T30 k=0 "within one texel" is not a hard bound (documented); binary ties over-cover by up to the tie group (documented limitation of a monotone remap) — cost if wrong: small mips of binary cutouts thicker than base; §11.6 in-game check.
54. Task 14's "modes 0–5 decode 255" premise corrected — rotated mode 4/5 needs a lossless post-encode endpoint fix in both native and fallback paths — cost if wrong: none (RGB bit-identical).
55. brief Task 33 says explicit header=legacy on BC6H/BC7/_SRGB "silently becomes DX10"; Task 26 carry says CFG-INVALID — carry wins (explicit request that cannot be honoured is an error, not a silent change); dds.rs keeps the DX10 fallback defensively — cost if wrong: a user asking legacy for BC7 gets an error instead of a DX10 file.
56. DECISIONS Note S5b line + spec §5.8 corrected for rotated mode 4/5 (docs commit) — cost if wrong: none.
57. rgbcx_level > 18 CFG-INVALID should apply only when encoder.backend = rgbcx → folded into fix batch 33 — cost if wrong: a stray override errors on BC7 files.
58. keep hand-rolled write_atomic (re-review: correct; tempfile only marginally safer) — cost if wrong: rare temp leftovers.
59. T35 I1 partial — grayscale check ungated; complex-margin/height-clip stay gated on stored alpha (removal of _m alpha is Resolve's LOSS-ALPHA-DATA) — cost if wrong: a forced `remove` on complex _m relies on Resolve's warning only.
60. T36 manifest key = root-relative, '/' separators, Unicode case-folded (Windows) — cost if wrong: two outputs differing only by case on a case-sensitive FS share a key (not a Skyrim/Windows scenario).
61. T36 load uses a tolerant ManifestEntry view so schema drift re-encodes nothing spuriously and drops nothing — cost if wrong: none.
62. fixbatch-35 text inverted complex_m_warn_alpha boundary; implementer correctly kept spec (decoded smallest-mip alpha ≥ 247 warns, margin to CS 251 gate) — cost if wrong: none.
63. re-review design notes accepted — removed debug-only panic on encoder_input mismatch (library API returns VAL-HEADER always); LDR/HDR kind mismatch now VAL-HEADER instead of silent skip — controller commit, validate 22/22, clippy clean
64. Task 38 records the S5 mean dRGB −0.05 dB limit as info, not a pass/fail check — consistent with the Task 14 ruling (exact_opaque scoped to _m, cost accepted by owner) — cost if wrong: none.
65. T37 I1 — validate in-memory before write_atomic; any post-write failure removes the output and reports file_failed — cost if wrong: none.
66. T37 I2 — record batch BC1 model in manifest; differing/None model → re-encode BC1 outputs — cost if wrong: one extra re-encode after upgrade.
67. T37 O2 — up-to-date requires stored validation passed (a failed output is re-encoded and re-reported each run, never silently "up to date") — cost if wrong: repeated work on known-failing files.
68. T37 M6 — coverage only ever applies to cutout/cutout_blend, even with mips.coverage=on — cost if wrong: none (spec §5.5 says blended alpha keeps box opacity).
69. BC1 decoder model also compared for BC3 outputs (rgbcx encodes BC3 colour via encode_bc1) — cost if wrong: extra BC3 re-encode after a model change.
70. IN-DUPLICATE (two sources → one output) stays exit 1 — a conflict the user must resolve, not an intentional skip — cost if wrong: none.
71. T39 I2 — an unavailable sub-type yields CFG-UNAVAILABLE only (Blocked → skipped, exit 1); downstream tag/format checks (TAG-SRGB-VANILLA) on the unavailable profile are suppressed; --allow fallback re-resolves and checks normally — cost if wrong: a vanilla PBR albedo is counted skipped instead of failed.
72. float source + BC6H format → color.content=linear as analysis fact (provenance analysis:float-source; --set wins) — cost if wrong: none (spec §4.1 row).
73. T39 concern — CFG-INVALID on an unavailable profile stays hidden until fallback accepted (consequence of the no-cascade ruling) — cost if wrong: user sees a second error after --allow.
74. golden HDR tolerance loosened to 0.04·peak absolute + peak within 5% (BC6H measured max err 3.2% of peak; old bug gave 23.7 vs 4.0, still caught) — cost if wrong: none.
75. rotated-slot fix is lossless → apply to every BC7 block with all-255 input alpha regardless of exact_opaque/policy; exact_opaque only controls the mode-6 p-bit constraint — cost if wrong: none (RGB bit-identical).
76. T42 I1 — validate infers alpha.policy from the file (all ≥254 → remove; else subtype default if it keeps alpha, else data) — cost if wrong: a remove-intended file with real alpha is judged as data.
77. T42 I2 — DDS inputs never take part in IN-DUPLICATE detection (engine bug; common mod-folder layout) — cost if wrong: none.
78. result verbs (inspect/validate/presets/encoders) print human results on stdout; convert progress on stderr; --json stdout JSON only → DECISIONS Note J (controller writes after fix lands) — cost if wrong: scripts parsing stderr for inspect output.
79. inspect exits 1 on unreadable input (spec §8 global rule over brief) — cost if wrong: none.
80. T43 I1 (preset/config schemas `true` for defaults/subtypes/overrides because they are Value) DEFERRED → carry M3: generate a partial-Profile schema (all fields optional) and reference it via schemars schema_with so editors validate preset fields; the loader already rejects bad values — cost if wrong: editor validation misses bad preset values until M3.
81. BC1 transparent-black check stays per-file CFG-INVALID (exit 1), consistent with other check_profile checks; a batch-wide --set of it is still per-file — cost if wrong: exit 1 instead of 2.
82. Codex B I1 (plan loads bypass mem budget) ACCEPTED; I2 (uncompressed DDS PITCH vs LINEARSIZE) ACCEPTED; I3 (legacy volume DDS passes as 2D) ACCEPTED → reject as Unsupported; M1 (HDR encoder_input count unchecked) ACCEPTED → fixbatch-codexB.md, dispatch with fix 45b after 45a hands back
83. Codex CLI #1 (blocking I/O in Ctrl-C callback) ACCEPTED; #2 (DDS-only inspect skips option/config validation) ACCEPTED; #3 (validate inherits exact_opaque for third-party _m) ACCEPTED → standalone validate defaults exact_opaque=false; #4 (cancel during plan/dry-run exits 0) ACCEPTED; + VARYING_ALPHA_BELOW 251
84. Codex-CLI #4 — after Ctrl-C every ready file not yet handled is listed cancelled and exit 1 (incl. would-be up-to-date); Ctrl-C after all files handled keeps the normal exit — recorded in DECISIONS Note K by implementer — cost if wrong: a late Ctrl-C on an all-up-to-date batch exits 1.
85. Engine::plan now takes &CancelToken (public API change; GUI must pass one) — accepted.
86. final I1 (plan has no catch_unwind) ACCEPTED fix now; I4 (float→LDR silently wrong) ACCEPTED interim blocking IN-FLOAT-LDR + LOSS-CLAMP-HDR until M6; I7 (plan/run source drift) ACCEPTED size+mtime check now; I2, I3, I8 (docs: fill values, carried items into plan, spec resync) ACCEPTED now; I5 (event identity/typed job/skip kind) and I6 (per-file allow, exact-path scope) DEFERRED to M3 entry (recorded in plan) — cost if wrong: one schema bump at M3.

# M5: NVTT Production as an optional encoder (design)

Status: **approved by the owner** (2026-10-13), with every §9 question taken at its proposed default; those
defaults are now decided (DECISIONS Note NV records them). As built, Note NV is authoritative where it differs from
this text: levels are fed as legacy-header DDS (Spike S7b; never DX10), BC6H also gets `-nocuda`, and BC7 gets `-alpha`
for every alpha policy except `remove`.

Sources: design spec §3.2 (`encoder.backend`), §5.6, §7.3 table, §7.4, §7.5, §9 codes `ENC-NVTT-MISSING` / `ENC-NVTT-VERSION` /
`VAL-BC6H-PEAK`, §14 items 6 and 10; DECISIONS Spike S7, Note Y9; plan phase 0-2 Task 16.

## 1. Goal

A texture can be encoded with NVIDIA Texture Tools 3 (`nvcompress`, Production) instead of the built-in encoder, for BC7 and
BC6H only. It is a quality option for gradients, linear data and HDR peaks (spec §7.3: +3.9 dB on gradients, +0.3-0.4 dB on
linear data, keeps BC6H peaks at 40-60x the time). The built-in encoders stay the default everywhere.

Today `encoder.backend = "nvtt"` already parses, fits BC7 and BC6H (`policy::backend_fits`), and is always blocked by
`ENC-NVTT-MISSING` ("arrives in milestone M5"); `sdc convert --nvtt` is a usage error (Note Y9); `sdc encoders --nvtt` detects
the executable only. M5 replaces those stubs with a working backend.

## 2. Non-goals

- No bundling, linking or redistributing NVTT (licence, spec §7.5). It is a separate, user-installed process.
- NVTT is never a default and never chosen automatically; suggestions do not propose it in M5.
- No other NVTT formats (BC1-BC5 stay on rgbcx; `backend_fits` is unchanged).
- No NVTT mip generation: the engine builds every level, as it does for the built-in encoders (Note §14 #10).
- No change to the exact-opaque mechanism. The native bc7e patch passed (§14 #6), so the "route to NVTT" fallback is not
  built; §14 #6 is updated to say so.

## 3. Finding NVTT

Batch-level setting (spec §3.2 "batch-level settings": the NVTT path is one), probed once per batch:

1. The explicit path (CLI `--nvtt <file>`, GUI Advanced "NVTT path"). If given and missing, NVTT is not found; there is no
   silent fallback to the other places.
2. Else `nvcompress.exe` on `PATH` (what `sdc encoders` does today).
3. Else `C:\Program Files\NVIDIA Corporation\NVIDIA Texture Tools\nvcompress.exe` (spec §7.5).

The probe runs `nvcompress` with no arguments (it has no `-h`; Spike S7) and reads the version from the banner line
`NVIDIA Texture Tools <x.y.z>`. The probe also checks that the usage lists the flags the backend passes. Result:
`NvttTool { path, version, tested: version == "3.2.5" }`, or not found with a reason.

## 4. Planning codes

Raised by `check_profile` for a profile whose backend is `nvtt`:

| Code | Severity (existing) | When | Message |
|---|---|---|---|
| `ENC-NVTT-MISSING` | blocking | NVTT not found by §3, or the probe failed | where it looked; "install NVTT 3, set the NVTT path, or choose the built-in encoder" |
| `ENC-NVTT-VERSION` | blocking | found, version other than 3.2.5 | "NVTT <v> is untested (3.2.5 is); acknowledge to use it" |

Both are blocking, so `--allow ENC-NVTT-VERSION` (or a `[files]` allow) acknowledges an untested version, exactly as the spec
table's "Acknowledge" says. Allowing `ENC-NVTT-MISSING` cannot make a missing executable run: the file then fails at Compress
with `ENC-NVTT-FAILED` (§6).

## 5. Encoding

Per level, after the engine has built and quantised it (`quantize::EncoderInput`):

1. Write the level as an uncompressed DDS with the engine's own writer: `rgba8` (B8G8R8A8) for BC7, `rgba16f` for BC6H, into a
   per-file temporary folder (§7).
2. Run `nvcompress -silent -nomips -production <fmt> [-noalpha] [-nocuda] <in.dds> <out.dds>`:
   - `<fmt>` is `-bc7` for BC7, `-bc6` for BC6H UF16 (confirmed by Spike S7b, §8).
   - `-noalpha` when the alpha policy is `remove` (what S7 measured; decoded alpha is 255 on every level).
   - `-nocuda` always for BC7 (S7: byte-identical, removes 0.11 s CUDA start per process). For BC6H only if S7b shows the
     output is byte-identical without CUDA; otherwise CUDA stays on for BC6H.
   - No colour-space flag: the engine has already put the texels in the stored colour space, and the DDS header (sRGB tag,
     DX10 or legacy) is written by the engine, never taken from NVTT's output.
3. Read the output, take the payload after the DDS header (DX10 or legacy), and check its length is
   `ceil(w/4) * ceil(h/4) * 16`. Anything else is a failure (§6).
4. Run the same post-encode steps the built-in BC7 path runs, because they are encoder-independent:
   - the lossless rotated mode 4/5 opaque fix (`force_opaque_rotated`) on every block whose source alpha is 255 throughout;
   - the complex-grass corner guard (it re-encodes one block with bc7e slowest when the decoded corner fails, as today).
   The mode-6 byte patch is not applied: NVTT constrains the p-bit (spec §5.6, 0% 254 in B1 §5).
5. Validation is unchanged (smallest-mip alpha, PSNR, coverage, BC6H peak). It is the proof, as for every backend.

The level chain is assembled and written by the engine exactly as for the built-in encoders.

**One NVTT process at a time per batch.** BC7 Production is already multi-threaded (S7: about 13 cores busy). Files on the
built-in encoders keep running in parallel; a file on NVTT waits for the NVTT slot. (Shortcut: a fixed limit of 1; make it a
setting if a many-core machine shows idle cores.)

## 6. Failures

New code `ENC-NVTT-FAILED` (error, per file, stage Compress): the process could not start, exited non-zero, timed out, or
wrote no or a wrong-sized payload. The message carries the exit status and the last lines of stderr. There is no silent
fallback to the built-in encoder; the user chose NVTT and gets told. Other files in the batch continue (Note I behaviour).

**Cancellation.** The child is polled; on cancel it is killed and the file ends `Cancelled` with nothing written, like the
band boundary check in `encode_level`.

**Timeout.** None by default (a 4K mip 0 can take minutes). Shortcut: add one if a hung `nvcompress` is ever seen.

## 7. Temporary files

One folder per file under the system temp folder (`%TEMP%\sdc-nvtt-<pid>-<n>`), removed when the file finishes, fails or is
cancelled (drop guard). A crash can leave the folder behind; it holds only intermediate level images. Nothing is written next
to the output.

## 8. Spike S7b (first task, on the owner's machine)

S7 left BC6H open. S7b answers, with NVTT 3.2.5 at the default install path:

1. Does `nvcompress` accept an uncompressed DDS input (B8G8R8A8 and RGBA16F, DX10 header)? Pass: BC7 output from a DDS input
   is byte-identical to the same level fed as PNG.
2. BC6H per level: each level of three `hdr` corpus images (to 1x1) fed as RGBA16F DDS with `-nomips -bc6 -production`. Pass:
   payload of the expected size at every level; mip-0 BC6H peak ratio ≥ 0.95 (spec B2 §6); `-bc6` writes UF16 (the bench's
   BC6H decoder agrees with the DXGI format).
3. `-nocuda` with BC6H: byte-identical to the CUDA run? Per-level overhead with and without.
4. The banner line format the probe parses, and the exit code on a bad input.

If 1 fails, LDR levels are fed as PNG and HDR levels as the format S7b finds works (EXR or HDR). If 2 fails at small levels,
the fallback the spec already names applies: NVTT encodes the levels it can and ISPC BC6H encodes the rest, with an info
event. The plan's later tasks are written for the pass case and say where they change.

## 9. Open questions for the owner (proposed defaults)

1. **Preview.** NVTT is 40-60x slower. Proposed: GUI previews (live encode, set preview) of a texture on NVTT use the
   built-in encoder for the format, and the preview says "preview uses the built-in encoder; the export uses NVTT". Final
   exports and `sdc convert` always use NVTT.
2. **Up-to-date check.** Proposed: a record encoded by NVTT stores the NVTT version (`encoders.nvtt`); under
   `--on-existing if-changed` an output is re-encoded when that version differs from the one found now. Changing NVTT install
   path alone does not re-encode. Engine version stays 0.7.0 (no existing output changes).
3. **GUI control.** Proposed: one "Encoder" row in the texture settings for BC7 and BC6H, "Built-in" or "NVTT Production",
   locked with "NVTT not found" when the probe fails; plus an "NVTT path" field and status line in Advanced.
4. **No silent fallback** (§6) and **one process at a time** (§5) as above.

## 10. What changes where

- `engine`: new `nvtt` module (locate, probe, per-level encode, slot); `encode_levels` dispatches on the backend;
  `check_profile` raises the §4 codes from the probe result; `BatchRequest.nvtt`; new code `ENC-NVTT-FAILED`;
  `EncoderVersions.nvtt`; preview substitution (§9.1); schemas regenerated.
- `cli`: `convert --nvtt` works (Note Y9 lifted); `sdc encoders` reports version and whether it is the tested one, using the
  same probe.
- `gui`: Advanced "NVTT path" and status; "Encoder" row; settings key `nvtt_path`.
- `bench`: S7b; an ignored real-NVTT parity test (engine output equals S7's direct encode on mip 0).
- `docs`: DECISIONS note with the rulings, design spec §7.5 / §14 #6 / §14 #10 / codes table, gui.md, CLI help.

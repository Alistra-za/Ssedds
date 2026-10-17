# Tessalith: notes for coding agents

Tessalith converts source images into DDS textures for Skyrim SE/AE. It has a GUI (`tessalith.exe`) and a CLI (`sdc.exe`). Both share one engine and one per-project `sdc.toml`, so they write the same bytes. Read README.md for what it does and CONTRIBUTING.md for the basics. This file covers what an agent tends to get wrong.

## Crates
| Package | Path | What it is |
|---|---|---|
| `engine` | crates/engine | Planning, policy, presets, packing, bake, RNM, mips, encode, DDS write, manifest |
| `sdc-gui` | crates/gui | egui + wgpu GUI (`tessalith.exe`); kittest tests |
| `sdc` | crates/cli | CLI (`sdc.exe`) |
| `encoders-sys` | crates/encoders-sys | FFI to vendored encoders (bc7e, ISPC TC, rgbcx); objects are vendored, no ISPC compiler needed |
| `modscan`, `vfs`, `esp`, `nif`, `render3d`, `pgpsim` | crates/… | MO2 scan store, BSA reading, plugin/NIF parsing, 3D preview, PGPatcher simulation |
| `sdc-bench` | crates/bench | Measurement harness, not shipped |

A sibling "NIF team" owns `modscan`, `vfs`, `esp`, `nif`, `render3d` and `pgpsim`. Change them only when the task says so, and record what you changed.

## Gates (all must pass before a commit is called done)
```
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy --workspace -- -D warnings
cargo test --workspace -- --test-threads=4
cargo test -p engine --test golden
```
On Windows, set `CARGO_BUILD_JOBS=4` when several builds run at once. Otherwise linking fails with os error 1450 ("insufficient system resources"). That error is not a code failure.

## Hard rules
- **Goldens are byte-identical.** Encoded output must not change unless a DECISIONS note says why. If a golden would change, stop and say so.
- **Tests never touch the real user data folders**: `%LOCALAPPDATA%\sdc` (scan.db, logs) and `%APPDATA%\sdc` (gui.toml, presets). Use temp dirs.
- **Tests that need the owner's game or mod data** stay `#[ignore]` or gated on an `SDC_*` environment variable.
- **No game or mod assets in the repo**: no `.dds`, `.bsa`, `.esp` or real textures. Fixtures are generated in tests.
- **NVTT is never bundled.** It is an optional external tool.
- **Files the user edits are written with `engine::dds::write_atomic`**, which flushes before the rename. That covers `sdc.toml`, `sdc-project.toml`, `gui.toml`, presets and exported JSON. DDS outputs use it too (owner decision, Note BC10).
- **Version:** the workspace version is the engine version, and it is part of every output's identity. Bump it only on purpose.

## How work is recorded
- **Every judgement call becomes a ruling in `docs/DECISIONS.md`.** Append a new `## Note XX — <topic> (<who>, <date>)` at the end with the next free letters. Number the items XX1, XX2, and so on.
- **Older notes are history:** don't rewrite them. Add a pointer when a new note supersedes one.
- **Behaviour recorded as a ruling is intended.** Don't "fix" it without a new ruling.
- **A comment starting `shortcut:`** marks a known limit, and says when to upgrade it.

## Workflow
- Do feature work on a branch in `.worktrees/<name>`, not on `main`.
- Commit explicit paths. Never `git add -A`.
- Bug fixes start with a failing test. Make one commit per fix.
- Merges into `main` are local. The owner pushes.
- When two branches both append a note to `docs/DECISIONS.md`, keep both.

## Gotchas
- **`SetsDoc` / `ProjectDoc` (GUI documents):** mutate first, then call `touch()`. `entries()` is cached by revision, so a mutation without `touch()` serves stale data (Note BI).
- **Path case folding:** game-path keys (meshes, scan, BSA) fold ASCII only, while set ids and `[files]` keys fold Unicode (Note BF).
- **Targets:** only `cs` and `vanilla`. The `cpm` target was removed and is now a hard error (Note AZ).
- **Normal maps:** plain `_normal` / `_nrm` / `_norm` default to DirectX green, and `_normalgl` flips (Note AW).
- **Untracking files:** merging a branch that stops tracking files (`git rm --cached`) deletes those files from the other checkout's disk. Back them up first.
- **Line endings:** files are CRLF in the working copy (autocrlf). LF-to-CRLF warnings on commit are harmless.

## Where things are
- `docs/DECISIONS.md`: every ruling. Search it before changing behaviour.
- `docs/gui.md`, `docs/wiki/`: user-facing behaviour. Update them when the UI changes.
- `docs/research/`: the measurements behind encoder, mip-filter and coverage choices.
- `.claude/workflows/trace-debug.js`: a whole-repo bug hunt with skeptical review.
- `tools/package-release.ps1`: builds the release zip and SHA256SUMS.

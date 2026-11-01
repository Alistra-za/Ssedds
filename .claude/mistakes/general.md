# Mistakes: Machine, shell, git and build mistakes that are not tied to one section

Format and rules: [README.md](README.md).

- 2026-11-01 The Bash tool returned no output on this Windows machine → use the PowerShell tool (Bash tool)
- 2026-11-01 `python -` / `python3 -` heredocs hung the shell → write a script file and run it with `py -3.13 -I <script>` (any shell)
- 2026-11-01 Read on `.claude/DECISIONS.md` failed: the file is too large → Grep it, never Read it whole (`.claude/DECISIONS.md`)
- 2026-11-01 Shell rewrites (.NET ReadAllText/WriteAllText, sed) mangled CRLF files → use the Edit tool (any CRLF file)
- 2026-11-01 Files are CRLF in the working copy (autocrlf); LF-to-CRLF warnings on commit are harmless → never mass-normalise line endings (git)
- 2026-11-01 Merging a branch that stops tracking files (`git rm --cached`) deleted those files from the other checkout's disk → back them up first (git merge)
- 2026-11-01 A shared CARGO_TARGET_DIR on C: grew to 432 GB and filled the drive → check its size and put it on F: (`CARGO_TARGET_DIR`)
- 2026-11-01 `cargo clean` missed the real build folder → pass `--target x86_64-pc-windows-msvc` too (`cargo clean`)
- 2026-11-01 A `///` doc-comment change on a `JsonSchema` field left `schema/*.json` stale and failed `committed_schemas_match_the_types` in the full gate → after editing such a comment run `sdc presets schema all --out schema` and commit the diff (`crates/engine/tests/schemas.rs`)

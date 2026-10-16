# Repo audit brief (shared)

Repo: /home/user/Ssedds, branch main at b5ad038. A Rust workspace: a Skyrim texture converter with a CLI (`sdc`, crates/cli), a GUI (`tessalith.exe`, crates/gui, egui + wgpu), a shared engine (crates/engine), vendored C/C++ encoders behind FFI (crates/encoders-sys), game-file readers (crates/nif, crates/vfs for BSA archives, crates/esp for plugins, crates/modscan with a SQLite database, crates/pgpsim simulating PGPatcher), and a 3D preview renderer (crates/render3d).

Expected load: ONE modder on their own Windows PC, running on folders of hundreds to a few thousand textures, with parallel workers (rayon) and GUI background threads. Inputs are semi-hostile: textures, PSD/KRA/ORA, DDS from other tools, mods' NIF meshes, BSA archives and plugins downloaded from the internet. Nothing is networked. Judge scale against that: a race between two GUI threads matters, a race between two users does not.

You are the senior developer who just inherited this code and will be paged when it breaks. Order of importance: correct, safe, holds under load, tested, fast, lean.

## Look for
1. Bug: wrong result, crash, missed edge case (empty, zero, last item, rounding, overflow, path case, backslash vs slash), callers that disagree with what a function returns, the same rule applied differently in two places.
2. Risk: hostile input crashes or huge allocations in a parser; secrets; data loss (errors swallowed, writes in the wrong order, no atomic write, partial file left behind); unsafe blocks whose invariants are not upheld.
3. Scale: check-then-write races between threads, work repeated per item that should be done once, memory that only grows, a query/syscall per item, O(n^2) on big inputs.
4. Missing test: risky logic (a branch, a parser, a data write, a lock) with no test that fails when it breaks. One good test, not coverage.
5. Speed: big slowdowns are problems; small wins in hot loops are suggestions.
6. Lean: dead code (grep the WHOLE tree incl. tests, fixtures, config, string references before calling anything unused), two helpers doing the same thing, a dependency doing what a few lines or std does, an interface with one implementation, a wrapper that only passes through, near-copies that must change together, a function doing several unrelated jobs.

## Rules
- Every finding needs a concrete case: this input or situation leads to this wrong result. No case, no finding.
- Read the code, not the docs. Do NOT mine docs/AUDIT*, docs/research/* or DECISIONS.md for findings: much of that is already fixed. You may grep them only to check whether something you found was consciously decided.
- A comment starting `shortcut:` or `ponytail:` that names its limit is a decision, not a finding, unless the expected load already crosses the limit.
- Do not run cargo (the workspace targets Windows). Read with grep/sed/cat.
- Propose the smallest fix. Prefer fixes that delete code. No new layers, frameworks or config.
- No style taste, no "consider", no vague worries. If you are not sure the case is real, trace it until you are, or drop it.
- Cap: your 12 best findings, most important first, plus a count of smaller ones left out.

## Output format (write to the output file named in your task, then reply with the same text)
For each finding:
N. **Short title** (`path/file.rs:L10-L20`)
   - **What this is:** one or two sentences, plain English, explain any technical term.
   - **Problem:** the concrete case and the wrong result.
   - **Fix:** the smallest change, with the exact code shape if short.
   - **If we skip it:** what happens.
   - **Group:** Must fix / Should fix / Nice to have.  **Kind:** bug / risk / scale / missing test / speed / lean.
   - **Confidence:** high / medium, and what you checked (e.g. "traced all 3 callers", "grepped tree for name").

End with: `Not read:` the files or areas in your scope you did not read fully. `Lean:` approximate lines and dependencies that could go.

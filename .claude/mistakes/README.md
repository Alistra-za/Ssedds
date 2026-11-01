# Mistakes log

One file per section of the section table in `.claude/workflows/trace-debug.js` (the same table `quality-audit.js` uses), plus `general.md` for machine, shell, git and build mistakes.

- **Before you start:** read the file for your section, and `general.md`.
- **When something goes wrong** that the next agent would hit too, append one line to that file.
- **Entry format:** `- YYYY-MM-DD <what went wrong> → <what to do instead> (<where: file or command>)`.
- A rule the owner decided goes in `.claude/DECISIONS.md`, not here.

export const meta = {
  name: 'merge-main',
  description: 'Merge a reviewed branch into main: dry-run, merge with note-letter checks, independent remerge-diff review, split gate, release build',
  whenToUse: 'After a branch passed pre-merge review and the owner said merge. Args: {branch: "fix/x" (required), into: "main", repo: "<main checkout path>", build: true}',
  phases: [
    { title: 'Preflight', detail: 'disk, clean tree, git merge-tree dry run' },
    { title: 'Merge', detail: 'merge --no-ff, keep both DECISIONS notes, rename colliding note letters' },
    { title: 'Check', detail: 'read-only remerge-diff review of the merge' },
    { title: 'Gate', detail: 'fmt, clippy x2, split test run, goldens, release exes' },
  ],
}

const a = args || {}
if (!a.branch) throw new Error('merge-main needs args.branch')
const INTO = a.into || 'main'
const REPO = a.repo || 'the current working directory (the main checkout)'
const BUILD = a.build !== false
const SHELL = `Never run \`python -\` heredocs. Never touch %LOCALAPPDATA%\\sdc or the owner's game and mod folders. Never push. Commit explicit paths only. Work in ${REPO}.`

phase('Preflight')
const pre = await agent(
  `Preflight for merging ${a.branch} into ${INTO}. ${SHELL} Read-only in this step.
1. Free space on the drive holding ${REPO}'s target/ (df). Report GB free.
2. \`git status --porcelain\` on ${INTO}: list anything tracked that is modified (untracked files are fine).
3. \`git merge-tree --write-tree ${INTO} ${a.branch}\` (dry run): list conflicted paths.
4. Whether the branch adds a "## Note XX" heading whose letters already exist on ${INTO} (grep both).`,
  { label: 'preflight', phase: 'Preflight', schema: {
    type: 'object',
    properties: {
      free_gb: { type: 'number' },
      dirty: { type: 'array', items: { type: 'string' } },
      conflicts: { type: 'array', items: { type: 'string' } },
      note_collisions: { type: 'array', items: { type: 'string' } },
    },
    required: ['free_gb', 'dirty', 'conflicts', 'note_collisions'],
  } })
if (!pre) throw new Error('preflight agent returned nothing')
if (pre.dirty.length) return { stopped: 'main has uncommitted tracked changes', dirty: pre.dirty }
if (pre.free_gb < 20) return { stopped: `only ${pre.free_gb} GB free; clear build folders first` }
const codeConflicts = pre.conflicts.filter(p => !/docs\/DECISIONS\.md$|\.gitignore$/.test(p))
if (codeConflicts.length) return { stopped: 'code conflicts need a human or a fix branch', conflicts: codeConflicts }
log(`Preflight ok: ${pre.free_gb} GB free; conflicts: ${pre.conflicts.join(', ') || 'none'}; note collisions: ${pre.note_collisions.join(', ') || 'none'}`)

phase('Merge')
const merged = await agent(
  `Merge ${a.branch} into ${INTO} with \`git merge --no-ff ${a.branch}\` (message: "Merge ${a.branch}: <one-line summary of the branch>" plus a blank line and \`Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>\`). ${SHELL}
Conflicts allowed here: docs/DECISIONS.md and .gitignore only, where both sides append at the end — keep both sides, the branch's block after ${INTO}'s. Preserve CRLF line endings.
Note letters: if the branch's note letters collide with ones already on ${INTO} (${pre.note_collisions.join(', ') || 'none expected'}), rename the branch's note to the next free letters and update its item ids (XX1, XX2…) and any references to them in the branch's own changes.
Then check \`grep -oE '^## Note [A-Z]+' docs/DECISIONS.md | sort | uniq -d\` prints nothing.
If any other conflict appears, run \`git merge --abort\` and report. Return the merge commit hash or the reason you stopped.`,
  { label: 'merge', phase: 'Merge', schema: {
    type: 'object',
    properties: { merged: { type: 'boolean' }, commit: { type: 'string' }, renamed_notes: { type: 'string' }, reason: { type: 'string' } },
    required: ['merged'],
  } })
if (!merged || !merged.merged) return { stopped: 'merge did not complete', detail: merged }

phase('Check')
const check = await agent(
  `Independent check of merge commit ${merged.commit} in ${REPO}. Read-only: do not edit or commit. ${SHELL}
Run \`git show --remerge-diff ${merged.commit}\` and review every conflict resolution: nothing from either side dropped, no duplicated block, no conflict markers left anywhere in the tree (\`git grep -n '^<<<<<<<\\|^>>>>>>>'\`), no duplicate DECISIONS note headings. Default to "bad" if unsure.`,
  { label: 'remerge-check', phase: 'Check', schema: {
    type: 'object',
    properties: { ok: { type: 'boolean' }, problems: { type: 'array', items: { type: 'string' } } },
    required: ['ok', 'problems'],
  } })
if (!check || !check.ok) return { stopped: 'remerge-diff check failed; the merge commit is local and can be reset by the owner', commit: merged.commit, problems: check && check.problems }

phase('Gate')
const gate = await agent(
  `Run the gates on ${INTO} at ${merged.commit} in ${REPO}. ${SHELL} Use CARGO_BUILD_JOBS=4 and the main checkout's own target dir. Capture each command's output to a file in the session scratchpad and read the \`test result:\` lines; never pipe through head.
1. cargo fmt --all --check
2. cargo clippy --offline --workspace --all-targets -- -D warnings
3. cargo clippy --offline --workspace -- -D warnings
4. cargo test --offline --workspace --no-fail-fast -- --skip render_ --test-threads=4
5. cargo test --offline --workspace -- render_ --test-threads=1
6. cargo test --offline -p engine --test golden
${BUILD ? '7. cargo build --offline --release -p sdc-gui -p sdc (exes land in target\\x86_64-pc-windows-msvc\\release\\; if the exe is locked because Tessalith is running, report that rather than failing).' : ''}
Report pass/fail per step with counts; for a failure, the test name and the first error lines.`,
  { label: 'gate', phase: 'Gate', schema: {
    type: 'object',
    properties: {
      passed: { type: 'boolean' },
      tests_passed: { type: 'integer' },
      tests_failed: { type: 'integer' },
      failures: { type: 'array', items: { type: 'string' } },
      exe_built: { type: 'boolean' },
    },
    required: ['passed', 'failures'],
  } })

return { branch: a.branch, commit: merged.commit, renamed_notes: merged.renamed_notes || 'none', gate }

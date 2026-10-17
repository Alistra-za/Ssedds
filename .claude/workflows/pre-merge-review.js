export const meta = {
  name: 'pre-merge-review',
  description: 'Whole-branch pre-merge review: package the diff, one skeptical reviewer, one fix wave in the branch worktree, scoped re-review',
  whenToUse: 'Before merging a finished branch. Args: {branch: "fix/x", worktree: ".worktrees/x", base: "<base commit>", requirements: "<path to brief/plan>" (required), focus: "<extra review questions>", fix: true}',
  phases: [
    { title: 'Package', detail: 'diff file: log + stat + -U8 diff against the base' },
    { title: 'Review', detail: 'read-only reviewer, verdicts + Critical/Important/Minor' },
    { title: 'Fix', detail: 'one fix wave for Critical/Important in the worktree' },
    { title: 'Re-review', detail: 'scoped re-review of the fix commits' },
  ],
}

const a = args || {}
for (const k of ['branch', 'worktree', 'base', 'requirements']) if (!a[k]) throw new Error(`pre-merge-review needs args.${k}`)
const FIX = a.fix !== false
const PKG = `.superpowers/sdd/reviews/${a.branch.replace(/[^A-Za-z0-9._-]+/g, '-')}`
const SHELL = `Never run \`python -\` heredocs. Never touch %LOCALAPPDATA%\\sdc or the owner's game and mod folders. Never push. If you run cargo: --offline, CARGO_BUILD_JOBS=4, the worktree's own target dir only, \`-- --skip render_ --test-threads=4\`, output captured to a file.`

const FINDINGS = {
  type: 'object',
  properties: {
    spec_ok: { type: 'boolean' },
    quality_ok: { type: 'boolean' },
    findings: { type: 'array', items: { type: 'object', properties: {
      severity: { type: 'string', enum: ['critical', 'important', 'minor'] },
      where: { type: 'string' }, problem: { type: 'string' }, failing_case: { type: 'string' }, fix: { type: 'string' },
    }, required: ['severity', 'where', 'problem', 'failing_case'] } },
  },
  required: ['spec_ok', 'quality_ok', 'findings'],
}

phase('Package')
const pkg = await agent(
  `In the main checkout, write the review package for ${a.branch}: \`git log --oneline ${a.base}..${a.branch}\`, \`git diff --stat ${a.base}..${a.branch}\` and \`git diff -U8 ${a.base}..${a.branch}\` into ${PKG}/diff.txt (create the folder; .superpowers is gitignored). ${SHELL} Return the file path and its line count.`,
  { label: 'package', phase: 'Package', effort: 'low' })

phase('Review')
const review = await agent(
  `Read-only pre-merge review of branch ${a.branch} (worktree ${a.worktree}, base ${a.base}). Do NOT edit or commit.
Requirements: ${a.requirements}. Diff package: ${pkg}. Rulings: docs/DECISIONS.md (the branch's new note).
Check spec compliance and quality: correctness on edge cases, no lost user data, goldens untouched unless a ruling says why, every caller of a changed function, tests that would fail without the fix (not weakened), docs/DECISIONS updated. ${a.focus ? 'Owner/lead focus: ' + a.focus : ''}
Each finding needs a concrete failing case. Minor = cosmetic or test gaps. ${SHELL}
Write the full review to ${PKG}/review.md.`,
  { label: 'review', phase: 'Review', schema: FINDINGS })
if (!review) throw new Error('reviewer returned nothing')
const blocking = review.findings.filter(f => f.severity !== 'minor')
log(`Review: spec ${review.spec_ok ? 'ok' : 'NOT ok'}, quality ${review.quality_ok ? 'ok' : 'NOT ok'}, ${blocking.length} blocking, ${review.findings.length - blocking.length} minor`)
if (!blocking.length || !FIX) return { branch: a.branch, ready: !blocking.length && review.spec_ok, review: `${PKG}/review.md`, findings: review.findings }

phase('Fix')
const fix = await agent(
  `Fix wave on branch ${a.branch} in worktree ${a.worktree}. Fix exactly these review findings (full review: ${PKG}/review.md):
${JSON.stringify(blocking, null, 1)}
For each: a failing test first, then the smallest correct fix, one commit each (explicit paths; message ends with a blank line and \`Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>\`). Add the rulings to the branch's DECISIONS note. Do not touch anything else. ${SHELL}
Then run fmt, both clippy runs and the affected test binaries. Return the new commit hashes and test counts.`,
  { label: 'fix', phase: 'Fix' })

phase('Re-review')
const re = await agent(
  `Scoped re-review, read-only. Branch ${a.branch} in ${a.worktree}. The fix wave after the review in ${PKG}/review.md reported: ${fix}
Check only that each blocking finding below is really fixed (failing case now passes, test would fail without the fix) and that the fix introduced nothing new: ${JSON.stringify(blocking.map(f => f.where + ': ' + f.problem))}. ${SHELL}`,
  { label: 're-review', phase: 'Re-review', schema: FINDINGS })

const still = re ? re.findings.filter(f => f.severity !== 'minor') : blocking
return { branch: a.branch, ready: still.length === 0, review: `${PKG}/review.md`, open: still, minor: review.findings.filter(f => f.severity === 'minor') }

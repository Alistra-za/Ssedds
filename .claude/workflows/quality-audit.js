export const meta = {
  name: 'quality-audit',
  description: 'Whole-repo quality audit: dead code, inefficiency, duplication, architecture; each finding skeptically checked, then a ranked report with a delete list and refactor briefs',
  whenToUse: 'Periodic quality pass over Tessalith (not a bug hunt: use trace-debug for bugs). Args (all optional): {date: "YYYY-MM-DD", repo: "<checkout path>", sections: ["E1",...], sweeps: ["arch",...], maxReviews: 100, briefs: 12, out: "<report dir>"}',
  phases: [
    { title: 'Scout', detail: 'map every crates/*/src file to a section; gather mechanical signals' },
    { title: 'Assess', detail: 'one assessor per section plus whole-repo sweeps' },
    { title: 'Check', detail: 'one skeptic per finding, defaults to rejected' },
    { title: 'Report', detail: 'refactor briefs for the best payoffs, then one report file' },
  ],
}

// Same sections as trace-debug.js (keep the two tables in sync; the Scout flags drift). Each is sized for one agent's
// context: about 6-8k lines of source.
const SECTIONS = [
  { id: 'E1', name: 'Engine: batch API, discovery, grouping, manifest',
    paths: ['crates/engine/src/api.rs', 'discover.rs', 'group.rs', 'sets.rs', 'session.rs', 'manifest.rs', 'events.rs', 'reports.rs'] },
  { id: 'E2', name: 'Engine: policy, config, presets, overrides, suggestions',
    paths: ['crates/engine/src/policy.rs', 'overrides.rs', 'presets.rs', 'config.rs', 'suggest.rs', 'codes.rs', 'schemas.rs', 'migrate.rs'] },
  { id: 'E3', name: 'Engine: pixel pipeline (load, ops, mips, coverage)', hot: true,
    paths: ['crates/engine/src/pipeline.rs', 'load.rs', 'decode.rs', 'psd.rs', 'archive.rs', 'image.rs', 'ops.rs', 'dilate.rs', 'resize.rs', 'filter.rs', 'mips.rs', 'coverage.rs', 'quantize.rs', 'sysmem.rs'] },
  { id: 'E4', name: 'Engine: encode, DDS write/patch, NVTT, validation; encoders-sys', hot: true,
    paths: ['crates/engine/src/encode.rs', 'dds.rs', 'dds_patch.rs', 'nvtt.rs', 'analyse.rs', 'crates/engine/src/validate/', 'crates/encoders-sys/src/'] },
  { id: 'E5', name: 'Engine: shaders, packing, bake, RNM, grass',
    paths: ['crates/engine/src/shader.rs', 'grass.rs', 'crates/engine/src/pack/', 'crates/engine/src/bake/', 'crates/engine/src/rnm/'] },
  { id: 'G1', name: 'GUI core: app shell, model root, documents, settings, worker', gui: true,
    paths: ['crates/gui/src/app.rs', 'main.rs', 'lib.rs', 'worker.rs', 'crash_log.rs', 'project.rs', 'sets_doc.rs', 'settings.rs', 'fonts.rs', 'layout.rs', 'crates/gui/src/settings/',
      'crates/gui/src/model/mod.rs', 'history.rs', 'shell.rs', 'items.rs', 'view.rs', 'nav.rs', 'messages.rs'] },
  { id: 'G2', name: 'GUI model: sets, bulk edit, rename, fields, input name', gui: true,
    paths: ['crates/gui/src/model/sets.rs', 'bulk.rs', 'rename.rs', 'input_name.rs', 'fields.rs', 'picker.rs', 'pin.rs', 'sections.rs', 'auto_flags.rs', 'encoding_follow.rs', 'pbr_follow.rs', 'pack_fills.rs'] },
  { id: 'G3', name: 'GUI exports: PGPatcher JSON, CS PBR records, complex material, presets, pgp/', gui: true,
    paths: ['crates/gui/src/model/pbr_export.rs', 'cs_pbr.rs', 'cs_pbr_export.rs', 'cm_json.rs', 'export.rs', 'crates/gui/src/model/presets.rs', 'skin.rs', 'crates/gui/src/pgp/'] },
  { id: 'G4', name: 'GUI preview: 2D preview, 3D viewport model, set preview, heatmaps', gui: true, hot: true,
    paths: ['crates/gui/src/model/viewport3d.rs', 'set_preview.rs', 'crates/gui/src/model/preview.rs', 'texture_view.rs', 'heatmap.rs', 'rnm_preview.rs', 'rnm.rs', 'dds_patch.rs', 'inspector.rs', 'crates/gui/src/render/'] },
  { id: 'G5', name: 'GUI main window UI: header, columns, set editor, dialogs', gui: true, hot: true,
    paths: ['crates/gui/src/ui/mod.rs', 'header.rs', 'columns.rs', 'sets_column.rs', 'set_editor.rs', 'input_name.rs', 'actions.rs', 'drawer.rs', 'bulk_panel.rs', 'rename_dialog.rs', 'slot_commands.rs', 'thumbs.rs', 'messages.rs', 'path_picker.rs', 'color.rs', 'dialogs.rs'] },
  { id: 'G6', name: 'GUI panels UI: JSON editor, settings, presets, PGPatcher fields, CS PBR, preview/3D panels', gui: true, hot: true,
    paths: ['crates/gui/src/ui/json_editor.rs', 'settings_panel.rs', 'crates/gui/src/ui/presets.rs', 'crates/gui/src/ui/preview.rs', 'pgp_fields.rs', 'crates/gui/src/ui/cs_pbr.rs', 'crates/gui/src/ui/viewport3d.rs', 'crates/gui/src/ui/set_preview.rs', 'advanced.rs', 'rnm_section.rs', 'cm_card.rs', 'skin_card.rs'] },
  { id: 'S1', name: 'Scan and mesh evidence: modscan + GUI mesh views', hot: true,
    paths: ['crates/modscan/src/', 'crates/gui/src/model/meshes.rs', 'used_by.rs', 'known_paths.rs', 'plugin_refs.rs', 'out_size.rs', 'readout.rs', 'crates/gui/src/ui/meshes.rs', 'crates/gui/src/ui/plugin_refs.rs'] },
  { id: 'R1', name: 'render3d (3D preview renderer)', hot: true,
    paths: ['crates/render3d/src/'] },
  { id: 'R2', name: 'Readers and CLI: nif, esp, vfs (BSA), pgpsim, cli',
    paths: ['crates/nif/src/', 'crates/esp/src/', 'crates/vfs/src/', 'crates/pgpsim/src/', 'crates/cli/src/'] },
]

// Whole-repo sweeps: things no single section can see.
const SWEEPS = [
  { id: 'arch', name: 'Architecture and crate boundaries',
    task: 'Assess the crate graph (Cargo.toml deps, `cargo tree --offline --workspace -e normal --depth 1`), layering (does the GUI reach into engine internals instead of its public API? does engine know about GUI concepts? model vs ui split in crates/gui), public API surface of each crate (pub items that should be pub(crate)), module size (files over ~2500 lines that mix responsibilities), and cycles or god objects (e.g. AppModel). Name concrete moves with their payoff and cost; no rewrites for taste.' },
  { id: 'dead-pub', name: 'Cross-crate dead code',
    task: 'rustc already rejects private dead code (clippy -D warnings), so look for what it cannot see: pub/pub(crate) items with no use outside their own definition and tests (grep each candidate across crates/, tests, benches, macros, serde attrs, FFI, schemas), modules only tests use, `#[allow(dead_code)]` / `#[allow(unused...)]` and `#[cfg(any())]`-style hiding, Cargo features nobody enables, Cargo.toml dependencies no source uses (grep the crate name; try `cargo machete` only if it is already installed), stale files under tools/ and schema/ nothing references, and retired config keys still parsed. Start from the largest pub surfaces.' },
  { id: 'dup', name: 'Duplicated rules and copy-paste',
    task: 'Find the same rule computed in two or more places (suffix tables, channel role tables, recipe choice, path folding, sub-type lists, hash/identity logic, JSON writers) where the copies can drift, and near-identical code blocks across crates/gui and crates/engine. For each, name the one place it should live. Check docs/DECISIONS.md: some duplication is a ruling (e.g. GUI tables that name what the engine cannot).' },
  { id: 'deps', name: 'Dependencies and build cost',
    task: 'Duplicate crate versions (`cargo tree --offline --workspace -d`), heavy dependencies used for a few lines, default features that could be off, and build-time hot spots (largest crates and generated code, proc-macro use). Offline only: never download. Report payoff as build seconds or binary size where you can estimate it honestly.' },
  { id: 'tests', name: 'Test suite health',
    task: 'Grep crates/*/tests and #[cfg(test)] modules: slow or sleep-based tests, tests that assert nothing or only that code ran, duplicated fixture builders that a shared helper should replace, #[ignore] tests with no SDC_* gate or reason, risky code paths (writes, overwrite guards, migrations, parsers of hostile input) with no test, and test files over ~3000 lines. Do not run the suite.' },
  { id: 'debt', name: 'Recorded shortcuts and stale notes',
    task: 'List every `shortcut:` (and older `ponytail:`) comment with its stated limit, and judge whether the limit is now crossed or the upgrade is cheap. Also find TODO/FIXME/XXX, comments that cite a DECISIONS note that no longer says that, and doc comments that contradict the code beside them. Read docs/DECISIONS.md only by grep.' },
]

const FINDINGS = {
  type: 'object',
  properties: {
    findings: {
      type: 'array',
      items: {
        type: 'object',
        properties: {
          title: { type: 'string' },
          kind: { type: 'string', enum: ['dead-code', 'efficiency', 'duplication', 'design', 'complexity', 'test-gap', 'consistency', 'dependency'] },
          file: { type: 'string', description: 'repo-relative path of the main site' },
          line: { type: 'integer' },
          sites: { type: 'array', items: { type: 'string' }, description: 'other file:line sites involved (copies, callers)' },
          payoff: { type: 'string', enum: ['high', 'medium', 'low'], description: 'what the change buys: less code, fewer bugs, measurable speed, easier change' },
          effort: { type: 'string', enum: ['S', 'M', 'L'], description: 'S < 1 hour, M < 1 day, L more' },
          claim: { type: 'string', description: 'what is wrong and what to do instead, in two or three sentences' },
          evidence: { type: 'string', description: 'what you read, grepped or ran that shows it: quote the code, give the grep and its hit count, or the measurement' },
        },
        required: ['title', 'kind', 'file', 'line', 'sites', 'payoff', 'effort', 'claim', 'evidence'],
      },
    },
    scorecard: {
      type: 'object',
      description: '1 (poor) to 5 (excellent), each with one sentence of why',
      properties: {
        structure: { type: 'string' }, readability: { type: 'string' }, efficiency: { type: 'string' },
        tests: { type: 'string' }, consistency: { type: 'string' },
      },
      required: ['structure', 'readability', 'efficiency', 'tests', 'consistency'],
    },
    strengths: { type: 'string', description: 'what this area does well and should be copied elsewhere' },
    not_covered: { type: 'string' },
  },
  required: ['findings', 'scorecard', 'strengths', 'not_covered'],
}

const VERDICT = {
  type: 'object',
  properties: {
    rejected: { type: 'boolean' },
    reason: { type: 'string' },
    verified: { type: 'string', description: 'the check you ran: the grep and its result, the caller list, the measurement' },
    payoff: { type: 'string', enum: ['high', 'medium', 'low'] },
    effort: { type: 'string', enum: ['S', 'M', 'L'] },
    duplicate_of: { type: 'string', description: 'title of an earlier finding this repeats, if any' },
  },
  required: ['rejected', 'reason', 'verified', 'payoff', 'effort'],
}

const BRIEF = {
  type: 'object',
  properties: {
    title: { type: 'string' },
    files: { type: 'array', items: { type: 'string' } },
    steps: { type: 'string', description: 'ordered steps, each one commit that builds' },
    safety_net: { type: 'string', description: 'the tests that must stay green, and any test to add first to pin current behaviour' },
    goldens: { type: 'string', description: 'whether encoded bytes or encode_version could change (they must not without a DECISIONS note)' },
    risk: { type: 'string' },
    owner: { type: 'string', description: '"main" or "nif-team" (modscan, nif, esp, vfs, render3d, pgpsim)' },
  },
  required: ['title', 'files', 'steps', 'safety_net', 'goldens', 'risk', 'owner'],
}

// Skills each agent loads before starting. Rust CLI Skills (plugin `rust-cli-skills`) are optional.
const RUST = n => `rust-cli-skills:rust-${n}`
const skillsFor = s => {
  const opt = [RUST('design'), RUST('idiomatic')]
  if (s.hot) opt.push(RUST('performance'), RUST('memory'))
  if (s.gui) opt.push('egui-ui-rules') // its per-frame cost rules (group 9) are the GUI efficiency yardstick
  return opt
}
const skillLine = (required, optional) =>
  `Before anything else, load these skills with the Skill tool and follow them: ${required.join(', ')}.` +
  (optional.length ? ` Also load each of these ONLY if the Skill tool lists it (skip silently if not): ${optional.join(', ')}. Apply them to your scope only.` : '')

const a = args || {}
const pick = a.sections ? SECTIONS.filter(s => a.sections.includes(s.id)) : SECTIONS
const sweeps = a.sweeps ? SWEEPS.filter(s => a.sweeps.includes(s.id)) : SWEEPS
const OUT = a.out || '.superpowers/sdd/quality-audit'
const DATE = a.date || 'undated'
const REPO = a.repo || 'the current working directory (the repo root)'
const MAX_REVIEWS = a.maxReviews || 100
const BRIEFS = a.briefs === undefined ? 12 : a.briefs
let reviewsStarted = 0
let reviewsSkipped = 0

const TARGET = 'target/quality-shared'
const BASE = `Read-only: do not edit, commit, push or spawn subagents. Never run \`python -\` heredocs. Never touch %LOCALAPPDATA%\\sdc, %APPDATA%\\sdc or the owner's game and mod folders. Download nothing. Work in ${REPO}.`
const RULES = `${BASE} Prefer reading and grepping. If you must run cargo, use --offline, CARGO_BUILD_JOBS=4 and CARGO_TARGET_DIR=${TARGET} (shared; never delete it yourself); never run the render_ GPU tests. Capture output to a file; never pipe it through head.`
const BAR = `Report only what a busy senior reviewer would act on: a finding must name a concrete change and what it buys. No style nits rustfmt or clippy would catch, no renames for taste, no "add comments". Behaviour recorded as a ruling in docs/DECISIONS.md is intended, and a \`shortcut:\` comment within its stated limit is a decision: cite them if you think they are wrong. Efficiency claims need a reason the code is hot (called per pixel, per frame, per file of a 10k batch) or a measurement; otherwise mark payoff low. Encoded output bytes are pinned by goldens: never propose a change that alters them as a quality fix.`
log(`Planned: 1 scout + ${pick.length} assessors + ${sweeps.length} sweeps + up to ${MAX_REVIEWS} checks + up to ${BRIEFS} briefs + 1 report`)

phase('Scout')
const scout = await agent(
  `List every .rs file under crates/*/src with \`git ls-files 'crates/*/src/*.rs' 'crates/*/src/**/*.rs'\`. Section table (a path ending in / is a whole folder; a bare file name is in the folder of the previous full path):\n${JSON.stringify(SECTIONS.map(s => ({ id: s.id, paths: s.paths })), null, 1)}\nReport files no section covers (ignore crates/bench) with the section each fits best, section paths that no longer exist, each section's line count today, and the 15 largest .rs files under crates/ by lines. ${RULES}`,
  { label: 'scout:coverage', phase: 'Scout', effort: 'low', schema: {
    type: 'object',
    properties: {
      unassigned: { type: 'array', items: { type: 'object', properties: { file: { type: 'string' }, section: { type: 'string' } }, required: ['file', 'section'] } },
      missing: { type: 'array', items: { type: 'object', properties: { path: { type: 'string' }, section: { type: 'string' } }, required: ['path', 'section'] } },
      lines: { type: 'array', items: { type: 'object', properties: { id: { type: 'string' }, lines: { type: 'integer' } }, required: ['id', 'lines'] } },
      largest: { type: 'array', items: { type: 'string' }, description: '"path: lines"' },
    },
    required: ['unassigned', 'missing', 'lines', 'largest'],
  } })
const extra = {}
for (const u of (scout && scout.unassigned) || []) (extra[u.section] = extra[u.section] || []).push(u.file)
if (scout && scout.unassigned.length) log(`Scout: ${scout.unassigned.length} unassigned files added to their nearest section`)
if (scout && scout.missing.length) log(`STALE SECTION TABLE: ${scout.missing.map(m => m.section + ':' + m.path).join(', ')}. Update SECTIONS in quality-audit.js and trace-debug.js.`)
const sizeOf = {}
for (const l of (scout && scout.lines) || []) {
  sizeOf[l.id] = l.lines
  if (l.lines > 9000) log(`Section ${l.id} is ${l.lines} lines: over one agent's comfortable budget; split it in SECTIONS`)
}
const largest = (scout && scout.largest || []).join(', ')

// Dedup across all assessors (sweeps and sections overlap on purpose): same file within 10 lines and overlapping
// titles, or the same kind within 2 lines. The checker's duplicate_of catches the rest.
const words = t => new Set(t.toLowerCase().split(/[^a-z0-9_]+/).filter(w => w.length > 2))
const similar = (x, y) => { const A = words(x), B = words(y); let n = 0; A.forEach(w => { if (B.has(w)) n++ }); return n >= Math.min(A.size, B.size) / 2 }
const seen = []
const isSeen = f => seen.some(s => s.file === f.file &&
  ((Math.abs(s.line - f.line) <= 10 && similar(s.title, f.title)) || (s.kind === f.kind && Math.abs(s.line - f.line) <= 2)))
const kept = []
const rejected = []
const cards = {}

// Sweeps first: they see what no section can, so they should not be the ones the check cap starves.
const scopes = sweeps.map(w => ({ kind: 'sweep', ...w })).concat(pick.map(s => ({ kind: 'section', ...s })))
const lenses = {
  'dead-code': 'Grep every name involved across the whole repo (src, tests, benches, macros, serde/schemars attributes, FFI, string-keyed lookups, features, cfgs, docs that promise it). Reject if anything real uses it, or if it is public API a ruling keeps.',
  efficiency: 'Is the code actually hot (per pixel, per frame, per batch file)? Does the proposed change keep output bytes identical? Is the gain real at Tessalith sizes (4k-16k textures, 10k-file batches)? Reject speculative micro-optimisations.',
  duplication: 'Do the copies really compute the same rule, and can they drift? Is one copy a ruling (docs/DECISIONS.md) or a deliberate GUI-side table? Reject if merging costs more than it saves.',
  design: 'Would the move make a real change easier, or is it taste? Check how often the area changed (`git log --oneline -- <file> | measure`). Reject churn without payoff.',
  complexity: 'Is the complexity essential to the domain (Skyrim/CS rules) or accidental? Would the proposed shape be shorter and clearer? Reject if it only moves code around.',
  'test-gap': 'Is the path really untested (grep tests for the function and its callers)? Is it risky (writes, overwrites, parsers of hostile input, migrations)? Reject gaps on trivial code.',
  consistency: 'Do the two places really disagree today, and does a user or caller see it? Reject cosmetic differences.',
  dependency: 'Verify with cargo tree --offline or a grep of use sites. Reject if the dependency is needed by a feature that is on.',
}

await pipeline(scopes,
  s => agent(
    s.kind === 'section'
      ? `You are a quality assessor for section ${s.id} — ${s.name} — of the Tessalith repo (Rust) in ${REPO}. This is not a bug hunt: assess dead code, efficiency, duplication, design, complexity, test gaps and consistency.
${skillLine(['ponytail:ponytail'], skillsFor(s))}
Your files: ${JSON.stringify(s.paths.concat(extra[s.id] || []))} (a bare file name is in the folder of the previous full path). About ${sizeOf[s.id] || 'unknown'} lines. Read the main files in full, skim the rest by signatures, grep tests instead of reading them.
Largest files in the repo, for context: ${largest}.
Look for: functions or types nothing calls (grep before you claim); code that recomputes, clones or allocates per call on a hot path${s.hot ? ' (this section is hot: per pixel, per frame or per batch file)' : ''}; rules computed twice; functions over ~120 lines or nesting over 4 that a split would make clearer; error handling that loses information; risky paths with no test; places that disagree with their neighbours or with docs/gui.md. Max 10 findings, best payoff first.
${BAR}
${RULES}`
      : `You are a whole-repo quality sweeper for "${s.name}" in the Tessalith repo (Rust) in ${REPO}.
${skillLine(['ponytail:ponytail'], [RUST('design'), RUST('build')])}
Task: ${s.task}
Largest files in the repo, for context: ${largest}. Max 12 findings, best payoff first. The scorecard rates the repo as a whole on this theme.
${BAR}
${RULES}`,
    { label: `assess:${s.id}`, phase: 'Assess', schema: FINDINGS }),
  (res, s) => {
    if (!res) { log(`${s.id}: assessor returned nothing`); return [] }
    cards[s.id] = { name: s.name, scorecard: res.scorecard, strengths: res.strengths, not_covered: res.not_covered }
    log(`${s.id}: ${res.findings.length} findings`)
    let fresh = res.findings.filter(f => !isSeen(f))
    fresh.forEach(f => seen.push(f))
    const room = Math.max(0, MAX_REVIEWS - reviewsStarted)
    if (fresh.length > room) {
      reviewsSkipped += fresh.length - room
      log(`${s.id}: check cap reached; ${fresh.length - room} findings not checked (raise args.maxReviews)`)
      fresh = fresh.slice(0, room) // assessors list best payoff first
    }
    reviewsStarted += fresh.length
    return parallel(fresh.map(f => () =>
      agent(
        `Skeptically check this code-quality finding from ${s.id}. Your job is to REJECT it unless the evidence holds and the change is worth making. Default to rejected=true when unsure.
${skillLine(['superpowers:verification-before-completion'], [])}
Kind: ${f.kind}. Title: ${f.title}
Location: ${f.file}:${f.line}; other sites: ${f.sites.join(', ') || 'none'}
Claim: ${f.claim}
Evidence given: ${f.evidence}
Claimed payoff ${f.payoff}, effort ${f.effort}.
How to check this kind: ${lenses[f.kind] || lenses.design}
Run the check yourself and put it in "verified". Re-grade payoff and effort honestly.
${RULES}`,
        { label: `check:${s.id}:${f.file.split('/').pop()}:${f.line}`, phase: 'Check', schema: VERDICT })
        .then(v => ({ f, s: s.id, v }))))
  },
  checks => {
    for (const r of (checks || []).filter(Boolean)) {
      if (!r.v || r.v.rejected || r.v.duplicate_of) rejected.push(r)
      else kept.push({ section: r.s, kind: r.f.kind, payoff: r.v.payoff, effort: r.v.effort, finding: r.f, verified: r.v.verified, reason: r.v.reason })
    }
  })
log(`${kept.length} findings kept, ${rejected.length} rejected, ${reviewsSkipped} not checked`)

phase('Report')
// Best value first: payoff high before low, then cheap before costly.
const P = { high: 0, medium: 1, low: 2 }, E = { S: 0, M: 1, L: 2 }
kept.sort((x, y) => P[x.payoff] - P[y.payoff] || E[x.effort] - E[y.effort])
const briefed = kept.filter(k => k.payoff !== 'low' && k.kind !== 'dead-code').slice(0, BRIEFS)
const skippedBriefs = kept.filter(k => k.payoff !== 'low' && k.kind !== 'dead-code').length - briefed.length
if (skippedBriefs > 0) log(`${skippedBriefs} medium/high findings get no brief (raise args.briefs)`)
await parallel(briefed.map(k => () =>
  agent(
    `Turn this checked quality finding into a self-contained refactor brief for an implementer who has not seen this conversation.
${skillLine(['ponytail:ponytail'], [RUST('design')])} Write the brief only; do not implement.
Finding (${k.kind}, payoff ${k.payoff}, effort ${k.effort}): ${k.finding.title} at ${k.finding.file}:${k.finding.line}; sites ${k.finding.sites.join(', ') || 'none'}
Claim: ${k.finding.claim}
Checker: ${k.reason} (verified: ${k.verified})
Give ordered steps where each is one commit that builds, the tests that must stay green (and a pinning test to add first when behaviour must not change), whether goldens or encode_version could be touched, the risk, and the owner. ${RULES}`,
    { label: `brief:${k.section}:${k.finding.file.split('/').pop()}`, phase: 'Report', schema: BRIEF, effort: 'low' })
    .then(b => { k.brief = b })))

const report = await agent(
  `Write the quality-audit report to ${OUT}/${DATE}-report.md under ${REPO} (create the folder; this path is gitignored scratch). Plain English, short sentences, tables where they help.
Sections:
1. Summary: counts by kind and payoff, scopes assessed, findings not checked because of the cap (${reviewsSkipped}), and the five changes worth doing first.
2. Scorecard: one table, a row per section or sweep, columns structure/readability/efficiency/tests/consistency (the 1-5 score; the reason as a footnote or hover-free short text), then each area's strengths in one line.
3. Delete list: every kept dead-code finding as a checklist line (file:line, what to delete, the grep that proved it unused).
4. Refactors and efficiency: one block per kept finding with a brief, best value first: title, file:line, kind, payoff/effort, claim, what the checker verified, then the brief (steps, safety net, goldens, risk, owner).
5. Other kept findings: one line each (title, file:line, kind, payoff/effort).
6. Architecture: a short narrative from the "arch" sweep's findings and scorecard; what to keep, what to move.
7. Rejected: one line each with the checker's reason (so the next run does not re-raise them).
8. Not covered, per scope.
After writing, delete the shared build folder ${TARGET} under ${REPO} if it exists (this run's scratch).
Data: ${JSON.stringify({ cards, kept, rejected: rejected.map(r => ({ s: r.s, title: r.f.title, at: `${r.f.file}:${r.f.line}`, why: r.v ? (r.v.duplicate_of ? 'duplicate of ' + r.v.duplicate_of : r.v.reason) : 'checker returned nothing' })) })}
Do not edit anything else. Return the report path.`,
  { label: 'report', phase: 'Report' })

return { report, kept: kept.length, rejected: rejected.length, unchecked: reviewsSkipped, by_kind: kept.reduce((m, k) => (m[k.kind] = (m[k.kind] || 0) + 1, m), {}) }

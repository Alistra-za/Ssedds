export const meta = {
  name: 'trace-debug',
  description: 'Systematic trace debugging: per-section tracers find bugs, each finding gets its own skeptical reviewer, confirmed ones become fix briefs',
  whenToUse: 'Whole-repo or multi-crate bug hunt in Tessalith. Args (all optional): {sections: ["E1",...], rounds: 1, out: "<dir for the report>", date: "YYYY-MM-DD"}',
  phases: [
    { title: 'Scout', detail: 'map every crates/*/src file to a section; flag unassigned files' },
    { title: 'Trace', detail: 'one tracer per section, entry point to sink' },
    { title: 'Review', detail: 'one skeptical reviewer per finding, defaults to refuted' },
    { title: 'Handoff', detail: 'fix brief per confirmed bug, then one report file' },
  ],
}

// Sections are sized for one agent's context: about 6-8k lines of source each (~10 tokens per line of Rust),
// leaving room for grep output, neighbours' signatures, tests and the agent's own reasoning. Tests are grepped,
// never read wholesale. Sizes are line counts on main 18eef60 (2026-10-16); the Scout re-checks coverage.
const SECTIONS = [
  { id: 'E1', name: 'Engine: batch API, discovery, grouping, manifest', lines: 5300,
    paths: ['crates/engine/src/api.rs', 'discover.rs', 'group.rs', 'sets.rs', 'session.rs', 'manifest.rs', 'events.rs', 'reports.rs'],
    entry: 'plan/convert entry points in api.rs (BatchRequest -> plan -> jobs -> run), discovery of inputs, set grouping, manifest load/append/compaction, if-changed' },
  { id: 'E2', name: 'Engine: policy, config, presets, overrides, suggestions', lines: 6100,
    paths: ['crates/engine/src/policy.rs', 'overrides.rs', 'presets.rs', 'config.rs', 'suggest.rs', 'codes.rs', 'schemas.rs', 'migrate.rs'],
    entry: 'profile resolution for one file: preset chain -> config -> rules -> [files] -> --set; sub-type suggestion; warning codes; schema generation' },
  { id: 'E3', name: 'Engine: pixel pipeline (load, ops, mips, coverage)', lines: 5600,
    paths: ['crates/engine/src/pipeline.rs', 'load.rs', 'decode.rs', 'psd.rs', 'archive.rs', 'image.rs', 'ops.rs', 'dilate.rs', 'resize.rs', 'filter.rs', 'mips.rs', 'coverage.rs', 'quantize.rs', 'sysmem.rs'],
    entry: 'one job: load a source (PNG/PSD/KRA/ORA/EXR/DDS) -> preprocess ops -> resize -> mip chain -> coverage preservation -> quantize; memory admission' },
  { id: 'E4', name: 'Engine: encode, DDS write/patch, NVTT, validation; encoders-sys', lines: 4100,
    paths: ['crates/engine/src/encode.rs', 'dds.rs', 'dds_patch.rs', 'nvtt.rs', 'analyse.rs', 'crates/engine/src/validate/', 'crates/encoders-sys/src/'],
    entry: 'mip chain -> BCn/encoder choice -> fixups -> DDS header + write_atomic; patch-header; NVTT subprocess; validation of written files; FFI into the vendored encoders' },
  { id: 'E5', name: 'Engine: shaders, packing, bake, RNM, grass', lines: 5900,
    paths: ['crates/engine/src/shader.rs', 'grass.rs', 'crates/engine/src/pack/', 'crates/engine/src/bake/', 'crates/engine/src/rnm/'],
    entry: 'set shader pick (explicit/auto rules/mesh votes) -> member re-typing -> pack recipes (_rmaos/_m/_orm) -> bakes -> RNM layer composition -> complex grass stacking' },
  { id: 'G1', name: 'GUI core: app shell, model root, documents, settings, worker', lines: 7700,
    paths: ['crates/gui/src/app.rs', 'main.rs', 'lib.rs', 'worker.rs', 'crash_log.rs', 'project.rs', 'sets_doc.rs', 'settings.rs', 'fonts.rs', 'layout.rs', 'crates/gui/src/settings/',
      'crates/gui/src/model/mod.rs', 'history.rs', 'shell.rs', 'items.rs', 'view.rs', 'nav.rs', 'messages.rs'],
    entry: 'frame loop -> AppModel::poll -> worker results -> re-plan; open folder; edit -> undo/redo -> dirty/rev -> save/reload/recovery; gui.toml settings' },
  { id: 'G2', name: 'GUI model: sets, bulk edit, rename, fields, input name', lines: 5900,
    paths: ['crates/gui/src/model/sets.rs', 'bulk.rs', 'rename.rs', 'input_name.rs', 'fields.rs', 'picker.rs', 'pin.rs', 'sections.rs', 'auto_flags.rs', 'encoding_follow.rs', 'pbr_follow.rs', 'pack_fills.rs'],
    entry: 'set views built from the plan + sets doc; multi-select bulk edits; input/output rename with regex; field edits written to sdc.toml/sdc-project.toml' },
  { id: 'G3', name: 'GUI exports: PGPatcher JSON, CS PBR records, complex material, presets, pgp/', lines: 6100,
    paths: ['crates/gui/src/model/pbr_export.rs', 'cs_pbr.rs', 'cs_pbr_export.rs', 'cm_json.rs', 'export.rs', 'crates/gui/src/model/presets.rs', 'skin.rs', 'crates/gui/src/pgp/'],
    entry: 'Export click -> DDS batch -> JSON blocks (PGPatcher, CS PBRTextureSets/MaterialObjects, _m.json) -> hash records -> overwrite guards; preset creator save/load' },
  { id: 'G4', name: 'GUI preview: 2D preview, 3D viewport model, set preview, heatmaps', lines: 6600,
    paths: ['crates/gui/src/model/viewport3d.rs', 'set_preview.rs', 'crates/gui/src/model/preview.rs', 'texture_view.rs', 'heatmap.rs', 'rnm_preview.rs', 'rnm.rs', 'dds_patch.rs', 'inspector.rs', 'crates/gui/src/render/'],
    entry: 'select a texture/set -> preview source decode -> GPU upload -> 3D render thread messages -> device loss handling; RNM preview; DDS header inspector/patch' },
  { id: 'G5', name: 'GUI main window UI: header, columns, set editor, dialogs', lines: 7700,
    paths: ['crates/gui/src/ui/mod.rs', 'header.rs', 'columns.rs', 'sets_column.rs', 'set_editor.rs', 'input_name.rs', 'actions.rs', 'drawer.rs', 'bulk_panel.rs', 'rename_dialog.rs', 'slot_commands.rs', 'thumbs.rs', 'messages.rs', 'path_picker.rs', 'color.rs', 'dialogs.rs'],
    entry: 'keyboard shortcuts and focus guards; drag-and-drop between columns; set editor cards; dialogs that write documents; thumbnail cache eviction' },
  { id: 'G6', name: 'GUI panels UI: JSON editor, settings, presets, PGPatcher fields, CS PBR, preview/3D panels', lines: 7400,
    paths: ['crates/gui/src/ui/json_editor.rs', 'settings_panel.rs', 'crates/gui/src/ui/presets.rs', 'crates/gui/src/ui/preview.rs', 'pgp_fields.rs', 'crates/gui/src/ui/cs_pbr.rs', 'crates/gui/src/ui/viewport3d.rs', 'crates/gui/src/ui/set_preview.rs', 'advanced.rs', 'rnm_section.rs', 'cm_card.rs', 'skin_card.rs'],
    entry: 'widget -> model edit paths (each control must write exactly one undoable edit); text boxes that parse numbers/paths; immediate viewports' },
  { id: 'S1', name: 'Scan and mesh evidence: modscan + GUI mesh views', lines: 8100,
    paths: ['crates/modscan/src/', 'crates/gui/src/model/meshes.rs', 'used_by.rs', 'known_paths.rs', 'plugin_refs.rs', 'out_size.rs', 'readout.rs', 'crates/gui/src/ui/meshes.rs', 'crates/gui/src/ui/plugin_refs.rs'],
    entry: 'MO2 instance -> modlist/BSAs -> parallel parse -> SQLite store (cache reuse, derived PGPatcher copies, winners) -> GUI queries (mesh votes, alpha, used-by, missing/unused)' },
  { id: 'R1', name: 'render3d (3D preview renderer)', lines: 8700,
    paths: ['crates/render3d/src/'],
    entry: 'NIF bytes -> parse -> material state -> shader permutation compile/cache -> texture upload (BC, mips, cubes) -> draw; HDRI backdrop; CS settings; device loss' },
  { id: 'R2', name: 'Readers and CLI: nif, esp, vfs (BSA), pgpsim, cli', lines: 6400,
    paths: ['crates/nif/src/', 'crates/esp/src/', 'crates/vfs/src/', 'crates/pgpsim/src/', 'crates/cli/src/'],
    entry: 'hostile input files (NIF/ESP/BSA from the internet) -> bounded parsers; PGPatcher simulation; CLI verbs -> exit codes and stdout/JSON' },
]
// Not covered on purpose: crates/bench (measurement harness, not shipped). The Scout reports anything else unassigned.

const FINDINGS = {
  type: 'object',
  properties: {
    findings: {
      type: 'array',
      items: {
        type: 'object',
        properties: {
          title: { type: 'string' },
          file: { type: 'string', description: 'repo-relative path' },
          line: { type: 'integer' },
          severity: { type: 'string', enum: ['critical', 'important', 'minor'] },
          trace: { type: 'string', description: 'the path traced: entry -> ... -> the faulty line, with file:line hops' },
          failing_case: { type: 'string', description: 'concrete input/state -> wrong output/crash/data loss' },
          evidence: { type: 'string', description: 'what you read or ran that shows it (quote the condition, name the test you ran)' },
        },
        required: ['title', 'file', 'line', 'severity', 'trace', 'failing_case', 'evidence'],
      },
    },
    covered: { type: 'string', description: 'entry points traced end to end' },
    not_covered: { type: 'string', description: 'what you did not get to, and why' },
  },
  required: ['findings', 'covered', 'not_covered'],
}

const VERDICT = {
  type: 'object',
  properties: {
    refuted: { type: 'boolean' },
    reason: { type: 'string' },
    reproduced: { type: 'boolean', description: 'true only if you ran something that showed the failure' },
    severity: { type: 'string', enum: ['critical', 'important', 'minor'] },
    duplicate_of: { type: 'string', description: 'title of an earlier finding this repeats, if any' },
  },
  required: ['refuted', 'reason', 'reproduced', 'severity'],
}

const BRIEF = {
  type: 'object',
  properties: {
    title: { type: 'string' },
    files: { type: 'array', items: { type: 'string' } },
    red_test: { type: 'string', description: 'the failing test to write first: file, name, what it asserts' },
    fix: { type: 'string', description: 'smallest correct fix, with the shared code to change' },
    risk: { type: 'string' },
    owner: { type: 'string', description: '"main" or "nif-team" (modscan, nif, esp, vfs, render3d, pgpsim)' },
  },
  required: ['title', 'files', 'red_test', 'fix', 'risk', 'owner'],
}

// Skills each agent loads with the Skill tool before starting. Rust CLI Skills (plugin `rust-cli-skills`) are optional:
// loaded only when the Skill tool lists them, so the workflow runs the same without the plugin.
const RUST = n => `rust-cli-skills:rust-${n}`
const SECTION_SKILLS = {
  E1: [RUST('concurrency'), RUST('filesystem'), RUST('errors')],   // threads, manifest lock, atomic writes
  E3: [RUST('memory'), RUST('errors')],                             // memory admission, big buffers
  E4: [RUST('filesystem'), RUST('processes'), RUST('errors')],      // write_atomic, NVTT subprocess
  S1: [RUST('concurrency'), RUST('filesystem')],                    // SQLite store, parallel scan
  R1: [RUST('memory')],                                             // GPU buffers, texture uploads
  R2: [RUST('io'), RUST('errors'), RUST('cli-testing')],            // hostile NIF/ESP/BSA parsers, CLI exit codes and streams
}
const skillLine = (required, optional) =>
  `Before anything else, load these skills with the Skill tool and follow them: ${required.join(', ')}.` +
  (optional.length ? ` Also load each of these ONLY if the Skill tool lists it (skip silently if not): ${optional.join(', ')}. Apply them to your section only, not as a reason to widen scope.` : '')

const a = args || {}
const ROUNDS = a.rounds || 1
const pick = a.sections ? SECTIONS.filter(s => a.sections.includes(s.id)) : SECTIONS
const OUT = a.out || '.superpowers/sdd/trace-debug'
const DATE = a.date || 'undated'

const RULES = `Read-only: do not edit, commit, push or spawn subagents. Never run \`python -\` heredocs. Never touch %LOCALAPPDATA%\\sdc or the owner's game and mod folders. If you run cargo, use --offline, CARGO_BUILD_JOBS=4, \`-- --test-threads=4\`, and CARGO_TARGET_DIR=target/trace-<your label> (delete it when done); targeted test binaries only.`

phase('Scout')
const scout = await agent(
  `List every .rs file under crates/*/src with \`git ls-files 'crates/*/src/*.rs' 'crates/*/src/**/*.rs'\`. Here is the section table (a path ending in / means the whole folder; a bare file name belongs to the folder of the previous full path in the same list):\n${JSON.stringify(SECTIONS.map(s => ({ id: s.id, paths: s.paths })), null, 1)}\nReport files that match no section (ignore crates/bench), and for each the section it fits best by subsystem. Also report each section's real line count today (wc -l of its files). ${RULES}`,
  { label: 'scout:coverage', schema: {
    type: 'object',
    properties: {
      unassigned: { type: 'array', items: { type: 'object', properties: { file: { type: 'string' }, section: { type: 'string' } }, required: ['file', 'section'] } },
      lines: { type: 'array', items: { type: 'object', properties: { id: { type: 'string' }, lines: { type: 'integer' } }, required: ['id', 'lines'] } },
    },
    required: ['unassigned', 'lines'],
  } })
const extra = {}
for (const u of (scout && scout.unassigned) || []) (extra[u.section] = extra[u.section] || []).push(u.file)
if (scout && scout.unassigned.length) log(`Scout: ${scout.unassigned.length} unassigned files added to their nearest section`)
for (const l of (scout && scout.lines) || []) {
  if (l.lines > 9000) log(`Section ${l.id} is ${l.lines} lines: over one agent's comfortable budget; split it in SECTIONS`)
}

// Dedup across sections and rounds: same file within 6 lines and same title stem counts as seen.
const seen = []
const isSeen = f => seen.some(s => s.file === f.file && Math.abs(s.line - f.line) <= 6)
const confirmed = []
const refuted = []
const notCovered = {}

for (let round = 1; round <= ROUNDS; round++) {
  const prior = seen.map(s => `${s.file}:${s.line} ${s.title}`).join('\n') || '(none)'
  await pipeline(pick,
    s => agent(
      `You are a trace debugger for section ${s.id} — ${s.name} — of the Tessalith repo (F:\\ai\\dds\\converter, Rust).
${skillLine(['superpowers:systematic-debugging'], SECTION_SKILLS[s.id] || [])} You are finding bugs, not fixing them: stop at root cause and evidence.
Your files: ${JSON.stringify(s.paths.concat(extra[s.id] || []))} (a bare file name is in the folder of the previous full path).
Size: about ${s.lines} lines of source. Context budget: read the files on the main paths in full; for the rest read signatures and skim. Do not read test files wholesale — grep them for what you need. You may follow a call into another section, but only read the called function, not its file.
Method — systematic tracing, not browsing:
1. List the section's entry points (${s.entry}).
2. For each, trace data from where it enters to where it leaves (written file, GPU, DB, UI state, exit code). At every hop ask: what input breaks this? Empty, zero, max size, non-ASCII path, duplicate key, concurrent call, cancelled mid-way, file changed on disk, error swallowed, callers that disagree with the callee's contract, the same rule computed two different ways.
3. Record only bugs with a concrete failing case (input/state -> wrong result, crash, data loss, hang). Shortcuts marked \`shortcut:\` are decisions, not bugs, unless the stated limit is already crossed. Behaviour recorded in docs/DECISIONS.md as a ruling is intended — cite it if you think it's wrong rather than reporting it as a bug.
4. Prefer evidence you can show: quote the condition, or run one targeted existing test / a tiny probe. Max 8 findings; most severe first.
Already reported (do not repeat):\n${prior}
${RULES}`,
      { label: `trace:${s.id}${ROUNDS > 1 ? ':r' + round : ''}`, phase: 'Trace', schema: FINDINGS }),
    (res, s) => {
      if (!res) { log(`${s.id}: tracer returned nothing`); return [] }
      log(`${s.id}: ${res.findings.length} findings; not covered: ${res.not_covered.slice(0, 120)}`)
      notCovered[s.id] = (notCovered[s.id] ? notCovered[s.id] + ' | ' : '') + res.not_covered
      const fresh = res.findings.filter(f => !isSeen(f))
      fresh.forEach(f => seen.push(f))
      return parallel(fresh.map(f => () =>
        agent(
          `Skeptically review this bug report from section ${s.id}. Your job is to REFUTE it. Default to refuted=true unless the code clearly shows the failure.
${skillLine(['superpowers:verification-before-completion', 'superpowers:receiving-code-review'], [])} Treat the report as a claim to verify, not as feedback to accept.
Title: ${f.title}
Location: ${f.file}:${f.line} (severity claimed: ${f.severity})
Trace: ${f.trace}
Failing case: ${f.failing_case}
Evidence: ${f.evidence}
Check: does the traced path really reach that line with that input? Is there a guard elsewhere (caller validation, earlier return, type invariant)? Is it intended (docs/DECISIONS.md ruling, a \`shortcut:\` comment within its limit)? Is the failing case reachable by a real user? Reproduce if a targeted existing test or a tiny probe can show it; set reproduced=true only if you saw it fail. Re-grade severity honestly.
${RULES}`,
          { label: `review:${s.id}:${f.file.split('/').pop()}:${f.line}`, phase: 'Review', schema: VERDICT, effort: 'high' })
          .then(v => ({ f, s: s.id, v }))))
    },
    async reviews => {
      for (const r of (reviews || []).filter(Boolean)) {
        if (!r.v || r.v.refuted || r.v.duplicate_of) { refuted.push(r); continue }
        const brief = await agent(
          `Turn this confirmed bug into a self-contained fix brief for an implementer who has not seen this conversation.
${skillLine(['superpowers:test-driven-development', 'ponytail:ponytail'], [])} Write the brief only; do not implement.
Bug: ${r.f.title} at ${r.f.file}:${r.f.line} (severity ${r.v.severity}; reviewer: ${r.v.reason}; reproduced: ${r.v.reproduced})
Trace: ${r.f.trace}
Failing case: ${r.f.failing_case}
Give the RED test to write first, the smallest correct fix (grep every caller of what you change; fix shared code once), the risk, and the owner. ${RULES}`,
          { label: `brief:${r.s}:${r.f.file.split('/').pop()}:${r.f.line}`, phase: 'Handoff', schema: BRIEF, effort: 'low' })
        confirmed.push({ section: r.s, severity: r.v.severity, reproduced: r.v.reproduced, finding: r.f, review: r.v.reason, brief })
      }
    })
  log(`Round ${round}: ${confirmed.length} confirmed, ${refuted.length} refuted so far`)
}

phase('Handoff')
const order = { critical: 0, important: 1, minor: 2 }
confirmed.sort((x, y) => order[x.severity] - order[y.severity])
const report = await agent(
  `Write the trace-debug report to ${OUT}/${DATE}-report.md in the repo root (create the folder; this path is gitignored scratch). Plain English, short sentences.
Sections: Summary (counts by severity, sections traced, rounds), Confirmed (one block per bug, most severe first: title, file:line, failing case, reviewer's reason, reproduced yes/no, then the fix brief: RED test, fix, risk, owner), Refuted (one line each with the reviewer's reason), Not covered (per section, from the tracers).
Data: ${JSON.stringify({ notCovered, confirmed, refuted: refuted.map(r => ({ s: r.s, title: r.f.title, at: `${r.f.file}:${r.f.line}`, why: r.v ? (r.v.duplicate_of ? 'duplicate of ' + r.v.duplicate_of : r.v.reason) : 'reviewer returned nothing' })) })}
Do not edit anything else. Return the report path.`,
  { label: 'report', phase: 'Handoff' })

return { report, confirmed: confirmed.length, refuted: refuted.length, by_severity: confirmed.reduce((m, c) => (m[c.severity] = (m[c.severity] || 0) + 1, m), {}) }

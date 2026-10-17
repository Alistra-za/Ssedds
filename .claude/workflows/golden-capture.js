export const meta = {
  name: 'golden-capture',
  description: 'Phase D golden captures: apply a CS settings profile, launch Skyrim via MO2 + DevBench, place subjects and camera relative to the coc marker, take RenderDoc captures, quit, restore settings byte-exact, keep the .rdc files as fixtures and run the golden extractors',
  whenToUse: 'NIF team Phase D golden RenderDoc captures (one profile per run: A, B, A_DC). Args: {plan: "<phase-d plan .json>", profile: "A"|"B"|"A_DC" (default plan x_profile), mo2: "G:\\\\Acherus", tools: "F:\\\\ai\\\\dds\\\\tools", owner_ok: true, dry_run: false, cell: "<override plan cell>", fixtures: "<root, default F:\\\\ai\\\\dds\\\\tools\\\\rdc-fixtures>", run_dir: "<default .superpowers/sdd/captures/golden-<profile>>", out: "<results .md>"}. dry_run runs only Preflight and returns the full plan without touching settings or launching the game.',
  phases: [
    { title: 'Preflight', detail: 'plan checks, CS version, disk, game not running; back up SettingsUser.json and apply the profile (dry_run: plan only)' },
    { title: 'Session', detail: 'launch via MO2, wait DevBench + shader compiles, coc, per group: place, frame, draw check, capture; qqq' },
    { title: 'Restore', detail: 'always: wait for exit, byte-restore settings (sha256), keep each .rdc as a fixture, copy the CS log' },
    { title: 'Analyse', detail: 'per capture, one at a time: p04 stage A/B + golden extractors, draws mapped to forms by position' },
    { title: 'Report', detail: 'fixture completeness per subject, probe/light answers, retests, restore status' },
  ],
}

// NIF-team fork of in-game-capture.js (main 93323c3) for Phase D golden captures
// (F:\ai\dds\nifparseref\plan\nif-edit-v2\phase-d-capture-requirements.md §3-§4, W1-W9).
//
// USAGE
//   Three runs, one game launch each (CS settings are read at boot):
//     1. {plan, profile: "A",    mo2, tools, owner_ok: true}   golden: P0 probe + A1-A6 (plan top level)
//     2. {plan, profile: "B",    mo2, tools, owner_ok: true}   as shipped: groups of x_profiles_other_runs.B
//     3. {plan, profile: "A_DC", mo2, tools, owner_ok: true}   A without DynamicCubemaps at boot
//   Always do a dry run first: add dry_run: true. It runs Preflight read-only and returns <run_dir>/plan.txt:
//   every settings edit (line, old -> new), every DevBench request per group (origin shown as 0,0,0), every capture.
//   If run 1's P0 probe shows no directional light, pass cell: "<x_cell_fallback[0]>" to every run.
//
// PLAN FORMAT (in-game-capture.js keys plus these x_ keys)
//   x_profile          name of the top-level profile (default "A")
//   x_cs_settings      overlay of SettingsUser.json sections/keys (nested objects); only listed keys are edited,
//                      missing keys are inserted. RenderDoc."Enable RenderDoc Capture" and Advanced."Frame Annotations"
//                      are always set true on top.
//   x_profiles_other_runs  {<profile>: {x_cs_settings: {...} | "<base> without <Feature> in Disable at Boot", groups: [labels]}}
//   x_cell_fallback    cells to use instead if the P0 probe finds no directional light (manual: args.cell)
//   groups[].x_pos / x_angle   per form, [x,y,z] relative to the coc marker / SetAngle degrees (default row at y+400)
//   groups[].x_camera  {rel:[x,y,z] from the marker, pitch, yaw} for camera drive (default rel [0,-150,120], 0.15, 0)
//   groups[].x_motion_keyframed  forms to SetMotionType 4 (keyframed) before positioning (MISC/Havok)
//   groups[].x_optional_if_s6_ok {form, tags, pos, angle}: added only when the x_s6_files mesh is on disk with its sha256
//   x_light            {anchor, fallback_ligh, fallback_pos}; default anchor 0009DA9C, LIGH 000E2174 at [0,250,120]
//   x_s6_files         files the owner places in mods\s6; checked, never written
//   x_extract, x_fixture_out, x_wait_shadercache: extractor list, extractor output root, W2 on/off
//   A group with empty forms and tags is a probe (W8); a question without "outcomes" is a fixture goal (W8).
//
// RESTORE GUARANTEES
//   - Preflight backs SettingsUser.json up to <run_dir> and records its sha256 before any edit; it refuses to
//     edit while the game runs, and refuses to start if an earlier run in the same run_dir never restored.
//   - Restore runs after the Session whatever happened (crash, agent death, thrown error). It never edits while
//     the game runs; it copies the backup back byte for byte and checks the sha256. Anything CS wrote to the file
//     during the run is discarded on purpose. Result: settings_restored.
//   - .rdc files are copied to <fixtures>\<cs-version>\<profile>\ (sha256 sidecar + captures.json); the Captures
//     copy is deleted only after the copy's sha256 matches.
// All file work is done by golden-capture-lib.mjs (same folder; workflow scripts have no filesystem).

const a = args || {}
if (!a.plan) throw new Error('golden-capture needs args.plan (path to the phase-d capture plan JSON)')
if (!a.mo2 || !a.tools) throw new Error('golden-capture needs args.mo2 (MO2 instance folder) and args.tools (tools folder with rdc-scripts and RenderDoc)')
const DRY = a.dry_run === true
if (!DRY && a.owner_ok !== true) throw new Error('golden-capture needs args.owner_ok: true (owner permission to edit CS settings for this run and enable RenderDoc capture)')
const PROFILE_ARG = a.profile || ''
const RUN_DIR = a.run_dir || `.superpowers/sdd/captures/golden-${PROFILE_ARG || 'default'}`
const OUT = a.out || `${RUN_DIR}/results.md`
const LIB = a.lib || '.claude/workflows/golden-capture-lib.mjs'
const MO2 = a.mo2
const TOOLS = a.tools
const node = (cmd) => `node ${LIB} ${cmd} --run-dir "${RUN_DIR}"`

const RULES = `Owner data under ${MO2} is read-only except what golden-capture-lib.mjs writes (SettingsUser.json via preflight/restore, Captures .rdc deletion via keep). Do not edit SettingsUser.json or touch the Captures folder any other way. Never write into ${MO2}\\mods. Never run \`python -\` heredocs (write a script file, run \`python -I file\`). Never send DevBench console \`help ... 4\` (stalls the main thread), \`tm\` (hides the overlay markers the scripts need), \`save\`/\`savegame\`, \`set timescale to 0\` or DevBench game setTimeScale/freeze. Never kill processes you did not start. A DevBench 504 is not fatal: poll GET /api/health until "frame" advances, then continue.`

// ---------------- Preflight (W1, W7, W8) ----------------
phase('Preflight')
const PRE_SCHEMA = {
  type: 'object',
  properties: {
    ok: { type: 'boolean' }, reason: { type: 'string' }, exit_code: { type: 'integer' },
    stdout_json: { type: 'string', description: 'the command stdout, verbatim' },
  },
  required: ['ok', 'reason', 'exit_code', 'stdout_json'],
}
const preCmd = `${node('preflight')} --plan "${a.plan}" --mo2 "${MO2}"${PROFILE_ARG ? ` --profile ${PROFILE_ARG}` : ''}${a.cell ? ` --cell "${a.cell}"` : ''}${a.fixtures ? ` --fixtures "${a.fixtures}"` : ''}${a.cs_mod ? ` --cs-mod "${a.cs_mod}"` : ''}${DRY ? ' --dry-run' : ''}`
const preRun = await agent(
  `Preflight for a golden capture run. ${RULES}
Run exactly this one command from the repository root (PowerShell or bash), once:
${preCmd}
It prints one JSON object. Return ok = that object's "ok", reason = its "reason" (or ""), exit_code, and stdout_json = the stdout verbatim. Do not retry, edit files or run anything else.`,
  { label: DRY ? 'preflight (dry run)' : 'preflight', phase: 'Preflight', schema: PRE_SCHEMA })
let pre = null
try { pre = preRun && JSON.parse(preRun.stdout_json) } catch (e) { pre = null }
if (!pre) {
  // Could have died after applying settings: Restore is a no-op when nothing was applied.
  const r = DRY ? null : await restoreSettingsOnly()
  return { stopped: 'preflight', reason: 'preflight agent returned nothing parseable', settings_restored: r ? !!r.restored : null }
}
if (DRY) {
  log(`Dry run: profile ${pre.profile}, cell ${pre.cell}, CS ${pre.cs_key}, ${pre.edits} settings edits, ${pre.planned_captures} captures; F: ${pre.free_gb_fixtures}/${pre.need_gb_fixtures} GB, captures drive ${pre.free_gb_captures}/${pre.need_gb_captures} GB`)
  if (pre.would_stop && pre.would_stop.length) log(`A real run would stop: ${pre.would_stop.join('; ')}`)
  log(`Full plan (settings edits, placements, commands, captures): ${RUN_DIR}/plan.txt; resolved: ${RUN_DIR}/resolved.json`)
  return { dry_run: true, plan_txt: `${RUN_DIR}/plan.txt`, resolved: `${RUN_DIR}/resolved.json`, preflight: pre }
}
if (!pre.ok) {
  const r = await restoreSettingsOnly()
  return { stopped: 'preflight', reason: pre.reason, preflight: pre, settings_restored: r ? !!r.restored : null }
}
log(`Preflight ok: profile ${pre.profile}, CS ${pre.cs_key}, ${pre.planned_captures} captures, ${pre.edits} settings edits applied, backup ${pre.backup}`)

// ---------------- Session (W2, W3, W4) ----------------
phase('Session')
const SESSION_SCHEMA = {
  type: 'object',
  properties: {
    launched: { type: 'boolean' }, game_exited: { type: 'boolean' }, crashed: { type: 'boolean' },
    cell: { type: 'string' }, origin: { type: 'array', items: { type: 'number' }, description: 'player position right after coc (the marker)' },
    shader_wait: { type: 'string', description: 'signal used for W2 and what it showed (tool fields, failedTasks, or log/frame-time settling)' },
    groups: { type: 'array', items: { type: 'object', properties: {
      label: { type: 'string' },
      refs: { type: 'array', items: { type: 'object', properties: { form: { type: 'string' }, ref: { type: 'string' }, model: { type: 'string' }, pos: { type: 'array', items: { type: 'number' } } }, required: ['form', 'ref', 'model', 'pos'] } },
      camera: { type: 'string', description: 'camera get result, compact' },
      light: { type: 'string', description: 'light anchor: logLP result and whether the LIGH fallback was used; empty if none' },
      draw_check: { type: 'string' },
    }, required: ['label', 'refs', 'camera', 'light', 'draw_check'] } },
    captures: { type: 'array', items: { type: 'object', properties: {
      group: { type: 'string' }, label: { type: 'string' }, file: { type: 'string', description: 'full path of the .rdc' }, frame: { type: 'integer' },
    }, required: ['group', 'label', 'file', 'frame'] } },
    notes: { type: 'string' },
  },
  required: ['launched', 'game_exited', 'crashed', 'cell', 'origin', 'shader_wait', 'groups', 'captures', 'notes'],
}
let session = null
try {
  // One agent drives the whole session: the steps share live game state (refs, camera, timing).
  session = await agent(
    `Drive one Skyrim golden-capture session (profile ${pre.profile}). ${RULES}
Read ${RUN_DIR}/resolved.json (groups, cell, captures_dir, port, setup_console) and ${RUN_DIR}/plan.txt first.
DevBench: http://127.0.0.1:${pre.port}; tools via POST /api/tool/<name> with JSON bodies. Fetch GET /api/tools once, save it to ${RUN_DIR}/devbench_tools.json, and confirm the parameter names used below.
1. Launch: \`Start-Process "${MO2}\\ModOrganizer.exe" -ArgumentList '"moshortcut://:SKSE"' -WorkingDirectory "${MO2}"\`. The game takes about 5 minutes and starts into a save. Poll /api/health until "frame" advances, then menu {"action":"list"} until "Loading Menu" is gone and inspect {"kind":"state"} says playerLoaded true. Accept (and describe) any messageBox.
2. W2 shader compiles: CS settings changed at boot, so shaders recompile. Before the first capture of every group wait until compiling is done. Signal, in order: (a) if devbench_tools.json has communityshaders.inspect with a shadercache kind, poll it until compiling is false and completedTasks == totalTasks (record failedTasks); (b) otherwise use both the CS log (CommunityShaders.log under Documents\\My Games\\Skyrim Special Edition*\\SKSE or ${MO2}\\overwrite; find it) not growing with "Compiling" lines for 20 s AND /api/health frame rate settled (three 5 s windows within 10% of each other). Record which signal you used and what it showed in shader_wait.
3. W3 origin: ALWAYS console "coc ${pre.cell}" (even if already in that cell: a save does not stand on the marker), wait for playerLoaded, then before anything else read the player position (inspect {"kind":"player"} or scene): that is the origin. Then console exec each of setup_console in order.
4. Groups in resolved.json order (risky ones are already last). For each group run:
   node ${LIB} steps --run-dir "${RUN_DIR}" --group <label> --origin <ox>,<oy>,<oz>
   and send its steps in order: "tool" = POST /api/tool/<tool> with "body", "$refN"/"$lightref" = the ref saved by the step with that "save", "wait_s" = sleep, "only_if" = only when that condition holds. Notes in the steps:
   - inspect refs after each placement: model must be the expected mesh; record the actual position. If the ref is an Actor, also papyrus Actor.SetDontMove [true].
   - Light anchor (W4): the logLP output (console read) must list a light on the anchor; also check po3_LightPlacer.log. If none, run the [if ...] LIGH fallback steps and record that in light.
   - Draw check: find the native png under ${MO2}\\overwrite, downscale it, look at it, and confirm every subject is on screen (objects outside the view only show up as shadow draws). If not, adjust the camera drive (record the change) and retry up to 3 times. A probe group (no forms) is just the empty view.
   - Captures: one communityshaders.capture request per capture. Wait until the new .rdc in the captures_dir appears and its size is stable for 5 s before the next request, except a capture marked consecutive, which is sent right after the previous request. Record each file's full path and the DevBench frame at request time. Label captures <group>_cap<n>.
   - Risky group: after placing, poll /api/health for 10 s; if the game died set crashed and stop (no retry).
5. Quit without saving: console exec "qqq". Wait until Get-Process SkyrimSE is gone (up to 3 min). If the game crashed at any point, stop driving, set crashed, and return what you have.
Return the cell, origin, per-group refs/camera/light/draw check, and every capture.`,
    { label: `session ${pre.profile}`, phase: 'Session', schema: SESSION_SCHEMA })
} catch (e) {
  log(`Session failed: ${e && e.message}`)
}

// ---------------- Restore (W1 restore, W6, W7 log copy) ----------------
phase('Restore')
const restore = await restoreSettingsOnly()
if (!restore || !restore.restored) log('WARNING: SettingsUser.json was NOT restored; restore it from the backup before the next launch')
const kept = []
const files = session && session.captures ? session.captures : []
const KEEP_SCHEMA = { type: 'object', properties: { results: { type: 'array', items: { type: 'object', properties: {
  file: { type: 'string' }, ok: { type: 'boolean' }, dst: { type: 'string' }, sha256: { type: 'string' }, source_deleted: { type: 'boolean' }, reason: { type: 'string' },
}, required: ['file', 'ok', 'dst', 'sha256', 'source_deleted', 'reason'] } }, cs_log: { type: 'string' }, other_rdc: { type: 'array', items: { type: 'string' } } }, required: ['results', 'cs_log', 'other_rdc'] }
const keepRes = await agent(
  `Keep the capture files of a golden run as fixtures. ${RULES}
First make sure Get-Process SkyrimSE is not running (poll up to 5 min; never kill it; if it still runs, return ok=false for every file with reason "game running").
For each capture below, run once and return its JSON fields (dst, sha256, source_deleted, ok, reason):
${files.map(c => `node ${LIB} keep --run-dir "${RUN_DIR}" --file "${c.file}" --group ${c.group} --label ${c.label} --frame ${c.frame}`).join('\n') || '(no captures)'}
Then find CommunityShaders.log (Documents\\My Games\\Skyrim Special Edition*\\SKSE or under ${MO2}\\overwrite) and run node ${LIB} keep --run-dir "${RUN_DIR}" --file "<log path>" --keep-source; return its dst as cs_log ("" if not found).
Finally list any other .rdc in the captures folder (${MO2}\\overwrite\\SKSE\\Plugins\\CommunityShaders\\Captures) not handled above (do not touch them).`,
  { label: 'keep captures', phase: 'Restore', schema: KEEP_SCHEMA })
for (const c of files) {
  const r = keepRes && keepRes.results.find(x => x.file === c.file)
  kept.push({ capture: c, keep: r || { ok: false, reason: 'no keep result' } })
  if (!r || !r.ok) log(`${c.label}: NOT kept (${r ? r.reason : 'no result'}); the original stays in Captures`)
}
if (!session || !session.launched) return { stopped: 'session', session, restore, kept, settings_restored: !!(restore && restore.restored) }

// ---------------- Analyse (W5, W9) ----------------
phase('Analyse')
// Env vars per golden_common.py (P04_CAPTURE, P04_CS_VERSION, P04_TAGS); it refuses G: paths, so the kept F: copy is read.
// shortcut: runner per script (qrenderdoc vs plain python) and golden_fixture/cslog_defines inputs are assumed until
// GOLDEN-EXTRACTORS.md (tools rdc-scripts folder) exists; the agent is told to prefer that file. Fix here only.
function extractorCommands(rdc, c, csLog) {
  const S = `${TOOLS}\\rdc-scripts`
  const QRD = `& '${TOOLS}\\RenderDoc_1.43_64\\qrenderdoc.exe' --python`
  return [
    `$env:P04_CAPTURE = '${rdc}'; $env:P04_CS_VERSION = '${pre.cs_key}'; $env:P04_TAGS = '${(pre.group_tags || {})[c.group] || '*'}'`,
    `$env:GOLDEN_PROFILE = '${pre.profile}'; $env:GOLDEN_GROUP = '${c.group}'; $env:GOLDEN_LABEL = '${c.label}'; $env:GOLDEN_REFS = '${RUN_DIR}\\refs_${c.group}.json'; $env:GOLDEN_CSLOG = '${csLog}'`,
    `${QRD} ${S}\\p04_stageA_enumerate.py`,
    `${QRD} ${S}\\p04_stageB_state.py`,
    `${QRD} ${S}\\p04_dispatch.py`,
    `${QRD} ${S}\\p04_stageB_extra.py`,
    `${QRD} ${S}\\p04_stageC_dump.py`,
    `${QRD} ${S}\\p04_shader_id.py`,
    `python -I ${S}\\cslog_defines.py`,
    `python -I ${S}\\golden_fixture.py`,
  ]
}
const ANALYSE_SCHEMA = {
  type: 'object',
  properties: {
    file: { type: 'string' }, out_dir: { type: 'string' },
    scripts: { type: 'array', items: { type: 'object', properties: { script: { type: 'string' }, ok: { type: 'boolean' }, note: { type: 'string' } }, required: ['script', 'ok', 'note'] } },
    subjects: { type: 'array', items: { type: 'object', properties: {
      tag: { type: 'string' }, form: { type: 'string' }, draw: { type: 'string', description: 'event, class, pass, hint, shape:index; or "missing" / "shadow-only"' },
      fixture_complete: { type: 'boolean' }, missing_files: { type: 'array', items: { type: 'string' } },
    }, required: ['tag', 'form', 'draw', 'fixture_complete', 'missing_files'] } },
    frame_facts: { type: 'string', description: 'SharedData Timer, FrameCount, InInterior, DirLightColor, DirectionalAmbient, NumLightNumShadowLight, PointLightColor[0], BufferDim' },
  },
  required: ['file', 'out_dir', 'scripts', 'subjects', 'frame_facts'],
}
const groupInfo = Object.fromEntries((session.groups || []).map(g => [g.label, g]))
const analysed = []
// Strictly one capture at a time: each is 1.2-1.9 GB and qrenderdoc needs several GB of RAM.
for (const k of kept) {
  const c = k.capture
  const rdc = k.keep.ok ? k.keep.dst : c.file
  const g = groupInfo[c.group] || { refs: [] }
  const r = await agent(
    `Turn one golden RenderDoc capture into fixtures. ${RULES}
Capture: ${rdc} (profile ${pre.profile}, group ${c.group}, label ${c.label}). Read-only input: never write next to it.
If F:\\ai\\dds\\tools\\rdc-scripts\\GOLDEN-EXTRACTORS.md exists, read it first and use its env vars/runners for these scripts; otherwise run as below. First write the placed refs of this group to ${RUN_DIR}\\refs_${c.group}.json: ${JSON.stringify(g.refs)} (origin ${JSON.stringify(session.origin)}).
Run in order in PowerShell, waiting for each qrenderdoc log to end with "done" (a missing script: record ok=false "not present" and go on):
${extractorCommands(rdc, c, keepRes && keepRes.cs_log ? keepRes.cs_log : '').join('\n')}
W9 tag -> form mapping: tags come from resolved.json group ${c.group}. When a tag matches draws of several forms (or cell furniture with the same shape name), map each draw by its World translation: world position = World matrix translation + the camera position (CS World is camera-relative; take the eye/PosAdjust from the same capture's cbuffers). Pick the placed ref within ~30 units; draws near no placed ref are cell clutter and are ignored. A tag found only in shadow/prepass draws is "shadow-only" (subject was off screen). If golden_fixture.py takes a mapping, give it ${RUN_DIR}\\refs_${c.group}.json.
Per tag report the main Lighting (or forward, hint 1) draw, whether its fixture folder is complete (state, tex, mesh, mrt crops, mask non-empty, composite), and the missing files. Report the frame facts too.`,
    { label: `analyse:${c.label}`, phase: 'Analyse', schema: ANALYSE_SCHEMA })
  analysed.push({ capture: c, kept: k.keep, result: r })
  if (r) { const bad = r.subjects.filter(s => !s.fixture_complete); if (bad.length) log(`${c.label}: incomplete: ${bad.map(s => `${s.tag} (${s.draw})`).join(', ')}`) }
}

// ---------------- Report ----------------
phase('Report')
const report = await agent(
  `Write the golden capture results to ${OUT}. Plain English, short sentences. Plan: ${a.plan}; resolved: ${RUN_DIR}/resolved.json.
Run: profile ${pre.profile}, CS ${pre.cs_key}, cell ${session.cell}, origin ${JSON.stringify(session.origin)}, settings edits ${pre.edits}, shader wait: ${session.shader_wait}.
Session: ${JSON.stringify({ crashed: session.crashed, notes: session.notes, groups: session.groups })}
Kept captures: ${JSON.stringify(kept.map(k => ({ label: k.capture.label, ...k.keep })))}
Per-capture results: ${JSON.stringify(analysed.map(x => ({ label: x.capture.label, group: x.capture.group, frame: x.capture.frame, ...(x.result || { error: 'analysis returned nothing' }) })))}
Sections:
1. Summary: per subject (tag, form, group) fixture complete yes/no; per capture the kept path and sha256.
2. Questions from the plan: fixture goals (no "outcomes") list missing tags/files per subject; questions with outcomes (P0 cell probe, point light) get the outcome per the plan. For P0: if DirLightColor is 0, say the next runs must pass args.cell = the plan's x_cell_fallback[0] (P0 is not analysed in-session). Time gaps use SharedData Timer, not the wall clock (the game stalls while an .rdc is written).
3. Retests: subjects missing, shadow-only, or incomplete, with what to change (camera, position, NPC instead of static).
4. Restore status: ${JSON.stringify(restore)}; any captures not kept: ${JSON.stringify(kept.filter(k => !k.keep.ok).map(k => k.capture.file))}; other .rdc left in Captures: ${JSON.stringify(keepRes ? keepRes.other_rdc : [])}.
Return the report path and one line per question.`,
  { label: 'report', phase: 'Report' })

return { report, profile: pre.profile, captures: files.length, kept: kept.filter(k => k.keep.ok).length, crashed: !!session.crashed, settings_restored: !!(restore && restore.restored) }

// Restore of the CS settings only; safe to call when nothing was applied (the lib then reports nothing_to_restore).
async function restoreSettingsOnly() {
  const R_SCHEMA = { type: 'object', properties: { game_running: { type: 'boolean' }, stdout_json: { type: 'string', description: 'the command stdout verbatim, or "" if not run' } }, required: ['game_running', 'stdout_json'] }
  const r = await agent(
    `Restore the CS settings after a golden capture run. ${RULES}
1. Wait until Get-Process SkyrimSE is not running (poll every 10 s, up to 5 min). Never kill it. If it still runs, return game_running=true and stdout_json "" without running anything else.
2. Run this once from the repository root:
${node('restore')}
Return game_running=false and its stdout verbatim. Do not edit any file yourself.`,
    { label: 'restore settings', phase: 'Restore', schema: R_SCHEMA })
  if (!r) return null
  let j = null
  try { j = JSON.parse(r.stdout_json) } catch (e) { j = null }
  if (!j) return { restored: false, game_running: r.game_running, reason: r.game_running ? 'game still running' : 'restore output unreadable' }
  return { ...j, restored: !!j.restored || !!j.nothing_to_restore }
}

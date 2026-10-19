export const meta = {
  name: 'golden-capture',
  description: 'Phase D golden captures: apply a CS settings profile, launch Skyrim via MO2 + DevBench, place subjects and camera relative to the coc marker, take RenderDoc captures, quit, restore settings byte-exact, keep the .rdc files as fixtures and run the golden extractors',
  whenToUse: 'NIF team Phase D golden RenderDoc captures (one profile per run: A, B, A_DC). Args: {plan: "<phase-d plan .json>", profile: "A"|"B"|"A_DC" (default plan x_profile), mo2: "G:\\\\Acherus", tools: "F:\\\\ai\\\\dds\\\\tools", owner_ok: true, dry_run: false, restore_only: false, cell: "<override plan cell>", journal: "<recovery journal, default F:\\\\ai\\\\dds\\\\tools\\\\rdc-fixtures\\\\.golden-recovery.json>", run_root: "<default .superpowers/sdd/captures/golden; each run gets a fresh <UTC time>-<run id> dir below it>", out: "<results .md, default in the run dir>"}. dry_run runs only Preflight and returns the full plan without touching settings or launching the game; restore_only only restores pending settings from the journal.',
  phases: [
    { title: 'Preflight', detail: 'plan checks, CS version, disk, journal, game not running; back up SettingsUser.json and apply the profile atomically (dry_run: plan only)' },
    { title: 'Session', detail: 'launch via MO2, wait DevBench + a positive shader-compile signal, coc, per group: place, frame, draw check, capture; qqq' },
    { title: 'Keep', detail: 'copy each .rdc to the fixture folder (sha256; Captures is never touched), copy the CS log' },
    { title: 'Restore', detail: 'always: wait for exit (up to 30 min), atomic byte-restore of settings from the journal backup (sha256)' },
    { title: 'Analyse', detail: 'per capture, one at a time: the x_extract stages, then draws mapped to placed refs' },
    { title: 'Report', detail: 'fixture completeness per subject, draw mapping, probe/light answers, retests, restore status' },
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
//   Any key of x_profiles_other_runs works the same way (A_RT retest, B_LOG / A_DC_LOG debug-log launches).
//   Always do a dry run first: add dry_run: true. It runs Preflight read-only and returns <its fresh run dir>/plan.txt:
//   every settings edit (line, old -> new), every DevBench request per group (origin shown as 0,0,0), every capture.
//   If run 1's P0 probe shows no directional light, pass cell: "<x_cell_fallback[0]>" to every run.
//   If a run ends with settings_restored: false, run {mo2, restore_only: true} once the game has exited.
//
// PLAN FORMAT (in-game-capture.js keys plus these x_ keys)
//   x_profile          name of the top-level profile (default "A")
//   x_cs_settings      overlay of SettingsUser.json sections/keys (nested objects); only listed keys are edited,
//                      missing keys are inserted. Always added on top: RenderDoc."Enable RenderDoc Capture",
//                      Advanced."Frame Annotations" = true. Log Level is not forced (debug level = CS developer mode = debug shaders).
//   x_profiles_other_runs  {<profile>: {x_cs_settings: {...} | "<base>" | "<base> without <Feature> in Disable at Boot",
//                      x_cs_settings_add: {...} merged on top (e.g. A_DC_LOG = A_DC + Log Level 1), groups: [labels]}}
//                      (A_RT = "A" with only the retest groups; A_DC_LOG = debug-log launch for P0, not a fixture)
//   x_origin_expected  [x,y,z] of the plan cell's coc marker; steps refuses an origin more than 2 units away (unsettled player)
//   groups[].x_disable_refs  cell refs (8 hex) disabled for that group only and enabled again after it (light owners
//                      that would fill a big subject's 7 light slots); every group also disables nearby NPCs for the session
//   x_cell_fallback    cells to use instead if the P0 probe finds no directional light (manual: args.cell)
//   groups[].x_pos / x_angle   per form, [x,y,z] relative to the coc marker / SetAngle degrees (default row at y+400)
//   groups[].x_camera  {rel:[x,y,z] from the marker, pitch, yaw} for camera drive (default rel [0,-150,120], 0.15, 0)
//   groups[].x_motion_keyframed  forms to SetMotionType 4 (keyframed) before positioning (MISC/Havok)
//   groups[].x_optional_if_s6_ok {form, tags, pos, angle}: added only when the x_s6_files mesh is on disk with its sha256
//   x_light            {anchor, fallback_ligh, fallback_pos}; default anchor 0009DA9C, LIGH 000E2174 at [0,250,120]
//   groups[].x_light_pos  [[x,y,z], ...] one PlaceAtMe LIGH each (the primary point-light method, placed every run);
//                      a group with the anchor but no x_light_pos gets fallback_pos. LightPlacer (logLP) is a diagnostic.
//   x_player_pos       [x,y,z] where the player is moved before each group, out of the camera's view (default [0,-600,0])
//   x_gamehour / x_gamehour_alternatives  sun hour for the setup console (default 12) and hours to try when subjects are in shadow
//   x_layout_yaw_deg / x_origin_shift  turn the whole plan frame (x_pos, x_angle Z, x_camera rel/yaw, light and player
//                      positions) clockwise by the heading, then shift it by [dx,dy,dz] world units from the marker
//   x_s6_files         files the owner places in mods\s6; checked, never written
//   x_keep_rdc         where kept .rdc go (per profile below it); default F:\ai\dds\tools\rdc-fixtures\<cs-version>
//   x_extract          stages to run: A, B, dispatch, C, shader_id, defines, fixture (default all; others rejected)
//   x_fixture_out      must equal the extractors' fixed output root rdc-out\golden\<cs-version> (checked)
//   x_shape_offsets    optional {form: {shapePrefix: [x,y,z]}} model offsets for W9; x_match_tol (default 60 units).
//                      W9 matches a draw by shape name when exactly one placed ref expects it; collided names
//                      need offsets for those forms or stay "ambiguous".
//   x_form_tags        optional {form: [shape prefixes]}; default: group tags paired with its forms by index when
//                      the counts match (light anchor excluded), else every tag for every form
//   A group with empty forms and tags is a probe (W8); a question without "outcomes" is a fixture goal (W8).
//
// RESTORE GUARANTEES
//   - Preflight takes an exclusive journal lock (<journal>.lock, created with 'wx'; a leftover lock is never broken
//     automatically), backs SettingsUser.json up next to the recovery journal (outside run dirs) and marks the
//     journal "applied" with a fresh run id before the first edit. Every run, whatever its profile or run dir,
//     refuses to start while the journal has a pending entry for the same SettingsUser.json. Settings, backup and
//     journal are written atomically (temp file, flush, rename).
//   - Restore runs in a finally block after Preflight, Session and Keep, whatever happened (agent failure, thrown
//     error, crash), but only for the run id this run's preflight reported: it never restores another run's entry.
//     It waits up to 30 min for the game to exit, never edits while it runs, copies the backup back byte for byte
//     and checks the sha256. If it cannot, or the preflight output was lost, the journal stays pending,
//     settings_restored is false/null, and only an explicit {mo2, restore_only: true} run restores it.
//     Anything CS wrote to the file during the run is discarded on purpose.
//   - Only .rdc files inside the Captures folder are kept: each is copied to the keep folder and its sha256 checked.
//     Nothing is ever deleted; the report lists each Captures file with its kept copy for the owner to clear.
// All file work is done by golden-capture-lib.mjs (same folder; workflow scripts have no filesystem).

const a = args || {}
if (!a.mo2) throw new Error('golden-capture needs args.mo2 (MO2 instance folder)')
const RESTORE_ONLY = a.restore_only === true
if (!RESTORE_ONLY && !a.plan) throw new Error('golden-capture needs args.plan (path to the phase-d capture plan JSON)')
if (!RESTORE_ONLY && !a.tools) throw new Error('golden-capture needs args.tools (tools folder with rdc-scripts and RenderDoc)')
const DRY = a.dry_run === true
if (DRY && RESTORE_ONLY) throw new Error('golden-capture: dry_run and restore_only cannot be combined (restore_only writes the settings file)')
if (!DRY && !RESTORE_ONLY && a.owner_ok !== true) throw new Error('golden-capture needs args.owner_ok: true (owner permission to edit CS settings for this run and enable RenderDoc capture)')
const PROFILE_ARG = a.profile || ''
if (a.run_dir) throw new Error('golden-capture: run_dir is not accepted; each run creates a fresh dir under run_root')
const RUN_ROOT = a.run_root || '.superpowers/sdd/captures/golden'
let RUN_DIR = null // the fresh dir preflight creates (never reused); set from its output
let OUT = a.out || null
const LIB = a.lib || '.claude/workflows/golden-capture-lib.mjs'
const JOURNAL = a.journal || 'F:\\ai\\dds\\tools\\rdc-fixtures\\.golden-recovery.json'
const MO2 = a.mo2
const TOOLS = a.tools
const node = (cmd) => `node ${LIB} ${cmd}`

const RULES = `Owner data under ${MO2} is read-only except what golden-capture-lib.mjs writes (SettingsUser.json via preflight/restore). Do not edit SettingsUser.json or delete anything yourself; never delete from the Captures folder. Never write into ${MO2}\\mods. Never run \`python -\` heredocs (write a script file, run \`python -I file\`). Never send DevBench console \`help ... 4\` (stalls the main thread), \`tm\` (hides the overlay markers the scripts need), \`save\`/\`savegame\`, \`set timescale to 0\` or DevBench game setTimeScale/freeze. Never kill processes you did not start. A DevBench 504 is not fatal: poll GET /api/health until "frame" advances, then continue.`

// Restore of the CS settings from the recovery journal. With a run id only that run's pending entry is restored
// (automatic cleanup); without one (restore_only) any pending entry. Safe when nothing is pending.
async function restoreSettings(runId, quitPort, pidFile) {
  const R_SCHEMA = { type: 'object', properties: { game_running: { type: 'boolean' }, stdout_json: { type: 'string', description: 'the command stdout verbatim, or "" if not run' } }, required: ['game_running', 'stdout_json'] }
  let r = null
  try {
    r = await agent(
      `Restore the CS settings after a golden capture run. ${RULES}
${quitPort && pidFile ? `0. This run's session did not finish, so a game it launched may still run with placed test refs and lights (never saved). Only if ${pidFile} exists AND Get-Process -Id <the id in it> is a running SkyrimSE whose StartTime.ToFileTimeUtc() equals the second number in it (the process this run launched; a reused id fails this), send once POST http://127.0.0.1:${quitPort}/api/tool/console with body {"action":"exec","command":"qqq"} (quit without saving; never save). Otherwise (no file, another id, or not running) send nothing: a Skyrim this run did not launch is the owner's. If DevBench does not answer, leave the game alone and go on.
` : ""}1. Wait until Get-Process SkyrimSE is not running: poll every 15 s for up to 30 minutes. Never kill it. If it still runs after 30 minutes, return game_running=true and stdout_json "" without running anything else.
2. Run this once from the repository root:
${node(`restore --mo2 "${MO2}" --journal "${JOURNAL}"${runId ? ` --run-id ${runId}` : ''}`)}
Return game_running=false and its stdout verbatim. Do not edit any file yourself.`,
      { label: 'restore settings', phase: 'Restore', schema: R_SCHEMA })
  } catch (e) { return { restored: false, reason: `restore agent failed: ${e && e.message}` } }
  if (!r) return { restored: false, reason: 'restore agent returned nothing' }
  let j = null
  try { j = JSON.parse(r.stdout_json) } catch (e) { j = null }
  if (!j) return { restored: false, game_running: r.game_running, reason: r.game_running ? 'game still running after 30 min; journal stays pending' : 'restore output unreadable' }
  return { ...j, restored: !!j.restored || !!j.nothing_to_restore }
}
const warnRestore = r => { if (!r || !r.restored) log(`WARNING: SettingsUser.json was NOT restored (${r ? r.reason : 'no result'}). The journal ${JOURNAL} keeps it pending: run golden-capture with {mo2, restore_only: true} after the game exits, before any launch.`) }

if (RESTORE_ONLY) {
  phase('Restore')
  const r = await restoreSettings()
  warnRestore(r)
  return { restore_only: true, restore: r, settings_restored: !!(r && r.restored) }
}

let pre = null, session = null, keepRes = null, restore = null, stopped = null, launchTried = false // launchTried: the Session agent (which launches the game) was started
try {
  // ---------------- Preflight (W1, W7, W8, W10) ----------------
  phase('Preflight')
  const PRE_SCHEMA = {
    type: 'object',
    properties: { exit_code: { type: 'integer' }, stdout_json: { type: 'string', description: 'the command stdout, verbatim' } },
    required: ['exit_code', 'stdout_json'],
  }
  const preCmd = node(`preflight --run-root "${RUN_ROOT}" --journal "${JOURNAL}" --plan "${a.plan}" --mo2 "${MO2}"${PROFILE_ARG ? ` --profile ${PROFILE_ARG}` : ''}${a.cell ? ` --cell "${a.cell}"` : ''}${a.keep_root ? ` --keep-root "${a.keep_root}"` : ''}${a.cs_mod ? ` --cs-mod "${a.cs_mod}"` : ''}${DRY ? ' --dry-run' : ''}`)
  const preRun = await agent(
    `Preflight for a golden capture run. ${RULES}
Run exactly this one command from the repository root (PowerShell or bash), once:
${preCmd}
It prints one JSON object. Return exit_code and stdout_json = the stdout verbatim. Do not retry, edit files or run anything else.`,
    { label: DRY ? 'preflight (dry run)' : 'preflight', phase: 'Preflight', schema: PRE_SCHEMA })
  try { pre = preRun && JSON.parse(preRun.stdout_json) } catch (e) { pre = null }
  if (pre && pre.extract_scripts && !pre.extract_scripts.some(s => s.stage === 'fixture')) log(`Note: x_extract (${pre.extract_scripts.map(s => s.stage).join(', ')}) has no "fixture" stage, so golden_fixture.py will not write state.json/manifest.json/completeness.json`)
  if (pre && pre.run_dir) { RUN_DIR = pre.run_dir; OUT = OUT || `${RUN_DIR}/results.md` }
  if (!pre) stopped = { stopped: 'preflight', reason: 'preflight agent returned nothing parseable' }
  else if (DRY) {
    log(`Dry run: profile ${pre.profile}, cell ${pre.cell}, CS ${pre.cs_key}, ${pre.edits} settings edits, ${pre.planned_captures} captures; keep ${pre.keep_dir}; F: ${pre.free_gb_fixtures}/${pre.need_gb_fixtures} GB, captures drive ${pre.free_gb_captures}/${pre.need_gb_captures} GB`)
    if (pre.would_stop && pre.would_stop.length) log(`A real run would stop: ${pre.would_stop.join('; ')}`)
    if (!pre.ok) log(`Dry run refused: ${pre.reason}`)
    log(`Full plan (settings edits, placements, commands, captures): ${RUN_DIR}/plan.txt; resolved: ${RUN_DIR}/resolved.json`)
    return { dry_run: true, plan_txt: `${RUN_DIR}/plan.txt`, resolved: `${RUN_DIR}/resolved.json`, preflight: pre }
  } else if (!pre.ok) stopped = { stopped: 'preflight', reason: pre.reason, preflight: pre }
  else {
    log(`Preflight ok: profile ${pre.profile}, CS ${pre.cs_key}, ${pre.planned_captures} captures, ${pre.edits} settings edits applied, backup ${pre.backup}`)

    // ---------------- Session (W2, W3, W4) ----------------
    phase('Session')
    const SESSION_SCHEMA = {
      type: 'object',
      properties: {
        launched: { type: 'boolean' }, game_exited: { type: 'boolean' }, crashed: { type: 'boolean' },
        compile_ready: { type: 'boolean', description: 'a positive shader-compile completion signal was seen before the first capture' },
        stop_reason: { type: 'string', description: '"" or why the session stopped early, e.g. "compile readiness unknown"' },
        cell: { type: 'string' }, origin: { type: 'array', items: { type: 'number' }, description: 'player position right after coc (the marker)' },
        shader_wait: { type: 'string', description: 'signal used for W2 and what it showed' },
        groups: { type: 'array', items: { type: 'object', properties: {
          label: { type: 'string' },
          refs: { type: 'array', items: { type: 'object', properties: { form: { type: 'string' }, ref: { type: 'string' }, model: { type: 'string' }, pos: { type: 'array', items: { type: 'number' } } }, required: ['form', 'ref', 'model', 'pos'] } },
          camera: { type: 'string', description: 'camera get result, compact JSON' },
          light: { type: 'string', description: 'placed LIGH refs and the inspect lights result (each subject has one within ~100 units?); anchor logLP result; empty if none' },
          draw_check: { type: 'string' },
        }, required: ['label', 'refs', 'camera', 'light', 'draw_check'] } },
        captures: { type: 'array', items: { type: 'object', properties: {
          group: { type: 'string' }, label: { type: 'string' }, file: { type: 'string', description: 'full path of the .rdc' }, frame: { type: 'integer' },
        }, required: ['group', 'label', 'file', 'frame'] } },
        notes: { type: 'string' },
      },
      required: ['launched', 'game_exited', 'crashed', 'compile_ready', 'stop_reason', 'cell', 'origin', 'shader_wait', 'groups', 'captures', 'notes'],
    }
    // One agent drives the whole session: the steps share live game state (refs, camera, timing).
    launchTried = true
    session = await agent(
      `Drive one Skyrim golden-capture session (profile ${pre.profile}). ${RULES}
Read ${RUN_DIR}/resolved.json (groups, cell, captures_dir, port, setup_console) and ${RUN_DIR}/plan.txt first.
DevBench: http://127.0.0.1:${pre.port}; tools via POST /api/tool/<name> with JSON bodies. Fetch GET /api/tools once, save it to ${RUN_DIR}/devbench_tools.json, and confirm the parameter names used below.
1. Launch: as soon as a new SkyrimSE process appears after the launch below, write "<Id> <StartTime.ToFileTimeUtc()>" to ${RUN_DIR}\\game_pid.txt (failure cleanup quits only that process). Only a process that started after this launch command counts. Never launch while a SkyrimSE already runs. \`Start-Process "${MO2}\\ModOrganizer.exe" -ArgumentList '"moshortcut://:SKSE"' -WorkingDirectory "${MO2}"\`. The game takes about 5 minutes and starts into a save. Poll /api/health until "frame" advances, then menu {"action":"list"} until "Loading Menu" is gone and inspect {"kind":"state"} says playerLoaded true. Accept (and describe) any messageBox.
2. W2 shader compiles need a POSITIVE completion signal; silence or a stable frame rate is not one. Accept either:
   (a) devbench_tools.json lists communityshaders.inspect with a shadercache kind: poll it until compiling is false and completedTasks + failedTasks == totalTasks (record failedTasks);
   (b) the CS log (CommunityShaders.log under Documents\\My Games\\Skyrim Special Edition*\\SKSE; only if the owner log level is debug, which this run no longer forces, so normally use (a)): a line "Compilation completed in" written after this launch and after the last "Compiling " line.
   Wait up to 45 minutes (a cleared shader cache recompiles everything). Before each group's first capture check again: if new "Compiling " lines appeared after the last "Compilation completed", wait for the next one (or the tool to report done). If no positive signal is available or it never comes, do not capture: set compile_ready=false and stop_reason "compile readiness unknown", go to step 5. Record the signal and what it showed in shader_wait.
3. W3 origin: ALWAYS console "coc ${pre.cell}" (even if already in that cell: a save does not stand on the marker), wait for playerLoaded, then before anything else read the player position (inspect {"kind":"scene"} position) every 0.5 s until z has changed by less than 0.5 units over 2 s (the player drops onto the floor after coc: run e718aa1a read z 9.7 units high and every subject floated). That settled position is the origin. If the steps command below refuses the origin ("had not settled"), re-read it and retry; never pass a value you did not read. Then console exec each of setup_console in order.
4. Groups in resolved.json order (risky ones are already last). For each group run:
   ${node(`steps --run-dir "${RUN_DIR}" --group <label> --origin <ox>,<oy>,<oz>`)}
   and send its steps in order, each after the previous one returned: "tool" = POST /api/tool/<tool> with "body"; when a step has "body_raw", send that text verbatim as the body instead (it writes whole-number Float args as 225.0: DevBench drops whole-number Floats, so SetAngle 225 left runs b40e2215/58f625f1/e718aa1a at 43.5 deg); "$refN"/"$lightN" = the ref saved by the step with that "save" (substitute it in body_raw too); "until" = repeat that request every 0.5 s (up to 10 s) until it returns that value; "wait_s" = sleep; "wait_rdc" = wait until the new .rdc in captures_dir exists and its size is stable for 5 s, and send nothing (no freecam off, no Disable) before that; "only_if" = only when that condition holds. Notes:
   - The inspect refs formType Actor step lists NPCs: disable (console "<formId>.disable") every listed actor that is not the player and not placed by this run within 2000 units of a subject or the camera, so none stands in frame (run e718aa1a: a guard over the A1 coffin). Never kill, moveto or save; the disable lasts only this session.
   - inspect refs after each placement: model must be the expected mesh; record the actual position. If the ref is an Actor, also papyrus Actor.SetDontMove [true].
   - Write ${RUN_DIR}/refs_<label>.json = [{"form","ref","pos":[x,y,z],"angle":[ax,ay,az]}] for the placed refs (actual positions) and ${RUN_DIR}/camera_<label>.json = the camera get result (JSON).
   - Point lights (W4): the LIGH steps run every time (primary method). Record the inspect lights result in light: every placed light listed, and each subject within ~100 units of one. The anchor's logLP is a LightPlacer diagnostic only: record what it shows; never ReloadLP or Disable/Enable the anchor.
   - The first step moves the player out of the camera's view; the draw check must show no player body.
   - Draw check: find the native png under ${MO2}\\overwrite, downscale it, look at it, and confirm every subject is on screen (objects outside the view only show up as shadow draws). If not, adjust the camera drive (record the change) and retry up to 3 times. A probe group (no forms) is just the empty view. Sun check (exterior cells): if the subjects stand in shadow (no direct sun on them or the floor around them), try each resolved.gamehour_alternatives hour in order with console "set gamehour to H", wait 5 s, re-check; keep the first lit hour for this and all later groups and record it. If none is lit, keep the plan hour and say so.
   - Captures: one communityshaders.capture request per capture, sent while the planned camera is still driven (camera get right before it must match camera_<label>.json). The wait_rdc step after it holds every later step, so the capture can never show the view after freecam off (run b40e2215 P0); a capture marked consecutive is sent right after the previous request. Record each file's full path and the DevBench frame at request time. Label captures <group>_cap<n>.
   - Risky group: after placing, poll /api/health for 10 s; if the game died set crashed and stop (no retry).
5. Quit without saving: console exec "qqq". Wait until Get-Process SkyrimSE is gone (up to 3 min). If the game crashed at any point, stop driving, set crashed, and return what you have.
Return the cell, origin, compile_ready, per-group refs/camera/light/draw check, and every capture.`,
      { label: `session ${pre.profile}`, phase: 'Session', schema: SESSION_SCHEMA })
    if (session && session.stop_reason) log(`Session stopped early: ${session.stop_reason}`)

    // ---------------- Keep (W6, W7 log copy) ----------------
    const files = session && session.captures ? session.captures : []
    if (files.length) {
      phase('Keep')
      const KEEP_SCHEMA = { type: 'object', properties: { results: { type: 'array', items: { type: 'object', properties: {
        file: { type: 'string' }, stdout_json: { type: 'string', description: 'the keep command stdout verbatim' },
      }, required: ['file', 'stdout_json'] } }, cs_log: { type: 'string', description: 'dst of the copied CS log, or ""' }, other_rdc: { type: 'array', items: { type: 'string' } } }, required: ['results', 'cs_log', 'other_rdc'] }
      keepRes = await agent(
        `Keep the capture files of a golden run as fixtures. ${RULES}
First make sure Get-Process SkyrimSE is not running (poll every 15 s, up to 30 min; never kill it; if it still runs, return no results).
For each capture below run its command once and return the stdout verbatim:
${files.map(c => node(`keep --run-dir "${RUN_DIR}" --file "${c.file}" --group ${c.group} --label ${c.label} --frame ${c.frame}`)).join('\n')}
Then find CommunityShaders.log (Documents\\My Games\\Skyrim Special Edition*\\SKSE, or under ${MO2}\\overwrite) and run once:
${node(`copy --run-dir "${RUN_DIR}" --file "<log path>"`)}
and return its "dst" as cs_log ("" if not found). Finally list any other .rdc in ${MO2}\\overwrite\\SKSE\\Plugins\\CommunityShaders\\Captures not handled above (do not touch them).`,
        { label: 'keep captures', phase: 'Keep', schema: KEEP_SCHEMA })
    }
  }
} finally {
  // Runs after any outcome above (agent failure or throw included), except the dry run, and only for the run id
  // this run's preflight reported (also on {ok:false} after applying). No run id = this run owns nothing.
  if (!DRY && pre && pre.run_id) { phase('Restore'); restore = await restoreSettings(pre.run_id, session && session.game_exited ? null : (launchTried ? pre.port : null), RUN_DIR && `${RUN_DIR}\\game_pid.txt`); warnRestore(restore) }
  else if (!DRY && !pre) log(`WARNING: preflight output was lost. If it applied settings, the journal ${JOURNAL} holds them pending: run golden-capture with {mo2, restore_only: true} after the game exits.`)
}
const settings_restored = restore ? !!restore.restored : null // null: this run never touched the settings
if (stopped) {
  if (!restore && /never restored/.test(stopped.reason || '')) log(`Settings of an earlier run are still pending in ${JOURNAL}: run golden-capture with {mo2, restore_only: true} after the game exits.`)
  return { ...stopped, restore, settings_restored }
}
if (!session || !session.launched) return { stopped: 'session', session, restore, settings_restored }

const base = f => String(f).split(/[\\/]/).pop().toLowerCase()
const kept = (session.captures || []).map(c => {
  const r = keepRes && keepRes.results.find(x => base(x.file) === base(c.file)) // the agent may return the basename
  let k = null
  try { k = r && JSON.parse(r.stdout_json) } catch (e) { k = null }
  if (!k || !k.ok) log(`${c.label}: NOT kept (${k ? k.reason : 'no result'}); the original stays in Captures`)
  return { capture: c, keep: k || { ok: false, reason: 'no keep result' } }
})

// ---------------- Analyse (W5, W9) ----------------
phase('Analyse')
// Env vars and run order per F:\ai\dds\tools\rdc-scripts\GOLDEN-EXTRACTORS.md; stages from the plan's x_extract.
function extractorCommands(rdc, c) {
  const S = `${TOOLS}\\rdc-scripts`
  const camFile = `${RUN_DIR}\\camera_${c.group}.json`
  return [
    `$env:P04_CAPTURE = '${rdc}'; $env:P04_TAGS = '${(pre.group_tags || {})[c.group] || '*'}'; $env:P04_CS_VERSION = '${pre.cs_key}'`,
    `$env:P04_PROFILE = '${pre.profile}'; $env:P04_CELL = '${session.cell}'; $env:P04_CAMERA = (Test-Path '${camFile}') ? (Get-Content -Raw '${camFile}') : ''`,
    `$env:P04_CS_LOG = '${keepRes && keepRes.cs_log ? keepRes.cs_log : ''}'; $env:P04_SETTINGS = '${pre.settings_applied_copy}'; $env:P04_CS_META = '${pre.cs_meta_path}'`,
    `$rd = '${TOOLS}\\RenderDoc_1.43_64\\qrenderdoc.exe'`,
    ...pre.extract_scripts.map(s => (s.runner === 'qrenderdoc' ? `Start-Process -Wait $rd -ArgumentList '--python','${S}\\${s.script}'` : `python -I ${S}\\${s.script}`)),
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
    drawmap: { type: 'string', description: 'map-draws stdout verbatim (file, matched/ambiguous/unmatched counts)' },
    frame_facts: { type: 'string', description: 'SharedData Timer, FrameCount, InInterior, DirLightColor, DirectionalAmbient, NumLightNumShadowLight, PointLightColor[0], BufferDim' },
  },
  required: ['file', 'out_dir', 'scripts', 'subjects', 'drawmap', 'frame_facts'],
}
const analysed = []
// Strictly one capture at a time: each is 1.2-1.9 GB and qrenderdoc needs several GB of RAM.
for (const k of kept) {
  const c = k.capture
  const rdc = k.keep.ok ? k.keep.dst : c.file
  const r = await agent(
    `Turn one golden RenderDoc capture into fixtures. ${RULES}
Capture: ${rdc} (profile ${pre.profile}, group ${c.group}, label ${c.label}). Read-only input. Extractor output: ${pre.extract_out}\\<capture stem>\\.
Run in PowerShell, in order, one at a time; each qrenderdoc script writes <name>.log in the output folder ending in "done" (errors: ERROR + traceback). A missing script: record ok=false "not present" and go on.
${extractorCommands(rdc, c).join('\n')}
W9 draw -> form mapping (golden_fixture.py has no input for it, so it is a separate file): for every Lighting or forward draw whose shape matches a tag of this group (P04_TAGS), take its world position = state.json World translation + the camera position of this capture (CS World is camera-relative; the eye/PosAdjust is in the same capture's cbuffers). Skinned draws have World null: use the root bone translation from cb_vs_b10.bin if you can, else leave the draw out and say so. Write ${RUN_DIR}\\draws_${c.label}.json = [{"event","shape","world":[x,y,z]}], where "shape" is the stage A identity "shapeName:shapeIndex" (just shapeName when shapeIndex is null), the same name as the subject folder; never drop the index (run b40e2215 A2 lost it, and three stairs draws then collided). Then run:
${node(`map-draws --run-dir "${RUN_DIR}" --group ${c.group} --draws "${RUN_DIR}\\draws_${c.label}.json" --label ${c.label}`)}
Use its matches to fill "form" per subject. Ambiguous or unmatched draws are not discarded: list them in the subject's draw text. A tag found only in shadow/prepass draws is "shadow-only" (subject was off screen).
Per tag report the main Lighting (or forward, hint 1) draw, whether its fixture is complete (completeness.json from golden_fixture.py, mask non-empty), and the missing files. Report the frame facts too.`,
    { label: `analyse:${c.label}`, phase: 'Analyse', schema: ANALYSE_SCHEMA })
  analysed.push({ capture: c, kept: k.keep, result: r })
  if (r) { const bad = r.subjects.filter(s => !s.fixture_complete); if (bad.length) log(`${c.label}: incomplete: ${bad.map(s => `${s.tag} (${s.draw})`).join(', ')}`) }
}

// ---------------- Report ----------------
phase('Report')
const report = await agent(
  `Write the golden capture results to ${OUT}. Plain English, short sentences. Plan: ${a.plan}; resolved: ${RUN_DIR}/resolved.json.
Run: profile ${pre.profile}, CS ${pre.cs_key}, cell ${session.cell}, origin ${JSON.stringify(session.origin)}, settings edits ${pre.edits}, shader wait: ${session.shader_wait} (compile_ready ${session.compile_ready}${session.stop_reason ? `, stopped: ${session.stop_reason}` : ''}).
Session: ${JSON.stringify({ crashed: session.crashed, notes: session.notes, groups: session.groups })}
Kept captures: ${JSON.stringify(kept.map(k => ({ label: k.capture.label, ...k.keep })))}
Per-capture results: ${JSON.stringify(analysed.map(x => ({ label: x.capture.label, group: x.capture.group, frame: x.capture.frame, ...(x.result || { error: 'analysis returned nothing' }) })))}
Sections:
1. Summary: per subject (tag, form, group) fixture complete yes/no; per capture the kept path and sha256.
2. Draw mapping: per capture the ${RUN_DIR}/drawmap_<label>.json counts, and every ambiguous or unmatched draw with its candidates (the fixture writer does not record this mapping; this report and those files do). A draw is matched by its shape name only when one placed ref expects that name and no other draw carries it; collided names (e.g. the A3 LoadScreen actor statics sharing EyesMale, MaleHeadIMF, HandMale_Big1stperson, or a tag shared with cell furniture) are confirmed only with x_shape_offsets for those forms and otherwise stay ambiguous: say which plan entries need x_shape_offsets (or x_form_tags).
3. Questions from the plan: fixture goals (no "outcomes") list missing tags/files per subject; questions with outcomes (P0 cell probe, point light) get the outcome per the plan. For P0: if DirLightColor is 0, say the next runs must pass args.cell = the plan's x_cell_fallback[0] (P0 is not analysed in-session). Time gaps use SharedData Timer, not the wall clock (the game stalls while an .rdc is written).
4. Retests: subjects missing, shadow-only, ambiguous or incomplete, with what to change (camera, position, NPC instead of static, x_shape_offsets).
5. Restore status: ${JSON.stringify(restore)}. Captures folder files the owner can delete now that they are kept (source -> kept copy, sha256): ${JSON.stringify(kept.filter(k => k.keep.ok).map(k => [k.capture.file, k.keep.dst, k.keep.sha256]))}; captures not kept: ${JSON.stringify(kept.filter(k => !k.keep.ok).map(k => k.capture.file))}; other .rdc left in Captures: ${JSON.stringify(keepRes ? keepRes.other_rdc : [])}.
Return the report path and one line per question.`,
  { label: 'report', phase: 'Report' })

return { report, profile: pre.profile, captures: kept.length, kept: kept.filter(k => k.keep.ok).length, crashed: !!session.crashed, compile_ready: !!session.compile_ready, settings_restored }

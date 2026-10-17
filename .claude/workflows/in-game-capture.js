export const meta = {
  name: 'in-game-capture',
  description: 'Drive Skyrim through MO2 + DevBench to place test meshes, take RenderDoc captures without a keyboard, quit, restore settings, and turn the captures into numbers',
  whenToUse: 'Answering engine-behaviour questions with real frames (shader/alpha/controller checks, preview golden values). Args: {plan: "<capture-plan .json>", mo2: "<MO2 instance folder>", tools: "<tools folder with the RenderDoc/DevBench guide>", out: "<results .md path>", owner_ok: true}. owner_ok must be true: it records that the owner allowed enabling CS RenderDoc capture for this run.',
  phases: [
    { title: 'Preflight', detail: 'plan sanity, game not running, mods enabled, disk, back up and enable CS capture settings' },
    { title: 'Session', detail: 'launch via MO2, wait for DevBench, per group: place, frame, check draws, capture; quit without saving' },
    { title: 'Restore', detail: 'confirm the game exited, restore CS settings, list the captures' },
    { title: 'Analyse', detail: 'p04 stage A/B per capture, one at a time' },
    { title: 'Report', detail: 'answer each question from the numbers; flag inconclusive groups for a retest' },
  ],
}

// Learned in the 2026-10-16 NIF-team session (its capture runbook and results) and the RenderDoc/DevBench agent guide in the
// tools folder (args.tools). The plan file is JSON:
// { "cell": "anse2StudioV2",                       // optional; coc there if the save starts elsewhere
//   "groups": [ { "label": "P1",
//                 "forms": ["000CC79E", "0200B3ED"], // load-order FormIDs of base forms whose mesh path holds the test NIF
//                 "tags": ["G1SSET_", "G1ASET_"],    // shape-name prefixes that must appear as draws (rename shapes in test NIFs)
//                 "spacing": 120, "settle_s": 3,
//                 "clips": [ { "form": "00094183", "clip": "SpecialIdle" } ],   // optional PlayGamebryoAnimation
//                 "captures": [ { "after_s": 0 }, { "after_s": 0, "consecutive": true }, { "after_s": 6 } ],
//                 "risky": false } ],               // risky groups (may crash the game) run last, alone
//   "questions": [ { "id": "G1", "groups": ["P1"], "read": "EmitColor of G1SCLR vs G1SSET between captures 1 and 3", "outcomes": "..." } ] }
// Test NIFs live in an enabled MO2 test mod at the mesh path of an otherwise unused form.

const a = args || {}
if (!a.plan) throw new Error('in-game-capture needs args.plan (path to the capture-plan JSON)')
if (a.owner_ok !== true) throw new Error('in-game-capture needs args.owner_ok: true (owner permission to enable CS RenderDoc capture for this run)')
const OUT = a.out || '.superpowers/sdd/captures/results.md'
if (!a.mo2 || !a.tools) throw new Error('in-game-capture needs args.mo2 (MO2 instance folder) and args.tools (folder with the RenderDoc/DevBench guide and p04 scripts)')
const MO2 = a.mo2
const CS_SETTINGS = `${MO2}\\overwrite\\SKSE\\Plugins\\CommunityShaders\\SettingsUser.json`
const CAPTURES = `${MO2}\\overwrite\\SKSE\\Plugins\\CommunityShaders\\Captures`
const TOOLS = a.tools

const RULES = `Owner data under ${MO2} is read-only except: the CS settings file (only the two RenderDoc keys, after a backup) and nothing else. Never write into ${CAPTURES}. Never run \`python -\` heredocs (write a script file, run \`python -I file\`). Never send DevBench console \`help ... 4\` (stalls the main thread), \`tm\` (hides the overlay markers the scripts need), \`save\`/\`savegame\`, \`set timescale to 0\` or DevBench game setTimeScale/freeze (controllers would stop). Never kill processes you did not start. A DevBench 504 is not fatal: poll GET /api/health until "frame" advances, then continue.`

phase('Preflight')
const pre = await agent(
  `Preflight for an in-game capture run. ${RULES}
Plan file: ${a.plan}. Read it and check it: every group has forms and tags; every question names existing groups; risky groups exist only if needed.
1. Get-Process SkyrimSE must NOT be running; if it is, stop with ok=false.
2. ${MO2}\\profiles\\<selected_profile from ModOrganizer.ini>\\modlist.txt: the test mod and +DevBench are enabled (report the test mod's line).
3. Free space on the drive of ${CAPTURES}: need about 1.5 GB per planned capture + 10 GB; report it.
4. Back up ${CS_SETTINGS} to the session scratchpad (report the backup path), then set ONLY "Enable RenderDoc Capture" and "Frame Annotations" to true with a regex, writing UTF-8 without BOM, keeping formatting. Re-read and confirm both are true.
5. DevBench port: ${MO2}\\overwrite\\SKSE\\Plugins\\devbench\\runtime.json (default 8920).`,
  { label: 'preflight', phase: 'Preflight', schema: {
    type: 'object',
    properties: {
      ok: { type: 'boolean' }, reason: { type: 'string' },
      backup: { type: 'string' }, port: { type: 'integer' }, free_gb: { type: 'number' },
      planned_captures: { type: 'integer' },
    },
    required: ['ok', 'reason', 'backup', 'port', 'free_gb', 'planned_captures'],
  } })
if (!pre || !pre.ok) return { stopped: 'preflight', reason: pre ? pre.reason : 'preflight agent returned nothing' }
log(`Preflight ok: ${pre.planned_captures} captures planned, ${pre.free_gb} GB free, backup ${pre.backup}`)

phase('Session')
// One agent drives the whole game session: the steps share live game state (refs, camera, timing) and cannot be split.
const session = await agent(
  `Drive one Skyrim capture session. ${RULES}
DevBench: http://127.0.0.1:${pre.port}; tools via POST /api/tool/<name> with JSON bodies (fetch GET /api/tools once and save it to the scratchpad to confirm parameter names). Plan: ${a.plan}.
1. Launch: \`Start-Process "${MO2}\\ModOrganizer.exe" -ArgumentList '"moshortcut://:SKSE"' -WorkingDirectory "${MO2}"\` (a running MO2 instance takes the request). The game takes about 5 minutes and starts straight into a save. Poll /api/health until it answers with an advancing "frame", then poll menu {"action":"list"} until "Loading Menu" is gone and inspect {"kind":"state"} says playerLoaded true. If a messageBox is open, describe and accept it. If the plan names a cell and inspect {"kind":"scene"} is elsewhere, console "coc <cell>" and wait for playerLoaded again.
2. Setup: console exec tai, tcai, tgm, "set gamehour to 12", "set timescale to 1". Read the player position from inspect scene. Check inspect {"kind":"mods"} load indexes match the plan's FormID prefixes.
3. Groups in plan order, risky groups last and only after every other capture file is complete:
   a. Place each form: papyrus call ObjectReference.PlaceAtMe self {"form":"0x14"} args [{"form":"0x<id>"},1,false,false]; record the returned ref; SetAngle [0,0,0]; SetPosition on a row centred on the player at y+400. Keep the row SHORT (spacing from the plan; at most ~6 objects visible per row; if more, use two rows) — wide rows put objects outside the view and only their shadows get captured (2026-10-16 lesson). Check each ref with inspect {"kind":"refs","formId":...}: model must be the expected mesh path.
   b. Frame: camera {"action":"freecam","on":true} (this is tfc), then camera {"action":"drive",...} so the whole row is in view, close enough to fill most of the frame.
   c. Draw check BEFORE capturing: capture {"kind":"native","allowNative":true,"checkpointId":"<label>_check","cleanup":true}, find the png under ${MO2}\\overwrite (resolve the path), downscale it, look at it, and confirm every object is on screen; adjust the camera and retry up to 3 times. Record what is visible.
   d. Clips: papyrus PlayGamebryoAnimation self <ref> args [clip,true,0.0]; record the returned bool.
   e. Captures: wait settle_s, then for each planned capture send communityshaders.capture {"kind":"renderdoc","frames":1}. ONE request per capture; wait until the new .rdc in ${CAPTURES} appears and its size is stable for 5 s before the next request (two requests sent back to back land 2 frames apart — use that deliberately only when the plan says consecutive). Record each file name and the DevBench frame at request time.
   f. Disable then Delete every placed ref (papyrus ObjectReference.Disable [false], Delete []), freecam off.
   g. Risky group: after placing, poll /api/health for 10 s; if the game died, record which form and stop the session (do not retry).
4. Quit without saving: console exec "qqq". Wait until Get-Process SkyrimSE is gone (up to 3 min).
Return every capture with its group, label, frame, file and what the draw check saw.`,
  { label: 'session', phase: 'Session', schema: {
    type: 'object',
    properties: {
      launched: { type: 'boolean' },
      game_exited: { type: 'boolean' },
      crashed_on: { type: 'string', description: 'form id of a risky item that crashed the game, or empty' },
      captures: { type: 'array', items: { type: 'object', properties: {
        group: { type: 'string' }, label: { type: 'string' }, file: { type: 'string' }, frame: { type: 'integer' },
        draw_check: { type: 'string' } }, required: ['group', 'label', 'file', 'frame', 'draw_check'] } },
      clips: { type: 'array', items: { type: 'object', properties: { form: { type: 'string' }, clip: { type: 'string' }, returned: { type: 'boolean' } }, required: ['form', 'clip', 'returned'] } },
      notes: { type: 'string' },
    },
    required: ['launched', 'game_exited', 'crashed_on', 'captures', 'clips', 'notes'],
  } })

phase('Restore')
// Always runs, even if the session failed: CS capture must not stay on.
const restore = await agent(
  `Restore after an in-game capture run. ${RULES}
1. Wait until Get-Process SkyrimSE is not running (poll up to 5 min; never kill it — if it is still running, report that and stop without editing).
2. In ${CS_SETTINGS} set "Enable RenderDoc Capture" and "Frame Annotations" back to false (regex, UTF-8 no BOM), or restore the backup ${pre.backup} if the file was otherwise unchanged. Re-read and confirm.
3. List the .rdc files in ${CAPTURES} newer than the backup's time, with sizes.`,
  { label: 'restore', phase: 'Restore', schema: {
    type: 'object',
    properties: { restored: { type: 'boolean' }, game_running: { type: 'boolean' }, files: { type: 'array', items: { type: 'string' } } },
    required: ['restored', 'game_running', 'files'],
  } })
if (!restore || !restore.restored) log('WARNING: CS RenderDoc settings were NOT restored; fix them before the next launch')
if (!session || !session.launched) return { stopped: 'session', session, restore }

phase('Analyse')
// Strictly one capture at a time: each is 1.2-1.9 GB and qrenderdoc needs several GB of RAM.
const analysed = []
for (const c of session.captures) {
  const r = await agent(
    `Turn one RenderDoc capture into numbers. ${RULES}
Capture: ${CAPTURES}\\${c.file} (group ${c.group}, label ${c.label}).
Run, waiting for each log to end with "done": set $env:P04_CAPTURE, then
& '${TOOLS}\\RenderDoc_1.43_64\\qrenderdoc.exe' --python ${TOOLS}\\rdc-scripts\\p04_stageA_enumerate.py
& '${TOOLS}\\RenderDoc_1.43_64\\qrenderdoc.exe' --python ${TOOLS}\\rdc-scripts\\p04_stageB_state.py
Output lands in ${TOOLS}\\rdc-out\\p04\\<capture stem>\\. Stage A lists only Lighting-class draws: if a plan tag is missing, also scan every draw class (shadow, prepass, Effect) and say which class each tag was found in. For every plan tag of this group, report per draw: class, pass, shapeName/index, and the cbuffer values the plan's questions read (EmitColor, TexcoordOffset, MaterialData, AlphaTestRef/AlphaTestRefRS, SharedData Timer, World translation, blend and depth state).`,
    { label: `analyse:${c.label}`, phase: 'Analyse', schema: {
      type: 'object',
      properties: { file: { type: 'string' }, out_dir: { type: 'string' }, tags_found: { type: 'array', items: { type: 'string' } },
        tags_missing: { type: 'array', items: { type: 'string' } }, values: { type: 'string', description: 'per tag, the numbers read, compact' } },
      required: ['file', 'out_dir', 'tags_found', 'tags_missing', 'values'],
    } })
  analysed.push({ capture: c, result: r })
  if (r && r.tags_missing.length) log(`${c.label}: tags missing from Lighting draws: ${r.tags_missing.join(', ')}`)
}

phase('Report')
const report = await agent(
  `Write the capture results to ${OUT}. Plain English, short sentences. Plan: ${a.plan}.
Session: ${JSON.stringify({ crashed_on: session.crashed_on, clips: session.clips, notes: session.notes, draw_checks: session.captures.map(c => [c.label, c.draw_check]) })}
Per-capture numbers: ${JSON.stringify(analysed.map(x => ({ label: x.capture.label, group: x.capture.group, frame: x.capture.frame, ...(x.result || { error: 'analysis returned nothing' }) })))}
Sections: Summary table (question, outcome, confidence), then per question: the numbers used (with capture labels and Δt from the Timer), the outcome per the plan's "outcomes", and what it means. Mark a question INCONCLUSIVE when its tags only appear in shadow/prepass draws, were missing, or the clip call returned false; list a retest for each (which forms, what to change). Also list the restore status: ${JSON.stringify(restore)}. Return the report path and one line per question.`,
  { label: 'report', phase: 'Report' })

return { report, captures: session.captures.length, crashed_on: session.crashed_on, settings_restored: !!(restore && restore.restored) }

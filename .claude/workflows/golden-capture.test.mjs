// Offline harness for golden-capture.js + golden-capture-lib.mjs. Never launches the game, never writes outside
// GOLDEN_TEST_DIR (default %TEMP%\golden-capture-test). Run: node .claude/workflows/golden-capture.test.mjs
// Optional: GOLDEN_TEST_SETTINGS = a COPY of the owner's SettingsUser.json, GOLDEN_TEST_PLAN = the phase-d plan JSON
// (both only read). Without them a small synthetic settings file and plan are used.
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import crypto from 'node:crypto'
import assert from 'node:assert/strict'
import { execSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import { planSettingsEdits, resolveProfile, validatePlan, groupSteps, resolveGroups, deepMerge, extractScripts, mapDraws, setupConsole, rawBody, checkOrigin } from './golden-capture-lib.mjs'

const HERE = path.dirname(fileURLToPath(import.meta.url))
const LIB = path.join(HERE, 'golden-capture-lib.mjs')
const T = process.env.GOLDEN_TEST_DIR || path.join(os.tmpdir(), 'golden-capture-test')
const sha = p => crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex')
let passed = 0
const ok = (name) => { passed++; console.log(`ok ${passed} - ${name}`) }

const SYNTH_SETTINGS = '{\n "Advanced": {\n  "Frame Annotations": false,\n  "Log Level": 2\n },\n "Disable at Boot": {\n  "UnifiedWater": true\n },\n "Empty": {},\n "RenderDoc": {\n  "Capture Frame Count": 1,\n  "Enable RenderDoc Capture": false\n },\n "Upscaling": {\n  "upscaleMethod": 3\n },\n "Version": "1-9-0-0"\n}'
const SYNTH_PLAN = {
  cell: 'XTestGrantInt', x_profile: 'A', x_cell_fallback: ['anse2StudioV2'],
  x_cs_settings: { Advanced: { 'Log Level': 1 }, 'Disable at Boot': { UnifiedWater: true, DynamicCubemaps: true, Skin: true }, Upscaling: { upscaleMethod: 0 } },
  x_profiles_other_runs: { B: { x_cs_settings: {}, groups: ['A1'] }, A_DC: { x_cs_settings: 'A without DynamicCubemaps in Disable at Boot', groups: ['A1'] } },
  groups: [
    { label: 'P0', forms: [], tags: [], settle_s: 2, captures: [{ after_s: 0 }] },
    { label: 'A1', forms: ['000EB09A', '000F4983'], tags: ['coffin', 'Core01'], x_pos: [[-60, 400, 0], [60, 400, 60]], x_angle: [[0, 0, 180], [0, 0, 180]], x_motion_keyframed: ['000F4983'], x_camera: { rel: [0, -150, 120], pitch: 0.15, yaw: 0 }, captures: [{ after_s: 0 }, { after_s: 0, consecutive: true }] },
    { label: 'A5', forms: ['000EB09A', '0009DA9C'], tags: ['coffin'], x_pos: [[0, 400, 0], [0, -250, 120]], x_angle: [[0, 0, 180], [0, 0, 0]], captures: [{ after_s: 0 }] },
  ],
  questions: [{ id: 'P0-cell', groups: ['P0'], outcomes: '...' }, { id: 'fix', groups: ['A1'] }],
}

// ---------- fake MO2 instance in the test dir ----------
fs.rmSync(T, { recursive: true, force: true })
const MO2 = path.join(T, 'mo2')
const CS = path.join(MO2, 'overwrite', 'SKSE', 'Plugins', 'CommunityShaders')
fs.mkdirSync(path.join(CS, 'Captures'), { recursive: true })
fs.mkdirSync(path.join(MO2, 'profiles', 'Nexus'), { recursive: true })
fs.mkdirSync(path.join(MO2, 'mods', 'CommunityShaders_AIO-test'), { recursive: true })
fs.writeFileSync(path.join(MO2, 'ModOrganizer.ini'), '[General]\nselected_profile=@ByteArray(Nexus)\n')
fs.writeFileSync(path.join(MO2, 'profiles', 'Nexus', 'modlist.txt'), '+s6\n+DevBench\n-CommunityShaders_AIO 0528\n+CommunityShaders_AIO-test\n')
fs.writeFileSync(path.join(MO2, 'mods', 'CommunityShaders_AIO-test', 'meta.ini'), '[General]\nversion=d2026.10.11.0\n')
const LIVE = path.join(CS, 'SettingsUser.json')
if (process.env.GOLDEN_TEST_SETTINGS) fs.copyFileSync(process.env.GOLDEN_TEST_SETTINGS, LIVE); else fs.writeFileSync(LIVE, SYNTH_SETTINGS)
const PLAN = path.join(T, 'plan.json')
if (process.env.GOLDEN_TEST_PLAN) fs.copyFileSync(process.env.GOLDEN_TEST_PLAN, PLAN); else fs.writeFileSync(PLAN, JSON.stringify(SYNTH_PLAN, null, 2))
const ORIG_SHA = sha(LIVE)
const ORIG_TEXT = fs.readFileSync(LIVE, 'utf8')
const plan = JSON.parse(fs.readFileSync(PLAN, 'utf8'))
console.log(`settings: ${process.env.GOLDEN_TEST_SETTINGS || 'synthetic'} (sha256 ${ORIG_SHA.slice(0, 12)}), plan: ${process.env.GOLDEN_TEST_PLAN || 'synthetic'}`)

const lib = (cmd, env = {}) => {
  try { return JSON.parse(execSync(`node "${LIB}" ${cmd}`, { encoding: 'utf8', env: { ...process.env, ...env } })) }
  catch (e) { return JSON.parse(e.stdout) }
}

// ---------- 1. settings edit planner ----------
{
  const t = '{\n "A": {\n  "x": 1,\n  "y": false\n },\n "E": {},\n "Z": "v"\n}'
  const { edits, result } = planSettingsEdits(t, { A: { x: 2, n: true }, E: { k: 1 }, New: { s: { q: 1 } } })
  assert.deepEqual(JSON.parse(result), { A: { x: 2, y: false, n: true }, E: { k: 1 }, Z: 'v', New: { s: { q: 1 } } })
  assert.equal(result, '{\n "A": {\n  "x": 2,\n  "y": false,\n  "n": true\n },\n "E": {\n  "k": 1\n },\n "Z": "v",\n "New": {\n  "s": {\n   "q": 1\n  }\n }\n}')
  assert.equal(edits.length, 4)
  assert.throws(() => planSettingsEdits(t, { Z: { a: 1 } }), /section/)
  assert.throws(() => planSettingsEdits(t, { A: 3 }), /section\/array/)
  assert.equal(planSettingsEdits(t, { A: { x: 1 } }).edits.length, 0) // equal value: no edit
  const crlf = t.replace(/\n/g, '\r\n')
  assert.ok(!/[^\r]\n/.test(planSettingsEdits(crlf, { A: { n: 1 } }).result), 'CRLF file keeps CRLF on inserts')
  ok('planSettingsEdits: replace, insert (section, empty object, new section), no-op, type clash, CRLF')
  // Review #5: several inserts into one existing empty object.
  const e = planSettingsEdits('{\n "Advanced": {},\n "Z": 1\n}', { Advanced: { 'Frame Annotations': true, 'Log Level': 1 }, Empty2: {} })
  assert.deepEqual(JSON.parse(e.result), { Advanced: { 'Frame Annotations': true, 'Log Level': 1 }, Z: 1, Empty2: {} })
  assert.equal(e.edits.length, 3)
  ok('planSettingsEdits: several keys into one empty object give valid JSON')
  // Review #6: inserted keys are JSON-escaped.
  const weird = { 'He said "hi"': 1, 'a\\b': 2, 'tab\there': 3 }
  const w = planSettingsEdits('{\n "S": {\n  "x": 0\n }\n}', { S: weird })
  assert.deepEqual(JSON.parse(w.result).S, { x: 0, ...weird })
  ok('planSettingsEdits: keys with quotes, backslashes and tabs are escaped')
}
{
  assert.deepEqual(extractScripts(['A', 'B', 'dispatch', 'C']).map(s => s.script), ['p04_stageA_enumerate.py', 'p04_stageB_state.py', 'p04_dispatch.py', 'p04_stageB_extra.py', 'p04_stageC_dump.py'])
  assert.equal(extractScripts(undefined).length, 8)
  assert.throws(() => extractScripts(['A', 'X']), /unsupported stage/)
  assert.throws(() => extractScripts(['A', 'dispatch', 'D']), /D needs "C"/)
  assert.deepEqual(extractScripts(['A', 'dispatch', 'C', 'D', 'fixture']).map(s => s.stage), ['A', 'dispatch', 'C', 'D', 'fixture'])
  assert.ok(!extractScripts(undefined).some(s => s.stage === 'D'), 'D is opt-in')
  assert.throws(() => extractScripts(['A', 'C']), /dispatch/)
  assert.deepEqual(extractScripts(['A']).map(s => s.stage), ['A'])
  ok('x_extract: stages map to scripts in run order; D opt-in after C; unknown stage and missing dispatch rejected')
  assert.ok(setupConsole().includes('set gamehour to 12'))
  assert.ok(setupConsole({ x_gamehour: 7.5 }).includes('set gamehour to 7.5'))
  for (const bad of [24, -1, '8', NaN]) assert.throws(() => setupConsole({ x_gamehour: bad }), /x_gamehour/)
  ok('x_gamehour: default 12, plan hour used, out-of-range or non-number rejected')
  // W9: identity by shape name; collisions need offsets.
  const refs = [
    { form: 'F1', ref: '0xFF1', pos: [-120, 400, 0], angle: [0, 0, 180], shapes: ['EyesMale', 'MaleHeadIMF'] },
    { form: 'F2', ref: '0xFF2', pos: [0, 400, 0], angle: [0, 0, 180], shapes: ['EyesMale', 'Off'] },
    { form: 'F3', ref: '0xFF3', pos: [120, 400, 0], angle: [0, 0, 180], shapes: ['coffin'] },
  ]
  const m = mapDraws([
    { event: 1, shape: 'MaleHeadIMF', world: [100, 395, 120] }, // unique name: matched by name even though nearer F3
    { event: 2, shape: 'EyesMale', world: [-118, 395, 120] }, // two refs expect it: no offsets -> ambiguous
    { event: 3, shape: 'coffin', world: [120, 400, 0] }, // F3's coffin ...
    { event: 4, shape: 'coffin', world: [900, 900, 0] }, // ... and cell furniture with the same name: both ambiguous
    { event: 5, shape: 'Rug', world: [0, 0, 0] }, // nobody expects it
  ], refs, { tol: 60 })
  assert.deepEqual(m.map(x => [x.status, x.by, x.form]), [['matched', 'name', 'F1'], ['ambiguous', null, null], ['ambiguous', null, null], ['ambiguous', null, null], ['unmatched', null, null]])
  assert.ok(m.slice(1).every(x => x.candidates.length), 'ambiguous/unmatched keep their candidates')
  {
    // Codex 4b4c02f P2: offsets turn clockwise like the layout. Refs at x 0 and 200, both Z 90, offset +Y 100 -> +X 100.
    const r90 = [{ form: 'R1', ref: 'a', pos: [0, 0, 0], angle: [0, 0, 90], shapes: ['S'] }, { form: 'R2', ref: 'b', pos: [200, 0, 0], angle: [0, 0, 90], shapes: ['S'] }]
    const m90 = mapDraws([{ event: 1, shape: 'S', world: [100, 0, 0] }], r90, { tol: 60, offsets: { R1: { S: [0, 100, 0] }, R2: { S: [0, 100, 0] } } })
    assert.deepEqual([m90[0].status, m90[0].form], ['matched', 'R1'], JSON.stringify(m90))
  }
  {
    // Codex aa95ee2 P2: case-folded offset prefixes that disagree must not give a confident match.
    const rr = [{ form: 'A1', ref: 'a', pos: [0, 0, 0], angle: [0, 0, 0], shapes: ['panel'] }, { form: 'B1', ref: 'b', pos: [300, 0, 0], angle: [0, 0, 0], shapes: ['panel'] }]
    const mm = mapDraws([{ event: 1, shape: 'panel:1', world: [0, 100, 0] }], rr, { tol: 60, offsets: { A1: { Panel: [0, 100, 0], panel: [0, -100, 0] }, B1: { panel: [0, 100, 0] } } })
    assert.notEqual(mm[0].form, 'A1', JSON.stringify(mm))
  }
  const mo = mapDraws([{ event: 6, shape: 'EyesMale', world: [-35, 315, 120] }], refs, { tol: 60, offsets: { F1: { EyesMale: [0, 0, 120] }, F2: { EyesMale: [35, 85, 120] } } })
  assert.deepEqual([mo[0].status, mo[0].by, mo[0].form], ['matched', 'offset', 'F2'])
  ok('mapDraws (W9): unique name matched by name; shared names and duplicate draws ambiguous; unexpected shape unmatched; collision resolved only with offsets')
  const two = [{ form: 'R0', ref: 'a', pos: [0, 0, 0], shapes: ['S'] }, { form: 'R120', ref: 'b', pos: [120, 0, 0], shapes: ['S'] }]
  for (const x of [85, 100]) {
    const cx = mapDraws([{ event: 9, shape: 'S', world: [x, 0, 0] }], two, { tol: 60 })
    assert.equal(cx[0].status, 'ambiguous', `X=${x}`); assert.deepEqual(cx[0].candidates.map(c => c.form).sort(), ['R0', 'R120'])
  }
  ok('mapDraws: refs at X=0 and X=120 both expecting the shape, draw at X=85 or X=100, no offsets -> ambiguous with both candidates')
  {
    // Run b40e2215: refs_*.json forms are 0x-prefixed, plan x_shape_offsets keys are bare 8-digit hex.
    const hx = [{ form: '0x0010BEA6', ref: 'a', pos: [0, 0, 0], angle: [0, 0, 0], shapes: ['EyesMale'] }, { form: '0x0010CDC0', ref: 'b', pos: [60, 0, 0], angle: [0, 0, 0], shapes: ['EyesMale'] }]
    const mh = mapDraws([{ event: 1, shape: 'EyesMale', world: [0, 0, 120] }], hx, { tol: 60, offsets: { '0010BEA6': { EyesMale: [0, 0, 120] }, '10cdc0': { EyesMale: [0, 0, 120] } } })
    assert.deepEqual([mh[0].status, mh[0].by, mh[0].form, mh[0].candidates[0].mode], ['matched', 'offset', '0x0010BEA6', '3d+offset'], JSON.stringify(mh))
  }
  ok('mapDraws: form ids normalised on both sides (0x prefix, case, padding) when looking up x_shape_offsets')
  {
    // Run b40e2215: plan tag "coffin", shape "Coffin"; collisions and offset prefixes ignore case too.
    const ci = [{ form: 'C1', ref: 'a', pos: [0, 0, 0], shapes: ['coffin'] }, { form: 'T1', ref: 'b', pos: [500, 0, 0], shapes: ['Object174'] }]
    const mc = mapDraws([{ event: 1, shape: 'Coffin', world: [0, 0, 0] }, { event: 2, shape: 'OBJECT174', world: [500, 0, 0] }, { event: 3, shape: 'object174', world: [2000, 0, 0] }], ci, { tol: 60, offsets: { T1: { object174: [0, 0, 0] } } })
    assert.deepEqual(mc.map(x => [x.status, x.by, x.form]), [['matched', 'name', 'C1'], ['matched', 'offset', 'T1'], ['ambiguous', null, null]], JSON.stringify(mc))
  }
  ok('mapDraws: shape/tag/offset matching is case-insensitive (coffin vs Coffin; Object174 name collision counted case-insensitively)')
}

// ---------- 2. profiles and plan checks (W1, W8) ----------
{
  const A = resolveProfile(plan, 'A'), B = resolveProfile(plan, 'B'), DC = resolveProfile(plan, 'A_DC')
  for (const p of [A, B, DC]) { assert.equal(p.overlay.RenderDoc['Enable RenderDoc Capture'], true); assert.equal(p.overlay.Advanced['Frame Annotations'], true) }
  assert.equal(A.overlay['Disable at Boot'].DynamicCubemaps, true)
  assert.ok(!('DynamicCubemaps' in DC.overlay['Disable at Boot']), 'A_DC drops DynamicCubemaps')
  assert.ok(!('Disable at Boot' in B.overlay), 'B edits only the capture keys')
  assert.deepEqual(B.groups.map(g => g.label), plan.x_profiles_other_runs.B.groups)
  assert.throws(() => resolveProfile(plan, 'NOPE'), /neither/)
  assert.deepEqual(validatePlan(plan, A.groups), [], 'probe group + fixture-goal question accepted')
  assert.ok(validatePlan({ groups: [{ label: 'X', forms: ['000EB09A'], tags: [], captures: [{}] }] }, [{ label: 'X', forms: ['000EB09A'], tags: [], captures: [{}] }]).length)
  ok('profiles A/B/A_DC resolve; probe group and fixture goals pass W8 checks; bad group rejected')
}

// ---------- 3. placement steps relative to the coc marker (W3, W4) ----------
{
  const gs = resolveGroups(plan, resolveProfile(plan, 'A').groups, [])
  const g = gs.find(x => x.forms.some(f => f.keyframed)) || gs.find(x => x.forms.length)
  const o1 = groupSteps(g, [0, 0, 0]), o2 = groupSteps(g, [1000, -2000, 50])
  const pos = s => s.filter(x => x.body && x.body.function === 'SetPosition').map(x => x.body.args)
  pos(o2).forEach((p, i) => assert.deepEqual(p, [pos(o1)[i][0] + 1000, pos(o1)[i][1] - 2000, pos(o1)[i][2] + 50]))
  const drive = s => s.find(x => x.body && x.body.action === 'drive').body
  assert.equal(drive(o2).x - drive(o1).x, 1000); assert.equal(drive(o2).pitch, drive(o1).pitch)
  if (g.forms.some(f => f.keyframed)) {
    const iMT = o1.findIndex(x => x.body && x.body.function === 'SetMotionType'), iPos = o1.findIndex((x, i) => i > iMT && x.body && x.body.function === 'SetPosition')
    assert.ok(iMT >= 0 && iPos > iMT, 'keyframed before SetPosition')
  }
  const lg = gs.find(x => x.forms.some(f => f.light_anchor))
  if (lg) assert.ok(groupSteps(lg, [0, 0, 0]).some(s => s.body && s.body.command === 'logLP'), 'light anchor checked with logLP')
  assert.equal(gs.filter(x => x.probe).every(x => groupSteps(x, [0, 0, 0]).filter(s => s.tool === 'communityshaders.capture').length === x.captures.length), true)
  ok('steps: identical framing for any origin, keyframed before positioning, logLP on the light anchor, probe captures')
}
{
  // Layout transform, player move, primary LIGH lights.
  const p = { x_layout_yaw_deg: 90, x_origin_shift: [1000, 0, 5], x_player_pos: [0, -600, 0], groups: [
    { label: 'G', forms: ['000EB09A', '0009DA9C'], tags: ['coffin'], x_pos: [[0, 400, 0], [100, 0, 120]], x_angle: [[0, 0, 180], [0, 0, 300]],
      x_camera: { rel: [0, -150, 120], pitch: 0.15, yaw: 0 }, x_light_pos: [[-50, 330, 100], [50, 330, 100]], captures: [{ after_s: 0 }] },
    { label: 'H', forms: ['0009DA9C'], tags: ['x'], captures: [{ after_s: 0 }] }] }
  const [g, h] = resolveGroups(p, p.groups, [])
  // yaw 90 clockwise: plan +Y (north) -> world +X (east), plan +X -> world -Y.
  assert.deepEqual(g.forms[0].rel, [1400, 0, 5]); assert.deepEqual(g.forms[1].rel, [1000, -100, 125])
  assert.deepEqual(g.forms[0].angle, [0, 0, 270]); assert.deepEqual(g.forms[1].angle, [0, 0, 30])
  assert.deepEqual(g.camera.rel, [850, 0, 125]); assert.equal(g.camera.yaw, 1.5708); assert.equal(g.camera.pitch, 0.15)
  assert.deepEqual(g.player, [400, 0, 5]); assert.deepEqual(g.lights, [[1330, 50, 105], [1330, -50, 105]])
  assert.deepEqual(h.lights, [[1250, 0, 125]], 'anchor without x_light_pos: one light at fallback_pos')
  assert.deepEqual(resolveGroups({ groups: [] }, [{ label: 'P', forms: [], tags: [], captures: [{}] }], [])[0].lights, [], 'probe: no lights')
  const s = groupSteps(g, [10, 20, 30])
  assert.deepEqual(s[0].body.self, { form: '0x14' }); assert.equal(s[0].body.function, 'SetPosition'); assert.deepEqual(s[0].body.args, [410, 20, 35])
  const placed = s.filter(x => x.save && x.save.startsWith('light'))
  assert.equal(placed.length, 2); assert.ok(placed.every(x => !x.only_if), 'lights are primary, not conditional')
  assert.ok(s.some(x => x.body && x.body.function === 'SetPosition' && x.body.self.form === '$light1' && x.body.args.join() === '1340,-30,135'))
  assert.equal(s.filter(x => x.body && x.body.function === 'Delete' && /^\$light/.test(x.body.self.form)).length, 2, 'every light deleted')
  assert.ok(s.some(x => x.body && x.body.kind === 'lights'), 'inspect lights recorded')
  assert.ok(!s.some(x => x.body && /ReloadLP/.test(x.body.command || '') ), 'never ReloadLP')
  const iCam = s.findIndex(x => x.body && x.body.action === 'drive')
  assert.ok(iCam > 0 && s.findIndex(x => x.save === 'light0') < iCam, 'player first, lights before the camera')
  ok('layout yaw/shift turns positions, angles, camera, player and lights; player moved first; LIGH lights primary and cleaned up')
}

{
  // Retest fixes (runs b40e2215, 58f625f1, e718aa1a).
  const p = { x_profile: 'A', x_cs_settings: { 'Disable at Boot': { DynamicCubemaps: true, Skin: true } },
    x_profiles_other_runs: { RT: { x_cs_settings: 'A', groups: ['K'] }, DCLOG: { x_cs_settings: 'A without DynamicCubemaps in Disable at Boot', x_cs_settings_add: { Advanced: { 'Log Level': 1 } }, groups: ['P'] } },
    groups: [{ label: 'P', forms: [], tags: [], captures: [{ after_s: 0 }] },
      { label: 'K', forms: ['000F4983'], tags: ['Core01'], x_pos: [[0, 400, 0]], x_angle: [[0, 0, 180]], x_motion_keyframed: ['000F4983'], x_disable_refs: ['0008287f'],
        x_light_pos: [[0, 330, 100]], captures: [{ after_s: 0 }, { after_s: 0, consecutive: true }] }] }
  const rt = resolveProfile(p, 'RT'), dl = resolveProfile(p, 'DCLOG')
  assert.deepEqual(rt.overlay['Disable at Boot'], p.x_cs_settings['Disable at Boot'], '"A" reuses the base overlay'); assert.deepEqual(rt.groups.map(g => g.label), ['K'])
  assert.ok(!('DynamicCubemaps' in dl.overlay['Disable at Boot'])); assert.equal(dl.overlay['Disable at Boot'].Skin, true)
  const dx = resolveProfile({ ...p, x_extract: ['A'], x_profiles_other_runs: { ...p.x_profiles_other_runs, D: { x_cs_settings: 'A', x_extract: ['A', 'dispatch', 'C', 'D'] } } }, 'D')
  assert.deepEqual(dx.extract, ['A', 'dispatch', 'C', 'D'], 'a profile x_extract overrides the plan'); assert.equal(rt.extract, undefined, 'else the plan x_extract')
  assert.equal(dl.overlay.Advanced['Log Level'], 1); assert.equal(dl.overlay.Advanced['Frame Annotations'], true)
  assert.throws(() => resolveProfile({ ...p, x_profiles_other_runs: { X: { x_cs_settings: 'B' } } }, 'X'), /not understood/)
  assert.deepEqual(validatePlan(p, p.groups), [])
  assert.ok(validatePlan(p, [{ ...p.groups[1], x_disable_refs: ['xyz'] }]).length, 'bad x_disable_refs id rejected')
  ok('profiles: "<base>" string reuses the overlay; x_cs_settings_add merges on top (A_DC_LOG)')

  const [, k] = resolveGroups(p, p.groups, []), [pr] = resolveGroups(p, p.groups.slice(0, 1), [])
  const s = groupSteps(k, [10, 20, -208]), at = f => s.findIndex(x => f(x))
  const fn = n => x => x.body && x.body.function === n
  // Float args survive DevBench: whole numbers carry a decimal point in body_raw.
  const ang = s.find(fn('SetAngle')); assert.match(ang.body_raw, /"args":\[0\.0,0\.0,180\.0\]/); assert.deepEqual(JSON.parse(ang.body_raw), ang.body)
  assert.match(s.find(x => fn('SetPosition')(x) && x.body.self.form === '$ref0').body_raw, /,-208\.0\]/)
  assert.ok(!s.find(fn('PlaceAtMe')).body_raw && !s.find(fn('SetMotionType')).body_raw, 'Int args stay as they are')
  assert.equal(rawBody({ args: [1.5, -2] }), '{"args":[1.5,-2.0]}')
  // Keyframed only after the 3D loaded, then placed, then checked.
  const i3d = at(fn('Is3DLoaded')), iMT = at(fn('SetMotionType')), iPos = at(x => fn('SetPosition')(x) && x.body.self.form === '$ref0')
  assert.ok(at(fn('PlaceAtMe')) < i3d && s[i3d].until === true && i3d < iMT && iMT < iPos, 'PlaceAtMe -> Is3DLoaded (until true) -> SetMotionType -> SetPosition')
  assert.ok(s[iPos + 1].wait_s && /tumbles/.test(s[iPos + 1].note), 'position re-checked after keyframing')
  // NPC sweep before placing; cell lights off for the group and back on after.
  const iNpc = at(x => x.body && x.body.formType === 'Actor')
  assert.ok(iNpc > 0 && iNpc < at(fn('PlaceAtMe')) && /Disable \[false\] with self \{form: <the entry reference formId, never its "base" id>\}[\s\S]*GetEnableParent/.test(s[iNpc].note), 'NPCs listed for disabling (by reference id, not base id) before placement')
  const self = (fn) => x => x.tool === 'papyrus' && x.body.function === fn && x.body.self.form === '0x0008287F'
  const iOff = at(self('Disable')), iOn = at(self('Enable'))
  assert.ok(iOff > 0 && iOff < at(x => x.body && x.body.action === 'drive') && iOn > at(x => x.tool === 'communityshaders.capture'), 'disable before the camera, enable after the captures')
  // Nothing changes the scene before the capture is on disk; consecutive captures stay back to back.
  const caps = s.map((x, i) => [x, i]).filter(([x]) => x.tool === 'communityshaders.capture').map(([, i]) => i)
  assert.equal(caps[1], caps[0] + 1, 'consecutive pair has no wait between'); assert.equal(s[caps[1] + 1].wait_rdc, 2, 'one wait_rdc for both files of the pair')
  const ps = groupSteps(pr, [0, 0, 0]), pc = ps.findIndex(x => x.tool === 'communityshaders.capture')
  assert.ok(ps[pc + 1].wait_rdc && ps.findIndex(x => x.body && x.body.action === 'freecam' && x.body.on === false) > pc + 1, 'P0: freecam off only after the .rdc is complete')
  // Unsettled origin refused (run e718aa1a read z -198.3 for a -208 marker).
  assert.match(checkOrigin([29288.988, 2687.362, -198.315], [29288.99, 2687.36, -208]), /not settled/)
  assert.equal(checkOrigin([29288.99, 2687.36, -207.99996], [29288.99, 2687.36, -208]), null); assert.equal(checkOrigin([1, 2, 3], null), null)
  assert.match(checkOrigin([29288.99, 2687.36, -205.996], [29288.99, 2687.36, -208]), /not settled/, '2.004 off is refused (no rounding)')
  assert.match(checkOrigin([29290.5, 2688.87, -208], [29288.99, 2687.36, -208]), /not settled/, '1.5 + 1.5 on two axes is 2.1 in 3D')
  // Shape names the engine gives skin parts start with a space; tags match them trimmed.
  const md = mapDraws([{ shape: ' (FE4CD828)[1]/ (FE4CD82E) [100%]', world: [0, 0, 0] }], [{ form: '000A2C8F', ref: 'r', pos: [0, 0, 0], shapes: ['(FE4CD828)'] }])
  assert.equal(md[0].status, 'matched')
  ok('steps: float body_raw, keyframed after Is3DLoaded, NPC sweep, x_disable_refs on/off, wait_rdc before freecam off; unsettled origin refused; leading-space shape names match')
}

// ---------- 4. preflight dry run writes nothing to the settings ----------
const RUN = path.join(T, 'run')
const FIX = path.join(T, 'fixtures')
const JOURNAL = path.join(T, 'journal', '.golden-recovery.json')
const TOOLS = path.join(T, 'tools')
let RUN_A = null // run dir created by the apply test
const preArgs = (run = RUN, profile = 'A') => `--plan "${PLAN}" --mo2 "${MO2}" --run-root "${run}" --journal "${JOURNAL}" --tools "${TOOLS}" --keep-root "${FIX}" --profile ${profile}`
const restoreCmd = `restore --mo2 "${MO2}" --journal "${JOURNAL}"`
const pending = () => { try { return Object.values(JSON.parse(fs.readFileSync(JOURNAL, 'utf8')).entries).some(e => e.state === 'applied') } catch { return false } }
{
  const r = lib(`preflight ${preArgs()} --dry-run`)
  assert.equal(r.ok, true, r.reason); assert.equal(r.dry_run, true)
  assert.equal(r.cs_key, 'cs-1.9.0-d2026.10.11.0')
  assert.equal(sha(LIVE), ORIG_SHA); assert.ok(!fs.existsSync(JOURNAL), 'dry run writes no journal')
  assert.ok(r.run_dir.startsWith(RUN) && fs.readFileSync(path.join(r.run_dir, 'plan.txt'), 'utf8').includes('communityshaders.capture'))
  const r2 = lib(`preflight ${preArgs()} --dry-run`)
  assert.notEqual(r2.run_dir, r.run_dir, 'every invocation gets a fresh run dir, dry runs too')
  assert.throws(() => execSync(`node "${LIB}" preflight --plan "${PLAN}" --mo2 "${MO2}" --run-dir "${r.run_dir}" --dry-run`, { stdio: 'pipe' }))
  ok(`preflight --dry-run: ${r.edits} edits planned, ${r.planned_captures} captures, settings untouched, plan.txt written`)
}
{
  // x_cs_feature_log: the profile's debug log reaches the Analyse env; a missing file is refused before anything runs.
  const withLog = (log) => { const p = path.join(T, `plan-featlog-${path.basename(log)}.json`); fs.writeFileSync(p, JSON.stringify({ ...JSON.parse(fs.readFileSync(PLAN, 'utf8')), x_cs_feature_log: { A: log } })); return p }
  const r = lib(`preflight ${preArgs().replace(PLAN, withLog(PLAN))} --dry-run`)
  assert.equal(r.ok, true, r.reason); assert.equal(r.cs_feature_log, PLAN)
  const bad = lib(`preflight ${preArgs().replace(PLAN, withLog(path.join(T, 'no-such.log')))} --dry-run`)
  assert.equal(bad.ok, false); assert.match(bad.reason, /x_cs_feature_log/)
  ok('preflight: x_cs_feature_log passed through; a missing log refused')
}
{
  // No machine path is built in: s6 paths are relative to --mo2 or rebased from x_mo2_root; --tools and --journal are required.
  const p = path.join(T, 'plan-s6.json')
  fs.writeFileSync(p, JSON.stringify({ ...JSON.parse(fs.readFileSync(PLAN, 'utf8')), x_mo2_root: 'X:\\OldMO2', x_s6_files: [{ path: 'mods\\s6\\rel.nif' }, { path: 'X:\\OldMO2\\mods\\s6\\abs.nif' }] }))
  const r = lib(`preflight ${preArgs().replace(PLAN, p)} --dry-run`)
  assert.equal(r.ok, true, r.reason)
  assert.deepEqual(r.s6, [`${path.join(MO2, 'mods', 's6', 'rel.nif')}: missing`, `${path.join(MO2, 'mods', 's6', 'abs.nif')}: missing`])
  assert.ok(r.keep_dir.startsWith(FIX) && r.extract_out.startsWith(path.join(TOOLS, 'rdc-out', 'golden')))
  assert.match(lib(`preflight ${preArgs().replace(` --tools "${TOOLS}"`, '')} --dry-run`).reason, /needs --tools/)
  assert.match(lib(`preflight ${preArgs().replace(` --journal "${JOURNAL}"`, '')} --dry-run`).reason, /needs --journal/)
  ok('preflight: s6 paths relative to --mo2 or rebased from x_mo2_root; --tools and --journal required')
}

// ---------- 5. game running: preflight refuses ----------
{
  const r = lib(`preflight ${preArgs()}`, { GOLDEN_GAME_PROCESS: 'node.exe' })
  assert.equal(r.ok, false); assert.match(r.reason, /game is running/); assert.equal(sha(LIVE), ORIG_SHA); assert.ok(!pending())
  ok('preflight refuses while the game process runs; settings untouched')
}

// ---------- 6. interrupted write: the atomic replace fails, live file intact, journal not pending ----------
{
  const r = lib(`preflight ${preArgs()}`, { GOLDEN_FAIL_RENAME: 'SettingsUser.json' })
  assert.equal(r.ok, false); assert.match(r.reason, /simulated rename failure/); assert.match(r.reason, /unchanged/)
  assert.equal(sha(LIVE), ORIG_SHA, 'live file byte-identical')
  assert.ok(!fs.readdirSync(path.dirname(LIVE)).some(f => f.includes('golden-tmp')), 'temp file removed')
  assert.ok(!pending(), 'journal not left pending')
  ok('interrupted settings write: live file intact, temp removed, journal not pending')
}

// ---------- 7. apply, cross-run refusal, byte-identical restore ----------
{
  const r = lib(`preflight ${preArgs()}`)
  assert.equal(r.ok, true, r.reason); assert.equal(r.applied, true)
  RUN_A = r.run_dir
  assert.equal(sha(r.backup), ORIG_SHA, 'backup is the original'); assert.ok(!r.backup.startsWith(RUN), 'backup outside the run dir')
  assert.ok(pending())
  const live = fs.readFileSync(LIVE)
  assert.notEqual(sha(LIVE), ORIG_SHA)
  assert.ok(!(live[0] === 0xef && live[1] === 0xbb), 'no BOM')
  const ov = resolveProfile(plan, 'A').overlay
  assert.equal(ov.Advanced['Log Level'], 1)
  assert.ok(!('Log Level' in (resolveProfile({ ...plan, x_cs_settings: {} }, 'A').overlay.Advanced || {})), 'capture keys do not force Log Level')
  assert.deepEqual(JSON.parse(live.toString('utf8')), deepMerge(JSON.parse(ORIG_TEXT.replace(/^\uFEFF/, '')), ov), 'only overlay keys changed')
  const before = ORIG_TEXT.split('\n'), after = live.toString('utf8').split('\n')
  const { edits } = planSettingsEdits(ORIG_TEXT, ov)
  const touched = new Set(edits.map(e => e.line))
  before.forEach((l, i) => { if (!touched.has(i + 1)) assert.ok(after.includes(l), `line ${i + 1} kept`) })
  assert.equal(after.length - before.length, edits.filter(e => e.before === null).length, 'one new line per inserted key')
  assert.equal(fs.readFileSync(path.join(RUN_A, 'SettingsUser.applied.json'), 'utf8'), live.toString('utf8'), 'applied copy for P04_SETTINGS')

  // Review #1: another profile in another run dir must refuse while A is pending (and not back up A's edits).
  const snapshot = d => Object.fromEntries(fs.readdirSync(d).map(n => [n, sha(path.join(d, n))]))
  const before5 = snapshot(RUN_A), dirs5 = fs.readdirSync(RUN)
  const other = lib(`preflight ${preArgs(RUN, 'B')}`)
  assert.deepEqual(snapshot(RUN_A), before5, 'refused run leaves the active run dir byte-identical'); assert.deepEqual(fs.readdirSync(RUN), dirs5, 'refused run creates no dir')
  assert.equal(other.ok, false); assert.match(other.reason, /never restored/)
  assert.equal(sha(r.backup), ORIG_SHA)

  const busy = lib(restoreCmd, { GOLDEN_GAME_PROCESS: 'node.exe' })
  assert.equal(busy.restored, false); assert.notEqual(sha(LIVE), ORIG_SHA); assert.ok(pending(), 'still pending while the game runs')

  const failed = lib(restoreCmd, { GOLDEN_FAIL_RENAME: 'SettingsUser.json' })
  assert.equal(failed.ok, false); assert.ok(pending(), 'interrupted restore stays pending')
  assert.deepEqual(JSON.parse(fs.readFileSync(LIVE, 'utf8')), JSON.parse(live.toString('utf8')), 'interrupted restore leaves a whole file')

  const wrong = lib(`${restoreCmd} --run-id not-this-run`)
  assert.equal(wrong.restored, false); assert.equal(wrong.not_owner, true); assert.ok(pending(), 'other run id does not restore')
  fs.writeFileSync(`${JOURNAL}.lock`, 'stale')
  const locked = lib(`${restoreCmd} --run-id ${r.run_id}`)
  assert.equal(locked.ok, false); assert.equal(locked.locked, true); assert.ok(pending())
  const lockedPre = lib(`preflight ${preArgs(path.join(T, 'run-lock'))}`)
  assert.equal(lockedPre.ok, false); assert.equal(lockedPre.locked, true)
  fs.rmSync(`${JOURNAL}.lock`)
  assert.ok(r.run_id, 'preflight reports its run id')
  const rs = lib(`${restoreCmd} --run-id ${r.run_id}`)
  assert.equal(rs.restored, true, rs.reason)
  assert.equal(sha(LIVE), ORIG_SHA, 'restore is byte-identical')
  assert.ok(Buffer.compare(fs.readFileSync(LIVE), Buffer.from(ORIG_TEXT, 'utf8')) === 0)
  assert.ok(!pending()); assert.ok(fs.existsSync(r.backup), 'backup kept')
  assert.equal(lib(restoreCmd).nothing_to_restore, true)
  ok(`apply ${edits.length} edits (only overlay keys, no BOM); other profile refused while pending (active run dir byte-identical, no new dir); other run id not restored; journal lock held -> restore and preflight refused, lock never broken; restore refused while running; interrupted restore stays pending; restore byte-identical (sha256 ${ORIG_SHA.slice(0, 12)})`)
}

// ---------- 7b. N4: B's view of the settings is taken under the lock ----------
{
  const a = lib(`preflight ${preArgs(path.join(T, 'run-n4a'))}`); assert.equal(a.ok, true, a.reason)
  assert.notEqual(sha(LIVE), ORIG_SHA)
  const { spawn } = await import('node:child_process')
  const b = new Promise(res => {
    const p = spawn(process.execPath, [LIB, 'preflight', '--plan', PLAN, '--mo2', MO2, '--run-root', path.join(T, 'run-n4b'), '--journal', JOURNAL, '--tools', TOOLS, '--keep-root', FIX, '--profile', 'B'], { env: { ...process.env, GOLDEN_TEST_DELAY_BEFORE_LOCK_MS: '2500' } })
    let out = ''; p.stdout.on('data', x => { out += x }); p.on('close', () => res(JSON.parse(out)))
  })
  await new Promise(r => setTimeout(r, 300)) // B is started and waiting before its lock
  assert.equal(lib(`${restoreCmd} --run-id ${a.run_id}`).restored, true) // A restores meanwhile
  const bres = await b
  assert.equal(bres.ok, true, bres.reason)
  assert.equal(sha(bres.backup), ORIG_SHA, "B's backup is the owner's original, not A's applied settings")
  assert.equal(lib(`${restoreCmd} --run-id ${bres.run_id}`).restored, true)
  assert.equal(sha(LIVE), ORIG_SHA)
  ok('N4: B delayed before the lock while A restores -> B reads settings under the lock; backup is the original; restore byte-identical')
}

// ---------- 8. keep: Captures .rdc only, distinct destination, delete after verify (W6) ----------
{
  const capDir = path.join(CS, 'Captures')
  const rdc = path.join(capDir, 'Skyrim_AE_test_frame1.rdc')
  fs.writeFileSync(rdc, crypto.randomBytes(1 << 20))
  const s = sha(rdc)
  const r = lib(`keep --run-dir "${RUN_A}" --file "${rdc}" --group A1 --label A1_cap0 --frame 1`)
  assert.equal(r.ok, true, r.reason); assert.equal(r.sha256, s); assert.equal(sha(r.dst), s)
  assert.ok(r.dst.startsWith(path.join(FIX, 'A')))
  assert.ok(fs.existsSync(rdc) && sha(rdc) === s, 'keep never deletes the Captures copy'); assert.equal(r.source_deleted, false)
  const again = lib(`keep --run-dir "${RUN_A}" --file "${rdc}"`); assert.equal(again.ok, true); assert.ok(fs.existsSync(rdc), 'second keep: same copy, source still there')
  assert.equal(JSON.parse(fs.readFileSync(path.join(path.dirname(r.dst), 'captures.json'), 'utf8'))[0].label, 'A1_cap0')
  // Review #3: a file outside Captures (even an .rdc), a non-.rdc, and the kept copy itself are refused, untouched.
  const s6 = path.join(MO2, 'mods', 's6', 'test.rdc'); fs.mkdirSync(path.dirname(s6), { recursive: true }); fs.writeFileSync(s6, 'x')
  const nif = path.join(capDir, 'test.nif'); fs.writeFileSync(nif, 'x')
  for (const f of [s6, nif, r.dst]) { const k = lib(`keep --run-dir "${RUN_A}" --file "${f}"`); assert.equal(k.ok, false, f); assert.ok(fs.existsSync(f), `${f} kept`) }
  // Same-path destination: point keep_dir at the Captures folder.
  const resolvedPath = path.join(RUN_A, 'resolved.json')
  const res = JSON.parse(fs.readFileSync(resolvedPath, 'utf8'))
  const rdc2 = path.join(capDir, 'Skyrim_AE_test_frame2.rdc'); fs.writeFileSync(rdc2, 'y')
  fs.writeFileSync(resolvedPath, JSON.stringify({ ...res, keep_dir: capDir }))
  const same = lib(`keep --run-dir "${RUN_A}" --file "${rdc2}"`)
  assert.equal(same.ok, false); assert.match(same.reason, /inside the Captures folder/); assert.ok(fs.existsSync(rdc2))
  // Destination aliasing: keep_dir is a junction that resolves into Captures.
  const alias = path.join(T, 'alias-keep'); fs.symlinkSync(capDir, alias, 'junction')
  fs.writeFileSync(resolvedPath, JSON.stringify({ ...res, keep_dir: alias }))
  const al = lib(`keep --run-dir "${RUN_A}" --file "${rdc2}"`)
  assert.equal(al.ok, false); assert.match(al.reason, /Captures/); assert.ok(fs.existsSync(rdc2))
  assert.equal(fs.readdirSync(capDir).filter(n => n.endsWith('.rdc')).length, 2, 'Captures unchanged')
  fs.writeFileSync(resolvedPath, JSON.stringify(res))
  const logf = path.join(T, 'CommunityShaders.log'); fs.writeFileSync(logf, 'x')
  const cp = lib(`copy --run-dir "${RUN_A}" --file "${logf}"`)
  assert.equal(cp.ok, true); assert.equal(cp.source_deleted, false); assert.ok(fs.existsSync(logf)); assert.ok(fs.existsSync(cp.dst))
  ok('keep: sha256-verified copy, never deletes; refuses outside-Captures, non-.rdc, already-kept, same-folder and junction-aliased destinations; copy never deletes')
}

// ---------- 9. the workflow script itself, with stubbed agents ----------
async function runWorkflow(args, { sessionThrows, preflightThrowsAfterApply }) {
  const src = fs.readFileSync(path.join(HERE, 'golden-capture.js'), 'utf8')
  assert.ok(!src.includes('\r'), 'LF only')
  const body = src.replace(/^export const meta/m, 'const meta')
  const AsyncFunction = (async () => {}).constructor
  const fn = new AsyncFunction('args', 'agent', 'log', 'phase', 'parallel', 'pipeline', 'budget', 'workflow', body)
  const calls = [], prompts = {}
  const runCmd = (prompt) => {
    const line = prompt.split('\n').find(l => l.startsWith('node '))
    let stdout
    try { stdout = execSync(line.replace(/^node \S+/, `node "${LIB}"`), { encoding: 'utf8' }) } catch (e) { stdout = e.stdout }
    return stdout
  }
  const agent = async (prompt, opts) => {
    calls.push(opts.label); prompts[opts.label] = prompt
    if (opts.label.startsWith('preflight')) {
      const out = runCmd(prompt)
      if (preflightThrowsAfterApply) throw new Error('preflight agent died after applying (stub)')
      return { exit_code: JSON.parse(out).ok ? 0 : 1, stdout_json: out }
    }
    if (opts.label.startsWith('session')) { if (sessionThrows) throw new Error('game crashed (stub)'); return null }
    if (opts.label === 'restore settings') return { game_running: false, stdout_json: runCmd(prompt) }
    if (opts.label === 'keep captures') return { results: [], cs_log: '', other_rdc: [] }
    throw new Error(`unexpected agent ${opts.label}`)
  }
  const logs = []
  let res, err
  try { res = await fn(args, agent, m => logs.push(m), () => {}, null, null, { total: null }, null) } catch (e) { err = e }
  return { res, err, calls, logs, prompts }
}
{
  const base = { plan: PLAN, mo2: MO2, tools: TOOLS, lib: LIB, keep_root: FIX, journal: JOURNAL, profile: 'A' }
  const d = await runWorkflow({ ...base, dry_run: true, run_root: path.join(T, 'wf-dry') }, {})
  assert.equal(d.res.dry_run, true); assert.deepEqual(d.calls, ['preflight (dry run)']); assert.equal(sha(LIVE), ORIG_SHA)
  ok(`workflow dry_run: only Preflight ran; returned ${path.basename(d.res.plan_txt)}`)
  await assert.rejects(runWorkflow({ ...base, run_root: path.join(T, 'wf-noperm') }, {}).then(x => { if (x.err) throw x.err }), /owner_ok/)

  const c = await runWorkflow({ ...base, owner_ok: true, run_root: path.join(T, 'wf-crash') }, { sessionThrows: true })
  assert.ok(c.err, 'session error propagates after the finally')
  assert.deepEqual(c.calls, ['preflight', 'session A', 'restore settings'])
  assert.equal(sha(LIVE), ORIG_SHA, 'settings byte-identical after a crashed session'); assert.ok(!pending())
  assert.match(c.prompts['restore settings'], /"command":"qqq"/, 'failed session: restore quits the game without saving first')
  assert.match(c.prompts['restore settings'], /game_pid\.txt exists AND Get-Process -Id[^\n]*StartTime/, 'qqq only for the process this run launched')
  ok('workflow: owner_ok required; session throws -> Restore quits the game (qqq) and still runs (finally), settings byte-identical')

  // The preflight agent dies after the lib applied the settings: output lost, so no automatic restore.
  const p = await runWorkflow({ ...base, owner_ok: true, run_root: path.join(T, 'wf-prefail') }, { preflightThrowsAfterApply: true })
  assert.ok(p.err); assert.deepEqual(p.calls, ['preflight']); assert.ok(pending()); assert.ok(p.logs.some(l => /restore_only/.test(l)))
  const pr = await runWorkflow({ mo2: MO2, journal: JOURNAL, lib: LIB, restore_only: true }, {})
  assert.equal(pr.res.settings_restored, true); assert.equal(sha(LIVE), ORIG_SHA); assert.ok(!pending())
  ok('workflow: preflight output lost after applying -> no automatic restore, restore_only advised and restores')

  // N1: a structured failure after applying ({ok:false} with the run id) is restored by its own run.
  process.env.GOLDEN_FAIL_RENAME = 'SettingsUser.applied'
  const n1 = await runWorkflow({ ...base, owner_ok: true, run_root: path.join(T, 'wf-n1') }, {})
  delete process.env.GOLDEN_FAIL_RENAME
  assert.equal(n1.res.stopped, 'preflight'); assert.match(n1.res.reason, /after applying settings/)
  assert.deepEqual(n1.calls, ['preflight', 'restore settings']); assert.equal(n1.res.settings_restored, true)
  assert.equal(sha(LIVE), ORIG_SHA); assert.ok(!pending())
  ok('workflow N1: {ok:false} after applying -> this run restores its own entry, byte-identical')

  // N2: run A pending; run B's preflight is refused and its output lost -> B must not restore A.
  const aRun = lib(`preflight ${preArgs(path.join(T, 'run-A2'))}`); assert.equal(aRun.ok, true)
  const appliedSha = sha(LIVE)
  const n2 = await runWorkflow({ ...base, owner_ok: true, profile: 'B', run_root: path.join(T, 'wf-n2') }, { preflightThrowsAfterApply: true })
  assert.deepEqual(n2.calls, ['preflight']); assert.ok(pending(), "A's entry still pending"); assert.equal(sha(LIVE), appliedSha, "A's settings untouched")
  assert.equal(lib(`${restoreCmd} --run-id ${aRun.run_id}`).restored, true); assert.equal(sha(LIVE), ORIG_SHA)
  ok("workflow N2: another run's refused/lost preflight never restores run A's settings")

  // N3: dry_run + restore_only is rejected before any agent call.
  const n3 = await runWorkflow({ mo2: MO2, journal: JOURNAL, lib: LIB, restore_only: true, dry_run: true }, {})
  assert.match(String(n3.err), /cannot be combined/); assert.deepEqual(n3.calls, [])
  ok('workflow N3: dry_run with restore_only rejected before any agent call')

  // A preflight refused by another run's pending entry must not restore it (that run may still be going).
  lib(`preflight ${preArgs(path.join(T, 'run-p'))}`) // leaves a pending entry
  assert.ok(pending())
  const q = await runWorkflow({ ...base, owner_ok: true, profile: 'B', run_root: path.join(T, 'wf-pending') }, {})
  assert.equal(q.res.stopped, 'preflight'); assert.match(q.res.reason, /never restored/); assert.equal(q.res.settings_restored, null)
  assert.deepEqual(q.calls, ['preflight']); assert.ok(pending(), 'earlier entry left pending'); assert.ok(q.logs.some(l => /restore_only/.test(l)))
  ok("workflow: preflight refused by another run's pending journal -> no restore of that run, restore_only advised")

  const ro = await runWorkflow({ mo2: MO2, journal: JOURNAL, lib: LIB, restore_only: true }, {})
  assert.equal(ro.res.settings_restored, true); assert.deepEqual(ro.calls, ['restore settings']); assert.equal(sha(LIVE), ORIG_SHA)
  ok('workflow restore_only: restores the pending journal entry')
}

console.log(`\n${passed} passed`)

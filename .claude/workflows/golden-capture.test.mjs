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
import { planSettingsEdits, resolveProfile, validatePlan, groupSteps, resolveGroups, deepMerge, extractScripts, mapDraws } from './golden-capture-lib.mjs'

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
  assert.throws(() => extractScripts(['A', 'D']), /unsupported stage/)
  assert.throws(() => extractScripts(['A', 'C']), /dispatch/)
  assert.deepEqual(extractScripts(['A']).map(s => s.stage), ['A'])
  ok('x_extract: stages map to scripts in run order; D and missing dispatch rejected')
  const refs = [{ form: 'F1', ref: '0xFF1', pos: [-120, 400, 0], angle: [0, 0, 180] }, { form: 'F2', ref: '0xFF2', pos: [0, 400, 0], angle: [0, 0, 180] }, { form: 'F3', ref: '0xFF3', pos: [120, 400, 0], angle: [0, 0, 180] }]
  const m = mapDraws([
    { event: 1, shape: 'EyesMale', world: [-118, 395, 120] }, // head height: XY match
    { event: 2, shape: 'EyesMale', world: [60, 400, 120] }, // halfway between F2 and F3
    { event: 3, shape: 'coffin', world: [900, 900, 0] }, // cell furniture
    { event: 4, shape: 'Off', world: [-35, 315, 0] }, // model offset 85 units: only matches with the offset
  ], refs, { tol: 60, offsets: { F2: { Off: [35, 85, 0] } } })
  assert.deepEqual(m.map(x => [x.status, x.form]), [['matched', 'F1'], ['ambiguous', null], ['unmatched', null], ['matched', 'F2']])
  assert.ok(m[1].candidates.length && m[2].candidates.length, 'ambiguous/unmatched keep their candidates')
  ok('mapDraws (W9): XY match, ambiguous and unmatched reported with candidates, model offset rotated by the ref angle')
  const cx = mapDraws([{ event: 9, shape: 'S', world: [85, 0, 0] }], [{ form: 'R0', ref: 'a', pos: [0, 0, 0] }, { form: 'R120', ref: 'b', pos: [120, 0, 0] }], { tol: 60 })
  assert.equal(cx[0].status, 'ambiguous'); assert.deepEqual(cx[0].candidates.map(c => c.form), ['R120', 'R0'])
  ok('mapDraws: refs at X=0 and X=120, shape at X=85, no offsets -> ambiguous with both candidates')
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

// ---------- 4. preflight dry run writes nothing to the settings ----------
const RUN = path.join(T, 'run')
const FIX = path.join(T, 'fixtures')
const JOURNAL = path.join(T, 'journal', '.golden-recovery.json')
const preArgs = (run = RUN, profile = 'A') => `--plan "${PLAN}" --mo2 "${MO2}" --run-dir "${run}" --journal "${JOURNAL}" --keep-root "${FIX}" --profile ${profile}`
const restoreCmd = `restore --mo2 "${MO2}" --journal "${JOURNAL}"`
const pending = () => { try { return Object.values(JSON.parse(fs.readFileSync(JOURNAL, 'utf8')).entries).some(e => e.state === 'applied') } catch { return false } }
{
  const r = lib(`preflight ${preArgs()} --dry-run`)
  assert.equal(r.ok, true, r.reason); assert.equal(r.dry_run, true)
  assert.equal(r.cs_key, 'cs-1.9.0-d2026.10.11.0')
  assert.equal(sha(LIVE), ORIG_SHA); assert.ok(!fs.existsSync(JOURNAL), 'dry run writes no journal')
  assert.ok(fs.readFileSync(path.join(RUN, 'plan.txt'), 'utf8').includes('communityshaders.capture'))
  ok(`preflight --dry-run: ${r.edits} edits planned, ${r.planned_captures} captures, settings untouched, plan.txt written`)
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
  assert.equal(sha(r.backup), ORIG_SHA, 'backup is the original'); assert.ok(!r.backup.startsWith(RUN), 'backup outside the run dir')
  assert.ok(pending())
  const live = fs.readFileSync(LIVE)
  assert.notEqual(sha(LIVE), ORIG_SHA)
  assert.ok(!(live[0] === 0xef && live[1] === 0xbb), 'no BOM')
  const ov = resolveProfile(plan, 'A').overlay
  assert.equal(ov.Advanced['Log Level'], 1)
  assert.deepEqual(JSON.parse(live.toString('utf8')), deepMerge(JSON.parse(ORIG_TEXT.replace(/^\uFEFF/, '')), ov), 'only overlay keys changed')
  const before = ORIG_TEXT.split('\n'), after = live.toString('utf8').split('\n')
  const { edits } = planSettingsEdits(ORIG_TEXT, ov)
  const touched = new Set(edits.map(e => e.line))
  before.forEach((l, i) => { if (!touched.has(i + 1)) assert.ok(after.includes(l), `line ${i + 1} kept`) })
  assert.equal(after.length - before.length, edits.filter(e => e.before === null).length, 'one new line per inserted key')
  assert.equal(fs.readFileSync(path.join(RUN, 'SettingsUser.applied.json'), 'utf8'), live.toString('utf8'), 'applied copy for P04_SETTINGS')

  // Review #1: another profile in another run dir must refuse while A is pending (and not back up A's edits).
  const other = lib(`preflight ${preArgs(path.join(T, 'run-B'), 'B')}`)
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
  ok(`apply ${edits.length} edits (only overlay keys, no BOM); other profile/run dir refused while pending; other run id not restored; journal lock held -> restore and preflight refused, lock never broken; restore refused while running; interrupted restore stays pending; restore byte-identical (sha256 ${ORIG_SHA.slice(0, 12)})`)
}

// ---------- 8. keep: Captures .rdc only, distinct destination, delete after verify (W6) ----------
{
  const capDir = path.join(CS, 'Captures')
  const rdc = path.join(capDir, 'Skyrim_AE_test_frame1.rdc')
  fs.writeFileSync(rdc, crypto.randomBytes(1 << 20))
  const s = sha(rdc)
  const r = lib(`keep --run-dir "${RUN}" --file "${rdc}" --group A1 --label A1_cap0 --frame 1`)
  assert.equal(r.ok, true, r.reason); assert.equal(r.sha256, s); assert.equal(sha(r.dst), s)
  assert.ok(r.dst.startsWith(path.join(FIX, 'A')))
  assert.ok(fs.existsSync(rdc) && sha(rdc) === s, 'keep never deletes the Captures copy'); assert.equal(r.source_deleted, false)
  const again = lib(`keep --run-dir "${RUN}" --file "${rdc}"`); assert.equal(again.ok, true); assert.ok(fs.existsSync(rdc), 'second keep: same copy, source still there')
  assert.equal(JSON.parse(fs.readFileSync(path.join(path.dirname(r.dst), 'captures.json'), 'utf8'))[0].label, 'A1_cap0')
  // Review #3: a file outside Captures (even an .rdc), a non-.rdc, and the kept copy itself are refused, untouched.
  const s6 = path.join(MO2, 'mods', 's6', 'test.rdc'); fs.mkdirSync(path.dirname(s6), { recursive: true }); fs.writeFileSync(s6, 'x')
  const nif = path.join(capDir, 'test.nif'); fs.writeFileSync(nif, 'x')
  for (const f of [s6, nif, r.dst]) { const k = lib(`keep --run-dir "${RUN}" --file "${f}"`); assert.equal(k.ok, false, f); assert.ok(fs.existsSync(f), `${f} kept`) }
  // Same-path destination: point keep_dir at the Captures folder.
  const resolvedPath = path.join(RUN, 'resolved.json')
  const res = JSON.parse(fs.readFileSync(resolvedPath, 'utf8'))
  const rdc2 = path.join(capDir, 'Skyrim_AE_test_frame2.rdc'); fs.writeFileSync(rdc2, 'y')
  fs.writeFileSync(resolvedPath, JSON.stringify({ ...res, keep_dir: capDir }))
  const same = lib(`keep --run-dir "${RUN}" --file "${rdc2}"`)
  assert.equal(same.ok, false); assert.match(same.reason, /inside the Captures folder/); assert.ok(fs.existsSync(rdc2))
  // Destination aliasing: keep_dir is a junction that resolves into Captures.
  const alias = path.join(T, 'alias-keep'); fs.symlinkSync(capDir, alias, 'junction')
  fs.writeFileSync(resolvedPath, JSON.stringify({ ...res, keep_dir: alias }))
  const al = lib(`keep --run-dir "${RUN}" --file "${rdc2}"`)
  assert.equal(al.ok, false); assert.match(al.reason, /Captures/); assert.ok(fs.existsSync(rdc2))
  assert.equal(fs.readdirSync(capDir).filter(n => n.endsWith('.rdc')).length, 2, 'Captures unchanged')
  fs.writeFileSync(resolvedPath, JSON.stringify(res))
  const logf = path.join(T, 'CommunityShaders.log'); fs.writeFileSync(logf, 'x')
  const cp = lib(`copy --run-dir "${RUN}" --file "${logf}"`)
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
  const calls = []
  const runCmd = (prompt) => {
    const line = prompt.split('\n').find(l => l.startsWith('node '))
    let stdout
    try { stdout = execSync(line.replace(/^node \S+/, `node "${LIB}"`), { encoding: 'utf8' }) } catch (e) { stdout = e.stdout }
    return stdout
  }
  const agent = async (prompt, opts) => {
    calls.push(opts.label)
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
  return { res, err, calls, logs }
}
{
  const base = { plan: PLAN, mo2: MO2, tools: 'F:\\ai\\dds\\tools', lib: LIB, keep_root: FIX, journal: JOURNAL, profile: 'A' }
  const d = await runWorkflow({ ...base, dry_run: true, run_dir: path.join(T, 'wf-dry') }, {})
  assert.equal(d.res.dry_run, true); assert.deepEqual(d.calls, ['preflight (dry run)']); assert.equal(sha(LIVE), ORIG_SHA)
  ok(`workflow dry_run: only Preflight ran; returned ${path.basename(d.res.plan_txt)}`)
  await assert.rejects(runWorkflow({ ...base, run_dir: path.join(T, 'wf-noperm') }, {}).then(x => { if (x.err) throw x.err }), /owner_ok/)

  const c = await runWorkflow({ ...base, owner_ok: true, run_dir: path.join(T, 'wf-crash') }, { sessionThrows: true })
  assert.ok(c.err, 'session error propagates after the finally')
  assert.deepEqual(c.calls, ['preflight', 'session A', 'restore settings'])
  assert.equal(sha(LIVE), ORIG_SHA, 'settings byte-identical after a crashed session'); assert.ok(!pending())
  ok('workflow: owner_ok required; session throws -> Restore still runs (finally), settings byte-identical')

  // The preflight agent dies after the lib applied the settings: output lost, so no automatic restore.
  const p = await runWorkflow({ ...base, owner_ok: true, run_dir: path.join(T, 'wf-prefail') }, { preflightThrowsAfterApply: true })
  assert.ok(p.err); assert.deepEqual(p.calls, ['preflight']); assert.ok(pending()); assert.ok(p.logs.some(l => /restore_only/.test(l)))
  const pr = await runWorkflow({ mo2: MO2, journal: JOURNAL, lib: LIB, restore_only: true }, {})
  assert.equal(pr.res.settings_restored, true); assert.equal(sha(LIVE), ORIG_SHA); assert.ok(!pending())
  ok('workflow: preflight output lost after applying -> no automatic restore, restore_only advised and restores')

  // N1: a structured failure after applying ({ok:false} with the run id) is restored by its own run.
  process.env.GOLDEN_FAIL_RENAME = 'SettingsUser.applied'
  const n1 = await runWorkflow({ ...base, owner_ok: true, run_dir: path.join(T, 'wf-n1') }, {})
  delete process.env.GOLDEN_FAIL_RENAME
  assert.equal(n1.res.stopped, 'preflight'); assert.match(n1.res.reason, /after applying settings/)
  assert.deepEqual(n1.calls, ['preflight', 'restore settings']); assert.equal(n1.res.settings_restored, true)
  assert.equal(sha(LIVE), ORIG_SHA); assert.ok(!pending())
  ok('workflow N1: {ok:false} after applying -> this run restores its own entry, byte-identical')

  // N2: run A pending; run B's preflight is refused and its output lost -> B must not restore A.
  const aRun = lib(`preflight ${preArgs(path.join(T, 'run-A2'))}`); assert.equal(aRun.ok, true)
  const appliedSha = sha(LIVE)
  const n2 = await runWorkflow({ ...base, owner_ok: true, profile: 'B', run_dir: path.join(T, 'wf-n2') }, { preflightThrowsAfterApply: true })
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
  const q = await runWorkflow({ ...base, owner_ok: true, profile: 'B', run_dir: path.join(T, 'wf-pending') }, {})
  assert.equal(q.res.stopped, 'preflight'); assert.match(q.res.reason, /never restored/); assert.equal(q.res.settings_restored, null)
  assert.deepEqual(q.calls, ['preflight']); assert.ok(pending(), 'earlier entry left pending'); assert.ok(q.logs.some(l => /restore_only/.test(l)))
  ok("workflow: preflight refused by another run's pending journal -> no restore of that run, restore_only advised")

  const ro = await runWorkflow({ mo2: MO2, journal: JOURNAL, lib: LIB, restore_only: true }, {})
  assert.equal(ro.res.settings_restored, true); assert.deepEqual(ro.calls, ['restore settings']); assert.equal(sha(LIVE), ORIG_SHA)
  ok('workflow restore_only: restores the pending journal entry')
}

console.log(`\n${passed} passed`)

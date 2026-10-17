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
import { planSettingsEdits, resolveProfile, validatePlan, groupSteps, resolveGroups, deepMerge } from './golden-capture-lib.mjs'

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
const preArgs = `--plan "${PLAN}" --mo2 "${MO2}" --run-dir "${RUN}" --fixtures "${FIX}" --profile A`
{
  const r = lib(`preflight ${preArgs} --dry-run`)
  assert.equal(r.ok, true, r.reason); assert.equal(r.dry_run, true)
  assert.equal(r.cs_key, 'cs-1.9.0-d2026.10.11.0')
  assert.equal(sha(LIVE), ORIG_SHA); assert.ok(!fs.existsSync(path.join(RUN, 'SettingsUser.backup.A.json')))
  assert.ok(fs.readFileSync(path.join(RUN, 'plan.txt'), 'utf8').includes('communityshaders.capture'))
  ok(`preflight --dry-run: ${r.edits} edits planned, ${r.planned_captures} captures, settings untouched, plan.txt written`)
}

// ---------- 5. game running: preflight and restore refuse ----------
{
  const r = lib(`preflight ${preArgs}`, { GOLDEN_GAME_PROCESS: 'node.exe' })
  assert.equal(r.ok, false); assert.match(r.reason, /game is running/); assert.equal(sha(LIVE), ORIG_SHA)
  ok('preflight refuses while the game process runs; settings untouched')
}

// ---------- 6. apply, then byte-identical restore ----------
{
  const r = lib(`preflight ${preArgs}`)
  assert.equal(r.ok, true, r.reason); assert.equal(r.applied, true)
  assert.equal(sha(r.backup), ORIG_SHA, 'backup is the original')
  const live = fs.readFileSync(LIVE)
  assert.notEqual(sha(LIVE), ORIG_SHA)
  assert.ok(!(live[0] === 0xef && live[1] === 0xbb), 'no BOM')
  const ov = resolveProfile(plan, 'A').overlay
  assert.deepEqual(JSON.parse(live.toString('utf8')), deepMerge(JSON.parse(ORIG_TEXT.replace(/^\uFEFF/, '')), ov), 'only overlay keys changed')
  // Byte check: every original line survives except replaced values / lines that gained a trailing comma.
  const before = ORIG_TEXT.split('\n'), after = live.toString('utf8').split('\n')
  const { edits } = planSettingsEdits(ORIG_TEXT, ov)
  const touched = new Set(edits.map(e => e.line))
  before.forEach((l, i) => { if (!touched.has(i + 1)) assert.ok(after.includes(l), `line ${i + 1} kept`) })
  assert.equal(after.length - before.length, edits.filter(e => e.before === null).length, 'one new line per inserted key')

  const again = lib(`preflight ${preArgs}`)
  assert.equal(again.ok, false); assert.match(again.reason, /never restored/)
  assert.equal(sha(r.backup), ORIG_SHA, 'unrestored backup not overwritten')

  const busy = lib(`restore --run-dir "${RUN}"`, { GOLDEN_GAME_PROCESS: 'node.exe' })
  assert.equal(busy.restored, false); assert.notEqual(sha(LIVE), ORIG_SHA)

  const rs = lib(`restore --run-dir "${RUN}"`)
  assert.equal(rs.restored, true, rs.reason)
  assert.equal(sha(LIVE), ORIG_SHA, 'restore is byte-identical')
  assert.ok(Buffer.compare(fs.readFileSync(LIVE), Buffer.from(ORIG_TEXT, 'utf8')) === 0)
  assert.equal(lib(`restore --run-dir "${RUN}"`).already, true)
  ok(`apply ${edits.length} edits (only overlay keys, no BOM), second run refused, restore refused while running, restore byte-identical (sha256 ${ORIG_SHA.slice(0, 12)})`)
}

// ---------- 7. keep: copy, verify, then delete the Captures copy (W6) ----------
{
  const rdc = path.join(CS, 'Captures', 'Skyrim_AE_test_frame1.rdc')
  fs.writeFileSync(rdc, crypto.randomBytes(1 << 20))
  const s = sha(rdc)
  const r = lib(`keep --run-dir "${RUN}" --file "${rdc}" --group A1 --label A1_cap0 --frame 1`)
  assert.equal(r.ok, true, r.reason); assert.equal(r.sha256, s); assert.equal(sha(r.dst), s)
  assert.ok(r.dst.startsWith(path.join(FIX, 'cs-1.9.0-d2026.10.11.0', 'A')))
  assert.ok(!fs.existsSync(rdc), 'source deleted after verify')
  assert.equal(JSON.parse(fs.readFileSync(path.join(path.dirname(r.dst), 'captures.json'), 'utf8'))[0].label, 'A1_cap0')
  const logf = path.join(T, 'CommunityShaders.log'); fs.writeFileSync(logf, 'x')
  assert.equal(lib(`keep --run-dir "${RUN}" --file "${logf}" --keep-source`).source_deleted, false); assert.ok(fs.existsSync(logf))
  ok('keep: copy verified by sha256, Captures copy deleted only then, manifest written, --keep-source keeps the log')
}

// ---------- 8. the workflow script itself, with stubbed agents ----------
async function runWorkflow(args, { sessionThrows }) {
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
    if (opts.label.startsWith('preflight')) { const out = runCmd(prompt); const j = JSON.parse(out); return { ok: j.ok, reason: j.reason || '', exit_code: j.ok ? 0 : 1, stdout_json: out } }
    if (opts.label.startsWith('session')) { if (sessionThrows) throw new Error('game crashed (stub)'); return null }
    if (opts.label === 'restore settings') return { game_running: false, stdout_json: runCmd(prompt) }
    if (opts.label === 'keep captures') return { results: [], cs_log: '', other_rdc: [] }
    throw new Error(`unexpected agent ${opts.label}`)
  }
  const logs = []
  const res = await fn(args, agent, m => logs.push(m), () => {}, null, null, { total: null }, null)
  return { res, calls, logs }
}
{
  const base = { plan: PLAN, mo2: MO2, tools: 'F:\\ai\\dds\\tools', lib: LIB, fixtures: FIX, profile: 'A' }
  const d = await runWorkflow({ ...base, dry_run: true, run_dir: path.join(T, 'wf-dry') }, {})
  assert.equal(d.res.dry_run, true); assert.deepEqual(d.calls, ['preflight (dry run)']); assert.equal(sha(LIVE), ORIG_SHA)
  ok(`workflow dry_run: only Preflight ran; returned ${path.basename(d.res.plan_txt)}`)
  await assert.rejects(runWorkflow({ ...base, run_dir: path.join(T, 'wf-noperm') }, {}), /owner_ok/)
  const c = await runWorkflow({ ...base, owner_ok: true, run_dir: path.join(T, 'wf-crash') }, { sessionThrows: true })
  assert.deepEqual(c.calls, ['preflight', 'session A', 'restore settings', 'keep captures'])
  assert.equal(c.res.stopped, 'session'); assert.equal(c.res.settings_restored, true, JSON.stringify(c.res.restore))
  assert.equal(sha(LIVE), ORIG_SHA, 'settings byte-identical after a crashed session')
  ok('workflow: owner_ok required; session crash -> Restore still runs, settings byte-identical')
}

console.log(`\n${passed} passed`)

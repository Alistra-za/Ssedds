#!/usr/bin/env node
// Deterministic half of the golden-capture workflow (golden-capture.js). Workflow scripts have no filesystem, so
// every file read/write/hash the workflow needs is a command here; its agents only run these and relay the JSON.
// Commands (all print one JSON object on stdout; exit 1 with {"ok":false,"reason":...} on refusal):
//   preflight --plan P --mo2 M --run-root R [--journal J] [--profile X] [--cell C] [--keep-root K] [--cs-mod NAME] [--dry-run]
//       W1/W7/W8/W10: validate the plan, resolve the profile, record the CS version, check disk, plan the settings
//       edits; without --dry-run also back up SettingsUser.json, record it in the recovery journal J and apply the
//       edits atomically, all under the journal lock. Creates a fresh run dir R\<UTC time>-<run id> (never reused) and
//       writes resolved.json and plan.txt there; a refused real run writes nothing.
//   steps --run-dir D --group L --origin x,y,z      W3/W4: the exact DevBench requests for one group.
//   restore --mo2 M [--journal J] [--run-id ID]     W1: atomic byte-copy of the journal's backup, verify sha256;
//       with --run-id only when the pending entry is that run's (automatic cleanup); without it: restore_only.
//   keep --run-dir D --file F [--group L --label X --frame N]
//       W6: copy a Captures .rdc to keep_dir and verify sha256. Never deletes anything.
//   copy --run-dir D --file F                       copy (never delete) e.g. the CS log into keep_dir.
//   map-draws --run-dir D --group G --draws F [--label X]   W9: draw -> placed ref, writes D/drawmap_<X>.json.
// Env (tests only): GOLDEN_TEST_DELAY_BEFORE_LOCK_MS = sleep before taking the lock; GOLDEN_GAME_PROCESS = process name checked for "game running" (default SkyrimSE.exe);
//   GOLDEN_FAIL_RENAME = basename substring whose atomic replace fails (simulated interruption).
import fs from 'node:fs'
import path from 'node:path'
import crypto from 'node:crypto'
import { execFileSync } from 'node:child_process'
import { pathToFileURL } from 'node:url'

const GB = 1024 ** 3
export const CAPTURE_GB = 1.9 // largest .rdc seen (guide §4: 1.3-1.9 GB)
export const LIGHT_DEFAULTS = { anchor: '0009DA9C', fallback_ligh: '000E2174', fallback_pos: [0, 250, 120] }
const DEFAULT_CAMERA = { rel: [0, -150, 120], pitch: 0.15, yaw: 0.0 }
const DEFAULT_PLAYER_POS = [0, -600, 0] // plan frame: 450 behind the default camera, out of its view

// Plan frame -> marker-relative world offsets. x_layout_yaw_deg turns the whole layout clockwise (heading sense, as
// SetAngle Z and camera yaw), then x_origin_shift (world units) moves it. Without both keys this is the identity.
// shortcut: Z is added to SetAngle as is, exact only while X/Y angles are 0 (all plan angles); compose properly if not.
export function layoutOf(plan) {
  const deg = plan.x_layout_yaw_deg || 0, sh = plan.x_origin_shift || [0, 0, 0]
  const a = deg * Math.PI / 180, c = Math.cos(a), s = Math.sin(a), r1 = v => Math.round(v * 10) / 10 || 0 // no -0 in plan.txt
  return {
    pos: p => [r1(sh[0] + p[0] * c + p[1] * s), r1(sh[1] - p[0] * s + p[1] * c), r1((sh[2] || 0) + (p[2] || 0))],
    angle: g => [g[0], g[1], (((g[2] + deg) % 360) + 360) % 360],
    yaw: y => Math.round((y + a) * 1e4) / 1e4,
  }
}
// Always on for a capture run, whatever the profile says: RenderDoc + Frame Annotations (base workflow). Log Level is NOT
// forced (owner 2026-10-18): Log Level <= 1 turns on CS developer mode, which compiles D3DCOMPILE_DEBUG shaders into the
// owner's disk cache. W2 uses the DevBench shadercache tool; X6 decodes descriptors offline (cslog_defines.py).
const CAPTURE_KEYS = { RenderDoc: { 'Enable RenderDoc Capture': true }, Advanced: { 'Frame Annotations': true } }
// x_extract stage names -> scripts, in run order (GOLDEN-EXTRACTORS.md "Run order"). X4 (stage D) is not built.
export const EXTRACT_STAGES = [
  { stage: 'A', script: 'p04_stageA_enumerate.py', runner: 'qrenderdoc' },
  { stage: 'B', script: 'p04_stageB_state.py', runner: 'qrenderdoc' },
  { stage: 'dispatch', script: 'p04_dispatch.py', runner: 'qrenderdoc' },
  { stage: 'B', script: 'p04_stageB_extra.py', runner: 'qrenderdoc' },
  { stage: 'C', script: 'p04_stageC_dump.py', runner: 'qrenderdoc' },
  { stage: 'shader_id', script: 'p04_shader_id.py', runner: 'qrenderdoc' },
  { stage: 'defines', script: 'cslog_defines.py', runner: 'python' },
  { stage: 'fixture', script: 'golden_fixture.py', runner: 'python' },
]
export const DEFAULT_JOURNAL = 'F:\\ai\\dds\\tools\\rdc-fixtures\\.golden-recovery.json'
export const EXTRACTOR_ROOT = 'F:\\ai\\dds\\tools\\rdc-out\\golden' // fixed in golden_common.GOLDEN_ROOT

// ---------- JSON text scanner: positions of every object member, so edits touch only value bytes ----------
function scan(text) {
  let i = 0
  const ws = () => { while (/\s/.test(text[i] || '')) i++ }
  const str = () => { const s = i; i++; while (text[i] !== '"') { if (text[i] === '\\') i++; i++; if (i >= text.length) throw new Error('unterminated string') } i++; return JSON.parse(text.slice(s, i)) }
  function value() {
    ws()
    const c = text[i]
    if (c === '{') {
      const node = { type: 'object', start: i, members: [] }
      i++; ws()
      if (text[i] === '}') { node.end = i; i++; return node }
      for (;;) {
        ws(); const keyStart = i; const key = str(); ws()
        if (text[i] !== ':') throw new Error(`expected : at ${i}`)
        i++; ws()
        const valueStart = i; const v = value()
        node.members.push({ key, keyStart, valueStart, valueEnd: i, node: v })
        ws()
        if (text[i] === ',') { i++; continue }
        if (text[i] === '}') { node.end = i; i++; return node }
        throw new Error(`expected , or } at ${i}`)
      }
    }
    if (c === '[') {
      const start = i; i++; ws()
      if (text[i] === ']') { i++; return { type: 'array', start } }
      for (;;) { value(); ws(); if (text[i] === ',') { i++; continue } if (text[i] === ']') { i++; return { type: 'array', start } } throw new Error(`expected , or ] at ${i}`) }
    }
    const start = i
    if (c === '"') { str(); return { type: 'scalar', start } }
    const m = /^(true|false|null|-?\d+(\.\d+)?([eE][+-]?\d+)?)/.exec(text.slice(i, i + 64))
    if (!m) throw new Error(`bad value at ${i}`)
    i += m[0].length
    return { type: 'scalar', start }
  }
  const root = value(); ws()
  if (i !== text.length) throw new Error(`trailing data at ${i}`)
  return root
}

const lineOf = (text, pos) => text.slice(0, pos).split('\n').length
const lineIndent = (text, pos) => { const ls = text.lastIndexOf('\n', pos - 1) + 1; return /^[ \t]*/.exec(text.slice(ls))[0] }

// Plan the minimal text edits that make `text` carry every key of `overlay` (nested objects = sections).
// Returns { edits: [{pos, end, insert, line, path, before, after}], result }. Throws on a type clash.
export function planSettingsEdits(text, overlay) {
  const bom = text.charCodeAt(0) === 0xfeff
  const body = bom ? text.slice(1) : text
  const root = scan(body)
  if (root.type !== 'object') throw new Error('settings root is not an object')
  const nl = body.includes('\r\n') ? '\r\n' : '\n'
  const firstMember = root.members[0]
  const unit = firstMember ? lineIndent(body, firstMember.keyStart) || ' ' : ' '
  const edits = [] // one per changed key (for the plan printout)
  const ops = [] // text operations; never overlapping
  const fmt = (v, indent) => JSON.stringify(v, null, unit).split('\n').join(nl + indent)
  function walk(obj, ov, keyPath, depthIndent) {
    const missing = []
    for (const [k, v] of Object.entries(ov)) {
      const p = [...keyPath, k]
      const m = obj.members.find(x => x.key === k)
      const isObj = v !== null && typeof v === 'object' && !Array.isArray(v)
      if (m) {
        if (isObj) {
          if (m.node.type !== 'object') throw new Error(`${p.join('.')}: overlay is a section but the file has a value`)
          walk(m.node, v, p, lineIndent(body, m.keyStart) + unit)
        } else {
          if (m.node.type !== 'scalar') throw new Error(`${p.join('.')}: file has a section/array, overlay a value`)
          const before = body.slice(m.valueStart, m.valueEnd)
          const after = JSON.stringify(v)
          if (JSON.stringify(JSON.parse(before)) !== after) {
            ops.push({ pos: m.valueStart, end: m.valueEnd, insert: after })
            edits.push({ line: lineOf(body, m.valueStart), path: p.join('.'), before, after })
          }
        }
      } else missing.push([k, v, p])
    }
    if (!missing.length) return
    // All missing members of one object go in one operation, so an empty object gets valid commas.
    const last = obj.members[obj.members.length - 1]
    const indent = last ? lineIndent(body, last.keyStart) : depthIndent
    const members = missing.map(([k, v]) => `${JSON.stringify(k)}: ${fmt(v, indent)}`).join(`,${nl}${indent}`)
    const at = last ? last.valueEnd : obj.start
    if (last) ops.push({ pos: last.valueEnd, end: last.valueEnd, insert: `,${nl}${indent}${members}` })
    else ops.push({ pos: obj.start + 1, end: obj.end, insert: `${nl}${indent}${members}${nl}${indent.slice(unit.length)}` })
    for (const [, v, p] of missing) edits.push({ line: lineOf(body, at), path: p.join('.'), before: null, after: JSON.stringify(v) })
  }
  walk(root, overlay, [], unit)
  let result = body
  for (const o of [...ops].sort((a, b) => b.pos - a.pos)) result = result.slice(0, o.pos) + o.insert + result.slice(o.end)
  return { edits, result }
}

export function deepMerge(base, ov) {
  const out = structuredClone(base)
  for (const [k, v] of Object.entries(ov)) {
    out[k] = v !== null && typeof v === 'object' && !Array.isArray(v) && out[k] && typeof out[k] === 'object' ? deepMerge(out[k], v) : structuredClone(v)
  }
  return out
}
const canon = v => JSON.stringify(v, (k, x) => x && typeof x === 'object' && !Array.isArray(x) ? Object.fromEntries(Object.keys(x).sort().map(n => [n, x[n]])) : x)

// ---------- plan resolution (W1 profile, W8 checks) ----------
export function resolveProfile(plan, profileArg) {
  const base = plan.x_profile || 'A'
  const profile = profileArg || base
  let overlay = plan.x_cs_settings || {}
  let groups = plan.groups || []
  if (profile !== base) {
    const other = (plan.x_profiles_other_runs || {})[profile]
    if (!other) throw new Error(`profile ${profile} is neither x_profile (${base}) nor a key of x_profiles_other_runs`)
    if (other.groups) {
      const missing = other.groups.filter(l => !groups.some(g => g.label === l))
      if (missing.length) throw new Error(`profile ${profile} names unknown groups: ${missing.join(', ')}`)
      groups = groups.filter(g => other.groups.includes(g.label))
    }
    overlay = other.x_cs_settings ?? {}
    if (typeof overlay === 'string') {
      // The one string form the plan uses: "<base> without <Feature> in Disable at Boot".
      const m = /^(\w+) without (\w+) in Disable at Boot$/.exec(overlay.trim())
      if (!m || m[1] !== base) throw new Error(`profile ${profile}: x_cs_settings string "${overlay}" not understood (only "${base} without <Feature> in Disable at Boot"); give an object`)
      overlay = structuredClone(plan.x_cs_settings || {})
      if (!overlay['Disable at Boot'] || !(m[2] in overlay['Disable at Boot'])) throw new Error(`profile ${profile}: ${m[2]} is not in ${base}'s Disable at Boot`)
      delete overlay['Disable at Boot'][m[2]]
    }
  }
  return { profile, overlay: deepMerge(overlay, CAPTURE_KEYS), groups }
}

export function validatePlan(plan, groups) {
  const errs = []
  if (!Array.isArray(plan.groups) || !plan.groups.length) errs.push('plan has no groups')
  for (const g of groups) {
    const f = g.forms || [], t = g.tags || []
    // W8: a probe group (no forms, no tags) is fine; forms without tags or tags without forms is not.
    if (!!f.length !== !!t.length) errs.push(`${g.label}: forms and tags must both be empty (probe) or both set`)
    if (g.x_pos && g.x_pos.length !== f.length) errs.push(`${g.label}: x_pos has ${g.x_pos.length} entries for ${f.length} forms`)
    if (g.x_angle && g.x_angle.length !== f.length) errs.push(`${g.label}: x_angle has ${g.x_angle.length} entries for ${f.length} forms`)
    if (!Array.isArray(g.captures) || !g.captures.length) errs.push(`${g.label}: no captures`)
    for (const id of [...f, ...(g.x_motion_keyframed || [])]) if (!/^[0-9A-Fa-f]{8}$/.test(id)) errs.push(`${g.label}: form ${id} is not 8 hex digits`)
  }
  const labels = new Set((plan.groups || []).map(g => g.label))
  // W8: questions without "outcomes" are fixture goals; only their groups must exist.
  for (const q of plan.questions || []) for (const l of q.groups || []) if (!labels.has(l)) errs.push(`question ${q.id}: unknown group ${l}`)
  return errs
}

const vadd = (a, b) => a.map((x, i) => x + (b[i] || 0))

export function resolveGroups(plan, groups, s6) {
  const light = { ...LIGHT_DEFAULTS, ...(plan.x_light || {}) }
  const L = layoutOf(plan)
  const player = L.pos(plan.x_player_pos || DEFAULT_PLAYER_POS)
  const fallback = Array.isArray(light.fallback_pos[0]) ? light.fallback_pos : [light.fallback_pos]
  const formTags = Object.fromEntries(Object.entries(plan.x_form_tags || {}).map(([k, v]) => [k.toUpperCase(), v]))
  const out = groups.map(g => {
    // W9 expected shapes per form: x_form_tags; else tags paired with the non-anchor forms by index when the counts
    // match (the draft plan's layout); else every tag of the group (then shared names stay ambiguous).
    const tags = g.tags || []
    const subjects = (g.forms || []).filter(f => f.toUpperCase() !== light.anchor.toUpperCase())
    const shapesFor = form => {
      const F = form.toUpperCase()
      if (formTags[F]) return formTags[F]
      if (F === light.anchor.toUpperCase()) return []
      return tags.length === subjects.length ? [tags[subjects.findIndex(x => x.toUpperCase() === F)]] : tags
    }
    const forms = (g.forms || []).map((form, i) => ({
      shapes: shapesFor(form),
      form: form.toUpperCase(),
      rel: L.pos(g.x_pos ? g.x_pos[i] : [((i - ((g.forms.length - 1) / 2)) * (g.spacing || 120)), 400, 0]), // legacy row
      angle: L.angle(g.x_angle ? g.x_angle[i] : [0, 0, 180]),
      keyframed: (g.x_motion_keyframed || []).map(x => x.toUpperCase()).includes(form.toUpperCase()),
      light_anchor: form.toUpperCase() === light.anchor.toUpperCase(),
    }))
    const opt = g.x_optional_if_s6_ok
    let optional = null
    if (opt) {
      // Only when the s6 mesh the plan names is on disk with the planned sha256 (never written here).
      const f = s6.find(x => x.copy_of && x.ok)
      optional = { form: opt.form, used: !!f, why: f ? `s6 file ${f.path} present` : 's6 file missing or sha256 differs' }
      if (f) forms.push({ shapes: formTags[opt.form.toUpperCase()] || opt.tags || [], form: opt.form.toUpperCase(), rel: L.pos(opt.pos), angle: L.angle(opt.angle || [0, 0, 180]), keyframed: false, light_anchor: false })
    }
    const cam = g.x_camera || DEFAULT_CAMERA
    // Point lights (W4): the PlaceAtMe LIGH is the primary method; LightPlacer on the anchor is only a diagnostic.
    // x_light_pos lists one light per subject (pair); a group with only the anchor gets the single default position.
    const lights = (g.x_light_pos || (forms.some(f => f.light_anchor) ? fallback : [])).map(L.pos)
    return {
      label: g.label, forms, tags: [...(g.tags || []), ...(optional && optional.used ? opt.tags || [] : [])],
      probe: !forms.length, camera: { ...cam, rel: L.pos(cam.rel), yaw: L.yaw(cam.yaw || 0) }, settle_s: g.settle_s ?? 3, player, lights,
      captures: g.captures, risky: !!g.risky, note: g.x_note || '', optional,
    }
  })
  // Risky groups last (base rule: they may crash the game).
  return [...out.filter(g => !g.risky), ...out.filter(g => g.risky)].map(g => ({ ...g, light }))
}

// W3/W4: the exact DevBench requests for one group, given the coc-marker origin.
export function groupSteps(g, origin) {
  const S = []
  const papyrus = (fn, self, args, script = 'ObjectReference') => ({ tool: 'papyrus', body: { action: 'call', script, function: fn, self, args } })
  // The free camera does not hide the player, so move the player out of view first (P0 probe was mostly the body).
  if (g.player) S.push({ ...papyrus('SetPosition', { form: '0x14' }, vadd(origin, g.player)), note: `player out of view, rel ${JSON.stringify(g.player)}` })
  g.forms.forEach((f, i) => {
    const ref = { form: `$ref${i}` }
    S.push({ ...papyrus('PlaceAtMe', { form: '0x14' }, [{ form: `0x${f.form}` }, 1, false, false]), save: `ref${i}`, note: `place ${f.form}${f.light_anchor ? ' (Light Placer anchor)' : ''}` })
    if (f.keyframed) S.push({ ...papyrus('SetMotionType', ref, [4, true]), note: 'keyframed: no Havok drift' })
    S.push(papyrus('SetAngle', ref, f.angle))
    S.push({ ...papyrus('SetPosition', ref, vadd(origin, f.rel)), note: `rel ${JSON.stringify(f.rel)}` })
    S.push({ tool: 'inspect', body: { kind: 'refs', formId: `$ref${i}` }, note: 'record model path and position; if it is an Actor also call Actor.SetDontMove [true]' })
    if (f.light_anchor) {
      S.push({ tool: 'console', body: { action: 'exec', command: `prid $ref${i}` }, note: 'ref id as 8 hex digits without 0x' })
      S.push({ tool: 'console', body: { action: 'exec', command: 'logLP', capture: true }, note: 'LightPlacer diagnostic only: record whether it lists a light on the anchor (also po3_LightPlacer.log). Never ReloadLP or Disable/Enable the anchor' })
    }
  })
  const lights = g.lights || []
  lights.forEach((p, k) => {
    S.push({ ...papyrus('PlaceAtMe', { form: '0x14' }, [{ form: `0x${g.light.fallback_ligh}` }, 1, false, false]), save: `light${k}`, note: `point light ${k + 1}/${lights.length} (primary method)` })
    S.push({ ...papyrus('SetPosition', { form: `$light${k}` }, vadd(origin, p)), note: `rel ${JSON.stringify(p)}` })
  })
  if (lights.length) S.push({ tool: 'inspect', body: { kind: 'lights', scope: 'scene' }, note: 'record: every placed light listed (owner, position); each subject has one within ~100 units' })
  const cam = g.camera
  S.push({ tool: 'camera', body: { action: 'freecam', on: true } })
  const [x, y, z] = vadd(origin, cam.rel)
  S.push({ tool: 'camera', body: { action: 'drive', x, y, z, pitch: cam.pitch, yaw: cam.yaw } })
  S.push({ tool: 'camera', body: { action: 'get' }, note: 'record the pose' })
  S.push({ tool: 'capture', body: { kind: 'native', allowNative: true, checkpointId: `${g.label}_check`, cleanup: true }, note: 'draw check: every subject on screen' })
  S.push({ wait_s: g.settle_s, note: 'settle; W2 shader-compile check before the first capture' })
  g.captures.forEach((c, n) => {
    if (c.after_s) S.push({ wait_s: c.after_s })
    S.push({ tool: 'communityshaders.capture', body: { kind: 'renderdoc', frames: 1 }, note: `${g.label}_cap${n}${c.consecutive ? ' (consecutive: send right after the previous request)' : ' (wait for the previous .rdc to be complete first)'}` })
  })
  g.forms.forEach((f, i) => { S.push(papyrus('Disable', { form: `$ref${i}` }, [false])); S.push(papyrus('Delete', { form: `$ref${i}` }, [])) })
  lights.forEach((p, k) => { S.push(papyrus('Disable', { form: `$light${k}` }, [false])); S.push(papyrus('Delete', { form: `$light${k}` }, [])) })
  S.push({ tool: 'camera', body: { action: 'freecam', on: false } })
  return S
}

// fw 81a = SkyrimClear: pins exterior weather (no effect indoors). plan.x_gamehour (0-24, default 12) sets the sun position.
export function setupConsole(plan = {}) {
  const h = plan.x_gamehour ?? 12
  if (typeof h !== 'number' || !(h >= 0 && h < 24)) throw new Error(`x_gamehour must be a number in [0, 24), got ${JSON.stringify(h)}`)
  return ['tai', 'tcai', 'tgm', `set gamehour to ${h}`, 'set timescale to 1', 'fw 81a']
}
export const SETUP_CONSOLE = setupConsole()

// ---------- machine facts ----------
function gameRunning() {
  const name = process.env.GOLDEN_GAME_PROCESS || 'SkyrimSE.exe'
  try { return execFileSync(path.join(process.env.SystemRoot || 'C:\\Windows', 'System32', 'tasklist.exe'), ['/FI', `IMAGENAME eq ${name}`, '/NH'], { encoding: 'utf8' }).toLowerCase().includes(name.toLowerCase()) }
  catch { return null } // unknown -> callers refuse
}
const sha256 = p => new Promise((res, rej) => { const h = crypto.createHash('sha256'); fs.createReadStream(p).on('data', d => h.update(d)).on('end', () => res(h.digest('hex'))).on('error', rej) })
const sha256Buf = b => crypto.createHash('sha256').update(b).digest('hex')
const freeGb = p => { const s = fs.statfsSync(p); return s.bavail * s.bsize / GB }
const readJson = p => JSON.parse(fs.readFileSync(p, 'utf8').replace(/^\uFEFF/, ''))
const existingAncestor = p => { while (!fs.existsSync(p)) { const up = path.dirname(p); if (up === p) break; p = up } return p }
const canonPath = p => fs.realpathSync.native(p).toLowerCase()
const inside = (child, parent) => { const rel = path.relative(parent, child); return !!rel && !rel.startsWith('..') && !path.isAbsolute(rel) }

// Atomic replace: a temp file in the same folder, flushed, then renamed over the target. An interruption leaves
// either the old file or the new one, never a truncated one.
export function writeAtomic(target, data) {
  const buf = typeof data === 'string' ? Buffer.from(data, 'utf8') : data // JS string -> UTF-8 without BOM
  const tmp = `${target}.golden-tmp-${process.pid}`
  const fd = fs.openSync(tmp, 'w')
  try { let off = 0; while (off < buf.length) off += fs.writeSync(fd, buf, off); fs.fsyncSync(fd) } finally { fs.closeSync(fd) }
  try {
    const fail = process.env.GOLDEN_FAIL_RENAME // test hook: simulate an interrupted replace of a matching file
    if (fail && path.basename(target).includes(fail)) throw new Error(`simulated rename failure for ${target}`)
    fs.renameSync(tmp, target)
  } catch (e) { try { fs.unlinkSync(tmp) } catch { /* already gone */ } throw e }
}
const writeJson = (p, v) => writeAtomic(p, JSON.stringify(v, null, 2) + '\n')

// Recovery journal (one per machine, outside run dirs): entries keyed by the canonical SettingsUser.json path.
// state "applied" = settings edited and not yet restored; every run refuses to start while one is pending.
const journalLoad = j => (fs.existsSync(j) ? readJson(j) : { entries: {} })
function journalSet(j, key, entry) { const d = journalLoad(j); d.entries[key] = entry; fs.mkdirSync(path.dirname(j), { recursive: true }); writeJson(j, d) }
// Exclusive journal access: <journal>.lock is created with 'wx' (fails if it exists) before any journal check and
// removed when done. A leftover lock is never broken automatically: the caller must check that no run is active.
export async function withJournalLock(j, owner, fn) {
  const lock = `${j}.lock`
  fs.mkdirSync(path.dirname(j), { recursive: true })
  let fd
  try { fd = fs.openSync(lock, 'wx') } catch (e) {
    if (e.code === 'EEXIST') return { ok: false, locked: true, reason: `journal lock ${lock} exists: another golden-capture preflight/restore is running, or one was killed. Check that no run is active, then delete the lock by hand` }
    throw e
  }
  try {
    fs.writeSync(fd, JSON.stringify({ pid: process.pid, owner }) + '\n'); fs.closeSync(fd); fd = undefined
    return await fn()
  } finally {
    if (fd !== undefined) fs.closeSync(fd)
    fs.rmSync(lock, { force: true })
  }
}

function mo2Facts(mo2, csModArg) {
  const ini = fs.readFileSync(path.join(mo2, 'ModOrganizer.ini'), 'utf8')
  const pm = /^selected_profile=(?:@ByteArray\()?([^)\r\n]+)\)?/m.exec(ini)
  if (!pm) throw new Error('ModOrganizer.ini has no selected_profile')
  const profile = pm[1]
  const modlist = fs.readFileSync(path.join(mo2, 'profiles', profile, 'modlist.txt'), 'utf8').split(/\r?\n/)
  const enabled = new Set(modlist.filter(l => l.startsWith('+')).map(l => l.slice(1)))
  let csMod = csModArg
  if (!csMod) {
    const cands = [...enabled].filter(m => /^CommunityShaders_AIO/i.test(m))
    if (cands.length !== 1) throw new Error(`cannot pick the CS mod (enabled CommunityShaders_AIO*: ${cands.join(', ') || 'none'}); pass --cs-mod`)
    csMod = cands[0]
  }
  const metaPath = path.join(mo2, 'mods', csMod, 'meta.ini')
  const vm = /^version=(.*)$/m.exec(fs.readFileSync(metaPath, 'utf8'))
  return { profile, enabled, csMod, csMeta: vm ? vm[1].trim() : '', metaPath }
}
const settingsPathOf = mo2 => path.join(mo2, 'overwrite', 'SKSE', 'Plugins', 'CommunityShaders', 'SettingsUser.json')

// W5/W10: the plan's x_extract (default: every stage), validated; returns the scripts in run order.
export function extractScripts(names) {
  const list = names || ['A', 'B', 'dispatch', 'C', 'shader_id', 'defines', 'fixture']
  const known = new Set(EXTRACT_STAGES.map(s => s.stage))
  const bad = list.filter(n => !known.has(n))
  if (bad.length) throw new Error(`x_extract: unsupported stage(s) ${bad.join(', ')} (known: ${[...known].join(', ')}; D/X4 is not built)`)
  if (list.some(n => ['B', 'C', 'shader_id'].includes(n)) && !list.includes('dispatch')) throw new Error('x_extract: B, C and shader_id need "dispatch" (X1 runs before X2/X3/X5)')
  if (list.some(n => n !== 'A') && !list.includes('A')) throw new Error('x_extract: every stage needs "A" (drawmap.json)')
  return EXTRACT_STAGES.filter(s => list.includes(s.stage))
}

// ---------- commands ----------
const sleepMs = ms => new Promise(r => setTimeout(r, ms))
// Exclusive per-invocation run directory <run-root>\<UTC time>-<run id>; a plain mkdir throws if it exists, so a
// run dir is never reused or overwritten by another invocation.
function newRunDir(root, runId) {
  fs.mkdirSync(root, { recursive: true })
  const dir = path.join(root, `${new Date().toISOString().replace(/[:.]/g, '-')}-${runId.slice(0, 8)}`)
  fs.mkdirSync(dir)
  return dir
}

async function preflight(o) {
  if (o['run-dir']) return { ok: false, reason: 'preflight takes --run-root and creates a fresh run dir itself; --run-dir is not accepted' }
  if (!o['run-root']) return { ok: false, reason: 'preflight needs --run-root' }
  const plan = readJson(o.plan)
  const { profile, overlay, groups: rawGroups } = resolveProfile(plan, o.profile)
  const errs = validatePlan(plan, rawGroups)
  let scripts = []
  try { scripts = extractScripts(plan.x_extract) } catch (e) { errs.push(e.message) }
  if (errs.length) return { ok: false, reason: `plan: ${errs.join('; ')}` }
  const journal = o.journal || DEFAULT_JOURNAL
  const delay = +(process.env.GOLDEN_TEST_DELAY_BEFORE_LOCK_MS || 0) // test hook: widen the window before the lock
  if (delay) await sleepMs(delay)
  if (o['dry-run']) return preflightBody(o, { plan, profile, overlay, rawGroups, scripts, journal, dry: true })
  // N4: everything that reads the live settings, checks the journal, writes the backup or applies runs under the lock.
  return withJournalLock(journal, `preflight ${profile}`, () => preflightBody(o, { plan, profile, overlay, rawGroups, scripts, journal, dry: false }))
}

async function preflightBody(o, { plan, profile, overlay, rawGroups, scripts, journal, dry }) {
  const runId = crypto.randomUUID()
  const settingsPath = settingsPathOf(o.mo2)
  const capturesDir = path.join(path.dirname(settingsPath), 'Captures')
  const key = canonPath(settingsPath)
  const pe = journalLoad(journal).entries[key]
  const pendingMsg = pe && pe.state === 'applied' ? `recovery journal ${journal}: profile ${pe.profile} (run ${pe.run_id}) applied settings and never restored them; run with restore_only first` : ''
  if (pendingMsg && !dry) return { ok: false, reason: pendingMsg } // N5: before any artifact is written
  const mo = mo2Facts(o.mo2, o['cs-mod'])
  const origBuf = fs.readFileSync(settingsPath)
  const settingsText = origBuf.toString('utf8')
  const csSettingsVersion = JSON.parse(settingsText.replace(/^\uFEFF/, '')).Version || ''
  if (!csSettingsVersion || !mo.csMeta) return { ok: false, reason: `CS version unknown (SettingsUser Version "${csSettingsVersion}", meta "${mo.csMeta}")` }
  const csKey = `cs-${csSettingsVersion.split('-').slice(0, 3).join('.')}-${mo.csMeta}`
  // x_fixture_out can only name where the extractors already write (golden_common.GOLDEN_ROOT\<cs-version>).
  const extractOut = path.join(EXTRACTOR_ROOT, csKey)
  if (plan.x_fixture_out && path.resolve(plan.x_fixture_out).toLowerCase() !== path.resolve(extractOut).toLowerCase()) return { ok: false, reason: `plan: x_fixture_out ${plan.x_fixture_out} differs from the extractors' fixed output ${extractOut}` }
  const s6 = (plan.x_s6_files || []).map(f => {
    const p = f.path.startsWith('G:\\Acherus') ? path.join(o.mo2, f.path.slice('G:\\Acherus'.length)) : f.path
    return { path: p, copy_of: f.copy_of || null, exists: fs.existsSync(p), sha256_want: f.sha256 || null, ok: false }
  })
  for (const f of s6) f.ok = f.exists && (!f.sha256_want || (await sha256(f.path)) === f.sha256_want)
  const groups = resolveGroups(plan, rawGroups, s6)
  const planned = groups.reduce((n, g) => n + g.captures.length, 0)
  // W6/W10: the plan's x_keep_rdc wins (per profile below it); --keep-root overrides both (tests).
  const keepRoot = o['keep-root'] || plan.x_keep_rdc || path.join('F:\\ai\\dds\\tools\\rdc-fixtures', csKey)
  const keepDir = path.join(keepRoot, profile)
  const needF = planned * CAPTURE_GB + 20, freeF = freeGb(existingAncestor(keepRoot))
  const needG = planned * CAPTURE_GB + 10, freeG = freeGb(existingAncestor(capturesDir))
  const missingMods = ['DevBench'].filter(m => !mo.enabled.has(m))
  let port = 8920
  try { port = readJson(path.join(o.mo2, 'overwrite', 'SKSE', 'Plugins', 'devbench', 'runtime.json')).port || 8920 } catch { /* default */ }
  const { edits, result } = planSettingsEdits(settingsText, overlay)
  // The edited text must parse to exactly the original merged with the overlay: no other key may change.
  if (canon(JSON.parse(result)) !== canon(deepMerge(JSON.parse(settingsText.replace(/^\uFEFF/, '')), overlay))) return { ok: false, reason: 'settings edit self-check failed (result != original + overlay)' }
  const running = gameRunning()
  const cell = o.cell || plan.cell || ''
  const stop = []
  if (!cell) stop.push('no cell: set plan.cell or --cell')
  let setup = SETUP_CONSOLE
  try { setup = setupConsole(plan) } catch (e) { stop.push(e.message) }
  if (pendingMsg) stop.push(pendingMsg)
  if (freeF < needF) stop.push(`fixture drive has ${freeF.toFixed(1)} GB free, needs ${needF.toFixed(1)} (captures + 20 GB)`)
  if (freeG < needG) stop.push(`captures drive has ${freeG.toFixed(1)} GB free, needs ${needG.toFixed(1)}`)
  if (missingMods.length) stop.push(`not enabled in MO2 profile ${mo.profile}: ${missingMods.join(', ')}`)
  if (running !== false) stop.push(running ? 'the game is running' : 'cannot tell whether the game is running')
  if (stop.length && !dry) return { ok: false, reason: stop.join('; ') } // N5: refused runs write nothing
  const runDir = newRunDir(o['run-root'], runId)
  const appliedCopy = path.join(runDir, 'SettingsUser.applied.json')
  const resolved = {
    run_id: runId, profile, cell, cell_fallback: plan.x_cell_fallback || [], cs_version: { settings: csSettingsVersion, meta: mo.csMeta, key: csKey, mod: mo.csMod, meta_path: mo.metaPath },
    mo2_profile: mo.profile, settings_path: settingsPath, settings_applied_copy: appliedCopy, captures_dir: capturesDir, journal, overlay,
    edits, keep_dir: keepDir, extract_out: extractOut, extract_scripts: scripts,
    shape_offsets: plan.x_shape_offsets || {}, match_tol: plan.x_match_tol || 60,
    s6, groups, planned_captures: planned, port, setup_console: setup, gamehour_alternatives: plan.x_gamehour_alternatives || [],
  }
  writeJson(path.join(runDir, 'resolved.json'), resolved)
  writeAtomic(path.join(runDir, 'plan.txt'), planText(resolved, dry))
  const summary = {
    run_dir: runDir, profile, cell, cs_key: csKey, settings_version: csSettingsVersion, meta_version: mo.csMeta, cs_meta_path: mo.metaPath, planned_captures: planned,
    free_gb_fixtures: +freeF.toFixed(1), need_gb_fixtures: +needF.toFixed(1), free_gb_captures: +freeG.toFixed(1), need_gb_captures: +needG.toFixed(1),
    keep_dir: keepDir, extract_out: extractOut, extract_scripts: scripts, settings_applied_copy: appliedCopy, journal,
    group_tags: Object.fromEntries(groups.map(g => [g.label, g.tags.join(',') || '*'])),
    port, edits: edits.length, game_running: running, backup: null, applied: false,
    s6: s6.map(f => `${f.path}: ${f.ok ? 'ok' : f.exists ? 'sha256 differs' : 'missing'}`),
  }
  if (dry) return { ok: true, dry_run: true, would_stop: stop, ...summary }
  // Backup next to the journal (outside run dirs), then journal "applied" BEFORE the live file changes.
  const backupSha = sha256Buf(origBuf)
  const backupDir = path.join(path.dirname(journal), 'settings-backups')
  fs.mkdirSync(backupDir, { recursive: true })
  const backup = path.join(backupDir, `SettingsUser.${profile}.${backupSha.slice(0, 12)}.json`)
  writeAtomic(backup, origBuf)
  if (await sha256(backup) !== backupSha) return { ok: false, reason: 'backup copy does not match the live file', ...summary }
  if (await sha256(settingsPath) !== backupSha) return { ok: false, reason: 'live settings changed since they were read (under the lock?); nothing edited', ...summary }
  // The run id is how this run's automatic cleanup recognises its own entry (restore --run-id).
  summary.run_id = runId
  const entry = { state: 'applied', run_id: runId, settings_path: settingsPath, backup, backup_sha256: backupSha, profile, run_dir: runDir }
  journalSet(journal, key, entry)
  try {
    const unchanged = async why => {
      if (await sha256(settingsPath) === backupSha) { journalSet(journal, key, { ...entry, state: 'restored', note: why }); return true }
      return false
    }
    if (gameRunning() !== false) { await unchanged('game started during preflight'); return { ok: false, reason: 'the game started during preflight; nothing edited', ...summary, backup } }
    try { writeAtomic(settingsPath, result) } catch (e) {
      const clean = await unchanged(`apply failed: ${e.message}`)
      return { ok: false, reason: `writing settings failed (${e.message}); ${clean ? 'live file unchanged' : 'live file changed: run restore_only'}`, ...summary, backup }
    }
    writeAtomic(appliedCopy, result)
    const reread = JSON.parse(fs.readFileSync(settingsPath, 'utf8'))
    if (canon(deepMerge(reread, overlay)) !== canon(reread)) return { ok: false, reason: 'live file lacks overlay values after writing; run restore_only', ...summary, backup }
    return { ok: true, ...summary, backup, backup_sha256: backupSha, applied: true }
  } catch (e) {
    // Any failure after the journal entry exists still names the run id, so the workflow restores it.
    return { ok: false, reason: `after applying settings: ${e.message}; journal entry ${runId} pending`, ...summary, backup, applied: true }
  }
}

function planText(r, dry) {
  const L = []
  L.push(`golden-capture plan${dry ? ' (DRY RUN: nothing written outside the run dir, game not launched)' : ''}`)
  L.push(`profile ${r.profile}; cell ${r.cell} (fallbacks ${r.cell_fallback.join(', ') || 'none'}); CS ${r.cs_version.key} (mod ${r.cs_version.mod})`)
  L.push(`settings: ${r.settings_path}; backup + recovery journal: ${r.journal}`)
  for (const e of r.edits) L.push(`  line ${e.line}: ${e.path}: ${e.before === null ? 'INSERT' : e.before} -> ${e.after}`)
  if (!r.edits.length) L.push('  (no settings edits)')
  L.push(`captures: ${r.planned_captures}, kept in ${r.keep_dir}; extractor output ${r.extract_out}`)
  L.push(`extract: ${r.extract_scripts.map(s => s.script).join(', ')}`)
  for (const f of r.s6) L.push(`s6 file ${f.path}: ${f.ok ? 'present' : f.exists ? 'sha256 differs' : 'missing'} (never written by this workflow)`)
  L.push(`session: launch MO2 SKSE, wait DevBench :${r.port}, coc ${r.cell}, origin = player position after coc (dry run shows origin 0,0,0)`)
  L.push('shader compiles (W2): positive signal required before the first capture, else the run stops ("compile readiness unknown")')
  L.push(`setup console: ${r.setup_console.join(' | ')}`)
  for (const g of r.groups) {
    L.push(`group ${g.label}${g.probe ? ' (probe)' : ''}${g.risky ? ' (risky, last)' : ''}: tags ${g.tags.join(', ') || '-'}${g.note ? `  # ${g.note}` : ''}`)
    if (g.optional) L.push(`  optional ${g.optional.form}: ${g.optional.used ? 'used' : 'skipped'} (${g.optional.why})`)
    for (const s of groupSteps(g, [0, 0, 0])) L.push(`  ${s.only_if ? `[if ${s.only_if}] ` : ''}${s.tool ? `${s.tool} ${JSON.stringify(s.body)}` : `wait ${s.wait_s}s`}${s.save ? ` => $${s.save}` : ''}${s.note ? `  # ${s.note}` : ''}`)
  }
  L.push('quit: console qqq; then restore settings (atomic byte copy of the backup, sha256 checked), keep each .rdc (copy + sha256; the Captures copies stay for the owner to delete)')
  return L.join('\n') + '\n'
}

// --run-id: automatic cleanup, restores only the entry this run applied. Without it (restore_only): any pending entry.
async function restore(o) {
  const journal = o.journal || DEFAULT_JOURNAL
  const settingsPath = settingsPathOf(o.mo2)
  const key = canonPath(settingsPath)
  return withJournalLock(journal, `restore ${o['run-id'] || 'unscoped'}`, () => restoreLocked(o, journal, settingsPath, key))
}
async function restoreLocked(o, journal, settingsPath, key) {
  const st = journalLoad(journal).entries[key]
  if (!st || st.state !== 'applied') return { ok: true, restored: false, nothing_to_restore: true, reason: 'no pending settings in the recovery journal' }
  if (o['run-id'] && st.run_id !== o['run-id']) return { ok: true, restored: false, not_owner: true, reason: `pending entry belongs to run ${st.run_id} (profile ${st.profile}), not ${o['run-id']}; left alone` }
  const running = gameRunning()
  if (running !== false) return { ok: false, restored: false, game_running: running, reason: 'game running (or unknown): CS may rewrite the file on exit; not touching it; journal stays pending' }
  const backupBuf = fs.readFileSync(st.backup)
  if (sha256Buf(backupBuf) !== st.backup_sha256) return { ok: false, restored: false, reason: `backup ${st.backup} changed since preflight; restore by hand; journal stays pending` }
  if (await sha256(settingsPath) !== st.backup_sha256) writeAtomic(settingsPath, backupBuf)
  const live = await sha256(settingsPath)
  if (live !== st.backup_sha256) return { ok: false, restored: false, reason: 'live file sha256 differs from the backup after the replace; journal stays pending' }
  journalSet(journal, key, { ...st, state: 'restored' })
  return { ok: true, restored: true, game_running: false, sha256: live, backup: st.backup }
}

async function copyInto(src, dir, name) {
  fs.mkdirSync(dir, { recursive: true })
  const dst = path.join(dir, name)
  const srcSha = await sha256(src)
  if (fs.existsSync(dst)) {
    if (await sha256(dst) !== srcSha) return { ok: false, reason: `${dst} exists with different content; nothing changed` }
  } else {
    const tmp = `${dst}.golden-tmp-${process.pid}`
    fs.copyFileSync(src, tmp)
    fs.renameSync(tmp, dst)
  }
  const dstSha = await sha256(dst)
  if (dstSha !== srcSha) return { ok: false, reason: 'copy sha256 differs; source kept', dst }
  return { ok: true, dst, sha256: dstSha }
}

// W6: only .rdc files inside the Captures folder are copied to keep_dir; the destination must resolve outside Captures
// and differ from the source. Nothing is ever deleted: the owner clears Captures by hand from the report's list.
async function keep(o) {
  const r = readJson(path.join(o['run-dir'], 'resolved.json'))
  const src = o.file
  if (!/\.rdc$/i.test(src || '')) return { ok: false, reason: `${src}: only .rdc files are kept` }
  if (!fs.existsSync(src)) return { ok: false, reason: `${src} not found` }
  if (!fs.existsSync(r.captures_dir)) return { ok: false, reason: `captures folder ${r.captures_dir} not found` }
  const srcC = canonPath(src), capC = canonPath(r.captures_dir)
  if (!inside(srcC, capC)) return { ok: false, reason: `${src} is not inside ${r.captures_dir}; refused` }
  fs.mkdirSync(r.keep_dir, { recursive: true })
  const keepC = canonPath(r.keep_dir)
  if (keepC === capC || inside(keepC, capC)) return { ok: false, reason: `keep folder ${r.keep_dir} is inside the Captures folder; refused` }
  const dst = path.join(r.keep_dir, path.basename(src))
  const dstC = fs.existsSync(dst) ? canonPath(dst) : path.join(keepC, path.basename(src)).toLowerCase()
  if (dstC === srcC || inside(dstC, capC)) return { ok: false, reason: `destination ${dst} resolves to the source or into Captures; refused` }
  const c = await copyInto(src, r.keep_dir, path.basename(src))
  if (!c.ok) return c
  writeAtomic(`${c.dst}.sha256`, `${c.sha256}  ${path.basename(c.dst)}\n`)
  const manifest = path.join(r.keep_dir, 'captures.json')
  const list = fs.existsSync(manifest) ? readJson(manifest) : []
  if (!list.some(x => x.file === path.basename(c.dst))) list.push({ file: path.basename(c.dst), sha256: c.sha256, bytes: fs.statSync(c.dst).size, profile: r.profile, cell: r.cell, cs: r.cs_version, group: o.group || null, label: o.label || null, frame: o.frame ? +o.frame : null })
  writeJson(manifest, list)
  if (await sha256(src) !== c.sha256) return { ok: false, reason: 'source changed during the copy', dst: c.dst }
  return { ok: true, source: src, dst: c.dst, sha256: c.sha256, source_deleted: false }
}

// Copy any file (the CS log) into keep_dir under a content-addressed name. Never deletes.
async function copy(o) {
  const r = readJson(path.join(o['run-dir'], 'resolved.json'))
  if (!o.file || !fs.existsSync(o.file)) return { ok: false, reason: `${o.file} not found` }
  const ext = path.extname(o.file), stem = path.basename(o.file, ext)
  const c = await copyInto(o.file, r.keep_dir, `${stem}.${(await sha256(o.file)).slice(0, 12)}${ext}`)
  return { ...c, source_deleted: false }
}

// W9: map draws to placed refs by identity, not by guessing from distance.
// - A ref "expects" the shapes in its `shapes` list (resolveGroups: x_form_tags, else the group's tags paired with
//   its forms by index, else every tag of the group).
// - A draw is matched "by name" when exactly one placed ref expects its shape and no other draw has that shape name.
// - When names collide (several refs expect it, or several draws carry it), a match is confirmed only with
//   x_shape_offsets for every candidate form: expected position = ref position + offset rotated by the ref's Z angle,
//   3D distance within tol and clearly nearer than the next candidate.
// - Everything else is "ambiguous" (collision without offsets) or "unmatched" (no ref expects the shape), always with
//   the candidates and their distances (XY when no offset) for the report.
const rotZ = (v, deg) => { const a = deg * Math.PI / 180; return [v[0] * Math.cos(a) - v[1] * Math.sin(a), v[0] * Math.sin(a) + v[1] * Math.cos(a), v[2]] } // shortcut: sign convention only checked for 0/180 degrees
export function mapDraws(draws, refs, { tol = 60, offsets = {} } = {}) {
  const offsetOf = (form, shape) => Object.entries(offsets[form] || {}).find(([p]) => shape.startsWith(p))
  const nameCount = {}
  for (const d of draws) nameCount[d.shape] = (nameCount[d.shape] || 0) + 1
  const near = (list, d) => list.map(r => {
    const off = offsetOf(r.form, d.shape)
    const exp = off ? vadd(r.pos, rotZ(off[1], (r.angle || [0, 0, 0])[2])) : r.pos
    const dist = off ? Math.hypot(d.world[0] - exp[0], d.world[1] - exp[1], d.world[2] - exp[2]) : Math.hypot(d.world[0] - exp[0], d.world[1] - exp[1])
    return { form: r.form, ref: r.ref, dist: +dist.toFixed(1), mode: off ? '3d+offset' : 'xy' }
  }).sort((x, y) => x.dist - y.dist)
  return draws.map(d => {
    const cands = refs.filter(r => (r.shapes || []).some(p => d.shape.startsWith(p)))
    const res = (status, by, hit, candidates) => ({ ...d, status, by, form: hit ? hit.form : null, ref: hit ? hit.ref : null, candidates })
    if (!cands.length) return res('unmatched', null, null, near(refs, d).slice(0, 3))
    if (cands.length === 1 && nameCount[d.shape] === 1) return res('matched', 'name', cands[0], near(cands, d))
    const c = near(cands, d)
    if (cands.every(r => offsetOf(r.form, d.shape))) {
      const [a, b] = c
      if (a.dist <= tol && !(b && b.dist < Math.max(a.dist * 1.5, a.dist + 10))) return res('matched', 'offset', a, c)
    }
    return res('ambiguous', null, null, c)
  })
}
function mapDrawsCmd(o) {
  const r = readJson(path.join(o['run-dir'], 'resolved.json'))
  const g = r.groups.find(x => x.label === o.group)
  if (!g) throw new Error(`no group ${o.group}`)
  const shapesOf = form => (g.forms.find(f => f.form === String(form).replace(/^0x/i, '').toUpperCase()) || {}).shapes || []
  const refs = readJson(path.join(o['run-dir'], `refs_${o.group}.json`)).map(x => ({ ...x, shapes: x.shapes || shapesOf(x.form) }))
  const out = mapDraws(readJson(o.draws), refs, { tol: r.match_tol, offsets: r.shape_offsets })
  const file = path.join(o['run-dir'], `drawmap_${o.label || o.group}.json`)
  writeJson(file, out)
  const n = s => out.filter(x => x.status === s).length
  return { ok: true, file, matched: n('matched'), ambiguous: n('ambiguous'), unmatched: n('unmatched') }
}

function parseArgs(argv) {
  const o = { _: [] }
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i]
    if (!a.startsWith('--')) { o._.push(a); continue }
    const k = a.slice(2)
    if (k === 'dry-run') o[k] = true
    else o[k] = argv[++i]
  }
  return o
}

async function main() {
  const o = parseArgs(process.argv.slice(2))
  const cmd = o._[0]
  let out
  try {
    if (cmd === 'preflight') out = await preflight(o)
    else if (cmd === 'steps') {
      const r = readJson(path.join(o['run-dir'], 'resolved.json'))
      const g = r.groups.find(x => x.label === o.group)
      if (!g) throw new Error(`no group ${o.group}`)
      const origin = o.origin.split(',').map(Number)
      if (origin.length !== 3 || origin.some(Number.isNaN)) throw new Error('--origin needs x,y,z')
      out = { ok: true, group: g.label, origin, steps: groupSteps(g, origin) }
    } else if (cmd === 'restore') out = await restore(o)
    else if (cmd === 'keep') out = await keep(o)
    else if (cmd === 'copy') out = await copy(o)
    else if (cmd === 'map-draws') out = mapDrawsCmd(o)
    else throw new Error(`unknown command ${cmd}; use preflight | steps | restore | keep | copy | map-draws`)
  } catch (e) { out = { ok: false, reason: e.message } }
  process.stdout.write(JSON.stringify(out, null, 2) + '\n')
  process.exitCode = out.ok ? 0 : 1
}

if (import.meta.url === pathToFileURL(process.argv[1] || '').href) main()

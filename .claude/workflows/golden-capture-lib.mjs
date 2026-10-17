#!/usr/bin/env node
// Deterministic half of the golden-capture workflow (golden-capture.js). Workflow scripts have no filesystem, so
// every file read/write/hash the workflow needs is a command here; its agents only run these and relay the JSON.
// Commands (all print one JSON object on stdout; exit 1 with {"ok":false,"reason":...} on refusal):
//   preflight --plan P --mo2 M --run-dir D [--profile X] [--cell C] [--fixtures F] [--cs-mod NAME] [--dry-run]
//       W1/W7/W8: validate the plan, resolve the profile, record the CS version, check disk, plan the settings
//       edits; without --dry-run also back up SettingsUser.json and apply the edits. Writes D/resolved.json and
//       D/plan.txt (every settings edit, placement, command and capture, human-readable).
//   steps --run-dir D --group L --origin x,y,z      W3/W4: the exact DevBench requests for one group.
//   restore --run-dir D                             W1: byte-copy the backup back, verify sha256.
//   keep --run-dir D --file F [--keep-source] [--group L --label X --frame N]
//       W6: copy F into the fixture folder, verify sha256, then delete F (unless --keep-source).
// Env: GOLDEN_GAME_PROCESS overrides the process name checked for "game running" (default SkyrimSE.exe; tests).
import fs from 'node:fs'
import path from 'node:path'
import crypto from 'node:crypto'
import { execFileSync } from 'node:child_process'
import { pathToFileURL } from 'node:url'

const GB = 1024 ** 3
export const CAPTURE_GB = 1.9 // largest .rdc seen (guide §4: 1.3-1.9 GB)
export const LIGHT_DEFAULTS = { anchor: '0009DA9C', fallback_ligh: '000E2174', fallback_pos: [0, 250, 120] }
const DEFAULT_CAMERA = { rel: [0, -150, 120], pitch: 0.15, yaw: 0.0 }
// Always on for a capture run (base workflow behaviour), whatever the profile says.
const CAPTURE_KEYS = { RenderDoc: { 'Enable RenderDoc Capture': true }, Advanced: { 'Frame Annotations': true } }

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
  const edits = []
  const fmt = (v, indent) => JSON.stringify(v, null, unit).split('\n').join(nl + indent)
  function walk(obj, ov, keyPath, depthIndent) {
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
          if (JSON.stringify(JSON.parse(before)) !== after) edits.push({ pos: m.valueStart, end: m.valueEnd, insert: after, line: lineOf(body, m.valueStart), path: p.join('.'), before, after })
        }
      } else {
        const last = obj.members[obj.members.length - 1]
        const indent = last ? lineIndent(body, last.keyStart) : depthIndent
        const member = `"${k}": ${fmt(v, indent)}`
        if (last) edits.push({ pos: last.valueEnd, end: last.valueEnd, insert: `,${nl}${indent}${member}`, line: lineOf(body, last.valueEnd), path: p.join('.'), before: null, after: JSON.stringify(v) })
        else edits.push({ pos: obj.start + 1, end: obj.end, insert: `${nl}${indent}${member}${nl}${indent.slice(unit.length)}`, line: lineOf(body, obj.start), path: p.join('.'), before: null, after: JSON.stringify(v) })
      }
    }
  }
  walk(root, overlay, [], unit)
  // Several inserts may share a position (appending to one section): keep their order, apply from the end.
  let result = body
  const ordered = edits.map((e, n) => ({ e, n })).sort((a, b) => b.e.pos - a.e.pos || b.n - a.n)
  for (const { e } of ordered) result = result.slice(0, e.pos) + e.insert + result.slice(e.end)
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
  const out = groups.map(g => {
    const forms = (g.forms || []).map((form, i) => ({
      form: form.toUpperCase(),
      rel: g.x_pos ? g.x_pos[i] : [((i - ((g.forms.length - 1) / 2)) * (g.spacing || 120)), 400, 0], // legacy row
      angle: g.x_angle ? g.x_angle[i] : [0, 0, 180],
      keyframed: (g.x_motion_keyframed || []).map(x => x.toUpperCase()).includes(form.toUpperCase()),
      light_anchor: form.toUpperCase() === light.anchor.toUpperCase(),
    }))
    const opt = g.x_optional_if_s6_ok
    let optional = null
    if (opt) {
      // Only when the s6 mesh the plan names is on disk with the planned sha256 (never written here).
      const f = s6.find(x => x.copy_of && x.ok)
      optional = { form: opt.form, used: !!f, why: f ? `s6 file ${f.path} present` : 's6 file missing or sha256 differs' }
      if (f) forms.push({ form: opt.form.toUpperCase(), rel: opt.pos, angle: opt.angle || [0, 0, 180], keyframed: false, light_anchor: false })
    }
    return {
      label: g.label, forms, tags: [...(g.tags || []), ...(optional && optional.used ? opt.tags || [] : [])],
      probe: !forms.length, camera: g.x_camera || DEFAULT_CAMERA, settle_s: g.settle_s ?? 3,
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
  g.forms.forEach((f, i) => {
    const ref = { form: `$ref${i}` }
    S.push({ ...papyrus('PlaceAtMe', { form: '0x14' }, [{ form: `0x${f.form}` }, 1, false, false]), save: `ref${i}`, note: `place ${f.form}${f.light_anchor ? ' (Light Placer anchor)' : ''}` })
    if (f.keyframed) S.push({ ...papyrus('SetMotionType', ref, [4, true]), note: 'keyframed: no Havok drift' })
    S.push(papyrus('SetAngle', ref, f.angle))
    S.push({ ...papyrus('SetPosition', ref, vadd(origin, f.rel)), note: `rel ${JSON.stringify(f.rel)}` })
    S.push({ tool: 'inspect', body: { kind: 'refs', formId: `$ref${i}` }, note: 'record model path and position; if it is an Actor also call Actor.SetDontMove [true]' })
    if (f.light_anchor) {
      S.push({ tool: 'console', body: { action: 'exec', command: `prid $ref${i}` }, note: 'ref id as 8 hex digits without 0x' })
      S.push({ tool: 'console', body: { action: 'exec', command: 'logLP', capture: true }, note: 'Light Placer must list a light on the anchor (also po3_LightPlacer.log); if not, once: console ReloadLP, Disable/Enable the anchor, logLP again' })
      S.push({ ...papyrus('PlaceAtMe', { form: '0x14' }, [{ form: `0x${g.light.fallback_ligh}` }, 1, false, false]), save: 'lightref', only_if: 'logLP shows no light on the anchor', note: 'fallback LIGH' })
      S.push({ ...papyrus('SetPosition', { form: '$lightref' }, vadd(origin, g.light.fallback_pos)), only_if: 'fallback LIGH placed' })
    }
  })
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
  if (g.forms.some(f => f.light_anchor)) S.push({ ...papyrus('Disable', { form: '$lightref' }, [false]), only_if: 'fallback LIGH placed' }, { ...papyrus('Delete', { form: '$lightref' }, []), only_if: 'fallback LIGH placed' })
  S.push({ tool: 'camera', body: { action: 'freecam', on: false } })
  return S
}

export const SETUP_CONSOLE = ['tai', 'tcai', 'tgm', 'set gamehour to 12', 'set timescale to 1']

// ---------- machine facts ----------
function gameRunning() {
  const name = process.env.GOLDEN_GAME_PROCESS || 'SkyrimSE.exe'
  try { return execFileSync(path.join(process.env.SystemRoot || 'C:\\Windows', 'System32', 'tasklist.exe'), ['/FI', `IMAGENAME eq ${name}`, '/NH'], { encoding: 'utf8' }).toLowerCase().includes(name.toLowerCase()) }
  catch { return null } // unknown -> callers refuse
}
const sha256 = p => new Promise((res, rej) => { const h = crypto.createHash('sha256'); fs.createReadStream(p).on('data', d => h.update(d)).on('end', () => res(h.digest('hex'))).on('error', rej) })
const freeGb = p => { const s = fs.statfsSync(p); return s.bavail * s.bsize / GB }
const readJson = p => JSON.parse(fs.readFileSync(p, 'utf8').replace(/^\uFEFF/, ''))
const writeJson = (p, v) => fs.writeFileSync(p, JSON.stringify(v, null, 2) + '\n')
const existingAncestor = p => { while (!fs.existsSync(p)) { const up = path.dirname(p); if (up === p) break; p = up } return p }

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
  const meta = fs.readFileSync(path.join(mo2, 'mods', csMod, 'meta.ini'), 'utf8')
  const vm = /^version=(.*)$/m.exec(meta)
  return { profile, enabled, csMod, csMeta: vm ? vm[1].trim() : '' }
}

// ---------- commands ----------
async function preflight(o) {
  const plan = readJson(o.plan)
  const { profile, overlay, groups: rawGroups } = resolveProfile(plan, o.profile)
  const errs = validatePlan(plan, rawGroups)
  if (errs.length) return { ok: false, reason: `plan: ${errs.join('; ')}` }
  const settingsPath = path.join(o.mo2, 'overwrite', 'SKSE', 'Plugins', 'CommunityShaders', 'SettingsUser.json')
  const capturesDir = path.join(path.dirname(settingsPath), 'Captures')
  const mo = mo2Facts(o.mo2, o['cs-mod'])
  const settingsText = fs.readFileSync(settingsPath, 'utf8')
  const csSettingsVersion = readJson(settingsPath).Version || ''
  if (!csSettingsVersion || !mo.csMeta) return { ok: false, reason: `CS version unknown (SettingsUser Version "${csSettingsVersion}", meta "${mo.csMeta}")` }
  const csKey = `cs-${csSettingsVersion.split('-').slice(0, 3).join('.')}-${mo.csMeta}`
  const s6 = (plan.x_s6_files || []).map(f => {
    const p = f.path.startsWith('G:\\Acherus') ? path.join(o.mo2, f.path.slice('G:\\Acherus'.length)) : f.path
    const exists = fs.existsSync(p)
    return { path: p, copy_of: f.copy_of || null, exists, sha256_want: f.sha256 || null, ok: false }
  })
  for (const f of s6) f.ok = f.exists && (!f.sha256_want || (await sha256(f.path)) === f.sha256_want)
  const groups = resolveGroups(plan, rawGroups, s6)
  const planned = groups.reduce((n, g) => n + g.captures.length, 0)
  const fixturesRoot = o.fixtures || 'F:\\ai\\dds\\tools\\rdc-fixtures'
  const fixtureDir = path.join(fixturesRoot, csKey, profile)
  const extractOut = plan.x_fixture_out || path.join('F:\\ai\\dds\\tools\\rdc-out\\golden', csKey)
  const needF = planned * CAPTURE_GB + 20, freeF = freeGb(existingAncestor(fixturesRoot))
  const needG = planned * CAPTURE_GB + 10, freeG = freeGb(existingAncestor(capturesDir))
  const missingMods = ['DevBench'].filter(m => !mo.enabled.has(m))
  let port = 8920
  try { port = readJson(path.join(o.mo2, 'overwrite', 'SKSE', 'Plugins', 'devbench', 'runtime.json')).port || 8920 } catch { /* default */ }
  const { edits, result } = planSettingsEdits(settingsText, overlay)
  // The edited text must parse to exactly the original merged with the overlay: no other key may change.
  if (canon(JSON.parse(result)) !== canon(deepMerge(JSON.parse(settingsText.replace(/^\uFEFF/, '')), overlay))) return { ok: false, reason: 'settings edit self-check failed (result != original + overlay)' }
  const running = gameRunning()
  fs.mkdirSync(o['run-dir'], { recursive: true })
  const backup = path.join(o['run-dir'], `SettingsUser.backup.${profile}.json`)
  const cell = o.cell || plan.cell || ''
  const resolved = {
    profile, cell, cell_fallback: plan.x_cell_fallback || [], cs_version: { settings: csSettingsVersion, meta: mo.csMeta, key: csKey, mod: mo.csMod },
    mo2_profile: mo.profile, settings_path: settingsPath, captures_dir: capturesDir, backup, overlay,
    edits: edits.map(({ line, path: p, before, after }) => ({ line, path: p, before, after })),
    fixture_dir: fixtureDir, extract_out: extractOut, extract: plan.x_extract || ['A', 'B'], wait_shadercache: plan.x_wait_shadercache !== false,
    s6, groups, planned_captures: planned, port, setup_console: SETUP_CONSOLE,
  }
  writeJson(path.join(o['run-dir'], 'resolved.json'), resolved)
  fs.writeFileSync(path.join(o['run-dir'], 'plan.txt'), planText(resolved, o['dry-run']))
  const summary = {
    run_dir: o['run-dir'], profile, cell, cs_key: csKey, settings_version: csSettingsVersion, meta_version: mo.csMeta, planned_captures: planned,
    free_gb_fixtures: +freeF.toFixed(1), need_gb_fixtures: +needF.toFixed(1), free_gb_captures: +freeG.toFixed(1), need_gb_captures: +needG.toFixed(1),
    fixture_dir: fixtureDir, extract_out: extractOut, port, edits: edits.length, game_running: running, backup: null, applied: false,
    s6: s6.map(f => `${f.path}: ${f.ok ? 'ok' : f.exists ? 'sha256 differs' : 'missing'}`),
  }
  const stop = []
  if (!cell) stop.push('no cell: set plan.cell or --cell')
  if (freeF < needF) stop.push(`fixture drive has ${freeF.toFixed(1)} GB free, needs ${needF.toFixed(1)} (captures + 20 GB)`)
  if (freeG < needG) stop.push(`captures drive has ${freeG.toFixed(1)} GB free, needs ${needG.toFixed(1)}`)
  if (missingMods.length) stop.push(`not enabled in MO2 profile ${mo.profile}: ${missingMods.join(', ')}`)
  if (running !== false) stop.push(running ? 'the game is running' : 'cannot tell whether the game is running')
  if (o['dry-run']) return { ok: true, dry_run: true, would_stop: stop, ...summary }
  if (stop.length) return { ok: false, reason: stop.join('; '), ...summary }
  // Backup. A backup left by a run that was never restored holds the owner's real file: never overwrite it.
  const statePath = path.join(o['run-dir'], 'settings-state.json')
  if (fs.existsSync(statePath)) {
    const st = readJson(statePath)
    if (st.applied && !st.restored) return { ok: false, reason: `an earlier run in ${o['run-dir']} applied settings and never restored them: run "restore" first`, ...summary }
  }
  fs.copyFileSync(settingsPath, backup)
  const backupSha = await sha256(backup)
  if (backupSha !== await sha256(settingsPath)) return { ok: false, reason: 'backup copy does not match the live file', ...summary }
  writeJson(statePath, { settings_path: settingsPath, backup, backup_sha256: backupSha, applied: true, restored: false })
  if (gameRunning() !== false) return { ok: false, reason: 'the game started during preflight; nothing edited', ...summary, backup }
  fs.writeFileSync(settingsPath, result, { encoding: 'utf8' }) // Buffer from a JS string: UTF-8, no BOM
  const reread = JSON.parse(fs.readFileSync(settingsPath, 'utf8'))
  if (canon(deepMerge(reread, overlay)) !== canon(reread)) return { ok: false, reason: 'live file lacks overlay values after writing; run restore', ...summary, backup }
  return { ok: true, ...summary, backup, backup_sha256: backupSha, applied: true }
}

function planText(r, dry) {
  const L = []
  L.push(`golden-capture plan${dry ? ' (DRY RUN: nothing written outside the run dir, game not launched)' : ''}`)
  L.push(`profile ${r.profile}; cell ${r.cell} (fallbacks ${r.cell_fallback.join(', ') || 'none'}); CS ${r.cs_version.key} (mod ${r.cs_version.mod})`)
  L.push(`settings: ${r.settings_path}; backup -> ${r.backup}`)
  for (const e of r.edits) L.push(`  line ${e.line}: ${e.path}: ${e.before === null ? 'INSERT' : e.before} -> ${e.after}`)
  if (!r.edits.length) L.push('  (no settings edits)')
  L.push(`captures: ${r.planned_captures}, kept in ${r.fixture_dir}; extractor output ${r.extract_out}`)
  for (const f of r.s6) L.push(`s6 file ${f.path}: ${f.ok ? 'present' : f.exists ? 'sha256 differs' : 'missing'} (never written by this workflow)`)
  L.push(`session: launch MO2 SKSE, wait DevBench :${r.port}, coc ${r.cell}, origin = player position after coc (dry run shows origin 0,0,0)`)
  L.push(`setup console: ${r.setup_console.join(' | ')}`)
  for (const g of r.groups) {
    L.push(`group ${g.label}${g.probe ? ' (probe)' : ''}${g.risky ? ' (risky, last)' : ''}: tags ${g.tags.join(', ') || '-'}`)
    if (g.optional) L.push(`  optional ${g.optional.form}: ${g.optional.used ? 'used' : 'skipped'} (${g.optional.why})`)
    for (const s of groupSteps(g, [0, 0, 0])) L.push(`  ${s.only_if ? `[if ${s.only_if}] ` : ''}${s.tool ? `${s.tool} ${JSON.stringify(s.body)}` : `wait ${s.wait_s}s`}${s.save ? ` => $${s.save}` : ''}${s.note ? `  # ${s.note}` : ''}`)
  }
  L.push('quit: console qqq; then restore settings (byte copy of the backup, sha256 checked), keep each .rdc (copy, sha256, then delete the Captures copy)')
  return L.join('\n') + '\n'
}

async function restore(o) {
  const statePath = path.join(o['run-dir'], 'settings-state.json')
  if (!fs.existsSync(statePath)) return { ok: true, restored: false, nothing_to_restore: true, reason: 'no settings were applied in this run dir' }
  const st = readJson(statePath)
  if (st.restored) return { ok: true, restored: true, already: true, sha256: st.backup_sha256 }
  const running = gameRunning()
  if (running !== false) return { ok: false, restored: false, game_running: running, reason: 'game running (or unknown): CS may rewrite the file on exit; not touching it' }
  if (await sha256(st.backup) !== st.backup_sha256) return { ok: false, restored: false, reason: `backup ${st.backup} changed since preflight; restore by hand` }
  fs.copyFileSync(st.backup, st.settings_path)
  const live = await sha256(st.settings_path)
  if (live !== st.backup_sha256) return { ok: false, restored: false, reason: 'live file sha256 differs from the backup after copying' }
  writeJson(statePath, { ...st, restored: true })
  return { ok: true, restored: true, game_running: false, sha256: live }
}

async function keep(o) {
  const r = readJson(path.join(o['run-dir'], 'resolved.json'))
  const src = o.file
  if (!fs.existsSync(src)) return { ok: false, reason: `${src} not found` }
  fs.mkdirSync(r.fixture_dir, { recursive: true })
  const dst = path.join(r.fixture_dir, path.basename(src))
  const srcSha = await sha256(src)
  if (fs.existsSync(dst) && await sha256(dst) !== srcSha) return { ok: false, reason: `${dst} exists with different content; kept both` }
  if (!fs.existsSync(dst)) fs.copyFileSync(src, dst)
  const dstSha = await sha256(dst)
  if (dstSha !== srcSha) return { ok: false, reason: 'copy sha256 differs; source kept', dst }
  fs.writeFileSync(`${dst}.sha256`, `${dstSha}  ${path.basename(dst)}\n`)
  const manifest = path.join(r.fixture_dir, 'captures.json')
  const list = fs.existsSync(manifest) ? readJson(manifest) : []
  if (!list.some(x => x.file === path.basename(dst))) list.push({ file: path.basename(dst), sha256: dstSha, bytes: fs.statSync(dst).size, profile: r.profile, cell: r.cell, cs: r.cs_version, group: o.group || null, label: o.label || null, frame: o.frame ? +o.frame : null })
  writeJson(manifest, list)
  let deleted = false
  if (!o['keep-source']) { fs.unlinkSync(src); deleted = true }
  return { ok: true, dst, sha256: dstSha, source_deleted: deleted }
}

function parseArgs(argv) {
  const o = { _: [] }
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i]
    if (!a.startsWith('--')) { o._.push(a); continue }
    const k = a.slice(2)
    if (['dry-run', 'keep-source'].includes(k)) o[k] = true
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
    else throw new Error(`unknown command ${cmd}; use preflight | steps | restore | keep`)
  } catch (e) { out = { ok: false, reason: e.message } }
  process.stdout.write(JSON.stringify(out, null, 2) + '\n')
  process.exitCode = out.ok ? 0 : 1
}

if (import.meta.url === pathToFileURL(process.argv[1] || '').href) main()

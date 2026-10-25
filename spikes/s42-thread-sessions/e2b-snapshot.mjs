// S42 q3 (prototype, not for main): can E2B snapshot a box after setup, and
// does a box made from the snapshot start clean? Run by e2b-snapshot.sh.
//
// One throwaway sandbox at a time, all tagged `arugula_spike=s42`, all
// deleted at the end (and listed, to show it), with every snapshot made.
// Nothing here enrolls anywhere: the optional real daemon is pointed at a
// control that isn't there (127.0.0.1:9).
//
//   1. builder  (E2B's `base`): setup-like writes, as `user`:
//        - a fake daemon state directory as an enrolled, set-up box has it
//          (daemon.key, control.json, run-grant-revokes.json + .kept,
//          hosted-setup.json, hosted-box, work/run_1.json);
//        - a secret setup wrote to disk (~/.npmrc, ~/.git-credentials) and
//          one held only in a process's environment (`sleep`);
//        - the real work setup leaves (~/repo/built.txt).
//      Then snapshot S1, full memory, as it is: no wipe.
//   2. child1 from S1: what carried over (files, the process, envs, the
//      sandbox id file, metadata). Then the wipe M263 needs, and, with
//      S42_ARUGULAD, the real `arugulad --hosted` started fresh so it waits
//      for a box of its own. Snapshot S2 (filesystem-only if the team has
//      it, else full: both are tried and said).
//   3. child2 from S2: nothing of the old box's state or secrets; the real
//      daemon (if any) saw a new sandbox id and made keys of its own.
//
// S42_BACKEND=loam runs the same on loam, whose E2B API has no snapshots
// (404): its own API's checkpoints and forks instead (`/v1/sandboxes/{id}/
// checkpoints`, `POST /v1/sandboxes {from}`), which delete with their
// source, so there a source is paused, not deleted, until its fork is made.
//
// Prints one line per check, `ok`/`FAIL`/`info`. Never prints the API key;
// the secret is this script's own random string, printed only as a prefix.
import crypto from 'node:crypto'
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const here = path.dirname(fileURLToPath(import.meta.url))
const { Sandbox } = await import(path.join(here, '../../packaging/hosted/node_modules/e2b/dist/index.mjs'))

const LOAM = process.env.S42_BACKEND === 'loam'
const LOAM_API = 'https://api.loam.inevitable.fyi'
if (!process.env.E2B_API_KEY) {
  console.error('E2B_API_KEY is not set')
  process.exit(1)
}
if (LOAM) process.env.E2B_DOMAIN ||= 'e2b.loam.inevitable.fyi'
async function loam(method, route, body) {
  const r = await fetch(`${LOAM_API}${route}`, {
    method,
    headers: { authorization: `Bearer ${process.env.E2B_API_KEY}`, 'content-type': 'application/json' },
    body: body ? JSON.stringify(body) : undefined,
  })
  const text = await r.text()
  if (!r.ok) throw new Error(`loam ${method} ${route}: ${r.status} ${text.slice(0, 200)}`)
  return text ? JSON.parse(text) : {}
}
const TAG = { arugula_spike: 's42' }
const SECRET = `s42-secret-${crypto.randomBytes(12).toString('hex')}`
const HOME = '/home/user'
const STATE = `${HOME}/.local/state/arugula`
const daemon = process.env.S42_ARUGULAD || ''
const made = { sandboxes: new Set(), snapshots: new Set() }
let failures = 0

const say = (kind, what, detail = '') => {
  if (kind === 'FAIL') failures++
  const d = String(detail).split(SECRET).join('<secret>').replace(/\s+/g, ' ').trim().slice(0, 400)
  console.log(`${kind.padEnd(4)} ${what}${d ? `: ${d}` : ''}`)
}
const check = (ok, what, detail) => say(ok ? 'ok' : 'FAIL', what, detail)

async function sh(sbx, cmd, opts = {}) {
  if (LOAM) {
    const r = await loam('POST', `/v1/sandboxes/${sbx.sandboxId}/exec/run`, {
      cmd: ['bash', '-c', cmd],
      env: opts.envs || {},
      user: opts.user || 'user',
      cwd: HOME,
      timeout_s: 120,
    }).catch((e) => ({ exit_code: -1, stdout: '', stderr: Buffer.from(e.message).toString('base64') }))
    const d = (b) => Buffer.from(b || '', 'base64').toString()
    return { code: r.exit_code, out: d(r.stdout) + d(r.stderr) }
  }
  try {
    const r = await sbx.commands.run(cmd, { timeoutMs: 120_000, ...opts })
    return { code: r.exitCode, out: (r.stdout || '') + (r.stderr || '') }
  } catch (e) {
    return { code: e.exitCode ?? -1, out: `${e.stdout || ''}${e.stderr || ''}${e.message || e}` }
  }
}

// A process left running after the call: E2B's background command, or on
// loam one detached from the one-shot exec.
async function background(sbx, cmd, envs) {
  if (LOAM) return sh(sbx, `setsid nohup bash -c ${JSON.stringify(cmd)} >/dev/null 2>&1 < /dev/null &`, { envs })
  return sbx.commands.run(cmd, { background: true, envs })
}

async function write(sbx, file, bytes) {
  if (LOAM) {
    const r = await fetch(`${LOAM_API}/v1/sandboxes/${sbx.sandboxId}/fs/file?path=${encodeURIComponent(file)}&mode=0755&user=user`, {
      method: 'PUT',
      headers: { authorization: `Bearer ${process.env.E2B_API_KEY}` },
      body: bytes,
    })
    if (!r.ok) throw new Error(`loam upload: ${r.status} ${await r.text()}`)
    return
  }
  await sbx.files.write(file, bytes)
}

async function create(template, envs, name) {
  const t0 = Date.now()
  if (LOAM) {
    const from = template.includes('/') ? { from: { sandbox: template.split('/')[0], checkpoint: template.split('/')[1] } } : { template: template === 'base' ? 'e2b' : template }
    const made_ = await loam('POST', '/v1/sandboxes', { ...from, env: envs, labels: { ...TAG, s42_box: name } })
    made.sandboxes.add(made_.id)
    const sbx = { sandboxId: made_.id }
    say('info', `${name} made`, `${sbx.sandboxId} from ${template} in ${Date.now() - t0} ms`)
    return sbx
  }
  const sbx = await Sandbox.create(template, {
    metadata: { ...TAG, s42_box: name },
    envs,
    timeoutMs: 15 * 60_000,
    secure: true,
    network: { allowPublicTraffic: false },
  })
  made.sandboxes.add(sbx.sandboxId)
  say('info', `${name} made`, `${sbx.sandboxId} from ${template} in ${Date.now() - t0} ms`)
  return sbx
}

async function kill(sbx, name) {
  if (LOAM && name !== 'child2') {
    // loam: a sandbox's checkpoints go with it, so a source is paused until
    // the end, when everything is deleted.
    await loam('POST', `/v1/sandboxes/${sbx.sandboxId}/pause`)
    say('info', `${name} paused (loam: deleted after its fork)`, sbx.sandboxId)
    return
  }
  const ok = LOAM
    ? await loam('DELETE', `/v1/sandboxes/${sbx.sandboxId}`).then(() => true, (e) => `error ${e.message}`)
    : await Sandbox.kill(sbx.sandboxId).catch((e) => `error ${e.message}`)
  if (ok === true) made.sandboxes.delete(sbx.sandboxId)
  say('info', `${name} deleted`, `${sbx.sandboxId} ${ok}`)
}

async function snapshot(sbx, mode) {
  const t0 = Date.now()
  const opts = mode ? { mode } : {}
  const s = LOAM
    ? await loam('POST', `/v1/sandboxes/${sbx.sandboxId}/checkpoints`, { comment: 's42', memory: mode !== 'filesystem' }).then((c) => ({
        snapshotId: `${sbx.sandboxId}/${c.id}`,
        names: [c.id, `memory=${c.memory}`],
      }))
    : await sbx.createSnapshot(opts)
  made.snapshots.add(s.snapshotId)
  say('info', `snapshot (${mode || 'full'})`, `${s.snapshotId} names=${JSON.stringify(s.names)} in ${Date.now() - t0} ms`)
  return s
}

// What a box looks like from inside: one JSON object.
const LOOK = `
id=$(cat /run/e2b/.E2B_SANDBOX_ID 2>/dev/null)
st=${STATE}
f() { [ -e "$1" ] && echo 1 || echo 0; }
probe="$S42_PROBE_A$S42_PROBE_B"
proc=$(pgrep -f '^s42-holds-secret' >/dev/null && echo 1 || echo 0)
envsec=0; for e in /proc/[0-9]*/environ; do grep -qaF "$probe" "$e" 2>/dev/null && envsec=1; done
disksec=$(grep -rlaF --exclude-dir=proc --exclude-dir=sys --exclude-dir=dev "$probe" / 2>/dev/null | head -5 | tr '\\n' ' ')
key=$(sha256sum "$st/daemon.key" 2>/dev/null | cut -c1-12)
printf '{"id":"%s","uptime":"%s","pid1":"%s","state_dir":%s,"daemon_key":"%s","control_json":%s,"revokes":%s,"setup_json":%s,"box_note":"%s","ledger":%s,"npmrc":%s,"built":%s,"secret_proc":%s,"secret_in_environ":%s,"secret_on_disk":"%s","create_env":"%s","arugulad":"%s"}\\n' \
  "$id" "$(cut -d' ' -f1 /proc/uptime)" "$(tr '\\0' ' ' < /proc/1/cmdline)" "$(f $st)" "$key" "$(f $st/control.json)" \
  "$(f $st/run-grant-revokes.json)" "$(f $st/hosted-setup.json)" "$(cat $st/hosted-box 2>/dev/null)" "$(f $st/work/run_1.json)" \
  "$(f ${HOME}/.npmrc)" "$(f ${HOME}/repo/built.txt)" "$proc" "$envsec" "$disksec" "$S42_CREATE_ENV" "$(pgrep -x arugulad | head -1)"
`

// The secret to look for, in two halves the shell joins: passed per
// command, so no process's environment holds the whole of it (the look's
// own would otherwise be found).
const probe = () => ({ S42_PROBE_A: SECRET.slice(0, 8), S42_PROBE_B: SECRET.slice(8) })

async function look(sbx, name) {
  // As root, with no sudo: sudo logs its command lines to the journal,
  // which would put the probe there.
  const r = await sh(sbx, LOOK, { envs: probe(), user: 'root' })
  let j
  try {
    j = JSON.parse(r.out.trim().split('\n').pop())
  } catch {
    say('FAIL', `${name} look`, r.out)
    return {}
  }
  say('info', `${name} look`, JSON.stringify(j))
  return j
}

async function sandboxesLeft() {
  if (LOAM) {
    const r = await loam('GET', '/v1/sandboxes?label=arugula_spike=s42')
    return (r.sandboxes || []).map((x) => ({ sandboxId: x.id }))
  }
  const p = Sandbox.list({ query: { metadata: TAG, state: ['running', 'paused'] } })
  const all = []
  while (p.hasNext) all.push(...(await p.nextItems()))
  return all
}

async function snapshotsLeft() {
  if (LOAM) return [] // a sandbox's checkpoints go with it
  const out = []
  for (const id of made.snapshots) {
    const p = Sandbox.listSnapshots({ name: id })
    while (p.hasNext) out.push(...(await p.nextItems()))
  }
  return out
}

async function main() {
  const before = await sandboxesLeft()
  check(before.length === 0, 'no s42 sandboxes before', before.map((s) => s.sandboxId).join(' '))

  // 1. The builder.
  const b = await create('base', { S42_CREATE_ENV: 'builder' }, 'builder')
  say('info', 'builder kernel and init', (await sh(b, 'uname -r; ps -o pid,user,cmd -p 1; id; sudo -n true && echo sudo-ok')).out)
  const setup = `
set -e
mkdir -p ${STATE}/work ${HOME}/repo && chmod 700 ${STATE}
head -c 64 /dev/urandom > ${STATE}/daemon.key
echo '{"control":"https://control.invalid","hosted":{"cert":{"device":"old-builder"}}}' > ${STATE}/control.json
echo '[{"revoke":"old"}]' > ${STATE}/run-grant-revokes.json; touch ${STATE}/run-grant-revokes.kept
echo '{"spec":"old","step":"ready"}' > ${STATE}/hosted-setup.json
cat /run/e2b/.E2B_SANDBOX_ID > ${STATE}/hosted-box
echo '{"state":"running"}' > ${STATE}/work/run_1.json
printf '//registry.npmjs.org/:_authToken=%s\\n' "$S42_SETUP_SECRET" > ${HOME}/.npmrc
printf 'https://x-access-token:%s@github.com\\n' "$S42_SETUP_SECRET" > ${HOME}/.git-credentials
echo built > ${HOME}/repo/built.txt
# A setup step that gives sudo the secret on its command line, as setup
# scripts do (sudo logs its command lines to the journal).
sudo -n sh -c "printf 'token=%s\\n' $S42_SETUP_SECRET > /etc/s42-app.conf"
`
  const w = await sh(b, setup, { envs: { S42_SETUP_SECRET: SECRET } })
  check(w.code === 0, 'builder: setup-like writes', w.out)
  // A process that holds the secret in its environment only.
  await background(b, `exec -a s42-holds-secret sleep 3600`, { S42_SECRET_HELD: SECRET })
  await new Promise((r) => setTimeout(r, 500))
  const lb = await look(b, 'builder')
  check(lb.secret_proc === 1 && lb.secret_in_environ === 1, 'builder holds the secret in a process')
  // Which journal entries hold it: who wrote them, the secret masked.
  const SRC = `
probe="$S42_PROBE_A$S42_PROBE_B"
command -v journalctl >/dev/null || { echo "no journald"; exit 0; }
journalctl -o json --no-pager 2>/dev/null | grep -aF -- "$probe" | python3 -c '
import json, os, sys
p = os.environ["S42_PROBE_A"] + os.environ["S42_PROBE_B"]
for line in sys.stdin:
    e = json.loads(line)
    m = e.get("MESSAGE", "")
    m = m if isinstance(m, str) else "<binary>"
    print({k: e.get(k) for k in ("SYSLOG_IDENTIFIER", "_COMM", "_EXE", "_SYSTEMD_UNIT", "_SYSTEMD_USER_UNIT", "_UID")}, m.replace(p, "<secret>")[:160])
'`
  say('info', 'builder: journal entries holding the secret', (await sh(b, SRC, { envs: probe(), user: 'root' })).out)
  const s1 = await snapshot(b, null)
  await kill(b, 'builder')

  // 2. child1, from the unwiped snapshot: what a snapshot carries.
  const c1 = await create(s1.snapshotId, { S42_CREATE_ENV: 'child1' }, 'child1')
  const l1 = await look(c1, 'child1')
  check(l1.id && l1.id !== lb.id, 'child1: the sandbox id file names the new box', `${lb.id} -> ${l1.id}`)
  say('info', 'child1: memory carried (process from the builder still running)', l1.secret_proc)
  say('info', 'child1: create env seen by a new command', l1.create_env)
  say('info', 'child1: unwiped, old state carried', `key=${l1.daemon_key} control=${l1.control_json} revokes=${l1.revokes} setup=${l1.setup_json} note=${l1.box_note} disk=${l1.secret_on_disk}`)
  const info1 = LOAM ? await loam('GET', `/v1/sandboxes/${c1.sandboxId}`) : await Sandbox.getInfo(c1.sandboxId).catch((e) => ({ error: e.message }))
  say('info', 'child1: metadata and template', JSON.stringify(LOAM ? { labels: info1.labels, env: info1.env, from: info1.from, template: info1.template } : { metadata: info1.metadata, templateId: info1.templateId, name: info1.name, error: info1.error }))

  // The wipe (M263): stop the daemon, take the whole state directory, the
  // files that hold a secret setup was given, and every process that holds
  // one; then, with a daemon, start it fresh so it waits for a box of its
  // own, as the template's does.
  const wipe = `
set -u
probe="$S42_PROBE_A$S42_PROBE_B"
pkill -x arugulad; sleep 0.5
rm -rf ${STATE}
# The journal first: it holds what sudo and others logged, and journald
# keeps its file open (removing it isn't enough). Rotate, then drop every
# archived file; if a hit is left, stop journald and remove them all.
if command -v journalctl >/dev/null; then
  journalctl --rotate >/dev/null 2>&1; journalctl --vacuum-files=1 >/dev/null 2>&1
  if grep -rlaqF -- "$probe" /var/log/journal /run/log/journal 2>/dev/null; then
    systemctl stop systemd-journald.socket systemd-journald-dev-log.socket systemd-journald-audit.socket systemd-journald.service 2>/dev/null
    rm -rf /var/log/journal/*/* /run/log/journal/*/*; echo "journal: stopped, removed, started"
    systemctl start systemd-journald.service 2>/dev/null
  else echo "journal: rotated and vacuumed"; fi
fi
for f in $(grep -rlaF --exclude-dir=proc --exclude-dir=sys --exclude-dir=dev -- "$probe" / 2>/dev/null); do echo "removed $f"; rm -f -- "$f"; done
for e in /proc/[0-9]*/environ; do p=\${e#/proc/}; p=\${p%/environ}; [ "$p" = $$ ] && continue; grep -qaF -- "$probe" "$e" 2>/dev/null && { echo "killed $p $(tr '\\0' ' ' < /proc/$p/cmdline)"; kill -9 "$p"; }; done
rm -f ${HOME}/.bash_history /root/.bash_history; sync
`
  say('info', 'child1: wipe', (await sh(c1, wipe, { envs: probe(), user: 'root' })).out)
  if (daemon) {
    await write(c1, `${HOME}/arugulad`, fs.readFileSync(daemon))
    await sh(c1, `chmod 755 ${HOME}/arugulad`)
    await background(c1, `exec ${HOME}/arugulad --hosted > ${HOME}/arugulad.log 2>&1`, {
      ARUGULA_CONTROL_URL: 'http://127.0.0.1:9',
      RUST_LOG: 'info',
    })
    await new Promise((r) => setTimeout(r, 3000))
    say('info', 'child1: arugulad started fresh', (await sh(c1, `pgrep -x arugulad; tail -5 ${HOME}/arugulad.log; ls -a ${STATE} 2>&1 | tr '\\n' ' '`)).out)
  }
  const lw = await look(c1, 'child1 wiped')
  check(!lw.daemon_key && !lw.control_json && !lw.revokes && !lw.secret_on_disk && lw.secret_in_environ === 0, 'child1: nothing left to carry')
  // Both kinds of snapshot of the wiped box: a full one has the fresh
  // daemon running, waiting; a filesystem-only one boots from disk.
  const s2 = {}
  for (const mode of ['full', 'filesystem']) {
    try {
      s2[mode] = await snapshot(c1, mode === 'full' ? null : mode)
    } catch (e) {
      say('info', `${mode} snapshot refused`, `${e.statusCode || ''} ${e.message}`)
    }
  }
  await kill(c1, 'child1')

  // 3. child2, from each wiped snapshot.
  for (const [mode, snap] of Object.entries(s2)) {
    const name = `child2 (${mode})`
    const c2 = await create(snap.snapshotId, { S42_CREATE_ENV: 'child2' }, name)
    if (daemon) await new Promise((r) => setTimeout(r, 6000))
    const l2 = await look(c2, name)
    check(l2.id && l2.id !== lw.id, `${name}: the sandbox id file names the new box`, `${lw.id} -> ${l2.id}`)
    check(!l2.control_json && !l2.revokes && !l2.setup_json && !l2.ledger, `${name}: no old daemon state (control.json, revokes, setup, ledger)`)
    check(!l2.secret_on_disk && l2.secret_in_environ === 0 && l2.secret_proc === 0, `${name}: no secret on disk or in a process`)
    check(l2.built === 1, `${name}: setup's real work is there`)
    if (daemon) {
      const d = await sh(c2, `pgrep -x arugulad; grep -aE 'new box|waiting for a box|starting over|enroll|reach control' ${HOME}/arugulad.log | tail -8`)
      say('info', `${name}: arugulad`, d.out)
      const ran = l2.arugulad !== ''
      ;(mode === 'full' ? check : (ok, what, d) => say('info', what, `${ok}${d ? ` ${d}` : ''}`))(ran, `${name}: arugulad running`, mode === 'full' ? '' : 'nothing starts it at boot here')
      if (ran) {
        check(l2.box_note === l2.id, `${name}: the daemon noted the new box as its own`, `${l2.box_note} vs ${l2.id}`)
        check(!!l2.daemon_key && l2.daemon_key !== lb.daemon_key, `${name}: a daemon key of its own, made in the new box`, `${l2.daemon_key} (old ${lb.daemon_key})`)
      }
    }
    await kill(c2, 'child2')
  }
}

try {
  await main()
} catch (e) {
  say('FAIL', 'the run stopped', e.stack || e.message)
} finally {
  // Everything this made, gone, and listed to show it.
  for (const s of await sandboxesLeft().catch(() => [])) {
    const gone = LOAM
      ? await loam('DELETE', `/v1/sandboxes/${s.sandboxId}`).then(() => true, (e) => e.message)
      : await Sandbox.kill(s.sandboxId).catch((e) => e.message)
    say('info', 'cleanup: delete sandbox', `${s.sandboxId} ${gone}`)
  }
  for (const id of LOAM ? [] : made.snapshots) {
    say('info', 'cleanup: delete snapshot', `${id} ${await Sandbox.deleteSnapshot(id).catch((e) => e.message)}`)
  }
  const sb = await sandboxesLeft().catch((e) => [{ sandboxId: `list failed: ${e.message}` }])
  const sn = await snapshotsLeft().catch((e) => [{ snapshotId: `list failed: ${e.message}` }])
  check(sb.length === 0, 'cleanup: s42 sandboxes left', sb.map((s) => s.sandboxId).join(' ') || 'none')
  check(sn.length === 0, 'cleanup: s42 snapshots left', sn.map((s) => s.snapshotId).join(' ') || 'none')
  console.log(failures ? `FAILED ${failures}` : 'PASS')
  process.exit(failures ? 1 : 0)
}

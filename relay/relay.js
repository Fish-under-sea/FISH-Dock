// Wakuwaku's crash relay. Her exe sends a crash report here (POST /report);
// this checks it, takes out again anything that could be the person's, and
// opens an issue for it on GitHub or Gitee, or adds to the issue already
// open for the same crash. The token to do that lives here, never in her exe
// (anyone can take a token out of a public download).
//
// As a Cloudflare Worker: this one file (wrangler.toml; or pasted into the
// dashboard). Elsewhere (Aliyun Function Compute, any server): server.js
// runs handle() under Node. relay/README.md says how to set either up.
//
// Settings (a Worker's variables and secrets; under Node, the environment):
//   FORGE            github (as it is) or gitee
//   REPO             owner/name: zhoupengjie/Wakuwaku
//   GITHUB_APP_ID, GITHUB_APP_KEY   a GitHub App's id and private key (PEM),
//                    installed on REPO with Issues read and write: issues
//                    come from the app's bot, and you hear of them
//   GITHUB_TOKEN     or a fine-grained token (REPO only, Issues read and
//                    write): issues come from you, and GitHub tells you
//                    nothing of your own
//   GITEE_TOKEN      a Gitee personal access token (issues)
//   SALT             any random words: an address is only counted, hashed with them
//   PER_IP_DAY       reports one address may send a day (20)
//   NEW_PER_DAY      new issues a day, from everyone (30)
//   COMMENTS         comments on one issue; after that only its count goes up,
//                    unless a version not seen before (10)
// A Worker's bindings, both optional: CRASH (a KV namespace: the counts, and
// which issue each crash has), PER_IP (a rate limit: 5 a minute per address).
//
// A report is what crash.rs makes (src-tauri/src/crash.rs): every field is
// checked by check() and anything else refused. Its free text (where, the
// message, the log) is scrubbed again (scrub()) and goes into code blocks.
// The same crash is the same signature (signature()): its kind and where (a
// panic's file and line, a native crash's code and module+offset), its tag
// [crash:<10 hex>] in the issue's title. A crash on an issue since closed,
// from a newer version than any seen on it, opens a new one (again).

const LIMIT = 48 * 1024
const KINDS = ['panic', 'native', 'exit', 'webview']

// --- The report --------------------------------------------------------------------

const ok = (v, re) => typeof v === 'string' && re.test(v)
const text = max => v => typeof v === 'string' && v.length <= max && !/[\u0000-\u001f\u007f]/.test(v)
const FIELDS = {
  v: v => v === 1,
  app: v => ok(v, /^\d{1,4}\.\d{1,4}\.\d{1,6}(-[0-9A-Za-z.-]{1,20})?$/),
  kind: v => KINDS.includes(v),
  // An exit code, or WebView2's kind/reason/exit code.
  code: v => ok(v, /^(0x[0-9A-F]{8}|[a-z-]{3,30}\/[a-z-]{3,20}(\/0x[0-9A-F]{8})?)$/),
  where: text(300),
  message: text(400),
  detail: text(300),
  frames: v => Array.isArray(v) && v.length <= 32 && v.every(f => ok(f, /^[A-Za-z0-9_.-]{1,64}\+0x[0-9a-f]{1,12}$/)),
  os: v => ok(v, /^[A-Za-z0-9 .()_-]{1,80}$/),
  webview2: v => ok(v, /^[0-9.]{1,24}$/),
  home: v => v === 'island' || v === 'taskbar',
  out: v => typeof v === 'boolean',
  lang: v => v === 'zh' || v === 'en',
  uptime: v => Number.isInteger(v) && v >= 0 && v <= 1e9,
  at: v => ok(v, /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}Z$/),
  log: v => Array.isArray(v) && v.length <= 80 && v.every(text(400)),
}
const REQUIRED = ['v', 'app', 'kind', 'at']

// What is wrong with a report, or null.
export function check(report) {
  if (!report || typeof report !== 'object' || Array.isArray(report)) return 'not an object'
  for (const key of REQUIRED) if (!(key in report)) return `${key} missing`
  for (const [key, value] of Object.entries(report)) {
    if (!FIELDS[key]) return `${key}: not a field`
    if (!FIELDS[key](value)) return `${key}: not as it should be`
  }
  return null
}

const OUR_HOSTS = ['github.com', 'api.github.com', 'gitee.com', 'codex-pets.net', 'localhost', '127.0.0.1']

// Anything that could still be the person's, taken out (her exe took it
// out already; this is in case it missed some): mail addresses, a user's
// folder, URLs' hosts and paths, ids, tokens, addresses, words outside ASCII.
export function scrub(s) {
  return String(s)
    .replace(/[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}/g, '<email>')
    .replace(/\b[A-Za-z]:[\\/]+(?:Users|Documents and Settings)[\\/]+[^\\/\s"']+/gi, '%USERPROFILE%')
    .replace(/\b([a-z][a-z0-9+.-]{1,9}):\/\/([^\s/"'<>()]+)([^\s"'<>()]*)/gi, (_, scheme, host, rest) => {
      const name = host.split('@').pop().split(':')[0]
      const path = rest.replace(/[:.,;]+$/, '')
      return `${scheme}://${OUR_HOSTS.includes(name.toLowerCase()) ? name : '<host>'}${path.length > 1 ? '/…' : ''}${rest.slice(path.length)}`
    })
    .replace(/\b[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\b/gi, '<id>')
    .replace(/\b(?:sk-[A-Za-z0-9_-]{8,}|gh[pousr]_[A-Za-z0-9]{10,}|github_pat_[A-Za-z0-9_]{10,}|xox[abprs]-[A-Za-z0-9-]{8,}|glpat-[A-Za-z0-9_-]{10,}|AKIA[0-9A-Z]{16}|AIza[0-9A-Za-z_-]{30,}|eyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]*)/g, '<token>')
    .replace(/[A-Za-z0-9_+/=-]{20,}/g, w => ((w.match(/\d/g) || []).length >= 2 && /[A-Z]/.test(w) && /[a-z]/.test(w) ? '<token>' : w))
    .replace(/\b(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})\b/g, (all, ...p) => (p.slice(0, 4).some(n => Number(n) > 255) || all === '127.0.0.1' || all === '0.0.0.0' ? all : '<ip>'))
    .replace(/[\p{L}\p{N}_]*[^\x00-\x7F](?<=[\p{L}\p{N}])[\p{L}\p{N}_]*/gu, '‹…›')
}

// Safe in a code block: its fence never closed early, nobody mentioned.
const block = s => scrub(s).replace(/~~~/g, '~~ ~').replace(/@/g, '@​')
// Safe in a table cell or a title.
const cell = s => scrub(s).replace(/[|`@<>\\]/g, c => ({ '|': '\\|', '`': "'", '@': '', '<': '‹', '>': '›', '\\': '/' })[c])

// A panic's place without its column; slashes one way.
const place = s => String(s || '').replace(/\\/g, '/').replace(/:(\d+):\d+$/, ':$1')

// The same crash, the same signature.
export function signature(r) {
  if (r.kind === 'panic') return `panic ${place(r.where)}`
  if (r.kind === 'native') return `native ${r.code || ''} ${r.where || ''}`.trim()
  return `${r.kind} ${r.code || ''}`.trim()
}

async function sha256(s) {
  const bytes = new Uint8Array(await crypto.subtle.digest('SHA-256', new TextEncoder().encode(s)))
  return [...bytes].map(b => b.toString(16).padStart(2, '0')).join('')
}

export async function tagOf(report) {
  return `crash:${(await sha256(signature(report))).slice(0, 10)}`
}

export function newer(a, b) {
  const parts = v => String(v || '0').replace(/^v/i, '').split('-')[0].split('.').map(n => Number(n) || 0)
  const [x, y] = [parts(a), parts(b)]
  for (let i = 0; i < Math.max(x.length, y.length); i++) if ((x[i] || 0) !== (y[i] || 0)) return (x[i] || 0) > (y[i] || 0)
  return false
}

const uptime = s => (s == null ? '' : s < 60 ? `${s} s` : s < 3600 ? `${Math.floor(s / 60)} min` : `${Math.floor(s / 3600)} h ${Math.floor((s % 3600) / 60)} min`)

function countLine(c) {
  return `<!-- count -->**Reports: ${c.count}** · versions: ${c.versions.map(cell).join(', ')} · last: ${cell(c.last)}<!-- /count -->`
}

function details(r) {
  const parts = []
  if (r.message) parts.push(`**Message**\n~~~text\n${block(r.message)}\n~~~`)
  if (r.frames?.length) parts.push(`**Stack** (module+offset, newest first)\n~~~text\n${r.frames.join('\n')}\n~~~`)
  if (r.log?.length) parts.push(`**Log** (the lines before, the last one nearest the crash)\n~~~text\n${r.log.map(block).join('\n')}\n~~~`)
  return parts.join('\n\n')
}

// An issue's title and body for a report (`again`: the closed issue it came back after).
export function render(r, tag, again) {
  const where = r.where ? ` at ${cell(r.where).slice(0, 80)}` : ''
  const code = r.code ? ` (${cell(r.code)})` : ''
  const title = `Crash: ${r.kind}${where}${code} [${tag}]`
  const rows = [
    ['Kind', r.kind],
    ['Code', r.code && `\`${cell(r.code)}\``],
    ['Where', r.where && `\`${cell(r.where)}\``],
    ['Detail', r.detail && cell(r.detail)],
    ['Version', cell(r.app)],
    ['Windows', r.os && cell(r.os)],
    ['WebView2', r.webview2 && cell(r.webview2)],
    ['Home', r.home && `${r.home}${r.out ? ' (she was out)' : ''}`],
    ['Up for', uptime(r.uptime)],
    ['Signature', `\`${cell(signature(r))}\``],
  ].filter(([, v]) => v)
  const body = [
    '<!-- wakuwaku crash report -->',
    'Sent by Wakuwaku by itself after a crash (with the person’s yes), user names, paths, window titles, mail addresses and the like taken out. The same crash again adds a comment here, or only to the count.',
    again ? `\nSeen again after ${again} was closed.` : '',
    '',
    countLine({ count: 1, versions: [r.app], last: r.at }),
    '',
    '| | |\n|---|---|\n' + rows.map(([k, v]) => `| ${k} | ${v} |`).join('\n'),
    '',
    details(r),
  ].join('\n')
  return { title, body }
}

function comment(r) {
  const head = `**Again**: v${cell(r.app)}${r.os ? ` · ${cell(r.os)}` : ''}${r.uptime != null ? ` · up ${uptime(r.uptime)}` : ''} · ${cell(r.at)}${r.detail ? ` · ${cell(r.detail)}` : ''}`
  const more = details(r)
  return more ? `${head}\n\n<details><summary>Details</summary>\n\n${more}\n\n</details>` : head
}

// --- Where the issues go -----------------------------------------------------------------

async function call(fetcher, url, init, what) {
  const res = await fetcher(url, init)
  if (!res.ok) {
    const detail = (await res.text().catch(() => '')).slice(0, 200)
    throw Object.assign(new Error(`${what}: ${res.status} ${detail}`), { status: res.status })
  }
  return res.status === 204 ? {} : res.json()
}

function b64url(bytes) {
  let s = ''
  for (const b of bytes) s += String.fromCharCode(b)
  return btoa(s).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '')
}

function der(tag, content) {
  const n = content.length
  const len = n < 128 ? [n] : n < 256 ? [0x81, n] : n < 65536 ? [0x82, n >> 8, n & 255] : [0x83, n >> 16, (n >> 8) & 255, n & 255]
  return new Uint8Array([tag, ...len, ...content])
}

// A PEM private key as PKCS#8 (what GitHub gives is PKCS#1: wrapped).
function pkcs8(pem) {
  const isPkcs1 = /BEGIN RSA PRIVATE KEY/.test(pem)
  const raw = Uint8Array.from(atob(pem.replace(/-----[^-]+-----/g, '').replace(/\s+/g, '')), c => c.charCodeAt(0))
  if (!isPkcs1) return raw
  const version = [0x02, 0x01, 0x00]
  const rsa = [0x30, 0x0d, 0x06, 0x09, 0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x01, 0x05, 0x00]
  return der(0x30, [...version, ...rsa, ...der(0x04, raw)])
}

// A GitHub App's JWT (ten minutes), signed with its key.
export async function appJwt(appId, pem, now = Math.floor(Date.now() / 1000)) {
  const key = await crypto.subtle.importKey('pkcs8', pkcs8(pem), { name: 'RSASSA-PKCS1-v1_5', hash: 'SHA-256' }, false, ['sign'])
  const enc = o => b64url(new TextEncoder().encode(JSON.stringify(o)))
  const head = `${enc({ alg: 'RS256', typ: 'JWT' })}.${enc({ iat: now - 60, exp: now + 540, iss: String(appId) })}`
  const sig = new Uint8Array(await crypto.subtle.sign('RSASSA-PKCS1-v1_5', key, new TextEncoder().encode(head)))
  return `${head}.${b64url(sig)}`
}

let appToken = null

export function github(env, fetcher = fetch) {
  const api = 'https://api.github.com'
  const repo = env.REPO
  const headers = token => ({ authorization: `Bearer ${token}`, accept: 'application/vnd.github+json', 'x-github-api-version': '2022-11-28', 'user-agent': 'wakuwaku-crash-relay', 'content-type': 'application/json' })
  async function token() {
    if (env.GITHUB_APP_ID && env.GITHUB_APP_KEY) {
      if (appToken && appToken.until > Date.now() + 60_000) return appToken.token
      const jwt = await appJwt(env.GITHUB_APP_ID, env.GITHUB_APP_KEY)
      const install = await call(fetcher, `${api}/repos/${repo}/installation`, { headers: headers(jwt) }, 'installation')
      const got = await call(fetcher, `${api}/app/installations/${install.id}/access_tokens`, { method: 'POST', headers: headers(jwt) }, 'access token')
      appToken = { token: got.token, until: Date.parse(got.expires_at) || Date.now() + 50 * 60_000 }
      return appToken.token
    }
    if (!env.GITHUB_TOKEN) throw new Error('neither GITHUB_APP_ID/GITHUB_APP_KEY nor GITHUB_TOKEN is set')
    return env.GITHUB_TOKEN
  }
  const go = async (path, init = {}, what = path) => call(fetcher, `${api}${path}`, { ...init, headers: headers(await token()) }, what)
  const issue = i => ({ number: i.number, url: i.html_url, state: i.state === 'closed' ? 'closed' : 'open', body: i.body || '' })
  return {
    name: 'github',
    async find(tag) {
      const q = encodeURIComponent(`repo:${repo} is:issue in:title "${tag}"`)
      const found = await go(`/search/issues?q=${q}&per_page=5`, {}, 'search')
      const hits = (found.items || []).filter(i => (i.title || '').includes(`[${tag}]`)).map(issue)
      return hits.find(i => i.state === 'open') || hits[0] || null
    },
    async get(number) {
      return issue(await go(`/repos/${repo}/issues/${number}`, {}, 'issue'))
    },
    async create(title, body) {
      return issue(await go(`/repos/${repo}/issues`, { method: 'POST', body: JSON.stringify({ title, body, labels: ['crash'] }) }, 'create'))
    },
    async comment(number, body) {
      await go(`/repos/${repo}/issues/${number}/comments`, { method: 'POST', body: JSON.stringify({ body }) }, 'comment')
    },
    async edit(number, body) {
      await go(`/repos/${repo}/issues/${number}`, { method: 'PATCH', body: JSON.stringify({ body }) }, 'edit')
    },
  }
}

export function gitee(env, fetcher = fetch) {
  const api = 'https://gitee.com/api/v5'
  const [owner, name] = String(env.REPO || '').split('/')
  if (!env.GITEE_TOKEN) throw new Error('GITEE_TOKEN is not set')
  const headers = { authorization: `token ${env.GITEE_TOKEN}`, 'content-type': 'application/json;charset=UTF-8', 'user-agent': 'wakuwaku-crash-relay' }
  const go = (path, init = {}, what = path) => call(fetcher, `${api}${path}`, { ...init, headers }, what)
  // Gitee's states: open, progressing, closed, rejected.
  const issue = i => ({ number: i.number, url: i.html_url, state: i.state === 'closed' || i.state === 'rejected' ? 'closed' : 'open', body: i.body || '' })
  return {
    name: 'gitee',
    // Its list read (300 newest at most): Gitee has no search to rely on for this.
    async find(tag) {
      for (let page = 1; page <= 3; page++) {
        const list = await go(`/repos/${owner}/${name}/issues?state=all&sort=created&direction=desc&per_page=100&page=${page}`, {}, 'list')
        const hits = list.filter(i => (i.title || '').includes(`[${tag}]`)).map(issue)
        if (hits.length) return hits.find(i => i.state === 'open') || hits[0]
        if (list.length < 100) break
      }
      return null
    },
    async get(number) {
      return issue(await go(`/repos/${owner}/${name}/issues/${number}`, {}, 'issue'))
    },
    // No labels: Gitee may refuse them from someone not in the repo (the bot).
    async create(title, body) {
      return issue(await go(`/repos/${owner}/issues`, { method: 'POST', body: JSON.stringify({ repo: name, title, body }) }, 'create'))
    },
    async comment(number, body) {
      await go(`/repos/${owner}/${name}/issues/${number}/comments`, { method: 'POST', body: JSON.stringify({ body }) }, 'comment')
    },
    async edit(number, body) {
      await go(`/repos/${owner}/issues/${number}`, { method: 'PATCH', body: JSON.stringify({ repo: name, body }) }, 'edit')
    },
  }
}

// --- What is kept between reports ----------------------------------------------------------

// In memory (under Node, and tests): lost when the process goes.
export function memoryStore() {
  const map = new Map()
  return {
    async get(key) {
      const v = map.get(key)
      if (!v) return null
      if (v.until < Date.now()) return map.delete(key), null
      return v.value
    },
    async put(key, value, seconds) {
      map.set(key, { value, until: Date.now() + seconds * 1000 })
    },
  }
}

// A Worker's KV namespace (none: kept nowhere, every report looked up).
export function kvStore(kv) {
  if (!kv) return memoryStore()
  return {
    get: key => kv.get(key),
    put: (key, value, seconds) => kv.put(key, value, { expirationTtl: Math.max(60, seconds) }),
  }
}

const DAY = 86400
const num = (v, fallback) => (Number.isFinite(Number(v)) && v !== undefined && v !== '' ? Number(v) : fallback)

async function bump(store, key, seconds) {
  const n = Number((await store.get(key)) || 0) + 1
  await store.put(key, String(n), seconds)
  return n
}

// --- A request -----------------------------------------------------------------------------

const answer = (status, json) => ({ status, json })

// { method, path, ip, header(name), body (text) } → { status, json }.
// `forge`: where the issues go (made from env when not given).
export async function handle(req, env, store, forge) {
  const path = String(req.path || '/').split('?')[0]
  if (req.method === 'GET' && (path === '/' || path === '')) return answer(200, { ok: true, relay: 'wakuwaku crash reports' })
  if (!path.endsWith('/report')) return answer(404, { ok: false, error: 'not here' })
  if (req.method !== 'POST') return answer(405, { ok: false, error: 'POST only' })
  if (req.header('x-wakuwaku') !== '1') return answer(400, { ok: false, error: 'not from Wakuwaku' })
  const body = String(req.body || '')
  if (body.length > LIMIT) return answer(413, { ok: false, error: 'too big' })
  let report
  try {
    report = JSON.parse(body)
  } catch {
    return answer(400, { ok: false, error: 'not JSON' })
  }
  const wrong = check(report)
  if (wrong) return answer(400, { ok: false, error: wrong })

  // One address: so many a minute (the binding), so many a day.
  const ip = String(req.ip || 'unknown')
  if (env.PER_IP?.limit) {
    const { success } = await env.PER_IP.limit({ key: ip })
    if (!success) return answer(429, { ok: false, error: 'too many: wait a minute' })
  }
  const day = new Date().toISOString().slice(0, 10)
  const who = (await sha256(`${env.SALT || ''}|${ip}`)).slice(0, 16)
  if ((await bump(store, `ip:${who}:${day}`, 2 * DAY)) > num(env.PER_IP_DAY, 20)) return answer(429, { ok: false, error: 'too many today' })

  try {
    forge = forge || (env.FORGE === 'gitee' ? gitee(env) : github(env))
    return await file(report, env, store, forge, day)
  } catch (e) {
    return answer(502, { ok: false, error: String(e.message || e).slice(0, 300) })
  }
}

async function file(r, env, store, forge, day) {
  const tag = await tagOf(r)
  const key = `sig:${forge.name}:${tag}`
  let kept = JSON.parse((await store.get(key)) || 'null')
  let issue = null
  if (kept) {
    // Its issue since deleted: as if never filed.
    issue = await forge.get(kept.number).catch(e => {
      if (e.status === 404 || e.status === 410) return null
      throw e
    })
    if (!issue) kept = null
  }
  if (!kept) {
    issue = await forge.find(tag)
    if (issue) {
      const count = Number((issue.body.match(/\*\*Reports: (\d+)\*\*/) || [])[1] || 1)
      const versions = ((issue.body.match(/· versions: ([^·]*) ·/) || [])[1] || '').split(',').map(v => v.trim()).filter(Boolean)
      kept = { number: issue.number, url: issue.url, count, versions, max: versions.reduce((m, v) => (newer(v, m) ? v : m), '0') }
    }
  }
  const save = () => store.put(key, JSON.stringify(kept), 180 * DAY)

  if (issue && issue.state === 'closed' && !newer(r.app, kept.max)) {
    // Fixed, and this one from a version it was fixed after: counted only.
    kept.count++
    await save()
    return answer(200, { ok: true, url: kept.url, number: kept.number, count: kept.count, closed: true })
  }
  if (issue && issue.state !== 'closed') {
    const isNewVersion = !kept.versions.includes(r.app)
    kept.count++
    if (isNewVersion) kept.versions.push(r.app)
    if (newer(r.app, kept.max)) kept.max = r.app
    const body = issue.body.replace(/<!-- count -->[\s\S]*?<!-- \/count -->/, countLine({ count: kept.count, versions: kept.versions, last: r.at }))
    if (body !== issue.body) await forge.edit(kept.number, body)
    if (kept.count <= num(env.COMMENTS, 10) || isNewVersion) await forge.comment(kept.number, comment(r))
    await save()
    return answer(200, { ok: true, url: kept.url, number: kept.number, count: kept.count })
  }

  // New (or back after its issue was closed): within the day's new issues.
  if ((await bump(store, `new:${forge.name}:${day}`, 2 * DAY)) > num(env.NEW_PER_DAY, 30)) return answer(429, { ok: false, error: 'enough new issues today' })
  const { title, body } = render(r, tag, issue ? issue.url : null)
  const made = await forge.create(title, body)
  kept = { number: made.number, url: made.url, count: 1, versions: [r.app], max: r.app }
  await save()
  return answer(201, { ok: true, url: made.url, number: made.number, count: 1, isNew: true })
}

// --- As a Cloudflare Worker --------------------------------------------------------------------

export default {
  async fetch(request, env) {
    const url = new URL(request.url)
    const length = Number(request.headers.get('content-length') || 0)
    if (length > LIMIT) return Response.json({ ok: false, error: 'too big' }, { status: 413 })
    const out = await handle(
      {
        method: request.method,
        path: url.pathname,
        ip: request.headers.get('cf-connecting-ip'),
        header: name => request.headers.get(name),
        body: request.method === 'POST' ? await request.text() : '',
      },
      env,
      kvStore(env.CRASH),
    )
    return Response.json(out.json, { status: out.status })
  },
}

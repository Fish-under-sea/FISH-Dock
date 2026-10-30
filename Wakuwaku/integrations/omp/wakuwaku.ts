// Wakuwaku for oh-my-pi (omp): an extension omp loads from its agent folder
// (<agent dir>/extensions/, ~/.omp/agent by default, each profile's too),
// written there by her settings' Connect page (connection.rs), her port, her
// exe and her data folder filled in below. It tells her what the session
// does, as Claude Code's hooks would (server.rs ?agent=omp, events_omp.rs):
//
//   session_start / _switch / _branch   SessionStart (the one before ends)
//   before_agent_start                  UserPromptSubmit, the person's words
//   tool_execution_start / _end         PreToolUse, PostToolUse(Failure)
//   agent_end, not to go on             Stop with the reply, StopFailure on
//                                       an error, Interrupt when stopped
//   tool_approval_requested / _resolved ApprovalRequested / ApprovalResolved:
//                                       waiting on the person in omp's own
//                                       window, and back (she cannot answer
//                                       omp's tool approvals)
//   plan_review_requested               PermissionRequest for ExitPlanMode,
//                                       held: her answer resolves the review
//                                       (ctx.resolvePlanReview), whoever
//                                       answers first
//   plan_review_resolved                PostToolUse for it: answered in the
//                                       terminal, her panel goes
//   session_shutdown                    SessionEnd
//
// Only the main agent's: a subagent runs in the same process with a session
// of its own (ctx.agent.kind "sub"). omp makes this module once per session
// (the main one, each subagent's), so what is kept is kept on globalThis.
//
// omp waits on each handler (up to 30 s; session_shutdown 2 s), and a
// rejection nobody catches can end the whole omp process: nothing here is
// waited on but the last word as omp exits, every promise is caught, and
// every handler's body is in a try. Her not up: the post fails at once, and
// for a while nothing more is tried; a session starting then starts her (her
// exe with --wakuwaku-ensure-running, unless the person quit her). Sent with
// node:http, not fetch: omp's Bun sends a fetch to 127.0.0.1 through
// HTTP_PROXY too (tried with omp 18.11.1, Bun 1.4.3), where it fails or comes
// to her from the proxy's process (no window to go to).
import { spawn } from 'node:child_process'
import http from 'node:http'

// Filled in as she writes the file (connection.rs omp_extension). 2.0.0 起两套
// 占位符都写、都填：新名（FISH_DOCK 前缀）是正式名，旧名（WAKUWAKU 前缀）保留，
// 因为已经发出去的 2.0.0 插件副本只认旧字面量（#t16：不许静默删旧名）。
const choose = (now, legacy) => (now && !now.startsWith('__')) ? now : ((legacy && !legacy.startsWith('__')) ? legacy : '')
const PORT = Number(choose("__FISH_DOCK_PORT__", "__WAKUWAKU_PORT__")) || 47213
const EXE = choose("__FISH_DOCK_EXE__", "__WAKUWAKU_EXE__")
const DATA = choose("__FISH_DOCK_DATA__", "__WAKUWAKU_DATA__")

const PATH = '/hook?from=wakuwaku&agent=omp'
// #t18：写出名是 --fish-dock-ensure-running。旧 exe 认的 --wakuwaku-ensure-running
// 只在读取侧兼容（flags.rs 别名表）：这份扩展由当前版本写出，宿主也是当前版本，
// 不需要在命令行里塞两个开关（塞了会打乱后面的 who/source 位置参数）。
const ENSURE_FLAG = '--fish-dock-ensure-running'
// How long a word to her may take, and the last one as omp exits.
const POST_MS = 1500
const LAST_MS = 1000
// A plan review waits on her answer this long at most (her own wait,
// promptWaitSec, is shorter: she answers nothing then).
const ASK_MS = 295 * 1000
// Not up: nothing tried for this long.
const DOWN_MS = 15 * 1000
// So much of a text, a tool's arguments, a plan, is sent.
const MAX_TEXT = 2000
const MAX_PLAN = 60 * 1000
const MAX_TOOLS = 200

const filled = value => (value && !value.startsWith('__') ? value : '')

// What every copy of this module shares in the process.
const S = (globalThis.__wakuwakuOmp1 ??= {
  queue: Promise.resolve(),
  // Answered once (her up), and nothing tried until then (her not up).
  up: false,
  downUntil: 0,
  started: false,
  // The main session now: the one before ends as another begins.
  session: '',
  // Each tool running: its name and arguments, for its end.
  tools: new Map(),
  // Each plan review she was asked: what she was shown.
  reviews: new Map(),
})

function call(object, name) {
  try {
    return typeof object?.[name] === 'function' ? object[name]() : undefined
  } catch {
    return undefined
  }
}

const isMain = ctx => (ctx?.agent?.kind ?? 'main') === 'main'

function clip(text, max = MAX_TEXT) {
  return typeof text === 'string' && text.length > max ? `${text.slice(0, max - 1)}…` : text
}

// A tool's arguments as sent: long texts (a file written whole) cut short.
function trimmed(value, depth = 0) {
  if (typeof value === 'string') return clip(value)
  if (value === null || typeof value !== 'object') return value
  if (depth > 3) return undefined
  if (Array.isArray(value)) return value.slice(0, 20).map(v => trimmed(v, depth + 1))
  const out = {}
  for (const [key, v] of Object.entries(value).slice(0, 40)) out[key] = trimmed(v, depth + 1)
  return out
}

// The text of a message's content.
function textOf(content) {
  if (typeof content === 'string') return content
  if (!Array.isArray(content)) return ''
  return content
    .filter(part => part && part.type === 'text' && typeof part.text === 'string')
    .map(part => part.text)
    .join('')
}

// The session an event belongs to, where it works, and its name.
function where(ctx) {
  const sm = ctx?.sessionManager
  const out = { session_id: call(sm, 'getSessionId') || '', cwd: call(sm, 'getCwd') || ctx?.cwd || '' }
  const title = call(sm, 'getSessionName')
  if (typeof title === 'string' && title.trim()) out.session_title = clip(title, 200)
  return out
}

function down() {
  S.up = false
  S.downUntil = Date.now() + DOWN_MS
}

// One event to her port: the text of her answer, or a rejection (not up,
// not answering within ms, the connection dropped).
function request(body, ms) {
  return new Promise((resolve, reject) => {
    const headers = { 'content-type': 'application/json', 'content-length': Buffer.byteLength(body) }
    const req = http.request({ host: '127.0.0.1', port: PORT, path: PATH, method: 'POST', headers, timeout: ms, agent: false }, res => {
      let text = ''
      res.setEncoding('utf8')
      res.on('data', chunk => {
        text += chunk
      })
      res.on('end', () => resolve(text))
      res.on('error', reject)
    })
    req.on('error', reject)
    req.on('timeout', () => req.destroy(new Error('no answer')))
    req.end(body)
  })
}

// An event to her, after the ones before it (they arrive in order); what
// comes back is let go. The promise is of it sent or given up on, never a
// rejection.
function post(event) {
  if (Date.now() < S.downUntil) return S.queue
  let body
  try {
    body = JSON.stringify(event)
  } catch {
    return S.queue
  }
  const send = () => {
    if (Date.now() < S.downUntil) return undefined
    return request(body, POST_MS).then(() => {
      S.up = true
    }, down)
  }
  S.queue = S.queue.then(send, send).catch(() => {})
  return S.queue
}

function tell(ctx, name, fields = {}) {
  return post({ ...where(ctx), hook_event_name: name, ...fields })
}

// Her not up as a session starts: her exe starts her, unless the person
// quit her (main.rs ensure_running), on her port and with her data folder;
// told it is omp's, and how the session started, for her log (starts.rs).
function startHer(source) {
  const exe = filled(EXE)
  if (S.started || !exe) return
  S.started = true
  try {
    const env = { ...process.env, WAKUWAKU_PORT: String(PORT) }
    if (filled(DATA)) env.WAKUWAKU_USER_DATA = filled(DATA)
    const child = spawn(exe, [ENSURE_FLAG, 'omp', source], { detached: true, stdio: 'ignore', windowsHide: true, env })
    child.on('error', () => {})
    child.unref()
    // Up in a second or two: tried again soon.
    S.downUntil = Math.min(S.downUntil, Date.now() + 2000)
  } catch {}
}

function begin(ctx, source) {
  const here = where(ctx)
  if (S.session && S.session !== here.session_id) post({ hook_event_name: 'SessionEnd', session_id: S.session, cwd: here.cwd, reason: 'other' })
  S.session = here.session_id
  post({ ...here, hook_event_name: 'SessionStart', source }).then(() => {
    if (!S.up) startHer(source)
  })
}

function remember(id, tool) {
  if (!id) return
  S.tools.set(id, tool)
  while (S.tools.size > MAX_TOOLS) S.tools.delete(S.tools.keys().next().value)
}

// How a turn ended, from its last answer: an error, stopped, or done.
function ended(ctx, messages) {
  const last = [...(Array.isArray(messages) ? messages : [])].reverse().find(m => m && m.role === 'assistant')
  const reply = clip(textOf(last?.content), 4000) || ''
  if (last?.stopReason === 'error') {
    return tell(ctx, 'StopFailure', { error: '', error_details: clip(last.errorMessage || reply || 'error', 400), last_assistant_message: reply })
  }
  if (last?.stopReason === 'aborted') return tell(ctx, 'Interrupt')
  return tell(ctx, 'Stop', { last_assistant_message: reply })
}

// A plan review on her too: the plan, held until she answers (an approval
// executes it, a no refines it with her words), the terminal answers it
// (plan_review_resolved), or her time is up.
function askPlan(e, ctx) {
  if (!e.reviewId || Date.now() < S.downUntil) return
  const input = { plan: clip(typeof e.planContent === 'string' ? e.planContent : '', MAX_PLAN) }
  S.reviews.set(e.reviewId, input)
  const event = { ...where(ctx), hook_event_name: 'PermissionRequest', tool_name: 'ExitPlanMode', tool_input: input, tool_use_id: e.reviewId, plan_title: clip(e.title || '', 200) }
  let body
  try {
    body = JSON.stringify(event)
  } catch {
    return
  }
  // After the events before it, but nothing after it waits on it.
  S.queue
    .then(() => request(body, ASK_MS))
    .then(text => JSON.parse(text || '{}'))
    .then(out => {
      const decision = out?.hookSpecificOutput?.decision
      if (!decision || !S.reviews.has(e.reviewId)) return
      // Still the review open, not one answered in the terminal meanwhile.
      if (call(ctx, 'getPendingPlanReview')?.reviewId !== e.reviewId) return
      if (decision.behavior === 'allow') ctx.resolvePlanReview(e.reviewId, 'execute')
      else if (decision.behavior === 'deny') ctx.resolvePlanReview(e.reviewId, 'refine', { feedback: clip(decision.message || 'Refine the plan.', 400) })
    })
    .catch(() => {})
}

export default function wakuwaku(pi) {
  // Each handler: the main agent's alone, nothing thrown, nothing waited on.
  const on = (name, handle) =>
    pi.on(name, (e, ctx) => {
      try {
        if (isMain(ctx)) return handle(e || {}, ctx)
      } catch {}
      return undefined
    })

  on('session_start', (e, ctx) => void begin(ctx, 'startup'))
  on('session_switch', (e, ctx) => void begin(ctx, 'resume'))
  on('session_branch', (e, ctx) => void begin(ctx, 'resume'))
  on('before_agent_start', (e, ctx) => void tell(ctx, 'UserPromptSubmit', { prompt: clip(typeof e.prompt === 'string' ? e.prompt : '', 4000) }))
  on('tool_execution_start', (e, ctx) => {
    const tool = { name: e.toolName || '', input: trimmed(e.args) ?? {}, intent: clip(e.intent || '', 200) }
    remember(e.toolCallId, tool)
    void tell(ctx, 'PreToolUse', { tool_name: tool.name, tool_input: tool.input, tool_use_id: e.toolCallId || '', intent: tool.intent })
  })
  on('tool_execution_end', (e, ctx) => {
    const tool = S.tools.get(e.toolCallId) || { name: e.toolName || '', input: {}, intent: '' }
    S.tools.delete(e.toolCallId)
    const details = e.result?.details || {}
    const file = [details.path, details.resolvedPath].find(p => typeof p === 'string' && p)
    const fields = { tool_name: e.toolName || tool.name, tool_input: tool.input, tool_use_id: e.toolCallId || '', intent: tool.intent }
    if (file) fields.file = file
    if (e.isError) fields.error = clip(textOf(e.result?.content), 400)
    void tell(ctx, e.isError ? 'PostToolUseFailure' : 'PostToolUse', fields)
  })
  on('agent_end', (e, ctx) => {
    if (!e.willContinue) void ended(ctx, e.messages)
  })
  on('tool_approval_requested', (e, ctx) => {
    const tool = S.tools.get(e.toolCallId)
    void tell(ctx, 'ApprovalRequested', { tool_name: e.toolName || tool?.name || '', tool_input: tool?.input ?? {}, tool_use_id: e.toolCallId || '', intent: tool?.intent || '' })
  })
  on('tool_approval_resolved', (e, ctx) => void tell(ctx, 'ApprovalResolved', { tool_name: e.toolName || '', tool_use_id: e.toolCallId || '', approved: e.approved === true }))
  on('plan_review_requested', (e, ctx) => void askPlan(e, ctx))
  on('plan_review_resolved', (e, ctx) => {
    const input = S.reviews.get(e.reviewId)
    if (!input) return
    S.reviews.delete(e.reviewId)
    void tell(ctx, 'PostToolUse', { tool_name: 'ExitPlanMode', tool_input: input, tool_use_id: e.reviewId, choice: e.choice || '', by: e.by || '' })
  })
  // The last word, waited on a moment (omp exits after it) while she is up.
  on('session_shutdown', (e, ctx) => {
    const here = where(ctx)
    const sent = post({ ...here, session_id: here.session_id || S.session, hook_event_name: 'SessionEnd', reason: 'exit' })
    S.session = ''
    S.tools.clear()
    if (!S.up) return undefined
    return Promise.race([sent, new Promise(done => setTimeout(done, LAST_MS))])
  })
}

// wakuwaku-dsh-plugin
// Wakuwaku's plugin for DeepSeek Harness (DSH): what DSH's sessions do, told
// to her on her port. Written by Wakuwaku (Connect → DeepSeek Harness) into
// DSH's own folder, <DSH home>/wakuwaku/, beside a package.json of its own,
// and inserted by its path into DSH's home-level patch layer
// (<DSH home>/cordis.patch.yml), which every profile applies: the desktop
// app's and the command line's. A path, not a package name: nothing goes into
// a profile's node_modules, so no pnpm run of DSH's ever takes it away (a
// bridge package that was not there is what zhoupengjie/Wakuwaku#61 found).
// Remove there takes it all away again. It needs nothing but Node's own
// modules.
//
// It runs inside DSH's own process (its Host), as any of DSH's plugins: no
// program of hers is started, nothing waits on her. Each event is handed to
// her port over HTTP, one at a time and in order, after those before it;
// DSH's own work never waits for one (every listener returns at once, the
// waterfalls' next() handed back as it is; but for the approvals and
// questions below, where DSH waits on the person anyway), a post gives up
// after a moment, and while she is not there (no answer) nothing is sent for
// a few seconds.
// Every error is swallowed: DSH never sees one of ours.
//
// What it tells, in the words of Claude Code's hooks as DSH's own Claude Code
// hook bridge said them (so her reading of them, events_dsh.rs, stays as it
// was): SessionStart as a session's agent is made, UserPromptSubmit as the
// person's words enter a turn, PreToolUse and PostToolUse around each tool,
// Stop as a turn ends. What only a plugin sees goes with them: the turn's last
// words (last_assistant_message), the session's name as DSH gave it
// (session_title), whose a session is (origin "subagent", the session it
// works for: parent_session, top_session), and a turn that ended without
// finishing (stopped by the person, failed: stop_reason, StopFailure).
//
// As it is loaded it says so twice: to her port (WakuwakuLoaded), and in a
// small file beside itself (loaded-<profile>.json: this process, when, which
// version and code), which her Connect page reads to tell whether DSH really
// loaded it, and this version of it, she running or not; the file goes
// again as the plugin is let go.
//
// DSH's approvals and its questions to the person (version 2): it takes the
// first place in DSH's two waterfalls for them, approval/request (a tool's
// call to allow: its sandbox widened for a write outside the workspace, a
// command run with more access) and user-questions/request (its
// ask_user_question tool, a plan to review), and tells her each as Claude
// Code's PermissionRequest, the call's tool and input with it (DSH's request
// carries neither but its callId: the input is the one tools/pre-execute
// had). DSH's own dialog is shown at once all the same (next()), and the
// first answer wins. Hers comes back on the same request, held open while
// she shows it on her island: { decision: allow | deny } for an approval
// (DSH's outcomes allowed-once and rejected: it has no "always"), or
// { answers: [{ id, selected, custom? }] } for questions; {} is none (she
// only shows it: out of sight, do not disturb, its time up, or the person
// went to DSH for it), and DSH's dialog stays the one to answer. Hers first,
// DSH's own dialog is withdrawn: the request's signal is made ours too
// before next() (DSH's gateway withdraws a request from its window as that
// signal aborts), and aborted. DSH's first, her request is let go and she is
// told (PermissionReplied). Nothing is ever answered but by the person on
// her island. She not there, DSH is left as it was: nothing taken over.
//
// The row that loads it gives its config: { port }, her port.

import { createHash } from 'node:crypto'
import { readFileSync, unlinkSync } from 'node:fs'
import { writeFile } from 'node:fs/promises'
import http from 'node:http'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'

// This file's version, for her log. Which code DSH runs is told by the
// file's own digest (sha, in the marker): DSH keeps a module it imported
// until it restarts, so a file written again under the same name is not
// what runs, and her Connect page compares the two.
export const VERSION = 2
export const name = 'wakuwaku'

const DEFAULT_PORT = 47213
// How long one event may take to reach her.
const SEND_MS = 2000
// How long she may hold an approval or a question: past the longest she
// waits on the person (290 s, asks.rs ASK_TIMEOUT_MS), DSH's dialog alone.
const HOLD_MS = 300 * 1000
// The tools whose call asks the person a question (user-questions/request
// names no call of its own).
const QUESTION_TOOLS = new Set(['ask_user_question', 'exit_plan_mode'])
// Not there: tried again after this long.
const AWAY_MS = 5000
// Events waiting at most; past that, new ones are dropped (she is slow or gone).
const QUEUE_MAX = 200
// Sessions remembered (their names, their last words) at most.
const KEPT = 200
// A tool's result as far as she reads it (whether it failed), and any text
// of its input: the start of it.
const TEXT_MAX = 2000

// A map kept small: the oldest go first.
function remember(map, key, value) {
  map.delete(key)
  map.set(key, value)
  while (map.size > KEPT) map.delete(map.keys().next().value)
}

// Content blocks' text, joined (what a hook payload carries).
function textOf(content, sep = '') {
  if (!Array.isArray(content)) return ''
  return content.filter(b => b && b.type === 'text' && typeof b.text === 'string').map(b => b.text).join(sep)
}

const clip = (text, max = TEXT_MAX) => (text.length > max ? text.slice(0, max) : text)

// A tool's input with any long text cut short (a file written whole is no
// business of hers), a few levels down.
function slim(value, depth = 0) {
  if (typeof value === 'string') return clip(value)
  if (value === null || typeof value !== 'object' || depth > 4) return value
  if (Array.isArray(value)) return value.slice(0, 100).map(v => slim(v, depth + 1))
  const out = {}
  for (const [k, v] of Object.entries(value)) out[k] = slim(v, depth + 1)
  return out
}

// The folder this file is in (its marker goes beside it).
function hereDir() {
  try {
    return fileURLToPath(new URL('.', import.meta.url))
  } catch {
    return ''
  }
}

// This file's digest as DSH imported it: SHA-256 of its text with CRLF made
// LF (a checkout may have either), its first 16 hex digits.
function ownSha() {
  try {
    return createHash('sha256').update(readFileSync(fileURLToPath(import.meta.url), 'utf8').replace(/\r\n/g, '\n')).digest('hex').slice(0, 16)
  } catch {
    return ''
  }
}

// A profile's name as a file name's part.
const fileSafe = text => String(text || 'default').replace(/[^A-Za-z0-9._-]/g, '_').slice(0, 64)

// Her answer to an approval as DSH's outcome, or undefined for none.
function decisionOf(reply) {
  const decision = reply && typeof reply === 'object' ? reply.decision : undefined
  return decision === 'allow' ? 'allowed-once' : decision === 'deny' ? 'rejected' : undefined
}

// Her answers to DSH's questions as its answer batch, or undefined for none
// (or one that does not fit them: one answer for each, by its id).
function answersOf(reply, questions) {
  const given = reply && typeof reply === 'object' ? reply.answers : undefined
  if (!Array.isArray(given) || !Array.isArray(questions) || given.length !== questions.length) return undefined
  const answers = []
  for (const [i, q] of questions.entries()) {
    const a = given[i]
    if (!a || a.id !== q?.id || !Array.isArray(a.selected) || !a.selected.every(l => typeof l === 'string')) return undefined
    if (a.custom !== undefined && typeof a.custom !== 'string') return undefined
    answers.push({ id: a.id, selected: [...a.selected], ...(a.custom === undefined ? {} : { custom: a.custom }) })
  }
  return { answers }
}

// A request's signal made ours too, so that DSH's own dialog for it goes as
// she answers first: its gateway withdraws a request from DSH's window as
// the signal the request carried aborts (dsh-api-gateway's
// startRemoteEvent, 0.2.1-rc.2), and the listeners after this one get the
// very object this one does (cordis' waterfall hands each the same
// arguments). Readonly by DSH's types, a plain object as DSH makes it; false
// when it cannot be made so (frozen, say), and then she only shows it.
function takeOver(req, ours) {
  try {
    const was = req.signal
    const both = was instanceof AbortSignal ? AbortSignal.any([was, ours.signal]) : ours.signal
    req.signal = both
    return req.signal === both
  } catch {
    return false
  }
}

// The plugin for a context and its config. options: { dir } where its marker
// goes (beside this file by default), { sha } this file's digest (its own by
// default), for the tests.
export function make(ctx, config = {}, options = {}) {
  const port = Number.isInteger(Number(config?.port)) && Number(config.port) > 0 ? Number(config.port) : DEFAULT_PORT
  const path = '/hook?from=wakuwaku&agent=dsh'
  const titles = new Map()
  const replies = new Map()
  // The tool calls under way, by callId: whose, which tool, its input.
  const calls = new Map()
  // Sessions whose turn was told ended (Stop), until the next one begins.
  const stopped = new Set()
  let queue = Promise.resolve()
  let waiting = 0
  let awayUntil = 0
  let closed = false
  // Approvals and questions told, for their ids.
  let asked = 0

  // One event to her port; resolves when she answered, or gave none in time.
  function post(body) {
    return new Promise(done => {
      if (closed || Date.now() < awayUntil) return done()
      let data
      try {
        data = JSON.stringify(body)
      } catch {
        return done()
      }
      let req
      try {
        req = http.request(
          { host: '127.0.0.1', port, path, method: 'POST', agent: false, headers: { 'content-type': 'application/json', 'content-length': Buffer.byteLength(data) } },
          res => {
            res.on('error', () => done())
            res.on('end', () => done())
            res.resume()
          },
        )
      } catch {
        return done()
      }
      // Nothing of hers keeps DSH's process up as it ends.
      req.on('socket', socket => socket.unref?.())
      req.setTimeout(SEND_MS, () => req.destroy(new Error('timeout')))
      req.on('error', err => {
        // No one on her port: away for a while. Slow to answer: only this one lost.
        if (err?.message !== 'timeout') awayUntil = Date.now() + AWAY_MS
        done()
      })
      req.end(data)
    })
  }

  // An event for her, after those before it.
  function send(body) {
    if (closed || waiting >= QUEUE_MAX) return
    waiting++
    queue = queue
      .then(() => post(body))
      .catch(() => {})
      .then(() => {
        waiting--
      })
  }

  // An event she may answer, after those before it: held open until she
  // does. { answer, stop }: answer resolves with what she gave back (null
  // for nothing: she not there, an answer that is no JSON, HOLD_MS gone, or
  // stop()); stop() lets it go. Those after it go as soon as it is written:
  // nothing of hers waits on the person.
  function hold(body) {
    let request = null
    let over = false
    let settle
    const answer = new Promise(done => (settle = done))
    const end = value => {
      if (over) return
      over = true
      settle(value)
    }
    const start = () =>
      new Promise(sent => {
        if (over || closed || Date.now() < awayUntil) return sent(end(null))
        let data
        try {
          data = JSON.stringify(body)
          request = http.request(
            { host: '127.0.0.1', port, path, method: 'POST', agent: false, headers: { 'content-type': 'application/json', 'content-length': Buffer.byteLength(data) } },
            res => {
              let text = ''
              res.setEncoding('utf8')
              res.on('data', chunk => (text = text.length < 1_000_000 ? text + chunk : text))
              res.on('error', () => end(null))
              res.on('end', () => {
                let value = null
                try {
                  value = JSON.parse(text)
                } catch {}
                end(value)
              })
            },
          )
        } catch {
          return sent(end(null))
        }
        request.on('socket', socket => socket.unref?.())
        request.setTimeout(HOLD_MS, () => request.destroy(new Error('timeout')))
        request.on('error', err => {
          // No one on her port: away for a while (not when let go here).
          if (!over && err?.message !== 'timeout') awayUntil = Date.now() + AWAY_MS
          end(null)
          sent()
        })
        request.on('finish', () => sent())
        request.end(data)
      })
    if (closed || waiting >= QUEUE_MAX) {
      end(null)
    } else {
      waiting++
      queue = queue
        .then(start)
        .catch(() => {})
        .then(() => {
          waiting--
        })
    }
    return {
      answer,
      // Whether it went out to her (so there is one of hers to close).
      went: () => request !== null,
      stop: () => {
        end(null)
        request?.destroy()
      },
    }
  }

  // DSH's answer (theirs: next()'s) and hers (asked), whichever comes first.
  // Hers, read by read() (undefined: none, DSH's still to come): ours
  // aborted, which withdraws DSH's own dialog (takeOver), and it is hers.
  // DSH's: her request let go. Either way she is told (tell(outcome, by)),
  // when it reached her. signal: the request's, aborted but not by us when
  // the turn was stopped (cancelled).
  function race(theirs, asked, ours, signal, read, tell) {
    return new Promise((resolve, reject) => {
      let over = false
      const win = (outcome, by) => {
        if (over) return false
        over = true
        asked.stop()
        if (asked.went()) tell(outcome, by)
        return true
      }
      const failed = error => (signal?.aborted || error?.code === 'ASK_CANCELLED' ? 'cancelled' : 'error')
      Promise.resolve(theirs).then(
        value => win(value, 'dsh') && resolve(value),
        // Its dialog withdrawn as hers came: nothing more of it.
        error => !ours.signal.aborted && win(failed(error), 'dsh') && reject(error),
      )
      asked.answer.then(reply => {
        const value = read(reply)
        if (value === undefined || !win(value, 'her')) return
        try {
          ours.abort(new Error('answered on Wakuwaku'))
        } catch {}
        resolve(value)
      })
    })
  }

  // What came of an approval or a question, for her (it closes hers, and the
  // session works on).
  function replied(session, id, callId) {
    return (outcome, by) => send({ ...base(session, 'PermissionReplied'), permission_id: id, tool_use_id: callId, outcome: typeof outcome === 'string' ? outcome : 'answered', by })
  }

  // DSH asks the person to allow a tool's call (approval/request): its own
  // dialog, and her island too. Its request names the call; its input is
  // the one tools/pre-execute had.
  function approval(req, next) {
    if (closed || Date.now() < awayUntil || !req || typeof req !== 'object') return next()
    let body, ours, answerable, session, id, callId
    try {
      session = req.agent?.session
      callId = typeof req.callId === 'string' ? req.callId : ''
      const call = calls.get(callId)
      ours = new AbortController()
      answerable = takeOver(req, ours)
      id = `${process.pid}-${++asked}`
      body = { ...base(session, 'PermissionRequest'), tool_name: call?.name || String(req.toolName || ''), tool_input: call?.input ?? {}, tool_use_id: callId, permission_id: id, reason: clip(String(req.reason ?? ''), 400), answerable }
      const shown = req.displayReason
      if (shown && typeof shown === 'object') body.display_reason = { en: clip(String(shown.en ?? ''), 400), zh: clip(String(shown.zh ?? ''), 400) }
    } catch {
      return next()
    }
    let theirs
    try {
      theirs = next()
    } catch (err) {
      theirs = Promise.reject(err)
    }
    // Not taken over, an answer of hers would leave DSH's dialog asking: none taken.
    return race(theirs, hold(body), ours, req.signal, reply => (answerable ? decisionOf(reply) : undefined), replied(session, id, callId))
  }

  // DSH asks the person questions (user-questions/request): its ask_user_question
  // tool's (the latest such call of the session's: the request names none),
  // a plan to review; told as Claude Code's AskUserQuestion.
  function question(req, next) {
    if (closed || Date.now() < awayUntil || !req || typeof req !== 'object' || !Array.isArray(req.questions) || !req.agent) return next()
    let body, ours, answerable, session, id, callId
    try {
      session = req.agent.session
      const sid = idOf(session)
      callId = typeof req.wait?.callId === 'string' ? req.wait.callId : ''
      for (const [key, call] of calls) if (!req.wait?.callId && call.session === sid && QUESTION_TOOLS.has(call.name)) callId = key
      ours = new AbortController()
      answerable = takeOver(req, ours)
      id = `${process.pid}-${++asked}`
      body = { ...base(session, 'PermissionRequest'), tool_name: 'ask_user_question', tool_input: { questions: slim(req.questions) }, tool_use_id: callId, permission_id: id, answerable }
    } catch {
      return next()
    }
    let theirs
    try {
      theirs = next()
    } catch (err) {
      theirs = Promise.reject(err)
    }
    return race(theirs, hold(body), ours, req.signal, reply => (answerable ? answersOf(reply, req.questions) : undefined), replied(session, id, callId))
  }

  const service = key => {
    try {
      return ctx.get?.(key)
    } catch {
      return undefined
    }
  }

  // The session at the top of a subagent's lineage, as far as live sessions say.
  function topOf(header) {
    let top = header.parentSession || ''
    const sessions = service('sessions')
    for (let i = 0; top && sessions && i < 8; i++) {
      let up
      try {
        up = sessions.get(top)?.header
      } catch {
        break
      }
      if (!up || up.origin !== 'subagent' || !up.parentSession || up.parentSession === top) break
      top = up.parentSession
    }
    return top
  }

  // What every event carries: the session, its folder (the one it began in:
  // DSH keeps its log under it), whose it is, its name if DSH gave one (a
  // subagent's, made of its task, is no name for her: none).
  function base(session, event) {
    const header = session?.header || {}
    const id = header.id || session?.id || ''
    const out = { session_id: id, transcript_path: '', cwd: header.cwd || process.cwd(), hook_event_name: event, origin: header.origin || '', parent_session: header.parentSession || '' }
    if (header.origin === 'subagent') out.top_session = topOf(header)
    else if (titles.has(id)) out.session_title = titles.get(id)
    return out
  }

  const idOf = session => session?.header?.id || session?.id || ''

  function sessionStart({ agent, source }) {
    send({ ...base(agent?.session, 'SessionStart'), source: typeof source === 'string' ? source : 'startup' })
  }

  // The person's words entering a turn (or a subagent's task): the messages
  // entering a step; none for a step that only goes on.
  function promptSubmit({ agent, messages }) {
    if (!Array.isArray(messages) || messages.length === 0) return
    const theirs = messages.filter(m => m?.source?.kind === 'user')
    const words = theirs.length ? theirs : messages
    send({ ...base(agent?.session, 'UserPromptSubmit'), prompt: words.map(m => textOf(m?.content)).join(''), source: theirs.length ? 'user' : messages[0]?.source?.kind || '' })
  }

  function preTool(exec) {
    const input = slim(exec?.arguments ?? {})
    if (exec?.callId) remember(calls, exec.callId, { session: idOf(exec.agent?.session), name: exec.name || '', input })
    send({ ...base(exec?.agent?.session, 'PreToolUse'), tool_name: exec?.name || '', tool_input: input, tool_use_id: exec?.callId || '' })
  }

  function postTool(exec, result) {
    calls.delete(exec?.callId)
    send({
      ...base(exec?.agent?.session, 'PostToolUse'),
      tool_name: exec?.name || '',
      tool_input: slim(exec?.arguments ?? {}),
      tool_use_id: exec?.callId || '',
      tool_response: clip(textOf(result?.content)),
      is_error: result?.isError === true,
    })
  }

  // A turn finishing: its last words with it.
  function stop({ agent }) {
    const session = agent?.session
    const id = idOf(session)
    stopped.add(id)
    send({ ...base(session, 'Stop'), stop_hook_active: false, last_assistant_message: replies.get(id) || '' })
  }

  // DSH's log as it is written: the turn's last words, the session's name,
  // and a turn that ended without finishing (no Stop for it: stopped by the
  // person, refused, failed).
  function logged(session, event) {
    switch (event?.type) {
      case 'turn/start': {
        const id = idOf(session)
        replies.delete(id)
        stopped.delete(id)
        return
      }
      case 'assistant/message': {
        const text = textOf(event.data?.message?.content, '\n').trim()
        if (text) remember(replies, idOf(session), text)
        return
      }
      case 'session/title': {
        const title = event.data?.title
        if (typeof title === 'string' && title.trim()) remember(titles, idOf(session), title.trim())
        return
      }
      case 'turn/end': {
        const id = idOf(session)
        if (stopped.delete(id)) return
        const reason = event.data?.reason || {}
        if (reason.kind === 'error') {
          const failure = reason.error || {}
          send({ ...base(session, 'StopFailure'), error_details: clip(String(failure.message || failure.code || 'error'), 300) })
        } else {
          send({ ...base(session, 'Stop'), stop_hook_active: false, stop_reason: String(reason.kind || ''), last_assistant_message: reason.kind === 'completed' ? replies.get(id) || '' : '' })
        }
        return
      }
    }
  }

  // Each of DSH's points, each listener first in line (it changes nothing)
  // and back at once.
  function listen() {
    const quiet = fn => (...args) => {
      try {
        fn(...args)
      } catch {}
    }
    ctx.on('agent/created', quiet(sessionStart))
    ctx.on('agent/turn-stopping', quiet(stop))
    ctx.on('session/event', quiet(logged))
    ctx.on('agent/pre-step', (arg, next) => (quiet(promptSubmit)(arg), next()), true)
    ctx.on('tools/pre-execute', (exec, next) => (quiet(preTool)(exec), next()), true)
    ctx.on('tools/post-execute', (exec, result, next) => (quiet(postTool)(exec, result), next()), true)
    // Not back at once: DSH waits on the person here anyway (its dialog
    // shown all the while), and on whichever answer comes first.
    ctx.on('approval/request', (req, next) => approval(req, next), true)
    ctx.on('user-questions/request', (req, next) => question(req, next), true)
  }

  // Loaded: said to her, and in the marker beside this file; the marker goes
  // as the plugin is let go (the row taken out, DSH ending).
  function hello() {
    const profile = String(service('profileContext')?.name || '')
    const dir = options.dir ?? hereDir()
    const marker = dir ? join(dir, `loaded-${fileSafe(profile)}.json`) : ''
    const sha = options.sha ?? ownSha()
    if (marker) writeFile(marker, JSON.stringify({ pid: process.pid, at: Date.now(), version: VERSION, sha, profile, port })).catch(() => {})
    send({ session_id: '', hook_event_name: 'WakuwakuLoaded', version: VERSION, pid: process.pid, profile })
    return () => {
      closed = true
      try {
        if (marker) unlinkSync(marker)
      } catch {}
    }
  }

  return { listen, hello, sessionStart, promptSubmit, preTool, postTool, stop, logged, approval, question, idle: () => queue, close: () => (closed = true) }
}

export function apply(ctx, config) {
  try {
    const plugin = make(ctx, config)
    plugin.listen()
    const letGo = plugin.hello()
    ctx.effect?.(() => letGo, 'wakuwaku: let go')
  } catch {}
}

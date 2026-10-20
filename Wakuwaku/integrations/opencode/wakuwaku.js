// wakuwaku-opencode-plugin
// Wakuwaku's plugin for OpenCode 2: what OpenCode's sessions do, told to her
// on her port, and the permissions OpenCode asks for, put to the person on
// her island too. Written into OpenCode's plugin folder by Wakuwaku (Connect
// → OpenCode); Remove there takes it away. It needs nothing but fetch.
//
// OpenCode 2 runs a plugin in its background service, once for each folder
// it works in (a Location), and every copy hears every event: a copy tells
// her only of its own folder's sessions. A sub-agent's session is not told,
// but a permission it asks for is, as its main session's.
//
// Each event goes to her as OpenCode gives it ({ type, ...data }), with
// what only this side knows put in: the folder, the session's title, the
// person's words for the turn, the tool a call is of, the last text of the
// turn. Wakuwaku reads them (src-tauri/src/events_opencode.rs). One at a
// time, in order; while she is not there (no answer), nothing is sent for a
// few seconds, and nothing waits on her.
//
// A permission asked: OpenCode asks the person as it always does, and she
// asks too; whichever answers first. Her answer ({ decision: once | always
// | reject }) is OpenCode's reply; none ({}) leaves it to OpenCode's own ask.
// Answered in OpenCode first, she is told (permission.replied) and lets hers go.

const PORT = 47213
const HOOK = `http://127.0.0.1:${PORT}/hook?from=wakuwaku&agent=opencode`
// How long one event may take to reach her, and a permission her answer
// (she holds one up to 290 s).
const SEND_MS = 2000
const ASK_MS = 300 * 1000
// Not there: tried again after this long.
const AWAY_MS = 5000
const KEPT = 200
// A permission goes to her only once it has waited this long unanswered:
// `opencode run` turns each down at once (it can not ask anyone), and she
// would call the person for nothing.
const ASK_AFTER_MS = 400

// A map kept small: the oldest go first.
function remember(map, key, value) {
  map.delete(key)
  map.set(key, value)
  while (map.size > KEPT) map.delete(map.keys().next().value)
}

export function make(ctx) {
  const here = ctx.location?.directory || ''
  const mine = new Set()
  // A sub-agent's session → the main session it works for.
  const parents = new Map()
  const titles = new Map()
  // The person's words waiting for their turn to begin, the last text a
  // turn said, and each tool call's tool and input.
  const prompts = new Map()
  const texts = new Map()
  const calls = new Map()
  // Permissions answered (in OpenCode, or by her).
  const replied = new Map()
  const stop = new AbortController()
  let queue = Promise.resolve()
  let awayUntil = 0

  // Her answer, or null. No one on her port, or no answer in time (but for
  // a permission, which waits on the person): away for a while.
  async function post(body, ms, waits = false) {
    if (Date.now() < awayUntil || stop.signal.aborted) return null
    try {
      const res = await fetch(HOOK, {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify(body),
        signal: AbortSignal.any([stop.signal, AbortSignal.timeout(ms)]),
      })
      return await res.json().catch(() => ({}))
    } catch (err) {
      if (!stop.signal.aborted && !(waits && err?.name === 'TimeoutError')) awayUntil = Date.now() + AWAY_MS
      return null
    }
  }

  // An event for her, after those before it.
  function send(body) {
    queue = queue.then(() => post(body, SEND_MS)).catch(() => {})
  }

  // A permission for her, after the events before it, unless answered at
  // once; her answer is OpenCode's reply.
  function ask(body, request) {
    queue = queue
      .then(() => new Promise(done => setTimeout(done, ASK_AFTER_MS)))
      .then(() => {
        if (replied.has(request.id)) return
        post(body, ASK_MS, true).then(answer => {
          const decision = answer?.decision
          if (!['once', 'always', 'reject'].includes(decision) || stop.signal.aborted) return
          // Answered in OpenCode meanwhile: not there any more, and nothing to do.
          ctx.permission.reply({ sessionID: request.sessionID, requestID: request.id, decision }).catch(() => {})
        })
      })
      .catch(() => {})
  }

  // The main session an event is of, or '' when it is not this folder's. A
  // session is known as this folder's by an event of its that says where
  // (not all do); a form (OpenCode's question) only of a session known.
  function owner(event) {
    const data = event.data || {}
    const id = data.sessionID || data.form?.sessionID || ''
    const known = mine.has(id) || parents.has(id)
    if (!id || !(known || (event.location?.directory === here && /^(session|permission)\./.test(event.type)))) return ''
    if (event.location && event.location.directory !== here) return ''
    if (event.type === 'session.created' && data.parentID) {
      remember(parents, id, parents.get(data.parentID) || data.parentID)
    } else if (!known) {
      mine.add(id)
      if (mine.size > KEPT) mine.delete(mine.values().next().value)
    }
    return parents.get(id) || id
  }

  function hear(event) {
    const data = event.data || {}
    const main = owner(event)
    if (!main) return
    const own = main === (data.sessionID || data.form?.sessionID)
    const base = { directory: here, sessionID: main }
    if (titles.has(main)) base.title = titles.get(main)
    switch (event.type) {
      // Each call's tool and input, a sub-agent's too (for what it asks).
      case 'session.tool.input.started':
        remember(calls, data.id, { tool: data.name })
        break
      case 'session.tool.called':
        remember(calls, data.id, { ...calls.get(data.id), input: data.input })
        break
      case 'permission.asked': {
        const call = calls.get(data.source?.id) || {}
        const body = { ...data, ...base, type: event.type, tool: call.tool, input: call.input }
        if (!own) body.agentID = data.sessionID
        return ask(body, data)
      }
      case 'permission.replied':
        remember(replied, data.requestID, true)
        return send({ ...data, ...base, type: event.type })
    }
    // The rest only of main sessions.
    if (!own) return
    switch (event.type) {
      case 'session.created':
        if (data.title) remember(titles, main, data.title)
        return send({ ...base, type: event.type, title: data.title })
      case 'session.renamed':
        remember(titles, main, data.title)
        return
      case 'session.inbox.enqueued':
        if (data.item?.type === 'user') remember(prompts, main, data.item.payload?.text || '')
        return
      case 'session.execution.started': {
        texts.delete(main)
        const body = { ...base, type: event.type }
        if (prompts.has(main)) body.prompt = prompts.get(main)
        prompts.delete(main)
        return send(body)
      }
      case 'session.text.ended':
        if (data.text?.trim()) remember(texts, main, data.text)
        return
      case 'session.tool.called':
        return send({ ...base, type: event.type, id: data.id, tool: calls.get(data.id)?.tool, input: data.input })
      case 'session.tool.success':
      case 'session.tool.failed': {
        const call = calls.get(data.id) || {}
        return send({ ...base, type: event.type, id: data.id, tool: call.tool, input: call.input, error: data.error })
      }
      case 'session.execution.succeeded':
        return send({ ...base, type: event.type, text: texts.get(main) })
      case 'session.execution.failed':
        return send({ ...base, type: event.type, error: data.error })
      case 'session.execution.interrupted':
        return send({ ...base, type: event.type, reason: data.reason })
      case 'form.created':
      case 'form.replied':
      case 'form.cancelled':
      case 'session.deleted':
        return send({ ...base, type: event.type })
    }
  }

  // Every event, until the plugin is let go (OpenCode reloading it, or the
  // folder closed); should the stream end, it is heard again.
  async function listen() {
    while (!stop.signal.aborted) {
      try {
        for await (const event of ctx.event.subscribe({ signal: stop.signal })) {
          if (stop.signal.aborted) break
          try {
            hear(event)
          } catch {}
        }
      } catch {}
      if (!stop.signal.aborted) await new Promise(done => setTimeout(done, 1000))
    }
  }

  return {
    listen,
    hear,
    close: () => stop.abort(),
    idle: () => queue,
  }
}

export default {
  id: 'wakuwaku',
  setup: async ctx => {
    const plugin = make(ctx)
    plugin.listen()
    return () => plugin.close()
  },
}

// The crash relay under Node (relay.js's handle()): for Aliyun Function
// Compute's web functions (the relay in China, its issues on Gitee), or any
// server. Settings from the environment, as relay.js says; PORT (9000 as FC
// wants it). Counts and which issue each crash has are kept in memory: an
// instance started anew looks an issue up on Gitee again.
//
//   FORGE=gitee REPO=zhoupengjie/Wakuwaku GITEE_TOKEN=… SALT=… node server.js
//
// Behind a proxy (FC is one), the address is the first of X-Forwarded-For;
// TRUST_PROXY=0 to take the connection's own instead.
import http from 'node:http'

import { handle, memoryStore } from './relay.js'

const LIMIT = 48 * 1024
const env = process.env
const store = memoryStore()
const port = Number(env.PORT || env.FC_SERVER_PORT || 9000)

http
  .createServer((req, res) => {
    const chunks = []
    let size = 0
    let tooBig = false
    req.on('data', chunk => {
      size += chunk.length
      if (size > LIMIT) tooBig = true
      else chunks.push(chunk)
    })
    req.on('end', async () => {
      const send = (status, json) => {
        res.writeHead(status, { 'content-type': 'application/json; charset=utf-8' })
        res.end(JSON.stringify(json))
      }
      if (tooBig) return send(413, { ok: false, error: 'too big' })
      const forwarded = env.TRUST_PROXY === '0' ? '' : String(req.headers['x-forwarded-for'] || '').split(',')[0].trim()
      try {
        const out = await handle(
          {
            method: req.method,
            path: req.url,
            ip: forwarded || req.socket.remoteAddress,
            header: name => req.headers[name.toLowerCase()],
            body: Buffer.concat(chunks).toString('utf8'),
          },
          env,
          store,
        )
        send(out.status, out.json)
      } catch (e) {
        send(500, { ok: false, error: String(e.message || e) })
      }
    })
  })
  .listen(port, () => console.log(`wakuwaku crash relay on port ${port}, issues on ${env.FORGE || 'github'} ${env.REPO || '(REPO not set)'}`))

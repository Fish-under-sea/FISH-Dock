import test from 'node:test'
import assert from 'node:assert/strict'
import http from 'node:http'

test('kilo-auto/free inflated multimodal usage keeps output but does not advertise exact input or total', async () => {
  const sse = value => `data: ${JSON.stringify(value)}\n\n`
  const server = http.createServer((req, res) => {
    req.resume()
    req.on('end', () => {
      res.writeHead(200, { 'content-type': 'text/event-stream' })
      res.end(sse({ choices: [], usage: { prompt_tokens: 618820, completion_tokens: 717, completion_tokens_details: { reasoning_tokens: 637 } } })
        + sse({ choices: [{ index: 0, delta: { content: 'fixture' } }] })
        + sse({ choices: [{ index: 0, delta: {}, finish_reason: 'stop' }] }) + 'data: [DONE]\n\n')
    })
  })
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve))
  const previousBase = process.env.OUR_FREE_MODEL_KILO_BASE
  const fixtureBase = `http://127.0.0.1:${server.address().port}`
  process.env.OUR_FREE_MODEL_KILO_BASE = fixtureBase
  try {
    const { FreeModelAdapter, ROUTE_MAIN } = await import('../src/adapter.js')
    const { KILO_BASE } = await import('../src/kilo.js')
    assert.equal(KILO_BASE, fixtureBase, 'run this import-time environment fixture in its own Node process; never contact a real gateway')
    const { buildKiloCatalog } = await import('../src/catalog.js')
    const entry = buildKiloCatalog([{ id: 'kilo-auto/free', isFree: true, context_length: 256000 }])[0]
    const adapter = new FreeModelAdapter({ state: () => ({ catalog: [entry], membership: { [ROUTE_MAIN]: [entry.id] }, settings: { enabled: true } }), recordUsage() {}, recordTurn() {} })
    const chunks = []
    for await (const chunk of adapter.stream({ provider: ROUTE_MAIN, model: entry.id, maxTokens: 4096, messages: [{ role: 'user', content: [{ type: 'text', text: 'test' }] }], signal: AbortSignal.timeout(5000) })) chunks.push(chunk)
    const usage = chunks.find(chunk => chunk.type === 'usage').usage
    assert.equal(usage.outputTokens, 717)
    assert.equal(usage.reasoningTokens, 637)
    assert.equal(usage.inputTokens, 0)
    assert.equal('totalTokens' in usage, false)
  } finally {
    if (previousBase === undefined) delete process.env.OUR_FREE_MODEL_KILO_BASE
    else process.env.OUR_FREE_MODEL_KILO_BASE = previousBase
    server.closeAllConnections()
    await new Promise(resolve => server.close(resolve))
  }
})

/** 使用真实构建页面与隔离 RPC 验证新增渠道，不冒充厂商真实账号验收。 */
import assert from 'node:assert/strict'
import http from 'node:http'
import fs from 'node:fs'
import path from 'node:path'
import { once } from 'node:events'
import { createRequire } from 'node:module'
import { createManagement } from '../packages/standalone/management.mjs'
import { buildStats } from '../src/core/stats.js'

const require = createRequire(import.meta.url)
const { chromium } = require(process.env.OFM_PLAYWRIGHT_MODULE ?? 'playwright')
const management = createManagement({ stores: { settings: { get: () => ({ forwardKey: 'fixture' }) } }, info: () => ({}) })
const server = http.createServer((req, res) => { void management.handleRequest(req, res) })
server.listen(0, '127.0.0.1'); await once(server, 'listening')
const baseUrl = `http://127.0.0.1:${server.address().port}`
management.setPort(server.address().port)
let browser
const errors = [], calls = []
let loginState = 'pending', loggedIn = false, accounts = [], dailyFailure = false, meooClaimed = false
let finishClaim
const raccoonAccounts = [{ id: 'daily-a', nickname: '商汤测试账号', enabled: true, provider: 'raccoon' }]
const catalog = [{ id: 'fixture/model', name: '测试模型', channel: 'anonymous', routable: true, availability: 'available' }]
const imageDir = process.env.OFM_UI_SCREENSHOT_DIR
if (imageDir) fs.mkdirSync(imageDir, { recursive: true })
try {
  browser = await chromium.launch({ headless: true, ...(process.env.OFM_BROWSER_CHANNEL ? { channel: process.env.OFM_BROWSER_CHANNEL } : {}) })
  const page = await browser.newPage({ viewport: { width: 1280, height: 900 }, reducedMotion: 'reduce' })
  page.on('pageerror', error => errors.push(error.message))
  await page.route('**/api/management/**', async route => {
    const endpoint = new URL(route.request().url()).pathname.replace('/api/management', '')
    const body = route.request().postDataJSON()
    const answer = (value, status = 200) => route.fulfill({ status, json: value })
    if (endpoint === '/session') { loggedIn = true; return answer({ ok: true }) }
    if (!loggedIn) return answer({ error: '会话未登录' }, 401)
    if (endpoint === '/summary') return answer({ version: 'fixture', baseUrl, dataDir: 'fixture', networkMode: 'fixture', automaticRefresh: false,
      settings: { enabled: true }, channels: { state: 'ready' }, eacAuth: { local: false }, catalog })
    if (endpoint === '/stats') return answer(buildStats({}, []))
    if (endpoint !== '/channels/rpc') return answer({ ok: true })
    const { method, payload = {} } = body
    calls.push(method)
    let value = {}
    if (method === 'provider.status') value = { statuses: Object.fromEntries(payload.providers.map(id => [id, { accounts: { total: id === 'meoo' ? accounts.length : id === 'raccoon' ? 1 : 0, enabled: id === 'meoo' ? accounts.length : id === 'raccoon' ? 1 : 0 }, models: { total: 0, disabled: 0 } }])) }
    if (method === 'usage.autoCheckin') value = { autoCheckin: { enabled: false } }
    if (method === 'account.list') value = { accounts: payload.provider === 'raccoon' ? raccoonAccounts : accounts }
    if (method === 'model.list') value = { models: [] }
    if (method === 'credits.balances' || method === 'credits.status') value = { accounts: [] }
    if (method === 'meoo.balances') value = { accounts: accounts.map(a => ({ accountId: a.id, balance: { total: 2400 } })) }
    if (method === 'meoo.refresh') value = { accounts }
    if (method === 'meoo.claimAll') {
      await new Promise(resolve => { finishClaim = resolve })
      value = { results: accounts.map(a => ({ accountId: a.id, nickname: a.name, outcome: meooClaimed ? { kind: 'already-claimed', credit: 0 } : { kind: 'claimed', credit: 2000 } })) }
      meooClaimed = true
    }
    if (method === 'credits.claimAll') value = { results: raccoonAccounts.map(a => ({ accountId: a.id, nickname: a.nickname, outcome: { kind: 'already-claimed', credit: 0 } })), summary: { claimed: 0, totalCredit: 0, failed: 0 } }
    if (method === 'credits.daily.audit') {
      if (dailyFailure) return answer({ ok: false, error: { message: '隔离账单接口暂时故障' } })
      value = { day: '2026-10-23', accounts: [
        { provider: 'raccoon', accountId: 'daily-a', nickname: '商汤测试账号', grantedToday: 300, balance: 420, unit: '积分' },
        { provider: 'meoo', accountId: 'fixture-account', nickname: '测试账号', grantedToday: meooClaimed ? 2000 : null, balance: 2400, unit: '积分' },
        { provider: 'buddy', accountId: 'daily-b', nickname: '待确认账号', grantedToday: null, balance: 120, unit: '积分' },
      ] }
    }
    if (method === 'meoo.status') value = { accounts }
    if (method === 'meoo.login.start') { loginState = 'pending'; value = { flowId: 'fixture-flow', loginUrl: 'about:blank#meoo-fixture' } }
    if (method === 'meoo.login.poll') {
      if (loginState === 'success' && accounts.length === 0) accounts = [{ id: 'fixture-account', name: '测试账号', models: [] }]
      value = { status: loginState }
    }
    if (method === 'meoo.login.cancel') value = { ok: true }
    if (method === 'meoo.projects') value = { projects: [{ id: 'existing-project', name: '已有项目' }] }
    if (method === 'meoo.bind') { Object.assign(accounts[0], { projectId: 'existing-project', projectName: '已有项目', models: ['model-a', 'model-b'] }); value = { account: accounts[0] } }
    if (method === 'meoo.remove') { accounts = accounts.filter(account => account.id !== payload.accountId); value = { ok: true } }
    if (method === 'raccoon.daily.status') {
      if (dailyFailure) return answer({ ok: false, error: { message: '隔离账单接口暂时故障' } })
      value = { accounts: [
        { accountId: 'daily-a', nickname: '已确认账号', state: 'confirmed', day: '2026-10-22', grantedToday: 300, dailyBalance: 420 },
        { accountId: 'daily-b', nickname: '待确认账号', state: 'unconfirmed', dailyBalance: 120, billingAvailable: false },
      ] }
    }
    return answer({ ok: true, value })
  })
  const nav = name => page.locator('.console-nav-item').filter({ hasText: name }).click()
  const openProvider = async name => {
    await nav('免费账号渠道')
    const back = page.getByRole('button', { name: '返回全部渠道', exact: true })
    if (await back.isVisible()) await back.click()
    await page.locator('.provider-card').filter({ has: page.getByRole('heading', { name, exact: true }) }).getByRole('button', { name: '管理渠道' }).click()
  }
  const assertLayout = async name => {
    assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), `${name} 页面不应横向溢出`)
    const escaped = await page.locator('.provider-detail-main button, .provider-detail-main select').evaluateAll(elements => elements.filter(element => {
      const box = element.getBoundingClientRect()
      return box.width > 0 && (box.left < -1 || box.right > innerWidth + 1)
    }).map(element => element.textContent))
    assert.deepEqual(escaped, [], `${name} 控件应在视口内`)
    if (imageDir) await page.screenshot({ path: path.join(imageDir, `${name}.png`), fullPage: true, animations: 'disabled' })
  }
  await page.goto(baseUrl)
  await page.locator('#login-key').fill('fixture')
  await page.getByRole('button', { name: '进入控制台', exact: true }).click()
  await page.getByTestId('overview-ready').waitFor()
  await nav('免费账号渠道')
  const expectedProviders = ['WorkBuddy 国内版', 'WorkBuddy 国际版', 'TRAE', 'Cline', 'CodeArts', 'LobsterAI', 'Loomy', 'Raccoon', 'Meoo', 'MiniMax Code', 'Qoder 国际版', 'Qoder 国内版', 'ZCode', 'Gemini']
  await page.locator('.provider-grid .provider-card h2').first().waitFor()
  assert.deepEqual(await page.locator('.provider-grid .provider-card h2').allTextContents(), expectedProviders, '全部厂商按用户指定顺序及名称展示')
  await page.waitForFunction(() => [...document.querySelectorAll('.provider-grid .provider-logo img')].every(image => image.complete && image.naturalWidth > 0))
  assert.equal(await page.locator('.provider-grid .provider-logo img').count(), 14, '总览全部厂商使用本地可加载图标')
  for (const name of expectedProviders) {
    await openProvider(name)
    const card = page.locator('article.ofm_chan')
    await card.locator('.ofm_chanid b').filter({ hasText: name }).waitFor()
    await card.getByRole('button', { name: '添加账号', exact: true }).waitFor()
    await page.waitForFunction(() => { const image = document.querySelector('article.ofm_chan .ofm_chanlogo img'); return image?.complete && image.naturalWidth > 0 })
    assert.equal(await card.locator('.ofm_chanacts .ui-button-default').count(), 1 + (name === 'Meoo' ? 1 : 0), `${name} 添加入口使用统一主按钮`)
    assert.equal(await card.locator('.ofm_chanacts .ofm_foldtoggle').count(), 0, `${name} 折叠入口独立排布`)
    assert.equal(await card.locator('.ofm_chanfolds .ofm_foldtoggle').count(), 2)
    if (['Cline', 'Gemini', 'WorkBuddy 国际版'].includes(name)) assert.equal(await card.getByRole('button', { name: '一键领取', exact: true }).count(), 0, `${name} 不增加领取能力`)
    if (name === 'ZCode') assert.equal(await card.getByRole('group').count(), 1, '保留 ZCode 服务来源选择')
    await page.setViewportSize({ width: 320, height: 900 })
    await assertLayout(`provider-${name.replaceAll(' ', '-')}-320`)
    await page.setViewportSize({ width: 1280, height: 900 })
    if (imageDir && ['WorkBuddy 国内版', 'Loomy', 'MiniMax Code', 'Gemini'].includes(name)) await assertLayout(`provider-${name.replaceAll(' ', '-')}-1280`)
  }
  await openProvider('Meoo')
  const meooCard = page.locator('article.ofm_chan').filter({ has: page.getByText('Meoo', { exact: true }) })
  await meooCard.waitFor()
  assert.equal(await meooCard.locator('.ofm_chanlogo img').evaluate(image => image.complete && image.naturalWidth > 0), true, 'Meoo 使用可加载的真实 Logo')
  await page.getByText('尚未绑定 Meoo 账号', { exact: true }).waitFor()
  const meoo = page.getByRole('region', { name: 'Meoo 账号管理', exact: true })
  const loginDialog = page.getByRole('dialog', { name: '登录 Meoo', exact: true })
  const start = async () => {
    const popup = page.waitForEvent('popup')
    await meoo.getByRole('button', { name: '添加账号', exact: true }).click()
    await (await popup).close()
    await loginDialog.waitFor({ timeout: 3000 })
    await loginDialog.getByText('登录成功后本页会自动完成，无需复制任何东西。', { exact: true }).waitFor()
    assert.equal(await loginDialog.getByRole('link', { name: '打开登录页', exact: true }).getAttribute('target'), '_blank')
    assert.ok(await loginDialog.locator('code').textContent())
    if (imageDir) await page.screenshot({ path: path.join(imageDir, 'meoo-login-1280.png'), fullPage: true })
  }
  await start()
  await loginDialog.getByRole('button', { name: '取消', exact: true }).click()
  assert.equal(calls.filter(method => method === 'meoo.login.cancel').length, 1)
  await start()
  loginState = 'success'
  await meoo.getByRole('button', { name: '账号 (1)', exact: true }).click()
  await meoo.getByRole('button', { name: '读取已有项目', exact: true }).click()
  await page.getByLabel('测试账号 的项目').selectOption('existing-project')
  await page.getByRole('button', { name: '绑定模型服务', exact: true }).click()
  await page.getByText('2 个模型：model-a、model-b', { exact: true }).waitFor()
  const refreshed = page.waitForResponse(response => response.url().endsWith('/channels/rpc') && response.request().postDataJSON()?.method === 'meoo.status')
  await meoo.getByRole('button', { name: '刷新', exact: true }).click()
  await refreshed
  await meoo.getByRole('button', { name: '刷新余额', exact: true }).click()
  await meoo.getByText('当前余额：2400 积分', { exact: true }).waitFor()
  for (const expected of ['已领取 2000 积分', '今日已领取，无新增积分']) {
    finishClaim = undefined
    await meoo.getByRole('button', { name: '一键领取', exact: true }).click()
    await meoo.getByRole('button', { name: '领取中…', exact: true }).waitFor()
    assert.equal(await meoo.getByRole('button', { name: '领取中…', exact: true }).isDisabled(), true)
    assert.equal(typeof finishClaim, 'function')
    finishClaim()
    await meoo.getByText(`测试账号：${expected}`, { exact: true }).waitFor()
  }
  await meoo.getByRole('button', { name: '刷新续期', exact: true }).click()
  await meoo.getByText('账号续期完成', { exact: true }).waitFor()
  for (const width of [1280, 768, 414, 375, 320]) {
    await page.setViewportSize({ width, height: 900 })
    await assertLayout(`meoo-${width}`)
  }
  await page.setViewportSize({ width: 1280, height: 900 })
  assert.deepEqual(await page.locator('.detail-provider-link span').allTextContents(), expectedProviders, '详情侧栏与总览采用同一顺序')
  await meoo.getByRole('button', { name: '模型 (2)', exact: true }).click()
  await meoo.locator('#meoo-models code').filter({ hasText: 'model-a' }).waitFor()
  assert.equal(await meoo.locator('#meoo-accounts').isVisible(), false)
  await meoo.getByRole('button', { name: '账号 (1)', exact: true }).click()
  await openProvider('Raccoon')
  await page.getByRole('button', { name: '一键领取', exact: true }).click()
  assert.equal(calls.filter(method => method === 'credits.claimAll').length, 1)
  assert.equal(await page.getByRole('button', { name: '核验今日积分', exact: true }).count(), 1)
  await page.getByRole('button', { name: '核验今日积分', exact: true }).click()
  const audit = page.locator('[aria-label="今日积分核验结果"]')
  await audit.locator('summary').first().waitFor()
  assert.deepEqual(await audit.locator('summary .daily-audit-provider-name').allTextContents(), ['WorkBuddy 国内版', 'Raccoon', 'Meoo'])
  assert.equal(await audit.locator('details[open]').count(), 0, '核验默认只显示厂商')
  assert.equal(await audit.getByText('今日已领：300 积分', { exact: true }).isVisible(), false)
  if (imageDir) await page.screenshot({ path: path.join(imageDir, 'audit-collapsed.png'), fullPage: true })
  const raccoonGroup = audit.locator('details').filter({ has: page.locator('summary').filter({ hasText: 'Raccoon' }) })
  await raccoonGroup.locator('summary').focus()
  await page.keyboard.press('Enter')
  await raccoonGroup.getByText('今日已领：300 积分', { exact: true }).waitFor()
  assert.equal(await audit.locator('details[open]').count(), 1, '只展开所点击厂商')
  const typography = await raccoonGroup.locator('.daily-audit-row').evaluate(node => ({ size: getComputedStyle(node).fontSize, weight: getComputedStyle(node).fontWeight, base: getComputedStyle(node.closest('.daily-audit').parentElement).fontSize }))
  assert.equal(parseFloat(typography.base) - parseFloat(typography.size), 3, '账号详情文字缩小 3px')
  assert.equal(typography.weight, '400', '账号详情使用正常细字重')
  for (const group of await audit.locator('details').all()) if (await group.getAttribute('open') === null) await group.locator('summary').click()
  await audit.getByText('今日已领：300 积分', { exact: true }).waitFor()
  await audit.getByText('今日已领：2000 积分', { exact: true }).waitFor()
  await audit.getByText('今日已领：待确认', { exact: true }).waitFor()
  await audit.getByText('当前余额：120 积分', { exact: true }).waitFor()
  for (const width of [1280, 768, 414, 375, 320]) {
    await page.setViewportSize({ width, height: 900 })
    await assertLayout(`raccoon-${width}`)
  }
  dailyFailure = true
  await page.getByRole('button', { name: '核验今日积分', exact: true }).click()
  await page.getByRole('alert').filter({ hasText: '隔离账单接口暂时故障' }).waitFor()
  dailyFailure = false
  await page.getByRole('button', { name: '核验今日积分', exact: true }).click()
  await page.waitForFunction(() => !Array.from(document.querySelectorAll('[role=alert]')).some(node => node.textContent.includes('隔离账单接口暂时故障')))
  await openProvider('Meoo')
  await meoo.getByRole('button', { name: '账号 (1)', exact: true }).click()
  accounts.push({ id: 'fixture-account-2', name: '第二账号', models: [] })
  await meoo.getByRole('button', { name: '刷新', exact: true }).click()
  await meoo.getByRole('button', { name: '账号 (2)', exact: true }).waitFor()
  await meoo.getByRole('button', { name: '刷新余额', exact: true }).click()
  await meoo.getByText('余额已刷新', { exact: true }).waitFor()
  assert.match(await page.locator('.provider-detail-main .ofm_chanmeta').innerText(), /剩余积分\s*4800/)
  page.once('dialog', dialog => dialog.accept())
  await page.getByRole('button', { name: '删除 测试账号', exact: true }).click()
  await meoo.getByRole('button', { name: '账号 (1)', exact: true }).waitFor()
  assert.match(await page.locator('.provider-detail-main .ofm_chanmeta').innerText(), /剩余积分\s*2400/, '删除账号后不累计它的旧余额')
  page.once('dialog', dialog => dialog.accept())
  await page.getByRole('button', { name: '删除 第二账号', exact: true }).click()
  await page.getByText('尚未绑定 Meoo 账号', { exact: true }).waitFor()
  assert.equal(accounts.length, 0)
  assert.equal(calls.filter(method => method === 'meoo.bind').length, 1)
  assert.deepEqual(errors, [])
  console.log('厂商页面浏览器验收通过：14 渠道顺序及 Qoder 名称、Meoo 图标与完整卡片、账号/模型折叠、双账号删除后的余额汇总；核验默认分组收起、键盘展开、小 3px 与字重 400；登录、绑定、领取、续期及错误恢复；5 种视口和 10 张渠道截图。厂商请求由隔离 RPC 替身提供。')
} finally {
  await browser?.close()
  management.dispose(); server.closeAllConnections(); await new Promise(resolve => server.close(resolve))
}

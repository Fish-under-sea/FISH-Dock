/** 仅转换独立端承载；渠道业务仍来自原插件页面。 */
import fs from 'node:fs'
import vm from 'node:vm'
import { presentProviders } from '../../packages/standalone/frontend/provider-data.mjs'
import { meooLogo } from '../../packages/standalone/frontend/assets/meoo-logo.mjs'
import { providerLogos } from '../../packages/standalone/frontend/assets/provider-logos.mjs'
let source = fs.readFileSync(new URL('../../client.js', import.meta.url), 'utf8')
function replaceOnce(from, to) {
  if (source.split(from).length !== 2) throw new Error(`共享页面替换位置改变：${from.slice(0, 90)}`)
  source = source.replace(from, to)
}
const anchor = '    exports.apply = apply'
if (source.split(anchor).length !== 2) throw new Error('共享页面导出位置改变')
source = source.replace(anchor, "    exports.standalone = { ChannelsPage, ChannelCard, EacAuth, useEacLogin, DICT, CSS, Tank, usePool, poolReading, capacityText, CHANNEL_PROVIDERS, CHANNEL_ICONS, LedgerPage, LogsPage }\n" + anchor)
const api = "try { return new URL('api/our-free-model', document.baseURI).pathname } catch { return '/api/our-free-model' }"
if (source.split(api).length !== 2) throw new Error('共享页面 API 位置改变')
source = source.replace(api, "return '/api/management'")
replaceOnce("api('/pool', { timeout: 25_000 })", "api('/eac/pool', { timeout: 25_000 })")
// 原页面 POST 空 body；独立管理接口要求 JSON 对象。
replaceOnce("...body === undefined ? {} : { body: JSON.stringify(body) },", "body: JSON.stringify(body ?? {}),")
replaceOnce("h(PageHero, { t, titleKey: 'dash.title', subKey: 'dash.sub' },",
  "h(PageHero, { title: t('dash.title'), sub: t('dash.sub') },")
replaceOnce("'chan.sub': '把各家的免费额度接进来：登录一次，模型就出现在对话框的模型选择器里。凭据只写入本机凭据库，页面永远拿不到明文。',",
  "'chan.sub': '登录各家的账号后，通过同一个 API 地址接入免费额度。凭据保存在本机；账号管理页面不显示明文，导出备份包含凭据。',")
// 独立端只转换卡片呈现：登录、余额及领取能力仍由原组件判断。
replaceOnce('CHANNEL_ICONS[channel.id] !== undefined', '(channel.logo ?? CHANNEL_ICONS[channel.id]) !== undefined')
replaceOnce("h('img', { src: CHANNEL_ICONS[channel.id], alt: '' })", "h('img', { src: channel.logo ?? CHANNEL_ICONS[channel.id], alt: '', width: 38, height: 38 })")
replaceOnce("className: 'ofm_btn', disabled: busy !== '' || !rpc, 'aria-busy': busy === 'add'", "className: 'ui-button ui-button-default', disabled: busy !== '' || !rpc, 'aria-busy': busy === 'add'")
const loginIcon = "h('svg', { width: 16, height: 16, viewBox: '0 0 24 24', fill: 'none', stroke: 'currentColor', strokeWidth: 2, strokeLinecap: 'round', strokeLinejoin: 'round', 'aria-hidden': 'true' }, h('path', { d: 'M9 18l6-6-6-6 M15 12H3 M15 3h4a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2h-4' }))"
replaceOnce("onClick: startLogin }, busy === 'add' ? '…' : t('chan.act.add'))", `onClick: startLogin }, ${loginIcon}, busy === 'add' ? '添加中…' : t('chan.act.add'))`)
for (const action of ['refreshAll', 'claim']) {
  replaceOnce(`className: 'ofm_btn ghost', disabled: busy !== '' || !rpc, 'aria-busy': busy === '${action}'`, `className: 'ui-button ui-button-outline', disabled: busy !== '' || !rpc, 'aria-busy': busy === '${action}'`)
}
replaceOnce("className: 'ofm_btn ghost', disabled: balanceLoading || busy !== '' || !rpc", "className: 'ui-button ui-button-outline', disabled: balanceLoading || busy !== '' || !rpc")
const refreshIcon = "h('svg', { width: 16, height: 16, viewBox: '0 0 24 24', fill: 'none', stroke: 'currentColor', strokeWidth: 2, strokeLinecap: 'round', strokeLinejoin: 'round', 'aria-hidden': 'true' }, h('path', { d: 'M3 11a9 9 0 0 1 15-6l3 3 M21 3v5h-5 M21 13a9 9 0 0 1-15 6l-3-3 M3 21v-5h5' }))"
replaceOnce("}, busy === 'refreshAll' ? '…' : t('chan.act.refresh'))", `}, ${refreshIcon}, busy === 'refreshAll' ? '刷新中…' : t('chan.act.refresh'))`)
replaceOnce("}, balanceLoading ? t('chan.credits.loading') : t('chan.credits.refresh'))", `}, ${refreshIcon}, balanceLoading ? t('chan.credits.loading') : t('chan.credits.refresh'))`)
replaceOnce("          h('button', { type: 'button', className: 'ofm_foldtoggle', 'data-open': open === 'accounts'", "        ),\n        h('div', { className: 'ofm_chanfolds' },\n          h('button', { type: 'button', className: 'ofm_foldtoggle', 'data-open': open === 'accounts'")
let record
vm.runInNewContext(source, { window: { __ModuleLoader__: { load: value => { record = value } } } })
const shared = record.factory(() => ({})).standalone
const css = shared.CSS + `
.ofm_root{
 max-width:none;
}
.ofm_root button,.ofm_root input,.ofm_root select{font-family:inherit}
.ofm_root img{max-width:100%}
.ofm_root input,.ofm_root select{max-width:100%;padding:7px 9px;border:1px solid var(--dsw-alias-border-l2);border-radius:8px;background:white;color:inherit}
.ofm_root input[type=checkbox]{padding:0}
`

export function standaloneSharedUi() {
  return { name: '独立端共享渠道',
    resolveId(id) {
      if (id === 'ofm-shared-client' || id === 'ofm-provider-list') return '\0' + id
      if (id === 'ofm-shared-styles.css') return '\0' + id
    },
    load(id) {
      if (id === '\0ofm-shared-client') return `import * as React from 'react'; export const shared = (${record.factory.toString()})(() => React).standalone;`
      if (id === '\0ofm-shared-styles.css') return css
      if (id === '\0ofm-provider-list') return `export const providers = ${JSON.stringify(presentProviders(shared.CHANNEL_PROVIDERS).map(row => ({ ...row, logo: row.id === 'meoo' ? meooLogo : providerLogos[row.id] ?? shared.CHANNEL_ICONS[row.id] })))}`
    },
  }
}

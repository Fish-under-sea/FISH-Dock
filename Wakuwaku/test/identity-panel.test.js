const assert = require('node:assert/strict')
const fs = require('node:fs')
const vm = require('node:vm')
const path = require('node:path')
const { test } = require('node:test')
function make(confirm = () => true) {
  const calls = []
  const window = {}
  vm.runInNewContext(fs.readFileSync(path.join(__dirname, '../src/pet/identity-panel.js'), 'utf8'), { window })
  const panel = window.IdentityPanel.create({
    lang: () => 'zh', confirm, redraw() {},
    invoke: async (command, args) => {
      calls.push({ command, args })
      if (command === 'identity_view') return { directory: 'X:/用户下载', missingMailCredentials: 1 }
      if (command === 'identity_preview') return { ticket: 'private-ticket', name: 'accounts.zip', ofmAccounts: 2, mailAccounts: null, missingCredentials: 1, format: 'fish-dock/ofm-backup' }
      if (command === 'identity_export') return { saved: true, path: 'X:/用户下载/new.zip' }
      if (command === 'identity_import') return { imported: true, backup: 'X:/身份/.backups/txn-one' }
    },
  })
  return { panel, calls }
}
test('备份界面说明整个配置仓及实际默认导出位置', async () => {
  const { panel } = make(); await panel.load(); const html = panel.render()
  assert.match(html, /FISH Dock 配置与备份/)
  assert.match(html, /宿主设置.*插件.*配置/)
  assert.match(html, /导出配置备份 ZIP/)
  assert.match(html, /exports/)
  assert.doesNotMatch(html, /首次使用系统「下载」目录|统一保存 OFM/)
})
test('预览不会提交，并清楚说明旧 OFM 包保留邮件身份', async () => {
  const { panel, calls } = make()
  await panel.load()
  await panel.preview()
  const html = panel.render()
  assert.match(html, /用户下载/)
  assert.match(html, /2/)
  assert.match(html, /邮件.*保持|保留.*邮件/)
  assert.match(html, /缺失.*1|1.*缺失/)
  assert.equal(calls.filter(c => c.command === 'identity_import').length, 0)
})
test('拒绝明文确认不会调用原生导出或导入', async () => {
  const { panel, calls } = make(() => false)
  await panel.load()
  await panel.preview()
  await panel.export()
  await panel.import()
  assert.deepEqual(calls.map(c => c.command), ['identity_view', 'identity_preview'])
  assert.equal(panel.busy(), false)
})
test('异步取消确认不会调用导入导出，确认等待期间没有业务请求', async () => {
  let answer
  const { panel, calls } = make(() => new Promise(resolve => { answer = resolve }))
  await panel.load(); await panel.preview()
  const exporting = panel.export()
  assert.equal(calls.filter(c => c.command === 'identity_export').length, 0)
  answer(false); await exporting
  const importing = panel.import()
  assert.equal(calls.filter(c => c.command === 'identity_import').length, 0)
  answer(false); await importing
})
test('保存使用编辑后的目录，提交只使用预览票据不接受任意文件路径', async () => {
  const { panel, calls } = make()
  await panel.load()
  panel.setDirectory('Y:/我的备份')
  await panel.export()
  await panel.preview()
  await panel.import()
  const saved = calls.find(c => c.command === 'identity_export')
  assert.equal(saved?.args.directory, 'Y:/我的备份')
  assert.equal(saved?.args.confirm, true)
  const commit = calls.find(c => c.command === 'identity_import')
  assert.equal(commit?.args.ticket, 'private-ticket')
  assert.deepEqual(Object.keys(commit.args).sort(), ['confirm', 'ticket'])
})

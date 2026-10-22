const assert = require('node:assert/strict')
const fs = require('node:fs')
const path = require('node:path')
const vm = require('node:vm')
const { test } = require('node:test')
const source = fs.readFileSync(path.join(__dirname, '../src/pet/settings.js'), 'utf8')
const view = source.slice(source.indexOf('  function inboxHTML('), source.indexOf('  function agentsHTML('))
function render({ count = 3, busy = false } = {}) {
  const context = vm.createContext({
    snap: { mail: [{ id: 'one', on: true, status: { unread: count } }] },
    inbox: { filter: 'all', letters: 60, threads: [], total: 0, errors: [], readingAll: busy },
    draft: null, writing: false, handNote: null, Widgets: { icon: () => 'check' }, twoPanes: () => false, roomForTwo: () => false,
    esc: x => String(x), T: x => x, seg: () => '', boxName: () => '', svg: x => x,
    unreadOf: a => a.status.unread, accountOf: id => context.snap.mail.find(a => a.id === id),
  })
  return vm.runInContext(`${view}; inboxHTML('one')`, context)
}
test('收件箱提供不受分页与星标筛选限制的一键已读按钮', () => {
  assert.match(render(), /data-mail-read-all/)
  assert.match(render(), /title="mail\.readAll/)
})
test('没有未读或正在处理时禁用重复一键已读', () => {
  assert.match(render({ busy: true }), /aria-busy="true"/)
  assert.match(render(), /class="mail-filter"/)
  assert.match(render({ busy: true }), /mail\.loading/)
  assert.match(render({ count: 0 }), /data-mail-read-all[^>]*disabled/)
  assert.match(render({ busy: true }), /data-mail-read-all[^>]*disabled/)
})

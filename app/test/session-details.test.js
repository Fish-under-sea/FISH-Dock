// T-J 的前端那一半：设置页的会话行（app/src/pet/settings.js 的 pageSessions）。
// 真的把那段函数拿出来，配一套最小的桩跑一遍，验：
//   · 有 fileList / recentSteps 时行上出现展开按钮，展开后看得到文件（最多 15 条 + 「等 N 个」）
//     与最近几步（成功 ✓ / 失败 ✗）；
//   · 「显示具体内容」关掉时，就算数据里带着 fileList / recentSteps 也不给展开按钮、
//     不显示路径，并把记着的展开状态清空（后端本来也不该给这些字段）。
import assert from 'node:assert/strict'
import fs from 'node:fs'
import path from 'node:path'
import vm from 'node:vm'
import { test } from 'node:test'

const source = fs.readFileSync(path.join(import.meta.dirname, '../src/pet/settings.js'), 'utf8')
const FROM = 'const openSessions = new Set()'
const TO = "  // General's quick switches"
const start = source.indexOf(FROM)
const end = source.indexOf(TO, start)
assert.ok(start > 0 && end > start, 'settings.js 里找得到 pageSessions 那一段')

const WORDS = {
  'home.sessions': '会话列表',
  'home.noSessions': '还没有会话。打开 {list}，她就会开始工作。',
  's.open': '展开看改动的文件与最近几步',
  's.files': '这一轮改的文件',
  's.steps': '最近几步',
  's.none': '没有记录',
  's.filesMore': '等 {n} 个文件',
  'jump.hint': '去它的窗口',
}

function render(snapOverrides) {
  const context = {
    snap: { settings: { lang: 'zh', details: true }, sessions: [], ...snapOverrides },
    lang: 'zh',
    T: (key, vars) => {
      const text = WORDS[key] || key
      return vars ? text.replace(/\{(\w+)\}/g, (_, name) => String(vars[name] ?? '')) : text
    },
    esc: text =>
      String(text).replace(/[&<>"]/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[c]),
    COLOR: { idle: '#888', working: '#5e9bff' },
    sec: title => `<h3>${title}</h3>`,
    clockTag: () => '',
    toolNames: () => 'Claude Code',
    toolsHere: () => [],
    Agents: { byId: id => ({ name: id }) },
    Status: {
      nameOf: (session, detailed) => (detailed && session.name) || session.project || '',
      status: () => '干活中',
      isEnding: () => false,
    },
  }
  vm.createContext(context)
  vm.runInContext(`${source.slice(start, end)}\nthis.pageSessions = pageSessions; this.openSessions = openSessions;`, context)
  return context
}

const SESSION = {
  id: 's1',
  agent: 'codex',
  project: 'pet',
  name: '修复岛',
  mood: 'working',
  fileList: ['D:/w/a.rs', 'D:/w/b.rs'],
  filesMore: 3,
  files: 5,
  recentSteps: [
    { key: 'step.edit', ok: true, since: 1 },
    { key: 'step.command', ok: false, since: 2 },
  ],
}

test('a session line opens to its files and recent steps', () => {
  const context = render({ sessions: [SESSION] })
  const folded = context.pageSessions()
  assert.match(folded, /data-sess-open="s1"/, 'a line with details has a toggle')
  assert.match(folded, /aria-expanded="false"/)
  // 折着的时候不把路径画出来。
  assert.ok(!folded.includes('D:/w/a.rs'))

  context.openSessions.add('s1')
  const open = context.pageSessions()
  assert.match(open, /aria-expanded="true"/)
  assert.ok(open.includes('D:/w/a.rs') && open.includes('D:/w/b.rs'))
  assert.ok(open.includes('等 3 个文件'), 'the count past 15 is said, not listed')
  assert.match(open, /class="st ok"[\s\S]*✓/)
  assert.match(open, /class="st bad"[\s\S]*✗/)
  assert.ok(open.includes('这一轮改的文件') && open.includes('最近几步'))
})

test('with the specifics off, nothing opens and no cached expansion is kept', () => {
  const context = render({ settings: { lang: 'zh', details: false }, sessions: [SESSION] })
  context.openSessions.add('s1')
  const html = context.pageSessions()
  assert.ok(!html.includes('data-sess-open'), 'no toggle when the details are off')
  assert.ok(!html.includes('D:/w/a.rs'), 'no paths leak into the page')
  assert.ok(!html.includes('这一轮改的文件'))
  assert.equal(context.openSessions.size, 0, 'the cached expansion is cleared')
  // 会话行本身还在（名字与状态照常）。
  assert.ok(html.includes('pet'), 'the line itself is still there')
})

test('a session with nothing to show gets no empty disclosure', () => {
  const context = render({ sessions: [{ id: 's2', project: 'pet', mood: 'working' }] })
  const html = context.pageSessions()
  assert.ok(!html.includes('data-sess-open'))
  assert.ok(html.includes('pet'))
})

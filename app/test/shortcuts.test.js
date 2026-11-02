const assert = require('node:assert/strict')
const { describe, it } = require('node:test')

const { STRINGS } = require('../src/shared/i18n')

// panel.js 的状态机：IIFE 挂 window，没有 window 就导不出；这里用 vm 起一个
// 假 window，把 window.Panel 的 decideHotkeyAction 拿出来。
const fs = require('node:fs')
const path = require('node:path')
const vm = require('node:vm')
const source = fs.readFileSync(path.join(__dirname, '../src/pet/panel.js'), 'utf8')
const context = {
  document: { getElementById: () => ({ hidden: true, addEventListener() {}, querySelector: () => null }), createElement: () => ({ setAttribute() {}, append() {} }), createTextNode: s => String(s) },
  location: { search: '?role=island' },
  URLSearchParams,
  setTimeout: () => 1,
  clearTimeout() {},
  window: { I18n: { t: (_, k) => k }, pet: { onShift: () => {}, onUpdate: () => {}, onAsks: () => {}, onHotkey: () => {}, hover: () => {}, answer: () => {}, dismiss: () => {}, jump: () => {}, panel: () => {}, keyboard: () => {}, settingsOpen: () => {} } },
}
context.window = context.window
vm.runInNewContext(source, context)
const { decideHotkeyAction } = context.window.Panel
// 跨 vm 的对象原型不同，deepEqual 认成不相等；比 JSON。
const same = (a, b) => JSON.stringify(a) === JSON.stringify(b)

describe('全局快捷键（shortcuts.rs，issue #45）', () => {
  describe('panel.js 的快捷键状态机 decideHotkeyAction', () => {
    it('island 永远是开关岛', () => {
      assert.ok(same(decideHotkeyAction({ which: 'island' }), { type: 'toggle' }))
    })

    it('不认识的 which 忽略', () => {
      assert.ok(same(decideHotkeyAction({ which: 'unknown' }), { type: 'ignore' }))
    })

    it('岛没收开：只开岛，绝不替人答', () => {
      const ask = { id: 101, kind: 'permission', tool: 'write' }
      assert.ok(same(decideHotkeyAction({ which: 'allow', ask, shownId: 101, isArmed: true, islandOpen: false }), { type: 'open' }))
    })

    it('提示刚冒出来（600 ms 内没解禁）：只开岛不答', () => {
      const ask = { id: 102, kind: 'permission', tool: 'bash' }
      assert.ok(same(decideHotkeyAction({ which: 'allow', ask, shownId: 102, isArmed: false, islandOpen: true }), { type: 'open' }))
    })

    it('问题（question）不替人答：只开岛', () => {
      const ask = { id: 103, kind: 'question', questions: [{ question: '哪个分支？' }] }
      assert.ok(same(decideHotkeyAction({ which: 'allow', ask, shownId: 103, isArmed: true, islandOpen: true }), { type: 'open' }))
    })

    it('ZCode、DeepSeek Harness 的审批快捷键也答（上游 #97）', () => {
      for (const from of ['zcode', 'dsh']) {
        for (const which of ['allow', 'deny']) {
          const ask = { id: 104, kind: 'permission', from }
          assert.ok(same(decideHotkeyAction({ which, ask, shownId: 104, isArmed: true, islandOpen: true }), { type: 'answer', which, id: 104 }), `${from} 的 ${which}`)
        }
      }
      // 计划也算：按钮能答的，快捷键就答得了。
      assert.ok(same(decideHotkeyAction({ which: 'allow', ask: { id: 104, kind: 'plan', from: 'dsh' }, shownId: 104, isArmed: true, islandOpen: true }), { type: 'answer', which: 'allow', id: 104 }))
    })

    it('答不了的（canAnswer=false）：只开岛', () => {
      assert.ok(same(decideHotkeyAction({ which: 'allow', ask: { id: 104, kind: 'permission', canAnswer: false }, shownId: 104, isArmed: true, islandOpen: true }), { type: 'open' }))
      assert.ok(same(decideHotkeyAction({ which: 'allow', ask: { id: 104, kind: 'permission', from: 'zcode', canAnswer: false }, shownId: 104, isArmed: true, islandOpen: true }), { type: 'open' }))
    })

    it('岛开着、解禁了、答得了：allow 替它答', () => {
      const ask = { id: 105, kind: 'permission', tool: 'edit' }
      assert.ok(same(decideHotkeyAction({ which: 'allow', ask, shownId: 105, isArmed: true, islandOpen: true }), { type: 'answer', which: 'allow', id: 105 }))
    })

    it('岛开着、解禁了、答得了：deny 替它答（计划也算）', () => {
      const ask = { id: 106, kind: 'plan' }
      assert.ok(same(decideHotkeyAction({ which: 'deny', ask, shownId: 106, isArmed: true, islandOpen: true }), { type: 'answer', which: 'deny', id: 106 }))
    })

    it('等着的那一个不是显示中的：不替它答', () => {
      const ask = { id: 107, kind: 'permission' }
      assert.ok(same(decideHotkeyAction({ which: 'allow', ask, shownId: 999, isArmed: true, islandOpen: true }), { type: 'open' }))
    })
  })

  describe('快捷键的文案键', () => {
    it('中英文都在', () => {
      const keys = [
        's.shortcuts',
        's.shortcutsEnable',
        's.shortcutsEnableNote',
        's.shortcutApprove',
        's.shortcutApproveNote',
        's.shortcutDeny',
        's.shortcutDenyNote',
        's.shortcutIsland',
        's.shortcutIslandNote',
        's.shortcutNone',
        's.shortcutRecording',
        's.shortcutOccupied',
        's.shortcutNeedMod',
        's.shortcutNoWin',
        's.shortcutDuplicate',
      ]
      for (const k of keys) {
        assert.ok(k in STRINGS.zh, `缺 zh：${k}`)
        assert.ok(k in STRINGS.en, `缺 en：${k}`)
      }
    })
  })

  describe('设置页：录制期间让路、退出恢复', () => {
    it('录制状态只在换键时叫 shortcutsPause，任何退出都传 null', () => {
      const calls = []
      const captured = {}
      const vmContext = {
        console: { log() {} },
        window: {
          I18n: { t: (_, k) => k },
          pet: {
            shortcutsPause: on => calls.push(on),
            settings: { get: () => Promise.resolve({ settings: {} }), set: () => Promise.resolve({ settings: {} }) },
            settingsClosed() {},
            keyboard() {},
            onShortcutStatus: null,
          },
        },
        document: {
          hasFocus: () => true,
          activeElement: null,
          addEventListener() {},
          querySelector: () => null,
          createElement: () => ({ setAttribute() {}, append() {} }),
          createTextNode: s => String(s),
        },
        location: { search: '' },
        CSS: { escape: x => x },
        setInterval: () => 1,
        clearInterval() {},
        clearTimeout() {},
        setTimeout: () => 1,
        requestAnimationFrame() {},
        screen: { availWidth: 1920, availHeight: 1080 },
        Date,
        Promise,
      }
      // 取 close 的那一段（到 open 为止，别把 patch 的声明卷进来），驱动
      // setRecording 的整个一圈：开录叫一次 true，退出叫一次 false，同键再按
      // 不重复叫。
      const all = fs.readFileSync(path.join(__dirname, '../src/pet/settings.js'), 'utf8')
      captured.close = all.slice(all.indexOf('  function close(why'), all.indexOf('  async function loadGallery(')).replace(/function patch\([^)]*\)[\s\S]*?\n  \}/, '')
      vm.runInNewContext(`let isOpen = true, snap = { settings: {} }, recordingShortcut = null, shortcutToast = '';
        const setRecording = key => { if (key === recordingShortcut) return; recordingShortcut = key; window.pet.shortcutsPause?.(key !== null) };
        const toastSettings = () => {}; const draw = () => {}; const patch = () => {};
        const extensionPanel = { closeAuth: null }, confirmActive = null, confirmQueue = [];
        const layer = { contains: () => false }, foot = { querySelector: () => ({ textContent: '' }) };
        const extensionPage = { park() {} }; const welcomeOpen = false; const clockTimer = 0;
        const thumbTimers = []; const agentOpen = ''; const othersOpen = false; const holdCustom = false;
        const eyeShown = new Set(); const inbox = {}; const tab = 'now'; const openRevision = 0;
        ${captured.close}
        this.controls = { stop: () => setRecording(null), record: key => setRecording(key) };`, vmContext)
      vmContext.controls.record('shortcutApprove')
      assert.deepEqual(calls, [true])
      vmContext.controls.stop()
      assert.deepEqual(calls, [true, false])
      // 同一键再按一遍不重复叫（setRecording 的早退）。
      vmContext.controls.record('shortcutApprove')
      vmContext.controls.record('shortcutApprove')
      assert.deepEqual(calls, [true, false, true])
    })
  })
})

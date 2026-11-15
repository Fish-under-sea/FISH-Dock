// 关闭设置时作废落在面板上的确认提问（上游 bf6f0adb）：settings.js 的 close() 把
// 设置页自己的 confirmActive / confirmQueue 交给 settings-asked.closeCleared 作废，
// 与取消/Esc/点背景走同一条路。这里钉纯逻辑：作废正在显示的、排队的都按否收掉、
// 一个失败的 resolve 不挡住其余、没有就返回 0。
const assert = require('node:assert/strict')
const { test } = require('node:test')

const { closeCleared } = require('../src/pet/settings-asked')

test('作废正在显示的与排队的确认提问', () => {
  const seen = []
  const state = {
    finishActive: () => {
      seen.push('finish')
      return true
    },
    queue: [{ resolve: v => seen.push(['a', v]) }, { resolve: v => seen.push(['b', v]) }],
  }
  assert.equal(closeCleared(state), 3, '1 个正显示的 + 2 个排队的')
  assert.deepEqual(seen, ['finish', ['a', false], ['b', false]])
  assert.equal(state.queue.length, 0, '队列清空')
})

test('没有正在显示的，只收排队的', () => {
  const seen = []
  const state = { finishActive: () => false, queue: [{ resolve: v => seen.push(v) }] }
  assert.equal(closeCleared(state), 1)
  assert.deepEqual(seen, [false])
})

test('一个 resolve 抛错不挡住其余', () => {
  const seen = []
  const state = {
    finishActive: () => false,
    queue: [{ resolve: () => { throw new Error('x') } }, { resolve: v => seen.push(v) }],
  }
  assert.equal(closeCleared(state), 2, '两条都算作废')
  assert.deepEqual(seen, [false], '后一条仍被收掉')
})

test('空状态返回 0', () => {
  assert.equal(closeCleared(null), 0)
  assert.equal(closeCleared({ finishActive: () => false, queue: [] }), 0)
})

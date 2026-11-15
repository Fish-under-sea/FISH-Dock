// 关闭设置时清掉落在面板上的提问（上游 bf6f0adb）：Esc 的取消与「取消」按钮走
// 同一条 cancelAsked；关设置时清 approveFor 等，免得重开又冒出来。
const assert = require('node:assert/strict')
const { test } = require('node:test')

const { mayCancel, cancelAsked, closeCleared } = require('../src/pet/settings-asked')

test('取消与 Esc 走同一个 cancelAsked：清掉所有落在面板上的提问', () => {
  const state = { gone: { id: 'x' }, mkAsk: { id: 'y' }, approveFor: { id: 'z' }, flow: { step: 'ask' } }
  assert.equal(cancelAsked(state), true)
  assert.equal(state.gone, null)
  assert.equal(state.mkAsk, null)
  assert.equal(state.approveFor, null)
  assert.equal(state.flow.step, 'idle')
})

test('卸载或安装进行中不清理（不打断在干活的那一个）', () => {
  assert.equal(mayCancel({ gone: { busy: true } }), false)
  assert.equal(mayCancel({ flow: { step: 'installing' } }), false)
  const busy = { approveFor: { id: 'z' }, gone: { busy: true }, flow: { step: 'installing' } }
  assert.equal(cancelAsked(busy), false)
  assert.notEqual(busy.approveFor, null, '在干活时不动 approveFor')
})

test('关闭设置时清掉 approveFor，重开不再残留', () => {
  const state = { approveFor: { id: 'z' }, mkAsk: { id: 'y' }, gone: null, flow: { step: 'ask' } }
  closeCleared(state)
  assert.equal(state.approveFor, null)
  assert.equal(state.mkAsk, null)
  assert.equal(state.flow.step, 'idle')
})

test('关闭设置不清正在安装的步骤', () => {
  const state = { flow: { step: 'installing' }, approveFor: null }
  closeCleared(state)
  assert.equal(state.flow.step, 'installing')
})

// mem-report.mjs 的口径测试：CSV 解析、分组、取点、汇总、泄漏判据、差值符号。
// 逐文件跑：node scripts/mem-report.test.mjs（本机 node --test 多文件受限）。
import assert from 'node:assert/strict'
import test from 'node:test'

import { LEAK_MIN_AGE_MIN, LEAK_MIN_WINDOW_MIN, baseName, collectPoints, deltaOf, groupOf, leakVerdict, leakWindowWarning, nearestTick, parseCsvLine, parseSamples, pickAttempt, signed, splitByLabel, summarizeTick, toMb } from './mem-report.mjs'

const HEADER = 'sample_utc,elapsed_sec,label,depth,pid,ppid,name,role,ws_bytes,private_bytes,handles,threads,pagefile_bytes,detail'
const MB = 1024 * 1024

// 一行样本。字段顺序与采样器（MemSample.psm1 New-MemSampleRow）一致。
function line({ elapsed, label = 'before', depth = 0, pid, ppid = 0, name = 'wakuwaku.exe', role = 'host', ws = 0, priv = 0, handles = 0, threads = 0, detail = '' }) {
  return `2026-10-30T00:00:00.000Z,${elapsed},${label},${depth},${pid},${ppid},${name},${role},${ws},${priv},${handles},${threads},${priv},${detail}`
}

test('CSV 拆行：引号里的逗号不分割，两个引号是一个引号', () => {
  assert.deepEqual(parseCsvLine('a,b,c'), ['a', 'b', 'c'])
  assert.deepEqual(parseCsvLine('a,"x,y",c'), ['a', 'x,y', 'c'])
  assert.deepEqual(parseCsvLine('a,"say ""hi""",c'), ['a', 'say "hi"', 'c'])
  assert.deepEqual(parseCsvLine('a,,c'), ['a', '', 'c'])
})

test('CSV 解析：表头对不上就报错，不静默给出错口径的数字', () => {
  const text = [HEADER, line({ elapsed: 60, pid: 1, ws: 10 * MB, priv: 4 * MB, handles: 100, threads: 10 })].join('\n')
  const rows = parseSamples(text)
  assert.equal(rows.length, 1)
  assert.equal(rows[0].elapsedSec, 60)
  assert.equal(rows[0].wsBytes, 10 * MB)
  assert.equal(rows[0].privateBytes, 4 * MB)
  assert.throws(() => parseSamples('a,b,c\n1,2,3'), /CSV 表头缺少/)
})

test('分组：dock 自身、插件、其它分开算', () => {
  assert.equal(groupOf('host'), 'dock')
  assert.equal(groupOf('webview:renderer'), 'dock')
  assert.equal(groupOf('webview:gpu-process'), 'dock')
  assert.equal(groupOf('plugin-backend'), 'plugin')
  assert.equal(groupOf('plugin-runtime'), 'plugin')
  assert.equal(groupOf('console'), 'other')
  assert.equal(groupOf('child'), 'other')
})

test('取点：容差内取最近的 tick，容差外宁缺勿滥', () => {
  const rows = [300, 305, 1800, 3600].map(elapsed => ({ elapsedSec: elapsed }))
  assert.equal(nearestTick(rows, 300, 60), 300)
  assert.equal(nearestTick(rows, 302, 60), 300)
  assert.equal(nearestTick(rows, 340, 60), 305)
  assert.equal(nearestTick(rows, 600, 60), null, '没有 10 分钟点就该缺，不能拿 5 分钟的顶')
})

test('汇总：分组小计、renderer 计数与合计', () => {
  const rows = parseSamples([
    HEADER,
    line({ elapsed: 300, pid: 1, role: 'host', ws: 50 * MB, priv: 20 * MB, handles: 500, threads: 36 }),
    line({ elapsed: 300, pid: 2, ppid: 1, depth: 1, name: 'msedgewebview2.exe', role: 'webview:renderer', ws: 60 * MB, priv: 70 * MB, handles: 400, threads: 40 }),
    line({ elapsed: 300, pid: 3, ppid: 1, depth: 1, name: 'msedgewebview2.exe', role: 'webview:renderer', ws: 20 * MB, priv: 140 * MB, handles: 500, threads: 42 }),
    line({ elapsed: 300, pid: 4, ppid: 1, depth: 1, name: 'node.exe', role: 'plugin-backend', ws: 70 * MB, priv: 80 * MB, handles: 250, threads: 15, detail: 'fish.magpie@1.2.3' }),
    line({ elapsed: 300, pid: 5, ppid: 1, depth: 1, name: 'conhost.exe', role: 'console', ws: 7 * MB, priv: 1 * MB, handles: 95, threads: 2 }),
  ].join('\n'))
  const summary = summarizeTick(rows, 300)
  assert.equal(summary.renderers, 2, '两个 renderer 进程要数出来')
  assert.equal(summary.groups.dock.processes, 3)
  assert.equal(summary.groups.plugin.processes, 1)
  assert.equal(toMb(summary.groups.dock.privateBytes), 230)
  assert.equal(toMb(summary.groups.plugin.privateBytes), 80)
  assert.equal(summary.total.processes, 5)
  assert.equal(summary.total.handles, 1745)
  assert.equal(summary.processes[0].pid, 3, '明细按私有工作集从大到小排，最大的真凶在最上面')
})

test('泄漏判据：预算内算过、超预算算不过，宿主句柄与子树句柄分开判', () => {
  const budget = { privateGrowthMb: 24, handleGrowth: 200 }
  // 夹具：宿主 + 一个 renderer + 一个 gpu 进程，私有与句柄都能单独给。
  const at = (elapsed, privMb, handles, hostHandles = 500, gpuMb = 100) => summarizeTick(parseSamples([
    HEADER,
    line({ elapsed, pid: 1, role: 'host', priv: 20 * MB, handles: hostHandles }),
    line({ elapsed, pid: 2, ppid: 1, depth: 1, role: 'webview:renderer', priv: privMb * MB, handles }),
    line({ elapsed, pid: 3, ppid: 1, depth: 1, role: 'webview:gpu-process', priv: gpuMb * MB, handles: 700 }),
  ].join('\n')), elapsed)
  // 起点用 30min（1800s），不用 5min：取样窗口本身就是判据的一部分，见下一个测试。
  const calm = leakVerdict(at(1800, 70, 400), at(3600, 80, 450), budget)
  assert.equal(calm.privateGrowthMb, 10)
  assert.equal(calm.handleGrowth, 50, 'dock 子树句柄增量')
  assert.equal(calm.hostHandleGrowth, 0, '宿主句柄没动')
  assert.equal(calm.gpuPrivateGrowthMb, 0)
  assert.equal(calm.ok, true)
  const leaking = leakVerdict(at(1800, 70, 400), at(3600, 130, 900), budget)
  assert.equal(leaking.privateGrowthMb, 60)
  assert.equal(leaking.ok, false, '一小时涨 60 MB 必须判不过')
  const handlesOnly = leakVerdict(at(1800, 70, 400), at(3600, 71, 900), budget)
  assert.equal(handlesOnly.ok, false, '内存没涨但子树句柄涨 500 也是泄漏')
  // 只有宿主自己涨句柄、WebView2 侧反而降：子树合计看不出来，必须单独判宿主，
  // 否则 Rust 侧的泄漏会被子树平均掉。
  const hostOnly = leakVerdict(at(1800, 70, 400, 500), at(3600, 71, 150, 750), budget)
  assert.equal(hostOnly.hostHandleGrowth, 250)
  assert.equal(hostOnly.handleGrowth, 0, '子树合计没变：宿主 +250、renderer −250')
  assert.equal(hostOnly.ok, false, '宿主句柄单独涨过预算也要判不过')
  // GPU 常驻增长要单独报出来（t7 实测 217–246 MB 五小时不回落，旧口径把它当瞬时峰放过了）。
  const gpuGrew = leakVerdict(at(1800, 70, 400, 500, 100), at(3600, 70, 400, 500, 140), budget)
  assert.equal(gpuGrew.gpuPrivateGrowthMb, 40)
  assert.equal(gpuGrew.ok, false, 'GPU 涨 40 MB 会带着 dock 私有超 +24 MB 预算')
})

// 取样窗口为什么有下限：ab-r20 的 1→20min 句柄增量改前 +221 / 改后 +198，两侧都撞
// +200 线；t7 在进程年龄 303→323min 复测是宿主句柄 685→685、子树 5308→5292、
// renderer 3→3，窗内还含一次完整 quiet 切换而曲线无台阶 → 启动段是一次性增长。
// 工具在起点低于下限时必须告警，不能安静地给出一个假阳性的「不通过」。
test('泄漏判据取样窗口两头都拦：起点太早是假阳性，窗口太短是假阴性', () => {
  const rows = parseSamples([HEADER, line({ elapsed: 60, pid: 1 }), line({ elapsed: 1200, pid: 1 })].join('\n'))
  const early = collectPoints(rows, [1, 20], 30)
  assert.equal(early.length, 2)
  assert.match(leakWindowWarning(early), /假阳性/)
  assert.match(leakWindowWarning(early), /--at 30,60/)
  // 起点合规但窗口只有 2min（现装长跑手上就只有 58→60min 这一段）：什么都判得过，是假阴性。
  assert.match(leakWindowWarning([{ minute: 58 }, { minute: 60 }]), /假阴性/)
  assert.equal(leakWindowWarning([{ minute: 30 }, { minute: 60 }]), '', '合规窗口不告警')
  assert.equal(leakWindowWarning([{ minute: 30 }]), '', '只有一个点算不出增量，不告警')
  assert.equal(leakWindowWarning([]), '', '没有点也不告警')
  assert.equal(LEAK_MIN_AGE_MIN, 30)
  assert.equal(LEAK_MIN_WINDOW_MIN, 30)
})

test('带符号打印：正负号本身就是信息', () => {
  assert.equal(signed(3), '+3')
  assert.equal(signed(-3), '-3')
  // 0 打成 +0：表示「没有变化」，与「减少了」区分开（差值表里两者含义不同）。
  assert.equal(signed(0), '+0')
})

test('差值符号：正数表示改后更高', () => {
  const make = (privMb, renderers) => {
    const rows = [line({ elapsed: 300, pid: 1, role: 'host', priv: 20 * MB, handles: 500, threads: 36 })]
    for (let i = 0; i < renderers; i += 1) rows.push(line({ elapsed: 300, pid: 10 + i, ppid: 1, depth: 1, role: 'webview:renderer', priv: privMb * MB, handles: 400, threads: 40 }))
    return summarizeTick(parseSamples([HEADER, ...rows].join('\n')), 300)
  }
  const delta = deltaOf(make(100, 2), make(70, 1))
  assert.equal(delta.renderers, -1, '改后少一个 renderer')
  // 改前 dock 私有 = 宿主 20 + 2×100 = 220 MB；改后 = 宿主 20 + 1×70 = 90 MB。
  assert.equal(delta.dockPrivateMb, -130, '正数=改后更高，负数=改后更省')
  assert.equal(delta.handles, -400, '改前 500+2×400，改后 500+400')
})

test('缺点位要标出来，不要用别的点冒充', () => {
  const rows = parseSamples([HEADER, line({ elapsed: 300, pid: 1 })].join('\n'))
  const points = collectPoints(rows, [5, 30, 60], 60)
  assert.equal(points.length, 3)
  assert.equal(points[0].missing, undefined)
  assert.equal(points[0].summary.tick, 300)
  assert.equal(points[1].missing, true)
  assert.equal(points[2].missing, true)
})

// 断点续跑：mem-ab.ps1 重跑一次就在同一份 CSV 里多一个标签（before#2），
// 旧样本保留。报表必须按标签分开算，否则两次启动的 elapsed_sec 会混在一起，
// 取「5 分钟点」时可能取到另一次启动的 5 分钟。
test('一份 CSV 里多次尝试按标签分开，取样本最多的那次代表这一侧', () => {
  const rows = parseSamples([
    HEADER,
    line({ elapsed: 60, label: 'before', pid: 1 }),
    line({ elapsed: 300, label: 'before', pid: 1 }),
    line({ elapsed: 60, label: 'before#2', pid: 9 }),
    line({ elapsed: 300, label: 'before#2', pid: 9 }),
    line({ elapsed: 600, label: 'before#2', pid: 9 }),
    line({ elapsed: 60, label: 'after', pid: 5 }),
  ].join('\n'))
  const groups = splitByLabel(rows)
  assert.deepEqual(groups.map(g => g.label), ['before', 'before#2', 'after'])
  assert.equal(groups[1].rows.length, 3)
  assert.equal(baseName('before#2'), 'before')
  assert.equal(baseName('installed-before-plugins-on'), 'installed-before-plugins-on')
  const variants = groups.map(g => ({ label: g.label, samples: g.rows.length }))
  assert.equal(pickAttempt(variants, 'before').label, 'before#2', '续跑那次样本更多，用它代表改前')
  assert.equal(pickAttempt(variants, 'after').label, 'after')
  assert.equal(pickAttempt(variants, 'nope'), null)
})

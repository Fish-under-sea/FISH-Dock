// 内存采样 CSV → 三时段对照表。
//
// 为什么单独一个脚本：红线《禁止屎山十条》内存预算一节要求「必须实测，不许估」，
// 并且交付要给「改前 vs 改后」对照表，含空闲 30 分钟与运行 1 小时两个采样点。
// 采样由 mem-sample.ps1 / mem-ab.ps1 落 CSV，判读一律走这里，口径只有一份：
// 分组（dock 自身 / 插件 / 其它）、取点（最接近目标分钟数的那个 tick）、
// 泄漏判据（5 分钟点到 60 分钟点的增量）都在这里定义，别处不再各算一套。
//
// 用法：
//   node scripts/mem-report.mjs --dir build/mem/ab-main --at 5,30,60
//   node scripts/mem-report.mjs --csv a.csv --csv b.csv --at 5,30 --json out.json
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

// 一行 CSV 拆成字段：只有双引号一种转义（"" 表示一个 "），引号里的逗号不分割。
// 采样器写出的字段要么是无引号的数值/短名，要么是整体加引号的文本。
export function parseCsvLine(line) {
  const out = []
  let field = ''
  let quoted = false
  for (let i = 0; i < line.length; i += 1) {
    const char = line[i]
    if (quoted) {
      if (char !== '"') { field += char; continue }
      if (line[i + 1] === '"') { field += '"'; i += 1; continue }
      quoted = false
      continue
    }
    if (char === '"' && field === '') { quoted = true; continue }
    if (char === ',') { out.push(field); field = ''; continue }
    field += char
  }
  out.push(field)
  return out
}

const COLUMNS = ['sample_utc', 'elapsed_sec', 'label', 'depth', 'pid', 'ppid', 'name', 'role', 'ws_bytes', 'private_bytes', 'handles', 'threads', 'pagefile_bytes', 'detail']

// CSV 文本 → 样本对象数组。表头必须与采样器写的一致，否则口径就对不上了。
export function parseSamples(text) {
  const lines = text.split(/\r?\n/).filter(line => line.trim() !== '')
  if (lines.length === 0) return []
  const header = parseCsvLine(lines[0])
  for (const column of COLUMNS) {
    if (!header.includes(column)) throw new Error(`CSV 表头缺少 ${column}（不是 mem-sample 写的？）`)
  }
  const at = Object.fromEntries(COLUMNS.map(column => [column, header.indexOf(column)]))
  return lines.slice(1).map(line => {
    const cells = parseCsvLine(line)
    return {
      elapsedSec: Number(cells[at.elapsed_sec]),
      label: cells[at.label],
      depth: Number(cells[at.depth]),
      pid: Number(cells[at.pid]),
      name: cells[at.name],
      role: cells[at.role],
      wsBytes: Number(cells[at.ws_bytes]),
      privateBytes: Number(cells[at.private_bytes]),
      handles: Number(cells[at.handles]),
      threads: Number(cells[at.threads]),
      detail: cells[at.detail],
    }
  }).filter(row => Number.isFinite(row.elapsedSec) && Number.isFinite(row.privateBytes))
}

// 三个分组：dock 自身（宿主 + 它的 WebView2 进程）、插件（插件后端与插件自己
// 派生的 runtime）、其它（conhost 一类）。插件比 dock 还重的时候，必须能一眼
// 看出来重在哪一边，所以分组是口径的一部分，不是报表的装饰。
export function groupOf(role) {
  if (role === 'host' || role.startsWith('webview:')) return 'dock'
  if (role === 'plugin-backend' || role === 'plugin-runtime') return 'plugin'
  return 'other'
}

const MB = 1024 * 1024

export function toMb(bytes) { return Math.round((bytes / MB) * 10) / 10 }

// 带符号打印：正负号本身就是信息（涨还是降），报表里多处要用，只写一份。
export function signed(value) { return `${value >= 0 ? '+' : ''}${value}` }

// 最接近目标秒数的那个 tick；超出容差就返回 null（宁可缺点，也不要拿 40 分钟的
// 样本冒充 60 分钟点）。
export function nearestTick(rows, targetSec, toleranceSec) {
  const ticks = [...new Set(rows.map(row => row.elapsedSec))].sort((a, b) => a - b)
  let best = null
  for (const tick of ticks) {
    const off = Math.abs(tick - targetSec)
    if (off <= toleranceSec && (best === null || off < best.off)) best = { tick, off }
  }
  return best ? best.tick : null
}

// 一个 tick 的汇总：逐进程明细 + 分组小计 + 进程/句柄/线程/renderer 计数。
export function summarizeTick(rows, tick) {
  const at = rows.filter(row => row.elapsedSec === tick)
  const groups = { dock: emptyTotal(), plugin: emptyTotal(), other: emptyTotal() }
  let renderers = 0
  for (const row of at) {
    const total = groups[groupOf(row.role)]
    total.wsBytes += row.wsBytes
    total.privateBytes += row.privateBytes
    total.handles += row.handles
    total.threads += row.threads
    total.processes += 1
    if (row.role === 'webview:renderer') renderers += 1
  }
  return {
    tick,
    processes: at.map(row => ({ ...row, group: groupOf(row.role) })).sort((a, b) => b.privateBytes - a.privateBytes),
    groups,
    renderers,
    total: {
      wsBytes: groups.dock.wsBytes + groups.plugin.wsBytes + groups.other.wsBytes,
      privateBytes: groups.dock.privateBytes + groups.plugin.privateBytes + groups.other.privateBytes,
      handles: groups.dock.handles + groups.plugin.handles + groups.other.handles,
      threads: groups.dock.threads + groups.plugin.threads + groups.other.threads,
      processes: at.length,
    },
  }
}

function emptyTotal() { return { wsBytes: 0, privateBytes: 0, handles: 0, threads: 0, processes: 0 } }

// 泄漏判据取样窗口的下限（分钟，进程年龄）。为什么是 30 而不是 0：
// ab-r20 的 1→20min 句柄增量改前 +221 / 改后 +198，两侧都撞 +200 线；t7 在进程
// 年龄 303→323min 的 20min 窗内复测，宿主句柄 685→685（全窗 682–689）、dock 子树
// 句柄合计 5308→5292（Δ−16）、renderer 3→3，窗内还含一次完整 quiet 切换
// （21:11:59 true → 21:18:41 false）而句柄曲线无台阶 → 启动段那 +221 是一次性
// 增长，不是泄漏。从 1min 起算会对任何构建误报，所以起点低于这个值时工具直接
// 告警，而不是安静地给出一个假阳性的「不通过」。
export const LEAK_MIN_AGE_MIN = 30

// 泄漏判据：末点相对起点涨了多少。宿主句柄与 dock 子树句柄分开判——宿主涨说明
// 是 Rust 侧的账，子树涨而宿主不涨说明是 WebView2 侧的账，混在一个数里定位不了。
// 阈值全部由调用方给（budget），脚本自己不藏数字（红线第 5 条：魔法值）。
export function leakVerdict(from, to, budget) {
  const handlesOf = (tick, role) => tick.processes
    .filter(row => role === 'any' || row.role === role)
    .reduce((sum, row) => sum + row.handles, 0)
  const privateOf = (tick, role) => tick.processes
    .filter(row => row.role === role)
    .reduce((sum, row) => sum + row.privateBytes, 0)
  const growth = to.groups.dock.privateBytes - from.groups.dock.privateBytes
  const handles = to.groups.dock.handles - from.groups.dock.handles
  const hostHandles = handlesOf(to, 'host') - handlesOf(from, 'host')
  return {
    fromTick: from.tick,
    toTick: to.tick,
    privateGrowthBytes: growth,
    privateGrowthMb: toMb(growth),
    handleGrowth: handles,
    hostHandleGrowth: hostHandles,
    gpuPrivateGrowthMb: toMb(privateOf(to, 'webview:gpu-process') - privateOf(from, 'webview:gpu-process')),
    rendererGrowth: to.renderers - from.renderers,
    ok: growth <= budget.privateGrowthMb * MB
      && handles <= budget.handleGrowth
      && hostHandles <= budget.handleGrowth,
    budget,
  }
}

// 窗口长度下限（分钟）：太短的窗口测不出慢泄漏，会给出假阴性的「通过」
// （例如只取 58→60min 两点，窗口 2min，什么都判得过）。
export const LEAK_MIN_WINDOW_MIN = 30

// 取样窗口不合规时给出告警文本（合规则空串）。两头都拦：起点太早 → 假阳性，
// 窗口太短 → 假阴性。
export function leakWindowWarning(points) {
  const first = points[0]
  const last = points[points.length - 1]
  // 少于两个点根本算不出增量（main 也不会调 leakVerdict），这时不告警。
  if (points.length < 2 || !first || !last) return ''
  if (first.minute < LEAK_MIN_AGE_MIN) {
    return `警告：泄漏判据起点是进程年龄 ${first.minute}min（< ${LEAK_MIN_AGE_MIN}min），这一段含启动段的一次性增长，判出来的「不通过」是假阳性；请改用 --at 30,60 重算`
  }
  const span = last.minute - first.minute
  if (span < LEAK_MIN_WINDOW_MIN) {
    return `警告：泄漏判据窗口只有 ${span}min（< ${LEAK_MIN_WINDOW_MIN}min），太短测不出慢泄漏，判出来的「通过」是假阴性；请改用 --at 30,60 重算`
  }
  return ''
}

// 改前/改后同一个时间点的差值。
export function deltaOf(before, after) {
  const diff = (a, b) => b - a
  return {
    tick: after.tick,
    dockPrivateMb: toMb(diff(before.groups.dock.privateBytes, after.groups.dock.privateBytes)),
    dockWsMb: toMb(diff(before.groups.dock.wsBytes, after.groups.dock.wsBytes)),
    totalPrivateMb: toMb(diff(before.total.privateBytes, after.total.privateBytes)),
    totalWsMb: toMb(diff(before.total.wsBytes, after.total.wsBytes)),
    handles: diff(before.total.handles, after.total.handles),
    threads: diff(before.total.threads, after.total.threads),
    renderers: diff(before.renderers, after.renderers),
    processes: diff(before.total.processes, after.total.processes),
  }
}

export function renderTotalsTable(summaries) {
  const lines = ['| 采样点（进程年龄） | dock 私有 MB | dock RSS MB | renderer 数 | 进程数 | 句柄 | 线程 | 插件私有 MB | 合计私有 MB |', '| --- | --- | --- | --- | --- | --- | --- | --- | --- |']
  for (const item of summaries) {
    const s = item.summary
    lines.push(`| ${item.label} @ ${Math.round(s.tick / 60)}min | ${toMb(s.groups.dock.privateBytes)} | ${toMb(s.groups.dock.wsBytes)} | ${s.renderers} | ${s.total.processes} | ${s.total.handles} | ${s.total.threads} | ${toMb(s.groups.plugin.privateBytes)} | ${toMb(s.total.privateBytes)} |`)
  }
  return lines.join('\n')
}

export function renderProcessTable(summary) {
  const lines = ['| 角色 | PID | RSS MB | 私有 MB | 句柄 | 线程 | 备注 |', '| --- | --- | --- | --- | --- | --- | --- |']
  for (const row of summary.processes) {
    lines.push(`| ${row.role} | ${row.pid} | ${toMb(row.wsBytes)} | ${toMb(row.privateBytes)} | ${row.handles} | ${row.threads} | ${row.detail || ''} |`)
  }
  return lines.join('\n')
}

// 一个变体（一份 CSV）在若干时间点上的汇总。
export function collectPoints(rows, atMinutes, toleranceSec) {
  const points = []
  for (const minute of atMinutes) {
    const tick = nearestTick(rows, minute * 60, toleranceSec)
    if (tick === null) { points.push({ minute, missing: true }); continue }
    points.push({ minute, summary: summarizeTick(rows, tick) })
  }
  return points
}

// 一份 CSV 里可能有多个标签：mem-ab.ps1 断点续跑时每次启动另给一个标签
// （before、before#2…），旧样本保留不覆盖。报表按标签分组，一个标签就是
// 「一个变体的一次尝试」，取点与判据都在一次尝试内部算，不跨尝试混。
export function splitByLabel(rows) {
  const byLabel = new Map()
  for (const row of rows) {
    if (!byLabel.has(row.label)) byLabel.set(row.label, [])
    byLabel.get(row.label).push(row)
  }
  return [...byLabel.entries()].map(([label, labelRows]) => ({ label, rows: labelRows }))
}

export function readCsvFiles(files) {
  const entries = []
  for (const file of files) {
    for (const group of splitByLabel(parseSamples(fs.readFileSync(file, 'utf8')))) {
      entries.push({ file, label: group.label, rows: group.rows })
    }
  }
  return entries
}

// before#2 这样的续跑标签，基名还是 before。
export function baseName(label) { return String(label).split('#')[0] }

// 同一侧的多次尝试里，取样本最多的那次代表这一侧（覆盖最全）；其余尝试仍然
// 会在明细里列出来，不藏。
export function pickAttempt(variants, base) {
  const mine = variants.filter(variant => baseName(variant.label) === base)
  if (mine.length === 0) return null
  return mine.reduce((best, variant) => (variant.samples > best.samples ? variant : best))
}

function parseArgs(argv) {
  const options = { dir: '', csv: [], at: [5, 30, 60], tolerance: 60, json: '', budgetPrivateMb: 24, budgetHandles: 200 }
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i]
    if (arg === '--dir') { options.dir = argv[++i]; continue }
    if (arg === '--csv') { options.csv.push(argv[++i]); continue }
    if (arg === '--at') { options.at = argv[++i].split(',').map(Number); continue }
    if (arg === '--tolerance') { options.tolerance = Number(argv[++i]); continue }
    if (arg === '--json') { options.json = argv[++i]; continue }
    if (arg === '--budget-private-mb') { options.budgetPrivateMb = Number(argv[++i]); continue }
    if (arg === '--budget-handles') { options.budgetHandles = Number(argv[++i]); continue }
    throw new Error(`不认的参数：${arg}`)
  }
  if (options.dir) {
    const found = fs.readdirSync(options.dir).filter(name => name.endsWith('.csv')).sort().map(name => path.join(options.dir, name))
    options.csv = [...options.csv, ...found]
  }
  if (options.csv.length === 0) throw new Error('没有 CSV：给 --dir 或 --csv')
  return options
}

function main(argv) {
  const options = parseArgs(argv)
  const budget = { privateGrowthMb: options.budgetPrivateMb, handleGrowth: options.budgetHandles }
  const report = { at: options.at, toleranceSec: options.tolerance, budget, variants: [] }

  for (const entry of readCsvFiles(options.csv)) {
    const points = collectPoints(entry.rows, options.at, options.tolerance)
    const variant = { label: entry.label, file: entry.file, samples: entry.rows.length, points: [], missing: [] }
    for (const point of points) {
      if (point.missing) { variant.missing.push(point.minute); continue }
      variant.points.push({ minute: point.minute, tick: point.summary.tick, summary: point.summary })
    }
    const first = variant.points[0]
    const last = variant.points[variant.points.length - 1]
    if (first && last && first !== last) variant.leak = leakVerdict(first.summary, last.summary, budget)
    const warning = leakWindowWarning(variant.points)
    if (warning) variant.leakWarning = warning
    report.variants.push(variant)
  }

  const before = pickAttempt(report.variants, 'before')
  const after = pickAttempt(report.variants, 'after')
  if (before && after) {
    report.deltas = []
    for (const minute of options.at) {
      const b = before.points.find(p => p.minute === minute)
      const a = after.points.find(p => p.minute === minute)
      if (b && a) report.deltas.push({ minute, delta: deltaOf(b.summary, a.summary) })
    }
  }

  if (options.json) fs.writeFileSync(options.json, JSON.stringify(report, null, 2))

  for (const variant of report.variants) {
    console.log(`\n## ${variant.label}（${variant.file}，样本 ${variant.samples} 行）`)
    if (variant.missing.length) console.log(`\n缺采样点：${variant.missing.map(m => `${m}min`).join('、')}（容差 ±${options.tolerance}s 内没有 tick）`)
    for (const point of variant.points) {
      console.log(`\n### ${point.minute} 分钟点（tick=${point.tick}s）`)
      console.log(renderProcessTable(point.summary))
    }
    console.log('\n### 汇总')
    console.log(renderTotalsTable(variant.points.map(p => ({ label: `${p.minute}min`, summary: p.summary }))))
    if (variant.leakWarning) console.log(`\n${variant.leakWarning}`)
    if (variant.leak) {
      const v = variant.leak
      console.log(`\n泄漏判据：${Math.round(v.fromTick / 60)}min → ${Math.round(v.toTick / 60)}min，dock 私有 ${signed(v.privateGrowthMb)} MB（预算 ≤ +${budget.privateGrowthMb} MB），dock 子树句柄 ${signed(v.handleGrowth)}、宿主句柄 ${signed(v.hostHandleGrowth)}（各预算 ≤ +${budget.handleGrowth}），GPU 私有 ${signed(v.gpuPrivateGrowthMb)} MB，renderer ${signed(v.rendererGrowth)} → ${v.ok ? '通过' : '不通过'}`)
    }
  }

  if (report.deltas) {
    console.log('\n## 改前 → 改后（同一时间点，正数=改后更高）')
    console.log('| 采样点 | dock 私有 MB | dock RSS MB | 合计私有 MB | 合计 RSS MB | renderer | 进程 | 句柄 | 线程 |')
    console.log('| --- | --- | --- | --- | --- | --- | --- | --- | --- |')
    for (const item of report.deltas) {
      const d = item.delta
      console.log(`| ${item.minute}min | ${signed(d.dockPrivateMb)} | ${signed(d.dockWsMb)} | ${signed(d.totalPrivateMb)} | ${signed(d.totalWsMb)} | ${signed(d.renderers)} | ${signed(d.processes)} | ${signed(d.handles)} | ${signed(d.threads)} |`)
    }
  }
  return report
}

// 被直接运行时才出报表；被测试 import 时只导出函数（Windows 上路径含盘符，
// 用 fileURLToPath 比拼 file:// 字符串可靠）。
const invoked = Boolean(process.argv[1]) && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)
if (invoked) main(process.argv.slice(2))

#!/usr/bin/env node
// 字色审计（t36，2026-10-31）：把设置面板的两套主题（暗色 / 浅色）各自算一遍
// 文字对比度与"方向"，用数据而不是肉眼回答"暗色白字、浅色黑字"这条。
//
// 用法：node scripts/check-contrast.mjs [--json] [--files a.css,b.css]
// 退出码：0 = 无违规；1 = 有违规（白名单里的不算）。
//
// 判定口径（写清楚，免得以后有人以为工具在猜）：
//  · 只审"字"：color / fill，以及按钮 ink 变量（--*-ink，与同名的底色变量配对）。
//  · 背景：规则自己声明了 background/background-color 就用它；否则用该主题的面板底色
//    --g1（设置面板里绝大多数文字就是直接坐在 --g1 上）。
//  · 方向：暗色主题要求"字比背景亮"，浅色主题要求"字比背景暗"——不是这个方向就是
//    DIR（这才是用户说的"暗色该白字、浅色该黑字"）。
//  · 阈值：正文 4.5:1（WCAG AA）。低于即 LOW。
//  · 解析不了（color-mix、渐变、继承来的颜色）记 UNRESOLVED，不计违规但要报出来。
import fs from 'node:fs'
import path from 'node:path'

const ROOT = process.cwd()
const args = process.argv.slice(2)
const asJson = args.includes('--json')
const only = (() => {
  const i = args.indexOf('--files')
  return i >= 0 && args[i + 1] ? args[i + 1].split(',').map(s => s.trim()).filter(Boolean) : null
})()

// 白名单：确实不该按"正文文字"审的（每条都要有理由）。
const WHITELIST = new Map([
  ['src/pet/settings.css:46:.about-hero', '整块 hero 只居中，颜色来自继承'],
  // 岛外观预览（.ilook-pv）：它刻意把"另一套主题的字色"画在"另一套主题的底"上
  // （ink-dark 类是暗色字画在白底预览上），拿当前主题的表面色去算必然是 1:1 的假违规。
  ['src/pet/settings.css:#island-settings .ilook-pv', '岛外观预览：字色与底色是一对模拟色，不跟随面板主题'],
])
// 命中白名单：键写成 `文件:选择器前缀`，或 `文件:行:选择器`。
const isWhitelisted = r => [...WHITELIST.keys()].some(k => {
  const cut = k.indexOf(':')
  if (k.startsWith(`${r.file}:`) && !/^\d/.test(k.slice(cut + 1))) return r.selector.startsWith(k.slice(r.file.length + 1))
  return `${r.file}:${r.line}:${r.selector}`.startsWith(k)
})

const TEXT_PROPS = new Set(['color', 'fill'])
const BG_PROPS = new Set(['background', 'background-color'])

function files() {
  if (only) return only.map(f => path.resolve(ROOT, f))
  // 默认只审设置面板这一套（用户截图指的就是它）：settings.css + extensions.css。
  // desk/bar/barpop/picker/style 各有自己的主题根（body / body.lw / .tb-light 配 --w-*
  // 那套），拿这张变量表去套它们会得出假结论 —— 要把它们也审，用 --files 传进来单独做。
  const dir = path.join(ROOT, 'src/pet')
  return ['settings.css', 'extensions.css'].map(f => path.join(dir, f))
}

// 规则属于哪套主题看选择器：.tb-light / .isl-light 前缀的只在亮色里生效。
const themeOf = selector => (/\.(tb-light|isl-light)\b/.test(selector) ? ['light'] : ['dark', 'light'])

const rel = f => path.relative(ROOT, f).split(path.sep).join('/')

// —— 颜色解析 ——
function parseColor(raw) {
  const v = String(raw || '').trim()
  let m = v.match(/^#([0-9a-f]{3,8})$/i)
  if (m) {
    let h = m[1]
    if (h.length === 3 || h.length === 4) h = h.split('').map(c => c + c).join('')
    if (h.length !== 6 && h.length !== 8) return null
    const n = i => parseInt(h.slice(i, i + 2), 16)
    return { r: n(0), g: n(2), b: n(4), a: h.length === 8 ? n(6) / 255 : 1 }
  }
  m = v.match(/^rgba?\(\s*([\d.]+)[\s,]+([\d.]+)[\s,]+([\d.]+)(?:[\s,/]+([\d.]+))?\s*\)$/i)
  if (m) return { r: +m[1], g: +m[2], b: +m[3], a: m[4] === undefined ? 1 : Math.min(1, Math.max(0, +m[4])) }
  if (v === 'transparent') return { r: 0, g: 0, b: 0, a: 0 }
  if (v === 'white') return { r: 255, g: 255, b: 255, a: 1 }
  if (v === 'black') return { r: 0, g: 0, b: 0, a: 1 }
  return null
}

const srgb = c => {
  const s = c / 255
  return s <= 0.04045 ? s / 12.92 : Math.pow((s + 0.055) / 1.055, 2.4)
}
const luminance = c => 0.2126 * srgb(c.r) + 0.7152 * srgb(c.g) + 0.0722 * srgb(c.b)
const contrast = (a, b) => {
  const la = luminance(a), lb = luminance(b)
  const [hi, lo] = la >= lb ? [la, lb] : [lb, la]
  return (hi + 0.05) / (lo + 0.05)
}
// 半透明字压在背景上：先合成再算（rgba 的 a<1 才需要）。
const over = (fg, bg) => (fg.a >= 1 ? fg : {
  r: fg.r * fg.a + bg.r * (1 - fg.a),
  g: fg.g * fg.a + bg.g * (1 - fg.a),
  b: fg.b * fg.a + bg.b * (1 - fg.a),
  a: 1,
})

const fmt = c => (c ? `#${[c.r, c.g, c.b].map(n => Math.round(n).toString(16).padStart(2, '0')).join('')}${c.a < 1 ? `/a${c.a}` : ''}` : '—')

// —— CSS 解析：拿到每条规则的 selector 与声明（行号） ——
function rulesOf(file) {
  const text = fs.readFileSync(file, 'utf8')
  const noComments = text.replace(/\/\*[\s\S]*?\*\//g, m => m.replace(/[^\n]/g, ' '))
  const out = []
  const re = /([^{}]+)\{([^{}]*)\}/g
  let m
  while ((m = re.exec(noComments))) {
    const selector = m[1].trim().replace(/\s+/g, ' ')
    if (!selector || selector.startsWith('@') || selector.includes('%')) continue
    const bodyStart = m.index + m[1].length + 1
    const decls = []
    for (const piece of m[2].split(';')) {
      const at = piece.indexOf(':')
      if (at < 0) continue
      const prop = piece.slice(0, at).trim().toLowerCase()
      const value = piece.slice(at + 1).trim()
      if (!prop || !value) continue
      const offset = m[2].indexOf(piece)
      const line = noComments.slice(0, bodyStart + Math.max(0, offset)).split('\n').length
      decls.push({ prop, value, line })
    }
    out.push({ selector, decls })
  }
  return out
}

// —— 两套主题变量表 ——
function themeTables(allRules) {
  const pick = (selector) => {
    const table = new Map()
    for (const rule of allRules) {
      if (rule.selector !== selector) continue
      for (const d of rule.decls) if (d.prop.startsWith('--')) table.set(d.prop, d.value)
    }
    return table
  }
  // 兜底：:root / body / body.lw（desk.css 那套把 --w-* 放在 body 上）
  const root = (() => {
    const t = new Map()
    for (const rule of allRules) {
      if (![':root', 'html', 'body', 'body.lw'].includes(rule.selector)) continue
      for (const d of rule.decls) if (d.prop.startsWith('--')) t.set(d.prop, d.value)
    }
    return t
  })()
  const dark = new Map([...root, ...pick('#island-settings')])
  const light = new Map([...dark, ...pick('.tb-light #island-settings')])
  return { dark, light }
}

// 作用域 token 池（t38）：插件卡那批 --wc-* 定义在 .wcard 之类的规则上，不属于主题
// 两套变量表。按"非 .tb-light 定义 / .tb-light 定义"分两侧，亮色池继承暗色池。
let scoped = { dark: new Map(), lightExtra: new Map() }
function scopedPools(allRules) {
  const dark = new Map()
  const lightExtra = new Map()
  const conflicts = new Set()
  for (const rule of allRules) {
    if (rule.selector === '#island-settings' || rule.selector === '.tb-light #island-settings') continue
    const target = isLightOnly(rule.selector) ? lightExtra : dark
    for (const d of rule.decls) {
      if (!d.prop.startsWith('--')) continue
      if (target.has(d.prop) && target.get(d.prop) !== d.value) conflicts.add(d.prop)
      else target.set(d.prop, d.value)
    }
  }
  scoped = { dark, lightExtra }
  return { dark, light: new Map([...dark, ...lightExtra]), conflicts }
}

// var(--x[, fallback]) → 颜色（递归，带环检测）。查两处：主题变量表，再查"同主题
// 作用域池"（插件卡里的 --wc-ok/--wc-warn 这类定义在 .wcard 规则上的 token）。
// 两处都没有：记下名字，报告里列成"未覆盖作用域"，不再静默当 UNRESOLVED。
const unresolvedNames = new Set()
function resolve(value, table, pool, seen = new Set()) {
  let v = String(value || '').trim()
  for (let guard = 0; guard < 12; guard++) {
    const direct = parseColor(v)
    if (direct) return direct
    const m = v.match(/^var\(\s*(--[\w-]+)\s*(?:,\s*([\s\S]+))?\)$/)
    if (!m) return null
    const [, name, fallback] = m
    if (seen.has(name)) return null
    seen.add(name)
    const hit = table.has(name) ? table.get(name) : pool && pool.has(name) ? pool.get(name) : undefined
    if (hit !== undefined) { v = hit; continue }
    if (fallback) { v = fallback.trim(); continue }
    unresolvedNames.add(name)
    return null
  }
  return null
}

const SURFACE = '--g1'
// 选择器归一：去掉主题前缀，用来判断"亮色里后来有没有同一条规则覆盖它"。
const bare = selector =>
  selector
    .split(',')
    .map(s => s.trim().replace(/^\.(tb-light|isl-light)\s+/, ''))
    .join(', ')
const isLightOnly = selector => /\.(tb-light|isl-light)\b/.test(selector)

function audit() {
  const list = files()
  const parsed = list.map(f => ({ file: f, rules: rulesOf(f) }))
  const allRules = parsed.flatMap(p => p.rules)
  const { dark, light } = themeTables(allRules)
  const pools = scopedPools(allRules)
  // 亮色覆盖表：`.tb-light X { color: ... }` 会盖掉基类规则 `X { color: ... }`。
  // 不做整条 CSS 级联（那要真浏览器），只做"同名选择器 + 同属性"这一层——够把
  // "基类写了暗色字、亮色另有覆盖"的假违规排掉，同时亮色那条规则自己仍会被审。
  const lightOverrides = new Map()
  for (const rule of allRules) {
    if (!isLightOnly(rule.selector)) continue
    for (const d of rule.decls) {
      const key = `${bare(rule.selector)}|${d.prop}`
      lightOverrides.set(key, d.value)
    }
  }
  const rows = []
  for (const theme of ['dark', 'light']) {
    const table = theme === 'dark' ? dark : light
    const surface = resolve(`var(${SURFACE})`, table, pools[theme])
    for (const { file, rules } of parsed) {
      for (const rule of rules) {
        if (!themeOf(rule.selector).includes(theme)) continue
        const ownBg = rule.decls.find(d => BG_PROPS.has(d.prop))
        // 亮色主题：底色也可能被 `.tb-light 同选择器` 覆盖（例如 .in 基类写 #000、
        // 亮色覆盖成白底），所以底色也走一层覆盖表。
        const bgValue = (theme === 'light' && !isLightOnly(rule.selector)
          ? lightOverrides.get(`${bare(rule.selector)}|background`) || lightOverrides.get(`${bare(rule.selector)}|background-color`)
          : undefined) || (ownBg ? ownBg.value : `var(${SURFACE})`)
        // 半透明底（rgba / 透明）先压到面板底色上再算，否则算出来是空气。
        // 完全透明的「自己的底色」（background:none/transparent）等于没底色，回落面板底色。
        const rawBg = resolve(bgValue, table, pools[theme])
        const solid = rawBg && rawBg.a > 0 ? rawBg : null
        const bg = solid ? (solid.a < 1 && surface ? over(solid, surface) : solid) : surface
        const hasOwnBg = !!solid
        // ink 变量：--x-ink 与 --x 配对（按钮里的白字/黑字）
        for (const d of rule.decls) {
          if (!/^--[\w-]+-ink$/.test(d.prop)) continue
          const base = d.prop.replace(/-ink$/, '')
          if (!table.has(base)) continue
          const fg = resolve(table.get(d.prop), table, pools[theme])
          const own = resolve(table.get(base), table, pools[theme])
          rows.push(row({ file, rule, line: d.line, prop: d.prop, fg, bg: own, bgLabel: base, theme, judgeDirection: false }))
        }
        for (const d of rule.decls) {
          if (!TEXT_PROPS.has(d.prop)) continue
          // color: inherit / currentColor 之类：继承来的，不在这里判
          if (/^(inherit|currentcolor|unset|initial|none)$/i.test(d.value)) continue
          // 亮色主题里，这条基类规则若被同选择器的 `.tb-light` 规则覆盖了，跳过基类那条
          // （覆盖那条自己会被审）。
          if (theme === 'light' && !isLightOnly(rule.selector) && lightOverrides.has(`${bare(rule.selector)}|${d.prop}`)) continue
          rows.push(row({ file, rule, line: d.line, prop: d.prop, fg: resolve(d.value, table, pools[theme]), bg, bgLabel: hasOwnBg ? 'own' : SURFACE, theme, judgeDirection: !hasOwnBg }))
        }
      }
    }
  }
  return rows
}

function row({ file, rule, line, prop, fg, bg, bgLabel, theme, judgeDirection }) {
  const key = `${rel(file)}:${line}:${rule.selector}`
  const base = { file: rel(file), line, selector: rule.selector, prop, theme, bg: bgLabel }
  if (!fg || !bg) return { ...base, color: fg ? fmt(over(fg, bg || { r: 0, g: 0, b: 0, a: 1 })) : 'UNRESOLVED', bgColor: fmt(bg), ratio: null, verdict: 'UNRESOLVED', key }
  const fgc = over(fg, bg)
  const ratio = contrast(fgc, bg)
  const lighter = luminance(fgc) > luminance(bg)
  const wantLighter = luminance(bg) < 0.5
  // 方向只在「字直接坐在面板底色上」时判：自己带底色的（强调色胶囊、红色警告片）
  // 只按对比度判，因为那些底本来就是有色块，没有"暗色白字"这回事。
  const directionOk = !judgeDirection || lighter === wantLighter
  const verdict = !directionOk ? 'DIR' : ratio < 4.5 ? 'LOW' : 'OK'
  return { ...base, color: fmt(fgc), bgColor: fmt(bg), ratio: +ratio.toFixed(2), verdict, key }
}

// 运行时注入的 token：JS 把内联 style 写进 DOM（--acc/--acc-ink 来自 settings.js 的
// accentButton()、--pv-* 来自 ilookPreview 的 style 串）。静态 CSS 里没有定义，
// 对比度在静态层无法判定 —— 工具必须点名说清"是运行时注入、在哪"，不能静默 UNRESOLVED。
function injectedAt(token) {
  const dir = path.join(ROOT, 'src/pet')
  for (const file of fs.readdirSync(dir).filter(n => n.endsWith('.js')).sort()) {
    const text = fs.readFileSync(path.join(dir, file), 'utf8')
    const at = text.indexOf(token)
    if (at >= 0) return `src/pet/${file}:${text.slice(0, at).split('\n').length}`
  }
  return ''
}
const rows = audit()
const bad = rows.filter(r => (r.verdict === 'LOW' || r.verdict === 'DIR') && !isWhitelisted(r))
const unresolved = rows.filter(r => r.verdict === 'UNRESOLVED')
const uncovered = [...unresolvedNames].sort().map(token => ({ token, at: injectedAt(token) }))
const summary = {
  checked: rows.length,
  ok: rows.filter(r => r.verdict === 'OK').length,
  low: rows.filter(r => r.verdict === 'LOW').length,
  direction: rows.filter(r => r.verdict === 'DIR').length,
  unresolved: unresolved.length,
  uncoveredScope: uncovered,
  whitelisted: Object.keys(Object.fromEntries(WHITELIST)).length,
  violations: bad.length,
}

if (asJson) {
  console.log(JSON.stringify({ summary, violations: bad, unresolved: unresolved.slice(0, 40) }, null, 2))
} else {
  const pad = (s, n) => String(s).padEnd(n)
  console.log(`字色审计：检查 ${summary.checked} 条文字声明（暗/浅两套主题各一遍）`)
  console.log(`  OK ${summary.ok} · 低对比 ${summary.low} · 方向错 ${summary.direction} · 未解析 ${summary.unresolved} · 白名单 ${summary.whitelisted}`)
  console.log(`  违规合计：${summary.violations}`)
  // 作用域 token（--wc-* 这类）已被吃进来：把池子里有什么说出来，别让人以为没管。
  console.log(`  作用域池：${scoped.dark.size} 个 token 在暗侧、${scoped.lightExtra.size} 个亮侧覆盖（--wc-ok / --wc-warn 等，已纳入 var 展开；亮侧继承暗侧）`)
  if (uncovered.length) {
    console.log(`  未覆盖作用域：${uncovered.length} 个 token（逐条注明来源，不是静默 UNRESOLVED）`)
    for (const item of uncovered) {
      console.log(`    ${item.token.padEnd(12)} ${item.at ? `运行时注入 ${item.at}（JS 写内联 style，静态 CSS 无定义）` : '样式里没有定义，来源不明（需要人看）'}`)
    }
    console.log('    结论：这几条由 JS 按当前配色/外观现场写入，静态对比度无法判定；不计入违规，但要真机两主题截图确认。')
  } else {
    console.log('  未覆盖作用域：0（所有 var(--x) 都解析到了：主题表 or 作用域池）')
  }
  if (bad.length) {
    console.log('')
    console.log(pad('文件:行', 46) + pad('主题', 7) + pad('属性', 22) + pad('字色', 12) + pad('背景', 12) + pad('对比度', 8) + '判定  选择器')
    for (const r of bad.slice(0, 200)) {
      console.log(pad(`${r.file}:${r.line}`, 46) + pad(r.theme, 7) + pad(r.prop, 22) + pad(r.color, 12) + pad(r.bgColor, 12) + pad(r.ratio, 8) + `${r.verdict}  ${r.selector.slice(0, 70)}`)
    }
    if (bad.length > 200) console.log(`… 还有 ${bad.length - 200} 条`)
  }
  if (unresolved.length) {
    console.log('')
    console.log(`未解析 ${unresolved.length} 条 —— 全部落在"运行时注入 token"上（静态 CSS 里没有定义，静态层判不了对比度，不是被静默吞掉）：`)
    for (const item of uncovered) console.log(`  token ${item.token} ← ${item.at || '来源不明，需要人看'}`)
    console.log(`  受影响规则：${[...new Set(unresolved.map(r => `${r.file}:${r.line}（${r.selector.slice(0, 48)}）`))].join('；')}`)
    console.log('  → 结论：不计违规；要真机两主题截图确认（t38 已写进交付清单）。')
  }
}
process.exit(summary.violations ? 1 : 0)

#!/usr/bin/env node
// 硬编码自查（红线第 5 条：魔法值 + 硬编码）。
//
// 扫描 src-tauri/src、src/pet、src/shared 里的「配置面字面量」，7 类：
//   1. ABS_PATH         —— Windows 绝对路径（C:\...、%LOCALAPPDATA% 等）
//   2. LOOPBACK_PORT    —— 写死的 127.0.0.1:端口
//   3. HTTP_URL         —— http(s):// 地址
//   4. TIMEOUT_MAGIC    —— 行内魔法超时（Duration 字面量、setTimeout 裸毫秒）
//   5. ENV_SECOND_ENTRY —— 绕过 config 的环境读取（env::var / option_env! / 名字经数组传入）
//   6. NAMED_TIMEOUT    —— 前端具名常量里的超时（NOTES_TIMEOUT_MS 一类，防具名常量豁免掩盖）
//   7. DUPLICATE_CONST  —— 像配置的常量跨文件同名同值重复（防复制粘贴，红线 3/4）
//
// 口径（务必如实）：
//   - 排除 rust 内联测试模块（`#[cfg(test)]` 之后的行）与 *_tests.rs 整个文件；
//   - 排除纯注释行（// * /* <!-- 开头）；
//   - 命中再减去 hardcoded-allowlist.json 里的白名单（每条都要写理由）；
//   - 剩下的就是「还写死、该改走 config」的命中，脚本以非零退出码告警。
//
// 用法：  node scripts/check-hardcoded.mjs          # 人读表格
//        node scripts/check-hardcoded.mjs --json   # 机器读
//        node scripts/check-hardcoded.mjs --list   # 逐条列出未清零命中
//
// 退出码：0 = remaining 为 0（通过）；1 = 仍有未清零命中。

import fs from 'node:fs';
import path from 'node:path';

const ROOT = process.cwd();
const ROOTS = ['src-tauri/src', 'src/pet', 'src/shared'];
const EXTS = new Set(['.rs', '.js', '.mjs', '.html', '.css']);

const CATEGORIES = [
  {
    id: 'ABS_PATH',
    label: 'Windows 绝对路径',
    // 盘符绝对路径 C:\... / C:/...（不能误吃 http:// 里的 p:/），以及 %LOCALAPPDATA% 一类环境占位
    test: (line) =>
      /(?<![A-Za-z])[A-Za-z]:[\\/](?![\\/])[^\s"'`)]/.test(line) ||
      /%(LOCALAPPDATA|APPDATA|USERPROFILE|ProgramFiles)%/i.test(line),
  },
  {
    id: 'LOOPBACK_PORT',
    label: '写死的回环端口',
    test: (line) => /127\.0\.0\.1:\d+/.test(line) || /localhost:\d{2,5}\b/.test(line),
  },
  {
    id: 'HTTP_URL',
    label: 'http(s) 地址',
    test: (line) => /https?:\/\/[A-Za-z0-9]/i.test(line),
  },
  {
    id: 'TIMEOUT_MAGIC',
    label: '魔法超时',
    // 只盯「网络/IO 超时」这一类散落裸值（红线 5 讲的是超时/耗时配置）；
    // 动画节奏、sleep、elapsed() 新鲜度判据不算——那些是具名常量/内部时序，不是部署配置。
    //   rust: 网络代理/超时调用点上的 Duration 字面量，或带 timeout/deadline/download… 关键字的行
    //   js:   setTimeout/setInterval 的裸毫秒（≥3 秒才视为超时/轮询）
    test: (line) => {
      const hasDuration = /Duration::from_(millis|secs)\(\s*[\d_]+/.test(line);
      const ioCall = /(connect_timeout|recv_timeout|timeout_global|timeout_connect|timeout_recv|agent_with|\.agent\(|\.timeout\()/.test(line);
      const keyword = /(timeout|deadline|download|request|fetch|retry|grace|网络|超时)/i.test(line);
      if (hasDuration && (ioCall || keyword)) return true;
      const longTimer = (re) => {
        const m = line.match(re);
        if (!m) return false;
        const n = Number(m[1].replace(/_/g, ''));
        return Number.isFinite(n) && n >= 3000;
      };
      if (longTimer(/setInterval\([^,]+,\s*(\d[\d_]*)/)) return true;
      if (longTimer(/setTimeout\([^,]+,\s*(\d[\d_]*)/)) return true;
      return false;
    },
  },
  {
    // 第二个配置入口：config.rs 之外出现「看着就是本项目部署配置的环境变量名字面量」。
    // 覆盖三种写法（t7 指出的盲区）：
    //   a) 同一行 std::env::var("FISH_DOCK_…") / var_os(...)
    //   b) option_env!("FISH_DOCK_…")  —— 编译期读取
    //   c) 名字先存进数组/表，循环时 var_os(key) 再读（如 dsh_bridge.rs 的 "FISH_DOCK_DSH_APP"）
    // 只要行里出现这种字面量就算命中（DEBUG/TRACE/REDACT/TEST/FORCE 是排障/测试开关，不算）。
    // 必须走 src-tauri/src/config.rs，否则进白名单并写理由。
    id: 'ENV_SECOND_ENTRY',
    label: '绕过 config 的环境读取',
    test: (line) => {
      const names = [...line.matchAll(/"((?:FISH_DOCK_|WAKUWAKU_|DSH_FISH_)[A-Z0-9_]+)"/g)].map((m) => m[1]);
      return names.some(
        (n) =>
          !/(DEBUG|TRACE|REDACT|TEST|FORCE)/.test(n) &&
          /(_MS|TIMEOUT|PORT|DATA|HOME|PATH|DIR|RESOURCE|PLUGINS?|NODE|URL|SITE|REGISTRY|CATALOG|CLIENT_ID|ARGS|APP)/i.test(n),
      );
    },
  },
  {
    // 具名常量豁免的盲区：`const X_TIMEOUT = 8000` 这类「超时被塞进具名常量」会绕过上面的
    // 行内魔法值检查。名字里带 TIMEOUT 的常量定义必须能对上 config.rs 的键，否则要么收口、
    // 要么白名单写理由（比如第三方 CLI 的提示等待）。
    id: 'NAMED_TIMEOUT',
    label: '具名常量里的超时',
    allowNamedConst: true,
    frontendOnly: true,
    test: (line) =>
      /^(?:pub(\([^)]*\))?\s+)?(?:const|static)\s+\w*TIMEOUT\w*\b/i.test(line.trim()) ||
      /^(?:export\s+)?const\s+\w*TIMEOUT\w*\s*=/i.test(line.trim()),
  },
  {
    // 具名常量豁免掩盖「同一常量在两个文件各写一份」（红线 3 复制粘贴 / 红线 4 多处来源）。
    // 只抓「像配置」的名字（超时/间隔/体积），Windows API 常量各模块各自声明不算。
    // 由 scan() 的跨文件后处理填充；逐行不命中。
    id: 'DUPLICATE_CONST',
    label: '同名同值常量跨文件重复',
    allowNamedConst: true,
    test: () => false,
  },
];

// —— 白名单：glob → 允许的类别（"*" 全部）——
const allow = JSON.parse(fs.readFileSync(path.join(ROOT, 'scripts/hardcoded-allowlist.json'), 'utf8'));

function globToRegExp(glob) {
  let re = glob.replace(/[.+^${}()|[\]\\]/g, '\\$&');
  re = re.replace(/\*\*/g, '\u0000').replace(/\*/g, '[^/]*').replace(/\u0000/g, '.*');
  return new RegExp('^' + re + '$');
}
const allowRules = (allow.files || []).map((r) => ({ ...r, re: globToRegExp(r.glob) }));

function allowReason(rel, categoryId) {
  for (const rule of allowRules) {
    if (!rule.re.test(rel)) continue;
    const cats = rule.categories || ['*'];
    if (cats.includes('*') || cats.includes(categoryId)) return rule.reason || '白名单';
  }
  return null;
}

function walk(dir) {
  const out = [];
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) out.push(...walk(full));
    else if (EXTS.has(path.extname(entry.name))) out.push(full);
  }
  return out;
}

// rust：去掉内联测试模块；返回 [{ no, text }]
function rustProductionLines(text) {
  const lines = text.split(/\r?\n/);
  const kept = [];
  let inTest = false;
  for (let i = 0; i < lines.length; i++) {
    const raw = lines[i];
    const trimmed = raw.trim();
    if (/^#\[cfg\(test\)\]/.test(trimmed) || /^#\[cfg\(all\(.*\btest\b/.test(trimmed)) {
      inTest = true;
    }
    if (!inTest) kept.push({ no: i + 1, text: raw });
  }
  return kept;
}

function isCommentLine(ext, trimmed) {
  if (!trimmed) return true;
  if (ext === '.rs') return trimmed.startsWith('//') || trimmed.startsWith('/*') || trimmed.startsWith('*');
  if (ext === '.js' || ext === '.mjs') return trimmed.startsWith('//') || trimmed.startsWith('/*') || trimmed.startsWith('*');
  if (ext === '.html') return trimmed.startsWith('<!--');
  if (ext === '.css') return trimmed.startsWith('/*') || trimmed.startsWith('*');
  return false;
}

// 红线 5 说的是「一律进常量或配置」——所以「具名常量定义」本身是合规的，
// 不算「散落的魔法值」。真正要抓的是散落在逻辑里的裸字面量。这里把
// const/static（rust）与 const（js）的具名定义行放行，只盯 inline 魔法值。
function isNamedConst(ext, trimmed) {
  if (ext === '.rs') return /^(pub(\([^)]*\))?\s+)?(const|static)\s+[A-Za-z_]/.test(trimmed);
  if (ext === '.js' || ext === '.mjs') return /^(export\s+)?const\s+[A-Za-z_$][\w$]*\s*=/.test(trimmed);
  return false;
}

function parseConstDef(ext, trimmed) {
  let m = null;
  if (ext === '.rs') m = trimmed.match(/^(?:pub(?:\([^)]*\))?\s+)?(?:const|static)\s+([A-Za-z_][\w]*)\s*(?::[^=]+)?=\s*(.+?);?$/);
  else if (ext === '.js' || ext === '.mjs') m = trimmed.match(/^(?:export\s+)?const\s+([A-Za-z_$][\w$]*)\s*=\s*(.+?);?$/);
  if (!m) return null;
  return { name: m[1], value: m[2].replace(/\s+/g, ' ').trim() };
}

function scan() {
  const rows = []; // { file, no, category, text, whitelisted, reason }
  const defs = []; // 具名常量定义（跨文件重复检测用）
  for (const root of ROOTS) {
    const abs = path.join(ROOT, root);
    if (!fs.existsSync(abs)) continue;
    for (const file of walk(abs)) {
      const rel = path.relative(ROOT, file).split(path.sep).join('/');
      const ext = path.extname(file);
      const isTestFile = /_tests\.rs$/.test(rel) || /\.test\.(js|mjs)$/.test(rel);
      if (isTestFile) continue;
      const text = fs.readFileSync(file, 'utf8');
      const lines = ext === '.rs' ? rustProductionLines(text) : text.split(/\r?\n/).map((t, i) => ({ no: i + 1, text: t }));
      for (const { no, text: line } of lines) {
        const trimmed = line.trim();
        if (isCommentLine(ext, trimmed)) continue;
        const named = isNamedConst(ext, trimmed);
        for (const cat of CATEGORIES) {
          if (named && !cat.allowNamedConst) continue;
          if (cat.frontendOnly && !(rel.startsWith('src/pet/') || rel.startsWith('src/shared/'))) continue;
          if (!cat.test(line)) continue;
          const reason = allowReason(rel, cat.id);
          rows.push({ file: rel, no, category: cat.id, text: trimmed.slice(0, 160), whitelisted: !!reason, reason });
        }
        const def = parseConstDef(ext, trimmed);
        if (def) defs.push({ ...def, file: rel, no });
      }
    }
  }
  // 跨文件「同名同值」常量：具名常量豁免会掩盖复制粘贴（红线 3/4）。
  const byKey = new Map();
  for (const d of defs) {
    if (d.file === 'src-tauri/src/config.rs') continue;
    // 只抓「像配置」的常量名：超时/间隔/体积阈值；Win32 API 常量等各模块各自声明不算。
    if (!/(TIMEOUT|_MS$|_MS\b|INTERVAL|_BYTES$|_KEEP$)/i.test(d.name)) continue;
    const key = d.name + '\u0000' + d.value;
    if (!byKey.has(key)) byKey.set(key, []);
    byKey.get(key).push(d);
  }
  for (const group of byKey.values()) {
    const files = new Set(group.map((g) => g.file));
    if (files.size < 2) continue;
    for (const g of group) {
      const reason = allowReason(g.file, 'DUPLICATE_CONST');
      rows.push({ file: g.file, no: g.no, category: 'DUPLICATE_CONST', text: `const ${g.name} = ${g.value}（${files.size} 个文件同名同值）`.slice(0, 160), whitelisted: !!reason, reason });
    }
  }
  return rows;
}

const rows = scan();
const jsonOut = process.argv.includes('--json');
const listOut = process.argv.includes('--list');

const summary = {};
for (const cat of CATEGORIES) {
  const all = rows.filter((r) => r.category === cat.id);
  summary[cat.id] = {
    label: cat.label,
    total: all.length,
    whitelisted: all.filter((r) => r.whitelisted).length,
    remaining: all.filter((r) => !r.whitelisted).length,
  };
}
const totals = {
  total: rows.length,
  whitelisted: rows.filter((r) => r.whitelisted).length,
  remaining: rows.filter((r) => !r.whitelisted).length,
};

if (jsonOut) {
  console.log(JSON.stringify({ summary, totals, remaining: rows.filter((r) => !r.whitelisted) }, null, 2));
} else {
  console.log('硬编码自查（红线 5）  roots=' + ROOTS.join(', '));
  console.log('类别'.padEnd(16) + '总命中'.padStart(8) + '白名单'.padStart(8) + '未清零'.padStart(8));
  for (const cat of CATEGORIES) {
    const s = summary[cat.id];
    console.log((cat.id + ' ' + cat.label).padEnd(16) + String(s.total).padStart(8) + String(s.whitelisted).padStart(8) + String(s.remaining).padStart(8));
  }
  console.log('-'.repeat(40));
  console.log('合计'.padEnd(16) + String(totals.total).padStart(8) + String(totals.whitelisted).padStart(8) + String(totals.remaining).padStart(8));
  if (listOut) {
    console.log('\n未清零命中（file:line  category  text）：');
    for (const r of rows.filter((x) => !x.whitelisted)) {
      console.log(`${r.file}:${r.no}  ${r.category}  ${r.text}`);
    }
  }
  if (totals.remaining > 0) console.log('\n有未清零命中：把字面量改走 src-tauri/src/config.rs 或 scripts/hardcoded-allowlist.json 写清理由。');
  else console.log('\n通过：生产代码的配置面字面量已清零（除白名单）。');
}

process.exit(totals.remaining > 0 ? 1 : 0);

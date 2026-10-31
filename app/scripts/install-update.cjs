// 安装器专用：按摘要归属更新，校验一次后同卷移动，失败恢复旧版本。
// 日志分两路（#16）：安装器用 nsExec::ExecToLog 转发本程序的输出，而 nsExec
// 按 ANSI/OEM 代码页解释这些字节，Node 写的是 UTF-8 —— 中文进「显示详细信息」
// 就是乱码。所以走安装器日志的每一句都只用 ASCII（英文），给用户看细节的中文
// 写进目标目录下的 install.log（UTF-8，带 BOM，记事本直接能读）。
'use strict';
const fs = require('node:fs'), path = require('node:path'), crypto = require('node:crypto');
const MANIFEST = 'install-manifest.json', STATE = 'install-state.json', TX = '.fish-install-transaction', LOG = 'install.log';
// 安装器日志里只出现 ASCII：这段文字经 nsExec 按 OEM 代码页转出来。
const SAY = {
  stop: 'Stopping the running copy of this installation...',
  verify: 'Checking the installed files',
  prepare: 'Preparing the changed files',
  commit: 'Committing the installation',
  done: (replaced, reused, secs) => `Installation complete: ${replaced} replaced, ${reused} reused, ${secs} s`,
  failed: message => 'Installation update failed: ' + message,
  migrate: 'Moving your data to its new home',
  migrated: (files, secs) => `Data moved to its new home: ${files} files verified, ${secs} s`,
  kept: 'Data left where it is: its new home already exists or was not asked for',
  migrateFailed: message => 'Data was NOT moved, and the old directory is untouched: ' + message,
};
/// #27-D：用户数据目录从旧名迁到新名（都在 %LOCALAPPDATA% 下）。
/// 新名字面量必须与 src-tauri/src/runtime.rs 的 app_home() 完全一致，
/// 也和 scripts/installer.mjs 导出的 HOME_DIRECTORY / LEGACY_HOME_DIRECTORY 一致
/// （installer.test.mjs 里有一条断言盯着这三处不许漂移）。
const HOME_FROM = 'DSH-FISH', HOME_TO = 'FISH Dock';
/// #27 P0 后半：比上面那对更早的遗留 —— %LOCALAPPDATA%\FISH\worker.json。
/// 它不是一个产品的数据根，只是旧版本留在那里的一个文件：新数据根就绪之后才删，
/// 删文件本身，`FISH` 目录空了才顺手收掉（里面还有别人的东西就原样留着）。
const STALE_DIR = 'FISH', STALE_FILE = 'worker.json';
/// 显式指定过数据目录时（用户自己选的家），安装器一律不碰任何目录。
/// 只认 2.0.0 的覆盖名：宿主已经不再读 DSH_FISH_RUNTIME_HOME，机器上残留那个
/// 变量的用户仍然是「用默认新根」，把它算进来会让 #27-D 静默失效
/// （旧目录既不迁也不删）。installer.test.mjs 有一条断言盯着这份名单。
const HOME_OVERRIDES = ['FISH_DOCK_APP_HOME'];
/// 迁移只在安装器显式要求时发生：默认不动手，跑测试或手工调用都不会迁。
const MIGRATE_FLAG = '--migrate-home', MIGRATE_SKIP = 'FISH_DOCK_SKIP_HOME_MIGRATION';
/// WebView2 的子进程可能还在往旧目录里写，复制期间摘要对不上是常事：
/// 每次失败都把半成品新目录整个删掉、旧目录原样留下，再重试。
const MIGRATE_ATTEMPTS = 3;
// 细节日志：UTF-8 带 BOM，换行用 CRLF，一句一行；写不进去也不影响安装。
// BOM 只写一次：记事本靠它认出 UTF-8，Windows PowerShell 5.1 的 Get-Content 也一样。
function detail(root) {
  let file = null;
  return function write(line) {
    try {
      if (!file) file = path.join(root, LOG);
      const first = !fs.existsSync(file);
      fs.appendFileSync(file, (first ? '\ufeff' : '') + line.replace(/\r?\n/g, ' ') + '\r\n', 'utf8');
    } catch { file = null; }
  };
}
// 出去的字一个都不能不过这里：报错信息里常有中文路径（安装目录可以是中文），
// 在安装日志里只会变成乱码。装不成的那句话只说是什么错、错了哪个文件，中文
// 细节和完整路径都在 install.log 里；ASCII 之外的字符换成问号。
const ascii = text => String(text).replace(/[^\x20-\x7e]+/g, '?');
const digest = bytes => crypto.createHash('sha256').update(bytes).digest('hex');
const canonical = name => process.platform === 'win32' ? path.resolve(name).toLowerCase() : path.resolve(name);
/// 安装根下的主程序名：2.0.0 起是 FISH-Dock.exe（#27 P2），旧发行名留在名单里
/// 是为了读得动旧归属清单（升级时旧清单里写的就是 DSH-FISH.exe / wakuwaku.exe），
/// 也为了把便携版旁边那份旧传感器助手（wakuwaku-sensors.exe）当成受管文件收走：
/// 它不在新清单里，「旧清单有、新清单无」正好删掉，不留两个助手（#t16）。
const EXE_NAMES = ['FISH-Dock.exe', 'DSH-FISH.exe', 'wakuwaku.exe', 'wakuwaku-sensors.exe'];
function relative(name) {
  if (typeof name !== 'string' || name.includes('\\') || name.includes(':') || name.split('/').some(p => !p || p === '.' || p === '..' || /[<>"|?*\x00-\x1f]/.test(p) || /[. ]$/.test(p) || /^(con|prn|aux|nul|com[1-9]|lpt[1-9])(?:\.|$)/i.test(p))) throw Error('安装清单路径无效');
  if (![...EXE_NAMES, 'install-update.cjs', 'install-stop.ps1'].includes(name) && !name.startsWith('resources/')) throw Error('安装清单路径不属于发行资源');
  return name;
}
// 一次安装内父目录只检查一次；缓存不跨安装，文件自身始终重新检查。
function resolver(root, io) {
  const checked = new Set();
  function directory(folder) {
    if (checked.has(folder)) return;
    const parent = path.dirname(folder); if (parent !== folder) directory(parent);
    try { const s = io.lstatSync(folder); if (s.isSymbolicLink() || !s.isDirectory()) throw Error('安装路径包含链接或非目录，已停止更新'); }
    catch (e) { if (e.code !== 'ENOENT') throw e; }
    checked.add(folder);
  }
  directory(root);
  return name => {
    const full = path.join(root, ...name.split('/')); directory(path.dirname(full));
    try { const s = io.lstatSync(full); if (s.isSymbolicLink() || !s.isFile()) throw Error('安装文件路径不是普通文件'); }
    catch (e) { if (e.code !== 'ENOENT') throw e; }
    return full;
  };
}
function stat(io, file) { try { return io.lstatSync(file, { bigint: true }); } catch (e) { if (e.code === 'ENOENT') return null; throw e; } }
const fingerprint = s => [s.dev, s.ino, s.size, s.mtimeNs, s.ctimeNs, s.birthtimeNs].map(String).join(':');
function hashFile(io, file, metrics) {
  const before = stat(io, file); if (!before || !before.isFile() || before.isSymbolicLink()) throw Error('校验文件缺失或类型无效');
  const hash = crypto.createHash('sha256'), buffer = Buffer.allocUnsafe(256 * 1024), fd = io.openSync(file, 'r');
  let bytes = 0;
  try { let n; while ((n = io.readSync(fd, buffer, 0, buffer.length, null)) > 0) { hash.update(buffer.subarray(0, n)); bytes += n; } }
  finally { io.closeSync(fd); }
  const after = stat(io, file);
  if (!after || fingerprint(before) !== fingerprint(after) || BigInt(bytes) !== after.size) throw Error('校验期间文件发生变化，请重新安装');
  if (metrics) { metrics.hashedBytes += bytes; metrics.hashedFiles++; }
  return { bytes, sha256: hash.digest('hex'), stamp: fingerprint(after) };
}
function manifest(root, io = fs, resolve = resolver(root, io)) {
  const file = resolve(MANIFEST); if (!io.existsSync(file)) return null;
  if (io.statSync(file).size > 16 * 1024 * 1024) throw Error('安装归属清单超过限制');
  const raw = io.readFileSync(file), m = JSON.parse(raw.toString('utf8'));
  if (m.schema !== 1 || m.product !== 'FISH Dock' || !Array.isArray(m.files) || m.files.length > 50000) throw Error('安装归属清单无效');
  const seen = new Set();
  for (const f of m.files) { relative(f.path); const key = f.path.toLowerCase(); if (seen.has(key) || !/^[a-f0-9]{64}$/.test(f.sha256) || !Number.isSafeInteger(f.bytes) || f.bytes < 0) throw Error('安装归属清单文件记录无效'); seen.add(key); }
  if (m.payload !== undefined && (!Array.isArray(m.payload) || new Set(m.payload).size !== m.payload.length || m.payload.some(p => !m.files.some(f => f.path === p)))) throw Error('精简更新包载荷清单无效');
  return { ...m, raw, hash: digest(raw) };
}
function recover(root, io = fs) {
  const target = resolver(root, io), dir = path.join(root, TX), log = path.join(dir, 'journal.json');
  if (!io.existsSync(dir)) return;
  const privatePath = resolver(dir, io);
  if (!io.existsSync(privatePath('journal.json'))) throw Error('检测到未完成安装暂存，请保留目录并运行修复安装');
  const journal = JSON.parse(io.readFileSync(log, 'utf8'));
  if (journal.schema !== 1 || !Array.isArray(journal.entries) || journal.entries.length > 50002) throw Error('安装恢复日志无效');
  const seen = new Set();
  for (const e of journal.entries) {
    if (![MANIFEST, STATE].includes(e.path)) relative(e.path);
    if (seen.has(e.path.toLowerCase()) || typeof e.existed !== 'boolean') throw Error('安装恢复日志包含重复或无效记录');
    seen.add(e.path.toLowerCase()); target(e.path); privatePath('old/' + e.path);
  }
  if (!journal.committed && journal.phase !== 'preparing') {
    for (const e of [...journal.entries].reverse()) {
      const dest = target(e.path), backup = privatePath('old/' + e.path);
      if (io.existsSync(backup)) { if (io.existsSync(dest)) io.unlinkSync(dest); io.mkdirSync(path.dirname(dest), { recursive: true }); io.renameSync(backup, dest); }
      else if (!e.existed && io.existsSync(dest)) io.unlinkSync(dest);
    }
  }
  // 仅删除本安装器的已验证事务目录；不遍历用户资料。
  io.rmSync(dir, { recursive: true, force: true });
}
function legacy(root, io, target, metrics) {
  const file = target('resources/manifest.json'); if (!io.existsSync(file)) return [];
  const m = JSON.parse(io.readFileSync(file, 'utf8'));
  if (m.schema !== 2 || !['DSH-FISH', 'FISH Dock'].includes(m.product) || !Array.isArray(m.files)) throw Error('旧运行资源清单无效，请先修复旧安装');
  const rows = m.files.map(f => ({ ...f, path: relative('resources/' + f.path) }));
  for (const f of rows) if (!/^[a-f0-9]{64}$/.test(f.sha256) || !Number.isSafeInteger(f.bytes)) throw Error('旧运行资源清单摘要无效');
  rows.push({ path: 'resources/manifest.json', ...hashFile(io, file, metrics) });
  for (const name of EXE_NAMES) {
    const p = target(name); if (!io.existsSync(p)) continue;
    const fd = io.openSync(p, 'r'), head = Buffer.alloc(2); try { io.readSync(fd, head, 0, 2, 0); } finally { io.closeSync(fd); }
    if (head[0] === 77 && head[1] === 90) rows.push({ path: name, ...hashFile(io, p, metrics) });
  }
  return rows;
}
function loadState(root, old, io, target) {
  if (!old) return null;
  try {
    const file = target(STATE); if (io.statSync(file).size > 16 * 1024 * 1024) return null;
    const record = JSON.parse(io.readFileSync(file, 'utf8'));
    return record.schema === 1 && record.root === canonical(root) && record.manifest === old.hash && record.files && typeof record.files === 'object' ? record : null;
  } catch { return null; }
}
/// 更新之后 `resources/` 下不该留下任何空目录：旧版本的运行时目录、换版后没人用的
/// 子目录、以及只有目录没有文件的层级（NSIS 的 `File /r` 会造出这种空壳，旧清单里
/// 从来没有它们的记录，所以「旧清单有、新清单无」这条规则管不到它们）。
/// 判据只有一条：**空**。这一轮更新只搬清单里的文件，新版本要用的目录里必然有文件，
/// 空着就说明它是上一版留下的壳；`resources/` 顶层本身不删（安装器要它）。
/// 链接一律跳过：跟着链接走会把树外的东西算进来，也可能删掉别处的东西。
/// 目录被占用（句柄、WebView2 子进程）时删不掉，留着即可，下一次更新再收。
function pruneEmpty(root, io) {
  const resources = path.join(root, 'resources');
  let rootMeta; try { rootMeta = io.lstatSync(resources); } catch { return []; }
  if (!rootMeta.isDirectory() || rootMeta.isSymbolicLink()) return [];
  const dirs = [];
  (function walk(dir, depth) {
    for (const entry of io.readdirSync(dir, { withFileTypes: true })) {
      if (!entry.isDirectory() || entry.isSymbolicLink()) continue;
      const full = path.join(dir, entry.name);
      dirs.push({ full, depth: depth + 1, name: path.relative(root, full).split(path.sep).join('/') });
      walk(full, depth + 1);
    }
  })(resources, 0);
  // 最深的先收：父目录只有等子目录删完才可能变空。
  dirs.sort((a, b) => b.depth - a.depth || (a.name < b.name ? -1 : a.name > b.name ? 1 : 0));
  const pruned = [];
  for (const { full, name } of dirs) {
    let meta; try { meta = io.lstatSync(full); } catch { continue; }
    if (!meta.isDirectory() || meta.isSymbolicLink()) continue;
    let entries; try { entries = io.readdirSync(full); } catch { continue; }
    if (entries.length) continue;
    try { io.rmdirSync(full); pruned.push(name); } catch { continue; }
  }
  return pruned;
}
function update(stage, root, io = fs, options = {}) {
  const start = Date.now(), metrics = { hashedBytes: 0, hashedFiles: 0, cachedFiles: 0, movedFiles: 0, copiedFiles: 0 };
  stage = path.resolve(stage); root = path.resolve(root);
  const stageInsideRoot = canonical(stage).startsWith(canonical(root) + path.sep);
  const ownedStage = stageInsideRoot && canonical(path.dirname(stage)) === canonical(root) && path.basename(stage).toLowerCase().startsWith('.fish-dock-stage-');
  if (canonical(stage) === canonical(root) || canonical(root).startsWith(canonical(stage) + path.sep) || (stageInsideRoot && !ownedStage)) throw Error('安装暂存路径必须位于目标目录外，或为该安装器创建的直接隐藏 stage');
  if (ownedStage && !io.existsSync(stage)) throw Error('目标卷隔离暂存目录缺失');
  const source = resolver(stage, io), next = manifest(stage, io, source); if (!next) throw Error('缺少安装归属清单');
  const report = typeof options.report === 'function' ? options.report : () => {};
  // 细节进 install.log；日志里写的是 ASCII 的进度与结果，细节留中文。
  const log = options.log || (() => {});
  let lastPercent = -1, lastReport = 0;
  const progress = (label, done, total, force = false) => {
    const percent = Math.floor(done * 100 / Math.max(1, total));
    if (force || percent >= lastPercent + 10 || Date.now() - lastReport >= 1000) { report(`${label} ${done}/${total} (${percent}%)`); lastPercent = percent; lastReport = Date.now(); }
  };
  io.mkdirSync(root, { recursive: true }); const target = resolver(root, io); recover(root, io);
  const old = manifest(root, io, target), previous = old ? old.files.slice() : legacy(root, io, target, metrics), receipt = loadState(root, old, io, target);
  const names = new Set(next.files.map(f => f.path.toLowerCase()));
  // #t18 / t17 FAIL-3：旧发行名（DSH-FISH.exe / wakuwaku.exe / wakuwaku-sensors.exe）
  // 不能只在「没有 install-manifest.json」时才扫。有清单的安装里，旧 exe 可能只被清单
  // 记了一个、旁边还摆着另几个（1.2.3 的清单只记 DSH-FISH.exe）：以前它们原样留着，
  // 升级后安装目录里同时有几个能启动的主程序。这里把「磁盘上确实存在、且新清单不再
  // 发行」的旧 exe 无条件并进受管清单，后面「旧清单有、新清单无」的规则就会收走它们。
  const known = new Set(previous.map(f => f.path.toLowerCase()));
  for (const name of EXE_NAMES) {
    const key = name.toLowerCase();
    if (names.has(key) || known.has(key)) continue;
    let full; try { full = target(name); } catch { continue; }
    if (stat(io, full)) { previous.push({ path: name }); known.add(key); }
  }
  const prior = new Map(previous.map(f => [f.path.toLowerCase(), f])), good = new Map(), changed = [];
  function matches(file, f, allowCache) {
    const meta = stat(io, file); if (!meta || !meta.isFile() || meta.size !== BigInt(f.bytes)) return false;
    const cached = allowCache && receipt?.files[f.path];
    if (cached && cached.sha256 === f.sha256 && cached.stamp === fingerprint(meta) && prior.get(f.path.toLowerCase())?.sha256 === f.sha256) { metrics.cachedFiles++; return true; }
    return hashFile(io, file, metrics).sha256 === f.sha256;
  }
  progress(SAY.verify, 0, next.files.length, true);
  const payload = next.payload ? new Set(next.payload) : null;
  for (const [index, f] of next.files.entries()) {
    const dest = target(f.path);
    if (matches(dest, f, true)) good.set(f.path, f);
    else {
      if (payload && !payload.has(f.path)) throw Error('精简更新包所需基础资源缺失或损坏，请下载完整安装包：' + f.path);
      const incoming = source(f.path);
      if (!io.existsSync(incoming)) throw Error('发行文件缺失，请重新下载完整安装包：' + f.path);
      if (!matches(incoming, f, false)) throw Error('发行文件校验失败：' + f.path);
      changed.push(f);
    }
    progress(SAY.verify, index + 1, next.files.length);
  }
  // ── 受管旧文件：旧归属清单里有、新清单里没有 → 一律清掉 ──────────────────
  // 这里**不按摘要过滤**。上一版安装中途被打断、或程序自己改写过同名文件时，
  // 现场内容与旧清单记的摘要对不上 —— 那恰恰是最常见的残留来源；按摘要过滤
  // 等于把它们永久留在安装目录里，也就是「旧版本文件越攒越多」。
  // 只有一种情形不动：该路径现在不是普通文件（目录、链接、设备等）—— 那已经
  // 不是我们放下的那个文件，删它就是越界。跳过并记一行，不中断安装。
  const obsolete = previous.filter(f => !names.has(f.path.toLowerCase()));
  const stale = [], occupied = [], presentBefore = new Set();
  for (const f of obsolete) {
    let full;
    try { full = target(f.path); } catch { occupied.push(f); continue; }
    const meta = stat(io, full);
    if (meta && (!meta.isFile() || meta.isSymbolicLink())) { occupied.push(f); continue; }
    if (meta) presentBefore.add(f.path);
    stale.push(f);
  }
  const entries = [...changed, ...stale, { path: MANIFEST }, { path: STATE }].map(f => ({ path: f.path, existed: io.existsSync(target(f.path)) }));
  const dir = path.join(root, TX), staging = path.join(dir, 'new');
  io.mkdirSync(path.join(dir, 'old'), { recursive: true }); io.mkdirSync(staging, { recursive: true });
  const journal = { schema: 1, committed: false, phase: 'preparing', entries };
  const save = () => { io.writeFileSync(path.join(dir, 'journal.json.tmp'), JSON.stringify(journal)); io.renameSync(path.join(dir, 'journal.json.tmp'), path.join(dir, 'journal.json')); };
  const put = e => {
    const dest = target(e.path), backup = path.join(dir, 'old', e.path), replacement = path.join(staging, e.path);
    if (e.existed) { io.mkdirSync(path.dirname(backup), { recursive: true }); io.renameSync(dest, backup); }
    if (io.existsSync(replacement)) { io.mkdirSync(path.dirname(dest), { recursive: true }); io.renameSync(replacement, dest); }
  };
  try {
    save(); lastPercent = -1;
    for (const [index, f] of changed.entries()) {
      const from = source(f.path), to = path.join(staging, f.path); io.mkdirSync(path.dirname(to), { recursive: true });
      // 正在执行的引导 Node 保留在临时目录，其他已校验文件优先同卷移动。
      let copied = canonical(from) === canonical(process.execPath);
      if (!copied) {
        try { io.renameSync(from, to); metrics.movedFiles++; }
        catch (e) { if (!['EXDEV', 'EPERM', 'EACCES', 'EBUSY'].includes(e.code)) throw e; copied = true; }
      }
      if (copied) { io.copyFileSync(from, to); metrics.copiedFiles++; const checked = hashFile(io, to, metrics); if (checked.bytes !== f.bytes || checked.sha256 !== f.sha256) throw Error('安装暂存校验失败：' + f.path); }
      progress(SAY.prepare, index + 1, changed.length);
    }
    io.writeFileSync(path.join(staging, MANIFEST), next.raw);
    journal.phase = 'applying'; save(); lastPercent = -1;
    for (const [index, e] of entries.entries()) {
      if (e.path === STATE) continue;
      put(e); progress(SAY.commit, index + 1, entries.length);
    }
    const files = Object.create(null);
    for (const f of next.files) {
      const meta = stat(io, target(f.path)); if (!meta || meta.size !== BigInt(f.bytes)) throw Error('安装提交后资源缺失：' + f.path);
      files[f.path] = { sha256: f.sha256, stamp: fingerprint(meta) };
    }
    io.writeFileSync(path.join(staging, STATE), JSON.stringify({ schema: 1, root: canonical(root), manifest: next.hash, files }));
    put(entries[entries.length - 1]); journal.committed = true; save();
  } catch (error) {
    try { recover(root, io); } catch (restore) { throw Error(error.message + '；安装恢复尚未完成，请保留 ' + dir + ' 后重试修复：' + restore.message); }
    throw error;
  }
  recover(root, io);
  if (ownedStage && options.cleanupStage !== false) { resolver(stage, io); io.rmSync(stage, { recursive: true, force: true }); }
  // 旧文件已经随事务目录一起清掉，这里只负责「记账」与「收拾空壳」：
  // 删掉的个数按「更新前在、更新后不在」数，不是按计划数；空目录把 resources/ 整棵
  // 走一遍（判据只有「空」，见 pruneEmpty）。
  // 这一步是新版本已经装好之后的收尾：出错只记一行，绝不把装好的版本判成失败。
  let cleaned = 0, pruned = [];
  try {
    cleaned = stale.filter(f => presentBefore.has(f.path) && !io.existsSync(target(f.path))).length;
    pruned = pruneEmpty(root, io);
  } catch { /* 收尾失败不影响已提交的安装，下一次更新会接着收 */ }
  log(`清理 ${cleaned} 个旧文件（旧归属清单里 ${obsolete.length} 个不再随新版本发行${occupied.length ? `，其中 ${occupied.length} 个路径已被目录或链接占据、按无归属内容保留` : ''}）`);
  if (pruned.length) log(`回收 ${pruned.length} 个空目录：${pruned.join('、')}`);
  if (occupied.length) log('这些路径现在不是普通文件，未删除：' + occupied.map(f => f.path).join('、'));
  const result = { version: next.version, replaced: changed.length, removed: cleaned, obsolete: obsolete.length, prunedDirs: pruned.length, occupied: occupied.length, needsWebviewAcl: next.files.some(f => f.path.startsWith('resources/webview2/')) && (!receipt || changed.some(f => f.path.startsWith('resources/webview2/'))), ...metrics, elapsedMs: Date.now() - start };
  report(SAY.done(result.replaced, metrics.cachedFiles, (result.elapsedMs / 1000).toFixed(1)));
  log(`安装完成：替换 ${result.replaced} 个文件，复用 ${metrics.cachedFiles} 个，耗时 ${(result.elapsedMs / 1000).toFixed(1)} 秒`);
  return result;
}
/// 一棵目录树里的全部普通文件：相对路径（用 `/` 分隔）、字节数与 SHA-256，按路径排序。
/// 迁移前后各盘一次，两份清单必须逐条对上，才敢删旧目录。
/// 链接（含 Windows 的 junction）与非普通文件一律拒绝：跟着链接走会把树外的东西也算进来。
function inventoryTree(root, io = fs) {
  const rows = [];
  (function walk(dir) {
    for (const entry of io.readdirSync(dir, { withFileTypes: true })) {
      const full = path.join(dir, entry.name), name = path.relative(root, full).split(path.sep).join('/');
      if (entry.isSymbolicLink()) throw Error('用户数据目录里有链接，已停止迁移：' + name);
      if (entry.isDirectory()) { walk(full); continue; }
      if (!entry.isFile()) throw Error('用户数据目录里有非普通文件，已停止迁移：' + name);
      const { bytes, sha256 } = hashFile(io, full);
      rows.push({ path: name, bytes, sha256 });
    }
  })(root);
  return rows.sort((a, b) => (a.path < b.path ? -1 : a.path > b.path ? 1 : 0));
}
const sameInventory = (a, b) => a.length === b.length && a.every((f, i) => f.path === b[i].path && f.bytes === b[i].bytes && f.sha256 === b[i].sha256);
/// 要迁移的两个目录。返回 null 就是「这次不动任何目录」：
/// 显式指定过数据目录、被开关关掉、或连 %LOCALAPPDATA% 都读不到。
function homePaths(env = process.env) {
  const off = String(env[MIGRATE_SKIP] ?? '').trim().toLowerCase();
  if (off && off !== '0' && off !== 'false') return null;
  if (HOME_OVERRIDES.some(name => env[name] && String(env[name]).length)) return null;
  const local = env.LOCALAPPDATA;
  if (!local) return null;
  const from = path.join(local, HOME_FROM), to = path.join(local, HOME_TO);
  if (canonical(from) === canonical(to)) return null;
  return { from, to };
}
/// #27-D：迁移完成后直接删掉旧目录，不留回滚副本（任务清单「遗留目录迁完直接删」）。
/// 顺序是「整树盘点 → 逐个复制 → 逐个核对摘要 → 两份清单对齐 → 才删旧目录」。
/// 任何一步不对：把刚建出来的新目录整个删掉、旧目录一个字节都不动，然后报错。
/// 删旧目录本身失败不算迁移失败（数据已经在新目录里核对过了），只记一行、返回 removed:false。
function migrateHome(from, to, io = fs, options = {}) {
  const report = typeof options.report === 'function' ? options.report : () => {};
  const log = options.log || (() => {});
  const attempts = Math.max(1, options.attempts ?? MIGRATE_ATTEMPTS);
  // 只迁这一对目录名，且必须在同一个父目录下：挡住把别的路径喂进来的调用。
  if (path.basename(from) !== HOME_FROM || path.basename(to) !== HOME_TO) throw Error('用户数据迁移的目录名不对：' + from + ' → ' + to);
  if (canonical(path.dirname(from)) !== canonical(path.dirname(to))) throw Error('用户数据迁移要求两个目录在同一个父目录下');
  if (!io.existsSync(from)) return { migrated: false, removed: false, reason: 'absent', files: 0 };
  if (io.existsSync(to)) { report(SAY.kept); return { migrated: false, removed: false, reason: 'target-exists', files: 0 }; }
  const start = Date.now();
  report(SAY.migrate);
  let last = null;
  for (let attempt = 1; attempt <= attempts; attempt++) {
    try {
      const before = inventoryTree(from, io);
      io.mkdirSync(to, { recursive: true });
      try {
        for (const f of before) {
          const parts = f.path.split('/'), source = path.join(from, ...parts), dest = path.join(to, ...parts);
          io.mkdirSync(path.dirname(dest), { recursive: true });
          io.copyFileSync(source, dest);
          const { bytes, sha256 } = hashFile(io, dest);
          if (bytes !== f.bytes || sha256 !== f.sha256) throw Error('用户数据迁移后摘要不一致：' + f.path);
        }
        if (!sameInventory(before, inventoryTree(to, io))) throw Error('用户数据迁移后的清单与旧目录不一致');
      } catch (error) {
        // 半成品新目录整个删掉：留着它，下一次就会被「新目录已存在」挡住，永远迁不完。
        try { io.rmSync(to, { recursive: true, force: true }); } catch { /* 旧目录完好，这一份残骸由下一次重试或用户手动清 */ }
        throw error;
      }
      let removed = false;
      try { io.rmSync(from, { recursive: true, force: true }); removed = !io.existsSync(from); }
      catch (error) { log('旧用户数据目录删除失败（数据已迁到新目录并核对过，可稍后手动删除）：' + error.message); }
      if (!removed) log('旧用户数据目录仍在（可能有文件被占用）：' + from);
      report(SAY.migrated(before.length, ((Date.now() - start) / 1000).toFixed(1)));
      return { migrated: true, removed, reason: removed ? 'removed' : 'delete-failed', files: before.length, from, to, attempts: attempt };
    } catch (error) {
      last = error;
      log(`用户数据迁移第 ${attempt}/${attempts} 次未成功：${error.message}`);
    }
  }
  report(SAY.migrateFailed(last.message));
  throw last;
}
/// #27 P0 后半：%LOCALAPPDATA%\FISH\worker.json 这个更早的遗留。
/// 前提是「新数据根已就绪」—— %LOCALAPPDATA%\FISH Dock 真的在，才认为迁移或
/// 落位已经完成，删这个旧文件；不在就一个字节都不碰。三闸照旧：显式指定过数据
/// 目录、被 FISH_DOCK_SKIP_HOME_MIGRATION 关掉、或读不到 LOCALAPPDATA 时不动。
/// 只删这一个文件；`FISH` 目录空了才顺手收，里面还有别人的东西就留着。
function staleWorker(env = process.env, io = fs, options = {}) {
  const log = typeof options.log === 'function' ? options.log : () => {};
  const home = homePaths(env);
  if (!home) return { removed: false, reason: 'skipped' };
  if (!io.existsSync(home.to)) return { removed: false, reason: 'no-new-root' };
  const folder = path.join(path.dirname(home.to), STALE_DIR), file = path.join(folder, STALE_FILE);
  if (!io.existsSync(file)) return { removed: false, reason: 'absent' };
  try {
    io.rmSync(file, { force: true });
    try { if (io.readdirSync(folder).length === 0) io.rmdirSync(folder); } catch { /* 目录还在用就留着 */ }
    return { removed: true, reason: 'removed', file };
  } catch (error) {
    log('旧的 %LOCALAPPDATA%\\' + STALE_DIR + '\\' + STALE_FILE + ' 删除失败（可稍后手动删除）：' + error.message);
    return { removed: false, reason: 'delete-failed' };
  }
}
module.exports = { update, recover, migrateHome, homePaths, inventoryTree, staleWorker, HOME_FROM, HOME_TO, HOME_OVERRIDES, MIGRATE_FLAG, MIGRATE_SKIP, MIGRATE_ATTEMPTS, STALE_DIR, STALE_FILE };
if (require.main === module) {
  // 安装器日志（nsExec 转出去的那一屏）只用 ASCII；细节写进 install.log。
  // 出口都过 ascii()：连报错里带的中文路径也不会漏到那一屏上。
  let log = () => {};
  try {
    if (!process.argv[2] || !process.argv[3]) throw Error('缺少安装暂存和目标路径');
    const stage = path.resolve(process.argv[2]), root = path.resolve(process.argv[3]);
    fs.mkdirSync(root, { recursive: true }); process.chdir(root);
    // 每次安装一份新的详情日志：上次留下的不该和这一次混在一起。
    try { fs.rmSync(path.join(root, LOG), { force: true }); } catch {}
    log = detail(root);
    log(`FISH Dock 安装详情 ${new Date().toISOString()}（安装暂存：${stage}，安装目录：${root}）`);
    const stop = path.join(stage, 'install-stop.ps1');
    if (process.platform === 'win32' && fs.existsSync(stop)) {
      console.log(ascii(SAY.stop));
      log('正在结束本安装的旧进程…');
      require('node:child_process').execFileSync(path.join(process.env.SystemRoot || 'C:\\Windows', 'System32/WindowsPowerShell/v1.0/powershell.exe'), ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', stop, '-InstallRoot', root], { stdio: 'inherit', windowsHide: true });
    }
    // 安装器那一屏只有这一个出口：进度、迁移与结果都从这里出去，全都过 ascii()。
    const report = text => console.log(ascii(text));
    const result = update(stage, root, fs, { report, log, cleanupStage: false });
    if (result.needsWebviewAcl) fs.writeFileSync(path.join(stage, 'webview-acl-needed'), '1');
    // #27-D：文件事务提交之后才迁移用户数据目录，且只有安装器显式传了 --migrate-home 才动手。
    // 迁移失败不回滚已经装好的程序：旧目录原样保留，落一个标记文件让安装器把话说清楚，
    // 退出码仍是 0（重跑一次安装程序就能补上）。
    if (process.argv.includes(MIGRATE_FLAG)) {
      const home = homePaths();
      if (!home) {
        result.home = { migrated: false, removed: false, reason: 'skipped', files: 0 };
        log('未迁移用户数据目录：显式指定了数据目录、被 FISH_DOCK_SKIP_HOME_MIGRATION 关掉，或读不到 LOCALAPPDATA。');
      } else {
        try {
          result.home = migrateHome(home.from, home.to, fs, { report, log });
          log(`用户数据目录迁移完成：${home.from} → ${home.to}（${result.home.files} 个文件已核对，旧目录${result.home.removed ? '已删除' : '仍在，可稍后手动删除'}）`);
        } catch (e) {
          result.home = { migrated: false, removed: false, reason: 'failed', files: 0, from: home.from, to: home.to };
          log('用户数据目录迁移未完成，旧目录已原样保留：' + e.message);
          try { fs.writeFileSync(path.join(stage, 'home-migration-failed'), '1'); } catch {}
        }
      }
      // 新数据根就绪之后，收掉更早遗留的 %LOCALAPPDATA%\FISH\worker.json（#27 P0）。
      result.staleWorker = staleWorker(process.env, fs, { log });
      if (result.staleWorker.removed) log('已删除更早遗留的 %LOCALAPPDATA%\\FISH\\worker.json');
    }
    console.log(ascii(JSON.stringify(result)));
  } catch (e) {
    console.error(ascii(SAY.failed(e.message)));
    log('安装更新失败：' + e.message);
    process.exitCode = 1;
  }
}

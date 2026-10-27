// 安装器专用：按摘要归属更新，校验一次后同卷移动，失败恢复旧版本。
'use strict';
const fs = require('node:fs'), path = require('node:path'), crypto = require('node:crypto');
const MANIFEST = 'install-manifest.json', STATE = 'install-state.json', TX = '.fish-install-transaction';
const digest = bytes => crypto.createHash('sha256').update(bytes).digest('hex');
const canonical = name => process.platform === 'win32' ? path.resolve(name).toLowerCase() : path.resolve(name);
function relative(name) {
  if (typeof name !== 'string' || name.includes('\\') || name.includes(':') || name.split('/').some(p => !p || p === '.' || p === '..' || /[<>"|?*\x00-\x1f]/.test(p) || /[. ]$/.test(p) || /^(con|prn|aux|nul|com[1-9]|lpt[1-9])(?:\.|$)/i.test(p))) throw Error('安装清单路径无效');
  if (!['DSH-FISH.exe', 'wakuwaku.exe', 'install-update.cjs', 'install-stop.ps1'].includes(name) && !name.startsWith('resources/')) throw Error('安装清单路径不属于发行资源');
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
  for (const name of ['DSH-FISH.exe', 'wakuwaku.exe']) {
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
function update(stage, root, io = fs, options = {}) {
  const start = Date.now(), metrics = { hashedBytes: 0, hashedFiles: 0, cachedFiles: 0, movedFiles: 0, copiedFiles: 0 };
  stage = path.resolve(stage); root = path.resolve(root);
  const stageInsideRoot = canonical(stage).startsWith(canonical(root) + path.sep);
  const ownedStage = stageInsideRoot && canonical(path.dirname(stage)) === canonical(root) && path.basename(stage).toLowerCase().startsWith('.fish-dock-stage-');
  if (canonical(stage) === canonical(root) || canonical(root).startsWith(canonical(stage) + path.sep) || (stageInsideRoot && !ownedStage)) throw Error('安装暂存路径必须位于目标目录外，或为该安装器创建的直接隐藏 stage');
  if (ownedStage && !io.existsSync(stage)) throw Error('目标卷隔离暂存目录缺失');
  const source = resolver(stage, io), next = manifest(stage, io, source); if (!next) throw Error('缺少安装归属清单');
  const report = typeof options.report === 'function' ? options.report : () => {};
  let lastPercent = -1, lastReport = 0;
  const progress = (label, done, total, force = false) => {
    const percent = Math.floor(done * 100 / Math.max(1, total));
    if (force || percent >= lastPercent + 10 || Date.now() - lastReport >= 1000) { report(`${label} ${done}/${total} (${percent}%)`); lastPercent = percent; lastReport = Date.now(); }
  };
  io.mkdirSync(root, { recursive: true }); const target = resolver(root, io); recover(root, io);
  const old = manifest(root, io, target), previous = old ? old.files : legacy(root, io, target, metrics), receipt = loadState(root, old, io, target);
  const prior = new Map(previous.map(f => [f.path.toLowerCase(), f])), good = new Map(), changed = [], names = new Set(next.files.map(f => f.path.toLowerCase()));
  function matches(file, f, allowCache) {
    const meta = stat(io, file); if (!meta || !meta.isFile() || meta.size !== BigInt(f.bytes)) return false;
    const cached = allowCache && receipt?.files[f.path];
    if (cached && cached.sha256 === f.sha256 && cached.stamp === fingerprint(meta) && prior.get(f.path.toLowerCase())?.sha256 === f.sha256) { metrics.cachedFiles++; return true; }
    return hashFile(io, file, metrics).sha256 === f.sha256;
  }
  progress('校验安装资源', 0, next.files.length, true);
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
    progress('校验安装资源', index + 1, next.files.length);
  }
  const obsolete = previous.filter(f => !names.has(f.path.toLowerCase())).filter(f => matches(target(f.path), f, true));
  const entries = [...changed, ...obsolete, { path: MANIFEST }, { path: STATE }].map(f => ({ path: f.path, existed: io.existsSync(target(f.path)) }));
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
      progress('准备变更文件', index + 1, changed.length);
    }
    io.writeFileSync(path.join(staging, MANIFEST), next.raw);
    journal.phase = 'applying'; save(); lastPercent = -1;
    for (const [index, e] of entries.entries()) {
      if (e.path === STATE) continue;
      put(e); progress('提交安装文件', index + 1, entries.length);
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
  const result = { version: next.version, replaced: changed.length, removed: obsolete.length, needsWebviewAcl: next.files.some(f => f.path.startsWith('resources/webview2/')) && (!receipt || changed.some(f => f.path.startsWith('resources/webview2/'))), ...metrics, elapsedMs: Date.now() - start };
  report(`安装完成：替换 ${result.replaced}，复用 ${metrics.cachedFiles}，耗时 ${(result.elapsedMs / 1000).toFixed(1)} 秒`);
  return result;
}
module.exports = { update, recover };
if (require.main === module) {
  try {
    if (!process.argv[2] || !process.argv[3]) throw Error('缺少安装暂存和目标路径');
    const stage = path.resolve(process.argv[2]), root = path.resolve(process.argv[3]);
    fs.mkdirSync(root, { recursive: true }); process.chdir(root);
    const stop = path.join(stage, 'install-stop.ps1');
    if (process.platform === 'win32' && fs.existsSync(stop)) {
      console.log('正在结束本安装的旧进程…');
      require('node:child_process').execFileSync(path.join(process.env.SystemRoot || 'C:\\Windows', 'System32/WindowsPowerShell/v1.0/powershell.exe'), ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', stop, '-InstallRoot', root], { stdio: 'inherit', windowsHide: true });
    }
    const result = update(stage, root, fs, { report: console.log, cleanupStage: false });
    if (result.needsWebviewAcl) fs.writeFileSync(path.join(stage, 'webview-acl-needed'), '1');
    console.log(JSON.stringify(result));
  } catch (e) { console.error('安装更新失败：' + e.message); process.exitCode = 1; }
}

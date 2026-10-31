import assert from 'node:assert/strict'
import {test} from 'node:test'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import {createHash} from 'node:crypto'
import {createRequire} from 'node:module'
import {spawnSync} from 'node:child_process'
import {fileURLToPath} from 'node:url'
const require=createRequire(import.meta.url)
const load=()=>require('./install-update.cjs')
const sha=b=>createHash('sha256').update(b).digest('hex')
function fixture(){const base=fs.mkdtempSync(path.join(os.tmpdir(),'fish-update-中文 ')),root=path.join(base,'安装 目录'),stage=path.join(base,'stage');fs.mkdirSync(root);fs.mkdirSync(stage);return {base,root,stage}}
function put(root,name,data){fs.mkdirSync(path.dirname(path.join(root,name)),{recursive:true});fs.writeFileSync(path.join(root,name),data)}
function release(stage,files,version='1.2.0'){for(const [p,b]of Object.entries(files))put(stage,p,b);const owned=Object.entries(files).map(([p,b])=>({path:p,bytes:Buffer.byteLength(b),sha256:sha(b)}));put(stage,'install-manifest.json',JSON.stringify({schema:1,product:'FISH Dock',version,files:owned}));}
test('覆盖更新仅清理受管旧文件，保留无归属文件与用户插件资料',()=>{const f=fixture();try{release(f.stage,{'DSH-FISH.exe':'new','resources/node/node.exe':'node24'});put(f.root,'DSH-FISH.exe','old');put(f.root,'resources/obsolete.js','oldjs');put(f.root,'resources/user.txt','mine');put(f.root,'data/plugins/config.json','secret');put(f.root,'install-manifest.json',JSON.stringify({schema:1,product:'FISH Dock',version:'1.1.0',files:[{path:'DSH-FISH.exe',sha256:sha('old'),bytes:3},{path:'resources/obsolete.js',sha256:sha('oldjs'),bytes:5}]}));load().update(f.stage,f.root);assert.equal(fs.readFileSync(path.join(f.root,'DSH-FISH.exe'),'utf8'),'new');assert.equal(fs.existsSync(path.join(f.root,'resources/obsolete.js')),false);assert.equal(fs.readFileSync(path.join(f.root,'resources/user.txt'),'utf8'),'mine');assert.equal(fs.readFileSync(path.join(f.root,'data/plugins/config.json'),'utf8'),'secret');assert.equal(JSON.parse(fs.readFileSync(path.join(f.root,'install-manifest.json'))).version,'1.2.0');load().update(f.stage,f.root)}finally{fs.rmSync(f.base,{recursive:true,force:true})}})
test('摘要错误或越界归属清单在更新前停止，不触碰旧版本',()=>{const f=fixture();try{release(f.stage,{'DSH-FISH.exe':'new'});put(f.root,'DSH-FISH.exe','old');put(f.stage,'DSH-FISH.exe','bad');assert.throws(()=>load().update(f.stage,f.root),/校验/);assert.equal(fs.readFileSync(path.join(f.root,'DSH-FISH.exe'),'utf8'),'old');release(f.stage,{'../escape':'x'});assert.throws(()=>load().update(f.stage,f.root),/路径/)}finally{fs.rmSync(f.base,{recursive:true,force:true})}})
test('更新中途文件替换失败会恢复旧文件和归属清单',()=>{const f=fixture();try{release(f.stage,{'DSH-FISH.exe':'new','resources/new.txt':'new'});put(f.root,'DSH-FISH.exe','old');const io={...fs,renameSync(from,to){if(to===path.join(f.root,'resources/new.txt'))throw Error('模拟文件占用');return fs.renameSync(from,to)}};assert.throws(()=>load().update(f.stage,f.root,io),/模拟文件占用/);assert.equal(fs.readFileSync(path.join(f.root,'DSH-FISH.exe'),'utf8'),'old');assert.equal(fs.existsSync(path.join(f.root,'resources/new.txt')),false);assert.equal(fs.existsSync(path.join(f.root,'install-manifest.json')),false)}finally{fs.rmSync(f.base,{recursive:true,force:true})}})

// 安装日志乱码（#16）：安装器用 nsExec::ExecToLog 转发这个脚本的输出，nsExec 按
// ANSI/OEM 代码页解释字节，Node 写的是 UTF-8 —— 中文就成了乱码。所以走安装器
// 那一屏的输出只用 ASCII；细节另写 install.log（UTF-8 带 BOM）。这里把两者的
// 字节都取出来看：进程的输出重定向到文件（沙箱里管道不可用），像安装器那样。
test('命令行输出走安装器日志时是纯 ASCII，中文细节写进带 BOM 的 install.log', () => {
  const f = fixture()
  const out = path.join(f.base, 'stdio.txt'), err = path.join(f.base, 'stderr.txt')
  try {
    release(f.stage, { 'DSH-FISH.exe': 'new', 'resources/runtime.bin': 'runtime' })
    const script = path.join(path.dirname(fileURLToPath(import.meta.url)), 'install-update.cjs')
    const ofd = fs.openSync(out, 'w'), efd = fs.openSync(err, 'w')
    let status
    try {
      status = spawnSync(process.execPath, [script, f.stage, f.root], { stdio: ['ignore', ofd, efd], windowsHide: true, timeout: 60000 }).status
    } finally { fs.closeSync(ofd); fs.closeSync(efd) }
    assert.equal(status, 0)
    const shown = fs.readFileSync(out)
    const text = shown.toString('utf8')
    assert.ok(text.includes('Installation complete'), `安装器那一屏该说结果：${text}`)
    for (const byte of shown) assert.ok(byte < 0x80, '安装器日志的字节只能是 ASCII（nsExec 按 OEM 代码页解释）')
    assert.equal(fs.readFileSync(err).length, 0, '成功安装不该往 stderr 写东西')
    assert.match(text.trimEnd(), /\{"version":"[^"]+","replaced":\d+.*\}$/, '最后一行仍是给安装器读的结果 JSON')
    const log = fs.readFileSync(path.join(f.root, 'install.log'))
    assert.deepEqual([...log.subarray(0, 3)], [0xef, 0xbb, 0xbf], 'install.log 要带 BOM，记事本才认得出 UTF-8')
    const detail = log.subarray(3).toString('utf8')
    assert.ok(detail.includes('安装完成'), `细节里该有用中文写的这一句：${detail}`)
    assert.match(detail, /\r\n/, '一句一行，记事本能正常换行')
  } finally { fs.rmSync(f.base, { recursive: true, force: true }) }
})

// 装不成的时候最会把中文漏出去：报错里带着安装目录的路径，而中文目录是常事
// （"安装 目录" 就是这个用例的根）。安装器那一屏只看得到问号，真正的原因在
// install.log 里原样留着。
test('装不成的报错也不把中文漏到安装器日志：路径换成问号，细节留在 install.log', () => {
  const f = fixture()
  const out = path.join(f.base, 'stdio.txt'), err = path.join(f.base, 'stderr.txt')
  try {
    // 暂存里没有归属清单：第一句就停。
    put(f.stage, 'DSH-FISH.exe', 'new')
    const script = path.join(path.dirname(fileURLToPath(import.meta.url)), 'install-update.cjs')
    const ofd = fs.openSync(out, 'w'), efd = fs.openSync(err, 'w')
    let status
    try {
      status = spawnSync(process.execPath, [script, f.stage, f.root], { stdio: ['ignore', ofd, efd], windowsHide: true, timeout: 60000 }).status
    } finally { fs.closeSync(ofd); fs.closeSync(efd) }
    assert.equal(status, 1, '装不成要给安装器一个失败码')
    const said = fs.readFileSync(err)
    for (const byte of said) assert.ok(byte < 0x80, 'stderr 也只能是 ASCII')
    assert.match(said.toString('utf8'), /^Installation update failed: /)
    for (const byte of fs.readFileSync(out)) assert.ok(byte < 0x80, 'stdout 也只能是 ASCII')
    const detail = fs.readFileSync(path.join(f.root, 'install.log')).subarray(3).toString('utf8')
    assert.match(detail, /安装更新失败：.*归属清单/, `install.log 里该有中文原因：${detail}`)
  } finally { fs.rmSync(f.base, { recursive: true, force: true }) }
})

// ── #t7 升级清理彻底性 ────────────────────────────────────────────────────
// 用户原话：「update 做的仍然不够好，不会删除彻底旧版本文件」。以前只按「旧清单有、
// 新清单无、而且现场摘要还对得上」删，于是上一版中途被打断、或程序自己改写过同名
// 文件时，那些文件就永久留在安装目录里，一次一次攒下去。这里把「该删什么」定成可
// 证明的规则，用一份真实的目录树前后对照来钉住：
//   受管旧文件（旧 `install-manifest.json` 里有、新清单里没有）→ 一个不剩；
//   无归属文件（用户自己放进安装目录的）→ 一个不少；
//   用户数据（`%LOCALAPPDATA%\FISH Dock`、`.fish-dock`）→ 更新器连读都不读。
/// 一棵目录树里的全部普通文件（`/` 分隔，相对安装根）。
function treeFiles(directory) {
  const rows = []
  const walk = dir => {
    for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
      const full = path.join(dir, entry.name)
      if (entry.isDirectory()) { walk(full); continue }
      rows.push(path.relative(directory, full).split(path.sep).join('/'))
    }
  }
  walk(directory)
  return rows.sort()
}
/// 指定若干个发行文件在安装根下的字节总数（不存在算 0）：体积对照只看这些，
/// 不把 install-manifest.json / install-state.json 两份账本算进来。
function releaseBytes(root, names) {
  return names.reduce((sum, name) => {
    const file = path.join(root, ...name.split('/'))
    return sum + (fs.existsSync(file) ? fs.statSync(file).size : 0)
  }, 0)
}
test('升级清理彻底性：旧清单 20 个 → 新清单 12 个，更新后受管文件恰好 12 个、旧的 8 个不剩、用户文件还在', () => {
  const f = fixture()
  try {
    // 12 个两版都有的文件：路径留着，内容被新版本替换（含主程序、Node、WebView2、Windhawk）。
    const shared = {
      'DSH-FISH.exe': 'old-exe',
      'resources/manifest.json': '{"schema":2,"product":"DSH-FISH","files":[]}',
      'resources/node/node.exe': 'node-20',
      'resources/node/LICENSE': 'node-license-20',
      'resources/webview2/msedgewebview2.exe': 'webview2-20',
      'resources/windhawk/windhawk.exe': 'windhawk-20',
      'resources/windhawk/Engine/64/windhawk.dll': 'windhawk-engine-20',
      'resources/licenses/Wakuwaku.txt': 'license-20',
      'resources/locale/zh-CN.pak': 'locale-20',
      'resources/theme/dark.css': 'theme-20',
      'resources/app/dispatcher.js': 'dispatcher-20',
      'resources/plugin-host.js': 'plugin-host-20',
    }
    // 8 个只有旧版本才有的受管文件：必须全部消失。
    const obsolete = {
      'resources/runtime-1.9/runtime.bin': 'runtime-19',
      'resources/runtime-1.9/deep/leaf.bin': 'leaf-19',
      'resources/node/node_modules/npm/bin/npm-cli.js': 'npm-19',
      'resources/windhawk/Compiler/bin/clang++.exe': 'clang-19',
      'resources/licenses/old-only-notice.txt': 'notice-19',
      'resources/webview2/EBWebView/old.dll': 'webview2-19',
      'resources/theme/light-old.css': 'theme-light-19',
      'resources/onedir-only/placeholder.txt': 'placeholder-19',
    }
    assert.equal(Object.keys(shared).length + Object.keys(obsolete).length, 20, '旧清单要有 20 条')
    for (const [name, data] of Object.entries({ ...shared, ...obsolete })) put(f.root, name, data)
    // 旧清单记的摘要与现场对不上：上一版更新在写这个文件时被打断，是残留的真实现场。
    // 以前正是靠摘要过滤把它判成「不是我放的」而永久留下。
    const drifted = { path: 'resources/windhawk/Compiler/bin/clang++.exe', bytes: 8, sha256: sha('clang-19') }
    put(f.root, drifted.path, 'clang-19-interrupted-rewrite')
    put(f.root, 'install-manifest.json', JSON.stringify({
      schema: 1, product: 'FISH Dock', version: '1.2.3',
      files: [...Object.entries({ ...shared, ...obsolete }).map(([p, b]) => ({ path: p, bytes: Buffer.byteLength(b), sha256: sha(b) }))
        .filter(row => row.path !== drifted.path), drifted],
    }))
    // 无归属文件：用户自己放进安装目录的，一个都不能少。
    put(f.root, 'resources/user-notes.txt', 'mine')
    // 只有目录、没有文件的空壳：NSIS 的 `File /r` 会造出这种层级（真实的 Node 发行包
    // 里就有），旧清单从来没有它们的记录，「旧清单有、新清单无」这条规则管不到，
    // 所以另外钉一条：更新后 resources/ 下不许有空目录。
    fs.mkdirSync(path.join(f.root, 'resources/node/tap-snapshots'), { recursive: true })
    // 用户数据：不在安装目录里，更新器连读都不读。
    put(f.base, 'FISH Dock/settings.json', '{"dock":"user-settings"}')
    put(f.base, '.fish-dock/state.json', '{"state":"user"}')
    const userDataBefore = { 'FISH Dock/settings.json': '{"dock":"user-settings"}', '.fish-dock/state.json': '{"state":"user"}' }

    // 新包：12 条，路径与 shared 一一对应，内容都是新的。
    const next = Object.fromEntries(Object.entries(shared).map(([name, data]) => [name, data.replace(/-20$/, '-22')]))
    release(f.stage, next, '2.0.0')
    assert.equal(Object.keys(next).length, 12, '新清单要有 12 条')

    // 前后对照要用的两组名字与体积：更新之前先记下来（更新之后旧文件已经没了）。
    const releaseNames = Object.keys(next), staleNames = Object.keys(obsolete)
    const beforeReleaseBytes = releaseBytes(f.root, [...releaseNames, ...staleNames])
    const before = treeFiles(f.root)
    const log = []
    const result = load().update(f.stage, f.root, fs, { log: line => log.push(line) })
    const after = treeFiles(f.root)
    const afterReleaseBytes = releaseBytes(f.root, releaseNames)

    // ① 受管文件：恰好 12 个，而且就是新清单那 12 个。
    const managed = new Set([...Object.keys(shared), ...Object.keys(obsolete), 'install-manifest.json', 'install-state.json'])
    const stillManaged = after.filter(name => managed.has(name))
    const expected = [...new Set(['install-manifest.json', 'install-state.json', ...Object.keys(next)])]
    assert.deepEqual(stillManaged, [...expected].sort(), `更新后目录内的受管文件必须恰好是新清单那 12 个：${JSON.stringify(stillManaged)}`)
    assert.equal(stillManaged.filter(name => Object.keys(next).includes(name)).length, 12, '受管发行文件恰好 12 个')
    // ② 旧清单有、新清单无的 8 个：一个都不剩。
    assert.deepEqual(Object.keys(obsolete).filter(name => after.includes(name)), [], '旧的 8 个受管文件必须全部删除')
    assert.equal(result.removed, 8, '结果里记的清理数就是这 8 个')
    assert.equal(result.obsolete, 8)
    // ③ 无归属文件必须原样保留。
    assert.equal(fs.readFileSync(path.join(f.root, 'resources/user-notes.txt'), 'utf8'), 'mine')
    // ④ 用户数据一个字节都不动。
    for (const [name, text] of Object.entries(userDataBefore)) assert.equal(fs.readFileSync(path.join(f.base, ...name.split('/')), 'utf8'), text)
    // ⑤ 旧文件清掉后空出来的目录（旧版本目录、不再使用的 resources 子目录）一并回收。
    for (const dir of ['resources/runtime-1.9', 'resources/node/node_modules', 'resources/windhawk/Compiler', 'resources/webview2/EBWebView', 'resources/onedir-only', 'resources/node/tap-snapshots']) {
      assert.equal(fs.existsSync(path.join(f.root, ...dir.split('/'))), false, `空目录要顺带清掉：${dir}`)
    }
    const emptyDirs = []
    const walkDirs = dir => {
      for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
        if (!entry.isDirectory()) continue
        const full = path.join(dir, entry.name)
        if (fs.readdirSync(full).length === 0) emptyDirs.push(path.relative(f.root, full).split(path.sep).join('/'))
        walkDirs(full)
      }
    }
    walkDirs(f.root)
    assert.deepEqual(emptyDirs, [], '安装目录里不该留下任何空目录（旧版本目录、没人用的 resources 子目录）')
    // 还要留着的那几层不能被一并收走：新清单里还有文件落在这里。
    for (const dir of ['resources', 'resources/node', 'resources/windhawk/Engine/64', 'resources/licenses', 'resources/theme']) {
      assert.equal(fs.existsSync(path.join(f.root, ...dir.split('/'))), true, `新布局要用的目录不能删：${dir}`)
    }
    // ⑥ 记数进 install.log，用户能看懂到底清了几个。
    const detail = log.join('\n')
    assert.match(detail, /清理 8 个旧文件/, `install.log 该记数：${detail}`)
    assert.match(detail, /回收 \d+ 个空目录/, `install.log 该记下回收的空目录：${detail}`)
    // ⑦ issue #27 的两点一起复验：「旧文件不删」→ 上面 ② 已经钉住 8 个全没；
    //    「清理后仍残留」→ 事务目录、journal、journal.json.tmp、暂存 new/ 一个都不许留。
    assert.equal(fs.existsSync(path.join(f.root, '.fish-install-transaction')), false, '事务目录必须随提交收回')
    const residue = after.filter(name => name.startsWith('.fish-') || name.includes('journal.json') || name.startsWith('new/'))
    assert.deepEqual(residue, [], `安装目录里不许留下任何事务或暂存残骸：${JSON.stringify(residue)}`)
    // ⑧ 体积对照：发行文件 20 个 → 12 个、字节数真的变小（不是只少了名字）。
    assert.equal(releaseBytes(f.root, staleNames), 0, '旧的 8 个发行文件更新后一个字节都不该留下')
    assert.ok(afterReleaseBytes > 0, '新版的 12 个发行文件必须真的落地')
    assert.ok(afterReleaseBytes < beforeReleaseBytes, `发行文件体积必须变小：${beforeReleaseBytes} → ${afterReleaseBytes} 字节`)
    console.log(`体积对照（发行文件）：更新前 20 个 / ${beforeReleaseBytes} 字节 → 更新后 12 个 / ${afterReleaseBytes} 字节（净减 ${beforeReleaseBytes - afterReleaseBytes} 字节）`)
    // 前后清单（报告里贴的就是这两段）：只列与清理有关的路径。
    const relevant = name => managed.has(name) || name.startsWith('resources/runtime') || name.includes('node_modules')
      || name.startsWith('resources/windhawk/Compiler') || name.includes('EBWebView') || name.startsWith('resources/onedir-only')
      || name === 'resources/user-notes.txt'
    console.log('更新前：\n  ' + before.filter(relevant).join('\n  ') + '\n更新后：\n  ' + after.filter(relevant).join('\n  '))
  } finally { fs.rmSync(f.base, { recursive: true, force: true }) }
})

// 清理必须可恢复：旧文件是「先移到事务暂存目录、提交成功后才真正丢弃」。中途失败时
// 已经移走的旧受管文件要原样回到原位，不能因为「这次要删它」就少了几个文件。
test('清理可回滚：提交中途失败时，已移走的旧受管文件与主程序原样恢复', () => {
  const f = fixture()
  try {
    const shared = { 'DSH-FISH.exe': 'old-exe', 'resources/keep.bin': 'keep-old' }
    const obsolete = { 'resources/gone.bin': 'gone-old', 'resources/old-dir/gone2.bin': 'gone2-old' }
    for (const [name, data] of Object.entries({ ...shared, ...obsolete })) put(f.root, name, data)
    put(f.root, 'install-manifest.json', JSON.stringify({
      schema: 1, product: 'FISH Dock', version: '1.2.3',
      files: Object.entries({ ...shared, ...obsolete }).map(([p, b]) => ({ path: p, bytes: Buffer.byteLength(b), sha256: sha(b) })),
    }))
    release(f.stage, { 'DSH-FISH.exe': 'new-exe', 'resources/keep.bin': 'keep-new' }, '2.0.0')
    const before = treeFiles(f.root)
    // 断在「新清单落到安装目录」这一步：此时旧受管文件已经移进事务暂存目录，
    // 只差最后一步提交。注入只命中暂存目录 → 安装目录这一跳，回滚自己的
    // 「备份 → 原位」不会被误伤。
    const staging = path.join(f.root, '.fish-install-transaction', 'new')
    const failing = {
      ...fs,
      renameSync(from, to) {
        if (to === path.join(f.root, 'install-manifest.json') && from.startsWith(staging + path.sep)) throw Object.assign(Error('模拟提交中断'), { code: 'EBUSY' })
        return fs.renameSync(from, to)
      },
    }
    assert.throws(() => load().update(f.stage, f.root, failing), /模拟提交中断/)
    assert.deepEqual(treeFiles(f.root), before, '失败后目录树必须与更新前逐条一致（旧文件回滚，不留暂存残骸）')
    assert.equal(fs.readFileSync(path.join(f.root, 'DSH-FISH.exe'), 'utf8'), 'old-exe')
    assert.equal(fs.readFileSync(path.join(f.root, 'resources/gone.bin'), 'utf8'), 'gone-old', '要删的旧文件在失败时必须回来')
    assert.equal(fs.readFileSync(path.join(f.root, 'resources/old-dir/gone2.bin'), 'utf8'), 'gone2-old', '深层旧文件同样要回来')
    assert.equal(fs.existsSync(path.join(f.root, '.fish-install-transaction')), false, '事务目录不留下')
  } finally { fs.rmSync(f.base, { recursive: true, force: true }) }
})

// t17 FAIL-3：安装目录**有** install-manifest.json（1.2.3 只记 DSH-FISH.exe），旁边还摆着
// wakuwaku.exe 与 wakuwaku-sensors.exe（0.x 的主程序与旧传感器助手）。以前这些名字只在
// 「没有清单」那条路上才扫，于是它们原样留着——升级完目录里有三个能启动的主程序。
// 现在按发行名无条件补扫：磁盘上有、新清单不再发行的，一律进受管清理。
test('有旧清单时仍按发行名收走旁边的旧 exe，无归属文件一个不动', () => {
  const f = fixture()
  try {
    const owned = { 'DSH-FISH.exe': 'MZ old program', 'resources/file.bin': 'runtime' }
    for (const [name, data] of Object.entries(owned)) put(f.root, name, data)
    put(f.root, 'wakuwaku.exe', 'MZ older program')
    put(f.root, 'wakuwaku-sensors.exe', 'MZ older helper')
    put(f.root, 'keep-me.txt', 'not ours')
    put(f.root, 'install-manifest.json', JSON.stringify({
      schema: 1, product: 'FISH Dock', version: '1.2.3',
      files: Object.entries(owned).map(([p, b]) => ({ path: p, bytes: Buffer.byteLength(b), sha256: sha(b) })),
    }))
    release(f.stage, { 'FISH-Dock.exe': 'MZ new program', 'resources/file.bin': 'runtime' }, '2.0.0')
    const result = load().update(f.stage, f.root)
    assert.equal(fs.existsSync(path.join(f.root, 'wakuwaku.exe')), false, '旧主程序必须被收走')
    assert.equal(fs.existsSync(path.join(f.root, 'wakuwaku-sensors.exe')), false, '旧传感器助手同样要收走')
    assert.equal(fs.existsSync(path.join(f.root, 'DSH-FISH.exe')), false, '清单里记过的旧名也照旧收')
    assert.equal(fs.readFileSync(path.join(f.root, 'keep-me.txt'), 'utf8'), 'not ours', '无归属文件一个都不能少')
    assert.equal(fs.readFileSync(path.join(f.root, 'FISH-Dock.exe'), 'utf8'), 'MZ new program')
    assert.ok(result.removed >= 3, `清理计数要含这三份旧 exe：${JSON.stringify(result)}`)
    console.log(`旧 exe 补扫：removed=${result.removed}，obsolete=${result.obsolete}`)
  } finally { fs.rmSync(f.base, { recursive: true, force: true }) }
})
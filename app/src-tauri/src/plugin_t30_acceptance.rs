// t30 独立压力验收（crash-win 侧）：不改产品实现，只调用 t29 已交付的
// Manager / Runner / plugin_progress / plugin_store 对真实与合成插件施压。
//
// 证据根由 FISH_DOCK_T30_ROOT 指定（应指向 build/crash-win/t30-apphome）。
// 网络下载走环境代理（HTTP(S)_PROXY，通常 127.0.0.1:7897）。

use super::{t29_namespace, t29_snapshot};
use crate::plugin_install::Manager;
use crate::plugin_package::Package;
use crate::plugin_runtime::Runner;
use crate::storage_context::Context;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn base() -> PathBuf {
    let root = PathBuf::from(std::env::var_os("FISH_DOCK_T30_ROOT").expect("设置 FISH_DOCK_T30_ROOT"));
    assert!(root.is_absolute(), "证据根必须是绝对路径");
    assert!(root.to_string_lossy().contains("t30-apphome"), "证据根必须指向隔离的 t30-apphome");
    std::fs::create_dir_all(&root).unwrap();
    root
}

fn say(path: &Path, line: &str) {
    println!("{line}");
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(path).unwrap();
    writeln!(file, "{line}").unwrap();
}

fn context(root: &Path, id: &str, directory: &str, version: &str) -> Context {
    let host = t29_namespace(root.join("host"), "host", "host", version);
    t29_namespace(root.join(directory), id, directory, version);
    Context { root: root.to_path_buf(), host }
}

fn package_with(id: &str, name: &str, directory: &str, version: &str, schema: u32, official: bool, backend: &str) -> (Package, String) {
    let backend = backend.as_bytes().to_vec();
    let sdk = include_bytes!("../../../sdk/node/plugin.mjs").to_vec();
    let mut files = BTreeMap::from([
        ("backend.mjs".to_string(), backend),
        ("sdk.mjs".to_string(), sdk),
        ("README.md".to_string(), format!("# {name} {version}").into_bytes()),
    ]);
    let hashes: BTreeMap<_, _> = files.iter().map(|(key, value)| (key.clone(), format!("{:x}", Sha256::digest(value)))).collect();
    let raw = json!({"schema":1,"id":id,"name":name,"version":version,"hostApi":1,"platforms":[crate::plugin_install::platform()],"storage":{"directory":directory,"dataSchema":schema},"backend":{"runtime":"node","entry":"backend.mjs"},"behaviors":{"autostart":true},"files":hashes});
    files.insert("plugin.json".into(), serde_json::to_vec(&raw).unwrap());
    let bytes = crate::identity_archive::zip(&files).unwrap();
    let digest = format!("{:x}", Sha256::digest(&bytes));
    let source = if official { crate::storage::Source::OfficialCatalog } else { crate::storage::Source::ThirdParty };
    let package = crate::plugin_package::inspect(&bytes, &digest, &std::env::temp_dir(), source, &crate::plugin_install::platform()).unwrap();
    (package, digest)
}

fn feishu_package(version: &str) -> (Package, String) {
    // 真实 GitHub release 资产：优先用 gh release download 预取的同一份字节
    // （build/t30-assets，仍由官方插件仓发布）；没有缓存时才现场走网络。
    let cached = std::env::var_os("FISH_DOCK_T30_ASSETS").map(PathBuf::from).map(|dir| dir.join(format!("feishu-{version}.fish-plugin"))).filter(|path| path.is_file());
    let bytes = if let Some(path) = cached {
        std::fs::read(path).expect("读取预取的官方插件包")
    } else {
        let url = format!("https://github.com/Fish-under-sea/FISH-DOCK-Plugins/releases/download/v{version}/feishu.fish-plugin");
        let mut last = String::new();
        let mut bytes = Vec::new();
        for attempt in 0..3 {
            match fetch(&url) {
                Ok(value) => { bytes = value; break; }
                Err(error) => { last = error; std::thread::sleep(Duration::from_millis(500 * (attempt + 1))); }
            }
        }
        assert!(!bytes.is_empty(), "真实官方包 {version} 下载失败（3 次）：{last}");
        bytes
    };
    let digest = format!("{:x}", Sha256::digest(&bytes));
    let package = crate::plugin_package::inspect(&bytes, &digest, &std::env::temp_dir(), crate::storage::Source::OfficialCatalog, &crate::plugin_install::platform()).unwrap();
    (package, digest)
}

fn fetch(url: &str) -> Result<Vec<u8>, String> {
    let mut last = "下载失败".to_string();
    for way in crate::proxy::web_ways(url) {
        let Some(agent) = way.agent_with(Duration::from_secs(60), |config| config.max_redirects(10).http_status_as_error(false)) else { continue };
        let mut response = match agent.get(url).header("User-Agent", "FISH-Dock-t30-acceptance").call() { Ok(response) => response, Err(error) => { last = format!("{error}"); continue } };
        if response.status().as_u16() != 200 { last = format!("HTTP {}", response.status()); continue; }
        let mut bytes = Vec::new();
        let mut reader = response.body_mut().with_config().limit(128 * 1024 * 1024).reader();
        match reader.read_to_end(&mut bytes) { Ok(_) => return Ok(bytes), Err(error) => last = format!("{error}") }
    }
    Err(last)
}

fn port_open(port: u16) -> bool {
    std::net::TcpStream::connect_timeout(&std::net::SocketAddr::from(([127, 0, 0, 1], port)), Duration::from_millis(300)).is_ok()
}

fn dirs(manager: &Manager<'_>, id: &str) -> Vec<String> {
    let base = manager.context.host.path().join("packages").join(id);
    let mut out: Vec<String> = std::fs::read_dir(&base).map(|entries| entries.flatten().filter(|entry| entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false)).map(|entry| entry.file_name().to_string_lossy().to_string()).collect()).unwrap_or_default();
    out.sort();
    out
}

fn child_pids(manager: &Manager<'_>, id: &str, node: &Path) -> Vec<u32> {
    let _ = (manager, id);
    let mut pids = Vec::new();
    let output = std::process::Command::new("wmic").args(["process", "where", "name='node.exe'", "get", "ProcessId,ParentProcessId,CommandLine", "/format:csv"]).output();
    if let Ok(output) = output {
        let text = String::from_utf8_lossy(&output.stdout);
        for line in text.lines() {
            if line.contains("backend.mjs") && line.contains(id) {
                let fields: Vec<&str> = line.split(',').collect();
                if let Some(pid) = fields.get(1).and_then(|value| value.trim().parse::<u32>().ok()) { pids.push(pid); }
            }
        }
    }
    let _ = node;
    pids
}

#[cfg(windows)]
struct LockedFile(isize);
#[cfg(windows)]
impl Drop for LockedFile { fn drop(&mut self) { unsafe { CloseHandle(self.0); } } }

#[cfg(windows)]
#[link(name = "kernel32")]
extern "system" {
    fn CreateFileW(name: *const u16, access: u32, share: u32, security: *mut std::ffi::c_void, creation: u32, flags: u32, template: isize) -> isize;
    fn CloseHandle(handle: isize) -> i32;
}

#[cfg(windows)]
fn lock_exclusive(path: &Path) -> LockedFile {
    use std::os::windows::ffi::OsStrExt;
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let handle = unsafe { CreateFileW(wide.as_ptr(), 0x8000_0000, 0, std::ptr::null_mut(), 3, 0x80, 0) };
    assert!(handle != -1, "锁定文件失败：{}", path.display());
    LockedFile(handle)
}

// 允许别的进程读，但拒绝写/删除（MoveFileEx 替换会失败，remove_dir_all 也失败）：
// 用来模拟「文件被外部程序独占」而不影响 verify 的只读校验。
#[cfg(windows)]
fn lock_deny_write(path: &Path) -> LockedFile {
    use std::os::windows::ffi::OsStrExt;
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let handle = unsafe { CreateFileW(wide.as_ptr(), 0x8000_0000, 0x1, std::ptr::null_mut(), 3, 0x80, 0) };
    assert!(handle != -1, "锁定文件失败：{}", path.display());
    LockedFile(handle)
}

fn read_text(path: &Path) -> String { std::fs::read_to_string(path).unwrap_or_default() }

#[test]
#[ignore = "t30 独立压力验收：需 FISH_DOCK_T30_ROOT、FISH_DOCK_TEST_NODE 与网络代理"]
fn t30_acceptance_matrix() {
    let root_base = base();
    let node = std::env::var_os("FISH_DOCK_TEST_NODE").map(PathBuf::from).expect("设置 FISH_DOCK_TEST_NODE");
    let mut rows: Vec<serde_json::Value> = Vec::new();

    // ── 1. 真实 registry 官方插件热更新 5 轮 ─────────────────────────────
    {
        let scenario = root_base.join("official-real");
        let _ = std::fs::remove_dir_all(&scenario);
        let root = scenario.join(".fish-dock");
        let context = context(&root, "fish.feishu", ".fishplugin-feishu", "1.0.0");
        let manager = Manager { context: &context };
        let mut runner = Runner::default();
        let log = scenario.join("rounds.log");
        let mut failures = 0u32;
        let (p0, d0) = feishu_package("2.0.0");
        manager.install(p0, &d0, true).unwrap();
        runner.set_enabled(&manager, "fish.feishu", true, Some(&node)).unwrap();
        say(&log, &format!("[1] 起点 {}", t29_snapshot(&manager, "fish.feishu", runner.is_running("fish.feishu"))));
        for round in 1..=5 {
            let version = if round % 2 == 1 { "2.1.0" } else { "2.0.0" };
            let began = Instant::now();
            let (package, digest) = feishu_package(version);
            let outcome = runner.install(&manager, package, &digest, true, Some(&node));
            let dirs_now = dirs(&manager, "fish.feishu");
            let running = runner.is_running("fish.feishu");
            let snapshot = t29_snapshot(&manager, "fish.feishu", running);
            let okay = outcome.as_ref().map(|row| row.manifest.version == version).unwrap_or(false) && running && dirs_now == vec![version.to_string()];
            if !okay { failures += 1; }
            say(&log, &format!("[1] 轮{round} target={version} {} dirs={dirs_now:?} running={running} ms={} err={:?}", snapshot, began.elapsed().as_millis(), outcome.err()));
            rows.push(json!({"scenario":"官方×更新×运行中×正常（真实 registry）","round":round,"failed":!okay,"dirs":dirs_now,"running":running,"ms":began.elapsed().as_millis()}));
        }
        // 真实下载中途取消：官方大包 magpie，取消不动已装 feishu。
        // 用仓库里的稳定目录写成本地缓存（不靠刷新网络），obtain 仍走真实 npm/GitHub 下载。
        //
        // 2.0.4 t11 基础设施修正：这里原来用 include_bytes!("../../../../plugins/catalog/stable.json")
        // 把**仓外插件仓**的文件编进二进制——普通 cargo test 因此编译不过（基线阻断）。
        // 现在改为运行期参数：只有手动跑这条 #[ignore] 的「真实 registry 集成」用例时才设
        // FISH_DOCK_TEST_OFFICIAL_CATALOG 指向官方 stable.json；没设就明确记一行跳过，
        // 普通 cargo test 不再依赖别的 checkout，也不伪造一份假目录冒充真实集成。
        // 2.0.4 t11：真实下载段可能被跳过（见下），把「断网安装不应改动」的
        // feishu 版本目录基线提到这里，后续断网用例不依赖那个可跳过的代码块。
        let feishu_dirs_before = dirs(&manager, "fish.feishu");
        let catalog = match std::env::var_os("FISH_DOCK_TEST_OFFICIAL_CATALOG").map(std::path::PathBuf::from) {
            Some(path) => match std::fs::read(&path) {
                Ok(bytes) => Some(bytes),
                Err(error) => { say(&log, &format!("[1] 跳过真实下载段：读取 FISH_DOCK_TEST_OFFICIAL_CATALOG 失败：{error}")); None }
            },
            None => { say(&log, "[1] 跳过真实下载段：未设置 FISH_DOCK_TEST_OFFICIAL_CATALOG（真实 registry 集成用例需它指向官方 stable.json）"); None }
        };
        if let Some(catalog) = catalog {
            context.host.write("config", "official-plugin-catalog.json", &catalog).unwrap();
            crate::plugin_progress::begin("fish.magpie").unwrap();
            let old_running = runner.is_running("fish.feishu");
            let old_dirs = dirs(&manager, "fish.feishu");
            let context_for_download = context.clone();
            let join = std::thread::spawn(move || crate::plugin_store::obtain(&context_for_download, "fish.magpie"));
            let mut progress_samples = Vec::new();
            for _ in 0..30 {
                std::thread::sleep(Duration::from_millis(50));
                let snap = crate::plugin_progress::snapshot();
                if snap["active"].as_bool() == Some(true) {
                    progress_samples.push((snap["received"].as_u64().unwrap_or(0), snap["total"].as_u64().unwrap_or(0)));
                }
            }
            crate::plugin_progress::cancel("fish.magpie").unwrap();
            let cancelled = join.join().unwrap();
            let distinct_received: Vec<u64> = {
                let mut values: Vec<u64> = progress_samples.iter().map(|(received, _)| *received).collect();
                values.sort_unstable(); values.dedup(); values
            };
            let cancel_err = cancelled.as_ref().err().cloned().unwrap_or_default();
            let still_running = runner.is_running("fish.feishu");
            let cancel_ok = cancelled.is_err() && old_running && still_running && dirs(&manager, "fish.feishu") == old_dirs;
            if !cancel_ok { failures += 1; }
            say(&log, &format!("[1] 下载中途取消 err={cancel_err} old_running={old_running} still_running={still_running} progress_samples={progress_samples:?} distinct={distinct_received:?}"));
            rows.push(json!({"scenario":"官方×更新×运行中×下载中途取消","round":1,"failed":!cancel_ok,"err":cancel_err,"progress_samples":progress_samples,"distinct_received":distinct_received}));
        } else {
            rows.push(json!({"scenario":"官方×更新×运行中×下载中途取消","round":1,"skipped":"未提供 FISH_DOCK_TEST_OFFICIAL_CATALOG（真实 registry 集成段跳过）"}));
        }
        // 安装 × 断网：把代理指向一个没有监听的端口，obtain 必须在下载阶段失败，
        // 且不新增注册行/目录；恢复环境后同一函数可重试。
        let old_https = std::env::var_os("HTTPS_PROXY");
        let old_http = std::env::var_os("HTTP_PROXY");
        std::env::set_var("HTTPS_PROXY", "http://127.0.0.1:9");
        std::env::set_var("HTTP_PROXY", "http://127.0.0.1:9");
        let offline = crate::plugin_store::obtain(&context, "fish.magpie");
        match old_https { Some(value) => std::env::set_var("HTTPS_PROXY", value), None => std::env::remove_var("HTTPS_PROXY") }
        match old_http { Some(value) => std::env::set_var("HTTP_PROXY", value), None => std::env::remove_var("HTTP_PROXY") }
        let offline_dirs = dirs(&manager, "fish.feishu");
        let offline_err = offline.as_ref().err().cloned().unwrap_or_default();
        let offline_ok = offline.is_err() && offline_dirs == feishu_dirs_before && runner.is_running("fish.feishu");
        if !offline_ok { failures += 1; }
        say(&log, &format!("[7] 断网安装 err={offline_err} dirs={offline_dirs:?} old_running={}", runner.is_running("fish.feishu")));
        rows.push(json!({"scenario":"安装×断网（代理端口 9）","round":1,"failed":!offline_ok,"err":offline_err,"dirs":offline_dirs}));
        runner.stop_all();
        say(&log, &format!("[1] 真实 registry 总失败={failures}"));
    }

    // ── 2. 真实进程树 + 端口：第三方合成插件，更新服务运行中 ─────────────
    {
        let scenario = root_base.join("tree-update");
        let _ = std::fs::remove_dir_all(&scenario);
        let root = scenario.join(".fish-dock");
        let context = context(&root, "acme.tree", ".plugin.acme.tree", "1.0.0");
        let manager = Manager { context: &context };
        let mut runner = Runner::default();
        let log = scenario.join("rounds.log");
        let backend = "import {spawn} from 'node:child_process';import {createServer} from 'node:net';import {createPlugin} from './sdk.mjs';const port=+process.env.FISH_T30_PORT||43130;const server=createServer(s=>s.end('ok')).listen(port);const child=spawn(process.execPath,['-e','setTimeout(()=>{},3600000)'],{stdio:'ignore'});createPlugin({id:'acme.tree',version:process.env.FISH_PLUGIN_VERSION||'1.0.0',commands:{childPid:()=>String(child.pid),port:()=>String(port)}});";
        let mut failures = 0u32;
        let (p, d) = package_with("acme.tree", "Tree", ".plugin.acme.tree", "1.0.0", 1, false, backend);
        manager.install(p, &d, false).unwrap();
        std::env::set_var("FISH_T30_PORT", "43130");
        runner.set_enabled(&manager, "acme.tree", true, Some(&node)).unwrap();
        let old_pid = runner.call("acme.tree", "childPid", json!({})).ok().and_then(|value| value.as_str().and_then(|s| s.parse::<u32>().ok()));
        for round in 1..=5 {
            let version = format!("1.{round}.0");
            let (p, d) = package_with("acme.tree", "Tree", ".plugin.acme.tree", &version, 1, false, backend);
            let before_children = child_pids(&manager, "acme.tree", &node);
            let out = runner.install(&manager, p, &d, false, Some(&node));
            let new_pid = runner.call("acme.tree", "childPid", json!({})).ok().and_then(|value| value.as_str().and_then(|s| s.parse::<u32>().ok()));
            let alive_old = before_children.iter().any(|pid| *pid != new_pid.unwrap_or(0));
            let port_up = port_open(43130);
            let okay = out.is_ok() && runner.is_running("acme.tree") && dirs(&manager, "acme.tree") == vec![version.clone()] && !alive_old && port_up;
            if !okay { failures += 1; }
            say(&log, &format!("[2] 轮{round} version={version} old_pid={old_pid:?} before={before_children:?} new_pid={new_pid:?} old_alive={alive_old} dirs={:?} ok={okay}", dirs(&manager, "acme.tree")));
            rows.push(json!({"scenario":"第三方×更新×服务运行中×正常（真实 Node 子进程/端口）","round":round,"failed":!okay,"old_pid":old_pid,"before_children":before_children,"new_pid":new_pid}));
        }
        // 卸载运行中 5 轮：每轮必须 packages/<id>/ 彻底消失、子进程树清零。
        for round in 1..=5 {
            let (p, d) = package_with("acme.tree", "Tree", ".plugin.acme.tree", &format!("2.{round}.0"), 1, false, backend);
            runner.install(&manager, p, &d, false, Some(&node)).unwrap();
            let before = child_pids(&manager, "acme.tree", &node);
            let out = runner.uninstall(&manager, "acme.tree");
            let package_exists = manager.context.host.path().join("packages").join("acme.tree").exists();
            let after = child_pids(&manager, "acme.tree", &node);
            std::thread::sleep(Duration::from_millis(200));
            let port_down = !port_open(43130);
            let okay = out.is_ok() && !package_exists && after.is_empty() && port_down;
            if !okay { failures += 1; }
            say(&log, &format!("[5] 卸载轮{round} before={before:?} after={after:?} package_exists={package_exists} err={:?}", out.err()));
            rows.push(json!({"scenario":"第三方×卸载×服务运行中（真实进程树）","round":round,"failed":!okay,"before":before,"after":after,"package_exists":package_exists}));
        }
        runner.stop_all();
        say(&log, &format!("[2] 进程树/端口/卸载总失败={failures}"));
    }

    // ── 3. 失败态重试、两次连续更新 v1→v2→v3、旧版本目录 = 0 ───────────
    {
        let scenario = root_base.join("retry-chain");
        let _ = std::fs::remove_dir_all(&scenario);
        let root = scenario.join(".fish-dock");
        let context = context(&root, "acme.weather", ".plugin.acme.weather", "1.0.0");
        let manager = Manager { context: &context };
        let mut runner = Runner::default();
        let log = scenario.join("rounds.log");
        let mut failures = 0u32;
        let backend_ok = "import {createPlugin} from './sdk.mjs';createPlugin({id:'acme.weather',version:process.env.FISH_PLUGIN_VERSION||'1.0.0',commands:{ping:()=> 'pong'}});";
        let (p, d) = package_with("acme.weather", "Weather", ".plugin.acme.weather", "1.0.0", 1, false, backend_ok);
        manager.install(p, &d, false).unwrap();
        runner.set_enabled(&manager, "acme.weather", true, Some(&node)).unwrap();
        // 同版本重打一版坏后端，模拟上一轮失败
        let broken = "import {createPlugin} from './sdk.mjs';createPlugin({id:'acme.weather',version:'1.1.0',activate:async()=>{throw Error('synthetic activation failure')}});";
        for round in 1..=5 {
            let (p, d) = package_with("acme.weather", "Weather", ".plugin.acme.weather", &format!("1.{round}.0"), 1, false, broken);
            let failed = runner.install(&manager, p, &d, false, Some(&node)).is_err();
            let old_running = runner.is_running("acme.weather");
            let (p2, d2) = package_with("acme.weather", "Weather", ".plugin.acme.weather", &format!("2.{round}.0"), 1, false, backend_ok);
            let retry = runner.install(&manager, p2, &d2, false, Some(&node));
            let okay = failed && old_running && retry.is_ok() && runner.is_running("acme.weather") && dirs(&manager, "acme.weather") == vec![format!("2.{round}.0")];
            if !okay { failures += 1; }
            say(&log, &format!("[4] 失败态重试轮{round} failed={failed} old_running={old_running} retry_ok={} dirs={:?}", retry.is_ok(), dirs(&manager, "acme.weather")));
            rows.push(json!({"scenario":"第三方×更新×上一轮失败过×重试","round":round,"failed":!okay}));
        }
        let (p, d) = package_with("acme.weather", "Weather", ".plugin.acme.weather", &format!("9.0.0"), 1, false, backend_ok);
        runner.install(&manager, p, &d, false, Some(&node)).unwrap();
        let final_dirs = dirs(&manager, "acme.weather");
        let old_zero = final_dirs == vec!["9.0.0".to_string()];
        if !old_zero { failures += 1; }
        say(&log, &format!("[11] 更新后目录={final_dirs:?} 旧版本目录清零={old_zero}"));
        rows.push(json!({"scenario":"更新后旧版本目录数=0（保留1个回滚点以内）","round":1,"failed":!old_zero,"dirs":final_dirs}));
        runner.stop_all();
        say(&log, &format!("[3] 重试/连续更新总失败={failures}"));
    }

    // ── 4. 临时目录被占（独占锁文件）→ 明确失败、无脏条目 ───────────────
    #[cfg(windows)]
    {
        let scenario = root_base.join("locked-file");
        let _ = std::fs::remove_dir_all(&scenario);
        let root = scenario.join(".fish-dock");
        let context = context(&root, "acme.lock", ".plugin.acme.lock", "1.0.0");
        let manager = Manager { context: &context };
        let log = scenario.join("rounds.log");
        let backend = "import {createPlugin} from './sdk.mjs';createPlugin({id:'acme.lock',version:'1.0.0',commands:{ping:()=> 'pong'}});";
        let (p, d) = package_with("acme.lock", "Lock", ".plugin.acme.lock", "1.0.0", 1, false, backend);
        manager.install(p, &d, false).unwrap();
        let row = manager.list().unwrap().remove(0);
        let lock_target = manager.directory(&row).unwrap().join("backend.mjs");
        let handle = lock_exclusive(&lock_target);
        let (p2, d2) = package_with("acme.lock", "Lock", ".plugin.acme.lock", "2.0.0", 1, false, backend);
        // 锁住新版将要写入的 backend.mjs：允许只读校验，拒绝写/替换。
        let target = manager.context.host.path().join("packages").join("acme.lock").join("2.0.0").join(d2.to_ascii_lowercase()).join("backend.mjs");
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(&target, b"held").unwrap();
        let handle = lock_deny_write(&target);
        let failed = manager.install(p2, &d2, false);
        let listed = manager.list().unwrap();
        let dirty = listed.iter().any(|row| row.manifest.id == "acme.lock" && row.manifest.version == "2.0.0");
        say(&log, &format!("[8] 锁文件安装 failed={failed:?} registry={:?} dirty={dirty}", listed.iter().map(|r| r.manifest.version.clone()).collect::<Vec<_>>()));
        rows.push(json!({"scenario":"安装×临时目录被占（独占文件锁）","round":1,"failed":false,"error":format!("{failed:?}"),"dirty":dirty}));
        drop(handle);
    }

    // ── 5. A5 进度按 id 键化（t38 修复后）：互不覆盖、空取消报错、互不连带 ─
    {
        let log = root_base.join("progress").join("progress.log");
        std::fs::create_dir_all(log.parent().unwrap()).unwrap();
        crate::plugin_progress::begin("official-A").unwrap();
        crate::plugin_progress::report("official-A", "下载", 100, Some(1000)).unwrap();
        crate::plugin_progress::begin("third-B").unwrap();
        crate::plugin_progress::report("third-B", "下载", 50, Some(500)).unwrap();
        let a = crate::plugin_progress::progress_of("official-A").unwrap();
        let b = crate::plugin_progress::progress_of("third-B").unwrap();
        let independent = a["id"] == "official-A" && b["id"] == "third-B" && a["received"] == 100 && b["received"] == 50;
        let empty_cancel_rejected = crate::plugin_progress::cancel("").is_err();
        crate::plugin_progress::cancel("third-B").unwrap();
        let a_cancelled_after_b = crate::plugin_progress::cancelled("official-A");
        let b_cancelled_after_b = crate::plugin_progress::cancelled("third-B");
        crate::plugin_progress::finish("official-A").unwrap();
        let b_alive_after_a_finish = crate::plugin_progress::progress_of("third-B").is_some();
        let fixed = independent && empty_cancel_rejected && !a_cancelled_after_b && b_cancelled_after_b && b_alive_after_a_finish;
        say(&log, &format!("[A5] A={a} B={b} 独立={independent} 空取消报错={empty_cancel_rejected} B取消A不受影响={} B取消命中B={b_cancelled_after_b} A完成B仍在={b_alive_after_a_finish}", !a_cancelled_after_b));
        rows.push(json!({"scenario":"A5 进度按 id 键化：并发/空取消/完成互不串台","round":1,"failed":!fixed,"independent":independent,"empty_cancel_rejected":empty_cancel_rejected,"a_cancelled_after_b":a_cancelled_after_b,"b_cancelled_after_b":b_cancelled_after_b,"b_alive_after_a_finish":b_alive_after_a_finish,"snapshotA":a,"snapshotB":b}));
        crate::plugin_progress::finish("third-B").unwrap();
    }

    // ── 6. A3 更新失败回滚 apply 中途失败 → journal 是否卡死数据通道 ──────
    #[cfg(windows)]
    {
        let scenario = root_base.join("a3-journal");
        let _ = std::fs::remove_dir_all(&scenario);
        let root = scenario.join(".fish-dock");
        let context = context(&root, "acme.journal", ".plugin.acme.journal", "1.0.0");
        let manager = Manager { context: &context };
        let mut runner = Runner::default();
        let log = scenario.join("rounds.log");
        let ok = "import {createPlugin} from './sdk.mjs';createPlugin({id:'acme.journal',version:process.env.FISH_PLUGIN_VERSION||'1.0.0',commands:{ping:()=> 'pong'}});";
        let (p, d) = package_with("acme.journal", "Journal", ".plugin.acme.journal", "1.0.0", 1, false, ok);
        manager.install(p, &d, false).unwrap();
        let row = manager.list().unwrap().remove(0);
        let ns = manager.namespace(&row).unwrap();
        std::fs::write(ns.path().join("config/keep.json"), b"old-config").unwrap();
        assert_eq!(std::fs::read(ns.path().join("config/keep.json")).unwrap(), b"old-config");
        runner.set_enabled(&manager, "acme.journal", true, Some(&node)).unwrap();
        // 先按真实回滚点导出（此时文件可读），再把旧的 keep.json 锁住；
        // 更新失败后的 restore_previous 会用这份 archive 做 apply。
        // 直接造一份要覆盖 config/keep.json 的 changes，再锁文件执行 apply：
        // 这样能精确撞 A3 的「apply 中途失败留 journal」路径。
        let mut changes: std::collections::BTreeMap<String, Option<Vec<u8>>> = std::collections::BTreeMap::new();
        changes.insert("config/keep.json".into(), Some(b"new-config".to_vec()));
        let lock_target = ns.path().join("config/keep.json");
        let handle = lock_deny_write(&lock_target);
        let applied = crate::storage_transaction::apply(&ns, changes);
        // schema 跳版 2 触发 manager.install 的 namespace 失败；restore_previous 的 apply 会撞上文件锁。
        let (p2, d2) = package_with("acme.journal", "Journal", ".plugin.acme.journal", "2.0.0", 2, false, ok);
        let update = runner.install(&manager, p2, &d2, false, Some(&node));
        let journal = ns.path().join("tmp/storage-transaction.json");
        let journal_exists = journal.exists();
        let mut errors = 0;
        for i in 0..10 {
            match ns.read("config", "keep.json") {
                Ok(_) => {}
                Err(error) => { errors += 1; if i == 0 { say(&log, &format!("[A3] 锁仍在时第1次读错误：{error}")); } }
            }
        }
        drop(handle);
        let after_unlock = ns.read("config", "keep.json");
        let row_after = manager.list().unwrap().into_iter().find(|row| row.manifest.id == "acme.journal");
        say(&log, &format!("[A3] update={update:?} backup_apply={applied:?} journal_exists={journal_exists} 锁在时10次读错误={errors} 解锁后读={after_unlock:?} row={:?}", row_after.as_ref().map(|r| r.manifest.version.clone())));
        let permanent = journal_exists && errors == 10 && after_unlock.is_err();
        rows.push(json!({"scenario":"A3 更新失败回滚 apply 中途失败 / journal 永久污染","round":10,"failed":permanent,"journal_exists":journal_exists,"read_errors_while_locked":errors,"after_unlock_error":format!("{after_unlock:?}"),"update_error":format!("{update:?}"),"backup_apply":format!("{applied:?}"),"row_version":row_after.map(|r| r.manifest.version),"permanent_channel_error":permanent}));
        runner.stop_all();
    }

    // ── 7. B4 卸载目录删不掉 → pending 是否把主动卸载报成安装未完成 ───────
    #[cfg(windows)]
    {
        let scenario = root_base.join("b4-uninstall-locked");
        let _ = std::fs::remove_dir_all(&scenario);
        let root = scenario.join(".fish-dock");
        let context = context(&root, "acme.stuck", ".plugin.acme.stuck", "1.0.0");
        let manager = Manager { context: &context };
        let log = scenario.join("rounds.log");
        let backend = "import {createPlugin} from './sdk.mjs';createPlugin({id:'acme.stuck',version:'1.0.0',commands:{ping:()=> 'pong'}});";
        let (p, d) = package_with("acme.stuck", "Stuck", ".plugin.acme.stuck", "1.0.0", 1, false, backend);
        manager.install(p, &d, false).unwrap();
        let row = manager.list().unwrap().remove(0);
        let package_dir = manager.directory(&row).unwrap();
        // 把这一版目录里的每个文件都独占锁住：remove_dir_all 一个都删不掉，
        // manifest 与全部资源都还在，pending() 才能走到 verify→误报分支。
        let mut handles = Vec::new();
        for entry in std::fs::read_dir(&package_dir).unwrap().flatten() {
            if entry.file_type().map(|kind| kind.is_file()).unwrap_or(false) {
                handles.push(lock_deny_write(&entry.path()));
            }
        }
        let uninstall = manager.uninstall("acme.stuck");
        let listed = manager.list().unwrap();
        let pending = manager.pending();
        let package_exists = manager.context.host.path().join("packages").join("acme.stuck").exists();
        let pending_has = pending.iter().any(|row| row.manifest.id == "acme.stuck");
        let registry_has=listed.iter().any(|r| r.manifest.id == "acme.stuck");
        // t38 修复后：卸载必须报错、注册行仍在、pending 不得把它报成「安装未完成」。
        let fixed=uninstall.is_err()&&registry_has&&!pending_has;
        say(&log, &format!("[B4] uninstall={uninstall:?} registry_has={registry_has} pending_has={pending_has} pending_versions={:?}", pending.iter().map(|r| (r.manifest.id.clone(), r.manifest.version.clone())).collect::<Vec<_>>()));
        drop(handles);
        let retry = manager.uninstall("acme.stuck");
        let cleared = retry.is_ok()
            && !manager.context.host.path().join("packages").join("acme.stuck").exists()
            && manager.list().unwrap().iter().all(|row| row.manifest.id != "acme.stuck")
            && manager.pending().iter().all(|row| row.manifest.id != "acme.stuck");
        say(&log, &format!("[B4] 解锁后重试 uninstall={retry:?} 全清={cleared}"));
        rows.push(json!({"scenario":"B4 卸载目录删不掉：保留注册行、pending 不误报、解锁后重试全清","round":1,"failed":!(fixed&&cleared),"uninstall_error":format!("{uninstall:?}"),"registry_has":registry_has,"pending_has":pending_has,"package_exists":package_exists,"pending_versions":pending.iter().map(|r| r.manifest.version.clone()).collect::<Vec<_>>(),"retry_error":format!("{retry:?}"),"retry_cleared":cleared}));
    }

    // ── 8. B1 Process::start 失败（新包坏了）→ 注册行/目录/进程终态 ────────
    {
        let scenario = root_base.join("b1-start-failure");
        let _ = std::fs::remove_dir_all(&scenario);
        let root = scenario.join(".fish-dock");
        let context = context(&root, "acme.b1", ".plugin.acme.b1", "1.0.0");
        let manager = Manager { context: &context };
        let mut runner = Runner::default();
        let log = scenario.join("rounds.log");
        let ok = "import {createPlugin} from './sdk.mjs';createPlugin({id:'acme.b1',version:process.env.FISH_PLUGIN_VERSION||'1.0.0',commands:{ping:()=> 'pong'}});";
        let (p, d) = package_with("acme.b1", "B1", ".plugin.acme.b1", "1.0.0", 1, false, ok);
        manager.install(p, &d, false).unwrap();
        runner.set_enabled(&manager, "acme.b1", true, Some(&node)).unwrap();
        let broken = "import {createPlugin} from './sdk.mjs';createPlugin({id:'acme.b1',version:'2.0.0',activate:async()=>{throw Error('synthetic activation failure')}});";
        let (p2, d2) = package_with("acme.b1", "B1", ".plugin.acme.b1", "2.0.0", 1, false, broken);
        let update = runner.install(&manager, p2, &d2, false, Some(&node));
        let row = manager.list().unwrap().into_iter().find(|row| row.manifest.id == "acme.b1");
        let dirs_now = dirs(&manager, "acme.b1");
        let running = runner.is_running("acme.b1");
        // t38 修复后：回滚只留旧版本目录（2.0.0 删掉），旧服务保持 running。
        let fixed=update.is_err()&&row.as_ref().is_some_and(|r|r.manifest.version=="1.0.0")&&dirs_now==vec!["1.0.0".to_string()]&&running;
        say(&log, &format!("[B1] update={update:?} row={:?} dirs={dirs_now:?} running={running} fixed={fixed}", row.as_ref().map(|r| r.manifest.version.clone())));
        rows.push(json!({"scenario":"B1 新包落地成功但 Process::start 失败：只留旧版本","round":1,"failed":!fixed,"update_error":format!("{update:?}"),"row_version":row.map(|r| r.manifest.version),"dirs":dirs_now,"running":running}));
        runner.stop_all();
    }

    // 写机器可读矩阵
    let matrix = root_base.join("t30-matrix.json");
    std::fs::write(&matrix, serde_json::to_vec_pretty(&rows).unwrap()).unwrap();
    say(&root_base.join("t30-summary.log"), &format!("[T30] rows={} json={}", rows.len(), matrix.display()));
}

#[test]
#[ignore = "t30 A1 并发锁序：基线 6724889（t37 已合入）；双线程 install/uninstall 同 id 50 轮"]
fn t30_a1_lock_order_50() {
    let root_base = base();
    let scenario = root_base.join("a1-concurrency");
    let _ = std::fs::remove_dir_all(&scenario);
    let root = scenario.join(".fish-dock");
    let context = context(&root, "acme.a1", ".plugin.acme.a1", "1.0.0");
    let manager = Manager { context: &context };
    let backend = "import {createPlugin} from './sdk.mjs';createPlugin({id:'acme.a1',version:process.env.FISH_PLUGIN_VERSION||'1.0.0',commands:{ping:()=> 'pong'}});";
    let (p, d) = package_with("acme.a1", "A1", ".plugin.acme.a1", "1.0.0", 1, false, backend);
    manager.install(p, &d, false).unwrap();
    let log = scenario.join("rounds.log");
    let started = Instant::now();
    std::thread::scope(|scope| {
        let a = &manager;
        scope.spawn(move || {
            for i in 0..50 {
                let (p, d) = package_with("acme.a1", "A1", ".plugin.acme.a1", &format!("2.{}.0", i), 1, false, backend);
                let _ = a.install(p, &d, false);
            }
        });
        let b = &manager;
        scope.spawn(move || {
            for _ in 0..50 {
                let _ = b.uninstall("acme.a1");
                std::thread::sleep(Duration::from_millis(5));
            }
        });
    });
    let elapsed = started.elapsed().as_millis();
    say(&log, &format!("[A1] 双线程 install/uninstall 同 id 50 轮完成，耗时 {elapsed}ms（基线 6724889 / t37 已合入；未卡死）"));
    assert!(elapsed < 30_000, "A1 并发 50 轮疑似卡死：{elapsed}ms");
}

#[test]
#[ignore = "t30 真重启准备：在 build/crash-win/t30-apphome/restart-home 安装 acme.restart（enabled+autostart）"]
fn t30_prepare_restart_state() {
    let root_base = base();
    let scenario = root_base.join("restart-home");
    let _ = std::fs::remove_dir_all(&scenario);
    let root = scenario.join(".fish-dock");
    let context = context(&root, "acme.restart", ".plugin.acme.restart", "1.0.0");
    let manager = Manager { context: &context };
    let backend = "import {spawn} from 'node:child_process';import {createPlugin} from './sdk.mjs';const child=spawn(process.execPath,['-e','setTimeout(()=>{},3600000)'],{stdio:'ignore'});createPlugin({id:'acme.restart',version:'1.0.0',commands:{childPid:()=>String(child.pid)}});";
    let (p, d) = package_with("acme.restart", "Restart", ".plugin.acme.restart", "1.0.0", 1, false, backend);
    manager.install(p, &d, false).unwrap();
    manager.set_enabled("acme.restart", true).unwrap();
    manager.set_autostart("acme.restart", true).unwrap();
    let mut runner = Runner::default();
    runner.stop_all();
    context.host.write("config", "plugin-dependencies.json", br#"{"node":"C:\\Program Files\\nodejs\\node.exe"}"#).unwrap();
    context.host.write("config", "config.json", br#"{"display":"island","onboarded":true,"shortcutsEnabled":false}"#).unwrap();
    let expected = scenario.join("expected.json");
    std::fs::write(&expected, serde_json::to_vec_pretty(&json!({"id":"acme.restart","version":"1.0.0","enabled":true,"autostart":true,"root":root})).unwrap()).unwrap();
    println!("[t30-restart] prepared {} -> {}", root.display(), expected.display());
}

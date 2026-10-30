//! 可恢复安装注册表：先核验资源，最后切换活动版本；卸载保留用户命名空间。
use std::{collections::BTreeMap, path::PathBuf};
use serde::{Deserialize, Serialize};
use crate::{plugin_manifest::Manifest, plugin_package::Package, storage::Source, storage_context::{Context, Namespace}};
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all="camelCase")]
pub struct Installed { pub manifest: Manifest, pub digest: String, pub official: bool, pub enabled: bool,
    // 随 Dock 启动就把它的本机服务拉起来（旧注册表没有这一项时按开处理）。
    #[serde(default = "desired_autostart")] pub autostart: bool }
fn desired_autostart() -> bool { true }
#[derive(Default,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
struct Registry { schema: u32, entries: BTreeMap<String, Installed> }
// InstallNotice 是一次安装没能收尾、被回滚掉之后留在插件卡上的一句话：
// 哪一版、为什么、改用哪条路装（#21）。落盘在 config/plugin-install-notices.json，
// 同一个插件只留最后一条，装成功即清掉。
// cleanup=true 表示「这一版装上了，只是旧文件没清干净」（#t8）：它是一条警告，
// 不是失败——前端不能把它写成「安装未完成」。
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(rename_all="camelCase")]
pub struct InstallNotice { pub version: String, pub reason: String, pub hint: String, pub at: u64,
    #[serde(default)] pub cleanup: bool }
pub fn platform() -> String { format!("{}-{}",std::env::consts::OS,std::env::consts::ARCH) }
// npm_install_hint 是安装回滚后告诉用户「还能怎么装」：官方插件按 id 的最后一段
// 对应 npm 单包（fish.magpie → @fish-under-sea/fish-dock-plugin-magpie）。
pub fn npm_install_hint(id:&str) -> String {
    let key=id.rsplit('.').next().filter(|key|!key.is_empty()).unwrap_or("plugin");
    format!("改用 npm 安装：npm install --no-save @fish-under-sea/fish-dock-plugin-{key}@latest，再 npx @fish-under-sea/fish-dock-plugin-{key} --out <目录> 导出 .fish-plugin，然后在插件页导入")
}
fn source(row: &Installed) -> Source { if row.official { Source::OfficialCatalog } else { Source::ThirdParty } }
pub struct Manager<'a> { pub context: &'a Context }
static EDIT_REGISTRY:std::sync::Mutex<()>=std::sync::Mutex::new(());
impl Manager<'_> {
    fn registry(&self) -> Result<Registry,String> {
        let Some(bytes) = self.context.host.read("config","installed-plugins.json")? else { return Ok(Registry { schema:1,entries:BTreeMap::new() }); };
        let registry: Registry = serde_json::from_slice(&bytes).map_err(|_| "插件安装注册表损坏")?;
        if registry.schema != 1 || registry.entries.len() > 100 { return Err("插件安装注册表版本或数量无效".into()); }
        for (id,row) in &registry.entries {
            row.manifest.validate(&self.context.root,source(row),&platform())?;
            if id != &row.manifest.id || row.digest.len() != 64 || !row.digest.bytes().all(|b| b.is_ascii_hexdigit()) { return Err("插件注册归属或摘要无效".into()); }
        }
        Ok(registry)
    }
    fn save(&self, registry: &Registry) -> Result<(),String> { self.context.host.write("config","installed-plugins.json",&serde_json::to_vec_pretty(registry).map_err(|_| "插件注册表编码失败")?) }
    pub fn list(&self) -> Result<Vec<Installed>,String> { Ok(self.registry()?.entries.into_values().collect()) }
    pub fn directory(&self, row:&Installed) -> Result<PathBuf,String> {
        row.manifest.validate(&self.context.root,source(row),&platform())?;
        if row.digest.len()!=64 || !row.digest.bytes().all(|b|b.is_ascii_hexdigit()) { return Err("插件包摘要无效".into()); }
        crate::plugin_manifest::package_path(self.context.host.path(),&format!("packages/{}/{}/{}",row.manifest.id,row.manifest.version,row.digest.to_ascii_lowercase()))
    }
    pub fn namespace(&self,row:&Installed) -> Result<Namespace,String> {
        let m=&row.manifest;
        let root=crate::storage::namespace(&self.context.root,&m.id,&m.storage.directory,source(row))?;
        match Namespace::existing(&root)? {
            Some(ns) => crate::storage_version::change(&ns,&m.version,m.storage.data_schema),
            None => self.context.plugin(&m.id,&m.storage.directory,source(row),&m.version,m.storage.data_schema),
        }
    }
    pub fn verify(&self,row:&Installed) -> Result<(),String> {
        use sha2::{Digest,Sha256};
        let root=self.directory(row)?;
        let raw=crate::identity_store::read_optional(&root.join("plugin.json"))?.ok_or("已安装插件清单缺失")?;
        let manifest=Manifest::parse(&raw,&self.context.root,source(row),&platform())?;
        if serde_json::to_value(&manifest).ok()!=serde_json::to_value(&row.manifest).ok() { return Err("安装清单与注册表不一致".into()); }
        for (name,digest) in &manifest.files {
            let path=crate::plugin_manifest::package_path(&root,name)?;
            let bytes=crate::plugin_package::read(&path)?.ok_or("已安装插件资源缺失")?;
            if !format!("{:x}",Sha256::digest(&bytes)).eq_ignore_ascii_case(digest) { return Err(format!("已安装插件资源被改动：{name}")); }
        }
        Ok(())
    }
    pub fn install(&self, package:Package,digest:&str,official:bool) -> Result<Installed,String> {
        let _lock=EDIT_REGISTRY.lock().map_err(|_|"插件安装锁不可用")?;
        self.recover_unlocked()?;
        let mut registry=self.registry()?;
        let previous=registry.entries.get(&package.manifest.id).cloned();
        // 自启动是两件事的与：新版本有没有声明 behaviors.autostart，以及用户上一版
        // 的选择。2.0.0 起 feishu 这类没有本机服务的插件不再声明这个钩子，而旧注册
        // 表里可能还留着 true ——那就是一条过期定义：它会继续把 FISH_PLUGIN_AUTOSTART=1
        // 传给一个不认这个变量的插件，插件卡上也还画着那颗开关（#f3「删净旧版本
        // 定义与配置」）。升级时就地收掉，并留一行日志说明为什么变了。
        let declared_autostart=package.manifest.behaviors.autostart;
        let inherited_autostart=previous.as_ref().map_or(true,|row|row.autostart);
        let autostart=declared_autostart&&inherited_autostart;
        // 只有「上一版确实开着、这一版不再声明」才算清掉了一条旧定义；从零装一个
        // 没有本机服务的插件是正常情况，不该在日志里说成「清掉旧设置」。
        if previous.as_ref().is_some_and(|row|row.autostart)&&!autostart { eprintln!("FISH Dock: 插件 {} 的新版本不再声明随 Dock 启动，已清掉旧版本留下的自启动设置",package.manifest.id); }
        let row=Installed { manifest:package.manifest,digest:digest.to_ascii_lowercase(),official,enabled:previous.as_ref().map_or(true,|r|r.enabled),autostart };
        row.manifest.validate(&self.context.root,source(&row),&platform())?;
        let directory=self.directory(&row)?;
        // 这一版是否已经完整落地：上次崩在写注册表前后、或那次 save 失败都会留下
        // 一个完整的版本目录。重试必须是幂等的——认出来就不再重写一遍包，直接补
        // 提交，否则用户点第二次还是从头下载、从头落地（#f3「更新拿不到」）。
        if self.verify(&row).is_err() {
            // 发行文件仅写入摘要专属目录，旧版本始终可用。
            for (name,bytes) in package.files {
                let path=crate::plugin_manifest::package_path(&directory,&name)?;
                crate::plugin_package::write(self.context.host.path(),&path,&bytes)?;
            }
        }
        // 落地不完整（半写、位数不对）时不能只把错误抛出去：那样 packages/ 下会留
        // 一个「看起来装好了」的目录、注册表里却没有这一行，插件卡直接消失，用户
        // 没有任何依据判断发生了什么。删掉这一版并留下提示。
        if let Err(error)=self.verify(&row) {
            let _=self.rollback_install(None,&row,&error);
            return Err(error);
        }
        let journal=serde_json::json!({"schema":1,"previous":previous,"next":row});
        let journal_name=format!("plugin-install-{}.json",row.manifest.id);
        self.context.host.write("tmp",&journal_name,&serde_json::to_vec(&journal).map_err(|_|"安装事务编码失败")?)?;
        // 事务先写，是为了崩在提交中途还能续做；但提交这一步一旦同步失败（例如
        // data schema 要换、还没有迁移适配器），这条事务就永远提交不了——留着它，
        // 每次 recover 都会撞在同一道守卫上，把整个插件管理卡死。就地回滚。
        if let Err(error)=self.namespace(&row) {
            let journal_path=self.context.host.file("tmp",&journal_name)?;
            if let Err(cleanup)=self.rollback_install(Some(&journal_path),&row,&error) { return Err(format!("{error}；回滚待重试：{cleanup}")); }
            return Err(error);
        }
        registry.entries.insert(row.manifest.id.clone(),row.clone());
        if let Err(error)=self.save(&registry) {
            // 注册表没写成：这一版不能留在盘上冒充「已安装」，旧命名空间也要还回去。
            // 以前这里既不删目录也不记提示，用户看到的是「装完了，列表里却没有」。
            let mut reason=error;
            if let Some(old)=&previous { if let Err(rollback)=self.namespace(old) { reason=format!("{reason}；旧版目录恢复待重试：{rollback}"); } }
            let journal_path=self.context.host.file("tmp",&journal_name)?;
            let _=self.rollback_install(Some(&journal_path),&row,&reason);
            return Err(reason);
        }
        let marker=self.context.host.file("tmp",&journal_name)?;
        std::fs::remove_file(marker).map_err(|_|"插件安装已提交，事务清理待重试")?;
        // 这一版装上了：上一次失败留下的提示不再成立。
        let _=self.clear_notice(&row.manifest.id);
        Ok(row)
    }
    pub fn recover(&self) -> Result<(),String> {
        let _lock=EDIT_REGISTRY.lock().map_err(|_|"插件安装锁不可用")?;
        self.recover_unlocked()
    }
    // recover_unlocked 收尾上次没走完的安装事务（#21）。
    //
    // 事务在「资源已落地、清单已核验」之后写，最后才提交注册表。以前这里只把事务
    // 删掉了事——用户看到的是「装了一晚上，重启回来还是『安装』按钮」，包却已经
    // 躺在 packages/ 里。现在分三种情况：
    //
    //   1. 注册表里已经是这一版：提交过了，只删事务；
    //   2. 不是这一版、但落地文件逐个核验通过：接着提交（与 install() 的提交路径
    //      一致：切命名空间 → 写注册表），用户不用重装；
    //   3. 核验不过（半写、被改动），或者核验过了但提交这一步被拒（数据 schema
    //      跨到没有迁移适配器的版本）：回滚——删这一版的目录与事务，并在插件卡上
    //      留一条提示，让他改用 npm 单包装。这里不能把错误抛出去：每次 install /
    //      set_enabled / set_autostart / uninstall 都从这个收尾开始，一条提交不了
    //      的事务抛错，就等于把整个插件管理卡死。
    fn recover_unlocked(&self) -> Result<(),String> {
        let mut registry=self.registry()?;
        let temporary=self.context.host.path().join("tmp");
        let entries=match std::fs::read_dir(&temporary) {
            Ok(entries)=>entries,
            // 从没写过事务：没有 tmp 目录是正常的，不是错误。
            Err(error) if error.kind()==std::io::ErrorKind::NotFound => return Ok(()),
            Err(_)=>return Err("插件事务目录读取失败".into()),
        };
        for entry in entries {
            let path=entry.map_err(|_|"插件事务条目读取失败")?.path();
            let name=path.file_name().and_then(|n|n.to_str()).unwrap_or("");
            if !name.starts_with("plugin-install-") || !name.ends_with(".json") { continue; }
            let bytes=crate::plugin_package::read(&path)?.ok_or("插件事务缺失")?;
            let value:serde_json::Value=serde_json::from_slice(&bytes).map_err(|_|"插件事务损坏")?;
            if value["schema"]!=1 { return Err("插件事务版本无效".into()); }
            let next:Installed=serde_json::from_value(value["next"].clone()).map_err(|_|"插件事务下一版本无效")?;
            if name!=format!("plugin-install-{}.json",next.manifest.id) { return Err("插件事务身份不一致".into()); }
            let id=next.manifest.id.clone();
            // 1) 已经提交过：只清事务。
            let committed=registry.entries.get(&id).is_some_and(|active|active.digest.eq_ignore_ascii_case(&next.digest)&&active.manifest.version==next.manifest.version);
            if committed { self.drop_journal(&path)?; self.clear_notice(&id)?; continue; }
            // 2) 落地文件齐、摘要一致：续做这次安装。
            match self.verify(&next) {
                Ok(()) => match self.namespace(&next) {
                    Ok(_namespace) => {
                        registry.entries.insert(id.clone(),next.clone());
                        self.save(&registry)?;
                        self.clear_notice(&id)?;
                        self.drop_journal(&path)?;
                    }
                    // 包本身是完整的，但提交这一步被拒（数据 schema 跨到没有迁移
                    // 适配器的版本就是这样，storage_version 的守卫会说清楚）：这不是
                    // 「崩在提交中途」，是一次真的提交不上去的安装，按失败安装回滚，
                    // 不能让 recover 抛错——否则一条事务就能把整个插件管理拖死。
                    Err(error) => self.rollback_install(Some(&path),&next,&error)?,
                },
                // 3) 包不完整：回滚，并把「改用 npm 装」写在插件卡上。
                Err(error) => self.rollback_install(Some(&path),&next,&error)?,
            }
        }
        Ok(())
    }
    // rollback_install 撤掉一次提交不上去的安装：删这一版的落地目录与事务，并在
    // 插件卡上留下「哪一版没装成、为什么」。用户命名空间一点不碰——活动版本还是
    // 上一版，所以这是一次纯回滚。journal 为 None 表示事务还没写（落地核验就失败
    // 的那条路），没有事务可删。
    fn rollback_install(&self,journal:Option<&std::path::Path>,next:&Installed,reason:&str) -> Result<(),String> {
        if let Ok(directory)=self.directory(next) {
            let _=std::fs::remove_dir_all(&directory);
            // 版本目录空了就一起收掉：留着空的 packages/<id>/<version>/ 会让人以为
            // 这一版还在盘上（非空时 remove_dir 会失败，正好不动别人的东西）。
            if let Some(version)=directory.parent() { let _=std::fs::remove_dir(version); }
        }
        if let Some(journal)=journal { self.drop_journal(journal)?; }
        self.note_install_rollback(&next.manifest.id,&next.manifest.version,reason)
    }
    // drop_journal 删掉一条已处理完的安装事务。
    fn drop_journal(&self,path:&std::path::Path) -> Result<(),String> {
        crate::storage::safe(path)?;
        std::fs::remove_file(path).map_err(|_|"插件事务清理失败".into())
    }
    // notices 是每个插件最后一条失败提示（按插件 id）。
    fn notices(&self) -> Result<BTreeMap<String,InstallNotice>,String> {
        let Some(bytes)=self.context.host.read("config","plugin-install-notices.json")? else { return Ok(BTreeMap::new()); };
        // 提示文件坏了不该拦住启动：读不出来就当没有。
        Ok(serde_json::from_slice(&bytes).unwrap_or_default())
    }
    fn save_notices(&self,notices:&BTreeMap<String,InstallNotice>) -> Result<(),String> {
        self.context.host.write("config","plugin-install-notices.json",&serde_json::to_vec_pretty(notices).map_err(|_|"安装提示编码失败")?)
    }
    // notice 是插件页要显示的那一条（没有就是 None）。
    pub fn notice(&self,id:&str) -> Option<InstallNotice> { self.notices().ok()?.get(id).cloned() }
    pub fn clear_notice(&self,id:&str) -> Result<(),String> {
        let mut notices=self.notices()?;
        if notices.remove(id).is_none() { return Ok(()); }
        self.save_notices(&notices)
    }
    // note_install_rollback 记一条「哪一版没装成、为什么、改用哪条路」。
    fn note_install_rollback(&self,id:&str,version:&str,reason:&str) -> Result<(),String> {
        self.store_notice(id,InstallNotice{version:version.into(),reason:reason.into(),hint:npm_install_hint(id),at:crate::now_ms(),cleanup:false})
    }
    // note_cleanup_issue 记一条「这一版装上了，但旧文件没清干净」（#t8）。它是警告：
    // 清理失败不回滚升级，也不能让用户以为没装上。
    fn note_cleanup_issue(&self,id:&str,version:&str,reason:&str) -> Result<(),String> {
        self.store_notice(id,InstallNotice{version:version.into(),reason:reason.into(),hint:String::new(),at:crate::now_ms(),cleanup:true})
    }
    fn store_notice(&self,id:&str,notice:InstallNotice) -> Result<(),String> {
        let mut notices=self.notices()?;
        notices.insert(id.into(),notice);
        // 最多留 20 条：这是提示，不是账本。
        while notices.len()>20 { let Some(first)=notices.keys().next().cloned() else { break }; notices.remove(&first); }
        self.save_notices(&notices)
    }
    // pending 是「盘上有一版、注册表里没有」的残留：安装崩在写注册表之前，或者
    // 注册表那一次写入失败，都会留下一个完整、能逐个核验的版本目录。它不算已安装
    // ——于是插件卡从列表里直接消失，用户看到的是「装完了，但没装上/更新拿不到」
    // （#f3）。这里把「比注册表里的版本更新、且自己能核验通过」的那些报上去，
    // 让插件页能说一句「安装未完成，点重试」，而不是一片沉默。
    // 比注册表更旧的目录不报：那是升级后没清干净的旧版本，属于清理的话题。
    pub fn pending(&self) -> Vec<Installed> {
        let mut found:BTreeMap<String,Installed>=BTreeMap::new();
        let Ok(registry)=self.registry() else { return Vec::new() };
        let Ok(plugins)=std::fs::read_dir(self.context.host.path().join("packages")) else { return Vec::new() };
        for plugin in plugins.flatten() {
            if !plugin.file_type().is_ok_and(|kind|kind.is_dir()) { continue; }
            let id=plugin.file_name().to_string_lossy().to_string();
            if id.is_empty()||id.contains('/')||id.contains('\\')||id.contains("..") { continue; }
            let source=if id.starts_with("fish."){Source::OfficialCatalog}else{Source::ThirdParty};
            let active=match registry.entries.get(&id) { Some(active)=>crate::plugin_manifest::version(&active.manifest.version).ok(), None=>None };
            let Ok(versions)=std::fs::read_dir(plugin.path()) else { continue };
            for version in versions.flatten() {
                if !version.file_type().is_ok_and(|kind|kind.is_dir()) { continue; }
                let version_name=version.file_name().to_string_lossy().to_string();
                let Ok(release)=crate::plugin_manifest::version(&version_name) else { continue };
                if active.is_some_and(|current|release<=current) { continue; }
                let Ok(digests)=std::fs::read_dir(version.path()) else { continue };
                for digest in digests.flatten() {
                    if !digest.file_type().is_ok_and(|kind|kind.is_dir()) { continue; }
                    let digest_name=digest.file_name().to_string_lossy().to_string();
                    if digest_name.len()!=64||!digest_name.bytes().all(|b|b.is_ascii_hexdigit()) { continue; }
                    let Some(raw)=crate::plugin_package::read(&digest.path().join("plugin.json")).ok().flatten() else { continue };
                    let Ok(manifest)=Manifest::parse(&raw,&self.context.root,source,&platform()) else { continue };
                    if manifest.id!=id||manifest.version!=version_name { continue; }
                    let row=Installed{manifest,digest:digest_name.to_ascii_lowercase(),official:source==Source::OfficialCatalog,enabled:false,autostart:false};
                    if self.verify(&row).is_err() { continue; }
                    // 一个插件只报最高的一版：插件卡上一颗「重试安装」就够了。
                    let better=found.get(&id).and_then(|known|crate::plugin_manifest::version(&known.manifest.version).ok()).map_or(true,|known|release>known);
                    if better { found.insert(id.clone(),row); }
                }
            }
        }
        found.into_values().collect()
    }
    pub(super) fn restore_registration(&self, row:&Installed) -> Result<(),String> {
        let _lock=EDIT_REGISTRY.lock().map_err(|_|"插件安装锁不可用")?;
        row.manifest.validate(&self.context.root,source(row),&platform())?;
        let mut registry=self.registry()?;
        self.namespace(row)?;
        registry.entries.insert(row.manifest.id.clone(),row.clone());self.save(&registry)
    }
    pub fn set_enabled(&self,id:&str,enabled:bool) -> Result<Installed,String> {
        let _lock=EDIT_REGISTRY.lock().map_err(|_|"插件安装锁不可用")?;
        self.recover_unlocked()?;
        let mut registry=self.registry()?;
        let row=registry.entries.get_mut(id).ok_or("插件未安装")?;
        if enabled { self.verify(row)?; self.namespace(row)?; }
        row.enabled=enabled; let row=row.clone(); self.save(&registry)?; Ok(row)
    }
    pub fn set_autostart(&self,id:&str,autostart:bool) -> Result<Installed,String> {
        let _lock=EDIT_REGISTRY.lock().map_err(|_|"插件安装锁不可用")?;
        self.recover_unlocked()?;
        let mut registry=self.registry()?;
        let row=registry.entries.get_mut(id).ok_or("插件未安装")?;
        row.autostart=autostart; let row=row.clone(); self.save(&registry)?; Ok(row)
    }
    pub fn uninstall(&self,id:&str) -> Result<(),String> {
        let _lock=EDIT_REGISTRY.lock().map_err(|_|"插件安装锁不可用")?;
        self.recover_unlocked()?;
        let mut registry=self.registry()?;
        if registry.entries.remove(id).is_none() { return Err("插件未安装".into()); }
        self.save(&registry)?;
        // 插件都卸了，它那条「哪一版没装成」的提示也不再成立；留着会在下次
        // 重新装同一个插件时冒出来，说一件早就不成立的事。
        let _=self.clear_notice(id);
        // 发行文件（packages/<id>/）是宿主管的，不属于用户数据：卸了就该一起走。
        // 以前只删注册行，几十 MB 的包永远留在盘上；而且「缺了注册行、包还在」正是
        // 插件页报「安装未完成」的判据——用户主动卸掉的东西不该长这样（#t8）。
        // 用户数据在 .fishplugin-* / .plugin.* 命名空间里，一个字节都不碰。
        let packages=self.context.host.path().join("packages");
        if !id.is_empty()&&!id.contains('/')&&!id.contains('\\')&&!id.contains("..") {
            let base=packages.join(id);
            // 只删 packages/ 的直接子目录，绝不顺着 id 往外走。
            if base.parent()==Some(packages.as_path()) {
                match std::fs::remove_dir_all(&base) {
                    Ok(()) => {}
                    Err(error) if error.kind()==std::io::ErrorKind::NotFound => {}
                    Err(_) => return Err("插件已卸载，发行文件清理待重试：请重启后再卸载一次".into()),
                }
            }
        }
        Ok(())
    }
    // prune_versions 在升级提交后删掉过期版本目录：除当前激活版本外一律删除，
    // 不再保留回退点（已确认取消回滚点，旧 exe 不留）。以前每个版本都永久留在
    // packages/<id>/ 下，一次升级多一份几十 MB；旧目录还带着旧 manifest，一旦有
    // 路径拼错就会读到旧界面。
    // 只删 packages/<id>/ 下一层的版本目录，名字里带分隔符或点段的一律不碰。
    //
    // 另外两样同类残留（#t8）：同一版号重打的包会在 <version>/ 下留下上一个摘要的
    // 目录，写包中途失败还会留下 .install-* 暂存文件——它们和旧版本目录一样占地方、
    // 一样会让人以为那一版还在。当前版本的目录、当前摘要的目录一律不碰。
    pub fn prune_versions(&self, active:&Installed) -> Result<Vec<String>,String> {
        let base=self.context.host.path().join("packages").join(&active.manifest.id);
        let entries=match std::fs::read_dir(&base) {
            Ok(entries)=>entries,
            Err(error) if error.kind()==std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(_)=>return Err("插件版本目录读取失败".into()),
        };
        let active_version=&active.manifest.version;
        let mut others:Vec<String>=Vec::new();
        for entry in entries {
            let entry=entry.map_err(|_|"插件版本目录条目读取失败")?;
            if !entry.file_type().map_err(|_|"插件版本目录类型读取失败")?.is_dir() { continue; }
            let name=entry.file_name().to_string_lossy().to_string();
            if name.is_empty()||name.contains('/')||name.contains('\\')||name.contains("..") { continue; }
            if &name==active_version { continue; }
            others.push(name);
        }
        // 剩下的全是该删的版本；按名字排一下只为了返回顺序确定（日志与测试好读），
        // 不代表任何保留优先级。
        others.sort();
        let mut removed=Vec::new();
        for name in others {
            std::fs::remove_dir_all(base.join(&name)).map_err(|_|format!("过期版本目录清理失败：{name}"))?;
            removed.push(name);
        }
        // 当前版本目录里只留当前摘要：同版本重打包留下的旧摘要目录、以及写包中途
        // 失败留下的 .install-* 暂存文件，一起收掉。
        let current=base.join(active_version);
        for entry in std::fs::read_dir(&current).map_err(|_|"当前插件版本目录读取失败")? {
            let entry=entry.map_err(|_|"当前插件版本目录条目读取失败")?;
            let name=entry.file_name().to_string_lossy().to_string();
            if name.is_empty()||name.contains('/')||name.contains('\\')||name.contains("..") { continue; }
            let digest=entry.file_type().map_err(|_|"当前插件版本目录类型读取失败")?.is_dir();
            if digest {
                if name.eq_ignore_ascii_case(&active.digest) { continue; }
                if name.len()!=64||!name.bytes().all(|b|b.is_ascii_hexdigit()) { continue; }
                std::fs::remove_dir_all(entry.path()).map_err(|_|format!("过期摘要目录清理失败：{active_version}/{name}"))?;
                removed.push(format!("{active_version}/{name}"));
            } else if name.starts_with(".install-") {
                std::fs::remove_file(entry.path()).map_err(|_|format!("插件暂存文件清理失败：{name}"))?;
                removed.push(format!("{active_version}/{name}"));
            }
        }
        Ok(removed)
    }
    // prune_backups 收掉插件命名空间里越攒越多的回滚点（#t8）：每次升级都会写一份
    // before-update-<ms>.zip 与一份 version-<hash>.json，以前只增不减——本机 7 次
    // 升级就留了 7 个 zip + 11 个 version.json。用户已确认取消回滚点，但保留最近
    // 一份仍然有价值（手动恢复上一步），所以每类只留最新的一个。
    // 只认这两类文件名，其余（用户自己放进 backups 的东西）一律不碰。
    pub fn prune_backups(&self, active:&Installed) -> Result<Vec<String>,String> {
        // 备份在插件命名空间里（.fishplugin-*/backups），不在 packages/ 下。这里只
        // 算路径、不碰命名空间本身：清理不该顺手改写归属记录或重建目录。
        let root=crate::storage::namespace(&self.context.root,&active.manifest.id,&active.manifest.storage.directory,source(active))?.join("backups");
        let entries=match std::fs::read_dir(&root) {
            Ok(entries)=>entries,
            Err(error) if error.kind()==std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(_)=>return Err("插件备份目录读取失败".into()),
        };
        let mut updates:Vec<(std::time::SystemTime,String)>=Vec::new(); let mut versions:Vec<(std::time::SystemTime,String)>=Vec::new();
        for entry in entries {
            let entry=entry.map_err(|_|"插件备份目录条目读取失败")?;
            if !entry.file_type().map_err(|_|"插件备份目录类型读取失败")?.is_file() { continue; }
            let name=entry.file_name().to_string_lossy().to_string();
            if name.is_empty()||name.contains('/')||name.contains('\\')||name.contains("..") { continue; }
            // before-update-<ms> 的名字自带先后，version-<hash> 没有——一律按修改时间排，
            // 两类才都能「留最新」。
            let modified=entry.metadata().ok().and_then(|meta|meta.modified().ok()).unwrap_or(std::time::UNIX_EPOCH);
            if name.starts_with("before-update-")&&name.ends_with(".zip") { updates.push((modified,name)); }
            else if name.starts_with("version-")&&name.ends_with(".json") { versions.push((modified,name)); }
        }
        let mut removed=Vec::new();
        for family in [&mut updates,&mut versions] {
            family.sort();
            let _newest=family.pop();
            for (_,name) in family.iter() {
                std::fs::remove_file(root.join(name)).map_err(|_|format!("过期备份清理失败：{name}"))?;
                removed.push(name.clone());
            }
        }
        Ok(removed)
    }
    // cleanup 收一次升级后的残留：旧版本目录、旧摘要目录、暂存文件、越攒越多的回滚点
    // 备份。**永不失败**——清理出问题不能回滚已经装好的升级，也不能卡住启动；失败只
    // 记一条警告（notice.cleanup=true）等下次启动再试。
    pub fn cleanup(&self, active:&Installed) -> Vec<String> {
        let mut problems=Vec::new();
        match self.prune_versions(active) { Ok(removed)=>{ if !removed.is_empty() { eprintln!("FISH Dock: 插件过期文件清理完成（{}）：{}",active.manifest.id,removed.join("、")); } }, Err(error)=>problems.push(error) }
        if let Err(error)=self.prune_backups(active) { problems.push(error); }
        if !problems.is_empty() { let reason=problems.join("；"); eprintln!("FISH Dock: 插件清理失败（{}）：{reason}",active.manifest.id); let _=self.note_cleanup_issue(&active.manifest.id,&active.manifest.version,&reason); }
        problems
    }
    // cleanup_staging 收宿主与各已是插件的 tmp 下的事务残留（#t11）：
    //   - tmp/txn-*：没有 journal 指认的暂存目录（提交完 journal 就没了，目录却留着）；
    //   - tmp/plugin-install-<id>.json：指向已经卸载掉的插件的安装事务。
    // 与 cleanup() 同样**永不失败**：失败只记日志 + 一条 cleanup 警告，等下次启动再试。
    // 和 cleanup() 分开，是因为这一类残留与「哪个插件在升级」无关——注册表为空时启动
    // 也要能补清。
    pub fn cleanup_staging(&self) -> Vec<String> {
        let mut problems=Vec::new();
        let mut roots=vec![self.context.host.path().to_path_buf()];
        match self.list() {
            Ok(rows)=>for row in rows { if let Ok(path)=crate::storage::namespace(&self.context.root,&row.manifest.id,&row.manifest.storage.directory,source(&row)) { if !roots.contains(&path) { roots.push(path); } } },
            Err(error)=>problems.push(error),
        }
        for root in roots {
            match crate::storage_transaction::prune_staging(&root) {
                Ok(removed)=>{ if !removed.is_empty() { eprintln!("FISH Dock: 事务暂存清理完成（{}）：{}",root.display(),removed.join("、")); } },
                Err(error)=>problems.push(error),
            }
        }
        match self.prune_install_journals() {
            Ok(removed)=>{ if !removed.is_empty() { eprintln!("FISH Dock: 孤儿安装事务清理完成：{}",removed.join("、")); } },
            Err(error)=>problems.push(error),
        }
        // 下载来源记录也是同一类残留：插件早就卸了，那条「上次是从 npm 装的」还留在
        // config/plugin-fetch.json 里，下次重装这个 id 之前会在插件卡上说一件已经不
        // 成立的事。只删记录，不动任何文件。
        match self.prune_orphan_fetch_records() {
            Ok(removed)=>{ if !removed.is_empty() { eprintln!("FISH Dock: 过期下载来源记录清理完成：{}",removed.join("、")); } },
            Err(error)=>problems.push(error),
        }
        // 这一类残留没有「哪张插件卡」可挂（可能一个插件都没装），所以只记日志，
        // 不写 notice——通知栏里挂一条没人认领的警告反而更乱。
        if !problems.is_empty() { eprintln!("FISH Dock: 临时目录清理失败：{}",problems.join("；")); }
        problems
    }
    // prune_orphan_fetch_records 删掉「已经不在注册表里」的插件的下载来源记录。
    // 这份记录只为插件卡显示，卸载之后就没有归属，留着就是过期定义。
    fn prune_orphan_fetch_records(&self) -> Result<Vec<String>,String> {
        let registry=self.registry()?;
        let mut records=crate::plugin_store::fetch_records(self.context);
        let stale:Vec<String>=records.keys().filter(|id|!registry.entries.contains_key(*id)).cloned().collect();
        if stale.is_empty() { return Ok(Vec::new()); }
        for id in &stale { records.remove(id); }
        crate::plugin_store::save_records(self.context,&records)?;
        Ok(stale)
    }
    // prune_install_journals 删掉「注册表里没有这个插件」的安装事务文件。recover 只处理
    // 得了一部分（已提交 / 可续做 / 该回滚），一条指向已卸载插件的事务留着没有意义。
    fn prune_install_journals(&self) -> Result<Vec<String>,String> {
        let registry=self.registry()?;
        let trusted=self.context.host.path().join("tmp");
        let entries=match std::fs::read_dir(&trusted) {
            Ok(entries)=>entries,
            Err(error) if error.kind()==std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(_)=>return Err("插件事务目录读取失败".into()),
        };
        let mut removed=Vec::new();
        for entry in entries {
            let entry=entry.map_err(|_|"插件事务条目读取失败")?;
            if !entry.file_type().map_err(|_|"插件事务条目类型读取失败")?.is_file() { continue; }
            let name=entry.file_name().to_string_lossy().to_string();
            let Some(id)=name.strip_prefix("plugin-install-").and_then(|rest|rest.strip_suffix(".json")) else { continue };
            if id.is_empty()||id.contains('/')||id.contains('\\')||id.contains("..") { continue; }
            if registry.entries.contains_key(id) { continue; }
            std::fs::remove_file(entry.path()).map_err(|_|format!("孤儿安装事务清理失败：{name}"))?;
            removed.push(name);
        }
        removed.sort();
        Ok(removed)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest,Sha256};
    // 合成包：id 配官方存储目录时走 OfficialCatalog，否则按第三方来源校验。
    // salt 让「同一版号重打一次」得到不同摘要的包（#t8 的旧摘要目录就是这么来的）。
    fn package_salted(id:&str,version:&str,schema:u32,salt:&str)->(Package,String) {
        package_with(id,version,schema,salt,false)
    }
    // package_with 多带一个 behaviors.autostart：2.0.0 起「有没有本机服务」由插件
    // 清单自己声明，宿主不能替它假定（#f3 的自启动定义清理要靠它做对照）。
    fn package_with(id:&str,version:&str,schema:u32,salt:&str,autostart:bool)->(Package,String) {
        let (name,directory)=match id {
            "fish.ofm"=>("OFM 本地模型",".fishplugin-ourfreemodel"),
            "fish.feishu"=>("飞书通知",".fishplugin-feishu"),
            "fish.clash"=>("Clash 代理",".fishplugin-clash"),
            "fish.magpie"=>("Magpie 网关",".fishplugin-magpie"),
            _=>("合成天气",".plugin.acme.weather"),
        };
        let official=id.starts_with("fish.");
        let payload=format!("synthetic backend {version}{salt}").into_bytes();
        let readme=format!("# 合成插件 {version}{salt}").into_bytes();
        let raw=serde_json::json!({"schema":1,"id":id,"name":name,"version":version,"hostApi":1,"platforms":[platform()],"storage":{"directory":directory,"dataSchema":schema},"backend":{"runtime":"node","entry":"backend.mjs"},"behaviors":{"autostart":autostart},"files":{"backend.mjs":format!("{:x}",Sha256::digest(&payload)),"README.md":format!("{:x}",Sha256::digest(&readme))}});
        let files=BTreeMap::from([("plugin.json".into(),serde_json::to_vec(&raw).unwrap()),("backend.mjs".into(),payload),("README.md".into(),readme)]);
        let bytes=crate::identity_archive::zip(&files).unwrap();
        let digest=format!("{:x}",Sha256::digest(&bytes));
        let source=if official{Source::OfficialCatalog}else{Source::ThirdParty};
        let package=crate::plugin_package::inspect(&bytes,&digest,&std::env::temp_dir().join("fish-install-inspect"),source,&platform()).unwrap();
        (package,digest)
    }
    fn package_of(id:&str,version:&str,schema:u32)->(Package,String) { package_salted(id,version,schema,"") }
    fn bundle(version:&str,schema:u32)->(Package,String) { package_of("acme.weather",version,schema) }
    // tree 把一棵目录树摊成「相对路径」清单（含目录本身），测试里用来贴升级前后对照。
    fn tree(base:&std::path::Path)->Vec<String> {
        fn walk(base:&std::path::Path,path:&std::path::Path,out:&mut Vec<String>) {
            let Ok(entries)=std::fs::read_dir(path) else { return };
            let mut entries:Vec<_>=entries.flatten().collect();
            entries.sort_by_key(|entry|entry.file_name());
            for entry in entries {
                let relative=entry.path().strip_prefix(base).unwrap_or(&entry.path()).to_string_lossy().replace('\\',"/");
                let directory=entry.file_type().is_ok_and(|kind|kind.is_dir());
                out.push(if directory { format!("{relative}/") } else { relative });
                if directory { walk(base,&entry.path(),out); }
            }
        }
        let mut out=Vec::new(); walk(base,base,&mut out); out
    }
    // #f3 回归①：升级的包落地后核验不过（半写、被改动），以前 install 直接把错误
    // 抛出去就完事——packages/ 下留着一个「看起来装好了」的版本目录，注册表里却
    // 只有旧版。用户看到的是「装完就没了」，插件卡上连一句为什么都没有。
    // 现在必须：删掉这一版目录、留一条 notice、注册表仍是旧版、pending 不报它。
    #[test] fn package_that_fails_verification_after_landing_leaves_no_orphan_and_reports_why() {
        let root=std::env::temp_dir().join(format!("fish-verify-fail-{}-{}",std::process::id(),crate::now_ms())).join(".fish-dock");
        fn namespace(root:PathBuf,id:&str,directory:&str)->Namespace {
            crate::storage::initialize(&root,id,"1.0.0",1).unwrap();
            let identity=crate::storage_context::Identity{plugin_id:id.into(),directory:directory.into(),plugin_version:"1.0.0".into(),data_schema:1};
            std::fs::write(root.join("storage.json"),serde_json::to_vec(&identity).unwrap()).unwrap();
            Namespace::existing(&root).unwrap().unwrap()
        }
        let host=namespace(root.join("host"),"host","host");
        namespace(root.join(".plugin.acme.weather"),"acme.weather",".plugin.acme.weather");
        let context=Context{root:root.clone(),host}; let manager=Manager{context:&context};
        let (package,digest)=bundle("1.0.0",1); manager.install(package,&digest,false).unwrap();
        // 包本身合法，但 write 进去的内容与清单摘要对不上（半写/被改动）。
        let (mut package,digest)=bundle("1.1.0",1);
        package.files.insert("README.md".into(),b"tampered after landing".to_vec());
        let error=manager.install(package,&digest,false).expect_err("落地后核验不过的包不许提交");
        assert!(error.contains("README.md"),"错误要说清是哪个资源不对：{error}");
        let base=root.join("host").join("packages").join("acme.weather");
        assert!(!base.join("1.1.0").exists(),"核验失败那一版的落地目录必须删掉，不能留在盘上冒充已安装");
        let active=manager.list().unwrap().remove(0);
        assert_eq!(active.manifest.version,"1.0.0","注册表必须还是旧版");
        manager.verify(&active).unwrap();
        let notice=manager.notice("acme.weather").expect("核验失败也要在插件卡上留一条");
        assert_eq!(notice.version,"1.1.0");
        assert!(notice.reason.contains("README.md"),"{}",notice.reason);
        assert!(manager.pending().is_empty(),"失败的那一版不许被报成「安装未完成」");
        std::fs::remove_dir_all(root.parent().unwrap()).unwrap();
    }
    // #f3 回归②（用户现场的死法）：升级的包已经完整落地、注册表却没写上（写注册表
    // 失败、或进程崩在提交前）。以前这就成了「装完了，但更新拿不到」：插件卡从列表
    // 消失，也没有任何提示。现在要能（a）把这一版报成 pending 让用户看见，
    // （b）再点一次安装是幂等的——复用已落地的包，不再重写，直接把注册补上。
    #[test] fn landed_but_unregistered_package_is_reported_and_retry_registers_it() {
        let root=std::env::temp_dir().join(format!("fish-unregistered-{}-{}",std::process::id(),crate::now_ms())).join(".fish-dock");
        fn namespace(root:PathBuf,id:&str,directory:&str)->Namespace {
            crate::storage::initialize(&root,id,"1.0.0",1).unwrap();
            let identity=crate::storage_context::Identity{plugin_id:id.into(),directory:directory.into(),plugin_version:"1.0.0".into(),data_schema:1};
            std::fs::write(root.join("storage.json"),serde_json::to_vec(&identity).unwrap()).unwrap();
            Namespace::existing(&root).unwrap().unwrap()
        }
        let host=namespace(root.join("host"),"host","host");
        namespace(root.join(".plugin.acme.weather"),"acme.weather",".plugin.acme.weather");
        let context=Context{root:root.clone(),host}; let manager=Manager{context:&context};
        let (package,digest)=bundle("1.0.0",1); manager.install(package,&digest,false).unwrap();
        let (package,digest)=bundle("1.2.0",1); manager.install(package,&digest,false).unwrap();
        // 复刻现场：2.0.0 的包与命名空间都到位了，注册表那一行没有。
        let (package,digest)=bundle("2.0.0",1);
        let row=Installed{manifest:package.manifest.clone(),digest:digest.to_ascii_lowercase(),official:false,enabled:true,autostart:true};
        let directory=manager.directory(&row).unwrap();
        for (name,bytes) in package.files { let path=crate::plugin_manifest::package_path(&directory,&name).unwrap(); crate::plugin_package::write(manager.context.host.path(),&path,&bytes).unwrap(); }
        manager.verify(&row).unwrap();
        let mut registry:serde_json::Value=serde_json::from_slice(&context.host.read("config","installed-plugins.json").unwrap().unwrap()).unwrap();
        registry["entries"].as_object_mut().unwrap().remove("acme.weather");
        context.host.write("config","installed-plugins.json",&serde_json::to_vec(&registry).unwrap()).unwrap();
        assert!(manager.list().unwrap().is_empty());
        // (a) 这一版必须被报出来，前端才有「安装未完成，点重试」可显示。
        let pending=manager.pending();
        assert_eq!(pending.len(),1,"盘上有、注册表里的最高一版要报出来");
        assert_eq!((pending[0].manifest.id.as_str(),pending[0].manifest.version.as_str()),("acme.weather","2.0.0"));
        assert!(pending[0].digest.eq_ignore_ascii_case(&digest));
        // (b) 重试：传进去的文件是坏内容。只有「认出现成落地目录、不再重写」时它才能成功。
        let retry=Package{manifest:pending[0].manifest.clone(),files:BTreeMap::from([("README.md".into(),b"must not be written".to_vec())])};
        let restored=manager.install(retry,&digest,false).expect("已完整落地的版本应直接复用并补登记，不再重写");
        assert_eq!((restored.manifest.version.as_str(),restored.digest.as_str()),("2.0.0",digest.to_ascii_lowercase().as_str()));
        let listed=manager.list().unwrap().remove(0);
        assert_eq!(listed.manifest.version,"2.0.0","重试之后注册表必须是这一版");
        manager.verify(&listed).unwrap();
        assert!(manager.pending().is_empty(),"补上登记之后不再是 pending");
        assert!(manager.notice("acme.weather").is_none(),"装成功要清掉旧提示");
        std::fs::remove_dir_all(root.parent().unwrap()).unwrap();
    }
    #[test] fn install_upgrade_failed_schema_recovery_and_uninstall_keep_user_data() {
        let root=std::env::temp_dir().join(format!("fish-install-{}-{}",std::process::id(),crate::now_ms())).join(".fish-dock");
        // 此用例验证安装事务，ACL 由 identity 的独立集成用例验证。
        fn namespace(root:PathBuf,id:&str,directory:&str)->Namespace {
            crate::storage::initialize(&root,id,"1.0.0",1).unwrap();
            let identity=crate::storage_context::Identity{plugin_id:id.into(),directory:directory.into(),plugin_version:"1.0.0".into(),data_schema:1};
            std::fs::write(root.join("storage.json"),serde_json::to_vec(&identity).unwrap()).unwrap();
            Namespace::existing(&root).unwrap().unwrap()
        }
        let host=namespace(root.join("host"),"host","host");
        namespace(root.join(".plugin.acme.weather"),"acme.weather",".plugin.acme.weather");
        let context=Context{root:root.clone(),host}; let manager=Manager{context:&context};
        let (package,digest)=bundle("1.0.0",1); let first=manager.install(package,&digest,false).unwrap();assert!(first.enabled);
        let ns=manager.namespace(&first).unwrap(); ns.write("keys","fixture.json",b"synthetic-key").unwrap();
        manager.set_enabled("acme.weather",true).unwrap();
        let (package,digest)=bundle("1.1.0",1); let next=manager.install(package,&digest,false).unwrap();
        assert!(next.enabled);assert_eq!(manager.namespace(&next).unwrap().read("keys","fixture.json").unwrap().unwrap(),b"synthetic-key");
        let (package,digest)=bundle("2.0.0",2);assert!(manager.install(package,&digest,false).is_err());
        // 失败的是「数据 schema 换版」这一步，不是包本身：install 已经就地回滚，
        // recover 不该再把这条提交不上去的事务当成待续做的安装（那样只会撞在同一
        // 道守卫上，把之后每一次插件操作都拖死）。
        manager.recover().unwrap();
        let active=manager.list().unwrap().remove(0);assert_eq!(active.manifest.version,"1.1.0");manager.verify(&active).unwrap();
        assert!(std::fs::read_to_string(ns.path().join("README.md")).unwrap().contains("1.1.0"));
        // 锁住「schema 升级安装失败 → recover 后不得把半成品当已提交」：
        // 命名空间还停在 1.1.0/schema 1，半成品包与事务都清掉，插件卡上留了记录。
        let identity:crate::storage_context::Identity=serde_json::from_slice(&std::fs::read(ns.path().join("storage.json")).unwrap()).unwrap();
        assert_eq!((identity.plugin_version.as_str(),identity.data_schema),("1.1.0",1));
        assert!(!root.join("host").join("packages").join("acme.weather").join("2.0.0").join(digest.to_ascii_lowercase()).exists(),"失败那一版的发行包要删掉");
        assert!(!root.join("host").join("tmp").join("plugin-install-acme.weather.json").exists(),"提交不上去的事务不能留下");
        let notice=manager.notice("acme.weather").expect("失败升级要在插件卡上留一条提示");
        assert_eq!((notice.version.as_str(),notice.reason.as_str()),("2.0.0","数据 schema 变化需要专用迁移适配器"));
        // ns 与 list() 说的是同一件事：同一个命名空间目录、同一个摘要目录。
        assert_eq!(manager.namespace(&active).unwrap().path(),ns.path());
        assert!(manager.directory(&active).unwrap().ends_with(&active.digest));
        // 半写资源使校验失败，保留旧包与用户目录供恢复，避免激活受损版本。
        std::fs::write(manager.directory(&active).unwrap().join("backend.mjs"),b"modified").unwrap();
        assert!(manager.set_enabled("acme.weather",true).is_err());
        manager.uninstall("acme.weather").unwrap();assert!(manager.list().unwrap().is_empty());
        assert_eq!(std::fs::read(ns.path().join("keys/fixture.json")).unwrap(),b"synthetic-key");
        std::fs::remove_dir_all(root.parent().unwrap()).unwrap();
    }
    // 升级提交后只留当前版本：旧版本目录一个都不留（回滚点已确认取消）。
    // 留着既占地方，也让"读到旧界面"这种事故有可乘之机。
    #[test] fn prune_versions_keeps_only_the_active_one() {
        let root=std::env::temp_dir().join(format!("fish-prune-{}-{}",std::process::id(),crate::now_ms())).join(".fish-dock");
        fn namespace(root:PathBuf,id:&str,directory:&str)->Namespace {
            crate::storage::initialize(&root,id,"1.0.0",1).unwrap();
            let identity=crate::storage_context::Identity{plugin_id:id.into(),directory:directory.into(),plugin_version:"1.0.0".into(),data_schema:1};
            std::fs::write(root.join("storage.json"),serde_json::to_vec(&identity).unwrap()).unwrap();
            Namespace::existing(&root).unwrap().unwrap()
        }
        let host=namespace(root.join("host"),"host","host");
        namespace(root.join(".plugin.acme.weather"),"acme.weather",".plugin.acme.weather");
        let context=Context{root:root.clone(),host}; let manager=Manager{context:&context};
        // 装了两个旧版本，再装当前版本：两个旧版本目录都要被删掉。
        for version in ["1.0.0","1.1.0","1.2.0"] {
            let (package,digest)=bundle(version,1);
            manager.install(package,&digest,false).unwrap();
        }
        let active=manager.list().unwrap().into_iter().find(|row|row.manifest.id=="acme.weather").unwrap();
        assert_eq!(active.manifest.version,"1.2.0");
        let removed=manager.prune_versions(&active).unwrap();
        assert_eq!(removed,vec!["1.0.0".to_string(),"1.1.0".to_string()]);
        let base=root.join("host").join("packages").join("acme.weather");
        let mut left:Vec<String>=std::fs::read_dir(&base).unwrap()
            .map(|entry|entry.unwrap().file_name().to_string_lossy().to_string()).collect();
        left.sort();
        assert_eq!(left,vec!["1.2.0".to_string()]);
        // 删的是目录本身，不只是名字：旧版本目录必须已经不存在。
        assert!(!base.join("1.0.0").exists());
        assert!(!base.join("1.1.0").exists());
        // 空目录不是错误：没有旧版本时什么都不用删。
        assert!(manager.prune_versions(&active).unwrap().is_empty());
        assert!(base.join("1.2.0").exists());
        std::fs::remove_dir_all(root.parent().unwrap()).unwrap();
    }
    // 三个旧版本目录 + 当前版本：结束后只剩当前版本目录（回滚点取消后不再留一个）。
    #[test] fn prune_versions_removes_every_stale_version_directory() {
        let root=std::env::temp_dir().join(format!("fish-prune-all-{}-{}",std::process::id(),crate::now_ms())).join(".fish-dock");
        fn namespace(root:PathBuf,id:&str,directory:&str)->Namespace {
            crate::storage::initialize(&root,id,"1.0.0",1).unwrap();
            let identity=crate::storage_context::Identity{plugin_id:id.into(),directory:directory.into(),plugin_version:"1.0.0".into(),data_schema:1};
            std::fs::write(root.join("storage.json"),serde_json::to_vec(&identity).unwrap()).unwrap();
            Namespace::existing(&root).unwrap().unwrap()
        }
        let host=namespace(root.join("host"),"host","host");
        namespace(root.join(".plugin.acme.weather"),"acme.weather",".plugin.acme.weather");
        let context=Context{root:root.clone(),host}; let manager=Manager{context:&context};
        for version in ["1.0.0","1.1.0","1.2.0","1.3.0"] {
            let (package,digest)=bundle(version,1);
            manager.install(package,&digest,false).unwrap();
        }
        let base=root.join("host").join("packages").join("acme.weather");
        for version in ["1.0.0","1.1.0","1.2.0","1.3.0"] { assert!(base.join(version).exists(),"装好的 {version} 应该在"); }
        let active=manager.list().unwrap().into_iter().find(|row|row.manifest.id=="acme.weather").unwrap();
        assert_eq!(active.manifest.version,"1.3.0");
        let removed=manager.prune_versions(&active).unwrap();
        assert_eq!(removed,vec!["1.0.0".to_string(),"1.1.0".to_string(),"1.2.0".to_string()]);
        let mut left:Vec<String>=std::fs::read_dir(&base).unwrap()
            .map(|entry|entry.unwrap().file_name().to_string_lossy().to_string()).collect();
        left.sort();
        assert_eq!(left,vec!["1.3.0".to_string()]);
        for version in ["1.0.0","1.1.0","1.2.0"] { assert!(!base.join(version).exists(),"旧版本目录 {version} 应已删除"); }
        assert!(base.join("1.3.0").exists(),"当前版本目录不许被删");
        // 当前版本自身的清单与资源仍可用：只删邻居，没伤到激活版本。
        manager.verify(&active).unwrap();
        std::fs::remove_dir_all(root.parent().unwrap()).unwrap();
    }
    // 真实发行包回归（#f3「安装插件依然会直接安装更新不上」）。
    //
    // 用本机已发布的 catalog（`catalog/stable.json`）与四个 `.fish-plugin`
    // 资产跑真实的安装/更新路径：目录里写的 bytes/sha256 必须就是包本身（A），
    // 更新后注册表要指向新版（B），文件必须真的落地并能 verify（C）。
    // 发行目录用 FISH_PLUGIN_RELEASE_DIR 指过去；没这个环境就跳过，不假装绿。
    #[test] fn published_catalog_packages_land_on_disk_and_replace_the_previous_version() {
        let Some(dist)=std::env::var_os("FISH_PLUGIN_RELEASE_DIR").map(PathBuf::from) else { return; };
        let catalog=crate::plugin_store::parse(&std::fs::read(dist.join("stable.json")).expect("发行目录缺少 stable.json")).expect("已发布的官方目录必须可解析");
        assert_eq!(catalog.schema,1);
        assert!(!catalog.plugins.is_empty(),"已发布的官方目录不能是空的");
        let root=std::env::temp_dir().join(format!("fish-published-{}-{}",std::process::id(),crate::now_ms())).join(".fish-dock");
        fn namespace(root:PathBuf,id:&str,directory:&str)->Namespace {
            crate::storage::initialize(&root,id,"1.0.0",1).unwrap();
            let identity=crate::storage_context::Identity{plugin_id:id.into(),directory:directory.into(),plugin_version:"1.0.0".into(),data_schema:1};
            std::fs::write(root.join("storage.json"),serde_json::to_vec(&identity).unwrap()).unwrap();
            Namespace::existing(&root).unwrap().unwrap()
        }
        let host=namespace(root.join("host"),"host","host");
        let context=Context{root:root.clone(),host}; let manager=Manager{context:&context};
        for entry in &catalog.plugins {
            let slug=entry.id.strip_prefix("fish.").unwrap_or(entry.id.as_str());
            let path=dist.join(format!("{slug}.fish-plugin"));
            let bytes=std::fs::read(&path).unwrap_or_else(|_|panic!("缺少已发布的发行包 {}",path.display()));
            // A：目录里的 bytes/sha256 必须就是真实包；对不上时 install 会以
            // 「插件包 SHA-256 校验失败」告终，用户看到的却是「点了一下没反应」。
            assert_eq!(bytes.len() as u64,entry.bytes,"{} 的 bytes 与官方目录不一致",entry.id);
            assert_eq!(format!("{:x}",Sha256::digest(&bytes)),entry.sha256,"{} 的 sha256 与官方目录不一致",entry.id);
            let package=crate::plugin_package::inspect(&bytes,&entry.sha256,&context.root,Source::OfficialCatalog,&platform()).unwrap_or_else(|error|panic!("{} 的发行包过不了宿主校验：{error}",entry.id));
            assert_eq!((package.manifest.id.as_str(),package.manifest.version.as_str()),(entry.id.as_str(),entry.version.as_str()),"{} 的包身份或版本与目录不一致",entry.id);
            // 先把上一版装上（模拟用户手上已有旧版），再走 update 这条路。
            let (old,old_digest)=package_of(&entry.id,"1.0.0",1);
            let previous=manager.install(old,&old_digest,package.manifest.id.starts_with("fish.")).expect("旧版本应先能装上");
            assert_eq!(previous.manifest.version,"1.0.0");
            // B：更新到目录里的版本。digest 用目录 sha256，与 plugin_store::obtain 一致。
            let updated=manager.install(package,&entry.sha256,true).unwrap_or_else(|error|panic!("{} 更新到 {} 失败：{error}",entry.id,entry.version));
            assert_eq!(updated.manifest.version,entry.version,"{} 更新后注册表版本必须是目录版本",entry.id);
            assert_eq!(updated.digest,entry.sha256.to_ascii_lowercase(),"{} 的注册摘要必须是目录摘要",entry.id);
            // C：不只是注册表换了——文件真落地、能逐个核验，列表也读得到新版。
            let directory=manager.directory(&updated).unwrap();
            assert!(directory.join("plugin.json").is_file(),"{} 的发行文件必须落到 packages/ 下",entry.id);
            manager.verify(&updated).unwrap_or_else(|error|panic!("{} 落地后核验失败：{error}",entry.id));
            let listed=manager.list().unwrap().into_iter().find(|row|row.manifest.id==entry.id).expect("更新后列表里应有这个插件");
            assert_eq!((listed.manifest.version.as_str(),listed.digest.as_str()),(entry.version.as_str(),entry.sha256.to_ascii_lowercase().as_str()),"{} 的列表结果必须就是新版",entry.id);
            // 升级后旧版本目录清掉，当前版本目录还在。
            let pruned=manager.prune_versions(&updated).unwrap();
            assert!(pruned.contains(&"1.0.0".to_string()),"{} 的旧版本目录应被清理：{pruned:?}",entry.id);
            assert!(!root.join("host").join("packages").join(&entry.id).join("1.0.0").exists());
            assert!(directory.exists(),"{} 的当前版本目录不许被清理",entry.id);
        }
        std::fs::remove_dir_all(root.parent().unwrap()).unwrap();
    }
    // 事务写完、注册表还没切换时进程没了（#21 的现场）。以前重启只会把事务删掉，
    // 安装凭空消失；现在要接着提交，用户不用重装。
    #[test] fn interrupted_install_is_committed_on_the_next_start() {
        let root=std::env::temp_dir().join(format!("fish-resume-{}-{}",std::process::id(),crate::now_ms())).join(".fish-dock");
        fn namespace(root:PathBuf,id:&str,directory:&str)->Namespace {
            crate::storage::initialize(&root,id,"1.0.0",1).unwrap();
            let identity=crate::storage_context::Identity{plugin_id:id.into(),directory:directory.into(),plugin_version:"1.0.0".into(),data_schema:1};
            std::fs::write(root.join("storage.json"),serde_json::to_vec(&identity).unwrap()).unwrap();
            Namespace::existing(&root).unwrap().unwrap()
        }
        let host=namespace(root.join("host"),"host","host");
        namespace(root.join(".plugin.acme.weather"),"acme.weather",".plugin.acme.weather");
        let context=Context{root:root.clone(),host}; let manager=Manager{context:&context};
        let (package,digest)=bundle("1.0.0",1); manager.install(package,&digest,false).unwrap();
        // 手工重放 install() 的前半段：写资源、核验、写事务，最后一步提交不做。
        let (package,digest)=bundle("1.1.0",1);
        let row=Installed{manifest:package.manifest.clone(),digest:digest.to_ascii_lowercase(),official:false,enabled:true,autostart:true};
        let directory=manager.directory(&row).unwrap();
        for (name,bytes) in package.files { let path=crate::plugin_manifest::package_path(&directory,&name).unwrap(); crate::plugin_package::write(manager.context.host.path(),&path,&bytes).unwrap(); }
        manager.verify(&row).unwrap();
        let journal=serde_json::json!({"schema":1,"previous":null,"next":row});
        manager.context.host.write("tmp","plugin-install-acme.weather.json",&serde_json::to_vec(&journal).unwrap()).unwrap();
        assert_eq!(manager.list().unwrap().remove(0).manifest.version,"1.0.0");
        // 重启：续做提交。
        manager.recover().unwrap();
        let active=manager.list().unwrap().remove(0);
        assert_eq!(active.manifest.version,"1.1.0");
        manager.verify(&active).unwrap();
        assert!(!manager.context.host.path().join("tmp").join("plugin-install-acme.weather.json").exists());
        assert!(manager.notice("acme.weather").is_none());
        std::fs::remove_dir_all(root.parent().unwrap()).unwrap();
    }
    // 包不完整（半写、被改动）：不许把坏包提交上去，而要回滚并在插件卡上说清楚
    // 「这一版没装成、改用 npm 装」（#21）。
    #[test] fn interrupted_install_that_cannot_verify_rolls_back_with_a_notice() {
        let root=std::env::temp_dir().join(format!("fish-rollback-{}-{}",std::process::id(),crate::now_ms())).join(".fish-dock");
        fn namespace(root:PathBuf,id:&str,directory:&str)->Namespace {
            crate::storage::initialize(&root,id,"1.0.0",1).unwrap();
            let identity=crate::storage_context::Identity{plugin_id:id.into(),directory:directory.into(),plugin_version:"1.0.0".into(),data_schema:1};
            std::fs::write(root.join("storage.json"),serde_json::to_vec(&identity).unwrap()).unwrap();
            Namespace::existing(&root).unwrap().unwrap()
        }
        let host=namespace(root.join("host"),"host","host");
        namespace(root.join(".plugin.acme.weather"),"acme.weather",".plugin.acme.weather");
        let context=Context{root:root.clone(),host}; let manager=Manager{context:&context};
        let (package,digest)=bundle("1.0.0",1); manager.install(package,&digest,false).unwrap();
        let (package,digest)=bundle("1.1.0",1);
        let row=Installed{manifest:package.manifest.clone(),digest:digest.to_ascii_lowercase(),official:false,enabled:true,autostart:true};
        let directory=manager.directory(&row).unwrap();
        // 只落 plugin.json，backend.mjs 与 README.md 没写：verify 必然失败。
        let path=crate::plugin_manifest::package_path(&directory,"plugin.json").unwrap();
        crate::plugin_package::write(manager.context.host.path(),&path,&serde_json::to_vec(&row.manifest).unwrap()).unwrap();
        let journal=serde_json::json!({"schema":1,"previous":null,"next":row});
        manager.context.host.write("tmp","plugin-install-acme.weather.json",&serde_json::to_vec(&journal).unwrap()).unwrap();
        manager.recover().unwrap();
        assert_eq!(manager.list().unwrap().remove(0).manifest.version,"1.0.0");
        let notice=manager.notice("acme.weather").expect("回滚要留下一条提示");
        assert_eq!(notice.version,"1.1.0");
        assert!(notice.hint.contains("@fish-under-sea/fish-dock-plugin-weather"),"{}",notice.hint);
        assert!(!directory.exists(),"回滚要删掉这一版的目录");
        assert!(!manager.context.host.path().join("tmp").join("plugin-install-acme.weather.json").exists());
        // 装成功之后这条提示要自动消失。
        let (package,digest)=bundle("1.2.0",1); manager.install(package,&digest,false).unwrap();
        assert!(manager.notice("acme.weather").is_none());
        std::fs::remove_dir_all(root.parent().unwrap()).unwrap();
    }
    // #t8 回归①（用户现场）：升级之后 packages/<id>/ 下只剩当前版本、当前摘要，
    // 旧版本目录、同版号重打包留下的旧摘要目录、写包中途留下的 .install-* 暂存
    // 文件都不许留；当前版本仍可 verify，用户 keys 一个字节都不许丢。
    #[test] fn upgrade_leaves_only_the_current_version_and_current_digest() {
        let root=std::env::temp_dir().join(format!("fish-prune-digest-{}-{}",std::process::id(),crate::now_ms())).join(".fish-dock");
        fn namespace(root:PathBuf,id:&str,directory:&str)->Namespace {
            crate::storage::initialize(&root,id,"1.0.0",1).unwrap();
            let identity=crate::storage_context::Identity{plugin_id:id.into(),directory:directory.into(),plugin_version:"1.0.0".into(),data_schema:1};
            std::fs::write(root.join("storage.json"),serde_json::to_vec(&identity).unwrap()).unwrap();
            Namespace::existing(&root).unwrap().unwrap()
        }
        let host=namespace(root.join("host"),"host","host");
        namespace(root.join(".plugin.acme.weather"),"acme.weather",".plugin.acme.weather");
        let context=Context{root:root.clone(),host}; let manager=Manager{context:&context};
        let (package,digest)=bundle("1.0.0",1); let first=manager.install(package,&digest,false).unwrap();
        manager.namespace(&first).unwrap().write("keys","fixture.json",b"synthetic-key").unwrap();
        // 同版号重打包：版本号一样，摘要不一样（真实的 2.0.0 就是这么重打出来的）。
        let (rebuilt,rebuilt_digest)=package_salted("acme.weather","2.0.0",1," rebuilt");
        let active=manager.install(rebuilt,&rebuilt_digest,false).unwrap();
        let base=root.join("host").join("packages").join("acme.weather");
        // 清理前：两个版本目录 + 当前版本里两个摘要目录 + 一个暂存文件。
        std::fs::create_dir_all(base.join("2.0.0").join("f".repeat(64))).unwrap();
        std::fs::write(base.join("2.0.0").join(".install-4242-1793"),b"half written").unwrap();
        assert!(base.join("1.0.0").exists()&&base.join("2.0.0").join("f".repeat(64)).exists());
        println!("升级前 packages/acme.weather：\n  {}",tree(&base).join("\n  "));
        let problems=manager.cleanup(&active);
        assert!(problems.is_empty(),"正常的清理不该报错：{problems:?}");
        println!("升级后 packages/acme.weather：\n  {}",tree(&base).join("\n  "));
        // 升级后：只剩当前版本、当前摘要。
        let mut versions:Vec<String>=std::fs::read_dir(&base).unwrap().map(|entry|entry.unwrap().file_name().to_string_lossy().to_string()).collect();
        versions.sort();
        assert_eq!(versions,vec!["2.0.0".to_string()],"旧版本目录必须删掉");
        let mut digests:Vec<String>=std::fs::read_dir(base.join("2.0.0")).unwrap().map(|entry|entry.unwrap().file_name().to_string_lossy().to_string()).collect();
        digests.sort();
        assert_eq!(digests,vec![active.digest.clone()],"同版号的旧摘要目录与暂存文件都必须删掉");
        // 当前版本照样可核验、用户数据还在。
        manager.verify(&active).unwrap();
        assert_eq!(manager.namespace(&active).unwrap().read("keys","fixture.json").unwrap().unwrap(),b"synthetic-key");
        assert!(!root.join("host").join("tmp").join("plugin-install-acme.weather.json").exists(),"装成功后不许留安装事务");
        std::fs::remove_dir_all(root.parent().unwrap()).unwrap();
    }
    // #t8 回归②：停用的插件不许被清理删掉当前版本（清理只认「比当前版本旧的东西」）。
    #[test] fn cleanup_keeps_the_current_version_of_a_disabled_plugin() {
        let root=std::env::temp_dir().join(format!("fish-prune-disabled-{}-{}",std::process::id(),crate::now_ms())).join(".fish-dock");
        fn namespace(root:PathBuf,id:&str,directory:&str)->Namespace {
            crate::storage::initialize(&root,id,"1.0.0",1).unwrap();
            let identity=crate::storage_context::Identity{plugin_id:id.into(),directory:directory.into(),plugin_version:"1.0.0".into(),data_schema:1};
            std::fs::write(root.join("storage.json"),serde_json::to_vec(&identity).unwrap()).unwrap();
            Namespace::existing(&root).unwrap().unwrap()
        }
        let host=namespace(root.join("host"),"host","host");
        namespace(root.join(".plugin.acme.weather"),"acme.weather",".plugin.acme.weather");
        let context=Context{root:root.clone(),host}; let manager=Manager{context:&context};
        for version in ["1.0.0","2.0.0"] { let (package,digest)=bundle(version,1); manager.install(package,&digest,false).unwrap(); }
        manager.set_enabled("acme.weather",false).unwrap();
        let active=manager.list().unwrap().remove(0);
        assert!(!active.enabled);
        assert!(manager.cleanup(&active).is_empty());
        let directory=manager.directory(&active).unwrap();
        assert!(directory.join("plugin.json").is_file(),"停用的插件当前版本不许被清理删掉");
        manager.verify(&active).unwrap();
        assert!(!root.join("host").join("packages").join("acme.weather").join("1.0.0").exists());
        std::fs::remove_dir_all(root.parent().unwrap()).unwrap();
    }
    // #t8 回归③：卸载要连发行文件一起走（几十 MB 的包不许永远留在盘上），但用户数据
    // 一个字节都不碰；卸完之后也不许被 pending 报成「安装未完成」。
    #[test] fn uninstall_removes_the_release_payload_but_keeps_user_data() {
        let root=std::env::temp_dir().join(format!("fish-uninstall-payload-{}-{}",std::process::id(),crate::now_ms())).join(".fish-dock");
        fn namespace(root:PathBuf,id:&str,directory:&str)->Namespace {
            crate::storage::initialize(&root,id,"1.0.0",1).unwrap();
            let identity=crate::storage_context::Identity{plugin_id:id.into(),directory:directory.into(),plugin_version:"1.0.0".into(),data_schema:1};
            std::fs::write(root.join("storage.json"),serde_json::to_vec(&identity).unwrap()).unwrap();
            Namespace::existing(&root).unwrap().unwrap()
        }
        let host=namespace(root.join("host"),"host","host");
        let plugin=namespace(root.join(".plugin.acme.weather"),"acme.weather",".plugin.acme.weather");
        let context=Context{root:root.clone(),host}; let manager=Manager{context:&context};
        let (package,digest)=bundle("2.0.0",1); manager.install(package,&digest,false).unwrap();
        plugin.write("keys","fixture.json",b"synthetic-key").unwrap();
        let base=root.join("host").join("packages").join("acme.weather");
        assert!(base.join("2.0.0").exists());
        manager.uninstall("acme.weather").unwrap();
        assert!(!base.exists(),"卸载要把发行文件一起删掉：几十 MB 的包不该永远留在盘上");
        assert_eq!(std::fs::read(plugin.path().join("keys/fixture.json")).unwrap(),b"synthetic-key","用户 keys 不许被卸载删掉");
        assert!(manager.list().unwrap().is_empty());
        assert!(manager.pending().is_empty(),"用户主动卸载的东西不许被报成「安装未完成」");
        assert!(manager.notice("acme.weather").is_none());
        std::fs::remove_dir_all(root.parent().unwrap()).unwrap();
    }
    // #t8 回归④：回滚点只增不减是本机现场（7 次升级留了 7 个 before-update zip + 11 个
    // version.json）。每类只留最新一个，用户自己放进 backups 的东西一律不碰。
    #[test] fn cleanup_bounds_rollback_points_and_never_touches_user_files() {
        let root=std::env::temp_dir().join(format!("fish-prune-backups-{}-{}",std::process::id(),crate::now_ms())).join(".fish-dock");
        fn namespace(root:PathBuf,id:&str,directory:&str)->Namespace {
            crate::storage::initialize(&root,id,"1.0.0",1).unwrap();
            let identity=crate::storage_context::Identity{plugin_id:id.into(),directory:directory.into(),plugin_version:"1.0.0".into(),data_schema:1};
            std::fs::write(root.join("storage.json"),serde_json::to_vec(&identity).unwrap()).unwrap();
            Namespace::existing(&root).unwrap().unwrap()
        }
        let host=namespace(root.join("host"),"host","host");
        let plugin=namespace(root.join(".plugin.acme.weather"),"acme.weather",".plugin.acme.weather");
        let context=Context{root:root.clone(),host}; let manager=Manager{context:&context};
        let (package,digest)=bundle("2.0.0",1); let active=manager.install(package,&digest,false).unwrap();
        let backups=plugin.path().join("backups");
        std::fs::create_dir_all(&backups).unwrap();
        for stamp in ["1000","2000","3000"] {
            std::fs::write(backups.join(format!("before-update-{stamp}.zip")),b"rollback-point").unwrap();
            std::fs::write(backups.join(format!("version-{stamp}.json")),b"version-journal").unwrap();
        }
        std::fs::write(backups.join("mine.zip"),b"user's own copy").unwrap();
        assert!(manager.cleanup(&active).is_empty());
        let mut left:Vec<String>=std::fs::read_dir(&backups).unwrap().map(|entry|entry.unwrap().file_name().to_string_lossy().to_string()).collect();
        left.sort();
        assert_eq!(left,vec!["README.md".to_string(),"before-update-3000.zip".to_string(),"mine.zip".to_string(),"version-3000.json".to_string()],"每类回滚点只留最新一个，用户自己的文件与宿主 README 不动");
        manager.verify(&active).unwrap();
        std::fs::remove_dir_all(root.parent().unwrap()).unwrap();
    }
    // #t8 回归⑤：清理失败不许回滚升级、不许卡启动——只记日志 + 一条 cleanup 警告。
    #[test] fn cleanup_failure_is_reported_as_a_warning_and_never_touches_the_active_version() {
        let root=std::env::temp_dir().join(format!("fish-prune-fail-{}-{}",std::process::id(),crate::now_ms())).join(".fish-dock");
        fn namespace(root:PathBuf,id:&str,directory:&str)->Namespace {
            crate::storage::initialize(&root,id,"1.0.0",1).unwrap();
            let identity=crate::storage_context::Identity{plugin_id:id.into(),directory:directory.into(),plugin_version:"1.0.0".into(),data_schema:1};
            std::fs::write(root.join("storage.json"),serde_json::to_vec(&identity).unwrap()).unwrap();
            Namespace::existing(&root).unwrap().unwrap()
        }
        let host=namespace(root.join("host"),"host","host");
        let plugin=namespace(root.join(".plugin.acme.weather"),"acme.weather",".plugin.acme.weather");
        let context=Context{root:root.clone(),host}; let manager=Manager{context:&context};
        let (package,digest)=bundle("2.0.0",1); let active=manager.install(package,&digest,false).unwrap();
        // 把 backups 换成一个普通文件：清理一读目录就失败，用来复刻「清理出错」。
        // （用权限造失败在 Windows 与 Unix 上行为不一致，这里要的是必然失败。）
        std::fs::remove_dir_all(plugin.path().join("backups")).unwrap();
        std::fs::write(plugin.path().join("backups"),b"not a directory").unwrap();
        let problems=manager.cleanup(&active);
        assert!(!problems.is_empty(),"清理失败要如实报出来");
        // 升级本身一点没退：注册表、落地目录、核验都正常。
        assert_eq!(manager.list().unwrap().remove(0).manifest.version,"2.0.0");
        manager.verify(&active).unwrap();
        assert!(manager.directory(&active).unwrap().join("plugin.json").is_file());
        // 只留一条警告，而且不是「安装未完成」。
        let notice=manager.notice("acme.weather").expect("清理失败要在插件卡上留一条警告");
        assert!(notice.cleanup,"cleanup 失败必须标成警告而不是安装失败");
        assert_eq!(notice.version,"2.0.0");
        assert!(notice.reason.contains("插件备份目录读取失败"),"{}",notice.reason);
        std::fs::remove_dir_all(root.parent().unwrap()).unwrap();
    }
    // 一处装好宿主命名空间 + 一个插件命名空间的夹具。旧用例各自内联了同一段准备
    // 代码，新用例不再复制第 10 遍；以后收编旧的也只改这一处。
    fn installed_fixture(tag:&str,id:&str,directory:&str)->(PathBuf,Context) {
        fn namespace(root:PathBuf,id:&str,directory:&str)->Namespace {
            crate::storage::initialize(&root,id,"1.0.0",1).unwrap();
            let identity=crate::storage_context::Identity{plugin_id:id.into(),directory:directory.into(),plugin_version:"1.0.0".into(),data_schema:1};
            std::fs::write(root.join("storage.json"),serde_json::to_vec(&identity).unwrap()).unwrap();
            Namespace::existing(&root).unwrap().unwrap()
        }
        let root=std::env::temp_dir().join(format!("fish-{tag}-{}-{}",std::process::id(),crate::now_ms())).join(".fish-dock");
        let host=namespace(root.join("host"),"host","host");
        namespace(root.join(directory),id,directory);
        (root.clone(),Context{root,host})
    }
    // 目录树里的文件总数（不含目录），用来把「旧文件数为 0」钉成可数的断言。
    fn file_count(base:&std::path::Path)->usize { tree(base).iter().filter(|row|!row.ends_with('/')).count() }
    // #f3 回归⑥（用户现场「安装插件依然会直接安装更新不上」）：同一个版本再点一次
    // 安装。以前每点一次都从头落地一遍包；现在必须是幂等的：注册表一条、版本目录
    // 一个、摘要目录一个、文件数不变、装完不留安装事务。
    #[test] fn reinstalling_the_same_version_is_idempotent() {
        let (root,context)=installed_fixture("reinstall","acme.weather",".plugin.acme.weather");
        let manager=Manager{context:&context};
        let (package,digest)=bundle("2.0.0",1); manager.install(package,&digest,false).unwrap();
        let base=root.join("host").join("packages").join("acme.weather");
        let first=file_count(&base);
        println!("第一次安装后 packages/acme.weather：\n  {}",tree(&base).join("\n  "));
        // 用户又点了一次「安装」：同样的版本、同样的摘要。
        let (again,_)=bundle("2.0.0",1);
        let row=manager.install(again,&digest,false).expect("同版本重装不许报错");
        let second=file_count(&base);
        println!("重装同版本后 packages/acme.weather：\n  {}",tree(&base).join("\n  "));
        assert_eq!((row.manifest.version.as_str(),row.digest.as_str()),("2.0.0",digest.as_str()));
        assert_eq!(first,second,"重装不许叠加文件（{first} → {second}）");
        assert_eq!(manager.list().unwrap().len(),1,"注册表里只能有一条");
        let mut versions:Vec<String>=std::fs::read_dir(&base).unwrap().map(|entry|entry.unwrap().file_name().to_string_lossy().to_string()).collect();
        versions.sort();
        assert_eq!(versions,vec!["2.0.0".to_string()],"重装不许叠出一个版本目录");
        let mut digests:Vec<String>=std::fs::read_dir(base.join("2.0.0")).unwrap().map(|entry|entry.unwrap().file_name().to_string_lossy().to_string()).collect();
        digests.sort();
        assert_eq!(digests,vec![digest.clone()],"重装不许叠出一个摘要目录");
        assert!(!root.join("host").join("tmp").join("plugin-install-acme.weather.json").exists(),"装成功不许留安装事务");
        manager.verify(&row).unwrap();
        std::fs::remove_dir_all(root.parent().unwrap()).unwrap();
    }
    // #f3 回归⑦：升级时把旧版本留下的 behaviors 定义一起清掉。2.0.0 起飞书这类没有
    // 本机服务的插件不再声明 behaviors.autostart，而旧注册表里还留着 true —— 那会
    // 继续把 FISH_PLUGIN_AUTOSTART=1 传给一个不认它的插件，卡上也还画着那颗开关。
    #[test] fn upgrading_drops_the_autostart_definition_the_old_version_left() {
        let (root,context)=installed_fixture("autostart","fish.feishu",".fishplugin-feishu");
        let manager=Manager{context:&context};
        let (package,digest)=package_with("fish.feishu","1.0.0",1,"",true);
        let first=manager.install(package,&digest,true).unwrap();
        assert!(first.autostart,"新装时声明了钩子、又没有上一版，按开处理");
        manager.set_autostart("fish.feishu",true).unwrap();
        let (package,digest)=package_with("fish.feishu","2.0.0",1,"",false);
        let next=manager.install(package,&digest,true).unwrap();
        assert!(!next.autostart,"新版本不声明钩子时，旧版本留下的自启动定义必须被清掉");
        assert!(!manager.list().unwrap().remove(0).autostart,"注册表里也不许继续留 true");
        // 从零装一个没有本机服务的插件：同样按关处理，不再默认给 true。
        let (package,digest)=package_with("fish.feishu","2.0.0",1,"fresh",false);
        let fresh=manager.install(package,&digest,true).unwrap();
        assert!(!fresh.autostart,"清单没声明钩子时，自启动必须是关");
        std::fs::remove_dir_all(root.parent().unwrap()).unwrap();
    }
    // #f3 回归⑧：启动时那一轮清理必须把与「哪个插件在升级」无关的残留也收掉：
    // tmp/txn-*、指向已卸载插件的安装事务、过期下载来源记录。以前 cleanup_staging
    // 只有定义、没人调用，这些文件一次都没被删过。
    #[test] fn startup_cleanup_removes_staging_and_orphan_definitions() {
        let (root,context)=installed_fixture("staging","acme.weather",".plugin.acme.weather");
        let manager=Manager{context:&context};
        let (package,digest)=bundle("2.0.0",1); manager.install(package,&digest,false).unwrap();
        let host_tmp=root.join("host").join("tmp");
        std::fs::create_dir_all(host_tmp.join("txn-4242-1793-0").join("config")).unwrap();
        std::fs::write(host_tmp.join("txn-4242-1793-0").join("config").join("staged.json"),b"staged").unwrap();
        std::fs::write(host_tmp.join("plugin-install-acme.weather.json"),b"{\"schema\":1}").unwrap();
        std::fs::write(host_tmp.join("plugin-install-gone.plugin.json"),b"{\"schema\":1}").unwrap();
        let plugin_tmp=context.root.join(".plugin.acme.weather").join("tmp");
        std::fs::create_dir_all(plugin_tmp.join("txn-1-2-3")).unwrap();
        let mut records=crate::plugin_store::fetch_records(&context);
        let attempt=crate::plugin_store::Attempt{origin:crate::plugin_store::Origin::NpmRegistry,url:"https://registry.npmjs.org/fixture".into(),ok:true,elapsed_ms:1,error:None};
        for (id,version) in [("acme.weather","2.0.0"),("gone.plugin","1.0.0")] {
            records.insert(id.into(),crate::plugin_store::FetchRecord{id:id.into(),version:version.into(),origin:crate::plugin_store::Origin::NpmRegistry,url:"https://registry.npmjs.org/fixture".into(),elapsed_ms:1,at:1,attempts:vec![attempt.clone()]});
        }
        crate::plugin_store::save_records(&context,&records).unwrap();
        let problems=manager.cleanup_staging();
        assert!(problems.is_empty(),"正常的清理不该报错：{problems:?}");
        assert!(!host_tmp.join("txn-4242-1793-0").exists(),"宿主的 tmp/txn-* 必须清掉");
        assert!(!plugin_tmp.join("txn-1-2-3").exists(),"插件命名空间里的 tmp/txn-* 同样要清");
        assert!(!host_tmp.join("plugin-install-gone.plugin.json").exists(),"指向已卸载插件的事务要删");
        assert!(host_tmp.join("plugin-install-acme.weather.json").exists(),"在装插件的事务不许动");
        let left=crate::plugin_store::fetch_records(&context);
        assert!(left.contains_key("acme.weather")&&!left.contains_key("gone.plugin"),"过期来源记录要删，在册的要留：{left:?}");
        std::fs::remove_dir_all(root.parent().unwrap()).unwrap();
    }
    // #f3 回归⑨：升级之后 packages/<id>/ 下「旧版本文件数」必须是 0，而且当前版本
    // 仍能逐个核验。数的是文件，不是目录名 —— 目录删了、文件还在同一条判据里抓不住。
    #[test] fn upgrade_leaves_zero_files_of_the_previous_version() {
        let (root,context)=installed_fixture("zero","acme.weather",".plugin.acme.weather");
        let manager=Manager{context:&context};
        let (package,digest)=bundle("1.0.0",1); let old=manager.install(package,&digest,false).unwrap();
        manager.namespace(&old).unwrap().write("keys","fixture.json",b"synthetic-key").unwrap();
        let (package,digest)=bundle("2.0.0",1); let active=manager.install(package,&digest,false).unwrap();
        assert!(manager.cleanup(&active).is_empty(),"正常升级的清理不该报错");
        let base=root.join("host").join("packages").join("acme.weather");
        let files=tree(&base);
        println!("升级后 packages/acme.weather：\n  {}",files.join("\n  "));
        let current=format!("2.0.0/{}/",active.digest);
        let outside:Vec<&String>=files.iter().filter(|path|path.as_str()!="2.0.0/"&&!path.starts_with(&current)).collect();
        assert!(outside.is_empty(),"更新后旧版本文件数必须为 0，实际残留：{outside:?}");
        assert!(!files.iter().any(|path|path.starts_with("1.0.0")),"旧版本目录一个文件都不许留");
        assert_eq!(file_count(&base),3,"当前版本只有 plugin.json/backend.mjs/README.md 三个文件");
        manager.verify(&active).unwrap();
        assert_eq!(manager.namespace(&active).unwrap().read("keys","fixture.json").unwrap().unwrap(),b"synthetic-key","清理不许碰用户数据");
        std::fs::remove_dir_all(root.parent().unwrap()).unwrap();
    }
}

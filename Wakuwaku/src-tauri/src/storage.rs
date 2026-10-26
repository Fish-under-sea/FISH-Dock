//! 宿主统一存储契约：动态根、受控命名空间及随版本生成的目录说明。
use std::{fs, path::{Path, PathBuf}};

pub const CATEGORIES: &[&str] = &["credentials", "roster", "keys", "config", "exports", "backups", "tmp"];
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source { OfficialCatalog, ThirdParty }

pub fn startup_root(home: &Path, args: &[String], environment: Option<&Path>) -> Result<PathBuf, String> {
    let positions: Vec<_> = args.iter().enumerate().filter(|(_, value)| value.as_str() == "--data-root").map(|(index, _)| index).collect();
    if positions.len() > 1 { return Err("--data-root 只能指定一次".into()); }
    let setting = positions.first().map(|index| {
        args.get(index + 1).filter(|s| !s.is_empty() && !s.starts_with("--")).map(Path::new).ok_or("--data-root 缺少完整路径")
    }).transpose()?;
    resolve_root(home, setting, environment)
}
/// 路径采用跨平台统一规则，额外排除 Windows 设备名和备用数据流。
pub fn category_file(root: &Path, category: &str, relative: &str) -> Result<PathBuf, String> {
    if !root.is_absolute() || !CATEGORIES.contains(&category) || relative.is_empty() || relative.len() > 512 || relative.contains('\\') {
        return Err("存储类别或相对路径无效".into());
    }
    for component in relative.split('/') {
        let stem = component.split('.').next().unwrap_or("").to_ascii_uppercase();
        let device = ["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"].contains(&stem.as_str())
            || ["COM", "LPT"].iter().any(|prefix| stem.strip_prefix(prefix).is_some_and(|n| n.len() == 1 && matches!(n.as_bytes()[0], b'1'..=b'9')));
        if component.is_empty() || component == "." || component == ".." || component.ends_with(['.', ' ']) || component.chars().any(|c| c.is_control() || "<>:\"|?*".contains(c)) || device || component.eq_ignore_ascii_case("README.md") {
            return Err("存储路径包含保留名称或越界片段".into());
        }
    }
    let path = root.join(category).join(relative);
    safe(&path)?;
    Ok(path)
}

#[cfg(windows)]
pub fn profile_directory() -> Result<PathBuf, String> {
    use std::os::windows::ffi::OsStringExt;
    #[repr(C)] struct Guid { a: u32, b: u16, c: u16, d: [u8; 8] }
    const PROFILE: Guid = Guid { a: 0x5e6c858f, b: 0x0e22, c: 0x4760, d: [0x9a, 0xfe, 0xea, 0x33, 0x17, 0xb6, 0x71, 0x73] };
    #[link(name = "shell32")] unsafe extern "system" { fn SHGetKnownFolderPath(id: *const Guid, flags: u32, token: isize, path: *mut *mut u16) -> i32; }
    #[link(name = "ole32")] unsafe extern "system" { fn CoTaskMemFree(memory: *mut std::ffi::c_void); }
    let mut path = std::ptr::null_mut();
    if unsafe { SHGetKnownFolderPath(&PROFILE, 0, 0, &mut path) } < 0 || path.is_null() { return Err("系统用户目录查询失败".into()); }
    let mut size = 0usize;
    unsafe { while size < 32768 && *path.add(size) != 0 { size += 1; } }
    let value = if size < 32768 { Some(std::ffi::OsString::from_wide(unsafe { std::slice::from_raw_parts(path, size) })) } else { None };
    unsafe { CoTaskMemFree(path.cast()); }
    value.map(PathBuf::from).filter(|p| p.is_absolute()).ok_or("系统用户目录无效".into())
}
#[cfg(not(windows))]
pub fn profile_directory() -> Result<PathBuf, String> {
    std::env::var_os("HOME").map(PathBuf::from).filter(|p| p.is_absolute()).ok_or("系统用户目录无效".into())
}

pub fn resolve_root(home: &Path, setting: Option<&Path>, environment: Option<&Path>) -> Result<PathBuf, String> {
    if !home.is_absolute() { return Err("用户目录必须是绝对路径".into()); }
    let root = setting.or(environment).map(Path::to_path_buf).unwrap_or_else(|| home.join(".fish-dock"));
    if !root.is_absolute() || root.components().any(|c| matches!(c, std::path::Component::ParentDir | std::path::Component::CurDir)) {
        return Err("数据根必须是无上级跳转的绝对路径".into());
    }
    safe(&root)?;
    Ok(root)
}
fn slug(value: &str) -> bool {
    !value.is_empty() && value.len() <= 64 && value.split('-').all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()))
}
pub fn namespace(root: &Path, plugin_id: &str, directory: &str, source: Source) -> Result<PathBuf, String> {
    if !root.is_absolute() { return Err("插件数据根必须是绝对路径".into()); }
    let expected = match source {
        Source::OfficialCatalog => match plugin_id {
            "fish.ofm" => ".fishplugin-ourfreemodel".to_owned(),
            "fish.feishu" => ".fishplugin-feishu".to_owned(),
            "fish.clash" => ".fishplugin-clash".to_owned(),
            "fish.magpie" => ".fishplugin-magpie".to_owned(),
            _ => return Err(format!("{plugin_id} 未在官方存储目录映射中登记")),
        },
        Source::ThirdParty => {
            let (publisher, name) = plugin_id.split_once('.').ok_or("第三方插件 ID 必须为 publisher.name")?;
            if publisher == "fish" || !slug(publisher) || !slug(name) { return Err("第三方插件 ID 无效或使用了保留发布者 fish".into()); }
            format!(".plugin.{publisher}.{name}")
        }
    };
    if directory != expected { return Err(format!("{plugin_id} 的存储目录必须为 {expected}")); }
    let target = root.join(directory);
    safe(&target)?;
    // Windows 不区分大小写；其他平台也禁止同一安装跨平台迁移后产生归属碰撞。
    if root.exists() {
        for entry in fs::read_dir(root).map_err(|_| "数据根目录读取失败")? {
            let name = entry.map_err(|_| "数据根条目读取失败")?.file_name();
            let name = name.to_string_lossy();
            if name.eq_ignore_ascii_case(directory) && name != directory { return Err("插件存储目录存在大小写冲突".into()); }
        }
    }
    Ok(target)
}
pub fn safe(path: &Path) -> Result<(), String> {
    if path.components().any(|c| matches!(c, std::path::Component::ParentDir)) { return Err("存储路径含上级跳转".into()); }
    for ancestor in path.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(meta) => {
                let mut linked = meta.file_type().is_symlink();
                #[cfg(windows)] { use std::os::windows::fs::MetadataExt; linked |= meta.file_attributes() & 0x400 != 0; }
                if linked { return Err("存储路径包含符号链接或目录联接".into()); }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err("存储路径检查失败".into()),
        }
    }
    Ok(())
}
#[cfg(windows)]
pub(crate) fn replace(from: &Path, to: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    unsafe extern "system" { fn MoveFileExW(from: *const u16, to: *const u16, flags: u32) -> i32; }
    let from: Vec<u16> = from.as_os_str().encode_wide().chain(Some(0)).collect();
    let to: Vec<u16> = to.as_os_str().encode_wide().chain(Some(0)).collect();
    if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), 0x1 | 0x8) } == 0 { Err(std::io::Error::last_os_error()) } else { Ok(()) }
}
#[cfg(not(windows))]
pub(crate) fn replace(from: &Path, to: &Path) -> std::io::Result<()> { fs::rename(from, to) }
fn put(path: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let serial = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let temporary = path.with_file_name(format!(".readme-{}-{serial}.tmp", std::process::id()));
    safe(path)?;
    let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&temporary).map_err(|error| format!("目录说明暂存失败（{:?}，系统码 {:?}）",error.kind(),error.raw_os_error()))?;
    file.write_all(bytes).and_then(|_| file.sync_all()).map_err(|_| "目录说明写入失败")?;
    drop(file);
    let result = replace(&temporary, path).map_err(|_| "目录说明提交失败".to_owned());
    if result.is_err() { let _ = fs::remove_file(&temporary); }
    result
}
fn purpose(category: &str) -> &'static str {
    match category {
        "credentials" => "原始凭据与刷新令牌；不要提交到代码仓库或分享。",
        "roster" => "账号花名册与账号标识，凭据实值单独放在 credentials。",
        "keys" => "API、网关和签名密钥；导出时按所选类别包含。",
        "config" => "配置键和值，以及插件自己的运行偏好。",
        "exports" => "用户显式导出的备份；默认保留在当前数据根内。",
        "backups" => "更新、迁移与恢复前的受控备份；确认恢复结果后再自行清理。",
        "tmp" => "当前命名空间内的受控暂存；不作为活跃身份来源。",
        _ => "统一数据命名空间；各类资料分目录保存。",
    }
}
pub fn initialize_root(root: &Path, version: &str) -> Result<(), String> {
    if !root.is_absolute() || version.is_empty() || !version.bytes().all(|b| b.is_ascii_alphanumeric() || b".-+".contains(&b)) { return Err("数据根版本说明参数无效".into()); }
    safe(root)?;
    let text = format!("# FISH Dock 统一数据根\n\n宿主版本：{version}\n\n宿主资料位于 host；官方插件使用 .fishplugin-*，第三方插件使用 .plugin.<publisher>.<name>。插件未安装时不会提前创建其目录。\n\n凭据、账号花名册、密钥和配置分别保存在 credentials、roster、keys、config；导出、恢复备份与暂存分别使用 exports、backups、tmp。各目录说明记录实际版本。\n\n路径由 --data-root、FISH_DOCK_HOME、系统用户目录下 .fish-dock 依次解析，不依赖安装位置。旧资料只读保留；导入前先校验归属与摘要，再生成恢复副本。用户笔记请使用 USER-NOTES.md。\n");
    put(&root.join("README.md"), text.as_bytes())
}
pub fn initialize(root: &Path, owner: &str, version: &str, schema: u32) -> Result<(), String> {
    static EDIT: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _lock = EDIT.lock().map_err(|_| "目录初始化锁不可用")?;
    if !root.is_absolute() || owner.is_empty() || owner.chars().any(char::is_control) || version.is_empty() || !version.bytes().all(|b| b.is_ascii_alphanumeric() || b".-+".contains(&b)) || schema == 0 {
        return Err("目录说明的路径、身份或版本无效".into());
    }
    let directories: Vec<_> = std::iter::once("").chain(CATEGORIES.iter().copied()).map(|category| (category, root.join(category))).collect();
    // 全部路径预检完成后才写说明，避免冲突目录产生半份版本说明。
    let mut previous = Vec::new();
    for (_, directory) in &directories {
        safe(directory)?;
        if directory.exists() && !directory.is_dir() { return Err("分类存储路径已被普通文件占用".into()); }
        let readme = directory.join("README.md");
        safe(&readme)?;
        match fs::metadata(&readme) {
            Ok(meta) if meta.is_file() && meta.len() <= 1024 * 1024 => previous.push(Some(fs::read(&readme).map_err(|_| "旧目录说明读取失败")?)),
            Ok(_) => return Err("目录说明路径无效或超过限制".into()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => previous.push(None),
            Err(_) => return Err("目录说明检查失败".into()),
        }
    }
    for (_, directory) in &directories {
        fs::create_dir_all(directory).map_err(|_| "分类目录创建失败")?;
        #[cfg(unix)] { use std::os::unix::fs::PermissionsExt; fs::set_permissions(directory, fs::Permissions::from_mode(0o700)).map_err(|_| "分类目录权限设置失败")?; }
    }
    for (index, (category, directory)) in directories.iter().enumerate() {
        let text = format!("# FISH Dock 数据目录\n\n插件/宿主：{owner}  \n版本：{version}  \nschema: {schema}\n\n{}\n\n数据根由宿主启动设置、FISH_DOCK_HOME、系统用户目录依次解析。插件只使用宿主传入的命名空间。\n\n导出在 exports 中生成；导入先校验身份、类别、相对路径与摘要，再在 tmp 暂存并提交。恢复前副本放入 backups。Magpie 与 OFM 身份各自独立。\n\n本说明随发行版本生成，不包含账号或密钥实值。用户自有说明请存为 USER-NOTES.md，更新时保留。\n", purpose(category));
        if let Err(error) = put(&directory.join("README.md"), text.as_bytes()) {
            let mut restored = true;
            for before in 0..index {
                let path = directories[before].1.join("README.md");
                restored &= if let Some(bytes) = &previous[before] { put(&path, bytes).is_ok() } else { fs::remove_file(&path).is_ok() };
            }
            return Err(if restored { error } else { format!("{error}；旧目录说明恢复失败，请保留目录并重试") });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn root() -> PathBuf {
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        std::env::temp_dir().join(format!("fish-storage-{}-{stamp}", std::process::id())).join("中文 空格").join(".fish-dock")
    }
    #[test]
    fn root_precedence_is_independent_of_working_directory() {
        let home = root();
        let environment = root();
        let setting = root();
        assert_eq!(resolve_root(&home, None, None).unwrap(), home.join(".fish-dock"));
        assert_eq!(resolve_root(&home, None, Some(&environment)).unwrap(), environment);
        assert_eq!(resolve_root(&home, Some(&setting), Some(&environment)).unwrap(), setting);
        assert!(resolve_root(&home, Some(Path::new("relative/.fish-dock")), None).is_err());
        assert!(resolve_root(&home, Some(Path::new("")), None).is_err());
    }
    #[test]
    fn official_names_are_bound_to_catalog_identity() {
        let root = root();
        assert_eq!(namespace(&root, "fish.ofm", ".fishplugin-ourfreemodel", Source::OfficialCatalog).unwrap(), root.join(".fishplugin-ourfreemodel"));
        assert!(namespace(&root, "fish.ofm", ".fishplugin-magpie", Source::OfficialCatalog).is_err());
        assert!(namespace(&root, "attacker.ofm", ".fishplugin-ourfreemodel", Source::ThirdParty).is_err());
        assert!(namespace(&root, "fish.ofm", ".fishplugin-ourfreemodel", Source::ThirdParty).is_err());
    }
    #[test]
    fn third_party_directory_must_match_publisher_and_name() {
        let root = root();
        assert!(namespace(&root, "acme.weather", ".plugin.acme.weather", Source::ThirdParty).is_ok());
        for directory in [".plugin.other.weather", ".Plugin.acme.weather", "../escape", "/escape", ".plugin.acme.weather/keys", ".plugin.acme.weather."] {
            assert!(namespace(&root, "acme.weather", directory, Source::ThirdParty).is_err(), "{directory}");
        }
        assert!(!root.exists());
    }
    #[test]
    fn initialization_writes_real_version_readmes_and_keeps_user_notes() {
        let root = root();
        let data = root.join(".fishplugin-ourfreemodel");
        initialize(&data, "fish.ofm", "2.5.3", 1).unwrap();
        for relative in std::iter::once("").chain(CATEGORIES.iter().copied()) {
            let text = fs::read_to_string(data.join(relative).join("README.md")).unwrap();
            assert!(text.contains("2.5.3") && text.contains("fish.ofm") && text.contains("schema: 1"));
            assert!(!text.contains(&root.to_string_lossy().to_string()));
        }
        fs::write(data.join("USER-NOTES.md"), "用户说明").unwrap();
        initialize(&data, "fish.ofm", "2.5.4", 2).unwrap();
        assert!(fs::read_to_string(data.join("keys/README.md")).unwrap().contains("2.5.4"));
        assert_eq!(fs::read_to_string(data.join("USER-NOTES.md")).unwrap(), "用户说明");
        fs::remove_dir_all(root.parent().unwrap().parent().unwrap()).unwrap();
    }
    #[test]
    fn startup_setting_is_explicit_and_invalid_values_never_fall_back() {
        let home = root();
        let setting = root();
        let environment = root();
        let args = vec!["dock.exe".into(), "--data-root".into(), setting.to_string_lossy().into_owned()];
        assert_eq!(startup_root(&home, &args, Some(&environment)).unwrap(), setting);
        for tail in [vec!["--data-root"], vec!["--data-root", "relative"], vec!["--data-root", ""], vec!["--data-root", "one", "--data-root", "two"]] {
            let args = tail.into_iter().map(str::to_owned).collect::<Vec<_>>();
            assert!(startup_root(&home, &args, Some(&environment)).is_err());
        }
    }
    #[test]
    fn classified_files_stay_inside_their_selected_category() {
        let root = root();
        assert_eq!(category_file(&root, "credentials", "account/ref.json").unwrap(), root.join("credentials/account/ref.json"));
        for (category, name) in [("missing", "one"), ("keys", "../escape"), ("keys", "/absolute"), ("keys", "C:/absolute"), ("config", "a\\b"), ("config", "CON"), ("config", "file."), ("keys", ""), ("keys", "README.md")] {
            assert!(category_file(&root, category, name).is_err(), "{category}/{name}");
        }
        assert!(!root.exists());
    }
    #[test]
    fn file_in_place_of_category_fails_before_publishing_any_readme() {
        let root = root();
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("keys"), "existing-data").unwrap();
        assert!(initialize(&root, "host", "1.0.2", 1).is_err());
        assert!(!root.join("README.md").exists());
        assert_eq!(fs::read_to_string(root.join("keys")).unwrap(), "existing-data");
        fs::remove_dir_all(root.parent().unwrap().parent().unwrap()).unwrap();
    }
}

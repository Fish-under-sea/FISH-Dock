//! 第三方插件：按 npm 包名检索与安装。包只要按插件格式发布即可，宿主不审核其行为。
//! 与官方商店的区别只在来源：身份一律按第三方命名空间登记，摘要取自下载到的 tarball。
use std::{collections::BTreeMap, io::Read};
use serde::{Deserialize, Serialize};
use crate::storage::Source;
const REGISTRY: &str = "https://registry.npmjs.org";
const SEARCH_LIMIT: usize = 512 * 1024;
const META_LIMIT: usize = 2 * 1024 * 1024;
const ARCHIVE_LIMIT: u64 = crate::plugin_package::TOTAL_LIMIT as u64;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hit { pub name: String, pub version: String, pub description: String, pub publisher: String, pub updated: String }

#[derive(Deserialize)]
struct SearchAnswer { objects: Vec<SearchObject> }
#[derive(Deserialize)]
struct SearchObject { package: SearchPackage }
#[derive(Deserialize)]
struct SearchPackage { name: String, version: String, #[serde(default)] description: String, #[serde(default)] date: String, #[serde(default)] publisher: Option<Publisher> }
#[derive(Deserialize)]
struct Publisher { #[serde(default)] username: String }

fn get(url: &str, limit: usize) -> Result<Vec<u8>, String> {
    let mut last = "npm 连接失败，请检查网络后重试".to_string();
    for way in crate::proxy::web_ways(url) {
        let Some(agent) = way.agent_with(std::time::Duration::from_secs(25), |config| config.max_redirects(4).http_status_as_error(false)) else { continue };
        match agent.get(url).header("User-Agent", "FISH-Dock-Plugin-Store").header("Accept", "application/json").call() {
            Ok(mut response) => {
                let status = response.status().as_u16();
                if status != 200 { last = format!("npm 请求失败（HTTP {status}）"); continue; }
                match response.body_mut().with_config().limit(limit as u64).read_to_vec() {
                    Ok(bytes) => return Ok(bytes),
                    Err(_) => last = "npm 响应超过大小限制或读取中断".into(),
                }
            }
            Err(_) => last = "npm 连接失败，请检查网络后重试".into(),
        }
    }
    Err(last)
}

fn segment(value: &str) -> bool {
    !value.is_empty() && value.len() <= 214 && !value.starts_with('.') && !value.starts_with('_')
        && value.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'.' | b'_'))
}
// npm 包名：`name`，或带 scope 的 `@scope/name`。
pub fn package_name(value: &str) -> Result<String, String> {
    let name = value.trim();
    let body = name.strip_prefix('@').unwrap_or(name);
    let valid = match body.split_once('/') {
        Some((scope, rest)) => name.starts_with('@') && segment(scope) && segment(rest),
        None => segment(body),
    };
    if !valid || name.len() > 214 { return Err("npm 包名无效（示例：@acme/fish-weather 或 fish-weather）".into()); }
    Ok(name.to_string())
}
// `name` 或 `name@1.2.3`；版本交给 registry 解析。
fn spec(value: &str) -> Result<(String, Option<String>), String> {
    let raw = value.trim();
    let (name, version) = match raw.rsplit_once('@') {
        Some((name, version)) if !name.is_empty() && !version.is_empty() => (name, Some(version.to_string())),
        _ => (raw, None),
    };
    let name = package_name(name)?;
    if let Some(version) = &version {
        if version.len() > 64 || !version.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'+')) { return Err("npm 包版本无效".into()); }
    }
    Ok((name, version))
}

pub fn search(query: &str) -> Result<Vec<Hit>, String> {
    let text = query.trim();
    if text.is_empty() || text.len() > 120 { return Err("请输入要检索的包名或关键字".into()); }
    let url = format!("{REGISTRY}/-/v1/search?text={}&size=20", urlencode(text));
    let bytes = get(&url, SEARCH_LIMIT)?;
    let answer: SearchAnswer = serde_json::from_slice(&bytes).map_err(|_| "npm 检索结果格式无效")?;
    let mut hits = Vec::new();
    for object in answer.objects.into_iter().take(20) {
        let row = object.package;
        if package_name(&row.name).is_err() { continue; }
        hits.push(Hit {
            publisher: row.publisher.map(|p| p.username).unwrap_or_default(),
            description: truncate(&row.description, 240),
            updated: truncate(&row.date, 40),
            name: row.name,
            version: row.version,
        });
    }
    Ok(hits)
}

fn truncate(value: &str, max: usize) -> String {
    let clean: String = value.chars().filter(|c| !c.is_control()).take(max).collect();
    clean
}
fn urlencode(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => out.push(byte as char),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

#[derive(Deserialize)]
struct Meta { #[serde(rename = "dist-tags")] dist_tags: BTreeMap<String, String>, versions: BTreeMap<String, Version> }
#[derive(Deserialize)]
struct Version { dist: Dist }
#[derive(Deserialize)]
struct Dist { tarball: String }

fn resolve(name: &str, version: Option<&str>) -> Result<(String, String), String> {
    let url = format!("{REGISTRY}/{}", urlencode(name).replace("%2F", "/").replace("%40", "@"));
    let bytes = get(&url, META_LIMIT)?;
    let meta: Meta = serde_json::from_slice(&bytes).map_err(|_| "npm 包信息格式无效")?;
    let wanted = version.map(str::to_string).or_else(|| meta.dist_tags.get("latest").cloned()).ok_or("npm 包没有可安装的版本")?;
    let row = meta.versions.get(&wanted).ok_or_else(|| format!("npm 包没有 {wanted} 这个版本"))?;
    let tarball = tauri::Url::parse(&row.dist.tarball).map_err(|_| "npm 包下载地址无效")?;
    if tarball.scheme() != "https" || !tarball.username().is_empty() || tarball.password().is_some() || tarball.fragment().is_some() {
        return Err("npm 包下载必须使用无身份信息的 HTTPS 地址".into());
    }
    let host = tarball.host_str().unwrap_or("");
    if !["registry.npmjs.org", "registry.npmjs.com", "registry.yarnpkg.com"].contains(&host) { return Err("npm 包下载地址不属于公开 registry".into()); }
    Ok((wanted, tarball.to_string()))
}

// ustar：512 字节头，名字、大小、类型、前缀都在固定位置；npm 的包里只有普通文件。
fn untar(bytes: &[u8]) -> Result<Vec<(String, Vec<u8>)>, String> {
    let mut out = Vec::new();
    let mut at = 0usize;
    while at + 512 <= bytes.len() {
        let header = &bytes[at..at + 512];
        if header.iter().all(|byte| *byte == 0) { break; }
        let field = |start: usize, len: usize| String::from_utf8_lossy(&header[start..start + len]).trim_end_matches(['\0', ' ']).to_string();
        let name = field(0, 100);
        let prefix = field(345, 155);
        let size = usize::from_str_radix(field(124, 12).trim(), 8).map_err(|_| "npm 包条目大小无效")?;
        let kind = header[156];
        let full = if prefix.is_empty() { name } else { format!("{prefix}/{name}") };
        let start = at + 512;
        if start + size > bytes.len() { return Err("npm 包内容被截断".into()); }
        if kind == b'0' || kind == 0 { out.push((full, bytes[start..start + size].to_vec())); }
        at = start + size.div_ceil(512) * 512;
    }
    Ok(out)
}

fn unpack(bytes: &[u8]) -> Result<BTreeMap<String, Vec<u8>>, String> {
    let mut decoder = flate2::read::GzDecoder::new(bytes);
    let mut plain = Vec::new();
    decoder.read_to_end(&mut plain).map_err(|_| "npm 包解压失败")?;
    if plain.len() as u64 > ARCHIVE_LIMIT { return Err("npm 包解压后超过大小限制".into()); }
    let mut files = BTreeMap::new();
    for (name, body) in untar(&plain)? {
        // npm 的归档把包内容放在 `package/` 下。
        let relative = name.strip_prefix("./").unwrap_or(&name);
        let Some(relative) = relative.strip_prefix("package/").or_else(|| relative.strip_prefix("package\\")) else { continue };
        if relative.is_empty() { continue; }
        crate::plugin_manifest::package_path(std::path::Path::new("."), relative).map_err(|_| format!("npm 包包含无效路径：{relative}"))?;
        if files.insert(relative.to_string(), body).is_some() { return Err(format!("npm 包包含重复文件：{relative}")); }
        if files.len() > 4096 { return Err("npm 包文件数量超过限制".into()); }
    }
    Ok(files)
}

// 下载并按插件格式校验；返回可直接交给安装器的包与 tarball 摘要。
pub fn obtain(context: &crate::storage_context::Context, value: &str) -> Result<(crate::plugin_package::Package, String), String> {
    use sha2::{Digest, Sha256};
    let (name, version) = spec(value)?;
    let (version, tarball) = resolve(&name, version.as_deref())?;
    let bytes = get(&tarball, ARCHIVE_LIMIT as usize)?;
    let digest = format!("{:x}", Sha256::digest(&bytes));
    let mut files = unpack(&bytes)?;
    let raw = files.get("plugin.json").cloned().ok_or("这个 npm 包不是 FISH Dock 插件：缺少 plugin.json")?;
    if !files.contains_key("README.md") { return Err("这个 npm 包缺少当前版本 README.md".into()); }
    let manifest = crate::plugin_manifest::Manifest::parse(&raw, &context.root, Source::ThirdParty, &crate::plugin_install::platform())?;
    if files.len() != manifest.files.len() + 1 { return Err("npm 包包含未声明的文件或缺少文件".into()); }
    for (file, expected) in &manifest.files {
        let body = files.get(file).ok_or_else(|| format!("npm 包缺少声明的资源：{file}"))?;
        if !format!("{:x}", Sha256::digest(body)).eq_ignore_ascii_case(expected) { return Err(format!("npm 包资源校验失败：{file}")); }
    }
    if manifest.version != version { return Err("npm 包清单版本与发布的版本不一致".into()); }
    Ok((crate::plugin_package::Package { manifest, files }, digest))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn package_names_and_specs_are_strict() {
        assert_eq!(package_name("fish-weather").unwrap(), "fish-weather");
        assert_eq!(package_name("@acme/fish-weather").unwrap(), "@acme/fish-weather");
        for bad in ["", "Fish-Weather", "../evil", "@acme/", ".hidden", "a".repeat(215).as_str()] {
            assert!(package_name(bad).is_err(), "{bad}");
        }
        assert_eq!(spec("fish-weather@1.2.3").unwrap(), ("fish-weather".to_string(), Some("1.2.3".to_string())));
        assert_eq!(spec("@acme/plugin").unwrap(), ("@acme/plugin".to_string(), None));
        assert!(spec("fish@1.2.3;rm -rf").is_err());
    }
    #[test]
    fn the_archive_reader_only_takes_files_under_package() {
        let mut plain = Vec::new();
        for (name, body) in [("package/plugin.json", b"{}".as_slice()), ("package/backend.mjs", b"x".as_slice()), ("other.txt", b"no".as_slice())] {
            let mut header = [0u8; 512];
            header[..name.len()].copy_from_slice(name.as_bytes());
            let size = format!("{:o}", body.len());
            header[124..124 + size.len()].copy_from_slice(size.as_bytes());
            header[156] = b'0';
            plain.extend_from_slice(&header);
            plain.extend_from_slice(body);
            plain.resize(plain.len() + (512 - body.len() % 512) % 512, 0);
        }
        plain.extend_from_slice(&[0u8; 1024]);
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        std::io::Write::write_all(&mut encoder, &plain).unwrap();
        let files = unpack(&encoder.finish().unwrap()).unwrap();
        assert_eq!(files.len(), 2);
        assert!(files.contains_key("plugin.json"));
        assert!(!files.keys().any(|name| name.starts_with("other")));
    }
    #[test]
    fn a_package_without_a_manifest_is_named_as_such() {
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        std::io::Write::write_all(&mut encoder, &[0u8; 1024]).unwrap();
        let files = unpack(&encoder.finish().unwrap()).unwrap();
        assert!(files.is_empty());
    }
}
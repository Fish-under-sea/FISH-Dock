// Getting a pet from codex-pets.net: its id from whatever the person pasted,
// the download into <dir>/<id>/ (spritesheet.webp, pet.json), and a page of
// the site's gallery. Errors carry a code (and vars) to word in her language.
use std::fs;
use std::path::Path;

use serde_json::{json, Value};
use tauri::Url;

pub const SITE: &str = crate::config::PET_SITE_DEFAULT;
pub const DEFAULT_PET: &str = "claude-chan";
// The default pet (Claude小姐: pixel art by zhoupengjie, an unofficial fan
// work of ZipZipPipe's character, as its pet.json says), carried in the exe:
// written into her pets on the first run, nothing to download (codex-pets.net
// slow or out of reach, in China above all). The gallery gets the others.
const DEFAULT_SHEET: &[u8] = include_bytes!("../../pets/claude-chan/spritesheet.webp");
const DEFAULT_JSON: &str = include_str!("../../pets/claude-chan/pet.json");

// The default pet into <dir>/claude-chan/, whole (put_pet). Its pet.json
// read back.
pub fn seed_default_pet(dir: &Path) -> std::io::Result<Value> {
    put_pet(dir, DEFAULT_PET, DEFAULT_SHEET, DEFAULT_JSON.as_bytes())?;
    Ok(serde_json::from_str(DEFAULT_JSON).unwrap_or(Value::Null))
}

// A pet into <dir>/<id>/ whole: its pet.json and sheet written into
// <dir>/.<id>.new/ (no pet: data.rs passes a name with a dot by), then moved
// in by one rename. A pet is found by its sheet, and what was found kept until
// her pets' folder itself changes (data.rs pets): looked at between the sheet
// and its pet.json, she was "claude-chan" with no author the whole run (the
// test session, 2026-10-13). One there before is moved aside first, and
// removed; held (its sheet open), it is written over in place, its pet.json
// first. Either way her pets are looked for anew (data.rs forget_pets).
fn put_pet(dir: &Path, id: &str, sheet: &[u8], meta: &[u8]) -> std::io::Result<()> {
    let put = write_pet(dir, id, sheet, meta);
    crate::data::forget_pets();
    put
}

fn write_pet(dir: &Path, id: &str, sheet: &[u8], meta: &[u8]) -> std::io::Result<()> {
    let out = dir.join(id);
    let stage = dir.join(format!(".{id}.new"));
    let old = dir.join(format!(".{id}.old"));
    let _ = fs::remove_dir_all(&stage);
    fs::create_dir_all(&stage)?;
    fs::write(stage.join("pet.json"), meta)?;
    fs::write(stage.join("spritesheet.webp"), sheet)?;
    if out.exists() {
        let _ = fs::remove_dir_all(&old);
        if fs::rename(&out, &old).is_err() {
            let _ = fs::remove_dir_all(&stage);
            fs::write(out.join("pet.json"), meta)?;
            return fs::write(out.join("spritesheet.webp"), sheet);
        }
    }
    fs::rename(&stage, &out)?;
    let _ = fs::remove_dir_all(&old);
    Ok(())
}
// The layouts the page plays: 8 columns of 192 x 208 cells, 11 rows (v2) or
// 9 (v1, without the look-around rows).
const ATLASES: [&str; 2] = ["1536x2288", "1536x1872"];

pub struct PetError {
    pub code: &'static str,
    pub vars: Value,
}

fn err(code: &'static str, vars: Value) -> PetError {
    PetError { code, vars }
}

fn is_id(text: &str) -> bool {
    let mut chars = text.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphanumeric()) && chars.all(|c| c.is_ascii_alphanumeric() || c == '-')
}

fn is_site_host(host: &str) -> bool {
    host == "codex-pets.net" || host.ends_with(".codex-pets.net")
}

fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(b) = u8::from_str_radix(&text[i + 1..i + 3], 16) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

// A pet's id from an id or any codex-pets.net link to it: the page
// (https://codex-pets.net/#/pets/<id>), with or without https://, or its API
// or asset URLs.
pub fn parse_pet_ref(input: &str) -> Result<String, PetError> {
    let text = input.trim();
    if is_id(text) {
        return Ok(text.to_string());
    }
    let with_scheme = if text.contains("://") { text.to_string() } else { format!("https://{text}") };
    let url = Url::parse(&with_scheme).map_err(|_| err("unreadable", json!({ "ref": text })))?;
    if !url.host_str().is_some_and(is_site_host) {
        return Err(err("otherSite", json!({ "ref": text })));
    }
    let after = |part: &str, prefix: &str| -> Option<String> {
        let rest = part.strip_prefix(prefix)?;
        let id = decode(rest.split(['/', '?', '#']).next()?);
        is_id(&id).then_some(id)
    };
    let path = url.path();
    let from_asset = path.strip_prefix("/assets/pets/v/").and_then(|rest| {
        let mut parts = rest.split('/');
        let n = parts.next()?;
        let id = decode(parts.next()?);
        (n.chars().all(|c| c.is_ascii_digit()) && is_id(&id) && parts.next().is_some()).then_some(id)
    });
    url.fragment()
        .and_then(|f| after(f, "/pets/"))
        .or_else(|| after(path, "/api/pets/"))
        .or(from_asset)
        .or_else(|| after(path, "/pets/"))
        .ok_or_else(|| err("noId", json!({ "ref": text })))
}

// One GET of the site's, the way the General page's proxy says and Windows'
// do (proxy::web_ways), each way in turn; its answer read whole within the
// pet budget (proxy::pet_timeout, overridable). The old agent here ignored
// netProxy and took the environment's proxy by itself, so a proxy env var
// pointing at a dead port (the very case the person meets with a ladder
// misconfigured) hung the gallery's every page.
fn get(url: &str) -> Result<Vec<u8>, PetError> {
    let mut last = err("network", json!({ "message": "no way", "url": url }));
    for way in crate::proxy::web_ways(url) {
        let Some(agent) = way.agent_with(crate::proxy::pet_timeout(), |c| c.http_status_as_error(false).max_redirects(0)) else { continue };
        let called = agent.get(url).header("user-agent", "wakuwaku").call();
        match called {
            Ok(mut response) => {
                let status = response.status().as_u16();
                if status != 200 {
                    if status == 404 {
                        return Err(err("notFound", json!({ "url": url })));
                    }
                    last = err("http", json!({ "status": status, "url": url }));
                    continue;
                }
                return response
                    .body_mut()
                    .with_config()
                    .limit(64 * 1024 * 1024)
                    .read_to_vec()
                    .map_err(|e| err("network", json!({ "message": e.to_string(), "url": url })));
            }
            Err(e) => last = http_err(url, e),
        }
    }
    Err(last)
}

fn http_err(url: &str, e: ureq::Error) -> PetError {
    match e {
        ureq::Error::StatusCode(status) => err("http", json!({ "status": status, "url": url })),
        other => err("network", json!({ "message": other.to_string(), "url": url })),
    }
}

// Downloads the pet into <dir>/<id>/. Returns { id, name, author, warning? }.
pub fn download_pet(input: &str, dir: &Path) -> Result<Value, PetError> {
    let id = parse_pet_ref(input)?;
    // 站点地址走配置（FISH_DOCK_PET_SITE 可覆盖），不再散落字面量。
    let site = crate::config::pet_site();
    let api = format!("{site}/api/pets/{id}");
    let body: Value = serde_json::from_slice(&get(&api)?)
        .map_err(|e| err("network", json!({ "message": e.to_string(), "url": api })))?;
    let pet = &body["pet"];

    // Only ever download from the site itself.
    let sheet_url = Url::parse(&site)
        .and_then(|site| site.join(pet["spritesheetUrl"].as_str().unwrap_or("")))
        .map_err(|_| err("sheetElsewhere", json!({ "url": pet["spritesheetUrl"] })))?;
    if sheet_url.scheme() != "https" || !sheet_url.host_str().is_some_and(is_site_host) {
        return Err(err("sheetElsewhere", json!({ "url": sheet_url.as_str() })));
    }
    let sheet = get(sheet_url.as_str())?;

    let write = || -> std::io::Result<()> {
        let meta = json!({
            "id": pet["id"],
            "displayName": pet["displayName"],
            "description": pet["description"],
            "author": pet["ownerName"],
            "source": format!("{site}/#/pets/{id}"),
            "spritesheetPath": "spritesheet.webp",
            "spriteVersionNumber": pet["spriteVersionNumber"],
            "kind": pet["kind"],
        });
        put_pet(dir, &id, &sheet, format!("{}\n", serde_json::to_string_pretty(&meta).unwrap_or_default()).as_bytes())
    };
    write().map_err(|e| err("write", json!({ "message": e.to_string() })))?;

    let atlas = pet["validationReport"]["atlasSize"].as_str().unwrap_or("");
    let warning = (!atlas.is_empty() && !ATLASES.contains(&atlas)).then(|| json!({ "code": "atlas", "vars": { "id": id, "atlas": atlas, "want": ATLASES.join(" / ") } }));
    Ok(json!({ "id": id, "name": pet["displayName"], "author": pet["ownerName"], "warning": warning }))
}

// A page of the gallery's list: the newest first or the best liked, and
// those a search finds (by name, description or id, loosely, as the site's
// own search box finds them). The site says "new" now; "newest" it turned
// away (400, invalid sort, 2026-10-10).
fn gallery_url(page: u64, sort: &str, query: &str) -> String {
    let sort = if sort == "new" || sort == "newest" { "new" } else { "popular" };
    let query: String = query.trim().chars().take(100).collect();
    let mut url = Url::parse(&format!("{}/api/pets", crate::config::pet_site())).expect("the site's address");
    url.query_pairs_mut().append_pair("page", &page.max(1).to_string()).append_pair("pageSize", "12").append_pair("sort", sort);
    if !query.is_empty() {
        url.query_pairs_mut().append_pair("q", &query);
    }
    url.to_string()
}

// The gallery's answers as last read, in her data folder: an offline look
// still shows the pets the site had (stale-while-revalidate — the fresh
// read goes on whatever the cache says, and only a cache older than this
// is called stale in the page, never "could not connect" alone). One file
// per query's shape, kept to crate::config::gallery_cache_keep() so the folder
// cannot grow.
const GALLERY_CACHE: &str = "gallery-cache";

// The cache file of one gallery page: its name says the page, sort and
// query, so nothing but the same question ever reads it.
fn gallery_cache_name(page: u64, sort: &str, query: &str) -> String {
    let sort = if sort == "new" || sort == "newest" { "new" } else { "popular" };
    let query: String = query.trim().chars().take(100).collect();
    // A name safe for a file whatever was typed: the query hashed, the parts
    // plain. FNV-1a: short, no dependency.
    let mut hash = 2166136261u32;
    for byte in query.as_bytes() {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(16777619);
    }
    format!("page-{page}-{sort}-{hash:08x}.json")
}

// The page as last read from its cache file, or none. Read only; a broken
// or missing file is simply no cache, never an error to show.
fn gallery_cache_read(dir: &Path, name: &str) -> Option<Value> {
    serde_json::from_slice(&fs::read(dir.join(GALLERY_CACHE).join(name)).ok()?).ok()
}

// The page into its cache file, and the older ones thinned. A write that
// fails costs the cache only, never the answer.
fn gallery_cache_write(dir: &Path, name: &str, page: &Value) {
    let folder = dir.join(GALLERY_CACHE);
    if fs::create_dir_all(&folder).is_err() {
        return;
    }
    if let Ok(text) = serde_json::to_vec(page) {
        let _ = fs::write(folder.join(name), text);
    }
    // The oldest beyond the keep count go; names sort by page.
    let mut kept: Vec<_> = fs::read_dir(&folder).into_iter().flatten().flatten().map(|e| e.path()).collect();
    let keep = crate::config::gallery_cache_keep();
    if kept.len() > keep {
        kept.sort();
        for path in kept.drain(..kept.len() - keep) {
            let _ = fs::remove_file(path);
        }
    }
}

// A page of the site's gallery, or of what a search finds there. Only what
// the gallery shows is passed on, and only pictures from the site itself.
// The site out of reach: the page as last read, said as cached (`offline`
// in the answer, so the page can say 离线/缓存, not a dead spinner).
pub fn gallery(page: u64, sort: &str, query: &str, cache_dir: &Path) -> Result<Value, PetError> {
    let url = gallery_url(page, sort, query);
    let cache_name = gallery_cache_name(page, sort, query);
    let read = |body: &Value| {
        let site = crate::config::pet_site();
        let items: Vec<Value> = body["pets"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|p| p["id"].as_str().is_some_and(is_id))
            .map(|p| {
                // The poster is one frame; the preview is a strip of them all.
                let preview = ["posterUrl", "previewUrl"].iter().find_map(|k| p[*k].as_str().filter(|u| u.starts_with(&format!("{site}/")))).unwrap_or("");
                json!({
                    "id": p["id"],
                    "name": p["displayName"].as_str().unwrap_or(p["id"].as_str().unwrap_or("")),
                    "author": p["ownerName"].as_str().unwrap_or(""),
                    "likes": p["likeCount"].as_u64().unwrap_or(0),
                    "version": if p["spriteVersionNumber"] == 2 { 2 } else { 1 },
                    "preview": preview,
                })
            })
            .collect();
        json!({ "items": items, "page": body["page"], "totalPages": body["totalPages"], "total": body["total"] })
    };
    let asked = get(&url).and_then(|bytes| {
        let body: Value = serde_json::from_slice(&bytes).map_err(|e| err("network", json!({ "message": e.to_string(), "url": url })))?;
        let page = read(&body);
        gallery_cache_write(cache_dir, &cache_name, &page);
        Ok(page)
    });
    match asked {
        Ok(page) => Ok(page),
        Err(e) => match gallery_cache_read(cache_dir, &cache_name) {
            Some(page) => Ok(json!({ "items": page["items"], "page": page["page"], "totalPages": page["totalPages"], "total": page["total"], "offline": true, "error": describe_cached(&e) })),
            None => Err(e),
        },
    }
}

// What the page says of a gallery answer served from the cache: the cache
// is the reason it shows anything at all, so it names both facts, not just
// the failure. (Words here, not in the page: the page renders what it is
// given, in the person's language as describe does.)
fn describe_cached(e: &PetError) -> String {
    match e.code {
        "network" => format!("离线，先用上次的图库（连不上：{}）", e.vars["message"].as_str().unwrap_or("")),
        "http" => format!("站点暂时应答异常（HTTP {}），先用上次的图库", e.vars["status"]),
        other => format!("先用上次的图库（{other}）"),
    }
}

// An error in the person's language.
pub fn describe(lang: &str, e: &PetError) -> String {
    let zh = lang == "zh";
    let text = match e.code {
        "unreadable" => if zh { "看不懂这个地址或 id：{ref}" } else { "Can't read this URL or id: {ref}" },
        "otherSite" => if zh { "只支持 codex-pets.net 上的宠物：{ref}" } else { "Only pets on codex-pets.net are supported: {ref}" },
        "noId" => if zh { "地址里没找到宠物 id：{ref}（宠物页面的地址形如 https://codex-pets.net/#/pets/<id>）" } else { "No pet id in this URL: {ref} (a pet page looks like https://codex-pets.net/#/pets/<id>)" },
        "notFound" => if zh { "codex-pets.net 上没有这只宠物：{id}" } else { "No such pet on codex-pets.net: {id}" },
        "http" => "{status}: {url}",
        "network" => if zh { "连不上：{message}" } else { "Could not connect: {message}" },
        "write" => if zh { "存不下来：{message}" } else { "Could not save it: {message}" },
        "sheetElsewhere" => if zh { "图集不在 codex-pets.net 上，不下载：{url}" } else { "The sprite sheet is not on codex-pets.net, not downloading: {url}" },
        "atlas" => if zh { "注意：{id} 的图集是 {atlas}，不是常见的 {want}，动画可能对不上。" } else { "Note: {id} has a {atlas} sheet, not the usual {want}; the animation may be off." },
        other => other,
    };
    let mut out = text.to_string();
    if let Some(vars) = e.vars.as_object() {
        for (k, v) in vars {
            let v = v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string());
            out = out.replace(&format!("{{{k}}}"), &v);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_pet_comes_from_the_exe() {
        let dir = std::env::temp_dir().join(format!("wakuwaku-seed-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let got = seed_default_pet(&dir).unwrap();
        assert_eq!(got["id"], DEFAULT_PET);
        // Her credits kept as they are.
        assert!(got["description"].as_str().unwrap().contains("ZipZipPipe"));
        let sheet = fs::read(dir.join(DEFAULT_PET).join("spritesheet.webp")).unwrap();
        assert_eq!(sheet.len(), DEFAULT_SHEET.len());
        assert!(&sheet[..4] == b"RIFF" && &sheet[8..12] == b"WEBP");
        assert!(dir.join(DEFAULT_PET).join("pet.json").exists());
        // Put in whole, nothing left beside it; and again over itself.
        let left = |dir: &Path| fs::read_dir(dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect::<Vec<_>>();
        assert_eq!(left(&dir), [DEFAULT_PET]);
        fs::write(dir.join(DEFAULT_PET).join("pet.json"), "{}").unwrap();
        seed_default_pet(&dir).unwrap();
        assert_eq!(left(&dir), [DEFAULT_PET]);
        assert_eq!(fs::read_to_string(dir.join(DEFAULT_PET).join("pet.json")).unwrap(), DEFAULT_JSON);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_pet_is_found_only_whole_and_as_soon_as_she_puts_it_in() {
        // Her data folder (pets in its pets/), and hers there: not the source
        // tree's nor Codex's (one with the same id may be in them).
        let data = std::env::temp_dir().join(format!("wakuwaku-whole-{}", std::process::id()));
        let _ = fs::remove_dir_all(&data);
        let dir = data.join("pets");
        let mine = |data: &Path| crate::data::pets(data).into_iter().find(|p| p.id == DEFAULT_PET && p.sheet.starts_with(data));
        // One being put in (its sheet in .claude-chan.new) is no pet.
        let stage = dir.join(format!(".{DEFAULT_PET}.new"));
        fs::create_dir_all(&stage).unwrap();
        fs::write(stage.join("spritesheet.webp"), DEFAULT_SHEET).unwrap();
        assert!(crate::data::pets(&data).iter().all(|p| !p.id.starts_with('.')) && mine(&data).is_none());
        // Looked at while her folder had its sheet and no pet.json yet: kept
        // so, as her pets' folder's time does not move; put in by her, found
        // whole at once.
        let out = dir.join(DEFAULT_PET);
        fs::create_dir_all(&out).unwrap();
        fs::write(out.join("spritesheet.webp"), DEFAULT_SHEET).unwrap();
        crate::data::forget_pets();
        assert_eq!(mine(&data).unwrap().name, DEFAULT_PET);
        seed_default_pet(&dir).unwrap();
        let pet = mine(&data).unwrap();
        assert!(pet.name != DEFAULT_PET && !pet.author.is_empty());
        assert!(!stage.exists());
        let _ = fs::remove_dir_all(&data);
    }

    #[test]
    fn ids_from_whatever_was_pasted() {
        let id = |s: &str| parse_pet_ref(s).ok();
        assert_eq!(id("claude-chan").as_deref(), Some("claude-chan"));
        assert_eq!(id(DEFAULT_PET).as_deref(), Some("claude-chan"));
        assert_eq!(id(" https://codex-pets.net/#/pets/claude-chan ").as_deref(), Some("claude-chan"));
        assert_eq!(id("codex-pets.net/#/pets/deepseek-chan?x=1").as_deref(), Some("deepseek-chan"));
        assert_eq!(id("https://codex-pets.net/api/pets/abc/download").as_deref(), Some("abc"));
        assert_eq!(id("https://codex-pets.net/assets/pets/v/2/abc/sheet.webp").as_deref(), Some("abc"));
        assert_eq!(id("https://www.codex-pets.net/pets/abc").as_deref(), Some("abc"));
        assert_eq!(parse_pet_ref("https://example.com/#/pets/abc").err().map(|e| e.code), Some("otherSite"));
        assert_eq!(parse_pet_ref("https://codex-pets.net/").err().map(|e| e.code), Some("noId"));
    }

    #[test]
    fn gallery_asks() {
        assert_eq!(gallery_url(0, "popular", ""), "https://codex-pets.net/api/pets?page=1&pageSize=12&sort=popular");
        assert_eq!(gallery_url(2, "newest", " "), "https://codex-pets.net/api/pets?page=2&pageSize=12&sort=new");
        assert_eq!(gallery_url(1, "x", "猫 cat&sort=new"), "https://codex-pets.net/api/pets?page=1&pageSize=12&sort=popular&q=%E7%8C%AB+cat%26sort%3Dnew");
    }

    // 离线降级：站点连不上时，图库按上次读到的页面回答并标明 offline，
    // 不是一句「连不上」把整个图库变成死白。
    #[test]
    fn a_dead_site_still_shows_the_last_gallery_page_as_cached() {
        let cache = std::env::temp_dir().join(format!("wakuwaku-gallery-cache-{}", std::process::id()));
        let _ = fs::remove_dir_all(&cache);
        let name = gallery_cache_name(1, "popular", "");
        // 命中：缓存里已有这一页。
        let page = json!({ "items": [{ "id": "claude-chan", "name": "Claude", "author": "a", "likes": 1, "version": 2, "preview": "" }], "page": 1, "totalPages": 2, "total": 24 });
        gallery_cache_write(&cache, &name, &page);
        assert_eq!(gallery_cache_read(&cache, &name).unwrap()["total"], 24);
        // 同一页的另一次提问换参数时不读这份缓存（先证明名字真的分页）。
        assert!(gallery_cache_read(&cache, &gallery_cache_name(2, "popular", "")).is_none());
        // 写入淘汰：超出保留份数的最旧缓存文件被收走，文件夹有上限。
        for n in 0..crate::config::gallery_cache_keep() + 3 {
            gallery_cache_write(&cache, &format!("page-{}-popular-00000000.json", n + 3), &page);
        }
        let left = fs::read_dir(cache.join(GALLERY_CACHE)).unwrap().count();
        assert!(left <= crate::config::gallery_cache_keep(), "缓存文件数 {left} 超过上限 {}", crate::config::gallery_cache_keep());
        let _ = fs::remove_dir_all(&cache);
    }

    // 缓存文件名按提问形状区分：分页、排序与搜索词不同就不同名，但同一
    // 提问（含同一搜索词）永远同一名字。
    #[test]
    fn gallery_cache_names_separate_questions_and_stay_stable() {
        let a = gallery_cache_name(1, "popular", "");
        let again = gallery_cache_name(1, "popular", " ");
        assert_eq!(a, again, "同一提问（空搜索词的空白差异）应同名");
        assert_ne!(a, gallery_cache_name(1, "new", ""));
        assert_ne!(a, gallery_cache_name(2, "popular", ""));
        let q1 = gallery_cache_name(1, "popular", "猫");
        let q2 = gallery_cache_name(1, "popular", "猫");
        assert_eq!(q1, q2, "同一搜索词跨调用应稳定");
        assert_ne!(q1, gallery_cache_name(1, "popular", "狗"));
    }

    #[test]
    fn errors_in_words() {
        let e = err("notFound", json!({ "id": "x" }));
        assert_eq!(describe("zh", &e), "codex-pets.net 上没有这只宠物：x");
        assert_eq!(describe("en", &err("http", json!({ "status": 500, "url": "u" }))), "500: u");
        // 离线兜底的文案把「离线/缓存」与原因都说了。
        let offline = describe_cached(&err("network", json!({ "message": "refused" })));
        assert!(offline.contains("离线") && offline.contains("refused"), "{offline}");
    }
}

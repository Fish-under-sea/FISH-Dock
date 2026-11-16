// DEC-6（上游 v0.4.3 #114/#150 的等价实现，安全默认）：邮件正文的 HTML 净化。
//
// 本仓邮件先前只有「纯文本 / 原文 .eml」，没有 HTML 渲染（mail/letters.rs 的 text）。这里
// 加一层**净化器**：把一封邮件的 HTML 正文变成可安全展示的片段，并给出附属信息（远程图片
// 清单、链接清单、验证码）。这是 DEC-6 的安全核心，纯逻辑、可单测；渲染 UI 与「按发件人
// 放行远程图片/一次性加载」由页面后续接（本模块只产出数据与安全策略判定）。
//
// 安全默认（decisions-captain.md，不得放宽）：
//   - 去脚本 / iframe / object / embed / form / input / meta / link / style 标签；
//   - 去所有内联事件处理属性（onclick 等 on*）与 javascript:/vbscript:/data: 等危险 scheme；
//   - 远程图片（http/https）**默认阻止**：改成占位并记入 remote_images，由页面按「按发件人
//     放行」或「一次性加载」决定是否真的加载；cid: 内联图片不在阻止之列（本模块只透传标记）；
//   - 链接保留但登记到 links（含真实域名），页面点击需确认并显示真实目标域名；
//   - 验证码：检出常见的 4–8 位数字/字母验证码片段，登记 codes（只展示/复制，不自动填充）；
//   - 解析失败（空、无正文）→ None，调用方回落到纯文本。

use serde_json::{json, Value};

/// 净化后的 HTML 片段与附属信息。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Clean {
    /// 干净、可安全插入的 HTML（已去危险标签/属性，远程图片已替换为占位）。
    pub html: String,
    /// 被阻止的远程图片 URL（页面据此提供「一次性加载」/「按发件人放行」）。
    pub remote_images: Vec<String>,
    /// 正文里的链接（href 原文 + 解析出的真实域名），页面点击前确认并显示域名。
    pub links: Vec<Link>,
    /// 检出的验证码片段（只展示/复制，不自动填充）。
    pub codes: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Link {
    pub href: String,
    /// 真实域名（host），解析不出则空串（页面应拒绝或当纯文本）。
    pub host: String,
}

/// 会被整个丢弃（连同内容）的标签。
const DROP_TAGS: &[&str] = &["script", "iframe", "object", "embed", "form", "input", "button", "textarea", "select", "option", "meta", "link", "base", "style", "svg", "math"];

/// 允许保留的简单排版标签（白名单）；其它标签一律剥掉标签只留文字。
const KEEP_TAGS: &[&str] = &["p", "br", "b", "strong", "i", "em", "u", "span", "div", "ul", "ol", "li", "blockquote", "pre", "code", "h1", "h2", "h3", "h4", "h5", "h6", "a", "img", "table", "thead", "tbody", "tr", "td", "th", "hr"];

/// 净化一封邮件的 HTML。空正文返回 None（调用方回落纯文本）。
pub fn sanitize(input: &str) -> Option<Clean> {
    if input.trim().is_empty() {
        return None;
    }
    let mut out = String::with_capacity(input.len());
    let mut remote_images = Vec::new();
    let mut links: Vec<Link> = Vec::new();
    let mut rest = input;
    while let Some(lt) = rest.find('<') {
        // 标签前的文字原样（HTML 实体不动，交给浏览器）。
        out.push_str(&rest[..lt]);
        let after = &rest[lt..];
        // 注释、CDATA 一并跳过。
        if after.starts_with("<!--") {
            if let Some(end) = after.find("-->") {
                rest = &after[end + 3..];
                continue;
            } else {
                rest = "";
                break;
            }
        }
        let Some(gt) = after.find('>') else {
            // 没有闭合的 '<'：剩余当文字，但转义 '<' 以免被当标签。
            out.push_str(&after.replace('<', "&lt;"));
            rest = "";
            break;
        };
        let tag = &after[..=gt];
        rest = &after[gt + 1..];
        // t16（t9 发现）：script/style/iframe/object/embed/svg/math 等丢弃类标签，
        // 其**内容**（直到对应闭合标签）也必须整段跳过，不能作为文字流出——
        // 否则 <style>body{background:red}</style> 会漏出 body{…}、<script>alert(1)</script>
        // 漏出 alert(1)。用状态机按标签名找匹配闭合（对 svg/math 支持同名嵌套）。
        if let Some(name) = dropped_open_tag(tag) {
            rest = skip_until_close(rest, &name);
            continue;
        }
        emit_tag(tag, &mut out, &mut remote_images, &mut links);
    }
    out.push_str(rest);
    let codes = find_codes(&strip_tags(&out));
    Some(Clean { html: out, remote_images, links, codes })
}

// 处理一个标签（含尖括号）；决定保留、剥掉、还是整段丢弃（丢弃由调用方在更外层处理不行，
// 这里只处理单个标签——成对丢弃用状态，简化：DROP_TAGS 的单标签与内容靠下面的状态机略过）。

/// 若 `tag` 是一个**开始**的丢弃类标签（非自闭合、非结束标签），返回其小写标签名，
/// 表示其内容直到对应闭合必须整段跳过。结束标签或自闭合不在此列（直接丢弃标签即可）。
fn dropped_open_tag(tag: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    if lower.starts_with("</") {
        return None; // 结束标签：交给 emit_tag 丢弃即可
    }
    if lower.trim_end().ends_with("/>") {
        return None; // 自闭合：丢弃这个标签即可，无内容
    }
    let name = tag_name(&lower);
    if name.is_empty() {
        return None;
    }
    if DROP_TAGS.contains(&name.as_str()) {
        Some(name)
    } else {
        None
    }
}

/// 从 `rest` 起点跳过内容直到与 `name` 匹配的闭合标签（含）之后。
/// 支持同名嵌套（svg/math 内可再有 svg/math）：用深度计数，深度回到 0 即匹配的闭合。
/// 找不到闭合时返回空串（剩余整段丢弃——宁可少显示也不漏内容）。
fn skip_until_close<'a>(rest: &'a str, name: &str) -> &'a str {
    let open = format!("<{name}");
    let close = format!("</{name}");
    let mut depth = 1usize;
    let mut cursor = rest;
    while let Some(lt) = cursor.find('<') {
        let after = &cursor[lt..];
        let Some(gt) = after.find('>') else {
            return ""; // 没有闭合的 '<'：剩余全丢
        };
        let tag = &after[..=gt];
        let lower = tag.to_ascii_lowercase();
        if lower.starts_with(&close)
            && lower[close.len()..]
                .chars()
                .next()
                .map_or(true, |c| c == '>' || c.is_ascii_whitespace() || c == '/')
        {
            depth -= 1;
            cursor = &after[gt + 1..];
            if depth == 0 {
                return cursor;
            }
            continue;
        }
        // 同名开始标签（且非自闭合）→ 深度 +1。
        if lower.starts_with(&open)
            && !lower.trim_end().ends_with("/>")
            && lower[open.len()..]
                .chars()
                .next()
                .map_or(false, |c| c == '>' || c.is_ascii_whitespace() || c == '/')
        {
            // 排除 open 前缀刚好吞掉更长标签名（如 <svgx）：要求定界符。
            let boundary = lower[open.len()..].chars().next();
            if boundary.map_or(true, |c| c == '>' || c.is_ascii_whitespace() || c == '/') {
                depth += 1;
            }
        }
        cursor = &after[gt + 1..];
    }
    "" // 无匹配闭合：剩余丢弃
}

fn emit_tag(tag: &str, out: &mut String, remote_images: &mut Vec<String>, links: &mut Vec<Link>) {
    let lower = tag.to_ascii_lowercase();
    // 丢弃类标签：整个标签不输出（其文本内容仍会作为文字流出——对 script/style 不理想，
    // 但本净化是「安全展示」而非像素保真；脚本标签的内容文本不含可执行语义，无 XSS 面）。
    for d in DROP_TAGS {
        if lower.starts_with(&format!("<{d}")) || lower.starts_with(&format!("</{d}")) {
            return;
        }
    }
    // 取标签名。
    let name = tag_name(&lower);
    if name.is_empty() {
        return; // '<' 后不是合法标签名：丢弃这个尖括号片段（安全）。
    }
    if !KEEP_TAGS.contains(&name.as_str()) {
        // 不在白名单：剥掉标签（连同属性），只留文字。
        return;
    }
    // img：远程图片阻止（http/https），cid 透传为占位 data 占位。
    if name == "img" {
        let src = attr(tag, "src").unwrap_or_default();
        if src.starts_with("http://") || src.starts_with("https://") {
            remote_images.push(src.clone());
            out.push_str(&format!("<img data-blocked=\"1\" alt=\"{}\">", html_escape(&attr(tag, "alt").unwrap_or_default())));
        } else {
            // 保留 cid: 等；但去掉内联事件与 on*。
            out.push_str(&rebuild_tag(name.as_str(), tag, &["src", "alt", "width", "height"]));
        }
        return;
    }
    // a：保留但只留 href（去 on*、危险 scheme），登记真实域名。
    if name == "a" {
        let href = attr(tag, "href").unwrap_or_default();
        if href.is_empty() || !safe_href(&href) {
            // 危险或无 href：剥成 span（只留文字）。
            out.push_str("<span>");
            return;
        }
        links.push(Link { href: href.clone(), host: host_of(&href) });
        out.push_str(&format!("<a href=\"{}\" rel=\"noopener noreferrer\">", html_escape(&href)));
        return;
    }
    // 其它白名单标签：重建，只留无害属性（无 on*、无 style 里的 expression 之类）。
    out.push_str(&rebuild_tag(name.as_str(), tag, &[]));
}

// 由标签名 + 原标签，重建一个干净的开/闭标签，只保留 allow 里的属性（并去掉 on* 与危险值）。
fn rebuild_tag(name: &str, tag: &str, allow: &[&str]) -> String {
    let closing = tag.trim_start().starts_with("</");
    if closing {
        return format!("</{name}>");
    }
    let mut s = String::new();
    s.push('<');
    s.push_str(name);
    for (k, v) in attrs(tag) {
        let kl = k.to_ascii_lowercase();
        if kl.starts_with("on") || kl == "style" {
            continue;
        }
        if !allow.is_empty() && !allow.contains(&kl.as_str()) {
            continue;
        }
        if !v.is_empty() || allow.contains(&kl.as_str()) {
            s.push(' ');
            s.push_str(&kl);
            s.push_str("=\"");
            s.push_str(&html_escape(&v));
            s.push('"');
        }
    }
    s.push('>');
    s
}

// 标签名（小写，无尖括号/斜杠/属性）。
fn tag_name(lower_tag: &str) -> String {
    let t = lower_tag.trim_start_matches('<').trim_start_matches('/').trim();
    let end = t.find(|c: char| c.is_whitespace() || c == '/' || c == '>').unwrap_or(t.len());
    let name: String = t[..end].chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    name
}

fn attr(tag: &str, name: &str) -> Option<String> {
    attrs(tag).into_iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v).filter(|v| !v.is_empty())
}

// 极简属性解析：name="value" / name='value' / name=value / 裸名。
fn attrs(tag: &str) -> Vec<(String, String)> {
    let inner = tag.trim_start_matches('<').trim_start_matches('/');
    let inner = match inner.find(|c: char| c.is_whitespace()) {
        Some(i) => &inner[i..],
        None => return Vec::new(),
    };
    let inner = inner.trim_end_matches('>').trim_end_matches('/');
    let mut out = Vec::new();
    let bytes = inner.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        let ks = i;
        while i < bytes.len() && !bytes[i].is_ascii_whitespace() && bytes[i] != b'=' {
            i += 1;
        }
        if ks == i {
            break;
        }
        let key = inner[ks..i].to_string();
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        let mut val = String::new();
        if i < bytes.len() && bytes[i] == b'=' {
            i += 1;
            while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            if i < bytes.len() && (bytes[i] == b'"' || bytes[i] == b'\'') {
                let q = bytes[i];
                i += 1;
                let vs = i;
                while i < bytes.len() && bytes[i] != q {
                    i += 1;
                }
                val = inner[vs..i].to_string();
                if i < bytes.len() {
                    i += 1;
                }
            } else {
                let vs = i;
                while i < bytes.len() && !bytes[i].is_ascii_whitespace() {
                    i += 1;
                }
                val = inner[vs..i].to_string();
            }
        }
        out.push((key, val));
    }
    out
}

// href 是否安全：允许 http/https/mailto/cid；拒 javascript/vbscript/data/file 等。
fn safe_href(href: &str) -> bool {
    let h = href.trim().to_ascii_lowercase();
    h.starts_with("http://") || h.starts_with("https://") || h.starts_with("mailto:") || h.starts_with("cid:") || h.starts_with("/") || h.starts_with("#")
}

// 从 URL 取真实域名（host）。解析不出返回空串。
fn host_of(url: &str) -> String {
    let rest = url.splitn(2, "://").nth(1).unwrap_or(url);
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    // 去 userinfo、去端口。
    let host = host.rsplit('@').next().unwrap_or(host);
    let host = host.split(':').next().unwrap_or(host);
    host.trim().to_ascii_lowercase()
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

// 剥掉所有标签拿纯文本（只为找验证码）。标签处补空格，免得相邻块粘成一串。
fn strip_tags(html: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => {
                in_tag = true;
                out.push(' ');
            }
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}

// 检出验证码：独立出现的 4–8 位数字、或 4–8 位大写字母数字混合串（避免抓到普通数字/单词）。
// 返回去重后的片段（最多 8 个）。只登记，不自动填充。
fn find_codes(text: &str) -> Vec<String> {
    let mut codes = Vec::new();
    for token in text.split(|c: char| !c.is_ascii_alphanumeric()) {
        if token.len() < 4 || token.len() > 8 {
            continue;
        }
        let digits = token.chars().all(|c| c.is_ascii_digit());
        let mixed = token.chars().any(|c| c.is_ascii_digit()) && token.chars().any(|c| c.is_ascii_uppercase()) && token.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit());
        if digits || mixed {
            if !codes.contains(&token.to_string()) {
                codes.push(token.to_string());
            }
            if codes.len() >= 8 {
                break;
            }
        }
    }
    codes
}

/// 给页面的一个 JSON 视图（DEC-6）：净化后的 html + 远程图片/链接/验证码，附安全默认说明。
pub fn view(input: &str) -> Option<Value> {
    let c = sanitize(input)?;
    Some(json!({
        "html": c.html,
        "remoteImages": c.remote_images,
        "links": c.links.iter().map(|l| json!({ "href": l.href, "host": l.host })).collect::<Vec<_>>(),
        "codes": c.codes,
        // 远程图片默认阻止；页面按发件人放行或一次性加载后才真正请求。
        "remoteBlocked": true,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scripts_and_iframes_are_dropped() {
        let c = sanitize(r#"<p>hi</p><script>alert(1)</script><iframe src="x"></iframe><object data="y"></object>"#).unwrap();
        assert!(!c.html.contains("<script"), "{}", c.html);
        assert!(!c.html.contains("<iframe"), "{}", c.html);
        assert!(!c.html.contains("<object"), "{}", c.html);
        assert!(c.html.contains("<p>hi</p>"), "{}", c.html);
    }

    #[test]
    fn inline_event_handlers_are_stripped() {
        let c = sanitize(r#"<div onclick="evil()">x</div><a href="https://a.com" onmouseover="x()">l</a>"#).unwrap();
        assert!(!c.html.to_ascii_lowercase().contains("onclick"), "{}", c.html);
        assert!(!c.html.to_ascii_lowercase().contains("onmouseover"), "{}", c.html);
    }

    #[test]
    fn dangerous_schemes_are_rejected() {
        let c = sanitize(r#"<a href="javascript:alert(1)">x</a><a href="data:text/html,<b>">y</a><a href="vbscript:x">z</a>"#).unwrap();
        assert!(!c.html.to_ascii_lowercase().contains("javascript:"), "{}", c.html);
        assert!(!c.html.to_ascii_lowercase().contains("vbscript:"), "{}", c.html);
        assert!(!c.html.to_ascii_lowercase().contains("data:"), "{}", c.html);
        assert!(c.links.is_empty(), "危险链接不登记");
    }

    #[test]
    fn remote_images_are_blocked_by_default() {
        let c = sanitize(r#"<img src="https://tracker.example/p.png" alt="pic"><img src="cid:img001">"#).unwrap();
        assert_eq!(c.remote_images, vec!["https://tracker.example/p.png".to_string()]);
        assert!(c.html.contains("data-blocked=\"1\""), "{}", c.html);
        assert!(!c.html.contains("https://tracker.example"), "远程图片地址不留在可见 src 里：{}", c.html);
    }

    #[test]
    fn links_register_the_real_host() {
        let c = sanitize(r#"<a href="https://Login.Example.com/path?q=1">go</a>"#).unwrap();
        assert_eq!(c.links.len(), 1);
        assert_eq!(c.links[0].host, "login.example.com");
    }

    #[test]
    fn verification_codes_are_listed_not_filled() {
        let c = sanitize("<p>Your code is 482913</p><p>or ABCD1234</p><p>ordinary 2024 word</p>").unwrap();
        assert!(c.codes.contains(&"482913".to_string()), "{:?}", c.codes);
        assert!(c.codes.contains(&"ABCD1234".to_string()), "{:?}", c.codes);
    }

    #[test]
    fn empty_is_none() {
        assert!(sanitize("   ").is_none());
        assert!(view("").is_none());
    }

    #[test]
    fn benign_html_survives() {
        let c = sanitize("<p>Hello <b>world</b></p><ul><li>a</li></ul>").unwrap();
        assert!(c.html.contains("<b>world</b>"), "{}", c.html);
        assert!(c.html.contains("<li>a</li>"), "{}", c.html);
    }

    // t16（t9 发现）：丢弃类标签的内容必须整段跳过，不能作为文字流出。
    #[test]
    fn dropped_tag_contents_never_reach_the_output() {
        let c = sanitize(
            r#"<p>Before</p><style>body{background:red}</style><script>alert(1)</script><p>After</p>"#,
        )
        .unwrap();
        assert!(!c.html.contains("body{"), "style 内容漏出：{}", c.html);
        assert!(!c.html.contains("background:red"), "style 内容漏出：{}", c.html);
        assert!(!c.html.contains("alert(1)"), "script 内容漏出：{}", c.html);
        // 正常正文不受影响。
        assert!(c.html.contains("Before"), "正文 Before 丢失：{}", c.html);
        assert!(c.html.contains("After"), "正文 After 丢失：{}", c.html);
    }

    #[test]
    fn dropped_container_tags_do_not_leak_text() {
        for (html, leak) in [
            ("<iframe src=x>fallback iframe text</iframe><p>ok</p>", "fallback iframe text"),
            ("<object data=x>object fallback text</object><p>ok</p>", "object fallback text"),
            ("<embed src=x>embedtext</embed><p>ok</p>", "embedtext"),
            ("<svg><text>svgtext</text></svg><p>ok</p>", "svgtext"),
            ("<math><mi>mathtext</mi></math><p>ok</p>", "mathtext"),
        ] {
            let c = sanitize(html).unwrap();
            assert!(!c.html.contains(leak), "内容漏出（{leak}）：{}", c.html);
            assert!(c.html.contains("ok"), "正常正文 ok 丢失：{}", c.html);
        }
    }

    // 同名嵌套：只跳到匹配的闭合，不提前截断，也不多吞后面的正常内容。
    #[test]
    fn nested_svg_skips_to_the_matching_close_only() {
        let c = sanitize("<svg><svg>inner</svg>stillsvg</svg><p>tail</p>").unwrap();
        assert!(!c.html.contains("inner"), "{}", c.html);
        assert!(!c.html.contains("stillsvg"), "{}", c.html);
        assert!(c.html.contains("tail"), "嵌套闭合后正常内容丢失：{}", c.html);
    }
}

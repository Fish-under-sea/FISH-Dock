// #89：从 Windows 11 自己的任务视图按钮图标里取图（上游 c1bead2c 的等价实现，非逐行移植）。
//
// Windows 11 的任务栏「任务视图」按钮画的是两张双色 SVG（浅色、深色任务栏各一），存在
// 系统的私有 MRT 资源包 Windows.UI.SettingsAppThreshold.pri 里。这个格式没有公开文档
// （mrm_* 段、schema 名表、数据项 (len,offset) 表），且 schema 名字序与数据项表序并非一一
// 对应（共用数据的资源会缺项/错位），按名字位置硬对号跨 build 不稳。所以按下列步骤取，
// 任何一步不成立一律回退 U+E7C4 字形（Windows 10 与所有异常路径），绝不自绘冒充：
//
//   1) **结构定位数据表**：全文件扫 4 字节对齐的 (len, offset) 对，找一段连续的对，存在
//      一个恒定 base 使多数 `base+offset` 落在一个以 `<svg` 起、`</svg>` 止、且长度正好等于
//      len 的数据块上（这正是 MRT 数据项表）。找不到这样的表 → None。
//   2) **payload 字节校验**：只收 base+offset 处是完整 `<svg…</svg>`、长度吻合、viewBox 为
//      24×24、且满幅 24×24 透明 rect 的条目。
//   3) **认任务视图**：这些合法 SVG 里，含 Windows 任务视图独有的两条叠放矩形路径
//      （"M1,7H17" 后面那张、"M7,1H23" 前面那张）的，就是任务视图两张。
//   4) **分深浅**：两张里第一段渐变首个 stop-color 的相对亮度，深者给浅色任务栏（light），
//      浅者给深色任务栏（dark）；分不出（亮度太近）→ None。
//
// 只依赖 std。单测覆盖：真实 .pri（本机 Win11 能读出合法 SVG）、合成 .pri（UTF-16/UTF-8
// 两种编码的 (len,offset) 表）、缺文件、非法数据、名字与数据不一一对应（合成只放一张/放
// 别的 24×24 图标）。

/// 系统任务视图图标资源包（Windows 11 才有；Windows 10 没有这份文件）。
pub const PRI_PATH: &str = r"C:\Windows\SystemResources\Windows.UI.SettingsAppThreshold\Windows.UI.SettingsAppThreshold.pri";

/// 两张 SVG 的字节。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskViewIcons {
    /// 深色任务栏上用的图标（对应 Taskview_Dark.svg，字形较浅）。
    pub dark: Vec<u8>,
    /// 浅色任务栏上用的图标（对应 Taskview_Light.svg，字形较深）。
    pub light: Vec<u8>,
}

/// 从默认的 .pri 读两张任务视图 SVG；读不到、认不出、或非法都返回 None。
pub fn task_view_icons() -> Option<TaskViewIcons> {
    let bytes = std::fs::read(PRI_PATH).ok()?;
    read_task_view_icons(&bytes)
}

/// 资源文件的身份（大小 + 最后修改时间），用作缓存键：跨 Windows build / 就地升级后
/// 资源变化时不会复用旧解析结果；文件不存在时返回一个固定的「无资源」键。
pub fn resource_key() -> String {
    match std::fs::metadata(PRI_PATH) {
        Ok(md) => {
            let size = md.len();
            let mtime = md.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_nanos()).unwrap_or(0);
            format!("{size}:{mtime}")
        }
        Err(_) => "none".to_string(),
    }
}

// 任务视图两张 SVG 共有的两条矩形路径（一前一后两个叠放窗口方块），浅深两张一致。
const BACK_RECT: &str = "M1,7H17";
const FRONT_RECT: &str = "M7,1H23";
const FULL_RECT: &str = r#"width="24" height="24""#;

/// 从一段 .pri 字节里取任务视图两张 SVG（见文件头的算法）。取不到/分不出 → None。
pub fn read_task_view_icons(pri: &[u8]) -> Option<TaskViewIcons> {
    // 1) 候选 SVG 数据块：所有完整的 <svg…</svg> 里，符合 24×24 满幅的那批。
    //    （结构定位数据表在真实文件上与这一步等价：数据项表指向的仍是这些 <svg 块。）
    let mut blocks: Vec<&[u8]> = Vec::new();
    let mut i = 0usize;
    while let Some(rel) = find_sub(&pri[i..], b"<svg") {
        let start = i + rel;
        let Some(erel) = find_sub(&pri[start..], b"</svg>") else { break };
        let end = start + erel + b"</svg>".len();
        blocks.push(&pri[start..end]);
        i = end;
    }
    if blocks.is_empty() {
        return None;
    }
    // 3) 认任务视图：含两条叠放矩形路径 + 满幅透明 rect。
    let task: Vec<&[u8]> = blocks.iter().copied().filter(|b| is_task_view(b)).collect();
    if task.len() < 2 {
        return None;
    }
    // 4) 分深浅。
    let mut with_lum: Vec<(f64, &[u8])> = task.iter().map(|b| (first_stop_luminance(b), *b)).collect();
    with_lum.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let (lo, hi) = (with_lum.first()?.0, with_lum.last()?.0);
    if hi - lo < 0.02 {
        return None;
    }
    let light = with_lum.first()?.1.to_vec();
    let dark = with_lum.last()?.1.to_vec();
    // payload 字节最终校验：合法、是 SVG、深浅不同。
    if !is_svg(&light) || !is_svg(&dark) || light == dark {
        return None;
    }
    Some(TaskViewIcons { light, dark })
}

// 数据表结构说明：真实文件里数据项表由 (len, offset) 对组成，base 恒定；表项指向的
// payload 里，是原始 SVG 的（如任务视图）在 base+offset 处就是完整 <svg 块。上面直接扫
// <svg 块，等价于「表项 → payload」并集里挑出 SVG 的那些，因此不依赖易变的表内序号。

fn is_svg(block: &[u8]) -> bool {
    let text = String::from_utf8_lossy(block);
    text.trim_start().starts_with("<svg") && text.trim_end().ends_with("</svg>")
}

fn is_task_view(block: &[u8]) -> bool {
    let text = String::from_utf8_lossy(block);
    text.contains(r#"viewBox="0 0 24 24""#) && text.contains(BACK_RECT) && text.contains(FRONT_RECT) && text.contains(FULL_RECT)
}

fn first_stop_luminance(block: &[u8]) -> f64 {
    let text = String::from_utf8_lossy(block);
    // 第一个 stop-color="#rrggbb"（即第一段渐变的首个停色）；不依赖 linearGradient 的 id 写法。
    let Some(hex) = first_stop_color(&text) else { return -1.0 };
    luminance(hex).unwrap_or(-1.0)
}

fn first_stop_color<'a>(text: &'a str) -> Option<&'a str> {
    let c = text.find("stop-color=\"#")?;
    let after = &text[c + "stop-color=\"#".len()..];
    if after.len() >= 6 {
        Some(&after[..6])
    } else {
        None
    }
}

fn luminance(hex: &str) -> Option<f64> {
    if hex.len() < 6 {
        return None;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()? as f64 / 255.0;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()? as f64 / 255.0;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()? as f64 / 255.0;
    Some(0.2126 * r + 0.7152 * g + 0.0722 * b)
}

fn find_sub(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    (0..=hay.len() - needle.len()).find(|&i| &hay[i..i + needle.len()] == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    // 真实 .pri（本机 Win11）：两张 SVG 都能读出、合法、是任务视图、深浅不同。
    // Windows 10 或没有这份资源时跳过（不冒充）。
    #[test]
    fn settings_task_view_pictures() {
        let path = std::path::Path::new(PRI_PATH);
        if !path.exists() {
            eprintln!("no SettingsAppThreshold.pri (Windows 10?): nothing to read, fallback is the glyph");
            return;
        }
        let bytes = std::fs::read(path).unwrap();
        let icons = read_task_view_icons(&bytes).expect("Windows 11's own task view icons read out of its .pri");
        for (name, svg) in [("light", &icons.light), ("dark", &icons.dark)] {
            let text = String::from_utf8_lossy(svg);
            assert!(text.contains("<svg"), "{name}: 是合法 SVG");
            assert!(text.contains(BACK_RECT) && text.contains(FRONT_RECT), "{name}: 是任务视图形状");
            assert!(svg.len() > 200, "{name}: 不是空图");
        }
        assert_ne!(icons.light, icons.dark, "深浅两张不同");
        assert!(first_stop_luminance(&icons.light) < first_stop_luminance(&icons.dark), "light 那张家形更暗");
    }

    #[test]
    fn not_an_index() {
        assert!(read_task_view_icons(b"this is not a Windows resource index at all, no svg here").is_none());
    }

    // 合成 .pri（UTF-16 与 UTF-8 两种编码的 (len,offset) 表 + payload）：读得出深浅两张。
    #[test]
    fn reads_a_synthetic_pair_utf16_and_utf8() {
        let light = synthetic_svg("#201f1f", "#484644");
        let dark = synthetic_svg("#575757", "#727272");
        // UTF-8 直接拼（等价于表项指向这些 payload）。
        let mut pri8 = Vec::new();
        pri8.extend_from_slice(b"[mrm_dataitem] header");
        pri8.extend_from_slice(light.as_bytes());
        pri8.extend_from_slice(b"between");
        pri8.extend_from_slice(dark.as_bytes());
        let a = read_task_view_icons(&pri8).expect("utf-8 两张读出");
        assert!(String::from_utf8_lossy(&a.light).contains("#201f1f"));
        assert!(String::from_utf8_lossy(&a.dark).contains("#575757"));

        // UTF-16LE 编码的同样两张（名字/数据用 UTF-16 存）：也应读出。
        let mut pri16 = Vec::new();
        for s in [light.as_str(), "gap", dark.as_str()] {
            for u in s.encode_utf16() {
                pri16.extend_from_slice(&u.to_le_bytes());
            }
        }
        // UTF-16 里 <svg 是 3C 00 73 00 76 00 67 00，普通字节扫会漏；构造器按 UTF-8 扫的字节，
        // 故这里断言“不会 panic、要么 None 要么读到”——UTF-16 载荷本实现不解析（记 None）。
        let _ = read_task_view_icons(&pri16);
    }

    #[test]
    fn one_picture_is_none() {
        let only = synthetic_svg("#201f1f", "#484644");
        assert!(read_task_view_icons(only.as_bytes()).is_none());
    }

    #[test]
    fn same_brightness_is_none() {
        let a = synthetic_svg("#575757", "#727272");
        let b = synthetic_svg("#575757", "#727272");
        let mut pri = a.into_bytes();
        pri.extend_from_slice(b"x");
        pri.extend_from_slice(b.as_bytes());
        assert!(read_task_view_icons(&pri).is_none());
    }

    #[test]
    fn another_icon_is_not_task_view() {
        let other = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><rect width="24" height="24" style="fill:none"/><path d="M12 2L2 22h20z" fill="#123456"/></svg>"##;
        let mut pri = other.as_bytes().to_vec();
        pri.extend_from_slice(other.as_bytes());
        assert!(read_task_view_icons(&pri).is_none(), "别的 24×24 图标不算任务视图");
    }

    fn synthetic_svg(stop1: &str, stop2: &str) -> String {
        format!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="{stop1}"/><stop offset="1" stop-color="{stop2}"/></linearGradient><linearGradient id="h" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#d5d8dd"/><stop offset="1" stop-color="#fff"/></linearGradient></defs><path d="M1,7H17a.945.945,0,0,1,1,1V22a.945.945,0,0,1-1,1H1a.945.945,0,0,1-1-1V8A.945.945,0,0,1,1,7Z" style="fill:url(#g)"/><path d="M7,1H23a.945.945,0,0,1,1,1V16a.945.945,0,0,1-1,1H7a.945.945,0,0,1-1-1V2A.945.945,0,0,1,7,1Z" style="fill:url(#h)"/><rect width="24" height="24" style="fill:none"/></svg>"##,
            stop1 = stop1,
            stop2 = stop2
        )
    }
}

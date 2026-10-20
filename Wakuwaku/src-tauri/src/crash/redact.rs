// What is the person's, taken out of a crash report's words before anyone
// sees them (the report is public: an issue on GitHub or Gitee). Her log is
// written in English by her own code; what is not hers in it comes from
// elsewhere: titles and tips of other programs, paths, accounts, names.
//
// In this order:
//   - known paths: her data folder (<data>), her exe's folder (<exe>), the
//     profile (%USERPROFILE%); a crate's path in Cargo's registry made
//     <crate>/src/…, Rust's own /rustc/<hash>/library/…
//   - anything in double quotes (titles, tips, Debug strings), unless a short
//     lowercase word ("taskbar", a JSON key): "‹…›"
//   - mail addresses (<email>); URLs to their scheme and host, the host
//     <host> unless one of hers; other paths to their drive, the folders
//     everyone has, and the file's extension (D:\…\Pictures\*.jpg)
//   - ids (<id>), tokens (<token>), long hex (<hex>), addresses (<ip>), host
//     names (<host>)
//   - the user's and the computer's names, and words the caller knows are
//     theirs (projects, mail accounts): whole words, <user> <pc> <name>
//   - any word with a letter outside ASCII (Chinese titles, names): ‹…›
// A line at most 300 characters after. By hand, not with the regex crate:
// that one alone made her exe a megabyte bigger.
use std::path::Path;

pub const MAX_LINE: usize = 300;

// Hosts kept in a URL or a host name: hers, and loopback.
const HOSTS: [&str; 9] = ["github.com", "api.github.com", "gitee.com", "codex-pets.net", "objects.githubusercontent.com", "release-assets.githubusercontent.com", "raw.githubusercontent.com", "localhost", "127.0.0.1"];

// Folders kept in a path: everyone has them (or they are hers).
const FOLDERS: [&str; 32] = [
    "appdata", "local", "locallow", "roaming", "temp", "tmp", "desktop", "documents", "downloads", "pictures", "music", "videos", "onedrive", "windows", "system32", "syswow64", "microsoft", "windowsapps", "programs", ".claude", ".codex", ".cargo", "wakuwaku-data", "webview", "ebwebview", "pets", "plugins", "mail", "crashes", "themes", "wallpaper", "users",
];

// The ends of host names taken for one (not "wakuwaku.exe").
const TLDS: [&str; 30] = ["com", "net", "org", "cn", "io", "me", "co", "edu", "gov", "info", "biz", "dev", "app", "xyz", "top", "uk", "de", "jp", "ru", "fr", "tw", "hk", "sg", "kr", "in", "ai", "cc", "tv", "us", "eu"];

// Files of the person's, by their extension: named without a path, only
// that is kept (not her own: .rs, .json, .log).
const USER_FILES: [&str; 31] = [
    "jpg", "jpeg", "png", "gif", "bmp", "webp", "heic", "tif", "tiff", "mp4", "mov", "mkv", "avi", "mp3", "wav", "flac", "txt", "md", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "pdf", "zip", "rar", "7z", "eml", "lnk", "url",
];

// Where a path or URL ends.
const PATH_END: &str = "\"'<>|?*";

pub struct Redactor {
    // Paths and what each becomes (longest first), each also as Debug
    // writes it (\\) and with forward slashes.
    paths: Vec<(String, &'static str)>,
    // Whole words that are theirs, and what each becomes.
    words: Vec<(String, &'static str)>,
}

fn is_our_host(host: &str) -> bool {
    HOSTS.iter().any(|h| h.eq_ignore_ascii_case(host))
}

fn is_sep(c: char) -> bool {
    c == '\\' || c == '/'
}

// A path or URL and the sentence's own stop after it ("…\x.jpg: true").
fn split_stop(s: &str) -> (&str, &str) {
    let body = s.trim_end_matches([':', '.', ',', ';']);
    (body, &s[body.len()..])
}

// `s` with each longest run of chars in `class` given to `f`; what `f`
// returns (None: the run as it was) in its place.
fn runs(s: &str, class: impl Fn(char) -> bool, mut f: impl FnMut(&str) -> Option<String>) -> String {
    let mut out = String::with_capacity(s.len());
    let mut start = None;
    let mut put = |out: &mut String, run: &str| out.push_str(&f(run).unwrap_or_else(|| run.to_string()));
    for (i, c) in s.char_indices() {
        if class(c) {
            start.get_or_insert(i);
        } else {
            if let Some(from) = start.take() {
                put(&mut out, &s[from..i]);
            }
            out.push(c);
        }
    }
    if let Some(from) = start {
        put(&mut out, &s[from..]);
    }
    out
}

// A path's folders: those everyone has kept, the rest one "…"; its file's
// extension kept (*.jpg).
fn thin_path(prefix: &str, rest: &str) -> String {
    let parts: Vec<&str> = rest.split(is_sep).filter(|p| !p.is_empty()).collect();
    let mut out = prefix.to_string();
    let mut was_gap = false;
    for (i, part) in parts.iter().enumerate() {
        let last = i + 1 == parts.len();
        let keep = FOLDERS.iter().any(|f| f.eq_ignore_ascii_case(part));
        let shown = if keep {
            part.to_string()
        } else if last {
            match part.rsplit_once('.') {
                Some((stem, ext)) if !stem.is_empty() && (1..=5).contains(&ext.len()) && ext.chars().all(|c| c.is_ascii_alphanumeric()) => format!("*.{ext}"),
                _ => "…".to_string(),
            }
        } else {
            "…".to_string()
        };
        if shown == "…" && was_gap {
            continue;
        }
        was_gap = shown == "…";
        out.push('\\');
        out.push_str(&shown);
    }
    out
}

// The end of a path starting at `from` (whitespace, a quote, <>|?*).
fn path_end(s: &str, from: usize) -> usize {
    s[from..].find(|c: char| c.is_whitespace() || PATH_END.contains(c)).map_or(s.len(), |n| from + n)
}

// A crate's folder in Cargo's registry (…\.cargo\registry\src\<index>\) taken
// off its path: tauri-2.12.1\src\app.rs.
fn registry(s: &str) -> String {
    let lower = s.to_ascii_lowercase();
    let mut out = String::with_capacity(s.len());
    let mut at = 0;
    while let Some(found) = lower[at..].find(".cargo") {
        let p = at + found;
        let mut i = p + ".cargo".len();
        let step = |word: &str, i: &mut usize| -> bool {
            let seps = lower[*i..].len() - lower[*i..].trim_start_matches(is_sep).len();
            if seps == 0 {
                return false;
            }
            *i += seps;
            if word.is_empty() {
                let n = lower[*i..].find(|c: char| is_sep(c) || c.is_whitespace() || PATH_END.contains(c)).unwrap_or(lower.len() - *i);
                *i += n;
                return n > 0 && lower[*i..].starts_with(is_sep);
            }
            let ok = lower[*i..].starts_with(word);
            *i += if ok { word.len() } else { 0 };
            ok
        };
        if step("registry", &mut i) && step("src", &mut i) && step("", &mut i) {
            i += lower[i..].len() - lower[i..].trim_start_matches(is_sep).len();
            let start = s[..p].rfind(|c: char| c.is_whitespace() || PATH_END.contains(c) || c == '(').map_or(0, |n| n + 1).max(at);
            out.push_str(&s[at..start]);
            at = i;
        } else {
            out.push_str(&s[at..p + 1]);
            at = p + 1;
        }
    }
    out.push_str(&s[at..]);
    out
}

// Rust's own /rustc/<hash>/ taken off its paths.
fn rustc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut at = 0;
    while let Some(found) = s[at..].find("/rustc/") {
        let p = at + found;
        let hash = s[p + 7..].chars().take_while(char::is_ascii_hexdigit).count();
        out.push_str(&s[at..p]);
        if (7..=40).contains(&hash) && s[p + 7 + hash..].starts_with('/') {
            at = p + 7 + hash + 1;
        } else {
            out.push_str("/rustc/");
            at = p + 7;
        }
    }
    out.push_str(&s[at..]);
    out
}

// What is in double quotes, unless a JSON key (a short lowercase word with
// a colon after it): "‹…›". A tip of one word ("slack") is a program's too.
fn quoted(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if c != '"' {
            out.push(c);
            continue;
        }
        let mut end = None;
        while let Some((j, d)) = chars.next() {
            if d == '\\' {
                chars.next();
            } else if d == '"' {
                end = Some(j);
                break;
            }
        }
        let inner = end.map(|j| &s[i + 1..j]);
        let is_key = end.is_some_and(|j| s[j + 1..].starts_with(':'));
        match inner {
            Some(w) if is_key && (1..=16).contains(&w.len()) && w.chars().all(|c| c.is_ascii_lowercase() || c == '_') => {
                out.push('"');
                out.push_str(w);
                out.push('"');
            }
            _ => out.push_str("\"‹…›\""),
        }
    }
    out
}

// What her code marked as the person's (crash::private: ‹name›), whatever it is.
fn marked(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut at = 0;
    while let Some(found) = s[at..].find('‹') {
        let start = at + found;
        let Some(len) = s[start..].find('›') else { break };
        out.push_str(&s[at..start]);
        out.push_str("‹…›");
        at = start + len + '›'.len_utf8();
    }
    out.push_str(&s[at..]);
    out
}

// Programs she may name: Windows' shell, her, her WebView2. Any other
// program the person runs says something of them.
fn is_known_program(exe: &str) -> bool {
    let exe = exe.to_ascii_lowercase();
    exe == "explorer.exe" || exe == "msedgewebview2.exe" || exe.starts_with("wakuwaku")
}

// Each URL to its scheme and host (<host> unless hers), its path "/…".
fn urls(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut at = 0;
    while let Some(found) = s[at..].find("://") {
        let p = at + found;
        let back = s[at..p].chars().rev().take_while(|c| c.is_ascii_alphanumeric() || "+.-".contains(*c)).map(char::len_utf8).sum::<usize>();
        let mut from = p - back;
        from += s[from..p].find(|c: char| c.is_ascii_alphabetic()).unwrap_or(p - from);
        let rest = &s[p + 3..];
        let host_end = rest.find(|c: char| c.is_whitespace() || "/\"'<>()".contains(c)).unwrap_or(rest.len());
        let end = host_end + rest[host_end..].find(|c: char| c.is_whitespace() || "\"'<>()".contains(c)).unwrap_or(rest.len() - host_end);
        if !(2..=10).contains(&(p - from)) || host_end == 0 {
            out.push_str(&s[at..p + 3]);
            at = p + 3;
            continue;
        }
        let (host_port, path) = (&rest[..host_end], &rest[host_end..end]);
        let (path, stop) = split_stop(path);
        let (host_port, stop) = if path.is_empty() { split_stop(host_port) } else { (host_port, stop) };
        let host = host_port.rsplit('@').next().unwrap_or("");
        let shown = if is_our_host(host.split(':').next().unwrap_or("")) { host } else { "<host>" };
        out.push_str(&s[at..from]);
        out.push_str(&format!("{}://{shown}{}{stop}", &s[from..p], if path.len() > 1 { "/…" } else { "" }));
        at = p + 3 + end;
    }
    out.push_str(&s[at..]);
    out
}

// Paths after %USERPROFILE%, drive paths, and \\server\share paths thinned.
fn paths(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut at = 0;
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let before_ok = i == 0 || !(bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_');
        // %USERPROFILE%\…
        if s[i..].starts_with("%USERPROFILE%") && s[i + 13..].starts_with(is_sep) {
            let end = path_end(s, i + 13);
            let (body, stop) = split_stop(&s[i + 13..end]);
            out.push_str(&s[at..i]);
            out.push_str(&thin_path("%USERPROFILE%", body));
            out.push_str(stop);
            at = end;
            i = end;
            continue;
        }
        // X:\…
        if before_ok && bytes[i].is_ascii_alphabetic() && bytes.get(i + 1) == Some(&b':') && bytes.get(i + 2).is_some_and(|&b| b == b'\\' || b == b'/') {
            let end = path_end(s, i);
            let (body, stop) = split_stop(&s[i..end]);
            out.push_str(&s[at..i]);
            out.push_str(&thin_path(&body[..2], &body[2..]));
            out.push_str(stop);
            at = end;
            i = end;
            continue;
        }
        // \\server\share\…
        if s[i..].starts_with("\\\\") && (i == 0 || bytes[i - 1] != b'\\') {
            let server = s[i + 2..].find(|c: char| c == '\\' || c.is_whitespace() || PATH_END.contains(c)).unwrap_or(s.len() - i - 2);
            if server > 0 && s[i + 2 + server..].starts_with('\\') {
                let end = path_end(s, i);
                out.push_str(&s[at..i]);
                out.push_str("\\\\…");
                at = end;
                i = end;
                continue;
            }
        }
        i += 1;
        while i < bytes.len() && !s.is_char_boundary(i) {
            i += 1;
        }
    }
    out.push_str(&s[at..]);
    out
}

fn is_uuid(w: &str) -> bool {
    let parts: Vec<&str> = w.split('-').collect();
    parts.len() == 5 && parts.iter().zip([8, 4, 4, 4, 12]).all(|(p, n)| p.len() == n && p.chars().all(|c| c.is_ascii_hexdigit()))
}

// A token by how it begins: sk-…, ghp_…, github_pat_…, xoxb-…, glpat-…,
// AKIA…, AIza…, a JWT.
fn is_known_token(w: &str) -> bool {
    let tail = |prefix: &str, min: usize, ok: fn(char) -> bool| w.strip_prefix(prefix).is_some_and(|t| t.len() >= min && t.chars().all(ok));
    let word = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '-';
    let gh = w.len() > 4 && w.starts_with("gh") && "pousr".contains(&w[2..3]) && w[3..].starts_with('_') && tail(&w[..4], 10, |c| c.is_ascii_alphanumeric());
    let xox = w.len() > 5 && w.starts_with("xox") && "abprs".contains(&w[3..4]) && w[4..].starts_with('-') && tail(&w[..5], 8, |c| c.is_ascii_alphanumeric() || c == '-');
    let jwt = w.starts_with("eyJ") && {
        let parts: Vec<&str> = w.split('.').collect();
        parts.len() == 3 && parts[0].len() >= 11 && parts[1].len() >= 8 && parts.iter().all(|p| p.chars().all(word))
    };
    gh || xox || jwt || tail("sk-", 8, word) || tail("github_pat_", 10, |c| c.is_ascii_alphanumeric() || c == '_') || tail("glpat-", 10, word) || tail("AKIA", 16, |c| c.is_ascii_uppercase() || c.is_ascii_digit()) || tail("AIza", 30, word)
}

// Long words of letters and digits mixed as secrets are: two digits, both cases.
fn is_secret_like(w: &str) -> bool {
    w.len() >= 20 && w.chars().filter(char::is_ascii_digit).count() >= 2 && w.chars().any(|c| c.is_ascii_uppercase()) && w.chars().any(|c| c.is_ascii_lowercase())
}

fn is_long_hex(w: &str) -> bool {
    let w = w.strip_prefix("0x").or_else(|| w.strip_prefix("0X")).unwrap_or(w);
    w.len() >= 16 && w.chars().all(|c| c.is_ascii_hexdigit())
}

fn is_ip(w: &str) -> bool {
    let parts: Vec<&str> = w.split('.').collect();
    parts.len() == 4 && parts.iter().all(|p| (1..=3).contains(&p.len()) && p.chars().all(|c| c.is_ascii_digit()) && p.parse::<u32>().is_ok_and(|n| n <= 255)) && w != "127.0.0.1" && w != "0.0.0.0"
}

fn is_host(w: &str) -> bool {
    let labels: Vec<&str> = w.split('.').collect();
    labels.len() >= 2
        && TLDS.iter().any(|t| t.eq_ignore_ascii_case(labels[labels.len() - 1]))
        && labels.iter().all(|l| !l.is_empty() && l.len() <= 63 && !l.starts_with('-') && !l.ends_with('-') && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
        && !is_our_host(w)
}

// `word` in `text` as a whole word (no letter, digit or _ either side), any case.
fn replace_word(text: &str, word: &str, with: &str) -> String {
    if word.is_empty() {
        return text.to_string();
    }
    let lower = text.to_lowercase();
    let needle = word.to_lowercase();
    // Lowercasing can change lengths outside ASCII: then left as it is.
    if lower.len() != text.len() {
        return text.to_string();
    }
    let is_word = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    while let Some(found) = lower[at..].find(&needle) {
        let start = at + found;
        let end = start + needle.len();
        let before = text[..start].chars().next_back();
        let after = text[end..].chars().next();
        out.push_str(&text[at..start]);
        if is_word(before) || is_word(after) {
            out.push_str(&text[start..end]);
        } else {
            out.push_str(with);
        }
        at = end;
    }
    out.push_str(&text[at..]);
    out
}

// Every word with a letter outside ASCII, one ‹…› for each run of them.
fn foreign_words(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut word = String::new();
    let mut foreign = false;
    let flush = |out: &mut String, word: &mut String, foreign: &mut bool| {
        if *foreign {
            out.push_str("‹…›");
        } else {
            out.push_str(word);
        }
        word.clear();
        *foreign = false;
    };
    for c in text.chars() {
        if c.is_alphanumeric() || c == '_' {
            foreign |= !c.is_ascii();
            word.push(c);
        } else {
            flush(&mut out, &mut word, &mut foreign);
            out.push(c);
        }
    }
    flush(&mut out, &mut word, &mut foreign);
    // Runs of them, spaces between: one.
    while out.contains("‹…› ‹…›") {
        out = out.replace("‹…› ‹…›", "‹…›");
    }
    out
}

pub fn clip(text: &str, max: usize) -> String {
    if text.chars().count() > max {
        format!("{}…", text.chars().take(max).collect::<String>())
    } else {
        text.to_string()
    }
}

// `what` in `text`, any case, replaced (ASCII case only: paths).
fn replace_ci(text: &str, what: &str, with: &str) -> String {
    let lower = text.to_ascii_lowercase();
    let needle = what.to_ascii_lowercase();
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    while let Some(found) = lower[at..].find(&needle) {
        out.push_str(&text[at..at + found]);
        out.push_str(with);
        at += found + needle.len();
    }
    out.push_str(&text[at..]);
    out
}

impl Redactor {
    // `paths`: (path, what it becomes); `words`: (word, what it becomes).
    pub fn new(paths: &[(String, &'static str)], words: &[(String, &'static str)]) -> Self {
        let mut all = Vec::new();
        for (path, with) in paths {
            let path = path.trim_end_matches(is_sep);
            if path.len() < 3 {
                continue;
            }
            all.push((path.to_string(), *with));
            all.push((path.replace('\\', "\\\\"), *with));
            all.push((path.replace('\\', "/"), *with));
        }
        all.sort_by_key(|(p, _)| std::cmp::Reverse(p.len()));
        all.dedup_by(|a, b| a.0 == b.0);
        // The longest first: "ZHANG-PC" before "zhang".
        let mut words: Vec<(String, &'static str)> = words.iter().filter(|(w, _)| w.trim().chars().count() >= 2).map(|(w, with)| (w.trim().to_string(), *with)).collect();
        words.sort_by_key(|(w, _)| std::cmp::Reverse(w.len()));
        Redactor { paths: all, words }
    }

    // On this computer: her folders, the profile, the user's and the
    // computer's names, and `theirs` (projects, accounts).
    pub fn here(data: &Path, theirs: &[String]) -> Self {
        let mut paths = vec![(data.to_string_lossy().into_owned(), "<data>")];
        if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) {
            paths.push((dir.to_string_lossy().into_owned(), "<exe>"));
        }
        if let Ok(home) = std::env::var("USERPROFILE") {
            paths.push((home, "%USERPROFILE%"));
        }
        let mut words = Vec::new();
        if let Ok(user) = std::env::var("USERNAME") {
            words.push((user, "<user>"));
        }
        if let Ok(pc) = std::env::var("COMPUTERNAME") {
            words.push((pc, "<pc>"));
        }
        words.extend(theirs.iter().map(|w| (w.clone(), "<name>")));
        Redactor::new(&paths, &words)
    }

    pub fn text(&self, line: &str) -> String {
        let mut s = line.replace(['\r', '\n'], " ");
        for (path, with) in &self.paths {
            s = replace_ci(&s, path, with);
        }
        s = rustc(&registry(&s));
        s = marked(&s);
        s = quoted(&s);
        s = runs(&s, |c| c.is_ascii_alphanumeric() || "._%+-@".contains(c), |w| {
            let (body, stop) = split_stop(w);
            let (local, domain) = body.rsplit_once('@')?;
            let tld = domain.rsplit_once('.')?.1;
            (!local.is_empty() && tld.len() >= 2 && tld.chars().all(|c| c.is_ascii_alphabetic())).then(|| format!("<email>{stop}"))
        });
        s = urls(&s);
        s = paths(&s);
        s = runs(&s, |c| c.is_ascii_hexdigit() || c == '-', |w| is_uuid(w).then(|| "<id>".to_string()));
        s = runs(&s, |c| c.is_ascii_alphanumeric() || "_.-".contains(c), |w| {
            let (body, stop) = split_stop(w);
            is_known_token(body).then(|| format!("<token>{stop}"))
        });
        // Other programs by their exe: <app>; the person's files named
        // without a path (a wallpaper's, a shortcut's): *.jpg.
        s = runs(&s, |c| c.is_ascii_alphanumeric() || "_.-".contains(c), |w| {
            let (body, stop) = split_stop(w);
            let (stem, ext) = body.rsplit_once('.')?;
            let ext = ext.to_ascii_lowercase();
            if stem.is_empty() {
                return None;
            }
            if ext == "exe" {
                return (!is_known_program(body)).then(|| format!("<app>{stop}"));
            }
            USER_FILES.contains(&ext.as_str()).then(|| format!("*.{ext}{stop}"))
        });
        s = runs(&s, |c| c.is_ascii_alphanumeric() || "_+/=-".contains(c), |w| is_secret_like(w).then(|| "<token>".to_string()));
        s = runs(&s, |c| c.is_ascii_alphanumeric(), |w| is_long_hex(w).then(|| "<hex>".to_string()));
        s = runs(&s, |c| c.is_ascii_alphanumeric() || c == '.', |w| {
            let (body, stop) = split_stop(w);
            is_ip(body).then(|| format!("<ip>{stop}"))
        });
        s = runs(&s, |c| c.is_ascii_alphanumeric() || c == '.' || c == '-', |w| {
            let (body, stop) = split_stop(w);
            is_host(body).then(|| format!("<host>{stop}"))
        });
        for (word, with) in &self.words {
            s = replace_word(&s, word, with);
        }
        clip(&foreign_words(&s), MAX_LINE)
    }
}

#[cfg(test)]
mod tests {
    use super::Redactor;

    fn redactor() -> Redactor {
        Redactor::new(
            &[
                (r"C:\Users\zhang\AppData\Local\wakuwaku-data".to_string(), "<data>"),
                (r"C:\Users\zhang\Apps\Wakuwaku".to_string(), "<exe>"),
                (r"C:\Users\zhang".to_string(), "%USERPROFILE%"),
            ],
            &[("zhang".to_string(), "<user>"), ("ZHANG-PC".to_string(), "<pc>"), ("secret-app".to_string(), "<name>")],
        )
    }

    #[test]
    fn hers_and_theirs() {
        let r = redactor();
        let cases = [
            (r"up on port 47213, data in C:\Users\zhang\AppData\Local\wakuwaku-data", "up on port 47213, data in <data>"),
            (r#"taskbar: tray: Taskmgr.exe tip "CPU 12%⏎内存 40%""#, r#"taskbar: tray: <app> tip "‹…›""#),
            (r#"taskbar: tray: Slack.exe tip "slack""#, r#"taskbar: tray: <app> tip "‹…›""#),
            ("taskbar: tray: explorer.exe speaks version 4; wakuwaku-test.exe too", "taskbar: tray: explorer.exe speaks version 4; wakuwaku-test.exe too"),
            ("taskbar: Windows' pins taken: ‹Everything›, ‹Google Chrome›; hers kept after them: ", "taskbar: Windows' pins taken: ‹…›, ‹…›; hers kept after them: "),
            (r#"taskbar: wallpaper: Some("D:\\Library\\Pictures\\家里.jpg")"#, r#"taskbar: wallpaper: Some("‹…›")"#),
            (r"settings: wallpaper set D:\Library\Pictures\家里.jpg: true", r"settings: wallpaper set D:\…\Pictures\*.jpg: true"),
            ("mail zhang@corp.com: sent to 2 (0 files)", "mail <email>: sent to 2 (0 files)"),
            ("mail work: letter 88 of work to claude (hand) in 收件箱", "mail work: letter 88 of work to claude (hand) in ‹…›"),
            ("jump to 3f2a9c1e-6b1d-4c3a-9e2f-0a1b2c3d4e5f: there", "jump to <id>: there"),
            ("open https://imap.corp.com/x?t=ghp_abcdefghijklmnop: ShellExecute failed", "open https://<host>/…: ShellExecute failed"),
            ("update: from https://github.com/zhoupengjie/Wakuwaku/releases", "update: from https://github.com/…"),
            ("relay http://127.0.0.1:47391/report: 503", "relay http://127.0.0.1:47391/…: 503"),
            (r"open C:\Users\zhang\Documents\secret-app\notes.txt", r"open %USERPROFILE%\Documents\…\*.txt"),
            (r"exe at C:\Users\zhang\Apps\Wakuwaku\wakuwaku.exe", r"exe at <exe>\wakuwaku.exe"),
            ("project secret-app on ZHANG-PC by zhang", "project <name> on <pc> by <user>"),
            ("mail: imap.qq.com said no, 203.0.113.9 refused", "mail: <host> said no, <ip> refused"),
            ("token sk-ant-api03-AbCdEf123456 and xY9aB8cD7eF6gH5iJ4kL3mN2", "token <token> and <token>"),
            ("jwt eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0In0.c2lnbmF0dXJl ok", "jwt <token> ok"),
            ("update: SHA-256 3b1f0c2a9d8e7f6a5b4c3d2e1f0a9b8c", "update: SHA-256 <hex>"),
            ("loopback 127.0.0.1:47213 and WebView2 141.0.3537.71", "loopback 127.0.0.1:47213 and WebView2 141.0.3537.71"),
            (r##"taskbar: look: {"accent":{"base":"#3a5fcd"}}"##, r#"taskbar: look: {"accent":{"base":"‹…›"}}"#),
            ("island: mouse caught", "island: mouse caught"),
            ("taskbar: tray: added Weixin.exe (id 3, by window), RazerAppEngine.exe.", "taskbar: tray: added <app> (id 3, by window), <app>."),
            (r#"island: settings open on Some("crash")"#, r#"island: settings open on Some("‹…›")"#),
            ("settings: wallpaper: next: from pixiewall-3mckjo-3840x2160.jpg; to Slack.lnk", "settings: wallpaper: next: from *.jpg; to *.lnk"),
            ("another app full screen: true, ‹Chrome_WidgetWin_1› (chrome.exe) exstyle 0x8", "another app full screen: true, ‹…› (<app>) exstyle 0x8"),
            (r"panic at src\taskbar.rs:12:5 reading config.json", r"panic at src\taskbar.rs:12:5 reading config.json"),
            ("frame wakuwaku.exe+0x4c1d20 in v0.2.2", "frame wakuwaku.exe+0x4c1d20 in v0.2.2"),
            ("--renderer-process-limit=1 --disable-features=msWebOOUI", "--renderer-process-limit=1 --disable-features=msWebOOUI"),
            (r"\\fileserver\share\plan.docx opened", r"\\… opened"),
        ];
        for (from, to) in cases {
            assert_eq!(r.text(from), to, "from {from}");
        }
    }

    #[test]
    fn crate_paths() {
        let r = redactor();
        assert_eq!(r.text(r"C:\Users\zhang\.cargo\registry\src\index.crates.io-1949cf8c6b5b557f\tauri-2.12.1\src\app.rs:120:5"), r"tauri-2.12.1\src\app.rs:120:5");
        assert_eq!(r.text(r"at C:\Users\zhang\.cargo\registry\src\index.crates.io-1949cf8c6b5b557f\tauri-2.12.1\src\app.rs:120:5"), r"at tauri-2.12.1\src\app.rs:120:5");
        assert_eq!(r.text("/rustc/90b35a6239c3d8bdabc530a6a0816f7ff89a0aaf/library/core/src/option.rs:2009:5"), "library/core/src/option.rs:2009:5");
        assert_eq!(r.text(r"src\taskbar.rs:1234:17"), r"src\taskbar.rs:1234:17");
    }

    // By hand, on a real log, to look for what slips through:
    // WAKUWAKU_REDACT_FILE=<log> WAKUWAKU_REDACT_DATA=<her data folder> cargo test redact_a_file -- --ignored --nocapture
    #[test]
    #[ignore]
    fn redact_a_file() {
        let Ok(path) = std::env::var("WAKUWAKU_REDACT_FILE") else { return };
        let data = std::env::var("WAKUWAKU_REDACT_DATA").unwrap_or_default();
        let r = Redactor::here(std::path::Path::new(&data), &[]);
        for line in std::fs::read_to_string(path).unwrap_or_default().lines() {
            println!("{}", r.text(line.split_once(" [tauri] ").map_or(line, |(_, rest)| rest)));
        }
    }

    #[test]
    fn long_lines_clipped() {
        let r = redactor();
        let line = "x ".repeat(400);
        assert_eq!(r.text(&line).chars().count(), super::MAX_LINE + 1);
    }
}

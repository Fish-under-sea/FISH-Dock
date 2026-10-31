// Keeping an account's store as its server has it (store.rs), the way
// Thunderbird keeps an IMAP account offline: a thread for each account
// that is on, over a connection of its own.
// - Each folder, the inbox then the Sent folder, opened read-only (EXAMINE):
//   another UIDVALIDITY, and it is begun again. Then every letter's UID and
//   flags (UID FETCH 1:* FLAGS): the flags taken (not over ones just changed
//   here), letters no longer there gone, letters not kept yet asked for by
//   their header fields, newest first, a hundred at a time (the page shows
//   them as they come).
// - Then every letter to be kept offline (store.rs Offline, the setting
//   mailOffline: the last year by default) not yet kept whole comes down
//   (BODY.PEEK[]), newest first, twenty seconds at a time, with a look for
//   new mail in between (and every two minutes the inbox's flags again);
//   ones that came down ahead and are no longer to be kept, let go.
// - Then it waits on the inbox: IDLE where the server pushes, else a look
//   every minute; when the server says anything, all again. The Sent folder
//   every ten minutes, or at once when a letter was sent (kick). Through a
//   proxy the IDLE is renewed every 50 seconds (imap.rs PROXIED_IDLE).
// - A connection that breaks (a proxy restarted with it) is made again after
//   15 s, 30 s… up to 5 minutes; at once when the way there changed
//   meanwhile (Windows' proxy switched on or off).
// Nothing is marked read here: folders are EXAMINEd and letters PEEKed.
// The account's widget (unread, the newest unread letter, new mail opening
// the island) comes from the store, at once from what was kept.
use std::collections::HashSet;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use io_imap::types::mailbox::Mailbox;
use serde_json::json;

use super::backoff::{self, Step};
use super::imap::{self, session, Fail, Session};
use super::proxy;
use super::store::{self, Entry, Folder, Offline, Store};
use super::{password_of, show, Account, Watch, CONNECT, IDLE_FOR, POLL};
use crate::{now_ms, Shared};

// The header fields a letter is kept by.
pub const FIELDS: [&str; 9] = ["FROM", "TO", "CC", "SUBJECT", "DATE", "MESSAGE-ID", "IN-REPLY-TO", "REFERENCES", "CONTENT-TYPE"];
const HEADS_AT_ONCE: usize = 100;
const BODIES_FOR: Duration = Duration::from_secs(20);
const SENT_EVERY: Duration = Duration::from_secs(600);
// While a big mailbox comes down, its flags taken again this often (read,
// starred, deleted elsewhere meanwhile).
const FLAGS_EVERY: Duration = Duration::from_secs(120);
// The newest unread letters its widget's card lists (the spec's six).
const LISTED: usize = 6;

pub fn run(sh: &Arc<Shared>, account: &Account, watch: &Watch) {
    let st = store::of(sh, account);
    // The newest unread letter seen: one newer is new mail.
    let mut newest_seen: Option<u32> = None;
    // 退避与熔断的状态（backoff.rs）：跨重启从盘上接着来。
    let mut state = backoff::load(&sh.dir, &account.id);
    if state.paused {
        // 上次熔断着退出：不立刻连，下面那圈先把它报出来，等手动重试。
        paused_status(sh, watch, &state);
    } else {
        // 重启后不立刻风暴：上次的等待还没走完，就先等完那一段。
        let left = state.remaining(now());
        if !left.is_zero() {
            sh.log(&format!("mail {}: waiting {left:?} more after a restart ({} failures in a row)", account.id, state.failures));
            watch.rest(left);
            if watch.stopped() {
                return;
            }
        }
        tell(sh, account, watch, &st, "connecting", &mut newest_seen);
    }
    while !watch.stopped() {
        // 熔断中：不自动重连，等手动重试（mail_kick / 刷新）或停。失败原因一直挂着，
        // 不再被下一轮的「connecting」盖掉。
        if state.paused {
            paused_status(sh, watch, &state);
            while !watch.stopped() && !watch.kick.load(Ordering::SeqCst) {
                watch.rest(Duration::from_secs(1));
            }
            if watch.stopped() {
                break;
            }
            // 手动重试：清空退避与熔断，立刻再连。
            watch.kick.store(false, Ordering::SeqCst);
            state.retry_now();
            backoff::save(&sh.dir, &account.id, &state);
            sh.log(&format!("mail {}: retry asked; connecting again now", account.id));
            tell(sh, account, watch, &st, "connecting", &mut newest_seen);
        }
        // Letters that would not come down (the server said no): not asked again this time.
        let mut skip: HashSet<(Folder, u32)> = HashSet::new();
        let started = Instant::now();
        let result = (|| -> Result<(), Fail> {
            let mut conn = session(&account.server, &password_of(sh, account)?, CONNECT)?;
            *watch.socket.lock().unwrap() = conn.socket.try_clone().ok();
            *watch.via.lock().unwrap() = conn.proxy.as_ref().map(|p| p.at());
            // 注意：这里**不**清零失败计数。旧代码一「连上」就计数归零，可服务器常常
            // 接受连接、过一会儿在 IDLE 里掐断——那样每轮都算第 1 次，退避永远停在
            // 第一档（现装日志里 294 次失败有 279 次是「1 in a row」，间隔恒定 ~180s）。
            let mut sent_at: Option<Instant> = None;
            loop {
                if watch.stopped() {
                    return Ok(());
                }
                sync_folder(sh, &mut conn, &st, Folder::Inbox, Mailbox::Inbox)?;
                let kicked = watch.kick.swap(false, Ordering::SeqCst);
                if kicked || sent_at.is_none_or(|t| t.elapsed() >= SENT_EVERY) {
                    sync_sent(sh, &mut conn, &st)?;
                    sent_at = Some(Instant::now());
                }
                // Letters come down ahead and no longer to be kept (older
                // now, or kept for less): let go.
                let gone = st.lock().unwrap().prune(offline(sh), now());
                if gone > 0 {
                    sh.log(&format!("mail {}: {gone} letters no longer kept offline", account.id));
                }
                save(&st);
                tell(sh, account, watch, &st, "ok", &mut newest_seen);
                // Whole letters, a while at a time; new mail between, and
                // now and then the inbox's flags.
                let mut flags_at = Instant::now();
                while !watch.stopped() && !watch.kick.load(Ordering::SeqCst) {
                    let left = download(sh, &mut conn, &st, watch, &mut skip)?;
                    save(&st);
                    let came = if flags_at.elapsed() >= FLAGS_EVERY {
                        flags_at = Instant::now();
                        sync_folder(sh, &mut conn, &st, Folder::Inbox, Mailbox::Inbox)?;
                        true
                    } else {
                        look_new(sh, &mut conn, &st)?
                    };
                    tell(sh, account, watch, &st, "ok", &mut newest_seen);
                    if left == 0 && !came {
                        break;
                    }
                }
                if watch.stopped() {
                    return Ok(());
                }
                if watch.kick.load(Ordering::SeqCst) {
                    continue;
                }
                conn.inbox(false)?;
                if conn.has("IDLE") {
                    conn.idle(IDLE_FOR, &watch.kick)?;
                } else {
                    watch.rest(POLL);
                    if watch.stopped() {
                        return Ok(());
                    }
                    conn.noop()?;
                }
            }
        })();
        watch.socket.lock().unwrap().take();
        save(&st);
        let Err(fail) = result else { break };
        if watch.stopped() {
            break;
        }
        // 连接活过 STABLE_MS 才叫健康：它之后的断开（服务器闲置超时、代理重启）不算
        // 风暴的一部分，清零后这次失败又从第 1 档起。短命连接（几秒/几分钟就被掐）则累计。
        if started.elapsed() >= Duration::from_millis(backoff::STABLE_MS) {
            state.healthy();
        }
        let seed = (now() as u64).wrapping_mul(6364136223846793005).wrapping_add(state.failures as u64);
        let step = state.fail(now(), seed);
        state.last_error = Some(fail.json());
        backoff::save(&sh.dir, &account.id, &state);
        sh.log(&format!("mail {}: {:?} ({} in a row)", account.id, fail, state.failures));
        let mut status = json!({ "state": "error", "error": fail.json(), "failures": state.failures });
        if let Step::Wait(d) = &step {
            status["retryInMs"] = json!(d.as_millis() as u64);
        }
        watch.set(status);
        sh.push_settings();
        if state.failures >= 2 {
            show(sh, account, None, &[], false);
        }
        // A kick from before (a letter sent, Refresh pressed) is no reason
        // to try again at once: cleared, so the wait is waited; one during
        // it (Refresh pressed now) tries once more, then it waits again.
        watch.kick.store(false, Ordering::SeqCst);
        match step {
            Step::Wait(d) => rest_or_new_way(account, watch, d),
            Step::Paused => {
                sh.log(&format!("mail {}: paused after {} failures in a row; waiting for a manual retry", account.id, state.failures));
                show(sh, account, None, &[], false);
                // 下一圈走上面的熔断分支，等手动重试。
            }
        }
    }
}

// 熔断时的状态：状态、原因、连续失败次数，以及「可以手动重试」。
fn paused_status(sh: &Shared, watch: &Watch, state: &backoff::State) {
    watch.set(json!({
        "state": "paused",
        "error": state.last_error.clone().map_or(serde_json::Value::Null, |v| v),
        "failures": state.failures,
        "paused": true,
    }));
    sh.push_settings();
}

// The wait after a failure, ended early when the way to the server is not
// the one that failed: Windows' proxy turned on, off or changed meanwhile
// (Clash's "system proxy" switched), looked at every two seconds.
fn rest_or_new_way(account: &Account, watch: &Watch, wait: Duration) {
    let host = &account.server.host;
    let tried = proxy::route(&account.server.via, host);
    let until = Instant::now() + wait;
    while !watch.stopped() && !watch.kick.load(Ordering::SeqCst) {
        let left = until.saturating_duration_since(Instant::now());
        if left.is_zero() || proxy::route(&account.server.via, host) != tried {
            return;
        }
        watch.rest(left.min(Duration::from_secs(2)));
    }
}

// How much is kept whole ahead (the setting mailOffline), and now (ms).
fn offline(sh: &Shared) -> Offline {
    Offline::of(&sh.setting("mailOffline"))
}

fn now() -> i64 {
    now_ms() as i64
}

fn save(st: &Mutex<Store>) {
    st.lock().unwrap().save();
}

// The account's widget and its line in the settings, from the store: the
// unread, the newest unread letter (new mail opens the island, once the
// first sync is done: `newest_seen` known), how much is kept whole.
fn tell(sh: &Shared, account: &Account, watch: &Watch, st: &Mutex<Store>, state: &str, newest_seen: &mut Option<u32>) {
    let (unread, newest, letters, offline) = {
        let s = st.lock().unwrap();
        let unseen: Vec<&Entry> = s.inbox.letters.values().filter(|e| !e.seen).collect();
        let newest = unseen.last().map(|e| e.uid);
        // The newest unread first, as many as its widget's card lists.
        let letters: Vec<(String, String, Option<i64>)> =
            unseen.iter().rev().take(LISTED).map(|e| (if e.from.name.is_empty() { e.from.address.clone() } else { e.from.name.clone() }, e.subject.clone(), e.date)).collect();
        (unseen.len(), newest, letters, s.offline(offline(sh), now()))
    };
    let first = state == "ok" && newest_seen.is_none();
    let is_new = state == "ok" && matches!((newest, *newest_seen), (Some(n), Some(seen)) if n > seen);
    if state == "ok" && (newest > *newest_seen || first) {
        *newest_seen = newest.or(Some(0));
    }
    // Before the first sync, what was kept (nothing said new).
    if state == "ok" || !st.lock().unwrap().inbox.letters.is_empty() {
        show(sh, account, Some(unread), &letters, is_new);
    }
    let (have, total, bytes) = offline;
    // Through a proxy: which (the page says so on the account's line).
    let via = watch.via.lock().unwrap().clone();
    watch.set(json!({ "state": state, "unread": unread, "offline": { "have": have, "total": total, "bytes": bytes }, "via": via }));
    sh.push_settings();
}

pub(super) fn flags_changed(sh: &Shared, account: &Account) {
    let watch = sh.mail.lock().unwrap().watches.get(&account.id).map(|(_, w)| w.clone());
    if let Some(watch) = watch {
        let st = super::store::of(sh, account);
        let mut newest = Some(u32::MAX);
        tell(sh, account, &watch, &st, "ok", &mut newest);
    }
}

// A folder made as the server has it (see the top).
fn sync_folder(sh: &Shared, conn: &mut Session, st: &Mutex<Store>, folder: Folder, mailbox: Mailbox<'static>) -> Result<(), Fail> {
    let name = imap::name_of(&mailbox);
    let opened = conn.folder(mailbox, false)?;
    {
        let mut s = st.lock().unwrap();
        let k = s.get(folder);
        if k.validity != opened.validity || k.mailbox != name {
            s.reset(folder, &name, opened.validity);
        }
    }
    let flags = if opened.exists == 0 { Vec::new() } else { conn.flags_all()? };
    let new = st.lock().unwrap().take_flags(folder, &flags);
    heads(sh, conn, st, folder, &new)?;
    let mut s = st.lock().unwrap();
    s.synced[if folder == Folder::Inbox { 0 } else { 1 }] = true;
    Ok(())
}

// Letters not kept yet, by their header fields, a hundred at a time.
fn heads(sh: &Shared, conn: &mut Session, st: &Mutex<Store>, folder: Folder, uids: &[u32]) -> Result<(), Fail> {
    for chunk in uids.chunks(HEADS_AT_ONCE) {
        let set = chunk.iter().map(u32::to_string).collect::<Vec<_>>().join(",");
        let got = conn.heads(&set, true, &FIELDS)?;
        {
            let mut s = st.lock().unwrap();
            for h in got {
                s.add(folder, Entry::from_head(h.uid, (h.seen, h.flagged, h.answered), h.size, h.received, &h.header));
            }
        }
        sh.push_settings();
    }
    Ok(())
}

// The Sent folder, found (SPECIAL-USE, else by its name) and made as the
// server has it; none, and none is kept.
fn sync_sent(sh: &Shared, conn: &mut Session, st: &Mutex<Store>) -> Result<(), Fail> {
    match conn.sent_box()? {
        Some(mailbox) => sync_folder(sh, conn, st, Folder::Sent, mailbox),
        None => {
            let mut s = st.lock().unwrap();
            if !s.sent.letters.is_empty() || !s.sent.mailbox.is_empty() {
                s.reset(Folder::Sent, "", 0);
            }
            s.synced[1] = true;
            Ok(())
        }
    }
}

// Letters kept whole, newest first, for a while (or until stopped or
// kicked); how many are still to come.
fn download(sh: &Shared, conn: &mut Session, st: &Mutex<Store>, watch: &Watch, skip: &mut HashSet<(Folder, u32)>) -> Result<usize, Fail> {
    let todo: Vec<(Folder, u32)> = st.lock().unwrap().missing(offline(sh), now()).into_iter().filter(|x| !skip.contains(x)).collect();
    let started = Instant::now();
    let mut open: Option<Folder> = None;
    let mut done = 0;
    for &(folder, uid) in &todo {
        if watch.stopped() || watch.kick.load(Ordering::SeqCst) || started.elapsed() > BODIES_FOR {
            break;
        }
        if open != Some(folder) {
            let name = st.lock().unwrap().get(folder).mailbox.clone();
            let Some(mailbox) = imap::mailbox(&name) else { break };
            conn.folder(mailbox, false)?;
            open = Some(folder);
        }
        match conn.letter(uid) {
            Ok(Some((raw, _))) => st.lock().unwrap().keep_body(folder, uid, &raw, false),
            Ok(None) | Err(Fail::Refused(_)) => {
                skip.insert((folder, uid));
            }
            Err(fail) => return Err(fail),
        }
        done += 1;
    }
    Ok(todo.len() - done)
}

// New mail in the inbox since the newest kept (UID SEARCH UID n:*), by its
// header fields; whether there was any.
fn look_new(sh: &Shared, conn: &mut Session, st: &Mutex<Store>) -> Result<bool, Fail> {
    conn.folder(Mailbox::Inbox, false)?;
    let newest = st.lock().unwrap().inbox.letters.keys().next_back().copied().unwrap_or(0);
    let mut new = conn.newer(newest)?;
    new.sort_unstable_by(|a, b| b.cmp(a));
    heads(sh, conn, st, Folder::Inbox, &new)?;
    Ok(!new.is_empty())
}

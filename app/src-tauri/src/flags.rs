//! 名字层（#t18）：命令行开关的**写出名**是 `--fish-dock-*`，旧名只作读取兼容。
//!
//! t16 先把新名做成「入口别名」（新名 → 旧名），方向反了：写进 Claude Code / Codex /
//! Zcode / Reasonix / DSH 配置里的仍然是 `--wakuwaku-*`，用户的旧定义永远换不掉。
//! 本批翻转过来：
//!
//! - 实现里的常量全部是 `--fish-dock-*`（写出、解析都用它们）；
//! - 旧名（`--wakuwaku-*`，以及更早的 `--claude-pets-ensure-running`）只留在下面
//!   这张表里：进程启动时把它换成新名（旧任务/旧 hooks 写的就是旧名，仍要能跑），
//!   识别「这一条是不是本程序写的」时也按同一张表比对。
//!
//! 表的方向是「旧 → 新」。只做整词替换，不做子串替换（`--wakuwaku-x=1` 这种带值
//! 写法不属于这些开关，原样留着）。
pub const ALIASES: [(&str, &str); 13] = [
    ("--claude-pets-ensure-running", "--fish-dock-ensure-running"),
    ("--wakuwaku-ensure-running", "--fish-dock-ensure-running"),
    ("--wakuwaku-codex-hook", "--fish-dock-codex-hook"),
    ("--wakuwaku-agy-hook", "--fish-dock-agy-hook"),
    ("--wakuwaku-zcode-hook", "--fish-dock-zcode-hook"),
    ("--wakuwaku-dsh-hook", "--fish-dock-dsh-hook"),
    ("--wakuwaku-reasonix-hook", "--fish-dock-reasonix-hook"),
    ("--wakuwaku-out-of-job", "--fish-dock-out-of-job"),
    ("--wakuwaku-hw-dump", "--fish-dock-hw-dump"),
    ("--wakuwaku-lhm", "--fish-dock-lhm"),
    ("--wakuwaku-sensors", "--fish-dock-sensors"),
    ("--wakuwaku-sensors-setup", "--fish-dock-sensors-setup"),
    ("--wakuwaku-sensors-remove", "--fish-dock-sensors-remove"),
];

/// 一个参数归一到写出名：旧名换成新名，其余原样返回。
pub fn canonical(arg: &str) -> &str {
    ALIASES.iter().find(|(legacy, _)| *legacy == arg).map_or(arg, |(_, now)| *now)
}

/// 一个参数是不是 `flag`（新名）——旧名同样算，因为它是同一个开关。
pub fn is_flag(arg: &str, flag: &str) -> bool {
    canonical(arg) == flag
}

/// 命令行整体归一（保持顺序与个数：调用点按位置取参数）。
pub fn modernize(args: Vec<String>) -> Vec<String> {
    args.into_iter().map(|arg| canonical(&arg).to_string()).collect()
}

/// 旧名的全部写法：识别「这条命令里有没有我们的开关」时，新旧一起比。
pub fn legacy_names() -> impl Iterator<Item = &'static str> {
    ALIASES.iter().map(|(legacy, _)| *legacy)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_flag_names_are_read_as_the_new_ones() {
        assert_eq!(canonical("--wakuwaku-codex-hook"), "--fish-dock-codex-hook");
        assert_eq!(canonical("--claude-pets-ensure-running"), "--fish-dock-ensure-running");
        // 新名照旧、别的东西一个字节都不动。
        assert_eq!(canonical("--fish-dock-codex-hook"), "--fish-dock-codex-hook");
        assert_eq!(canonical("--fish-dock-at-login"), "--fish-dock-at-login", "不在表里的开关原样");
        assert_eq!(canonical("Stop"), "Stop");
        assert_eq!(canonical("--wakuwaku-hook"), "--wakuwaku-hook", "不做子串/近似匹配");
        assert!(is_flag("--wakuwaku-sensors", "--fish-dock-sensors"), "旧名与同名新开关是同一个");
        assert!(!is_flag("--wakuwaku-sensors-setup", "--fish-dock-sensors"), "相邻开关不许混");
    }

    #[test]
    fn modernizing_keeps_the_command_line_shape() {
        let args: Vec<String> = ["FISH-Dock.exe", "--wakuwaku-sensors", "47213", "C:\\data"].iter().map(|a| a.to_string()).collect();
        assert_eq!(modernize(args), vec!["FISH-Dock.exe", "--fish-dock-sensors", "47213", "C:\\data"]);
    }

    #[test]
    fn every_legacy_name_maps_to_a_distinct_new_name() {
        for (legacy, now) in ALIASES {
            assert!(legacy.starts_with("--"), "{legacy}");
            assert!(now.starts_with("--fish-dock-"), "{now}");
            assert_ne!(legacy, now);
        }
        // 写出名不许和任何旧名相同（那就是没翻转）。
        for (_, now) in ALIASES {
            assert!(!ALIASES.iter().any(|(legacy, _)| *legacy == now), "{now} 仍是旧名写法");
        }
    }
}

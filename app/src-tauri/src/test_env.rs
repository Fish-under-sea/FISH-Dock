//! 测试运行环境的事实（仅测试编译）。
//!
//! 带 Low 强制完整性标签的目录（DSH 沙箱工作区）里编译出的测试 exe 以低完整性运行：
//! 进程快照里看不到中完整性的父进程，任务计划根目录也读不到（0x80070003），
//! 而 \Microsoft 子目录能打开。正式安装的 Dock 以中完整性运行，不受此影响
//! （#80、#81，2026-11-09 实测：同一份测试二进制只见 25 个进程，外部 pwsh 见 351 个）。

/// 当前进程完整性低于 Medium。读不出时按否处理，让依赖环境的断言照常执行。
#[cfg(windows)]
pub fn low_integrity() -> bool {
    use std::ffi::c_void;
    #[link(name = "advapi32")]
    extern "system" {
        fn OpenProcessToken(process: isize, access: u32, token: *mut isize) -> i32;
        fn GetTokenInformation(token: isize, class: i32, info: *mut c_void, len: u32, needed: *mut u32) -> i32;
        fn GetSidSubAuthorityCount(sid: *mut c_void) -> *mut u8;
        fn GetSidSubAuthority(sid: *mut c_void, index: u32) -> *mut u32;
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentProcess() -> isize;
        fn CloseHandle(handle: isize) -> i32;
    }
    const TOKEN_QUERY: u32 = 0x0008;
    const TOKEN_INTEGRITY_LEVEL: i32 = 25;
    const MEDIUM: u32 = 0x2000;
    // SAFETY: 本进程令牌；缓冲区按 GetTokenInformation 给出的长度分配并按指针对齐，
    // TOKEN_MANDATORY_LABEL 的首字段是 SID 指针，指向缓冲区内部，用完即关句柄。
    unsafe {
        let mut token = 0isize;
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 { return false; }
        let mut needed = 0u32;
        GetTokenInformation(token, TOKEN_INTEGRITY_LEVEL, std::ptr::null_mut(), 0, &mut needed);
        let mut buffer = vec![0usize; (needed as usize).div_ceil(std::mem::size_of::<usize>()).max(1)];
        let ok = GetTokenInformation(token, TOKEN_INTEGRITY_LEVEL, buffer.as_mut_ptr().cast(), (buffer.len() * std::mem::size_of::<usize>()) as u32, &mut needed) != 0;
        CloseHandle(token);
        if !ok { return false; }
        let sid = buffer[0] as *mut c_void;
        if sid.is_null() { return false; }
        let count = *GetSidSubAuthorityCount(sid);
        if count == 0 { return false; }
        *GetSidSubAuthority(sid, u32::from(count) - 1) < MEDIUM
    }
}

#[cfg(not(windows))]
pub fn low_integrity() -> bool { false }

#[cfg(test)]
mod tests {
    #[cfg(windows)]
    #[test]
    fn integrity_is_read_from_this_process() {
        // 只验能读出，不假定在哪种环境运行。
        let low = super::low_integrity();
        eprintln!("test process integrity: {}", if low { "low" } else { "medium or higher" });
    }
}

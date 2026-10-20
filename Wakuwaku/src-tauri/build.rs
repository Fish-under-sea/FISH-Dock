// The installed release ships its resources beside the program, expanded by the
// installer at install time (runtime.rs); nothing is embedded in the exe, so
// there is no bundle to check here — only that this build is the one the
// installer can produce: Windows x64.
fn main() {
    if std::env::var_os("CARGO_FEATURE_INSTALLED").is_some() {
        assert_eq!(std::env::var("CARGO_CFG_TARGET_ARCH").as_deref(), Ok("x86_64"), "the installed build supports Windows x64 only");
        assert_eq!(std::env::var("CARGO_CFG_TARGET_OS").as_deref(), Ok("windows"), "the installed build supports Windows x64 only");
    }
    tauri_build::build()
}

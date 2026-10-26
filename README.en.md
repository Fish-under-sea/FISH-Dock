<div align="center">
<p>
<a href="README.md"><picture><source media="(prefers-color-scheme: dark)" srcset="assets/readme/key-zh-zh-dark.png"><img src="assets/readme/key-zh-zh-light.png" width="74" alt="中文"></picture></a>
<a href="README.en.md"><picture><source media="(prefers-color-scheme: dark)" srcset="assets/readme/key-en-zh-dark.png"><img src="assets/readme/key-en-zh-light.png" width="108" alt="English"></picture></a>
<a href="#install"><picture><source media="(prefers-color-scheme: dark)" srcset="assets/readme/key-start-en-dark.png"><img src="assets/readme/key-start-en-light.png" width="126" alt="Get started"></picture></a>
</p>
<h1><picture><source media="(prefers-color-scheme: dark)" srcset="assets/readme/hero-en-dark.png"><img src="assets/readme/hero-en-light.png" width="1440" alt="FISH Dock: a dock for AI sessions and a home for model APIs. Original icon and user-provided hanging mascot."></picture></h1>
<p>
<a href="Wakuwaku/docs/release-1.1.0.md"><picture><source media="(prefers-color-scheme: dark)" srcset="assets/readme/stat-version-zh-dark.png"><img src="assets/readme/stat-version-zh-light.png" width="23%" alt="Current version v1.1.0"></picture></a>
<a href="#install"><picture><source media="(prefers-color-scheme: dark)" srcset="assets/readme/stat-desktop-en-dark.png"><img src="assets/readme/stat-desktop-en-light.png" width="23%" alt="Two docking modes: taskbar and island"></picture></a>
<a href="#data-and-development"><picture><source media="(prefers-color-scheme: dark)" srcset="assets/readme/stat-mail-en-dark.png"><img src="assets/readme/stat-mail-en-light.png" width="23%" alt="Mail management: combined inbox and mark all as read"></picture></a>
<a href="https://github.com/Fish-under-sea/FISH-DOCK-Plugins"><picture><source media="(prefers-color-scheme: dark)" srcset="assets/readme/stat-channels-zh-dark.png"><img src="assets/readme/stat-channels-zh-light.png" width="23%" alt="Four independent official plugins"></picture></a>
</p>
<p>
<a href="https://github.com/Fish-under-sea/FISH-Dock/releases/tag/v1.1.0"><picture><source media="(prefers-color-scheme: dark)" srcset="assets/readme/key-download-en-dark.png"><img src="assets/readme/key-download-en-light.png" width="168" alt="Downloads and Releases; installer assets require a published release"></picture></a>
<a href="Wakuwaku/docs/release-1.1.0.md"><picture><source media="(prefers-color-scheme: dark)" srcset="assets/readme/key-release-en-dark.png"><img src="assets/readme/key-release-en-light.png" width="112" alt="Changelog"></picture></a>
<a href="https://github.com/Fish-under-sea/FISH-Dock/issues"><picture><source media="(prefers-color-scheme: dark)" srcset="assets/readme/key-issues-en-dark.png"><img src="assets/readme/key-issues-en-light.png" width="120" alt="Report an issue"></picture></a>
</p>
</div>

**1.1.0 · Windows 10/11 x64** — a desktop workspace for Agent sessions, notifications, mail and independent plugins.

[Download](https://github.com/Fish-under-sea/FISH-Dock/releases/tag/v1.1.0) · [中文](README.md) · [Official plugins](https://github.com/Fish-under-sea/FISH-DOCK-Plugins) · [Release notes](Wakuwaku/docs/release-1.1.0.md)

## Install

Download `FISH-Dock-Setup-1.1.0.exe` and verify it with `SHA256SUMS.txt` from the same release. Exit the previous version from the tray before upgrading. The installer bundles Node.js 24.14.0, Fixed Version WebView2 155.0.4283.62, and Windhawk 1.7.3 with its LLVM-MinGW compiler. A separate Windhawk installation is unnecessary. The installer is currently unsigned.

## Changes

- OFM, Feishu, Clash and Magpie are independent plugins. Use Settings → Plugins to sign in to GitHub and verify your star on the main FISH Dock repository. Official installation actions and feature pages become available after both checks. Installed plugins are enabled automatically; service credentials still require configuration. The desktop installer does not preinstall business plugins.
- Plugin manifests, resources, platform and storage identity are validated. Failed updates restore the previous version and data.
- OFM and Magpie retain their native management UI in a larger embedded window. Magpie's management server, model gateway and embedding proxy use separate loopback ports.
- Feishu and Clash offer one- and two-column layouts. Tabs support ordering, horizontal scrolling and mouse dragging.
- Windhawk lives in Appearance, with source or collection import, compilation, switches, backups and rollback.
- Opening a plugin no longer forces persistent pinning or always-on-top. GitHub sign-in uses an expiring temporary focus state, copies the device code before opening your browser, and polls for completion. Closing or cancelling restores manual preferences.
- Application confirmations stay inside Dock. Fullscreen exit restores visibility; explicit wake-up and approval prompts also work while another application is fullscreen.
- About and plugin cards provide update checks. Store access has a GitHub API fallback and validated offline cache. Package sizes automatically use KB, MB or GB.
- Settings restore the last tab and retain saved mode, position and display choices.

## Data and development

The complete data root is resolved from `--data-root`, then `FISH_DOCK_HOME`, then `.fish-dock` under the current user profile. The host uses `host`; official plugins use separate `.fishplugin-*` namespaces. Disabling or uninstalling a plugin preserves its data. Backups can contain account secrets.

The desktop host, SDK, installer and Windhawk integration belong in this repository. Plugin business code and distribution belong in [FISH-DOCK-Plugins](https://github.com/Fish-under-sea/FISH-DOCK-Plugins). Service startup is on demand. Model requests still use the selected upstream provider and its own accounts and quotas.

```powershell
cargo run --locked --no-default-features --manifest-path Wakuwaku/src-tauri/Cargo.toml
node --test --test-isolation=none 'sdk/node/*.test.mjs' 'Wakuwaku/test/*.test.js'
cargo test --locked --no-default-features --manifest-path Wakuwaku/src-tauri/Cargo.toml -- --test-threads=1
```

Build inputs are pinned in `Wakuwaku/scripts/runtime-sources.json`. Prepare a new resource directory with `prepare-installer.mjs`, then build with `build-installer.ps1` and NSIS. See the Chinese README for detailed storage and build instructions.

FISH Dock derives from Wakuwaku; the historical model integration derives from Our Free Model. Original licenses and attribution remain included. Third-party runtime licenses ship with the installed resources; see [asset credits](assets/readme/SOURCES.md).

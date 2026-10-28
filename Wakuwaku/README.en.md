<p align="center"><img src="src-tauri/icons/dsh-fish.png" width="112" alt="FISH Dock icon"></p>
<h1 align="center">FISH Dock</h1>
<p align="center">A place for AI sessions to dock, and model connections to come together.</p>
<p align="center"><strong>1.2.1</strong> · Windows x64 · MIT-licensed code</p>
<p align="center"><a href="README.md">简体中文</a> · <strong>English</strong> · <a href="https://github.com/Fish-under-sea/FISH-Dock/releases/tag/v1.2.1">Downloads</a> · <a href="https://github.com/Fish-under-sea/FISH-Dock/issues">Issues</a> · <a href="docs/release-1.2.1.md">Release notes</a></p>

> **A derivative integration, not an official upstream release.** FISH Dock builds on the desktop and AI-session experience of [Wakuwaku](https://github.com/zhoupengjie/Wakuwaku) and integrates the model channels and API workspace of [Our Free Model / dsh-our-free-model](https://github.com/Ebony-Vinyl/dsh-our-free-model). Both core systems remain upstream work. Original authors, copyrights, and licenses are credited below.

> **Business plugins are distributed separately.** See the [official plugin repository](https://github.com/Fish-under-sea/FISH-DOCK-Plugins) for supported channels and rollout status. The desktop installer does not preinstall business services.

This repository contains the desktop host, plugin SDK, storage and runtime management, tests and installer. The old OFM service snapshot has been removed; business implementations and packages live in the official plugin repository.

## One Workspace, Two Capabilities

| Desktop docking, from Wakuwaku | Model connections, from OFM |
| --- | --- |
| Taskbar, dynamic island, pets, and AI session activity | Accounts, model catalog, and OpenAI-compatible APIs |
| Agent integrations, plugins, mail, and desktop widgets | Usage, local API Key, and a separate LAN Key |
| Remembered mode, position, display, and startup entrance | An on-demand service with an owned process lifecycle |

**Local does not mean offline model inference.** The workspace, proxy, and stored data run on your computer. Model requests still go to the selected provider. Eligibility, quotas, pricing, and availability are controlled by those providers. This project does not promise unlimited usage, permanently free access, or bypasses for provider restrictions.

## New in 1.2.1

**1.2.1** settles two things: GitHub sign-in becomes “one sign-in, valid onward”, with the account identity and main-repository star rechecked on every launch — and an application with token expiry switched off is now accepted as-is. Whole-configuration backups carry credentials and rosters but no mail bodies, keep unreadable credentials as they are, and restore archives written by an older plugin version. It also brings PR #14's island placement, PR #15's per-agent bills, plugin autostart and third-party plugins. Download the full and compact packages from [v1.2.1](https://github.com/Fish-under-sea/FISH-Dock/releases/tag/v1.2.1).

- Installed-release updates: automatic checks after launch and daily, manual checks, and an official release-page fallback when the API is unavailable. Upgrades use full or compact installers.
- In-app tagline: “Everything is a Plugin”, with clear host and independent-plugin responsibilities.
- Installation: same-volume staging, removal of verified obsolete owned files, verification reuse, visible progress and rollback; full and compact update packages.
- Configuration: whole-configuration backups with preview, before-copies and transactional restore — credentials and rosters included, mail bodies left out, archives from an older plugin version still restore.
- Sign-in: smaller device-code dialogs, accurate copy feedback, manual release of temporary pinning and island-position recovery.
- Runtime discovery: actual Node source, path and version, with recovery from stale overrides.

These existing capabilities are retained:

- Independent Feishu, Clash, Magpie and OFM plugins; newly installed plugins are enabled automatically.
- GitHub device sign-in and main-repository star verification, with device-code copying and automatic status polling.
- In-window confirmations and temporary authentication focus, preserving manual pin/top preferences.
- Fullscreen recovery, explicit wake-up and approval visibility while fullscreen.
- About/plugin update checks, catalog API fallback and readable KB/MB/GB package sizes.
- Bundled Windhawk/compiler, native management pages, draggable tabs and configurable columns.

See the [1.2.1 release notes](docs/release-1.2.1.md) for verification scope.

## Installation and Updates

Target: **Windows 10/11 x64**. System-taskbar integration mainly targets Windows 11; other Windows releases and third-party taskbar software require separate verification.

1. Choose a published version from [Releases](https://github.com/Fish-under-sea/FISH-Dock/releases/tag/v1.2.1) and check the matching `SHA256SUMS.txt`. `FISH-Dock-Setup-1.2.1.exe` is for first installation or repair; `FISH-Dock-Update-1.2.1.exe` is for an existing 1.1.0 or earlier 1.2.1 installation with intact base resources.
2. Fully exit the previous version from the tray and any standalone OFM service. Run the installer and choose a destination. Fresh installs default to `%LOCALAPPDATA%\Programs\FISH Dock`; upgrades reuse the registered directory instead of moving it.
3. Start **FISH Dock** from the desktop or Start menu. Node.js, Fixed Version WebView2, Windhawk, its compiler, and license notices are bundled. Business plugins are downloaded separately.
4. The first launch confirms runtime terms and verifies resources. Later launches use installed files without repeatedly extracting the entire runtime.

**The installer has no Authenticode signature.** Windows may display unknown-publisher or SmartScreen warnings. Renaming, new artwork, and self-signing cannot reliably remove reputation warnings. Use a trusted source, compare the digest, and do not disable protection, install unknown roots, or bypass organizational policy. Normal use does not generally require administrator rights.

`Error writing temporary file. Make sure your temp folder is valid.` is an NSIS bootstrap error before the install script runs. Check the temporary directory, permissions, free space, and security-software logs. If an upgrade cannot write a DLL, make sure the previous application and its children have exited; do not ignore the write failure.

## Getting Started

**Desktop:** the first launch defaults to taskbar mode. Settings and the tray switch between taskbar and island, choose the edge, and select left, center, or right island positioning. Choices survive restarts; an unavailable saved display falls back to the primary. Reduced motion uses a short fade.

**Plugins and models:** sign in to GitHub and verify your star on the main repository under **Plugins**. The in-window prompt shows the code and actual copy result. Paste it in the browser and confirm; Dock polls for completion. Temporary pinning can be released without cancelling sign-in. New plugins are enabled after installation. Configure credentials and start services as needed. Use the endpoint actually shown by each plugin, for example:

```text
http://127.0.0.1:18901/v1
```

Agents, mail, and plugins still need their own clients, accounts, and permissions. Installation grants no service entitlement. Third-party authentication, downloads, and galleries keep their real destinations.

### LAN Access

1. In OFM's **API Access → LAN Access**, enable sharing only on a trusted network.
2. Copy a displayed address and the **LAN API Key**. A remote device cannot use `127.0.0.1`, and the local API Key cannot substitute for the LAN Key.
3. Configure an OpenAI-compatible client on the other device. Rotating the LAN Key invalidates the old key immediately, so update clients too.
4. Disable sharing when unnecessary. After an interface change or binding failure, turn it off and on to rebind. Only successfully bound addresses are displayed.

Management stays local. Transport uses HTTP and is intended only for trusted networks; never expose it to the public Internet. The program does not create firewall exceptions or guarantee that another computer or phone is reachable. Check firewall and network isolation yourself. Sharing a port number does not mean opening the loopback management listener on all interfaces.

### Configuration and Credential Migration

The OFM **Service Settings → OFM Migration Backup** creates an OFM-only ZIP, excluding mail and usage statistics. It is unencrypted and may contain the local API Key, LAN Key, provider credentials, and EAC authorization.

For a complete move, use **General → FISH Dock configuration and backups**. The ZIP includes host settings, layouts, extension registration and persistent namespace configuration, credentials, rosters and keys; **mail bodies (`host/mail-store`) stay out**, while account rosters and sign-in credentials travel with it. Runtime records (`trash`, `routing`, usage logs, model and market caches), caches, temporary files, program packages and old backups are excluded. The default export folder is `host/exports` in the configuration repository. Exported secrets are plaintext and are re-encrypted locally on restore; credentials the current user cannot decrypt travel as they are and need one sign-in on the target machine. Handle the archive like a password file.

Exit standalone OFM but keep FISH Dock open for import. The app pauses its own model service and mail sync. Review the preview and confirm again. A whole-configuration archive restores the previewed scope and keeps a pre-import copy. Legacy backups keep their original scope; import old OFM material through the corresponding plugin migration entry. The app attempts rollback after activation failure. Existing `%USERPROFILE%\.dsh-fish` data is copied to `%USERPROFILE%\.fish-dock` and the old folder remains; an existing new folder is authoritative and is not merged. The archive supplies a fixed `README.md`.

## Data and Uninstallation

| Content | Location |
| --- | --- |
| Application and bundled dependencies | Chosen directory; fresh installs default to `%LOCALAPPDATA%\Programs\FISH Dock` |
| Settings, pets, layouts, and logs | `host` under the data root |
| WebView2 profile | `host/webview` under the data root |
| Runtime terms and verification state | System per-user runtime state |
| Plugin configuration and secrets | Isolated `.fishplugin-*` / `.plugin.<publisher>.<name>` namespaces |

`DSH-FISH.exe`, the application identifier, uninstall registration, and data directories remain for compatibility. Uninstall removes owned installation files and registration while preserving user data. Startup tasks are deleted only when verified to belong to this installation. Back up data before deleting it manually.

## Building From Source

### GitHub and Plugin Configuration

GitHub authentication uses a public OAuth Client ID and device flow. Tokens are encrypted by the host using DPAPI and are not exposed to plugins. **One sign-in lasts**: credentials live in `host/keys` of the configuration repository, so installing or updating to a new version never touches them; every launch (including right after an update) rechecks the account identity and the main-repository star once, and an offline machine only shows “recheck pending” instead of clearing the session. With token expiry disabled on the OAuth application the issued token never expires, and the rotating refresh token covers older ones. `FISH_DOCK_GITHUB_CLIENT_ID` can select your own application. Provider-specific OAuth setup belongs to each plugin.

The desktop uses **Rust + Tauri 2 + HTML/CSS/JavaScript**. The model UI uses **Node.js + React**. Installers use **NSIS 3**. Building requires Windows x64, MSVC/Rust, Node.js, NSIS, and pinned official runtime downloads. Offline builds require cached dependencies; retain lock files.

```text
FISH-Dock/
  Wakuwaku/              # Desktop source and this README
  sdk/                   # Plugin SDK
  licenses/              # Historical component notices
```

From the desktop directory; the host build no longer needs the old OFM snapshot:

```powershell
node scripts/prepare-installer.mjs --downloads <runtime-cache> --out <new-resource-directory> --version 1.2.1
cargo test --locked --offline --features installed --manifest-path src-tauri/Cargo.toml -- --test-threads=1
node --test "test/*.test.js" "scripts/*.test.mjs"
./scripts/build-installer.ps1 -Resources <prepared-directory> -Cargo <cargo.exe> -Node <node.exe> -Makensis <makensis.exe>
```

Pinned versions and digests are in [runtime-sources.json](scripts/runtime-sources.json). Actual tests, environment limits, and historical artifacts are in the [build record](docs/installer-build.md). The installer includes no model weights or Rust/MSVC environment for building Dock. It does include the LLVM-MinGW compiler used by Windhawk mods.

## Sources and Credits

| Project | Original author / maintainers | Retained contribution | License |
| --- | --- | --- | --- |
| [Wakuwaku](https://github.com/zhoupengjie/Wakuwaku) | Pengjie Zhou / zhoupengjie | Taskbar, island, pets, sessions, and desktop functions | [MIT](LICENSE), Copyright 2026 Pengjie Zhou |
| [dsh-our-free-model](https://github.com/Ebony-Vinyl/dsh-our-free-model) | Ebony-Vinyl and contributors | Model channels, accounts, usage, and compatible APIs | Historical model integration; MIT, Copyright 2026 Ebony-Vinyl; [retained license](../licenses/Our-Free-Model.txt), business code in the plugin repository |

**Fish-under-sea** maintains the FISH Dock integration. New integration code uses MIT, and upstream copyright notices remain intact. Do not describe the whole feature set as original FISH Dock work or make upstream maintainers responsible for this distribution.

The application uses the user's original blue-haired character icon, not redrawn artwork or a claimed original MIT asset. Pet sprites, application artwork, and galleries do not automatically inherit the source-code license; verify their rights before redistribution. The default Claude character is designed by **ZipZipPipe**, with pixel adaptation by **zhoupengjie**. Node.js, WebView2, fflate, and other dependencies retain their own licenses.

## Feedback and Security

Report Windows and application versions, docking mode, reproduction steps, and redacted logs through [FISH Dock Issues](https://github.com/Fish-under-sea/FISH-Dock/issues). Never upload usable keys, full Cookies, private mail, or backup ZIPs, including in vulnerability reports. The 1.2.1 full installer, compact update package and checksums are available in the linked release.

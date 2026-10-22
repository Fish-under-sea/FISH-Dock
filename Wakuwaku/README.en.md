<p align="center"><img src="src-tauri/icons/dsh-fish.png" width="104" alt="FISH Dock"></p>
<h1 align="center">FISH Dock</h1>
<p align="center">A desktop home for AI sessions, model connections and everyday tools.</p>
<p align="center"><strong>v1.0.2</strong> · Windows 10/11 x64 · <a href="README.md">简体中文</a> · <a href="https://github.com/Fish-under-sea/FISH-Dock/releases/tag/v1.0.2">Download</a></p>

> **Meoo is currently in a limited rollout and is unavailable for use.** The experimental UI and source remain included. Login, project binding and inference are not advertised as usable in this release.

FISH Dock combines taskbar / island docking, pets, AI session status, mail and a local OFM management service. Both complete projects are included in this repository, with service code, React frontend, channel adapters, tests and build scripts.

## What's new in 1.0.2

- WorkBuddy validation distinguishes temporary errors from authentication failures; rate limits and timeouts no longer permanently stop the service.
- Batch mark-as-read for individual and combined mailboxes, with progress, confirmed counts and preserved flags.
- Entering OFM enables pinning and always-on-top. Leaving disables both. Other apps entering fullscreen trigger a choice; pinned windows with topmost already disabled unlock after two continuous seconds.
- Account-scoped refresh, balance recovery and isolated errors.
- Raccoon daily-credit checks backed by actual billing evidence; grouped per-vendor audits separate daily grants from balances.
- Consistent cards, logos and “添加账号” buttons across 14 channel entries, preserving each channel's existing capabilities.
- Original supplied brand icon and complete Dock / OFM source publication.

Meoo remains experimental and unavailable. See the [release notes](docs/release-1.0.2.md) for evidence and limitations.

## Install and update

Download the installer and SHA-256 file from [v1.0.2](https://github.com/Fish-under-sea/FISH-Dock/releases/tag/v1.0.2). Exit the old tray app and any standalone OFM service, then update in the existing installation directory. Uninstalling first is unnecessary. The package includes Node.js, fixed-version WebView2 and OFM. It is not Authenticode signed.

Accounts, keys, usage and mail identities remain under `%USERPROFILE%\.fish-dock`; settings and WebView data remain under `%LOCALAPPDATA%\DSH-FISH`. Legacy `.dsh-fish` migration copies and verifies files only when the new directory is absent and keeps originals. Data directory overrides remain supported.

The service runs locally but sends inference requests to selected providers. LAN inference is off by default, uses its own key and does not expose management or backups. Identity ZIP exports can contain plaintext credentials; keep them private.

## Source and build

- [Wakuwaku](): Rust / Tauri desktop, mail, identities, window state and installer.
- [dsh-our-free-model](../dsh-our-free-model/): full OFM service, React UI, channels and tests.
- [Source scope](../SOURCE.md), [upstream references](../UPSTREAM.json), [Chinese build guide](README.md#从源码构建).

Build on Windows x64 using Rust / MSVC, Node.js 22.19+ or 24+ and NSIS 3. Install frontend dependencies with `npm --prefix dsh-our-free-model/packages/standalone ci`, rebuild using `node dsh-our-free-model/scripts/build-standalone-ui.mjs`, prepare locked official runtimes using `Wakuwaku/scripts/prepare-installer.mjs`, then invoke `Wakuwaku/scripts/build-installer.ps1`. Full commands and tests are in the Chinese guide.

Public source and release assets leave Google OAuth client defaults empty. Configure `CMDC_PAK_GOOGLE_CLIENT_ID` and `CMDC_PAK_GOOGLE_CLIENT_SECRET` in the service environment if using Gemini OAuth.

Automated tests cover isolated protocols, state machines and browser rendering. Native Windows fullscreen, actual mail servers, provider logins and next-day grants require real-device verification. Meoo is not available for normal use.

## Credits

Integrated and maintained by [Fish-under-sea](https://github.com/Fish-under-sea), based on [Wakuwaku](https://github.com/zhoupengjie/Wakuwaku) (Pengjie Zhou, MIT) and [OFM](https://github.com/Ebony-Vinyl/dsh-our-free-model) (Ebony-Vinyl and contributors, MIT). Original copyrights and third-party notices remain in each subtree. Artwork and provider logos do not automatically inherit the code's MIT license.

Report issues with version, reproduction steps and redacted logs at [FISH Dock Issues](https://github.com/Fish-under-sea/FISH-Dock/issues).

# Rustty - Cross-Platform SSH Client🦀⚡

> 🌐 Versión en español: [README.es.md](README.es.md)

> ⚠️ **Notice**: this repository contains code and documentation generated in part with AI agents.
> Contributions and/or criticism are welcome. When taking part, follow the [Code of Conduct](CODE_OF_CONDUCT.md); for how the binaries are signed see [`CODE_SIGNING.md`](CODE_SIGNING.md).

**Rustty** is a cross-platform, modern and lightweight terminal client and connection manager, designed to deliver a smooth experience for administering remote servers. Built with **Rust** and **Tauri**, it combines the power of low-level tooling with a modern, agile web interface.

## Key features

- **Multi-protocol**: SSH, SFTP, FTP, FTPS, RDP, VNC and Telnet connections (RDP, VNC and Telnet via the system's external clients: `xfreerdp`/`mstsc`, `vncviewer`/TigerVNC and `telnet`). On Windows, RDP connects directly without asking for the password twice (a `TERMSRV` credential is injected through the system API and removed on close); on Linux, if the external RDP client fails at startup, Rustty shows the real reason (e.g. a changed server certificate).
- **Modern terminal**: xterm.js with GPU (WebGL) rendering, themes, configurable cursor, scrollback, **in-buffer search** (Ctrl+F), **double-click selection** that breaks on field separators (`:`, `@`, `/`, `=`, `.`, `|`) to isolate chunks in dense output (`grep`, logs), batched output draining so very chatty commands (`cat` on large logs, `journalctl`, etc.) do not freeze the UI, a bottom bar with status/latency/diagnostics, OSC 7 support (tracking the remote `cwd`) and a **multi-line command editor** (Ctrl+Shift+E) to compose long commands, with a per-profile draft and a **shared command history across tabs** (opt-in) with **autocomplete** in that same editor: as you type it filters previous commands by match (prefix first) and you navigate with the arrow keys, accepting with `Tab` or `Enter`.
- **Snippets, commands and palette**: a library of **snippets** that insert into the active terminal and a catalog of **local commands** (run a command, open a URL or a folder), both configurable in Preferences → Commands with `${host}/${user}/${var:…}/${ask:…}` substitutions and optional confirmation. A **global command palette** (`Ctrl+Shift+P`) offers fuzzy search over profiles, snippets, commands and app actions. Plus built-in **profile templates** (Linux SSH, SSH with key, bastion, legacy SSH, RDP, FTPS) to create connections with sensible defaults, and you can mark your own profiles as templates.
- **Markdown notes per connection (runbooks)**: right-click a connection to **add or edit a Markdown note**, with a live-preview editor, formatting toolbar, title and tags. Each note is stored as a self-contained `.md` file (syncable, opt-in in Backups), resolves `${host}/${user}/…` variables in the preview and can be shown as a **runbook panel** next to the session with interactive task checkboxes. Shortcut `Ctrl+Shift+M`.
- **Integrated file panel**: SFTP/FTP/FTPS explorer with a **split remote / local view** (remote on the left or right, configurable in Preferences → Aesthetics), recursive folder transfers, drag & drop between panels **and from the operating system's file manager** (dropping files/folders onto the remote panel uploads them), configurable conflicts, transfer queue, resizable tabbed logs, context menus, **quick rename with F2**, **back/forward history and a clickable breadcrumb path** on each side (`Ctrl+L` to type the path), **path autocompletion** (`Tab` and a suggestions dropdown), **file search** by name in the current directory or recursively, creation of folders and empty files on both sides, optional tracking of the terminal directory over SSH, and elevation to **sudo** when the server allows it. SFTP transfers use **pipelining** (simultaneous in-flight requests of 256 KiB, configurable in Preferences) and saturate the connection's actual bandwidth instead of being limited by RTT; the number of parallel requests can be lowered for servers with a handle limit (e.g. Hetzner Storage Box).
- **SSH CLI**: list saved connections with `rustty -l`, connect directly with `rustty -c <name|id|ip|host>` and run remote commands with `--exec`, `--` or the alias `rustty -c <profile> "cmd"`, without opening the graphical interface.
- **Integrated SSH tunnels**: **local** (`-L`), **remote** (`-R`) and **dynamic / SOCKS** (`-D`) port forwarding over an active session or from the global quick access, with a status panel, traffic, saved tunnels and optional per-profile auto-connect.
- **Advanced per-profile SSH options**: **keep the session alive** (keep-alive against idle/NAT disconnects; off by default, toggled per connection and by right-clicking the tab, with immediate effect), **automatic reconnection with backoff** on drops, **session recording** to file, bastion / ProxyJump, agent forwarding, X11 forwarding and an option to allow legacy ciphers / kex / MACs (aes-cbc, 3des-cbc, dh-sha1, hmac-sha1, ssh-rsa) on old servers, letting you pick which specific algorithms to offer.
- **Multiple users per connection**: add extra identities to a profile (each with its own authentication: password, master credential, SSH key or agent). Connecting uses the primary one; right-click → **“Connect as another user”** picks an alternative identity, and `Ctrl+P` pastes the password of the user that session logged in with. Available for SSH, RDP and SFTP/FTP.
- **Restore previous screen**: right-click → **“Connect and restore previous screen”** reconnects, repainting what was shown in the last session (a *visual* scrollback restore, not the remote process). The capture is stored on disk per profile and can be turned off in Preferences → Security; it is never synced.
- **Multi-tab and split views**: work with multiple simultaneous sessions, arrange them in horizontal / vertical / grid *splits* and enable *broadcast* to type into several at once.
- **Polished sidebar**: a vertical rail of icons (Profiles, Favorites, Tunnels, Activity, Sync, Preferences and quick actions), **drag & drop** between folders and workspaces, per-folder colors, remembering of the expanded tree, **renaming the selected connection with F2** and automatic selection of the connection associated with the active tab.
- **Diagnostics and activity**: a **Test** button in the connection modal without saving the profile, staged SSH logs, TCP checks for RDP/FTP/FTPS and a persistent global activity center with transfers, sync, errors and updates grouped by day.
- **System tray / quick launcher**: quick access to favorites, recents, workspaces, local console, **Wake On LAN** for profiles with a MAC address and show/hide window from the tray icon. Optionally **start Rustty with the system** and **start minimized** to the tray (opt-in, under Preferences → System).
- **Granular export**: export all profiles, those of a folder or those of a workspace to JSON from the context menu, asking first whether saved passwords/passphrases should be included.
- **Import from other tools**: import your `~/.ssh/config` or, via a **step-by-step wizard**, connections from **mRemoteNG** (`.xml`) or **Ásbrú Connection Manager** (`.yml`) — it rebuilds the folder tree into a new workspace, lets you choose what to import, shows progress and optionally decrypts the saved passwords (all locally).
- **Security**:
  - Native integration with the system keyring (Secret Service/KWallet on Linux, Windows Credential Store).
  - Support for **KeePass** databases (`.kdbx`) as a password source.
  - Reusable **master credentials**: define a password once and reference it from several profiles with `${master:name}`; the value lives only in the keyring and rotating it updates every profile that uses it. It is part of a **variable engine** (`${host}`, `${env:…}`, `${var:…}`, `${ask:…}`) resolved at connection time, including fields such as the host or the user.
  - `Ctrl+P` shortcut to paste the active profile's password (the one of the **user the session logged in with**, if an additional user was used) without exposing it on screen. With *broadcast* active it is no longer blocked: a preference decides whether to send it to all panes, only the active one, or ask each time (see the Security tab).
  - **Security tab** in Preferences that centralizes sensitive options under a clear principle —never block, always offer choices— including password paste with *broadcast*, risky-paste confirmation, screen capture, `known_hosts` management and log retention.
  - **Private / ephemeral session** ("Open in private" from the profile menu): leaves no trace in recents, the activity center, drafts or session recording, and the tab is marked as private.
  - `known_hosts` verification with TOFU and a warning on fingerprint changes, plus a **visual `known_hosts` manager** in Preferences → Security to review fingerprints and remove conflicting entries.
  - Warning and confirmation when **enabling agent forwarding**, so you don't share the SSH agent with untrusted hosts by mistake.
  - **Configurable retention of session logs** (by age and size) with manual cleanup and a sensitive-content warning.
- **E2E backups and synchronization**: profiles, preferences, themes, shortcuts, connection notes and, if you enable it, saved passwords can be synchronized with Google Drive, a local folder / NAS or WebDAV. The remote blob is encrypted locally with `age` and a master passphrase. **Event-based** synchronization (it checks at startup and syncs if there are local/remote changes) and **restoration of historical snapshots** from the Backups tab.
- **Organization**: group connections into independent **container profiles (workspaces)**, in folders within each workspace, **favorite connections** and sidebar views (current workspace, all profiles, favorites), quick search and duplication of connections / sessions from the context menu. **Alphabetical or manual ordering** of folders and connections: in manual mode, "Move up / down" from each folder's or machine's context menu.
- **Customization**: 12 built-in base themes and an extended library of 221 preloaded Rustty v2 themes for the interface and terminal, plus cursor, scrollback and *bell* settings. Ability to import custom themes in JSON v2 format with UI and terminal tokens; see the [custom theme guide](web/docs/Temas.md#crear-un-tema-personalizado). **Adjustable interface size** (rail, sidebar, tabs and modals) independent of the terminal, with a control in Preferences and `Ctrl+Alt` shortcuts with `+` / `-` / `0`, and **interface density** in three levels (Spacious / Comfortable / Compact). Preferences → Appearance also offers **accessibility** settings (stronger contrast, reduced motion, visible focus, minimum terminal contrast and color-blind mode).
- **Internationalization**: interface translated into Spanish, English, French, Portuguese, German, Traditional Chinese and Indonesian. (Translations done with AI)

## Screenshots

Welcome screen with the system light theme:

![Welcome screen](images/Captura1.png)

Several sessions open in tabs and the connection panel context menu (dark theme):

![Tabs and context menu](images/Captura2.png)

Grid split view: four panels in the same tab with the *layout* selector in the top-right corner:

![Grid split view](images/Captura5.png)

Preferences → **Appearance**: global interface theme and an independent terminal theme (with the "Same as interface" *swatch* for inheritance):

![Theme selector](images/Captura4.png)

Preferences → **Language**: interface available in Spanish, English, French, Portuguese, German, Traditional Chinese and Indonesian:

![Language selector](images/Captura3.png)

## Keyboard shortcuts

Rustty includes a **shortcut editor** in Preferences → *Shortcuts* that lets you reassign any action with live capture (press "Edit" and the new combination). The default shortcuts are:

| Shortcut                       | Action                                                 |
|--------------------------------|--------------------------------------------------------|
| `Ctrl+Shift+N`                 | New connection                                         |
| `Ctrl+Shift+T`                 | New local console                                      |
| `Ctrl+W`                       | Close active tab                                       |
| `Ctrl+Tab`                     | Next tab                                               |
| `Ctrl+Shift+Tab`               | Previous tab                                           |
| `Ctrl+,`                       | Open preferences                                       |
| `Ctrl+Alt+C`                   | Copy terminal selection                                |
| `Ctrl+Alt+V`                   | Paste into the terminal                                |
| `Ctrl+P`                       | Paste the active profile's password into the shell    |
| `Ctrl+Shift+E`                 | Open the multi-line command editor                     |
| `Ctrl+Shift+P`                 | Open the global command palette                        |
| `Ctrl+K`                       | Search connections from any view                       |
| `Ctrl+F`                       | Search within the terminal buffer                      |
| `Ctrl++` / `Ctrl+-` / `Ctrl+0` | Increase / decrease / reset the font size              |

## SSH CLI

Rustty can also be used from the terminal to work with saved SSH connections:

```bash
rustty -l
rustty --list
rustty -l --json
rustty -c <nombre|id|ip|host>
rustty --connect <nombre|id|ip|host>
rustty -c <nombre|id|ip|host> --exec "uptime"
rustty -c <nombre|id|ip|host> -- hostname
rustty -c <nombre|id|ip|host> "hostname"
rustty -c <nombre|id|ip|host> --tty -- sudo systemctl status nginx
```

`-c` reuses the profile data, the system keyring, `known_hosts`, ProxyJump, keepalive, agent forwarding and the legacy compatibility configured on the connection. If a password or passphrase is not stored in the keyring, it will prompt for it in the terminal without showing it.

When a remote command is added, Rustty opens an SSH `exec` channel, writes `stdout`/`stderr` to the local terminal and exits with the remote exit code. `--exec` is the recommended form for commands with quotes or pipes; `--` accepts a short form similar to `ssh`, and extra text after the profile is left as a convenient alias. `--tty` requests a pseudo-terminal for commands that need it, such as some uses of `sudo`.

## Installation

Rustty supports two platforms, and each GitHub release ships one artifact for each:

| Platform | File | Format |
| --- | --- | --- |
| **Windows 10/11 (x64)** | `Rustty_<version>_x64-portable.exe` | Portable executable, no installer |
| **Debian / Ubuntu (x86_64)** | `Rustty_<version>_amd64.deb` | `.deb` package |

Download them from the [Releases](https://github.com/rosdyana/Rustty/releases) page. Other systems are not supported; you can still try [building from source](#development-and-building).

Rustty does not update itself: **Preferences → About → Check for updates** (or **Check at startup**) queries the GitHub Releases API and, if there is a newer version, opens its release page so you can download it.

### Windows (portable)

Download `Rustty_<version>_x64-portable.exe` and run it: a single executable with no installation and no administrator rights, ideal for USB drives or locked-down machines.

The **Microsoft Edge WebView2 Runtime** is required (already included in Windows 10 22H2 and Windows 11). If your system does not have it, install it manually from [here](https://developer.microsoft.com/microsoft-edge/webview2/).

> Windows binaries are **not signed yet** (code signing is under evaluation; see [`CODE_SIGNING.md`](CODE_SIGNING.md)). In the meantime SmartScreen or some antivirus may show a warning or a false positive: [verify the download](#integrity-verification).

To update, download the new `-portable.exe` and put it in the same folder as the previous one (you can delete the old one); it will read the same configuration folder.

#### True portable mode

When Rustty runs as `Rustty_<version>_x64-portable.exe` (filename with the `-portable.exe` suffix), it does **not** use `%APPDATA%`. It stores all configuration in a `.conf\com.rustty.app\` folder created automatically **next to the executable itself**. This includes `profiles.json` and other app data, so the USB drive remains *self-contained*: copy it to another machine and the configuration travels with it.

Caveats:

- The **Windows keyring** (Credential Manager) still belongs to the user running the binary, not to the USB drive. To move credentials between machines you can use a **KeePass `.kdbx`** database next to the portable build or enable E2E synchronization of saved passwords in **Preferences → Backups**.
- The window state (size, position) is saved in the user profile (`tauri-plugin-window-state` plugin); the visual session of the USB drive is not 100% portable.
- If you rename the `.exe` and remove the `-portable.exe` suffix, it returns to normal mode and reads `%APPDATA%\com.rustty.app\`.

### Debian / Ubuntu (.deb)

Rustty requires **WebKitGTK 4.1** and **libayatana-appindicator** at runtime; `apt` installs them as dependencies of the package. It works on Ubuntu 22.04 or later, Debian 12 or later and their derivatives (Linux Mint, Pop!_OS…).

```bash
sudo apt install ./Rustty_*_amd64.deb
```

To update, download the new `.deb` and install it with the same command: `apt` upgrades the previous version in place and keeps your data.

### Integrity verification

Each release includes a **`SHA256SUMS.txt`** file with the digest of every artifact in the release:

```bash
# Download SHA256SUMS.txt and the artifact you want, into the same directory:
sha256sum --check --ignore-missing SHA256SUMS.txt
```

On Windows (PowerShell), compare `Get-FileHash .\Rustty_*_x64-portable.exe` with the matching line of `SHA256SUMS.txt`.

That manifest also carries a GitHub **build provenance attestation**, tying it to the workflow, the commit and the runner that produced it. Verifying it does not rely on the integrity of the release page, so it answers the question the `sha256` alone cannot — "what if whoever altered the binary also altered the number next to it?":

```bash
gh attestation verify SHA256SUMS.txt --repo rosdyana/Rustty
```

**Windows** code signing is under evaluation. The full signing model (mechanisms, roles, verifiable build and how to verify a download) is in [`CODE_SIGNING.md`](CODE_SIGNING.md).

## Technologies used

- **Backend**: [Rust](https://www.rust-lang.org/) — 100% pure for SSH/SFTP and FTPS over `rustls` (no dependency on `libssh2`).
- **App framework**: [Tauri v2](https://tauri.app/)
- **Frontend**: [Vite](https://vitejs.dev/) + Vanilla JavaScript / CSS
- **Terminal**: [xterm.js](https://xtermjs.org/)
- **Protocols**: [russh](https://github.com/warp-tech/russh) (SSH), [russh-sftp](https://github.com/warp-tech/russh-sftp) (SFTP), [`suppaftp`](https://github.com/veeso/suppaftp) (FTP/FTPS)
- **Embedded RDP client**: [IronRDP](https://github.com/Devolutions/IronRDP) (RDP protocol, sessions, graphics, input, dynamic channels, shared clipboard and shared folder), [arboard](https://github.com/1Password/arboard) (local clipboard), [tokio-rustls](https://github.com/rustls/tokio-rustls) (TLS) and [x509-cert](https://github.com/RustCrypto/formats/tree/master/x509-cert) (certificates)
- **Security**: [keyring-rs](https://github.com/hwchen/keyring-rs), [keepass-rs](https://github.com/sseemayer/keepass-rs)

## Acknowledgments and third-party dependencies

Rustty is a fork of the original project [Aleixenandros/Rustty](https://github.com/Aleixenandros/Rustty), by Alejandro Soriano. The product attribution is in the [NOTICE](NOTICE) file.

The direct Rust dependencies added for the embedded RDP client are all licensed under permissive terms:

| Crate | Used for | License |
| --- | --- | --- |
| `ironrdp-connector`, `ironrdp-session`, `ironrdp-tokio`, `ironrdp-dvc`, `ironrdp-displaycontrol`, `ironrdp-pdu`, `ironrdp-graphics`, `ironrdp-input`, `ironrdp-core`, `ironrdp-svc` | RDP connection, sessions, graphics, input and dynamic channels ([IronRDP](https://github.com/Devolutions/IronRDP) by Devolutions) | MIT OR Apache-2.0 |
| `ironrdp-cliprdr`, `ironrdp-rdpdr`, `ironrdp-rdpdr-native` | Shared clipboard (text and files) and shared folder as a remote drive ([IronRDP](https://github.com/Devolutions/IronRDP)) | MIT OR Apache-2.0 |
| `arboard` | Reading and writing the local clipboard (text and copied files) ([arboard](https://github.com/1Password/arboard)) | MIT OR Apache-2.0 |
| `tokio-rustls` | TLS for RDP, with the `ring` crypto provider ([tokio-rustls](https://github.com/rustls/tokio-rustls)) | MIT OR Apache-2.0 |
| `x509-cert` | X.509 certificate parsing for RDP server certificates ([RustCrypto](https://github.com/RustCrypto/formats)) | Apache-2.0 OR MIT |

No new npm packages were added for this feature. The full transitive dependency tree is recorded in `src-tauri/Cargo.lock`; you can list the license of each crate with `cargo license` (after `cargo install cargo-license`).

## Contributing

Rustty is developed in the open, and contributions are welcome.

- **To the original project**: open issues and pull requests at [Aleixenandros/Rustty](https://github.com/Aleixenandros/Rustty). Changes that should be part of the upstream project are best proposed there.
- **To this fork**: open a pull request against `main` in this repository and fill in the [pull request template](.github/PULL_REQUEST_TEMPLATE.md).

Before opening a pull request:

1. Follow the [Code of Conduct](CODE_OF_CONDUCT.md).
2. Run the checks: `npm test`, `npm run lint`, `npm run typecheck` and `cargo test` in `src-tauri/`.
3. Do not commit secrets or credentials. To report a security vulnerability, follow [SECURITY.md](SECURITY.md) instead of opening a public issue.
4. Update the [CHANGELOG](CHANGELOG.md) if the change is visible to users.

## Backups and synchronization

Rustty includes a **Preferences → Backups** tab with three flows:

- **Cloud synchronization**: Google Drive, local folder / NAS or WebDAV.
- **Encrypted backup**: export/import a `.rustty-sync.bin` encrypted with your passphrase, independent of any backend.
- **Local data**: JSON export/import of profiles for interoperability and simple copies.

Synchronization is opt-in and encrypts the state before uploading it. Profiles, preferences, custom themes, shortcuts, connection notes and snippets are synchronized. The **saved passwords/passphrases** have their own checkbox: if you enable it, Rustty reads the `password:<profile_id>` / `passphrase:<profile_id>` secrets from the local keyring, places them in the E2E-encrypted blob and restores them in the keyring of other machines. The unlocked KeePass database and local paths such as `keepassPath` or `keepassKeyfile` are never synchronized.

The local JSON exports of connections/folders/workspaces ask before including secrets. If you choose to include them, the JSON contains readable credentials; prefer the encrypted `.rustty-sync.bin` backup to transport passwords.

Synchronization checks the state when the app starts, is triggered when it detects local changes (1-minute debounce) and, if you enable it, also runs **periodically at the interval you choose** (1–60 min or disabled, in Preferences → Backups) to pick up changes from other machines without restarting. If the local and remote logical content already match, it does not rewrite the remote blob, does not create a new snapshot and leaves the UI untouched. Before overwriting a different remote blob, an encrypted snapshot is saved; from the **Restore backup** dropdown you can revert to any previous snapshot available in the backend.

Also: the **first sync** against an already populated server shows a preview (what would be added, changed or deleted) and asks for confirmation; an **activity log** records what each pass did and from which machine (each machine can have its own name); **closing the app** with pending changes runs a quick final sync (can be disabled); network outages show as an "offline" state with automatic retry; the passphrase gets a **strength meter, a local generator and a rotation assistant** that re-encrypts the remote state (and optionally the history); and a button lets you **delete the encrypted state and all snapshots from the server**.

Backends:

- **Google Drive**: OAuth in the browser with a local callback; Rustty uses the `appDataFolder` space and stores the refresh token in the keyring.
- **Local folder / NAS**: useful for Syncthing, shared folders or external cloud clients.
- **WebDAV**: compatible with Nextcloud, ownCloud and generic WebDAV servers.

## Development and Building

If you want to compile the project from source, follow these steps:

### Prerequisites

1. **Rust**: [Install Rust](https://www.rust-lang.org/tools/install)
2. **Node.js**: v24 recommended to match the CI workflow.
3. **System dependencies**:

   #### Debian / Ubuntu (compilation)

    ```bash
    sudo apt-get install -y libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf libdbus-1-dev libssl-dev pkg-config
    ```

   #### Windows (compilation)

    You need to install the [Visual Studio C++ Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) and have the **WebView2 Runtime** installed (included by default in Windows 10 and 11).

### Steps to run in development

1. Clone the repository:

    ```bash
    git clone https://github.com/rosdyana/Rustty.git
    cd Rustty
    ```

2. Install the Node.js dependencies:

    ```bash
    npm install
    ```

3. Run the application in development mode:

    ```bash
    npm run tauri dev
    ```

### Building for production

Build the same artifacts the release publishes:

```bash
# Debian / Ubuntu: .deb package
npm run tauri build -- --bundles deb
# → src-tauri/target/release/bundle/deb/Rustty_<version>_amd64.deb

# Windows: portable executable (no installer)
npm run tauri build -- --no-bundle
# → src-tauri/target/release/rustty.exe
```

Rename `rustty.exe` to something ending in `-portable.exe` if you want the [true portable mode](#true-portable-mode).

### Automatic release

The GitHub Actions workflow (`.github/workflows/build.yml`) runs when a `v*` tag is pushed: it verifies the code and the tag, builds the Debian/Ubuntu `.deb` and the Windows portable `.exe`, uploads them to the release together with `SHA256SUMS.txt` and its provenance attestation, and publishes it.

The single source of version truth is `package.json`. To prepare a release, change only the `version` field there and run:

```bash
npm run sync-version
```

That command synchronizes `Cargo.toml`, `Cargo.lock` and `package-lock.json`. The public website resolves its version from the latest GitHub release, not from `package.json`. `npm run build`, `npm run tauri dev` and `npm run tauri build` also run the synchronization automatically.

```bash
git tag v1.0.0
git push --tags
```

The artifacts are uploaded to a draft GitHub release, which the workflow publishes once both builds have finished.

For the official builds to include Google Drive, define these secrets in GitHub Actions:

```text
RUSTTY_GOOGLE_DRIVE_CLIENT_ID
RUSTTY_GOOGLE_DRIVE_CLIENT_SECRET
```

## Data paths

- **Linux**: `~/.local/share/com.rustty.app/` (profiles, configuration)
- **Windows**: `%APPDATA%\com.rustty.app\` (or `.conf\com.rustty.app\` next to the executable in [true portable mode](#true-portable-mode))

Passwords are not stored in these files: they live in the system keyring under the `rustty` service, or are resolved from a KeePass database referenced by UUID. If you enable password sync, they only travel inside the E2E-encrypted blob and are rehydrated into the local keyring again.

The synchronization configuration lives in `sync_config.json` and the last local snapshot in `sync_state.json` (cleared when synchronization is disabled). The sync secrets (master passphrase, WebDAV password, and the Google Drive OAuth token and client secret) are stored in the system keyring.

---

## 📄 License

Rustty is distributed under the [Apache License, Version 2.0](LICENSE).

```text
Copyright 2026 Alejandro Soriano

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

    http://www.apache.org/licenses/LICENSE-2.0
```

See the [NOTICE](NOTICE) file for the attributions required when redistributing.

---

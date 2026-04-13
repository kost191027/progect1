# RKN

**Recursive Kinetic Network** is a self-managed stealth gateway client for macOS and Windows.

It combines a lightweight desktop shell, a high-performance native backend, and a flexible network core to give the user a simple flow:

`enter server -> deploy node -> start tunnel -> stay protected`

<p>
  <a href="https://github.com/kost191027/progect1/releases/latest/download/RKN-latest-x64.dmg">
    <img alt="Download for Intel Mac" src="https://img.shields.io/badge/Download%20for-Intel%20Mac-1F2937?style=for-the-badge&logo=apple&logoColor=white">
  </a>
  <a href="https://github.com/kost191027/progect1/releases/latest/download/RKN-latest-arm64.dmg">
    <img alt="Download for Apple Silicon" src="https://img.shields.io/badge/Download%20for-Apple%20Silicon-111827?style=for-the-badge&logo=apple&logoColor=white">
  </a>
  <a href="https://github.com/kost191027/progect1/releases/latest/download/RKN-latest-x64-setup.exe">
    <img alt="Download for Windows" src="https://img.shields.io/badge/Download%20for-Windows-0F172A?style=for-the-badge&logo=windows11&logoColor=white">
  </a>
</p>

## First launch on macOS

If macOS says that **RKN is damaged and cannot be opened**, do not worry. This is a standard warning for unsigned or not-yet-notarized builds.

After copying the app into `Applications`, run:

```bash
sudo xattr -rd com.apple.quarantine "/Applications/RKN.app"
```

Then launch `RKN.app` again from `Applications`.

<p>
  <img alt="Tauri v2" src="https://img.shields.io/badge/Tauri-v2-24C8DB?style=flat-square&logo=tauri&logoColor=white">
  <img alt="Rust" src="https://img.shields.io/badge/Rust-backend-000000?style=flat-square&logo=rust&logoColor=white">
  <img alt="React" src="https://img.shields.io/badge/React-frontend-61DAFB?style=flat-square&logo=react&logoColor=0B0F16">
  <img alt="TypeScript" src="https://img.shields.io/badge/TypeScript-5.x-3178C6?style=flat-square&logo=typescript&logoColor=white">
  <img alt="sing-box" src="https://img.shields.io/badge/sing--box-core-111111?style=flat-square">
  <img alt="ShadowTLS" src="https://img.shields.io/badge/ShadowTLS-stealth-2F855A?style=flat-square">
  <img alt="Shadowsocks 2022" src="https://img.shields.io/badge/Shadowsocks--2022-transport-1F6FEB?style=flat-square">
  <img alt="macOS and Windows" src="https://img.shields.io/badge/macOS%20%2B%20Windows-supported-1D1D1F?style=flat-square&logo=windows11&logoColor=white">
</p>

## Overview

RKN is designed around two principles:

- **Speed**: a Rust backend with a Tauri shell keeps the app small, responsive, and lightweight compared to Electron-style clients.
- **Freedom**: the networking layer is powered by `sing-box`, which gives the project far more flexibility than a fixed single-protocol client.

Instead of baking transport logic directly into the UI shell, RKN bundles `sing-box` as a sidecar and uses the desktop app as a local control layer for:

- server deployment
- client config generation
- tunnel lifecycle
- tray mode and background behavior
- diagnostics and log streaming
- user-facing state summaries above raw technical logs

## Why this project is different

Most applications trade one of these away:

- performance
- protocol flexibility
- self-hosted control
- understandable UX

RKN tries to keep all four.

### 1. Performance by design

RKN uses:

- **Tauri v2** instead of Electron
- **Rust** for the backend and system-facing logic
- the system WebView instead of bundling a browser runtime

That gives the app:

- low overhead
- fast startup
- small frontend bundle size
- better desktop responsiveness during logging and background work

### 2. Protocol freedom through sing-box

The project uses `sing-box` as the network core because it provides:

- TUN support
- routing and split tunneling
- DNS control
- sidecar execution model
- room for future transport iteration without rewriting the application shell

### 3. Stealth-oriented transport stack

The current transport direction is based on:

- **ShadowTLS** as the outer disguise layer
- **Shadowsocks-2022** as the inner encrypted transport

This is not only about encryption. It is about survivability, plausible traffic behavior, and long-term flexibility under restrictive or hostile networks.

## What the app does today

RKN already covers the core local user loop:

- stores the current server profile locally for convenience
- connects to the server over SSH
- deploys or updates the remote transport stack
- generates a matching local client config
- starts and stops the local tunnel
- restores active session state between launches
- streams technical logs to the desktop UI
- keeps a simpler user-facing state model above raw logs

The app currently ships with a minimal 3-screen desktop UX:

- **Settings**: server access, deploy/update, diagnostics, and activity logs
- **Start**: a one-button everyday tunnel control screen
- **Info**: built-in operational notes and FAQ

## Core features

- One-flow deploy from the desktop client to a remote Linux VPS
- Local tunnel start/stop with administrator prompt on macOS and Windows
- Tray mode with background continuity
- Remote diagnostics and server status checks
- Split-routing oriented toward practical local/direct behavior
- Runtime guard state for degraded proxy-path conditions
- Lightweight packaging model built around a sidecar binary

## Supported platforms and system requirements

### Desktop OS

- macOS:
  - Apple Silicon or Intel
  - modern Tauri-compatible macOS release
  - administrator confirmation available for TUN startup
- Windows:
  - Windows 10 or Windows 11
  - x64 is the primary release target
  - administrator rights available for tunnel startup
  - Windows Firewall and local security software must allow the app to start the bundled network core

### Server requirements

- Ubuntu or Debian VPS
- public IPv4 address
- SSH access
- Docker installed or installable by the deploy flow
- 1 vCPU / 1 GB RAM minimum for a personal node
- 2 GB RAM recommended for heavier browsing or multiple devices

## Firewall, antivirus, and driver notes

- On macOS and Windows, RKN starts a local TUN-based networking core and may request administrator confirmation.
- On Windows, the local tunnel depends on the system allowing the bundled `sing-box` sidecar and Wintun-based networking behavior.
- If Windows Defender Firewall shows a prompt, allow the app or the bundled network core for the network types you actually use.
- If third-party antivirus or endpoint protection blocks the app, the sidecar binary, or TUN creation, add an exception for the installed RKN app folder and retry the tunnel start.
- If a corporate firewall, endpoint agent, or local filtering product interferes with Wintun/TUN traffic, test first on a clean personal machine before assuming the server is broken.
- If the tunnel starts but traffic does not pass, check the in-app diagnostics first, then temporarily disable conflicting security software for a controlled test.

## Architecture

### Desktop shell

- **Tauri v2**
- **React**
- **TypeScript**

### Native backend

- **Rust**
- process management
- SSH deploy flow
- TUN lifecycle
- recovery logic
- tray integration
- event streaming to the frontend

### Network core

- **sing-box** sidecar
- **ShadowTLS**
- **Shadowsocks-2022**

### Frontend structure

The frontend follows an FSD-lite layout:

- `src/app`
- `src/pages`
- `src/widgets`
- `src/features`
- `src/shared`

The UI is intentionally restrained. It does not try to be a dashboard. The main product path remains:

`deploy -> start -> verify state`

## Screenshots

Screenshots are worth adding and should be part of the public-facing README.

The best set for this project is:

1. `Settings` screen
2. `Start` screen
3. `Info` screen
4. Optional tray/menu preview

For now, this README is structured so screenshots can be inserted cleanly later without rewriting the document.

Suggested section layout:

```md
## Screenshots

### Settings
![Settings](docs/screenshots/settings.png)

### Start
![Start](docs/screenshots/start.png)

### Info
![Info](docs/screenshots/info.png)
```

## Privacy model

RKN is built for a **self-hosted** workflow:

- the user controls their own server
- the app connects directly over SSH
- there is no mandatory cloud control plane for deployment or runtime traffic

Current MVP behavior:

- the last successful server credentials are stored **locally on the Mac** for convenience
- those credentials are not uploaded to a third-party backend by the app
- traffic is handled between the user’s device and the user’s own server

## Working with multiple devices

RKN can be used from multiple client devices against the same server, but `Rotate SNI` changes the active ShadowTLS cover domain on the server. That means each device must refresh its local client config after the rotation.

Recommended flow:

1. On device A, click `Rotate SNI`.
2. If the tunnel was already running on device A, confirm that the logs contain:
   - `[SYSTEM] SNI rotated to ...`
   - `Tunnel config changed after SNI rotation. Restarting core to apply the updated client config.`
3. On device B, click `Deploy`.
4. Confirm that device B attaches to the existing server instead of redeploying it, and if the tunnel was already running there, confirm that the logs contain:
   - `[SSH] Existing RKN transport detected on this server. Reusing it instead of rotating transport credentials.`
   - `Tunnel config changed after attaching to the existing server. Restarting core to apply the updated client config.`

Important notes:

- `Rotate SNI` does not create a brand-new server stack. It rotates the active cover domain and updates the local `client_config.json` on the current device.
- Other devices should use `Deploy` after the rotation so they can refresh their local config from the active remote transport.
- If a device still runs an old tunnel state after the server-side SNI has changed, you may see client-side `traffic hijacked` errors and server-side `client hello verify failed: hmac mismatch` warnings.
- Automatic restart is only expected if the local tunnel was already running at the moment the config changed.

### Troubleshooting

- If device A completed `Rotate SNI` but still cannot pass traffic, check whether the tunnel was running before the rotation. If it was not running, start it manually so the updated config is actually loaded.
- If device B still fails after `Deploy`, make sure the logs show reuse of the existing transport rather than a fresh server redeploy. The expected reuse log is `[SSH] Existing RKN transport detected on this server. Reusing it instead of rotating transport credentials.`
- If the client shows `traffic hijacked` while the server shows `client hello verify failed: hmac mismatch`, that usually means the device is still using an old live tunnel state. Run `Deploy` on that device again and confirm the automatic restart log appears.
- If multiple devices are connected to the same server, avoid pressing `Rotate SNI` on several devices at once. Rotate on one device first, then refresh the others with `Deploy`.

## Release strategy

The current desktop release targets are:

- **macOS x64**
- **macOS arm64**
- **Windows x64**

Later candidates:

- **macOS universal**
- **Windows arm64** if packaging and QA demand it

Android and iOS are future product tracks, not simple rebuild targets.

## Build outputs

The project currently builds into:

- `.app`
- `.dmg`
- `.exe`
- `.msi` or NSIS installer, depending on the active Windows bundle target

Typical macOS release artifacts:

- `RKN-<version>-x64.dmg`
- `RKN-<version>-arm64.dmg`
- later: `RKN-<version>-universal.dmg`

Typical Windows release artifacts:

- `RKN-<version>-x64-setup.exe`
- `RKN-<version>-x64.msi`

The final size is driven mostly by the bundled `sing-box` sidecar.

## Local development

From the project root:

```bash
npm ci
npm run tauri dev
```

TypeScript check:

```bash
npm run build-check
```

Rust check:

```bash
cargo check --manifest-path src-tauri/Cargo.toml
```

Release build:

```bash
npm run tauri build
```

Version bump:

```bash
npm run version:bump -- 0.1.1
```

Quick semver bump:

```bash
npm run version:bump -- patch
```

## CI/CD

GitHub Actions is used for:

- validation checks
- macOS release builds
- build metadata injection
- artifact publishing

The current release direction is GitHub Releases with attached build assets.

## Release QA

Packaged builds are validated with a dedicated manual QA flow:

- [macOS release checklist](docs/qa/macos-release-checklist.md)
- [release report template](docs/qa/release-report-template.md)

This keeps release testing focused on the real `.app` and `.dmg` behavior instead of only the dev environment.

## Design philosophy

RKN should feel like a quiet system tool, not a noisy consumer app.

The interface is intentionally:

- minimal
- fast
- readable
- operational

The goal is not to impress with charts and panels.
The goal is to make deployment, activation, and status understandable in seconds.

## Current status

RKN is in MVP hardening and release preparation:

- core deploy flow exists
- local tunnel control exists
- tray mode exists
- release packaging exists
- CI build automation is being finalized

The main engineering constraint at this stage is preserving the already stable network path while improving packaging, release quality, and delivery.

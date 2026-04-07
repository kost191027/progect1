# RKN

**Recursive Kinetic Network** is a self-managed stealth gateway client for macOS.

It combines a lightweight desktop shell, a high-performance native backend, and a flexible network core to give the user a simple flow:

`enter server -> deploy node -> start tunnel -> stay protected`

<p>
  <a href="https://github.com/kost191027/progect1/releases/latest/download/RKN-latest-x64.dmg">
    <img alt="Download for Intel Mac" src="https://img.shields.io/badge/Download%20for-Intel%20Mac-1F2937?style=for-the-badge&logo=apple&logoColor=white">
  </a>
  <a href="https://github.com/kost191027/progect1/releases/latest/download/RKN-latest-arm64.dmg">
    <img alt="Download for Apple Silicon" src="https://img.shields.io/badge/Download%20for-Apple%20Silicon-111827?style=for-the-badge&logo=apple&logoColor=white">
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
  <img alt="macOS first" src="https://img.shields.io/badge/macOS-first-1D1D1F?style=flat-square&logo=apple&logoColor=white">
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
- Local tunnel start/stop with administrator prompt on macOS
- Tray mode with background continuity
- Remote diagnostics and server status checks
- Split-routing oriented toward practical local/direct behavior
- Runtime guard state for degraded proxy-path conditions
- Lightweight packaging model built around a sidecar binary

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

## Release strategy

The current platform priority is:

- **macOS x64**
- **macOS arm64**
- later: **macOS universal**

Windows is planned as a separate platform track after the macOS release path is fully stabilized.

Android and iOS are future product tracks, not simple rebuild targets.

## Build outputs

The project currently builds into:

- `.app`
- `.dmg`

Typical macOS release artifacts:

- `RKN-<version>-x64.dmg`
- `RKN-<version>-arm64.dmg`
- later: `RKN-<version>-universal.dmg`

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

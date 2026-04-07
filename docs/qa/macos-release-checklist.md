# RKN macOS Release QA Checklist

This checklist is the main QA gate for release builds.

Use it for:

- `RKN-<version>-x64.dmg`
- `RKN-<version>-arm64.dmg`
- later: `RKN-<version>-universal.dmg`

The goal is to validate the packaged product, not only the dev workflow.

## 1. Test Session Header

Fill this in before starting:

- Version:
- Build:
- Artifact:
- Architecture:
- macOS version:
- Device:
- Test date:
- Tester:

## 2. Artifact Integrity

- `.dmg` opens without corruption warnings
- `.app` can be copied into `Applications`
- bundle name is `RKN.app`
- app icon is correct in Finder and Dock
- tray icon is visible and readable in the macOS menu bar
- version/build shown inside the app matches the intended release

## 3. First Launch

- app launches from the packaged `.app`, not from `tauri dev`
- Gatekeeper behavior is acceptable for the current release stage
- main window opens without layout shifts or broken assets
- first screen is `Settings` on a fresh local profile
- bottom navigation is visible and does not overlap content

## 4. Settings Screen

- server fields render correctly
- `Deploy` label is shown for a fresh server profile
- `Update` label is shown only for an already known matching profile
- `Activity Log` opens and closes correctly
- `Diagnostics` opens and closes correctly
- status blocks are readable without opening logs

## 5. Start Screen

- screen header says `Recursive Kinetic Network | Start`
- central button is visually neutral before connection
- status reads `Inactive` before connection
- activating the tunnel changes the button state and icon correctly
- status changes to `Active` after successful tunnel start
- stopping the tunnel returns the state to `Inactive`

## 6. Info Screen

- FAQ accordion opens and closes correctly
- privacy and security section is present
- server deployment safety section is present
- version and build are visible at the bottom

## 7. Clean Deploy Scenario

Run on a clean Linux VPS.

- save server profile
- deploy succeeds
- client config is generated locally
- start tunnel succeeds
- normal browsing works
- direct local services still work as expected
- stop tunnel succeeds

## 8. Dirty Cloud Deploy Scenario

Run on a server that already hosts other software.

- deploy does not remove unrelated services
- deploy uses only its own working directory and container
- if preferred port is busy, fallback logic behaves as expected
- repeated deploy updates the existing RKN stack instead of leaving duplicates

## 9. Tunnel Lifecycle

- start works from `Settings`
- start works from `Start`
- stop works from both screens
- app restore after relaunch reflects the real tunnel state
- tray `Quit` stops the tunnel and exits the app cleanly
- closing the window hides the app without breaking the tunnel session

## 10. Network Reliability

- normal TCP traffic works through the proxy path
- QUIC / UDP-dependent scenarios behave as expected
- direct RU services still go direct
- no obvious DNS leak behavior appears during the smoke test
- recovery after short network change works
- recovery after sleep / wake works

## 11. Logs and Diagnostics

- logs stream in the packaged build
- warnings and errors are visible and readable
- `Check Server Status` works from the packaged build
- copying logs works
- no broken paths or missing assets appear in the log stream

## 12. Release Verdict

Mark one:

- PASS
- PASS WITH MINOR ISSUES
- BLOCKED

## 13. Notes

Record only actionable findings:

- issue:
- reproduction:
- expected behavior:
- actual behavior:
- severity:


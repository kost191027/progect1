# Android Dev Loop

This document is the local Android development/testing loop for the `6A` track.

## Baseline

- Prefer a real `arm64-v8a` Android device once we move beyond shell/UI checks.
- Use emulator mainly for fast shell/UI iterations.
- Keep macOS and Windows desktop release workflows separate from the Android loop.

## Prerequisites

Run:

```bash
npm run android:doctor
```

Expected critical checks:

- `JAVA_HOME`
- `ANDROID_HOME`
- `NDK_HOME`
- Android scaffold in `src-tauri/gen/android`

## Fast local loop

Start the hot-reload Android dev session:

```bash
npm run tauri:android:dev
```

Use this for:

- mobile shell checks
- WebView regressions
- Android-specific layout / navigation validation

## Production-like smoke

Run the Android app in production mode:

```bash
npm run tauri:android:run
```

Use this for:

- release-like startup
- sidecar/process smoke tests
- packaging sanity checks

## APK build

Build a test APK:

```bash
npm run tauri:android:build:apk
```

Build an AAB:

```bash
npm run tauri:android:build:aab
```

## Device checks

List connected devices:

```bash
adb devices
```

After `6A.2+`, the preferred order is:

1. emulator for shell/UI iteration
2. real arm64 Android device for tunnel/runtime checks
3. GitHub Android Test Build only as a convenience artifact path

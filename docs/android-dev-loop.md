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
- Android-compatible Java (`17` or `21` preferred over `26`)
- `ANDROID_HOME`
- `NDK_HOME`
- Android scaffold in `src-tauri/gen/android`
- Android `llvm-ranlib` availability for cross-compiling crates like `openssl-sys`

## Fast local loop

Start the hot-reload Android dev session:

```bash
npm run tauri:android:dev
```

These Android npm scripts now bootstrap the required mobile toolchain env automatically:

- `JAVA_HOME`
- `ANDROID_HOME`
- `ANDROID_SDK_ROOT`
- `NDK_HOME`
- `TARGET_CC`
- `TARGET_AR`
- `TARGET_RANLIB`

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

## Fast device reinstall

If the debug APK is already built and you only want to reinstall it onto a connected phone:

```bash
npm run android:install:debug
```

This does:

- `adb install -r ...app-arm64-debug.apk`
- `adb shell am start -n com.freedom.rkn/.MainActivity`

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

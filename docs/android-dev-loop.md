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
- local `libbox.aar` presence at `src-tauri/gen/android/app/libs/libbox.aar` for the preferred Android-native runtime path

## Preferred runtime path

The preferred Android runtime path is now `libbox`, not the old standalone CLI fallback.

Place the local AAR here:

```text
src-tauri/gen/android/app/libs/libbox.aar
```

Notes:

- This file is intentionally ignored by git.
- The repo keeps only `src-tauri/gen/android/app/libs/.gitkeep`.
- If `libbox.aar` is missing, the Android runtime selection falls back to the stub backend and tunnel traffic still will not start for real.

Prepare the AAR through the official upstream build path:

```bash
npm run android:libbox:prepare
```

What this does:

- clones or updates `SagerNet/sing-box`
- clones or updates `SagerNet/sing-box-for-android`
- runs upstream `go run ./cmd/internal/build_libbox -target android`
- copies `libbox.aar` and `libbox-legacy.aar` into `src-tauri/gen/android/app/libs`
- runs the local inspect step afterwards

Inspect the dropped AAR before wiring the bridge:

```bash
npm run android:libbox:inspect
```

This prints:

- whether `classes.jar` is present
- which `jni/arm64-v8a/*.so` entries exist
- which package/class paths look relevant for the future libbox bridge

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

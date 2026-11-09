# 7 on 7 No Pads Football on Android

The game is the same game. `crates/android` is a `cdylib` that Android loads into
a `NativeActivity`, and `crates/cli/src/android.rs` is the platform layer: it puts
the renderer's CPU framebuffer on the activity's surface and turns pad events into
the `Input` the game already polls. The renderer needed no changes at all, because
it never asked for a GPU in the first place.

There is no Java in the APK: a `NativeActivity` loads `libnopads_android.so` and
calls `android_main`, so the package carries no dex.

## Build

Needs the Android NDK, an SDK with `build-tools` and a `platform`, Java for
`apksigner`, and the Rust target:

```sh
rustup target add aarch64-linux-android
ANDROID_NDK_HOME=/path/to/ndk ANDROID_HOME=/path/to/sdk ./android/build-apk.sh
# writes target/android/7on7-<version>-android-arm64.apk
```

The APK is arm64 only, min SDK 26, signed with a throwaway debug key (the script
makes one). `.github/workflows/android.yml` runs the same script on every `v*` tag
and attaches the APK to the release next to the Windows zip.

## Controls

It is played with a controller, exactly as on a desktop: left stick to move,
A/B/X/Y for the face buttons and the runner's moves, right trigger to throw, right
bumper to check, left bumper for the dummy count, right stick up/down for the
shoulder and the stop, left and right for the jump cut, Start to pause, Select to
cycle the camera. Touch input is ignored on purpose, so a finger on the screen
cannot move the quarterback.

Only arm64 phones with a controller are covered so far: no x86_64 build, and no
on-screen controls.

# Using it on mobile (iOS, Android)

English | [日本語](README.ja.md)

Everything needed to use the modplay-rs Rust engines instead of libopenmpt in an app that uses libopenmpt.

- `ahmod/` — a Rust static library that exports **C functions with the same names as libopenmpt** (the commonly used ones, such as `openmpt_module_create_from_memory2` and `openmpt_module_read_float_stereo`). Existing code that calls libopenmpt works unchanged.
  - IT and S3M are played by the IT engine, XM by the XM engine (a port of ft2-clone), MOD by the ProTracker engine.
  - The supported functions are listed in `ahmod/include/libopenmpt/libopenmpt.h`. `ahmod_set_channel_mute` is added for muting channels.
- `tracker/` — an example Expo native module (module name `AHTracker`). It loops a module through AVAudioEngine on iOS and AudioTrack on Android.
  - From JavaScript: `play(path, volume)`, `stop()`, `setVolume(volume)`, `isPlaying()`.
  - It is a module originally written for libopenmpt, with only the build settings (`AHTracker.podspec` and `android/src/main/cpp/CMakeLists.txt`) changed to use ahmod. The Swift, Objective-C++, Kotlin and JNI code calls the same functions as with libopenmpt.
- `build-mobile.ps1` — a script (Windows PowerShell) that builds the static libraries for iOS and Android and puts them into `tracker/`.

## 1. Build the libraries

Only rustup and cargo are needed. It only builds static libraries, so Xcode and the Android NDK are not required; linking happens in the app build (EAS or similar).

```powershell
powershell -ExecutionPolicy Bypass -File mobile\build-mobile.ps1
```

For an app that plays only IT, S3M and XM, the ProTracker engine for MOD can be left out to save about 50 KB (MOD files can then not be loaded):

```powershell
powershell -ExecutionPolicy Bypass -File mobile\build-mobile.ps1 -NoMod
```

Output:

- `tracker\android\libs\{arm64-v8a,armeabi-v7a,x86_64,x86}\libahmod.a`
- `tracker\ios\AHMod.xcframework` (arm64 for devices, arm64 for the simulator on Apple Silicon Macs)

On macOS or Linux, the same steps work wherever `cargo build --target ...` is available (run `cargo build --release` for each target and place the `.a` files).

## 2. Add it to the app

1. Copy `tracker/` under the Expo app's `modules/` folder (for example `modules/tracker`). Expo autolinking finds `expo-module.config.json` and includes it.
2. If a libopenmpt version of the module is already there, move it out of `modules/`; otherwise both are loaded.
3. Build the app as usual (`npx expo run:ios`, `npx eas build` and so on) and check the sound on a device.

## 3. Differences from libopenmpt

- Interpolation is linear. libopenmpt defaults to an 8-tap sinc, so the high frequencies sound slightly different.
- `set_repeat_count` is accepted, but songs always loop.
- Supported formats are MOD / IT / S3M / XM. For anything else, create returns NULL.
- Compared with libopenmpt on 26 real songs used as game background music (10 IT, 16 XM):
  - Playback position matched 100% on every song.
  - The loudness ratio was 0.96–1.02.
  - CPU time was 0.19–0.55 times that of libopenmpt with its default settings (measured on a PC).
- The library is about 0.5 MB, against about 1.56 MB for libopenmpt (when built for a PC).

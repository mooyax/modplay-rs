# モバイル（iOS・Android）で使う

libopenmpt を使っているアプリで、libopenmpt の代わりに modplay-rs の Rust エンジンを使うための一式です。

- `ahmod/` — Rust の静的ライブラリ。libopenmpt と**同じ名前の C 関数**（`openmpt_module_create_from_memory2`、`openmpt_module_read_float_stereo` など、よく使うもの）を出しています。そのため、libopenmpt を呼んでいる既存のコードはそのまま動きます。
  - IT と S3M は IT エンジン、XM は XM エンジン（ft2-clone の移植）、MOD は ProTracker エンジンで鳴らします。
  - 対応している関数は `ahmod/include/libopenmpt/libopenmpt.h` にあります。チャンネルのミュート用に `ahmod_set_channel_mute` も足してあります。
- `tracker/` — Expo のネイティブモジュールの例（モジュール名 `AHTracker`）。iOS は AVAudioEngine、Android は AudioTrack で、曲をループ再生します。
  - JS からは `play(path, volume)`・`stop()`・`setVolume(volume)`・`isPlaying()` が使えます。
  - libopenmpt で作ったモジュールを、ビルド設定（`AHTracker.podspec` と `android/src/main/cpp/CMakeLists.txt`）だけ ahmod 用に変えたものです。Swift・Objective-C++・Kotlin・JNI のコードは libopenmpt のときと同じ呼び方をしています。
- `build-mobile.ps1` — iOS 用と Android 用の静的ライブラリを作って `tracker/` に置くスクリプト（Windows の PowerShell）。

## 1. ライブラリを作る

rustup と cargo だけあれば動きます。静的ライブラリを作るだけなので Xcode や Android NDK は要らず、リンクはアプリのビルド（EAS など）の側で行われます。

```powershell
powershell -ExecutionPolicy Bypass -File mobile\build-mobile.ps1
```

IT・S3M・XM だけを鳴らすアプリなら、MOD 用の ProTracker エンジンを外して約 50 KB 小さくできます（MOD ファイルは読めなくなります）。

```powershell
powershell -ExecutionPolicy Bypass -File mobile\build-mobile.ps1 -NoMod
```

できあがるもの:

- `tracker\android\libs\{arm64-v8a,armeabi-v7a,x86_64,x86}\libahmod.a`
- `tracker\ios\AHMod.xcframework`（端末用 arm64 と、Apple Silicon Mac のシミュレータ用 arm64）

Rust の `cargo build --target ...` が使える環境なら、macOS や Linux でも同じ手順（各ターゲットで `cargo build --release` して `.a` を置く）で作れます。

## 2. アプリに入れる

1. `tracker/` を、Expo アプリの `modules/` の下にコピーします（例：`modules/tracker`）。Expo の autolinking が `expo-module.config.json` を見つけて組み込みます。
2. libopenmpt 版のモジュールがすでにあるなら、それは `modules/` の外へ退避してください。中に残すと二重に読み込まれます。
3. アプリをいつもどおりビルドして（`npx expo run:ios` / `npx eas build` など）、実機で音を確かめます。

## 3. libopenmpt との違い

- 補間は線形です。libopenmpt の既定は 8 タップの sinc なので、高音の質感がわずかに違います。
- `set_repeat_count` は受け付けますが、曲は常にループします。
- 対応形式は MOD / IT / S3M / XM です。それ以外を読ませると、create は NULL を返します。
- ゲームの BGM に使っている実曲 26 曲（IT 10 曲・XM 16 曲）で libopenmpt と比べた結果:
  - 再生位置は全曲 100% 一致しました。
  - 音量比は 0.96〜1.02 でした。
  - CPU 時間は libopenmpt（既定の設定）の 0.19〜0.55 倍でした（PC で計測）。
- ライブラリの大きさは、libopenmpt の約 1.56 MB に対して約 0.5 MB です（PC 向けに組んだ場合）。

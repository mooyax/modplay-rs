# 🎼 トラッカーの部品（iOS）。音を作るのは modplay-rs の Rust エンジン（ahmod）——libopenmpt と同じ C の顔を持つ静的ライブラリを
#   ios/AHMod.xcframework に置き、AVAudioEngine へ流す（AHTrackerEngine.mm は libopenmpt の時のまま）。
#   xcframework は modplay-rs の mobile/build-mobile.ps1 で作る（端末用 arm64 と、シミュレータ用 arm64）。
# ⚠ 互換の頭（include/libopenmpt/libopenmpt.h）は**表に出さない**——Swift が読む傘の頭に混ぜない。道（HEADER_SEARCH_PATHS）で探させる。
Pod::Spec.new do |s|
  s.name           = 'AHTracker'
  s.version        = '2.0.0'
  s.summary        = 'Tracker music for Expo apps (modplay-rs Rust engines, libopenmpt-compatible C API)'
  s.license        = { :type => 'BSD-3-Clause', :file => 'LICENSE' }
  s.author         = 'mooyax'
  s.homepage       = 'https://github.com/mooyax/modplay-rs'
  s.platforms      = { :ios => '15.1' }
  s.swift_version  = '5.9'
  s.source         = { git: '' }
  s.static_framework = true

  s.dependency 'ExpoModulesCore'

  s.source_files = 'ios/**/*.{h,mm,swift}'
  s.public_header_files = 'ios/AHTrackerEngine.h'
  s.preserve_paths = ['include/**/*', 'ios/AHMod.xcframework']
  s.vendored_frameworks = 'ios/AHMod.xcframework'
  s.frameworks = 'AVFoundation'
  s.libraries = 'c++'

  s.pod_target_xcconfig = {
    'DEFINES_MODULE' => 'YES',
    'USE_HEADERMAP' => 'NO',
    'SWIFT_COMPILATION_MODE' => 'wholemodule',
    'CLANG_CXX_LANGUAGE_STANDARD' => 'c++17',
    'HEADER_SEARCH_PATHS' => '$(inherited) "$(PODS_TARGET_SRCROOT)/include"',
  }
end

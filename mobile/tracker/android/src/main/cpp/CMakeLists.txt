# 🎼 トラッカーの部品（Android）。音を作るのは modplay-rs の Rust エンジン（ahmod）——libopenmpt と同じ C の顔を持つ静的ライブラリ
#   android/libs/<ABI>/libahmod.a を ah_tracker_jni.cpp（libopenmpt の時のまま）と一緒に libahtracker.so へ組む。
#   .a は modplay-rs の mobile/build-mobile.ps1 で作る（arm64-v8a / armeabi-v7a / x86_64 / x86）。
cmake_minimum_required(VERSION 3.13)
project(ahtracker CXX)

set(AHMOD_LIB ${CMAKE_CURRENT_SOURCE_DIR}/../../../libs/${ANDROID_ABI}/libahmod.a)
set(AHMOD_INCLUDE ${CMAKE_CURRENT_SOURCE_DIR}/../../../../include)
if(NOT EXISTS ${AHMOD_LIB})
  message(FATAL_ERROR "libahmod.a for ${ANDROID_ABI} not found: run mobile/build-mobile.ps1 in modplay-rs and copy the module again")
endif()

add_library(ahmod STATIC IMPORTED)
set_target_properties(ahmod PROPERTIES IMPORTED_LOCATION ${AHMOD_LIB})

add_library(ahtracker SHARED ah_tracker_jni.cpp)
target_include_directories(ahtracker PRIVATE ${AHMOD_INCLUDE})
target_compile_options(ahtracker PRIVATE -std=c++17)
# Rust の標準ライブラリが使う dl / m と、ログの log
target_link_libraries(ahtracker ahmod log dl m)
# ⚠⚠ **16KB のページで組む**——Google Play は 16 KB のメモリページへの対応を求めている。
#   Android 15 以降の 16KB ページの端末で読めるよう、読み込みの区切りを 16KB に揃える。
#   使われない Rust のコードは --gc-sections で落とす
target_link_options(ahtracker PRIVATE "-Wl,-z,max-page-size=16384" "-Wl,-z,common-page-size=16384" "-Wl,--gc-sections")

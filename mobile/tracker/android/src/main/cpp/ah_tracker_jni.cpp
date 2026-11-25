// 🎼 AHTrackerModule（Kotlin）と ahmod（libopenmpt 互換の C API）の橋
#include <jni.h>
#include <cstdint>
#include "libopenmpt/libopenmpt.h"

extern "C" {

JNIEXPORT jlong JNICALL
Java_expo_modules_ahtracker_Engine_nativeLoad(JNIEnv *env, jclass, jbyteArray data) {
  jsize len = env->GetArrayLength(data);
  jbyte *bytes = env->GetByteArrayElements(data, nullptr);
  // ahmod（libopenmpt も同じ）は中身を自分の側へ写すので、渡したらすぐ返してよい
  openmpt_module *mod = openmpt_module_create_from_memory2(bytes, (size_t)len, nullptr, nullptr, nullptr, nullptr, nullptr, nullptr, nullptr);
  env->ReleaseByteArrayElements(data, bytes, JNI_ABORT);
  if (!mod) return 0;
  openmpt_module_set_repeat_count(mod, -1);   // 終わりなく繰り返す
  return (jlong)(intptr_t)mod;
}

// 左右交互（LRLR…）で frames ぶん書く。書けた数を返す（0 なら終わり）
JNIEXPORT jint JNICALL
Java_expo_modules_ahtracker_Engine_nativeRender(JNIEnv *env, jclass, jlong mod, jint rate, jfloatArray out, jint frames) {
  jfloat *buf = env->GetFloatArrayElements(out, nullptr);
  size_t n = openmpt_module_read_interleaved_float_stereo((openmpt_module *)(intptr_t)mod, rate, (size_t)frames, buf);
  env->ReleaseFloatArrayElements(out, buf, 0);
  return (jint)n;
}

JNIEXPORT void JNICALL
Java_expo_modules_ahtracker_Engine_nativeDestroy(JNIEnv *, jclass, jlong mod) {
  openmpt_module_destroy((openmpt_module *)(intptr_t)mod);
}

}

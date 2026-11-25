// 🎼 トラッカーの曲を端末で鳴らす（Android）——modplay-rs の ahmod（JNI 経由）で音を作り、AudioTrack へ流す
//
// ⭐ **音は再生の糸で作る**：AudioTrack の書き込みが詰まるまで、エンジンに要るだけ書かせる。
//   ファイルに書き出さないので、**曲の繋ぎ目はエンジンの繰り返しそのもの**（ループの継ぎ目が切れない）。
// ⚠ 曲の差し替えは JS の糸から来るので、**`lock` で挟む**（描画側は塊ごとに短く取る）。
// ⚠ 音の焦点（AudioFocus）は取らない——アプリ側（たとえば効果音の expo-audio）と取り合うと、どちらかが黙る。
package expo.modules.ahtracker

import android.media.AudioAttributes
import android.media.AudioFormat
import android.media.AudioTrack
import expo.modules.kotlin.modules.Module
import expo.modules.kotlin.modules.ModuleDefinition
import java.io.File

class AHTrackerModule : Module() {
  override fun definition() = ModuleDefinition {
    Name("AHTracker")

    Function("play") { path: String, volume: Double ->
      Engine.play(path.removePrefix("file://"), volume.toFloat())
    }
    Function("stop") { Engine.stop() }
    Function("setVolume") { volume: Double -> Engine.setVolume(volume.toFloat()) }
    Function("isPlaying") { Engine.isPlaying() }
  }
}

private object Engine {
  private const val RATE = 48000
  private val lock = Any()
  private var mod = 0L
  private var path: String? = null
  private var volume = 1f
  private var track: AudioTrack? = null
  private var thread: Thread? = null
  @Volatile private var running = false

  init { System.loadLibrary("ahtracker") }

  @JvmStatic private external fun nativeLoad(data: ByteArray): Long
  @JvmStatic private external fun nativeRender(mod: Long, rate: Int, out: FloatArray, frames: Int): Int
  @JvmStatic private external fun nativeDestroy(mod: Long)

  fun play(p: String, v: Float): Boolean {
    setVolume(v)
    synchronized(lock) { if (mod != 0L && p == path && running) return true }
    val bytes = try { File(p).readBytes() } catch (e: Exception) { return false }
    val m = nativeLoad(bytes)
    if (m == 0L) return false
    val old: Long
    synchronized(lock) { old = mod; mod = m; path = p }
    if (old != 0L) nativeDestroy(old)
    start()
    return true
  }

  fun stop() {
    running = false
    thread?.join(500)
    thread = null
    track?.let { try { it.stop() } catch (_: Exception) {}; it.release() }
    track = null
    val old: Long
    synchronized(lock) { old = mod; mod = 0L; path = null }
    if (old != 0L) nativeDestroy(old)
  }

  fun setVolume(v: Float) {
    volume = v.coerceIn(0f, 1f)
    track?.setVolume(volume)
  }

  fun isPlaying(): Boolean = running && synchronized(lock) { mod != 0L }

  private fun start() {
    if (running) return
    val minBytes = AudioTrack.getMinBufferSize(RATE, AudioFormat.CHANNEL_OUT_STEREO, AudioFormat.ENCODING_PCM_FLOAT)
    val t = AudioTrack.Builder()
      .setAudioAttributes(AudioAttributes.Builder()
        .setUsage(AudioAttributes.USAGE_GAME)
        .setContentType(AudioAttributes.CONTENT_TYPE_MUSIC)
        .build())
      .setAudioFormat(AudioFormat.Builder()
        .setSampleRate(RATE)
        .setEncoding(AudioFormat.ENCODING_PCM_FLOAT)
        .setChannelMask(AudioFormat.CHANNEL_OUT_STEREO)
        .build())
      .setBufferSizeInBytes(maxOf(minBytes, RATE / 5 * 8))   // 0.2 秒ぶん（途切れにくく、止めても遅れが小さい）
      .setTransferMode(AudioTrack.MODE_STREAM)
      .build()
    t.setVolume(volume)
    t.play()
    track = t
    running = true
    thread = Thread({
      val frames = 1024
      val buf = FloatArray(frames * 2)
      while (running) {
        val n = synchronized(lock) { if (mod != 0L) nativeRender(mod, RATE, buf, frames) else 0 }
        if (n <= 0) java.util.Arrays.fill(buf, 0f)
        t.write(buf, 0, frames * 2, AudioTrack.WRITE_BLOCKING)
      }
    }, "ah-tracker").apply { priority = Thread.MAX_PRIORITY; start() }
  }
}

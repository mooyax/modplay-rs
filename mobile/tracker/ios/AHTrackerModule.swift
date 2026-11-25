// 🎼 JS から見える顔（`globalThis.expo.modules.AHTracker`）。中身は AHTrackerEngine（Objective-C++）
import ExpoModulesCore

public class AHTrackerModule: Module {
  public func definition() -> ModuleDefinition {
    Name("AHTracker")

    // 曲の道（file://… でもよい）と音量（0〜1）。鳴り始めたら true
    Function("play") { (path: String, volume: Double) -> Bool in
      return AHTrackerEngine.shared().playPath(path, volume: Float(volume))
    }

    Function("stop") {
      AHTrackerEngine.shared().stop()
    }

    Function("setVolume") { (volume: Double) in
      AHTrackerEngine.shared().setVolume(Float(volume))
    }

    Function("isPlaying") { () -> Bool in
      return AHTrackerEngine.shared().isPlaying()
    }
  }
}

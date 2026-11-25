// 🎼 ahmod（libopenmpt 互換の C API）→ AVAudioEngine
//
// ⭐ **音を作るのは再生の糸の上**（AVAudioSourceNode の描画の塊ごとに、エンジンに要るだけ書かせる）。
//   ファイルに書き出さないので、**曲の繋ぎ目はエンジンの繰り返しそのもの**（ループの継ぎ目が切れない）。
// ⚠ 曲の差し替えは別の糸から来るので、**鍵（os_unfair_lock）で挟む**。描画の糸では待たない（取れなければ無音を返す）。
// ⚠ 音の部屋（AVAudioSession）には触らない——アプリ側（たとえば効果音の expo-audio）が決めたものに乗る（二人で取り合うと、どちらかが黙る）。
#import "AHTrackerEngine.h"
#import <AVFoundation/AVFoundation.h>
#import <os/lock.h>
#include "libopenmpt/libopenmpt.h"

@implementation AHTrackerEngine {
  AVAudioEngine *_engine;
  AVAudioSourceNode *_source;
  os_unfair_lock _lock;
  openmpt_module *_mod;
  NSString *_path;
  double _rate;
  float _volume;
}

+ (instancetype)shared {
  static AHTrackerEngine *s;
  static dispatch_once_t once;
  dispatch_once(&once, ^{ s = [AHTrackerEngine new]; });
  return s;
}

- (instancetype)init {
  if ((self = [super init])) {
    _lock = OS_UNFAIR_LOCK_INIT;
    _volume = 1;
    // 電話・他のアプリ・耳当ての抜き差しで、音の機関は黙って止まる——鳴らしているはずなら起こし直す
    [[NSNotificationCenter defaultCenter] addObserver:self selector:@selector(restartIfNeeded)
                                                 name:AVAudioEngineConfigurationChangeNotification object:nil];
    [[NSNotificationCenter defaultCenter] addObserver:self selector:@selector(restartIfNeeded)
                                                 name:AVAudioSessionInterruptionNotification object:nil];
  }
  return self;
}

- (void)ensureEngine {
  if (_engine) return;
  _engine = [AVAudioEngine new];
  AVAudioFormat *out = [_engine.outputNode outputFormatForBus:0];
  _rate = out.sampleRate > 0 ? out.sampleRate : 48000;
  AVAudioFormat *fmt = [[AVAudioFormat alloc] initStandardFormatWithSampleRate:_rate channels:2];
  __unsafe_unretained AHTrackerEngine *weakSelf = self;
  _source = [[AVAudioSourceNode alloc] initWithFormat:fmt renderBlock:^OSStatus(BOOL *isSilence, const AudioTimeStamp *ts, AVAudioFrameCount frames, AudioBufferList *abl) {
    return [weakSelf render:frames into:abl silence:isSilence];
  }];
  _source.volume = _volume;
  [_engine attachNode:_source];
  [_engine connect:_source to:_engine.mainMixerNode format:fmt];
}

- (OSStatus)render:(AVAudioFrameCount)frames into:(AudioBufferList *)abl silence:(BOOL *)isSilence {
  float *l = (float *)abl->mBuffers[0].mData;
  float *r = abl->mNumberBuffers > 1 ? (float *)abl->mBuffers[1].mData : l;
  AVAudioFrameCount done = 0;
  if (os_unfair_lock_trylock(&_lock)) {
    if (_mod) {
      while (done < frames) {
        size_t n = openmpt_module_read_float_stereo(_mod, (int32_t)_rate, frames - done, l + done, r + done);
        if (n == 0) break;   // 終わり（繰り返しは -1 なので、ふつうは来ない）
        done += (AVAudioFrameCount)n;
      }
    }
    os_unfair_lock_unlock(&_lock);
  }
  if (done < frames) {
    memset(l + done, 0, (frames - done) * sizeof(float));
    if (r != l) memset(r + done, 0, (frames - done) * sizeof(float));
  }
  if (done == 0) *isSilence = YES;
  return noErr;
}

- (BOOL)playPath:(NSString *)path volume:(float)volume {
  if ([path hasPrefix:@"file://"]) path = [[NSURL URLWithString:path] path];
  [self setVolume:volume];
  if (_mod && [_path isEqualToString:path] && _engine.isRunning) return YES;
  NSData *data = [NSData dataWithContentsOfFile:path];
  if (!data) return NO;
  openmpt_module *mod = openmpt_module_create_from_memory2(data.bytes, data.length, NULL, NULL, NULL, NULL, NULL, NULL, NULL);
  if (!mod) return NO;
  openmpt_module_set_repeat_count(mod, -1);
  [self ensureEngine];
  os_unfair_lock_lock(&_lock);
  openmpt_module *old = _mod;
  _mod = mod;
  _path = [path copy];
  os_unfair_lock_unlock(&_lock);
  if (old) openmpt_module_destroy(old);
  if (!_engine.isRunning) {
    NSError *err = nil;
    [_engine prepare];
    if (![_engine startAndReturnError:&err]) return NO;
  }
  return YES;
}

- (void)stop {
  os_unfair_lock_lock(&_lock);
  openmpt_module *old = _mod;
  _mod = NULL;
  _path = nil;
  os_unfair_lock_unlock(&_lock);
  if (old) openmpt_module_destroy(old);
  if (_engine.isRunning) [_engine pause];
}

- (void)setVolume:(float)volume {
  if (volume < 0) volume = 0;
  if (volume > 1) volume = 1;
  _volume = volume;
  if (_source) _source.volume = volume;   // 機関がまだなら、作るときに入れる
}

- (BOOL)isPlaying {
  return _mod != NULL && _engine.isRunning;
}

- (void)restartIfNeeded {
  dispatch_async(dispatch_get_main_queue(), ^{
    if (self->_mod && self->_engine && !self->_engine.isRunning) {
      [self->_engine prepare];
      [self->_engine startAndReturnError:nil];
    }
  });
}

@end

// 🎼 トラッカーの曲（.mod / .it / .s3m / .xm）を端末で鳴らす——modplay-rs の ahmod（libopenmpt 互換の C API）で音を作り、AVAudioEngine へ流す
// Swift（AHTrackerModule）から呼ぶので、ここは Objective-C の顔だけを出す。C の呼び出しは .mm の中に閉じる。
#import <Foundation/Foundation.h>

NS_ASSUME_NONNULL_BEGIN

@interface AHTrackerEngine : NSObject
+ (instancetype)shared;
// 曲を読んで頭から鳴らす（繰り返しは終わりなし）。同じ道の曲が鳴っていれば何もしない。読めなければ NO
- (BOOL)playPath:(NSString *)path volume:(float)volume;
- (void)stop;
- (void)setVolume:(float)volume;
- (BOOL)isPlaying;
@end

NS_ASSUME_NONNULL_END

// Headless reference renderer using the original pt2-clone engine code.
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <SDL2/SDL.h>
#include "pt2_header.h"
#include "pt2_structs.h"
#include "pt2_config.h"
#include "pt2_audio.h"
#include "pt2_paula.h"
#include "pt2_replayer.h"
#include "pt2_downsample2x.h"
#include "pt2_module_loader.h"

module_t *song = NULL;

int main(int argc, char **argv)
{
	if (argc < 3) { fprintf(stderr, "usage: harness in.mod out.wav [stereo] [model 0/1] [loops] [fade]\n"); return 1; }

	memset(&config, 0, sizeof(config));
	config.amigaModel = MODEL_A1200;
	config.stereoSeparation = 20;
	config.mod2WavOutputFreq = 44100;
	config.maxSampleLength = 65534;
	config.soundFrequency = 48000;
	config.soundBufferSize = 1024;
	if (argc > 3) config.stereoSeparation = atoi(argv[3]);
	if (argc > 4) config.amigaModel = atoi(argv[4]);
	editor.mod2WavNumLoops = (argc > 5) ? atoi(argv[5]) : 0;
	int fade = (argc > 6) ? atoi(argv[6]) : 0;
	editor.mod2WavFadeOutSeconds = 6;
	if (fade > 0) { editor.mod2WavFadeOut = true; editor.mod2WavFadeOutSeconds = fade; }

	editor.initialTempo = 125;
	editor.initialSpeed = 6;

	SDL_setenv("SDL_AUDIODRIVER", "dummy", 1);
	SDL_Init(SDL_INIT_AUDIO | SDL_INIT_TIMER);

	editor.mod2WavOngoing = true; // keep the audio callback silent
	if (!setupAudio()) { fprintf(stderr, "setupAudio failed\n"); return 1; }

	song = createEmptyMod();
	module_t *m = modLoad(argv[1]);
	if (m == NULL) { fprintf(stderr, "modLoad failed\n"); return 1; }
	free(song); song = m;

	// setupLoadedMod() (playback-relevant parts)
	editor.muted[0] = editor.muted[1] = editor.muted[2] = editor.muted[3] = false;
	setLEDFilter(false);
	editor.timingMode = TEMPO_MODE_CIA;
	updateReplayerTimingMode();
	modSetSpeed(6);
	modSetTempo(song->header.initialTempo, false);

	// mod2WavRender()
	const int32_t paulaMixFrequency = config.mod2WavOutputFreq * 2;
	audio.oversamplingFlag = true;
	generateBpmTable(config.mod2WavOutputFreq, editor.timingMode == TEMPO_MODE_VBLANK);
	paulaSetup(paulaMixFrequency, audio.amigaModel);
	storeTempVariables();
	restartSong();
	clearDownsample2xStates();

	// mod2WavThreadFunc()
	int32_t maxSamplesPerTick = (int32_t)ceil(paulaMixFrequency / (MIN_BPM / 2.5)) + 1;
	int16_t *buf = (int16_t *)malloc(maxSamplesPerTick * 2 * sizeof(int16_t));
	size_t cap = 1 << 22, n = 0;
	int16_t *all = (int16_t *)malloc(cap * sizeof(int16_t));

	uint64_t samplesToMixFrac = 0;
	int8_t numLoops = editor.mod2WavNumLoops;
	bool renderDone = false;
	const char *mf = getenv("MAXFRAMES");
	size_t maxFrames = mf ? (size_t)atol(mf) : 0;
	while (!renderDone)
	{
		if (maxFrames && n/2 >= maxFrames) break;
		if (!tickReplayer())
		{
			if (--numLoops < 0) renderDone = true;
			else memset(editor.rowVisitTable, 0, 128 * MOD_ROWS);
		}

		uint32_t samplesToMix = audio.samplesPerTickInt;
		samplesToMixFrac += audio.samplesPerTickFrac;
		if (samplesToMixFrac >= BPM_FRAC_SCALE) { samplesToMixFrac &= BPM_FRAC_MASK; samplesToMix++; }

		outputAudio(buf, samplesToMix);
		if (n + samplesToMix * 2 > cap) { cap *= 2; all = realloc(all, cap * sizeof(int16_t)); }
		memcpy(all + n, buf, samplesToMix * 2 * sizeof(int16_t));
		n += samplesToMix * 2;
	}

	uint32_t frames = (uint32_t)(n / 2);
	if (editor.mod2WavFadeOut)
	{
		uint32_t numFade = config.mod2WavOutputFreq * editor.mod2WavFadeOutSeconds;
		if (numFade > frames) numFade = frames;
		const double d = 1.0 / numFade; double v = 1.0;
		int16_t *p = all + (frames - numFade) * 2;
		for (uint32_t i = 0; i < numFade; i++)
		{
			p[i*2+0] = (int16_t)(p[i*2+0] * v);
			p[i*2+1] = (int16_t)(p[i*2+1] * v);
			v -= d;
		}
	}

	wavHeader_t h;
	h.chunkID = 0x46464952; h.chunkSize = 36 + frames * 4; h.format = 0x45564157;
	h.subchunk1ID = 0x20746D66; h.subchunk1Size = 16; h.audioFormat = 1; h.numChannels = 2;
	h.sampleRate = config.mod2WavOutputFreq; h.bitsPerSample = 16;
	h.byteRate = h.sampleRate * 4; h.blockAlign = 4;
	h.subchunk2ID = 0x61746164; h.subchunk2Size = frames * 4;
	FILE *f = fopen(argv[2], "wb");
	fwrite(&h, sizeof(h), 1, f);
	fwrite(all, sizeof(int16_t), n, f);
	fclose(f);
	fprintf(stderr, "frames=%u\n", frames);
	return 0;
}

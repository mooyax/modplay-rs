# modplay-rs

English | [日本語](README.ja.md)

Tracker music (MOD, IT, S3M, XM) playback engines and a player, written in pure Rust.

**▶ Try it in your browser: https://mooyax.github.io/modplay-rs/** (drop a module file on the page, then click to start the sound)

- **MOD**: a port of the replayer of [pt2-clone](https://github.com/8bitbubsy/pt2-clone) (8bitbubsy's ProTracker 2 clone). It sounds the same as the original.
- **XM**: a port of the replayer of [ft2-clone](https://github.com/8bitbubsy/ft2-clone) (8bitbubsy's FastTracker 2 clone).
- **IT, S3M**: an engine of our own, checked song by song against libopenmpt so that playback position and loudness match.
- **modplay**: a GUI player with a tracker-style screen (Windows / Linux / macOS).
- **web/**: the same player in the browser (WebAssembly). Nothing to install: drop a module on the page and listen ([live version](https://mooyax.github.io/modplay-rs/)).
- **mobile/**: a static library for iOS and Android that exports the same C functions as libopenmpt, to replace libopenmpt in existing apps ([details](mobile/README.md)).

No C/C++ libraries are needed (only the comparison tools can optionally use libopenmpt).

![The modplay player](docs/screenshot.png)

> **The playback engines now live in [featheraudio](https://github.com/mooyax/featheraudio)** (a feather-light game audio engine), under `src/tracker/` with the features `mod`, `it` and `xm`. modplay-rs is the player built on top of them: the GUI, the browser version, the mobile library and the tools. Paths below such as `tracker/protracker/paula.rs` are in the featheraudio repository.


## ProTracker engine (MOD)

| Original C file | Rust | Contents |
|---|---|---|
| `pt2_paula.c` | `tracker/protracker/paula.rs` | Paula (Amiga sound chip) emulation, BLEP synthesis |
| `pt2_blep.c` / `pt2_rcfilters.c` / `pt2_downsample2x.c` | `tracker/protracker/dsp.rs` | minBLEP, A500/A1200 RC filters and LED filter, 2x downsampler |
| `pt2_replayer.c` | `tracker/protracker/replayer.rs` | ProTracker 2.3D replayer (all effects, including PT's quirks) |
| `pt2_audio.c` (output part) | `tracker/protracker/engine.rs` | mixer, stereo separation, dithering, BPM → tick length table |
| `pt2_module_loader.c` / `pt2_load_mod31.c` / `pt2_load_mod15.c` | `tracker/protracker/module.rs` | MOD loading (31/15 samples, NT/HMNT/FLT/STK conversion) |
| `pt2_mod2wav.c` | `src/mod2wav.rs` (modplay-rs) | MOD → WAV rendering (loop count, fade-out) |
| `pt2_tables.c` | `tracker/protracker/tables.rs` | period, vibrato and BLEP tables (generated from the C sources) |

The playback engines (the library) and `pt2wav` have no external crate dependencies. The `modplay` GUI uses `eframe` (egui, for the window and screen), `cpal` (audio output) and `rfd` (file dialog, Windows only).

### Usage

```
cargo build --release
target\release\pt2wav.exe song.mod                 # writes song.wav
target\release\pt2wav.exe song.mod out.wav --model a500 --stereo 100 --loops 1 --fadeout 6
target\release\pt2wav.exe --help
```

As a library:

```rust
let data = std::fs::read("song.mod")?;
let song = modplay::Module::load(&data, modplay::module::DEFAULT_MAX_SAMPLE_LENGTH)?;
let mut engine = modplay::Engine::new(song, modplay::EngineConfig::default());

// real-time playback (call from an audio callback)
engine.play_song();
let mut buf = vec![0i16; 1024 * 2]; // interleaved stereo
engine.render(&mut buf);
```

### Verification against the original C version

`tools/` contains the verification tools:

- `tools/cref/` … a headless reference MOD2WAV that builds the original pt2-clone C sources (engine part) unchanged (Linux / WSL; needs the SDL2 development headers)
- `tools/compare.sh` … renders the same MOD with the C and the Rust version and compares the WAV files byte by byte
- `tools/genfuzz.py` … generates MODs that use every effect at random (fuzzing)

Results (as of 2026-11-23):

- libxmp's 42 test MODs: **all byte-identical** (A1200/A500, stereo separation 0/20/100%, with each loop and fade-out setting)
  - Three of the MOD files are truncated. The C version reads past the end of the file there (undefined behaviour), so they do not match as they are; the Rust version treats the missing part as silence (0). With the missing bytes padded with zeros, all three match exactly.
- 300 randomly generated MODs (A1200, stereo separation 20%): **all byte-identical**
- 200 randomly generated MODs (A500, stereo separation 100%): **all byte-identical**

```
# on WSL or similar
git clone https://github.com/8bitbubsy/pt2-clone
tools/cref/build.sh ./pt2-clone
ls *.mod | tools/compare.sh      # (adjust the paths in compare.sh to your setup)
```

Note: some songs never end (certain combinations of E6x pattern loops, for example), and the original pt2-clone keeps rendering them forever. To be safe, `pt2wav` stops after 30 minutes by default (`--max-seconds 1800`); `--max-seconds 0` removes the limit.

## modplay: tracker-style player (GUI)

Plays modules on a tracker-style screen (made with egui). All sound comes from the Rust engines; no C/C++ libraries are used.

- **MOD** (4-channel ProTracker format): the ProTracker engine (pt2-clone port, sounds like the original)
- **IT, S3M**: the Rust IT engine (`tracker/it/`)
- **XM**: the Rust XM engine (`tracker/xm/`, a port of ft2-clone's replayer)

The screen:

- Top: play / pause, stop, previous / next position, open file, song title, POSITION / PATTERN / ROW / SPEED / TEMPO / TIME, About
- Channel headers: a scope per channel (the real waveform), VU meter, M (mute), S (solo)
- Pattern: all channels, scrolled so that the playing row stays in the middle. Columns are `note instrument volume-column effect`; with many channels it scrolls sideways.
- Right: the order list (click to jump there) and the instrument / sample list
- Files can also be dropped on the window.

```
cargo build --release
target\release\modplay.exe                 # opens a file dialog
target\release\modplay.exe song.it
```

| Key | Action |
|---|---|
| Space | play / pause |
| PageUp / PageDown (or - / +) | previous / next position |
| O | open a file |
| Esc | quit |
| M / S buttons | mute / solo a channel |

Options: `--size 1600x900` (window size), `--stereo 0-100` (stereo separation, default 20), `--screenshot out.png --at seconds` (save the window as PNG and quit).

### Browser version (`web/`)

The same engines and screen, compiled to WebAssembly and running in the browser. Drop a module on the page or open one with the folder button. Browsers only allow sound after a user action, so the sound starts with the first click (or key press).

```
powershell -ExecutionPolicy Bypass -File web\build-web.ps1 -Serve
```

This builds the page (`index.html`, `.js`, `.wasm`) into `web\dist`; with `-Serve` it also opens it at http://localhost:8000/ (needs Python). WebAssembly pages do not work when opened as files (file://), so open them through a web server.

- Add `?url=<module URL>` to open the page with that module loaded (the server hosting the module must allow CORS).
- When the repository is public, `.github/workflows/web.yml` builds the browser version on every push and publishes it on GitHub Pages (set Settings → Pages → Source to "GitHub Actions").

### Only for the comparison tools: setting up libopenmpt

The comparison tools in `examples/` (itcmp, xmcmp, and bench in mpt mode) play the same songs with libopenmpt to check the engines. They are only built with `--features openmpt`, and only then is libopenmpt needed. The player and the engines do not need it.

- Windows: download the **libopenmpt** development package for Windows 10 / amd64 from https://lib.openmpt.org/libopenmpt/download/ and extract it into a `libopenmpt` folder in the project root. The `.lib` and `.dll` files are found automatically at build time, and the DLLs are copied to `target\release`. To keep it elsewhere, set the `LIBOPENMPT_DIR` environment variable.
- Linux / macOS: install libopenmpt with your package manager (it is found through pkg-config).

```
cargo run --release --features openmpt --example itcmp -- song.it 60
```

## Rust IT engine (`tracker/it/`)

- `s3m.rs`: S3M loading. Converts to the same internal format as IT; ST3-specific behaviour (period-based pitch, shared effect memory, fast volume slides, loop priority and so on) is switched on in the player.
- `format.rs`: IT loading (old and new instrument formats, IT 2.14/2.15 compressed samples, stereo samples, packed patterns)
- `player.rs`: playback (effects A–Z, volume column, envelopes with carry, fade-out, NNA/DCT/DCA, pattern loops, auto-vibrato, resonant filters and more)
  - Resonant filters: the instrument's initial cutoff / resonance, Zxx (MIDI macros, from the file's embedded configuration or the defaults, switched with SFx) and the filter envelope. The coefficients use the same formulas as OpenMPT's IT-compatible mode.
- `examples/itcmp.rs`: plays the same song with libopenmpt side by side and compares playback position and loudness

```
cargo run --release --features openmpt --example itcmp -- song.it 60          # compare 60 seconds
cargo run --release --features openmpt --example itcmp -- song.it 60 --solo 3 # channel 3 only
```

Of libxmp's 114 IT test files, 107 match libopenmpt's playback position (order and row) exactly. Real songs (Fight2.it, 4th_Symmetriad.it and others) also match in loudness; even 4th_Symmetriad.it, which uses filters heavily, is within ±0.5 dB in every frequency band.

## Rust XM engine (`tracker/xm/`)

The playback logic is ported to Rust from 8bitbubsy's [ft2-clone](https://github.com/8bitbubsy/ft2-clone), a faithful reproduction of FastTracker II's replayer.

- `format.rs`: XM loading (v1.02–1.04, delta-encoded samples, ModPlug ADPCM, stereo samples mixed to mono, the same value clamping as FT2)
- `player.rs`: playback (tickReplayer / getNewNote / all effects / volume column / envelopes / fade-out / auto-vibrato), including FT2's quirks such as its 16-bit period arithmetic. The mixer is our own (linear interpolation, volume ramps); output level and pan law follow libopenmpt (songs made with FT2 use FT2's square-root pan law).
- `tables.rs`: ft2-clone's period, vibrato and other tables
- `examples/xmcmp.rs`: comparison tool against libopenmpt (used like itcmp)

With the five real songs that come with MilkyTracker, playback position matches exactly and the loudness ratio is 0.99–1.01. Where libopenmpt differs from ft2-clone on test songs for FT2's finer behaviour, ft2-clone (real FT2) is taken as correct. OpenMPT's own extensions (stereo samples, reverse playback and so on) are not supported.

## Not supported

- Unpacking MODs compressed with PowerPacker (PP20) or XPK (`pt2_pp_unpack.c` / `pt2_xpk_unpack.c`)
- IT engine: MIDI macros for external MIDI output or VST, OpenMPT's own extensions (smooth MIDI macros `\xx` and so on)
- XM engine: OpenMPT's own extensions (stereo samples, reverse playback and so on)
- Linear interpolation only (libopenmpt defaults to an 8-tap sinc)

## Acknowledgements

This project stands on the work of these projects and people:

- **[pt2-clone](https://github.com/8bitbubsy/pt2-clone)** (Olav Sørensen / 8bitbubsy) — the MOD engine is a port of its replayer, Paula emulation and filters.
- **[ft2-clone](https://github.com/8bitbubsy/ft2-clone)** (also by Olav Sørensen) — the XM engine is a port of its replayer.
- **[OpenMPT / libopenmpt](https://openmpt.org/)** (the OpenMPT Project Developers, Olivier Lapicque) — the IT and S3M engine was built using libopenmpt's playback as the reference. The resonant filter coefficients and the mix-level tables for old ModPlug-made songs follow OpenMPT's source.
- **[libxmp](https://github.com/libxmp/libxmp)** — its test modules were used to check the engines (they are not included in this repository).
- And the original trackers: Ultimate Soundtracker and ProTracker (Amiga), Scream Tracker 3 (Future Crew), FastTracker 2 (Triton), Impulse Tracker (Jeffrey Lim). Thanks to everyone who has made music with them.

The player's "About" button shows the same credits.

## License

BSD 3-Clause (see `LICENSE`). The ProTracker engine is derived from pt2-clone and the XM engine from ft2-clone (both by Olav Sørensen, BSD 3-Clause). The IT / S3M engine uses formulas and tables from OpenMPT (BSD 3-Clause). Their copyright notices are included in `LICENSE`.

//! modplay-rs: a tracker music player (MOD / IT / S3M / XM).
//!
//! The playback engines live in [featheraudio](https://github.com/mooyax/featheraudio)
//! (`featheraudio::tracker`); this crate adds the player layer used by the GUI, the browser
//! version, the mobile library and the tools (a common `Player` trait, MOD2WAV, the optional
//! libopenmpt bindings). The engine modules are re-exported here under their old paths.

pub use featheraudio::tracker::protracker::{consts, dsp, engine, module, paula, replayer, tables};
pub use featheraudio::tracker::{it, xm};

pub mod mod2wav;
pub mod player;
#[cfg(has_openmpt)]
pub mod openmpt;

pub use consts::{AmigaModel, TimingMode};
pub use engine::{Engine, EngineConfig};
pub use mod2wav::{render_mod, write_wav, Mod2WavOptions};
pub use module::{LoadError, Module};

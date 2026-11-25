//! modplay-rs: tracker music playback in pure Rust. The ProTracker engine is a port of
//! 8bitbubsy's pt2-clone (ProTracker 2 clone).
//!
//! Phase 1: playback engine (MOD loader, ProTracker 2.3D replayer,
//! Paula emulation with BLEP + Amiga filters) and MOD2WAV.

pub mod consts;
pub mod dsp;
pub mod engine;
pub mod mod2wav;
pub mod module;
pub mod paula;
pub mod it;
pub mod xm;
pub mod player;
#[cfg(has_openmpt)]
pub mod openmpt;
pub mod replayer;
pub mod tables;

pub use consts::{AmigaModel, TimingMode};
pub use engine::{Engine, EngineConfig};
pub use mod2wav::{render_mod, write_wav, Mod2WavOptions};
pub use module::{LoadError, Module};

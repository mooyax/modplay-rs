//! Impulse Tracker (.IT) engine written in Rust.

pub mod format;
pub mod s3m;
pub mod player;

pub use format::ItModule;
pub use player::ItPlayer;

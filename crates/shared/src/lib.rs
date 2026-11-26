//! Types partages entre le control-plane, l'agent, le proxy et la CLI.

pub mod config;
pub mod error;
pub mod ids;
pub mod images;
#[cfg(feature = "latest")]
pub mod latest;
#[cfg(feature = "age")]
pub mod secretbox;
#[cfg(feature = "update")]
pub mod update;
pub mod validate;

pub const NAME: &str = "ciphera";

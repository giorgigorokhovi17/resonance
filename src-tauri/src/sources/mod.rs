//! Getting music in from elsewhere: the sources live in the core; the app adds its Tauri
//! commands and where the Soulseek sign-in is kept.

pub use undertone_core::sources::*;

pub mod commands;
#[cfg(windows)]
pub mod credentials;

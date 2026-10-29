//! Undertone's core, shared by the Windows app and the Android app: the audio engine, the
//! library and its database, playlist import and the matcher, backups, and the download sources.
//! Nothing here depends on Tauri; events go out through [`events::Events`].

pub mod audio;
pub mod backup;
pub mod events;
pub mod library;
pub mod persist;
pub mod queue;
pub mod rt;
pub mod settings;
pub mod sleep;
pub mod sources;

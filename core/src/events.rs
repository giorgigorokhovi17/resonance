//! Events from the core to whatever shows them: the Tauri UI on both platforms, and on Android
//! also the media session. The shell decides where each one goes.

use std::sync::Arc;

use serde::Serialize;

type Sink = dyn Fn(&str, serde_json::Value) + Send + Sync;

/// Sends named events with a JSON payload ("slsk:status", "sleep:changed", …).
#[derive(Clone)]
pub struct Events(Arc<Sink>);

impl Events {
    pub fn new(sink: impl Fn(&str, serde_json::Value) + Send + Sync + 'static) -> Self {
        Self(Arc::new(sink))
    }

    /// Events that go nowhere (tests, tools).
    pub fn none() -> Self {
        Self::new(|_, _| {})
    }

    pub fn emit<T: Serialize>(&self, event: &str, payload: T) {
        match serde_json::to_value(payload) {
            Ok(v) => (self.0)(event, v),
            Err(e) => log::warn!("event {event} couldn't be sent: {e}"),
        }
    }
}

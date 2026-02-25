/// MANET/Reticulum transport stub.
///
/// Drop-in replacement for [`crate::sync::HttpSyncTransport`] that routes
/// tile batches over a mesh network instead of HTTP/VPN.
///
/// Current status: **stub** — returns NotImplemented until Reticulum
/// integration is wired in Phase 5. The interface is complete so the
/// collector and hub work unchanged when this transport is selected.
///
/// ## How it will work (Phase 5)
/// ```text
/// Collector → ManetSyncTransport::push()
///                → Reticulum LXMF message (chunk if >180 bytes)
///                → Mesh network → hub Reticulum node
///                → hub ManetIngester::receive()
///                → HubDb::merge_update()
/// ```
///
/// ## Configuration
/// ```toml
/// [sync]
/// transport = "manet"
/// reticulum_config = "~/.reticulum/config"
/// hub_destination = "abc123def456..."  # Reticulum destination hash
/// ```
use crate::{Error, wire::TileUpdate};
use crate::sync::{AckResult, SyncCursor, SyncTransport, TileDelta};

/// MANET transport via Reticulum mesh network.
///
/// Phase 5 implementation will use the `reticulum` crate or spawn
/// `rnsd` as a subprocess and communicate via its API.
pub struct ManetSyncTransport {
    /// Reticulum destination hash of the hub node
    pub hub_destination: String,
    /// Path to Reticulum config directory
    pub reticulum_config: String,
}

impl ManetSyncTransport {
    pub fn new(hub_destination: impl Into<String>, reticulum_config: impl Into<String>) -> Self {
        Self {
            hub_destination: hub_destination.into(),
            reticulum_config: reticulum_config.into(),
        }
    }
}

impl SyncTransport for ManetSyncTransport {
    fn push(&self, _batch: &TileUpdate) -> Result<AckResult, Error> {
        // Phase 5: serialize batch → LXMF message → Reticulum send
        // Chunk large batches to fit LXMF message limits
        // Track delivery receipts
        Err(Error::Other(
            "MANET transport not yet implemented — use HttpSyncTransport over VPN".to_string()
        ))
    }

    fn pull(&self, _cursor: &SyncCursor) -> Result<TileDelta, Error> {
        // Phase 5: subscribe to Reticulum announcements from hub
        // Receive delta packets, reassemble chunked messages
        Err(Error::Other(
            "MANET transport not yet implemented — use HttpSyncTransport over VPN".to_string()
        ))
    }
}

/// Selects the appropriate transport from config string.
/// Returns boxed transport ready to use in the collector.
///
/// Supported values: "http" | "manet" | "null"
/// Defaults to "http" for unknown values.
pub fn transport_from_config(
    transport_type: &str,
    hub_url: &str,
    device_id: &str,
) -> Box<dyn SyncTransport> {
    match transport_type {
        "null" => Box::new(crate::sync::NullSyncTransport),
        "manet" => {
            log::warn!("MANET transport selected but not yet implemented; falling back to HTTP");
            Box::new(crate::sync::HttpSyncTransport::new(hub_url, device_id))
        }
        _ => Box::new(crate::sync::HttpSyncTransport::new(hub_url, device_id)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manet_push_returns_not_implemented() {
        let t = ManetSyncTransport::new("abc123", "~/.reticulum");
        let batch = TileUpdate::new("test", "entity");
        assert!(t.push(&batch).is_err());
    }

    #[test]
    fn manet_pull_returns_not_implemented() {
        let t = ManetSyncTransport::new("abc123", "~/.reticulum");
        assert!(t.pull(&SyncCursor::default()).is_err());
    }

    #[test]
    fn transport_from_config_null() {
        let t = transport_from_config("null", "http://localhost:8789", "dev");
        // NullSyncTransport succeeds
        let batch = TileUpdate::new("dev", "entity");
        assert!(t.push(&batch).is_ok());
    }

    #[test]
    fn transport_from_config_default_is_http() {
        // Should not panic on construction
        let _t = transport_from_config("http", "http://localhost:8789", "dev");
        let _t2 = transport_from_config("unknown", "http://localhost:8789", "dev");
    }
}

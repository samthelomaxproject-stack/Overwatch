/// Sync transport — push tile batches to hub, pull deltas back.
///
/// The trait is transport-agnostic. Currently implemented:
/// - [`HttpSyncTransport`] — HTTP/JSON over LAN/VPN (MVP)
/// - [`NullSyncTransport`] — no-op, for hub-only mode with collection disabled
///
/// Phase 3 will add a MANET/Reticulum implementation behind the same trait.
use crate::{Error, wire::TileUpdate};
use serde::{Deserialize, Serialize};

// ── Wire types ────────────────────────────────────────────────────────────────

/// Acknowledgement returned by the hub after a successful push.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AckResult {
    pub accepted: u32,
    pub rejected: u32,
    pub cursor: u64,
}

/// Delta returned by hub — tiles updated since the node's last pull cursor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TileDelta {
    pub tiles: Vec<TileUpdate>,
    pub cursor: u64,
}

/// Sync cursor stored in node DB (sync_state key = "last_pull_cursor").
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SyncCursor {
    pub timestamp: u64,
}

// ── Trait ─────────────────────────────────────────────────────────────────────

pub trait SyncTransport: Send + Sync {
    fn push(&self, batch: &TileUpdate) -> Result<AckResult, Error>;
    fn pull(&self, cursor: &SyncCursor) -> Result<TileDelta, Error>;
}

// ── HTTP implementation ───────────────────────────────────────────────────────

/// HTTP sync transport — communicates with hub-api over LAN/VPN.
/// Uses blocking ureq (appropriate for background thread use).
pub struct HttpSyncTransport {
    pub base_url: String,
    pub device_id: String,
}

impl HttpSyncTransport {
    pub fn new(base_url: impl Into<String>, device_id: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            device_id: device_id.into(),
        }
    }

    fn push_url(&self) -> String {
        format!("{}/api/push", self.base_url)
    }

    fn pull_url(&self, cursor: u64) -> String {
        format!("{}/api/delta?device_id={}&cursor={}", self.base_url, self.device_id, cursor)
    }
}

impl SyncTransport for HttpSyncTransport {
    fn push(&self, batch: &TileUpdate) -> Result<AckResult, Error> {
        let resp = ureq::post(&self.push_url())
            .set("Content-Type", "application/json")
            .send_json(serde_json::to_value(batch)?)
            .map_err(|e| Error::Other(format!("HTTP push failed: {e}")))?;

        resp.into_json::<AckResult>()
            .map_err(|e| Error::Other(format!("Failed to parse AckResult: {e}")))
    }

    fn pull(&self, cursor: &SyncCursor) -> Result<TileDelta, Error> {
        let resp = ureq::get(&self.pull_url(cursor.timestamp))
            .call()
            .map_err(|e| Error::Other(format!("HTTP pull failed: {e}")))?;

        resp.into_json::<TileDelta>()
            .map_err(|e| Error::Other(format!("Failed to parse TileDelta: {e}")))
    }
}

// ── Null transport (hub with local collection disabled) ───────────────────────

/// No-op transport — used when hub has `HUB_COLLECTOR_ENABLED=false`.
/// The hub aggregates external data only; local node never syncs.
pub struct NullSyncTransport;

impl SyncTransport for NullSyncTransport {
    fn push(&self, _batch: &TileUpdate) -> Result<AckResult, Error> {
        Ok(AckResult { accepted: 0, rejected: 0, cursor: 0 })
    }

    fn pull(&self, _cursor: &SyncCursor) -> Result<TileDelta, Error> {
        Ok(TileDelta { tiles: vec![], cursor: 0 })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_transport_push_succeeds() {
        let t = NullSyncTransport;
        let batch = TileUpdate::new("test-device", "entity");
        let result = t.push(&batch).unwrap();
        assert_eq!(result.accepted, 0);
    }

    #[test]
    fn null_transport_pull_returns_empty() {
        let t = NullSyncTransport;
        let result = t.pull(&SyncCursor::default()).unwrap();
        assert!(result.tiles.is_empty());
    }

    #[test]
    fn http_transport_builds_correct_urls() {
        let t = HttpSyncTransport::new("http://10.0.0.1:8789/", "dev-abc");
        assert_eq!(t.push_url(), "http://10.0.0.1:8789/api/push");
        assert_eq!(t.pull_url(1000), "http://10.0.0.1:8789/api/delta?device_id=dev-abc&cursor=1000");
    }
}

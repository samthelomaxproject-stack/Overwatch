//! SIGINT — RF and Wi-Fi heatmap collection and aggregation for Overwatch.
//!
//! # Phase 1: Foundation
//! - [`wire`] — TileUpdate JSON wire format (serde structs)
//! - [`confidence`] — Pure confidence scoring (fully unit tested)
//! - [`gps`] — GPS provider trait + stub/macOS implementations  
//! - [`rf`] — hackrf_sweep parser, ring buffer, and RF tile aggregation
//! - [`wifi`] — Wi-Fi scanner trait, macOS airport impl, privacy filter, tile aggregation
//! - [`storage`] — SQLite schema and CRUD for node-side persistence
//!
//! # Phase 2 (upcoming)
//! - H3 cell lookup (h3o crate)
//! - hub-api HTTP server
//! - Sync protocol (push/pull with cursors)
//! - Tauri integration
//!
//! # Architecture
//! ```text
//! HackRF USB → hackrf_sweep → RingBuffer → flush (5s) → RfObservation
//!                                                         ↓
//! GPS fix → tile_id (H3 res10) ──────────────────→ RfTileBucket.upsert()
//!                                                         ↓
//! airport/iw → WifiNetwork → Mode A filter → ChannelObservation
//!                                                         ↓
//!                                              WifiTileBucket.upsert()
//!                                                         ↓
//!                                              NodeDb (SQLite)
//!                                                         ↓
//!                                              TileUpdate (wire format)
//!                                                         ↓
//!                                              SyncTransport → hub-api
//! ```

pub mod collector;
pub mod confidence;
pub mod crypto;
pub mod error;
pub mod gps;
pub mod hub;
pub mod manet;
pub mod rf;
pub mod sanitize;
pub mod storage;
pub mod sync;
pub mod wifi;
pub mod wire;

pub use error::Error;

# Overwatch — Tactical Operations Center

**Offline-first tactical communications and situational awareness for mesh networks.**

Inspired by XTOC™, Anduril Lattice, and built for emergency response, field operations, and off-grid communications.

---

## Status

**Version:** 0.2.2  
**Status:** Production-ready desktop app with live ADS-B, Meshtastic, and SIGINT RF/Wi-Fi heatmap foundation

### What's New in v0.2.2 (2026-02-24–25)
- ✅ **ADS-B live tracking** — HackRF/dump1090 integration with real-time aircraft on 2D and 3D maps
- ✅ **ADS-B on Cesium 3D** — Aircraft shown at real altitude with vertical drop-lines and color-by-altitude
- ✅ **Stale aircraft pruning** — Aircraft disappear after 60s of no transmissions on both 2D and 3D maps
- ✅ **SIGINT foundation crate** — New `sigint/` Rust crate: RF heatmap collection and aggregation (see below)
- ✅ **Dynamic Entities** — `ingestPLI()` hook ready for EUD mesh PLI data
- ✅ **Squad management** — Full add/edit/delete with modal, empty state
- ✅ **Map layer toggles** — UNITS, ADS-B, RF, Wi-Fi buttons actually toggle marker/hex visibility
- ✅ **RF + Wi-Fi heatmap panels** — New SDR/SIGINT sidebar panels with tile count, peak power, detail view
- ✅ **H3 hex overlay** — h3-js renders live hexagons on the tactical map, click for tile detail

### What's New in v0.2.1
- ✅ **Meshtastic CLI integration** — Connect to Meshtastic mesh networks via official CLI
- ✅ Node discovery — Display all nodes in your mesh with names and positions
- ✅ Message sending — Send text messages to the mesh
- ✅ ATAK mode — Send/receive CoT (Cursor on Target) XML packets for ATAK compatibility
- ✅ Dual baud rate support — Automatically tries 921600 and 115200 baud rates

### What's New in v0.2.0
- ✅ Complete UI redesign — Anduril Lattice-inspired glassmorphism
- ✅ Military symbol system — MIL-STD-2525 inspired entity tracking
- ✅ Tactical color palette — Cyan/orange accents with dark grid background
- ✅ Keyboard shortcuts — Rapid navigation (Cmd+1 through Cmd+9)
- ✅ New Entities view — Track friendly, hostile, neutral, unknown units
- ✅ Priority indicators — Visual flash alerts for FLASH priority traffic
- ✅ Native GPS integration — macOS CoreLocation with real-time position updates
- ✅ MGRS grid coordinates — Military grid reference system display
- ✅ Pulsing location marker — Animated position indicator on tactical map
- ✅ 3D Cesium view — Interactive 3D globe with terrain and satellite imagery
- ✅ GPS status sync — Permission state properly reflected in UI

---

## Overview

Overwatch is a local-first, offline-ready Tactical Operations Center (TOC) application. It enables structured communications (SITREPs, TASKs, CHECKINs) over any transport — mesh networks, radio, QR, or copy/paste. Designed for seamless integration with Anduril Lattice and TAK/ATAK environments.

## Core Features

### Communication
- **Packet-based reports**: SITREP, TASK, CHECKIN/LOC, CONTACT, RESOURCE, ASSET, ZONE, MISSION, EVENT
- **Priority levels**: ROUTINE, PRIORITY, URGENT, FLASH (with visual indicators)
- **Transport agnostic**: Works over any text transport (planned)
- **Packet chunking**: Split long messages for tight character limits (planned)
- **QR workflows**: Scan/share packets between devices (planned)

### Mapping & Situational Awareness
- **Tactical map**: Leaflet-based with dark/satellite/terrain layers
- **3D Globe View**: CesiumJS-powered interactive 3D view with terrain
- **Native GPS tracking**: macOS CoreLocation integration with real-time position updates
- **MGRS grid coordinates**: Military Grid Reference System display
- **Pulsing position marker**: Animated cyan dot with accuracy radius and popup info
- **Callsign display**: Your callsign shown on map marker and status bar
- **Layer controls**: Toggle UNITS, ZONES, ADS-B, AIS overlays
- **Zones**: Tactical zone editor with friendly/hostile/neutral/unknown classifications
- **SATCOM**: Satellite tracking panel with TLE support (UI ready)
- **SDR Panel**: ADS-B, AIS, SATCOM receiver interface (UI ready)

### Organization
- **Squads**: Persistent groups with roster management
- **Entities**: Track units with military affiliation symbols (◈ friendly, ◉ hostile, ◐ neutral, ◆ unknown)
- **Callsign support**: Customizable with real-time status bar updates
- **Multi-unit tagging**: Multiple sources per packet (planned)
- **Local-first**: No accounts, no central server, no cloud dependency

### Tactical UI
- **Glassmorphism design**: Semi-transparent panels with backdrop blur
- **Tactical grid background**: Subtle coordinate grid pattern
- **Monospace typography**: For callsigns, coordinates, and tactical data
- **Color-coded priorities**: Green (routine) → Yellow (priority) → Orange (urgent) → Red (flash)
- **Military symbols**: MIL-STD-2525 inspired affiliation markers
- **Keyboard shortcuts**:
  - `Cmd+1` — Packets
  - `Cmd+2` — Tactical Map
  - `Cmd+3` — SDR/SIGINT
  - `Cmd+4` — Squads
  - `Cmd+5` — Entities
  - `Cmd+6` — SATCOM
  - `Cmd+7` — Zones
  - `Cmd+8` — ATAK/CoT
  - `Cmd+9` — Settings

---

## Screenshots

*Tactical UI with glassmorphism panels and military symbols*

---

## Build Instructions

### Prerequisites
- Rust 1.77+ (`cargo`, `rustc`)
- macOS 12+ (for desktop app)

### Desktop App (Tauri)

```bash
# Clone the repo
git clone https://github.com/samthelomaxproject-stack/Overwatch.git
cd Overwatch

# Build full macOS app bundle
cd src-tauri
cargo tauri build --bundles app

# The app will be at:
# target/release/bundle/macos/Overwatch.app
```

### Run the App

```bash
# Open the macOS app
open ./src-tauri/target/release/bundle/macos/Overwatch.app

# Or run binary directly
./src-tauri/target/release/overwatch
```

### Development Mode

```bash
cd src-tauri
cargo tauri dev
```

---

## Transport Comparison

| Transport | Max Message | Best For | Status |
|-----------|-------------|----------|--------|
| Meshtastic | ~180 chars | Quick updates | ✅ Working (CLI-based) |
| MeshCore | ~160 bytes | Tight limits | Planned |
| Reticulum | Flexible | MeshChat | Planned |
| LAN/MANET | Unlimited | Local networks | Planned |
| QR | Depends on screen | Offline handoff | Planned |
| Voice | N/A | Manual relay | Planned |

---

## CLEAR vs SECURE

- **CLEAR**: Default, openly decodable (ham-friendly)
- **SECURE**: Encrypted payload, shared team key (non-ham only)

> ⚠️ Do NOT transmit encrypted content on amateur radio. Follow local regulations.

---

## Architecture

```
Overwatch/
├── webui/                  # Tactical frontend
│   ├── index.html          # Single-file app
│   └── tac-ui.css          # Design system
├── src-tauri/              # Desktop wrapper (Rust)
│   ├── src/                # Backend code
│   ├── icons/              # App icons
│   └── tauri.conf.json     # App config
├── reticulum-bridge/       # Reticulum transport (planned)
├── meshtastic-bridge/      # Meshtastic transport (planned)
├── atak-bridge/            # ATAK CoT integration (planned)
├── satcom/                 # Satellite tracking (planned)
└── docs/                   # Documentation
```

---

## Development Roadmap

### Phase 1: Core UI (v0.1-0.2) ✅
- [x] Basic HTML/CSS/JS frontend
- [x] Tauri desktop wrapper
- [x] Sidebar navigation with keyboard shortcuts
- [x] Packet creation and display
- [x] Tactical map with Leaflet
- [x] Squad management with add/edit/delete
- [x] GPS tracking with MGRS
- [x] **v0.2.0: Tactical UI overhaul — glassmorphism, military symbols, priority indicators**

### Phase 2: Data Layer (v0.3)
- [ ] IndexedDB local storage
- [ ] Packet persistence and history
- [ ] Export/import functionality (JSON/KML)
- [ ] QR code generation and scanning

### Phase 3: Mesh Integration (v0.4)
- [x] Reticulum bridge (LXMF transport)
- [x] Meshtastic CLI integration — **v0.2.1 COMPLETE**
- [ ] Meshtastic Web Bluetooth integration
- [ ] Packet chunking/dechunking
- [ ] Transport abstraction layer

### Phase 4: Advanced Features (v0.5)
- [x] **ADS-B live tracking** — HackRF/dump1090, TCP bridge, real-time map, 60s stale pruning — **v0.2.2**
- [x] **ADS-B on 3D Cesium** — aircraft at real altitude, vertical drop-lines, color-by-altitude — **v0.2.2**
- [ ] AIS vessel tracking (hardware ready, integration planned)
- [ ] SATCOM tracking with TLE propagation (ISS panel scaffolded)
- [ ] ATAK KML/CoT import/export
- [ ] Zone editor with polygon drawing
- [ ] SECURE mode encryption (AES-256)

### Phase 5: SIGINT RF + Wi-Fi Heatmap ✅ (Foundation Complete)

**`sigint/` Rust crate — 78 tests, 0 failures**

#### Phase 5.1 — Foundation (complete)
- [x] `wire.rs` — TileUpdate JSON wire format (schema v1)
- [x] `confidence.rs` — GPS × sample × dwell × speed scoring formula, 11 tests
- [x] `gps.rs` — GpsProvider trait, real h3o cell lookup at H3 resolution 10
- [x] `rf.rs` — hackrf_sweep CSV parser, ring buffer, Welford online mean, tile bucket
- [x] `wifi.rs` — WifiScanner trait, macOS airport impl, Privacy Mode A filter, tile bucket
- [x] `storage.rs` — Node SQLite DB (WAL), upsert with ON CONFLICT merge, sync cursor

#### Phase 5.2 — Hub + Sync (complete)
- [x] `sync.rs` — SyncTransport trait; HttpSyncTransport (VPN/LAN), NullSyncTransport
- [x] `hub.rs` — HubDb (merged_tiles, node_registry, delta_cursors), hub-api HTTP server
  - Routes: `GET /health`, `POST /api/push`, `GET /api/delta`
  - Confidence-weighted mean merge with ON CONFLICT upsert
  - Cursor-based delta queries
- [x] `collector.rs` — Full collection loop: GPS → RF flush (5s) → Wi-Fi scan (30s) → sync push/pull (30s)

#### Phase 5.3 — Visualization (complete)
- [x] h3-js hex overlay on Leaflet tactical map
- [x] RF color ramp: blue (weak, −100 dBm) → red (strong, −40 dBm), opacity by confidence
- [x] Wi-Fi color ramp: blue (sparse) → orange (dense), opacity by confidence
- [x] Hover tooltip: mean/max/samples/confidence
- [x] Click-to-detail: hex click switches to SDR view, populates detail panel
- [x] RF Environment + Wi-Fi Density panels in SDR/SIGINT sidebar
- [x] `get_sigint_delta` Tauri command — polls hub-api every 5s, graceful fallback

#### Phase 5.4 — Hardening (complete)
- [x] `crypto.rs` — Ed25519 keypair generation, `device_id` derived from public key
  - `sign_payload()` / `verify_payload()` — canonical JSON → sign → base64
- [x] `sanitize.rs` — Anti-poisoning pipeline
  - RF: freq bounds (10 MHz–6 GHz), power clamp (−120–0 dBm), mean>max rejection
  - Wi-Fi: RSSI clamp (−100–−10 dBm), band validation, mean>max rejection
  - GPS: lat/lon bounds, accuracy >500m rejected, speed >300 m/s rejected
  - `RateLimiter`: 200 RF / 20 Wi-Fi contributions per node per tile per bucket
  - `decay_factor()`: exponential decay, RF 5min half-life, Wi-Fi 2min half-life
  - Time decay applied in `get_delta()` — confidence fades with age for opacity rendering
- [x] `manet.rs` — MANET/Reticulum transport stub (drop-in for Phase 6)
  - `transport_from_config("http"|"manet"|"null")` factory

#### Phase 5.5 — Remaining
- [ ] Wire Ed25519 signing into sync push path (keys generated, signing functions ready)
- [ ] Hub signature verification on merge (skeleton in place)
- [ ] hackrf_sweep spawner thread (parser complete, spawner integration pending)
- [ ] Android EUD collector client (same sigint crate, platform Wi-Fi scanner impl)

### Phase 6: MANET + Android (v0.7)
- [ ] Reticulum LXMF transport implementation (ManetSyncTransport stub ready)
- [ ] Android version — PLI + SIGINT collector sharing over mesh
- [ ] Key rotation (device_id stable, rotation hook documented)
- [ ] Full MIL-STD-2525 symbol support

### Phase 7: Lattice/ATAK Integration (v0.8)
- [ ] Anduril Lattice entity data model
- [x] CoT (Cursor on Target) message parsing/generation — **v0.2.1**
- [x] ATAK mode for Meshtastic — **v0.2.1**
- [ ] TAK server connection (TCP/UDP)
- [ ] Real-time entity synchronization

### Phase 8: AI + Security (v0.9)
- [ ] Gjallarhorn security integration
- [ ] AI-powered threat detection
- [ ] Anomaly detection in packet patterns

---

## Integration Targets

### Anduril Lattice
- Entity data model compatibility
- Objects API integration
- Real-time entity streaming
- Tactical data link support

### TAK/ATAK
- CoT (Cursor on Target) format support
- KML/KMZ import/export
- TAK server connectivity
- Full MIL-STD-2525 symbology

### Reticulum
```bash
pip install rns lxmf
# Native mesh networking with store-and-forward
```

### Meshtastic (CLI Integration)
```bash
# Install Meshtastic CLI
python3 -m pip install --break-system-packages meshtastic

# Verify installation
meshtastic --version

# Connect in Overwatch
# 1. Click "CONNECT CLI" in the Meshtastic panel
# 2. View discovered nodes
# 3. Send messages to the mesh
# 4. Enable ATAK mode for CoT XML packets
```

**Note:** Close the official Meshtastic.app GUI before connecting to avoid port conflicts.

---

## Getting Started

### Quick Start
1. Download the latest release from GitHub
2. Launch Overwatch.app (macOS)
3. Set your callsign in Settings (Cmd+9)
4. Allow location access for GPS tracking
5. Create your first packet (Cmd+1) or explore the map (Cmd+2)

### Web Browser (Development)
```bash
cd webui
python3 -m http.server 8080
# Open http://localhost:8080
```

---

## Comparison: Overwatch vs XTOC vs Anduril

| Feature | Overwatch | XTOC | Anduril Lattice |
|---------|-----------|------|-----------------|
| Open Source | ✅ | ❌ | ❌ |
| AI Agents | ✅ (our bots) | ❌ | ✅ |
| Lattice Integration | Planned | ❌ | N/A |
| TAK/ATAK Compatible | Planned | ❌ | ✅ |
| Self-hosted | ✅ | ✅ | ❌ |
| Desktop App | ✅ | ✅ | ❌ |
| GPS/MGRS | ✅ | ? | ✅ |
| Military Symbols | ✅ | ? | ✅ |
| Mesh Networking | Planned | ❌ | Via integration |

---

## License

MIT

---

## Related Projects

- [Gjallarhorn](../Gjallarhorn) — Mobile SOC Toolkit (future security integration)
- [Reticulum](https://github.com/markqvist/Reticulum) — Mesh networking stack
- [NomadNet](https://github.com/markqvist/NomadNet) — Mesh messaging
- [Meshtastic](https://meshtastic.org/) — LoRa mesh communication
- [Anduril Lattice](https://www.anduril.com/lattice/) — AI-powered defense OS
- [ATAK](https://tak.gov/) — Android Team Awareness Kit

---

---

## Troubleshooting & Post-Mortems

### ADS-B TCP Bridge — Silent Deadlock (Fixed 2026-02-24)

#### Symptom
Clicking **START** in the ADS-B panel appeared to work:
- Rust command returned `"RTL-SDR streaming started"` ✅
- `dump1090` started and connected to the RTL-SDR dongle ✅
- Python bridge (`rtl_sdr_socket.py`) launched and connected to `dump1090:30003` ✅
- Rust made a TCP connection to `127.0.0.1:30004` ✅
- Aircraft count stayed at **0** — no data ever appeared in the UI ❌

#### Root Cause
A **threading deadlock** in `rtl_sdr_socket.py`.

`flush_to_clients()` held `self.lock` while iterating the aircraft database, then called `self.broadcast()` — which also tried to acquire the **same** `self.lock`. Python's `threading.Lock` is not reentrant: the second acquisition blocks forever.

```python
# BROKEN — deadlock on first broadcast tick (500ms after start)
def flush_to_clients(self):
    while True:
        time.sleep(0.5)
        with self.lock:                  # ← acquires lock
            ...
            for icao, data in self.aircraft_db.items():
                self.broadcast(msg)      # ← tries to re-acquire self.lock → DEADLOCK

def broadcast(self, message):
    with self.lock:                      # ← blocks forever
        ...
```

The broadcast thread silently hung on the very first iteration. No exception, no log entry — just silence. `dump1090` was streaming real aircraft data, the Python parser was populating `aircraft_db` correctly, Rust was connected and waiting — but zero bytes ever left the bridge.

#### Diagnosis Steps
1. Manually connected a raw Python socket to port `30004` → confirmed zero bytes received
2. Confirmed `dump1090:30003` was actively streaming (16+ aircraft)
3. Ran the parsing logic in isolation → `aircraft_db` populated correctly with full aircraft data
4. Identified that `flush_to_clients` held the lock while calling `broadcast` → classic non-reentrant deadlock

#### Fix
Build a snapshot of the aircraft data **under** the lock, release the lock, then broadcast **outside** the lock:

```python
# FIXED — build snapshot under lock, broadcast outside lock
def flush_to_clients(self):
    while True:
        time.sleep(0.5)
        with self.lock:
            # Remove stale, build message list
            messages = [json.dumps({"aircraft": {...}}) for data in self.aircraft_db.values()]
        # Lock released — now safe to call broadcast
        for msg in messages:
            self.broadcast(msg)

def broadcast(self, message):
    with self.lock:
        clients_snapshot = list(self.clients)
    # Send outside lock
    for client in clients_snapshot:
        client.send((message + "\n").encode('utf-8'))
```

#### Verification
After fix: connected raw socket to port `30004` → immediately received live aircraft JSON (UAL584, EJA744, SKW5336, etc. with lat/lon/altitude/speed). 16 aircraft in DB, data streaming at 2Hz.

### ADS-B — Tauri Event Delivery Failure + Unbound Listener (Fixed 2026-02-24)

#### Symptom
Even after the Python bridge deadlock was resolved and aircraft data was confirmed flowing over TCP, the ADS-B panel still showed **DISCONNECTED** and zero aircraft. No `rtl-sdr-status` or `rtl-sdr-aircraft` messages appeared in the debug log — despite `location-update` events from the same app working fine.

#### Root Cause 1 — Unreliable `emit()` from Background Threads

Tauri v2's `AppHandle::emit()` is fire-and-forget. When called from a `std::thread::spawn` context, events are not guaranteed to be delivered if the WebView's event loop is busy or the listener hasn't fully registered. The location events worked because they were emitted from the `setup()` closure thread, which uses a different internal dispatch path.

The RTL-SDR status events were emitted at specific moments (after 3s sleep, after 4s sleep) — if the WebView wasn't ready at that exact instant, the event was silently dropped.

#### Root Cause 2 — Unbound `listen` Function Reference

```javascript
// BROKEN — listen() loses its 'this' binding when called as a bare function
function getTauriListener() {
    return window.__TAURI__?.event?.listen;  // detached reference
}
const tauriListen = getTauriListener();
tauriListen('rtl-sdr-status', handler);     // 'this' is undefined → silent failure
```

```javascript
// FIXED — bind preserves the correct 'this' context
function getTauriListener() {
    const listen = window.__TAURI__?.event?.listen;
    if (listen) return listen.bind(window.__TAURI__.event);
    return null;
}
```

#### Fix — Invoke-Based Polling

Replaced one-shot `emit()` calls with a shared Rust static (`RTL_SDR_STATUS`, `RTL_SDR_AIRCRAFT`) that the background thread writes to on every state change. Added a `get_rtl_sdr_status` Tauri command that JS calls every second via `invoke()`.

`invoke()` is synchronous and request/response — guaranteed delivery regardless of timing or WebView state.

```rust
// Rust: write status to shared static on every transition
static RTL_SDR_STATUS: Mutex<String> = Mutex::new(String::new());
static RTL_SDR_AIRCRAFT: Mutex<Vec<serde_json::Value>> = Mutex::new(Vec::new());

#[tauri::command]
fn get_rtl_sdr_status() -> serde_json::Value {
    serde_json::json!({
        "status": RTL_SDR_STATUS.lock().map(|s| s.clone()).unwrap_or_default(),
        "aircraft": RTL_SDR_AIRCRAFT.lock().map(|db| db.clone()).unwrap_or_default(),
    })
}
```

```javascript
// JS: poll every 1 second via invoke instead of waiting for events
setInterval(async () => {
    const { status, aircraft } = await window.__TAURI__.core.invoke('get_rtl_sdr_status');
    // update UI from polled state
}, 1000);
```

#### Status Flow (After Fix)
```
START clicked → STARTING (yellow, 3s) → BRIDGE_UP → CONNECTING (orange, 1s) → CONNECTED (cyan) → aircraft appear
```

---

## Changelog

### v0.2.2 (2026-02-24–25) — ADS-B + SIGINT Foundation

#### ADS-B
- ✅ **Fixed ADS-B dead silence bug** — threading deadlock in Python TCP bridge (`flush_to_clients` held lock while calling `broadcast`)
- ✅ **Fixed Tauri event delivery** — background thread `emit()` unreliable; replaced with `get_rtl_sdr_status` invoke polling
- ✅ **Fixed unbound event listener** — `window.__TAURI__.event.listen` stored without `.bind()` — silent registration failure
- ✅ **Live aircraft on 2D and 3D maps** — status STARTING → CONNECTING → CONNECTED, color-by-altitude, vertical drop-lines on Cesium
- ✅ **60s stale pruning** — aircraft removed from both maps and aircraft count synchronized to on-screen truth
- ✅ **ADS-B layer toggle** — RF/Wi-Fi/ADS-B/Units buttons actually show/hide markers

#### UI Improvements
- ✅ **Dynamic entities** — `ingestPLI()` ready for EUD mesh PLI data; empty state with mesh hint
- ✅ **Squad CRUD** — add, edit (pre-filled modal), delete with confirm
- ✅ **SATCOM** — ISS only (NOAA 19 decommissioned, AO-91 inactive removed)
- ✅ **SDR status persistence** — status text survives `renderSDR()` re-renders

#### SIGINT RF + Wi-Fi Heatmap (`sigint/` crate)

**Phase 5.1 — Foundation** (78 tests, 0 failures across all phases)
- New Rust crate `sigint/` — portable, no Tauri dependency, Linux/Android-ready
- `wire.rs`: TileUpdate schema v1 wire format (serde, versioned)
- `confidence.rs`: GPS × sample × dwell × speed scoring, 11 boundary tests
- `gps.rs`: GpsProvider trait + real h3o H3 cell lookup at resolution 10
- `rf.rs`: hackrf_sweep CSV parser, RingBuffer, Welford online mean, RfTileBucket
- `wifi.rs`: WifiScanner trait, macOS `airport` impl, Privacy Mode A, WifiTileBucket
- `storage.rs`: NodeDb SQLite (WAL), ON CONFLICT weighted-mean merge, sync cursors

**Phase 5.2 — Hub + Sync**
- `sync.rs`: SyncTransport trait — HttpSyncTransport (VPN/LAN), NullSyncTransport
- `hub.rs`: HubDb (merged_tiles, node_registry, delta_cursors), minimal HTTP server
  - `POST /api/push` → sanitize → rate-limit → merge
  - `GET /api/delta?cursor=` → time-decayed confidence → TileDelta
- `collector.rs`: full collection loop — RF flush every 5s, Wi-Fi scan every 30s, sync push/pull every 30s

**Phase 5.3 — Visualization**
- h3-js@4.1.0 hex overlay on Leaflet via `h3.cellToBoundary()`
- RF: blue (−100 dBm) → red (−40 dBm), opacity by confidence
- Wi-Fi: blue → orange density, opacity by confidence
- Hover tooltip: mean/max/samples/confidence/band
- Click hex → SDR view detail panel with cell id, dimension, stats, age
- `get_sigint_delta` Tauri command polls hub every 5s, graceful fallback if hub not running

**Phase 5.4 — Hardening**
- `crypto.rs`: Ed25519 keypair, device_id from public key, sign/verify pipeline, 5 tests
- `sanitize.rs`: anti-poisoning (RF freq/power bounds, Wi-Fi RSSI/band, GPS validity), per-node rate limiter (200 RF / 20 Wi-Fi per tile per bucket), exponential time decay (RF 5min, Wi-Fi 2min half-life), 20 tests
- `manet.rs`: Reticulum MANET transport stub with `transport_from_config()` factory

### v0.2.0 (2026-02-23) — Tactical UI Overhaul + Native GPS + 3D View
- Complete visual redesign with Anduril Lattice inspiration
- Glassmorphism panels with backdrop blur effects
- Tactical grid background pattern
- Military symbol system (MIL-STD-2525 inspired)
- New color palette: cyan/orange accents on deep slate
- Keyboard shortcuts for all views (Cmd+1 through Cmd+9)
- New "Entities" view for tracking units by affiliation
- Priority indicators with flash animation for FLASH level
- Layer toggle controls on map (UNITS, ZONES, ADS-B, AIS)
- Monospace typography for tactical data
- **Native GPS integration**: macOS CoreLocation with Rust/Objective-C bridge
- **Real-time position updates**: Polling-based location with event emission
- **MGRS grid display**: Military coordinates in status bar and map popup
- **Pulsing location marker**: Animated cyan dot on tactical map
- **3D Cesium view**: Interactive 3D globe with terrain and satellite imagery
- Fixed GPS permission sync between Rust backend and JavaScript UI
- Fixed Tauri API injection with `withGlobalTauri: true`
- Fixed resource bundling in Tauri app
- Fixed map initialization timing for GPS location display
- Fixed 3D view overlay positioning to expose Cesium controls

### v0.1.2 (2026-02-21)
- Fixed GPS implementation breaking sidebar navigation
- Re-implemented GPS with MGRS grid coordinates
- Built native macOS app bundle
- Working desktop application

### v0.1.1
- Added GPS tracking (buggy, reverted)

### v0.1.0
- Initial UI implementation
- Sidebar navigation
- Packet management
- Tactical map
- Squad management
- Settings panel

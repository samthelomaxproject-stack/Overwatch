# Overwatch — Tactical Operations Center

**Offline-first tactical communications and situational awareness for mesh networks.**

Built for emergency response, field operations, and off-grid communications.

---

## Status

**Version:** 0.2.3  
**Status:** Production-ready desktop app + Android EUD DAT camera streaming updates (Meta glasses local stream path stabilized)

### What's New in v0.2.3 (2026-03-11)
- ✅ **Android DAT package auth fixed** — GitHub Packages token wiring validated in CI (`META_PACKAGES_TOKEN`)
- ✅ **Android DAT compatibility updates** — `minSdk` raised to 29, Kotlin plugin upgraded to 2.1.0 for DAT 0.4.0 metadata compatibility
- ✅ **DAT API surface fixes** — registration/permission status handling updated for current SDK sealed types
- ✅ **Meta stream reliability fix** — force fresh DAT session when stream reports active but no frames (`Frame:NO`)
- ✅ **Meta reconnect hardening** — added explicit **Reconnect Glasses** control + auto-reconnect attempt when Watch Live sees repeated frame misses
- ✅ **Live feed UX update** — feed window now auto-sizes to incoming video, includes **Full Screen** toggle, plus **Stop Feed** action for camera/glasses sessions
- ✅ **Hub-aligned feed rendering on Android** — direct video/HLS URLs now use `<video>` playback; page/EarthCam links stay in iframe with external fallback behavior

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

Overwatch is a local-first, offline-ready Tactical Operations Center (TOC) application. It enables structured communications (SITREPs, TASKs, CHECKINs) over any transport — mesh networks, radio, QR, or copy/paste. Designed for seamless integration with TAK/ATAK environments.

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

## Android Installer (MVP)

An Android scaffold now exists in `android/`.

### Build & Download APK from GitHub

1. Open GitHub → **Actions**
2. Run workflow: **Android APK** (or push changes under `android/`)
3. Download artifact: **`overwatch-eud-debug-apk`**
4. Install `app-debug.apk` on Android (allow unknown sources)

Current Android MVP includes:
- Hub URL + callsign configuration
- Wi-Fi privacy mode selector (A/B/C)
- Foreground collector service (location + Wi-Fi scan push loop)
- Tactical Map button opens in-app map view (WebView to configured hub)
- Live debug/status log relay from collector service

> Note: Android is currently an MVP field client focused on collection + map access. Native tactical Android UI layers are still in progress.

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

#### Phase 5.5 — Complete ✅
- [x] **hackrf_sweep spawner** — `Sweeper::run()` spawns per-band, streams CSV into ring buffer. Binary auto-detected at `/opt/homebrew/bin/hackrf_sweep` (installed)
- [x] **Ed25519 signing on push** — `sync_push()` signs every batch; signature embedded in `TileUpdate.signature`
- [x] **Hub signature verification** — `merge_update()` verifies against stored node public key; rejects invalid; first-contact grace for new nodes
- [x] **Hub auto-start** — `start_hub()` Tauri command; binds `0.0.0.0:8789`; auto-started on app launch
- [x] **Node collector auto-start** — `start_collector()` Tauri command; GPS + CoreWLAN + sync loop in background thread
- [x] **RF sweep toggle** — `start_sweeper()` Tauri command; ⚡ START RF button in Hub panel
- [x] **PLI from Meshtastic → Entities** — `meshtastic-position` event feeds `ingestPLI()` directly
- [x] **CoreWLAN scanner** — `airport` CLI removed in macOS 15; replaced with `scan_wifi.swift` (CWWiFiClient)
- [x] **Live GPS → collector** — `SHARED_GPS_FIX` static bridges Tauri CoreLocation thread to collector
- [x] **Leaflet heatmap** — `leaflet-heat` replaces hex polygons; RF blue→red, Wi-Fi blue→orange; semi-transparent with map visible
- [x] **Privacy mode selector** — Mode A/B/C in Settings; live switch without restart via `SHARED_PRIVACY_MODE` static
  - Mode A: channel + RSSI only (default, no network identity)
  - Mode B: salted FNV-1a hashed BSSID/SSID
  - Mode C: raw SSID + BSSID (explicit opt-in)
- [x] **Live Wi-Fi panel** — `get_wifi_scan_results` Tauri command; bypasses hub aggregation; shows real SSIDs in Mode C, hashes in B, channels in A
- [x] **Location following** — GPS cell change resets hub poll cursor; new area tiles appear automatically
- [x] **Full-width scrollable panels** — RF Environment + Wi-Fi Density span full grid, 280px scrollable with sticky headers
- [x] **Android EUD** — same sigint crate, platform scanner impl (pending hardware)

### Phase 6: MANET + Android (v0.7)
- [ ] Reticulum LXMF transport implementation (ManetSyncTransport stub ready)
- [x] Android MVP scaffold committed (`android/`)
  - Kotlin app shell (hub URL + privacy mode config)
  - Collector/map controls scaffolded in UI
  - GitHub Actions APK workflow (`.github/workflows/android-apk.yml`)
  - Downloadable debug APK artifact on each Android-related push/workflow dispatch
- [ ] Android version — PLI + SIGINT collector sharing over mesh (full implementation)
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

### Android DAT / Meta Glasses Build + Stream Issues (Fixed 2026-03-11)

#### 1) CI failed to resolve DAT artifacts (401 Unauthorized)
**Symptom:**
`Could not GET https://maven.pkg.github.com/facebook/meta-wearables-dat-android/... 401 Unauthorized`

**Root cause:** Missing/invalid GitHub Packages token in repo secrets.

**Fix:** Added `META_PACKAGES_TOKEN` secret and wired it into Android workflow + Gradle credentials path.

#### 2) Manifest merge failed (`minSdkVersion 26` vs DAT camera min 29)
**Symptom:**
`uses-sdk:minSdkVersion 26 cannot be smaller than version 29 declared in library com.meta.wearable:mwdat-camera:0.4.0`

**Fix:** Raised Android app `minSdk` to **29**.

#### 3) Kotlin metadata mismatch with DAT 0.4.0
**Symptom:**
`Module was compiled with an incompatible version of Kotlin. metadata is 2.1.0, expected 1.9.0`

**Fix:** Upgraded Kotlin Android plugin to **2.1.0** in `android/build.gradle.kts`.

#### 4) DAT API surface changes broke compile
**Symptoms:**
- `Unresolved reference 'name'` on `PermissionStatus`
- `Unresolved reference 'Unregistered'` on `RegistrationState`

**Fix:** Updated status mapping logic to current sealed-type surface and removed obsolete branch assumptions.

#### 5) UI showed STREAMING but feed stayed black (`Frame:NO`)
**Symptom:**
- Sidebar: `Glasses: REGISTERED • Cam:GRANTED • STREAMING • Frame:NO`
- Watch Live modal opened but remained black.

**Root causes:**
- Session could remain logically active without delivering frames.
- Entity/feed mapping and stream context drift could prevent frame replay.

**Fixes:**
- Added frame-readiness telemetry (`Frame:YES/NO`) in UI status.
- Allowed latest-frame fallback when bound UID drifts.
- Forced a **fresh DAT session** when stream exists but no frame has arrived.
- Added **Stop Feed** action and resizable live-feed window for quicker recovery/testing.

#### 6) Could not reconnect after app restart or glasses power-cycle
**Symptom:**
- After app reopen (or glasses off/on), previous link appeared present but no new video arrived.

**Root causes:**
- Existing session could be retained despite stale frame timestamp.
- Reconnect path was hub-only; no explicit local glasses reconnect flow.

**Fixes:**
- Added stale-frame detection (`frame_age_ms`) and only trust active sessions with fresh frames.
- Added **Reconnect Glasses** UI action to force stop/start local DAT stream for selected entity.
- Added automatic one-time reconnect attempt during Watch Live when repeated frame polls are empty.

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

### SIGINT Troubleshooting Log — 2026-02-25

#### 1) No hex/heatmap visible despite hub running
**Symptom:** No RF/Wi-Fi colors appeared on map, but hub health endpoint responded.

**Root causes:**
- Hub had little/no local data because collector and sweeper startup path was incomplete.
- Early test tiles were inserted in the wrong geography (far from local map area).
- Heatmap intensity used `intensity *= confidence`; decayed confidence reached 0.0 and made layers effectively invisible.

**Fixes:**
- Added/verified Tauri commands for `start_hub`, `start_collector`, `start_sweeper` and auto-start flow in UI.
- Seeded/validated local-area test tiles for debugging, then removed test-only dependency.
- Reworked heatmap intensity model:
  - signal strength drives intensity
  - confidence affects opacity floor (min 0.3), not hard-zero multiplier
  - added semi-transparent gradients so basemap remains visible.

#### 2) Wi-Fi scanner broke on current macOS
**Symptom:** `airport -s` path unavailable, scanner returned no data.

**Root cause:** Apple removed/deprecated the old `airport` CLI path on current macOS.

**Fix:** Replaced scanner backend with CoreWLAN via bundled `scan_wifi.swift` helper (`CWWiFiClient.scanForNetworks`).

#### 3) Collector had no live GPS fix
**Symptom:** collector skipped useful aggregation because provider returned no fix.

**Root cause:** `MacosGpsProvider` lacked a real bridge from Tauri location thread.

**Fix:** Added `SHARED_GPS_FIX` in `sigint::gps` and updated Tauri `update_global_location()` to publish fixes into shared state consumed by collector.

#### 4) Privacy mode looked "stuck" on Mode A
**Symptom:** switching to Mode B/C in Settings did not change displayed Wi-Fi detail.

**Root causes:**
- Collector read privacy mode only at startup (static config snapshot).
- Hub aggregate schema (`ChannelHotness`) is intentionally channel-centric and does not carry SSID/BSSID detail.

**Fixes:**
- Added `SHARED_PRIVACY_MODE`; collector reads it each scan cycle (no restart required).
- Added direct live-scan feed for panel detail:
  - `get_wifi_scan_results` Tauri command
  - panel renders per-mode output immediately:
    - Mode A: channel-only
    - Mode B: hashed identifiers
    - Mode C: raw SSID/BSSID.

#### 5) SDR panel usability issues
**Symptom:** RF/Wi-Fi findings were cramped and hard to review.

**Fix:** Converted both RF Environment and Wi-Fi Density to full-width, scrollable panels with sticky headers and expanded finding tables.

---

_Last maintenance update: 2026-02-25 20:54 CST (README troubleshooting + SIGINT phase sync)_

## Changelog

### v0.2.3 (2026-03-11) — Android DAT Stability + Live Feed Controls

#### Android / DAT
- ✅ Fixed DAT package auth path in CI (GitHub Packages token secret + Gradle resolution path)
- ✅ Raised Android `minSdk` to 29 for `mwdat-camera:0.4.0`
- ✅ Upgraded Kotlin Android plugin to 2.1.0 to match DAT 0.4.0 metadata
- ✅ Updated DAT status handling for current SDK sealed types (`PermissionStatus`, `RegistrationState`)
- ✅ Added fresh-session fallback when DAT stream is active but no frames are produced (`Frame:NO`)

#### Live Feed UX
- ✅ Live feed modal now **auto-sizes to video dimensions** (camera and glasses feeds) to reduce black space
- ✅ Added **Full Screen** toggle in feed modal
- ✅ Added **Stop Feed** button to explicitly stop/clear active live sessions
- ✅ Added frame readiness indicator in glasses status for easier field troubleshooting
- ✅ Added `frame_age_ms` freshness telemetry + **Reconnect Glasses** control for post-restart / post-power-cycle recovery
- ✅ Added auto one-shot reconnect when Watch Live detects repeated empty frame polls
- ✅ Android feed renderer now matches hub strategy end-to-end: feed-type detection, page→direct-media extraction attempt, direct media playback in `<video>` (including HLS fallback via hls.js), and source-page feeds opened in a dedicated in-app viewer (hub-style source window behavior) with external-open fallback
- ✅ Added in-modal "embed blocked" hint for page feeds (same operator guidance as hub: use Open External when CSP/X-Frame blocks render)

### v0.2.2 (2026-02-24–25) — ADS-B Live + Full SIGINT Pipeline

#### ADS-B
- ✅ **Fixed ADS-B dead silence** — threading deadlock in Python TCP bridge (`flush_to_clients` held lock while calling `broadcast`)
- ✅ **Fixed Tauri event delivery** — background thread `emit()` unreliable; replaced with `get_rtl_sdr_status` invoke polling
- ✅ **Fixed unbound event listener** — `window.__TAURI__.event.listen` without `.bind()` — silent failure
- ✅ **Live aircraft on 2D and 3D maps** — STARTING → CONNECTING → CONNECTED, color-by-altitude, Cesium drop-lines
- ✅ **60s stale pruning** — aircraft removed from both maps simultaneously, count reflects on-screen truth
- ✅ **ADS-B layer toggle** — toolbar buttons actually show/hide Leaflet markers and Cesium entities

#### UI Improvements
- ✅ **Dynamic PLI entities** — `ingestPLI()` populates Tracked Entities and map markers from Meshtastic position events
- ✅ **Squad CRUD** — add, edit (pre-filled modal), delete with confirm; starts empty
- ✅ **SATCOM** — ISS only; NOAA 19 (decommissioned) and AO-91 (inactive) removed
- ✅ **Map layer toggles** — UNITS, ADS-B, RF, Wi-Fi buttons all functional

#### SIGINT RF + Wi-Fi Heatmap (`sigint/` crate — 84 tests, 0 failures)

**Phase 5.1 — Foundation**
- `sigint/` Rust crate — portable, no Tauri dependency, Linux/Android-ready
- `wire.rs`: TileUpdate JSON wire format (schema v1, Ed25519 signature field)
- `confidence.rs`: GPS × sample × dwell × speed formula, 11 boundary tests
- `gps.rs`: GpsProvider trait, h3o cell lookup at H3 resolution 10, SHARED_GPS_FIX static
- `rf.rs`: hackrf_sweep CSV parser, RingBuffer, Welford online mean, RfTileBucket
- `wifi.rs`: WifiScanner trait, CoreWLAN via `scan_wifi.swift` (airport CLI removed in macOS 15), Privacy Modes A/B/C, WifiTileBucket
- `storage.rs`: NodeDb SQLite (WAL), ON CONFLICT weighted-mean merge, sync cursors

**Phase 5.2 — Hub + Sync**
- `sync.rs`: SyncTransport trait — HttpSyncTransport (VPN/LAN), NullSyncTransport
- `hub.rs`: HubDb, minimal std::net HTTP server on `0.0.0.0:8789` — `POST /api/push`, `GET /api/delta`
- `collector.rs`: RF flush 5s, Wi-Fi scan 30s, sync push/pull 30s

**Phase 5.3 — Visualization**
- Leaflet.heat heatmap — RF: blue→red (−100→−40 dBm), Wi-Fi: blue→orange (sparse→dense)
- Semi-transparent so map remains visible; confidence drives opacity floor (min 0.3)
- Hub auto-start on app launch, collector auto-start after hub
- `get_sigint_delta` polls hub every 5s; location change resets cursor for new area
- Full-width scrollable RF + Wi-Fi panels with sticky column headers

**Phase 5.4 — Hardening**
- `crypto.rs`: Ed25519 keypair, device_id from public key, sign/verify, 5 tests
- `sanitize.rs`: RF freq/power bounds, Wi-Fi RSSI/band validation, GPS bounds, per-node rate limiter, exponential time decay (RF 5min, Wi-Fi 2min), 20 tests
- `manet.rs`: Reticulum transport stub, `transport_from_config()` factory

**Phase 5.5 — Integration Complete**
- `sweeper.rs`: `hackrf_sweep` spawner, auto-detects binary, per-band ring buffer feed
- Ed25519 signing wired into `sync_push()`; hub verifies on merge
- CoreWLAN scanner via bundled `scan_wifi.swift` (replaces removed `airport` CLI)
- `SHARED_GPS_FIX` bridges CoreLocation → collector thread (real GPS positions)
- `SHARED_PRIVACY_MODE` bridges Settings UI → collector thread (live mode switch)
- Privacy mode selector in Settings: A (channel only) / B (hashed IDs) / C (raw SSIDs)
- `get_wifi_scan_results` Tauri command — bypasses hub aggregation, returns live scan data formatted per privacy mode; panel updates every 5s
- Hub URL configurable in Settings with instant reconnect

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

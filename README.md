# Overwatch — Tactical Operations Center

**Offline-first tactical communications and situational awareness for mesh networks.**

Inspired by XTOC™, Anduril Lattice, and built for emergency response, field operations, and off-grid communications.

---

## Status

**Version:** 0.2.2  
**Status:** Production-ready desktop app with Meshtastic mesh network support and live ADS-B tracking

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
- [x] Squad management
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
- [ ] SATCOM tracking with TLE propagation
- [ ] AIS/ADS-B SDR hardware integration
- [ ] ATAK KML/CoT import/export
- [ ] Zone editor with polygon drawing
- [ ] SECURE mode encryption (AES-256)

### Phase 5: Lattice/ATAK Integration (v0.6)
- [ ] Anduril Lattice entity data model
- [x] CoT (Cursor on Target) message parsing/generation — **v0.2.1**
- [x] ATAK mode for Meshtastic — **v0.2.1**
- [ ] TAK server connection (TCP/UDP)
- [ ] Full MIL-STD-2525 symbol support
- [ ] Real-time entity synchronization

### Phase 6: Security (v0.7)
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

---

## Changelog

### v0.2.2 (2026-02-24) — ADS-B Live Tracking Fix
- ✅ **Fixed ADS-B dead silence bug** — threading deadlock in Python TCP bridge caused zero aircraft data to reach UI
- Root cause: `flush_to_clients()` held `self.lock` while calling `broadcast()`, which also tried to acquire `self.lock` — instant deadlock on first 500ms tick
- Fix: snapshot aircraft data under lock, release lock, broadcast outside lock
- Verified: 16+ live aircraft streaming in real-time after fix (UAL584, EJA744, SKW5336, etc.)
- See [Troubleshooting & Post-Mortems](#troubleshooting--post-mortems) for full details

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

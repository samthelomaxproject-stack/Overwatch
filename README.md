# Overwatch — Tactical Operations Center

**Offline-first tactical communications and situational awareness for mesh networks.**

Inspired by XTOC™ and built for emergency response, field operations, and off-grid communications.

---

## Status

**Version:** 0.1.2  
**Status:** Desktop app functional — UI navigation and GPS tracking working

### Recent Fixes
- ✅ Reverted GPS changes that broke sidebar navigation icons
- ✅ Re-implemented GPS with proper MGRS grid coordinates
- ✅ Built native macOS app bundle

---

## Overview

Overwatch is a local-first, offline-ready Tactical Operations Center (TOC) application. It enables structured communications (SITREPs, TASKs, CHECKINs) over any transport — mesh networks, radio, QR, or copy/paste.

## Core Features (Implemented)

### Communication
- **Packet-based reports**: SITREP, TASK, CHECKIN/LOC, CONTACT, RESOURCE, ASSET, ZONE, MISSION, EVENT
- **Transport agnostic**: Works over any text transport
- **Packet chunking**: Split long messages for tight character limits (planned)
- **QR workflows**: Scan/share packets between devices (planned)

### Mapping & Situational Awareness
- **Tactical map**: Leaflet-based with multiple tile layers (dark, satellite, terrain, streets)
- **GPS tracking**: Real-time location with MGRS grid coordinates
- **Member markers**: Pulsing blue dot for own position
- **Zones**: Draw circles/polygons, share as packets (UI ready, backend planned)
- **SATCOM**: Satellite tracking panel (UI ready, TLE integration planned)
- **SDR Panel**: ADS-B, AIS, SATCOM receiver interface (UI ready)

### Organization
- **Squads**: Persistent groups with roster management
- **Callsign support**: Customizable with real-time status bar updates
- **Multi-unit tagging**: Multiple sources per packet (planned)
- **Local-first**: No accounts, no central server

### UI
- **Dark tactical theme**: Optimized for field use
- **Responsive sidebar navigation**: Packets, Map, SDR, Squads, SATCOM, Zones, ATAK, Settings
- **Modal dialogs**: Clean packet/squad/zone creation
- **Status bar**: Connection, callsign, grid coordinates

---

## Build Instructions

### Prerequisites
- Rust 1.77+ (`cargo`, `rustc`)
- Node.js (for future frontend tooling)

### Desktop App (Tauri)

```bash
# Clone the repo
git clone https://github.com/samthelomaxproject-stack/Overwatch.git
cd Overwatch

# Build release binary
cd src-tauri
cargo build --release

# Or build full macOS app bundle
cargo tauri build --bundles app
```

### Run the App

```bash
# Binary directly
./src-tauri/target/release/overwatch

# Or open the macOS app
open ./src-tauri/target/release/bundle/macos/Overwatch.app
```

---

## Transport Comparison

| Transport | Max Message | Best For | Status |
|-----------|-------------|----------|--------|
| Meshtastic | ~180 chars | Quick updates | Planned |
| MeshCore | ~160 bytes | Tight limits | Planned |
| Reticulum | Flexible | MeshChat | Planned |
| LAN/MANET | Unlimited | Local networks | Planned |
| QR | Depends on screen | Offline handoff | Planned |
| Voice | N/A | Manual relay | Planned |

---

## CLEAR vs SECURE

- **CLEAR**: Default, openly decodable (ham-friendly)
- **SECURE**: Encrypted payload, shared team key (non-ham only)

> ⚠️ Do NOT transmit encrypted content on amateur radio. Follow local laws.

---

## Architecture

```
Overwatch/
├── webui/              # PWA frontend (pure HTML/JS/CSS)
│   └── index.html      # Single-file app
├── src-tauri/          # Desktop wrapper
│   ├── src/            # Rust backend
│   ├── icons/          # App icons
│   └── tauri.conf.json # App config
├── reticulum-bridge/   # Reticulum transport (planned)
├── meshtastic-bridge/  # Meshtastic transport (planned)
├── atak-bridge/        # ATAK CoT integration (planned)
├── satcom/             # Satellite tracking (planned)
└── docs/               # Documentation
```

---

## Development Roadmap

### Phase 1: Core UI (v0.1) ✅
- [x] Basic HTML/CSS/JS frontend
- [x] Tauri desktop wrapper
- [x] Sidebar navigation
- [x] Packet creation UI
- [x] Map with Leaflet
- [x] Squad management
- [x] Settings panel
- [x] GPS tracking with MGRS

### Phase 2: Data Layer (v0.2)
- [ ] IndexedDB local storage
- [ ] Packet persistence
- [ ] Export/import functionality
- [ ] QR code generation

### Phase 3: Mesh Integration (v0.3)
- [ ] Reticulum bridge
- [ ] Meshtastic Web Bluetooth
- [ ] Packet chunking/dechunking
- [ ] Transport abstraction layer

### Phase 4: Advanced Features (v0.4)
- [ ] SATCOM tracking with TLE
- [ ] AIS/ADS-B SDR integration
- [ ] ATAK KML/CoT import/export
- [ ] Zone editor
- [ ] SECURE mode encryption

### Phase 5: Security (v0.5)
- [ ] Gjallarhorn integration
- [ ] AI analysis features
- [ ] Threat detection

---

## Getting Started

### Desktop App
1. Download or build the app
2. Launch Overwatch
3. Set your callsign in Settings
4. Allow location access for GPS
5. Create your first packet or explore the map

### Web Browser (Development)
```bash
cd webui
python3 -m http.server 8080
# Open http://localhost:8080
```

---

## Integrations (Planned)

### Reticulum
```bash
pip install rns nomadnet
# Run reticulum-bridge
```

### Meshtastic
```bash
# Connect via Web Bluetooth
# Import channel labels automatically
```

### ATAK
```bash
# Use atak-helper for KML/CoT bridge
```

---

## Comparison: Overwatch vs XTOC

| Feature | Overwatch | XTOC |
|---------|-----------|------|
| Open Source | ✅ | ❌ |
| AI Agents | ✅ (our bots) | ❌ |
| Customizable | ✅ | Limited |
| Gjallarhorn | Future integration | ❌ |
| Self-hosted | ✅ | ✅ |
| Desktop App | ✅ | ✅ |
| GPS/MGRS | ✅ | ? |

---

## License

MIT

---

## Related Projects

- [Gjallarhorn](../Gjallarhorn) — Mobile SOC Toolkit (future security integration)
- [Reticulum](https://github.com/markqvist/Reticulum) — Mesh networking stack
- [NomadNet](https://github.com/markqvist/NomadNet) — Mesh messaging
- [Meshtastic](https://meshtastic.org/) — LoRa mesh communication

---

## Changelog

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

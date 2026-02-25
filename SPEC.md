# Overwatch — Technical Specification

**Version:** 0.1.0  
**Status:** In Development  
**Last Updated:** 2026-02-18

---

## 1. Project Overview

### Mission

Build an open-source Tactical Operations Center (TOC) that enables structured communications and situational awareness over mesh networks, designed for emergency response, field operations, and off-grid communications.

### Core Philosophy

- **Offline-first**: Works without internet connectivity
- **Local-first**: No accounts, no central server, no dependencies
- **Transport-agnostic**: Works over any text-based transport
- **AI-ready**: Designed for agent integration (future Gjallarhorn security features)

### Inspired By

- XTOC™ (commercial product)
- Reticulum/LXMF mesh networking
- ATAK (Android Team Awareness Kit)
- Traditional ham radio Winlink workflows

---

## 2. Architecture

### High-Level Design

```
┌─────────────────────────────────────────────────────────┐
│                    Overwatch PWA                        │
│  ┌──────────┐  ┌──────────┐  ┌──────────────────────┐ │
│  │  Packet  │  │   Map    │  │    Transport Layer    │ │
│  │ Manager  │  │  View    │  │                      │ │
│  └──────────┘  └──────────┘  │  ┌──────────────┐    │ │
│                                │  │  Reticulum  │    │ │
│  ┌──────────┐  ┌──────────┐  │  ├──────────────┤    │ │
│  │  Squad   │  │   SATCOM │  │  │  Meshtastic  │    │ │
│  │ Manager  │  │  Tracker │  │  ├──────────────┤    │ │
│  └──────────┘  └──────────┘  │  │  Web Bluetooth │   │ │
│                              │  ├──────────────┤    │ │
│  ┌──────────┐  ┌──────────┐  │  │  QR Import   │    │ │
│  │   Zone   │  │   AIS    │  │  ├──────────────┤    │ │
│  │  Editor  │  │ Tracker  │  │  │  Clipboard   │    │ │
│  └──────────┘  └──────────┘  │  └──────────────┘    │ │
│                              └──────────────────────────┘ │
└─────────────────────────────────────────────────────────┘
```

### Module Breakdown

| Module | Responsibility |
|--------|---------------|
| `webui/` | PWA frontend (HTML/CSS/JS) |
| `reticulum-bridge/` | Reticulum network transport |
| `meshtastic-bridge/` | Meshtastic Bluetooth integration |
| `atak-bridge/` | ATAK KML/CoT import/export |
| `satcom/` | Satellite tracking (TLE processing) |

---

## 3. Packet System

### Packet Types

| Type | Purpose | Example |
|------|---------|---------|
| SITREP | Situation Report | "Enemy forces at grid square 1234" |
| TASK | Task Assignment | "Alpha team: secure checkpoint" |
| CHECKIN | Location/Status | "Alpha-1: at CP-3, all clear" |
| CONTACT | Contact Report | "Hostile vehicle, 2x PKM, bearing 270" |
| RESOURCE | Resource Request | "Need MEDEVAC at grid 1234" |
| ASSET | Asset Status | "UAV-1: battery 45%, 30min loiter" |
| ZONE | Zone Definition | "No-fly zone: 1234-5678" |
| MISSION | Mission Brief | "Operation Phoenix: 0600-1800" |
| EVENT | Event Log | "Artillery impact: 1234, 0615" |

### Packet Format (CLEAR)

```
[FROM:ALPHA-1][TO:ALL][TYPE:SITREP][PRI:ROUTINE][ID:001]
Grid: 34.1234N, 117.5678W
Status: SECURE
Activity: None
Requests: None
```

### Packet Format (SECURE)

```
[FROM:ALPHA-1][TO:ALL][TYPE:SITREP][PRI:ROUTINE][ID:001][HASH:abc123]
<encrypted payload>
<HMAC signature>
```

### Chunking

- Meshtastic: ~180 chars
- MeshCore: ~160 bytes
- Auto-split with sequence: `1/3`, `2/3`, `3/3`
- Receiver reassembles and deduplicates

---

## 4. Transport Layer

### Supported Transports

#### Reticulum (Primary)

- **Protocol**: LXMF (LXMessage Format)
- **Hardware**: RNode (LoRa), packet radio
- **Features**:
  - End-to-end encryption
  - Store-and-forward
  - Multi-hop routing

```python
# reticulum-bridge/example.py
import RNS
import LXMF

# Initialize Reticulum
RNS.configure()
reticulum = RNS.Reticulum()

# Initialize LXMF
lxmf = LXMF.LXMF(reticulum)

# Send packet
destination = RNS.Destination(reticulum, None, "mesh.chat")
lxmf.send(destination, packet_text)
```

#### Meshtastic

- **Interface**: Web Bluetooth
- **Features**:
  - Channel labels import
  - Node DM
  - Auto-import received packets

#### Web Bluetooth

```javascript
// meshtastic-bridge/connect.js
const device = await navigator.bluetooth.requestDevice({
  filters: [{ services: ['6ba1e986-8c4c-4b20-8c5c-0d8a1b2c3d4e'] }]
});
const server = await device.gatt.connect();
```

#### QR Code

- **Library**: qrcode.js or qr-scanner
- **Workflow**: Generate → Display → Scan → Import
- **Multipart**: Stitch multiple QR codes

---

## 5. Mapping System

### Tactical Map

- **Library**: Leaflet.js or Mapbox GL
- **Tiles**: Offline-capable (vector tiles)
- **Features**:
  - Member markers (color-coded by squad)
  - Packet markers (auto-placed from location data)
  - Zone overlays (circles, polygons)

### Zone Types

| Type | Visual | Use Case |
|------|--------|----------|
| Restricted | Red hatched | No-fly, danger area |
| Friendly | Blue fill | Own forces |
| Neutral | Green fill | Safe corridor |
| Objective | Yellow outline | Target area |

### SATCOM Tracking

- **TLE Source**: CelesTrak, Space-Track
- **Library**: satellite.js
- **Features**:
  - Pass predictions
  - Ground track visualization
  - Elevation/azimuth indicators

```javascript
// satcom/track.js
import { Satellite } from 'satellite.js';

const satrec = Satellite.twoline2satrec(TLE_LINE1, TLE_LINE2);
const position = Satellite.propagate(satrec, new Date());
const gmst = Satellite.gstime(new Date());
const lookAngles = Satellite.ecfToLookAngles(position, gmst, location);
```

### ATAK Integration

- **Import**: KML/KMZ files
- **Export**: KML/KMZ
- **Bridge**: CoT (Cursor on Target) format

```xml
<!-- CoT Example -->
<event type="a-f-G-U-C" uid="ALPHA-1" time="2026-02-18T12:00:00Z" start="2026-02-18T12:00:00Z" stale="2026-02-18T12:05:00Z">
  <point lat="34.1234" lon="-117.5678" hae="0"/>
  <detail>
    <contact callsign="ALPHA-1"/>
  </detail>
</event>
```

---

## 6. Data Model

### Local Storage

```javascript
// IndexedDB schema
{
  units: [
    { id: "ALPHA-1", callsign: "Alpha 1", role: "Team Lead", color: "#ff0000" }
  ],
  squads: [
    { id: "ALPHA", name: "Alpha Team", members: ["ALPHA-1", "ALPHA-2"] }
  ],
  packets: [
    { id: "001", type: "SITREP", from: "ALPHA-1", to: "ALL", content: "...", timestamp: "..." }
  ],
  zones: [
    { id: "z1", type: "restricted", coordinates: [[lat, lon], ...] }
  ],
  settings: {
    transport: "reticulum",
    encryptionKey: "...",
    callsign: "..."
  }
}
```

### Packet Schema

```typescript
interface Packet {
  id: string;
  type: PacketType;
  priority: Priority;
  from: string;
  to: string;
  timestamp: Date;
  location?: {
    lat: number;
    lon: number;
    accuracy: number;
  };
  content: string;
  encrypted?: boolean;
  signature?: string;
}
```

---

## 7. UI/UX Design

### Color Scheme

| Role | Color | Hex |
|------|-------|-----|
| Primary | Tactical Blue | #1a3a5c |
| Secondary | Dark Slate | #2d3748 |
| Accent | Amber | #f6ad55 |
| Success | Green | #48bb78 |
| Warning | Orange | #ed8936 |
| Danger | Red | #f56565 |
| Background | Charcoal | #1a202c |
| Text | Light Gray | #e2e8f0 |

### Packet Priority

| Priority | Color | Icon |
|----------|-------|------|
| ROUTINE | Blue | ○ |
| PRIORITY | Yellow | ◐ |
| URGENT | Orange | ◑ |
| FLASH | Red | ● |

### Layout

```
┌────────────────────────────────────────────────┐
│  HEADER: Logo | Transport | Status | Settings │
├────────────┬───────────────────────────────────┤
│            │                                   │
│  SIDEBAR   │         MAIN CONTENT              │
│            │                                   │
│  - Packets │  Map / Packet Detail / Squads    │
│  - Map     │                                   │
│  - Squads  │                                   │
│  - SATCOM  │                                   │
│  - Zones   │                                   │
│  - ATAK    │                                   │
│            │                                   │
├────────────┴───────────────────────────────────┤
│  FOOTER: Connection Status | Last Sync | Time │
└────────────────────────────────────────────────┘
```

---

## 8. Security

### CLEAR Mode (Default)

- No encryption
- Human-readable format
- Ham radio compliant
- Any receiver can decode

### SECURE Mode

- **Algorithm**: AES-256-CBC
- **Key Exchange**: Pre-shared team key
- **Integrity**: HMAC-SHA256
- **Key Derivation**: PBKDF2

> ⚠️ WARNING: Do NOT use SECURE mode on amateur radio. Follow local regulations.

---

## 9. Integration Points

### Future: Gjallarhorn

```python
# Security scanning integration
from gjallarhorn import SecurityScanner

# Scan received packets for indicators
scanner = SecurityScanner()
result = scanner.scan(packet.content)
if result.threat_detected:
    alert_operator(result)
```

### AI Agent Integration

```python
# AI analysis of packet data
from openclaw import Agent

agent = Agent("Overwatch-Analyst")
summary = agent.analyze(packet_history, query="Identify patterns")
```

---

## 10. Development Roadmap

### Phase 1: Core (v0.1)

- [x] Project initialization
- [ ] Basic packet creation UI
- [ ] Local storage (IndexedDB)
- [ ] QR import/export
- [ ] Basic mapping

### Phase 2: Mesh (v0.2)

- [ ] Reticulum bridge
- [ ] Meshtastic bridge
- [ ] Web Bluetooth integration
- [ ] Packet chunking/dechunking

### Phase 3: Advanced (v0.3)

- [ ] SATCOM tracking
- [ ] ATAK import/export
- [ ] Squad management
- [ ] Zone editor

### Phase 4: Security (v0.4)

- [ ] SECURE mode encryption
- [ ] Gjallarhorn integration
- [ ] AI analysis features

---

## 11. Dependencies

### Frontend

```json
{
  "dependencies": {
    "leaflet": "^1.9.4",
    "qrcode": "^1.5.3",
    "idb": "^7.1.1",
    "satellite.js": "^5.0.0"
  }
}
```

### Backend (Bridges)

```python
# reticulum-bridge/requirements.txt
rns>=0.9.0
lxmf>=0.3.0
```

---

## 12. File Structure

```
Overwatch/
├── SPEC.md                    # This file
├── README.md                  # Overview
├── LICENSE                    # MIT
│
├── webui/                     # Frontend
│   ├── index.html
│   ├── css/
│   │   └── styles.css
│   ├── js/
│   │   ├── app.js
│   │   ├── packets.js
│   │   ├── map.js
│   │   └── storage.js
│   └── manifest.json
│
├── reticulum-bridge/          # Reticulum transport
│   ├── bridge.py
│   └── requirements.txt
│
├── meshtastic-bridge/        # Meshtastic transport
│   ├── connect.js
│   └── protocol.js
│
├── atak-bridge/              # ATAK integration
│   ├── kml_import.py
│   ├── cot_bridge.py
│   └── requirements.txt
│
├── satcom/                   # Satellite tracking
│   ├── tracker.js
│   └── tle_loader.js
│
└── docs/
    ├── packet-format.md
    ├── transport-guide.md
    └── setup.md
```

---

## 13. References

- [Reticulum](https://github.com/markqvist/Reticulum)
- [LXMF](https://github.com/markqvist/lxmf)
- [NomadNet](https://github.com/markqvist/NomadNet)
- [Meshtastic](https://meshtastic.org/)
- [satellite.js](https.com/satellitejs)
- [Leaflet.js](https://leafletjs.com/)
- [XTOC](https://store.mkme.org/product/xtoc-tactical-operations-center-software-suite/)

---

*This spec is a living document. Updates will be made as the project evolves.*

---

## SIGINT / RF + Wi-Fi Heatmap System

### Finalized Parameters
| Parameter | Value |
|-----------|-------|
| H3 resolution | 10 (collection), 8–10 served by zoom |
| Time bucket | 60 seconds |
| Raw flush window | 5 seconds (hackrf_sweep → observation) |
| Privacy default | Mode A (channel hotness only) |
| RF source | `hackrf_sweep` CLI (HackRF One/Portapack USB) |
| Wi-Fi source | OS-native (airport/iw/WifiManager) |
| Transport | HTTP MVP, interface-abstracted for MANET/VPN/Starlink |
| Hub OS | Linux-ready from day one |
| Hub host model | Standalone `hub-api` binary, Tauri spawns it on Mac |
| VPN/Network | Ubiquiti Dream Machine Pro via Wifiman |
| PLI transport | Meshtastic (low-bandwidth, always-on) |
| Tile sync transport | HTTP over VPN (30s intervals) |

### Build Order
**Phase 1 — Foundation:** Wire format → confidence formula → hackrf_sweep parser → GPS provider → RF aggregation → Wi-Fi scanner → Wi-Fi aggregation
**Phase 2 — Hub + Sync:** hub-api binary → node sync client → Tauri integration
**Phase 3 — Visualization:** h3-js Leaflet overlay → color ramp → sidebar panels
**Phase 4 — Hardening:** Ed25519 → anti-poisoning → time decay → MANET transport

### Confidence Formula
```
confidence = GPS_factor × sample_factor × dwell_factor × speed_factor
GPS_factor    = clamp(1 − (gps_accuracy_m / 20.0), 0, 1)
sample_factor = min(sample_count / 10.0, 1.0)
dwell_factor  = min(dwell_seconds / 30.0, 1.0)
speed_factor  = clamp(1 − (speed_mps / 30.0), 0, 1)
```

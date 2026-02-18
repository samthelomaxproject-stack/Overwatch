# Overwatch — Tactical Operations Center

**Offline-first tactical communications and situational awareness for mesh networks.**

Inspired by XTOC™ and built for emergency response, field operations, and off-grid communications.

---

## Overview

Overwatch is a local-first, offline-ready Tactical Operations Center (TOC) web application. It enables structured communications (SITREPs, TASKs, CHECKINs) over any transport — mesh networks, radio, QR, or copy/paste.

## Core Features

### Communication
- **Packet-based reports**: SITREP, TASK, CHECKIN/LOC, CONTACT, RESOURCE, ASSET, ZONE, MISSION, EVENT
- **Transport agnostic**: Works over any text transport
- **Packet chunking**: Split long messages for tight character limits
- **QR workflows**: Scan/share packets between devices
- **TTS voice output**: Spell packets for manual radio relay

### Mesh Networking
- **Reticulum/LXMF**: Native support via reticulum-bridge
- **Meshtastic**: Bluetooth integration, channel labels, node DM
- **MeshCore**: Support for MeshCore devices
- **MANET**: LAN-based mesh (WiFi HaLow, Open MANET)
- **Winlink-style**: Traditional ham radio email workflows

### Mapping & Situational Awareness
- **Tactical map**: Member locations + packet markers
- **Zones**: Draw circles/polygons, share as packets
- **SATCOM**: Satellite tracking (TLE-based)
- **AIS**: Ship tracking overlay
- **ATAK integration**: KML/CoT import/export

### Organization
- **Squads**: Persistent groups with roster management
- **Multi-unit tagging**: Multiple sources per packet
- **Local-first**: No accounts, no central server

---

## Transport Comparison

| Transport | Max Message | Best For |
|----------|-------------|----------|
| Meshtastic | ~180 chars | Quick updates |
| MeshCore | ~160 bytes | Tight limits |
| Reticulum | Flexible | MeshChat |
| LAN/MANET | Unlimited | Local networks |
| QR | Depends on screen | Offline handoff |
| Voice | N/A | Manual relay |

---

## CLEAR vs SECURE

- **CLEAR**: Default, openly decodable (ham-friendly)
- **SECURE**: Encrypted payload, shared team key (non-ham only)

> ⚠️ Do NOT transmit encrypted content on amateur radio. Follow local laws.

---

## Architecture

```
Overwatch/
├── webui/           # PWA frontend
├── reticulum-bridge/ # Reticulum transport
├── meshtastic-bridge/ # Meshtastic integration
├── atak-bridge/     # ATAK CoT <-> Overwatch
├── satcom/          # Satellite tracking
└── docs/            # Documentation
```

---

## Getting Started

1. Install as PWA (Safari → Add to Home Screen / Chrome → Install)
2. Set up team (Unit IDs, callsigns, roles)
3. Configure mesh transport (optional)
4. Start creating packets

---

## Integrations

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

---

## License

MIT

---

## Related Projects

- [Gjallarhorn](../Gjallarhorn) — Mobile SOC Toolkit (future security integration)
- [Reticulum](https://github.com/markqvist/Reticulum) — Mesh networking stack
- [NomadNet](https://github.com/markqvist/NomadNet) — Mesh messaging
- [Meshtastic](https://meshtastic.org/) — LoRa mesh communication

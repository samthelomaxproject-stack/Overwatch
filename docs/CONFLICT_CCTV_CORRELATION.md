# Conflict-to-CCTV Correlation

Minimal event-to-camera correlation for the Conflict layer.

## Overview

Conflict events now show nearby CCTV cameras in their popups, allowing operators to open live streams from event context.

## How It Works

**Frontend-only distance calculation:**
1. When a Conflict event popup opens, it searches `cctvCameras` global object
2. Calculates distance using Haversine formula
3. Finds cameras within 500m radius
4. Shows top 3 nearest cameras sorted by distance

**No backend changes:**
- Reuses existing `cctvCameras` data structure
- Reuses existing `startCctvLive(camId)` stream function
- No new API endpoints
- No new map layer

## Configuration

```javascript
const CCTV_EVENT_RADIUS_METERS = 500;  // Search radius
const CCTV_EVENT_MAX_NEARBY_DISPLAY = 3;  // Max cameras shown in popup
```

Edit in `webui/conflict-module.js` if needed.

## UI Behavior

### Event Popup Shows:
```
🎥 Nearby CCTV: 3
🟢 Downtown Cam A (120m) [▶️ Live]
🟢 Plaza Cam B (250m) [▶️ Live]
🔴 Station Cam C (480m) [▶️ Live]
```

- **Green dot**: Camera status ACTIVE
- **Red dot**: Camera offline/unavailable
- **Distance**: Meters from event
- **▶️ Live button**: Opens stream in existing CCTV popup

### If No Cameras Nearby:
- Camera section is omitted
- Event popup works normally

### Stream Opening:
- Clicking "▶️ Live" calls `window.startCctvLive(camId)`
- Reuses existing CCTV streaming logic (HLS/MJPEG/direct)
- If camera unavailable, existing error handling applies

## Technical Details

### Distance Calculation
Uses Haversine formula:
```javascript
function calculateDistance(lat1, lon1, lat2, lon2) {
  const R = 6371000; // Earth radius in meters
  const φ1 = lat1 * Math.PI / 180;
  const φ2 = lat2 * Math.PI / 180;
  const Δφ = (lat2 - lat1) * Math.PI / 180;
  const Δλ = (lon2 - lon1) * Math.PI / 180;

  const a = Math.sin(Δφ/2) * Math.sin(Δφ/2) +
            Math.cos(φ1) * Math.cos(φ2) *
            Math.sin(Δλ/2) * Math.sin(Δλ/2);
  const c = 2 * Math.atan2(Math.sqrt(a), Math.sqrt(1-a));

  return R * c;
}
```

### Data Flow
```
Conflict Event → findNearbyCameras(lat, lon)
                 ↓
         window.cctvCameras (global)
                 ↓
         Distance calculation
                 ↓
         Sort by nearest
                 ↓
         Show top 3 in popup
                 ↓
   User clicks ▶️ Live
                 ↓
   window.startCctvLive(camId)
                 ↓
   Existing CCTV stream handler
```

## Fail-Soft Behavior

- **No cctvCameras object**: Camera section omitted, event works normally
- **No cameras in radius**: Camera section omitted
- **Camera offline**: Shows red dot, button still present (existing CCTV error handling)
- **Stream unavailable**: Existing CCTV popup shows error

## Files Changed

```
webui/conflict-module.js  - Added correlation functions + popup integration
webui/index.html          - Exposed cctvCameras globally
```

## Testing

1. Enable Conflict layer
2. Enable CCTV layer (ensure cameras loaded)
3. Click any Conflict event near a camera
4. Check popup shows "🎥 Nearby CCTV: N"
5. Click "▶️ Live" button
6. Verify stream opens in CCTV marker popup

## Future Enhancements

- [ ] Optional side panel with full camera list
- [ ] Visual indicator on event markers when cameras nearby
- [ ] Reverse correlation: Show nearby events when viewing camera
- [ ] Adjustable radius via UI settings

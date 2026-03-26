window.initConflictModule = function initConflictModule(map, options = {}) {
  if (!window.L || !map) return null;

  const apiBase = (options.apiBase || 'http://127.0.0.1:8790').replace(/\/$/, '');
  const markerLayer = window.L.layerGroup();

  const state = {
    visible: false,
    windowRange: 'week', // day | week | month
    lastLoadedAt: 0,
  };

  function normalizeItems(payload) {
    if (Array.isArray(payload)) return payload;
    if (payload && Array.isArray(payload.items)) return payload.items;
    return [];
  }

  function clearMarkers() {
    markerLayer.clearLayers();
  }

  // Config for camera correlation
  const CCTV_EVENT_RADIUS_METERS = 500;
  const CCTV_EVENT_MAX_NEARBY_DISPLAY = 3;

  function calculateDistance(lat1, lon1, lat2, lon2) {
    // Haversine formula for distance in meters
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

  function findNearbyCameras(eventLat, eventLon) {
    // Access global cctvCameras from index.html
    if (typeof window.cctvCameras === 'undefined') return [];

    const nearby = [];
    
    Object.values(window.cctvCameras).forEach(cam => {
      if (!cam.lat || !cam.lon) return;
      
      const distance = calculateDistance(eventLat, eventLon, cam.lat, cam.lon);
      
      if (distance <= CCTV_EVENT_RADIUS_METERS) {
        nearby.push({
          id: cam.id,
          name: cam.name,
          lat: cam.lat,
          lon: cam.lon,
          distance: Math.round(distance),
          hasStream: !!cam.snapshotUrl,
          status: cam.status
        });
      }
    });

    // Sort by distance
    nearby.sort((a, b) => a.distance - b.distance);
    
    return nearby;
  }

  function makePopup(ev) {
    const title = ev.title || 'Conflict Event';
    const eventType = ev.event_type || ev.type || 'other';
    const location =
      ev.location ||
      [ev.city, ev.admin1, ev.country].filter(Boolean).join(', ') ||
      'Unknown location';
    const summary = ev.summary || 'No summary available';
    const source = ev.source_name || ev.source_type || 'Unknown source';
    const published = ev.published_at || ev.date || '';
    
    // Show source badge
    const sourceType = ev.source_type || 'rss';
    const confidence = ev.confidence_score ? ` (${Math.round(ev.confidence_score * 100)}%)` : '';
    let sourceBadge = '';
    
    if (sourceType === 'social') {
      const verification = ev.verification_status || 'unverified';
      sourceBadge = `<br/><span style="color: orange; font-size: 0.9em;">⚠️ Social OSINT - ${verification}${confidence}</span>`;
    } else if (sourceType === 'usgs') {
      sourceBadge = `<br/><span style="color: #9333EA; font-size: 0.9em;">🌍 USGS Verified${confidence}</span>`;
    } else if (sourceType === 'reliefweb') {
      sourceBadge = `<br/><span style="color: #9333EA; font-size: 0.9em;">🏛️ ReliefWeb${confidence}</span>`;
    } else if (sourceType === 'firms') {
      sourceBadge = `<br/><span style="color: #9333EA; font-size: 0.9em;">🛰️ NASA FIRMS${confidence}</span>`;
    }

    // Find nearby cameras
    const nearbyCameras = findNearbyCameras(ev.lat, ev.lon);
    let cameraSection = '';
    
    if (nearbyCameras.length > 0) {
      const nearest = nearbyCameras[0];
      const cameraList = nearbyCameras.slice(0, CCTV_EVENT_MAX_NEARBY_DISPLAY).map(cam => {
        const statusBadge = cam.status === 'ACTIVE' ? '🟢' : '🔴';
        const streamBtn = cam.hasStream 
          ? `<button onclick="window.startCctvLive('${cam.id}')" style="font-size:10px;padding:2px 6px;margin-left:4px;cursor:pointer;">▶️ Live</button>`
          : '';
        return `<div style="font-size:11px;margin-top:4px;">${statusBadge} ${cam.name} (${cam.distance}m)${streamBtn}</div>`;
      }).join('');
      
      cameraSection = `
        <hr style="margin:8px 0;border:none;border-top:1px solid #333;">
        <div style="font-size:12px;">
          <strong>🎥 Nearby CCTV:</strong> ${nearbyCameras.length}
          ${cameraList}
        </div>
      `;
    }

    return `
      <div style="min-width:300px;">
        <strong>${title}</strong><br/>
        ${eventType}<br/>
        ${location}<br/><br/>
        ${summary}<br/><br/>
        <strong>Source:</strong> ${source}${published ? `<br/><strong>Date:</strong> ${published}` : ''}${sourceBadge}
        ${cameraSection}
      </div>
    `;
  }

  function addMarker(ev) {
    const lat = Number(ev.lat);
    const lon = Number(ev.lon);
    if (!Number.isFinite(lat) || !Number.isFinite(lon)) return false;

    const markerOptions = {};
    
    // Color-code by source type
    const sourceType = ev.source_type || 'rss';
    
    if (sourceType === 'social') {
      // Orange marker for social sources
      const verification = ev.verification_status || 'unverified';
      const orangeIcon = window.L.icon({
        iconUrl: 'data:image/svg+xml;base64,' + btoa(`
          <svg xmlns="http://www.w3.org/2000/svg" width="25" height="41" viewBox="0 0 25 41">
            <path fill="${verification === 'corroborated' ? '#FFA500' : '#FF6B00'}" stroke="#000" stroke-width="1" 
              d="M12.5 0C5.6 0 0 5.6 0 12.5c0 8.4 12.5 28.5 12.5 28.5S25 20.9 25 12.5C25 5.6 19.4 0 12.5 0z"/>
            <circle cx="12.5" cy="12.5" r="7" fill="white" opacity="${verification === 'corroborated' ? '1' : '0.7'}"/>
          </svg>
        `),
        iconSize: [25, 41],
        iconAnchor: [12, 41],
        popupAnchor: [1, -34]
      });
      markerOptions.icon = orangeIcon;
      markerOptions.opacity = verification === 'corroborated' ? 0.9 : 0.7;
    } else if (['usgs', 'firms', 'reliefweb'].includes(sourceType)) {
      // Purple/violet marker for structured high-confidence sources
      const purpleIcon = window.L.icon({
        iconUrl: 'data:image/svg+xml;base64,' + btoa(`
          <svg xmlns="http://www.w3.org/2000/svg" width="25" height="41" viewBox="0 0 25 41">
            <path fill="#9333EA" stroke="#000" stroke-width="1" 
              d="M12.5 0C5.6 0 0 5.6 0 12.5c0 8.4 12.5 28.5 12.5 28.5S25 20.9 25 12.5C25 5.6 19.4 0 12.5 0z"/>
            <circle cx="12.5" cy="12.5" r="7" fill="white" opacity="1"/>
          </svg>
        `),
        iconSize: [25, 41],
        iconAnchor: [12, 41],
        popupAnchor: [1, -34]
      });
      markerOptions.icon = purpleIcon;
      markerOptions.opacity = 1.0;
    }
    // Default (rss/gdelt) uses standard blue Leaflet marker

    const marker = window.L.marker([lat, lon], markerOptions);
    marker.bindPopup(makePopup(ev));
    markerLayer.addLayer(marker);
    return true;
  }

  async function load() {
    const url = `${apiBase}/api/conflict/events?window=${encodeURIComponent(state.windowRange)}`;
    const res = await fetch(url);
    if (!res.ok) {
      throw new Error(`Conflict events HTTP ${res.status}`);
    }

    const payload = await res.json();
    const items = normalizeItems(payload);

    clearMarkers();

    let rendered = 0;
    for (const ev of items) {
      try {
        if (addMarker(ev)) rendered++;
      } catch (_) {
        // skip malformed row, continue
      }
    }

    state.lastLoadedAt = Date.now();
    return { fetched: items.length, rendered };
  }

  async function setVisible(v) {
    state.visible = !!v;

    if (state.visible) {
      if (!map.hasLayer(markerLayer)) map.addLayer(markerLayer);
      return await load();
    } else {
      clearMarkers();
      if (map.hasLayer(markerLayer)) map.removeLayer(markerLayer);
      return { fetched: 0, rendered: 0 };
    }
  }

  async function setFilters(filters = {}) {
    if (filters.windowRange && ['day', 'week', 'month'].includes(filters.windowRange)) {
      state.windowRange = filters.windowRange;
    }
    if (state.visible) {
      return await load();
    }
    return { fetched: 0, rendered: 0 };
  }

  async function refreshIfVisible(force = false) {
    if (!state.visible) return { fetched: 0, rendered: 0 };
    if (!force && Date.now() - state.lastLoadedAt < 30000) {
      return { fetched: 0, rendered: 0 };
    }
    return await load();
  }

  async function loadMeta() {
    const url = `${apiBase}/api/conflict/meta`;
    const res = await fetch(url);
    if (!res.ok) {
      throw new Error(`Conflict meta HTTP ${res.status}`);
    }
    return await res.json();
  }

  return {
    setVisible,
    setFilters,
    refreshIfVisible,
    load,
    loadMeta,
  };
};

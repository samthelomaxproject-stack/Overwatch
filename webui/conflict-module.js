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
    
    // Show verification badge for social sources
    const isSocial = ev.source_type === 'social';
    const verification = ev.verification_status || 'unverified';
    const confidence = ev.confidence_score ? ` (${Math.round(ev.confidence_score * 100)}%)` : '';
    
    const verificationBadge = isSocial
      ? `<br/><span style="color: orange; font-size: 0.9em;">⚠️ Social OSINT - ${verification}${confidence}</span>`
      : '';

    return `
      <div>
        <strong>${title}</strong><br/>
        ${eventType}<br/>
        ${location}<br/><br/>
        ${summary}<br/><br/>
        <strong>Source:</strong> ${source}${published ? `<br/><strong>Date:</strong> ${published}` : ''}${verificationBadge}
      </div>
    `;
  }

  function addMarker(ev) {
    const lat = Number(ev.lat);
    const lon = Number(ev.lon);
    if (!Number.isFinite(lat) || !Number.isFinite(lon)) return false;

    // Different marker for social sources
    const isSocial = ev.source_type === 'social';
    const markerOptions = {};
    
    if (isSocial) {
      // Orange marker with lower opacity for social/unverified
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
    }

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

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

    return `
      <div>
        <strong>${title}</strong><br/>
        ${eventType}<br/>
        ${location}<br/><br/>
        ${summary}<br/><br/>
        <strong>Source:</strong> ${source}${published ? `<br/><strong>Date:</strong> ${published}` : ''}
      </div>
    `;
  }

  function addMarker(ev) {
    const lat = Number(ev.lat);
    const lon = Number(ev.lon);
    if (!Number.isFinite(lat) || !Number.isFinite(lon)) return false;

    const marker = window.L.marker([lat, lon]);
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

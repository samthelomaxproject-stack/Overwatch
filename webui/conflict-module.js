// Conflict module for Overwatch Hub
// Fetches OSINT conflict events from /api/conflict/events

window.initConflictModule = async function initConflictModule(map, options = {}) {
  if (!window.L || !map) return null;

  const apiBase = (options.apiBase || '').replace(/\/$/, '');
  const markerLayer = (window.L.markerClusterGroup ? window.L.markerClusterGroup() : window.L.layerGroup());
  map.addLayer(markerLayer);

  const state = {
    visible: false,
    windowRange: 'week',
    lastLoadedAt: 0,
  };

  async function load() {
    const url = `${apiBase || 'http://127.0.0.1:8790'}/api/conflict/events?window=${state.windowRange}`;
    const res = await fetch(url);
    if (!res.ok) throw new Error(`API returned ${res.status}`);
    
    const data = await res.json();
    const events = data.items || [];
    
    markerLayer.clearLayers();
    
    events.forEach(ev => {
      if (!ev.lat || !ev.lon) return;
      
      const colorMap = {
        conflict: '#ef4444',
        protest: '#f59e0b',
        strike: '#eab308',
        military_activity: '#7c2d12',
        disaster: '#b91c1c',
        security_incident: '#dc2626',
        other: '#6b7280'
      };
      
      const color = colorMap[ev.event_type] || '#6b7280';
      const m = window.L.circleMarker([ev.lat, ev.lon], {
        radius: 7,
        color,
        fillColor: color,
        fillOpacity: 0.6,
        weight: 2,
      });
      
      const published = ev.published_at ? new Date(ev.published_at).toLocaleString() : 'Unknown';
      
      m.bindPopup(`
        <div style="min-width:280px">
          <b>${ev.title || 'Untitled'}</b><br/>
          <small style="color:#94a3b8;">${ev.event_type || 'other'} • ${ev.location || 'Unknown'}</small><br/><br/>
          <b>Summary:</b> ${ev.summary || 'N/A'}<br/>
          <b>Source:</b> ${ev.source_name || ev.source_type || 'Unknown'}<br/>
          <b>Published:</b> ${published}<br/>
          ${ev.source_url ? `<a href="${ev.source_url}" target="_blank" style="color:#00d4ff;">View Source →</a>` : ''}
        </div>
      `);
      
      markerLayer.addLayer(m);
    });
    
    state.lastLoadedAt = Date.now();
  }

  async function setVisible(v) {
    state.visible = !!v;
    if (state.visible) {
      if (!map.hasLayer(markerLayer)) map.addLayer(markerLayer);
      if (state.lastLoadedAt === 0) await load();
    } else if (map.hasLayer(markerLayer)) {
      map.removeLayer(markerLayer);
    }
  }

  async function refreshIfVisible(force = false) {
    if (!state.visible) return;
    if (!force && Date.now() - state.lastLoadedAt < 60_000) return;
    await load();
  }

  return {
    setVisible,
    setFilters(filters = {}) {
      if (filters.windowRange) state.windowRange = filters.windowRange;
      if (typeof filters.country === 'string') state.country = filters.country;
      if (typeof filters.eventTypes === 'string') state.eventTypes = filters.eventTypes;
    },
    refreshIfVisible,
    load,
  };
};

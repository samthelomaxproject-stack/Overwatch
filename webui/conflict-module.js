// Initial conflict module scaffold for Overwatch Hub
// Expects backend endpoint: GET /api/events?window=1d|7d|30d

window.initConflictModule = async function initConflictModule(map, options = {}) {
  if (!window.L || !map) return null;

  const apiBase = (options.apiBase || '').replace(/\/$/, '');
  const markerLayer = (window.L.markerClusterGroup ? L.markerClusterGroup() : L.layerGroup());
  map.addLayer(markerLayer);

  const state = {
    visible: false,
    windowRange: 'week',  // day, week, month
    lastLoadedAt: 0,
  };

  function buildUrl() {
    const u = new URL((apiBase ? apiBase : '') + '/api/events', window.location.origin);
    if (state.windowRange === 'custom') {
      if (state.dateFrom) u.searchParams.set('date_from', state.dateFrom);
      if (state.dateTo) u.searchParams.set('date_to', state.dateTo);
      if (!state.dateFrom && !state.dateTo) u.searchParams.set('window', '1d');
    } else {
      u.searchParams.set('window', state.windowRange);
    }
    if (state.country) u.searchParams.set('country', state.country);
    if (state.eventTypes) u.searchParams.set('event_types', state.eventTypes);
    return u.toString();
  }

  async function load() {
    try {
      console.log(`Conflict load: fetching window=${state.windowRange}, apiBase=${apiBase}`);
      
      // Fetch conflict events (RSS/GDELT) from new persistent storage
      let conflictEvents = [];
      try {
        const url = `${apiBase || 'http://127.0.0.1:8790'}/api/conflict/events?window=${state.windowRange}&limit=500`;
        console.log(`Conflict fetch URL: ${url}`);
        const conflictRes = await fetch(url);
        if (conflictRes.ok) {
          const data = await conflictRes.json();
          conflictEvents = data.items || [];
          console.log(`Conflict: ${conflictEvents.length} events loaded`);
        } else {
          console.error(`Conflict API error: ${conflictRes.status}`);
          throw new Error(`API returned ${conflictRes.status}`);
        }
      } catch (e) {
        console.error('Conflict events fetch error:', e);
        throw e;
      }

      // Ensure layer is on map before adding markers
      if (!map.hasLayer(markerLayer)) {
        map.addLayer(markerLayer);
      }
      markerLayer.clearLayers();
    
    console.log(`Conflict: Rendering ${conflictEvents.length} events`);
    console.log('Leaflet available:', !!window.L, 'L.circleMarker:', !!window.L?.circleMarker);
    
    // Render conflict events
    let rendered = 0;
    for (const ev of conflictEvents) {
      try {
        if (!ev.lat || !ev.lon) {
          console.log(`Skipping event (no coords): ${ev.title}`);
          continue;
        }
        
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
        const m = L.circleMarker([ev.lat, ev.lon], {
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
            <small style="color:#94a3b8;">${ev.event_type || 'other'} • ${ev.location || 'Unknown location'}</small><br/><br/>
            <b>Summary:</b> ${ev.summary || 'N/A'}<br/>
            <b>Source:</b> ${ev.source_name || ev.source_type || 'Unknown'}<br/>
            <b>Published:</b> ${published}<br/>
            ${ev.source_url ? `<a href="${ev.source_url}" target="_blank" rel="noopener" style="color:#00d4ff;">View Source →</a>` : ''}
          </div>
        `);
        
        markerLayer.addLayer(m);
        rendered++;
      } catch (err) {
        console.error('Error rendering event:', ev, err);
      }
    }
    
    console.log(`Conflict: ${rendered} markers added to layer`);
    state.lastLoadedAt = Date.now();
    } catch (err) {
      console.error('Conflict load error:', err);
      console.error('Error stack:', err.stack);
      throw err; // Rethrow original error instead of wrapping it
    }
  }

  function setVisible(v) {
    state.visible = !!v;
    if (state.visible) {
      if (!map.hasLayer(markerLayer)) map.addLayer(markerLayer);
    } else if (map.hasLayer(markerLayer)) {
      map.removeLayer(markerLayer);
    }
  }

  async function refreshIfVisible(force = false) {
    if (!state.visible) return;
    if (!force && Date.now() - state.lastLoadedAt < 60_000) return;
    await load();
  }

  async function loadMeta() {
    const u = new URL((apiBase ? apiBase : '') + '/api/meta', window.location.origin).toString();
    const res = await fetch(u);
    if (!res.ok) throw new Error(`Conflict meta API error: ${res.status}`);
    return await res.json();
  }

  return {
    setVisible,
    setFilters(filters = {}) {
      if (filters.windowRange) state.windowRange = filters.windowRange;
      if (typeof filters.country === 'string') state.country = filters.country;
      if (typeof filters.eventTypes === 'string') state.eventTypes = filters.eventTypes;
      if (typeof filters.dateFrom === 'string') state.dateFrom = filters.dateFrom;
      if (typeof filters.dateTo === 'string') state.dateTo = filters.dateTo;
    },
    refreshIfVisible,
    load,
    loadMeta,
  };
};

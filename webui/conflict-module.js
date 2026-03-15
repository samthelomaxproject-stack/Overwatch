// Initial conflict module scaffold for Overwatch Hub
// Expects backend endpoint: GET /api/events?window=1d|7d|30d

window.initConflictModule = async function initConflictModule(map, options = {}) {
  if (!window.L || !map) return null;

  const apiBase = (options.apiBase || '').replace(/\/$/, '');
  const markerLayer = (window.L.markerClusterGroup ? L.markerClusterGroup() : L.layerGroup());
  map.addLayer(markerLayer);

  const state = {
    visible: false,
    windowRange: '1d',
    country: '',
    eventTypes: '',
    dateFrom: '',
    dateTo: '',
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
    const res = await fetch(buildUrl());
    if (!res.ok) throw new Error(`Conflict API error: ${res.status}`);
    const events = await res.json();

    markerLayer.clearLayers();
    for (const ev of events) {
      if (!ev.latitude || !ev.longitude) continue;
      const fatal = Number(ev.fatalities || 0);
      const color = fatal >= 10 ? '#ef4444' : '#f59e0b';
      const m = L.circleMarker([ev.latitude, ev.longitude], {
        radius: Math.max(5, Math.min(14, 4 + (fatal * 0.15))),
        color,
        weight: 1,
      });

      const sourceItems = (ev.sources || []).map(s => `<li>${s.url ? `<a href="${s.url}" target="_blank" rel="noopener">${s.name}</a>` : s.name}</li>`).join('')
        || '<li>No direct links available</li>';

      m.bindPopup(`
        <div style="min-width:280px">
          <b>${ev.event_type || 'Conflict event'}</b><br/>
          <small>${ev.event_date || ''} • ${ev.location || ''}, ${ev.country || ''}</small><br/><br/>
          <b>Actors:</b> ${ev.actor1 || 'N/A'} ${ev.actor2 ? ' vs ' + ev.actor2 : ''}<br/>
          <b>Fatalities:</b> ${ev.fatalities ?? 'Unknown'}<br/>
          <b>Notes:</b> ${ev.notes || 'N/A'}<br/>
          <b>Sources:</b><ul>${sourceItems}</ul>
        </div>
      `);

      markerLayer.addLayer(m);
    }
    state.lastLoadedAt = Date.now();
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

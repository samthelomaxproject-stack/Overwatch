// Initial conflict module scaffold for Overwatch Hub
// Expects backend endpoint: GET /api/events?window=1d|7d|30d

window.initConflictModule = async function initConflictModule(map) {
  if (!window.L || !map) return;

  const layer = L.markerClusterGroup();
  map.addLayer(layer);

  async function load(windowRange = '7d') {
    const res = await fetch(`/api/events?window=${encodeURIComponent(windowRange)}`);
    if (!res.ok) return;
    const events = await res.json();

    layer.clearLayers();
    for (const ev of events) {
      if (!ev.latitude || !ev.longitude) continue;
      const m = L.circleMarker([ev.latitude, ev.longitude], {
        radius: Math.max(5, Math.min(14, 4 + (Number(ev.fatalities || 0) * 0.15))),
        color: '#ef4444',
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

      layer.addLayer(m);
    }
  }

  return { load };
};

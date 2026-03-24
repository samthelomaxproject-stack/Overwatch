// Minimal Conflict layer for Overwatch Hub
window.initConflictModule = function(map, opts) {
  const base = (opts.apiBase || 'http://127.0.0.1:8790').replace(/\/$/, '');
  const layer = window.L.layerGroup();
  let visible = false;
  let markers = {};

  async function load() {
    const url = `${base}/api/conflict/events?window=week`;
    try {
      const res = await fetch(url);
      if (!res.ok) return;
      const json = await res.json();
      const items = json.items || json || [];
      
      layer.clearLayers();
      markers = {};
      
      items.forEach(ev => {
        const lat = Number(ev.lat || ev.latitude);
        const lon = Number(ev.lon || ev.longitude);
        if (!isFinite(lat) || !isFinite(lon)) return;
        
        const m = window.L.circleMarker([lat, lon], {
          radius: 6,
          color: '#ef4444',
          fillColor: '#ef4444',
          fillOpacity: 0.6,
          weight: 2
        });
        
        m.bindPopup(`<b>${ev.title || 'Event'}</b><br>${ev.location || ''}`);
        layer.addLayer(m);
        markers[ev.id || Math.random()] = m;
      });
    } catch(e) {}
  }

  function setVisible(v) {
    visible = !!v;
    if (visible) {
      if (!map.hasLayer(layer)) map.addLayer(layer);
      if (Object.keys(markers).length === 0) load();
    } else {
      if (map.hasLayer(layer)) map.removeLayer(layer);
    }
  }

  return {
    setVisible,
    setFilters() {},
    refreshIfVisible(force) { if (visible || force) return load(); },
    load
  };
};

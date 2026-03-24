// Minimal Conflict layer for Overwatch Hub
window.initConflictModule = function(map, opts) {
  const base = (opts.apiBase || 'http://127.0.0.1:8790').replace(/\/$/, '');
  const layer = window.L.layerGroup();
  let visible = false;
  let markers = {};

  async function load() {
    const url = `${base}/api/conflict/events?window=week`;
    const res = await fetch(url);
    if (!res.ok) return;
    const json = await res.json();
    const items = json.items || json || [];
    
    layer.clearLayers();
    markers = {};
    
    let created = 0;
    items.forEach(ev => {
      const lat = Number(ev.lat || ev.latitude);
      const lon = Number(ev.lon || ev.longitude);
      if (!isFinite(lat) || !isFinite(lon)) return;
      
      const m = window.L.marker([lat, lon]);
      m.bindPopup(`<b>${ev.title || 'Event'}</b><br>${ev.location || ''}`);
      layer.addLayer(m);
      markers[ev.id || Math.random()] = m;
      created++;
    });
    
    if (visible && !map.hasLayer(layer)) map.addLayer(layer);
  }

  async function setVisible(v) {
    visible = !!v;
    if (visible) {
      await load();
      if (!map.hasLayer(layer)) map.addLayer(layer);
    } else {
      if (map.hasLayer(layer)) map.removeLayer(layer);
    }
  }

  return {
    setVisible,
    setFilters() {},
    refreshIfVisible(force) { if (visible || force) return load(); },
    load,
    loadMeta() { return Promise.resolve({}); }
  };
};

package ai.overwatch.android

import android.Manifest
import android.annotation.SuppressLint
import android.content.pm.PackageManager
import android.location.Location
import android.location.LocationManager
import android.os.Bundle
import android.webkit.GeolocationPermissions
import android.webkit.WebChromeClient
import android.webkit.WebView
import android.widget.Toast
import androidx.appcompat.app.AppCompatActivity
import androidx.core.app.ActivityCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat

class TacticalMapActivity : AppCompatActivity() {

    private lateinit var webView: WebView

    @SuppressLint("SetJavaScriptEnabled")
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableImmersiveMode()

        webView = WebView(this)
        setContentView(webView)

        val hub = intent.getStringExtra(EXTRA_HUB_URL)?.trim().orEmpty()
        val callsign = intent.getStringExtra(EXTRA_CALLSIGN)?.trim().orEmpty().ifEmpty { "ANDROID-EUD" }
        val pliMode = intent.getStringExtra(EXTRA_PLI_MODE)?.trim().orEmpty().ifEmpty { "COP" }
        val pullEntities = intent.getBooleanExtra(EXTRA_PULL_ENTITIES, true)
        val pullHeat = intent.getBooleanExtra(EXTRA_PULL_HEAT, true)
        val pullCams = intent.getBooleanExtra(EXTRA_PULL_CAMS, false)
        val pullSat = intent.getBooleanExtra(EXTRA_PULL_SAT, false)

        val baseUrl = normalizeHubBase(hub)
        val loc = getBestLocation()
        val initLat = loc?.latitude ?: 32.7767
        val initLon = loc?.longitude ?: -96.7970

        supportActionBar?.title = "Tactical Map"
        supportActionBar?.subtitle = "$callsign • $baseUrl"

        webView.settings.javaScriptEnabled = true
        webView.settings.domStorageEnabled = true
        webView.settings.useWideViewPort = true
        webView.settings.loadWithOverviewMode = true
        webView.settings.allowFileAccess = true
        webView.settings.allowContentAccess = true
        webView.settings.setGeolocationEnabled(true)

        webView.webChromeClient = object : WebChromeClient() {
            override fun onGeolocationPermissionsShowPrompt(origin: String?, callback: GeolocationPermissions.Callback?) {
                callback?.invoke(origin, true, false)
            }
        }

        val html = tacticalHtml(callsign, baseUrl, pliMode, pullEntities, pullHeat, pullCams, pullSat, initLat, initLon)
        runCatching { webView.loadDataWithBaseURL(baseUrl, html, "text/html", "utf-8", null) }
            .onFailure { Toast.makeText(this, "Failed to open tactical map: ${it.message}", Toast.LENGTH_LONG).show() }
    }

    private fun normalizeHubBase(hubUrl: String): String {
        val fallback = "http://192.168.1.143:8789/"
        if (!hubUrl.startsWith("http://") && !hubUrl.startsWith("https://")) return fallback
        return hubUrl.trimEnd('/') + "/"
    }

    private fun getBestLocation(): Location? {
        val lm = getSystemService(LOCATION_SERVICE) as LocationManager
        if (ActivityCompat.checkSelfPermission(this, Manifest.permission.ACCESS_FINE_LOCATION) != PackageManager.PERMISSION_GRANTED &&
            ActivityCompat.checkSelfPermission(this, Manifest.permission.ACCESS_COARSE_LOCATION) != PackageManager.PERMISSION_GRANTED
        ) return null

        runCatching { lm.getLastKnownLocation(LocationManager.GPS_PROVIDER) }.getOrNull()?.let { return it }
        val providers = runCatching { lm.getProviders(true) }.getOrDefault(emptyList())
        var best: Location? = null
        for (p in providers) {
            val loc = runCatching { lm.getLastKnownLocation(p) }.getOrNull() ?: continue
            if (best == null || loc.accuracy < best!!.accuracy) best = loc
        }
        return best
    }

    private fun tacticalHtml(
        callsign: String,
        hubBase: String,
        pliMode: String,
        pullEntities: Boolean,
        pullHeat: Boolean,
        pullCams: Boolean,
        pullSat: Boolean,
        initLat: Double,
        initLon: Double
    ): String {
        val callsignJs = org.json.JSONObject.quote(callsign)
        val pliModeJs = org.json.JSONObject.quote(pliMode)
        val pullEntitiesJs = if (pullEntities) "true" else "false"
        val pullHeatJs = if (pullHeat) "true" else "false"
        val pullCamsJs = if (pullCams) "true" else "false"
        val pullSatJs = if (pullSat) "true" else "false"

        return """
<!doctype html>
<html>
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>Overwatch EUD Tactical Map</title>
  <link rel="stylesheet" href="https://unpkg.com/leaflet@1.9.4/dist/leaflet.css" />
  <style>
    html, body, #map { height: 100%; margin: 0; background: #0b1220; }
    .hud { position: fixed; top: 8px; left: 8px; z-index: 9999; background: rgba(11,18,32,0.85); color: #cbd5e1; border: 1px solid rgba(255,255,255,0.12); border-radius: 8px; font: 12px monospace; padding: 8px 10px; max-width: 64vw; }
    .sidebar { position: fixed; top: 8px; right: 8px; z-index: 9999; width: min(86vw, 280px); max-height: calc(100vh - 16px); overflow-y: auto; background: rgba(11,18,32,0.9); color: #cbd5e1; border: 1px solid rgba(255,255,255,0.12); border-radius: 8px; font: 12px monospace; padding: 10px; }
    .sb-row { margin-bottom: 8px; }
    .sb-label { font-size: 11px; color: #94a3b8; margin-bottom: 4px; }
    .sb-input { width: 100%; box-sizing: border-box; background: #0f172a; color: #e2e8f0; border: 1px solid #334155; border-radius: 4px; padding: 6px; }
    .sb-btn { width: 100%; margin-top: 6px; background: #1e293b; color: #e2e8f0; border: 1px solid #334155; border-radius: 4px; padding: 6px; }
  </style>
</head>
<body>
  <div id="map"></div>
  <div class="hud">
    <div>● EUD Tactical Map • ${callsign}</div>
    <div id="status">Connecting…</div>
    <div id="count">Entities: 0</div>
    <div id="cmp">Compare: booting…</div>
    <div id="diag">PLI: n/a</div>
  </div>
  <div class="sidebar">
    <div class="sb-row"><div class="sb-label">Callsign</div><input id="cfgCallsign" class="sb-input" value="${callsign}" readonly /></div>
    <div class="sb-row"><div class="sb-label">Hub</div><input id="cfgHub" class="sb-input" value="${hubBase}" readonly /></div>
    <div class="sb-row"><div class="sb-label">PLI Mode (settings)</div><input class="sb-input" value="${pliMode}" readonly /></div>
    <div class="sb-row"><div class="sb-label">Map Type</div>
      <select id="mapType" class="sb-input" onchange="setMapType(this.value)">
        <option value="dark">Dark</option><option value="sat">Satellite</option><option value="topo">Topo</option>
      </select>
    </div>
    <div class="sb-row"><div class="sb-label">Layer Visibility</div>
      <label><input id="layerEntities" type="checkbox" checked onchange="applyLayerVisibility()" /> Entities</label>
      <label><input id="layerHeat" type="checkbox" checked onchange="applyLayerVisibility()" /> Heatmaps</label>
      <label><input id="layerCams" type="checkbox" onchange="applyLayerVisibility()" /> Cameras</label>
      <label><input id="layerSat" type="checkbox" onchange="applyLayerVisibility()" /> Satellites</label>
    </div>
    <div class="sb-label">PLI pull configured on main screen.</div>
    <button class="sb-btn" onclick="focusOwn()">Focus EUD</button>
    <button class="sb-btn" onclick="reloadDelta()">Reconnect Hub</button>
  </div>


  <button id="msgFab" onclick="toggleMsgPanel()" style="position:fixed;right:18px;bottom:18px;z-index:10001;width:54px;height:54px;border-radius:50%;border:1px solid #334155;background:rgba(15,23,42,0.95);color:#e2e8f0;font-size:22px;">✉<span id="msgBadge" style="display:none;position:absolute;right:8px;top:8px;width:10px;height:10px;border-radius:50%;background:#22c55e;"></span></button>

  <div id="msgPanel" style="display:none;position:fixed;right:18px;bottom:82px;z-index:10001;width:min(92vw,320px);max-height:55vh;overflow-y:auto;background:rgba(11,18,32,0.96);border:1px solid rgba(255,255,255,0.2);border-radius:12px;padding:10px;">
    <div style="font:12px monospace;color:#cbd5e1;margin-bottom:6px;">Messaging</div>
    <div class="sb-label">Target Type</div>
    <select id="msgType" class="sb-input" onchange="refreshMsgTargets()"><option value="device">Individual</option><option value="group">Group</option></select>
    <div class="sb-label" style="margin-top:6px;">Target</div>
    <select id="msgTarget" class="sb-input"></select>
    <div class="sb-label" style="margin-top:6px;">Body</div>
    <input id="msgBody" class="sb-input" placeholder="Message" />
    <button class="sb-btn" onclick="sendPanelMessage()">Send</button>
    <button class="sb-btn" onclick="pollInbox()">Refresh Inbox</button>
    <div id="msgInfo" style="font:11px monospace;color:#93c5fd;margin-top:6px;">Ready</div>
    <div id="msgInbox" style="margin-top:8px;max-height:180px;overflow-y:auto;border:1px solid #334155;border-radius:6px;padding:6px;font:11px monospace;color:#e2e8f0;"></div>
  </div>

  <div id="entityWheel" style="display:none;position:fixed;z-index:10000;background:rgba(11,18,32,0.96);border:1px solid rgba(255,255,255,0.2);border-radius:12px;padding:8px;min-width:180px;">
    <div id="wheelTitle" style="font:12px monospace;color:#cbd5e1;margin-bottom:6px;">Entity</div>
    <button class="sb-btn" onclick="wheelDirectMessage()">Direct Message</button>
    <button class="sb-btn" onclick="wheelAddToGroup()">Add to Group</button>
    <button class="sb-btn" onclick="hideEntityWheel()">Close</button>
  </div>

  <script src="https://unpkg.com/leaflet@1.9.4/dist/leaflet.js"></script>
  <script src="https://unpkg.com/h3-js@4.1.0/dist/h3-js.umd.js"></script>
  <script>
    const OWN_CALLSIGN = $callsignJs;
    const PLI_MODE = $pliModeJs;
    const PULL_ENTITIES = $pullEntitiesJs;
    const PULL_HEAT = $pullHeatJs;
    const PULL_CAMS = $pullCamsJs;
    const PULL_SAT = $pullSatJs;

    const map = L.map('map').setView([$initLat, $initLon], 15);
    let wheelSelectedId = null;
    let msgGroups = [];
    let msgUnread = 0;
    let msgAfterId = 0;

    function renderMsgBadge() {
      const b = document.getElementById('msgBadge');
      if (!b) return;
      b.style.display = msgUnread > 0 ? 'block' : 'none';
    }

    function toggleMsgPanel() {
      const p = document.getElementById('msgPanel');
      if (!p) return;
      p.style.display = (p.style.display === 'none' || !p.style.display) ? 'block' : 'none';
      if (p.style.display === 'block') {
        refreshMsgGroups();
        refreshMsgTargets();
        pollInbox(true);
      }
    }

    async function refreshMsgGroups() {
      const hub = (document.getElementById('cfgHub')?.value || '').trim().replace(/\/$/, '');
      try {
        const r = await fetch(`${'$'}{hub}/api/msg/groups?device_id=${'$'}{encodeURIComponent(ownCallsign())}`);
        if (!r.ok) return;
        msgGroups = await r.json();
      } catch (_) {}
    }

    function refreshMsgTargets() {
      const sel = document.getElementById('msgTarget');
      const type = document.getElementById('msgType')?.value || 'device';
      if (!sel) return;
      if (type === 'group') {
        const groups = (msgGroups || []).map(g => ({id:g.group_id, label:`${'$'}{g.name} (${'$'}{g.group_id})`}));
        sel.innerHTML = groups.map(g => `<option value="${'$'}{g.id}">${'$'}{g.label}</option>`).join('');
      } else {
        const ids = Object.keys(markers).filter(x => x && x !== ownCallsign());
        sel.innerHTML = ids.map(id => `<option value="${'$'}{id}">${'$'}{id}</option>`).join('');
      }
    }

    async function sendPanelMessage() {
      const hub = (document.getElementById('cfgHub')?.value || '').trim().replace(/\/$/, '');
      const type = document.getElementById('msgType')?.value || 'device';
      const target = document.getElementById('msgTarget')?.value || '';
      const body = document.getElementById('msgBody')?.value || '';
      const info = document.getElementById('msgInfo');
      if (!target || !body) { if (info) info.textContent = 'Target/body required'; return; }
      const req = { from: ownCallsign(), body };
      if (type === 'group') req.to_group = target; else req.to_device = target;
      try {
        const r = await fetch(`${'$'}{hub}/api/msg/send`, { method:'POST', headers:{'Content-Type':'application/json'}, body: JSON.stringify(req) });
        if (!r.ok) throw new Error(`HTTP ${'$'}{r.status}`);
        if (info) info.textContent = `Sent -> ${'$'}{target}`;
        const b = document.getElementById('msgBody'); if (b) b.value = '';
      } catch (e) {
        if (info) info.textContent = `Send failed: ${'$'}{e}`;
      }
    }

    async function pollInbox(markRead = false) {
      const hub = (document.getElementById('cfgHub')?.value || '').trim().replace(/\/$/, '');
      const info = document.getElementById('msgInfo');
      const box = document.getElementById('msgInbox');
      try {
        const r = await fetch(`${'$'}{hub}/api/msg/inbox?device_id=${'$'}{encodeURIComponent(ownCallsign())}&after_id=${'$'}{msgAfterId}&limit=100`);
        if (!r.ok) throw new Error(`HTTP ${'$'}{r.status}`);
        const arr = await r.json();
        if (!Array.isArray(arr)) return;
        let maxId = msgAfterId;
        const lines = [];
        for (const m of arr) {
          maxId = Math.max(maxId, Number(m.id || 0));
          lines.push(`from ${'$'}{m.from}: ${'$'}{m.body}`);
          if (markRead) {
            try {
              await fetch(`${'$'}{hub}/api/msg/ack`, { method:'POST', headers:{'Content-Type':'application/json'}, body: JSON.stringify({ device_id: ownCallsign(), id: m.id }) });
            } catch (_) {}
          }
        }
        if (arr.length > 0) {
          msgAfterId = maxId;
          if (!markRead) msgUnread += arr.length;
        }
        if (markRead) msgUnread = 0;
        renderMsgBadge();
        if (box && lines.length) {
          const existing = box.innerText || '';
          box.innerText = `${'$'}{existing}${'$'}{existing ? '\n' : ''}${'$'}{lines.join('\n')}`.trim();
          box.scrollTop = box.scrollHeight;
        }
        if (info) info.textContent = `Inbox ${'$'}{arr.length ? 'updated' : 'no new messages'}`;
      } catch (e) {
        if (info) info.textContent = `Inbox failed: ${'$'}{e}`;
      }
    }

    function hideEntityWheel() {
      const el = document.getElementById('entityWheel');
      if (el) el.style.display = 'none';
      wheelSelectedId = null;
    }

    function showEntityWheel(id, latlng) {
      wheelSelectedId = id;
      const p = map.latLngToContainerPoint(latlng);
      const wheel = document.getElementById('entityWheel');
      const title = document.getElementById('wheelTitle');
      if (!wheel || !title) return;
      title.textContent = `Entity: ${'$'}{id}`;
      wheel.style.left = `${'$'}{Math.max(8, p.x + 12)}px`;
      wheel.style.top = `${'$'}{Math.max(8, p.y - 10)}px`;
      wheel.style.display = 'block';
    }

    async function postJson(url, payload) {
      const r = await fetch(url, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(payload) });
      if (!r.ok) throw new Error(`HTTP ${'$'}{r.status}`);
      return await r.text();
    }

    async function wheelDirectMessage() {
      if (!wheelSelectedId) return;
      const body = prompt(`Message to ${'$'}{wheelSelectedId}:`);
      if (!body) return;
      const hub = (document.getElementById('cfgHub')?.value || '').trim().replace(/\/$/, '');
      try {
        await postJson(`${'$'}{hub}/api/msg/send`, { from: ownCallsign(), to_device: wheelSelectedId, body });
        document.getElementById('status').textContent = `DM sent -> ${'$'}{wheelSelectedId}`;
        hideEntityWheel();
      } catch (e) {
        document.getElementById('status').textContent = `DM failed: ${'$'}{e}`;
      }
    }

    async function wheelAddToGroup() {
      if (!wheelSelectedId) return;
      const gid = prompt(`Group ID for ${'$'}{wheelSelectedId}:`);
      if (!gid) return;
      const hub = (document.getElementById('cfgHub')?.value || '').trim().replace(/\/$/, '');
      try {
        await postJson(`${'$'}{hub}/api/msg/group/join`, { group_id: gid, name: gid, device_id: wheelSelectedId });
        document.getElementById('status').textContent = `${'$'}{wheelSelectedId} added to ${'$'}{gid}`;
        refreshMsgGroups();
        hideEntityWheel();
      } catch (e) {
        document.getElementById('status').textContent = `Group add failed: ${'$'}{e}`;
      }
    }

    map.on('click', () => hideEntityWheel());

    const layerDark = L.tileLayer('https://{s}.basemaps.cartocdn.com/dark_all/{z}/{x}/{y}{r}.png', { maxZoom: 19, attribution: '&copy; OpenStreetMap &copy; CARTO' });
    const layerSat = L.tileLayer('https://server.arcgisonline.com/ArcGIS/rest/services/World_Imagery/MapServer/tile/{z}/{y}/{x}', { maxZoom: 19, attribution: 'Tiles &copy; Esri' });
    const layerTopo = L.tileLayer('https://{s}.tile.openstreetmap.org/{z}/{x}/{y}.png', { maxZoom: 19, attribution: '&copy; OpenStreetMap' });
    const baseLayers = { dark: layerDark, sat: layerSat, topo: layerTopo };
    let activeBase = 'dark';
    baseLayers[activeBase].addTo(map);

    function setMapType(kind) { if (!baseLayers[kind] || kind === activeBase) return; map.removeLayer(baseLayers[activeBase]); baseLayers[kind].addTo(map); activeBase = kind; }
    window.setMapType = setMapType;

    let cursor = 0;
    const markers = {};
    const heatLayers = {};
    const camLayers = {};
    const satLayers = {};

    let centeredOnOwn = false;
    let ownGps = { lat: $initLat, lon: $initLon };
    let lastPliIds = [];
    let lastPliRxMs = 0;

    function ensureOwnMarker() {
      // Always maintain a local marker from device GPS. Dedupe is handled by same callsign key.
      const id = ownCallsign();
      const lat = ownGps.lat, lon = ownGps.lon;
      upsertMarker(id, lat, lon, 'local');
      localRxMs = Date.now();

      // Local heat point (single-node immediate awareness); merged with COP by unique key.
      const hk = 'local:' + id;
      let h = heatLayers[hk];
      if (!h) {
        h = L.circleMarker([lat, lon], { radius: 16, color: '#22c55e', weight: 1, fillColor: '#22c55e', fillOpacity: 0.25 }).addTo(map);
        h.bindPopup('LOCAL HEAT');
        heatLayers[hk] = h;
      } else {
        h.setLatLng([lat, lon]);
        if (!map.hasLayer(h)) h.addTo(map);
      }

      if (!centeredOnOwn) { map.setView([lat, lon], 16); centeredOnOwn = true; }
    }

    function ownCallsign() { return OWN_CALLSIGN || 'ANDROID-EUD'; }
    function updateCompare() {
      const cmp = document.getElementById('cmp');
      if (!cmp) return;
      if (ownGps) cmp.textContent = `EUD ${'$'}{ownGps.lat.toFixed(5)}, ${'$'}{ownGps.lon.toFixed(5)} • mode ${'$'}{PLI_MODE}`;
      else cmp.textContent = 'Compare: waiting for local GPS…';
    }
    function focusOwn() { const m = markers[ownCallsign()]; if (m) map.setView(m.getLatLng(), 16); }
    function reloadDelta() { cursor = 0; document.getElementById('status').textContent = 'Reconnecting hub delta…'; pollDelta(); }

    function parseAnyTile(tileId) {
      // 1) Android quantized tile id
      const a = parseAndroidTile(tileId);
      if (a) return a;

      // 2) H3 index tile id from hub COP heat/rf/wifi layers
      try {
        const h = String(tileId || '').trim();
        if (!h) return null;
        const h3 = window.h3 || window.h3js || null;
        if (!h3) return null;

        // h3-js v4: cellToLatLng ; v3: h3ToGeo
        if (typeof h3.cellToLatLng === 'function') {
          const [lat, lon] = h3.cellToLatLng(h);
          if (Number.isFinite(lat) && Number.isFinite(lon)) return { lat, lon };
        }
        if (typeof h3.h3ToGeo === 'function') {
          const [lat, lon] = h3.h3ToGeo(h);
          if (Number.isFinite(lat) && Number.isFinite(lon)) return { lat, lon };
        }
      } catch (_) {}

      return null;
    }

    function parseAndroidTile(tileId) {
      if (!tileId || !tileId.startsWith('android_')) return null;
      const parts = tileId.split('_'); if (parts.length !== 3) return null;
      const latQ = parseInt(parts[1], 10), lonQ = parseInt(parts[2], 10);
      if (!Number.isFinite(latQ) || !Number.isFinite(lonQ)) return null;
      const divisor = (Math.abs(latQ) > 9000 || Math.abs(lonQ) > 9000) ? 10000.0 : 100.0;
      return { lat: latQ / divisor, lon: lonQ / divisor };
    }

    function applyLayerVisibility() {
      const showEntities = document.getElementById('layerEntities')?.checked !== false;
      const showHeat = document.getElementById('layerHeat')?.checked !== false;
      const showCams = document.getElementById('layerCams')?.checked !== false;
      const showSat = document.getElementById('layerSat')?.checked !== false;

      Object.values(markers).forEach(m => {
        if (showEntities) { if (!map.hasLayer(m)) m.addTo(map); }
        else { if (map.hasLayer(m)) map.removeLayer(m); }
      });
      Object.values(heatLayers).forEach(l => {
        if (showHeat) { if (!map.hasLayer(l)) l.addTo(map); }
        else { if (map.hasLayer(l)) map.removeLayer(l); }
      });
      Object.values(camLayers).forEach(l => {
        if (showCams) { if (!map.hasLayer(l)) l.addTo(map); }
        else { if (map.hasLayer(l)) map.removeLayer(l); }
      });
      Object.values(satLayers).forEach(l => {
        if (showSat) { if (!map.hasLayer(l)) l.addTo(map); }
        else { if (map.hasLayer(l)) map.removeLayer(l); }
      });
    }
    window.applyLayerVisibility = applyLayerVisibility;

    const entityLast = {};
    function idJitter(id) {
      let h = 0;
      const s = String(id || '');
      for (let i = 0; i < s.length; i++) h = ((h << 5) - h + s.charCodeAt(i)) | 0;
      const a = (h & 0xffff) / 65535.0 * Math.PI * 2;
      return { dLat: Math.sin(a) * 0.00012, dLon: Math.cos(a) * 0.00012 };
    }

    function upsertMarker(id, lat, lon, sourceType) {
      if (Math.abs(lat) < 0.2 && Math.abs(lon) < 0.2) return;
      const isOwn = String(id||'').toUpperCase() === String(ownCallsign()||'').toUpperCase();
      const j = idJitter(id);
      const rLat = lat + j.dLat;
      const rLon = lon + j.dLon;
      const color = isOwn ? '#22c55e' : (sourceType === 'drone' ? '#f97316' : '#60a5fa');
      const prev = entityLast[id]; let heading = 0;
      if (prev) { const dy = lat - prev.lat, dx = lon - prev.lon; if (Math.abs(dx)+Math.abs(dy) > 0.00001) heading = ((Math.atan2(dx, dy) * 180 / Math.PI) + 360) % 360; }
      entityLast[id] = { lat, lon };

      const icon = L.divIcon({ className: 'eud-marker', html: `<div style="position:relative;width:22px;height:22px;"><div style="position:absolute;left:3px;top:3px;width:16px;height:16px;border-radius:50%;background:${'$'}{color};border:2px solid #fff;box-shadow:0 0 10px rgba(0,0,0,0.65);"></div><div style="position:absolute;left:9px;top:-1px;width:0;height:0;border-left:3px solid transparent;border-right:3px solid transparent;border-bottom:7px solid #fff;transform:rotate(${'$'}{heading}deg);transform-origin:50% 12px;"></div></div>`, iconSize:[22,22], iconAnchor:[11,11] });

      if (!markers[id]) markers[id] = L.marker([rLat, rLon], { icon }).addTo(map);
      else { markers[id].setLatLng([rLat, rLon]); markers[id].setIcon(icon); }
      markers[id]
        .bindPopup(`<b>${'$'}{id}</b><br/>${'$'}{sourceType || 'unknown'}<br/>${'$'}{lat.toFixed(5)}, ${'$'}{lon.toFixed(5)}`)
        .bindTooltip(id, { permanent: true, direction: 'top', offset: [0,-12] });
      markers[id].off('click');
      markers[id].on('click', (e) => {
        showEntityWheel(id, e.latlng || markers[id].getLatLng());
      });

      applyLayerVisibility();
      if (isOwn) {
        ownGps = { lat, lon };
        updateCompare();
      }
      if (isOwn && !centeredOnOwn) { map.setView([lat, lon], 16); centeredOnOwn = true; }
    }

    function renderCopLayers(snap) {
      const heat = snap?.heat || [];
      const cams = snap?.cameras || [];
      const sats = snap?.satellites || [];
      copRxMs = Date.now();

      const nextHeat = {};
      for (const h of heat) {
        const p = parseAnyTile(h.tile_id); if (!p) continue;
        const key = `${'$'}{h.tile_id}:${'$'}{h.dimension || ''}`;
        const intensity = Math.max(0.1, Math.min(1.0, Number(h.max || h.mean || 0) / 100.0));
        const radius = 18 + Math.round(intensity * 26);
        const color = (h.sensor_type === 'wifi') ? '#22d3ee' : '#f97316';
        let c = heatLayers[key];
        if (!c) {
          c = L.circleMarker([p.lat, p.lon], { radius, color, weight: 1, fillColor: color, fillOpacity: 0.28 + intensity * 0.35 }).addTo(map);
          c.bindPopup(`HEAT ${'$'}{h.sensor_type || ''} ${'$'}{h.dimension || ''}<br/>max:${'$'}{h.max} mean:${'$'}{h.mean}`);
          heatLayers[key] = c;
        } else {
          c.setLatLng([p.lat, p.lon]);
          c.setStyle({ radius, color, fillColor: color, fillOpacity: 0.28 + intensity * 0.35 });
          if (!map.hasLayer(c)) c.addTo(map);
        }
        nextHeat[key] = true;
      }
      Object.keys(heatLayers).forEach(k => {
        if (k.startsWith('local:')) return;
        if (!nextHeat[k]) { if (map.hasLayer(heatLayers[k])) map.removeLayer(heatLayers[k]); delete heatLayers[k]; }
      });

      const nextCam = {};
      for (const c of cams) {
        const p = parseAnyTile(c.tile_id); if (!p) continue;
        const key = `${'$'}{c.tile_id}:${'$'}{c.dimension || ''}`;
        let m = camLayers[key];
        const icon = L.divIcon({ className:'cop-cam', html:'<div style="width:10px;height:10px;border-radius:50%;background:#22c55e;border:2px solid #fff"></div>', iconSize:[10,10], iconAnchor:[5,5] });
        if (!m) {
          m = L.marker([p.lat,p.lon], { icon }).addTo(map);
          m.bindPopup(`CAM ${'$'}{c.dimension || ''}<br/>count:${'$'}{c.count || 0}`);
          camLayers[key] = m;
        } else {
          m.setLatLng([p.lat,p.lon]);
          if (!map.hasLayer(m)) m.addTo(map);
        }
        nextCam[key] = true;
      }
      Object.keys(camLayers).forEach(k => { if (!nextCam[k]) { if (map.hasLayer(camLayers[k])) map.removeLayer(camLayers[k]); delete camLayers[k]; } });

      const nextSat = {};
      for (const t of sats) {
        const p = parseAnyTile(t.tile_id); if (!p) continue;
        const key = `${'$'}{t.tile_id}:${'$'}{t.dimension || ''}`;
        let m = satLayers[key];
        const icon = L.divIcon({ className:'cop-sat', html:'<div style="width:10px;height:10px;border-radius:50%;background:#ffea00;border:2px solid #111"></div>', iconSize:[10,10], iconAnchor:[5,5] });
        if (!m) {
          m = L.marker([p.lat,p.lon], { icon }).addTo(map);
          m.bindPopup(`SAT ${'$'}{t.dimension || ''}<br/>count:${'$'}{t.count || 0}`);
          satLayers[key] = m;
        } else {
          m.setLatLng([p.lat,p.lon]);
          if (!map.hasLayer(m)) m.addTo(map);
        }
        nextSat[key] = true;
      }
      Object.keys(satLayers).forEach(k => { if (!nextSat[k]) { if (map.hasLayer(satLayers[k])) map.removeLayer(satLayers[k]); delete satLayers[k]; } });

      applyLayerVisibility();
    }

    async function pollDelta() {
      const statusEl = document.getElementById('status');
      const countEl = document.getElementById('count');

      // LOCAL mode = show only this EUD local marker and skip COP pulls.
      if (PLI_MODE === 'LOCAL') {
        ensureOwnMarker();
        statusEl.textContent = 'LOCAL mode • COP pull disabled';
        countEl.textContent = `Entities: ${'$'}{markers[ownCallsign()] ? 1 : 0} • updates: local`;
        const diagEl = document.getElementById('diag');
        if (diagEl) {
          const la = localRxMs ? Math.floor((Date.now()-localRxMs)/1000) : -1;
          const localAgeTxt = (la >= 0) ? (la + 's') : 'n/a';
          diagEl.textContent = 'SRC local:' + localAgeTxt + ' cop:off • ids:' + ownCallsign();
        }
        applyLayerVisibility();
        return;
      }

      try {
        if (PLI_MODE === 'MERGED') ensureOwnMarker();
        const hub = (document.getElementById('cfgHub')?.value || '').trim().replace(/\/$/, '');
        let seen = 0;

        if (PULL_ENTITIES) {
          let ids = [];
          let pliOk = false;
          let pliSource = 'none';

          // COP snapshot feed (authoritative hub -> EUD entity view)
          try {
            if (PLI_MODE === 'COP' || PLI_MODE === 'MERGED') {
              const snapResp = await fetch(`${'$'}{hub}/api/cop_snapshot?max_age_secs=7200`);
              if (snapResp.ok) {
                const snap = await snapResp.json();
                (snap.entities || []).forEach(pt => {
                  const p = parseAnyTile(pt.tile_id); if (!p) return;
                  const id = pt.device_id || 'unknown';
                  const sourceType = pt.source_type || 'unknown';
                  if (sourceType === 'hub_local' || String(id).toLowerCase() === 'hub') return;
                  ids.push(id);
                  upsertMarker(id, p.lat, p.lon, sourceType); seen += 1;
                });
                renderCopLayers(snap);
                const diagEl = document.getElementById('diag');
                if (diagEl) {
                  diagEl.textContent = `COP ts:${'$'}{snap.ts || 'n/a'} ent:${'$'}{(snap.entities||[]).length} heat:${'$'}{(snap.heat||[]).length} cam:${'$'}{(snap.cameras||[]).length} sat:${'$'}{(snap.satellites||[]).length}`;
                }
                pliOk = true;
                pliSource = 'cop_snapshot';
              }
            }
          } catch (_) {}

          // Legacy/cursor feed fallback
          if (!pliOk) {
            try {
              const pliResp = await fetch(`${'$'}{hub}/api/pli_delta?device_id=${'$'}{encodeURIComponent(ownCallsign())}&cursor=${'$'}{cursor}&max_age_secs=7200`);
              if (pliResp.ok) {
                const pliDelta = await pliResp.json();
                cursor = pliDelta.cursor || cursor;
                (pliDelta.tiles || []).forEach(batch => {
                  (batch.tiles || []).forEach(pt => {
                    const p = parseAnyTile(pt.tile_id); if (!p) return;
                    const id = pt.device_id || batch.device_id || 'unknown';
                    const sourceType = pt.source_type || batch.source_type || 'unknown';
                    if (sourceType === 'hub_local' || String(id).toLowerCase() === 'hub') return;
                    ids.push(id);
                    upsertMarker(id, p.lat, p.lon, sourceType); seen += 1;
                  });
                });
                pliOk = true;
                pliSource = 'delta';
              }
            } catch (_) {}
          }

          // If delta has no ids, refresh from snapshot to avoid "appears then disappears" behavior.
          if (!pliOk || ids.length === 0) {
            try {
              const r = await fetch(`${'$'}{hub}/api/pli?max_age_secs=7200`);
              if (r.ok) {
                const pli = await r.json();
                ids = [];
                (pli || []).forEach(pt => {
                  const p = parseAnyTile(pt.tile_id); if (!p) return;
                  const id = pt.device_id || 'unknown';
                  const sourceType = pt.source_type || 'unknown';
                  if (sourceType === 'hub_local' || String(id).toLowerCase() === 'hub') return;
                  ids.push(id);
                  upsertMarker(id, p.lat, p.lon, sourceType); seen += 1;
                });
                pliOk = true;
                pliSource = 'snapshot';
              }
            } catch (_) {}
          }

          if (ids.length > 0) {
            lastPliIds = [...new Set(ids)];
            lastPliRxMs = Date.now();
          }

          const diagEl = document.getElementById('diag');
          if (diagEl) {
            const copAge = copRxMs ? Math.max(0, Math.floor((Date.now() - copRxMs) / 1000)) : -1;
            const localAge = localRxMs ? Math.max(0, Math.floor((Date.now() - localRxMs) / 1000)) : -1;
            const idText = lastPliIds.length ? lastPliIds.join(', ') : 'none';
            const localOnMap = !!markers[ownCallsign()];
            const localAgeTxt = (localAge >= 0) ? (localAge + 's') : 'n/a';
            const copAgeTxt = (copAge >= 0) ? (copAge + 's') : 'n/a';
            diagEl.textContent = pliOk
              ? ('SRC local:' + localAgeTxt + ' cop:' + copAgeTxt + ' via:' + pliSource + ' • ids:' + idText + ' • local:' + (localOnMap ? 'on' : 'off'))
              : ('SRC local:' + localAgeTxt + ' cop:fail • local:' + (localOnMap ? 'on' : 'off'));
          }
        }

        // Optional delta pull for non-entity layers.
        try {
          const q = `device_id=android-eud-map&cursor=${'$'}{cursor}&mode=${'$'}{encodeURIComponent(PLI_MODE)}&entities=0&heat=${'$'}{PULL_HEAT?1:0}&cams=${'$'}{PULL_CAMS?1:0}&sat=${'$'}{PULL_SAT?1:0}`;
          const resp = await fetch(`${'$'}{hub}/api/delta?${'$'}{q}`);
          if (resp.ok) {
            const data = await resp.json();
            cursor = Math.max(cursor, data.cursor || 0);
          }
        } catch (_) {
          // non-fatal
        }

        statusEl.textContent = `Connected • cursor ${'$'}{cursor}`;
        countEl.textContent = `Entities: ${'$'}{Object.keys(markers).length} • updates: ${'$'}{seen}`;
      } catch (e) {
        statusEl.textContent = `PLI pull error: ${'$'}{e}`;
      }
    }

    if (navigator.geolocation) {
      navigator.geolocation.watchPosition((pos) => {
        const lat = pos.coords.latitude, lon = pos.coords.longitude;
        ownGps = { lat, lon };
        ensureOwnMarker();
        updateCompare();
      }, (err) => {
        document.getElementById('status').textContent = `GPS fallback (${'$'}{err.message})`;
        updateCompare();
      }, { enableHighAccuracy: true, maximumAge: 2000, timeout: 10000 });
    }

    ensureOwnMarker();
    document.getElementById('status').textContent = 'Map loaded • connecting to hub delta…';
    updateCompare();
    renderMsgBadge();
    setInterval(() => pollInbox(false), 5000);
    setInterval(pollDelta, 3000);
    pollInbox(false);
    pollDelta();
  </script>
</body>
</html>
""".trimIndent()
    }

    private fun enableImmersiveMode() {
        WindowCompat.setDecorFitsSystemWindows(window, false)
        val controller = WindowInsetsControllerCompat(window, window.decorView)
        controller.systemBarsBehavior = WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
        controller.hide(WindowInsetsCompat.Type.systemBars())
    }

    override fun onWindowFocusChanged(hasFocus: Boolean) {
        super.onWindowFocusChanged(hasFocus)
        if (hasFocus) enableImmersiveMode()
    }

    companion object {
        const val EXTRA_HUB_URL = "extra_hub_url"
        const val EXTRA_CALLSIGN = "extra_callsign"
        const val EXTRA_PLI_MODE = "extra_pli_mode"
        const val EXTRA_PULL_ENTITIES = "extra_pull_entities"
        const val EXTRA_PULL_HEAT = "extra_pull_heat"
        const val EXTRA_PULL_CAMS = "extra_pull_cams"
        const val EXTRA_PULL_SAT = "extra_pull_sat"
    }
}

package ai.overwatch.android

import android.Manifest
import android.annotation.SuppressLint
import android.content.pm.PackageManager
import android.location.Location
import android.location.LocationManager
import android.os.Bundle
import android.webkit.GeolocationPermissions
import android.webkit.JavascriptInterface
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

    inner class Bridge {
        @JavascriptInterface
        fun getLocationJson(): String {
            val loc = getBestLocation() ?: return "{}"
            return "{\"lat\":${loc.latitude},\"lon\":${loc.longitude},\"acc\":${loc.accuracy}}"
        }
        
        @JavascriptInterface
        fun getDeviceCallsign(): String {
            return intent.getStringExtra(EXTRA_CALLSIGN)?.trim()?.ifEmpty { "ANDROID-EUD" } ?: "ANDROID-EUD"
        }
    }

    @SuppressLint("SetJavaScriptEnabled")
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableImmersiveMode()

        webView = WebView(this)
        setContentView(webView)

        val hub = intent.getStringExtra(EXTRA_HUB_URL)?.trim().orEmpty()
        val callsign = intent.getStringExtra(EXTRA_CALLSIGN)?.trim().orEmpty().ifEmpty { "ANDROID-EUD" }
        val pliMode = intent.getStringExtra(EXTRA_PLI_MODE)?.trim().orEmpty().ifEmpty { "LOCAL" }
        val pullEntities = intent.getBooleanExtra(EXTRA_PULL_ENTITIES, true)
        val pullHeat = intent.getBooleanExtra(EXTRA_PULL_HEAT, false)
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
        webView.settings.mediaPlaybackRequiresUserGesture = false

        webView.webChromeClient = object : WebChromeClient() {
            override fun onGeolocationPermissionsShowPrompt(origin: String?, callback: GeolocationPermissions.Callback?) {
                callback?.invoke(origin, true, false)
            }
        }
        webView.addJavascriptInterface(Bridge(), "AndroidBridge")

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
        val initLatJs = initLat.toString()
        val initLonJs = initLon.toString()

        return """
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0, maximum-scale=1.0, user-scalable=no">
    <title>Overwatch EUD Tactical Map</title>
    <link rel="stylesheet" href="https://unpkg.com/leaflet@1.9.4/dist/leaflet.css" />
    <style>
        :root {
            --tac-friendly: #00c851;
            --tac-hostile: #ff3333;
            --tac-neutral: #00d4ff;
            --tac-unknown: #ffcc00;
            --bg-panel: rgba(11, 18, 32, 0.95);
            --text-primary: #e2e8f0;
            --text-secondary: #94a3b8;
            --border-subtle: rgba(255, 255, 255, 0.12);
        }
        html, body, #map { height: 100%; margin: 0; background: #0b1220; overflow: hidden; }
        
        /* Tactical Symbols - Matching macOS */
        .tac-symbol {
            width: 32px;
            height: 32px;
            display: flex;
            align-items: center;
            justify-content: center;
            border: 2px solid;
            border-radius: 4px;
            font-size: 14px;
            font-weight: 700;
            font-family: monospace;
            background: rgba(0, 0, 0, 0.5);
        }
        .tac-friendly { border-color: var(--tac-friendly); color: var(--tac-friendly); background: rgba(0, 200, 81, 0.15); }
        .tac-hostile { border-color: var(--tac-hostile); color: var(--tac-hostile); background: rgba(255, 51, 51, 0.15); border-radius: 50%; }
        .tac-neutral { border-color: var(--tac-neutral); color: var(--tac-neutral); background: rgba(0, 212, 255, 0.15); }
        .tac-unknown { border-color: var(--tac-unknown); color: var(--tac-unknown); background: rgba(255, 204, 0, 0.15); }
        
        /* Own position marker */
        .my-location-marker {
            width: 20px;
            height: 20px;
            background: var(--tac-neutral);
            border: 3px solid white;
            border-radius: 50%;
            box-shadow: 0 0 15px var(--tac-neutral), 0 0 30px var(--tac-neutral);
            position: relative;
        }
        .my-location-marker::after {
            content: '';
            position: absolute;
            width: 40px;
            height: 40px;
            background: rgba(0, 212, 255, 0.2);
            border-radius: 50%;
            top: 50%;
            left: 50%;
            transform: translate(-50%, -50%);
            animation: pulse 2s infinite;
        }
        @keyframes pulse {
            0% { transform: translate(-50%, -50%) scale(1); opacity: 0.5; }
            100% { transform: translate(-50%, -50%) scale(1.5); opacity: 0; }
        }
        
        /* HUD */
        .hud {
            position: fixed;
            top: 8px;
            left: 8px;
            z-index: 9999;
            background: var(--bg-panel);
            border: 1px solid var(--border-subtle);
            border-radius: 8px;
            padding: 10px;
            color: var(--text-primary);
            font: 12px monospace;
            max-width: 280px;
            backdrop-filter: blur(8px);
        }
        .hud-title { font-weight: bold; margin-bottom: 6px; color: var(--tac-neutral); }
        .hud-row { margin: 4px 0; }
        .hud-label { color: var(--text-secondary); }
        
        /* Sidebar */
        .sidebar {
            position: fixed;
            top: 8px;
            right: 8px;
            z-index: 9999;
            width: 260px;
            max-height: calc(100vh - 16px);
            overflow-y: auto;
            background: var(--bg-panel);
            border: 1px solid var(--border-subtle);
            border-radius: 8px;
            padding: 12px;
            color: var(--text-primary);
            font: 12px monospace;
            backdrop-filter: blur(8px);
        }
        .sb-section { margin-bottom: 12px; }
        .sb-label { font-size: 11px; color: var(--text-secondary); margin-bottom: 4px; text-transform: uppercase; }
        .sb-input {
            width: 100%;
            box-sizing: border-box;
            background: rgba(15, 23, 42, 0.8);
            color: var(--text-primary);
            border: 1px solid #334155;
            border-radius: 4px;
            padding: 6px;
            font: 12px monospace;
        }
        .sb-btn {
            width: 100%;
            margin-top: 6px;
            background: rgba(30, 41, 59, 0.9);
            color: var(--text-primary);
            border: 1px solid #334155;
            border-radius: 4px;
            padding: 8px;
            cursor: pointer;
        }
        .sb-btn:hover { background: rgba(51, 65, 85, 0.9); border-color: var(--tac-neutral); }
        .layer-group { display: flex; flex-wrap: wrap; gap: 8px; }
        .layer-group label { display: flex; align-items: center; gap: 4px; cursor: pointer; }
        
        /* Entity List */
        .entity-list { max-height: 200px; overflow-y: auto; margin-top: 8px; }
        .entity-item {
            display: flex;
            align-items: center;
            gap: 8px;
            padding: 6px;
            background: rgba(15, 23, 42, 0.6);
            border-radius: 4px;
            margin-bottom: 4px;
            font-size: 11px;
        }
        .entity-item:hover { background: rgba(30, 41, 59, 0.8); }
        
        /* Leaflet customizations */
        .leaflet-container { background: #0b1220; }
        .leaflet-popup-content-wrapper {
            background: var(--bg-panel);
            border: 1px solid var(--border-subtle);
            border-radius: 6px;
            color: var(--text-primary);
        }
        .leaflet-popup-tip { background: var(--bg-panel); }
    </style>
</head>
<body>
    <div id="map"></div>
    
    <div class="hud">
        <div class="hud-title">● EUD Tactical Map • $callsign</div>
        <div class="hud-row"><span class="hud-label">Status:</span> <span id="status">Initializing...</span></div>
        <div class="hud-row"><span class="hud-label">Entities:</span> <span id="entityCount">0</span></div>
        <div class="hud-row"><span class="hud-label">Position:</span> <span id="position">--</span></div>
    </div>
    
    <div class="sidebar">
        <div class="sb-section">
            <div class="sb-label">Settings</div>
            <input id="cfgCallsign" class="sb-input" value="$callsign" placeholder="Callsign" />
            <input id="cfgHub" class="sb-input" value="$hubBase" placeholder="Hub URL" style="margin-top:6px;" />
            <select id="pliModeSel" class="sb-input" style="margin-top:6px;">
                <option value="LOCAL" ${if (pliMode == "LOCAL") "selected" else ""}>LOCAL</option>
                <option value="COP" ${if (pliMode == "COP") "selected" else ""}>COP</option>
                <option value="MERGED" ${if (pliMode == "MERGED") "selected" else ""}>MERGED</option>
            </select>
        </div>
        
        <div class="sb-section">
            <div class="sb-label">Map Layers</div>
            <div class="layer-group">
                <label><input id="layerEntities" type="checkbox" checked onchange="applyLayerVisibility()" /> Entities</label>
                <label><input id="layerHeat" type="checkbox" ${if (pullHeat) "checked" else ""} onchange="applyLayerVisibility()" /> Heat</label>
                <label><input id="layerCams" type="checkbox" ${if (pullCams) "checked" else ""} onchange="applyLayerVisibility()" /> Cams</label>
                <label><input id="layerSat" type="checkbox" ${if (pullSat) "checked" else ""} onchange="applyLayerVisibility()" /> SAT</label>
                <label><input id="layerAdsb" type="checkbox" checked onchange="applyLayerVisibility()" /> ADS-B</label>
            </div>
        </div>
        
        <div class="sb-section">
            <button class="sb-btn" onclick="applySettings()">Apply Settings</button>
            <button class="sb-btn" onclick="focusOwn()">Focus EUD</button>
            <button class="sb-btn" onclick="reconnectHub()">Reconnect</button>
        </div>
        
        <div class="sb-section">
            <div class="sb-label">Tracked Entities</div>
            <div id="entityList" class="entity-list">No entities tracked</div>
        </div>
    </div>

    <script src="https://unpkg.com/leaflet@1.9.4/dist/leaflet.js"></script>
    <script src="https://unpkg.com/h3-js@4.1.0/dist/h3-js.umd.js"></script>
    <script>
        // Configuration
        const OWN_CALLSIGN = $callsignJs;
        const INITIAL_PLI_MODE = $pliModeJs;
        const PULL_ENTITIES_DEFAULT = $pullEntitiesJs;
        const PULL_HEAT_DEFAULT = $pullHeatJs;
        const PULL_CAMS_DEFAULT = $pullCamsJs;
        const PULL_SAT_DEFAULT = $pullSatJs;
        
        // State
        let PLI_MODE = localStorage.getItem('eud:pli_mode') || INITIAL_PLI_MODE;
        let currentHub = localStorage.getItem('eud:hub') || document.getElementById('cfgHub').value;
        if (currentHub && !currentHub.endsWith('/')) currentHub += '/';
        let trackedEntities = [];
        let entityMarkers = {};
        let copCursor = 0;
        let lastDeltaHeatCount = 0;
        let ownPosition = { lat: $initLatJs, lon: $initLonJs };
        let layerVisibility = {
            entities: true,
            heat: $pullHeatJs,
            cams: $pullCamsJs,
            sat: $pullSatJs,
            adsb: true
        };
        
        // Layer markers storage
        let heatMarkers = {};
        let camMarkers = {};
        let satMarkers = {};
        let adsbMarkers = {};
        
        // Track if we've centered the map on user's position
        let hasCenteredOnUser = false;
        
        // Initialize map
        const map = L.map('map', { zoomControl: false }).setView([ownPosition.lat, ownPosition.lon], 15);
        L.control.zoom({ position: 'bottomright' }).addTo(map);
        
        // Dark theme tiles
        L.tileLayer('https://{s}.basemaps.cartocdn.com/dark_all/{z}/{x}/{y}{r}.png', {
            attribution: '&copy; OpenStreetMap contributors &copy; CARTO',
            subdomains: 'abcd',
            maxZoom: 19
        }).addTo(map);
        
        // Update PLI mode selector
        document.getElementById('pliModeSel').value = PLI_MODE;
        
        // ===== ENTITY MANAGEMENT =====
        
        function ingestPLI(pli) {
            // pli = { uid, callsign, type, affiliation, lat, lon, timestamp }
            const existing = trackedEntities.findIndex(e => e.uid === pli.uid);
            const now = Date.now();
            
            if (existing >= 0) {
                trackedEntities[existing] = { 
                    ...trackedEntities[existing], 
                    ...pli, 
                    lastSeen: now 
                };
            } else {
                trackedEntities.push({ 
                    ...pli, 
                    lastSeen: now,
                    affiliation: pli.affiliation || 'friendly'
                });
            }
            
            updateEntityOnMap(trackedEntities.find(e => e.uid === pli.uid));
            renderEntityList();
            updateStatus();
        }
        
        function updateEntityOnMap(entity) {
            if (!entity || !entity.lat || !entity.lon) return;
            if (Math.abs(entity.lat) < 0.2 && Math.abs(entity.lon) < 0.2) return;
            
            const affilColor = { 
                friendly: '#00c851', 
                hostile: '#ff3333', 
                neutral: '#00d4ff', 
                unknown: '#ffcc00' 
            };
            const color = affilColor[entity.affiliation] || '#ffcc00';
            
            const isOwn = entity.uid === OWN_CALLSIGN || entity.callsign === OWN_CALLSIGN;
            
            let icon;
            if (isOwn) {
                // Own position - cyan pulsing marker
                icon = L.divIcon({
                    className: '',
                    html: '<div class="my-location-marker"></div>',
                    iconSize: [20, 20],
                    iconAnchor: [10, 10]
                });
            } else {
                // Other entities - tactical symbol
                const affilClass = 'tac-' + (entity.affiliation || 'unknown');
                const symbolChar = entity.affiliation === 'friendly' ? '◈' : 
                                  entity.affiliation === 'hostile' ? '◉' : 
                                  entity.affiliation === 'neutral' ? '◐' : '◆';
                
                icon = L.divIcon({
                    className: '',
                    html: '<div class="tac-symbol ' + affilClass + '">' + symbolChar + '</div>',
                    iconSize: [32, 32],
                    iconAnchor: [16, 16]
                });
            }
            
            console.log('Creating marker for', entity.uid, 'at', entity.lat, entity.lon);
            
            if (entityMarkers[entity.uid]) {
                entityMarkers[entity.uid].setLatLng([entity.lat, entity.lon]);
                entityMarkers[entity.uid].setIcon(icon);
                if (!map.hasLayer(entityMarkers[entity.uid])) {
                    entityMarkers[entity.uid].addTo(map);
                }
            } else {
                entityMarkers[entity.uid] = L.marker([entity.lat, entity.lon], { icon }).addTo(map);
            }
            
            entityMarkers[entity.uid]
                .bindPopup('<b>' + (entity.callsign || entity.uid) + '</b><br>' + 
                          (entity.type || 'Unknown') + '<br>' + 
                          entity.lat.toFixed(5) + ', ' + entity.lon.toFixed(5))
                .bindTooltip(entity.callsign || entity.uid, { 
                    permanent: true, 
                    direction: 'top', 
                    offset: [0, -16] 
                });
            
            // Force marker visible
            if (!map.hasLayer(entityMarkers[entity.uid])) {
                entityMarkers[entity.uid].addTo(map);
            }
        }
        
        function renderEntityList() {
            const list = document.getElementById('entityList');
            if (!trackedEntities.length) {
                list.innerHTML = 'No entities tracked';
                return;
            }
            
            const now = Date.now();
            list.innerHTML = trackedEntities.map(e => {
                const age = Math.floor((now - e.lastSeen) / 1000);
                const ageStr = age < 60 ? age + 's' : Math.floor(age / 60) + 'm';
                const stale = age > 120;
                const affilClass = 'tac-' + (e.affiliation || 'unknown');
                const symbolChar = e.affiliation === 'friendly' ? '◈' : 
                                  e.affiliation === 'hostile' ? '◉' : 
                                  e.affiliation === 'neutral' ? '◐' : '◆';
                
                return '<div class="entity-item" style="opacity:' + (stale ? 0.5 : 1) + '" onclick="focusEntity(\'' + e.uid + '\')">' +
                       '<div class="tac-symbol ' + affilClass + '" style="width:20px;height:20px;font-size:10px;">' + symbolChar + '</div>' +
                       '<div style="flex:1;">' +
                       '<div style="font-weight:bold;">' + (e.callsign || e.uid) + '</div>' +
                       '<div style="font-size:10px;color:#94a3b8;">' + ageStr + (stale ? ' STALE' : '') + '</div>' +
                       '</div>' +
                       '</div>';
            }).join('');
        }
        
        function applyLayerVisibility() {
            layerVisibility.entities = document.getElementById('layerEntities').checked;
            layerVisibility.heat = document.getElementById('layerHeat').checked;
            layerVisibility.cams = document.getElementById('layerCams').checked;
            layerVisibility.sat = document.getElementById('layerSat').checked;
            layerVisibility.adsb = document.getElementById('layerAdsb').checked;
            
            Object.values(entityMarkers).forEach(m => {
                if (layerVisibility.entities) {
                    if (!map.hasLayer(m)) m.addTo(map);
                } else {
                    if (map.hasLayer(m)) map.removeLayer(m);
                }
            });
            
            Object.values(heatMarkers).forEach(m => {
                if (layerVisibility.heat) {
                    if (!map.hasLayer(m)) m.addTo(map);
                } else {
                    if (map.hasLayer(m)) map.removeLayer(m);
                }
            });
            
            Object.values(camMarkers).forEach(m => {
                if (layerVisibility.cams) {
                    if (!map.hasLayer(m)) m.addTo(map);
                } else {
                    if (map.hasLayer(m)) map.removeLayer(m);
                }
            });
            
            Object.values(satMarkers).forEach(m => {
                if (layerVisibility.sat) {
                    if (!map.hasLayer(m)) m.addTo(map);
                } else {
                    if (map.hasLayer(m)) map.removeLayer(m);
                }
            });

            Object.values(adsbMarkers).forEach(m => {
                if (layerVisibility.adsb) {
                    if (!map.hasLayer(m)) m.addTo(map);
                } else {
                    if (map.hasLayer(m)) map.removeLayer(m);
                }
            });
        }
        
        // ===== LOCATION HANDLING =====
        
        function updateOwnPosition(lat, lon) {
            ownPosition = { lat, lon };
            document.getElementById('position').textContent = lat.toFixed(5) + ', ' + lon.toFixed(5);
            
            // Center map on user position on first GPS fix
            if (!hasCenteredOnUser && map) {
                map.setView([lat, lon], 16);
                hasCenteredOnUser = true;
            }
            
            // Add self as tracked entity
            ingestPLI({
                uid: OWN_CALLSIGN,
                callsign: OWN_CALLSIGN,
                type: 'EUD',
                affiliation: 'friendly',
                lat: lat,
                lon: lon,
                timestamp: Date.now()
            });
        }
        
        function refreshNativeLocation() {
            try {
                if (window.AndroidBridge && typeof window.AndroidBridge.getLocationJson === 'function') {
                    const raw = window.AndroidBridge.getLocationJson();
                    if (!raw || raw === '{}') return;
                    const j = JSON.parse(raw);
                    if (typeof j.lat === 'number' && typeof j.lon === 'number') {
                        updateOwnPosition(j.lat, j.lon);
                    }
                }
            } catch (e) {
                console.error('Location refresh error:', e);
            }
        }
        
        // ===== COP/HUB INTEGRATION =====
        
        async function pushLocalPliToHub() {
            if (!ownPosition) return;
            
            try {
                const hub = currentHub.endsWith('/') ? currentHub : currentHub + '/';
                if (!hub.startsWith('http')) return;
                
                // Convert lat/lon to H3 tile
                let tileId = null;
                if (window.h3 && window.h3.latLngToCell) {
                    try {
                        tileId = window.h3.latLngToCell(ownPosition.lat, ownPosition.lon, 10);
                    } catch (e) {}
                }
                
                // Fallback to android_ format if H3 not available
                if (!tileId) {
                    tileId = 'android_' + Math.round(ownPosition.lat * 10000) + '_' + Math.round(ownPosition.lon * 10000);
                }
                
                const nowSec = Math.floor(Date.now() / 1000);
                const payload = {
                    schema_version: 1,
                    device_id: OWN_CALLSIGN,
                    source_type: 'entity',
                    timestamp_utc: nowSec,
                    tiles: [{ 
                        tile_id: tileId, 
                        time_bucket: Math.floor(nowSec / 60) * 60,
                        lat: ownPosition.lat,
                        lon: ownPosition.lon
                    }]
                };
                
                await fetch(hub + 'api/push', {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify(payload)
                });
            } catch (e) {
                console.log('Push failed:', e.message);
            }
        }
        
        async function pollCOP() {
            try {
                // Push local position first
                await pushLocalPliToHub();
                await pollDeltaLayers();
                
                const resp = await fetch(currentHub + 'api/cop_snapshot?max_age_secs=7200');
                if (!resp.ok) throw new Error('HTTP ' + resp.status);
                
                const data = await resp.json();
                
                // Process entities
                let entityCount = 0;
                if (data.entities && Array.isArray(data.entities)) {
                    entityCount = data.entities.length;
                    data.entities.forEach(ent => {
                        if (!ent.tile_id) return;
                        
                        const pos = parseTileToLatLon(ent.tile_id);
                        if (!pos) return;
                        
                        ingestPLI({
                            uid: ent.device_id || 'unknown',
                            callsign: ent.device_id || 'unknown',
                            type: ent.source_type || 'unknown',
                            affiliation: 'friendly',
                            lat: pos.lat,
                            lon: pos.lon,
                            timestamp: Date.now()
                        });
                    });
                }
                
                let camCount = (data.cameras && Array.isArray(data.cameras)) ? data.cameras.length : 0;
                let heatCount = (data.heat && Array.isArray(data.heat)) ? data.heat.length : 0;
                let satCount = (data.satellites && Array.isArray(data.satellites)) ? data.satellites.length : 0;
                
                // Process cameras
                if (data.cameras && Array.isArray(data.cameras)) {
                    renderCameras(data.cameras);
                }
                
                // Process heat
                if (data.heat && Array.isArray(data.heat)) {
                    renderHeat(data.heat);
                }
                
                // Process satellites
                if (data.satellites && Array.isArray(data.satellites)) {
                    renderSatellites(data.satellites);
                }
                
                document.getElementById('status').textContent = 'COP: ' + entityCount + ' entities, ' + camCount + ' cams, ' + heatCount + ' heat, ' + satCount + ' sat • Δheat:' + lastDeltaHeatCount;
            } catch (e) {
                document.getElementById('status').textContent = 'COP Error: ' + e.message;
            }
        }

        async function pollDeltaLayers() {
            try {
                const url = currentHub + 'api/delta?device_id=' + encodeURIComponent(OWN_CALLSIGN) + '&cursor=' + copCursor;
                const resp = await fetch(url);
                if (!resp.ok) return;
                const data = await resp.json();
                copCursor = Math.max(copCursor, Number(data.cursor || 0));

                const derivedHeat = [];
                const updates = Array.isArray(data.tiles) ? data.tiles : [];
                updates.forEach(update => {
                    const tiles = Array.isArray(update.tiles) ? update.tiles : [];
                    tiles.forEach(t => {
                        const pos = parseTileToLatLon(t.tile_id);
                        if (!pos) return;

                        const dev = t.device_id || update.device_id;
                        const src = t.source_type || update.source_type || 'entity';
                        if (dev && src !== 'hub_local' && String(dev).toLowerCase() !== 'hub') {
                            ingestPLI({
                                uid: dev,
                                callsign: dev,
                                type: src,
                                affiliation: 'friendly',
                                lat: pos.lat,
                                lon: pos.lon,
                                timestamp: Date.now()
                            });
                        }

                        if (t.rf && Array.isArray(t.rf.channel_occupancy) && t.rf.channel_occupancy.length > 0) {
                            const vals = t.rf.channel_occupancy.map(c => Number(c.utilization_pct || 0)).filter(v => Number.isFinite(v));
                            const max = vals.length ? Math.max(...vals) : 0;
                            const mean = vals.length ? (vals.reduce((a,b)=>a+b,0)/vals.length) : 0;
                            derivedHeat.push({ tile_id: t.tile_id, sensor_type: 'rf', dimension: 'delta', max, mean });
                        }
                        if (t.wifi && Array.isArray(t.wifi.channel_hotness) && t.wifi.channel_hotness.length > 0) {
                            const vals = t.wifi.channel_hotness.map(c => Number(c.count || 0)).filter(v => Number.isFinite(v));
                            const max = vals.length ? Math.max(...vals) : 0;
                            const mean = vals.length ? (vals.reduce((a,b)=>a+b,0)/vals.length) : 0;
                            derivedHeat.push({ tile_id: t.tile_id, sensor_type: 'wifi', dimension: 'delta', max, mean });
                        }
                    });
                });

                lastDeltaHeatCount = derivedHeat.length;
                if (derivedHeat.length > 0) renderHeat(derivedHeat);
            } catch (e) {
                console.log('Delta poll error:', e.message);
            }
        }
        
        function renderCameras(cameras) {
            const nextCams = {};
            
            cameras.forEach(cam => {
                if (!cam.tile_id) return;
                const pos = parseTileToLatLon(cam.tile_id);
                if (!pos) return;
                
                const key = cam.tile_id + ':' + (cam.dimension || 'default');
                const count = cam.count || 0;
                
                let marker = camMarkers[key];
                const icon = L.divIcon({
                    className: '',
                    html: '<div style="width:14px;height:14px;border-radius:50%;border:2px solid #fff;box-shadow:0 0 8px #22c55e;background:rgba(34,197,94,0.7);"></div>',
                    iconSize: [14, 14],
                    iconAnchor: [7, 7]
                });
                
                if (!marker) {
                    marker = L.marker([pos.lat, pos.lon], { icon });
                    camMarkers[key] = marker;
                } else {
                    marker.setLatLng([pos.lat, pos.lon]);
                }
                
                marker.bindPopup('Camera<br>Count: ' + count + '<br>' + pos.lat.toFixed(5) + ', ' + pos.lon.toFixed(5));
                
                if (layerVisibility.cams && !map.hasLayer(marker)) {
                    marker.addTo(map);
                }
                
                nextCams[key] = true;
            });
            
            // Remove stale cameras
            Object.keys(camMarkers).forEach(key => {
                if (!nextCams[key]) {
                    if (map.hasLayer(camMarkers[key])) map.removeLayer(camMarkers[key]);
                    delete camMarkers[key];
                }
            });
        }
        
        function renderHeat(heatData) {
            const nextHeat = {};
            
            heatData.forEach(h => {
                if (!h.tile_id) return;
                const pos = parseTileToLatLon(h.tile_id);
                if (!pos) return;
                
                const key = h.tile_id + ':' + (h.sensor_type || 'unknown') + ':' + (h.dimension || 'default');
                const maxVal = h.max || 0;
                const meanVal = h.mean || 0;
                const intensity = Math.min(1, maxVal / 100);
                const radius = 15 + (intensity * 25);
                
                const color = h.sensor_type === 'wifi' ? '#22d3ee' : '#f97316';
                
                let marker = heatMarkers[key];
                
                if (!marker) {
                    marker = L.circleMarker([pos.lat, pos.lon], {
                        radius: radius,
                        color: color,
                        weight: 1,
                        fillColor: color,
                        fillOpacity: 0.3 + (intensity * 0.3)
                    });
                    heatMarkers[key] = marker;
                } else {
                    marker.setLatLng([pos.lat, pos.lon]);
                    marker.setStyle({
                        radius: radius,
                        fillOpacity: 0.3 + (intensity * 0.3)
                    });
                }
                
                marker.bindPopup('Heat ' + (h.sensor_type || 'unknown') + '<br>Max: ' + maxVal + '<br>Mean: ' + meanVal);
                
                if (layerVisibility.heat && !map.hasLayer(marker)) {
                    marker.addTo(map);
                }
                
                nextHeat[key] = true;
            });
            
            // Remove stale heat
            Object.keys(heatMarkers).forEach(key => {
                if (!nextHeat[key]) {
                    if (map.hasLayer(heatMarkers[key])) map.removeLayer(heatMarkers[key]);
                    delete heatMarkers[key];
                }
            });
        }
        
        function renderSatellites(sats) {
            const nextSats = {};
            
            sats.forEach(sat => {
                if (!sat.tile_id) return;
                const pos = parseTileToLatLon(sat.tile_id);
                if (!pos) return;
                
                const key = sat.tile_id + ':' + (sat.dimension || 'default');
                const count = sat.count || 0;
                
                let marker = satMarkers[key];
                const icon = L.divIcon({
                    className: '',
                    html: '<div style="width:16px;height:16px;transform:rotate(45deg);border:2px solid #ffea00;background:rgba(255,234,0,0.6);box-shadow:0 0 10px #ffea00;"></div>',
                    iconSize: [16, 16],
                    iconAnchor: [8, 8]
                });
                
                if (!marker) {
                    marker = L.marker([pos.lat, pos.lon], { icon });
                    satMarkers[key] = marker;
                } else {
                    marker.setLatLng([pos.lat, pos.lon]);
                }
                
                marker.bindPopup('Satellite<br>Count: ' + count + '<br>' + pos.lat.toFixed(5) + ', ' + pos.lon.toFixed(5));
                
                if (layerVisibility.sat && !map.hasLayer(marker)) {
                    marker.addTo(map);
                }
                
                nextSats[key] = true;
            });
            
            // Remove stale satellites
            Object.keys(satMarkers).forEach(key => {
                if (!nextSats[key]) {
                    if (map.hasLayer(satMarkers[key])) map.removeLayer(satMarkers[key]);
                    delete satMarkers[key];
                }
            });
        }

        async function pollAdsb() {
            try {
                const resp = await fetch(currentHub + 'api/adsb?max_age_secs=1200');
                if (!resp.ok) return;
                const data = await resp.json();
                const aircraft = Array.isArray(data?.aircraft) ? data.aircraft : (Array.isArray(data) ? data : []);
                const next = {};

                aircraft.forEach(ac => {
                    const lat = Number(ac.latitude ?? ac.lat);
                    const lon = Number(ac.longitude ?? ac.lon);
                    if (!Number.isFinite(lat) || !Number.isFinite(lon)) return;
                    const id = String(ac.icao || ac.callsign || ac.id || 'AC');

                    let marker = adsbMarkers[id];
                    const icon = L.divIcon({
                        className: '',
                        html: '<div style="width:12px;height:12px;border-radius:50%;border:2px solid #fff;background:#f59e0b;box-shadow:0 0 8px #f59e0b;"></div>',
                        iconSize: [12, 12],
                        iconAnchor: [6, 6]
                    });

                    if (!marker) {
                        marker = L.marker([lat, lon], { icon });
                        adsbMarkers[id] = marker;
                    } else {
                        marker.setLatLng([lat, lon]);
                    }

                    marker.bindPopup('ADS-B ' + id + '<br>' + lat.toFixed(5) + ', ' + lon.toFixed(5));
                    if (layerVisibility.adsb && !map.hasLayer(marker)) marker.addTo(map);
                    next[id] = true;
                });

                Object.keys(adsbMarkers).forEach(id => {
                    if (!next[id]) {
                        if (map.hasLayer(adsbMarkers[id])) map.removeLayer(adsbMarkers[id]);
                        delete adsbMarkers[id];
                    }
                });
            } catch (e) {
                console.log('ADS-B poll error:', e.message);
            }
        }
        
        function parseTileToLatLon(tileId) {
            if (!tileId) return null;

            // Try H3 first (support both window.h3 and window.h3js exports)
            try {
                const h3lib = (window.h3 && typeof window.h3.cellToLatLng === 'function')
                    ? window.h3
                    : ((window.h3js && typeof window.h3js.cellToLatLng === 'function') ? window.h3js : null);
                if (h3lib && /^[0-9a-f]{15,16}$/i.test(tileId)) {
                    const ll = h3lib.cellToLatLng(tileId);
                    if (Array.isArray(ll) && Number.isFinite(ll[0]) && Number.isFinite(ll[1])) {
                        return { lat: ll[0], lon: ll[1] };
                    }
                }
            } catch (e) {}
            
            // Android fallback tile format
            if (tileId.startsWith('android_')) {
                const parts = tileId.split('_');
                if (parts.length >= 3) {
                    const latQ = parseFloat(parts[1]);
                    const lonQ = parseFloat(parts[2]);
                    if (!isNaN(latQ) && !isNaN(lonQ)) {
                        return { lat: latQ / 10000.0, lon: lonQ / 10000.0 };
                    }
                }
            }
            
            return null;
        }
        
        // ===== UI ACTIONS =====
        
        function applySettings() {
            const cs = document.getElementById('cfgCallsign').value.trim();
            const hub = document.getElementById('cfgHub').value.trim();
            const mode = document.getElementById('pliModeSel').value;
            
            if (cs) localStorage.setItem('eud:callsign', cs);
            if (hub) {
                currentHub = hub.endsWith('/') ? hub : hub + '/';
                localStorage.setItem('eud:hub', currentHub);
                document.getElementById('cfgHub').value = currentHub;
            }
            PLI_MODE = mode;
            localStorage.setItem('eud:pli_mode', PLI_MODE);
            
            applyLayerVisibility();
            document.getElementById('status').textContent = 'Settings applied: ' + PLI_MODE;
        }
        
        function focusOwn() {
            const m = entityMarkers[OWN_CALLSIGN];
            if (m) {
                map.setView(m.getLatLng(), 16);
            } else {
                map.setView([ownPosition.lat, ownPosition.lon], 16);
            }
        }
        
        function focusEntity(uid) {
            const m = entityMarkers[uid];
            if (m) map.setView(m.getLatLng(), 16);
        }
        
        function reconnectHub() {
            copCursor = 0;
            document.getElementById('status').textContent = 'Reconnecting...';
            pollCOP();
        }
        
        function updateStatus() {
            document.getElementById('entityCount').textContent = trackedEntities.length;
        }
        
        // ===== INIT =====
        
        // Seed local marker immediately from initial/fallback coordinates
        updateOwnPosition(ownPosition.lat, ownPosition.lon);

        // Try browser geolocation first
        if (navigator.geolocation) {
            navigator.geolocation.watchPosition(
                (pos) => {
                    updateOwnPosition(pos.coords.latitude, pos.coords.longitude);
                },
                (err) => {
                    console.log('Browser geolocation error:', err.message);
                    refreshNativeLocation();
                },
                { enableHighAccuracy: true, maximumAge: 2000, timeout: 10000 }
            );
        }
        
        // Fallback to native location bridge
        setInterval(refreshNativeLocation, 3000);
        refreshNativeLocation();
        
        // Poll COP + ADS-B continuously (LOCAL keeps local primary but still syncs/pulls)
        setInterval(pollCOP, 5000);
        setInterval(pollAdsb, 5000);
        pollCOP();
        pollAdsb();
        
        document.getElementById('status').textContent = PLI_MODE === 'LOCAL' ? 'Local Mode + COP Sync' : 'Connecting...';
        updateStatus();
        
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

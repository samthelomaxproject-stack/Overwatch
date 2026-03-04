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

class TacticalMapActivity : AppCompatActivity() {

    private lateinit var webView: WebView

    @SuppressLint("SetJavaScriptEnabled")
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        webView = WebView(this)
        setContentView(webView)

        val hub = intent.getStringExtra(EXTRA_HUB_URL)?.trim().orEmpty()
        val callsign = intent.getStringExtra(EXTRA_CALLSIGN)?.trim().orEmpty().ifEmpty { "ANDROID-EUD" }
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

        val html = tacticalHtml(callsign, baseUrl, initLat, initLon)
        runCatching {
            webView.loadDataWithBaseURL(
                baseUrl,
                html,
                "text/html",
                "utf-8",
                null
            )
        }.onFailure {
            Toast.makeText(this, "Failed to open tactical map: ${it.message}", Toast.LENGTH_LONG).show()
        }
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
        ) {
            return null
        }

        val providers = runCatching { lm.getProviders(true) }.getOrDefault(emptyList())
        var best: Location? = null
        for (p in providers) {
            val loc = runCatching { lm.getLastKnownLocation(p) }.getOrNull() ?: continue
            if (best == null || loc.accuracy < best!!.accuracy) best = loc
        }
        return best
    }

    private fun tacticalHtml(callsign: String, hubBase: String, initLat: Double, initLon: Double): String {
        val callsignJs = org.json.JSONObject.quote(callsign)
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
    .hud {
      position: fixed; top: 8px; left: 8px; z-index: 9999;
      background: rgba(11,18,32,0.85); color: #cbd5e1;
      border: 1px solid rgba(255,255,255,0.12); border-radius: 8px;
      font: 12px monospace; padding: 8px 10px;
      max-width: 60vw;
    }
    .sidebar {
      position: fixed; top: 8px; right: 8px; z-index: 9999;
      width: 280px; background: rgba(11,18,32,0.9); color: #cbd5e1;
      border: 1px solid rgba(255,255,255,0.12); border-radius: 8px;
      font: 12px monospace; padding: 10px;
    }
    .sb-row { margin-bottom: 8px; }
    .sb-label { font-size: 11px; color: #94a3b8; margin-bottom: 4px; }
    .sb-input { width: 100%; box-sizing: border-box; background: #0f172a; color: #e2e8f0; border: 1px solid #334155; border-radius: 4px; padding: 6px; }
    .sb-btn { width: 100%; margin-top: 6px; background: #1e293b; color: #e2e8f0; border: 1px solid #334155; border-radius: 4px; padding: 6px; }
    .dot { display:inline-block; width:10px; height:10px; border-radius:50%; background:#22c55e; margin-right:6px; }
  </style>
</head>
<body>
  <div id="map"></div>
  <div class="hud">
    <div><span class="dot"></span>EUD Tactical Map • ${callsign}</div>
    <div id="status">Connecting…</div>
    <div id="count">Entities: 0</div>
  </div>
  <div class="sidebar">
    <div class="sb-row">
      <div class="sb-label">Callsign</div>
      <input id="cfgCallsign" class="sb-input" value="${callsign}" />
    </div>
    <div class="sb-row">
      <div class="sb-label">Hub</div>
      <input id="cfgHub" class="sb-input" value="${hubBase}" />
    </div>
    <button class="sb-btn" onclick="focusOwn()">Focus EUD</button>
    <button class="sb-btn" onclick="reloadDelta()">Reconnect Hub</button>
  </div>

  <script src="https://unpkg.com/leaflet@1.9.4/dist/leaflet.js"></script>
  <script>
    const OWN_CALLSIGN = $callsignJs;
    const map = L.map('map').setView([${initLat}, ${initLon}], 15);

    const layerDark = L.tileLayer('https://{s}.basemaps.cartocdn.com/dark_all/{z}/{x}/{y}{r}.png', {
      maxZoom: 19,
      attribution: '&copy; OpenStreetMap &copy; CARTO'
    });
    const layerSat = L.tileLayer('https://server.arcgisonline.com/ArcGIS/rest/services/World_Imagery/MapServer/tile/{z}/{y}/{x}', {
      maxZoom: 19,
      attribution: 'Tiles &copy; Esri'
    });
    const layerTopo = L.tileLayer('https://{s}.tile.openstreetmap.org/{z}/{x}/{y}.png', {
      maxZoom: 19,
      attribution: '&copy; OpenStreetMap'
    });

    layerDark.addTo(map);
    L.control.layers({ 'Dark': layerDark, 'Satellite': layerSat, 'Topo': layerTopo }).addTo(map);
    document.getElementById('status').textContent = 'Map loaded • connecting to hub delta…';

    let cursor = 0;
    const markers = {};
    let ownGpsMarker = null;
    let centeredOnOwn = false;

    function ownCallsign() {
      return (document.getElementById('cfgCallsign')?.value || OWN_CALLSIGN || 'ANDROID-EUD').trim();
    }

    function focusOwn() {
      const id = ownCallsign();
      const m = markers[id] || ownGpsMarker;
      if (m) map.setView(m.getLatLng(), 16);
    }

    function reloadDelta() {
      cursor = 0;
      document.getElementById('status').textContent = 'Reconnecting hub delta…';
      pollDelta();
    }

    function parseAndroidTile(tileId) {
      if (!tileId || !tileId.startsWith('android_')) return null;
      const parts = tileId.split('_');
      if (parts.length !== 3) return null;
      const latQ = parseInt(parts[1], 10);
      const lonQ = parseInt(parts[2], 10);
      if (!Number.isFinite(latQ) || !Number.isFinite(lonQ)) return null;
      return { lat: latQ / 10000.0, lon: lonQ / 10000.0 };
    }

    function upsertMarker(id, lat, lon, sourceType) {
      const isOwn = String(id || '').toUpperCase() === String(ownCallsign() || '').toUpperCase();
      const color = isOwn ? '#22c55e' : (sourceType === 'drone' ? '#f97316' : '#60a5fa');
      const size = isOwn ? 16 : 12;
      const icon = L.divIcon({
        className: 'eud-marker',
        html: `<div style="width:${'$'}{size}px;height:${'$'}{size}px;border-radius:50%;background:${'$'}{color};border:2px solid #fff;box-shadow:0 0 10px rgba(0,0,0,0.65);"></div>`,
        iconSize: [size, size],
        iconAnchor: [Math.round(size/2), Math.round(size/2)]
      });

      if (!markers[id]) {
        markers[id] = L.marker([lat, lon], { icon }).addTo(map);
      } else {
        markers[id].setLatLng([lat, lon]);
        markers[id].setIcon(icon);
      }
      markers[id].bindPopup(`<b>${'$'}{id}</b><br/>${'$'}{sourceType || 'unknown'}<br/>${'$'}{lat.toFixed(5)}, ${'$'}{lon.toFixed(5)}`);

      if (isOwn && !centeredOnOwn) {
        map.setView([lat, lon], 16);
        centeredOnOwn = true;
      }
    }

    async function pollDelta() {
      const statusEl = document.getElementById('status');
      const countEl = document.getElementById('count');
      try {
        const resp = await fetch(`/api/delta?device_id=android-eud-map&cursor=${'$'}{cursor}`);
        if (!resp.ok) throw new Error(`HTTP ${'$'}{resp.status}`);
        const data = await resp.json();
        cursor = data.cursor || cursor;

        let seen = 0;
        (data.tiles || []).forEach(batch => {
          (batch.tiles || []).forEach(t => {
            const p = parseAndroidTile(t.tile_id);
            if (!p) return;
            const id = t.device_id || batch.device_id || 'unknown';
            const sourceType = t.source_type || batch.source_type || 'unknown';
            upsertMarker(id, p.lat, p.lon, sourceType);
            seen += 1;
          });
        });

        statusEl.textContent = `Connected • cursor ${'$'}{cursor}`;
        countEl.textContent = `Entities: ${'$'}{Object.keys(markers).length} • updates: ${'$'}{seen}`;
      } catch (e) {
        statusEl.textContent = `Delta error: ${'$'}{e}`;
      }
    }

    if (navigator.geolocation) {
      navigator.geolocation.watchPosition((pos) => {
        const lat = pos.coords.latitude;
        const lon = pos.coords.longitude;
        const icon = L.divIcon({
          className: 'eud-self',
          html: `<div style="width:18px;height:18px;border-radius:50%;background:#22c55e;border:3px solid #ffffff;box-shadow:0 0 12px rgba(34,197,94,0.8);"></div>`,
          iconSize: [18, 18],
          iconAnchor: [9, 9]
        });
        if (!ownGpsMarker) {
          ownGpsMarker = L.marker([lat, lon], { icon }).addTo(map);
          ownGpsMarker.bindPopup(`<b>${'$'}{ownCallsign()}</b><br/>Live GPS`);
        } else {
          ownGpsMarker.setLatLng([lat, lon]);
        }
        if (!centeredOnOwn) {
          map.setView([lat, lon], 16);
          centeredOnOwn = true;
        }
      }, (err) => {
        document.getElementById('status').textContent = `GPS error: ${'$'}{err.message}`;
      }, { enableHighAccuracy: true, maximumAge: 2000, timeout: 10000 });
    }

    setInterval(pollDelta, 3000);
    pollDelta();
  </script>
</body>
</html>
""".trimIndent()
    }

    companion object {
        const val EXTRA_HUB_URL = "extra_hub_url"
        const val EXTRA_CALLSIGN = "extra_callsign"
    }
}

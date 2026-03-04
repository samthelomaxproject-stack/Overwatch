package ai.overwatch.android

import android.annotation.SuppressLint
import android.os.Bundle
import android.webkit.WebChromeClient
import android.webkit.WebView
import android.widget.Toast
import androidx.appcompat.app.AppCompatActivity

class TacticalMapActivity : AppCompatActivity() {

    private lateinit var webView: WebView

    @SuppressLint("SetJavaScriptEnabled")
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        webView = WebView(this)
        setContentView(webView)

        val hub = intent.getStringExtra(EXTRA_HUB_URL)?.trim().orEmpty()
        val baseUrl = normalizeHubBase(hub)

        supportActionBar?.title = "Tactical Map"
        supportActionBar?.subtitle = baseUrl

        webView.settings.javaScriptEnabled = true
        webView.settings.domStorageEnabled = true
        webView.settings.useWideViewPort = true
        webView.settings.loadWithOverviewMode = true
        webView.settings.allowFileAccess = true
        webView.settings.allowContentAccess = true

        webView.webChromeClient = WebChromeClient()

        val html = tacticalHtml()
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
        val fallback = "http://10.0.0.5:8789/"
        if (!hubUrl.startsWith("http://") && !hubUrl.startsWith("https://")) return fallback
        return hubUrl.trimEnd('/') + "/"
    }

    private fun tacticalHtml(): String = """
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
    }
    .dot { display:inline-block; width:10px; height:10px; border-radius:50%; background:#22c55e; margin-right:6px; }
  </style>
</head>
<body>
  <div id="map"></div>
  <div class="hud">
    <div><span class="dot"></span>EUD Tactical Map</div>
    <div id="status">Connecting…</div>
    <div id="count">Entities: 0</div>
  </div>

  <script src="https://unpkg.com/leaflet@1.9.4/dist/leaflet.js"></script>
  <script>
    const map = L.map('map').setView([32.7767, -96.7970], 12);
    L.tileLayer('https://{s}.tile.openstreetmap.org/{z}/{x}/{y}.png', {
      maxZoom: 19,
      attribution: '&copy; OpenStreetMap'
    }).addTo(map);

    let cursor = 0;
    const markers = {};

    function parseAndroidTile(tileId) {
      // Collector MVP tile format: android_{latQ}_{lonQ} where lat/lon were multiplied by 10000
      if (!tileId || !tileId.startsWith('android_')) return null;
      const parts = tileId.split('_');
      if (parts.length !== 3) return null;
      const latQ = parseInt(parts[1], 10);
      const lonQ = parseInt(parts[2], 10);
      if (!Number.isFinite(latQ) || !Number.isFinite(lonQ)) return null;
      return { lat: latQ / 10000.0, lon: lonQ / 10000.0 };
    }

    function upsertMarker(id, lat, lon, sourceType) {
      const color = sourceType === 'drone' ? '#f97316' : sourceType === 'handheld' ? '#22c55e' : '#60a5fa';
      const icon = L.divIcon({
        className: 'eud-marker',
        html: `<div style="width:12px;height:12px;border-radius:50%;background:${color};border:2px solid #fff;box-shadow:0 0 8px rgba(0,0,0,0.6);"></div>`,
        iconSize: [12, 12],
        iconAnchor: [6, 6]
      });

      if (!markers[id]) {
        markers[id] = L.marker([lat, lon], { icon }).addTo(map);
      } else {
        markers[id].setLatLng([lat, lon]);
        markers[id].setIcon(icon);
      }
      markers[id].bindPopup(`<b>${id}</b><br/>${sourceType || 'unknown'}<br/>${lat.toFixed(5)}, ${lon.toFixed(5)}`);
    }

    async function pollDelta() {
      const statusEl = document.getElementById('status');
      const countEl = document.getElementById('count');
      try {
        const resp = await fetch(`/api/delta?device_id=android-eud-map&cursor=${cursor}`);
        if (!resp.ok) throw new Error(`HTTP ${resp.status}`);
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

        statusEl.textContent = `Connected • cursor ${cursor}`;
        countEl.textContent = `Entities: ${Object.keys(markers).length} • updates: ${seen}`;
      } catch (e) {
        statusEl.textContent = `Delta error: ${e}`;
      }
    }

    setInterval(pollDelta, 3000);
    pollDelta();
  </script>
</body>
</html>
""".trimIndent()

    companion object {
        const val EXTRA_HUB_URL = "extra_hub_url"
    }
}

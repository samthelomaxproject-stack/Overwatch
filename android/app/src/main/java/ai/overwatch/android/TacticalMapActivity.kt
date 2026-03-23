package ai.overwatch.android

import android.Manifest
import android.annotation.SuppressLint
import android.content.Intent
import android.content.pm.PackageManager
import android.location.Location
import android.net.Uri
import android.location.LocationManager
import android.os.Bundle
import android.graphics.ImageFormat
import android.graphics.Rect
import android.graphics.YuvImage
import android.util.Base64
import android.webkit.GeolocationPermissions
import android.webkit.JavascriptInterface
import android.webkit.WebChromeClient
import android.webkit.WebView
import android.widget.Toast
import androidx.appcompat.app.AppCompatActivity
import androidx.core.app.ActivityCompat
import androidx.lifecycle.lifecycleScope
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat
import com.meta.wearable.dat.camera.StreamSession
import com.meta.wearable.dat.camera.startStreamSession
import com.meta.wearable.dat.camera.types.StreamConfiguration
import com.meta.wearable.dat.camera.types.VideoFrame
import com.meta.wearable.dat.camera.types.VideoQuality
import com.meta.wearable.dat.core.Wearables
import com.meta.wearable.dat.core.selectors.AutoDeviceSelector
import com.meta.wearable.dat.core.types.Permission
import com.meta.wearable.dat.core.types.PermissionStatus
import com.meta.wearable.dat.core.types.RegistrationState
import java.io.ByteArrayOutputStream
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.collect
import kotlinx.coroutines.launch

class TacticalMapActivity : AppCompatActivity() {

    private lateinit var webView: WebView
    private val wearablesPermissionLauncher = registerForActivityResult(Wearables.RequestPermissionContract()) { result ->
        val status = result.getOrDefault(PermissionStatus.Denied)
        metaCameraPermissionStatus = when (status) {
            is PermissionStatus.Granted -> "GRANTED"
            is PermissionStatus.Denied -> "DENIED"
            else -> status::class.java.simpleName ?: "UNKNOWN"
        }
    }
    private var metaStreamSession: StreamSession? = null
    private var metaStreamJob: Job? = null
    @Volatile private var metaLatestFrameBase64: String = ""
    @Volatile private var metaLastFrameTsMs: Long = 0L
    @Volatile private var metaBoundEntityUid: String = ""
    @Volatile private var metaRegistrationStatus: String = "UNKNOWN"
    @Volatile private var metaCameraPermissionStatus: String = "UNKNOWN"

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

        @JavascriptInterface
        fun updateCallsign(newCallsign: String): String {
            val cs = newCallsign.trim().ifEmpty { "ANDROID-EUD" }
            ConfigStore.setCallsign(this@TacticalMapActivity, cs)
            return cs
        }

        @JavascriptInterface
        fun openExternalUrl(url: String) {
            runCatching {
                val i = Intent(Intent.ACTION_VIEW, Uri.parse(url))
                i.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
                startActivity(i)
            }
        }

        @JavascriptInterface
        fun openFeedSourceInApp(url: String, title: String?) {
            runCatching {
                val i = Intent(this@TacticalMapActivity, CctvSourceActivity::class.java)
                i.putExtra(CctvSourceActivity.EXTRA_URL, url)
                i.putExtra(CctvSourceActivity.EXTRA_TITLE, title ?: "CCTV Source")
                startActivity(i)
            }
        }

        @JavascriptInterface
        fun bindLocalGlasses(entityUid: String): Boolean {
            return startMetaLocalStream(entityUid)
        }

        @JavascriptInterface
        fun getMetaFrameBase64(entityUid: String): String {
            val frame = metaLatestFrameBase64
            if (frame.isEmpty()) return ""
            if (entityUid == metaBoundEntityUid) return frame
            // Fallback: only one local Meta stream exists, so allow replaying latest frame
            // even if UI/watch context UID drifts from the originally bound UID.
            return frame
        }

        @JavascriptInterface
        fun activateGlasses() {
            runCatching {
                Wearables.startRegistration(this@TacticalMapActivity)
            }
        }

        @JavascriptInterface
        fun requestGlassesCameraPermission() {
            runCatching {
                wearablesPermissionLauncher.launch(Permission.CAMERA)
            }
        }

        @JavascriptInterface
        fun stopLocalGlassesStream(): Boolean {
            return stopMetaLocalStream()
        }

        @JavascriptInterface
        fun reconnectLocalGlasses(entityUid: String): Boolean {
            stopMetaLocalStream()
            return startMetaLocalStream(entityUid)
        }

        @JavascriptInterface
        fun getGlassesStatusJson(): String {
            val streaming = if (metaStreamSession != null) "STREAMING" else "IDLE"
            val frameReady = if (metaLatestFrameBase64.isNotEmpty()) "YES" else "NO"
            val frameAgeMs = if (metaLastFrameTsMs > 0) (System.currentTimeMillis() - metaLastFrameTsMs).coerceAtLeast(0L) else -1L
            return "{\"registration\":\"$metaRegistrationStatus\",\"camera_permission\":\"$metaCameraPermissionStatus\",\"stream\":\"$streaming\",\"frame_ready\":\"$frameReady\",\"frame_age_ms\":$frameAgeMs,\"bound_uid\":\"$metaBoundEntityUid\"}"
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

        runCatching { Wearables.initialize(this) }
        lifecycleScope.launch {
            runCatching {
                Wearables.registrationState.collect { st ->
                    metaRegistrationStatus = when (st) {
                        is RegistrationState.Registered -> "REGISTERED"
                        is RegistrationState.Registering -> "REGISTERING"
                        is RegistrationState.Unregistering -> "UNREGISTERING"
                        else -> st::class.java.simpleName ?: "UNKNOWN"
                    }
                }
            }
        }

        val html = tacticalHtml(callsign, baseUrl, pliMode, pullEntities, pullHeat, pullCams, pullSat, initLat, initLon)
        runCatching { webView.loadDataWithBaseURL(baseUrl, html, "text/html", "utf-8", null) }
            .onFailure { Toast.makeText(this, "Failed to open tactical map: ${it.message}", Toast.LENGTH_LONG).show() }
    }

    private fun startMetaLocalStream(entityUid: String): Boolean {
        return runCatching {
            metaBoundEntityUid = entityUid

            val now = System.currentTimeMillis()
            val hasRecentFrame = metaLatestFrameBase64.isNotEmpty() && (metaLastFrameTsMs > 0L) && ((now - metaLastFrameTsMs) < 3000L)
            // Keep existing session only if it is actively producing fresh frames.
            if (metaStreamSession != null && hasRecentFrame) return true

            stopMetaLocalStream()

            Wearables.initialize(this)
            val selector = AutoDeviceSelector()
            val session = Wearables.startStreamSession(
                applicationContext,
                selector,
                StreamConfiguration(videoQuality = VideoQuality.MEDIUM, frameRate = 24),
            )
            metaStreamSession = session
            metaStreamJob = lifecycleScope.launch {
                session.videoStream.collect { frame ->
                    runCatching {
                        metaLatestFrameBase64 = encodeVideoFrameToJpegBase64(frame)
                        metaLastFrameTsMs = System.currentTimeMillis()
                    }
                }
            }
            true
        }.getOrDefault(false)
    }

    private fun stopMetaLocalStream(): Boolean {
        return runCatching {
            metaStreamJob?.cancel()
            metaStreamJob = null
            metaStreamSession?.close()
            metaStreamSession = null
            metaLatestFrameBase64 = ""
            metaLastFrameTsMs = 0L
            true
        }.getOrDefault(false)
    }

    private fun encodeVideoFrameToJpegBase64(videoFrame: VideoFrame): String {
        val buffer = videoFrame.buffer
        val dataSize = buffer.remaining()
        val byteArray = ByteArray(dataSize)
        val originalPosition = buffer.position()
        buffer.get(byteArray)
        buffer.position(originalPosition)

        val nv21 = convertI420toNV21(byteArray, videoFrame.width, videoFrame.height)
        val image = YuvImage(nv21, ImageFormat.NV21, videoFrame.width, videoFrame.height, null)
        val out = ByteArrayOutputStream()
        image.compressToJpeg(Rect(0, 0, videoFrame.width, videoFrame.height), 65, out)
        return Base64.encodeToString(out.toByteArray(), Base64.NO_WRAP)
    }

    private fun convertI420toNV21(input: ByteArray, width: Int, height: Int): ByteArray {
        val output = ByteArray(input.size)
        val size = width * height
        val quarter = size / 4
        input.copyInto(output, 0, 0, size)
        for (n in 0 until quarter) {
            output[size + n * 2] = input[size + quarter + n]
            output[size + n * 2 + 1] = input[size + n]
        }
        return output
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
    <link rel="stylesheet" href="https://unpkg.com/leaflet.markercluster@1.5.3/dist/MarkerCluster.css" />
    <link rel="stylesheet" href="https://unpkg.com/leaflet.markercluster@1.5.3/dist/MarkerCluster.Default.css" />
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
            right: 8px;
            bottom: 8px;
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
        .messenger-fab {
            position: fixed;
            right: 12px;
            bottom: 130px;
            z-index: 10000;
            width: 42px;
            height: 42px;
            border-radius: 50%;
            border: 1px solid var(--border-subtle);
            background: var(--bg-panel);
            color: var(--text-primary);
            font-size: 18px;
            cursor: pointer;
            backdrop-filter: blur(8px);
        }
        .messenger-panel {
            position: fixed;
            right: 12px;
            bottom: 178px;
            width: 320px;
            max-height: 52vh;
            z-index: 10001;
            background: var(--bg-panel);
            border: 1px solid var(--border-subtle);
            border-radius: 8px;
            padding: 8px;
            font: 12px monospace;
            color: var(--text-primary);
            display: none;
        }
        .messenger-panel.open { display: block; }
        .msg-list { max-height: 110px; overflow-y: auto; border:1px solid #334155; border-radius:6px; padding:6px; margin-bottom:6px; }
        .msg-input { width:100%; box-sizing:border-box; background:#0f172a; color:#e2e8f0; border:1px solid #334155; border-radius:6px; padding:6px; }
        .msg-row { display:flex; gap:6px; }
        .msg-chip { padding:3px 6px; border:1px solid #334155; border-radius:999px; cursor:pointer; margin:2px; display:inline-block; color:#e2e8f0; }
        .feed-modal {
            position: fixed;
            inset: 0;
            background: rgba(2,6,23,0.86);
            z-index: 10020;
            display: none;
            align-items: center;
            justify-content: center;
            padding: 10px;
        }
        .feed-modal.open { display: flex; }
        .feed-card {
            width: min(96vw, 1200px);
            height: min(86vh, 800px);
            min-width: 320px;
            min-height: 220px;
            max-width: 98vw;
            max-height: 92vh;
            background: #020617;
            border: 1px solid #334155;
            border-radius: 8px;
            overflow: hidden;
            display: flex;
            flex-direction: column;
        }
        .feed-header { display:flex; justify-content:space-between; align-items:center; gap:8px; padding:8px; color:#e2e8f0; border-bottom:1px solid #334155; }
        .feed-title { flex:1; min-width:0; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
        .feed-actions { display:flex; gap:6px; flex-shrink:0; }
        .feed-frame { width:100%; height:100%; min-height: 180px; border:0; background:#000; }
        
        /* Sidebar */
        .sidebar {
            position: fixed;
            top: 8px;
            left: 8px;
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
            transition: transform 0.2s ease, opacity 0.2s ease;
            transform: translateX(0);
            opacity: 1;
        }
        .sidebar.collapsed {
            transform: translateX(-120%);
            opacity: 0;
            pointer-events: none;
        }
        .sidebar-toggle {
            position: fixed;
            top: 8px;
            left: 8px;
            z-index: 10000;
            width: 36px;
            height: 36px;
            border-radius: 8px;
            border: 1px solid var(--border-subtle);
            background: var(--bg-panel);
            color: var(--text-primary);
            font-size: 18px;
            cursor: pointer;
            backdrop-filter: blur(8px);
        }
        .sidebar-toggle.shifted { left: 276px; }
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
        .sub-menu { margin-top:6px; padding:6px; border:1px solid #334155; border-radius:6px; }
        .sub-menu label { font-size:11px; margin-right:8px; }
        
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
        #cesiumContainer { display:none; position:absolute; top:0; left:0; width:100%; height:100%; z-index:500; }
    </style>
<link href="https://cesium.com/downloads/cesiumjs/releases/1.110/Build/Cesium/Widgets/widgets.css" rel="stylesheet" />
</head>
<body>
    <div id="map"></div>
    <div id="cesiumContainer"></div>
    
    <button id="messengerFab" class="messenger-fab" onclick="toggleMessenger()">💬</button>
    <div id="messengerPanel" class="messenger-panel">
        <div style="display:flex;justify-content:space-between;align-items:center;margin-bottom:6px;">
            <b>Messenger</b><button class="sb-btn" style="width:auto;padding:2px 8px;margin:0;" onclick="toggleMessenger(false)">✕</button>
        </div>
        <div><span class="sb-label">Entities</span><div id="msgEntities" class="msg-list"></div></div>
        <div><span class="sb-label">Groups</span><div id="msgGroups" class="msg-list"></div></div>
        <div class="msg-row">
            <input id="msgGroupName" class="msg-input" placeholder="New group name" />
            <button class="sb-btn" style="width:auto;margin:0;" onclick="addGroup()">Add</button>
            <button class="sb-btn" style="width:auto;margin:0;" onclick="deleteGroup()">Del</button>
        </div>
        <div style="margin-top:6px;" class="sb-label">To: <span id="msgTarget">(none)</span></div>
        <div id="msgHistory" class="msg-list" style="max-height:90px;"></div>
        <input id="msgText" class="msg-input" placeholder="Type message and press Enter" onkeydown="if(event.key==='Enter'){ sendMessage(); event.preventDefault(); }" />
    </div>
    <div id="feedModal" class="feed-modal" onclick="if(event.target.id==='feedModal') closeCameraFeed()">
        <div class="feed-card">
            <div class="feed-header">
                <span id="feedTitle" class="feed-title">Camera Feed</span>
                <div class="feed-actions">
                    <button class="sb-btn" style="width:auto;margin:0;padding:4px 8px;" onclick="openCameraFeedExternal()">Open External</button>
                    <button class="sb-btn" style="width:auto;margin:0;padding:4px 8px;" onclick="toggleFeedFullscreen()">Full Screen</button>
                    <button class="sb-btn" style="width:auto;margin:0;padding:4px 8px;" onclick="stopLiveFeed()">Stop Feed</button>
                    <button class="sb-btn" style="width:auto;margin:0;padding:4px 8px;" onclick="closeCameraFeed()">Close</button>
                </div>
            </div>
            <iframe id="feedFrame" class="feed-frame" allow="autoplay; fullscreen; encrypted-media" referrerpolicy="no-referrer-when-downgrade"></iframe>
            <video id="feedVideo" class="feed-frame" style="display:none;object-fit:contain;" controls autoplay muted playsinline></video>
            <img id="metaFeedImage" class="feed-frame" style="display:none;object-fit:contain;" />
            <div id="feedHint" style="display:none;padding:8px 10px;background:rgba(15,23,42,0.92);border-top:1px solid #334155;color:#cbd5e1;font-size:11px;">If this feed is blank, the source may block embedding (X-Frame-Options/CSP). Use Open External.</div>
        </div>
    </div>
    <div class="hud">
        <div class="hud-title">● EUD Tactical Map • $callsign</div>
        <div class="hud-row"><span class="hud-label">Status:</span> <span id="status">Initializing...</span></div>
        <div class="hud-row"><span class="hud-label">Entities:</span> <span id="entityCount">0</span></div>
        <div class="hud-row"><span class="hud-label">Position:</span> <span id="position">--</span></div>
    </div>
    
    <button id="sidebarToggle" class="sidebar-toggle shifted" onclick="toggleSidebar()">☰</button>
    <div id="sidebar" class="sidebar">
        <div class="sb-section">
            <div class="sb-label">Settings</div>
            <input id="cfgCallsign" class="sb-input" value="$callsign" placeholder="Callsign" />
            <select id="cfgUnitType" class="sb-input" style="margin-top:6px;">
                <option>Individual Soldier</option>
                <option>HMMWV</option>
                <option>JLTV</option>
                <option>Stryker</option>
                <option>Mechanized</option>
            </select>
            <input id="cfgHub" class="sb-input" value="$hubBase" placeholder="Hub URL" style="margin-top:6px;" />
            <select id="pliModeSel" class="sb-input" style="margin-top:6px;">
                <option value="LOCAL" ${if (pliMode == "LOCAL") "selected" else ""}>LOCAL</option>
                <option value="COP" ${if (pliMode == "COP") "selected" else ""}>COP</option>
                <option value="MERGED" ${if (pliMode == "MERGED") "selected" else ""}>MERGED</option>
            </select>
            <div class="sb-label" style="margin-top:8px;">Conflict Feed Window</div>
            <select id="conflictWindowSel" class="sb-input" style="margin-top:6px;">
                <option value="1d">Day</option>
                <option value="7d">Week</option>
                <option value="30d">Month</option>
                <option value="custom">Custom Range</option>
            </select>
            <input id="conflictDateFrom" class="sb-input" type="date" style="margin-top:6px;" />
            <input id="conflictDateTo" class="sb-input" type="date" style="margin-top:6px;" />
            <input id="conflictCountry" class="sb-input" placeholder="Conflict country filter (optional)" style="margin-top:6px;" />
        </div>
        
        <div class="sb-section">
            <div class="sb-label">Map Layers</div>
            <div class="layer-group">
                <label><input id="layerEntities" type="checkbox" checked onchange="applyLayerVisibility()" /> Entities</label>
                <label><input id="layerHeat" type="checkbox" ${if (pullHeat) "checked" else ""} onchange="applyLayerVisibility()" /> Heat</label>
                <label><input id="layerCams" type="checkbox" ${if (pullCams) "checked" else ""} onchange="applyLayerVisibility()" /> Cams</label>
                <label><input id="layerSat" type="checkbox" ${if (pullSat) "checked" else ""} onchange="applyLayerVisibility()" /> SAT</label>
                <label><input id="layerAdsb" type="checkbox" checked onchange="applyLayerVisibility()" /> ADS-B</label>
                <label><input id="layerConflict" type="checkbox" onchange="applyLayerVisibility()" /> Conflict</label>
                <label><input id="layerShodan" type="checkbox" onchange="applyLayerVisibility()" /> Shodan</label>
            </div>
            <div class="sub-menu">
                <div class="sb-label">SAT Groups</div>
                <label><input type="checkbox" data-sat-group value="stations" checked onchange="applySatGroups()" /> Stations</label>
                <label><input type="checkbox" data-sat-group value="weather" checked onchange="applySatGroups()" /> Weather</label>
                <label><input type="checkbox" data-sat-group value="starlink" checked onchange="applySatGroups()" /> Starlink</label>
                <label><input type="checkbox" data-sat-group value="military" onchange="applySatGroups()" /> Military</label>
                <label><input type="checkbox" data-sat-group value="active" onchange="applySatGroups()" /> Active</label>
                <div style="margin-top:6px;display:flex;gap:6px;align-items:center;">
                    <span style="font-size:11px;color:#94a3b8;">Max</span>
                    <input id="satMaxInput" class="sb-input" type="number" min="20" max="500" step="10" style="max-width:96px;" />
                    <button class="sb-btn" style="width:auto;margin:0;" onclick="applySatMax()">Apply</button>
                </div>
                <button class="sb-btn" style="margin-top:8px;" onclick="pollLocalSatcom(true)">Refresh SAT</button>
                <button class="sb-btn" style="margin-top:8px;" onclick="testCelestrakConnection()">Test CelesTrak</button>
                <div id="satDiag" style="font-size:11px;color:#94a3b8;margin-top:6px;">SAT link: unknown</div>
            </div>
        </div>
        
        <div class="sb-section">
            <button class="sb-btn" onclick="applySettings()">Apply Settings</button>
            <button class="sb-btn" onclick="focusOwn()">Focus EUD</button>
            <button class="sb-btn" onclick="toggle3D()">Toggle 3D SAT</button>
            <button id="northLockBtn" class="sb-btn" onclick="toggleNorthLock()">North Lock: OFF</button>
            <button class="sb-btn" onclick="activateGlasses()">Activate Glasses</button>
            <button class="sb-btn" onclick="grantGlassesCamera()">Grant Glasses Camera</button>
            <button class="sb-btn" onclick="reconnectGlasses()">Reconnect Glasses</button>
            <div id="glassesStatus" style="font-size:11px;color:#94a3b8;margin-top:4px;">Glasses: unknown</div>
            <button class="sb-btn" onclick="reconnectHub()">Reconnect</button>
        </div>
        
        <div class="sb-section">
            <div class="sb-label">Tracked Entities</div>
            <div id="entityList" class="entity-list">No entities tracked</div>
            <div id="entityDetail" class="sub-menu" style="margin-top:8px;">Select an entity for details.</div>
            <div class="sub-menu" style="margin-top:8px;">
                <div class="sb-label">Entity Live Feed</div>
                <div style="display:flex;gap:6px;flex-wrap:wrap;">
                    <button class="sb-btn" onclick="bindLocalGlasses()">Bind Local Glasses</button>
                    <button class="sb-btn" onclick="watchEntityFeed()">Watch Live</button>
                    <button class="sb-btn" onclick="clearEntityFeed()">Clear Feed</button>
                </div>
            </div>
        </div>
    </div>

    <script src="https://unpkg.com/leaflet@1.9.4/dist/leaflet.js"></script>
    <script src="https://unpkg.com/leaflet.markercluster@1.5.3/dist/leaflet.markercluster.js"></script>
    <script src="https://unpkg.com/leaflet.heat@0.2.0/dist/leaflet-heat.js"></script>
    <script src="https://cesium.com/downloads/cesiumjs/releases/1.110/Build/Cesium/Cesium.js"></script>
    <script src="https://unpkg.com/satellite.js@5.0.0/dist/satellite.min.js"></script>
    <script src="https://unpkg.com/h3-js@4.1.0/dist/h3-js.umd.js"></script>
    <script>
        // Configuration
        let OWN_CALLSIGN = $callsignJs;
        const INITIAL_PLI_MODE = $pliModeJs;
        const PULL_ENTITIES_DEFAULT = $pullEntitiesJs;
        const PULL_HEAT_DEFAULT = $pullHeatJs;
        const PULL_CAMS_DEFAULT = $pullCamsJs;
        const PULL_SAT_DEFAULT = $pullSatJs;
        
        // State
        let PLI_MODE = localStorage.getItem('eud:pli_mode') || INITIAL_PLI_MODE;
        let UNIT_TYPE = localStorage.getItem('eud:unit_type') || 'Individual Soldier';
        let currentHub = localStorage.getItem('eud:hub') || document.getElementById('cfgHub').value;
        if (currentHub && !currentHub.endsWith('/')) currentHub += '/';
        let conflictWindow = localStorage.getItem('eud:conflict_window') || '1d';
        let conflictDateFrom = localStorage.getItem('eud:conflict_date_from') || '';
        let conflictDateTo = localStorage.getItem('eud:conflict_date_to') || '';
        let conflictCountry = localStorage.getItem('eud:conflict_country') || '';
        let trackedEntities = [];
        let entityMarkers = {};
        let copCursor = 0;
        let lastDeltaHeatCount = 0;
        let deltaHeatCache = {};
        let camCache = {};
        let lastLocalCamFetchAt = 0;
        const DELTA_HEAT_TTL_MS = 180000;
        const CAM_TTL_MS = 300000;
        const ENTITY_STALE_MS = 180000;
        const GPS_JITTER_METERS = 10;
        const GPS_SMOOTH_ALPHA = 0.25;
        let lastRawOwnPosition = null;
        let ownPosition = { lat: $initLatJs, lon: $initLonJs };
        let layerVisibility = {
            entities: true,
            heat: $pullHeatJs,
            cams: $pullCamsJs,
            sat: $pullSatJs,
            adsb: true,
            conflict: false,
            shodan: false
        };
        
        // Layer markers storage
        let heatMarkers = {};
        let rfHeatLayer = null;
        let wifiHeatLayer = null;
        let camMarkers = {};
        let camCones = {};
        let satMarkers = {};
        let adsbMarkers = {};
        let conflictMarkers = {};
        let shodanMarkers = {};
        let shodanClusterGroup = null;
        let lastConflictFetchAt = 0;
        let lastShodanFetchAt = 0;
        let is3DMode = false;
        let cesiumViewer = null;
        let cesiumNorthLock = false;
        let cesiumSatEntities = {};
        let cesiumEntityEntities = {};
        let cesiumAdsbEntities = {};
        let satSelectedGroups = (() => {
            try {
                const saved = JSON.parse(localStorage.getItem('sat:selectedGroups') || 'null');
                return Array.isArray(saved) && saved.length ? saved : ['stations','weather','starlink'];
            } catch (_) { return ['stations','weather','starlink']; }
        })();
        let satLastDiag = { ok: false, group: '-', count: 0, at: 0, err: '' };
        let satLastPushAt = 0;
        const SAT_PUSH_INTERVAL_MS = 60000;
        let satMaxMarkers = (() => {
            const n = parseInt(localStorage.getItem('sat:maxMarkers') || '180', 10);
            return Number.isFinite(n) ? Math.max(20, Math.min(500, n)) : 180;
        })();
        let selectedEntityUid = null;
        let entityFeedMap = (() => {
            try { return JSON.parse(localStorage.getItem('eud:entity_feeds') || '{}') || {}; }
            catch (_) { return {}; }
        })();
        let deltaCamCache = {};
        let deltaSatCache = {};
        
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

        // Viewport-based camera fetch (hub-style)
        const CCTV_MAX_BBOX_DEG2 = 0.08; // guardrail for large viewports
        let cctvFetchInFlight = false;
        function getViewportBbox() {
            if (!map) return null;
            const b = map.getBounds();
            if (!b) return null;
            const s = b.getSouth();
            const n = b.getNorth();
            const w = b.getWest();
            const e = b.getEast();
            const areaDeg2 = Math.abs((n - s) * (e - w));
            if (areaDeg2 > CCTV_MAX_BBOX_DEG2) return null; // Too large, skip
            return { s, n, w, e };
        }
        let lastCctvBboxKey = '';
        async function fetchViewportCameras(force = false) {
            if (cctvFetchInFlight) return;
            const bbox = getViewportBbox();
            if (!bbox) return;
            const key = [bbox.s.toFixed(5), bbox.n.toFixed(5), bbox.w.toFixed(5), bbox.e.toFixed(5)].join(',');
            if (!force && key === lastCctvBboxKey) return; // already fetched this bbox
            lastCctvBboxKey = key;
            cctvFetchInFlight = true;
            try {
                const q = '[out:json][timeout:20];(node["man_made"="surveillance"](' + bbox.s.toFixed(6) + ',' + bbox.w.toFixed(6) + ',' + bbox.n.toFixed(6) + ',' + bbox.e.toFixed(6) + ');node["surveillance:type"](' + bbox.s.toFixed(6) + ',' + bbox.w.toFixed(6) + ',' + bbox.n.toFixed(6) + ',' + bbox.e.toFixed(6) + '););out body 120;';
                const resp = await fetch('https://overpass-api.de/api/interpreter', {
                    method: 'POST',
                    headers: { 'Content-Type': 'text/plain' },
                    body: q,
                });
                if (!resp.ok) { cctvFetchInFlight = false; return; }
                const js = await resp.json();
                const elems = Array.isArray(js?.elements) ? js.elements : [];
                const cams = elems.map((e, idx) => {
                    const tags = e?.tags || {};
                    const feedUrl = tags['contact:webcam'] || tags['camera:url'] || tags['surveillance:feed'] || tags['image'] || tags['url'] || tags['website'] || null;
                    return {
                        tile_id: 'android_' + Math.round(Number(e.lat || 0) * 10000) + '_' + Math.round(Number(e.lon || 0) * 10000),
                        dimension: 'viewport-osm',
                        count: 1,
                        bearing: Number(tags.direction || 0) || 0,
                        fov: 70,
                        id: 'osm-cam-' + (e.id || idx),
                        name: tags.name || ('OSM Camera ' + (e.id || idx)),
                        snapshotUrl: feedUrl,
                        sourceType: feedUrl ? 'PUBLIC' : 'OSM'
                    };
                }).filter(c => c.tile_id.includes('android_'));
                if (cams.length > 0) {
                    upsertCameraBatch(cams, 'viewport');
                    renderCameras(getCameraSnapshot());
                }
            } catch (e) {
                console.log('Viewport camera fetch failed:', e.message);
            } finally {
                cctvFetchInFlight = false;
            }
        }
        // Fetch cameras on map move/zoom
        map.on('moveend zoomend', () => {
            if (map.getZoom() >= 12) { // sensible zoom threshold like desktop
                fetchViewportCameras(false);
            }
        });

        // Update PLI mode selector
        document.getElementById('pliModeSel').value = PLI_MODE;
        document.getElementById('cfgUnitType').value = UNIT_TYPE;
        document.getElementById('conflictWindowSel').value = conflictWindow;
        document.getElementById('conflictDateFrom').value = conflictDateFrom;
        document.getElementById('conflictDateTo').value = conflictDateTo;
        document.getElementById('conflictCountry').value = conflictCountry;
        toggleSidebar(localStorage.getItem('eud:sidebar_collapsed') === '1');
        document.querySelectorAll('input[data-sat-group]').forEach(chk => {
            chk.checked = satSelectedGroups.includes(chk.value);
        });
        const satMaxInput = document.getElementById('satMaxInput');
        if (satMaxInput) satMaxInput.value = String(satMaxMarkers);

        function syncConflictDateInputs() {
            const custom = document.getElementById('conflictWindowSel').value === 'custom';
            const from = document.getElementById('conflictDateFrom');
            const to = document.getElementById('conflictDateTo');
            if (from) from.style.display = custom ? 'block' : 'none';
            if (to) to.style.display = custom ? 'block' : 'none';
        }
        document.getElementById('conflictWindowSel').addEventListener('change', syncConflictDateInputs);
        syncConflictDateInputs();

        function ensureCesiumViewer() {
            if (cesiumViewer || !window.Cesium) return;
            cesiumViewer = new Cesium.Viewer('cesiumContainer', {
                terrainProvider: new Cesium.EllipsoidTerrainProvider(),
                animation: false,
                timeline: false,
                sceneModePicker: true,
                baseLayerPicker: true,
                geocoder: false,
                homeButton: true,
                navigationHelpButton: true,
                infoBox: true,
                selectionIndicator: true,
            });

            cesiumViewer.scene.preRender.addEventListener(() => {
                if (!cesiumNorthLock) return;
                const cam = cesiumViewer.camera;
                cam.setView({
                    destination: cam.position,
                    orientation: {
                        heading: 0.0,
                        pitch: cam.pitch,
                        roll: 0.0,
                    }
                });
            });
        }

        function toggle3D() {
            const c = document.getElementById('cesiumContainer');
            if (!is3DMode) {
                ensureCesiumViewer();
                c.style.display = 'block';
                is3DMode = true;
                if (cesiumViewer && ownPosition) {
                    cesiumViewer.camera.flyTo({ destination: Cesium.Cartesian3.fromDegrees(ownPosition.lon, ownPosition.lat, 2000000) });
                }
            } else {
                c.style.display = 'none';
                is3DMode = false;
            }
        }

        function toggleNorthLock() {
            cesiumNorthLock = !cesiumNorthLock;
            const b = document.getElementById('northLockBtn');
            if (b) b.textContent = 'North Lock: ' + (cesiumNorthLock ? 'ON' : 'OFF');
        }

        let currentFeedUrl = '';
        let metaFeedTimer = null;

        function detectFeedType(url) {
            const u = String(url || '').toLowerCase();
            if (!u) return 'unknown';
            if (u.startsWith('meta://')) return 'meta';
            if (u.startsWith('rtsp://') || u.startsWith('rtmp://')) return 'rtsp';
            if (u.includes('.m3u8')) return 'hls';
            if (u.match(/\.(mp4|webm|mov)(\?|$)/)) return 'video';
            if (u.match(/\.(jpg|jpeg|png|gif|webp|mjpg|mjpeg)(\?|$)/)) return 'image';
            if (u.includes('/view') || u.includes('/camera') || u.includes('/live') || u.includes('/webcam') || u.includes('earthcam') || u.includes('youtube.com') || u.includes('youtu.be')) return 'page';
            return 'unknown';
        }

        async function ensureHlsJs() {
            if (window.Hls) return true;
            return new Promise((resolve) => {
                const s = document.createElement('script');
                s.src = 'https://cdn.jsdelivr.net/npm/hls.js@latest';
                s.onload = () => resolve(!!window.Hls);
                s.onerror = () => resolve(false);
                document.head.appendChild(s);
            });
        }

        function tryExtractDirectFeedFromText(html, baseUrl) {
            if (!html) return '';
            const candidates = [];
            const abs = (u) => {
                try { return new URL(u, baseUrl).toString(); } catch (_) { return u; }
            };

            const mediaRegex = /https?:\/\/[^"'\s<>]+\.(m3u8|mp4|webm|mov|mjpg|mjpeg|jpg|jpeg|png)(\?[^"'\s<>]*)?/gi;
            for (const m of html.matchAll(mediaRegex)) candidates.push(m[0]);

            const attrRegex = /(?:content|src)=["']([^"']+)["']/gi;
            for (const m of html.matchAll(attrRegex)) {
                const raw = m[1] || '';
                const lc = raw.toLowerCase();
                if (lc.includes('.m3u8') || lc.includes('.mp4') || lc.includes('.webm') || lc.includes('.mov') || lc.includes('.mjpg') || lc.includes('.mjpeg') || lc.includes('.jpg') || lc.includes('.jpeg') || lc.includes('.png')) {
                    candidates.push(abs(raw));
                }
            }

            const score = (u) => {
                const lc = String(u).toLowerCase();
                let s = 0;
                if (lc.includes('.m3u8')) s += 120;
                if (lc.match(/\.(mp4|webm|mov)(\?|$)/)) s += 100;
                if (lc.includes('.mjpg') || lc.includes('.mjpeg')) s += 90;
                if (lc.match(/\.(jpg|jpeg|png)(\?|$)/)) s += 40;
                if (lc.includes('stream') || lc.includes('live') || lc.includes('playlist')) s += 20;
                if (lc.includes('logo') || lc.includes('sprite') || lc.includes('thumbnail') || lc.includes('placeholder')) s -= 80;
                if (lc.includes('earthcam') && (lc.includes('logo') || lc.includes('default'))) s -= 120;
                return s;
            };

            candidates.sort((a,b) => score(b)-score(a));
            return candidates[0] || '';
        }

        function fitFeedCardToAspect(srcW, srcH) {
            const card = document.querySelector('#feedModal .feed-card');
            if (!card) return;
            const w = Number(srcW || 0);
            const h = Number(srcH || 0);
            if (!w || !h) return;

            const headerH = 42;
            const maxW = Math.floor(window.innerWidth * 0.96);
            const maxH = Math.floor(window.innerHeight * 0.90) - headerH;

            let targetW = w;
            let targetH = h;
            const scale = Math.min(maxW / targetW, maxH / targetH, 1);
            targetW = Math.max(320, Math.floor(targetW * scale));
            targetH = Math.max(180, Math.floor(targetH * scale));

            card.style.width = targetW + 'px';
            card.style.height = (targetH + headerH) + 'px';
        }

        function resetFeedCardSize() {
            const card = document.querySelector('#feedModal .feed-card');
            if (!card) return;
            card.style.width = 'min(96vw, 1200px)';
            card.style.height = 'min(86vh, 800px)';
        }

        async function openCameraFeed(url) {
            if (!url) return;
            currentFeedUrl = url;
            const modal = document.getElementById('feedModal');
            const frame = document.getElementById('feedFrame');
            const video = document.getElementById('feedVideo');
            const img = document.getElementById('metaFeedImage');
            const hint = document.getElementById('feedHint');
            const title = document.getElementById('feedTitle');
            if (title) title.textContent = 'Camera Feed • ' + url;

            if (metaFeedTimer) { clearInterval(metaFeedTimer); metaFeedTimer = null; }
            resetFeedCardSize();

            if (frame) { frame.style.display = 'none'; frame.src = 'about:blank'; }
            if (video) {
                video.style.display = 'none';
                try { video.pause(); } catch (_) {}
                video.src = '';
                video.load();
            }
            if (img) { img.style.display = 'none'; img.src = ''; }
            if (hint) hint.style.display = 'none';

            let resolvedUrl = url;
            let feedType = detectFeedType(resolvedUrl);

            if (feedType === 'meta') {
                const uid = String(resolvedUrl).replace('meta://', '');
                if (img) {
                    img.style.display = 'block';
                    img.onload = () => fitFeedCardToAspect(img.naturalWidth, img.naturalHeight);
                }
                let misses = 0;
                let reconnectAttempted = false;
                metaFeedTimer = setInterval(() => {
                    try {
                        if (!window.AndroidBridge || typeof window.AndroidBridge.getMetaFrameBase64 !== 'function') return;
                        const b64 = window.AndroidBridge.getMetaFrameBase64(uid);
                        if (b64 && img) {
                            img.src = 'data:image/jpeg;base64,' + b64;
                            misses = 0;
                            return;
                        }
                        misses += 1;
                        if (!reconnectAttempted && misses > 12 && window.AndroidBridge && typeof window.AndroidBridge.reconnectLocalGlasses === 'function') {
                            reconnectAttempted = true;
                            try { window.AndroidBridge.reconnectLocalGlasses(uid); } catch (_) {}
                        }
                    } catch (_) {}
                }, 180);
                if (modal) modal.classList.add('open');
                return;
            }

            // Hub parity: try resolving page/unknown URLs to direct media first.
            if (feedType === 'page' || feedType === 'unknown') {
                try {
                    const resp = await fetch(resolvedUrl);
                    if (resp.ok) {
                        const html = await resp.text();
                        const extracted = tryExtractDirectFeedFromText(html, resolvedUrl);
                        if (extracted) {
                            resolvedUrl = extracted;
                            feedType = detectFeedType(resolvedUrl);
                            currentFeedUrl = resolvedUrl;
                            if (title) title.textContent = 'Camera Feed • ' + resolvedUrl;
                        }
                    }
                } catch (_) {
                    // Keep original page URL fallback
                }
            }

            if (feedType === 'video' || feedType === 'hls') {
                if (video) {
                    video.style.display = 'block';
                    video.onloadedmetadata = () => fitFeedCardToAspect(video.videoWidth || 1280, video.videoHeight || 720);
                    if (feedType === 'hls' && !video.canPlayType('application/vnd.apple.mpegurl')) {
                        const ok = await ensureHlsJs();
                        if (ok && window.Hls?.isSupported?.()) {
                            const hls = new window.Hls();
                            hls.loadSource(resolvedUrl);
                            hls.attachMedia(video);
                        } else {
                            if (hint) {
                                hint.textContent = 'HLS unsupported in this environment. Use Open External.';
                                hint.style.display = 'block';
                            }
                        }
                    } else {
                        video.src = resolvedUrl;
                    }
                    video.play().catch(() => {});
                }
                fitFeedCardToAspect(1280, 720);
            } else if (feedType === 'rtsp') {
                if (hint) {
                    hint.textContent = 'RTSP/RTMP is not browser-native here. Use Open External or a transcoded HTTP/HLS feed.';
                    hint.style.display = 'block';
                }
                if (frame) {
                    frame.style.display = 'block';
                    frame.src = 'about:blank';
                }
                fitFeedCardToAspect(960, 540);
            } else if (feedType === 'image') {
                if (img) {
                    img.style.display = 'block';
                    img.onload = () => fitFeedCardToAspect(img.naturalWidth, img.naturalHeight);
                    img.src = resolvedUrl;
                }
            } else {
                // Hard hub parity for source-page feeds: open in a dedicated in-app source window
                // instead of relying on nested iframe embedding.
                if (window.AndroidBridge && typeof window.AndroidBridge.openFeedSourceInApp === 'function') {
                    try { window.AndroidBridge.openFeedSourceInApp(resolvedUrl, 'CCTV Source'); } catch (_) {}
                    document.getElementById('status').textContent = 'Opened source feed in dedicated in-app viewer';
                    return;
                }
                if (frame) {
                    frame.style.display = 'block';
                    frame.src = resolvedUrl;
                }
                if (hint) {
                    hint.textContent = 'If this feed is blank, the source may block embedding (X-Frame-Options/CSP). Use Open External.';
                    hint.style.display = 'block';
                }
                fitFeedCardToAspect(1280, 720);
            }
            if (modal) modal.classList.add('open');
        }

        function closeCameraFeed() {
            const modal = document.getElementById('feedModal');
            const frame = document.getElementById('feedFrame');
            const video = document.getElementById('feedVideo');
            const img = document.getElementById('metaFeedImage');
            const hint = document.getElementById('feedHint');
            if (metaFeedTimer) { clearInterval(metaFeedTimer); metaFeedTimer = null; }
            if (frame) frame.src = 'about:blank';
            if (video) {
                try { video.pause(); } catch (_) {}
                video.src = '';
                video.load();
            }
            if (img) img.src = '';
            if (hint) hint.style.display = 'none';
            if (document.fullscreenElement) {
                try { document.exitFullscreen(); } catch (_) {}
            }
            if (modal) modal.classList.remove('open');
        }

        function stopLiveFeed() {
            const isMeta = String(currentFeedUrl || '').startsWith('meta://');
            if (isMeta && window.AndroidBridge && typeof window.AndroidBridge.stopLocalGlassesStream === 'function') {
                try { window.AndroidBridge.stopLocalGlassesStream(); } catch (_) {}
            }
            closeCameraFeed();
            currentFeedUrl = '';
            document.getElementById('status').textContent = 'Live feed stopped';
            refreshGlassesStatus();
        }

        async function toggleFeedFullscreen() {
            const card = document.querySelector('#feedModal .feed-card');
            if (!card) return;
            try {
                if (!document.fullscreenElement) {
                    await card.requestFullscreen();
                } else {
                    await document.exitFullscreen();
                }
            } catch (_) {}
        }

        function openCameraFeedExternal() {
            if (!currentFeedUrl) return;
            if (String(currentFeedUrl).startsWith('meta://')) {
                document.getElementById('status').textContent = 'Local Meta feed is in-app only';
                return;
            }
            if (window.AndroidBridge && typeof window.AndroidBridge.openExternalUrl === 'function') {
                window.AndroidBridge.openExternalUrl(currentFeedUrl);
            } else {
                window.open(currentFeedUrl, '_blank');
            }
        }

        function activateGlasses() {
            try {
                if (window.AndroidBridge && typeof window.AndroidBridge.activateGlasses === 'function') {
                    window.AndroidBridge.activateGlasses();
                    document.getElementById('status').textContent = 'Starting glasses activation...';
                }
            } catch (_) {}
        }

        function grantGlassesCamera() {
            try {
                if (window.AndroidBridge && typeof window.AndroidBridge.requestGlassesCameraPermission === 'function') {
                    window.AndroidBridge.requestGlassesCameraPermission();
                    document.getElementById('status').textContent = 'Requesting glasses camera permission...';
                }
            } catch (_) {}
        }

        function reconnectGlasses() {
            try {
                if (!selectedEntityUid) {
                    document.getElementById('status').textContent = 'Select an entity first for glasses reconnect';
                    return;
                }
                if (window.AndroidBridge && typeof window.AndroidBridge.reconnectLocalGlasses === 'function') {
                    const ok = !!window.AndroidBridge.reconnectLocalGlasses(selectedEntityUid);
                    document.getElementById('status').textContent = ok ? ('Glasses reconnected to ' + selectedEntityUid) : 'Glasses reconnect failed';
                }
                refreshGlassesStatus();
            } catch (_) {}
        }

        function refreshGlassesStatus() {
            try {
                if (!window.AndroidBridge || typeof window.AndroidBridge.getGlassesStatusJson !== 'function') return;
                const js = window.AndroidBridge.getGlassesStatusJson();
                const st = JSON.parse(js || '{}');
                const el = document.getElementById('glassesStatus');
                const ageMs = Number(st.frame_age_ms || -1);
                const ageTxt = ageMs >= 0 ? (' (' + Math.round(ageMs/1000) + 's)') : '';
                if (el) el.textContent = 'Glasses: ' + (st.registration || 'UNKNOWN') + ' • Cam:' + (st.camera_permission || 'UNKNOWN') + ' • ' + (st.stream || 'IDLE') + ' • Frame:' + (st.frame_ready || 'NO') + ageTxt;
            } catch (_) {}
        }

        function upsertCameraBatch(cams, source='unknown') {
            const now = Date.now();
            (cams || []).forEach(cam => {
                const k = String(cam.id || cam.tile_id || ((cam.lat || 0) + ',' + (cam.lon || 0)));
                if (!k) return;
                camCache[k] = { ...camCache[k], ...cam, _ts: now, _source: source };
            });
            Object.keys(camCache).forEach(k => {
                if ((now - (camCache[k]._ts || 0)) > CAM_TTL_MS) delete camCache[k];
            });
        }

        function getCameraSnapshot() {
            const now = Date.now();
            Object.keys(camCache).forEach(k => {
                if ((now - (camCache[k]._ts || 0)) > CAM_TTL_MS) delete camCache[k];
            });
            return Object.values(camCache).map(c => ({ ...c }));
        }

        let msgGroups = JSON.parse(localStorage.getItem('eud:msg_groups') || '[]');
        let msgTarget = null;
        let msgStore = JSON.parse(localStorage.getItem('eud:msg_store') || '{}');

        function toggleMessenger(forceOpen = null) {
            const p = document.getElementById('messengerPanel');
            const open = forceOpen === null ? !p.classList.contains('open') : !!forceOpen;
            p.classList.toggle('open', open);
            if (open) renderMessenger();
        }
        function selectMsgTarget(t) { msgTarget = t; renderMessenger(); }
        function addGroup() {
            const n = (document.getElementById('msgGroupName').value || '').trim();
            if (!n) return;
            if (!msgGroups.includes(n)) msgGroups.push(n);
            localStorage.setItem('eud:msg_groups', JSON.stringify(msgGroups));
            document.getElementById('msgGroupName').value = '';
            renderMessenger();
        }
        function deleteGroup() {
            if (!msgTarget || !msgTarget.startsWith('group:')) return;
            const g = msgTarget.replace('group:', '');
            msgGroups = msgGroups.filter(x => x !== g);
            delete msgStore[msgTarget];
            localStorage.setItem('eud:msg_groups', JSON.stringify(msgGroups));
            localStorage.setItem('eud:msg_store', JSON.stringify(msgStore));
            msgTarget = null;
            renderMessenger();
        }
        function sendMessage() {
            const inp = document.getElementById('msgText');
            const txt = (inp.value || '').trim();
            if (!txt || !msgTarget) return;
            const arr = msgStore[msgTarget] || [];
            arr.push({ at: Date.now(), from: OWN_CALLSIGN, text: txt });
            msgStore[msgTarget] = arr.slice(-50);
            localStorage.setItem('eud:msg_store', JSON.stringify(msgStore));
            inp.value = ''; // clear on Enter/send
            renderMessenger();
        }
        function renderMessenger() {
            const ents = trackedEntities
                .filter(e => e.uid !== OWN_CALLSIGN && hasAssignedCallsign(e.callsign || e.uid))
                .map(e => '<span class="msg-chip" onclick="selectMsgTarget(\'entity:' + e.uid + '\')">' + (e.callsign || e.uid) + '</span>')
                .join('') || '<span style="color:#94a3b8;">No entities</span>';
            const grps = msgGroups.map(g => '<span class="msg-chip" onclick="selectMsgTarget(\'group:' + g + '\')"># ' + g + '</span>').join('') || '<span style="color:#94a3b8;">No groups</span>';
            document.getElementById('msgEntities').innerHTML = ents;
            document.getElementById('msgGroups').innerHTML = grps;
            document.getElementById('msgTarget').textContent = msgTarget || '(none)';
            const hist = (msgTarget && msgStore[msgTarget]) ? msgStore[msgTarget] : [];
            document.getElementById('msgHistory').innerHTML = hist.map(m => '<div><b>' + m.from + '</b>: ' + m.text + '</div>').join('') || '<span style="color:#94a3b8;">No messages</span>';
        }

        function toggleSidebar(forceState = null) {
            const sb = document.getElementById('sidebar');
            const btn = document.getElementById('sidebarToggle');
            if (!sb || !btn) return;
            const collapse = forceState === null ? !sb.classList.contains('collapsed') : !!forceState;
            sb.classList.toggle('collapsed', collapse);
            btn.classList.toggle('shifted', !collapse);
            btn.textContent = collapse ? '☰' : '✕';
            localStorage.setItem('eud:sidebar_collapsed', collapse ? '1' : '0');
        }
        
        // ===== ENTITY MANAGEMENT =====

        function hasAssignedCallsign(v) {
            const s = String(v || '').trim();
            if (!s) return false;
            const l = s.toLowerCase();
            if (l === 'unknown' || l === 'entity' || l === 'device') return false;
            if (/^android_\d+_\d+$/i.test(s)) return false;
            const compact = s.replace(/[-_:]/g, '');
            if (/^[0-9a-f]{12,}$/i.test(compact)) return false; // hashes/mac-like ids
            if (!/[a-z]/i.test(s)) return false; // require at least one letter
            return true;
        }
        
        function ingestPLI(pli) {
            // pli = { uid, callsign, type, affiliation, lat, lon, timestamp }
            const displayName = (pli.callsign || pli.uid || '').trim();
            if (pli.uid !== OWN_CALLSIGN && !hasAssignedCallsign(displayName)) return;
            const dedupeKey = String(displayName).toUpperCase();
            const existing = trackedEntities.findIndex(e =>
                e.uid === pli.uid || String((e.callsign || e.uid || '')).trim().toUpperCase() === dedupeKey
            );
            const now = Date.now();

            if (existing >= 0) {
                const prevUid = trackedEntities[existing].uid;
                trackedEntities[existing] = {
                    ...trackedEntities[existing],
                    ...pli,
                    lastSeen: now
                };
                const newUid = trackedEntities[existing].uid;
                if (prevUid && newUid && prevUid !== newUid && entityMarkers[prevUid] && !entityMarkers[newUid]) {
                    entityMarkers[newUid] = entityMarkers[prevUid];
                    delete entityMarkers[prevUid];
                }
            } else {
                trackedEntities.push({
                    ...pli,
                    lastSeen: now,
                    affiliation: pli.affiliation || 'friendly'
                });
            }

            pruneAndDedupeEntities();
            const target = trackedEntities.find(e => e.uid === pli.uid) || trackedEntities.find(e => String((e.callsign||'')).toUpperCase() === dedupeKey);
            if (target) updateEntityOnMap(target);
            renderEntityList();
            updateStatus();
        }
        
        function pruneAndDedupeEntities() {
            const now = Date.now();
            const newestByKey = {};

            trackedEntities.forEach(e => {
                const key = String((e.callsign || e.uid || '')).trim().toUpperCase();
                if (!key) return;
                const stale = (now - (e.lastSeen || 0)) > ENTITY_STALE_MS;
                if (stale && e.uid !== OWN_CALLSIGN) return;
                const prev = newestByKey[key];
                if (!prev || (e.lastSeen || 0) > (prev.lastSeen || 0)) newestByKey[key] = e;
            });

            const keepUids = new Set(Object.values(newestByKey).map(e => e.uid));
            Object.keys(entityMarkers).forEach(uid => {
                if (!keepUids.has(uid)) {
                    if (map.hasLayer(entityMarkers[uid])) map.removeLayer(entityMarkers[uid]);
                    delete entityMarkers[uid];
                    if (cesiumViewer && cesiumEntityEntities[uid]) {
                        cesiumViewer.entities.remove(cesiumEntityEntities[uid]);
                        delete cesiumEntityEntities[uid];
                    }
                }
            });

            trackedEntities = Object.values(newestByKey);
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
            
            const entityFeed = getEntityFeedUrl(entity);
            const popupHtml = '<b>' + (entity.callsign || entity.uid) + '</b><br>'
                + (entity.type || 'Unknown') + '<br>'
                + entity.lat.toFixed(5) + ', ' + entity.lon.toFixed(5)
                + (entityFeed ? ('<br><button class="sb-btn" style="width:auto;padding:4px 8px;" onclick="openCameraFeed(\'' + String(entityFeed).replace(/'/g, "\\'") + '\')">Live Video</button>') : '');

            entityMarkers[entity.uid]
                .bindPopup(popupHtml)
                .bindTooltip(entity.callsign || entity.uid, { 
                    permanent: true, 
                    direction: 'top', 
                    offset: [0, -16] 
                });

            entityMarkers[entity.uid].off('mouseover');
            entityMarkers[entity.uid].on('mouseover', () => {
                const f = getEntityFeedUrl(entity);
                if (f) openCameraFeed(f);
            });
            
            // Force marker visible
            if (!map.hasLayer(entityMarkers[entity.uid])) {
                entityMarkers[entity.uid].addTo(map);
            }

            if (cesiumViewer) {
                const key = entity.uid;
                const color = isOwn ? Cesium.Color.CYAN : Cesium.Color.LIME;
                if (!cesiumEntityEntities[key]) {
                    cesiumEntityEntities[key] = cesiumViewer.entities.add({
                        id: 'ent-' + key,
                        position: Cesium.Cartesian3.fromDegrees(entity.lon, entity.lat, 20),
                        point: { pixelSize: isOwn ? 10 : 8, color, outlineColor: Cesium.Color.WHITE, outlineWidth: 1 },
                        label: { text: entity.callsign || key, font: '12px sans-serif', fillColor: color, pixelOffset: new Cesium.Cartesian2(0, -14) }
                    });
                } else {
                    cesiumEntityEntities[key].position = Cesium.Cartesian3.fromDegrees(entity.lon, entity.lat, 20);
                    cesiumEntityEntities[key].label.text = entity.callsign || key;
                }
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
                const stale = age > Math.floor(ENTITY_STALE_MS / 1000);
                const affilClass = 'tac-' + (e.affiliation || 'unknown');
                const symbolChar = e.affiliation === 'friendly' ? '◈' : 
                                  e.affiliation === 'hostile' ? '◉' : 
                                  e.affiliation === 'neutral' ? '◐' : '◆';
                const label = (e.callsign || e.uid);
                
                return '<div class="entity-item" style="opacity:' + (stale ? 0.5 : 1) + '" onclick="focusEntity(\'' + e.uid + '\')" onmouseenter="selectMsgTarget(\'entity:' + e.uid + '\')">' +
                       '<div class="tac-symbol ' + affilClass + '" style="width:20px;height:20px;font-size:10px;">' + symbolChar + '</div>' +
                       '<div style="flex:1;">' +
                       '<div style="font-weight:bold;">' + label + '</div>' +
                       '<div style="font-size:10px;color:#94a3b8;">' + ageStr + (stale ? ' STALE' : '') + '</div>' +
                       '</div>' +
                       '<button class="sb-btn" style="width:auto;padding:2px 6px;margin:0;" onclick="event.stopPropagation();selectMsgTarget(\'entity:' + e.uid + '\');toggleMessenger(true)">💬</button>' +
                       '</div>';
            }).join('');
            renderMessenger();
        }
        
        function applyLayerVisibility() {
            layerVisibility.entities = document.getElementById('layerEntities').checked;
            layerVisibility.heat = document.getElementById('layerHeat').checked;
            layerVisibility.cams = document.getElementById('layerCams').checked;
            layerVisibility.sat = document.getElementById('layerSat').checked;
            layerVisibility.adsb = document.getElementById('layerAdsb').checked;
            layerVisibility.conflict = document.getElementById('layerConflict').checked;
            layerVisibility.shodan = document.getElementById('layerShodan').checked;
            
            Object.values(entityMarkers).forEach(m => {
                if (layerVisibility.entities) {
                    if (!map.hasLayer(m)) m.addTo(map);
                } else {
                    if (map.hasLayer(m)) map.removeLayer(m);
                }
            });
            
            [rfHeatLayer, wifiHeatLayer].forEach(layer => {
                if (!layer) return;
                if (layerVisibility.heat) {
                    if (!map.hasLayer(layer)) layer.addTo(map);
                } else {
                    if (map.hasLayer(layer)) map.removeLayer(layer);
                }
            });
            
            Object.values(camMarkers).forEach(m => {
                if (layerVisibility.cams) {
                    if (!map.hasLayer(m)) m.addTo(map);
                } else {
                    if (map.hasLayer(m)) map.removeLayer(m);
                }
            });
            Object.values(camCones).forEach(m => {
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

            Object.values(conflictMarkers).forEach(m => {
                if (layerVisibility.conflict) {
                    if (!map.hasLayer(m)) m.addTo(map);
                } else {
                    if (map.hasLayer(m)) map.removeLayer(m);
                }
            });

            if (layerVisibility.shodan) {
                if (!shodanClusterGroup && window.L && L.markerClusterGroup) {
                    shodanClusterGroup = L.markerClusterGroup({ showCoverageOnHover: false, maxClusterRadius: 50 });
                }
                if (shodanClusterGroup && !map.hasLayer(shodanClusterGroup)) map.addLayer(shodanClusterGroup);
                Object.values(shodanMarkers).forEach(m => { if (shodanClusterGroup && !shodanClusterGroup.hasLayer(m)) shodanClusterGroup.addLayer(m); });
            } else {
                if (shodanClusterGroup) shodanClusterGroup.clearLayers();
                if (shodanClusterGroup && map.hasLayer(shodanClusterGroup)) map.removeLayer(shodanClusterGroup);
                Object.values(shodanMarkers).forEach(m => { if (map.hasLayer(m)) map.removeLayer(m); });
            }

            if (layerVisibility.conflict) {
                pollConflictEvents(false);
            }
            if (layerVisibility.shodan) {
                pollShodanEvents(false);
            }
        }

        async function pollConflictEvents(force=false) {
            const now = Date.now();
            if (!force && (now - lastConflictFetchAt) < 15_000) return;
            lastConflictFetchAt = now;
            try {
                const qs = new URLSearchParams();
                qs.set('limit', '2000');
                if (conflictWindow === 'custom') {
                    if (conflictDateFrom) qs.set('date_from', conflictDateFrom);
                    if (conflictDateTo) qs.set('date_to', conflictDateTo);
                    if (!conflictDateFrom && !conflictDateTo) qs.set('window', '1d');
                } else {
                    qs.set('window', conflictWindow || '1d');
                }
                if (conflictCountry) qs.set('country', conflictCountry);
                let resp = await fetch(currentHub + 'api/events?' + qs.toString());
                if (!resp.ok) {
                    // Fallback to OSINT sidecar on :8790 if hub API doesn't expose /api/events
                    try {
                        const u = new URL(currentHub);
                        const sidecarBase = u.protocol + '//' + u.hostname + ':8790/';
                        resp = await fetch(sidecarBase + 'api/events?' + qs.toString());
                    } catch (_) {}
                }
                if (!resp.ok) return;
                const rows = await resp.json();
                const seen = new Set();
                (Array.isArray(rows) ? rows : []).forEach(ev => {
                    const lat = Number(ev.latitude);
                    const lon = Number(ev.longitude);
                    if (!isFinite(lat) || !isFinite(lon)) return;
                    const key = String(ev.external_id || ev.id || (lat.toFixed(5) + ',' + lon.toFixed(5) + ':' + (ev.event_date || '')));
                    seen.add(key);

                    const fatal = Number(ev.fatalities || 0);
                    const color = fatal >= 10 ? '#ef4444' : '#f59e0b';
                    const fireHtml = '<div style="width:20px;height:20px;display:flex;align-items:center;justify-content:center;color:' + color + ';font-size:18px;line-height:1;filter: drop-shadow(0 0 2px rgba(0,0,0,0.8));">🔥</div>';
                    const fireIcon = L.divIcon({ html: fireHtml, className: 'conflict-fire-icon', iconSize: [20,20], iconAnchor: [10,10] });
                    const sources = Array.isArray(ev.sources) ? ev.sources : [];
                    const srcHtml = sources.length
                        ? '<ul style="margin:4px 0 0 16px;padding:0;">' + sources.map(s => {
                            const name = String(s.name || 'source');
                            const url = String(s.url || '');
                            return url ? ('<li><a href="' + url + '" target="_blank">' + name + '</a></li>') : ('<li>' + name + '</li>');
                        }).join('') + '</ul>'
                        : '<div style="font-size:11px;color:#94a3b8;">No direct source links</div>';

                    const popup = '<div style="min-width:240px;">'
                        + '<b>' + (ev.event_type || 'Conflict event') + '</b><br>'
                        + '<span style="font-size:11px;color:#94a3b8;">' + (ev.event_date || '') + ' • ' + (ev.location || '-') + ', ' + (ev.country || '-') + '</span><br>'
                        + '<b>Actors:</b> ' + (ev.actor1 || 'N/A') + (ev.actor2 ? (' vs ' + ev.actor2) : '') + '<br>'
                        + '<b>Fatalities:</b> ' + (ev.fatalities ?? 'Unknown') + '<br>'
                        + '<b>Notes:</b> ' + (ev.notes || 'N/A') + '<br>'
                        + '<b>Sources:</b>' + srcHtml
                        + '</div>';

                    let marker = conflictMarkers[key];
                    if (!marker) {
                        marker = L.marker([lat, lon], { icon: fireIcon });
                        marker.bindPopup(popup);
                        conflictMarkers[key] = marker;
                    } else {
                        marker.setLatLng([lat, lon]);
                        marker.setIcon(fireIcon);
                        marker.bindPopup(popup);
                    }

                    if (layerVisibility.conflict && !map.hasLayer(marker)) marker.addTo(map);
                    if (!layerVisibility.conflict && map.hasLayer(marker)) map.removeLayer(marker);
                });

                Object.keys(conflictMarkers).forEach(k => {
                    if (!seen.has(k)) {
                        if (map.hasLayer(conflictMarkers[k])) map.removeLayer(conflictMarkers[k]);
                        delete conflictMarkers[k];
                    }
                });
            } catch (_) {}
        }

        async function pollShodanEvents(force=false) {
            const now = Date.now();
            if (!force && (now - lastShodanFetchAt) < 30_000) return;
            lastShodanFetchAt = now;
            try {
                const qs = new URLSearchParams();
                qs.set('limit', '200');
                let resp = await fetch(currentHub + 'api/shodan/events?' + qs.toString());
                if (!resp.ok) {
                    try {
                        const u = new URL(currentHub);
                        const sidecarBase = u.protocol + '//' + u.hostname + ':8790/';
                        resp = await fetch(sidecarBase + 'api/shodan/events?' + qs.toString());
                    } catch (_) {}
                }
                if (!resp.ok) return;
                const payload = await resp.json();
                const rows = Array.isArray(payload?.items) ? payload.items : [];
                const seen = new Set();
                rows.forEach(r => {
                    const lat = Number(r.lat);
                    const lon = Number(r.lon);
                    if (!isFinite(lat) || !isFinite(lon)) return;
                    const key = String(r.id || (r.ip + ':' + r.port));
                    seen.add(key);
                    const html = '<div style="width:20px;height:20px;display:flex;align-items:center;justify-content:center;filter:drop-shadow(0 0 3px rgba(0,0,0,0.8));"><svg width="20" height="20" viewBox="0 0 24 24" fill="none"><ellipse cx="12" cy="12" rx="10" ry="6" fill="#E1251B" stroke="#000" stroke-width="0.5"/><circle cx="12" cy="12" r="3.5" fill="#000"/><circle cx="12" cy="12" r="1.5" fill="#fff"/></svg></div>';
                    const icon = L.divIcon({ html, className: 'shodan-icon', iconSize: [18,18], iconAnchor: [9,9] });
                    const popup = '<div style="min-width:220px;">'
                        + '<b>Shodan ' + (r.ip || '-') + ':' + (r.port || '-') + '</b><br>'
                        + '<span style="font-size:11px;color:#94a3b8;">' + (r.city || '-') + ', ' + (r.country_name || r.country_code || '-') + '</span><br>'
                        + '<b>Org:</b> ' + (r.org || '-') + '<br>'
                        + '<b>ASN:</b> ' + (r.asn || '-') + '<br>'
                        + '<b>Product:</b> ' + (r.product || '-') + '<br>'
                        + '<b>Category:</b> ' + (r.category || '-') + '<br>'
                        + '<b>Source:</b> Shodan (Hub Cache)</div>';
                    let marker = shodanMarkers[key];
                    if (!marker) {
                        marker = L.marker([lat, lon], { icon });
                        shodanMarkers[key] = marker;
                    } else {
                        marker.setLatLng([lat, lon]);
                        marker.setIcon(icon);
                    }
                    marker.bindPopup(popup);
                    if (layerVisibility.shodan) {
                        if (!shodanClusterGroup && window.L && L.markerClusterGroup) {
                            shodanClusterGroup = L.markerClusterGroup({ showCoverageOnHover: false, maxClusterRadius: 50 });
                        }
                        if (shodanClusterGroup) {
                            if (!map.hasLayer(shodanClusterGroup)) map.addLayer(shodanClusterGroup);
                            if (!shodanClusterGroup.hasLayer(marker)) shodanClusterGroup.addLayer(marker);
                        } else if (!map.hasLayer(marker)) {
                            marker.addTo(map);
                        }
                    }
                });
                Object.keys(shodanMarkers).forEach(k => {
                    if (!seen.has(k)) {
                        if (shodanClusterGroup && shodanClusterGroup.hasLayer(shodanMarkers[k])) shodanClusterGroup.removeLayer(shodanMarkers[k]);
                        if (map.hasLayer(shodanMarkers[k])) map.removeLayer(shodanMarkers[k]);
                        delete shodanMarkers[k];
                    }
                });
            } catch (_) {}
        }
        
        // ===== LOCATION HANDLING =====

        function distanceMeters(aLat, aLon, bLat, bLon) {
            const R = 6371000;
            const toRad = x => x * Math.PI / 180;
            const dLat = toRad(bLat - aLat);
            const dLon = toRad(bLon - aLon);
            const q = Math.sin(dLat/2) * Math.sin(dLat/2)
                + Math.cos(toRad(aLat)) * Math.cos(toRad(bLat)) * Math.sin(dLon/2) * Math.sin(dLon/2);
            return 2 * R * Math.atan2(Math.sqrt(q), Math.sqrt(1-q));
        }
        
        function updateOwnPosition(lat, lon) {
            if (!Number.isFinite(lat) || !Number.isFinite(lon)) return;

            if (lastRawOwnPosition) {
                const rawJump = distanceMeters(lastRawOwnPosition.lat, lastRawOwnPosition.lon, lat, lon);
                if (rawJump < GPS_JITTER_METERS) return; // ignore tiny jitter
            }
            lastRawOwnPosition = { lat, lon };

            if (ownPosition) {
                const smoothedLat = ownPosition.lat + (lat - ownPosition.lat) * GPS_SMOOTH_ALPHA;
                const smoothedLon = ownPosition.lon + (lon - ownPosition.lon) * GPS_SMOOTH_ALPHA;
                ownPosition = { lat: smoothedLat, lon: smoothedLon };
            } else {
                ownPosition = { lat, lon };
            }

            document.getElementById('position').textContent = ownPosition.lat.toFixed(5) + ', ' + ownPosition.lon.toFixed(5);
            
            // Center map on user position on first GPS fix
            if (!hasCenteredOnUser && map) {
                map.setView([ownPosition.lat, ownPosition.lon], 16);
                hasCenteredOnUser = true;
            }
            
            // Add self as tracked entity
            ingestPLI({
                uid: OWN_CALLSIGN,
                callsign: OWN_CALLSIGN,
                type: UNIT_TYPE || 'EUD',
                affiliation: 'friendly',
                lat: ownPosition.lat,
                lon: ownPosition.lon,
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
                    unit_type: UNIT_TYPE || 'Individual Soldier',
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
                
                // Process cameras (COP + LOCAL in MERGED mode), keep merged cache stable
                if (data.cameras && Array.isArray(data.cameras) && data.cameras.length > 0) {
                    upsertCameraBatch(data.cameras, 'cop');
                }
                const nowMs = Date.now();
                const shouldPullLocal = (PLI_MODE === 'LOCAL' || PLI_MODE === 'MERGED' || !(data.cameras && data.cameras.length > 0));
                if (shouldPullLocal && (nowMs - lastLocalCamFetchAt > 15000)) {
                    const localCams = await fetchLocalCameras();
                    if (localCams.length > 0) upsertCameraBatch(localCams, 'local');
                    lastLocalCamFetchAt = nowMs;
                }
                const camSnap = getCameraSnapshot();
                camCount = camSnap.length;
                if (camSnap.length > 0) renderCameras(camSnap);
                
                // Process heat from COP only when non-empty.
                // (Avoid clearing delta-derived heat overlays when COP snapshot has [] )
                if (Array.isArray(data.heat) && data.heat.length > 0) {
                    renderHeat(data.heat);
                }
                
                // Process satellites (do not wipe local satcom on empty COP arrays)
                if (Array.isArray(data.satellites) && data.satellites.length > 0) {
                    renderSatellites(data.satellites);
                }
                
                document.getElementById('status').textContent = 'COP: ' + entityCount + ' entities, ' + camCount + ' cams, ' + heatCount + ' heat, ' + satCount + ' sat • Δheat:' + lastDeltaHeatCount + ' cache:' + Object.keys(deltaHeatCache).length + ' camsΔ:' + Object.keys(deltaCamCache).length + ' satΔ:' + Object.keys(deltaSatCache).length + ' cursor:' + copCursor;
            } catch (e) {
                document.getElementById('status').textContent = 'COP Error: ' + e.message;
            }
        }

        async function pollDeltaLayers() {
            try {
                const url = currentHub + 'api/delta?cursor=' + copCursor;
                const resp = await fetch(url);
                if (!resp.ok) return;
                const data = await resp.json();
                copCursor = Math.max(copCursor, Number(data.cursor || 0));

                const nowMs = Date.now();
                const derivedHeat = [];
                const derivedCams = [];
                const derivedSats = [];
                const updates = Array.isArray(data.tiles) ? data.tiles : [];
                updates.forEach(update => {
                    const tiles = Array.isArray(update.tiles) ? update.tiles : [];
                    tiles.forEach(t => {
                        const pos = parseTileToLatLon(t.tile_id);
                        if (!pos) return;

                        const dev = t.device_id || update.device_id;
                        const src = String(t.source_type || update.source_type || 'entity').toLowerCase();
                        const pliSrc = (src === 'entity' || src === 'handheld' || src === 'eud' || src === 'user');
                        if (pliSrc && dev && src !== 'hub_local' && String(dev).toLowerCase() !== 'hub') {
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

                        if (src === 'cctv' || src === 'camera') {
                            derivedCams.push({ tile_id: t.tile_id, dimension: 'delta', count: 1, bearing: 0, fov: 70 });
                        }
                        if (src === 'sat' || src === 'satellite' || src === 'satcom') {
                            derivedSats.push({ tile_id: t.tile_id, dimension: 'delta', count: 1 });
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

                // Merge into cache so overlays persist across delta gaps
                derivedHeat.forEach(h => {
                    const k = h.tile_id + ':' + (h.sensor_type || 'unknown') + ':' + (h.dimension || 'delta');
                    deltaHeatCache[k] = { ...h, _ts: nowMs };
                });
                Object.keys(deltaHeatCache).forEach(k => {
                    if ((nowMs - (deltaHeatCache[k]._ts || 0)) > DELTA_HEAT_TTL_MS) delete deltaHeatCache[k];
                });

                const cachedHeat = Object.values(deltaHeatCache).map(h => ({
                    tile_id: h.tile_id,
                    sensor_type: h.sensor_type,
                    dimension: h.dimension,
                    max: h.max,
                    mean: h.mean,
                }));


                derivedCams.forEach(c => { const k = c.tile_id + ':' + (c.dimension || 'delta'); deltaCamCache[k] = { ...c, _ts: nowMs }; });
                derivedSats.forEach(c => { const k = c.tile_id + ':' + (c.dimension || 'delta'); deltaSatCache[k] = { ...c, _ts: nowMs }; });
                Object.keys(deltaCamCache).forEach(k => { if ((nowMs - (deltaCamCache[k]._ts || 0)) > DELTA_HEAT_TTL_MS) delete deltaCamCache[k]; });
                Object.keys(deltaSatCache).forEach(k => { if ((nowMs - (deltaSatCache[k]._ts || 0)) > DELTA_HEAT_TTL_MS) delete deltaSatCache[k]; });
                const cachedCams = Object.values(deltaCamCache).map(c => ({ tile_id: c.tile_id, dimension: c.dimension, count: c.count, bearing: c.bearing, fov: c.fov }));
                const cachedSats = Object.values(deltaSatCache).map(c => ({ tile_id: c.tile_id, dimension: c.dimension, count: c.count }));
                if (cachedCams.length > 0) {
                    upsertCameraBatch(cachedCams, 'delta');
                    renderCameras(getCameraSnapshot());
                }
                if (cachedSats.length > 0) renderSatellites(cachedSats);

                lastDeltaHeatCount = derivedHeat.length;
                if (cachedHeat.length > 0) renderHeat(cachedHeat);
            } catch (e) {
                console.log('Delta poll error:', e.message);
            }
        }
        
        async function fetchLocalCameras() {
            try {
                const lat = Number(ownPosition?.lat);
                const lon = Number(ownPosition?.lon);
                if (!Number.isFinite(lat) || !Number.isFinite(lon)) return [];

                // same general source family as desktop fallback: OSM camera nodes
                const d = 0.02; // ~2km box
                const bbox = '(' + (lat - d).toFixed(6) + ',' + (lon - d).toFixed(6) + ',' + (lat + d).toFixed(6) + ',' + (lon + d).toFixed(6) + ')';
                const q = '[out:json][timeout:20];(node["man_made"="surveillance"]' + bbox + ';node["surveillance:type"]' + bbox + ';);out body 120;';
                const resp = await fetch('https://overpass-api.de/api/interpreter', {
                    method: 'POST',
                    headers: { 'Content-Type': 'text/plain' },
                    body: q,
                });
                if (!resp.ok) return [];
                const js = await resp.json();
                const elems = Array.isArray(js?.elements) ? js.elements : [];
                return elems.map((e, idx) => {
                    const tags = e?.tags || {};
                    const feedUrl = tags['contact:webcam'] || tags['camera:url'] || tags['surveillance:feed'] || tags['image'] || tags['url'] || tags['website'] || null;
                    return {
                        tile_id: 'android_' + Math.round(Number(e.lat || 0) * 10000) + '_' + Math.round(Number(e.lon || 0) * 10000),
                        dimension: 'local-osm',
                        count: 1,
                        bearing: Number(tags.direction || 0) || 0,
                        fov: 70,
                        id: 'osm-cam-' + (e.id || idx),
                        name: tags.name || ('OSM Camera ' + (e.id || idx)),
                        snapshotUrl: feedUrl,
                        sourceType: feedUrl ? 'PUBLIC' : 'OSM'
                    };
                }).filter(c => c.tile_id.includes('android_'));
            } catch (e) {
                console.log('Local camera discovery failed:', e.message);
                return [];
            }
        }

        function conePolygon(lat, lon, bearing=0, fov=70, range=0.0022) {
            const pts = [[lat, lon]];
            const half = fov / 2;
            for (let a = bearing - half; a <= bearing + half; a += 10) {
                const r = a * Math.PI / 180;
                const dy = Math.cos(r) * range;
                const dx = Math.sin(r) * range;
                pts.push([lat + dy, lon + dx]);
            }
            pts.push([lat, lon]);
            return pts;
        }

        function renderCameras(cameras) {
            const nextCams = {};

            cameras.forEach(cam => {
                let pos = null;
                if (cam.tile_id) pos = parseTileToLatLon(cam.tile_id);
                if (!pos && Number.isFinite(Number(cam.lat)) && Number.isFinite(Number(cam.lon))) {
                    pos = { lat: Number(cam.lat), lon: Number(cam.lon) };
                }
                if (!pos) return;

                const key = (cam.id || cam.tile_id || (pos.lat.toFixed(5)+','+pos.lon.toFixed(5))) + ':' + (cam.dimension || 'default');
                const count = cam.count || 0;
                const hasFeed = !!(cam.snapshotUrl || cam.url || cam.feed_url || cam.feedUrl);
                const feedUrl = cam.snapshotUrl || cam.url || cam.feed_url || cam.feedUrl || '';
                const markerColor = hasFeed ? '#60a5fa' : '#f59e0b'; // blue with URL, orange without

                let marker = camMarkers[key];
                const icon = L.divIcon({
                    className: 'cctv-marker',
                    html: '<div style="width:16px;height:16px;border-radius:50%;background:' + markerColor + ';border:2px solid white;box-shadow:0 0 0 1px rgba(255,255,255,0.35),0 0 8px rgba(0,0,0,0.55);"></div>',
                    iconSize: [16, 16],
                    iconAnchor: [8, 8]
                });

                if (!marker) {
                    marker = L.marker([pos.lat, pos.lon], { icon, zIndexOffset: 1000 });
                    camMarkers[key] = marker;
                } else {
                    marker.setLatLng([pos.lat, pos.lon]);
                }

                const camName = cam.name || 'Camera';
                const popupHtml = '<div style="min-width:220px;">'
                    + '<b>' + camName + '</b><br>'
                    + 'Count: ' + count + '<br>'
                    + 'Feed: ' + (hasFeed ? 'Available' : 'No URL') + '<br>'
                    + pos.lat.toFixed(5) + ', ' + pos.lon.toFixed(5)
                    + (hasFeed ? ('<br><button style="margin-top:6px;padding:4px 8px;background:#1e293b;color:#e2e8f0;border:1px solid #334155;border-radius:4px;" onclick="openCameraFeed(\'' + String(feedUrl).replace(/'/g, "\\'") + '\')">Open Feed</button>') : '')
                    + '</div>';
                marker.bindPopup(popupHtml);

                // CCTV cone (desktop-style)
                const bearing = Number(cam.bearing ?? 0);
                const fov = Number(cam.fov ?? 70);
                const cone = conePolygon(pos.lat, pos.lon, bearing, fov, 0.0022);
                if (!camCones[key]) {
                    camCones[key] = L.polygon(cone, {
                        color: markerColor,
                        weight: 1,
                        opacity: 0.8,
                        fillColor: markerColor,
                        fillOpacity: 0.18
                    });
                } else {
                    camCones[key].setLatLngs(cone);
                    camCones[key].setStyle({ color: markerColor, fillColor: markerColor });
                }

                if (layerVisibility.cams) {
                    if (!map.hasLayer(camCones[key])) camCones[key].addTo(map);
                    if (!map.hasLayer(marker)) marker.addTo(map);
                    if (camCones[key].bringToBack) camCones[key].bringToBack();
                }

                nextCams[key] = true;
            });

            Object.keys(camMarkers).forEach(key => {
                if (!nextCams[key]) {
                    if (map.hasLayer(camMarkers[key])) map.removeLayer(camMarkers[key]);
                    if (camCones[key] && map.hasLayer(camCones[key])) map.removeLayer(camCones[key]);
                    delete camMarkers[key];
                    delete camCones[key];
                }
            });
        }

        function renderHeat(heatData) {
            if (!map || !window.L || !L.heatLayer) return;

            const rfPoints = [];
            const wifiPoints = [];

            heatData.forEach(h => {
                if (!h.tile_id) return;
                const pos = parseTileToLatLon(h.tile_id);
                if (!pos) return;

                const sensor = String(h.sensor_type || '').toLowerCase();
                const maxVal = Number(h.max || 0);
                const meanVal = Number(h.mean || 0);

                if (sensor === 'rf') {
                    const signalIntensity = Math.max(0, Math.min(1, (meanVal + 100) / 60));
                    const finalIntensity = Math.max(0.05, signalIntensity);
                    rfPoints.push([pos.lat, pos.lon, finalIntensity]);
                } else {
                    // wifi + unknown -> wifi heat style
                    const rssiIntensity = Math.max(0, Math.min(1, (meanVal + 100) / 70));
                    const countBoost = Math.min(1, maxVal / 8);
                    const finalIntensity = Math.max(0.05, (rssiIntensity * 0.7 + countBoost * 0.3));
                    wifiPoints.push([pos.lat, pos.lon, finalIntensity]);
                }
            });

            const rfOpts = {
                radius: 40,
                blur: 30,
                maxZoom: 18,
                max: 1.0,
                minOpacity: 0.25,
                gradient: {
                    0.0: 'rgba(0,0,180,0.6)',
                    0.3: 'rgba(0,200,200,0.65)',
                    0.55: 'rgba(0,220,0,0.7)',
                    0.75: 'rgba(255,200,0,0.75)',
                    1.0: 'rgba(255,40,0,0.8)'
                }
            };
            const wifiOpts = {
                radius: 40,
                blur: 30,
                maxZoom: 18,
                max: 1.0,
                minOpacity: 0.25,
                gradient: {
                    0.0: 'rgba(0,50,200,0.6)',
                    0.4: 'rgba(0,180,255,0.65)',
                    0.65: 'rgba(255,140,0,0.7)',
                    1.0: 'rgba(255,60,0,0.8)'
                }
            };

            if (rfHeatLayer) { rfHeatLayer.setLatLngs(rfPoints); }
            else { rfHeatLayer = L.heatLayer(rfPoints, rfOpts); }

            if (wifiHeatLayer) { wifiHeatLayer.setLatLngs(wifiPoints); }
            else { wifiHeatLayer = L.heatLayer(wifiPoints, wifiOpts); }

            if (layerVisibility.heat) {
                if (rfHeatLayer && !map.hasLayer(rfHeatLayer)) rfHeatLayer.addTo(map);
                if (wifiHeatLayer && !map.hasLayer(wifiHeatLayer)) wifiHeatLayer.addTo(map);
            }
        }

        const SAT_CACHE_TTL_MS = 6 * 60 * 60 * 1000;

        async function ensureSatelliteJs() {
            if (window.satellite && typeof window.satellite.twoline2satrec === 'function') return true;
            const urls = [
                'https://unpkg.com/satellite.js@5.0.0/dist/satellite.min.js',
                'https://cdn.jsdelivr.net/npm/satellite.js@5.0.0/dist/satellite.min.js'
            ];
            for (const u of urls) {
                try {
                    await new Promise((resolve, reject) => {
                        const sc = document.createElement('script');
                        sc.src = u;
                        sc.async = true;
                        sc.onload = resolve;
                        sc.onerror = reject;
                        document.head.appendChild(sc);
                    });
                    if (window.satellite && typeof window.satellite.twoline2satrec === 'function') return true;
                } catch (_) {}
            }
            return false;
        }

        function satGroupUrl(group) {
            const m = {
                stations: 'stations',
                weather: 'weather',
                starlink: 'starlink',
                military: 'military',
                active: 'active'
            };
            const g = m[group] || 'stations';
            return 'https://celestrak.org/NORAD/elements/gp.php?GROUP=' + encodeURIComponent(g) + '&FORMAT=TLE';
        }

        function parseTleText(text, group) {
            const normalized = String(text || '').replace(/\r/g, '');
            const lines = normalized.split(String.fromCharCode(10)).map(l => l.trim()).filter(Boolean);
            const out = [];
            for (let i = 0; i + 2 < lines.length; i += 3) {
                const name = lines[i];
                const l1 = lines[i + 1];
                const l2 = lines[i + 2];
                if (!l1.startsWith('1 ') || !l2.startsWith('2 ')) continue;
                const norad = (l1.slice(2, 7) || '').trim();
                out.push({ id: group + '-' + (norad || i), norad, name, line1: l1, line2: l2, group });
            }
            return out;
        }

        async function fetchSatGroup(group, force=false) {
            const key = 'sat:tles:' + group;
            if (!force) {
                try {
                    const cached = JSON.parse(localStorage.getItem(key) || 'null');
                    if (cached && (Date.now() - cached.ts) < SAT_CACHE_TTL_MS) return cached.items || [];
                } catch (_) {}
            }
            const resp = await fetch(satGroupUrl(group));
            if (!resp.ok) throw new Error('TLE fetch ' + group + ' ' + resp.status);
            const txt = await resp.text();
            const items = parseTleText(txt, group);
            localStorage.setItem(key, JSON.stringify({ ts: Date.now(), items }));
            satLastDiag = { ok: true, group, count: items.length, at: Date.now(), err: '' };
            return items;
        }

        function updateSatDiag() {
            const el = document.getElementById('satDiag');
            if (!el) return;
            if (!satLastDiag.at) {
                el.textContent = 'SAT link: unknown';
                return;
            }
            const t = new Date(satLastDiag.at).toLocaleTimeString();
            el.textContent = satLastDiag.ok
                ? ('SAT link: OK • ' + satLastDiag.group + ' • ' + satLastDiag.count + ' TLE • ' + t)
                : ('SAT link: FAIL • ' + (satLastDiag.err || 'unknown error'));
        }

        async function testCelestrakConnection() {
            const group = satSelectedGroups[0] || 'stations';
            try {
                await fetchSatGroup(group, true);
                updateSatDiag();
                document.getElementById('status').textContent = 'CelesTrak OK: ' + group;
            } catch (e) {
                satLastDiag = { ok: false, group, count: 0, at: Date.now(), err: e.message || String(e) };
                updateSatDiag();
                document.getElementById('status').textContent = 'CelesTrak FAIL: ' + satLastDiag.err;
            }
        }

        function satSubpointFromTle(s) {
            try {
                if (!window.satellite) return null;
                const satrec = window.satellite.twoline2satrec(s.line1, s.line2);
                const now = new Date();
                const gmst = window.satellite.gstime(now);
                const pv = window.satellite.propagate(satrec, now);
                if (!pv || !pv.position) return null;
                const gd = window.satellite.eciToGeodetic(pv.position, gmst);
                return {
                    lat: window.satellite.degreesLat(gd.latitude),
                    lon: window.satellite.degreesLong(gd.longitude),
                    altKm: gd.height
                };
            } catch (_) { return null; }
        }

        async function fetchSatellitesFromHub() {
            // Fallback: get satellite positions from hub if local TLE fetch fails
            try {
                const hub = currentHub.endsWith('/') ? currentHub : currentHub + '/';
                if (!hub.startsWith('http')) return [];
                
                const resp = await fetch(hub + 'api/cop?max_age_sec=300');
                if (!resp.ok) return [];
                
                const data = await resp.json();
                const sats = Array.isArray(data.satellites) ? data.satellites : [];
                return sats.filter(s => Number.isFinite(s.lat) && Number.isFinite(s.lon))
                    .map(s => ({
                        id: s.norad || s.name || 'sat-' + Math.random().toString(36).slice(2),
                        name: s.name || 'Unknown',
                        norad: s.norad || '',
                        lat: s.lat,
                        lon: s.lon,
                        altKm: s.altKm || 0,
                        dimension: s.dimension || 'hub'
                    }));
            } catch (e) {
                console.log('Hub satellite fallback failed:', e.message);
                return [];
            }
        }

        async function pollLocalSatcom(force=false) {
            try {
                if (!layerVisibility.sat) return;
                const ok = await ensureSatelliteJs();
                if (!ok) {
                    document.getElementById('status').textContent = 'SAT error: satellite.js unavailable';
                    // Try hub fallback
                    const hubSats = await fetchSatellitesFromHub();
                    if (hubSats.length > 0) {
                        renderSatellites(hubSats);
                        document.getElementById('status').textContent = 'SAT: Using hub data (' + hubSats.length + ')';
                    }
                    return;
                }
                let all = [];
                let failedGroups = [];
                for (const g of satSelectedGroups) {
                    try {
                        const items = await fetchSatGroup(g, force);
                        all = all.concat(items);
                    } catch (e) {
                        console.log('SAT fetch failed', g, e.message);
                        failedGroups.push(g);
                    }
                }
                
                // If all groups failed, try hub fallback
                if (all.length === 0 && failedGroups.length > 0) {
                    const hubSats = await fetchSatellitesFromHub();
                    if (hubSats.length > 0) {
                        renderSatellites(hubSats);
                        satLastDiag = { ok: true, group: 'hub-fallback', count: hubSats.length, at: Date.now(), err: '' };
                        updateSatDiag();
                        return;
                    }
                }
                
                const dedup = new Map();
                all.forEach(s => dedup.set(s.norad || s.id, s));
                const localSats = Array.from(dedup.values());

                const out = [];
                localSats.slice(0, satMaxMarkers).forEach(s => {
                    const p = satSubpointFromTle(s);
                    if (!p || !Number.isFinite(p.lat) || !Number.isFinite(p.lon)) return;
                    out.push({ id: s.id, name: s.name, norad: s.norad, lat: p.lat, lon: p.lon, altKm: p.altKm, dimension: s.group });
                });
                renderSatellites(out);
                await pushLocalSatcomToHub(out);
                updateSatDiag();
            } catch (e) {
                satLastDiag = { ok: false, group: satSelectedGroups[0] || '-', count: 0, at: Date.now(), err: e.message || String(e) };
                updateSatDiag();
                console.log('Local satcom poll error:', e.message);
                // Final fallback attempt
                const hubSats = await fetchSatellitesFromHub();
                if (hubSats.length > 0) {
                    renderSatellites(hubSats);
                }
            }
        }

        function applySatGroups() {
            satSelectedGroups = Array.from(document.querySelectorAll('input[data-sat-group]'))
                .filter(x => x.checked).map(x => x.value);
            localStorage.setItem('sat:selectedGroups', JSON.stringify(satSelectedGroups));
            if (satSelectedGroups.length === 0) {
                renderSatellites([]);
                return;
            }
            pollLocalSatcom(true);
        }

        function applySatMax() {
            const el = document.getElementById('satMaxInput');
            const n = parseInt(el?.value || '180', 10);
            satMaxMarkers = Number.isFinite(n) ? Math.max(20, Math.min(500, n)) : 180;
            localStorage.setItem('sat:maxMarkers', String(satMaxMarkers));
            pollLocalSatcom(true);
        }

        async function pushLocalSatcomToHub(sats) {
            try {
                if (!Array.isArray(sats) || sats.length === 0) return;
                const nowMs = Date.now();
                if ((nowMs - satLastPushAt) < SAT_PUSH_INTERVAL_MS) return;

                const hub = currentHub.endsWith('/') ? currentHub : currentHub + '/';
                if (!hub.startsWith('http')) return;

                const nowSec = Math.floor(nowMs / 1000);
                const tiles = sats.map(s => {
                    let tileId = null;
                    if (window.h3 && window.h3.latLngToCell) {
                        try { tileId = window.h3.latLngToCell(Number(s.lat), Number(s.lon), 6); } catch (_) {}
                    }
                    if (!tileId) {
                        tileId = 'android_' + Math.round(Number(s.lat) * 10000) + '_' + Math.round(Number(s.lon) * 10000);
                    }
                    return {
                        tile_id: tileId,
                        time_bucket: Math.floor(nowSec / 60) * 60,
                        sat: {
                            name: String(s.name || ''),
                            norad: String(s.norad || ''),
                            alt_km: Number(s.altKm || 0),
                            group: String(s.dimension || '')
                        }
                    };
                }).slice(0, Math.min(120, satMaxMarkers));

                if (!tiles.length) return;

                const payload = {
                    schema_version: 1,
                    device_id: OWN_CALLSIGN,
                    source_type: 'satellite',
                    timestamp_utc: nowSec,
                    tiles
                };

                const resp = await fetch(hub + 'api/push', {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify(payload)
                });

                if (resp.ok) satLastPushAt = nowMs;
            } catch (e) {
                console.log('SAT push failed:', e.message);
            }
        }

        function renderSatellites(sats) {
            const nextSats = {};

            sats.forEach(sat => {
                let pos = null;
                if (sat.tile_id) pos = parseTileToLatLon(sat.tile_id);
                if (!pos && Number.isFinite(Number(sat.lat)) && Number.isFinite(Number(sat.lon))) {
                    pos = { lat: Number(sat.lat), lon: Number(sat.lon) };
                }
                if (!pos) return;

                const key = (sat.id || sat.tile_id || (pos.lat.toFixed(4)+','+pos.lon.toFixed(4))) + ':' + (sat.dimension || 'default');
                const count = sat.count || 1;

                let marker = satMarkers[key];
                const satName = String(sat.name || sat.norad || sat.id || key);
                const satAbbr = satName.replace(/\s+/g, ' ').trim().slice(0, 8).toUpperCase();
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

                marker.bindPopup('Satellite: ' + satAbbr + '<br>Count: ' + count + '<br>' + pos.lat.toFixed(5) + ', ' + pos.lon.toFixed(5));

                if (layerVisibility.sat && !map.hasLayer(marker)) {
                    marker.addTo(map);
                }

                if (cesiumViewer) {
                    if (!cesiumSatEntities[key]) {
                        cesiumSatEntities[key] = cesiumViewer.entities.add({
                            id: 'sat-' + key,
                            position: Cesium.Cartesian3.fromDegrees(pos.lon, pos.lat, 550000),
                            point: { pixelSize: 8, color: Cesium.Color.YELLOW, outlineColor: Cesium.Color.WHITE, outlineWidth: 1 },
                            label: { text: satAbbr, font: '12px sans-serif', fillColor: Cesium.Color.YELLOW, pixelOffset: new Cesium.Cartesian2(0, -14) }
                        });
                    } else {
                        cesiumSatEntities[key].position = Cesium.Cartesian3.fromDegrees(pos.lon, pos.lat, 550000);
                        if (cesiumSatEntities[key].label) cesiumSatEntities[key].label.text = satAbbr;
                    }
                }

                nextSats[key] = true;
            });

            Object.keys(satMarkers).forEach(key => {
                if (!nextSats[key]) {
                    if (map.hasLayer(satMarkers[key])) map.removeLayer(satMarkers[key]);
                    delete satMarkers[key];
                    if (cesiumViewer && cesiumSatEntities[key]) {
                        cesiumViewer.entities.remove(cesiumSatEntities[key]);
                        delete cesiumSatEntities[key];
                    }
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

                    if (cesiumViewer) {
                        if (!cesiumAdsbEntities[id]) {
                            cesiumAdsbEntities[id] = cesiumViewer.entities.add({
                                id: 'adsb-' + id,
                                position: Cesium.Cartesian3.fromDegrees(lon, lat, 11000),
                                point: { pixelSize: 7, color: Cesium.Color.ORANGE, outlineColor: Cesium.Color.WHITE, outlineWidth: 1 },
                                label: { text: id, font: '11px sans-serif', fillColor: Cesium.Color.ORANGE, pixelOffset: new Cesium.Cartesian2(0, -12) }
                            });
                        } else {
                            cesiumAdsbEntities[id].position = Cesium.Cartesian3.fromDegrees(lon, lat, 11000);
                        }
                    }
                    next[id] = true;
                });

                Object.keys(adsbMarkers).forEach(id => {
                    if (!next[id]) {
                        if (map.hasLayer(adsbMarkers[id])) map.removeLayer(adsbMarkers[id]);
                        delete adsbMarkers[id];
                        if (cesiumViewer && cesiumAdsbEntities[id]) {
                            cesiumViewer.entities.remove(cesiumAdsbEntities[id]);
                            delete cesiumAdsbEntities[id];
                        }
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
            const unitType = document.getElementById('cfgUnitType').value;
            const hub = document.getElementById('cfgHub').value.trim();
            const mode = document.getElementById('pliModeSel').value;
            const cWindow = document.getElementById('conflictWindowSel').value;
            const cFrom = document.getElementById('conflictDateFrom').value;
            const cTo = document.getElementById('conflictDateTo').value;
            const cCountry = document.getElementById('conflictCountry').value.trim();
            
            // Update callsign: persist to Android app storage + update JS variable + re-push
            if (cs) {
                localStorage.setItem('eud:callsign', cs);
                // Update Kotlin-side persistent storage
                if (window.AndroidBridge && typeof window.AndroidBridge.updateCallsign === 'function') {
                    try {
                        const saved = window.AndroidBridge.updateCallsign(cs);
                        console.log('Callsign saved to Android:', saved);
                    } catch (e) {
                        console.log('Failed to update Android callsign:', e);
                    }
                }
                // Update current session variable
                const oldCallsign = OWN_CALLSIGN;
                OWN_CALLSIGN = cs;
                // Update any existing entity marker for this callsign
                if (entityMarkers[oldCallsign]) {
                    entityMarkers[cs] = entityMarkers[oldCallsign];
                    delete entityMarkers[oldCallsign];
                }
                // Immediately push with new callsign
                pushLocalPliToHub();
            }
            
            if (hub) {
                currentHub = hub.endsWith('/') ? hub : hub + '/';
                localStorage.setItem('eud:hub', currentHub);
                document.getElementById('cfgHub').value = currentHub;
            }
            UNIT_TYPE = unitType || 'Individual Soldier';
            localStorage.setItem('eud:unit_type', UNIT_TYPE);
            PLI_MODE = mode;
            localStorage.setItem('eud:pli_mode', PLI_MODE);

            conflictWindow = cWindow || '1d';
            conflictDateFrom = cFrom || '';
            conflictDateTo = cTo || '';
            conflictCountry = cCountry || '';
            localStorage.setItem('eud:conflict_window', conflictWindow);
            localStorage.setItem('eud:conflict_date_from', conflictDateFrom);
            localStorage.setItem('eud:conflict_date_to', conflictDateTo);
            localStorage.setItem('eud:conflict_country', conflictCountry);

            applyLayerVisibility();
            if (layerVisibility.conflict) pollConflictEvents(true);
            document.getElementById('status').textContent = 'Callsign: ' + OWN_CALLSIGN + ' • Mode: ' + PLI_MODE + ' • Conflict: ' + (conflictWindow === 'custom' ? (conflictDateFrom + '→' + conflictDateTo) : conflictWindow) + (conflictCountry ? (' • ' + conflictCountry) : '');
        }
        
        function focusOwn() {
            const m = entityMarkers[OWN_CALLSIGN];
            if (m) {
                map.setView(m.getLatLng(), 16);
            } else {
                map.setView([ownPosition.lat, ownPosition.lon], 16);
            }
        }
        
        function getEntityFeedUrl(entity) {
            if (!entity) return '';
            return entityFeedMap[entity.uid] || entityFeedMap[entity.callsign] || '';
        }

        async function syncEntityFeedsFromHub() {
            try {
                const resp = await fetch(currentHub + 'api/entity_feeds');
                if (!resp.ok) return;
                const data = await resp.json();
                const rows = Array.isArray(data?.feeds) ? data.feeds : (Array.isArray(data) ? data : []);
                rows.forEach(r => {
                    const u = String(r.uid || '').trim();
                    const c = String(r.callsign || '').trim();
                    const f = String(r.feed_url || r.url || '').trim();
                    if (!f) return;
                    if (u) entityFeedMap[u] = f;
                    if (c) entityFeedMap[c] = f;
                });
                localStorage.setItem('eud:entity_feeds', JSON.stringify(entityFeedMap));
            } catch (_) {}
        }

        async function upsertEntityFeed(e, url) {
            if (!e || !url) return;
            entityFeedMap[e.uid] = url;
            entityFeedMap[e.callsign] = url;
            localStorage.setItem('eud:entity_feeds', JSON.stringify(entityFeedMap));
            try {
                await fetch(currentHub + 'api/entity_feeds/upsert', {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify({ uid: e.uid, callsign: e.callsign, feed_url: url, updated_by: OWN_CALLSIGN })
                });
            } catch (_) {}
            showEntityDetail(e.uid);
        }

        async function bindLocalGlasses() {
            if (!selectedEntityUid) return;
            const e = trackedEntities.find(x => x.uid === selectedEntityUid);
            if (!e) return;
            const localUrl = 'meta://' + e.uid;
            let ok = true;
            try {
                if (window.AndroidBridge && typeof window.AndroidBridge.bindLocalGlasses === 'function') {
                    ok = !!window.AndroidBridge.bindLocalGlasses(e.uid);
                }
            } catch (_) { ok = false; }
            if (!ok) {
                document.getElementById('status').textContent = 'Meta glasses bind failed';
                return;
            }
            await upsertEntityFeed(e, localUrl);
            document.getElementById('status').textContent = 'Bound local glasses stream to ' + (e.callsign || e.uid);
            openCameraFeed(localUrl);
        }

        function watchEntityFeed() {
            if (!selectedEntityUid) return;
            const e = trackedEntities.find(x => x.uid === selectedEntityUid);
            if (!e) return;
            const feed = getEntityFeedUrl(e);
            if (!feed) {
                document.getElementById('status').textContent = 'No live feed linked for ' + (e.callsign || e.uid);
                return;
            }
            openCameraFeed(feed);
        }

        async function clearEntityFeed() {
            if (!selectedEntityUid) return;
            const e = trackedEntities.find(x => x.uid === selectedEntityUid);
            if (!e) return;
            delete entityFeedMap[e.uid];
            delete entityFeedMap[e.callsign];
            localStorage.setItem('eud:entity_feeds', JSON.stringify(entityFeedMap));
            try {
                await fetch(currentHub + 'api/entity_feeds/delete', {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify({ uid: e.uid, callsign: e.callsign, updated_by: OWN_CALLSIGN })
                });
            } catch (_) {}
            showEntityDetail(e.uid);
        }

        function showEntityDetail(uid) {
            selectedEntityUid = uid;
            const e = trackedEntities.find(x => x.uid === uid);
            const box = document.getElementById('entityDetail');
            if (!box) return;
            if (!e) { box.textContent = 'Select an entity for details.'; return; }
            const feed = getEntityFeedUrl(e);
            box.innerHTML = '<div><b>' + (e.callsign || e.uid) + '</b></div>'
                + '<div>Type: ' + (e.type || 'Unknown') + '</div>'
                + '<div>UID: ' + (e.uid || '-') + '</div>'
                + '<div>Lat/Lon: ' + Number(e.lat || 0).toFixed(5) + ', ' + Number(e.lon || 0).toFixed(5) + '</div>'
                + '<div>Affiliation: ' + (e.affiliation || 'unknown') + '</div>'
                + '<div>Live Feed: ' + (feed ? 'Linked' : 'Not linked') + '</div>'
                + (feed ? ('<button class="sb-btn" onclick="openCameraFeed(\'' + String(feed).replace(/'/g, "\\'") + '\')">Watch Live</button>') : '');
        }

        function focusEntity(uid) {
            const m = entityMarkers[uid];
            if (m) map.setView(m.getLatLng(), 16);
            showEntityDetail(uid);
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

        // Prune stale entities + dedupe duplicate callsigns
        setInterval(() => {
            pruneAndDedupeEntities();
            renderEntityList();
            updateStatus();
        }, 10000);
        
        // Poll COP + ADS-B continuously (LOCAL keeps local primary but still syncs/pulls)
        setInterval(pollCOP, 3000);
        setInterval(pollAdsb, 5000);
        setInterval(pollLocalSatcom, 15000);
        setInterval(syncEntityFeedsFromHub, 10000);
        setInterval(refreshGlassesStatus, 1500);
        setInterval(() => { if (layerVisibility.conflict) pollConflictEvents(false); }, 30000);
        setInterval(() => { if (layerVisibility.shodan) pollShodanEvents(false); }, 30000);
        pollCOP();
        pollAdsb();
        pollLocalSatcom();
        syncEntityFeedsFromHub();
        refreshGlassesStatus();
        pollConflictEvents(true);
        pollShodanEvents(true);
        updateSatDiag();
        
        document.getElementById('status').textContent = PLI_MODE === 'LOCAL' ? 'Local Mode + COP Sync' : 'Connecting...';
        updateStatus();
        renderMessenger();
        
    </script>
</body>
</html>
""".trimIndent()
    }

    override fun onDestroy() {
        super.onDestroy()
        metaStreamJob?.cancel()
        metaStreamJob = null
        metaStreamSession?.close()
        metaStreamSession = null
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

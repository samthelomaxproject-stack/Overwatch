package ai.overwatch.android

import android.Manifest
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.location.Location
import android.location.LocationListener
import android.location.LocationManager
import android.net.wifi.WifiManager
import android.os.Build
import android.os.IBinder
import androidx.core.app.ActivityCompat
import androidx.core.app.NotificationCompat
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import org.json.JSONArray
import org.json.JSONObject
import java.time.Instant
import java.util.Locale

class CollectorService : Service() {
    private val serviceScope = CoroutineScope(Dispatchers.IO)
    private var loopJob: Job? = null
    private val client = OkHttpClient()

    @Volatile
    private var latestLocation: Location? = null

    private val gpsListener = LocationListener { loc -> latestLocation = loc }

    override fun onCreate() {
        super.onCreate()
        ensureNotificationChannel()
        startForeground(NOTIFICATION_ID, buildNotification("Collector starting…"))
        startLocationUpdates()
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (loopJob?.isActive == true) return START_STICKY

        loopJob = serviceScope.launch {
            while (isActive) {
                runCollectorCycle()
                delay(30_000) // 30s cadence
            }
        }

        return START_STICKY
    }

    override fun onDestroy() {
        loopJob?.cancel()
        stopLocationUpdates()
        super.onDestroy()
    }

    override fun onBind(intent: Intent?): IBinder? = null

    private fun runCollectorCycle() {
        val hubUrl = ConfigStore.getHubUrl(this).trimEnd('/')
        val privacyMode = ConfigStore.getPrivacyMode(this)
        val callsign = ConfigStore.getCallsign(this)
        val now = Instant.now().epochSecond
        val bucket = (now / 60) * 60

        val location = getBestLocation()
        if (location == null) {
            updateNotification("Waiting for location permission/fix")
            return
        }

        val tileId = h3CellApprox(location.latitude, location.longitude)
        val wifiScan = collectWifiNetworks(privacyMode)

        if (wifiScan.isEmpty()) {
            updateNotification("No Wi-Fi scan results; sending GPS heartbeat only")
        }

        val payload = buildTileUpdate(
            deviceId = callsign,
            sourceType = "handheld", 
            timestampUtc = now,
            tileId = tileId,
            timeBucket = bucket,
            wifiScan = wifiScan
        )

        val req = Request.Builder()
            .url("$hubUrl/api/push")
            .post(payload.toString().toRequestBody("application/json".toMediaType()))
            .build()

        runCatching {
            client.newCall(req).execute().use { resp ->
                if (!resp.isSuccessful) throw IllegalStateException("HTTP ${resp.code}")
                updateNotification("$callsign • pushed ${wifiScan.size} Wi-Fi obs + GPS heartbeat • ${location.latitude.format(4)}, ${location.longitude.format(4)} • $tileId")
            }
        }.onFailure { err ->
            val reason = when (err) {
                is java.net.SocketTimeoutException -> "TIMEOUT"
                is java.net.ConnectException -> "CONNECT_REFUSED"
                is java.net.UnknownHostException -> "DNS"
                is java.net.SocketException -> "SOCKET"
                else -> err.javaClass.simpleName
            }
            updateNotification("Push failed [$reason]: ${err.message}")
        }
    }

    private fun collectWifiNetworks(privacyMode: String): List<WifiObs> {
        val wifiManager = applicationContext.getSystemService(Context.WIFI_SERVICE) as WifiManager

        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            if (ActivityCompat.checkSelfPermission(this, Manifest.permission.NEARBY_WIFI_DEVICES) != PackageManager.PERMISSION_GRANTED) {
                return emptyList()
            }
        }

        // Request a fresh scan (Android may throttle in background; we still read latest cache)
        runCatching { wifiManager.startScan() }

        @Suppress("DEPRECATION")
        val results = wifiManager.scanResults ?: emptyList()

        return results.mapNotNull { sr ->
            val channel = freqToChannel(sr.frequency)
            if (channel <= 0) return@mapNotNull null
            val band = when {
                sr.frequency >= 5925 -> "6"
                sr.frequency >= 5000 -> "5"
                else -> "2.4"
            }

            val ssidRaw = sr.SSID
            val bssidRaw = sr.BSSID

            val (ssid, bssid) = when (privacyMode) {
                "B" -> hashPair(ssidRaw, bssidRaw)
                "C" -> Pair(ssidRaw, bssidRaw)
                else -> Pair("", "")
            }

            WifiObs(
                band = band,
                channel = channel,
                rssi = sr.level.coerceIn(-100, -10),
                ssid = ssid,
                bssid = bssid
            )
        }
    }

    private fun hashPair(ssid: String, bssid: String): Pair<String, String> {
        fun sha256Short(input: String): String {
            val md = java.security.MessageDigest.getInstance("SHA-256")
            val digest = md.digest(input.toByteArray(Charsets.UTF_8))
            return digest.joinToString("") { "%02x".format(it) }.take(16)
        }
        return Pair(sha256Short("ssid:$ssid"), sha256Short("bssid:$bssid"))
    }

    private fun buildTileUpdate(
        deviceId: String,
        sourceType: String,
        timestampUtc: Long,
        tileId: String,
        timeBucket: Long,
        wifiScan: List<WifiObs>
    ): JSONObject {
        val channelAgg = wifiScan.groupBy { "${it.band}:${it.channel}" }.map { (k, list) ->
            val band = k.split(':')[0]
            val channel = k.split(':')[1].toInt()
            val mean = list.map { it.rssi }.average()
            val max = list.maxOf { it.rssi }.toDouble()

            JSONObject().apply {
                put("band", band)
                put("channel", channel)
                put("count", list.size)
                put("mean_rssi_dbm", mean)
                put("max_rssi_dbm", max)
                put("confidence", 0.8)
            }
        }

        return JSONObject().apply {
            put("schema_version", 1)
            put("device_id", deviceId)
            put("source_type", sourceType)
            put("timestamp_utc", timestampUtc)
            put("tiles", JSONArray().put(
                JSONObject().apply {
                    put("tile_id", tileId)
                    put("time_bucket", timeBucket)
                    put("wifi", JSONObject().apply {
                        put("channel_hotness", JSONArray(channelAgg))
                    })
                }
            ))
        }
    }

    private fun startLocationUpdates() {
        val lm = getSystemService(Context.LOCATION_SERVICE) as LocationManager
        if (ActivityCompat.checkSelfPermission(this, Manifest.permission.ACCESS_FINE_LOCATION) != PackageManager.PERMISSION_GRANTED &&
            ActivityCompat.checkSelfPermission(this, Manifest.permission.ACCESS_COARSE_LOCATION) != PackageManager.PERMISSION_GRANTED
        ) {
            return
        }

        // Prefer true device GPS; avoid network-provider drift (e.g., VPN/IP geolocation effects).
        runCatching {
            if (lm.isProviderEnabled(LocationManager.GPS_PROVIDER)) {
                lm.requestLocationUpdates(
                    LocationManager.GPS_PROVIDER,
                    3_000L,
                    3f,
                    gpsListener
                )
            }
        }

        // Keep network provider as fallback only if GPS is unavailable.
        runCatching {
            if (!lm.isProviderEnabled(LocationManager.GPS_PROVIDER) && lm.isProviderEnabled(LocationManager.NETWORK_PROVIDER)) {
                lm.requestLocationUpdates(
                    LocationManager.NETWORK_PROVIDER,
                    5_000L,
                    5f,
                    gpsListener
                )
            }
        }
    }

    private fun stopLocationUpdates() {
        val lm = getSystemService(Context.LOCATION_SERVICE) as LocationManager
        runCatching { lm.removeUpdates(gpsListener) }
    }

    private fun getBestLocation(): Location? {
        // Prefer actively updated location first
        latestLocation?.let { return it }

        val lm = getSystemService(Context.LOCATION_SERVICE) as LocationManager
        if (ActivityCompat.checkSelfPermission(this, Manifest.permission.ACCESS_FINE_LOCATION) != PackageManager.PERMISSION_GRANTED &&
            ActivityCompat.checkSelfPermission(this, Manifest.permission.ACCESS_COARSE_LOCATION) != PackageManager.PERMISSION_GRANTED
        ) {
            return null
        }

        // Prefer GPS provider first for true device location.
        runCatching { lm.getLastKnownLocation(LocationManager.GPS_PROVIDER) }.getOrNull()?.let {
            latestLocation = it
            return it
        }

        val providers = lm.getProviders(true)
        var best: Location? = null
        for (p in providers) {
            val loc = runCatching { lm.getLastKnownLocation(p) }.getOrNull() ?: continue
            if (best == null || loc.accuracy < best!!.accuracy) best = loc
        }
        latestLocation = best
        return best
    }

    private fun freqToChannel(freqMhz: Int): Int {
        return when {
            freqMhz in 2412..2472 -> (freqMhz - 2407) / 5
            freqMhz == 2484 -> 14
            freqMhz in 5000..5895 -> (freqMhz - 5000) / 5
            freqMhz in 5925..7125 -> (freqMhz - 5950) / 5
            else -> 0
        }
    }

    // Placeholder H3 approx until shared Rust JNI lands.
    private fun h3CellApprox(lat: Double, lon: Double): String {
        // deterministic pseudo-cell for MVP networking tests
        // quantize to ~11m resolution so movement updates map smoothly
        val latQ = (lat * 10000).toInt()
        val lonQ = (lon * 10000).toInt()
        return "android_${latQ}_${lonQ}"
    }

    private fun ensureNotificationChannel() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val channel = NotificationChannel(
                CHANNEL_ID,
                "Overwatch Collector",
                NotificationManager.IMPORTANCE_LOW
            )
            val manager = getSystemService(NotificationManager::class.java)
            manager.createNotificationChannel(channel)
        }
    }

    private fun buildNotification(text: String): Notification {
        return NotificationCompat.Builder(this, CHANNEL_ID)
            .setSmallIcon(android.R.drawable.ic_menu_mylocation)
            .setContentTitle("Overwatch EUD Collector")
            .setContentText(text)
            .setOngoing(true)
            .build()
    }

    private fun updateNotification(text: String) {
        val manager = getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
        manager.notify(NOTIFICATION_ID, buildNotification(text))
        sendBroadcast(Intent(ACTION_STATUS).putExtra("msg", text))
    }

    private fun Double.format(d: Int): String = String.format(Locale.US, "%.${d}f", this)

    data class WifiObs(
        val band: String,
        val channel: Int,
        val rssi: Int,
        val ssid: String,
        val bssid: String
    )

    companion object {
        const val ACTION_STATUS = "ai.overwatch.android.COLLECTOR_STATUS"
        private const val CHANNEL_ID = "overwatch_collector"
        private const val NOTIFICATION_ID = 4201
    }
}

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

    override fun onCreate() {
        super.onCreate()
        ensureNotificationChannel()
        startForeground(NOTIFICATION_ID, buildNotification("Collector starting…"))
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
            updateNotification("No Wi-Fi scan results yet")
            return
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
                updateNotification("$callsign • pushed ${wifiScan.size} Wi-Fi obs • ${location.latitude.format(4)}, ${location.longitude.format(4)}")
            }
        }.onFailure {
            updateNotification("Push failed: ${it.message}")
        }
    }

    private fun collectWifiNetworks(privacyMode: String): List<WifiObs> {
        val wifiManager = applicationContext.getSystemService(Context.WIFI_SERVICE) as WifiManager

        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            if (ActivityCompat.checkSelfPermission(this, Manifest.permission.NEARBY_WIFI_DEVICES) != PackageManager.PERMISSION_GRANTED) {
                return emptyList()
            }
        }

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

    private fun getBestLocation(): Location? {
        val lm = getSystemService(Context.LOCATION_SERVICE) as LocationManager
        if (ActivityCompat.checkSelfPermission(this, Manifest.permission.ACCESS_FINE_LOCATION) != PackageManager.PERMISSION_GRANTED &&
            ActivityCompat.checkSelfPermission(this, Manifest.permission.ACCESS_COARSE_LOCATION) != PackageManager.PERMISSION_GRANTED
        ) {
            return null
        }

        val providers = lm.getProviders(true)
        var best: Location? = null
        for (p in providers) {
            val loc = runCatching { lm.getLastKnownLocation(p) }.getOrNull() ?: continue
            if (best == null || loc.accuracy < best!!.accuracy) best = loc
        }
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
        val latQ = (lat * 100).toInt()
        val lonQ = (lon * 100).toInt()
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
        private const val CHANNEL_ID = "overwatch_collector"
        private const val NOTIFICATION_ID = 4201
    }
}

package ai.overwatch.android

import android.Manifest
import android.content.Intent
import android.content.IntentFilter
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import android.content.BroadcastReceiver
import android.content.Context
import android.widget.ArrayAdapter
import android.widget.Button
import android.widget.CheckBox
import android.widget.EditText
import android.widget.Spinner
import android.widget.TextView
import org.json.JSONArray
import org.json.JSONObject
import java.io.BufferedReader
import java.io.InputStreamReader
import java.io.OutputStreamWriter
import java.net.HttpURLConnection
import java.net.URL
import java.util.concurrent.Executors
import androidx.activity.result.contract.ActivityResultContracts
import androidx.appcompat.app.AppCompatActivity
import androidx.core.content.ContextCompat
import androidx.core.content.ContextCompat.RECEIVER_NOT_EXPORTED

class MainActivity : AppCompatActivity() {

    private lateinit var callsignInput: EditText
    private lateinit var hubUrlInput: EditText
    private lateinit var privacySpinner: Spinner
    private lateinit var pliModeSpinner: Spinner
    private lateinit var pullEntitiesCheck: CheckBox
    private lateinit var pullHeatCheck: CheckBox
    private lateinit var pullCamsCheck: CheckBox
    private lateinit var pullSatCheck: CheckBox
    private lateinit var statusText: TextView
    private lateinit var debugLogText: TextView
    private lateinit var msgTargetInput: EditText
    private lateinit var msgBodyInput: EditText
    private lateinit var groupIdInput: EditText
    private lateinit var msgLogText: TextView

    private val io = Executors.newSingleThreadExecutor()
    private var lastInboxId: Long = 0

    private val collectorReceiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context?, intent: Intent?) {
            if (intent?.action != CollectorService.ACTION_STATUS) return
            val msg = intent.getStringExtra("msg") ?: return
            appendDebug(msg)
        }
    }

    private val permissionLauncher = registerForActivityResult(
        ActivityResultContracts.RequestMultiplePermissions()
    ) { result ->
        val denied = result.filterValues { !it }.keys
        if (denied.isEmpty()) {
            statusText.text = "Permissions granted"
        } else {
            statusText.text = "Missing permissions: ${denied.joinToString()}"
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_main)

        callsignInput = findViewById(R.id.callsignInput)
        hubUrlInput = findViewById(R.id.hubUrlInput)
        privacySpinner = findViewById(R.id.privacySpinner)
        pliModeSpinner = findViewById(R.id.pliModeSpinner)
        pullEntitiesCheck = findViewById(R.id.pullEntitiesCheck)
        pullHeatCheck = findViewById(R.id.pullHeatCheck)
        pullCamsCheck = findViewById(R.id.pullCamsCheck)
        pullSatCheck = findViewById(R.id.pullSatCheck)
        statusText = findViewById(R.id.statusText)
        debugLogText = findViewById(R.id.debugLogText)
        msgTargetInput = findViewById(R.id.msgTargetInput)
        msgBodyInput = findViewById(R.id.msgBodyInput)
        groupIdInput = findViewById(R.id.groupIdInput)
        msgLogText = findViewById(R.id.msgLogText)

        callsignInput.setText(ConfigStore.getCallsign(this))
        hubUrlInput.setText(ConfigStore.getHubUrl(this))

        val privacyModes = listOf(
            "A - Channel only (default)",
            "B - Hashed identifiers",
            "C - Raw identifiers"
        )

        privacySpinner.adapter = ArrayAdapter(
            this,
            android.R.layout.simple_spinner_dropdown_item,
            privacyModes
        )

        val savedMode = ConfigStore.getPrivacyMode(this)
        privacySpinner.setSelection(
            when (savedMode) { "B" -> 1; "C" -> 2; else -> 0 }
        )

        val pliModes = listOf("COP", "LOCAL", "MERGED")
        pliModeSpinner.adapter = ArrayAdapter(this, android.R.layout.simple_spinner_dropdown_item, pliModes)
        pliModeSpinner.setSelection(when (ConfigStore.getPliMode(this)) { "LOCAL" -> 1; "MERGED" -> 2; else -> 0 })
        pullEntitiesCheck.isChecked = ConfigStore.getPullEntities(this)
        pullHeatCheck.isChecked = ConfigStore.getPullHeat(this)
        pullCamsCheck.isChecked = ConfigStore.getPullCams(this)
        pullSatCheck.isChecked = ConfigStore.getPullSat(this)

        findViewById<Button>(R.id.saveConfigBtn).setOnClickListener {
            val mode = when (privacySpinner.selectedItemPosition) {
                1 -> "B"
                2 -> "C"
                else -> "A"
            }
            val callsign = callsignInput.text.toString().trim().ifEmpty { "ANDROID-EUD" }
            val hub = hubUrlInput.text.toString().trim()
            val pliMode = when (pliModeSpinner.selectedItemPosition) { 1 -> "LOCAL"; 2 -> "MERGED"; else -> "COP" }
            ConfigStore.setCallsign(this, callsign)
            ConfigStore.setHubUrl(this, hub)
            ConfigStore.setPrivacyMode(this, mode)
            ConfigStore.setPliMode(this, pliMode)
            ConfigStore.setPullEntities(this, pullEntitiesCheck.isChecked)
            ConfigStore.setPullHeat(this, pullHeatCheck.isChecked)
            ConfigStore.setPullCams(this, pullCamsCheck.isChecked)
            ConfigStore.setPullSat(this, pullSatCheck.isChecked)
            statusText.text = "Config saved • $callsign • Hub: $hub • Privacy: $mode • PLI: $pliMode"
        }

        findViewById<Button>(R.id.startCollectorBtn).setOnClickListener {
            requestPermissionsIfNeeded()
            val intent = Intent(this, CollectorService::class.java)
            ContextCompat.startForegroundService(this, intent)
            statusText.text = "Collector service started"
        }

        findViewById<Button>(R.id.openMapBtn).setOnClickListener {
            val hub = ConfigStore.getHubUrl(this)
            val callsign = ConfigStore.getCallsign(this)
            val intent = Intent(this, TacticalMapActivity::class.java)
                .putExtra(TacticalMapActivity.EXTRA_HUB_URL, hub)
                .putExtra(TacticalMapActivity.EXTRA_CALLSIGN, callsign)
                .putExtra(TacticalMapActivity.EXTRA_PLI_MODE, ConfigStore.getPliMode(this))
                .putExtra(TacticalMapActivity.EXTRA_PULL_ENTITIES, ConfigStore.getPullEntities(this))
                .putExtra(TacticalMapActivity.EXTRA_PULL_HEAT, ConfigStore.getPullHeat(this))
                .putExtra(TacticalMapActivity.EXTRA_PULL_CAMS, ConfigStore.getPullCams(this))
                .putExtra(TacticalMapActivity.EXTRA_PULL_SAT, ConfigStore.getPullSat(this))
            startActivity(intent)
        }

        findViewById<Button>(R.id.sendMsgBtn).setOnClickListener { sendMessage() }
        findViewById<Button>(R.id.pollInboxBtn).setOnClickListener { pollInbox() }
        findViewById<Button>(R.id.createGroupBtn).setOnClickListener { upsertGroup(join = false) }
        findViewById<Button>(R.id.joinGroupBtn).setOnClickListener { upsertGroup(join = true) }
    }

    override fun onStart() {
        super.onStart()
        val filter = IntentFilter(CollectorService.ACTION_STATUS)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            ContextCompat.registerReceiver(this, collectorReceiver, filter, RECEIVER_NOT_EXPORTED)
        } else {
            @Suppress("DEPRECATION")
            registerReceiver(collectorReceiver, filter)
        }
    }

    override fun onStop() {
        runCatching { unregisterReceiver(collectorReceiver) }
        super.onStop()
    }

    private fun appendDebug(msg: String) {
        val now = java.time.LocalTime.now().withNano(0).toString()
        val existing = debugLogText.text?.toString().orEmpty()
        val lines = (existing + "\n[$now] $msg").trim().lines()
        val trimmed = if (lines.size > 80) lines.takeLast(80).joinToString("\n") else lines.joinToString("\n")
        debugLogText.text = trimmed
    }

    private fun appendMsgLog(msg: String) {
        val now = java.time.LocalTime.now().withNano(0).toString()
        val existing = msgLogText.text?.toString().orEmpty()
        val lines = (existing + "\n[$now] $msg").trim().lines()
        val trimmed = if (lines.size > 120) lines.takeLast(120).joinToString("\n") else lines.joinToString("\n")
        msgLogText.text = trimmed
    }

    private fun hubBase(): String = ConfigStore.getHubUrl(this).trim().trimEnd('/')
    private fun selfId(): String = ConfigStore.getCallsign(this).trim().ifEmpty { "ANDROID-EUD" }

    private fun httpJson(method: String, url: String, body: JSONObject? = null): String {
        val conn = URL(url).openConnection() as HttpURLConnection
        conn.requestMethod = method
        conn.connectTimeout = 5000
        conn.readTimeout = 7000
        conn.setRequestProperty("Content-Type", "application/json")
        conn.doInput = true
        if (body != null) {
            conn.doOutput = true
            OutputStreamWriter(conn.outputStream).use { it.write(body.toString()) }
        }
        val code = conn.responseCode
        val stream = if (code in 200..299) conn.inputStream else conn.errorStream
        val text = BufferedReader(InputStreamReader(stream)).use { it.readText() }
        if (code !in 200..299) throw IllegalStateException("HTTP $code: $text")
        return text
    }

    private fun sendMessage() {
        val target = msgTargetInput.text.toString().trim()
        val body = msgBodyInput.text.toString().trim()
        if (target.isEmpty() || body.isEmpty()) {
            statusText.text = "Message target/body required"
            return
        }
        io.execute {
            try {
                val req = JSONObject().put("from", selfId()).put("body", body)
                if (target.startsWith("group:")) req.put("to_group", target.removePrefix("group:"))
                else req.put("to_device", target)
                val out = httpJson("POST", "${hubBase()}/api/msg/send", req)
                runOnUiThread {
                    appendMsgLog("TX -> $target :: $body")
                    statusText.text = "Message sent"
                    msgBodyInput.setText("")
                }
            } catch (e: Exception) {
                runOnUiThread { statusText.text = "Send failed: ${e.message}" }
            }
        }
    }

    private fun pollInbox() {
        io.execute {
            try {
                val raw = httpJson("GET", "${hubBase()}/api/msg/inbox?device_id=${selfId()}&after_id=$lastInboxId&limit=100")
                val arr = JSONArray(raw)
                var maxId = lastInboxId
                val logs = mutableListOf<String>()
                for (i in 0 until arr.length()) {
                    val m = arr.getJSONObject(i)
                    val id = m.optLong("id", 0L)
                    val from = m.optString("from", "?")
                    val txt = m.optString("body", "")
                    logs += "RX <$from>: $txt"
                    if (id > maxId) maxId = id
                }
                lastInboxId = maxId
                runOnUiThread {
                    if (logs.isEmpty()) appendMsgLog("Inbox: no new messages")
                    else logs.forEach { appendMsgLog(it) }
                    statusText.text = "Inbox synced"
                }
            } catch (e: Exception) {
                runOnUiThread { statusText.text = "Inbox failed: ${e.message}" }
            }
        }
    }

    private fun upsertGroup(join: Boolean) {
        val gid = groupIdInput.text.toString().trim()
        if (gid.isEmpty()) {
            statusText.text = "Group ID required"
            return
        }
        io.execute {
            try {
                val req = JSONObject()
                    .put("group_id", gid)
                    .put("name", gid)
                    .put("device_id", selfId())
                val endpoint = if (join) "/api/msg/group/join" else "/api/msg/group/upsert"
                httpJson("POST", "${hubBase()}$endpoint", req)
                runOnUiThread {
                    appendMsgLog(if (join) "Joined group: $gid" else "Created group: $gid")
                    statusText.text = "Group updated"
                }
            } catch (e: Exception) {
                runOnUiThread { statusText.text = "Group op failed: ${e.message}" }
            }
        }
    }

    private fun requestPermissionsIfNeeded() {
        val needed = mutableListOf(
            Manifest.permission.ACCESS_FINE_LOCATION,
            Manifest.permission.ACCESS_COARSE_LOCATION
        )
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            needed += Manifest.permission.NEARBY_WIFI_DEVICES
            needed += Manifest.permission.POST_NOTIFICATIONS
        }

        val missing = needed.filter {
            ContextCompat.checkSelfPermission(this, it) != PackageManager.PERMISSION_GRANTED
        }

        if (missing.isNotEmpty()) {
            permissionLauncher.launch(missing.toTypedArray())
        }
    }
}

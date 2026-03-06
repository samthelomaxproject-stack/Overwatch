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

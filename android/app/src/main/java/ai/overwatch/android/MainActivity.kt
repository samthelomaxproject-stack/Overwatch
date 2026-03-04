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

        findViewById<Button>(R.id.saveConfigBtn).setOnClickListener {
            val mode = when (privacySpinner.selectedItemPosition) {
                1 -> "B"
                2 -> "C"
                else -> "A"
            }
            val callsign = callsignInput.text.toString().trim().ifEmpty { "ANDROID-EUD" }
            val hub = hubUrlInput.text.toString().trim()
            ConfigStore.setCallsign(this, callsign)
            ConfigStore.setHubUrl(this, hub)
            ConfigStore.setPrivacyMode(this, mode)
            statusText.text = "Config saved • $callsign • Hub: $hub • Privacy: $mode"
        }

        findViewById<Button>(R.id.startCollectorBtn).setOnClickListener {
            requestPermissionsIfNeeded()
            val intent = Intent(this, CollectorService::class.java)
            ContextCompat.startForegroundService(this, intent)
            statusText.text = "Collector service started"
        }

        findViewById<Button>(R.id.openMapBtn).setOnClickListener {
            val hub = hubUrlInput.text.toString().trim().ifEmpty { ConfigStore.getHubUrl(this) }
            val callsign = callsignInput.text.toString().trim().ifEmpty { ConfigStore.getCallsign(this) }
            val intent = Intent(this, TacticalMapActivity::class.java)
                .putExtra(TacticalMapActivity.EXTRA_HUB_URL, hub)
                .putExtra(TacticalMapActivity.EXTRA_CALLSIGN, callsign)
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

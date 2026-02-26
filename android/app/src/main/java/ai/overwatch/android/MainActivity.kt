package ai.overwatch.android

import android.Manifest
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import android.widget.ArrayAdapter
import android.widget.Button
import android.widget.EditText
import android.widget.Spinner
import android.widget.TextView
import androidx.activity.result.contract.ActivityResultContracts
import androidx.appcompat.app.AppCompatActivity
import androidx.core.content.ContextCompat

class MainActivity : AppCompatActivity() {

    private lateinit var hubUrlInput: EditText
    private lateinit var privacySpinner: Spinner
    private lateinit var statusText: TextView

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

        hubUrlInput = findViewById(R.id.hubUrlInput)
        privacySpinner = findViewById(R.id.privacySpinner)
        statusText = findViewById(R.id.statusText)

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
            val hub = hubUrlInput.text.toString().trim()
            ConfigStore.setHubUrl(this, hub)
            ConfigStore.setPrivacyMode(this, mode)
            statusText.text = "Config saved • Hub: $hub • Privacy: $mode"
        }

        findViewById<Button>(R.id.startCollectorBtn).setOnClickListener {
            requestPermissionsIfNeeded()
            val intent = Intent(this, CollectorService::class.java)
            ContextCompat.startForegroundService(this, intent)
            statusText.text = "Collector service started"
        }

        findViewById<Button>(R.id.openMapBtn).setOnClickListener {
            statusText.text = "Map/heatmap view is next milestone"
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

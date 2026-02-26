package ai.overwatch.android

import android.os.Bundle
import android.widget.ArrayAdapter
import android.widget.Button
import android.widget.EditText
import android.widget.Spinner
import android.widget.TextView
import androidx.appcompat.app.AppCompatActivity

class MainActivity : AppCompatActivity() {

    private lateinit var hubUrlInput: EditText
    private lateinit var privacySpinner: Spinner
    private lateinit var statusText: TextView

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_main)

        hubUrlInput = findViewById(R.id.hubUrlInput)
        privacySpinner = findViewById(R.id.privacySpinner)
        statusText = findViewById(R.id.statusText)

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

        findViewById<Button>(R.id.saveConfigBtn).setOnClickListener {
            val mode = when (privacySpinner.selectedItemPosition) {
                1 -> "B"
                2 -> "C"
                else -> "A"
            }
            val hub = hubUrlInput.text.toString().trim()
            statusText.text = "Config saved • Hub: $hub • Privacy: $mode"
            // TODO: persist to DataStore + wire collector service
        }

        findViewById<Button>(R.id.startCollectorBtn).setOnClickListener {
            statusText.text = "Collector start requested (stub)"
            // TODO: start foreground service for GPS + Wi-Fi scan + sync
        }

        findViewById<Button>(R.id.openMapBtn).setOnClickListener {
            statusText.text = "Map/heatmap view coming in next step"
            // TODO: open tactical map fragment with heat overlays
        }
    }
}

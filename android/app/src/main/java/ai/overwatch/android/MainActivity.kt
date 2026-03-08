package ai.overwatch.android

import android.Manifest
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import androidx.activity.result.contract.ActivityResultContracts
import androidx.appcompat.app.AppCompatActivity
import androidx.core.content.ContextCompat

class MainActivity : AppCompatActivity() {

    private val permissionLauncher = registerForActivityResult(
        ActivityResultContracts.RequestMultiplePermissions()
    ) {
        launchMapAndFinish()
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        requestPermissionsIfNeededAndLaunch()
    }

    private fun requestPermissionsIfNeededAndLaunch() {
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
        } else {
            launchMapAndFinish()
        }
    }

    private fun launchMapAndFinish() {
        val hub = ConfigStore.getHubUrl(this)
        val callsign = ConfigStore.getCallsign(this)

        // Start collector automatically so local PLI and sync pipeline are active.
        val collectorIntent = Intent(this, CollectorService::class.java)
        ContextCompat.startForegroundService(this, collectorIntent)

        val mapIntent = Intent(this, TacticalMapActivity::class.java)
            .putExtra(TacticalMapActivity.EXTRA_HUB_URL, hub)
            .putExtra(TacticalMapActivity.EXTRA_CALLSIGN, callsign)
            .putExtra(TacticalMapActivity.EXTRA_PLI_MODE, ConfigStore.getPliMode(this))
            .putExtra(TacticalMapActivity.EXTRA_PULL_ENTITIES, ConfigStore.getPullEntities(this))
            .putExtra(TacticalMapActivity.EXTRA_PULL_HEAT, ConfigStore.getPullHeat(this))
            .putExtra(TacticalMapActivity.EXTRA_PULL_CAMS, ConfigStore.getPullCams(this))
            .putExtra(TacticalMapActivity.EXTRA_PULL_SAT, ConfigStore.getPullSat(this))

        startActivity(mapIntent)
        finish()
    }
}

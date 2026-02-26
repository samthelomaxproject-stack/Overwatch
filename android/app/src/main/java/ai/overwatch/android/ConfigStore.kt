package ai.overwatch.android

import android.content.Context

object ConfigStore {
    private const val PREFS = "overwatch_eud"
    private const val KEY_HUB_URL = "hub_url"
    private const val KEY_PRIVACY_MODE = "privacy_mode"
    private const val KEY_CALLSIGN = "callsign"

    fun getHubUrl(context: Context): String {
        return context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
            .getString(KEY_HUB_URL, "http://10.0.0.5:8789") ?: "http://10.0.0.5:8789"
    }

    fun setHubUrl(context: Context, url: String) {
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
            .edit().putString(KEY_HUB_URL, url).apply()
    }

    fun getPrivacyMode(context: Context): String {
        return context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
            .getString(KEY_PRIVACY_MODE, "A") ?: "A"
    }

    fun setPrivacyMode(context: Context, mode: String) {
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
            .edit().putString(KEY_PRIVACY_MODE, mode).apply()
    }

    fun getCallsign(context: Context): String {
        return context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
            .getString(KEY_CALLSIGN, "ANDROID-EUD") ?: "ANDROID-EUD"
    }

    fun setCallsign(context: Context, callsign: String) {
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
            .edit().putString(KEY_CALLSIGN, callsign).apply()
    }
}

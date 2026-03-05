package ai.overwatch.android

import android.content.Context

object ConfigStore {
    private const val PREFS = "overwatch_eud"
    private const val KEY_HUB_URL = "hub_url"
    private const val KEY_PRIVACY_MODE = "privacy_mode"
    private const val KEY_CALLSIGN = "callsign"
    private const val KEY_PLI_MODE = "pli_mode"
    private const val KEY_PULL_ENTITIES = "pull_entities"
    private const val KEY_PULL_HEAT = "pull_heat"
    private const val KEY_PULL_CAMS = "pull_cams"
    private const val KEY_PULL_SAT = "pull_sat"

    fun getHubUrl(context: Context): String {
        return context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
            .getString(KEY_HUB_URL, "http://192.168.1.143:8789") ?: "http://192.168.1.143:8789"
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

    fun getPliMode(context: Context): String {
        return context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
            .getString(KEY_PLI_MODE, "COP") ?: "COP"
    }

    fun setPliMode(context: Context, mode: String) {
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
            .edit().putString(KEY_PLI_MODE, mode).apply()
    }

    fun getPullEntities(context: Context): Boolean = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).getBoolean(KEY_PULL_ENTITIES, true)
    fun getPullHeat(context: Context): Boolean = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).getBoolean(KEY_PULL_HEAT, true)
    fun getPullCams(context: Context): Boolean = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).getBoolean(KEY_PULL_CAMS, false)
    fun getPullSat(context: Context): Boolean = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).getBoolean(KEY_PULL_SAT, false)

    fun setPullEntities(context: Context, v: Boolean) { context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit().putBoolean(KEY_PULL_ENTITIES, v).apply() }
    fun setPullHeat(context: Context, v: Boolean) { context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit().putBoolean(KEY_PULL_HEAT, v).apply() }
    fun setPullCams(context: Context, v: Boolean) { context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit().putBoolean(KEY_PULL_CAMS, v).apply() }
    fun setPullSat(context: Context, v: Boolean) { context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit().putBoolean(KEY_PULL_SAT, v).apply() }
}

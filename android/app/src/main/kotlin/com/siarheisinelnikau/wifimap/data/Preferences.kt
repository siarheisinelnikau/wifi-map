package com.siarheisinelnikau.wifimap.data

import android.content.Context
import org.json.JSONObject

data class UserPreferences(val language: String = "en", val dark: Boolean = false, val reducedMotion: Boolean = false, val startupUpdates: Boolean = true)
class Preferences(context: Context) {
    private val storage = context.getSharedPreferences("wifimap-preferences", Context.MODE_PRIVATE)
    fun load() = UserPreferences(storage.getString("language", "en") ?: "en", storage.getBoolean("dark", false), storage.getBoolean("reduced_motion", false), storage.getBoolean("startup_updates", true))
    fun save(value: UserPreferences) { storage.edit().putString("language", value.language).putBoolean("dark", value.dark).putBoolean("reduced_motion", value.reducedMotion).putBoolean("startup_updates", value.startupUpdates).apply() }
}
class Text(context: Context, language: String) {
    private val index = listOf("en", "ru", "fr", "de", "es", "it").indexOf(language).coerceAtLeast(0)
    private val catalog = JSONObject(context.assets.open("catalog.json").bufferedReader().use { it.readText() })
    operator fun get(key: String): String = catalog.optJSONArray(key)?.optString(index)?.takeIf { it.isNotBlank() } ?: catalog.optJSONArray(key)?.optString(0) ?: key
}

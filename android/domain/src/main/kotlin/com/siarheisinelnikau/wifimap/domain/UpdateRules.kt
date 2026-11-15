package com.siarheisinelnikau.wifimap.domain

import com.google.gson.JsonParser

data class StableVersion(val major: Long, val minor: Long, val patch: Long) : Comparable<StableVersion> {
    override fun compareTo(other: StableVersion): Int = compareValuesBy(this, other, { it.major }, { it.minor }, { it.patch })
    override fun toString() = "$major.$minor.$patch"
    companion object {
        fun parse(text: String): StableVersion {
            val match = Regex("^v?(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)$").matchEntire(text) ?: throw SurveyFailure("update_channel")
            return try { StableVersion(match.groupValues[1].toLong(), match.groupValues[2].toLong(), match.groupValues[3].toLong()) } catch (_: Exception) { throw SurveyFailure("update_channel") }
        }
    }
}
data class AndroidRelease(val version: StableVersion, val name: String, val url: String, val size: Long, val sha256: String?, val checksumUrl: String?)
object UpdateRules {
    const val REPOSITORY = "siarheisinelnikau/wifi-map"
    fun dispatch(manual: Boolean, startupEnabled: Boolean, request: () -> Unit) { if (manual || startupEnabled) request() }
    fun release(json: String, installed: String): AndroidRelease? {
        try {
            val root = JsonParser.parseString(json).asJsonObject
            if (root["draft"].asBoolean || root["prerelease"].asBoolean) throw SurveyFailure("update_channel")
            val tag = root["tag_name"].asString; val version = StableVersion.parse(tag)
            if (version <= StableVersion.parse(installed)) return null
            val name = "WifiMap-v$version.apk"; val prefix = "https://github.com/$REPOSITORY/releases/download/$tag/"
            val assets = root["assets"].asJsonArray.map { it.asJsonObject }
            val asset = assets.find { it["name"].asString == name } ?: throw SurveyFailure("update_asset")
            val url = asset["browser_download_url"].asString; val size = asset["size"].asLong
            if (url != prefix + name || size !in 512..80L * 1024 * 1024) throw SurveyFailure("update_asset")
            val digest = asset["digest"]?.takeUnless { it.isJsonNull }?.asString?.removePrefix("sha256:")?.also { if (!it.matches(Regex("[0-9a-fA-F]{64}"))) throw SurveyFailure("update_checksum") }
            val sidecar = assets.find { it["name"].asString == "$name.sha256" && it["browser_download_url"].asString == "$prefix$name.sha256" && it["size"].asLong <= 1024 }?.get("browser_download_url")?.asString
            if (digest == null && sidecar == null) throw SurveyFailure("update_checksum")
            return AndroidRelease(version, name, url, size, digest?.lowercase(), sidecar)
        } catch (e: SurveyFailure) { throw e } catch (e: Exception) { throw SurveyFailure("update_error", e.javaClass.simpleName) }
    }
}

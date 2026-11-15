package com.siarheisinelnikau.wifimap.domain

data class ScanClock(val requestedElapsedUs: Long, val collectedElapsedUs: Long, val collectedUnixMs: Long)
object ScanRules {
    fun observation(clock: ScanClock, seenElapsedUs: Long, ssid: List<Int>, bssid: List<Int>, rssi: Int, frequencyMhz: Int, rawSsid: Boolean, connected: List<Int>?): Observation {
        val ageUs = clock.collectedElapsedUs - seenElapsedUs
        if (seenElapsedUs < clock.requestedElapsedUs || seenElapsedUs <= 0 || ageUs !in 0..15_000_000) throw SurveyFailure("wlan_stale")
        val khz = frequencyMhz.takeIf { it in 1..100_000 }?.times(1000)
        return Observation("android-system-wifi", ssid, bssid, khz, khz?.let(::channelFromKhz), rssi.takeIf { it in -127..0 }, null, clock.collectedUnixMs, clock.collectedUnixMs - ageUs / 1000, connected, ObservationSource.AndroidScan, if (rawSsid) SsidEncoding.RawBytes else SsidEncoding.Utf8Display, seenElapsedUs)
    }
    fun checkConnection(before: List<Int>?, after: List<Int>?) { if (before != after) throw SurveyFailure("wlan_roamed") }
}
class ScanThrottle {
    private val requests = ArrayDeque<Long>()
    @Synchronized fun reserve(nowMs: Long): Long {
        if (requests.lastOrNull()?.let { nowMs < it } == true) requests.clear()
        while (requests.firstOrNull()?.let { nowMs - it >= 120_000 } == true) requests.removeFirst()
        if (requests.size >= 4) return ((120_000 - (nowMs - requests.first())) + 999) / 1000
        requests.addLast(nowMs)
        return 0
    }
}

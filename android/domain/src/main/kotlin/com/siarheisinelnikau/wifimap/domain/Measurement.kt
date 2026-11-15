package com.siarheisinelnikau.wifimap.domain

interface ScanPort { suspend fun scan(target: List<Int>?): List<Observation> }
data class MeasurementTarget(val surveyId: Long, val position: Point, val bssid: List<Int>)
class CollectMeasurement(private val scanner: ScanPort) {
    suspend operator fun invoke(target: MeasurementTarget): Observation {
        val observation = scanner.scan(target.bssid).firstOrNull { it.bssid == target.bssid } ?: throw SurveyFailure("wlan_no_networks")
        if (observation.source != ObservationSource.AndroidScan || observation.observedUnixMs > observation.collectedUnixMs || observation.collectedUnixMs - observation.observedUnixMs > 15_000) throw SurveyFailure("wlan_stale")
        return observation
    }
}

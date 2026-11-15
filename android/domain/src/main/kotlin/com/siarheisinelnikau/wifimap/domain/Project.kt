package com.siarheisinelnikau.wifimap.domain

import kotlin.math.hypot

const val SCHEMA_VERSION = 2
const val MAX_SAMPLES = 10_000
const val MAX_SURVEYS = 64
const val MAX_SAFE_ID = 9_007_199_254_740_991L
const val MAX_PROJECT_BYTES = 48 * 1024 * 1024
const val MAX_IMAGE_BYTES = 24 * 1024 * 1024

class SurveyFailure(val key: String, val detail: String = "") : Exception("$key: $detail")

data class Point(val x: Float, val y: Float) {
    fun distance(other: Point): Float = hypot(x - other.x, y - other.y)
    fun inside(size: List<Int>): Boolean = x.isFinite() && y.isFinite() && x >= 0 && y >= 0 && x <= size[0] && y <= size[1]
}
data class Calibration(val a: Point, val b: Point, val meters: Float) {
    fun metersPerPixel(): Float? = if (meters.isFinite() && meters > 0 && meters <= 10_000 && a.distance(b) >= 1) meters / a.distance(b) else null
}
data class Wall(val a: Point, val b: Point)
data class PlanImage(val name: String, val png: String)
data class FloorPlan(val size: List<Int> = listOf(1000, 700), val image: PlanImage? = null, val walls: List<Wall> = emptyList(), val calibration: Calibration? = null)
enum class ObservationSource { WindowsBss, AndroidScan, Demo }
enum class SsidEncoding { RawBytes, Utf8Display }
enum class Metric(val unit: String, val minimum: Float, val maximum: Float) {
    RssiDbm("dBm", -90f, -40f), LinkQualityPercent("%", 0f, 100f);
    val differenceUnit: String get() = if (this == RssiDbm) "dB" else "pp"
}
data class Observation(
    val adapterId: String,
    val ssid: List<Int>,
    val bssid: List<Int>,
    val frequencyKhz: Int?,
    val channel: Int?,
    val rssiDbm: Int?,
    val linkQualityPercent: Int?,
    val collectedUnixMs: Long,
    val observedUnixMs: Long,
    val connectedBssid: List<Int>?,
    val source: ObservationSource,
    val ssidEncoding: SsidEncoding? = null,
    val androidObservedElapsedUs: Long? = null,
) {
    fun value(metric: Metric): Float? = when (metric) { Metric.RssiDbm -> rssiDbm?.toFloat(); Metric.LinkQualityPercent -> linkQualityPercent?.toFloat() }
    fun ssidLabel(): String = if (ssid.isEmpty()) "[${bssidLabel(bssid)}]" else ssid.map(Int::toByte).toByteArray().toString(Charsets.UTF_8).filterNot(Char::isISOControl)
}
data class LocalProbe(val peer: String, val collectedUnixMs: Long, val medianRttMs: Double, val payloadMbps: Double, val receivedBytes: Int, val elapsedMs: Long)
data class Sample(val id: Long, val position: Point, val observation: Observation, val localProbe: LocalProbe? = null)
data class Survey(val id: Long, val name: String = "", val router: Point? = null, val samples: List<Sample> = emptyList())
data class WifiProject(val schemaVersion: Int = SCHEMA_VERSION, val name: String = "", val plan: FloorPlan = FloorPlan(), val surveys: List<Survey> = listOf(Survey(1)), val nextId: Long = 2, val demo: Boolean = false)

fun bssidLabel(bytes: List<Int>): String = bytes.joinToString(":") { "%02X".format(it) }
fun parseBssid(text: String?): List<Int>? {
    if (text == null || !text.matches(Regex("(?i)[0-9a-f]{2}(:[0-9a-f]{2}){5}"))) return null
    val bytes = text.split(':').map { it.toInt(16) }
    return bytes.takeUnless { it.all { b -> b == 0 } || it == listOf(2, 0, 0, 0, 0, 0) }
}
fun channelFromKhz(khz: Int): Int? {
    if (khz % 1000 != 0) return null
    val mhz = khz / 1000
    return when {
        mhz == 2484 -> 14
        mhz in 2412..2472 && (mhz - 2407) % 5 == 0 -> (mhz - 2407) / 5
        mhz in 5000..5895 && (mhz - 5000) % 5 == 0 -> (mhz - 5000) / 5
        mhz == 5935 -> 2
        mhz in 5955..7115 && (mhz - 5950) % 5 == 0 -> (mhz - 5950) / 5
        else -> null
    }
}

object SurveyRules {
    fun addSurvey(project: WifiProject, name: String): WifiProject {
        if (project.surveys.size >= MAX_SURVEYS) throw SurveyFailure("survey_limit")
        return project.copy(surveys = project.surveys + Survey(project.nextId, name), nextId = project.nextId + 1)
    }
    fun record(project: WifiProject, surveyId: Long, position: Point, observation: Observation): WifiProject {
        if (!position.inside(project.plan.size) || project.surveys.none { it.id == surveyId }) throw SurveyFailure("invalid_project")
        if (project.demo != (observation.source == ObservationSource.Demo)) throw SurveyFailure("demo_isolated")
        if (project.surveys.sumOf { it.samples.size } >= MAX_SAMPLES) throw SurveyFailure("sample_limit")
        val next = project.copy(nextId = project.nextId + 1, surveys = project.surveys.map { survey ->
            if (survey.id != surveyId) survey else survey.copy(samples = survey.samples + Sample(project.nextId, position, observation))
        })
        ProjectCodec.validate(next.copy(plan = next.plan.copy(image = null)))
        return next
    }
    fun move(project: WifiProject, surveyId: Long, sampleId: Long, position: Point): WifiProject {
        if (!position.inside(project.plan.size)) throw SurveyFailure("invalid_project")
        return project.copy(surveys = project.surveys.map { survey -> if (survey.id != surveyId) survey else survey.copy(samples = survey.samples.map { if (it.id == sampleId) it.copy(position = position) else it }) })
    }
}

data class ViewTransform(var origin: Point = Point(0f, 0f), var zoom: Float = 1f) {
    fun toScreen(point: Point) = Point(origin.x + point.x * zoom, origin.y + point.y * zoom)
    fun toPlan(point: Point) = Point((point.x - origin.x) / zoom, (point.y - origin.y) / zoom)
    fun zoomAt(screen: Point, factor: Float) {
        val anchor = toPlan(screen)
        zoom = (zoom * factor).coerceIn(0.02f, 15f)
        origin = Point(screen.x - anchor.x * zoom, screen.y - anchor.y * zoom)
    }
}

fun exampleProject(): WifiProject {
    var project = WifiProject(name = "Apartment example", demo = true, plan = FloorPlan(
        walls = listOf(Wall(Point(40f, 40f), Point(960f, 40f)), Wall(Point(960f, 40f), Point(960f, 660f)), Wall(Point(960f, 660f), Point(40f, 660f)), Wall(Point(40f, 660f), Point(40f, 40f)), Wall(Point(410f, 40f), Point(410f, 300f)), Wall(Point(410f, 390f), Point(410f, 660f)), Wall(Point(410f, 350f), Point(710f, 350f))),
        calibration = Calibration(Point(40f, 40f), Point(960f, 40f), 11.5f)), surveys = listOf(Survey(1, "Before", Point(170f, 150f)), Survey(2, "After", Point(490f, 330f))), nextId = 3)
    val locations = listOf(Point(150f, 150f), Point(300f, 220f), Point(160f, 500f), Point(340f, 570f), Point(520f, 160f), Point(800f, 170f), Point(550f, 510f), Point(850f, 540f))
    for (survey in project.surveys) for ((index, point) in locations.withIndex()) {
        val rssi = (-38f - point.distance(survey.router!!) * 0.058f - if (point.x > 410 && survey.id == 1L) 8f else 0f).toInt()
        project = SurveyRules.record(project, survey.id, point, Observation("example", "Home example".toByteArray().map { it.toInt() and 255 }, listOf(2, 0, 0, 0, 0, 1), 5_180_000, 36, rssi, null, 1_794_700_800_000 + index * 20_000, 1_794_700_799_900 + index * 20_000, listOf(2, 0, 0, 0, 0, 1), ObservationSource.Demo))
    }
    return project
}

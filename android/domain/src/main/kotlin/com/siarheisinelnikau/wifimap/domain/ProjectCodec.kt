package com.siarheisinelnikau.wifimap.domain

import com.google.gson.*
import java.nio.ByteBuffer
import java.util.Base64

object ProjectCodec {
    private val gson = GsonBuilder().serializeNulls().setFieldNamingPolicy(FieldNamingPolicy.LOWER_CASE_WITH_UNDERSCORES).disableHtmlEscaping().create()
    fun encode(project: WifiProject): String {
        validate(project)
        return gson.toJson(project.copy(schemaVersion = SCHEMA_VERSION)).also { if (it.toByteArray(Charsets.UTF_8).size > MAX_PROJECT_BYTES) throw SurveyFailure("file_too_large") }
    }
    fun decode(json: String): WifiProject {
        if (json.length > MAX_PROJECT_BYTES) throw SurveyFailure("file_too_large")
        try {
            val root = JsonParser.parseString(json).asJsonObject
            fun point(value: JsonElement) = value.asJsonObject.let { Point(it["x"].asFloat, it["y"].asFloat) }
            fun bytes(value: JsonElement): List<Int> = value.asJsonArray.map { it.asBigDecimal.intValueExact() }
            fun long(value: JsonElement): Long = value.asBigDecimal.longValueExact()
            fun <T> optional(value: JsonElement?, parse: (JsonElement) -> T): T? = if (value == null || value.isJsonNull) null else parse(value)
            val plan = root["plan"].asJsonObject
            val calibration = optional(plan["calibration"]) { e -> e.asJsonObject.let { Calibration(point(it["a"]), point(it["b"]), it["meters"].asFloat) } }
            val image = optional(plan["image"]) { e -> e.asJsonObject.let { PlanImage(it["name"].asString, it["png"].asString) } }
            val surveys = root["surveys"].asJsonArray.map { entry -> entry.asJsonObject.let { s ->
                Survey(long(s["id"]), s["name"].asString, optional(s["router"], ::point), s["samples"].asJsonArray.map { sample -> sample.asJsonObject.let { v ->
                    val o = v["observation"].asJsonObject
                    val observation = Observation(o["adapter_id"].asString, bytes(o["ssid"]), bytes(o["bssid"]), optional(o["frequency_khz"]) { it.asBigDecimal.intValueExact() }, optional(o["channel"]) { it.asBigDecimal.intValueExact() }, optional(o["rssi_dbm"]) { it.asBigDecimal.intValueExact() }, optional(o["link_quality_percent"]) { it.asBigDecimal.intValueExact() }, long(o["collected_unix_ms"]), long(o["observed_unix_ms"]), optional(o["connected_bssid"], ::bytes), ObservationSource.valueOf(o["source"].asString), optional(o["ssid_encoding"]) { SsidEncoding.valueOf(it.asString) }, optional(o["android_observed_elapsed_us"], ::long))
                    val probe = optional(v["local_probe"]) { p -> p.asJsonObject.let { LocalProbe(it["peer"].asString, long(it["collected_unix_ms"]), it["median_rtt_ms"].asDouble, it["payload_mbps"].asDouble, it["received_bytes"].asBigDecimal.intValueExact(), long(it["elapsed_ms"])) } }
                    Sample(long(v["id"]), point(v["position"]), observation, probe)
                } })
            } }
            val project = WifiProject(root["schema_version"].asBigDecimal.intValueExact(), root["name"].asString, FloorPlan(bytes(plan["size"]), image, plan["walls"].asJsonArray.map { it.asJsonObject.let { w -> Wall(point(w["a"]), point(w["b"])) } }, calibration), surveys, long(root["next_id"]), root["demo"].asBoolean)
            validate(project)
            return project.copy(schemaVersion = SCHEMA_VERSION)
        } catch (failure: SurveyFailure) { throw failure } catch (failure: Exception) { throw SurveyFailure("invalid_project", failure.javaClass.simpleName) }
    }
    fun validate(project: WifiProject) {
        fun requireValid(condition: Boolean) { if (!condition) throw SurveyFailure("invalid_project") }
        if (project.schemaVersion !in 1..SCHEMA_VERSION) throw SurveyFailure("project_version")
        requireValid(project.name.toByteArray().size <= 512 && project.plan.size.size == 2 && project.plan.size.all { it in 1..4096 })
        requireValid(project.surveys.size in 1..MAX_SURVEYS && project.plan.walls.size <= 10_000 && project.surveys.sumOf { it.samples.size } <= MAX_SAMPLES)
        requireValid(project.nextId in 2..MAX_SAFE_ID)
        val ids = HashSet<Long>()
        fun identity(id: Long) { requireValid(id in 1 until project.nextId && ids.add(id)) }
        fun bytes(value: List<Int>, length: IntRange) = value.size in length && value.all { it in 0..255 }
        project.plan.image?.let { image ->
            requireValid(image.name.toByteArray().size <= 1024 && image.png.length <= MAX_IMAGE_BYTES * 4 / 3 + 4)
            val png = try { Base64.getDecoder().decode(image.png) } catch (_: Exception) { throw SurveyFailure("image_error") }
            requireValid(png.size in 24..MAX_IMAGE_BYTES && png.take(8).toByteArray().contentEquals(byteArrayOf(-119, 80, 78, 71, 13, 10, 26, 10)))
            requireValid(ByteBuffer.wrap(png, 16, 8).let { listOf(it.int, it.int) } == project.plan.size)
        }
        project.plan.calibration?.let { requireValid(it.a.inside(project.plan.size) && it.b.inside(project.plan.size) && it.metersPerPixel() != null) }
        project.plan.walls.forEach { requireValid(it.a.inside(project.plan.size) && it.b.inside(project.plan.size)) }
        project.surveys.forEach { survey ->
            identity(survey.id); requireValid(survey.name.toByteArray().size <= 512 && (survey.router == null || survey.router.inside(project.plan.size)))
            survey.samples.forEach { sample ->
                identity(sample.id); requireValid(sample.position.inside(project.plan.size))
                val o = sample.observation
                requireValid(o.adapterId.toByteArray().size <= 256 && bytes(o.ssid, 0..32) && bytes(o.bssid, 6..6) && (o.connectedBssid == null || bytes(o.connectedBssid, 6..6)))
                requireValid(o.rssiDbm == null || o.rssiDbm in -127..0); requireValid(o.linkQualityPercent == null || o.linkQualityPercent in 0..100)
                requireValid(o.collectedUnixMs in 1..MAX_SAFE_ID && o.observedUnixMs in 1..(o.collectedUnixMs + 1000))
                requireValid(o.frequencyKhz == null || o.frequencyKhz > 0); requireValid(o.channel == null || o.channel in 0..65535)
                requireValid(project.demo == (o.source == ObservationSource.Demo))
                requireValid(o.source != ObservationSource.AndroidScan || (project.schemaVersion == 2 && o.linkQualityPercent == null && o.ssidEncoding != null && (o.androidObservedElapsedUs ?: 0) > 0))
                sample.localProbe?.let { requireValid(it.peer.length <= 128 && it.medianRttMs.isFinite() && it.medianRttMs >= 0 && it.payloadMbps.isFinite() && it.payloadMbps >= 0 && it.receivedBytes in 0..8 * 1024 * 1024 && it.elapsedMs in 0..60_000 && it.collectedUnixMs in 1..MAX_SAFE_ID) }
            }
        }
    }
}

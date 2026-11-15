package com.siarheisinelnikau.wifimap.domain

import org.junit.Assert.*
import org.junit.Test
import java.io.File
import kotlin.coroutines.*

class ProjectTest {
    private fun fixture() = javaClass.getResource("/mixed-platform.wifimap")!!.readText()
    @Test fun interoperableRoundTripPreservesBothPlatforms() {
        val project = ProjectCodec.decode(fixture())
        assertEquals(2, project.schemaVersion)
        assertEquals(8, project.surveys.sumOf { it.samples.size })
        assertEquals(ObservationSource.AndroidScan, project.surveys[0].samples[2].observation.source)
        assertNull(project.surveys[0].samples[2].observation.linkQualityPercent)
        assertEquals(listOf(255, 128, 65), project.surveys[0].samples[2].observation.ssid)
        val encoded = ProjectCodec.encode(project)
        assertEquals(project, ProjectCodec.decode(encoded))
        val output = File(System.getProperty("interop.output")); output.parentFile.mkdirs(); output.writeText(encoded)
    }
    @Test fun damagedCoordinatesIdentitiesAndQualityAreRejected() {
        val p = ProjectCodec.decode(fixture()); val s = p.surveys[0].samples[0]
        fun invalid(sample: Sample) = assertThrows(SurveyFailure::class.java) { ProjectCodec.validate(p.copy(surveys = listOf(p.surveys[0].copy(samples = listOf(sample, sample))))) }
        invalid(s.copy(position = Point(Float.NaN, 0f)))
        invalid(s)
        val android = p.surveys[0].samples[2]
        val invalidAndroid = android.copy(observation = android.observation.copy(linkQualityPercent = 85))
        assertThrows(SurveyFailure::class.java) { ProjectCodec.validate(p.copy(surveys = listOf(p.surveys[0].copy(samples = listOf(invalidAndroid))))) }
    }
    @Test fun transformsScaleAndChannelsHaveRealUnits() {
        val transform = ViewTransform(Point(20f, -40f), 0.8f); val cursor = Point(200f, 320f); val anchor = transform.toPlan(cursor)
        transform.zoomAt(cursor, 1.7f); assertTrue(transform.toScreen(anchor).distance(cursor) < 0.001f)
        assertEquals(0.02f, Calibration(Point(0f, 0f), Point(200f, 0f), 4f).metersPerPixel()!!, 0.00001f)
        assertNull(Calibration(Point(0f, 0f), Point(0f, 0f), 4f).metersPerPixel())
        assertEquals(14, channelFromKhz(2_484_000)); assertEquals(36, channelFromKhz(5_180_000)); assertEquals(1, channelFromKhz(5_955_000)); assertNull(channelFromKhz(123))
    }
    @Test fun movingSampleKeepsOriginalIdentityAndObservation() {
        val p = ProjectCodec.decode(fixture()); val before = p.surveys[0].samples[2]
        val moved = SurveyRules.move(p, 1, before.id, Point(200f, 200f)); val after = moved.surveys[0].samples[2]
        assertEquals(before.id, after.id); assertEquals(before.observation, after.observation); assertNotEquals(before.position, after.position)
        assertThrows(SurveyFailure::class.java) { SurveyRules.record(exampleProject(), 1, Point(50f, 50f), before.observation) }
    }
    @Test fun heatmapIsDeterministicBoundedAndCancellable() {
        val p = ProjectCodec.decode(fixture()); val ap = p.surveys[0].samples[0].observation.bssid
        val a = Heatmaps.compute(p, 1, null, ap, Metric.RssiDbm)!!; val b = Heatmaps.compute(p, 1, null, ap, Metric.RssiDbm)!!
        assertArrayEquals(a.argb(), b.argb()); assertTrue(a.cells.any { it != null }); assertNull(a.cells[0])
        assertNull(Heatmaps.compute(p, 1, null, ap, Metric.RssiDbm) { true })
        assertTrue(Heatmaps.compute(p, 1, null, ap, Metric.RssiDbm, 0.1f)!!.cells.all { it == null })
        val difference = Heatmaps.compute(p, 2, 1, ap, Metric.RssiDbm)!!; assertTrue(difference.cells.filterNotNull().all { kotlin.math.abs(it.value - 7f) < 0.001f })
        assertTrue(Heatmaps.compute(p, 1, null, ap, Metric.LinkQualityPercent)!!.cells.all { it == null })
        val report = File(System.getProperty("interop.output")).parentFile.resolve("android-heat.json")
        report.parentFile.mkdirs()
        report.writeText(com.google.gson.Gson().toJson(a.cells.map { it?.value }))
    }
    @Test fun noDataAndCollinearSamplesDoNotInventArea() {
        val p = exampleProject(); val ap = p.surveys[0].samples[0].observation.bssid
        assertTrue(Heatmaps.compute(p.copy(surveys = listOf(Survey(1))), 1, null, ap, Metric.RssiDbm)!!.cells.all { it == null })
        val line = p.surveys[0].copy(samples = p.surveys[0].samples.mapIndexed { i,s -> s.copy(position = Point(i * 50f, 100f)) })
        assertTrue(Heatmaps.compute(p.copy(surveys = listOf(line)), 1, null, ap, Metric.RssiDbm)!!.cells.all { it == null })
    }
    @Test fun androidClockFreshnessAndRoamingAreExplicit() {
        val c = ScanClock(1_000_000, 2_000_000, 1_800_000_000_000)
        val observation = ScanRules.observation(c, 1_500_000, listOf(65), listOf(2, 0, 0, 0, 0, 1), -66, 5180, true, null)
        assertEquals(c.collectedUnixMs - 500, observation.observedUnixMs); assertEquals(1_500_000L, observation.androidObservedElapsedUs); assertNull(observation.linkQualityPercent)
        assertThrows(SurveyFailure::class.java) { ScanRules.observation(c, 999_999, listOf(65), observation.bssid, -66, 5180, true, null) }
        assertThrows(SurveyFailure::class.java) { ScanRules.checkConnection(observation.bssid, null) }
    }
    @Test fun throttleDoesNotPollOrBypassAndroidBudget() {
        val policy = ScanThrottle(); repeat(4) { assertEquals(0L, policy.reserve(it * 1000L)) }; assertEquals(116L, policy.reserve(4000)); assertEquals(0L, policy.reserve(120_000)); assertEquals(0L, policy.reserve(100))
    }
    private fun <T> synchronous(block: suspend () -> T): T {
        var outcome: Result<T>? = null
        block.startCoroutine(object : Continuation<T> { override val context = EmptyCoroutineContext; override fun resumeWith(result: Result<T>) { outcome = result } })
        return requireNotNull(outcome).getOrThrow()
    }
    @Test fun failedAdapterNeverCreatesAZeroSignalSample() {
        val before = ProjectCodec.decode(fixture()); val target = MeasurementTarget(1, Point(40f, 40f), before.surveys[0].samples[0].observation.bssid)
        val failed = CollectMeasurement(object : ScanPort { override suspend fun scan(target: List<Int>?): List<Observation> = throw SurveyFailure("android_scan_throttled") })
        assertEquals("android_scan_throttled", assertThrows(SurveyFailure::class.java) { synchronous { failed(target) } }.key)
        assertEquals(8, before.surveys.sumOf { it.samples.size })
    }
    @Test fun stableUpdatesUseNumericVersionsAndStartupOptOut() {
        assertTrue(StableVersion.parse("0.10.0") > StableVersion.parse("0.9.0")); assertThrows(SurveyFailure::class.java) { StableVersion.parse("0.3.0-beta") }
        var requests = 0; UpdateRules.dispatch(false, false) { requests++ }; assertEquals(0, requests); UpdateRules.dispatch(true, false) { requests++ }; assertEquals(1, requests)
        val json = """{"tag_name":"v0.3.0","draft":false,"prerelease":false,"assets":[{"name":"WifiMap-v0.3.0.apk","size":1024,"browser_download_url":"https://github.com/siarheisinelnikau/wifi-map/releases/download/v0.3.0/WifiMap-v0.3.0.apk","digest":"sha256:${"a".repeat(64)}"}]}"""
        assertEquals("0.3.0", UpdateRules.release(json, "0.2.0")!!.version.toString())
        assertThrows(SurveyFailure::class.java) { UpdateRules.release(json.replace("https://github.com/", "https://example.com/"), "0.2.0") }
    }
    @Test fun reportEscapesInputsAndSeparatesComparisonUnits() {
        val p = ProjectCodec.decode(fixture()).copy(name = "<script>&\"")
        val report = SvgReport.create(p, 2, 1, p.surveys[0].samples[0].observation.bssid, Metric.RssiDbm, null) { it }
        assertTrue(report.contains("&lt;script&gt;&amp;&quot;")); assertFalse(report.contains("<script>")); assertTrue(report.contains("dB")); assertTrue(report.contains("measured_absolute"))
    }
}

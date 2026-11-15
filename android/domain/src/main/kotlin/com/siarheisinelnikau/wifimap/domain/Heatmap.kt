package com.siarheisinelnikau.wifimap.domain

import kotlin.math.floor
import kotlin.math.roundToInt

data class Estimate(val value: Float, val nearestPx: Float, val contributors: Int)
data class Heatmap(val width: Int, val height: Int, val cells: List<Estimate?>, val metric: Metric, val difference: Boolean, val radiusPx: Float) {
    fun argb(): IntArray = cells.map { cell -> if (cell == null) 0 else {
        val color = Heatmaps.color(cell.value, metric, difference)
        val alpha = (150f * (1f - cell.nearestPx / radiusPx).coerceIn(0.25f, 1f)).toInt()
        (alpha shl 24) or color
    } }.toIntArray()
}
object Heatmaps {
    private data class Datum(val p: Point, val value: Float, val count: Int = 1)
    private fun data(survey: Survey, ap: List<Int>, metric: Metric): List<Datum> {
        val values = survey.samples.filter { it.observation.bssid == ap }.mapNotNull { s -> s.observation.value(metric)?.let { Datum(s.position, it) } }.sortedWith(compareBy({ it.p.x }, { it.p.y }))
        val grouped = ArrayList<Datum>()
        values.forEach { v ->
            val last = grouped.lastOrNull()
            if (last != null && last.p.distance(v.p) < 0.01f) grouped[grouped.lastIndex] = last.copy(value = (last.value * last.count + v.value) / (last.count + 1), count = last.count + 1) else grouped.add(v)
        }
        return grouped
    }
    private fun cross(o: Point, a: Point, b: Point): Float = (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x)
    private fun hull(data: List<Datum>): List<Point> {
        if (data.size < 3) return emptyList()
        val points = ArrayList<Point>()
        data.forEach { d -> while (points.size >= 2 && cross(points[points.size - 2], points.last(), d.p) <= 0) points.removeAt(points.lastIndex); points.add(d.p) }
        val lower = points.size
        data.asReversed().drop(1).forEach { d -> while (points.size > lower && cross(points[points.size - 2], points.last(), d.p) <= 0) points.removeAt(points.lastIndex); points.add(d.p) }
        points.removeAt(points.lastIndex)
        return points.takeIf { it.size >= 3 } ?: emptyList()
    }
    private fun estimate(data: List<Datum>, hull: List<Point>, at: Point, radius: Float): Estimate? {
        if (hull.size < 3 || hull.indices.any { cross(hull[it], hull[(it + 1) % hull.size], at) < -0.01f }) return null
        val distances = FloatArray(8) { Float.POSITIVE_INFINITY }; val values = FloatArray(8)
        for (datum in data) {
            val d = datum.p.distance(at)
            if (d <= radius && d < distances[7]) {
                var index = 0; while (index < 8 && distances[index] < d) index++
                for (i in 7 downTo index + 1) { distances[i] = distances[i - 1]; values[i] = values[i - 1] }
                distances[index] = d; values[index] = datum.value
            }
        }
        val count = distances.count { it.isFinite() }
        if (count < 3) return null
        if (distances[0] < 0.01f) return Estimate(values[0], 0f, count)
        var total = 0f; var weight = 0f
        for (i in 0 until count) { val w = 1f / (distances[i] * distances[i]).coerceAtLeast(1f); weight += w; total += w * values[i] }
        return Estimate(total / weight, distances[0], count)
    }
    fun compute(project: WifiProject, surveyId: Long, baselineId: Long?, ap: List<Int>, metric: Metric, radiusMeters: Float = 3f, cancelled: () -> Boolean = { false }): Heatmap? {
        val currentSurvey = project.surveys.find { it.id == surveyId } ?: return null
        val baselineSurvey = baselineId?.let { id -> project.surveys.find { it.id == id } ?: return null }
        val width = 160; val height = (160f * project.plan.size[1] / project.plan.size[0]).roundToInt().coerceIn(16, 160)
        val radius = project.plan.calibration?.metersPerPixel()?.let { radiusMeters / it } ?: 200f
        val current = data(currentSurvey, ap, metric); val currentHull = hull(current)
        val before = baselineSurvey?.let { data(it, ap, metric) }; val beforeHull = before?.let(::hull)
        val cells = ArrayList<Estimate?>(width * height)
        for (y in 0 until height) {
            if (cancelled()) return null
            for (x in 0 until width) {
                if (x % 16 == 0 && cancelled()) return null
                val point = Point((x + 0.5f) * project.plan.size[0] / width, (y + 0.5f) * project.plan.size[1] / height)
                val a = estimate(current, currentHull, point, radius)
                val cell = if (before != null && beforeHull != null) {
                    val b = estimate(before, beforeHull, point, radius)
                    if (a == null || b == null) null else Estimate(a.value - b.value, maxOf(a.nearestPx, b.nearestPx), minOf(a.contributors, b.contributors))
                } else a
                cells.add(cell)
            }
        }
        return Heatmap(width, height, cells, metric, baselineId != null, radius)
    }
    fun color(value: Float, metric: Metric, difference: Boolean): Int {
        val t = (if (difference) (value / 20f + 1f) * 0.5f else (value - metric.minimum) / (metric.maximum - metric.minimum)).coerceIn(0f, 1f)
        val stops = if (difference) arrayOf(intArrayOf(124, 83, 176), intArrayOf(225, 231, 232), intArrayOf(15, 151, 139)) else arrayOf(intArrayOf(102, 65, 156), intArrayOf(64, 119, 184), intArrayOf(22, 166, 154), intArrayOf(245, 199, 93))
        val at = t * (stops.size - 1); val index = floor(at).toInt().coerceAtMost(stops.size - 2); val fraction = at - index
        val rgb = IntArray(3) { c -> (stops[index][c] * (1f - fraction) + stops[index + 1][c] * fraction).roundToInt() }
        return (rgb[0] shl 16) or (rgb[1] shl 8) or rgb[2]
    }
}

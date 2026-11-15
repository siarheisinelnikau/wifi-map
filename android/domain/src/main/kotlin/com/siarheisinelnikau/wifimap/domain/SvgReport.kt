package com.siarheisinelnikau.wifimap.domain

import java.time.Instant

object SvgReport {
    fun escape(text: String): String = text.filter { it == '\n' || it == '\t' || it >= ' ' }.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;").replace("\"", "&quot;").replace("'", "&apos;")
    fun create(project: WifiProject, surveyId: Long, baselineId: Long?, ap: List<Int>, metric: Metric, map: Heatmap?, text: (String) -> String): String {
        val survey = project.surveys.first { it.id == surveyId }; val baseline = project.surveys.find { it.id == baselineId }
        val samples = survey.samples.filter { it.observation.bssid == ap }
        val width = maxOf(project.plan.size[0], 1280); val height = project.plan.size[1] + 420 + samples.size * 25
        val planTop = 110
        return buildString {
            append("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"$width\" height=\"$height\" viewBox=\"0 0 $width $height\"><rect width=\"100%\" height=\"100%\" fill=\"#f5f7fa\"/><g font-family=\"sans-serif\" fill=\"#19303e\">")
            append("<title>${escape(project.name)}</title><text x=\"24\" y=\"30\" font-size=\"22\">${escape(text("report_title"))} · ${escape(project.name.take(36))}</text>")
            append("<text x=\"24\" y=\"58\">${escape(survey.name.take(48))} · ${bssidLabel(ap)} · ${metric.unit}</text>")
            val line = listOfNotNull(if (project.demo) text("demo_badge") else null, baseline?.let { text("baseline") + ": " + it.name }).joinToString(" · ")
            append("<text x=\"24\" y=\"84\">${escape(line)}</text><g transform=\"translate(0,$planTop)\">")
            project.plan.image?.let { append("<image width=\"${project.plan.size[0]}\" height=\"${project.plan.size[1]}\" href=\"data:image/png;base64,${it.png}\"/>") }
            map?.let { m ->
                val cellWidth = project.plan.size[0].toFloat() / m.width; val cellHeight = project.plan.size[1].toFloat() / m.height
                m.cells.forEachIndexed { index, cell -> if (cell != null) {
                    val rgb = Heatmaps.color(cell.value, metric, m.difference); val opacity = (150f * (1 - cell.nearestPx / m.radiusPx).coerceIn(0.25f, 1f)) / 255f
                    append("<rect x=\"${index % m.width * cellWidth}\" y=\"${index / m.width * cellHeight}\" width=\"$cellWidth\" height=\"$cellHeight\" fill=\"#${rgb.toString(16).padStart(6, '0')}\" fill-opacity=\"$opacity\"/>")
                } }
            }
            project.plan.walls.forEach { append("<path d=\"M ${it.a.x} ${it.a.y} L ${it.b.x} ${it.b.y}\" stroke=\"#304552\" stroke-width=\"6\" stroke-linecap=\"round\"/>") }
            survey.router?.let { append("<rect x=\"${it.x - 10}\" y=\"${it.y - 10}\" width=\"20\" height=\"20\" rx=\"4\" fill=\"#0b736f\"/>") }
            samples.forEachIndexed { index, sample ->
                val color = sample.observation.value(metric)?.let { Heatmaps.color(it, metric, false) } ?: 0x7d8c91
                append("<circle cx=\"${sample.position.x}\" cy=\"${sample.position.y}\" r=\"10\" fill=\"#${color.toString(16).padStart(6, '0')}\" stroke=\"white\" stroke-width=\"3\"/><text x=\"${sample.position.x + 14}\" y=\"${sample.position.y + 4}\">${index + 1}</text>")
            }
            append("</g>")
            var y = project.plan.size[1] + planTop + 28
            val difference = baseline != null; val minimum = if (difference) -20f else metric.minimum; val maximum = if (difference) 20f else metric.maximum
            append("<text x=\"24\" y=\"$y\" font-size=\"18\">${escape(text(if (difference) "difference" else "legend"))} · $minimum … $maximum ${if (difference) metric.differenceUnit else metric.unit}</text>")
            y += 26
            for (i in 0..99) { val c = Heatmaps.color(minimum + (maximum - minimum) * i / 99, metric, difference); append("<rect x=\"${24 + i * 4}\" y=\"$y\" width=\"4\" height=\"12\" fill=\"#${c.toString(16).padStart(6, '0')}\"/>") }
            for (key in listOf("estimate_note", "confidence_note", if (difference) "compare_note" else "android_api_note")) { y += 30; append("<text x=\"24\" y=\"$y\" font-size=\"12\">${escape(text(key))}</text>") }
            y += 36
            append("<text x=\"24\" y=\"$y\" font-size=\"18\">${escape(text(if (difference) "measured_absolute" else "measured"))} · ${samples.size}</text>")
            samples.forEachIndexed { index, sample -> y += 25; val o = sample.observation; append("<text x=\"24\" y=\"$y\" font-size=\"12\">${index + 1} · ${sample.position.x},${sample.position.y} · ${o.value(metric)?.toString() ?: "—"} ${metric.unit} · ${Instant.ofEpochMilli(o.observedUnixMs)} · ${o.source} · ${bssidLabel(o.bssid)}</text>") }
            append("<text x=\"24\" y=\"${height - 16}\" font-size=\"12\">${escape(text("report_privacy"))}</text></g></svg>")
        }
    }
}

use crate::{
    error::{AppError, Result},
    heatmap::{self, Heatmap},
    i18n::Text,
    model::*,
};
use base64::{Engine, engine::general_purpose::STANDARD};
use std::io::Cursor;
pub fn xml(text: &str) -> String {
    text.chars()
        .filter(|c| *c == '\n' || *c == '\t' || *c >= ' ')
        .collect::<String>()
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
fn short_label(text: &str, limit: usize) -> String {
    let mut result: String = text.chars().take(limit).collect();
    if text.chars().count() > limit {
        result.push('…');
    }
    result
}
pub fn svg(
    project: &Project,
    survey: usize,
    baseline: Option<usize>,
    ap: Bssid,
    metric: Metric,
    map: Option<&Heatmap>,
    text: Text,
) -> Result<String> {
    let w = project.plan.size[0];
    let h = project.plan.size[1];
    let report_width = w.max(1280);
    let samples: Vec<_> = project.surveys[survey]
        .samples
        .iter()
        .filter(|s| s.observation.bssid == ap)
        .collect();
    let table_y = h + 250;
    let full_height = table_y + 80 + samples.len() as u32 * 25;
    let mut out = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{report_width}\" height=\"{full_height}\" viewBox=\"0 0 {report_width} {full_height}\"><rect width=\"100%\" height=\"100%\" fill=\"#f5f7fa\"/><g font-family=\"Segoe UI,Arial,sans-serif\" fill=\"#182c3c\"><text x=\"24\" y=\"32\" font-size=\"22\" font-weight=\"600\">{} · {}</text><text x=\"24\" y=\"60\" font-size=\"15\">{} · BSSID {ap} · {}</text>",
        xml(text.get("report_title")),
        xml(&short_label(&project.name, 32)),
        xml(&short_label(&project.surveys[survey].name, 48)),
        metric.unit()
    );
    let mut line = if project.demo {
        text.get("demo_badge").to_string()
    } else {
        String::new()
    };
    if let Some(b) = baseline {
        if !line.is_empty() {
            line.push_str(" · ");
        }
        line.push_str(&format!(
            "{}: {}",
            text.get("baseline"),
            short_label(&project.surveys[b].name, 48)
        ));
    }
    out.push_str(&format!(
        "<desc>{} · {} · {}: {}</desc><text x=\"24\" y=\"84\" font-size=\"13\">{}</text>",
        xml(&project.name),
        xml(&project.surveys[survey].name),
        xml(text.get("baseline")),
        xml(baseline
            .map(|b| project.surveys[b].name.as_str())
            .unwrap_or("—")),
        xml(&line)
    ));
    out.push_str("<g transform=\"translate(0,100)\">");
    if let Some(img) = &project.plan.image {
        out.push_str(&format!(
            "<image width=\"{w}\" height=\"{h}\" href=\"data:image/png;base64,{}\"/>",
            STANDARD.encode(&img.png)
        ));
    }
    if let Some(map) = map {
        let rgba = image::RgbaImage::from_raw(map.width as u32, map.height as u32, map.rgba())
            .ok_or_else(|| AppError::new("file_error"))?;
        let mut png = Cursor::new(Vec::new());
        rgba.write_to(&mut png, image::ImageFormat::Png)
            .map_err(|e| AppError::detail("file_error", e))?;
        out.push_str(&format!(
            "<image width=\"{w}\" height=\"{h}\" href=\"data:image/png;base64,{}\"/>",
            STANDARD.encode(png.into_inner())
        ));
    }
    for wall in &project.plan.walls {
        out.push_str(&format!("<path d=\"M {} {} L {} {}\" stroke=\"#304552\" stroke-width=\"6\" stroke-linecap=\"round\"/>",wall.a.x,wall.a.y,wall.b.x,wall.b.y));
    }
    for (i, sample) in samples.iter().enumerate() {
        let value = sample.observation.value(metric);
        let color = value
            .map(|v| heatmap::color(v, metric, false))
            .unwrap_or([130, 130, 130]);
        out.push_str(&format!("<circle cx=\"{}\" cy=\"{}\" r=\"12\" fill=\"rgb({},{},{})\" stroke=\"#ffffff\" stroke-width=\"3\"/><text x=\"{}\" y=\"{}\" font-size=\"13\" font-weight=\"600\">{}</text>",sample.position.x,sample.position.y,color[0],color[1],color[2],sample.position.x+16.0,sample.position.y+5.0,i+1));
    }
    if let Some(router) = project.surveys[survey].router {
        out.push_str(&format!("<rect x=\"{}\" y=\"{}\" width=\"20\" height=\"20\" rx=\"4\" fill=\"#173f50\"/><text x=\"{}\" y=\"{}\" font-size=\"14\">{}</text>",router.x-10.0,router.y-10.0,router.x+16.0,router.y+5.0,xml(text.get("router_tool"))));
    }
    out.push_str("</g>");
    let y = h + 130;
    let (min, max) = if baseline.is_some() {
        (-20.0, 20.0)
    } else {
        metric.range()
    };
    out.push_str(&format!(
        "<text x=\"24\" y=\"{y}\" font-size=\"16\">{} · {min:.0} … {max:.0} {}</text>",
        xml(text.get(if baseline.is_some() {
            "difference"
        } else {
            "legend"
        })),
        if baseline.is_some() {
            if metric == Metric::RssiDbm {
                "dB"
            } else {
                "pp"
            }
        } else {
            metric.unit()
        }
    ));
    for i in 0..100 {
        let c = heatmap::color(
            min + (max - min) * i as f32 / 99.0,
            metric,
            baseline.is_some(),
        );
        out.push_str(&format!(
            "<rect x=\"{}\" y=\"{}\" width=\"4\" height=\"12\" fill=\"rgb({},{},{})\"/>",
            24 + i * 4,
            y + 12,
            c[0],
            c[1],
            c[2]
        ));
    }
    for (dy, key) in [
        (50, "estimate_note"),
        (73, "confidence_note"),
        (
            96,
            if baseline.is_some() {
                "compare_note"
            } else {
                "api_note"
            },
        ),
    ] {
        out.push_str(&format!(
            "<text x=\"24\" y=\"{}\" font-size=\"12\">{}</text>",
            y + dy,
            xml(text.get(key))
        ));
    }
    out.push_str(&format!("<text x=\"24\" y=\"{table_y}\" font-size=\"18\">{} · {}</text><text x=\"24\" y=\"{}\" font-size=\"12\"># · x,y · {} · {} · BSSID · {}</text>",xml(text.get(if baseline.is_some(){"measured_absolute"}else{"measured"})),samples.len(),table_y+26,metric.unit(),xml(text.get("timestamp")),xml(text.get("channel"))));
    for (i, s) in samples.iter().enumerate() {
        let o = &s.observation;
        let value = o
            .value(metric)
            .map(|v| format!("{v:.0}"))
            .unwrap_or_else(|| text.get("unavailable").into());
        out.push_str(&format!("<text x=\"24\" y=\"{}\" font-size=\"12\">{} · {:.1},{:.1} · {} {} · {} · {} · {}</text>",table_y+52+i as u32*25,i+1,s.position.x,s.position.y,value,metric.unit(),utc_time(o.observed_unix_ms),o.bssid,o.channel.map(|c|c.to_string()).unwrap_or_else(||"—".into())));
    }
    out.push_str(&format!(
        "<text x=\"24\" y=\"{}\" font-size=\"11\">{}</text></g></svg>",
        full_height - 12,
        xml(text.get("report_privacy"))
    ));
    Ok(out)
}
/// Civil date conversion; stored timestamps always remain raw Unix milliseconds.
pub fn utc_time(ms: u64) -> String {
    let seconds = (ms / 1000) as i64;
    let days = seconds / 86400;
    let z = days + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let mut year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02}.{:03}Z",
        seconds % 86400 / 3600,
        seconds % 3600 / 60,
        seconds % 60,
        ms % 1000
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn report_escapes_untrusted_names_and_preserves_timestamps() {
        let mut p = demo_project();
        p.name = "<script>&\"".into();
        let ap = p.surveys[0].samples[0].observation.bssid;
        let out = svg(&p, 0, None, ap, Metric::RssiDbm, None, Text::new("en")).unwrap();
        assert!(out.contains("&lt;script&gt;&amp;&quot;"));
        assert!(!out.contains("<script>"));
        assert!(out.contains("SYNTHETIC DATA"));
        let comparison = svg(
            &p,
            1,
            Some(0),
            ap,
            Metric::LinkQualityPercent,
            None,
            Text::new("en"),
        )
        .unwrap();
        assert!(comparison.contains("Baseline: Before"));
        assert!(comparison.contains("Measured points · absolute values"));
        assert!(comparison.contains("20 pp"));
        assert_eq!(short_label(&"🏠".repeat(100), 32).chars().count(), 33);
        assert_eq!(utc_time(0), "1970-01-01 00:00:00.000Z");
        assert_eq!(utc_time(1_704_067_200_123), "2024-01-01 00:00:00.123Z");
    }
}

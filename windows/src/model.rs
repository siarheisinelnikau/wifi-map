use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

pub const MAX_SAMPLES: usize = 10_000;
pub const MAX_SURVEYS: usize = 64;
pub const CURRENT_SCHEMA: u32 = 2;
pub const MAX_SAFE_ID: u64 = 9_007_199_254_740_991;

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}
impl Point {
    pub fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
    pub fn distance(self, other: Self) -> f32 {
        (self.x - other.x).hypot(self.y - other.y)
    }
    pub fn inside(self, size: [u32; 2]) -> bool {
        self.x.is_finite()
            && self.y.is_finite()
            && self.x >= 0.0
            && self.y >= 0.0
            && self.x <= size[0] as f32
            && self.y <= size[1] as f32
    }
}

/// Reversible image-pixel <-> viewport transform. Zoom is anchored at the cursor.
#[derive(Clone, Copy, Debug)]
pub struct ViewTransform {
    pub origin: Point,
    pub zoom: f32,
}
impl ViewTransform {
    pub fn to_screen(self, p: Point) -> Point {
        Point::new(
            self.origin.x + p.x * self.zoom,
            self.origin.y + p.y * self.zoom,
        )
    }
    pub fn to_plan(self, p: Point) -> Point {
        Point::new(
            (p.x - self.origin.x) / self.zoom,
            (p.y - self.origin.y) / self.zoom,
        )
    }
    pub fn zoom_at(&mut self, screen: Point, factor: f32) {
        let anchor = self.to_plan(screen);
        self.zoom = (self.zoom * factor).clamp(0.02, 15.0);
        self.origin = Point::new(
            screen.x - anchor.x * self.zoom,
            screen.y - anchor.y * self.zoom,
        );
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Calibration {
    pub a: Point,
    pub b: Point,
    pub meters: f32,
}
impl Calibration {
    pub fn meters_per_pixel(&self) -> Option<f32> {
        let distance = self.a.distance(self.b);
        (distance >= 1.0 && self.meters.is_finite() && self.meters > 0.0 && self.meters <= 10_000.0)
            .then(|| self.meters / distance)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EmbeddedImage {
    pub name: String,
    #[serde(with = "image_bytes")]
    pub png: Vec<u8>,
}
mod image_bytes {
    use base64::{Engine, engine::general_purpose::STANDARD};
    use serde::{Deserialize, Deserializer, Serializer};
    pub fn serialize<S: Serializer>(v: &[u8], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&STANDARD.encode(v))
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let value = String::deserialize(d)?;
        STANDARD.decode(value).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Wall {
    pub a: Point,
    pub b: Point,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FloorPlan {
    pub size: [u32; 2],
    pub image: Option<EmbeddedImage>,
    pub walls: Vec<Wall>,
    pub calibration: Option<Calibration>,
}
impl Default for FloorPlan {
    fn default() -> Self {
        Self {
            size: [1000, 700],
            image: None,
            walls: Vec::new(),
            calibration: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Bssid(pub [u8; 6]);
impl std::fmt::Display for Bssid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let b = self.0;
        write!(
            f,
            "{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
            b[0], b[1], b[2], b[3], b[4], b[5]
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Metric {
    RssiDbm,
    LinkQualityPercent,
}
impl Metric {
    pub fn unit(self) -> &'static str {
        match self {
            Self::RssiDbm => "dBm",
            Self::LinkQualityPercent => "%",
        }
    }
    pub fn range(self) -> (f32, f32) {
        match self {
            Self::RssiDbm => (-90.0, -40.0),
            Self::LinkQualityPercent => (0.0, 100.0),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Observation {
    pub adapter_id: String,
    /// Raw SSID bytes, retained even when not valid UTF-8 or hidden.
    pub ssid: Vec<u8>,
    pub bssid: Bssid,
    pub frequency_khz: Option<u32>,
    pub channel: Option<u16>,
    /// Only driver-reported dBm, never derived from a percentage.
    pub rssi_dbm: Option<i32>,
    pub link_quality_percent: Option<u8>,
    pub collected_unix_ms: u64,
    pub observed_unix_ms: u64,
    pub connected_bssid: Option<Bssid>,
    pub source: ObservationSource,
    #[serde(default)]
    pub ssid_encoding: Option<SsidEncoding>,
    #[serde(default)]
    pub android_observed_elapsed_us: Option<u64>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ObservationSource {
    WindowsBss,
    AndroidScan,
    Demo,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SsidEncoding {
    RawBytes,
    Utf8Display,
}
impl Observation {
    pub fn value(&self, metric: Metric) -> Option<f32> {
        match metric {
            Metric::RssiDbm => self.rssi_dbm.map(|v| v as f32),
            Metric::LinkQualityPercent => self.link_quality_percent.map(|v| v as f32),
        }
    }
    pub fn ssid_label(&self) -> String {
        if self.ssid.is_empty() {
            format!("[{}]", self.bssid)
        } else {
            String::from_utf8_lossy(&self.ssid)
                .chars()
                .filter(|c| !c.is_control())
                .collect()
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Sample {
    pub id: u64,
    pub position: Point,
    pub observation: Observation,
    pub local_probe: Option<crate::peer::ProbeResult>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Survey {
    pub id: u64,
    pub name: String,
    pub router: Option<Point>,
    pub samples: Vec<Sample>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Project {
    pub schema_version: u32,
    pub name: String,
    pub plan: FloorPlan,
    pub surveys: Vec<Survey>,
    pub next_id: u64,
    pub demo: bool,
}
impl Default for Project {
    fn default() -> Self {
        Self {
            schema_version: CURRENT_SCHEMA,
            name: String::new(),
            plan: FloorPlan::default(),
            surveys: vec![Survey {
                id: 1,
                name: String::new(),
                router: None,
                samples: Vec::new(),
            }],
            next_id: 2,
            demo: false,
        }
    }
}
impl Project {
    pub fn take_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }
    pub fn add_survey(&mut self, name: String) -> Result<usize, &'static str> {
        if self.surveys.len() >= MAX_SURVEYS {
            return Err("survey_limit");
        }
        let id = self.take_id();
        self.surveys.push(Survey {
            id,
            name,
            router: None,
            samples: Vec::new(),
        });
        Ok(self.surveys.len() - 1)
    }
    pub fn add_sample(
        &mut self,
        survey: usize,
        position: Point,
        observation: Observation,
    ) -> Result<u64, &'static str> {
        if !position.inside(self.plan.size) || survey >= self.surveys.len() {
            return Err("invalid_project");
        }
        if self.surveys.iter().map(|s| s.samples.len()).sum::<usize>() >= MAX_SAMPLES {
            return Err("sample_limit");
        }
        if self.demo != (observation.source == ObservationSource::Demo) {
            return Err("demo_isolated");
        }
        let id = self.take_id();
        self.surveys[survey].samples.push(Sample {
            id,
            position,
            observation,
            local_probe: None,
        });
        Ok(id)
    }
    pub fn access_points(&self, survey: usize) -> Vec<(Bssid, String)> {
        let mut list = Vec::new();
        for s in &self.surveys[survey].samples {
            if !list.iter().any(|(b, _)| *b == s.observation.bssid) {
                list.push((s.observation.bssid, s.observation.ssid_label()));
            }
        }
        list
    }
}
pub fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
pub fn channel_from_khz(khz: u32) -> Option<u16> {
    let mhz = khz / 1000;
    if !khz.is_multiple_of(1000) {
        return None;
    }
    match mhz {
        2484 => Some(14),
        2412..=2472 if (mhz - 2407).is_multiple_of(5) => Some(((mhz - 2407) / 5) as u16),
        5000..=5895 if (mhz - 5000).is_multiple_of(5) => Some(((mhz - 5000) / 5) as u16),
        5935 => Some(2),
        5955..=7115 if (mhz - 5950).is_multiple_of(5) => Some(((mhz - 5950) / 5) as u16),
        _ => None,
    }
}

/// Synthetic, explicitly isolated fixture. It never calls a WLAN or network API.
pub fn demo_project() -> Project {
    let mut p = Project {
        name: "Apartment · example".into(),
        demo: true,
        ..Project::default()
    };
    p.plan.walls = vec![
        Wall {
            a: Point::new(40.0, 40.0),
            b: Point::new(960.0, 40.0),
        },
        Wall {
            a: Point::new(960.0, 40.0),
            b: Point::new(960.0, 660.0),
        },
        Wall {
            a: Point::new(960.0, 660.0),
            b: Point::new(40.0, 660.0),
        },
        Wall {
            a: Point::new(40.0, 660.0),
            b: Point::new(40.0, 40.0),
        },
        Wall {
            a: Point::new(410.0, 40.0),
            b: Point::new(410.0, 300.0),
        },
        Wall {
            a: Point::new(410.0, 390.0),
            b: Point::new(410.0, 660.0),
        },
        Wall {
            a: Point::new(410.0, 350.0),
            b: Point::new(710.0, 350.0),
        },
        Wall {
            a: Point::new(800.0, 350.0),
            b: Point::new(960.0, 350.0),
        },
    ];
    p.plan.calibration = Some(Calibration {
        a: Point::new(40.0, 40.0),
        b: Point::new(960.0, 40.0),
        meters: 11.5,
    });
    p.surveys[0].name = "Before".into();
    p.surveys[0].router = Some(Point::new(170.0, 150.0));
    p.add_survey("After".into()).unwrap();
    p.surveys[1].router = Some(Point::new(490.0, 330.0));
    let positions = [
        (150.0, 150.0),
        (300.0, 220.0),
        (160.0, 500.0),
        (340.0, 570.0),
        (520.0, 160.0),
        (800.0, 170.0),
        (550.0, 510.0),
        (850.0, 540.0),
    ];
    for survey in 0..2 {
        for (i, (x, y)) in positions.into_iter().enumerate() {
            let pos = Point::new(x, y);
            let distance = pos.distance(p.surveys[survey].router.unwrap());
            let rssi = (-38.0 - distance * 0.058 - if x > 410.0 && survey == 0 { 8.0 } else { 0.0 })
                as i32;
            p.add_sample(
                survey,
                pos,
                Observation {
                    adapter_id: "demo-adapter".into(),
                    ssid: b"Home example".to_vec(),
                    bssid: Bssid([2, 0, 0, 0, 0, 1]),
                    frequency_khz: Some(5_180_000),
                    channel: Some(36),
                    rssi_dbm: Some(rssi),
                    link_quality_percent: Some((100 - i * 7) as u8),
                    collected_unix_ms: 1_794_700_800_000 + i as u64 * 20_000,
                    observed_unix_ms: 1_794_700_799_900 + i as u64 * 20_000,
                    connected_bssid: Some(Bssid([2, 0, 0, 0, 0, 1])),
                    source: ObservationSource::Demo,
                    ssid_encoding: None,
                    android_observed_elapsed_us: None,
                },
            )
            .unwrap();
        }
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn transform_and_cursor_anchor() {
        let mut t = ViewTransform {
            origin: Point::new(43.0, -20.0),
            zoom: 0.75,
        };
        let p = Point::new(234.0, 123.0);
        assert!(t.to_plan(t.to_screen(p)).distance(p) < 0.001);
        let cursor = Point::new(400.0, 300.0);
        let anchor = t.to_plan(cursor);
        t.zoom_at(cursor, 1.7);
        assert!(t.to_screen(anchor).distance(cursor) < 0.001);
    }
    #[test]
    fn calibration_and_channels() {
        assert_eq!(
            Calibration {
                a: Point::default(),
                b: Point::new(200.0, 0.0),
                meters: 4.0
            }
            .meters_per_pixel(),
            Some(0.02)
        );
        assert!(
            Calibration {
                a: Point::default(),
                b: Point::default(),
                meters: 4.0
            }
            .meters_per_pixel()
            .is_none()
        );
        assert_eq!(channel_from_khz(2_484_000), Some(14));
        assert_eq!(channel_from_khz(5_180_000), Some(36));
        assert_eq!(channel_from_khz(5_955_000), Some(1));
        assert_eq!(channel_from_khz(123), None);
    }
    #[test]
    fn metric_identity_and_demo_isolation() {
        let p = demo_project();
        let mut o = p.surveys[0].samples[0].observation.clone();
        o.rssi_dbm = None;
        assert_eq!(o.value(Metric::RssiDbm), None);
        assert!(o.value(Metric::LinkQualityPercent).is_some());
        assert!(
            Project::default()
                .add_sample(0, Point::new(10.0, 10.0), o)
                .is_err()
        );
    }
}

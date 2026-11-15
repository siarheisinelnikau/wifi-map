use crate::model::{Bssid, Metric, Point, Project, Survey};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[derive(Clone, Default)]
pub struct Cancellation(pub Arc<AtomicBool>);
impl Cancellation {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Cell {
    pub value: f32,
    pub nearest_px: f32,
    pub contributors: u8,
}
#[derive(Clone, Debug)]
pub struct Heatmap {
    pub width: usize,
    pub height: usize,
    pub cells: Vec<Option<Cell>>,
    pub metric: Metric,
    pub difference: bool,
    pub radius_px: f32,
}
#[derive(Clone, Copy, Debug)]
struct Datum {
    p: Point,
    value: f32,
    count: u32,
}

fn data(survey: &Survey, ap: Bssid, metric: Metric) -> Vec<Datum> {
    let mut values: Vec<_> = survey
        .samples
        .iter()
        .filter(|s| s.observation.bssid == ap)
        .filter_map(|s| {
            s.observation.value(metric).map(|value| Datum {
                p: s.position,
                value,
                count: 1,
            })
        })
        .collect();
    values.sort_by(|a, b| a.p.x.total_cmp(&b.p.x).then(a.p.y.total_cmp(&b.p.y)));
    let mut grouped: Vec<Datum> = Vec::new();
    for v in values {
        if let Some(last) = grouped.last_mut().filter(|d| d.p.distance(v.p) < 0.01) {
            last.value = (last.value * last.count as f32 + v.value) / (last.count + 1) as f32;
            last.count += 1;
        } else {
            grouped.push(v);
        }
    }
    grouped
}
fn cross(o: Point, a: Point, b: Point) -> f32 {
    (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x)
}
fn hull(data: &[Datum]) -> Vec<Point> {
    if data.len() < 3 {
        return Vec::new();
    }
    let mut h = Vec::new();
    for p in data.iter().map(|d| d.p) {
        while h.len() >= 2 && cross(h[h.len() - 2], h[h.len() - 1], p) <= 0.0 {
            h.pop();
        }
        h.push(p);
    }
    let lower = h.len();
    for p in data.iter().rev().skip(1).map(|d| d.p) {
        while h.len() > lower && cross(h[h.len() - 2], h[h.len() - 1], p) <= 0.0 {
            h.pop();
        }
        h.push(p);
    }
    h.pop();
    if h.len() < 3 { Vec::new() } else { h }
}
fn inside(hull: &[Point], p: Point) -> bool {
    hull.len() >= 3
        && (0..hull.len()).all(|i| cross(hull[i], hull[(i + 1) % hull.len()], p) >= -0.01)
}
fn estimate(data: &[Datum], hull: &[Point], p: Point, radius: f32) -> Option<Cell> {
    if !inside(hull, p) {
        return None;
    }
    let mut nearest = [(f32::INFINITY, 0.0); 8];
    for d in data {
        let dist = d.p.distance(p);
        if dist <= radius && dist < nearest[7].0 {
            let index = nearest.partition_point(|n| n.0 < dist);
            for i in (index + 1..8).rev() {
                nearest[i] = nearest[i - 1];
            }
            nearest[index] = (dist, d.value);
        }
    }
    let count = nearest.iter().filter(|n| n.0.is_finite()).count();
    if count < 3 {
        return None;
    }
    if nearest[0].0 < 0.01 {
        return Some(Cell {
            value: nearest[0].1,
            nearest_px: 0.0,
            contributors: count as u8,
        });
    }
    let mut weight = 0.0;
    let mut total = 0.0;
    for (d, v) in nearest.iter().take(count) {
        let w = 1.0 / (d * d).max(1.0);
        weight += w;
        total += w * v;
    }
    Some(Cell {
        value: total / weight,
        nearest_px: nearest[0].0,
        contributors: count as u8,
    })
}

/// No wall/propagation model is implied: IDW is restricted to the sample convex
/// hull, a finite radius and >=3 nearby locations. Raw samples stay independent.
pub fn compute(
    project: &Project,
    survey: usize,
    baseline: Option<usize>,
    ap: Bssid,
    metric: Metric,
    radius_m: f32,
    cancel: &Cancellation,
) -> Option<Heatmap> {
    let size = project.plan.size;
    let width = 160;
    let height = ((width as f32 * size[1] as f32 / size[0] as f32).round() as usize).clamp(16, 160);
    let radius_px = project
        .plan
        .calibration
        .as_ref()
        .and_then(|c| c.meters_per_pixel())
        .map(|m| radius_m / m)
        .unwrap_or(200.0);
    let current = data(&project.surveys[survey], ap, metric);
    let current_hull = hull(&current);
    let before = baseline.map(|s| data(&project.surveys[s], ap, metric));
    let before_hull = before.as_ref().map(|d| hull(d));
    let mut cells = Vec::with_capacity(width * height);
    for y in 0..height {
        if cancel.is_cancelled() {
            return None;
        }
        for x in 0..width {
            if x % 16 == 0 && cancel.is_cancelled() {
                return None;
            }
            let p = Point::new(
                (x as f32 + 0.5) * size[0] as f32 / width as f32,
                (y as f32 + 0.5) * size[1] as f32 / height as f32,
            );
            let c = estimate(&current, &current_hull, p, radius_px);
            let result = match (&before, &before_hull) {
                (Some(b), Some(h)) => c.zip(estimate(b, h, p, radius_px)).map(|(a, b)| Cell {
                    value: a.value - b.value,
                    nearest_px: a.nearest_px.max(b.nearest_px),
                    contributors: a.contributors.min(b.contributors),
                }),
                _ => c,
            };
            cells.push(result);
        }
    }
    Some(Heatmap {
        width,
        height,
        cells,
        metric,
        difference: baseline.is_some(),
        radius_px,
    })
}

/// Accessible purple -> blue -> teal -> gold. Difference: purple loss, neutral,
/// teal gain. Alpha encodes distance support, not statistical confidence.
pub fn color(value: f32, metric: Metric, difference: bool) -> [u8; 3] {
    let (min, max) = metric.range();
    let t = if difference {
        (value / 20.0 + 1.0) * 0.5
    } else {
        (value - min) / (max - min)
    }
    .clamp(0.0, 1.0);
    let stops: &[[u8; 3]] = if difference {
        &[[124, 83, 176], [225, 231, 232], [15, 151, 139]]
    } else {
        &[
            [102, 65, 156],
            [64, 119, 184],
            [22, 166, 154],
            [245, 199, 93],
        ]
    };
    let at = t * (stops.len() - 1) as f32;
    let i = (at.floor() as usize).min(stops.len() - 2);
    let f = at - i as f32;
    std::array::from_fn(|c| {
        (stops[i][c] as f32 * (1.0 - f) + stops[i + 1][c] as f32 * f).round() as u8
    })
}
impl Heatmap {
    pub fn rgba(&self) -> Vec<u8> {
        self.cells
            .iter()
            .flat_map(|c| match c {
                None => [0, 0, 0, 0],
                Some(c) => {
                    let rgb = color(c.value, self.metric, self.difference);
                    [
                        rgb[0],
                        rgb[1],
                        rgb[2],
                        (150.0 * (1.0 - c.nearest_px / self.radius_px).clamp(0.25, 1.0)) as u8,
                    ]
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::demo_project;
    #[test]
    fn deterministic_no_data_and_cancellation() {
        let mut p = demo_project();
        let ap = p.surveys[0].samples[0].observation.bssid;
        let a = compute(
            &p,
            0,
            None,
            ap,
            Metric::RssiDbm,
            8.0,
            &Cancellation::default(),
        )
        .unwrap();
        assert!(a.cells.iter().any(Option::is_some));
        assert!(a.cells[0].is_none());
        let b = compute(
            &p,
            0,
            None,
            ap,
            Metric::RssiDbm,
            8.0,
            &Cancellation::default(),
        )
        .unwrap();
        assert_eq!(a.rgba(), b.rgba());
        let sparse = compute(
            &p,
            0,
            None,
            ap,
            Metric::RssiDbm,
            0.1,
            &Cancellation::default(),
        )
        .unwrap();
        assert!(sparse.cells.iter().all(Option::is_none));
        let c = Cancellation::default();
        c.cancel();
        assert!(compute(&p, 0, None, ap, Metric::RssiDbm, 8.0, &c).is_none());
        p.surveys[0].samples.clear();
        assert!(
            compute(
                &p,
                0,
                None,
                ap,
                Metric::RssiDbm,
                8.0,
                &Cancellation::default()
            )
            .unwrap()
            .cells
            .iter()
            .all(Option::is_none)
        );
    }
    #[test]
    fn compare_only_overlapping_same_ap_and_units() {
        let mut p = demo_project();
        let ap = p.surveys[0].samples[0].observation.bssid;
        let h = compute(
            &p,
            0,
            Some(0),
            ap,
            Metric::RssiDbm,
            8.0,
            &Cancellation::default(),
        )
        .unwrap();
        assert!(h.cells.iter().flatten().all(|c| c.value == 0.0));
        for s in &mut p.surveys[1].samples {
            s.observation.rssi_dbm = None;
        }
        let h = compute(
            &p,
            1,
            Some(0),
            ap,
            Metric::RssiDbm,
            8.0,
            &Cancellation::default(),
        )
        .unwrap();
        assert!(h.cells.iter().all(Option::is_none));
    }
    #[test]
    fn collinear_and_duplicates_do_not_invent_area() {
        let d = [
            Datum {
                p: Point::new(0.0, 0.0),
                value: -50.0,
                count: 1,
            },
            Datum {
                p: Point::new(10.0, 0.0),
                value: -60.0,
                count: 1,
            },
            Datum {
                p: Point::new(20.0, 0.0),
                value: -70.0,
                count: 1,
            },
        ];
        assert!(hull(&d).is_empty());
    }
}

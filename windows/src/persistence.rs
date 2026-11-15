use crate::{
    error::{AppError, Result},
    model::*,
};
use image::{ImageFormat, ImageReader};
use std::{
    collections::HashSet,
    fs::{self, OpenOptions},
    io::{Cursor, Read, Write},
    path::{Path, PathBuf},
};
pub const MAX_PROJECT_BYTES: u64 = 48 * 1024 * 1024;
pub const MAX_IMAGE_BYTES: u64 = 24 * 1024 * 1024;

fn io_error(e: impl ToString) -> AppError {
    AppError::detail("file_error", e)
}
pub fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>> {
    let file = fs::File::open(path).map_err(io_error)?;
    if file.metadata().map_err(io_error)?.len() > limit {
        return Err(AppError::new("file_too_large"));
    }
    let mut data = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut data)
        .map_err(io_error)?;
    if data.len() as u64 > limit {
        return Err(AppError::new("file_too_large"));
    }
    Ok(data)
}
pub fn decode_image(bytes: &[u8]) -> Result<image::RgbaImage> {
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(io_error)?;
    if !matches!(reader.format(), Some(ImageFormat::Png | ImageFormat::Jpeg)) {
        return Err(AppError::new("image_error"));
    }
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(96 * 1024 * 1024);
    reader.limits(limits);
    reader
        .decode()
        .map(|i| i.into_rgba8())
        .map_err(|e| AppError::detail("image_error", e))
}
pub fn import_plan(path: &Path) -> Result<FloorPlan> {
    let bytes = read_bounded(path, MAX_IMAGE_BYTES)?;
    let rgba = decode_image(&bytes)?;
    let size = [rgba.width(), rgba.height()];
    let mut png = Cursor::new(Vec::new());
    rgba.write_to(&mut png, ImageFormat::Png)
        .map_err(io_error)?;
    if png.get_ref().len() as u64 > MAX_IMAGE_BYTES {
        return Err(AppError::new("file_too_large"));
    }
    Ok(FloorPlan {
        size,
        image: Some(EmbeddedImage {
            name: path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into(),
            png: png.into_inner(),
        }),
        walls: Vec::new(),
        calibration: None,
    })
}
pub fn validate(p: &Project) -> Result<()> {
    let invalid = || AppError::new("invalid_project");
    if !(1..=CURRENT_SCHEMA).contains(&p.schema_version) {
        return Err(AppError::new("project_version"));
    }
    let size = p.plan.size;
    if size.iter().any(|s| *s == 0 || *s > 4096)
        || p.surveys.is_empty()
        || p.surveys.len() > MAX_SURVEYS
        || p.name.len() > 512
        || p.plan.walls.len() > 10_000
    {
        return Err(invalid());
    }
    if let Some(img) = &p.plan.image {
        if img.png.len() as u64 > MAX_IMAGE_BYTES || img.name.len() > 1024 {
            return Err(invalid());
        }
        let image = decode_image(&img.png)?;
        if [image.width(), image.height()] != size {
            return Err(invalid());
        }
    }
    if p.plan
        .walls
        .iter()
        .any(|w| !w.a.inside(size) || !w.b.inside(size))
    {
        return Err(invalid());
    }
    if let Some(c) = &p.plan.calibration
        && (!c.a.inside(size) || !c.b.inside(size) || c.meters_per_pixel().is_none())
    {
        return Err(invalid());
    }
    let mut ids = HashSet::new();
    let mut count = 0;
    for survey in &p.surveys {
        if survey.name.len() > 512
            || !ids.insert(survey.id)
            || survey.router.is_some_and(|r| !r.inside(size))
        {
            return Err(invalid());
        }
        for s in &survey.samples {
            count += 1;
            let o = &s.observation;
            if !ids.insert(s.id)
                || !s.position.inside(size)
                || o.ssid.len() > 32
                || o.adapter_id.len() > 256
                || o.rssi_dbm.is_some_and(|v| !(-127..=0).contains(&v))
                || o.link_quality_percent.is_some_and(|v| v > 100)
                || o.observed_unix_ms == 0
                || o.observed_unix_ms > o.collected_unix_ms.saturating_add(1000)
                || p.demo != (o.source == ObservationSource::Demo)
                || o.collected_unix_ms > MAX_SAFE_ID
                || (o.source == ObservationSource::AndroidScan
                    && (p.schema_version < 2
                        || o.link_quality_percent.is_some()
                        || o.ssid_encoding.is_none()
                        || o.android_observed_elapsed_us
                            .is_none_or(|v| v == 0 || v > MAX_SAFE_ID)))
            {
                return Err(invalid());
            }
            if let Some(probe) = &s.local_probe
                && !probe.valid()
            {
                return Err(invalid());
            }
        }
    }
    if count > MAX_SAMPLES
        || ids.iter().any(|id| *id == 0 || *id >= p.next_id)
        || p.next_id > MAX_SAFE_ID
    {
        return Err(invalid());
    }
    Ok(())
}
pub fn load(path: &Path) -> Result<Project> {
    let bytes = read_bounded(path, MAX_PROJECT_BYTES)?;
    let mut p: Project =
        serde_json::from_slice(&bytes).map_err(|e| AppError::detail("invalid_project", e))?;
    validate(&p)?;
    p.schema_version = CURRENT_SCHEMA;
    Ok(p)
}
pub fn save(path: &Path, project: &Project) -> Result<()> {
    validate(project)?;
    let bytes = serde_json::to_vec(project).map_err(io_error)?;
    if bytes.len() as u64 > MAX_PROJECT_BYTES {
        return Err(AppError::new("file_too_large"));
    }
    atomic_write(path, &bytes)
}
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut temp = None;
    for index in 0..16 {
        let candidate = parent.join(format!(
            ".wifimap-{}-{}-{index}.tmp",
            std::process::id(),
            unix_ms()
        ));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => {
                temp = Some((candidate, file));
                break;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(io_error(e)),
        }
    }
    let (temp_path, mut file) = temp.ok_or_else(|| AppError::new("file_error"))?;
    let result = (|| {
        file.write_all(bytes).map_err(io_error)?;
        file.sync_all().map_err(io_error)?;
        drop(file);
        replace(&temp_path, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    result
}
#[cfg(windows)]
pub fn replace(from: &Path, to: &Path) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };
    let from: Vec<_> = from.as_os_str().encode_wide().chain(Some(0)).collect();
    let to: Vec<_> = to.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: NUL-terminated paths remain alive for the duration of the call.
    if unsafe {
        MoveFileExW(
            from.as_ptr(),
            to.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    } == 0
    {
        Err(io_error(std::io::Error::last_os_error()))
    } else {
        Ok(())
    }
}
#[cfg(not(windows))]
pub fn replace(from: &Path, to: &Path) -> Result<()> {
    fs::rename(from, to).map_err(io_error)
}

pub fn preferences_path() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("WifiMap")
        .join("preferences.json")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn project_roundtrip_is_self_contained_and_atomic() {
        let dir =
            std::env::temp_dir().join(format!("wifimap-test-{}-{}", std::process::id(), unix_ms()));
        fs::create_dir(&dir).unwrap();
        let path = dir.join("survey.wifimap");
        let mut p = demo_project();
        let image = image::RgbaImage::from_pixel(1000, 700, image::Rgba([245, 245, 245, 255]));
        let mut png = Cursor::new(Vec::new());
        image.write_to(&mut png, ImageFormat::Png).unwrap();
        p.plan.image = Some(EmbeddedImage {
            name: "deleted-original.png".into(),
            png: png.into_inner(),
        });
        save(&path, &p).unwrap();
        let q = load(&path).unwrap();
        assert_eq!(q.surveys[1].samples[7].id, p.surveys[1].samples[7].id);
        assert_eq!(
            q.plan.image.unwrap().png,
            p.plan.image.as_ref().unwrap().png
        );
        let old = fs::read(&path).unwrap();
        p.next_id = 1;
        assert!(save(&path, &p).is_err());
        assert_eq!(fs::read(&path).unwrap(), old);
        p.next_id = 100;
        save(&path, &p).unwrap();
        assert_eq!(load(&path).unwrap().next_id, 100);
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn corrupt_projects_are_rejected() {
        let mut p = demo_project();
        p.surveys[0].samples[0].position.x = f32::NAN;
        assert!(validate(&p).is_err());
        let mut p = demo_project();
        p.surveys[1].samples[0].id = p.surveys[0].samples[0].id;
        assert!(validate(&p).is_err());
        let mut p = demo_project();
        p.surveys[0].samples[0].observation.rssi_dbm = Some(-300);
        assert!(validate(&p).is_err());
        assert!(decode_image(b"not an image").is_err());
    }
}

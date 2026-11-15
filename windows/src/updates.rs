//! Public HTTPS release checks and a user-initiated, verified portable updater.
use crate::{
    error::{AppError, Result},
    heatmap::Cancellation,
    persistence,
};
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};
pub const REPOSITORY: &str = "siarheisinelnikau/wifi-map";
pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
const MAX_EXE_BYTES: u64 = 100 * 1024 * 1024;
#[derive(Clone, Debug, Deserialize)]
pub struct Asset {
    pub name: String,
    pub size: u64,
    pub browser_download_url: String,
    pub digest: Option<String>,
}
#[derive(Debug, Deserialize)]
pub struct GithubRelease {
    pub tag_name: String,
    pub draft: bool,
    pub prerelease: bool,
    pub assets: Vec<Asset>,
}
#[derive(Clone, Debug)]
pub struct AvailableUpdate {
    pub version: String,
    pub asset: Asset,
    pub checksum: Option<[u8; 32]>,
    pub checksum_url: Option<String>,
}
#[derive(Clone, Debug)]
pub enum CheckResult {
    Current,
    Available(AvailableUpdate),
}
fn error(e: impl ToString) -> AppError {
    AppError::detail("update_error", e)
}
fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(60)))
        .timeout_connect(Some(Duration::from_secs(8)))
        .https_only(true)
        .user_agent(concat!("WifiMap/", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}
fn digest_hex(text: &str) -> Result<[u8; 32]> {
    if text.len() != 64 || !text.is_ascii() {
        return Err(AppError::new("update_checksum"));
    }
    let mut bytes = [0; 32];
    for (i, b) in bytes.iter_mut().enumerate() {
        *b = u8::from_str_radix(&text[i * 2..i * 2 + 2], 16)
            .map_err(|_| AppError::new("update_checksum"))?;
    }
    Ok(bytes)
}
pub fn select_release(release: GithubRelease, installed: &str) -> Result<CheckResult> {
    let version = Version::parse(release.tag_name.trim_start_matches('v')).map_err(error)?;
    if release.draft || release.prerelease || !version.pre.is_empty() {
        return Err(AppError::new("update_channel"));
    }
    let installed = Version::parse(installed).map_err(error)?;
    if version <= installed {
        return Ok(CheckResult::Current);
    }
    let expected = format!("WifiMap-v{version}.exe");
    let prefix = format!(
        "https://github.com/{REPOSITORY}/releases/download/{}/",
        release.tag_name
    );
    let asset = release
        .assets
        .iter()
        .find(|a| a.name == expected)
        .cloned()
        .ok_or_else(|| AppError::new("update_asset"))?;
    if asset.browser_download_url != format!("{prefix}{expected}")
        || !(512..=MAX_EXE_BYTES).contains(&asset.size)
    {
        return Err(AppError::new("update_asset"));
    }
    let checksum = asset
        .digest
        .as_ref()
        .map(|d| {
            d.strip_prefix("sha256:")
                .ok_or_else(|| AppError::new("update_checksum"))
                .and_then(digest_hex)
        })
        .transpose()?;
    let checksum_name = format!("{expected}.sha256");
    let checksum_url = release
        .assets
        .iter()
        .find(|a| {
            a.name == checksum_name
                && a.browser_download_url == format!("{prefix}{checksum_name}")
                && a.size <= 1024
        })
        .map(|a| a.browser_download_url.clone());
    if checksum.is_none() && checksum_url.is_none() {
        return Err(AppError::new("update_checksum"));
    }
    Ok(CheckResult::Available(AvailableUpdate {
        version: version.to_string(),
        asset,
        checksum,
        checksum_url,
    }))
}
pub fn dispatch_check(manual: bool, startup_enabled: bool, start: impl FnOnce()) {
    if manual || startup_enabled {
        start();
    }
}
pub fn check() -> Result<CheckResult> {
    let url = format!("https://api.github.com/repos/{REPOSITORY}/releases/latest");
    let mut response = agent()
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(error)?;
    let mut bytes = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take(2 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(error)?;
    if bytes.len() > 2 * 1024 * 1024 {
        return Err(AppError::new("update_error"));
    }
    let release = serde_json::from_slice(&bytes).map_err(error)?;
    select_release(release, CURRENT_VERSION)
}
pub fn hash_file(path: &Path) -> Result<[u8; 32]> {
    let mut file = fs::File::open(path).map_err(error)?;
    let mut buffer = [0; 65536];
    let mut hash = Sha256::new();
    loop {
        let n = file.read(&mut buffer).map_err(error)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    Ok(hash.finalize().into())
}
pub fn verify_executable(path: &Path, expected: [u8; 32]) -> Result<()> {
    if hash_file(path)? != expected {
        return Err(AppError::new("update_checksum"));
    }
    let bytes = persistence::read_bounded(path, MAX_EXE_BYTES)?;
    // Require a PE x64 executable matching the sole supported release architecture.
    if bytes.len() < 512 || &bytes[..2] != b"MZ" {
        return Err(AppError::new("update_asset"));
    }
    let offset = u32::from_le_bytes(bytes[0x3c..0x40].try_into().unwrap()) as usize;
    let header = bytes
        .get(offset..offset.saturating_add(26))
        .ok_or_else(|| AppError::new("update_asset"))?;
    if &header[..4] != b"PE\0\0" || header[4..6] != [0x64, 0x86] || header[24..26] != [0x0b, 0x02] {
        return Err(AppError::new("update_asset"));
    }
    Ok(())
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InstallJob {
    pub target: PathBuf,
    pub staged: PathBuf,
    pub backup: PathBuf,
    pub expected_sha256: [u8; 32],
    pub original_sha256: [u8; 32],
    pub parent_pid: u32,
    pub reopen_project: Option<PathBuf>,
}
#[derive(Clone, Debug)]
pub struct PreparedInstall {
    pub helper: PathBuf,
    pub job_path: PathBuf,
}
#[derive(Clone, Debug)]
pub struct VerifiedDownload {
    pub path: PathBuf,
    pub sha256: [u8; 32],
}
pub fn download(
    update: &AvailableUpdate,
    cancel: &Cancellation,
    mut progress: impl FnMut(u8),
) -> Result<VerifiedDownload> {
    let current = std::env::current_exe()
        .map_err(error)?
        .canonicalize()
        .map_err(error)?;
    let parent = current
        .parent()
        .ok_or_else(|| AppError::new("update_install"))?;
    let staged = parent.join(format!(
        ".WifiMap-download-{}-{}.exe",
        std::process::id(),
        crate::model::unix_ms()
    ));
    let result = (|| {
        let checksum = if let Some(hash) = update.checksum {
            hash
        } else {
            let mut response = agent()
                .get(
                    update
                        .checksum_url
                        .as_ref()
                        .ok_or_else(|| AppError::new("update_checksum"))?,
                )
                .call()
                .map_err(error)?;
            let mut text = String::new();
            response
                .body_mut()
                .as_reader()
                .take(1025)
                .read_to_string(&mut text)
                .map_err(error)?;
            if text.len() > 1024 {
                return Err(AppError::new("update_checksum"));
            }
            let mut parts = text.split_whitespace();
            let hash = digest_hex(parts.next().unwrap_or_default())?;
            if parts
                .next()
                .is_some_and(|name| name.trim_start_matches('*') != update.asset.name)
            {
                return Err(AppError::new("update_checksum"));
            }
            hash
        };
        let mut response = agent()
            .get(&update.asset.browser_download_url)
            .call()
            .map_err(error)?;
        let mut reader = response.body_mut().as_reader();
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&staged)
            .map_err(error)?;
        let mut count = 0u64;
        let mut buffer = [0; 65536];
        let mut last = 0;
        loop {
            if cancel.is_cancelled() {
                return Err(AppError::new("cancelled"));
            }
            let n = reader.read(&mut buffer).map_err(error)?;
            if n == 0 {
                break;
            }
            count += n as u64;
            if count > update.asset.size {
                return Err(AppError::new("update_incomplete"));
            }
            file.write_all(&buffer[..n]).map_err(error)?;
            let percent = ((count * 95) / update.asset.size) as u8;
            if percent != last {
                progress(percent);
                last = percent;
            }
        }
        file.sync_all().map_err(error)?;
        drop(file);
        if count != update.asset.size {
            return Err(AppError::new("update_incomplete"));
        }
        verify_executable(&staged, checksum)?;
        progress(100);
        Ok(VerifiedDownload {
            path: staged.clone(),
            sha256: checksum,
        })
    })();
    if result.is_err() {
        let _ = fs::remove_file(&staged);
    }
    result
}
pub fn prepare_install(
    download: &VerifiedDownload,
    reopen_project: Option<PathBuf>,
) -> Result<PreparedInstall> {
    let staged = download.path.canonicalize().map_err(error)?;
    let target = std::env::current_exe()
        .map_err(error)?
        .canonicalize()
        .map_err(error)?;
    let parent = target
        .parent()
        .ok_or_else(|| AppError::new("update_install"))?;
    if staged.parent() != Some(parent) {
        return Err(AppError::new("update_install"));
    }
    let nonce = format!("{}-{}", std::process::id(), crate::model::unix_ms());
    let helper = parent.join(format!(".WifiMap-helper-{nonce}.exe"));
    let job_path = parent.join(format!(".WifiMap-job-{nonce}.json"));
    let job = InstallJob {
        target: target.clone(),
        staged: staged.clone(),
        backup: parent.join(format!(".WifiMap-rollback-{nonce}.exe")),
        expected_sha256: download.sha256,
        original_sha256: hash_file(&target)?,
        parent_pid: std::process::id(),
        reopen_project,
    };
    verify_executable(&staged, job.expected_sha256)?;
    // create_new prevents overwriting any pre-existing helper/job/user file.
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&helper)
        .map_err(error)?;
    let mut input = fs::File::open(&target).map_err(error)?;
    std::io::copy(&mut input, &mut output).map_err(error)?;
    output.sync_all().map_err(error)?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&job_path)
        .map_err(error)?;
    output
        .write_all(&serde_json::to_vec(&job).map_err(error)?)
        .map_err(error)?;
    output.sync_all().map_err(error)?;
    Ok(PreparedInstall { helper, job_path })
}
pub fn start_install(prepared: &PreparedInstall) -> Result<()> {
    std::process::Command::new(&prepared.helper)
        .arg("--apply-update")
        .arg(&prepared.job_path)
        .spawn()
        .map_err(error)?;
    Ok(())
}
#[cfg(windows)]
fn wait_parent(pid: u32, target: &Path) -> Result<()> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::{
        Foundation::{CloseHandle, ERROR_INVALID_PARAMETER, GetLastError, WAIT_OBJECT_0},
        System::Threading::{
            OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
            QueryFullProcessImageNameW, WaitForSingleObject,
        },
    };
    let handle = unsafe {
        OpenProcess(
            PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION,
            0,
            pid,
        )
    };
    if handle.is_null() {
        // The parent may exit before this helper opens it.
        if unsafe { GetLastError() } == ERROR_INVALID_PARAMETER {
            return Ok(());
        }
        return Err(AppError::new("update_install"));
    }
    let mut name = vec![0u16; 32768];
    let mut length = name.len() as u32;
    let queried = unsafe { QueryFullProcessImageNameW(handle, 0, name.as_mut_ptr(), &mut length) };
    let actual = PathBuf::from(std::ffi::OsString::from_wide(&name[..length as usize]));
    if queried == 0 || actual.canonicalize().ok().as_deref() != Some(target) {
        unsafe {
            CloseHandle(handle);
        }
        return Err(AppError::new("update_install"));
    }
    let result = unsafe { WaitForSingleObject(handle, 30_000) };
    unsafe {
        CloseHandle(handle);
    }
    if result != WAIT_OBJECT_0 {
        return Err(AppError::new("update_install"));
    }
    Ok(())
}
#[cfg(not(windows))]
fn wait_parent(_: u32, _: &Path) -> Result<()> {
    Err(AppError::new("windows_only"))
}
pub fn apply_install(job_path: &Path) -> Result<()> {
    let bytes = persistence::read_bounded(job_path, 32 * 1024)?;
    let job: InstallJob = serde_json::from_slice(&bytes).map_err(error)?;
    let helper = std::env::current_exe()
        .map_err(error)?
        .canonicalize()
        .map_err(error)?;
    let parent = helper
        .parent()
        .ok_or_else(|| AppError::new("update_install"))?;
    for path in [&job.target, &job.staged, &job.backup, job_path] {
        if path.parent() != Some(parent)
            || path.file_name().is_none()
            || path
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return Err(AppError::new("update_install"));
        }
    }
    if job.target == helper
        || job.target == job.staged
        || job.target == job.backup
        || job.staged == job.backup
        || job.parent_pid == std::process::id()
    {
        return Err(AppError::new("update_install"));
    }
    wait_parent(job.parent_pid, &job.target)?;
    let result = (|| {
        verify_executable(&job.staged, job.expected_sha256)?;
        if hash_file(&job.target)? != job.original_sha256 {
            return Err(AppError::new("update_install"));
        }
        // Recovery copy is retained. No settings/projects are moved or deleted.
        let mut backup = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&job.backup)
            .map_err(error)?;
        std::io::copy(
            &mut fs::File::open(&job.target).map_err(error)?,
            &mut backup,
        )
        .map_err(error)?;
        backup.sync_all().map_err(error)?;
        drop(backup);
        persistence::replace(&job.staged, &job.target)?;
        let mut relaunch = std::process::Command::new(&job.target);
        if let Some(project) = &job.reopen_project {
            relaunch.arg("--open").arg(project);
        }
        if let Err(e) = relaunch.spawn() {
            persistence::replace(&job.backup, &job.target)?;
            return Err(error(e));
        }
        Ok(())
    })();
    if result.is_err() && hash_file(&job.target).ok() == Some(job.original_sha256) {
        let mut recovery = std::process::Command::new(&job.target);
        recovery.arg("--update-failed").arg(job_path);
        if let Some(project) = &job.reopen_project {
            recovery.arg("--open").arg(project);
        }
        let _ = recovery.spawn();
    }
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    fn release(version: &str) -> GithubRelease {
        let name = format!("WifiMap-v{version}.exe");
        GithubRelease {
            tag_name: format!("v{version}"),
            draft: false,
            prerelease: false,
            assets: vec![Asset {
                name: name.clone(),
                size: 1024,
                browser_download_url: format!(
                    "https://github.com/{REPOSITORY}/releases/download/v{version}/{name}"
                ),
                digest: Some(format!("sha256:{}", "a".repeat(64))),
            }],
        }
    }
    #[test]
    fn numeric_versions_channel_asset_and_digest() {
        assert!(matches!(
            select_release(release("0.10.0"), "0.9.0").unwrap(),
            CheckResult::Available(_)
        ));
        assert!(matches!(
            select_release(release("0.1.0"), "0.1.0").unwrap(),
            CheckResult::Current
        ));
        let mut r = release("0.2.0");
        r.assets[0].browser_download_url = "https://example.com/file.exe".into();
        assert!(select_release(r, "0.1.0").is_err());
        let mut r = release("0.2.0");
        r.prerelease = true;
        assert!(select_release(r, "0.1.0").is_err());
        let mut r = release("0.2.0");
        r.assets[0].digest = None;
        assert_eq!(
            select_release(r, "0.1.0").unwrap_err().key,
            "update_checksum"
        );
    }
    #[test]
    fn incomplete_or_corrupt_executable_is_rejected() {
        let path =
            std::env::temp_dir().join(format!("wifimap-update-test-{}", crate::model::unix_ms()));
        fs::write(&path, b"not an exe").unwrap();
        assert!(verify_executable(&path, [0; 32]).is_err());
        fs::remove_file(path).unwrap();
    }
    #[test]
    fn startup_opt_out_makes_no_request_and_keeps_manual_check_available() {
        let requests = std::cell::Cell::new(0);
        dispatch_check(false, false, || requests.set(requests.get() + 1));
        assert_eq!(requests.get(), 0);
        dispatch_check(true, false, || requests.set(requests.get() + 1));
        assert_eq!(requests.get(), 1);
        dispatch_check(false, true, || requests.set(requests.get() + 1));
        assert_eq!(requests.get(), 2);
    }
    #[test]
    fn verified_pe_rejects_wrong_architecture_and_changed_payload() {
        let path =
            std::env::temp_dir().join(format!("wifimap-pe-test-{}", crate::model::unix_ms()));
        let mut bytes = vec![0u8; 512];
        bytes[..2].copy_from_slice(b"MZ");
        bytes[0x3c..0x40].copy_from_slice(&128u32.to_le_bytes());
        bytes[128..132].copy_from_slice(b"PE\0\0");
        bytes[132..134].copy_from_slice(&[0x64, 0x86]);
        bytes[152..154].copy_from_slice(&[0x0b, 0x02]);
        fs::write(&path, &bytes).unwrap();
        let hash = hash_file(&path).unwrap();
        verify_executable(&path, hash).unwrap();
        bytes[300] = 1;
        fs::write(&path, &bytes).unwrap();
        assert_eq!(
            verify_executable(&path, hash).unwrap_err().key,
            "update_checksum"
        );
        bytes[132..134].copy_from_slice(&[0x4c, 0x01]);
        fs::write(&path, bytes).unwrap();
        assert_eq!(
            verify_executable(&path, hash_file(&path).unwrap())
                .unwrap_err()
                .key,
            "update_asset"
        );
        fs::remove_file(path).unwrap();
    }
}

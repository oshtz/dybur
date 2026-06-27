//! Portable updater support for dybur.

use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::env;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

pub const UPDATE_MANIFEST_URL: &str =
    "https://github.com/oshtz/dybur/releases/latest/download/dybur-update.json";
pub const UPDATE_MANIFEST_URL_ENV: &str = "DYBUR_UPDATE_MANIFEST_URL";
pub const UPDATE_HELPER_FLAG: &str = "--dybur-update-helper";

#[derive(Debug, Clone, Deserialize)]
pub struct UpdateManifest {
    pub version: String,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub pub_date: Option<String>,
    pub platforms: HashMap<String, UpdateAsset>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct UpdateAsset {
    pub url: String,
    pub sha256: String,
    #[serde(default)]
    pub size: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectedUpdate {
    pub version: String,
    pub platform_key: String,
    pub asset: UpdateAsset,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallPlatform {
    WindowsPortable,
    MacDmg,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelperInstallArgs {
    pub platform: InstallPlatform,
    pub pid: u32,
    pub artifact_path: PathBuf,
    pub target_exe_path: PathBuf,
    pub bundle_path: PathBuf,
    pub relaunch_path: PathBuf,
    pub log_path: PathBuf,
}

pub fn platform_key_for(target_os: &str, arch: &str) -> Option<String> {
    let arch = match arch {
        "x86_64" | "x64" => "x64",
        "aarch64" | "arm64" => "arm64",
        _ => return None,
    };

    match target_os {
        "windows" | "win32" => Some(format!("windows-{}", arch)),
        "macos" | "darwin" => Some(format!("darwin-{}", arch)),
        _ => None,
    }
}

pub fn current_platform_key() -> Option<String> {
    platform_key_for(std::env::consts::OS, std::env::consts::ARCH)
}

pub fn update_manifest_url() -> String {
    env::var(UPDATE_MANIFEST_URL_ENV)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| UPDATE_MANIFEST_URL.to_string())
}

pub fn install_platform_for_key(platform_key: &str) -> Option<InstallPlatform> {
    if platform_key.starts_with("windows-") {
        Some(InstallPlatform::WindowsPortable)
    } else if platform_key.starts_with("darwin-") {
        Some(InstallPlatform::MacDmg)
    } else {
        None
    }
}

fn macos_bundle_path_from_exe(current_exe: &Path) -> Option<PathBuf> {
    current_exe
        .ancestors()
        .find(|path| path.extension().map(|ext| ext == "app").unwrap_or(false))
        .map(Path::to_path_buf)
}

pub fn helper_install_args_for(
    platform: InstallPlatform,
    pid: u32,
    artifact_path: PathBuf,
    current_exe: PathBuf,
    log_path: PathBuf,
) -> Result<HelperInstallArgs, String> {
    let bundle_path = match platform {
        InstallPlatform::WindowsPortable => current_exe.clone(),
        InstallPlatform::MacDmg => macos_bundle_path_from_exe(&current_exe).ok_or_else(|| {
            format!(
                "Cannot find .app bundle ancestor for {}",
                current_exe.display()
            )
        })?,
    };

    Ok(HelperInstallArgs {
        platform,
        pid,
        artifact_path,
        target_exe_path: current_exe.clone(),
        relaunch_path: match platform {
            InstallPlatform::WindowsPortable => current_exe,
            InstallPlatform::MacDmg => bundle_path.clone(),
        },
        bundle_path,
        log_path,
    })
}

impl InstallPlatform {
    fn as_arg(&self) -> &'static str {
        match self {
            Self::WindowsPortable => "windows-portable",
            Self::MacDmg => "macos-dmg",
        }
    }

    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "windows-portable" => Ok(Self::WindowsPortable),
            "macos-dmg" => Ok(Self::MacDmg),
            other => Err(format!("Unsupported update platform: {}", other)),
        }
    }
}

fn parse_semver(version: &str) -> Option<(u64, u64, u64)> {
    let base = version
        .trim()
        .trim_start_matches('v')
        .split_once('-')
        .map(|(base, _)| base)
        .unwrap_or_else(|| version.trim().trim_start_matches('v'));
    let mut parts = base.split('.');

    let major = parts.next()?.parse::<u64>().ok()?;
    let minor = parts.next()?.parse::<u64>().ok()?;
    let patch = parts.next()?.parse::<u64>().ok()?;
    if parts.next().is_some() {
        return None;
    }

    Some((major, minor, patch))
}

pub fn is_newer_version(candidate: &str, current: &str) -> bool {
    match (parse_semver(candidate), parse_semver(current)) {
        (Some(candidate), Some(current)) => candidate > current,
        _ => false,
    }
}

pub fn select_platform_asset(
    manifest: &UpdateManifest,
    platform_key: &str,
    current_version: &str,
) -> Result<Option<SelectedUpdate>, String> {
    if !is_newer_version(&manifest.version, current_version) {
        return Ok(None);
    }

    let Some(asset) = manifest.platforms.get(platform_key) else {
        return Ok(None);
    };

    if asset.url.trim().is_empty() {
        return Err(format!(
            "Update manifest asset {} is missing url",
            platform_key
        ));
    }
    if asset.sha256.len() != 64 || !asset.sha256.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Err(format!(
            "Update manifest asset {} has invalid SHA-256",
            platform_key
        ));
    }

    Ok(Some(SelectedUpdate {
        version: manifest.version.clone(),
        platform_key: platform_key.to_string(),
        asset: asset.clone(),
    }))
}

pub fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file =
        File::open(path).map_err(|e| format!("Failed to open {}: {}", path.display(), e))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];

    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }

    Ok(format!("{:x}", hasher.finalize()))
}

pub fn verify_file_sha256(path: &Path, expected_sha256: &str) -> Result<(), String> {
    let actual = sha256_file(path)?;
    if actual.eq_ignore_ascii_case(expected_sha256) {
        Ok(())
    } else {
        Err(format!(
            "SHA-256 mismatch for {}: expected {}, got {}",
            path.display(),
            expected_sha256,
            actual
        ))
    }
}

pub fn build_helper_args(args: &HelperInstallArgs) -> Vec<String> {
    vec![
        UPDATE_HELPER_FLAG.to_string(),
        "--platform".to_string(),
        args.platform.as_arg().to_string(),
        "--pid".to_string(),
        args.pid.to_string(),
        "--artifact".to_string(),
        args.artifact_path.to_string_lossy().to_string(),
        "--target-exe".to_string(),
        args.target_exe_path.to_string_lossy().to_string(),
        "--bundle".to_string(),
        args.bundle_path.to_string_lossy().to_string(),
        "--relaunch".to_string(),
        args.relaunch_path.to_string_lossy().to_string(),
        "--log".to_string(),
        args.log_path.to_string_lossy().to_string(),
    ]
}

pub fn parse_helper_args(args: &[String]) -> Result<Option<HelperInstallArgs>, String> {
    if args.first().map(String::as_str) != Some(UPDATE_HELPER_FLAG) {
        return Ok(None);
    }

    let mut platform = None;
    let mut pid = None;
    let mut artifact_path = None;
    let mut target_exe_path = None;
    let mut bundle_path = None;
    let mut relaunch_path = None;
    let mut log_path = None;

    let mut index = 1;
    while index < args.len() {
        let key = args[index].as_str();
        let value = args
            .get(index + 1)
            .ok_or_else(|| format!("Missing value for update helper argument {}", key))?;

        match key {
            "--platform" => platform = Some(InstallPlatform::parse(value)?),
            "--pid" => {
                pid = Some(
                    value
                        .parse::<u32>()
                        .map_err(|e| format!("Invalid update helper pid {}: {}", value, e))?,
                )
            }
            "--artifact" => artifact_path = Some(PathBuf::from(value)),
            "--target-exe" => target_exe_path = Some(PathBuf::from(value)),
            "--bundle" => bundle_path = Some(PathBuf::from(value)),
            "--relaunch" => relaunch_path = Some(PathBuf::from(value)),
            "--log" => log_path = Some(PathBuf::from(value)),
            other => return Err(format!("Unknown update helper argument: {}", other)),
        }

        index += 2;
    }

    Ok(Some(HelperInstallArgs {
        platform: platform.ok_or_else(|| "Missing update helper platform".to_string())?,
        pid: pid.ok_or_else(|| "Missing update helper pid".to_string())?,
        artifact_path: artifact_path
            .ok_or_else(|| "Missing update helper artifact path".to_string())?,
        target_exe_path: target_exe_path
            .ok_or_else(|| "Missing update helper target executable path".to_string())?,
        bundle_path: bundle_path.ok_or_else(|| "Missing update helper bundle path".to_string())?,
        relaunch_path: relaunch_path
            .ok_or_else(|| "Missing update helper relaunch path".to_string())?,
        log_path: log_path.ok_or_else(|| "Missing update helper log path".to_string())?,
    }))
}

pub fn backup_path_for(target_path: &Path) -> PathBuf {
    PathBuf::from(format!("{}.bak", target_path.to_string_lossy()))
}

fn retry_file_op<T>(mut operation: impl FnMut() -> std::io::Result<T>) -> std::io::Result<T> {
    #[cfg(target_os = "windows")]
    {
        let mut last_error = None;
        for _ in 0..20 {
            match operation() {
                Ok(value) => return Ok(value),
                Err(error) => {
                    last_error = Some(error);
                    std::thread::sleep(Duration::from_millis(200));
                }
            }
        }
        Err(last_error.unwrap_or_else(|| std::io::Error::last_os_error()))
    }

    #[cfg(not(target_os = "windows"))]
    {
        operation()
    }
}

pub fn replace_file_with_backup(artifact_path: &Path, target_path: &Path) -> Result<(), String> {
    if !artifact_path.is_file() {
        return Err(format!(
            "Update artifact does not exist: {}",
            artifact_path.display()
        ));
    }

    if let Some(parent) = target_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create {}: {}", parent.display(), e))?;
    }

    let backup_path = backup_path_for(target_path);
    if backup_path.exists() {
        retry_file_op(|| fs::remove_file(&backup_path)).map_err(|e| {
            format!(
                "Failed to remove stale backup {}: {}",
                backup_path.display(),
                e
            )
        })?;
    }

    let had_target = target_path.exists();
    if had_target {
        retry_file_op(|| fs::rename(target_path, &backup_path)).map_err(|e| {
            format!(
                "Failed to move {} to backup {}: {}",
                target_path.display(),
                backup_path.display(),
                e
            )
        })?;
    }

    if let Err(copy_error) = retry_file_op(|| fs::copy(artifact_path, target_path)) {
        if had_target && backup_path.exists() {
            let _ = retry_file_op(|| fs::remove_file(target_path));
            let _ = retry_file_op(|| fs::rename(&backup_path, target_path));
        }
        return Err(format!(
            "Failed to copy update artifact {} to {}: {}",
            artifact_path.display(),
            target_path.display(),
            copy_error
        ));
    }

    if backup_path.exists() {
        retry_file_op(|| fs::remove_file(&backup_path)).map_err(|e| {
            format!(
                "Failed to remove backup {} after update: {}",
                backup_path.display(),
                e
            )
        })?;
    }

    Ok(())
}

pub fn update_work_dir() -> PathBuf {
    crate::config::get_data_dir().join("updates")
}

pub fn updater_log_path() -> PathBuf {
    crate::config::get_data_dir()
        .join("logs")
        .join("updater.log")
}

fn append_helper_log(log_path: &Path, message: &str) {
    if let Some(parent) = log_path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(mut file) = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
    {
        let _ = writeln!(
            file,
            "{} [updater] {}",
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f"),
            message
        );
    }
}

pub fn fetch_update_manifest(manifest_url: &str) -> Result<UpdateManifest, String> {
    let response = ureq::get(manifest_url)
        .set("User-Agent", "dybur-updater")
        .call()
        .map_err(|e| format!("Failed to fetch update manifest: {}", e))?;

    let body = response
        .into_string()
        .map_err(|e| format!("Failed to read update manifest: {}", e))?;
    serde_json::from_str::<UpdateManifest>(&body)
        .map_err(|e| format!("Failed to parse update manifest: {}", e))
}

pub fn check_for_update(current_version: &str) -> Result<Option<SelectedUpdate>, String> {
    let platform_key = current_platform_key()
        .ok_or_else(|| "Updates are not supported on this platform".to_string())?;
    let manifest_url = update_manifest_url();
    let manifest = fetch_update_manifest(&manifest_url)?;
    select_platform_asset(&manifest, &platform_key, current_version)
}

fn artifact_file_name(url: &str) -> Result<String, String> {
    let name = url
        .split('?')
        .next()
        .unwrap_or(url)
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .ok_or_else(|| format!("Update URL has no file name: {}", url))?;

    if name.trim().is_empty() {
        Err(format!("Update URL has no file name: {}", url))
    } else {
        Ok(name.to_string())
    }
}

pub fn download_update_artifact(
    update: &SelectedUpdate,
    update_dir: &Path,
) -> Result<PathBuf, String> {
    fs::create_dir_all(update_dir).map_err(|e| {
        format!(
            "Failed to create update directory {}: {}",
            update_dir.display(),
            e
        )
    })?;

    let artifact_path = update_dir.join(artifact_file_name(&update.asset.url)?);
    let response = ureq::get(&update.asset.url)
        .set("User-Agent", "dybur-updater")
        .call()
        .map_err(|e| format!("Failed to download update artifact: {}", e))?;
    let mut reader = response.into_reader();
    let mut output = File::create(&artifact_path)
        .map_err(|e| format!("Failed to create {}: {}", artifact_path.display(), e))?;
    std::io::copy(&mut reader, &mut output)
        .map_err(|e| format!("Failed to write {}: {}", artifact_path.display(), e))?;

    verify_file_sha256(&artifact_path, &update.asset.sha256)?;
    Ok(artifact_path)
}

pub fn copy_current_exe_to_helper(
    current_exe: &Path,
    update_dir: &Path,
) -> Result<PathBuf, String> {
    fs::create_dir_all(update_dir).map_err(|e| {
        format!(
            "Failed to create update directory {}: {}",
            update_dir.display(),
            e
        )
    })?;

    let extension = current_exe
        .extension()
        .map(|ext| format!(".{}", ext.to_string_lossy()))
        .unwrap_or_default();
    let helper_path = update_dir.join(format!("dybur-helper-{}{}", std::process::id(), extension));

    fs::copy(current_exe, &helper_path).map_err(|e| {
        format!(
            "Failed to copy update helper from {} to {}: {}",
            current_exe.display(),
            helper_path.display(),
            e
        )
    })?;

    Ok(helper_path)
}

pub fn spawn_update_helper(args: &HelperInstallArgs, current_exe: &Path) -> Result<(), String> {
    let helper_path = copy_current_exe_to_helper(current_exe, &update_work_dir())?;
    let helper_args = build_helper_args(args);
    let mut command = Command::new(&helper_path);
    command.args(helper_args);

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    command.spawn().map_err(|e| {
        format!(
            "Failed to launch update helper {}: {}",
            helper_path.display(),
            e
        )
    })?;

    Ok(())
}

pub fn run_helper_from_env() -> Result<bool, String> {
    let args: Vec<String> = env::args().skip(1).collect();
    let Some(helper_args) = parse_helper_args(&args)? else {
        return Ok(false);
    };

    run_update_helper(&helper_args)?;
    Ok(true)
}

pub fn run_update_helper(args: &HelperInstallArgs) -> Result<(), String> {
    append_helper_log(&args.log_path, "helper started");
    wait_for_process_exit(args.pid, Duration::from_secs(45), &args.log_path)?;

    let install_result = match args.platform {
        InstallPlatform::WindowsPortable => {
            append_helper_log(&args.log_path, "installing Windows portable update");
            replace_file_with_backup(&args.artifact_path, &args.target_exe_path)
        }
        InstallPlatform::MacDmg => {
            append_helper_log(&args.log_path, "installing macOS DMG update");
            install_macos_dmg(&args.artifact_path, &args.bundle_path, &args.log_path)
        }
    };

    if let Err(error) = install_result {
        append_helper_log(&args.log_path, &format!("install failed: {}", error));
        return Err(error);
    }
    append_helper_log(&args.log_path, "install complete");

    if env::var_os("DYBUR_UPDATE_HELPER_SKIP_RELAUNCH").is_some() {
        append_helper_log(&args.log_path, "skipping relaunch by environment request");
        return Ok(());
    }

    let (program, program_args) = relaunch_command_parts(args);
    append_helper_log(
        &args.log_path,
        &format!("relaunching dybur from {}", args.relaunch_path.display()),
    );
    Command::new(&program)
        .args(program_args)
        .spawn()
        .map_err(|e| {
            format!(
                "Failed to relaunch dybur from {}: {}",
                args.relaunch_path.display(),
                e
            )
        })?;

    Ok(())
}

fn relaunch_command_parts(args: &HelperInstallArgs) -> (PathBuf, Vec<String>) {
    match args.platform {
        InstallPlatform::WindowsPortable => (args.relaunch_path.clone(), Vec::new()),
        InstallPlatform::MacDmg => (
            PathBuf::from("open"),
            vec![args.relaunch_path.to_string_lossy().to_string()],
        ),
    }
}

fn wait_for_process_exit(pid: u32, timeout: Duration, log_path: &Path) -> Result<(), String> {
    let started = Instant::now();
    while process_is_running(pid) {
        if started.elapsed() > timeout {
            return Err(format!("Timed out waiting for process {} to exit", pid));
        }
        std::thread::sleep(Duration::from_millis(200));
    }

    append_helper_log(log_path, &format!("process {} exited", pid));
    Ok(())
}

#[cfg(target_os = "windows")]
fn process_is_running(pid: u32) -> bool {
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    const STILL_ACTIVE: u32 = 259;

    #[link(name = "kernel32")]
    extern "system" {
        fn OpenProcess(
            dwDesiredAccess: u32,
            bInheritHandle: i32,
            dwProcessId: u32,
        ) -> *mut std::ffi::c_void;
        fn CloseHandle(hObject: *mut std::ffi::c_void) -> i32;
        fn GetExitCodeProcess(hProcess: *mut std::ffi::c_void, lpExitCode: *mut u32) -> i32;
    }

    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return false;
        }

        let mut exit_code = 0u32;
        let ok = GetExitCodeProcess(handle, &mut exit_code as *mut u32) != 0;
        CloseHandle(handle);

        ok && exit_code == STILL_ACTIVE
    }
}

#[cfg(not(target_os = "windows"))]
fn process_is_running(pid: u32) -> bool {
    Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn replace_directory_with_backup(source_dir: &Path, target_dir: &Path) -> Result<(), String> {
    if !source_dir.is_dir() {
        return Err(format!(
            "Update source directory does not exist: {}",
            source_dir.display()
        ));
    }

    if let Some(parent) = target_dir.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create {}: {}", parent.display(), e))?;
    }

    let backup_path = backup_path_for(target_dir);
    if backup_path.exists() {
        fs::remove_dir_all(&backup_path).map_err(|e| {
            format!(
                "Failed to remove stale backup {}: {}",
                backup_path.display(),
                e
            )
        })?;
    }

    let had_target = target_dir.exists();
    if had_target {
        fs::rename(target_dir, &backup_path).map_err(|e| {
            format!(
                "Failed to move {} to backup {}: {}",
                target_dir.display(),
                backup_path.display(),
                e
            )
        })?;
    }

    if let Err(error) = copy_dir_all(source_dir, target_dir) {
        if had_target && backup_path.exists() {
            let _ = fs::remove_dir_all(target_dir);
            let _ = fs::rename(&backup_path, target_dir);
        }
        return Err(error);
    }

    if backup_path.exists() {
        fs::remove_dir_all(&backup_path).map_err(|e| {
            format!(
                "Failed to remove backup {} after update: {}",
                backup_path.display(),
                e
            )
        })?;
    }

    Ok(())
}

fn copy_dir_all(source: &Path, target: &Path) -> Result<(), String> {
    fs::create_dir_all(target)
        .map_err(|e| format!("Failed to create {}: {}", target.display(), e))?;

    for entry in
        fs::read_dir(source).map_err(|e| format!("Failed to read {}: {}", source.display(), e))?
    {
        let entry = entry.map_err(|e| format!("Failed to read directory entry: {}", e))?;
        let file_type = entry
            .file_type()
            .map_err(|e| format!("Failed to read file type: {}", e))?;
        let from = entry.path();
        let to = target.join(entry.file_name());

        if file_type.is_dir() {
            copy_dir_all(&from, &to)?;
        } else {
            fs::copy(&from, &to).map_err(|e| {
                format!(
                    "Failed to copy {} to {}: {}",
                    from.display(),
                    to.display(),
                    e
                )
            })?;
        }
    }

    Ok(())
}

fn install_macos_dmg(dmg_path: &Path, bundle_path: &Path, log_path: &Path) -> Result<(), String> {
    let mount_point = env::temp_dir().join(format!("dybur-update-dmg-{}", std::process::id()));
    if mount_point.exists() {
        fs::remove_dir_all(&mount_point).map_err(|e| {
            format!(
                "Failed to remove stale mount directory {}: {}",
                mount_point.display(),
                e
            )
        })?;
    }
    fs::create_dir_all(&mount_point).map_err(|e| {
        format!(
            "Failed to create mount directory {}: {}",
            mount_point.display(),
            e
        )
    })?;

    let attach = Command::new("hdiutil")
        .arg("attach")
        .arg(dmg_path)
        .arg("-mountpoint")
        .arg(&mount_point)
        .arg("-nobrowse")
        .arg("-readonly")
        .arg("-quiet")
        .status()
        .map_err(|e| format!("Failed to run hdiutil attach: {}", e))?;

    if !attach.success() {
        return Err(format!("hdiutil attach failed for {}", dmg_path.display()));
    }

    let install_result = (|| {
        let app_bundle = fs::read_dir(&mount_point)
            .map_err(|e| {
                format!(
                    "Failed to read mounted DMG {}: {}",
                    mount_point.display(),
                    e
                )
            })?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .find(|path| path.extension().map(|ext| ext == "app").unwrap_or(false))
            .ok_or_else(|| "Mounted DMG did not contain a .app bundle".to_string())?;

        replace_directory_with_backup(&app_bundle, bundle_path)?;

        let _ = Command::new("xattr")
            .arg("-rd")
            .arg("com.apple.quarantine")
            .arg(bundle_path)
            .status();

        Ok(())
    })();

    let detach = Command::new("hdiutil")
        .arg("detach")
        .arg(&mount_point)
        .arg("-quiet")
        .status();
    if detach
        .as_ref()
        .map(|status| !status.success())
        .unwrap_or(true)
    {
        let _ = Command::new("hdiutil")
            .arg("detach")
            .arg(&mount_point)
            .arg("-force")
            .arg("-quiet")
            .status();
    }
    let _ = fs::remove_dir_all(&mount_point);

    if let Err(error) = &install_result {
        append_helper_log(log_path, &format!("macOS install failed: {}", error));
    }

    install_result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::Mutex;

    static UPDATE_ENV_LOCK: Mutex<()> = Mutex::new(());

    fn sample_manifest() -> UpdateManifest {
        serde_json::from_str(
            r#"{
              "version": "1.3.0",
              "notes": "Portable auto updater",
              "pub_date": "2026-06-10T00:00:00Z",
              "platforms": {
                "windows-x64": {
                  "url": "https://github.com/oshtz/dybur/releases/download/v1.3.0/dybur-windows-x64.exe",
                  "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                  "size": 24000000
                },
                "darwin-arm64": {
                  "url": "https://github.com/oshtz/dybur/releases/download/v1.3.0/dybur-macos-arm64.dmg",
                  "sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
                }
              }
            }"#,
        )
        .unwrap()
    }

    #[test]
    fn newer_version_accepts_semver_with_or_without_v_prefix() {
        assert!(is_newer_version("1.3.0", "1.2.4"));
        assert!(is_newer_version("v1.10.0", "1.9.9"));
        assert!(!is_newer_version("1.2.4", "1.2.4"));
        assert!(!is_newer_version("1.2.3", "1.2.4"));
    }

    #[test]
    fn select_platform_asset_returns_matching_newer_platform() {
        let manifest = sample_manifest();
        let selected = select_platform_asset(&manifest, "windows-x64", "1.2.4")
            .unwrap()
            .unwrap();

        assert_eq!(selected.version, "1.3.0");
        assert_eq!(selected.platform_key, "windows-x64");
        assert_eq!(
            selected.asset.url,
            "https://github.com/oshtz/dybur/releases/download/v1.3.0/dybur-windows-x64.exe"
        );
    }

    #[test]
    fn select_platform_asset_returns_none_for_current_or_unsupported_platform() {
        let manifest = sample_manifest();

        assert!(select_platform_asset(&manifest, "windows-x64", "1.3.0")
            .unwrap()
            .is_none());
        assert!(select_platform_asset(&manifest, "darwin-x64", "1.2.4")
            .unwrap()
            .is_none());
    }

    #[test]
    fn verify_file_sha256_accepts_matching_hash_and_rejects_mismatch() {
        let path = std::env::temp_dir().join(format!(
            "dybur-updater-sha-{}-{}.txt",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        fs::write(&path, b"abc").unwrap();

        let expected = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        assert!(verify_file_sha256(&path, expected).is_ok());
        assert!(verify_file_sha256(&path, &"0".repeat(64)).is_err());

        let _ = fs::remove_file(path);
    }

    #[test]
    fn helper_args_round_trip_all_required_paths() {
        let expected = HelperInstallArgs {
            platform: InstallPlatform::WindowsPortable,
            pid: 42,
            artifact_path: PathBuf::from(r"C:\Users\USER\AppData\Local\Temp\dybur-new.exe"),
            target_exe_path: PathBuf::from(r"C:\Users\USER\.dybur\bin\dybur.exe"),
            bundle_path: PathBuf::from(r"C:\Users\USER\.dybur\bin\dybur.exe"),
            relaunch_path: PathBuf::from(r"C:\Users\USER\.dybur\bin\dybur.exe"),
            log_path: PathBuf::from(r"C:\Users\USER\.dybur\logs\updater.log"),
        };

        let args = build_helper_args(&expected);
        assert_eq!(args.first().map(String::as_str), Some(UPDATE_HELPER_FLAG));

        let parsed = parse_helper_args(&args).unwrap().unwrap();
        assert_eq!(parsed, expected);
    }

    #[test]
    fn platform_key_maps_rust_os_and_arch_to_release_manifest_keys() {
        assert_eq!(
            platform_key_for("windows", "x86_64").as_deref(),
            Some("windows-x64")
        );
        assert_eq!(
            platform_key_for("windows", "aarch64").as_deref(),
            Some("windows-arm64")
        );
        assert_eq!(
            platform_key_for("macos", "aarch64").as_deref(),
            Some("darwin-arm64")
        );
        assert_eq!(
            platform_key_for("macos", "x86_64").as_deref(),
            Some("darwin-x64")
        );
        assert!(platform_key_for("linux", "x86_64").is_none());
    }

    #[test]
    fn update_manifest_url_uses_environment_override_when_present() {
        let _guard = UPDATE_ENV_LOCK.lock().unwrap();
        let original = env::var_os("DYBUR_UPDATE_MANIFEST_URL");
        env::set_var(
            "DYBUR_UPDATE_MANIFEST_URL",
            "http://127.0.0.1:8123/dybur-update.json",
        );

        assert_eq!(
            update_manifest_url(),
            "http://127.0.0.1:8123/dybur-update.json"
        );

        match original {
            Some(value) => env::set_var("DYBUR_UPDATE_MANIFEST_URL", value),
            None => env::remove_var("DYBUR_UPDATE_MANIFEST_URL"),
        }
    }

    #[test]
    fn helper_install_args_target_current_windows_exe() {
        let args = helper_install_args_for(
            InstallPlatform::WindowsPortable,
            55,
            PathBuf::from(r"C:\Temp\dybur-new.exe"),
            PathBuf::from(r"C:\Users\USER\.dybur\bin\dybur.exe"),
            PathBuf::from(r"C:\Users\USER\.dybur\logs\updater.log"),
        )
        .unwrap();

        assert_eq!(args.platform, InstallPlatform::WindowsPortable);
        assert_eq!(args.pid, 55);
        assert_eq!(
            args.target_exe_path,
            PathBuf::from(r"C:\Users\USER\.dybur\bin\dybur.exe")
        );
        assert_eq!(args.bundle_path, args.target_exe_path);
        assert_eq!(args.relaunch_path, args.target_exe_path);
    }

    #[test]
    fn copied_helper_name_avoids_windows_installer_detection_keywords() {
        let dir = unique_temp_dir("helper-name");
        let source = dir.join("dybur.exe");
        fs::write(&source, b"helper").unwrap();

        let helper = copy_current_exe_to_helper(&source, &dir).unwrap();
        let helper_name = helper.file_name().unwrap().to_string_lossy().to_lowercase();

        assert!(!helper_name.contains("update"));
        assert!(!helper_name.contains("install"));
        assert!(!helper_name.contains("setup"));

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn helper_install_args_target_parent_macos_app_bundle() {
        let args = helper_install_args_for(
            InstallPlatform::MacDmg,
            77,
            PathBuf::from("/tmp/dybur.dmg"),
            PathBuf::from("/Users/user/.dybur/bin/dybur.app/Contents/MacOS/dybur"),
            PathBuf::from("/Users/user/.dybur/logs/updater.log"),
        )
        .unwrap();

        assert_eq!(args.platform, InstallPlatform::MacDmg);
        assert_eq!(
            args.bundle_path,
            PathBuf::from("/Users/user/.dybur/bin/dybur.app")
        );
        assert_eq!(
            args.target_exe_path,
            PathBuf::from("/Users/user/.dybur/bin/dybur.app/Contents/MacOS/dybur")
        );
        assert_eq!(
            args.relaunch_path,
            PathBuf::from("/Users/user/.dybur/bin/dybur.app")
        );
    }

    #[test]
    fn macos_relaunch_uses_open_on_app_bundle() {
        let args = HelperInstallArgs {
            platform: InstallPlatform::MacDmg,
            pid: 77,
            artifact_path: PathBuf::from("/tmp/dybur.dmg"),
            target_exe_path: PathBuf::from("/Applications/dybur.app/Contents/MacOS/dybur"),
            bundle_path: PathBuf::from("/Applications/dybur.app"),
            relaunch_path: PathBuf::from("/Applications/dybur.app"),
            log_path: PathBuf::from("/tmp/updater.log"),
        };

        let (program, program_args) = relaunch_command_parts(&args);
        assert_eq!(program, PathBuf::from("open"));
        assert_eq!(program_args, vec!["/Applications/dybur.app"]);
    }

    fn unique_temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "dybur-updater-{}-{}-{}",
            name,
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn replace_file_with_backup_replaces_existing_target_and_removes_backup() {
        let dir = unique_temp_dir("replace");
        let artifact = dir.join("new.exe");
        let target = dir.join("dybur.exe");
        let backup = backup_path_for(&target);
        fs::write(&artifact, b"new-version").unwrap();
        fs::write(&target, b"old-version").unwrap();

        replace_file_with_backup(&artifact, &target).unwrap();

        assert_eq!(fs::read(&target).unwrap(), b"new-version");
        assert!(!backup.exists());

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn replace_file_with_backup_restores_original_when_copy_fails() {
        let dir = unique_temp_dir("rollback");
        let artifact = dir.join("missing.exe");
        let target = dir.join("dybur.exe");
        fs::write(&target, b"old-version").unwrap();

        let result = replace_file_with_backup(&artifact, &target);

        assert!(result.is_err());
        assert_eq!(fs::read(&target).unwrap(), b"old-version");
        assert!(!backup_path_for(&target).exists());

        let _ = fs::remove_dir_all(dir);
    }
}

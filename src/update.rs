use anyhow::{Context, Result, ensure};
use reqwest::blocking::{Client, Response};
use semver::Version;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    time::Duration,
};

const LATEST_RELEASE: &str = "https://api.github.com/repos/alvyn16/Neko/releases/latest";
const MAX_INSTALLER_BYTES: u64 = 150 * 1024 * 1024;
const MAX_CHECKSUM_BYTES: u64 = 4096;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateInfo {
    pub version: String,
    pub release_url: String,
    pub installer_url: Option<String>,
    pub checksum_url: Option<String>,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    html_url: String,
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

fn parse_version(value: &str) -> Result<Version> {
    Version::parse(value.trim().trim_start_matches('v')).context("Release has an invalid version")
}

fn client() -> Result<Client> {
    Client::builder()
        .user_agent(format!("Neko/{}", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(8))
        .timeout(Duration::from_secs(90))
        .build()
        .context("Could not start the update client")
}

pub fn check() -> Result<Option<UpdateInfo>> {
    let response = client()?
        .get(LATEST_RELEASE)
        .send()
        .context("Could not reach GitHub Releases")?
        .error_for_status()
        .context("GitHub could not return the latest release")?;
    let release: Release = response
        .json()
        .context("GitHub returned invalid release information")?;
    if parse_version(&release.tag_name)? <= parse_version(env!("CARGO_PKG_VERSION"))? {
        return Ok(None);
    }
    let installer_url = release
        .assets
        .iter()
        .find(|asset| asset.name == "Neko-Setup-x64.exe")
        .map(|asset| asset.browser_download_url.clone());
    let checksum_url = release
        .assets
        .iter()
        .find(|asset| asset.name == "Neko-Setup-x64.exe.sha256")
        .map(|asset| asset.browser_download_url.clone());
    Ok(Some(UpdateInfo {
        version: release.tag_name.trim_start_matches('v').to_owned(),
        release_url: release.html_url,
        installer_url,
        checksum_url,
    }))
}

fn checked_response(url: &str, limit: u64) -> Result<Response> {
    ensure!(
        url.starts_with("https://github.com/alvyn16/Neko/releases/download/"),
        "The update link is not an official Neko release"
    );
    let response = client()?
        .get(url)
        .send()
        .context("Could not download the update")?
        .error_for_status()
        .context("GitHub could not provide the update")?;
    let host = response.url().host_str().unwrap_or_default();
    ensure!(
        matches!(
            host,
            "github.com" | "objects.githubusercontent.com" | "release-assets.githubusercontent.com"
        ),
        "The update download redirected outside GitHub"
    );
    if let Some(length) = response.content_length() {
        ensure!(length <= limit, "The update download is unexpectedly large");
    }
    Ok(response)
}

fn bounded_bytes(response: Response, limit: u64) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    response
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .context("Could not read the update download")?;
    ensure!(
        bytes.len() as u64 <= limit,
        "The update download is unexpectedly large"
    );
    Ok(bytes)
}

fn parse_checksum(bytes: &[u8]) -> Result<&str> {
    let checksum = std::str::from_utf8(bytes)
        .context("The update checksum is not valid text")?
        .split_whitespace()
        .next()
        .context("The update checksum is empty")?;
    ensure!(
        checksum.len() == 64 && checksum.chars().all(|c| c.is_ascii_hexdigit()),
        "The update checksum is invalid"
    );
    Ok(checksum)
}

fn verify_checksum(bytes: &[u8], expected: &str) -> Result<()> {
    let actual = format!("{:x}", Sha256::digest(bytes));
    ensure!(
        actual.eq_ignore_ascii_case(expected),
        "The downloaded installer did not match its published checksum"
    );
    Ok(())
}

pub fn download_installer(info: &UpdateInfo, cache_dir: &Path) -> Result<PathBuf> {
    let installer_url = info
        .installer_url
        .as_deref()
        .context("This release does not include the Windows installer")?;
    let checksum_url = info.checksum_url.as_deref().context(
        "This release cannot be installed automatically because its checksum is missing",
    )?;
    let checksum = bounded_bytes(
        checked_response(checksum_url, MAX_CHECKSUM_BYTES)?,
        MAX_CHECKSUM_BYTES,
    )?;
    let checksum = parse_checksum(&checksum)?;

    let installer = bounded_bytes(
        checked_response(installer_url, MAX_INSTALLER_BYTES)?,
        MAX_INSTALLER_BYTES,
    )?;
    verify_checksum(&installer, checksum)?;
    let directory = cache_dir.join("updates");
    fs::create_dir_all(&directory).context("Could not create the update cache")?;
    let output = directory.join(format!("Neko-Setup-{}.exe", info.version));
    crate::local::atomic_cache_write(&output, &installer)?;
    Ok(output)
}

#[cfg(windows)]
pub fn launch_installer(path: &Path) -> Result<()> {
    use std::os::windows::process::CommandExt;
    ensure!(path.is_file(), "The update installer no longer exists");
    std::process::Command::new(path)
        .args(["/SILENT", "/NORESTART", "/CLOSEAPPLICATIONS"])
        .creation_flags(0x08000000)
        .spawn()
        .context("Could not start the update installer")?;
    Ok(())
}

#[cfg(not(windows))]
pub fn launch_installer(_path: &Path) -> Result<()> {
    anyhow::bail!("Automatic installation is available on Windows")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_versions_accept_a_v_prefix_and_compare_semantically() {
        assert_eq!(parse_version("v1.2.3").unwrap(), Version::new(1, 2, 3));
        assert!(parse_version("0.10.0").unwrap() > parse_version("0.9.9").unwrap());
        assert!(parse_version("latest").is_err());
    }

    #[test]
    fn installer_checksum_must_be_well_formed_and_match() {
        let expected = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        assert_eq!(
            parse_checksum(format!("{expected}  Neko.exe\n").as_bytes()).unwrap(),
            expected
        );
        verify_checksum(b"abc", expected).unwrap();
        assert!(verify_checksum(b"tampered", expected).is_err());
        assert!(parse_checksum(b"not-a-sha256").is_err());
    }
}

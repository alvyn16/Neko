//! Blocking provider work. Call these functions from workers, never the GPUI thread.
mod bjarneo;
mod frenzy;
mod wallhaven;

use crate::model::{Provider, SearchResults, Wallpaper};
use anyhow::{Context, Result, bail, ensure};
use reqwest::blocking::{Client, Response};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use url::Url;

const MAX_DOWNLOAD: u64 = 100 * 1024 * 1024;
const MAX_THUMBNAIL: u64 = 8 * 1024 * 1024;
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub fn search(
    provider: Provider,
    query: &str,
    color: Option<&str>,
    category: Option<&str>,
    page: u32,
    cache_dir: &Path,
    refresh: bool,
) -> Result<SearchResults> {
    ensure!(
        query.chars().count() <= 512,
        "Keep your search under 512 characters."
    );
    match provider {
        Provider::Wallhaven => wallhaven::search(query, page.max(1)),
        Provider::Bjarneo => bjarneo::search(query, color, page.max(1), cache_dir, refresh),
        Provider::Frenzy => frenzy::search(query, category, page.max(1), cache_dir, refresh),
    }
}

/// Download the original image, preserving its original bytes and detected format.
/// A filename derived from the complete URL prevents provider/name collisions.
pub fn download(item: &Wallpaper, cache_dir: &Path) -> Result<PathBuf> {
    if let Some(path) = &item.local_path {
        crate::local::image_dimensions(path)?;
        return Ok(path.clone());
    }
    let url = item
        .image_url
        .as_deref()
        .context("This wallpaper has no download URL.")?;
    cached_image(url, &cache_dir.join("downloads"), MAX_DOWNLOAD)
}

/// Cache only the provider's preview in the grid; full originals are fetched on demand.
pub fn thumbnail(item: &Wallpaper, cache_dir: &Path) -> Result<PathBuf> {
    if let Some(path) = &item.local_path {
        return crate::local::thumbnail(path, cache_dir);
    }
    let url = item
        .thumbnail_url
        .as_deref()
        .context("This wallpaper does not have a preview.")?;
    let limit = if item.source == Some(Provider::Frenzy) {
        MAX_DOWNLOAD
    } else {
        MAX_THUMBNAIL
    };
    let raw = cached_image(url, &cache_dir.join("network-thumbnails"), limit)?;
    crate::local::thumbnail(&raw, cache_dir)
}

fn cached_image(url: &str, directory: &Path, limit: u64) -> Result<PathBuf> {
    validate_url(&Url::parse(url).context("Invalid image URL.")?)?;
    fs::create_dir_all(directory).context("Could not create the image cache.")?;
    let key = hash(url.as_bytes());
    // Magic bytes decide the extension, rather than potentially misleading URL suffixes.
    for extension in ["jpg", "png", "webp", "bmp", "gif", "tiff"] {
        let candidate = directory.join(format!("{key}.{extension}"));
        if candidate.is_file() {
            if crate::local::read_image(&candidate).is_ok() {
                return Ok(candidate);
            }
            // This is a corrupt cache entry under our own directory, never a user image.
            let _ = fs::remove_file(&candidate);
        }
    }
    let bytes = response_bytes(get(url)?, limit)?;
    let format = image::guess_format(&bytes).context("The server returned an invalid image.")?;
    let extension = match format {
        image::ImageFormat::Jpeg => "jpg",
        image::ImageFormat::Png => "png",
        image::ImageFormat::WebP => "webp",
        image::ImageFormat::Bmp => "bmp",
        image::ImageFormat::Gif => "gif",
        image::ImageFormat::Tiff => "tiff",
        _ => bail!("This image format is not supported."),
    };
    let destination = directory.join(format!("{key}.{extension}"));
    let temporary = temporary_path(&destination);
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        crate::local::read_image(&temporary)
            .context("The downloaded image is damaged or too large.")?;
        fs::rename(&temporary, &destination).context("Could not save the downloaded image.")?;
        Ok(destination)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn client() -> Result<&'static Client> {
    static CLIENT: OnceLock<std::result::Result<Client, String>> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            Client::builder()
                .user_agent(concat!(
                    "Neko/",
                    env!("CARGO_PKG_VERSION"),
                    " (desktop wallpaper manager)"
                ))
                .connect_timeout(Duration::from_secs(10))
                .timeout(Duration::from_secs(60))
                .redirect(reqwest::redirect::Policy::custom(|attempt| {
                    if attempt.previous().len() >= 4 {
                        attempt.error("Too many redirects")
                    } else if validate_url(attempt.url()).is_err() {
                        attempt.error("Unexpected download destination")
                    } else {
                        attempt.follow()
                    }
                }))
                .build()
                .map_err(|error| error.to_string())
        })
        .as_ref()
        .map_err(|error| anyhow::anyhow!(error.clone()))
}

fn get(url: &str) -> Result<Response> {
    let parsed = Url::parse(url).context("The provider returned an invalid URL.")?;
    validate_url(&parsed)?;
    let response = client()?
        .get(parsed)
        .send()
        .context("Could not connect. Check your internet connection and try again.")?;
    check_response(response)
}

fn check_response(response: Response) -> Result<Response> {
    match response.status().as_u16() {
        200..=299 => Ok(response),
        429 => bail!("The provider is receiving too many requests. Wait a minute, then retry."),
        401 | 403 => bail!("The provider denied this request. Try again later or switch sources."),
        404 => bail!("This wallpaper or collection is no longer available."),
        code => bail!("The provider could not complete the request (HTTP {code}). Try again."),
    }
}

fn validate_url(url: &Url) -> Result<()> {
    let host = url.host_str().unwrap_or_default();
    let allowed = matches!(
        host,
        "wallhaven.cc"
            | "w.wallhaven.cc"
            | "th.wallhaven.cc"
            | "api.github.com"
            | "raw.githubusercontent.com"
            | "bjarneo.github.io"
    ) || host.ends_with(".your-objectstorage.com");
    ensure!(
        url.scheme() == "https"
            && allowed
            && url.username().is_empty()
            && url.password().is_none()
            && url.port_or_known_default() == Some(443),
        "The provider returned an unexpected download URL."
    );
    Ok(())
}

fn response_bytes(response: Response, maximum: u64) -> Result<Vec<u8>> {
    if let Some(length) = response.content_length() {
        ensure!(length <= maximum, "The download exceeds Neko's size limit.");
    }
    read_bounded(response, maximum)
}

fn read_bounded(reader: impl Read, maximum: u64) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader
        .take(maximum + 1)
        .read_to_end(&mut bytes)
        .context("The connection was interrupted. Try again.")?;
    ensure!(
        bytes.len() as u64 <= maximum,
        "The download exceeds Neko's size limit."
    );
    Ok(bytes)
}

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn temporary_path(path: &Path) -> PathBuf {
    // Preserve the suffix for image decoders that inspect the filename.
    let file = path.file_name().unwrap_or_default().to_string_lossy();
    path.with_file_name(format!(
        ".{}-{}-{file}",
        std::process::id(),
        TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ))
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().context("Invalid cache location.")?;
    fs::create_dir_all(parent)?;
    let temporary = temporary_path(path);
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_limits_reject_oversized_and_accept_exact_limit() {
        assert_eq!(read_bounded(&b"abcd"[..], 4).unwrap(), b"abcd");
        assert!(read_bounded(&b"abcde"[..], 4).is_err());
    }

    #[test]
    fn urls_cannot_redirect_to_insecure_or_local_destinations() {
        for input in [
            "http://w.wallhaven.cc/a.jpg",
            "https://127.0.0.1/a",
            "file:///C:/secret",
            "https://evil.example/a",
            "https://wallhaven.cc@evil.example/a",
            "https://w.wallhaven.cc:8080/a",
        ] {
            assert!(
                validate_url(&Url::parse(input).unwrap()).is_err(),
                "{input}"
            );
        }
        assert!(
            validate_url(
                &Url::parse("https://wallpapers.hel1.your-objectstorage.com/a.jpg").unwrap()
            )
            .is_ok()
        );
        assert!(validate_url(&Url::parse("https://api.github.com/repos/a/b").unwrap()).is_ok());
        assert!(
            validate_url(
                &Url::parse("https://raw.githubusercontent.com/a/b/main/image.png").unwrap()
            )
            .is_ok()
        );
    }

    #[test]
    fn atomic_cache_updates_replace_old_contents() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("index.json");
        write_atomic(&path, b"first").unwrap();
        write_atomic(&path, b"second").unwrap();
        assert_eq!(fs::read(path).unwrap(), b"second");
        assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 1);
    }

    #[test]
    #[ignore = "Uses the public internet; run manually with --ignored --nocapture"]
    fn online_provider_smoke() {
        let cache = tempfile::tempdir().unwrap();
        let mut failures = Vec::new();
        for provider in [Provider::Frenzy, Provider::Wallhaven, Provider::Bjarneo] {
            let query = if provider == Provider::Frenzy {
                ""
            } else {
                "mountain"
            };
            let result = (|| -> Result<()> {
                let results = search(provider, query, None, None, 1, cache.path(), true)?;
                ensure!(!results.items.is_empty(), "the provider returned no images");
                let item = &results.items[0];
                let preview = thumbnail(item, cache.path())?;
                let (width, height) = crate::local::image_dimensions(&preview)?;
                ensure!(
                    width <= 720 && height <= 720,
                    "the generated preview exceeds 720 px"
                );
                let original = download(item, cache.path())?;
                crate::local::read_image(&original)?;
                println!(
                    "{provider:?}: {} matches; preview and original validated",
                    results.total
                );
                Ok(())
            })();
            if let Err(error) = result {
                failures.push(format!("{provider:?}: {error:#}"));
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }
}

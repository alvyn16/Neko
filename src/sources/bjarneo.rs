//! Schema audited against the upstream JSON and gallery scripts on 2026-09-13.
//! `wallpapers.json` is a path-keyed object, not an array of name/url records.
//! `live.json` is a separate animated collection and is intentionally excluded:
//! SystemParametersInfoW applies still images. The site's script supplies the
//! object-storage media base; neither GitHub Pages nor raw GitHub hosts originals.

use super::{
    check_response, client, get, hash, read_bounded, timestamp, validate_url, write_atomic,
};
use crate::model::{Provider, SearchResults, Wallpaper};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap},
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
};
use url::Url;

const INDEX_URL: &str = "https://raw.githubusercontent.com/bjarneo/wallpapers/main/wallpapers.json";
const SCRIPT_URL: &str = "https://bjarneo.github.io/wallpapers/wallpapers.js";
const VERIFIED_MEDIA_BASE: &str = "https://wallpapers.hel1.your-objectstorage.com/";
const CACHE_LIFETIME: u64 = 24 * 60 * 60;
const ERROR_RETRY_DELAY: u64 = 5 * 60;
const MAX_INDEX_BYTES: u64 = 96 * 1024 * 1024;
const MAX_CACHE_BYTES: u64 = 16 * 1024 * 1024;
const PAGE_SIZE: usize = 24;

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default)]
struct Metadata {
    title: String,
    description: String,
    tags: Vec<String>,
    color: String,
    width: u32,
    height: u32,
    dimensions: String,
    thumb_path: String,
    // Upstream currently has no author field. Honor it if the collection adds one.
    author: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Index {
    version: u32,
    fetched_at: u64,
    media_base: String,
    entries: BTreeMap<String, Metadata>,
}

#[derive(Clone)]
struct MemoryIndex {
    index: Arc<Index>,
    retry_after: u64,
    notice: Option<String>,
}

pub(super) fn search(
    query: &str,
    color: Option<&str>,
    page: u32,
    cache_dir: &Path,
    refresh: bool,
) -> Result<SearchResults> {
    let state = load_index(cache_dir, refresh)?;
    let mut results = filter_index(&state.index, query, color, page)?;
    results.notice = state.notice;
    Ok(results)
}

fn load_index(cache_dir: &Path, refresh: bool) -> Result<MemoryIndex> {
    // The compact, typed index stays in memory for local searches and pagination.
    // Unknown upstream fields (including thousands of generated theme palettes)
    // are skipped by serde and are never retained in the application cache.
    static MEMORY: OnceLock<Mutex<HashMap<PathBuf, MemoryIndex>>> = OnceLock::new();
    let mut memory = MEMORY
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .map_err(|_| anyhow::anyhow!("The gallery cache is unavailable. Restart Neko."))?;
    let cache_path = cache_dir.join("providers").join("bjarneo-v1.json");
    let now = timestamp();
    let previous = memory.get(&cache_path).cloned().or_else(|| {
        read_cache(&cache_path).ok().map(|index| MemoryIndex {
            index: Arc::new(index),
            retry_after: 0,
            notice: None,
        })
    });
    if !refresh {
        if let Some(state) = &previous {
            if now.saturating_sub(state.index.fetched_at) < CACHE_LIFETIME
                || now < state.retry_after
            {
                memory.insert(cache_path, state.clone());
                return Ok(state.clone());
            }
        }
    }
    let state = match fetch_index() {
        Ok(index) => {
            let serialized = serde_json::to_vec(&index)?;
            let notice = write_atomic(&cache_path, &serialized).err().map(|_| {
                "Gallery loaded. Its cache could not be saved; check available disk space."
                    .to_string()
            });
            MemoryIndex {
                index: Arc::new(index),
                retry_after: 0,
                notice,
            }
        }
        Err(error) => match previous {
            Some(previous) => MemoryIndex {
                index: previous.index,
                retry_after: now + ERROR_RETRY_DELAY,
                notice: Some(
                    "Showing the saved gallery. Refresh when your connection returns.".to_string(),
                ),
            },
            None => {
                return Err(error.context(
                    "Could not load the bjarneo gallery. Check your connection and retry.",
                ));
            }
        },
    };
    memory.insert(cache_path, state.clone());
    Ok(state)
}

fn fetch_index() -> Result<Index> {
    // Read only the beginning even when the CDN ignores the Range header: the
    // generated JS can be tens of megabytes and is never evaluated as code.
    let media_base = fetch_media_base().unwrap_or_else(|_| VERIFIED_MEDIA_BASE.to_string());
    let response = get(INDEX_URL)?;
    if let Some(length) = response.content_length() {
        ensure!(
            length <= MAX_INDEX_BYTES,
            "The gallery index exceeds Neko's size limit."
        );
    }
    // Stream deserialization to avoid buffering the large, theme-heavy index.
    let entries = parse_index(response.take(MAX_INDEX_BYTES + 1))?;
    Ok(Index {
        version: 1,
        fetched_at: timestamp(),
        media_base,
        entries,
    })
}

fn fetch_media_base() -> Result<String> {
    let response = client()?
        .get(SCRIPT_URL)
        .header(reqwest::header::RANGE, "bytes=0-1023")
        .send()
        .context("Could not read the gallery media location.")?;
    let bytes = read_bounded(check_response(response)?.take(1024), 1024)?;
    parse_media_base(std::str::from_utf8(&bytes)?)
}

fn parse_media_base(prefix: &str) -> Result<String> {
    let first_statement = prefix.split(';').next().unwrap_or_default().trim();
    let (name, value) = first_statement
        .split_once('=')
        .context("Missing gallery media base.")?;
    ensure!(
        name.trim() == "window.WALLPAPERS_BASE_URL",
        "Unrecognized gallery media base."
    );
    let base: String = serde_json::from_str(value.trim()).context("Invalid gallery media base.")?;
    let parsed = Url::parse(&base)?;
    validate_url(&parsed)?;
    ensure!(
        parsed.query().is_none() && parsed.fragment().is_none(),
        "Invalid gallery media base."
    );
    Ok(format!("{}/", base.trim_end_matches('/')))
}

fn parse_index(reader: impl Read) -> Result<BTreeMap<String, Metadata>> {
    let mut entries: BTreeMap<String, Metadata> =
        serde_json::from_reader(std::io::BufReader::new(reader))
            .context("The gallery index format could not be read. Try refreshing later.")?;
    entries.retain(|path, _| is_still_path(path));
    ensure!(
        !entries.is_empty(),
        "The gallery contains no supported still images."
    );
    Ok(entries)
}

fn read_cache(path: &Path) -> Result<Index> {
    let file = fs::File::open(path)?;
    ensure!(
        file.metadata()?.len() <= MAX_CACHE_BYTES,
        "Gallery cache is too large."
    );
    let index: Index =
        serde_json::from_reader(std::io::BufReader::new(file.take(MAX_CACHE_BYTES + 1)))?;
    ensure!(
        index.version == 1 && !index.entries.is_empty(),
        "Gallery cache needs a refresh."
    );
    validate_url(&Url::parse(&index.media_base)?)?;
    Ok(index)
}

fn filter_index(
    index: &Index,
    query: &str,
    color: Option<&str>,
    requested_page: u32,
) -> Result<SearchResults> {
    let query = query.trim().to_lowercase();
    let color = color
        .map(str::trim)
        .filter(|c| !c.is_empty() && !c.eq_ignore_ascii_case("all"))
        .map(str::to_lowercase);
    let matches: Vec<_> = index
        .entries
        .iter()
        .filter(|(path, metadata)| {
            if !is_still_path(path) {
                return false;
            }
            if let Some(color) = &color {
                if metadata.color.to_lowercase() != *color {
                    return false;
                }
            }
            if query.is_empty() {
                return true;
            }
            let haystack = format!(
                "{path} {} {} {} {}",
                metadata.title,
                metadata.description,
                metadata.tags.join(" "),
                metadata.author.as_deref().unwrap_or_default()
            )
            .to_lowercase();
            // Mirrors the gallery's case-insensitive substring search (including tags).
            haystack.contains(&query)
        })
        .collect();
    let total = matches.len();
    let last_page = total.div_ceil(PAGE_SIZE).max(1) as u32;
    let page = requested_page.max(1).min(last_page);
    let skip = (page as usize - 1) * PAGE_SIZE;
    let mut items = Vec::with_capacity(PAGE_SIZE);
    for (path, metadata) in matches.into_iter().skip(skip).take(PAGE_SIZE) {
        let (width, height) = dimensions(path, metadata);
        let thumb_path = if metadata.thumb_path.is_empty() {
            path.as_str()
        } else {
            &metadata.thumb_path
        };
        let image_url = media_url(&index.media_base, path)?;
        let thumbnail_url = media_url(&index.media_base, thumb_path)?;
        let title = if metadata.title.trim().is_empty() {
            Path::new(path)
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .replace(['_', '-'], " ")
        } else {
            metadata.title.clone()
        };
        let attribution = metadata
            .author
            .clone()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| "bjarneo/wallpapers".to_string());
        items.push(Wallpaper {
            id: format!("bjarneo-{}", &hash(path.as_bytes())[..20]),
            title,
            source: Some(Provider::Bjarneo),
            local_path: None,
            image_url: Some(image_url),
            thumbnail_url: Some(thumbnail_url),
            thumbnail_path: None,
            width,
            height,
            color: (!metadata.color.is_empty()).then(|| metadata.color.clone()),
            attribution: Some(attribution),
        });
    }
    Ok(SearchResults {
        items,
        page,
        last_page,
        total,
        notice: None,
    })
}

fn media_url(base: &str, path: &str) -> Result<String> {
    // Treat every segment as a literal filename. URL syntax in an upstream path
    // must never change the origin, inject query parameters, or traverse parents.
    ensure!(
        !path.starts_with('/') && !path.contains('\\') && !path.contains("://"),
        "Invalid gallery image path."
    );
    let segments: Vec<_> = path.split('/').collect();
    ensure!(
        segments
            .iter()
            .all(|s| !s.is_empty() && *s != "." && *s != ".."),
        "Invalid gallery image path."
    );
    let mut url = Url::parse(base)?;
    validate_url(&url)?;
    url.path_segments_mut()
        .map_err(|_| anyhow::anyhow!("Invalid gallery media base."))?
        .pop_if_empty()
        .extend(segments);
    Ok(url.into())
}

fn is_still_path(path: &str) -> bool {
    let lower = path.to_lowercase();
    if lower.starts_with("live/") {
        return false;
    }
    [".jpg", ".jpeg", ".png", ".webp", ".bmp", ".tif", ".tiff"]
        .iter()
        .any(|extension| lower.ends_with(extension))
}

fn dimensions(path: &str, metadata: &Metadata) -> (u32, u32) {
    if metadata.width > 0 && metadata.height > 0 {
        return (metadata.width, metadata.height);
    }
    let resolution = if !metadata.dimensions.is_empty() {
        metadata.dimensions.as_str()
    } else {
        path.rsplit('/')
            .next()
            .unwrap_or_default()
            .split('_')
            .next()
            .unwrap_or_default()
    };
    if let Some((w, h)) = resolution.split_once('x') {
        return (w.parse().unwrap_or(0), h.parse().unwrap_or(0));
    }
    (0, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Representative of the actual path-keyed upstream schema. Unknown theme
    // payloads are kept here to ensure the lean parser keeps skipping them.
    const FIXTURE: &str = r##"{
      "dark/orange/3840x2160_forest.jpg":{"title":"Amber Forest","description":"Warm autumn evening","tags":["trees","nature"],"color":"orange","width":3840,"height":2160,"thumb_path":"cache/thumb/dark/orange/3840x2160_forest.jpg","themes":{"large-generated-theme":{"colors":{"accent":"#dd8800"}}}},
      "light/blue/1920x1080_sea.jpg":{"title":"Open Sea","tags":["coast"],"color":"blue","dimensions":"1920x1080","author":"A. Photographer","thumb_path":"cache/thumb/light/blue/1920x1080_sea.jpg"},
      "dark/blue/2560x1440_night sky #1.jpg":{"title":"Night Sky","color":"blue","thumb_path":"cache/thumb/night sky #1.jpg"},
      "live/1920x1080_waves.mp4":{"title":"Waves","color":"live"},
      "live/1920x1080_sparkle.gif":{"title":"Sparkle","color":"live"}
    }"##;

    fn fixture_index() -> Index {
        Index {
            version: 1,
            fetched_at: timestamp(),
            media_base: VERIFIED_MEDIA_BASE.into(),
            entries: parse_index(FIXTURE.as_bytes()).unwrap(),
        }
    }

    #[test]
    fn parses_real_schema_excludes_animation_and_builds_cdn_thumbnails() {
        let index = fixture_index();
        assert_eq!(index.entries.len(), 3);
        let result = filter_index(&index, " forest ", Some("ORANGE"), 1).unwrap();
        assert_eq!(result.total, 1);
        let image = &result.items[0];
        assert_eq!(image.title, "Amber Forest");
        assert_eq!((image.width, image.height), (3840, 2160));
        assert_eq!(
            image.thumbnail_url.as_deref(),
            Some(
                "https://wallpapers.hel1.your-objectstorage.com/cache/thumb/dark/orange/3840x2160_forest.jpg"
            )
        );
        assert_eq!(image.attribution.as_deref(), Some("bjarneo/wallpapers"));
        assert!(
            !serde_json::to_string(&index)
                .unwrap()
                .contains("large-generated-theme")
        );
    }

    #[test]
    fn query_matches_description_tags_and_author_with_exact_color() {
        let index = fixture_index();
        assert_eq!(filter_index(&index, "autumn", None, 1).unwrap().total, 1);
        assert_eq!(filter_index(&index, "TREES", None, 1).unwrap().total, 1);
        assert_eq!(
            filter_index(&index, "photographer", Some("blue"), 1)
                .unwrap()
                .total,
            1
        );
        assert_eq!(
            filter_index(&index, "", Some("orange"), 1).unwrap().total,
            1
        );
        assert_eq!(filter_index(&index, "", Some("range"), 1).unwrap().total, 0);
        assert_eq!(filter_index(&index, "missing", None, 999).unwrap().page, 1);
    }

    #[test]
    fn pagination_has_stable_non_overlapping_pages() {
        let mut index = fixture_index();
        for i in 0..50 {
            index
                .entries
                .insert(format!("dark/blue/{i:03}.jpg"), Metadata::default());
        }
        let first = filter_index(&index, "", None, 1).unwrap();
        let second = filter_index(&index, "", None, 2).unwrap();
        let last = filter_index(&index, "", None, u32::MAX).unwrap();
        assert_eq!(
            (first.total, first.items.len(), first.last_page),
            (53, 24, 3)
        );
        assert_eq!(last.items.len(), 5);
        assert_eq!(last.page, 3);
        assert!(
            first
                .items
                .iter()
                .all(|a| second.items.iter().all(|b| a.id != b.id))
        );
    }

    #[test]
    fn media_paths_are_encoded_and_cannot_escape_origin() {
        assert_eq!(
            media_url(VERIFIED_MEDIA_BASE, "dark/night sky #1.jpg").unwrap(),
            "https://wallpapers.hel1.your-objectstorage.com/dark/night%20sky%20%231.jpg"
        );
        for path in [
            "../secret.jpg",
            "/evil.jpg",
            "https://evil.example/a.jpg",
            "a/../b.jpg",
            "a\\b.jpg",
        ] {
            assert!(media_url(VERIFIED_MEDIA_BASE, path).is_err());
        }
    }

    #[test]
    fn script_prefix_is_parsed_as_data_never_executed() {
        let valid = "window.WALLPAPERS_BASE_URL = \"https://wallpapers.hel1.your-objectstorage.com\";\nwindow.WALLPAPERS = {";
        assert_eq!(parse_media_base(valid).unwrap(), VERIFIED_MEDIA_BASE);
        assert!(
            parse_media_base("window.WALLPAPERS_BASE_URL = \"https://evil.example\";").is_err()
        );
        assert!(parse_media_base("alert('not data');").is_err());
    }

    #[test]
    fn persisted_fresh_index_works_without_network() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("providers/bjarneo-v1.json");
        write_atomic(&path, &serde_json::to_vec(&fixture_index()).unwrap()).unwrap();
        let results = search("sea", None, 1, temporary.path(), false).unwrap();
        assert_eq!(results.total, 1);
        assert_eq!(
            (results.items[0].width, results.items[0].height),
            (1920, 1080)
        );
    }

    #[test]
    fn invalid_index_is_not_silently_an_empty_gallery() {
        assert!(parse_index(b"[]".as_slice()).is_err());
        assert!(parse_index(b"{}".as_slice()).is_err());
        assert!(parse_index(br#"{"error":"unavailable"}"#.as_slice()).is_err());
    }
}

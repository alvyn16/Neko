use super::{check_response, client, hash, response_bytes, timestamp, validate_url, write_atomic};
use crate::model::{Provider, SearchResults, Wallpaper};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeSet, HashMap},
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
};
use url::Url;

const TREE_URL: &str =
    "https://api.github.com/repos/FrenzyExists/wallpapers/git/trees/main?recursive=1";
const RAW_BASE: &str = "https://raw.githubusercontent.com/FrenzyExists/wallpapers/main/";
const CACHE_LIFETIME: u64 = 7 * 24 * 60 * 60;
const ERROR_RETRY_DELAY: u64 = 30 * 60;
const MAX_TREE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_CACHE_BYTES: u64 = 2 * 1024 * 1024;
const PAGE_SIZE: usize = 24;

const ALLOWED_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp"];
const EXCLUDED_FOLDERS: &[&str] = &["Shitpost"];
const EXCLUDED_STEM_TERMS: &[&str] = &["pattern"];
const EXCLUDED_FILES: &[&str] = &["pikaWall.png", "nord-qsave-1.png", "nord-qsave-2.png"];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct Entry {
    path: String,
    category: String,
    title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Index {
    version: u32,
    fetched_at: u64,
    etag: String,
    tree_sha: String,
    entries: Vec<Entry>,
}

#[derive(Clone)]
struct MemoryIndex {
    index: Arc<Index>,
    retry_after: u64,
    notice: Option<String>,
}

#[derive(Debug)]
enum FetchOutcome {
    Fresh(Index),
    NotModified(Option<String>),
}

#[derive(Debug, Deserialize)]
struct TreeResponse {
    sha: String,
    truncated: bool,
    tree: Vec<TreeItem>,
}

#[derive(Debug, Deserialize)]
struct TreeItem {
    path: String,
    #[serde(rename = "type")]
    kind: String,
}

pub(super) fn search(
    query: &str,
    category: Option<&str>,
    page: u32,
    cache_dir: &Path,
    refresh: bool,
) -> Result<SearchResults> {
    let state = load_index(cache_dir, refresh)?;
    let mut results = filter_index(&state.index, query, category, page)?;
    results.notice = state.notice;
    Ok(results)
}

fn load_index(cache_dir: &Path, refresh: bool) -> Result<MemoryIndex> {
    load_index_with(cache_dir, refresh, fetch_index)
}

fn load_index_with(
    cache_dir: &Path,
    refresh: bool,
    fetch: impl FnOnce(Option<&str>) -> Result<FetchOutcome>,
) -> Result<MemoryIndex> {
    static MEMORY: OnceLock<Mutex<HashMap<PathBuf, MemoryIndex>>> = OnceLock::new();
    let mut memory = MEMORY
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .map_err(|_| anyhow::anyhow!("The Frenzy gallery cache is unavailable. Restart Neko."))?;
    let cache_path = cache_dir.join("providers").join("frenzy-v1.json");
    let now = timestamp();
    let previous = memory.get(&cache_path).cloned().or_else(|| {
        read_cache(&cache_path).ok().map(|index| MemoryIndex {
            index: Arc::new(index),
            retry_after: 0,
            notice: None,
        })
    });
    if !refresh
        && let Some(state) = &previous
        && (now.saturating_sub(state.index.fetched_at) < CACHE_LIFETIME || now < state.retry_after)
    {
        memory.insert(cache_path, state.clone());
        return Ok(state.clone());
    }

    let fetched = fetch(previous.as_ref().map(|state| state.index.etag.as_str()));
    let state = match fetched {
        Ok(FetchOutcome::Fresh(mut index)) => {
            if let Some(previous) = &previous
                && previous.index.tree_sha == index.tree_sha
            {
                index.entries = previous.index.entries.clone();
            }
            let notice = save_index(&cache_path, &index);
            MemoryIndex {
                index: Arc::new(index),
                retry_after: 0,
                notice,
            }
        }
        Ok(FetchOutcome::NotModified(etag)) => {
            let Some(previous) = previous else {
                bail!("GitHub reported an unchanged gallery, but no saved catalog exists.");
            };
            let mut index = (*previous.index).clone();
            index.fetched_at = now;
            if let Some(etag) = etag {
                index.etag = etag;
            }
            let notice = save_index(&cache_path, &index);
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
                notice: Some(stale_notice(&error)),
            },
            None => {
                return Err(error.context(
                    "Could not load FrenzyExists/wallpapers. Check your connection and retry.",
                ));
            }
        },
    };
    memory.insert(cache_path, state.clone());
    Ok(state)
}

fn save_index(path: &Path, index: &Index) -> Option<String> {
    serde_json::to_vec(index)
        .map_err(anyhow::Error::from)
        .and_then(|bytes| write_atomic(path, &bytes))
        .err()
        .map(|_| {
            "Gallery loaded. Its cache could not be saved; check available disk space.".to_string()
        })
}

fn stale_notice(error: &anyhow::Error) -> String {
    let message = format!("{error:#}").to_lowercase();
    if message.contains("rate") || message.contains("too many") || message.contains("denied") {
        "Showing the saved Frenzy gallery because GitHub's rate limit was reached.".to_string()
    } else if message.contains("no longer available") || message.contains("404") {
        "Showing the saved Frenzy gallery because the upstream repository is unavailable."
            .to_string()
    } else {
        "Showing the saved Frenzy gallery. Refresh when your connection returns.".to_string()
    }
}

fn fetch_index(etag: Option<&str>) -> Result<FetchOutcome> {
    let parsed = Url::parse(TREE_URL)?;
    validate_url(&parsed)?;
    let mut request = client()?
        .get(parsed)
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28");
    if let Some(etag) = etag.filter(|value| !value.is_empty()) {
        request = request.header(reqwest::header::IF_NONE_MATCH, etag);
    }
    let response = request
        .send()
        .context("Could not reach GitHub. Check your connection, then retry.")?;
    if response.status() == reqwest::StatusCode::NOT_MODIFIED {
        let etag = response
            .headers()
            .get(reqwest::header::ETAG)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        return Ok(FetchOutcome::NotModified(etag));
    }
    let response = check_response(response)?;
    let etag = response
        .headers()
        .get(reqwest::header::ETAG)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    let bytes = response_bytes(response, MAX_TREE_BYTES)?;
    Ok(FetchOutcome::Fresh(parse_tree(&bytes, etag, timestamp())?))
}

fn parse_tree(bytes: &[u8], etag: String, fetched_at: u64) -> Result<Index> {
    let response: TreeResponse = serde_json::from_slice(bytes)
        .context("GitHub returned an unreadable Frenzy gallery listing.")?;
    ensure!(
        !response.truncated,
        "GitHub returned a partial Frenzy gallery listing. Try refreshing later."
    );
    let mut entries: Vec<_> = response
        .tree
        .into_iter()
        .filter(|item| item.kind == "blob")
        .filter_map(|item| catalog_entry(&item.path))
        .collect();
    entries.sort_by(|left, right| {
        left.category
            .to_lowercase()
            .cmp(&right.category.to_lowercase())
            .then_with(|| left.title.to_lowercase().cmp(&right.title.to_lowercase()))
            .then_with(|| left.path.cmp(&right.path))
    });
    ensure!(
        !entries.is_empty(),
        "The Frenzy gallery contains no supported still images."
    );
    Ok(Index {
        version: 1,
        fetched_at,
        etag,
        tree_sha: response.sha,
        entries,
    })
}

fn catalog_entry(path: &str) -> Option<Entry> {
    let (category, file_name) = path.split_once('/')?;
    if category.is_empty()
        || file_name.is_empty()
        || file_name.contains('/')
        || EXCLUDED_FOLDERS
            .iter()
            .any(|excluded| category.eq_ignore_ascii_case(excluded))
        || EXCLUDED_FILES
            .iter()
            .any(|excluded| file_name.eq_ignore_ascii_case(excluded))
    {
        return None;
    }
    let file = Path::new(file_name);
    let extension = file.extension()?.to_str()?;
    if !ALLOWED_EXTENSIONS
        .iter()
        .any(|allowed| extension.eq_ignore_ascii_case(allowed))
    {
        return None;
    }
    let stem = file.file_stem()?.to_string_lossy();
    let normalized_stem = stem.to_lowercase();
    if EXCLUDED_STEM_TERMS
        .iter()
        .any(|term| normalized_stem.contains(term))
    {
        return None;
    }
    Some(Entry {
        path: path.to_owned(),
        category: category.to_owned(),
        title: stem.replace(['-', '_'], " "),
    })
}

fn read_cache(path: &Path) -> Result<Index> {
    let file = fs::File::open(path)?;
    ensure!(
        file.metadata()?.len() <= MAX_CACHE_BYTES,
        "Frenzy gallery cache is too large."
    );
    let index: Index =
        serde_json::from_reader(std::io::BufReader::new(file.take(MAX_CACHE_BYTES + 1)))?;
    ensure!(
        index.version == 1 && !index.tree_sha.is_empty() && !index.entries.is_empty(),
        "Frenzy gallery cache needs a refresh."
    );
    Ok(index)
}

fn filter_index(
    index: &Index,
    query: &str,
    category: Option<&str>,
    requested_page: u32,
) -> Result<SearchResults> {
    let query = normalize(query.trim());
    let category = category.map(str::trim).filter(|value| !value.is_empty());
    let categories: Vec<_> = index
        .entries
        .iter()
        .map(|entry| entry.category.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let matches: Vec<_> = index
        .entries
        .iter()
        .filter(|entry| {
            category.is_none_or(|selected| entry.category.eq_ignore_ascii_case(selected))
                && (query.is_empty()
                    || normalize(&format!("{} {}", entry.category, entry.title)).contains(&query))
        })
        .collect();
    let total = matches.len();
    let last_page = total.div_ceil(PAGE_SIZE).max(1) as u32;
    let page = requested_page.max(1).min(last_page);
    let skip = (page as usize - 1) * PAGE_SIZE;
    let mut items = Vec::with_capacity(PAGE_SIZE);
    for entry in matches.into_iter().skip(skip).take(PAGE_SIZE) {
        let url = media_url(&entry.path)?;
        items.push(Wallpaper {
            id: format!("frenzy-{}", &hash(entry.path.as_bytes())[..20]),
            title: entry.title.clone(),
            source: Some(Provider::Frenzy),
            local_path: None,
            image_url: Some(url.clone()),
            thumbnail_url: Some(url),
            thumbnail_path: None,
            width: 0,
            height: 0,
            color: None,
            attribution: Some("FrenzyExists/wallpapers, original artist unknown".into()),
        });
    }
    Ok(SearchResults {
        items,
        page,
        last_page,
        total,
        categories,
        notice: None,
    })
}

fn normalize(value: &str) -> String {
    value.replace(['-', '_'], " ").to_lowercase()
}

fn media_url(path: &str) -> Result<String> {
    ensure!(
        !path.starts_with('/') && !path.contains('\\') && !path.contains("://"),
        "Invalid Frenzy gallery image path."
    );
    let segments: Vec<_> = path.split('/').collect();
    ensure!(
        segments
            .iter()
            .all(|segment| !segment.is_empty() && *segment != "." && *segment != ".."),
        "Invalid Frenzy gallery image path."
    );
    let mut url = Url::parse(RAW_BASE)?;
    validate_url(&url)?;
    url.path_segments_mut()
        .map_err(|_| anyhow::anyhow!("Invalid Frenzy gallery media base."))?
        .pop_if_empty()
        .extend(segments);
    Ok(url.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = r#"{
      "sha":"tree-sha-1",
      "truncated":false,
      "tree":[
        {"path":"Anime/quiet-night_wall.png","type":"blob"},
        {"path":"Aquarium/pattern1-aquarium.png","type":"blob"},
        {"path":"Aquarium/pikaWall.png","type":"blob"},
        {"path":"Blobs And Waves/soft-wave.webp","type":"blob"},
        {"path":"Nord/nord-qsave-1.png","type":"blob"},
        {"path":"Nord/nord-qsave-2.png","type":"blob"},
        {"path":"Shitpost/meme.jpg","type":"blob"},
        {"path":"Tea And Coffee/coffee-time.JPG","type":"blob"},
        {"path":"Other/photo.jpeg","type":"blob"},
        {"path":"Other/LOUD-PATTERN.jpeg","type":"blob"},
        {"path":"Other/animated.gif","type":"blob"},
        {"path":"Other/readme.txt","type":"blob"},
        {"path":"Other/nested/deep.png","type":"blob"},
        {"path":"Anime","type":"tree"}
      ]
    }"#;

    fn fixture_index() -> Index {
        parse_tree(FIXTURE.as_bytes(), "etag-1".into(), 0).unwrap()
    }

    fn write_fixture_cache(directory: &Path) -> PathBuf {
        let path = directory.join("providers/frenzy-v1.json");
        write_atomic(&path, &serde_json::to_vec(&fixture_index()).unwrap()).unwrap();
        path
    }

    #[test]
    fn parses_tree_filters_extensions_folders_and_tiles_and_derives_categories() {
        let index = fixture_index();
        let paths: Vec<_> = index
            .entries
            .iter()
            .map(|entry| entry.path.as_str())
            .collect();
        assert_eq!(
            paths,
            [
                "Anime/quiet-night_wall.png",
                "Blobs And Waves/soft-wave.webp",
                "Other/photo.jpeg",
                "Tea And Coffee/coffee-time.JPG"
            ]
        );
        let results = filter_index(&index, "", None, 1).unwrap();
        assert_eq!(
            results.categories,
            ["Anime", "Blobs And Waves", "Other", "Tea And Coffee"]
        );
        assert!(results.items.iter().all(|item| item.color.is_none()));
        assert!(results.items.iter().all(|item| {
            item.attribution.as_deref() == Some("FrenzyExists/wallpapers, original artist unknown")
        }));
    }

    #[test]
    fn search_normalizes_dashes_and_underscores_and_filters_category() {
        let index = fixture_index();
        assert_eq!(
            filter_index(&index, "quiet night", None, 1).unwrap().total,
            1
        );
        assert_eq!(
            filter_index(&index, "QUIET NIGHT", None, 1).unwrap().total,
            1
        );
        assert_eq!(
            filter_index(&index, "coffee_time", None, 1).unwrap().total,
            1
        );
        assert_eq!(
            filter_index(&index, "wave", Some("Blobs And Waves"), 1)
                .unwrap()
                .total,
            1
        );
        assert_eq!(
            filter_index(&index, "wave", Some("Anime"), 1)
                .unwrap()
                .total,
            0
        );
    }

    #[test]
    fn pagination_is_stable_and_non_overlapping() {
        let mut index = fixture_index();
        for number in 0..50 {
            index.entries.push(Entry {
                path: format!("Anime/{number:03}.png"),
                category: "Anime".into(),
                title: format!("{number:03}"),
            });
        }
        let first = filter_index(&index, "", Some("Anime"), 1).unwrap();
        let second = filter_index(&index, "", Some("Anime"), 2).unwrap();
        let last = filter_index(&index, "", Some("Anime"), u32::MAX).unwrap();
        assert_eq!(
            (first.total, first.items.len(), first.last_page),
            (51, 24, 3)
        );
        assert_eq!(last.items.len(), 3);
        assert!(
            first
                .items
                .iter()
                .all(|left| second.items.iter().all(|right| left.id != right.id))
        );
    }

    #[test]
    fn media_urls_encode_spaces_and_cannot_escape_the_allowed_host() {
        assert_eq!(
            media_url("Tea And Coffee/coffee time #1.png").unwrap(),
            "https://raw.githubusercontent.com/FrenzyExists/wallpapers/main/Tea%20And%20Coffee/coffee%20time%20%231.png"
        );
        for path in [
            "../secret.png",
            "/evil.png",
            "https://evil.example/a.png",
            "Anime/../secret.png",
            "Anime\\secret.png",
        ] {
            assert!(media_url(path).is_err(), "{path}");
        }
    }

    #[test]
    fn truncated_and_malformed_trees_are_rejected() {
        let truncated = FIXTURE.replace("\"truncated\":false", "\"truncated\":true");
        assert!(parse_tree(truncated.as_bytes(), "etag".into(), 0).is_err());
        assert!(parse_tree(br#"{"message":"rate limited"}"#, "etag".into(), 0).is_err());
    }

    #[test]
    fn etag_not_modified_reuses_and_refreshes_the_saved_catalog() {
        let directory = tempfile::tempdir().unwrap();
        let path = write_fixture_cache(directory.path());
        let state = load_index_with(directory.path(), true, |etag| {
            assert_eq!(etag, Some("etag-1"));
            Ok(FetchOutcome::NotModified(Some("etag-2".into())))
        })
        .unwrap();
        assert_eq!(state.index.entries, fixture_index().entries);
        let saved = read_cache(&path).unwrap();
        assert_eq!(saved.etag, "etag-2");
        assert!(saved.fetched_at > 0);
    }

    #[test]
    fn stale_cache_survives_rate_limit_missing_repo_and_network_errors() {
        for error in [
            "GitHub rate limit reached (HTTP 403)",
            "This collection is no longer available (HTTP 404)",
            "Could not connect to GitHub",
        ] {
            let directory = tempfile::tempdir().unwrap();
            write_fixture_cache(directory.path());
            let state = load_index_with(directory.path(), true, |_| bail!(error)).unwrap();
            assert_eq!(state.index.entries.len(), 4);
            assert!(state.notice.as_deref().unwrap().contains("saved Frenzy"));
        }
    }

    #[test]
    fn malformed_refresh_does_not_overwrite_a_valid_cache() {
        let directory = tempfile::tempdir().unwrap();
        let path = write_fixture_cache(directory.path());
        let original = fs::read(&path).unwrap();
        let state = load_index_with(directory.path(), true, |_| {
            Ok(FetchOutcome::Fresh(parse_tree(
                b"not json",
                "new-etag".into(),
                timestamp(),
            )?))
        })
        .unwrap();
        assert_eq!(state.index.entries.len(), 4);
        assert_eq!(fs::read(path).unwrap(), original);
    }
}

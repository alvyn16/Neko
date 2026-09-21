use super::{check_response, client, response_bytes};
use crate::model::{Provider, SearchResults, Wallpaper};
use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct ApiResponse {
    data: Vec<ApiWallpaper>,
    meta: Meta,
}

#[derive(Debug, Deserialize)]
struct Meta {
    current_page: u32,
    last_page: u32,
    total: usize,
}

#[derive(Debug, Deserialize)]
struct ApiWallpaper {
    id: String,
    path: String,
    purity: String,
    dimension_x: u32,
    dimension_y: u32,
    thumbs: Thumbs,
    #[serde(default)]
    colors: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct Thumbs {
    large: String,
}

pub(super) fn search(query: &str, page: u32) -> Result<SearchResults> {
    let page = page.max(1).to_string();
    let params = parameters(query, &page);
    let response = client()?
        .get("https://wallhaven.cc/api/v1/search")
        .query(&params)
        .send()
        .context("Could not reach Wallhaven. Check your connection, then retry.")?;
    let bytes = response_bytes(check_response(response)?, 2 * 1024 * 1024)?;
    parse(&bytes)
}

fn parameters<'a>(query: &'a str, page: &'a str) -> [(&'a str, &'a str); 6] {
    [
        ("q", query.trim()),
        ("page", page),
        ("purity", "100"),
        ("categories", "111"),
        (
            "sorting",
            if query.trim().is_empty() {
                "toplist"
            } else {
                "relevance"
            },
        ),
        ("order", "desc"),
    ]
}

fn parse(bytes: &[u8]) -> Result<SearchResults> {
    let response: ApiResponse = serde_json::from_slice(bytes)
        .context("Wallhaven returned an unreadable response. Try again later.")?;
    let mut items = Vec::with_capacity(response.data.len());
    for entry in response.data {
        // Retain a second purity check even though the request is already SFW-only.
        if entry.purity != "sfw" {
            continue;
        }
        if super::validate_url(&url::Url::parse(&entry.path)?).is_err()
            || super::validate_url(&url::Url::parse(&entry.thumbs.large)?).is_err()
        {
            continue;
        }
        items.push(Wallpaper {
            title: format!("Wallhaven · {}", entry.id),
            id: format!("wallhaven-{}", entry.id),
            source: Some(Provider::Wallhaven),
            local_path: None,
            image_url: Some(entry.path),
            thumbnail_url: Some(entry.thumbs.large),
            thumbnail_path: None,
            width: entry.dimension_x,
            height: entry.dimension_y,
            color: entry.colors.first().cloned(),
            attribution: Some("Wallhaven".into()),
        });
    }
    Ok(SearchResults {
        items,
        page: response.meta.current_page.max(1),
        last_page: response.meta.last_page.max(1),
        total: response.meta.total,
        categories: vec![],
        notice: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn search_always_requests_sfw_and_preserves_query() {
        let params = parameters("  mountain & forest  ", "3");
        assert!(params.contains(&("purity", "100")));
        assert!(params.contains(&("q", "mountain & forest")));
        assert!(params.contains(&("page", "3")));
    }
    #[test]
    fn parses_pagination_and_defensively_excludes_non_sfw() {
        let bytes = br##"{"data":[{"id":"abc123","path":"https://w.wallhaven.cc/full/ab/abc123.jpg","purity":"sfw","dimension_x":1920,"dimension_y":1080,"thumbs":{"large":"https://th.wallhaven.cc/lg/ab/abc123.jpg"},"colors":["#ff8800"]},{"id":"unsafe","path":"https://w.wallhaven.cc/a.jpg","purity":"sketchy","dimension_x":1,"dimension_y":1,"thumbs":{"large":"https://th.wallhaven.cc/a.jpg"}}],"meta":{"current_page":2,"last_page":10,"total":230}}"##;
        let result = parse(bytes).unwrap();
        assert_eq!(result.items.len(), 1);
        assert_eq!((result.page, result.last_page, result.total), (2, 10, 230));
        assert_eq!(result.items[0].width, 1920);
        assert_eq!(result.items[0].color.as_deref(), Some("#ff8800"));
    }
    #[test]
    fn malformed_response_is_reported() {
        assert!(parse(br#"{"error":"unavailable"}"#).is_err());
    }
}

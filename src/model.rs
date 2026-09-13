use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Tab {
    #[default]
    Local,
    Search,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    #[default]
    Wallhaven,
    Bjarneo,
}

#[derive(Debug, Clone)]
pub struct Wallpaper {
    pub id: String,
    pub title: String,
    pub source: Option<Provider>,
    pub local_path: Option<PathBuf>,
    pub image_url: Option<String>,
    pub thumbnail_url: Option<String>,
    pub thumbnail_path: Option<PathBuf>,
    pub width: u32,
    pub height: u32,
    pub color: Option<String>,
    pub attribution: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct SearchResults {
    pub items: Vec<Wallpaper>,
    pub page: u32,
    pub last_page: u32,
    pub total: usize,
    pub notice: Option<String>,
}

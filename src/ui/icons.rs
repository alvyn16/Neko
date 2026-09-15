use gpui::{AssetSource, SharedString, Svg, prelude::*, px, svg};
use std::borrow::Cow;

pub struct Icons;
impl AssetSource for Icons {
    fn load(&self, path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
        match path {
            // The symbolic artwork is used by the compact title bar icon. GPUI applies the
            // element's text color to the SVG mask, so `currentColor` stays theme-aware.
            "cat" | "neko-symbolic" => {
                return Ok(Some(Cow::Borrowed(include_bytes!(
                    "../../assets/neko-icon-symbolic.svg",
                ))));
            }
            // Keep the full-color source available to consumers that need the larger artwork.
            "neko-color" => {
                return Ok(Some(Cow::Borrowed(include_bytes!(
                    "../../assets/neko-icon.svg",
                ))));
            }
            _ => {}
        }
        let body = match path {
            "folder" => {
                r#"<path d="M3 7V5a2 2 0 0 1 2-2h5l2 3h7a2 2 0 0 1 2 2v11a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2Z"/>"#
            }
            "search" => r#"<circle cx="10.5" cy="10.5" r="6.5"/><path d="m16 16 5 5"/>"#,
            "close" => r#"<path d="m6 6 12 12M6 18 18 6"/>"#,
            "minus" => r#"<path d="M5 12h14"/>"#,
            "refresh" => {
                r#"<path d="M20 7v5h-5M4 17v-5h5M19 11a7 7 0 0 0-12-6l-3 3M5 13a7 7 0 0 0 12 6l3-3"/>"#
            }
            "monitor" => {
                r#"<rect x="3" y="3" width="18" height="13" rx="2"/><path d="M12 16v5M8 21h8"/>"#
            }
            "download" => {
                r#"<path d="M12 3v12m-5-5 5 5 5-5M4 16v4a1 1 0 0 0 1 1h14a1 1 0 0 0 1-1v-4"/>"#
            }
            "edit" => r#"<path d="m15 4 5 5M4 20l5-1L21 7a2 2 0 0 0-5-5L4 14Z"/>"#,
            "trash" => r#"<path d="M3 6h18M9 6V3h6v3M5 6l1 15h12l1-15M10 10v7M14 10v7"/>"#,
            "arrow" => r#"<path d="M4 12h16m-6-6 6 6-6 6"/>"#,
            "check" => r#"<path d="m5 12 4 4L19 6"/>"#,
            "image" => {
                r#"<rect x="3" y="3" width="18" height="18" rx="3"/><circle cx="8" cy="8" r="1"/><path d="m3 17 6-6 4 4 3-3 5 5"/>"#
            }
            "globe" => {
                r#"<circle cx="12" cy="12" r="9"/><ellipse cx="12" cy="12" rx="4" ry="9"/><path d="M3 12h18"/>"#
            }
            "info" => r#"<circle cx="12" cy="12" r="9"/><path d="M12 11v6M12 7h.01"/>"#,
            "settings" => {
                r#"<circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.7 1.7 0 0 0 .34 1.88l.06.06-2.83 2.83-.06-.06a1.7 1.7 0 0 0-1.88-.34 1.7 1.7 0 0 0-1.03 1.56V21h-4v-.08A1.7 1.7 0 0 0 8.94 19.4a1.7 1.7 0 0 0-1.88.34l-.06.06-2.83-2.83.06-.06A1.7 1.7 0 0 0 4.6 15a1.7 1.7 0 0 0-1.53-1H3v-4h.08A1.7 1.7 0 0 0 4.6 8.94a1.7 1.7 0 0 0-.34-1.88L4.2 7l2.83-2.83.06.06A1.7 1.7 0 0 0 8.97 4.6 1.7 1.7 0 0 0 10 3.08V3h4v.08a1.7 1.7 0 0 0 1.06 1.53 1.7 1.7 0 0 0 1.88-.34L17 4.2 19.83 7l-.06.06a1.7 1.7 0 0 0-.34 1.88A1.7 1.7 0 0 0 20.92 10H21v4h-.08A1.7 1.7 0 0 0 19.4 15Z"/>"#
            }
            _ => return Ok(None),
        };
        Ok(Some(Cow::Owned(format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="white" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round">{body}</svg>"#).into_bytes())))
    }
    fn list(&self, _: &str) -> anyhow::Result<Vec<SharedString>> {
        Ok(vec![])
    }
}
pub fn icon(name: &'static str) -> Svg {
    svg().path(name).size(px(16.0)).flex_none()
}

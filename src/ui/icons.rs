use gpui::{AssetSource, SharedString, Svg, prelude::*, px, rgb, svg};
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
            "refresh" => r#"<path d="M20 12a8 8 0 1 1-2.34-5.66L20 8"/><path d="M20 3v5h-5"/>"#,
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
                r#"<path d="M4 7h16M4 12h16M4 17h16"/><circle cx="9" cy="7" r="2"/><circle cx="15" cy="12" r="2"/><circle cx="11" cy="17" r="2"/>"#
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
    // Svg paints only when its own computed style has a text color. A color on the
    // parent container is not inherited by this GPUI element.
    svg()
        .path(name)
        .size(px(16.0))
        .flex_none()
        .text_color(rgb(0xffffff))
}

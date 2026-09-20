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
                r#"<path fill="white" stroke="none" d="M19.43 12.98c.04-.32.07-.65.07-.98s-.03-.66-.07-.98l2.11-1.65a.5.5 0 0 0 .12-.64l-2-3.46a.5.5 0 0 0-.61-.22l-2.49 1a7.4 7.4 0 0 0-1.69-.98l-.38-2.65A.49.49 0 0 0 14 2h-4a.49.49 0 0 0-.49.42l-.38 2.65c-.61.25-1.18.58-1.69.98l-2.49-1a.5.5 0 0 0-.61.22l-2 3.46a.5.5 0 0 0 .12.64l2.11 1.65c-.04.32-.07.65-.07.98s.03.66.07.98l-2.11 1.65a.5.5 0 0 0-.12.64l2 3.46c.12.22.38.31.61.22l2.49-1c.51.4 1.08.73 1.69.98l.38 2.65c.04.24.24.42.49.42h4c.25 0 .45-.18.49-.42l.38-2.65c.61-.25 1.18-.58 1.69-.98l2.49 1c.23.09.49 0 .61-.22l2-3.46a.5.5 0 0 0-.12-.64l-2.11-1.65ZM12 15.5a3.5 3.5 0 1 1 0-7 3.5 3.5 0 0 1 0 7Z"/>"#
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

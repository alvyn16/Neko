use crate::config::WallpaperFit;
use anyhow::{Context, Result};
use std::{
    fs,
    path::{Path, PathBuf},
};

fn style_values(fit: WallpaperFit) -> (&'static str, &'static str) {
    match fit {
        WallpaperFit::Fill => ("10", "0"),
        WallpaperFit::Fit => ("6", "0"),
        WallpaperFit::Stretch => ("2", "0"),
        WallpaperFit::Center => ("0", "0"),
        WallpaperFit::Tile => ("0", "1"),
    }
}

#[cfg(windows)]
fn set_fit(fit: WallpaperFit) -> Result<()> {
    use winreg::{RegKey, enums::HKEY_CURRENT_USER};
    let desktop = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags("Control Panel\\Desktop", winreg::enums::KEY_SET_VALUE)
        .context("Could not open Windows wallpaper settings")?;
    let (style, tile) = style_values(fit);
    desktop
        .set_value("WallpaperStyle", &style)
        .context("Could not set the Windows wallpaper fit mode")?;
    desktop
        .set_value("TileWallpaper", &tile)
        .context("Could not set the Windows wallpaper tile mode")?;
    Ok(())
}

/// Keep the applied image in persistent cache so moving/deleting the source cannot
/// break the Windows desktop after a restart. BMP also handles WebP/GIF/TIFF input.
fn prepare(path: &Path, cache_dir: &Path) -> Result<PathBuf> {
    let directory = cache_dir.join("applied");
    fs::create_dir_all(&directory).context("Could not create the wallpaper cache")?;
    let output = directory.join(format!("{}.bmp", crate::local::cache_key(path)?));
    if !output.is_file() || image::image_dimensions(&output).is_err() {
        let image = crate::local::read_image(path)?.into_rgb8();
        let mut encoded = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(image).write_to(&mut encoded, image::ImageFormat::Bmp)?;
        crate::local::atomic_cache_write(&output, encoded.get_ref())?;
    }
    std::path::absolute(output).context("Could not resolve the wallpaper path")
}

#[cfg(windows)]
pub fn apply(path: &Path, cache_dir: &Path, fit: WallpaperFit) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::UI::WindowsAndMessaging::{
        SPI_SETDESKWALLPAPER, SPIF_SENDCHANGE, SPIF_UPDATEINIFILE, SystemParametersInfoW,
    };
    let prepared = prepare(path, cache_dir)?;
    set_fit(fit)?;
    let mut wide: Vec<u16> = prepared.as_os_str().encode_wide().chain(Some(0)).collect();
    // The owned, nul-terminated path stays alive until the synchronous API returns.
    unsafe {
        SystemParametersInfoW(
            SPI_SETDESKWALLPAPER,
            0,
            Some(wide.as_mut_ptr().cast()),
            SPIF_UPDATEINIFILE | SPIF_SENDCHANGE,
        )
    }
    .context("Windows could not apply this wallpaper")
}

#[cfg(not(windows))]
pub fn apply(_path: &Path, _cache_dir: &Path, _fit: WallpaperFit) -> Result<()> {
    anyhow::bail!("Applying wallpapers is available on Windows")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prepare_converts_and_keeps_a_stable_persistent_copy() {
        let source = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        let image = source.path().join("猫 picture.png");
        image::RgbaImage::from_pixel(30, 20, image::Rgba([120, 80, 20, 255]))
            .save(&image)
            .unwrap();
        let prepared = prepare(&image, cache.path()).unwrap();
        assert!(prepared.is_absolute());
        assert_eq!(prepared.extension().unwrap(), "bmp");
        assert_eq!(image::image_dimensions(&prepared).unwrap(), (30, 20));
        assert_eq!(prepared, prepare(&image, cache.path()).unwrap());
        fs::remove_file(image).unwrap();
        assert!(prepared.is_file());
    }

    #[test]
    fn fit_modes_map_to_windows_desktop_values() {
        assert_eq!(style_values(WallpaperFit::Fill), ("10", "0"));
        assert_eq!(style_values(WallpaperFit::Fit), ("6", "0"));
        assert_eq!(style_values(WallpaperFit::Stretch), ("2", "0"));
        assert_eq!(style_values(WallpaperFit::Center), ("0", "0"));
        assert_eq!(style_values(WallpaperFit::Tile), ("0", "1"));
    }
}

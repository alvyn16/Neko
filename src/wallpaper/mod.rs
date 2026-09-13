use anyhow::{Context, Result};
use std::{
    fs,
    path::{Path, PathBuf},
};

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
pub fn apply(path: &Path, cache_dir: &Path) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::UI::WindowsAndMessaging::{
        SPI_SETDESKWALLPAPER, SPIF_SENDCHANGE, SPIF_UPDATEINIFILE, SystemParametersInfoW,
    };
    let prepared = prepare(path, cache_dir)?;
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
pub fn apply(_path: &Path, _cache_dir: &Path) -> Result<()> {
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
}

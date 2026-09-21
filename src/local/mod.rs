use crate::model::Wallpaper;
use anyhow::{Context, Result, bail, ensure};
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, Limits};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{BufReader, Write},
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

const MAX_FILE_BYTES: u64 = 100 * 1024 * 1024;
const MAX_PIXELS: u64 = 80_000_000;
const MAX_DECODED_BYTES: u64 = 512 * 1024 * 1024;
const MAX_DIMENSION: u32 = 16_384;
const THUMBNAIL_SIZE: u32 = 720;
const PREVIEW_SIZE: u32 = 1_280;

fn supported_extension(path: &Path) -> bool {
    path.extension().and_then(|s| s.to_str()).is_some_and(|s| {
        matches!(
            s.to_ascii_lowercase().as_str(),
            "jpg" | "jpeg" | "png" | "webp" | "bmp" | "gif" | "tif" | "tiff"
        )
    })
}

fn reader(path: &Path) -> Result<ImageReader<BufReader<fs::File>>> {
    let file =
        fs::File::open(path).with_context(|| format!("Could not open {}", path.display()))?;
    let metadata = file.metadata()?;
    ensure!(metadata.is_file(), "Please choose an image file");
    ensure!(
        metadata.len() <= MAX_FILE_BYTES,
        "This image is larger than the 100 MB limit"
    );
    let mut reader = ImageReader::new(BufReader::new(file)).with_guessed_format()?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_DIMENSION);
    limits.max_image_height = Some(MAX_DIMENSION);
    limits.max_alloc = Some(MAX_DECODED_BYTES);
    reader.limits(limits);
    Ok(reader)
}

fn check_dimensions(width: u32, height: u32) -> Result<()> {
    ensure!(width > 0 && height > 0, "This image has invalid dimensions");
    ensure!(
        width <= MAX_DIMENSION
            && height <= MAX_DIMENSION
            && u64::from(width) * u64::from(height) <= MAX_PIXELS,
        "This image is too large to open safely (maximum 80 megapixels and 16,384 pixels per side)"
    );
    Ok(())
}

pub fn image_dimensions(path: &Path) -> Result<(u32, u32)> {
    let decoder = reader(path)?
        .into_decoder()
        .context("This file is not a supported image")?;
    let (width, height) = decoder.dimensions();
    check_dimensions(width, height)?;
    ensure!(
        decoder.total_bytes() <= MAX_DECODED_BYTES,
        "This image needs too much memory to open"
    );
    Ok((width, height))
}

/// Decode with explicit pixel and output-buffer limits before allocating the image.
pub fn read_image(path: &Path) -> Result<DynamicImage> {
    let mut decoder = reader(path)?
        .into_decoder()
        .context("This file is not a supported image")?;
    let (width, height) = decoder.dimensions();
    check_dimensions(width, height)?;
    ensure!(
        decoder.total_bytes() <= MAX_DECODED_BYTES,
        "This image needs too much memory to open"
    );
    let orientation = decoder
        .orientation()
        .context("Could not read image orientation")?;
    let mut image = DynamicImage::from_decoder(decoder).context("Could not decode this image")?;
    image.apply_orientation(orientation);
    Ok(image)
}

pub(crate) fn cache_key(path: &Path) -> Result<String> {
    let path = fs::canonicalize(path)?;
    let metadata = fs::metadata(&path)?;
    let modified = metadata
        .modified()?
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let mut digest = Sha256::new();
    digest.update(b"neko-image-v2\0");
    digest.update(path.as_os_str().as_encoded_bytes());
    digest.update(metadata.len().to_le_bytes());
    digest.update(modified.to_le_bytes());
    Ok(format!("{:x}", digest.finalize()))
}

fn cached_preview(
    path: &Path,
    cache_dir: &Path,
    directory_name: &str,
    size: u32,
    quality: u8,
) -> Result<PathBuf> {
    let directory = cache_dir.join(directory_name);
    fs::create_dir_all(&directory).context("Could not create thumbnail cache")?;
    let output = directory.join(format!("{}.jpg", cache_key(path)?));
    if output.is_file()
        && image::image_dimensions(&output).is_ok_and(|(w, h)| w <= size && h <= size)
    {
        return Ok(output);
    }
    let source = read_image(path)?;
    let image = if source.width() > size || source.height() > size {
        source.thumbnail(size, size)
    } else {
        source
    }
    .to_rgb8();
    let mut encoded = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut encoded, quality)
        .encode_image(&image)?;
    atomic_cache_write(&output, &encoded)?;
    Ok(output)
}

pub fn thumbnail(path: &Path, cache_dir: &Path) -> Result<PathBuf> {
    cached_preview(path, cache_dir, "thumbnails-v2", THUMBNAIL_SIZE, 92)
}

pub fn preview(path: &Path, cache_dir: &Path) -> Result<PathBuf> {
    cached_preview(path, cache_dir, "previews-v2", PREVIEW_SIZE, 92)
}

pub(crate) fn atomic_cache_write(output: &Path, contents: &[u8]) -> Result<()> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SERIAL: AtomicU64 = AtomicU64::new(0);
    let temporary = output.with_extension(format!(
        "{}-{}.tmp",
        std::process::id(),
        SERIAL.fetch_add(1, Ordering::Relaxed)
    ));
    let result = (|| -> Result<()> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(contents)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, output)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.context("Could not save image cache")
}

/// Scan only the selected directory. Broken, unsupported and oversized files are skipped.
pub fn image_paths(folder: &Path) -> Result<Vec<PathBuf>> {
    let entries = fs::read_dir(folder)
        .with_context(|| format!("Could not read wallpaper folder {}", folder.display()))?;
    let mut paths: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            (entry.file_type().ok()?.is_file() && supported_extension(&entry.path()))
                .then(|| entry.path())
        })
        .collect();
    paths.sort_by_cached_key(|path| {
        (
            path.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_lowercase(),
            path.clone(),
        )
    });
    Ok(paths)
}

/// Scan only the selected directory. Broken, unsupported and oversized files are skipped.
pub fn scan(folder: &Path, cache_dir: &Path) -> Result<Vec<Wallpaper>> {
    let paths = image_paths(folder)?;
    let mut items = Vec::with_capacity(paths.len());
    for path in paths {
        let Ok((width, height)) = image_dimensions(&path) else {
            continue;
        };
        let Ok(thumbnail_path) = thumbnail(&path, cache_dir) else {
            continue;
        };
        let Ok(id) = cache_key(&path) else { continue };
        items.push(Wallpaper {
            id: format!("local-{id}"),
            title: path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            source: None,
            local_path: Some(path),
            image_url: None,
            thumbnail_url: None,
            thumbnail_path: Some(thumbnail_path),
            width,
            height,
            color: None,
            attribution: None,
        });
    }
    Ok(items)
}

fn validate_filename(name: &str) -> Result<()> {
    ensure!(
        !name.is_empty() && name != "." && name != "..",
        "Please enter a file name"
    );
    ensure!(
        !name
            .chars()
            .any(|c| c.is_control()
                || matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*')),
        "File names cannot contain < > : \" / \\ | ? * or control characters"
    );
    ensure!(
        !name.ends_with([' ', '.']),
        "File names cannot end with a space or period"
    );
    ensure!(
        name.encode_utf16().count() <= 255,
        "This file name is too long"
    );
    let base = name
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end()
        .to_uppercase();
    let numbered_device = ["COM", "LPT"].iter().any(|prefix| {
        base.strip_prefix(prefix).is_some_and(|suffix| {
            matches!(
                suffix,
                "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
            )
        })
    });
    ensure!(
        !matches!(
            base.as_str(),
            "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
        ) && !numbered_device,
        "This name is reserved by Windows; please choose another"
    );
    Ok(())
}

fn ensure_image_file(path: &Path) -> Result<()> {
    ensure!(
        fs::symlink_metadata(path)?.file_type().is_file(),
        "Please choose a regular image file"
    );
    ensure!(supported_extension(path), "This file type is not supported");
    Ok(())
}

pub fn rename(path: &Path, new_name: &str) -> Result<PathBuf> {
    ensure_image_file(path)?;
    validate_filename(new_name)?;
    let extension = path
        .extension()
        .context("This image has no file extension")?
        .to_string_lossy();
    let candidate = Path::new(new_name);
    let stem = if supported_extension(candidate) {
        candidate
            .file_stem()
            .unwrap()
            .to_string_lossy()
            .into_owned()
    } else {
        new_name.to_owned()
    };
    let filename = format!("{stem}.{extension}");
    validate_filename(&filename)?;
    let output = path.with_file_name(filename);
    if output == path {
        return Ok(output);
    }
    rename_without_overwrite(path, &output).with_context(|| {
        format!(
            "Could not rename to {}. A file with that name may already exist.",
            output.file_name().unwrap_or_default().to_string_lossy()
        )
    })?;
    Ok(output)
}

#[cfg(windows)]
fn rename_without_overwrite(from: &Path, to: &Path) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::{Win32::Storage::FileSystem::MoveFileW, core::PCWSTR};
    let from: Vec<u16> = from.as_os_str().encode_wide().chain(Some(0)).collect();
    let to: Vec<u16> = to.as_os_str().encode_wide().chain(Some(0)).collect();
    unsafe { MoveFileW(PCWSTR(from.as_ptr()), PCWSTR(to.as_ptr())) }
        .context("Windows could not rename the file")
}

#[cfg(not(windows))]
fn rename_without_overwrite(from: &Path, to: &Path) -> Result<()> {
    // Creating the link fails atomically if the destination exists.
    fs::hard_link(from, to)?;
    if let Err(error) = fs::remove_file(from) {
        let _ = fs::remove_file(to);
        return Err(error.into());
    }
    Ok(())
}

pub fn delete(path: &Path) -> Result<()> {
    ensure_image_file(path)?;
    trash::delete(path).context("Could not move this image to the Recycle Bin")
}

fn downloaded_filename(download: &Path, name: &str) -> Result<(String, &'static str)> {
    let format = reader(download)?
        .format()
        .context("The download is not a supported image")?;
    let extension = match format {
        ImageFormat::Png => "png",
        ImageFormat::Jpeg => "jpg",
        ImageFormat::WebP => "webp",
        ImageFormat::Bmp => "bmp",
        ImageFormat::Gif => "gif",
        ImageFormat::Tiff => "tiff",
        _ => bail!("The download is not a supported image"),
    };
    let name = name.trim();
    let stem = if supported_extension(Path::new(name)) {
        Path::new(name)
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned()
    } else {
        name.to_owned()
    };
    let mut stem: String = stem
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') {
                '_'
            } else {
                c
            }
        })
        .take(120)
        .collect();
    stem = stem.trim().trim_end_matches('.').to_owned();
    if stem.is_empty() {
        stem = "wallpaper".to_owned();
    }
    if validate_filename(&stem).is_err() {
        stem = format!("wallpaper-{stem}");
    }
    Ok((stem, extension))
}

pub fn save_download(download: &Path, folder: &Path, name: &str) -> Result<PathBuf> {
    ensure!(
        folder.is_dir(),
        "Choose an existing wallpaper folder before saving"
    );
    // Validate the whole file before leaving a downloaded image in the user's library.
    let _ = read_image(download)?;
    let (stem, extension) = downloaded_filename(download, name)?;
    for suffix in 0..10_000 {
        let filename = if suffix == 0 {
            format!("{stem}.{extension}")
        } else {
            format!("{stem} ({suffix}).{extension}")
        };
        let output = folder.join(filename);
        let mut destination = match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&output)
        {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(error).context("Could not save image to your wallpaper folder");
            }
        };
        let result = (|| -> Result<()> {
            let mut source = fs::File::open(download)?;
            std::io::copy(&mut source, &mut destination)?;
            destination.sync_all()?;
            Ok(())
        })();
        drop(destination);
        if let Err(error) = result {
            let _ = fs::remove_file(&output);
            return Err(error).context("The image could not be saved completely");
        }
        return Ok(output);
    }
    bail!("Too many images already have that name; please rename some of them")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImportSummary {
    pub imported: usize,
    pub skipped: usize,
}

pub fn import_files(paths: &[PathBuf], folder: &Path) -> Result<ImportSummary> {
    ensure!(
        folder.is_dir(),
        "Choose an existing wallpaper folder before importing"
    );
    let mut summary = ImportSummary {
        imported: 0,
        skipped: 0,
    };
    for path in paths {
        if !path.is_file() || !supported_extension(path) {
            summary.skipped += 1;
            continue;
        }
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        match save_download(path, folder, &name) {
            Ok(_) => summary.imported += 1,
            Err(_) => summary.skipped += 1,
        }
    }
    ensure!(
        summary.imported > 0,
        "Drop JPG, PNG, WebP, BMP, GIF, or TIFF image files"
    );
    Ok(summary)
}

pub fn show_in_folder(path: &Path) -> Result<()> {
    ensure!(path.is_file(), "This image no longer exists");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        std::process::Command::new("explorer.exe")
            .arg("/select,")
            .arg(std::path::absolute(path)?)
            .creation_flags(0x08000000)
            .spawn()
            .context("Could not open File Explorer")?;
        Ok(())
    }
    #[cfg(not(windows))]
    {
        bail!("Show in folder is available on Windows")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_image(path: &Path, width: u32, height: u32) {
        image::RgbImage::from_pixel(width, height, image::Rgb([35, 80, 130]))
            .save(path)
            .unwrap();
    }

    #[test]
    fn scan_sorted_valid_images_only_and_cache_small_thumbnails() {
        let directory = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        write_image(&directory.path().join("zebra.png"), 1280, 720);
        write_image(&directory.path().join("Aurora.jpg"), 800, 1200);
        fs::write(directory.path().join("broken.png"), b"not a picture").unwrap();
        fs::write(directory.path().join("notes.txt"), b"notes").unwrap();
        fs::create_dir(directory.path().join("nested.png")).unwrap();
        let images = scan(directory.path(), cache.path()).unwrap();
        assert_eq!(images.len(), 2);
        assert_eq!(images[0].title, "Aurora");
        assert_eq!((images[1].width, images[1].height), (1280, 720));
        for item in &images {
            let thumbnail = item.thumbnail_path.as_ref().unwrap();
            let (w, h) = image::image_dimensions(thumbnail).unwrap();
            assert!(w <= THUMBNAIL_SIZE && h <= THUMBNAIL_SIZE);
            assert_ne!(thumbnail, item.local_path.as_ref().unwrap());
        }
        let again = scan(directory.path(), cache.path()).unwrap();
        assert_eq!(images[0].thumbnail_path, again[0].thumbnail_path);
    }

    #[test]
    fn renaming_preserves_extension_and_rejects_windows_hazards_and_collisions() {
        let directory = tempfile::tempdir().unwrap();
        let original = directory.path().join("first.png");
        write_image(&original, 20, 10);
        for name in [
            "../outside",
            "bad:name",
            "CON",
            "NUL.txt",
            "LPT1",
            "COM¹",
            "trailing.",
            "trailing ",
            "",
            ".",
            "..",
        ] {
            assert!(rename(&original, name).is_err(), "{name:?}");
            assert!(original.exists());
        }
        let target = directory.path().join("second.png");
        write_image(&target, 40, 30);
        assert!(rename(&original, "second").is_err());
        assert_eq!(image::image_dimensions(&target).unwrap(), (40, 30));
        let renamed = rename(&original, "new name.jpg").unwrap();
        assert_eq!(renamed.file_name().unwrap(), "new name.png");
        assert!(!original.exists());
        assert_eq!(image::image_dimensions(&renamed).unwrap(), (20, 10));
    }

    #[test]
    fn downloads_use_actual_image_format_and_never_overwrite() {
        let directory = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        let download = source.path().join("download.png");
        write_image(&download, 20, 10);
        let first = save_download(&download, directory.path(), "../Aurora.jpg").unwrap();
        let second = save_download(&download, directory.path(), "../Aurora.jpg").unwrap();
        assert_eq!(first.parent(), Some(directory.path()));
        assert_eq!(first.extension().unwrap(), "png");
        assert_ne!(first, second);
        assert_eq!(fs::read(first).unwrap(), fs::read(second).unwrap());
        fs::write(&download, "HTML error page").unwrap();
        assert!(save_download(&download, directory.path(), "broken").is_err());
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 2);
    }

    #[test]
    fn importing_files_copies_supported_images_and_skips_other_files() {
        let source = tempfile::tempdir().unwrap();
        let destination = tempfile::tempdir().unwrap();
        let image = source.path().join("wallpaper.png");
        image::RgbaImage::from_pixel(4, 4, image::Rgba([10, 20, 30, 255]))
            .save(&image)
            .unwrap();
        let text = source.path().join("notes.txt");
        fs::write(&text, "not an image").unwrap();

        let summary = import_files(&[image, text], destination.path()).unwrap();
        assert_eq!(summary.imported, 1);
        assert_eq!(summary.skipped, 1);
        assert!(destination.path().join("wallpaper.png").is_file());
    }

    #[test]
    fn reject_dimensions_that_would_exhaust_memory() {
        assert!(check_dimensions(7680, 4320).is_ok());
        assert!(check_dimensions(10_667, 6000).is_ok());
        assert!(check_dimensions(0, 100).is_err());
        assert!(check_dimensions(16_384, 16_384).is_err());
        assert!(check_dimensions(100_000, 1).is_err());
    }

    #[test]
    fn bounded_reader_rejects_a_large_header_and_an_oversized_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("oversized.bmp");
        let mut header = vec![0_u8; 54];
        header[0..2].copy_from_slice(b"BM");
        header[2..6].copy_from_slice(&54_u32.to_le_bytes());
        header[10..14].copy_from_slice(&54_u32.to_le_bytes());
        header[14..18].copy_from_slice(&40_u32.to_le_bytes());
        header[18..22].copy_from_slice(&30_000_i32.to_le_bytes());
        header[22..26].copy_from_slice(&30_000_i32.to_le_bytes());
        header[26..28].copy_from_slice(&1_u16.to_le_bytes());
        header[28..30].copy_from_slice(&24_u16.to_le_bytes());
        fs::write(&path, header).unwrap();
        assert!(read_image(&path).is_err());
        assert!(image_dimensions(&path).is_err());

        let oversized = fs::File::create(&path).unwrap();
        oversized.set_len(MAX_FILE_BYTES + 1).unwrap();
        drop(oversized);
        let error = read_image(&path).unwrap_err();
        assert!(error.to_string().contains("100 MB"));
    }

    #[test]
    fn thumbnails_refresh_after_source_changes_and_recover_from_cache_corruption() {
        let directory = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        let path = directory.path().join("landscape.png");
        write_image(&path, 800, 400);
        let first = thumbnail(&path, cache.path()).unwrap();
        fs::write(&first, b"interrupted cache write").unwrap();
        assert_eq!(first, thumbnail(&path, cache.path()).unwrap());
        assert_eq!(image::image_dimensions(&first).unwrap(), (720, 360));
        write_image(&path, 400, 800);
        let changed = thumbnail(&path, cache.path()).unwrap();
        assert_ne!(first, changed);
        assert_eq!(image::image_dimensions(&changed).unwrap(), (360, 720));
    }

    #[test]
    fn full_preview_is_sharper_than_the_grid_thumbnail() {
        let directory = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        let path = directory.path().join("large.png");
        write_image(&path, 3840, 2160);

        let thumbnail = thumbnail(&path, cache.path()).unwrap();
        let full_preview = preview(&path, cache.path()).unwrap();

        assert_eq!(image::image_dimensions(thumbnail).unwrap(), (720, 405));
        assert_eq!(image::image_dimensions(full_preview).unwrap(), (1280, 720));
    }
}

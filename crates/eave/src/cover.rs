//! The host side of [`eave_api::ui::cover`]: pictures a program hands over at runtime, like a
//! track's cover art ([D-57](../../../docs/decisions.md)). A `file://` URL is read, an `https://`
//! one is downloaded, both on tokio's blocking pool and with a size cap. PNG and JPEG are decoded,
//! cropped to a square from the middle, shrunk by averaging and given rounded corners, twice: at
//! icon size and at [`theme::COVER_SIZE`]. Only those small bitmaps are kept, by URL.

use std::io::Read;
use std::sync::Arc;
use std::time::Duration;

use iced::Element;
use iced::widget::image;
use resvg::tiny_skia::Pixmap;

use crate::apps::Picture;
use crate::theme;

/// The largest file read or download taken: covers are rarely over a megabyte.
const MAX_BYTES: u64 = 8 * 1024 * 1024;
/// How long a download may take, all told.
const TIMEOUT: Duration = Duration::from_secs(10);
/// The largest picture decoded, either way, so a hostile file can't take all the memory.
const MAX_SIDE: usize = 4096;

/// A cover, drawn at both sizes.
#[derive(Debug, Clone)]
pub struct Cover {
    small: image::Handle,
    large: image::Handle,
}

impl Cover {
    /// At icon size, or [`theme::COVER_SIZE`] when `large`, faded to `opacity`.
    pub fn view<'a, M: 'a>(&self, large: bool, opacity: f32) -> Element<'a, M> {
        let (handle, size) = if large {
            (self.large.clone(), theme::COVER_SIZE)
        } else {
            (self.small.clone(), theme::ICON_SIZE)
        };
        Element::new(Picture {
            handle,
            opacity,
            size,
        })
    }
}

/// Fetches and draws the cover at `url`, off the UI thread. `None` when it can't be had.
pub async fn load(url: String) -> Option<Cover> {
    tokio::task::spawn_blocking(move || {
        let cover = fetch(&url).and_then(|bytes| draw(&bytes));
        if cover.is_none() {
            eprintln!("eave: no cover from {url}");
        }
        cover
    })
    .await
    .ok()
    .flatten()
}

fn fetch(url: &str) -> Option<Vec<u8>> {
    if let Some(path) = url.strip_prefix("file://") {
        let file = std::fs::File::open(percent_decode(path)?).ok()?;
        let mut bytes = Vec::new();
        file.take(MAX_BYTES).read_to_end(&mut bytes).ok()?;
        Some(bytes)
    } else if url.starts_with("https://") {
        download(url)
    } else {
        None
    }
}

/// A new agent for each download, dropped after it: covers change a few times an hour, and a
/// kept agent would hold the TLS setup and the system's certificates in memory all the while.
fn agent() -> ureq::Agent {
    let tls = ureq::tls::TlsConfig::builder()
        .provider(ureq::tls::TlsProvider::Rustls)
        .unversioned_rustls_crypto_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .root_certs(ureq::tls::RootCerts::PlatformVerifier)
        .build();
    ureq::Agent::config_builder()
        .tls_config(tls)
        .https_only(true)
        .timeout_global(Some(TIMEOUT))
        .build()
        .new_agent()
}

fn download(url: &str) -> Option<Vec<u8>> {
    let mut response = agent()
        .get(url)
        .call()
        .map_err(|error| eprintln!("eave: {url}: {error}"))
        .ok()?;
    response
        .body_mut()
        .with_config()
        .limit(MAX_BYTES)
        .read_to_vec()
        .ok()
}

/// `%20` and the like, as file URLs have them.
fn percent_decode(path: &str) -> Option<String> {
    let mut bytes = Vec::with_capacity(path.len());
    let mut rest = path.as_bytes();
    while let [first, tail @ ..] = rest {
        if *first == b'%' {
            let hex = std::str::from_utf8(tail.get(..2)?).ok()?;
            bytes.push(u8::from_str_radix(hex, 16).ok()?);
            rest = &tail[2..];
        } else {
            bytes.push(*first);
            rest = tail;
        }
    }
    String::from_utf8(bytes).ok()
}

/// Pixels, row by row, four bytes each, not premultiplied.
struct Rgba {
    width: usize,
    height: usize,
    pixels: Vec<u8>,
}

fn draw(bytes: &[u8]) -> Option<Cover> {
    let rgba = decode(bytes)?;
    let small = (2.0 * theme::ICON_SIZE) as usize;
    let large = (2.0 * theme::COVER_SIZE) as usize;
    Some(Cover {
        small: square(&rgba, small, theme::COVER_ICON_RADIUS / theme::ICON_SIZE),
        large: square(&rgba, large, theme::COVER_RADIUS / theme::COVER_SIZE),
    })
}

fn decode(bytes: &[u8]) -> Option<Rgba> {
    let rgba = if bytes.starts_with(b"\x89PNG") {
        let pixmap = Pixmap::decode_png(bytes).ok()?;
        let pixels = pixmap
            .pixels()
            .iter()
            .flat_map(|pixel| {
                let color = pixel.demultiply();
                [color.red(), color.green(), color.blue(), color.alpha()]
            })
            .collect();
        Rgba {
            width: pixmap.width() as usize,
            height: pixmap.height() as usize,
            pixels,
        }
    } else if bytes.starts_with(&[0xFF, 0xD8]) {
        use zune_jpeg::zune_core::colorspace::ColorSpace;
        use zune_jpeg::zune_core::options::DecoderOptions;

        let options = DecoderOptions::default()
            .jpeg_set_out_colorspace(ColorSpace::RGBA)
            .set_max_width(MAX_SIDE)
            .set_max_height(MAX_SIDE);
        let mut decoder =
            zune_jpeg::JpegDecoder::new_with_options(std::io::Cursor::new(bytes), options);
        let pixels = decoder.decode().ok()?;
        let info = decoder.info()?;
        Rgba {
            width: usize::from(info.width),
            height: usize::from(info.height),
            pixels,
        }
    } else {
        return None;
    };
    let size_ok = rgba.width > 0 && rgba.height > 0 && rgba.width.max(rgba.height) <= MAX_SIDE;
    (size_ok && rgba.pixels.len() == rgba.width * rgba.height * 4).then_some(rgba)
}

/// The middle square of `rgba`, at most `size` pixels across (smaller pictures aren't blown up
/// here; the renderer scales them), with corners rounded by `radius` of its side.
fn square(rgba: &Rgba, size: usize, radius: f32) -> image::Handle {
    let side = rgba.width.min(rgba.height);
    let (left, top) = ((rgba.width - side) / 2, (rgba.height - side) / 2);
    let size = size.min(side);
    let mut pixels = Vec::with_capacity(size * size * 4);
    for y in 0..size {
        let (y0, y1) = span(y, size, side);
        for x in 0..size {
            let (x0, x1) = span(x, size, side);
            // Every source pixel under this one, averaged.
            let mut sum = [0u32; 4];
            for sy in y0..y1 {
                let row = (top + sy) * rgba.width;
                for sx in x0..x1 {
                    let at = (row + left + sx) * 4;
                    for (total, value) in sum.iter_mut().zip(&rgba.pixels[at..at + 4]) {
                        *total += u32::from(*value);
                    }
                }
            }
            let count = ((y1 - y0) * (x1 - x0)) as u32;
            let [red, green, blue, alpha] = sum.map(|total| (total / count) as u8);
            let alpha = (f32::from(alpha) * corner(x, y, size, radius)).round() as u8;
            pixels.extend([red, green, blue, alpha]);
        }
    }
    image::Handle::from_rgba(size as u32, size as u32, pixels)
}

/// The source pixels target pixel `index` of `size` covers, out of `side`.
fn span(index: usize, size: usize, side: usize) -> (usize, usize) {
    let start = index * side / size;
    let end = ((index + 1) * side / size).max(start + 1);
    (start, end)
}

/// How much of pixel (`x`, `y`) is inside a `size` square with corners rounded by `radius` of
/// its side: 1 inside, 0 outside, in between along the curve.
fn corner(x: usize, y: usize, size: usize, radius: f32) -> f32 {
    let size = size as f32;
    let radius = radius * size;
    let (cx, cy) = (x as f32 + 0.5, y as f32 + 0.5);
    let dx = (radius - cx).max(cx - (size - radius)).max(0.0);
    let dy = (radius - cy).max(cy - (size - radius)).max(0.0);
    let outside = (dx * dx + dy * dy).sqrt() - radius;
    (0.5 - outside).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checker(width: usize, height: usize) -> Rgba {
        let pixels = (0..width * height)
            .flat_map(|i| {
                let on = (i % width + i / width).is_multiple_of(2);
                let value = if on { 255 } else { 0 };
                [value, value, value, 255]
            })
            .collect();
        Rgba {
            width,
            height,
            pixels,
        }
    }

    fn pixels(handle: &image::Handle) -> (u32, u32, Vec<u8>) {
        let image::Handle::Rgba {
            width,
            height,
            pixels,
            ..
        } = handle
        else {
            panic!("rgba");
        };
        (*width, *height, pixels.to_vec())
    }

    #[test]
    fn crops_the_middle_square_and_averages_it_down() {
        let (width, height, pixels) = pixels(&square(&checker(80, 40), 10, 0.0));
        assert_eq!((width, height), (10, 10));
        // A 4 × 4 block of checks averages to grey.
        assert!(pixels.chunks(4).all(|p| (126..=128).contains(&p[0])));
    }

    #[test]
    fn small_pictures_are_not_blown_up() {
        let (width, height, _) = pixels(&square(&checker(20, 30), 64, 0.0));
        assert_eq!((width, height), (20, 20));
    }

    #[test]
    fn corners_are_rounded() {
        let (_, _, pixels) = pixels(&square(&checker(40, 40), 40, 0.25));
        let alpha = |x: usize, y: usize| pixels[(y * 40 + x) * 4 + 3];
        assert_eq!(alpha(0, 0), 0, "the very corner is cut");
        assert_eq!(alpha(20, 20), 255, "the middle isn't");
        assert_eq!(alpha(20, 0), 255, "nor an edge's middle");
    }

    #[test]
    fn decodes_png_and_jpeg_and_nothing_else() {
        let png = Pixmap::new(3, 2).unwrap().encode_png().unwrap();
        let rgba = decode(&png).unwrap();
        assert_eq!((rgba.width, rgba.height), (3, 2));
        assert!(decode(b"GIF89a").is_none());
        assert!(decode(&[0xFF, 0xD8, 0, 0]).is_none(), "a broken JPEG");
    }

    #[test]
    fn file_urls_are_percent_decoded() {
        assert_eq!(
            percent_decode("/tmp/My%20Song.jpg").as_deref(),
            Some("/tmp/My Song.jpg")
        );
        assert_eq!(percent_decode("/tmp/bad%2"), None);
    }

    #[test]
    fn only_files_and_https_are_fetched() {
        assert!(fetch("http://example.com/a.jpg").is_none());
        assert!(fetch("data:image/png;base64,AAAA").is_none());
        assert!(fetch("file:///nonexistent/eave-cover.png").is_none());
    }

    /// `EAVE_COVER=<url> cargo test -p eave cover -- --ignored --nocapture` fetches and draws a
    /// real cover: a file, or over https.
    #[test]
    #[ignore = "fetches a real cover"]
    fn draws_a_real_cover() {
        let url = std::env::var("EAVE_COVER").expect("EAVE_COVER=<url>");
        let bytes = fetch(&url).expect("fetched");
        let rgba = decode(&bytes).expect("decoded");
        println!(
            "{url}: {} bytes, {}×{}",
            bytes.len(),
            rgba.width,
            rgba.height
        );
        assert!(draw(&bytes).is_some());
    }
}

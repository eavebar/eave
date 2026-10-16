//! A picture of what some widgets draw, taken once and drawn as an image after (D-124). iced has
//! no group opacity, so two things can't cross-fade live: a fade covers with the island's color,
//! and that would cover the other side too. Pictures have real alpha, so two can cross, and they
//! can blur: each is kept sharp and at a few blurs, from smaller and smaller copies, and drawn
//! between the two nearest. Taken on the CPU, with a headless tiny-skia renderer of its own,
//! whichever renderer draws the bar.

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::pin::pin;
use std::sync::{Arc, OnceLock};
use std::task::{Context, Poll, Waker};

use iced::advanced::image::{self as image_renderer, FilterMethod};
use iced::advanced::renderer::{self, Headless};
use iced::advanced::widget::Tree;
use iced::advanced::{Layout, Renderer as _, mouse};
use iced::widget::image;
use iced::{Color, Element, Rectangle, Size, Vector};

/// How blurred each copy is, in logical pixels: the first sharp, each next one from a copy half
/// as big, so blurring them costs about the same.
const BLURS: [f32; 4] = [0.0, 2.0, 4.0, 8.0];

thread_local! {
    /// The screen's scale, physical pixels to logical: pictures are taken at it.
    static SCALE: Cell<f32> = const { Cell::new(2.0) };
    /// One renderer for every picture, so its glyphs and images stay cached between them. `None`
    /// until the first, and if it can't be made.
    static RENDERER: RefCell<Option<iced::Renderer>> = const { RefCell::new(None) };
}

/// Takes pictures at `scale` from now on.
pub fn set_scale(scale: f32) {
    if scale.is_finite() && scale > 0.0 {
        SCALE.with(|cell| cell.set(scale));
    }
}

/// A picture of something, sharp and blurred.
#[derive(Debug, Clone)]
pub struct Snapshot {
    /// Where it was taken, room for the blur included, in the coordinates it was drawn in.
    pub rect: Rectangle,
    sharp: image::Handle,
    /// One for each of [`BLURS`] past the first, made on a thread of their own: the first
    /// frames are drawn sharp until they're there, and the morph starts where its pictures are
    /// sharp anyway.
    blurred: Arc<OnceLock<Vec<image::Handle>>>,
}

impl Snapshot {
    /// Takes a picture of what `draw` draws in `rect`, with room round it for the blur to spread
    /// into. `draw` gets a renderer moved so `rect` is where it draws, as it would on screen.
    /// None if there's no renderer, or nothing to take.
    pub fn take(rect: Rectangle, draw: impl FnOnce(&mut iced::Renderer)) -> Option<Self> {
        let scale = SCALE.with(Cell::get);
        let room = BLURS[BLURS.len() - 1] * 2.0;
        let rect = rect.expand(room);
        // Whole physical pixels, so it lands on them.
        let size = Size::new(
            (rect.width * scale).ceil() as u32,
            (rect.height * scale).ceil() as u32,
        );
        if size.width == 0 || size.height == 0 {
            return None;
        }
        let rect = Rectangle {
            width: size.width as f32 / scale,
            height: size.height as f32 / scale,
            ..rect
        };
        let pixels = RENDERER.with(|cell| {
            let mut cell = cell.borrow_mut();
            let renderer = match &mut *cell {
                Some(renderer) => renderer,
                none => none.insert(made()?),
            };
            renderer.reset(Rectangle::with_size(rect.size()));
            renderer.with_translation(Vector::new(-rect.x, -rect.y), |renderer| {
                // What it draws isn't on screen: the glass plugin doesn't hear of it.
                crate::glass::frame::hushed(|| draw(renderer));
            });
            Some(renderer.screenshot(size, scale, Color::TRANSPARENT))
        })?;
        let sharp = image::Handle::from_rgba(size.width, size.height, straight(&pixels));
        let blurred = Arc::new(OnceLock::new());
        let into = blurred.clone();
        let blurring = move || {
            let _ = into.set(blur(size, scale, &pixels));
        };
        // Without a thread, it stays sharp.
        let _ = std::thread::Builder::new()
            .name("eave-blur".into())
            .spawn(blurring);
        Some(Self {
            rect,
            sharp,
            blurred,
        })
    }

    /// Draws it in `bounds` (where `rect` goes, scaled or not), blurred by `blur` logical pixels
    /// and as opaque as `opacity`.
    pub fn draw(&self, renderer: &mut iced::Renderer, bounds: Rectangle, blur: f32, opacity: f32) {
        use image_renderer::Renderer as _;

        if opacity <= 0.0 || bounds.width < 1.0 || bounds.height < 1.0 {
            return;
        }
        let mut draw = |handle: &image::Handle, opacity: f32| {
            if opacity > 0.0 {
                renderer.draw_image(
                    image_renderer::Image {
                        filter_method: FilterMethod::Linear,
                        opacity,
                        snap: false,
                        ..image_renderer::Image::new(handle.clone())
                    },
                    bounds,
                    bounds,
                );
            }
        };
        let blur = blur.clamp(0.0, BLURS[BLURS.len() - 1]);
        // Sharp, or not blurred yet.
        let Some(blurred) = self.blurred.get().filter(|_| blur > 0.0) else {
            draw(&self.sharp, opacity);
            return;
        };
        let upper = BLURS
            .iter()
            .position(|level| *level >= blur)
            .unwrap_or(BLURS.len() - 1)
            .max(1);
        let (low, high) = (BLURS[upper - 1], BLURS[upper]);
        let between = ((blur - low) / (high - low)).clamp(0.0, 1.0);
        let level = |level: usize| {
            if level == 0 {
                &self.sharp
            } else {
                &blurred[level - 1]
            }
        };
        // The blurrier over the sharper, as much as it's between them. The sharper is a little
        // lighter, so where both cover, the two add up to `opacity`.
        let over = opacity * between;
        let under = if over < 1.0 {
            opacity * (1.0 - between) / (1.0 - over)
        } else {
            0.0
        };
        draw(level(upper - 1), under);
        draw(level(upper), over);
    }
}

/// The picture taken at `size` and `scale` (premultiplied RGBA), blurred by each of [`BLURS`]
/// past the first, from copies half as big each time.
fn blur(size: Size<u32>, scale: f32, pixels: &[u8]) -> Vec<image::Handle> {
    let width = size.width as usize;
    let mut copy = Pixels::halved_from(width, size.height as usize, pixels);
    BLURS
        .iter()
        .enumerate()
        .skip(1)
        .map(|(level, blur)| {
            if level > 1 {
                copy = copy.halved();
            }
            let mut blurred = copy.clone();
            // Three box blurs about make a gaussian. Each copy is half as big as the last, so its
            // pixels are twice as wide; the sigma in them is about half each box's width.
            let sigma = blur * scale / (width as f32 / copy.width as f32);
            let radius = (sigma - 0.5).round().max(1.0) as usize;
            for _ in 0..3 {
                blurred.blur(radius);
            }
            blurred.handle()
        })
        .collect()
}

/// Draws `element` into a picture of `rect`: a panel's content, as it is now.
pub fn of<M>(
    element: &Element<'_, M>,
    tree: &Tree,
    layout: Layout<'_>,
    theme: &iced::Theme,
    style: &renderer::Style,
    cursor: mouse::Cursor,
) -> Option<Snapshot> {
    let rect = layout.bounds();
    Snapshot::take(rect, |renderer| {
        element.as_widget().draw(
            tree,
            renderer,
            theme,
            style,
            layout,
            cursor,
            &rect.expand(64.0),
        );
    })
}

/// A headless tiny-skia renderer. Making one does nothing that waits, so the future is ready
/// the first time it's asked.
fn made() -> Option<iced::Renderer> {
    let future = <iced::Renderer as Headless>::new(
        iced::Font::DEFAULT,
        iced::Pixels(crate::theme::TEXT_SIZE),
        Some("tiny-skia"),
    );
    match pin!(future).poll(&mut Context::from_waker(Waker::noop())) {
        Poll::Ready(renderer) => renderer,
        Poll::Pending => None,
    }
}

/// Premultiplied RGBA bytes made straight, as images take them.
fn straight(premultiplied: &[u8]) -> Vec<u8> {
    let mut out = premultiplied.to_vec();
    for pixel in out.chunks_exact_mut(4) {
        let alpha = u16::from(pixel[3]);
        if alpha == 0 {
            pixel.fill(0);
        } else if alpha < 255 {
            for channel in &mut pixel[..3] {
                *channel = ((u16::from(*channel) * 255 + alpha / 2) / alpha).min(255) as u8;
            }
        }
    }
    out
}

/// Premultiplied RGBA, one f32 a channel.
#[derive(Debug, Clone)]
struct Pixels {
    width: usize,
    height: usize,
    data: Vec<f32>,
}

impl Pixels {
    #[cfg(test)]
    fn premultiplied(width: usize, height: usize, bytes: &[u8]) -> Self {
        Self {
            width,
            height,
            data: bytes.iter().map(|byte| f32::from(*byte)).collect(),
        }
    }

    /// [`Pixels::premultiplied`], half as big: each pixel the average of the four it covers.
    fn halved_from(width: usize, height: usize, bytes: &[u8]) -> Self {
        let (half_width, half_height) = (width.div_ceil(2), height.div_ceil(2));
        let mut data = vec![0.0; half_width * half_height * 4];
        for y in 0..height {
            let row = &bytes[y * width * 4..(y + 1) * width * 4];
            let out = &mut data[(y / 2) * half_width * 4..(y / 2 + 1) * half_width * 4];
            // An odd last row or column counts twice, as if repeated.
            let rows = if y == height - 1 && height % 2 == 1 {
                2.0
            } else {
                1.0
            };
            for (x, pixel) in row.chunks_exact(4).enumerate() {
                let columns = if x == width - 1 && width % 2 == 1 {
                    2.0
                } else {
                    1.0
                };
                let at = (x / 2) * 4;
                for (out, value) in out[at..at + 4].iter_mut().zip(pixel) {
                    *out += f32::from(*value) * rows * columns / 4.0;
                }
            }
        }
        Self {
            width: half_width,
            height: half_height,
            data,
        }
    }

    /// Half as big, each pixel the average of the four it covers.
    fn halved(&self) -> Self {
        let (width, height) = (self.width.div_ceil(2), self.height.div_ceil(2));
        let mut data = vec![0.0; width * height * 4];
        for y in 0..height {
            for x in 0..width {
                let at = (y * width + x) * 4;
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let (sx, sy) = (
                        (2 * x + dx).min(self.width - 1),
                        (2 * y + dy).min(self.height - 1),
                    );
                    let from = (sy * self.width + sx) * 4;
                    for (out, value) in data[at..at + 4].iter_mut().zip(&self.data[from..from + 4])
                    {
                        *out += value / 4.0;
                    }
                }
            }
        }
        Self {
            width,
            height,
            data,
        }
    }

    /// A box blur `radius` pixels each way, across then down. Past the edges is transparent.
    fn blur(&mut self, radius: usize) {
        if radius == 0 {
            return;
        }
        let (width, height) = (self.width, self.height);
        let window = (2 * radius + 1) as f32;
        // Across: each row on its own, a running sum over the window.
        let mut row = vec![0.0; width * 4];
        for y in 0..height {
            let line = &mut self.data[y * width * 4..(y + 1) * width * 4];
            row.copy_from_slice(line);
            let mut sum = [0.0f32; 4];
            for x in 0..radius.min(width) {
                for (sum, value) in sum.iter_mut().zip(&row[x * 4..x * 4 + 4]) {
                    *sum += value;
                }
            }
            for x in 0..width {
                if x + radius < width {
                    let at = (x + radius) * 4;
                    for (sum, value) in sum.iter_mut().zip(&row[at..at + 4]) {
                        *sum += value;
                    }
                }
                for (out, sum) in line[x * 4..x * 4 + 4].iter_mut().zip(sum) {
                    *out = sum / window;
                }
                if x >= radius {
                    let at = (x - radius) * 4;
                    for (sum, value) in sum.iter_mut().zip(&row[at..at + 4]) {
                        *sum -= value;
                    }
                }
            }
        }
        // Down: a running sum for every column at once, a row at a time.
        let stride = width * 4;
        let before = self.data.clone();
        let mut sums = vec![0.0f32; stride];
        for y in 0..radius.min(height) {
            for (sum, value) in sums.iter_mut().zip(&before[y * stride..(y + 1) * stride]) {
                *sum += value;
            }
        }
        for y in 0..height {
            if y + radius < height {
                let at = (y + radius) * stride;
                for (sum, value) in sums.iter_mut().zip(&before[at..at + stride]) {
                    *sum += value;
                }
            }
            for (out, sum) in self.data[y * stride..(y + 1) * stride]
                .iter_mut()
                .zip(&sums)
            {
                *out = sum / window;
            }
            if y >= radius {
                let at = (y - radius) * stride;
                for (sum, value) in sums.iter_mut().zip(&before[at..at + stride]) {
                    *sum -= value;
                }
            }
        }
    }

    /// Straight RGBA bytes, as images take them.
    fn straight(&self) -> Vec<u8> {
        self.data
            .chunks_exact(4)
            .flat_map(|pixel| {
                let alpha = pixel[3];
                let byte = |value: f32| value.round().clamp(0.0, 255.0) as u8;
                if alpha <= 0.0 {
                    return [0; 4];
                }
                let un = |value: f32| byte(value * 255.0 / alpha);
                [un(pixel[0]), un(pixel[1]), un(pixel[2]), byte(alpha)]
            })
            .collect()
    }

    fn handle(&self) -> image::Handle {
        image::Handle::from_rgba(self.width as u32, self.height as u32, self.straight())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alpha(pixels: &Pixels) -> f32 {
        pixels.data.chunks_exact(4).map(|pixel| pixel[3]).sum()
    }

    #[test]
    fn blurring_spreads_without_losing_any() {
        let mut bytes = vec![0u8; 32 * 32 * 4];
        let at = (16 * 32 + 16) * 4;
        bytes[at..at + 4].copy_from_slice(&[255, 255, 255, 255]);
        let mut pixels = Pixels::premultiplied(32, 32, &bytes);
        let before = alpha(&pixels);
        for _ in 0..3 {
            pixels.blur(2);
        }
        assert!((alpha(&pixels) - before).abs() < 0.5);
        assert!(pixels.data[at + 3] < 64.0, "it spread out");
        assert!(
            pixels.data[(16 * 32 + 19) * 4 + 3] > 0.0,
            "it reached the neighbours"
        );
    }

    #[test]
    fn halving_keeps_the_average() {
        let bytes: Vec<u8> = (0..8 * 8 * 4).map(|i| (i * 7 % 256) as u8).collect();
        let pixels = Pixels::premultiplied(8, 8, &bytes);
        let half = pixels.halved();
        assert_eq!((half.width, half.height), (4, 4));
        let mean = |pixels: &Pixels| alpha(pixels) / (pixels.width * pixels.height) as f32;
        assert!((mean(&half) - mean(&pixels)).abs() < 0.01);
        // Straight from the bytes, the same, odd sizes included.
        assert_eq!(Pixels::halved_from(8, 8, &bytes).data, half.data);
        let odd: Vec<u8> = (0..5 * 3 * 4).map(|i| (i * 11 % 256) as u8).collect();
        assert_eq!(
            Pixels::halved_from(5, 3, &odd).data,
            Pixels::premultiplied(5, 3, &odd).halved().data
        );
    }

    #[test]
    fn straight_alpha_undoes_the_premultiplying() {
        // Half-covered white is 128 premultiplied: straight, it's white at half.
        let pixels = Pixels::premultiplied(1, 1, &[128, 128, 128, 128]);
        assert_eq!(pixels.straight(), vec![255, 255, 255, 128]);
        let clear = Pixels::premultiplied(1, 1, &[0, 0, 0, 0]);
        assert_eq!(clear.straight(), vec![0, 0, 0, 0]);
        assert_eq!(straight(&[128, 128, 128, 128]), vec![255, 255, 255, 128]);
        assert_eq!(
            straight(&[0, 0, 0, 0, 10, 20, 30, 255]),
            vec![0, 0, 0, 0, 10, 20, 30, 255]
        );
    }

    #[test]
    fn a_label_is_taken_with_its_ink_and_room_to_blur() {
        let mut label: Element<'_, ()> = iced::widget::text("Battery").size(14).into();
        let mut tree = Tree::new(&label);
        let renderer = made().expect("tiny-skia");
        let limits = iced::advanced::layout::Limits::new(Size::ZERO, Size::new(200.0, 40.0));
        let node = label
            .as_widget_mut()
            .layout(&mut tree, &renderer, &limits)
            .move_to(iced::Point::new(10.0, 6.0));
        let snapshot = of(
            &label,
            &tree,
            Layout::new(&node),
            &iced::Theme::Dark,
            &renderer::Style {
                text_color: Color::WHITE,
            },
            mouse::Cursor::Unavailable,
        )
        .expect("a picture");
        let start = std::time::Instant::now();
        while snapshot.blurred.get().is_none() && start.elapsed().as_secs() < 5 {
            std::thread::yield_now();
        }
        assert_eq!(snapshot.blurred.get().map(Vec::len), Some(BLURS.len() - 1));
        assert!(snapshot.rect.contains(node.bounds().position()));
        assert!(snapshot.rect.width > node.bounds().width);
        let image::Handle::Rgba { pixels, .. } = &snapshot.sharp else {
            panic!("RGBA");
        };
        assert!(
            pixels.chunks_exact(4).any(|pixel| pixel[3] > 200),
            "the text is in it"
        );
    }
}

#[cfg(test)]
mod timing {
    use super::*;

    #[test]
    #[ignore]
    fn how_long_a_panel_takes() {
        let mut panel: Element<'_, ()> = iced::widget::column(
            (0..16).map(|i| iced::widget::text(format!("Row number {i} with some words")).into()),
        )
        .into();
        let mut tree = Tree::new(&panel);
        let renderer = made().unwrap();
        let limits = iced::advanced::layout::Limits::new(Size::ZERO, Size::new(340.0, 600.0));
        let node = panel.as_widget_mut().layout(&mut tree, &renderer, &limits);
        let style = renderer::Style {
            text_color: Color::WHITE,
        };
        for _ in 0..30 {
            let start = std::time::Instant::now();
            of(
                &panel,
                &tree,
                Layout::new(&node),
                &iced::Theme::Dark,
                &style,
                mouse::Cursor::Unavailable,
            )
            .unwrap();
            eprintln!("{:?} for {:?}", start.elapsed(), node.bounds().size());
        }
    }
}

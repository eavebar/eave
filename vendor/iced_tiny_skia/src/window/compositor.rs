use crate::core::{Color, Rectangle, Size};
use crate::graphics::compositor::{self, Information};
use crate::graphics::damage;
use crate::graphics::error::{self, Error};
use crate::graphics::{self, Shell, Viewport};
use crate::engine::ClipMask;
use crate::{Layer, Renderer, Settings};

use std::collections::VecDeque;
use std::num::NonZeroU32;
use std::time::{Duration, Instant};

pub struct Compositor {
    context: softbuffer::Context<Box<dyn compositor::Display>>,
    settings: Settings,
}

pub struct Surface {
    window: softbuffer::Surface<
        Box<dyn compositor::Display>,
        Box<dyn compositor::Window>,
    >,
    clip_mask: ClipMask,
    layer_stack: VecDeque<Vec<Layer>>,
    background_color: Color,
    max_age: u8,
    probe: Option<Probe>,
}

impl crate::graphics::Compositor for Compositor {
    type Renderer = Renderer;
    type Surface = Surface;

    async fn with_backend(
        settings: graphics::Settings,
        display: impl compositor::Display,
        _compatible_window: impl compositor::Window,
        _shell: Shell,
        backend: Option<&str>,
    ) -> Result<Self, Error> {
        match backend {
            None | Some("tiny-skia") | Some("tiny_skia") => {
                Ok(new(settings.into(), display))
            }
            Some(backend) => Err(Error::GraphicsAdapterNotFound {
                backend: "tiny-skia",
                reason: error::Reason::DidNotMatch {
                    preferred_backend: backend.to_owned(),
                },
            }),
        }
    }

    fn create_renderer(&self) -> Self::Renderer {
        Renderer::new(
            self.settings.default_font,
            self.settings.default_text_size,
        )
    }

    fn create_surface<W: compositor::Window + Clone>(
        &mut self,
        window: W,
        width: u32,
        height: u32,
    ) -> Self::Surface {
        let window = softbuffer::Surface::new(
            &self.context,
            Box::new(window.clone()) as _,
        )
        .expect("Create softbuffer surface for window");

        let mut surface = Surface {
            window,
            clip_mask: ClipMask::new(1, 1),
            layer_stack: VecDeque::new(),
            background_color: Color::BLACK,
            max_age: 0,
            probe: Probe::from_env(),
        };

        if width > 0 && height > 0 {
            self.configure_surface(&mut surface, width, height);
        }

        surface
    }

    fn configure_surface(
        &mut self,
        surface: &mut Self::Surface,
        width: u32,
        height: u32,
    ) {
        surface
            .window
            .resize(
                NonZeroU32::new(width).expect("Non-zero width"),
                NonZeroU32::new(height).expect("Non-zero height"),
            )
            .expect("Resize surface");

        surface.clip_mask = ClipMask::new(width, height);
        surface.layer_stack.clear();
    }

    fn information(&self) -> Information {
        Information {
            adapter: String::from("CPU"),
            backend: String::from("tiny-skia"),
        }
    }

    fn present(
        &mut self,
        renderer: &mut Self::Renderer,
        surface: &mut Self::Surface,
        viewport: &Viewport,
        background_color: Color,
        on_pre_present: impl FnOnce(),
    ) -> Result<(), compositor::SurfaceError> {
        present(
            renderer,
            surface,
            viewport,
            background_color,
            on_pre_present,
        )
    }

    fn screenshot(
        &mut self,
        renderer: &mut Self::Renderer,
        viewport: &Viewport,
        background_color: Color,
    ) -> Vec<u8> {
        screenshot(renderer, viewport, background_color)
    }
}

pub fn new(
    settings: Settings,
    display: impl compositor::Display,
) -> Compositor {
    #[allow(unsafe_code)]
    let context = softbuffer::Context::new(Box::new(display) as _)
        .expect("Create softbuffer context");

    Compositor { context, settings }
}

pub fn present(
    renderer: &mut Renderer,
    surface: &mut Surface,
    viewport: &Viewport,
    background_color: Color,
    on_pre_present: impl FnOnce(),
) -> Result<(), compositor::SurfaceError> {
    let physical_size = viewport.physical_size();

    let mut buffer = surface
        .window
        .buffer_mut()
        .map_err(|_| compositor::SurfaceError::Lost)?;

    let last_layers = {
        let age = buffer.age();

        surface.max_age = surface.max_age.max(age);
        surface.layer_stack.truncate(surface.max_age as usize);

        if age > 0 {
            surface.layer_stack.get(age as usize - 1)
        } else {
            None
        }
    };

    let damage = last_layers
        .and_then(|last_layers| {
            (surface.background_color == background_color).then(|| {
                damage::diff(
                    last_layers,
                    renderer.layers(),
                    |layer| vec![layer.bounds],
                    Layer::damage,
                )
            })
        })
        .unwrap_or_else(|| vec![Rectangle::with_size(viewport.logical_size())]);

    // eave patch: tell the compositor what changed, not the whole surface. What the compositor
    // shows is the last frame presented, so its damage is the change since that frame, not since
    // the frame this buffer last held (`age` frames ago). Usually the two are the same; with an
    // older buffer, both are sent.
    let shown = match surface.layer_stack.front() {
        Some(previous) if buffer.age() > 1 => {
            (surface.background_color == background_color).then(|| {
                damage::diff(
                    previous,
                    renderer.layers(),
                    |layer| vec![layer.bounds],
                    Layer::damage,
                )
            })
        }
        _ => None,
    };

    let started = surface.probe.as_ref().map(|_| Instant::now());
    let layers = renderer.layers().len();

    let presented = if damage.is_empty() {
        if let Some(last_layers) = last_layers {
            surface.layer_stack.push_front(last_layers.clone());
        }

        shown.unwrap_or_default()
    } else {
        surface.layer_stack.push_front(renderer.layers().to_vec());
        surface.background_color = background_color;

        let damage = damage::group(
            damage,
            Rectangle::with_size(viewport.logical_size()),
        );

        let mut pixels = tiny_skia::PixmapMut::from_bytes(
            bytemuck::cast_slice_mut(&mut buffer),
            physical_size.width,
            physical_size.height,
        )
        .expect("Create pixel map");

        renderer.draw(
            &mut pixels,
            &mut surface.clip_mask,
            viewport,
            &damage,
            background_color,
        );

        match shown {
            Some(mut shown) => {
                shown.extend(damage);
                shown
            }
            None => damage,
        }
    };

    let scale = viewport.scale_factor() as f32;
    let rects: Vec<softbuffer::Rect> = presented
        .iter()
        .filter_map(|bounds| physical(*bounds * scale, physical_size))
        .collect();

    if let (Some(probe), Some(started)) = (&mut surface.probe, started) {
        let pixels = rects
            .iter()
            .map(|rect| rect.width.get() as u64 * rect.height.get() as u64)
            .sum();
        probe.frame(started.elapsed(), pixels, layers, physical_size);
    }

    on_pre_present();
    buffer
        .present_with_damage(&rects)
        .map_err(|_| compositor::SurfaceError::Lost)
}

/// `bounds` as whole pixels inside the surface, rounded out; `None` if nothing is left.
fn physical(bounds: Rectangle, size: Size<u32>) -> Option<softbuffer::Rect> {
    let x0 = bounds.x.floor().max(0.0) as u32;
    let y0 = bounds.y.floor().max(0.0) as u32;
    let x1 = ((bounds.x + bounds.width).ceil().max(0.0) as u32).min(size.width);
    let y1 = ((bounds.y + bounds.height).ceil().max(0.0) as u32).min(size.height);

    Some(softbuffer::Rect {
        x: x0,
        y: y0,
        width: NonZeroU32::new(x1.checked_sub(x0)?)?,
        height: NonZeroU32::new(y1.checked_sub(y0)?)?,
    })
}

/// eave patch: with `EAVE_FRAMES` set, a line on stderr each second about the frames drawn.
#[derive(Default)]
struct Probe {
    since: Option<Instant>,
    frames: u32,
    drawn: Duration,
    slowest: Duration,
    pixels: u64,
    most_pixels: u64,
    layers: usize,
}

impl Probe {
    fn from_env() -> Option<Self> {
        std::env::var_os("EAVE_FRAMES").map(|_| Self::default())
    }

    fn frame(&mut self, took: Duration, pixels: u64, layers: usize, size: Size<u32>) {
        let since = *self.since.get_or_insert_with(Instant::now);
        self.frames += 1;
        self.drawn += took;
        self.slowest = self.slowest.max(took);
        self.pixels += pixels;
        self.most_pixels = self.most_pixels.max(pixels);
        self.layers = self.layers.max(layers);

        if since.elapsed() >= Duration::from_secs(1) {
            eprintln!(
                "frames {:>3}  draw avg {:>5.2} ms max {:>5.2} ms  damage avg {:>8} px max {:>8} px  layers {:>3}  surface {}x{}",
                self.frames,
                self.drawn.as_secs_f64() * 1000.0 / self.frames as f64,
                self.slowest.as_secs_f64() * 1000.0,
                self.pixels / self.frames as u64,
                self.most_pixels,
                self.layers,
                size.width,
                size.height,
            );
            *self = Self::default();
        }
    }
}

pub fn screenshot(
    renderer: &mut Renderer,
    viewport: &Viewport,
    background_color: Color,
) -> Vec<u8> {
    let size = viewport.physical_size();

    let mut offscreen_buffer: Vec<u32> =
        vec![0; size.width as usize * size.height as usize];

    let mut clip_mask = ClipMask::new(size.width, size.height);

    renderer.draw(
        &mut tiny_skia::PixmapMut::from_bytes(
            bytemuck::cast_slice_mut(&mut offscreen_buffer),
            size.width,
            size.height,
        )
        .expect("Create offscreen pixel map"),
        &mut clip_mask,
        viewport,
        &[Rectangle::with_size(Size::new(
            size.width as f32,
            size.height as f32,
        ))],
        background_color,
    );

    offscreen_buffer.iter().fold(
        Vec::with_capacity(offscreen_buffer.len() * 4),
        |mut acc, pixel| {
            const A_MASK: u32 = 0xFF_00_00_00;
            const R_MASK: u32 = 0x00_FF_00_00;
            const G_MASK: u32 = 0x00_00_FF_00;
            const B_MASK: u32 = 0x00_00_00_FF;

            let a = ((A_MASK & pixel) >> 24) as u8;
            let r = ((R_MASK & pixel) >> 16) as u8;
            let g = ((G_MASK & pixel) >> 8) as u8;
            let b = (B_MASK & pixel) as u8;

            acc.extend([r, g, b, a]);
            acc
        },
    )
}

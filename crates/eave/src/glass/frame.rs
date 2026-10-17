//! What a frame says about glass, gathered while it's drawn: the islands that may turn to glass,
//! with their fill and which way their ink goes, and the pictures over them, kept as drawn inside
//! their rounded corners (D-129); and in every theme, where the notch and the grown edge are,
//! which the pointer goes under (D-113). Sent when the frame is ([`super::Present`]), so it lands with that frame's buffer. With glass, the
//! controls drawn on islands are gathered too, as reliefs cut into the glass or standing out of
//! it (D-126).

use std::cell::{Cell, RefCell};

use iced::{Color, Point, Rectangle, Vector};

use crate::theme;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    /// An island whose `fill` turns to glass, its grey ink going lighter (0) or darker (1) over it,
    /// or in between while it turns.
    Glass { fill: Color, darker: f32 },
    /// Content that never does, whatever it's over.
    Solid,
    /// The notch or the grown edge, exactly where it's drawn: the pointer goes under it. Sent in
    /// every theme, and it doesn't keep glass out: a `Solid` around it does.
    Bezel,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Record {
    /// Surface coordinates, logical px.
    pub rect: Rectangle,
    pub kind: Kind,
}

/// How a control shapes the glass it's on (D-126).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Shape {
    /// Cut into it.
    Well,
    /// Standing out of it.
    Bead,
}

/// A control on an island, as the glass sees it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Relief {
    /// Surface coordinates, logical px.
    pub rect: Rectangle,
    pub radius: f32,
    pub shape: Shape,
    /// How deep the well, or how high the bead, at rest, in logical px.
    pub depth: f32,
    /// How far it's pressed in, 0 to 1.
    pub press: f32,
    /// How milky the glass turns inside it, up to `fill_x` (surface x): where it's on.
    pub milk: f32,
    pub fill_x: f32,
    /// Where the finger is, and how bright the light around it is.
    pub light: Point,
    pub lit: f32,
}

thread_local! {
    static RECORDS: RefCell<Vec<Record>> = const { RefCell::new(Vec::new()) };
    static RELIEFS: RefCell<Vec<Relief>> = const { RefCell::new(Vec::new()) };
    static PICTURES: RefCell<Vec<Picture>> = const { RefCell::new(Vec::new()) };
    static ON: Cell<bool> = const { Cell::new(false) };
    /// How far what's being drawn is moved from its layout, as far as the islands say.
    static OFFSET: Cell<Vector> = const { Cell::new(Vector::new(0.0, 0.0)) };
    /// The top left of the scrollable being drawn, if any: what's in it is moved by how far it's
    /// scrolled, which only its viewport tells.
    static SCROLL: Cell<Option<Point>> = const { Cell::new(None) };
    /// How what's being drawn is squeezed, if it is: about a point, by a factor, inside how far
    /// it was moved when the squeeze began.
    static SQUEEZE: Cell<Option<(Point, f32, Vector)>> = const { Cell::new(None) };
}

/// Whether to gather records this frame: only while there's glass to tell.
pub fn gather(on: bool) {
    ON.with(|cell| cell.set(on));
    if !on {
        RECORDS.with(|records| records.borrow_mut().clear());
        RELIEFS.with(|reliefs| reliefs.borrow_mut().clear());
        PICTURES.with(|pictures| pictures.borrow_mut().clear());
    }
}

/// Whether there's glass this frame: what's drawn on it may shape it.
pub fn on() -> bool {
    ON.with(Cell::get)
}

/// Draws without gathering: what's drawn isn't on screen, like a picture being taken of it.
pub fn hushed<R>(draw: impl FnOnce() -> R) -> R {
    let before = ON.with(|cell| cell.replace(false));
    let out = draw();
    ON.with(|cell| cell.set(before));
    out
}

fn push(record: Record) {
    if ON.with(Cell::get) {
        RECORDS.with(|records| records.borrow_mut().push(record));
    }
}

/// An island at `rect` (surface coordinates, as drawn) that may turn to glass where it's `fill`,
/// its ink going `darker` (0 to 1) over it.
pub fn island(rect: Rectangle, fill: Color, darker: f32) {
    push(Record {
        rect: rect + OFFSET.with(Cell::get),
        kind: Kind::Glass { fill, darker },
    });
}

/// Something drawn at `rect` (surface coordinates, as drawn) that's never glass, even near an
/// island that is: the notch, the grown edge, an island attached to an edge.
pub fn opaque(rect: Rectangle) {
    push(Record {
        rect: rect + OFFSET.with(Cell::get),
        kind: Kind::Solid,
    });
}

/// The notch or the grown edge at `rect` (surface coordinates, as drawn). Recorded in every
/// theme, glass or not.
pub fn bezel(rect: Rectangle) {
    RECORDS.with(|records| {
        records.borrow_mut().push(Record {
            rect: rect + OFFSET.with(Cell::get),
            kind: Kind::Bezel,
        })
    });
}

/// A picture on an island, as the glass sees it: a rounded rectangle kept as drawn, with the glass
/// showing through its corners (D-129).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Picture {
    /// Surface coordinates, logical px.
    pub rect: Rectangle,
    /// Its corners' radius, circular.
    pub radius: f32,
    /// How opaque it's drawn, 0 to 1.
    pub opacity: f32,
}

/// Where what's drawn at `at` in its layout, with the `viewport` it was given, is on the surface:
/// moved as the islands say, scrolled, and squeezed with its island, about the island's center
/// and inside the island's own move. And by how much it's squeezed.
fn placer(viewport: &Rectangle) -> (impl Fn(Point) -> Point, f32) {
    let scrolled = SCROLL
        .with(Cell::get)
        .map_or(Vector::ZERO, |origin| origin - viewport.position());
    let by = OFFSET.with(Cell::get) + scrolled;
    let squeeze = SQUEEZE.with(Cell::get);
    let place = move |at: Point| match squeeze {
        Some((center, scale, outer)) => {
            let inner = at + (by - outer);
            center + (inner - center) * scale + outer
        }
        None => at + by,
    };
    (place, squeeze.map_or(1.0, |(_, scale, _)| scale))
}

/// A picture drawn at `bounds` in its layout, with the `viewport` it was given, its corners
/// rounded by `radius` and faded to `opacity`: never glass where it's drawn. Only what shows: a
/// long list draws pictures scrolled far out of it too.
pub fn picture(bounds: Rectangle, viewport: &Rectangle, radius: f32, opacity: f32) {
    if !on() || bounds.intersection(viewport).is_none() {
        return;
    }
    let (place, scale) = placer(viewport);
    let picture = Picture {
        rect: Rectangle::new(place(bounds.position()), bounds.size() * scale),
        radius: radius * scale,
        opacity,
    };
    PICTURES.with(|pictures| pictures.borrow_mut().push(picture));
}

/// A control drawn at `relief.rect` in its layout, with the `viewport` it was given: only what
/// shows of it, moved and scrolled as a picture is.
pub fn relief(relief: Relief, viewport: &Rectangle) {
    if !on() {
        return;
    }
    let Some(shown) = relief.rect.intersection(viewport) else {
        return;
    };
    let (place, scale) = placer(viewport);
    RELIEFS.with(|reliefs| {
        reliefs.borrow_mut().push(Relief {
            rect: Rectangle::new(place(shown.position()), shown.size() * scale),
            radius: relief.radius * scale,
            fill_x: place(Point::new(relief.fill_x, 0.0)).x,
            light: place(relief.light),
            ..relief
        })
    });
}

/// Draws with what's drawn squeezed by `scale` about `center`, as an island squeezes its items.
pub fn squeezed<R>(center: Point, scale: f32, draw: impl FnOnce() -> R) -> R {
    let outer = OFFSET.with(Cell::get);
    let before = SQUEEZE.with(|cell| cell.replace(Some((center, scale, outer))));
    let out = draw();
    SQUEEZE.with(|cell| cell.set(before));
    out
}

/// Draws with `by` added to how far what's drawn is moved.
pub fn translated<R>(by: Vector, draw: impl FnOnce() -> R) -> R {
    let before = OFFSET.with(|cell| cell.replace(cell.get() + by));
    let out = draw();
    OFFSET.with(|cell| cell.set(before));
    out
}

/// Draws a scrollable's content, its top left at `origin`.
pub fn scrolled<R>(origin: Point, draw: impl FnOnce() -> R) -> R {
    let before = SCROLL.with(|cell| cell.replace(Some(origin)));
    let out = draw();
    SCROLL.with(|cell| cell.set(before));
    out
}

/// This frame's records so far, leaving none.
pub fn take() -> Vec<Record> {
    RECORDS.with(|records| std::mem::take(&mut *records.borrow_mut()))
}

/// This frame's reliefs so far, leaving none.
pub fn take_reliefs() -> Vec<Relief> {
    RELIEFS.with(|reliefs| std::mem::take(&mut *reliefs.borrow_mut()))
}

/// This frame's pictures so far, leaving none.
pub fn take_pictures() -> Vec<Picture> {
    PICTURES.with(|pictures| std::mem::take(&mut *pictures.borrow_mut()))
}

/// The pictures as solid records, for a compositor from before pictures (version 4): their whole
/// box, a pixel more all round, as it was.
pub fn solids(pictures: &[Picture]) -> impl Iterator<Item = Record> + '_ {
    pictures.iter().map(|picture| Record {
        rect: picture.rect.expand(1.0),
        kind: Kind::Solid,
    })
}

/// What of `pictures` the compositor hears: those on an island of `records`, at most
/// [`theme::GLASS_MAX_PICTURES`], largest first.
pub fn fit_pictures(mut pictures: Vec<Picture>, records: &[Record]) -> Vec<Picture> {
    pictures.retain(|picture| {
        picture.rect.width >= 1.0
            && picture.rect.height >= 1.0
            && records.iter().any(|record| {
                matches!(record.kind, Kind::Glass { .. })
                    && record
                        .rect
                        .expand(theme::GLASS_MARGIN)
                        .intersection(&picture.rect)
                        .is_some()
            })
    });
    pictures.sort_by(|a, b| b.rect.area().total_cmp(&a.rect.area()));
    pictures.truncate(theme::GLASS_MAX_PICTURES);
    pictures
}

/// The pictures as `set_pictures` takes them: six native-endian i32 each, lengths in 24.8 fixed
/// point.
pub fn encode_pictures(pictures: &[Picture]) -> Vec<u8> {
    let mut out = Vec::with_capacity(pictures.len() * 24);
    for picture in pictures {
        let rect = picture.rect;
        for value in [
            fixed(rect.x),
            fixed(rect.y),
            fixed(rect.width),
            fixed(rect.height),
            fixed(picture.radius.min(rect.width.min(rect.height) / 2.0)),
            (picture.opacity.clamp(0.0, 1.0) * 255.0).round() as i32,
        ] {
            out.extend_from_slice(&value.to_ne_bytes());
        }
    }
    out
}

/// A length in 24.8 fixed point.
fn fixed(value: f32) -> i32 {
    (value * 256.0)
        .round()
        .clamp(i32::MIN as f32, i32::MAX as f32) as i32
}

/// What of `reliefs` the compositor hears: those on an island of `records`, at most
/// [`theme::GLASS_MAX_RELIEFS`], in the order they were drawn.
pub fn fit_reliefs(mut reliefs: Vec<Relief>, records: &[Record]) -> Vec<Relief> {
    reliefs.retain(|relief| {
        relief.rect.width >= 1.0
            && relief.rect.height >= 1.0
            && records.iter().any(|record| {
                matches!(record.kind, Kind::Glass { .. })
                    && record.rect.intersection(&relief.rect).is_some()
            })
    });
    reliefs.truncate(theme::GLASS_MAX_RELIEFS);
    reliefs
}

/// The reliefs as `set_reliefs` takes them: thirteen native-endian i32 each, lengths in 24.8
/// fixed point.
pub fn encode_reliefs(reliefs: &[Relief]) -> Vec<u8> {
    let byte = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as i32;
    let mut out = Vec::with_capacity(reliefs.len() * 52);
    for relief in reliefs {
        for value in [
            fixed(relief.rect.x),
            fixed(relief.rect.y),
            fixed(relief.rect.width),
            fixed(relief.rect.height),
            fixed(relief.radius),
            match relief.shape {
                Shape::Well => 0,
                Shape::Bead => 1,
            },
            fixed(relief.depth),
            byte(relief.press),
            byte(relief.milk),
            fixed(relief.fill_x),
            fixed(relief.light.x),
            fixed(relief.light.y),
            byte(relief.lit),
        ] {
            out.extend_from_slice(&value.to_ne_bytes());
        }
    }
    out
}

/// What of `records` the compositor hears: at most [`theme::GLASS_MAX_REGIONS`], so a frame full
/// of pictures can't make a request too long for one message (which would cut the bar's
/// connection). The notch and the edge first, then islands, then the pictures that touch one,
/// largest first; one left out can only turn to glass where it's exactly an island's fill.
pub fn fit(mut records: Vec<Record>) -> Vec<Record> {
    records.retain(|record| record.rect.width >= 1.0 && record.rect.height >= 1.0);
    let islands: Vec<Rectangle> = records
        .iter()
        .filter(|record| matches!(record.kind, Kind::Glass { .. }))
        .map(|record| record.rect.expand(theme::GLASS_MARGIN))
        .collect();
    records.retain(|record| {
        matches!(record.kind, Kind::Glass { .. } | Kind::Bezel)
            || islands
                .iter()
                .any(|island| island.intersection(&record.rect).is_some())
    });
    let rank = |record: &Record| match record.kind {
        Kind::Bezel => (2, 0.0),
        Kind::Glass { .. } => (1, 0.0),
        Kind::Solid => (0, record.rect.area()),
    };
    // Stable: islands keep their order.
    records.sort_by(|a, b| {
        let (a, b) = (rank(a), rank(b));
        b.0.cmp(&a.0).then(b.1.total_cmp(&a.1))
    });
    records.truncate(theme::GLASS_MAX_REGIONS);
    records
}

/// The records as `set_regions` takes them: seven native-endian i32 each, rounded outward.
pub fn encode(records: &[Record]) -> Vec<u8> {
    let mut out = Vec::with_capacity(records.len() * 28);
    for record in records {
        let rect = record.rect;
        let (x0, y0) = (rect.x.floor(), rect.y.floor());
        let (x1, y1) = ((rect.x + rect.width).ceil(), (rect.y + rect.height).ceil());
        let (kind, fill, darker) = match record.kind {
            Kind::Glass { fill, darker } => (0, fill, darker),
            Kind::Solid => (1, Color::TRANSPARENT, 0.0),
            Kind::Bezel => (2, Color::TRANSPARENT, 0.0),
        };
        let [r, g, b, a] = fill.into_rgba8();
        let argb = u32::from_be_bytes([a, r, g, b]) as i32;
        for value in [
            x0 as i32,
            y0 as i32,
            (x1 - x0) as i32,
            (y1 - y0) as i32,
            kind,
            argb,
            (darker.clamp(0.0, 1.0) * 255.0).round() as i32,
        ] {
            out.extend_from_slice(&value.to_ne_bytes());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::Size;

    fn words(bytes: &[u8]) -> Vec<i32> {
        bytes
            .chunks(4)
            .map(|word| i32::from_ne_bytes(word.try_into().unwrap()))
            .collect()
    }

    #[test]
    fn records_go_out_rounded_outward_with_their_kind_and_fill() {
        let island = Record {
            rect: Rectangle::new(Point::new(10.5, 4.2), Size::new(20.0, 30.0)),
            kind: Kind::Glass {
                fill: Color::WHITE,
                darker: 1.0,
            },
        };
        let cover = Record {
            rect: Rectangle::new(Point::new(1.0, 2.0), Size::new(3.0, 4.0)),
            kind: Kind::Solid,
        };
        assert_eq!(
            words(&encode(&[island, cover])),
            vec![10, 4, 21, 31, 0, -1, 255, 1, 2, 3, 4, 1, 0, 0]
        );
    }

    #[test]
    fn the_notch_is_recorded_without_glass_and_kept_first() {
        gather(false);
        let notch = Rectangle::new(Point::new(100.0, 0.0), Size::new(200.0, 32.0));
        island(Rectangle::default(), Color::BLACK, 0.0);
        bezel(notch);
        let records = take();
        assert_eq!(
            records,
            vec![Record {
                rect: notch,
                kind: Kind::Bezel
            }]
        );
        let island = Record {
            rect: Rectangle::new(Point::new(0.0, 0.0), Size::new(80.0, 30.0)),
            kind: Kind::Glass {
                fill: Color::BLACK,
                darker: 0.0,
            },
        };
        let mut many = vec![island; theme::GLASS_MAX_REGIONS];
        many.extend(records);
        let fitted = fit(many);
        assert_eq!(fitted[0].kind, Kind::Bezel, "never left out");
        assert_eq!(fitted.len(), theme::GLASS_MAX_REGIONS);
        assert_eq!(words(&encode(&fitted[..1])), vec![100, 0, 200, 32, 2, 0, 0]);
    }

    #[test]
    fn nothing_is_gathered_without_glass() {
        gather(false);
        island(Rectangle::default(), Color::BLACK, 0.0);
        assert!(take().is_empty());
        gather(true);
        island(Rectangle::default(), Color::BLACK, 0.0);
        assert_eq!(take().len(), 1);
        gather(false);
    }

    #[test]
    fn pictures_are_where_they_are_drawn() {
        gather(true);
        let picture = Rectangle::new(Point::new(10.0, 100.0), Size::new(4.0, 4.0));
        // Moved by the island, and scrolled 30 px down a list whose top is at y = 50.
        translated(Vector::new(5.0, 0.0), || {
            scrolled(Point::new(0.0, 50.0), || {
                let viewport = Rectangle::new(Point::new(0.0, 80.0), Size::new(100.0, 100.0));
                super::picture(picture, &viewport, 2.0, 0.5);
            })
        });
        // Squeezed to half about (0, 0) inside an island moved 10 across.
        translated(Vector::new(10.0, 0.0), || {
            squeezed(Point::ORIGIN, 0.5, || {
                super::picture(picture, &Rectangle::INFINITE, 2.0, 1.0)
            })
        });
        let pictures = take_pictures();
        assert_eq!(
            pictures[0],
            Picture {
                rect: Rectangle::new(Point::new(15.0, 70.0), Size::new(4.0, 4.0)),
                radius: 2.0,
                opacity: 0.5,
            }
        );
        assert_eq!(
            pictures[1].rect,
            Rectangle::new(Point::new(15.0, 50.0), Size::new(2.0, 2.0))
        );
        assert_eq!(pictures[1].radius, 1.0);
        assert!(take().is_empty(), "not records");
        gather(false);
    }

    #[test]
    fn pictures_scrolled_out_of_view_are_not_recorded() {
        gather(true);
        let viewport = Rectangle::new(Point::new(0.0, 80.0), Size::new(100.0, 100.0));
        super::picture(
            Rectangle::new(Point::new(10.0, 10.0), Size::new(4.0, 4.0)),
            &viewport,
            0.0,
            1.0,
        );
        assert!(take_pictures().is_empty());
        gather(false);
    }

    #[test]
    fn pictures_go_out_in_fixed_point_with_their_radius_and_opacity() {
        let picture = Picture {
            rect: Rectangle::new(Point::new(1.5, 2.0), Size::new(128.0, 64.0)),
            radius: 40.0,
            opacity: 1.0,
        };
        assert_eq!(
            words(&encode_pictures(&[picture])),
            vec![384, 512, 32768, 16384, 8192, 255],
            "a radius no more than half the shorter side"
        );
    }

    #[test]
    fn pictures_off_every_island_are_left_out_and_the_rest_fit_or_go_as_solids() {
        let island = Record {
            rect: Rectangle::new(Point::new(0.0, 0.0), Size::new(320.0, 400.0)),
            kind: Kind::Glass {
                fill: Color::BLACK,
                darker: 0.0,
            },
        };
        let picture = |x: f32, size: f32| Picture {
            rect: Rectangle::new(Point::new(x, 5.0), Size::new(size, size)),
            radius: 3.0,
            opacity: 1.0,
        };
        let mut pictures: Vec<Picture> = (0..200).map(|i| picture(i as f32, 8.0)).collect();
        pictures.push(picture(1000.0, 50.0));
        pictures.push(picture(10.0, 128.0));
        let fitted = fit_pictures(pictures, &[island]);
        assert_eq!(fitted.len(), theme::GLASS_MAX_PICTURES);
        assert_eq!(fitted[0], picture(10.0, 128.0), "largest first");
        assert!(fitted.iter().all(|picture| picture.rect.x < 1000.0));
        assert!(8 + 4 + encode_pictures(&fitted).len() <= 4096);
        // Before pictures, their whole box, a pixel more all round.
        assert_eq!(
            solids(&[picture(10.0, 4.0)]).collect::<Vec<_>>(),
            vec![Record {
                rect: Rectangle::new(Point::new(9.0, 4.0), Size::new(6.0, 6.0)),
                kind: Kind::Solid,
            }]
        );
    }

    fn well(x: f32) -> Relief {
        Relief {
            rect: Rectangle::new(Point::new(x, 10.0), Size::new(40.0, 20.0)),
            radius: 10.0,
            shape: Shape::Well,
            depth: 2.5,
            press: 0.5,
            milk: 1.0,
            fill_x: x + 20.0,
            light: Point::new(x + 5.0, 15.0),
            lit: 0.25,
        }
    }

    #[test]
    fn reliefs_go_out_in_fixed_point() {
        assert_eq!(
            words(&encode_reliefs(&[well(1.5)])),
            vec![
                384, 2560, 10240, 5120, 2560, 0, 640, 128, 255, 5504, 1664, 3840, 64
            ]
        );
    }

    #[test]
    fn reliefs_are_where_they_are_drawn_and_only_with_glass() {
        gather(false);
        relief(well(0.0), &Rectangle::INFINITE);
        assert!(take_reliefs().is_empty(), "nothing without glass");
        gather(true);
        assert!(on());
        let viewport = Rectangle::new(Point::new(0.0, 0.0), Size::new(30.0, 100.0));
        translated(Vector::new(5.0, 2.0), || relief(well(0.0), &viewport));
        let reliefs = take_reliefs();
        assert_eq!(
            reliefs[0].rect,
            Rectangle::new(Point::new(5.0, 12.0), Size::new(30.0, 20.0)),
            "moved, and cut to what shows"
        );
        assert_eq!(reliefs[0].fill_x, 25.0);
        assert_eq!(reliefs[0].light, Point::new(10.0, 17.0));
        relief(
            well(0.0),
            &Rectangle::new(Point::new(100.0, 0.0), Size::new(9.0, 9.0)),
        );
        assert!(take_reliefs().is_empty(), "out of view, none");

        // Squeezed to half about (0, 0) inside an island moved 10 across, then an item moved 4.
        translated(Vector::new(10.0, 0.0), || {
            squeezed(Point::ORIGIN, 0.5, || {
                translated(Vector::new(4.0, 0.0), || {
                    relief(well(0.0), &Rectangle::INFINITE)
                })
            })
        });
        let squeezed = take_reliefs()[0];
        assert_eq!(squeezed.rect.position(), Point::new(12.0, 5.0));
        assert_eq!(squeezed.rect.size(), Size::new(20.0, 10.0));
        assert_eq!(squeezed.radius, 5.0);
        gather(false);
    }

    #[test]
    fn reliefs_off_every_island_are_left_out_and_the_rest_fit_in_a_message() {
        let island = Record {
            rect: Rectangle::new(Point::new(0.0, 0.0), Size::new(320.0, 400.0)),
            kind: Kind::Glass {
                fill: Color::BLACK,
                darker: 0.0,
            },
        };
        let mut reliefs: Vec<Relief> = (0..200).map(|i| well(i as f32)).collect();
        reliefs.push(well(1000.0));
        let fitted = fit_reliefs(reliefs, &[island]);
        assert_eq!(fitted.len(), theme::GLASS_MAX_RELIEFS);
        assert_eq!(fitted[0], well(0.0), "in the order drawn");
        assert!(fit_reliefs(vec![well(1000.0)], &[island]).is_empty());
        assert!(8 + 4 + encode_reliefs(&fitted).len() <= 4096);
    }

    #[test]
    fn a_frame_full_of_pictures_still_fits_in_a_message() {
        let island = |x: f32| Record {
            rect: Rectangle::new(Point::new(x, 0.0), Size::new(100.0, 40.0)),
            kind: Kind::Glass {
                fill: Color::BLACK,
                darker: 0.0,
            },
        };
        let picture = |i: usize, size: f32| Record {
            rect: Rectangle::new(Point::new(i as f32 % 90.0, 5.0), Size::new(size, size)),
            kind: Kind::Solid,
        };
        let mut records: Vec<Record> = (0..500).map(|i| picture(i, 8.0)).collect();
        records.insert(250, island(0.0));
        records.push(island(200.0));
        records.push(picture(0, 30.0));
        // Far from every island: changes nothing.
        records.push(Record {
            rect: Rectangle::new(Point::new(1000.0, 0.0), Size::new(50.0, 50.0)),
            kind: Kind::Solid,
        });
        let fitted = fit(records);
        assert_eq!(fitted.len(), theme::GLASS_MAX_REGIONS);
        assert_eq!(fitted[0], island(0.0));
        assert_eq!(fitted[1], island(200.0));
        assert_eq!(fitted[2], picture(0, 30.0), "the largest picture next");
        assert!(fitted.iter().all(|record| record.rect.x < 1000.0));
        // libwayland's limit, with the request's header and the array's length.
        assert!(8 + 4 + encode(&fitted).len() <= 4096);
    }
}

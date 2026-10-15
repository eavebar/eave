//! What a frame says about glass, gathered while it's drawn: the islands that may turn to glass,
//! with their fill and which way their ink goes, and the pictures over them that never do; and in
//! every theme, where the notch and the grown edge are, which the pointer goes under (D-113). Sent
//! when the frame is ([`super::Present`]), so it lands with that frame's buffer.

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

thread_local! {
    static RECORDS: RefCell<Vec<Record>> = const { RefCell::new(Vec::new()) };
    static ON: Cell<bool> = const { Cell::new(false) };
    /// How far what's being drawn is moved from its layout, as far as the islands say.
    static OFFSET: Cell<Vector> = const { Cell::new(Vector::new(0.0, 0.0)) };
    /// The top left of the scrollable being drawn, if any: what's in it is moved by how far it's
    /// scrolled, which only its viewport tells.
    static SCROLL: Cell<Option<Point>> = const { Cell::new(None) };
}

/// Whether to gather records this frame: only while there's glass to tell.
pub fn gather(on: bool) {
    ON.with(|cell| cell.set(on));
    if !on {
        RECORDS.with(|records| records.borrow_mut().clear());
    }
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

/// A picture drawn at `bounds` in its layout, with the `viewport` it was given: never glass.
pub fn solid(bounds: Rectangle, viewport: &Rectangle) {
    if !ON.with(Cell::get) {
        return;
    }
    // Only what shows: a long list draws pictures scrolled far out of it too.
    let Some(shown) = bounds.intersection(viewport) else {
        return;
    };
    let scrolled = SCROLL
        .with(Cell::get)
        .map_or(Vector::ZERO, |origin| origin - viewport.position());
    // A pixel more all round: an island's squeeze moves it a little.
    push(Record {
        rect: (shown + OFFSET.with(Cell::get) + scrolled).expand(1.0),
        kind: Kind::Solid,
    });
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
                solid(picture, &viewport);
            })
        });
        let records = take();
        assert_eq!(
            records[0].rect,
            Rectangle::new(Point::new(14.0, 69.0), Size::new(6.0, 6.0))
        );
        gather(false);
    }

    #[test]
    fn pictures_scrolled_out_of_view_are_not_recorded() {
        gather(true);
        let viewport = Rectangle::new(Point::new(0.0, 80.0), Size::new(100.0, 100.0));
        solid(
            Rectangle::new(Point::new(10.0, 10.0), Size::new(4.0, 4.0)),
            &viewport,
        );
        assert!(take().is_empty());
        gather(false);
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

//! What a frame says about glass, gathered while it's drawn: the islands that may turn to glass,
//! with their tone and their fill, and the pictures over them that never do. Sent when the frame
//! is ([`super::Present`]), so it lands with that frame's buffer.

use std::cell::{Cell, RefCell};

use iced::{Color, Point, Rectangle, Vector};

/// Which way an island's ink goes over glass: lighter on a dark island, darker on a light one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tone {
    Dark,
    Light,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    /// An island whose `fill` turns to glass.
    Glass(Tone, Color),
    /// Content that never does, whatever it's over.
    Solid,
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

/// An island at `rect` (surface coordinates, as drawn) that may turn to glass where it's `fill`.
pub fn island(rect: Rectangle, tone: Tone, fill: Color) {
    push(Record {
        rect: rect + OFFSET.with(Cell::get),
        kind: Kind::Glass(tone, fill),
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

/// A picture drawn at `bounds` in its layout, with the `viewport` it was given: never glass.
pub fn solid(bounds: Rectangle, viewport: &Rectangle) {
    if !ON.with(Cell::get) {
        return;
    }
    let scrolled = SCROLL
        .with(Cell::get)
        .map_or(Vector::ZERO, |origin| origin - viewport.position());
    // A pixel more all round: an island's squeeze moves it a little.
    push(Record {
        rect: (bounds + OFFSET.with(Cell::get) + scrolled).expand(1.0),
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

/// The records as `set_regions` takes them: six native-endian i32 each, rounded outward.
pub fn encode(records: &[Record]) -> Vec<u8> {
    let mut out = Vec::with_capacity(records.len() * 24);
    for record in records {
        let rect = record.rect;
        let (x0, y0) = (rect.x.floor(), rect.y.floor());
        let (x1, y1) = ((rect.x + rect.width).ceil(), (rect.y + rect.height).ceil());
        let (kind, fill) = match record.kind {
            Kind::Glass(Tone::Dark, fill) => (0, fill),
            Kind::Glass(Tone::Light, fill) => (1, fill),
            Kind::Solid => (2, Color::TRANSPARENT),
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
            kind: Kind::Glass(Tone::Light, Color::WHITE),
        };
        let cover = Record {
            rect: Rectangle::new(Point::new(1.0, 2.0), Size::new(3.0, 4.0)),
            kind: Kind::Solid,
        };
        assert_eq!(
            words(&encode(&[island, cover])),
            vec![10, 4, 21, 31, 1, -1, 1, 2, 3, 4, 2, 0]
        );
    }

    #[test]
    fn nothing_is_gathered_without_glass() {
        gather(false);
        island(Rectangle::default(), Tone::Dark, Color::BLACK);
        assert!(take().is_empty());
        gather(true);
        island(Rectangle::default(), Tone::Dark, Color::BLACK);
        assert_eq!(take().len(), 1);
        gather(false);
    }

    #[test]
    fn pictures_are_where_they_are_drawn() {
        gather(true);
        let picture = Rectangle::new(Point::new(10.0, 10.0), Size::new(4.0, 4.0));
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
            Rectangle::new(Point::new(14.0, -21.0), Size::new(6.0, 6.0))
        );
        gather(false);
    }
}

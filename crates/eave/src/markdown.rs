//! The host side of [`eave_api::ui::markdown`]: Markdown set in the theme's type and colors.
//!
//! pulldown-cmark reads the text into events, and they become plain iced widgets: a paragraph is a
//! rich text of styled spans, lists and quotes are rows with a marker or a bar, code blocks are
//! monospace on a tinted box that scrolls sideways, colored for their language ([`crate::code`]). Everything is owned, so nothing borrows from
//! the plugin's view. Parsing runs on each view, which is after a message, not on every frame.

use iced::widget::text::{Rich, Span};
use iced::widget::{Space, column, container, row, rule, text};
use iced::{Background, Border, Color, Element, Font, Length, Padding, alignment, font};
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use crate::theme;

/// `source`, as widgets, its text in `color`.
pub fn view<'a, M: 'a>(source: &str, color: Color) -> Element<'a, M> {
    let mut builder = Builder::new(color);
    let options =
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    for event in Parser::new_ext(source, options) {
        builder.event(event);
    }
    builder.finish()
}

/// A block being built, and the blocks inside it so far.
struct Frame<'a, M> {
    kind: FrameKind,
    blocks: Vec<Element<'a, M>>,
}

enum FrameKind {
    Root,
    Quote,
    /// The next item's number, if it's numbered.
    List(Option<u64>),
    /// Its marker: a bullet or a number.
    Item(String),
}

#[derive(Default, Clone, Copy)]
struct Style {
    strong: u8,
    emphasis: u8,
    strike: u8,
    link: u8,
    heading: Option<HeadingLevel>,
}

struct Builder<'a, M> {
    color: Color,
    frames: Vec<Frame<'a, M>>,
    spans: Vec<Span<'static>>,
    style: Style,
    /// Inside a code block: its text so far, and the language its fence names.
    code: Option<String>,
    language: String,
    /// Inside a table: its rows, each a list of cells' text, and whether the first is the head.
    table: Option<Vec<Vec<String>>>,
    cell: Option<String>,
}

impl<'a, M: 'a> Builder<'a, M> {
    fn new(color: Color) -> Self {
        Self {
            color,
            frames: vec![Frame {
                kind: FrameKind::Root,
                blocks: Vec::new(),
            }],
            spans: Vec::new(),
            style: Style::default(),
            code: None,
            language: String::new(),
            table: None,
            cell: None,
        }
    }

    fn event(&mut self, event: Event<'_>) {
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(text) => {
                if let Some(code) = &mut self.code {
                    code.push_str(&text);
                } else if let Some(cell) = &mut self.cell {
                    cell.push_str(&text);
                } else {
                    self.span(text.into_string());
                }
            }
            Event::Code(code) => {
                if let Some(cell) = &mut self.cell {
                    cell.push_str(&code);
                    return;
                }
                let span = Span::new(code.into_string())
                    .font(theme::MONO_FONT)
                    .size(theme::TEXT_SIZE - 1.0)
                    .color(self.color)
                    .background(Background::Color(theme::CODE_BG))
                    .border(Border::default().rounded(3.0))
                    .padding(Padding::from([0, 2]));
                self.spans.push(span);
            }
            Event::SoftBreak => self.span(" ".into()),
            Event::HardBreak => self.span("\n".into()),
            Event::Rule => {
                self.flush();
                self.push(separator());
            }
            Event::TaskListMarker(done) => self.span(if done { "☑ " } else { "☐ " }.into()),
            Event::Html(html) | Event::InlineHtml(html) => {
                let span =
                    Span::new(html.into_string()).color(theme::tone(eave_api::ui::Tone::Dim));
                self.spans.push(span);
            }
            Event::InlineMath(math) | Event::DisplayMath(math) => self.span(math.into_string()),
            Event::FootnoteReference(name) => self.span(format!("[{name}]")),
        }
    }

    fn start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Paragraph => self.flush(),
            Tag::Heading { level, .. } => {
                self.flush();
                self.style.heading = Some(level);
            }
            Tag::BlockQuote(_) => self.open(FrameKind::Quote),
            Tag::CodeBlock(kind) => {
                self.flush();
                self.code = Some(String::new());
                self.language = match kind {
                    // The first word of the fence's info: "rust" of "rust,ignore".
                    CodeBlockKind::Fenced(info) => info
                        .split(|c: char| c == ',' || c.is_whitespace())
                        .next()
                        .unwrap_or_default()
                        .to_string(),
                    CodeBlockKind::Indented => String::new(),
                };
            }
            Tag::List(start) => self.open(FrameKind::List(start)),
            Tag::Item => {
                let marker = match self.frames.last_mut().map(|frame| &mut frame.kind) {
                    Some(FrameKind::List(Some(next))) => {
                        let marker = format!("{next}.");
                        *next += 1;
                        marker
                    }
                    _ => "•".into(),
                };
                self.open(FrameKind::Item(marker));
            }
            Tag::Table(_) => {
                self.flush();
                self.table = Some(Vec::new());
            }
            Tag::TableHead | Tag::TableRow => {
                if let Some(table) = &mut self.table {
                    table.push(Vec::new());
                }
            }
            Tag::TableCell => self.cell = Some(String::new()),
            Tag::Emphasis => self.style.emphasis += 1,
            Tag::Strong => self.style.strong += 1,
            Tag::Strikethrough => self.style.strike += 1,
            Tag::Link { .. } => self.style.link += 1,
            Tag::Image { .. } => self.style.emphasis += 1,
            Tag::HtmlBlock
            | Tag::FootnoteDefinition(_)
            | Tag::DefinitionList
            | Tag::DefinitionListTitle
            | Tag::DefinitionListDefinition
            | Tag::Superscript
            | Tag::Subscript
            | Tag::MetadataBlock(_) => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph | TagEnd::HtmlBlock => self.flush(),
            TagEnd::Heading(_) => {
                self.flush();
                self.style.heading = None;
            }
            TagEnd::BlockQuote(_) | TagEnd::List(_) | TagEnd::Item => self.close(),
            TagEnd::CodeBlock => {
                let code = self.code.take().unwrap_or_default();
                let language = std::mem::take(&mut self.language);
                self.push(crate::code::block(code.trim_end_matches('\n'), &language));
            }
            TagEnd::Table => {
                if let Some(rows) = self.table.take() {
                    self.push(table(rows, self.color));
                }
            }
            TagEnd::TableHead | TagEnd::TableRow => {}
            TagEnd::TableCell => {
                if let (Some(cell), Some(row)) = (
                    self.cell.take(),
                    self.table.as_mut().and_then(|table| table.last_mut()),
                ) {
                    row.push(cell);
                }
            }
            TagEnd::Emphasis | TagEnd::Image => {
                self.style.emphasis = self.style.emphasis.saturating_sub(1);
            }
            TagEnd::Strong => self.style.strong = self.style.strong.saturating_sub(1),
            TagEnd::Strikethrough => self.style.strike = self.style.strike.saturating_sub(1),
            TagEnd::Link => self.style.link = self.style.link.saturating_sub(1),
            _ => {}
        }
    }

    /// A piece of text in the current style.
    fn span(&mut self, content: String) {
        let style = self.style;
        let heavy = style.strong > 0 || style.heading.is_some();
        let font = Font {
            weight: if heavy {
                font::Weight::Bold
            } else {
                font::Weight::Normal
            },
            style: if style.emphasis > 0 {
                font::Style::Italic
            } else {
                font::Style::Normal
            },
            ..Font::DEFAULT
        };
        let size = match style.heading {
            Some(HeadingLevel::H1) => theme::HEADING_SIZE + 1.0,
            Some(HeadingLevel::H2) => theme::HEADING_SIZE,
            Some(_) => theme::TEXT_SIZE + 1.0,
            None => theme::TEXT_SIZE,
        };
        let color = if style.link > 0 {
            theme::tone(eave_api::ui::Tone::Accent)
        } else {
            self.color
        };
        self.spans.push(
            Span::new(content)
                .font(font)
                .size(size)
                .color(color)
                .underline(style.link > 0)
                .strikethrough(style.strike > 0),
        );
    }

    /// The text so far, as a paragraph.
    fn flush(&mut self) {
        if self.spans.is_empty() {
            return;
        }
        let spans = std::mem::take(&mut self.spans);
        let paragraph: Rich<'a, (), M> = Rich::with_spans(spans)
            .size(theme::TEXT_SIZE)
            .line_height(1.35)
            .wrapping(text::Wrapping::WordOrGlyph)
            .width(Length::Fill);
        self.push(paragraph.into());
    }

    fn push(&mut self, block: Element<'a, M>) {
        if let Some(frame) = self.frames.last_mut() {
            frame.blocks.push(block);
        }
    }

    fn open(&mut self, kind: FrameKind) {
        self.flush();
        self.frames.push(Frame {
            kind,
            blocks: Vec::new(),
        });
    }

    fn close(&mut self) {
        self.flush();
        if self.frames.len() < 2 {
            return;
        }
        let Some(frame) = self.frames.pop() else {
            return;
        };
        let block: Element<'a, M> = match frame.kind {
            FrameKind::Root => return,
            FrameKind::List(_) => column(frame.blocks).spacing(3).into(),
            FrameKind::Item(marker) => {
                let marker = container(
                    text(marker)
                        .size(theme::TEXT_SIZE)
                        .line_height(1.35)
                        .color(theme::tone(eave_api::ui::Tone::Dim)),
                )
                .width(18)
                .align_x(alignment::Horizontal::Right);
                row![marker, column(frame.blocks).spacing(theme::BLOCK_SPACING)]
                    .spacing(6)
                    .into()
            }
            FrameKind::Quote => {
                let bar = container(Space::new().width(3).height(Length::Fill)).style(|_| {
                    container::Style {
                        background: Some(Background::Color(theme::QUOTE_BAR)),
                        border: Border::default().rounded(1.5),
                        ..container::Style::default()
                    }
                });
                row![bar, column(frame.blocks).spacing(theme::BLOCK_SPACING)]
                    .spacing(8)
                    .height(Length::Shrink)
                    .into()
            }
        };
        self.push(block);
    }

    fn finish(mut self) -> Element<'a, M> {
        self.flush();
        while self.frames.len() > 1 {
            self.close();
        }
        let root = self
            .frames
            .pop()
            .map(|frame| frame.blocks)
            .unwrap_or_default();
        column(root)
            .spacing(theme::BLOCK_SPACING)
            .width(Length::Fill)
            .into()
    }
}

fn separator<'a, M: 'a>() -> Element<'a, M> {
    rule::horizontal(1)
        .style(|_| rule::Style {
            color: theme::SEPARATOR,
            radius: 0.0.into(),
            fill_mode: rule::FillMode::Full,
            snap: true,
        })
        .into()
}

/// Rows of cells sharing the width, the first row heavier.
fn table<'a, M: 'a>(rows: Vec<Vec<String>>, color: Color) -> Element<'a, M> {
    let columns = rows.iter().map(Vec::len).max().unwrap_or(0).max(1);
    let lines = rows.into_iter().enumerate().map(|(index, cells)| {
        let cells = (0..columns).map(|at| {
            let content = cells.get(at).cloned().unwrap_or_default();
            container(
                text(content)
                    .size(theme::SMALL_SIZE)
                    .font(if index == 0 {
                        theme::STRONG_FONT
                    } else {
                        Font::DEFAULT
                    })
                    .color(color)
                    .wrapping(text::Wrapping::WordOrGlyph),
            )
            .width(Length::FillPortion(1))
            .padding([2, 4])
            .into()
        });
        let line: Element<'a, M> = row(cells).into();
        if index == 0 {
            column![line, separator()].into()
        } else {
            line
        }
    });
    column(lines).spacing(2).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// How many top-level blocks `source` makes.
    fn blocks(source: &str) -> usize {
        let mut builder = Builder::<()>::new(Color::WHITE);
        let options = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH;
        for event in Parser::new_ext(source, options) {
            builder.event(event);
        }
        builder.flush();
        while builder.frames.len() > 1 {
            builder.close();
        }
        builder.frames[0].blocks.len()
    }

    #[test]
    fn blocks_come_out_in_order() {
        assert_eq!(blocks("one\n\ntwo"), 2);
        assert_eq!(blocks("# Title\n\ntext\n\n- a\n- b\n\n```\ncode\n```"), 4);
        assert_eq!(
            blocks("> quoted\n> more\n\n| a | b |\n|---|---|\n| 1 | 2 |"),
            2
        );
    }

    #[test]
    fn unclosed_and_empty_input_is_fine() {
        assert_eq!(blocks(""), 0);
        assert_eq!(blocks("- a\n  - b\n    - c"), 1);
        assert_eq!(blocks("**bold `code` *both*"), 1);
        assert_eq!(blocks("```\nnever closed"), 1);
    }

    #[test]
    fn numbered_items_count_on_from_their_start() {
        let mut builder = Builder::<()>::new(Color::WHITE);
        for event in Parser::new("3. c\n4. d") {
            if let Event::End(TagEnd::Item) = event {
                break;
            }
            builder.event(event);
        }
        let numbers: Vec<_> = builder
            .frames
            .iter()
            .filter_map(|frame| match &frame.kind {
                FrameKind::List(next) => *next,
                _ => None,
            })
            .collect();
        assert_eq!(numbers, [4]);
    }
}

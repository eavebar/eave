//! Code and changes to it: Markdown's code blocks and [`eave_api::ui::diff`], colored by
//! [`highlight`](crate::highlight) (D-65).

use eave_api::ui::{Change, Tone};
use iced::widget::scrollable::{self, Scrollbar};
use iced::widget::text::{Rich, Span};
use iced::widget::{column, container, row, text};
use iced::{Background, Border, Color, Element, Font, Length, Padding, font};

use crate::glide::Glide;
use crate::highlight::{self, Token};
use crate::theme::{self, Palette};

const LINE_HEIGHT: f32 = 1.35;

/// A code block: monospace on a tinted box, colored for its language, its lines numbered once
/// there are a few. Long lines don't wrap: the box scrolls sideways. A `diff` block reads as a
/// [`diff`].
pub fn block<'a, M: 'a>(code: &str, language: &str, p: &'static Palette) -> Element<'a, M> {
    if matches!(language, "diff" | "patch") {
        let lines = code.lines().map(|line| {
            let change = match line.chars().next() {
                Some('+') if !line.starts_with("+++") => Change::Added,
                Some('-') if !line.starts_with("---") => Change::Removed,
                Some('@') => Change::Hunk,
                _ => Change::Context,
            };
            let text = match change {
                Change::Added | Change::Removed => &line[1..],
                _ => line,
            };
            (change, text.to_string())
        });
        return diff(None, lines.collect(), "", p);
    }
    let count = code.lines().count();
    let numbers = (count >= theme::CODE_NUMBERED).then(|| count.to_string().len());
    let number = |n: usize, width: usize| {
        Span::new(format!("{n:>width$}  "))
            .font(theme::MONO_FONT)
            .color(p.code_gutter)
    };
    let mut spans = Vec::new();
    let mut line = 1;
    if let Some(width) = numbers {
        spans.push(number(line, width));
    }
    for (token, range) in highlight::tokens(code, language) {
        // Split at line ends, to number the line after each.
        let mut pieces = code[range].split('\n').peekable();
        while let Some(piece) = pieces.next() {
            if !piece.is_empty() {
                spans.push(span(piece.to_string(), token, p));
            }
            if pieces.peek().is_some() {
                spans.push(Span::new("\n"));
                line += 1;
                if let Some(width) = numbers {
                    spans.push(number(line, width));
                }
            }
        }
    }
    let code: Rich<'a, (), M> = Rich::with_spans(spans)
        .font(theme::MONO_FONT)
        .size(theme::SMALL_SIZE)
        .line_height(LINE_HEIGHT)
        .wrapping(text::Wrapping::None);
    let scroll = scrollable::Scrollable::new(container(code).padding([8, 10]))
        .direction(scrollable::Direction::Horizontal(
            Scrollbar::new().width(3).scroller_width(3).margin(1),
        ))
        .width(Length::Fill);
    container(Glide::new(scroll))
        .width(Length::Fill)
        .style(boxed)
        .into()
}

/// A change to a file: its name and how many lines came and went, then the lines, the added and
/// removed ones on a faint green or red. Lines wrap, so every line's color reaches the box's edge.
pub fn diff<'a, M: 'a>(
    title: Option<String>,
    lines: Vec<(Change, String)>,
    language: &str,
    p: &'static Palette,
) -> Element<'a, M> {
    // Without a language, the file's extension says it.
    let extension = title
        .as_deref()
        .and_then(|title| title.rsplit_once('.'))
        .map(|(_, extension)| extension.to_string());
    let language = match (&extension, language) {
        (Some(extension), "") => extension.as_str(),
        (_, language) => language,
    };
    let count = |change| lines.iter().filter(|(c, _)| *c == change).count();
    let header = title.map(|title| {
        let small = |content: String, color: Color| {
            text(content)
                .font(theme::MONO_FONT)
                .size(theme::SMALL_SIZE)
                .color(color)
        };
        let added = count(Change::Added);
        let removed = count(Change::Removed);
        row![
            container(small(title, p.tone(Tone::Normal)).wrapping(text::Wrapping::WordOrGlyph))
                .width(Length::Fill),
            small(format!("+{added}"), p.diff_added),
            small(format!("−{removed}"), p.diff_removed),
        ]
        .spacing(6)
        .padding([6, 10])
    });
    let rows = lines.into_iter().map(|(change, line)| {
        let (mark, background) = match change {
            Change::Added => ("+", Some(p.diff_added_bg)),
            Change::Removed => ("−", Some(p.diff_removed_bg)),
            Change::Context | Change::Hunk => (" ", None),
        };
        let mark_color = match change {
            Change::Added => p.diff_added,
            Change::Removed => p.diff_removed,
            _ => p.code_gutter,
        };
        let spans = if change == Change::Hunk {
            vec![Span::new(line).color(p.tone(Tone::Dim))]
        } else {
            highlight::tokens(&line, language)
                .into_iter()
                .map(|(token, range)| span(line[range].to_string(), token, p))
                .collect()
        };
        let content: Rich<'a, (), M> = Rich::with_spans(spans)
            .font(theme::MONO_FONT)
            .size(theme::SMALL_SIZE)
            .line_height(LINE_HEIGHT)
            .wrapping(text::Wrapping::WordOrGlyph)
            .width(Length::Fill);
        let mark = text(mark)
            .font(theme::MONO_FONT)
            .size(theme::SMALL_SIZE)
            .line_height(LINE_HEIGHT)
            .color(mark_color)
            .width(Length::Fixed(theme::SMALL_SIZE));
        container(row![mark, content].spacing(4))
            .width(Length::Fill)
            .padding([0, 10])
            .style(move |_| container::Style {
                background: background.map(Background::Color),
                ..container::Style::default()
            })
            .into()
    });
    let body =
        column(rows).padding(
            Padding::ZERO
                .bottom(6)
                .top(if header.is_some() { 0 } else { 6 }),
        );
    container(column![].push(header).push(body))
        .width(Length::Fill)
        .style(boxed)
        .into()
}

/// A piece of code in its token's color.
fn span(content: String, token: Token, p: &Palette) -> Span<'static> {
    let color = match token {
        Token::Plain => p.code_plain,
        Token::Keyword => p.code_keyword,
        Token::Type => p.code_type,
        Token::Function => p.code_function,
        Token::String => p.code_string,
        Token::Number => p.code_number,
        Token::Comment => p.code_comment,
        Token::Punctuation => p.code_punctuation,
    };
    let span = Span::new(content).font(theme::MONO_FONT).color(color);
    if token == Token::Comment {
        span.font(Font {
            style: font::Style::Italic,
            ..theme::MONO_FONT
        })
    } else {
        span
    }
}

fn boxed(theme: &iced::Theme) -> container::Style {
    let p = Palette::of(theme);
    container::Style {
        background: Some(Background::Color(p.code_bg)),
        border: Border::default().rounded(theme::BUTTON_RADIUS),
        ..container::Style::default()
    }
}

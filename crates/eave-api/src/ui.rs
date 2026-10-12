//! The closed UI vocabulary widgets build their views from.
//!
//! Every node is semantic: it says *what* something is (a label, a heading, a button), never how
//! it looks. The host maps nodes to pixels using its theme, so every plugin gets the same style,
//! and the same animations, without being able to break them.

/// The role of a piece of text. The theme decides the color.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Tone {
    #[default]
    Normal,
    /// Secondary information.
    Dim,
    /// The thing to notice: today in a calendar, the active workspace.
    Accent,
    Warning,
    Critical,
}

/// A status symbol. It names what to show and how full it is; the host draws it in the text's
/// tone, at the text's size. Fractions are 0.0 to 1.0.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Icon {
    /// A speaker with sound waves for the level. Above 1.0 is amplified.
    Volume { level: f32, muted: bool },
    /// A Wi-Fi fan with as many bars as the signal. `None` when there's no link.
    Wifi { strength: Option<f32> },
    /// A wired connection.
    Wired,
    /// A battery filled to its charge.
    Battery { charge: f32, charging: bool },
}

/// A piece of UI. Build it with the functions in this module.
pub struct Node<M>(Kind<M>);

#[doc(hidden)]
pub enum Kind<M> {
    Label {
        text: String,
        tone: Option<Tone>,
    },
    Heading {
        text: String,
        tone: Option<Tone>,
    },
    Icon {
        icon: Icon,
        tone: Option<Tone>,
    },
    Row(Vec<Node<M>>),
    Column(Vec<Node<M>>),
    Grid {
        columns: usize,
        cells: Vec<Node<M>>,
    },
    Button {
        content: Box<Node<M>>,
        on_press: M,
    },
    Switcher {
        options: Vec<(Node<M>, M)>,
        current: Option<usize>,
    },
    Separator,
    Empty,
}

pub fn label<M>(text: impl Into<String>) -> Node<M> {
    Node(Kind::Label {
        text: text.into(),
        tone: None,
    })
}

/// A title, larger than a label. Meant for popovers.
pub fn heading<M>(text: impl Into<String>) -> Node<M> {
    Node(Kind::Heading {
        text: text.into(),
        tone: None,
    })
}

/// A symbol, sized like a label.
pub fn icon<M>(icon: Icon) -> Node<M> {
    Node(Kind::Icon { icon, tone: None })
}

/// Children side by side, vertically centered.
pub fn row<M>(children: impl IntoIterator<Item = Node<M>>) -> Node<M> {
    Node(Kind::Row(children.into_iter().collect()))
}

/// Children stacked top to bottom.
pub fn column<M>(children: impl IntoIterator<Item = Node<M>>) -> Node<M> {
    Node(Kind::Column(children.into_iter().collect()))
}

/// Cells laid out left to right in rows of `columns`, all the same width. A calendar, a keypad.
pub fn grid<M>(columns: usize, cells: impl IntoIterator<Item = Node<M>>) -> Node<M> {
    Node(Kind::Grid {
        columns: columns.max(1),
        cells: cells.into_iter().collect(),
    })
}

pub fn button<M>(content: Node<M>, on_press: M) -> Node<M> {
    Node(Kind::Button {
        content: Box::new(content),
        on_press,
    })
}

/// A row of choices, one of them current: workspaces, output devices. Each option sends its
/// message when clicked. The host marks the current option and moves the mark smoothly when
/// `current` changes.
pub fn switcher<M>(
    options: impl IntoIterator<Item = (Node<M>, M)>,
    current: Option<usize>,
) -> Node<M> {
    Node(Kind::Switcher {
        options: options.into_iter().collect(),
        current,
    })
}

/// A thin horizontal line between groups.
pub fn separator<M>() -> Node<M> {
    Node(Kind::Separator)
}

/// Takes up a grid cell, or nothing elsewhere.
pub fn empty<M>() -> Node<M> {
    Node(Kind::Empty)
}

impl<M> Node<M> {
    /// Sets the tone of this text, or of every text inside this node that has none yet.
    pub fn tone(mut self, tone: Tone) -> Self {
        self.set_tone(tone);
        self
    }

    fn set_tone(&mut self, new: Tone) {
        match &mut self.0 {
            Kind::Label { tone, .. } | Kind::Heading { tone, .. } | Kind::Icon { tone, .. } => {
                tone.get_or_insert(new);
            }
            Kind::Row(children)
            | Kind::Column(children)
            | Kind::Grid {
                cells: children, ..
            } => {
                children.iter_mut().for_each(|child| child.set_tone(new));
            }
            Kind::Button { content, .. } => content.set_tone(new),
            Kind::Switcher { options, .. } => {
                options
                    .iter_mut()
                    .for_each(|(child, _)| child.set_tone(new));
            }
            Kind::Separator | Kind::Empty => {}
        }
    }

    pub fn map<N>(self, f: &impl Fn(M) -> N) -> Node<N> {
        let map_all = |nodes: Vec<Node<M>>| nodes.into_iter().map(|node| node.map(f)).collect();

        Node(match self.0 {
            Kind::Label { text, tone } => Kind::Label { text, tone },
            Kind::Heading { text, tone } => Kind::Heading { text, tone },
            Kind::Icon { icon, tone } => Kind::Icon { icon, tone },
            Kind::Row(children) => Kind::Row(map_all(children)),
            Kind::Column(children) => Kind::Column(map_all(children)),
            Kind::Grid { columns, cells } => Kind::Grid {
                columns,
                cells: map_all(cells),
            },
            Kind::Button { content, on_press } => Kind::Button {
                content: Box::new(content.map(f)),
                on_press: f(on_press),
            },
            Kind::Switcher { options, current } => Kind::Switcher {
                options: options
                    .into_iter()
                    .map(|(node, message)| (node.map(f), f(message)))
                    .collect(),
                current,
            },
            Kind::Separator => Kind::Separator,
            Kind::Empty => Kind::Empty,
        })
    }

    #[doc(hidden)]
    pub fn into_kind(self) -> Kind<M> {
        self.0
    }
}

/// What a widget shows in the bar.
pub struct Item<M> {
    content: Option<Node<M>>,
    on_press: Option<M>,
    on_secondary: Option<M>,
    on_scroll: Option<(M, M)>,
}

impl<M> Item<M> {
    pub fn new(content: Node<M>) -> Self {
        Self {
            content: Some(content),
            on_press: None,
            on_secondary: None,
            on_scroll: None,
        }
    }

    /// Takes no space in the bar. For widgets with nothing to show, like a battery on a desktop.
    pub fn hidden() -> Self {
        Self {
            content: None,
            on_press: None,
            on_secondary: None,
            on_scroll: None,
        }
    }

    /// Sent on a primary click. Replaces the default of toggling the popover.
    pub fn on_press(mut self, message: M) -> Self {
        self.on_press = Some(message);
        self
    }

    /// Sent on a secondary (right) click.
    pub fn on_secondary(mut self, message: M) -> Self {
        self.on_secondary = Some(message);
        self
    }

    /// Sent once per scroll step over the item, `up` or `down`. The host turns smooth touchpad
    /// scrolling into steps.
    pub fn on_scroll(mut self, up: M, down: M) -> Self {
        self.on_scroll = Some((up, down));
        self
    }

    pub fn map<N>(self, f: &impl Fn(M) -> N) -> Item<N> {
        Item {
            content: self.content.map(|content| content.map(f)),
            on_press: self.on_press.map(f),
            on_secondary: self.on_secondary.map(f),
            on_scroll: self.on_scroll.map(|(up, down)| (f(up), f(down))),
        }
    }

    #[doc(hidden)]
    pub fn into_parts(self) -> ItemParts<M> {
        ItemParts {
            content: self.content,
            on_press: self.on_press,
            on_secondary: self.on_secondary,
            on_scroll: self.on_scroll,
        }
    }
}

#[doc(hidden)]
pub struct ItemParts<M> {
    pub content: Option<Node<M>>,
    pub on_press: Option<M>,
    pub on_secondary: Option<M>,
    /// Up, then down.
    pub on_scroll: Option<(M, M)>,
}

/// The contents of a popover. The host draws the surface, its background and its edges.
pub struct Panel<M>(Node<M>);

impl<M> Panel<M> {
    pub fn new(content: Node<M>) -> Self {
        Self(content)
    }

    pub fn map<N>(self, f: &impl Fn(M) -> N) -> Panel<N> {
        Panel(self.0.map(f))
    }

    #[doc(hidden)]
    pub fn into_node(self) -> Node<M> {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_converts_every_message() {
        let node = row([
            button(label("a"), 1),
            column([button(label("b"), 2), label("c")]),
            switcher([(label("d"), 3)], Some(0)),
        ]);

        let Kind::Row(children) = node.map(&|n: i32| n * 10).into_kind() else {
            panic!("row became something else");
        };
        let mut messages = Vec::new();
        collect(children, &mut messages);
        assert_eq!(messages, [10, 20, 30]);
    }

    fn collect(nodes: Vec<Node<i32>>, out: &mut Vec<i32>) {
        for node in nodes {
            match node.into_kind() {
                Kind::Button { on_press, .. } => out.push(on_press),
                Kind::Switcher { options, .. } => {
                    out.extend(options.into_iter().map(|(_, message)| message))
                }
                Kind::Row(children) | Kind::Column(children) => collect(children, out),
                _ => {}
            }
        }
    }

    #[test]
    fn tone_fills_only_unset_text() {
        let node: Node<()> =
            row([label("a").tone(Tone::Accent), label("b"), icon(Icon::Wired)]).tone(Tone::Dim);
        let Kind::Row(children) = node.into_kind() else {
            unreachable!()
        };
        let tones: Vec<_> = children
            .into_iter()
            .map(|child| match child.into_kind() {
                Kind::Label { tone, .. } | Kind::Icon { tone, .. } => tone,
                _ => None,
            })
            .collect();
        assert_eq!(
            tones,
            [Some(Tone::Accent), Some(Tone::Dim), Some(Tone::Dim)]
        );
    }
}

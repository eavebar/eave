//! The closed UI vocabulary widgets build their views from.
//!
//! Every node is semantic: it says *what* something is (a label, a heading, a button), never how
//! it looks. The host maps nodes to pixels using its theme, so every plugin gets the same style,
//! and the same animations, without being able to break them.

use std::ops::RangeInclusive;

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
    /// Healthy, or gaining: a charging battery.
    Good,
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
    /// A padlock: a secured network.
    Lock,
    /// An `i` in a circle: more about this.
    Info,
    /// A chevron pointing left: back, or earlier.
    Back,
    /// A chevron pointing right: on, or later.
    Forward,
    /// A check mark: the one in use.
    Check,
    /// A circling arrow: look again.
    Refresh,
    /// A cross: close this.
    Close,
    /// An eye: show what's hidden.
    Reveal,
    /// A lightning bolt: charging.
    Bolt,
    /// A power plug: on mains power.
    Plug,
    /// A microphone, struck through when muted.
    Mic { muted: bool },
    /// A loudspeaker box: built-in speakers.
    Speaker,
    /// Headphones, or a headset.
    Headphones,
    /// A screen: a monitor or TV that plays sound.
    Display,
    /// A bell: notifications, struck through when they're silenced.
    Bell { muted: bool },
    /// Three level bars, like an equalizer: media. They dance while `playing` and rest low when
    /// not.
    Media { playing: bool },
    /// A triangle pointing right: play.
    Play,
    /// Two upright bars: pause.
    Pause,
    /// A triangle pointing left against a bar: back to the previous track.
    Previous,
    /// A triangle pointing right against a bar: on to the next track.
    Next,
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
    AppIcon {
        app_id: String,
        tone: Option<Tone>,
    },
    Image {
        file: &'static [u8],
        tint: bool,
        tone: Option<Tone>,
    },
    Counter {
        value: i64,
        range: RangeInclusive<i64>,
        unit: String,
        tone: Option<Tone>,
    },
    Alternate {
        options: Vec<Node<M>>,
        current: usize,
        /// Shown instead of `current` while the pointer is over the item.
        hovered: Option<usize>,
        /// As wide as the option on screen, hovered by its own width, after a rest ([`expand`]).
        grows: bool,
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
        on_secondary: Option<M>,
        /// The main action of a popover page: filled with the accent.
        prominent: bool,
    },
    Switcher {
        options: Vec<(Node<M>, M)>,
        current: Option<usize>,
    },
    Gulp(Box<Node<M>>),
    Separator,
    Empty,
    ListItem {
        title: String,
        subtitle: Option<String>,
        leading: Option<Box<Node<M>>>,
        trailing: Option<Box<Node<M>>>,
        selected: bool,
        on_press: Option<M>,
        on_secondary: Option<M>,
    },
    List(Vec<Node<M>>),
    Section {
        title: String,
        trailing: Option<Box<Node<M>>>,
    },
    Header {
        title: String,
        on_back: Option<M>,
        trailing: Option<Box<Node<M>>>,
    },
    Detail {
        name: String,
        value: Box<Node<M>>,
    },
    Fill,
    Slider {
        value: f32,
        range: RangeInclusive<f32>,
        step: f32,
        on_change: Box<dyn Fn(f32) -> M>,
        on_release: Option<M>,
    },
    Toggle {
        on: bool,
        on_toggle: M,
    },
    Meter {
        fraction: f32,
        /// Where a limit sits along it, like a battery's charge limit.
        limit: Option<f32>,
        tone: Option<Tone>,
    },
    TextInput {
        placeholder: String,
        value: String,
        on_input: Box<dyn Fn(String) -> M>,
        on_submit: Option<M>,
        secret: bool,
        trailing: Option<Box<Node<M>>>,
        tone: Option<Tone>,
    },
    Display(Box<Node<M>>),
    Mark(Box<Node<M>>),
    Badge {
        text: String,
        tone: Option<Tone>,
    },
    /// Pages and their names. The host shows one at a time and owns which.
    Tabs(Vec<(String, Node<M>)>),
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

/// An application's icon, found by its app id or window class (`firefox`, `org.gnome.Nautilus`)
/// in the user's icon theme. Sized like an [`icon`], in the app's own colors: a tone only dims it.
/// Until the host has found it, or if there is none, it shows the id's first letter.
pub fn app_icon<M>(app_id: impl Into<String>) -> Node<M> {
    Node(Kind::AppIcon {
        app_id: app_id.into(),
        tone: None,
    })
}

/// An icon the plugin ships: an SVG or PNG file compiled into it with `include_bytes!`, told
/// apart by its first bytes. Sized like an [`icon`], in the file's own colors: a tone only dims
/// it, as with [`app_icon`]. The host draws it once, off the UI thread, and shows nothing in its
/// place until then.
pub fn image<M>(file: &'static [u8]) -> Node<M> {
    Node(Kind::Image {
        file,
        tint: false,
        tone: None,
    })
}

/// Like [`image`], but only the file's shape counts: it's drawn in the text's tone, like an
/// [`icon`]. For a monochrome symbol the vocabulary doesn't have.
pub fn symbol<M>(file: &'static [u8]) -> Node<M> {
    Node(Kind::Image {
        file,
        tint: true,
        tone: None,
    })
}

/// A number from `range`, with a unit after it (`"%"`, `"°"`, or `""`), shown like a label. It
/// keeps room for the widest number in `range`, so it doesn't change width as it changes. When
/// `value` changes, the host counts to it through every number in between.
pub fn counter<M>(value: i64, range: RangeInclusive<i64>, unit: impl Into<String>) -> Node<M> {
    Node(Kind::Counter {
        value,
        range,
        unit: unit.into(),
        tone: None,
    })
}

/// One of `options` at a time: the one at `current`, or the last one if it's past the end. It's as
/// big as the biggest option, so it doesn't change size when `current` does. The host fades from
/// one to the other. Keep the options the same from one
/// view to the next, so the host can tell which one is coming in.
pub fn alternate<M>(options: impl IntoIterator<Item = Node<M>>, current: usize) -> Node<M> {
    Node(Kind::Alternate {
        options: options.into_iter().collect(),
        current,
        hovered: None,
        grows: false,
    })
}

/// `normal`, or `hovered` while the pointer is over the item, fading between them like an
/// [`alternate`]: an icon that shows its percent, a time that shows the date.
pub fn hover<M>(normal: Node<M>, hovered: Node<M>) -> Node<M> {
    alternate([normal, hovered], 0).on_hover(1)
}

/// `collapsed`, or `expanded` once the pointer rests on this node, fading between them like a
/// [`hover`]. Unlike `hover`, it's only as big as what it shows, so the item grows to make room
/// and the island springs along, and it reacts to the pointer over it, not the whole item: a
/// workspace that shows every window when you point at it.
pub fn expand<M>(collapsed: Node<M>, expanded: Node<M>) -> Node<M> {
    Node(Kind::Alternate {
        options: vec![collapsed, expanded],
        current: 0,
        hovered: Some(1),
        grows: true,
    })
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
        on_secondary: None,
        prominent: false,
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

/// Keeps gulping, like a bottle being poured: the host swells `content` gently and steadily for as
/// long as it's on screen. For something filling up, like a charging battery's percent.
pub fn gulp<M>(content: Node<M>) -> Node<M> {
    Node(Kind::Gulp(Box::new(content)))
}

/// A thin horizontal line between groups.
pub fn separator<M>() -> Node<M> {
    Node(Kind::Separator)
}

/// Takes up a grid cell, or nothing elsewhere.
pub fn empty<M>() -> Node<M> {
    Node(Kind::Empty)
}

/// A row in a popover list: a network, a device, a window. Give it a [`subtitle`](Node::subtitle),
/// a [`leading`](Node::leading) icon, a [`trailing`](Node::trailing) node at the far end (a lock,
/// an info button), mark the one in use with [`selected`](Node::selected), and make it clickable
/// with [`on_press`](Node::on_press). A long title fades out at the end rather than wrapping.
pub fn list_item<M>(title: impl Into<String>) -> Node<M> {
    Node(Kind::ListItem {
        title: title.into(),
        subtitle: None,
        leading: None,
        trailing: None,
        selected: false,
        on_press: None,
        on_secondary: None,
    })
}

/// Rows stacked closely, usually [`list_item`]s. In a popover, a long list scrolls instead of
/// growing past a few rows.
pub fn list<M>(rows: impl IntoIterator<Item = Node<M>>) -> Node<M> {
    Node(Kind::List(rows.into_iter().collect()))
}

/// A small caption over a group in a popover ("Output", "Health"), with an optional
/// [`trailing`](Node::trailing) node at the far end.
pub fn section<M>(title: impl Into<String>) -> Node<M> {
    Node(Kind::Section {
        title: title.into(),
        trailing: None,
    })
}

/// The title of a popover page. Sub-pages get a back button with [`on_back`](Node::on_back); a
/// [`trailing`](Node::trailing) node sits at the far end (a switch, a count).
pub fn header<M>(title: impl Into<String>) -> Node<M> {
    Node(Kind::Header {
        title: title.into(),
        on_back: None,
        trailing: None,
    })
}

/// A named value on one line: the name dimmed at the start, the value at the end.
pub fn detail<M>(name: impl Into<String>, value: Node<M>) -> Node<M> {
    Node(Kind::Detail {
        name: name.into(),
        value: Box::new(value),
    })
}

/// Takes the free space in a popover row, pushing what follows to the end. Nothing in the bar.
pub fn fill<M>() -> Node<M> {
    Node(Kind::Fill)
}

/// A value dragged along `range`: a volume. Sends `on_change` while dragging; add
/// [`on_release`](Node::on_release) to act once it's let go, and [`step`](Node::step) to snap.
pub fn slider<M: 'static>(
    value: f32,
    range: RangeInclusive<f32>,
    on_change: impl Fn(f32) -> M + 'static,
) -> Node<M> {
    Node(Kind::Slider {
        value,
        range,
        step: 1.0,
        on_change: Box::new(on_change),
        on_release: None,
    })
}

/// An on/off switch. Sends `on_toggle` when clicked; the host slides it over.
pub fn toggle<M>(on: bool, on_toggle: M) -> Node<M> {
    Node(Kind::Toggle { on, on_toggle })
}

/// A bar filled to `fraction` (0.0 to 1.0), in its tone: a charge, a signal. The host eases it to
/// a new value.
pub fn meter<M>(fraction: f32) -> Node<M> {
    Node(Kind::Meter {
        fraction,
        limit: None,
        tone: None,
    })
}

/// A field to type into, showing `value`. Sends `on_input` with the new text on every change.
/// It takes the focus when it appears, and while it has it, keys stay in the popover; Esc still
/// closes it. [`on_submit`](Node::on_submit) is sent on Enter. One per popover page. A
/// [`Tone::Critical`] tone marks it as wrong.
pub fn text_input<M: 'static>(
    placeholder: impl Into<String>,
    value: impl Into<String>,
    on_input: impl Fn(String) -> M + 'static,
) -> Node<M> {
    Node(Kind::TextInput {
        placeholder: placeholder.into(),
        value: value.into(),
        on_input: Box::new(on_input),
        on_submit: None,
        secret: false,
        trailing: None,
        tone: None,
    })
}

/// Its text, large: the one number a popover is about, like a battery's charge.
pub fn display<M>(content: Node<M>) -> Node<M> {
    Node(Kind::Display(Box::new(content)))
}

/// Its content on a filled accent circle: today in a calendar.
pub fn mark<M>(content: Node<M>) -> Node<M> {
    Node(Kind::Mark(Box::new(content)))
}

/// A short code in a small outlined box: a keyboard layout's `US`.
pub fn badge<M>(text: impl Into<String>) -> Node<M> {
    Node(Kind::Badge {
        text: text.into(),
        tone: None,
    })
}

/// Pages behind a row of tabs, one page showing at a time: a popover's Output, Input and Apps.
/// The host draws the tabs, switches pages when one is clicked and starts on the first each time
/// the popover opens, so the plugin keeps no state for it. In the bar, only the first page shows.
pub fn tabs<M>(pages: impl IntoIterator<Item = (impl Into<String>, Node<M>)>) -> Node<M> {
    Node(Kind::Tabs(
        pages
            .into_iter()
            .map(|(name, page)| (name.into(), page))
            .collect(),
    ))
}

impl<M> Node<M> {
    /// Sets the tone of this text, or of every text inside this node that has none yet.
    pub fn tone(mut self, tone: Tone) -> Self {
        self.set_tone(tone);
        self
    }

    /// For an [`alternate`]: shows the option at `index` while the pointer is over the item,
    /// whatever `current` is. Other nodes have nothing to show on hover and ignore it.
    pub fn on_hover(mut self, index: usize) -> Self {
        if let Kind::Alternate { hovered, .. } = &mut self.0 {
            *hovered = Some(index);
        }
        self
    }

    /// For a [`button`] or [`list_item`]: sent on a secondary (right) click on it. Other nodes
    /// ignore it.
    pub fn on_secondary(mut self, message: M) -> Self {
        if let Kind::Button { on_secondary, .. } | Kind::ListItem { on_secondary, .. } = &mut self.0
        {
            *on_secondary = Some(message);
        }
        self
    }

    /// For a [`button`]: the main action of its page, filled with the accent. Other nodes ignore
    /// it.
    pub fn prominent(mut self) -> Self {
        if let Kind::Button { prominent, .. } = &mut self.0 {
            *prominent = true;
        }
        self
    }

    /// For a [`list_item`]: sent on a click on it. Other nodes ignore it.
    pub fn on_press(mut self, message: M) -> Self {
        if let Kind::ListItem { on_press, .. } = &mut self.0 {
            *on_press = Some(message);
        }
        self
    }

    /// For a [`list_item`]: a second, dimmer line under the title. Other nodes ignore it.
    pub fn subtitle(mut self, text: impl Into<String>) -> Self {
        if let Kind::ListItem { subtitle, .. } = &mut self.0 {
            *subtitle = Some(text.into());
        }
        self
    }

    /// For a [`list_item`]: an icon before the title. Other nodes ignore it.
    pub fn leading(mut self, node: Node<M>) -> Self {
        if let Kind::ListItem { leading, .. } = &mut self.0 {
            *leading = Some(Box::new(node));
        }
        self
    }

    /// For a [`list_item`], [`section`], [`header`] or [`text_input`]: a node at the far end, like
    /// a lock or an info button. Other nodes ignore it.
    pub fn trailing(mut self, node: Node<M>) -> Self {
        if let Kind::ListItem { trailing, .. }
        | Kind::Section { trailing, .. }
        | Kind::Header { trailing, .. }
        | Kind::TextInput { trailing, .. } = &mut self.0
        {
            *trailing = Some(Box::new(node));
        }
        self
    }

    /// For a [`list_item`]: marks it as the one in use. Other nodes ignore it.
    pub fn selected(mut self, yes: bool) -> Self {
        if let Kind::ListItem { selected, .. } = &mut self.0 {
            *selected = yes;
        }
        self
    }

    /// For a [`header`]: shows a back button that sends `message`. Other nodes ignore it.
    pub fn on_back(mut self, message: M) -> Self {
        if let Kind::Header { on_back, .. } = &mut self.0 {
            *on_back = Some(message);
        }
        self
    }

    /// For a [`slider`]: the step its value snaps to (1 by default). Other nodes ignore it.
    pub fn step(mut self, by: f32) -> Self {
        if let Kind::Slider { step, .. } = &mut self.0 {
            *step = by;
        }
        self
    }

    /// For a [`slider`]: sent when it's let go. Other nodes ignore it.
    pub fn on_release(mut self, message: M) -> Self {
        if let Kind::Slider { on_release, .. } = &mut self.0 {
            *on_release = Some(message);
        }
        self
    }

    /// For a [`text_input`]: sent on Enter. Other nodes ignore it.
    pub fn on_submit(mut self, message: M) -> Self {
        if let Kind::TextInput { on_submit, .. } = &mut self.0 {
            *on_submit = Some(message);
        }
        self
    }

    /// For a [`text_input`]: hides what's typed, like a password. Other nodes ignore it.
    pub fn secret(mut self, yes: bool) -> Self {
        if let Kind::TextInput { secret, .. } = &mut self.0 {
            *secret = yes;
        }
        self
    }

    /// For a [`meter`]: marks a limit along it (0.0 to 1.0). Other nodes ignore it.
    pub fn limit(mut self, at: f32) -> Self {
        if let Kind::Meter { limit, .. } = &mut self.0 {
            *limit = Some(at);
        }
        self
    }

    fn set_tone(&mut self, new: Tone) {
        match &mut self.0 {
            Kind::Label { tone, .. }
            | Kind::Heading { tone, .. }
            | Kind::Icon { tone, .. }
            | Kind::AppIcon { tone, .. }
            | Kind::Image { tone, .. }
            | Kind::Counter { tone, .. }
            | Kind::Meter { tone, .. }
            | Kind::TextInput { tone, .. }
            | Kind::Badge { tone, .. } => {
                tone.get_or_insert(new);
            }
            Kind::Alternate {
                options: children, ..
            }
            | Kind::Row(children)
            | Kind::Column(children)
            | Kind::Grid {
                cells: children, ..
            }
            | Kind::List(children) => {
                children.iter_mut().for_each(|child| child.set_tone(new));
            }
            Kind::Button { content, .. }
            | Kind::Gulp(content)
            | Kind::Display(content)
            | Kind::Mark(content)
            | Kind::Detail { value: content, .. } => content.set_tone(new),
            Kind::ListItem {
                leading, trailing, ..
            } => {
                leading
                    .iter_mut()
                    .chain(trailing.iter_mut())
                    .for_each(|child| child.set_tone(new));
            }
            Kind::Tabs(pages) => {
                for (_, page) in pages {
                    page.set_tone(new);
                }
            }
            Kind::Switcher { options, .. } => {
                options
                    .iter_mut()
                    .for_each(|(child, _)| child.set_tone(new));
            }
            Kind::Separator
            | Kind::Empty
            | Kind::Section { .. }
            | Kind::Header { .. }
            | Kind::Fill
            | Kind::Slider { .. }
            | Kind::Toggle { .. } => {}
        }
    }

    /// Converts every message in this node. Sliders and text fields keep `f` to convert the
    /// messages they make later, so it must be `Clone + 'static`, like a function or a closure
    /// that captures nothing.
    pub fn map<N: 'static>(self, f: &(impl Fn(M) -> N + Clone + 'static)) -> Node<N>
    where
        M: 'static,
    {
        let map_all = |nodes: Vec<Node<M>>| nodes.into_iter().map(|node| node.map(f)).collect();
        let map_box = |node: Option<Box<Node<M>>>| node.map(|node| Box::new(node.map(f)));

        Node(match self.0 {
            Kind::Label { text, tone } => Kind::Label { text, tone },
            Kind::Heading { text, tone } => Kind::Heading { text, tone },
            Kind::Icon { icon, tone } => Kind::Icon { icon, tone },
            Kind::AppIcon { app_id, tone } => Kind::AppIcon { app_id, tone },
            Kind::Image { file, tint, tone } => Kind::Image { file, tint, tone },
            Kind::Counter {
                value,
                range,
                unit,
                tone,
            } => Kind::Counter {
                value,
                range,
                unit,
                tone,
            },
            Kind::Alternate {
                options,
                current,
                hovered,
                grows,
            } => Kind::Alternate {
                options: map_all(options),
                current,
                hovered,
                grows,
            },
            Kind::Row(children) => Kind::Row(map_all(children)),
            Kind::Column(children) => Kind::Column(map_all(children)),
            Kind::Grid { columns, cells } => Kind::Grid {
                columns,
                cells: map_all(cells),
            },
            Kind::Button {
                content,
                on_press,
                on_secondary,
                prominent,
            } => Kind::Button {
                content: Box::new(content.map(f)),
                on_press: f(on_press),
                on_secondary: on_secondary.map(f),
                prominent,
            },
            Kind::Switcher { options, current } => Kind::Switcher {
                options: options
                    .into_iter()
                    .map(|(node, message)| (node.map(f), f(message)))
                    .collect(),
                current,
            },
            Kind::Gulp(content) => Kind::Gulp(Box::new(content.map(f))),
            Kind::Separator => Kind::Separator,
            Kind::Empty => Kind::Empty,
            Kind::ListItem {
                title,
                subtitle,
                leading,
                trailing,
                selected,
                on_press,
                on_secondary,
            } => Kind::ListItem {
                title,
                subtitle,
                leading: map_box(leading),
                trailing: map_box(trailing),
                selected,
                on_press: on_press.map(f),
                on_secondary: on_secondary.map(f),
            },
            Kind::List(rows) => Kind::List(map_all(rows)),
            Kind::Section { title, trailing } => Kind::Section {
                title,
                trailing: map_box(trailing),
            },
            Kind::Header {
                title,
                on_back,
                trailing,
            } => Kind::Header {
                title,
                on_back: on_back.map(f),
                trailing: map_box(trailing),
            },
            Kind::Detail { name, value } => Kind::Detail {
                name,
                value: Box::new(value.map(f)),
            },
            Kind::Fill => Kind::Fill,
            Kind::Slider {
                value,
                range,
                step,
                on_change,
                on_release,
            } => {
                let g = f.clone();
                Kind::Slider {
                    value,
                    range,
                    step,
                    on_change: Box::new(move |value| g(on_change(value))),
                    on_release: on_release.map(f),
                }
            }
            Kind::Toggle { on, on_toggle } => Kind::Toggle {
                on,
                on_toggle: f(on_toggle),
            },
            Kind::Meter {
                fraction,
                limit,
                tone,
            } => Kind::Meter {
                fraction,
                limit,
                tone,
            },
            Kind::TextInput {
                placeholder,
                value,
                on_input,
                on_submit,
                secret,
                trailing,
                tone,
            } => {
                let g = f.clone();
                Kind::TextInput {
                    placeholder,
                    value,
                    on_input: Box::new(move |text| g(on_input(text))),
                    on_submit: on_submit.map(f),
                    secret,
                    trailing: map_box(trailing),
                    tone,
                }
            }
            Kind::Display(content) => Kind::Display(Box::new(content.map(f))),
            Kind::Mark(content) => Kind::Mark(Box::new(content.map(f))),
            Kind::Badge { text, tone } => Kind::Badge { text, tone },
            Kind::Tabs(pages) => Kind::Tabs(
                pages
                    .into_iter()
                    .map(|(name, page)| (name, page.map(f)))
                    .collect(),
            ),
        })
    }

    #[doc(hidden)]
    pub fn into_kind(self) -> Kind<M> {
        self.0
    }

    #[doc(hidden)]
    pub fn kind(&self) -> &Kind<M> {
        &self.0
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

    pub fn map<N: 'static>(self, f: &(impl Fn(M) -> N + Clone + 'static)) -> Item<N>
    where
        M: 'static,
    {
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

/// The contents of a popover. The host draws the surface, its background and its edges, and lays
/// every popover out at the same width.
pub struct Panel<M> {
    content: Node<M>,
    on_open: Option<M>,
    on_close: Option<M>,
}

impl<M> Panel<M> {
    pub fn new(content: Node<M>) -> Self {
        Self {
            content,
            on_open: None,
            on_close: None,
        }
    }

    /// Sent each time the popover opens: start a scan, show the first page.
    pub fn on_open(mut self, message: M) -> Self {
        self.on_open = Some(message);
        self
    }

    /// Sent each time the popover starts closing: stop listening, forget what was typed.
    pub fn on_close(mut self, message: M) -> Self {
        self.on_close = Some(message);
        self
    }

    pub fn map<N: 'static>(self, f: &(impl Fn(M) -> N + Clone + 'static)) -> Panel<N>
    where
        M: 'static,
    {
        Panel {
            content: self.content.map(f),
            on_open: self.on_open.map(f),
            on_close: self.on_close.map(f),
        }
    }

    #[doc(hidden)]
    pub fn into_node(self) -> Node<M> {
        self.content
    }

    #[doc(hidden)]
    pub fn on_open_message(self) -> Option<M> {
        self.on_open
    }

    #[doc(hidden)]
    pub fn on_close_message(self) -> Option<M> {
        self.on_close
    }
}

/// How much a [`Notice`] matters. The host decides what that looks like.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Urgency {
    /// Background news: a download finished.
    Low,
    #[default]
    Normal,
    /// Needs attention now: a battery about to die.
    Critical,
}

/// A notification to show on screen, from [`Widget::notices`](crate::Widget::notices).
///
/// It says what the notification is, not how it looks: the host lays out the notification, places it and
/// animates it in and out. Its `key` is how the host tells notices apart from one view to the
/// next: a notice that keeps its key and changes its text is updated in place, and one whose key
/// is gone from the list leaves the screen.
pub struct Notice<M> {
    key: u64,
    summary: String,
    app: Option<String>,
    app_icon: Option<String>,
    body: Option<String>,
    urgency: Urgency,
    actions: Vec<(String, M)>,
    on_press: Option<M>,
    on_dismiss: Option<M>,
    expires: Option<(std::time::Duration, M)>,
    reply: Option<Reply<M>>,
}

/// A field on a [`Notice`] to type an answer into, like a reply to a message.
///
/// While a notice shows one, the host takes the keyboard, so show it only once asked to (after
/// a click on a Reply button, say), and drop it once the answer is sent or cancelled. The
/// notice doesn't expire meanwhile.
pub struct Reply<M> {
    placeholder: String,
    value: String,
    on_input: std::rc::Rc<dyn Fn(String) -> M>,
    on_submit: Option<M>,
    on_cancel: Option<M>,
}

impl<M> Reply<M> {
    /// Shows `value`, and sends `on_input` with the new text on every change.
    pub fn new(
        placeholder: impl Into<String>,
        value: impl Into<String>,
        on_input: impl Fn(String) -> M + 'static,
    ) -> Self {
        Self {
            placeholder: placeholder.into(),
            value: value.into(),
            on_input: std::rc::Rc::new(on_input),
            on_submit: None,
            on_cancel: None,
        }
    }

    /// Sent on Enter, or a click on the field's send button.
    pub fn on_submit(mut self, message: M) -> Self {
        self.on_submit = Some(message);
        self
    }

    /// Sent on Esc.
    pub fn on_cancel(mut self, message: M) -> Self {
        self.on_cancel = Some(message);
        self
    }

    pub fn map<N: 'static>(self, f: &(impl Fn(M) -> N + Clone + 'static)) -> Reply<N>
    where
        M: 'static,
    {
        let g = f.clone();
        let on_input = self.on_input;
        Reply {
            placeholder: self.placeholder,
            value: self.value,
            on_input: std::rc::Rc::new(move |text| g(on_input(text))),
            on_submit: self.on_submit.map(f),
            on_cancel: self.on_cancel.map(f),
        }
    }

    #[doc(hidden)]
    pub fn into_parts(self) -> ReplyParts<M> {
        ReplyParts {
            placeholder: self.placeholder,
            value: self.value,
            on_input: self.on_input,
            on_submit: self.on_submit,
            on_cancel: self.on_cancel,
        }
    }
}

#[doc(hidden)]
#[derive(Clone)]
pub struct ReplyParts<M> {
    pub placeholder: String,
    pub value: String,
    pub on_input: std::rc::Rc<dyn Fn(String) -> M>,
    pub on_submit: Option<M>,
    pub on_cancel: Option<M>,
}

impl<M> Notice<M> {
    pub fn new(key: u64, summary: impl Into<String>) -> Self {
        Self {
            key,
            summary: summary.into(),
            app: None,
            app_icon: None,
            body: None,
            urgency: Urgency::Normal,
            actions: Vec::new(),
            on_press: None,
            on_dismiss: None,
            expires: None,
            reply: None,
        }
    }

    /// The name of the app it comes from.
    pub fn app(mut self, name: impl Into<String>) -> Self {
        self.app = Some(name.into());
        self
    }

    /// The app's icon, by app id, like [`app_icon`].
    pub fn app_icon(mut self, app_id: impl Into<String>) -> Self {
        self.app_icon = Some(app_id.into());
        self
    }

    /// Plain text under the summary. The host wraps it and cuts it off after a few lines.
    pub fn body(mut self, text: impl Into<String>) -> Self {
        self.body = Some(text.into());
        self
    }

    pub fn urgency(mut self, urgency: Urgency) -> Self {
        self.urgency = urgency;
        self
    }

    /// A button on the notification that sends `message`. The first one is the main action.
    pub fn action(mut self, label: impl Into<String>, message: M) -> Self {
        self.actions.push((label.into(), message));
        self
    }

    /// Sent on a click on the notification itself, outside its buttons.
    pub fn on_press(mut self, message: M) -> Self {
        self.on_press = Some(message);
        self
    }

    /// Sent when its close button is clicked. Without it, it has none.
    pub fn on_dismiss(mut self, message: M) -> Self {
        self.on_dismiss = Some(message);
        self
    }

    /// Sent once it has been on screen for `after`. The host doesn't count while the pointer is
    /// on it or it's waiting to be shown. Without it, it stays
    /// until it leaves the list.
    pub fn expires(mut self, after: std::time::Duration, message: M) -> Self {
        self.expires = Some((after, message));
        self
    }

    /// A field to type an answer into ([`Reply`]).
    pub fn reply(mut self, reply: Reply<M>) -> Self {
        self.reply = Some(reply);
        self
    }

    pub fn map<N: 'static>(self, f: &(impl Fn(M) -> N + Clone + 'static)) -> Notice<N>
    where
        M: 'static,
    {
        Notice {
            key: self.key,
            summary: self.summary,
            app: self.app,
            app_icon: self.app_icon,
            body: self.body,
            urgency: self.urgency,
            actions: self
                .actions
                .into_iter()
                .map(|(label, message)| (label, f(message)))
                .collect(),
            on_press: self.on_press.map(f),
            on_dismiss: self.on_dismiss.map(f),
            expires: self.expires.map(|(after, message)| (after, f(message))),
            reply: self.reply.map(|reply| reply.map(f)),
        }
    }

    pub fn key(&self) -> u64 {
        self.key
    }

    #[doc(hidden)]
    pub fn into_parts(self) -> NoticeParts<M> {
        NoticeParts {
            key: self.key,
            summary: self.summary,
            app: self.app,
            app_icon: self.app_icon,
            body: self.body,
            urgency: self.urgency,
            actions: self.actions,
            on_press: self.on_press,
            on_dismiss: self.on_dismiss,
            expires: self.expires,
            reply: self.reply.map(Reply::into_parts),
        }
    }
}

#[doc(hidden)]
#[derive(Clone)]
pub struct NoticeParts<M> {
    pub key: u64,
    pub summary: String,
    pub app: Option<String>,
    pub app_icon: Option<String>,
    pub body: Option<String>,
    pub urgency: Urgency,
    pub actions: Vec<(String, M)>,
    pub on_press: Option<M>,
    pub on_dismiss: Option<M>,
    pub expires: Option<(std::time::Duration, M)>,
    pub reply: Option<ReplyParts<M>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_converts_every_message() {
        let node = row([
            button(label("a"), 1).on_secondary(4),
            column([button(label("b"), 2), label("c")]),
            switcher([(label("d"), 3)], Some(0)),
        ]);

        let Kind::Row(children) = node.map(&|n: i32| n * 10).into_kind() else {
            panic!("row became something else");
        };
        let mut messages = Vec::new();
        collect(children, &mut messages);
        assert_eq!(messages, [10, 40, 20, 30]);
    }

    fn collect(nodes: Vec<Node<i32>>, out: &mut Vec<i32>) {
        for node in nodes {
            match node.into_kind() {
                Kind::Button {
                    on_press,
                    on_secondary,
                    ..
                } => {
                    out.push(on_press);
                    out.extend(on_secondary);
                }
                Kind::Switcher { options, .. } => {
                    out.extend(options.into_iter().map(|(_, message)| message))
                }
                Kind::Row(children) | Kind::Column(children) => collect(children, out),
                _ => {}
            }
        }
    }

    #[test]
    fn hover_shows_the_second_option() {
        let node: Node<()> = hover(label("a"), label("b"));
        assert!(matches!(
            node.into_kind(),
            Kind::Alternate {
                current: 0,
                hovered: Some(1),
                ..
            }
        ));
        let node: Node<()> = label("a").on_hover(1);
        assert!(matches!(node.into_kind(), Kind::Label { .. }));
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

    #[test]
    fn map_reaches_new_nodes_and_their_closures() {
        let node = column([
            header("h").on_back(1).trailing(toggle(true, 2)),
            list([list_item("a")
                .on_press(3)
                .on_secondary(4)
                .trailing(button(icon(Icon::Info), 5))]),
            slider(0.5, 0.0..=1.0, |v| (v * 10.0) as i32).on_release(6),
            text_input("p", "v", |text: String| text.len() as i32).on_submit(7),
        ]);
        let Kind::Column(children) = node.map(&|n: i32| n * 10).into_kind() else {
            panic!("column became something else");
        };
        let mut children = children.into_iter().map(Node::into_kind);
        let Some(Kind::Header {
            on_back: Some(10),
            trailing: Some(trailing),
            ..
        }) = children.next()
        else {
            panic!("header lost its back message");
        };
        assert!(matches!(
            trailing.into_kind(),
            Kind::Toggle { on_toggle: 20, .. }
        ));
        let Some(Kind::List(rows)) = children.next() else {
            panic!("list lost");
        };
        let Some(Kind::ListItem {
            on_press: Some(30),
            on_secondary: Some(40),
            trailing: Some(trailing),
            ..
        }) = rows.into_iter().next().map(Node::into_kind)
        else {
            panic!("list item lost its messages");
        };
        assert!(matches!(
            trailing.into_kind(),
            Kind::Button { on_press: 50, .. }
        ));
        let Some(Kind::Slider {
            on_change,
            on_release: Some(60),
            ..
        }) = children.next()
        else {
            panic!("slider lost");
        };
        assert_eq!(on_change(0.3), 30);
        let Some(Kind::TextInput {
            on_input,
            on_submit: Some(70),
            ..
        }) = children.next()
        else {
            panic!("text input lost");
        };
        assert_eq!(on_input("abcd".into()), 40);
    }

    #[test]
    fn tabs_map_and_tone_every_page() {
        let node = tabs([("a", button(label("x"), 1)), ("b", button(label("y"), 2))])
            .tone(Tone::Dim)
            .map(&|n: i32| n * 10);
        let Kind::Tabs(pages) = node.into_kind() else {
            panic!("tabs became something else");
        };
        let names: Vec<_> = pages.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, ["a", "b"]);
        let mut messages = Vec::new();
        for (_, page) in pages {
            let Kind::Button {
                content, on_press, ..
            } = page.into_kind()
            else {
                panic!("page lost its button");
            };
            assert!(matches!(
                content.into_kind(),
                Kind::Label {
                    tone: Some(Tone::Dim),
                    ..
                }
            ));
            messages.push(on_press);
        }
        assert_eq!(messages, [10, 20]);
    }

    #[test]
    fn notice_map_reaches_every_message() {
        let notice = Notice::new(7, "s")
            .action("a", 1)
            .action("b", 2)
            .on_press(3)
            .on_dismiss(4)
            .expires(std::time::Duration::from_secs(5), 5)
            .reply(
                Reply::new("p", "v", |text: String| text.len() as i32)
                    .on_submit(6)
                    .on_cancel(7),
            )
            .map(&|n: i32| n * 10);
        assert_eq!(notice.key(), 7);
        let parts = notice.into_parts();
        let actions: Vec<_> = parts.actions.into_iter().map(|(_, m)| m).collect();
        assert_eq!(actions, [10, 20]);
        assert_eq!(parts.on_press, Some(30));
        assert_eq!(parts.on_dismiss, Some(40));
        assert_eq!(parts.expires.map(|(_, m)| m), Some(50));
        let reply = parts.reply.unwrap();
        assert_eq!((reply.on_input)("abc".into()), 30);
        assert_eq!((reply.on_submit, reply.on_cancel), (Some(60), Some(70)));
    }

    #[test]
    fn modifiers_only_touch_their_nodes() {
        let node: Node<i32> = label("a").trailing(label("b")).on_press(1).selected(true);
        assert!(matches!(node.into_kind(), Kind::Label { .. }));
        let node: Node<i32> = button(label("a"), 1).on_press(2).prominent();
        assert!(matches!(
            node.into_kind(),
            Kind::Button {
                on_press: 1,
                prominent: true,
                ..
            }
        ));
    }

    #[test]
    fn tone_reaches_details_and_row_icons() {
        let node: Node<()> = column([
            detail("n", label("v")),
            list_item("t").leading(icon(Icon::Lock)),
        ])
        .tone(Tone::Good);
        let Kind::Column(children) = node.into_kind() else {
            unreachable!()
        };
        let mut children = children.into_iter().map(Node::into_kind);
        let Some(Kind::Detail { value, .. }) = children.next() else {
            unreachable!()
        };
        assert!(matches!(
            value.into_kind(),
            Kind::Label {
                tone: Some(Tone::Good),
                ..
            }
        ));
        let Some(Kind::ListItem {
            leading: Some(leading),
            ..
        }) = children.next()
        else {
            unreachable!()
        };
        assert!(matches!(
            leading.into_kind(),
            Kind::Icon {
                tone: Some(Tone::Good),
                ..
            }
        ));
    }
}

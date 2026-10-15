//! The closed UI vocabulary widgets build their views from.
//!
//! Every node is semantic: it says *what* something is (a label, a heading, a button), never how
//! it looks. The host maps nodes to pixels using its theme, so every plugin gets the same style,
//! and the same animations, without being able to break them.

use std::ops::RangeInclusive;
use std::sync::Arc;
use std::time::SystemTime;

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
    /// A sun whose rays light up with the level: a screen's brightness.
    Brightness { level: f32 },
    /// Light or dark mode: a crescent moon when dark, a ringed sun when light (not the
    /// brightness one).
    Appearance { dark: bool },
    /// A keyboard: its backlight.
    KeyboardLight,
    /// Three level bars, like an equalizer: media. Uneven while `playing`, low and level when
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
    /// The Bluetooth rune, with a dot on each side while a device is connected.
    Bluetooth { connected: bool },
}

/// Where a [`step`] of a [`timeline`] stands. The host draws it as the step's mark on the line.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Mark {
    /// Something said or noted along the way.
    #[default]
    Note,
    /// What you did or said: where a stretch of steps starts.
    You,
    /// Under way.
    Running,
    /// Finished.
    Done,
    /// Finished, and it went wrong.
    Failed,
    /// Held up until you answer.
    Waiting,
}

/// What a line of a [`diff`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    /// Unchanged, shown around the changes.
    Context,
    Added,
    Removed,
    /// Where the next stretch of changes is, like `@@ -88,7 +88,9 @@`.
    Hunk,
}

/// A key a [`Shortcut`] names. There is no Escape: it always closes the popover (D-41).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    /// A key that types a character, as it reads with no modifier held: `'k'`, `'1'`.
    Char(char),
    Enter,
    Space,
    Tab,
    Up,
    Down,
    Left,
    Right,
}

impl From<char> for Key {
    fn from(c: char) -> Self {
        Key::Char(c)
    }
}

/// A key, with the modifiers held for it: `Ctrl+K`. Build it with [`shortcut`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shortcut {
    pub key: Key,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    /// The Super key.
    pub logo: bool,
}

/// A row's named shortcut: the name config.toml rebinds it by, and the key it has now. The host
/// replaces the plugin's key with the user's, or none, before drawing and binding it (D-91).
#[doc(hidden)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding {
    pub id: String,
    pub key: Option<Shortcut>,
}

/// A key to press, for a [`list_item`]'s or a [`button`]'s [`shortcut`](Node::shortcut), or to
/// show with [`keys`]: `shortcut('1')`, `shortcut('k').ctrl()`.
pub fn shortcut(key: impl Into<Key>) -> Shortcut {
    Shortcut {
        key: key.into(),
        ctrl: false,
        alt: false,
        shift: false,
        logo: false,
    }
}

impl Shortcut {
    pub fn ctrl(mut self) -> Self {
        self.ctrl = true;
        self
    }

    pub fn alt(mut self) -> Self {
        self.alt = true;
        self
    }

    pub fn shift(mut self) -> Self {
        self.shift = true;
        self
    }

    /// Held with the Super key.
    pub fn logo(mut self) -> Self {
        self.logo = true;
        self
    }
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
    Sprite {
        frames: &'static [&'static [u8]],
        /// Indices into `frames`, in the order they play.
        order: &'static [u8],
        /// Plays for as long as it shows, not only when it shows up or changes.
        looping: bool,
        tone: Option<Tone>,
    },
    Cover {
        url: String,
        /// Shown until the picture is loaded, and instead of it if it can't be.
        fallback: Box<Node<M>>,
        /// The main picture of a popover, not an icon.
        large: bool,
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
        /// The main action of a popover page: filled, so it stands out.
        prominent: bool,
        /// A key that presses it while its popover or dialog is open.
        shortcut: Option<Binding>,
    },
    Switcher {
        options: Vec<(Node<M>, M)>,
        current: Option<usize>,
    },
    Gulp(Box<Node<M>>),
    /// Centered across the panel's width.
    Center(Box<Node<M>>),
    /// One page of a sequence: a change of `order` hands over to the next.
    Page {
        order: i64,
        content: Box<Node<M>>,
    },
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
        shortcut: Option<Binding>,
    },
    List(Vec<Node<M>>),
    Section {
        title: String,
        trailing: Option<Box<Node<M>>>,
    },
    Header {
        title: String,
        subtitle: Option<String>,
        on_back: Option<M>,
        leading: Option<Box<Node<M>>>,
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
    Counted {
        content: Box<Node<M>>,
        count: u32,
    },
    Badge {
        text: String,
        tone: Option<Tone>,
    },
    Keys {
        shortcut: Shortcut,
        tone: Option<Tone>,
    },
    /// Pages and their names. The host shows one at a time and owns which.
    Tabs(Vec<(String, Node<M>)>),
    Markdown {
        text: String,
        tone: Option<Tone>,
    },
    TextArea {
        placeholder: String,
        value: String,
        on_input: Box<dyn Fn(String) -> M>,
        on_submit: Option<M>,
        trailing: Option<Box<Node<M>>>,
    },
    Scroll {
        content: Box<Node<M>>,
        /// Keeps the end in view as content arrives there, like a chat.
        bottom: bool,
    },
    Literal {
        text: String,
        tone: Option<Tone>,
    },
    Divider {
        text: String,
    },
    Timeline(Vec<Node<M>>),
    Step {
        mark: Mark,
        /// Drawn on the line in place of the mark.
        leading: Option<Box<Node<M>>>,
        content: Box<Node<M>>,
    },
    Diff {
        title: String,
        lines: Vec<(Change, String)>,
    },
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

/// A little animation the plugin ships, like a pixel-art mascot: `frames` are PNG or SVG files
/// compiled in with `include_bytes!`, all the same size, and `order` lists which frame shows when,
/// by index into `frames`. The host keeps the frames' shape and picks the height (a little under
/// a label's in the bar, taller in a popover), scaling pixels up whole, never blurred: ship pixel
/// art at its own size. The host shows a frame for a quarter of a second. It plays the loop a
/// couple of times when it shows up or changes, then rests on the first frame of `order`;
/// [`Node::looping`] keeps it going. A tone only dims it.
pub fn sprite<M>(frames: &'static [&'static [u8]], order: &'static [u8]) -> Node<M> {
    Node(Kind::Sprite {
        frames,
        order,
        looping: false,
        tone: None,
    })
}

/// A picture a program hands over at runtime, by URL: a track's cover art. `file://` and
/// `https://` URLs load; the host fetches, crops it square and rounds it, off the UI thread, and
/// keeps the few latest. Drawn at icon size, or big with [`Node::large`]. Until it's there, or if
/// it can't be loaded, `fallback` shows in its place. A tone only dims it.
pub fn cover<M>(url: impl Into<String>, fallback: Node<M>) -> Node<M> {
    Node(Kind::Cover {
        url: url.into(),
        fallback: Box::new(fallback),
        large: false,
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

/// `content` centered across the panel's width: a cover, a title, a row of controls. Text in it
/// stays on one line, cut and faded at the edges of its room if it's too long. In the bar, it's
/// just `content`.
pub fn center<M>(content: Node<M>) -> Node<M> {
    Node(Kind::Center(Box::new(content)))
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
        shortcut: None,
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
/// with [`on_press`](Node::on_press). A [`shortcut`](Node::shortcut) presses it from the keyboard.
/// A long title fades out at the end rather than wrapping.
pub fn list_item<M>(title: impl Into<String>) -> Node<M> {
    Node(Kind::ListItem {
        title: title.into(),
        subtitle: None,
        leading: None,
        trailing: None,
        selected: false,
        on_press: None,
        on_secondary: None,
        shortcut: None,
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
/// [`leading`](Node::leading) picture goes before the title, a [`subtitle`](Node::subtitle)
/// under it (where it is, what it's about), and a [`trailing`](Node::trailing) node sits at the
/// far end (a switch, a count).
pub fn header<M>(title: impl Into<String>) -> Node<M> {
    Node(Kind::Header {
        title: title.into(),
        subtitle: None,
        on_back: None,
        leading: None,
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

/// A field to type several lines into, showing `value`: a message. It grows with its text up
/// to a few lines, then scrolls. Sends `on_input` with the new text on every change, and
/// [`on_submit`](Node::on_submit) on Enter; Shift+Enter starts a new line. In a popover it takes
/// the focus when it appears, like a [`text_input`]. One per popover page.
pub fn text_area<M: 'static>(
    placeholder: impl Into<String>,
    value: impl Into<String>,
    on_input: impl Fn(String) -> M + 'static,
) -> Node<M> {
    Node(Kind::TextArea {
        placeholder: placeholder.into(),
        value: value.into(),
        on_input: Box::new(on_input),
        on_submit: None,
        trailing: None,
    })
}

/// Text written in Markdown, as people and programs write messages: paragraphs, emphasis,
/// `code`, lists, quotes, headings, code blocks and tables. The host sets it in its own type and
/// colors, wrapped to the width it has. Links show as links but don't open: the bar starts no
/// programs.
pub fn markdown<M>(text: impl Into<String>) -> Node<M> {
    Node(Kind::Markdown {
        text: text.into(),
        tone: None,
    })
}

/// Its content, scrolling in a popover once it's taller than the host allows a page to grow: a
/// conversation, a long document. With [`Node::bottom`] it keeps its end in view. In the bar it's
/// just its content.
pub fn scroll<M>(content: Node<M>) -> Node<M> {
    Node(Kind::Scroll {
        content: Box::new(content),
        bottom: false,
    })
}

/// Text exactly as typed, not prose: a path, a command, a name in code. It doesn't wrap: when it
/// doesn't fit, its end fades out.
pub fn literal<M>(text: impl Into<String>) -> Node<M> {
    Node(Kind::Literal {
        text: text.into(),
        tone: None,
    })
}

/// A [`separator`] with a short caption in the middle of it, like the time between two turns of
/// a conversation.
pub fn divider<M>(text: impl Into<String>) -> Node<M> {
    Node(Kind::Divider { text: text.into() })
}

/// Steps one after another, strung on a line: what someone did, in order. Each child is a
/// [`step`], whose mark says where it stands; other nodes count as steps marked
/// [`Mark::Note`].
pub fn timeline<M>(steps: impl IntoIterator<Item = Node<M>>) -> Node<M> {
    Node(Kind::Timeline(steps.into_iter().collect()))
}

/// One step of a [`timeline`]: `content`, marked on the line as `mark`. A
/// [`leading`](Node::leading) node, like a small picture, stands on the line in place of the
/// mark. Outside a timeline it's just its content.
pub fn step<M>(mark: Mark, content: Node<M>) -> Node<M> {
    Node(Kind::Step {
        mark,
        leading: None,
        content: Box::new(content),
    })
}

/// A change to a file, titled with its name: lines added, removed, and some unchanged ones around
/// them. The host counts what was added and removed.
pub fn diff<M>(
    title: impl Into<String>,
    lines: impl IntoIterator<Item = (Change, String)>,
) -> Node<M> {
    Node(Kind::Diff {
        title: title.into(),
        lines: lines.into_iter().collect(),
    })
}

/// Its text, large: the one number a popover is about, like a battery's charge.
pub fn display<M>(content: Node<M>) -> Node<M> {
    Node(Kind::Display(Box::new(content)))
}

/// Its content marked with a ring: today in a calendar.
pub fn mark<M>(content: Node<M>) -> Node<M> {
    Node(Kind::Mark(Box::new(content)))
}

/// Its content with `count` in a red badge at its top right, overlapping it a little: unread
/// notifications on a bell. At 0 there's no badge. The host grows the badge in and out and counts
/// through the numbers in between; past 99 it shows 99.
pub fn counted<M>(content: Node<M>, count: u32) -> Node<M> {
    Node(Kind::Counted {
        content: Box::new(content),
        count,
    })
}

/// A short code in a small outlined box: a keyboard layout's `US`.
pub fn badge<M>(text: impl Into<String>) -> Node<M> {
    Node(Kind::Badge {
        text: text.into(),
        tone: None,
    })
}

/// A shortcut drawn as keycaps, `[Ctrl] [K]`, to say which key does something. It only shows
/// the keys; to bind one, give a [`list_item`] a [`shortcut`](Node::shortcut).
pub fn keys<M>(shortcut: Shortcut) -> Node<M> {
    Node(Kind::Keys {
        shortcut,
        tone: None,
    })
}

/// One page of a sequence the plugin moves through: a list and one of its rows' details, a month
/// of a calendar. `order` says which page it is. When it changes, the host hands over: the old
/// page slides away and fades, then the new one comes in from the side it's on, from the right
/// for a higher `order` (further in, later), from the left for a lower one (back, earlier). While
/// `order` stays, the content just changes, as anywhere else. The plugin keeps which page it's
/// on; the host only draws the way between them.
pub fn page<M>(order: i64, content: Node<M>) -> Node<M> {
    Node(Kind::Page {
        order,
        content: Box::new(content),
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

    /// For a [`button`]: the main action of its page, filled so it stands out. Other nodes ignore
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

    /// For a [`list_item`] or [`header`]: a second, dimmer line under the title. Other nodes
    /// ignore it.
    pub fn subtitle(mut self, text: impl Into<String>) -> Self {
        if let Kind::ListItem { subtitle, .. } | Kind::Header { subtitle, .. } = &mut self.0 {
            *subtitle = Some(text.into());
        }
        self
    }

    /// For a [`list_item`] or [`header`]: an icon or small picture before the title. For a
    /// [`step`]: a small picture on the line, in place of its mark. Other nodes ignore it.
    pub fn leading(mut self, node: Node<M>) -> Self {
        if let Kind::ListItem { leading, .. }
        | Kind::Header { leading, .. }
        | Kind::Step { leading, .. } = &mut self.0
        {
            *leading = Some(Box::new(node));
        }
        self
    }

    /// For a [`list_item`], [`section`], [`header`], [`text_input`] or [`text_area`]: a node at the
    /// far end, like a lock or an info button. Other nodes ignore it.
    pub fn trailing(mut self, node: Node<M>) -> Self {
        if let Kind::ListItem { trailing, .. }
        | Kind::Section { trailing, .. }
        | Kind::Header { trailing, .. }
        | Kind::TextInput { trailing, .. }
        | Kind::TextArea { trailing, .. } = &mut self.0
        {
            *trailing = Some(Box::new(node));
        }
        self
    }

    /// For a [`list_item`] with an [`on_press`](Node::on_press), or a [`button`] in a popover or
    /// dialog: a key that presses it while that is open, named `id`. `key` is the default: the
    /// user can give it another key, or none, under that name in config.toml
    /// (`keys = { chat-1 = "alt+1" }`). The host shows the key at the row's end, or on the
    /// button. A key with no Ctrl, Alt or Super does nothing while a text field has the focus, so
    /// it can be typed. Nothing inside [`tabs`] gets a key. Other nodes ignore it.
    pub fn shortcut(mut self, id: impl Into<String>, key: Shortcut) -> Self {
        if let Kind::ListItem { shortcut, .. } | Kind::Button { shortcut, .. } = &mut self.0 {
            *shortcut = Some(Binding {
                id: id.into(),
                key: Some(key),
            });
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

    /// For a [`text_input`] or [`text_area`]: sent on Enter. Other nodes ignore it.
    pub fn on_submit(mut self, message: M) -> Self {
        if let Kind::TextInput { on_submit, .. } | Kind::TextArea { on_submit, .. } = &mut self.0 {
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

    /// For a [`sprite`]: plays for as long as it shows, not only when it shows up or changes.
    /// Every frame redraws the bar, so keep it for what wants your eye, or for popovers. Other
    /// nodes ignore it.
    pub fn looping(mut self) -> Self {
        if let Kind::Sprite { looping, .. } = &mut self.0 {
            *looping = true;
        }
        self
    }

    /// For a [`cover`]: drawn big, as the main picture of a popover, not as an icon. Other nodes
    /// ignore it.
    pub fn large(mut self) -> Self {
        if let Kind::Cover { large, .. } = &mut self.0 {
            *large = true;
        }
        self
    }

    /// For a [`scroll`]: keeps its end in view as content arrives there, unless you've scrolled
    /// up to read. Other nodes ignore it.
    pub fn bottom(mut self) -> Self {
        if let Kind::Scroll { bottom, .. } = &mut self.0 {
            *bottom = true;
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
            | Kind::Sprite { tone, .. }
            | Kind::Counter { tone, .. }
            | Kind::Meter { tone, .. }
            | Kind::TextInput { tone, .. }
            | Kind::Markdown { tone, .. }
            | Kind::Literal { tone, .. }
            | Kind::Badge { tone, .. }
            | Kind::Keys { tone, .. } => {
                tone.get_or_insert(new);
            }
            Kind::Cover { tone, fallback, .. } => {
                tone.get_or_insert(new);
                fallback.set_tone(new);
            }
            Kind::Alternate {
                options: children, ..
            }
            | Kind::Row(children)
            | Kind::Column(children)
            | Kind::Grid {
                cells: children, ..
            }
            | Kind::List(children)
            | Kind::Timeline(children) => {
                children.iter_mut().for_each(|child| child.set_tone(new));
            }
            Kind::Step {
                leading, content, ..
            } => {
                leading.iter_mut().for_each(|child| child.set_tone(new));
                content.set_tone(new);
            }
            Kind::Button { content, .. }
            | Kind::Gulp(content)
            | Kind::Center(content)
            | Kind::Page { content, .. }
            | Kind::Display(content)
            | Kind::Mark(content)
            | Kind::Counted { content, .. }
            | Kind::Scroll { content, .. }
            | Kind::Detail { value: content, .. } => content.set_tone(new),
            Kind::ListItem {
                leading, trailing, ..
            }
            | Kind::Header {
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
            | Kind::Divider { .. }
            | Kind::Diff { .. }
            | Kind::Empty
            | Kind::Section { .. }
            | Kind::Fill
            | Kind::Slider { .. }
            | Kind::TextArea { .. }
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
            Kind::Sprite {
                frames,
                order,
                looping,
                tone,
            } => Kind::Sprite {
                frames,
                order,
                looping,
                tone,
            },
            Kind::Cover {
                url,
                fallback,
                large,
                tone,
            } => Kind::Cover {
                url,
                fallback: Box::new(fallback.map(f)),
                large,
                tone,
            },
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
                shortcut,
            } => Kind::Button {
                content: Box::new(content.map(f)),
                on_press: f(on_press),
                on_secondary: on_secondary.map(f),
                prominent,
                shortcut,
            },
            Kind::Switcher { options, current } => Kind::Switcher {
                options: options
                    .into_iter()
                    .map(|(node, message)| (node.map(f), f(message)))
                    .collect(),
                current,
            },
            Kind::Gulp(content) => Kind::Gulp(Box::new(content.map(f))),
            Kind::Center(content) => Kind::Center(Box::new(content.map(f))),
            Kind::Page { order, content } => Kind::Page {
                order,
                content: Box::new(content.map(f)),
            },
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
                shortcut,
            } => Kind::ListItem {
                title,
                subtitle,
                leading: map_box(leading),
                trailing: map_box(trailing),
                selected,
                on_press: on_press.map(f),
                on_secondary: on_secondary.map(f),
                shortcut,
            },
            Kind::List(rows) => Kind::List(map_all(rows)),
            Kind::Section { title, trailing } => Kind::Section {
                title,
                trailing: map_box(trailing),
            },
            Kind::Header {
                title,
                subtitle,
                on_back,
                leading,
                trailing,
            } => Kind::Header {
                title,
                subtitle,
                on_back: on_back.map(f),
                leading: map_box(leading),
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
            Kind::Counted { content, count } => Kind::Counted {
                content: Box::new(content.map(f)),
                count,
            },
            Kind::Badge { text, tone } => Kind::Badge { text, tone },
            Kind::Keys { shortcut, tone } => Kind::Keys { shortcut, tone },
            Kind::Tabs(pages) => Kind::Tabs(
                pages
                    .into_iter()
                    .map(|(name, page)| (name, page.map(f)))
                    .collect(),
            ),
            Kind::Markdown { text, tone } => Kind::Markdown { text, tone },
            Kind::TextArea {
                placeholder,
                value,
                on_input,
                on_submit,
                trailing,
            } => {
                let g = f.clone();
                Kind::TextArea {
                    placeholder,
                    value,
                    on_input: Box::new(move |text| g(on_input(text))),
                    on_submit: on_submit.map(f),
                    trailing: map_box(trailing),
                }
            }
            Kind::Scroll { content, bottom } => Kind::Scroll {
                content: Box::new(content.map(f)),
                bottom,
            },
            Kind::Literal { text, tone } => Kind::Literal { text, tone },
            Kind::Divider { text } => Kind::Divider { text },
            Kind::Timeline(steps) => Kind::Timeline(map_all(steps)),
            Kind::Step {
                mark,
                leading,
                content,
            } => Kind::Step {
                mark,
                leading: map_box(leading),
                content: Box::new(content.map(f)),
            },
            Kind::Diff { title, lines } => Kind::Diff { title, lines },
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

    #[doc(hidden)]
    pub fn kind_mut(&mut self) -> &mut Kind<M> {
        &mut self.0
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
/// every popover out at the same width, or at a wider one if it asks ([`Panel::wide`]).
pub struct Panel<M> {
    content: Node<M>,
    on_open: Option<M>,
    on_close: Option<M>,
    wide: bool,
}

impl<M> Panel<M> {
    pub fn new(content: Node<M>) -> Self {
        Self {
            content,
            on_open: None,
            on_close: None,
            wide: false,
        }
    }

    /// Laid out wider than other popovers, for a page that's mostly reading: a conversation, a
    /// document. The host decides how wide.
    pub fn wide(mut self) -> Self {
        self.wide = true;
        self
    }

    #[doc(hidden)]
    pub fn is_wide(&self) -> bool {
        self.wide
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
            wide: self.wide,
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
    icon: Option<Icon>,
    image: Option<String>,
    pixels: Option<Pixels>,
    body: Option<String>,
    markdown: bool,
    urgency: Urgency,
    time: Option<SystemTime>,
    thread: Option<String>,
    thread_name: Option<String>,
    progress: Option<f32>,
    actions: Vec<(String, M)>,
    choices: Option<ChoicesParts<M>>,
    step: Option<StepParts<M>>,
    on_press: Option<M>,
    on_dismiss: Option<M>,
    expires: Option<(std::time::Duration, M)>,
    reply: Option<Reply<M>>,
}

/// A picture as its pixels, for a [`Notice`] whose picture came as bytes rather than a file: RGBA,
/// 8 bits a channel, row after row with nothing between them.
///
/// Cheap to clone. Keep the same `Pixels` for as long as the notice shows: the host draws it once
/// and knows it again by its bytes' address.
#[derive(Clone)]
pub struct Pixels {
    width: u32,
    height: u32,
    rgba: Arc<[u8]>,
}

impl Pixels {
    /// `None` if `rgba` isn't `width × height` pixels of 4 bytes, or the picture is empty.
    pub fn new(width: u32, height: u32, rgba: impl Into<Arc<[u8]>>) -> Option<Self> {
        let rgba = rgba.into();
        let expected = (width as usize)
            .checked_mul(height as usize)?
            .checked_mul(4)?;
        (width > 0 && height > 0 && rgba.len() == expected).then_some(Self {
            width,
            height,
            rgba,
        })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn rgba(&self) -> &Arc<[u8]> {
        &self.rgba
    }
}

impl std::fmt::Debug for Pixels {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Pixels({}×{})", self.width, self.height)
    }
}

/// Options to pick from on a [`Notice`], like the answers to a question: one, which a click sends,
/// or several, checked and then sent together.
pub struct Choices<M> {
    parts: ChoicesParts<M>,
}

impl<M> Choices<M> {
    /// A click on an option sends its message: that's the answer.
    pub fn one() -> Self {
        Self::new(false)
    }

    /// A click on an option sends its message, to check or uncheck it, and
    /// [`on_send`](Self::on_send) sends what's checked.
    pub fn many() -> Self {
        Self::new(true)
    }

    fn new(multiple: bool) -> Self {
        Self {
            parts: ChoicesParts {
                multiple,
                options: Vec::new(),
                on_send: None,
            },
        }
    }

    /// An option, with a line saying what it means, and whether it's checked.
    pub fn option(
        mut self,
        label: impl Into<String>,
        description: Option<String>,
        picked: bool,
        on_pick: M,
    ) -> Self {
        self.parts.options.push(ChoiceParts {
            label: label.into(),
            description: description.filter(|d| !d.is_empty()),
            recommended: false,
            picked,
            on_pick,
        });
        self
    }

    /// The option just added is the one to suggest.
    pub fn recommended(mut self) -> Self {
        if let Some(option) = self.parts.options.last_mut() {
            option.recommended = true;
        }
        self
    }

    /// Sent by the button that sends what's checked, for [`many`](Self::many).
    pub fn on_send(mut self, message: M) -> Self {
        self.parts.on_send = Some(message);
        self
    }

    fn map<N>(self, f: &impl Fn(M) -> N) -> Choices<N> {
        let parts = self.parts;
        Choices {
            parts: ChoicesParts {
                multiple: parts.multiple,
                options: parts
                    .options
                    .into_iter()
                    .map(|option| ChoiceParts {
                        label: option.label,
                        description: option.description,
                        recommended: option.recommended,
                        picked: option.picked,
                        on_pick: f(option.on_pick),
                    })
                    .collect(),
                on_send: parts.on_send.map(f),
            },
        }
    }
}

#[doc(hidden)]
#[derive(Clone)]
pub struct ChoicesParts<M> {
    pub multiple: bool,
    pub options: Vec<ChoiceParts<M>>,
    pub on_send: Option<M>,
}

#[doc(hidden)]
#[derive(Clone)]
pub struct ChoiceParts<M> {
    pub label: String,
    pub description: Option<String>,
    pub recommended: bool,
    pub picked: bool,
    pub on_pick: M,
}

#[doc(hidden)]
#[derive(Clone)]
pub struct StepParts<M> {
    /// From 0.
    pub index: usize,
    pub count: usize,
    pub on_back: Option<M>,
}

/// A field on a [`Notice`] to type an answer into, like a reply to a message: several lines,
/// Enter sends and Shift+Enter starts a new line.
///
/// It shows with the notice, but the host takes the keyboard only once it's clicked, and gives it
/// back on Esc, on sending, or on a click elsewhere. The notice doesn't expire while it has the
/// keyboard.
pub struct Reply<M> {
    placeholder: String,
    value: String,
    on_input: std::rc::Rc<dyn Fn(String) -> M>,
    on_submit: Option<M>,
    on_cancel: Option<M>,
    sent: bool,
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
            sent: false,
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

    /// The answer went: the host says so in the field's place. Keep the notice a moment after
    /// sending, so it can be read.
    pub fn sent(mut self, sent: bool) -> Self {
        self.sent = sent;
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
            sent: self.sent,
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
            sent: self.sent,
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
    pub sent: bool,
}

impl<M> Notice<M> {
    pub fn new(key: u64, summary: impl Into<String>) -> Self {
        Self {
            key,
            summary: summary.into(),
            app: None,
            app_icon: None,
            icon: None,
            image: None,
            pixels: None,
            body: None,
            markdown: false,
            urgency: Urgency::Normal,
            time: None,
            thread: None,
            thread_name: None,
            progress: None,
            actions: Vec::new(),
            choices: None,
            step: None,
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

    /// The app's icon, by app id, like [`app_icon`], or a `file://` URL to a picture of it.
    pub fn app_icon(mut self, app_id: impl Into<String>) -> Self {
        self.app_icon = Some(app_id.into());
        self
    }

    /// A symbol as its icon, drawn in the text's tone, for what's about the device rather than
    /// an app: a battery running low, a network that dropped. It wins over
    /// [`app_icon`](Self::app_icon).
    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// A picture of its own, by URL, like a [`cover`]: an avatar, a screenshot. The host shows it
    /// beside the text, or under it, full width, if it's wide.
    pub fn image(mut self, url: impl Into<String>) -> Self {
        self.image = Some(url.into());
        self
    }

    /// A picture of its own, as its pixels: what an app sent as bytes rather than a file. It wins
    /// over [`image`](Self::image).
    pub fn pixels(mut self, pixels: Pixels) -> Self {
        self.pixels = Some(pixels);
        self
    }

    /// When it arrived. The host says how long ago, beside where it's from.
    pub fn time(mut self, time: SystemTime) -> Self {
        self.time = Some(time);
        self
    }

    /// The conversation it's part of: a chat, a session. The host stacks notices of the same app
    /// and thread that come one after another into one, the newest in full.
    pub fn thread(mut self, key: impl Into<String>) -> Self {
        self.thread = Some(key.into());
        self
    }

    /// What to call its [`thread`](Self::thread), beside the app's name: the chat, the session.
    pub fn thread_name(mut self, name: impl Into<String>) -> Self {
        self.thread_name = Some(name.into());
        self
    }

    /// How far along what it's about is, from 0.0 to 1.0: a download, a copy.
    pub fn progress(mut self, fraction: f32) -> Self {
        self.progress = Some(fraction.clamp(0.0, 1.0));
        self
    }

    /// Options to pick from ([`Choices`]), above the reply field and the actions.
    pub fn choices(mut self, choices: Choices<M>) -> Self {
        self.choices = Some(choices.parts);
        self
    }

    /// It's question `index` (from 0) of `count`, asked one at a time on the same notice. The host
    /// shows where it is, and slides the content along when `index` changes. `on_back` goes to
    /// the one before.
    pub fn step(mut self, index: usize, count: usize, on_back: Option<M>) -> Self {
        self.step = Some(StepParts {
            index,
            count,
            on_back,
        });
        self
    }

    /// Plain text under the summary. The host wraps it and cuts it off after a few lines.
    pub fn body(mut self, text: impl Into<String>) -> Self {
        self.body = Some(text.into());
        self
    }

    /// The body is Markdown, like a [`markdown`] node.
    pub fn markdown(mut self) -> Self {
        self.markdown = true;
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
            icon: self.icon,
            image: self.image,
            pixels: self.pixels,
            body: self.body,
            markdown: self.markdown,
            urgency: self.urgency,
            time: self.time,
            thread: self.thread,
            thread_name: self.thread_name,
            progress: self.progress,
            actions: self
                .actions
                .into_iter()
                .map(|(label, message)| (label, f(message)))
                .collect(),
            choices: self.choices.map(|parts| Choices { parts }.map(f).parts),
            step: self.step.map(|step| StepParts {
                index: step.index,
                count: step.count,
                on_back: step.on_back.map(f),
            }),
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
            icon: self.icon,
            image: self.image,
            pixels: self.pixels,
            body: self.body,
            markdown: self.markdown,
            urgency: self.urgency,
            time: self.time,
            thread: self.thread,
            thread_name: self.thread_name,
            progress: self.progress,
            actions: self.actions,
            choices: self.choices,
            step: self.step,
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
    pub icon: Option<Icon>,
    pub image: Option<String>,
    pub pixels: Option<Pixels>,
    pub body: Option<String>,
    pub markdown: bool,
    pub urgency: Urgency,
    pub time: Option<SystemTime>,
    pub thread: Option<String>,
    pub thread_name: Option<String>,
    pub progress: Option<f32>,
    pub actions: Vec<(String, M)>,
    pub choices: Option<ChoicesParts<M>>,
    pub step: Option<StepParts<M>>,
    pub on_press: Option<M>,
    pub on_dismiss: Option<M>,
    pub expires: Option<(std::time::Duration, M)>,
    pub reply: Option<ReplyParts<M>>,
}

/// Something the widget can do, from [`Widget::actions`](crate::Widget::actions), for the dormer:
/// the host's search dialog. Typing finds it by its title, subtitle and keywords, and picking it
/// sends `message` to the widget.
pub struct Action<M> {
    id: Option<String>,
    shortcut: Option<Shortcut>,
    title: String,
    subtitle: Option<String>,
    keywords: Vec<String>,
    leading: Option<Node<M>>,
    message: M,
}

/// An action titled `title` that sends `message` when picked.
pub fn action<M>(title: impl Into<String>, message: M) -> Action<M> {
    Action {
        id: None,
        shortcut: None,
        title: title.into(),
        subtitle: None,
        keywords: Vec::new(),
        leading: None,
        message,
    }
}

impl<M> Action<M> {
    /// Its name, for `eave action <widget> <id>` and for `keys` in config.toml. Give one to every
    /// action that's always there, like `mute`; leave it out on ones that come and go, like a
    /// window to switch to.
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// A key that runs it while the dormer is open, shown at its row's end. It needs Ctrl, Alt or
    /// Super, since the dormer's field has the keys that type, and an [`id`](Action::id), which
    /// the user can give it another key by. When two widgets' actions want the same key, the
    /// widget earlier in config.toml keeps it.
    pub fn shortcut(mut self, key: Shortcut) -> Self {
        self.shortcut = Some(key);
        self
    }

    /// A line under the title: what it acts on, or what kind of thing it is.
    pub fn subtitle(mut self, text: impl Into<String>) -> Self {
        self.subtitle = Some(text.into());
        self
    }

    /// Other words it should be found by, not shown.
    pub fn keywords(mut self, words: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.keywords.extend(words.into_iter().map(Into::into));
        self
    }

    /// A small picture before the title: an [`icon`], [`app_icon`], [`image`] or [`symbol`].
    pub fn leading(mut self, node: Node<M>) -> Self {
        self.leading = Some(node);
        self
    }

    pub fn map<N: 'static>(self, f: &(impl Fn(M) -> N + Clone + 'static)) -> Action<N>
    where
        M: 'static,
    {
        Action {
            id: self.id,
            shortcut: self.shortcut,
            title: self.title,
            subtitle: self.subtitle,
            keywords: self.keywords,
            leading: self.leading.map(|node| node.map(f)),
            message: f(self.message),
        }
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    #[doc(hidden)]
    pub fn id_str(&self) -> Option<&str> {
        self.id.as_deref()
    }

    #[doc(hidden)]
    pub fn into_parts(self) -> ActionParts<M> {
        ActionParts {
            id: self.id,
            shortcut: self.shortcut,
            title: self.title,
            subtitle: self.subtitle,
            keywords: self.keywords,
            leading: self.leading,
            message: self.message,
        }
    }
}

#[doc(hidden)]
pub struct ActionParts<M> {
    pub id: Option<String>,
    pub shortcut: Option<Shortcut>,
    pub title: String,
    pub subtitle: Option<String>,
    pub keywords: Vec<String>,
    pub leading: Option<Node<M>>,
    pub message: M,
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
            scroll(text_area("p", "v", |text: String| text.len() as i32 + 1).on_submit(8)).bottom(),
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
        let Some(Kind::Scroll {
            content,
            bottom: true,
        }) = children.next()
        else {
            panic!("scroll lost");
        };
        let Kind::TextArea {
            on_input,
            on_submit: Some(80),
            ..
        } = content.into_kind()
        else {
            panic!("text area lost");
        };
        assert_eq!(on_input("abcd".into()), 50);
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
            .choices(
                Choices::many()
                    .option("x", None, false, 8)
                    .option("y", Some("why".into()), true, 9)
                    .recommended()
                    .on_send(10),
            )
            .step(1, 3, Some(11))
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
        let choices = parts.choices.unwrap();
        let picks: Vec<_> = choices.options.iter().map(|o| o.on_pick).collect();
        assert_eq!(picks, [80, 90]);
        assert!(choices.multiple && choices.options[1].recommended && choices.options[1].picked);
        assert_eq!(choices.on_send, Some(100));
        let step = parts.step.unwrap();
        assert_eq!((step.index, step.count, step.on_back), (1, 3, Some(110)));
    }

    #[test]
    fn pixels_must_be_whole() {
        assert!(Pixels::new(2, 2, vec![0; 16]).is_some());
        assert!(Pixels::new(2, 2, vec![0; 15]).is_none());
        assert!(Pixels::new(0, 0, Vec::new()).is_none());
        assert!(Pixels::new(u32::MAX, u32::MAX, Vec::new()).is_none());
    }

    #[test]
    fn progress_stays_between_none_and_all() {
        let parts = Notice::<()>::new(1, "s").progress(1.5).into_parts();
        assert_eq!(parts.progress, Some(1.0));
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
    fn a_header_takes_a_picture_and_a_subtitle() {
        let node = header("t")
            .subtitle("s")
            .leading(button(icon(Icon::Info), 1))
            .tone(Tone::Dim)
            .map(&|n: i32| n * 10);
        let Kind::Header {
            subtitle: Some(subtitle),
            leading: Some(leading),
            ..
        } = node.into_kind()
        else {
            panic!("header lost its picture or subtitle");
        };
        assert_eq!(subtitle, "s");
        let Kind::Button {
            content,
            on_press: 10,
            ..
        } = leading.into_kind()
        else {
            panic!("picture lost its message");
        };
        assert!(matches!(
            content.into_kind(),
            Kind::Icon {
                tone: Some(Tone::Dim),
                ..
            }
        ));
    }

    #[test]
    fn timelines_map_and_tone_their_steps() {
        let node = timeline([
            step(Mark::You, label("a")),
            step(Mark::Failed, button(literal("b"), 1)).leading(icon(Icon::Check)),
            label("c"),
        ])
        .tone(Tone::Dim)
        .map(&|n: i32| n * 10);
        let Kind::Timeline(steps) = node.into_kind() else {
            panic!("timeline became something else");
        };
        let mut steps = steps.into_iter().map(Node::into_kind);
        let Some(Kind::Step {
            mark: Mark::You,
            leading: None,
            content,
        }) = steps.next()
        else {
            panic!("first step lost");
        };
        assert!(matches!(
            content.into_kind(),
            Kind::Label {
                tone: Some(Tone::Dim),
                ..
            }
        ));
        let Some(Kind::Step {
            mark: Mark::Failed,
            leading: Some(leading),
            content,
        }) = steps.next()
        else {
            panic!("second step lost its picture");
        };
        assert!(matches!(
            leading.into_kind(),
            Kind::Icon {
                tone: Some(Tone::Dim),
                ..
            }
        ));
        let Kind::Button {
            content,
            on_press: 10,
            ..
        } = content.into_kind()
        else {
            panic!("button lost its message");
        };
        assert!(matches!(
            content.into_kind(),
            Kind::Literal {
                tone: Some(Tone::Dim),
                ..
            }
        ));
        assert!(matches!(steps.next(), Some(Kind::Label { .. })));
        let node: Node<()> = diff("f", [(Change::Added, "x".to_string())]).leading(label("y"));
        assert!(matches!(node.into_kind(), Kind::Diff { lines, .. } if lines.len() == 1));
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

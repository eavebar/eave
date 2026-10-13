//! The dormer: the host's search dialog for widgets' actions (D-84). A keybind opens it, and it
//! drops out of the top of the screen into its upper middle, the way a popover grows out of its
//! island. What it lists comes from [`Widget::actions`](eave_api::Widget::actions), and from every
//! widget with a popover, which it can open. Typing narrows the list down, and picking a row sends
//! its message to its widget.

use iced::Rectangle;

/// How many rows it shows at most: the best matches. Enough to pick from without scrolling.
pub const ROWS: usize = 8;
/// How many picks it remembers, to list first while nothing is typed and to rank higher.
const RECENT: usize = 16;

/// What happens to the dormer.
#[derive(Debug, Clone)]
pub enum Event {
    /// The keybind: open it, or close it if it's open.
    Toggle,
    /// It's ready to open, this far from the surface's top.
    Opened(f32),
    /// What's typed changed.
    Query(String),
    /// The selection moves down (1) or up (-1) a row, round at the ends.
    Move(i32),
    /// The row with this place in the list was picked.
    Run(usize),
    /// Its island placed it.
    Placed(Rectangle),
    /// Its island finished turning back.
    Closed,
}

/// Where a row comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// The widget's action with this place in its list.
    Action(usize),
    /// Opens the widget's popover.
    Popover,
}

/// Something it can list, as it was when the widgets were last asked.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub instance: u64,
    pub source: Source,
    pub title: String,
    pub subtitle: Option<String>,
    pub keywords: Vec<String>,
}

/// One time the dormer is open, from the keybind until it has folded back.
pub struct Session {
    /// False while it folds back, until its island says it's done.
    pub open: bool,
    /// Where its top goes, from the surface's.
    pub top: f32,
    /// Where its island put it, in surface coordinates. The surface grows to fit it.
    pub rect: Option<Rectangle>,
    pub query: String,
    /// The selected row's place among `shown`.
    pub selected: usize,
    pub entries: Vec<Entry>,
    /// The rows, as places in `entries`, best first.
    pub shown: Vec<usize>,
}

impl Session {
    pub fn new(top: f32) -> Self {
        Self {
            open: true,
            top,
            rect: None,
            query: String::new(),
            selected: 0,
            entries: Vec::new(),
            shown: Vec::new(),
        }
    }

    /// The entry of the selected row, if there are any.
    pub fn row(&self, index: usize) -> Option<&Entry> {
        self.entries.get(*self.shown.get(index)?)
    }

    /// Takes in what the widgets list now and ranks it again. The selection stays on its row if
    /// that's still there.
    pub fn refresh(&mut self, entries: Vec<Entry>, recent: &Recent) {
        let selected = self.row(self.selected).cloned();
        self.entries = entries;
        self.shown = rank(&self.query, &self.entries, recent);
        self.selected = selected
            .and_then(|entry| {
                self.shown
                    .iter()
                    .position(|&index| self.entries[index] == entry)
            })
            .unwrap_or(0);
    }

    pub fn search(&mut self, query: String, recent: &Recent) {
        self.query = query;
        self.shown = rank(&self.query, &self.entries, recent);
        self.selected = 0;
    }

    pub fn step(&mut self, by: i32) {
        let rows = self.shown.len() as i32;
        if rows > 0 {
            self.selected = (self.selected as i32 + by).rem_euclid(rows) as usize;
        }
    }
}

/// The last picks, newest first, by instance and title.
#[derive(Debug, Default)]
pub struct Recent(Vec<(u64, String)>);

impl Recent {
    pub fn push(&mut self, entry: &Entry) {
        self.0
            .retain(|(instance, title)| (*instance, title) != (entry.instance, &entry.title));
        self.0.insert(0, (entry.instance, entry.title.clone()));
        self.0.truncate(RECENT);
    }

    /// How recently `entry` was picked: 0 for the last pick.
    fn rank(&self, entry: &Entry) -> Option<usize> {
        self.0
            .iter()
            .position(|(instance, title)| *instance == entry.instance && *title == entry.title)
    }
}

/// The entries to show for `query`, best first, at most [`ROWS`]. With nothing typed: the recent
/// picks, then the rest in the widgets' order.
fn rank(query: &str, entries: &[Entry], recent: &Recent) -> Vec<usize> {
    let query: Vec<char> = query
        .chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect();
    let mut scored: Vec<(i32, usize, usize)> = entries
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| {
            let recently = recent.rank(entry);
            let score = if query.is_empty() {
                0
            } else {
                let title = score(&query, &entry.title).map(|score| 2 * score);
                let others = entry
                    .keywords
                    .iter()
                    .chain(&entry.subtitle)
                    .filter_map(|text| score(&query, text));
                let best = title.into_iter().chain(others).max()?;
                best + recently.map_or(0, |rank| 2 * (RECENT - rank) as i32)
            };
            Some((score, recently.unwrap_or(usize::MAX), index))
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
    scored
        .into_iter()
        .take(ROWS)
        .map(|(.., index)| index)
        .collect()
}

/// How well `text` matches `query` (lowercase, no spaces): every character of the query must
/// come in order. Characters at the start, at the start of a word, or right after the last match
/// count for more, and so does `text` starting with all of it.
fn score(query: &[char], text: &str) -> Option<i32> {
    let chars: Vec<char> = text.chars().collect();
    let mut wanted = query.iter().peekable();
    let mut score = 0;
    let mut last = None;
    for (index, &c) in chars.iter().enumerate() {
        let Some(&&next) = wanted.peek() else {
            break;
        };
        if c.to_lowercase().next() != Some(next) {
            continue;
        }
        let mut points = 1;
        if index == 0 {
            points += 8;
        } else {
            let before = chars[index - 1];
            if !before.is_alphanumeric() || (before.is_lowercase() && c.is_uppercase()) {
                points += 6;
            }
        }
        if last.is_some_and(|last| last + 1 == index) {
            points += 4;
        }
        score += points;
        last = Some(index);
        wanted.next();
    }
    if wanted.peek().is_some() {
        return None;
    }
    let lower: String = text.chars().flat_map(char::to_lowercase).collect();
    let prefix: String = query.iter().collect();
    if lower.starts_with(&prefix) {
        score += 10;
    }
    Some(score)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(instance: u64, title: &str) -> Entry {
        Entry {
            instance,
            source: Source::Action(0),
            title: title.into(),
            subtitle: None,
            keywords: Vec::new(),
        }
    }

    fn titles<'a>(query: &str, entries: &'a [Entry], recent: &Recent) -> Vec<&'a str> {
        rank(query, entries, recent)
            .into_iter()
            .map(|index| entries[index].title.as_str())
            .collect()
    }

    #[test]
    fn every_character_must_come_in_order() {
        let q = |s: &str| s.chars().collect::<Vec<_>>();
        assert!(score(&q("vup"), "Volume up").is_some());
        assert!(score(&q("upv"), "Volume up").is_none());
        assert!(score(&q("x"), "Volume up").is_none());
    }

    #[test]
    fn a_prefix_beats_letters_spread_out() {
        let entries = [entry(0, "Previous track"), entry(0, "Pause")];
        assert_eq!(
            titles("pa", &entries, &Recent::default()),
            ["Pause", "Previous track"]
        );
    }

    #[test]
    fn word_starts_beat_letters_inside_words() {
        let entries = [entry(0, "Disconnect"), entry(0, "Do not disturb")];
        assert_eq!(
            titles("dnd", &entries, &Recent::default())[0],
            "Do not disturb"
        );
    }

    #[test]
    fn spaces_in_the_query_are_ignored() {
        let entries = [entry(0, "Workspace 3"), entry(0, "Workspace 4")];
        assert_eq!(
            titles("work 3", &entries, &Recent::default()),
            ["Workspace 3"]
        );
    }

    #[test]
    fn keywords_and_subtitles_find_an_entry_too() {
        let mut firefox = entry(0, "Firefox");
        firefox.keywords = vec!["browser".into()];
        let mut mute = entry(1, "Mute");
        mute.subtitle = Some("Speakers".into());
        let entries = [firefox, mute];
        assert_eq!(titles("brow", &entries, &Recent::default()), ["Firefox"]);
        assert_eq!(titles("speak", &entries, &Recent::default()), ["Mute"]);
    }

    #[test]
    fn recent_picks_come_first_and_rank_higher() {
        let entries = [
            entry(0, "Brighter"),
            entry(1, "Bluetooth"),
            entry(2, "Battery"),
        ];
        let mut recent = Recent::default();
        assert_eq!(
            titles("", &entries, &recent),
            ["Brighter", "Bluetooth", "Battery"]
        );
        recent.push(&entries[2]);
        assert_eq!(
            titles("", &entries, &recent),
            ["Battery", "Brighter", "Bluetooth"]
        );
        // Equal matches: the recent one wins.
        assert_eq!(titles("b", &entries, &recent)[0], "Battery");
        // Picked again, it moves to the front without a duplicate.
        recent.push(&entries[0]);
        recent.push(&entries[2]);
        assert_eq!(recent.0.len(), 2);
        assert_eq!(recent.rank(&entries[2]), Some(0));
    }

    #[test]
    fn at_most_a_few_rows_show() {
        let entries: Vec<_> = (0..20).map(|n| entry(n, &format!("Row {n}"))).collect();
        assert_eq!(rank("row", &entries, &Recent::default()).len(), ROWS);
    }

    #[test]
    fn the_selection_wraps_and_follows_its_row() {
        let recent = Recent::default();
        let mut session = Session::new(0.0);
        session.refresh(vec![entry(0, "A"), entry(0, "B"), entry(0, "C")], &recent);
        session.step(-1);
        assert_eq!(session.row(session.selected).unwrap().title, "C");
        session.step(1);
        assert_eq!(session.selected, 0);
        session.step(1);
        // A new row comes in before it: the selection stays on B.
        session.refresh(
            vec![entry(0, "Z"), entry(0, "A"), entry(0, "B"), entry(0, "C")],
            &recent,
        );
        assert_eq!(session.row(session.selected).unwrap().title, "B");
        // Typing starts over at the top.
        session.search("c".into(), &recent);
        assert_eq!(session.selected, 0);
        assert_eq!(session.row(0).unwrap().title, "C");
    }
}

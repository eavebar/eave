//! Check for updates and Update eave, in the dormer (D-97). Nothing in the bar itself.
//!
//! ```toml
//! [[dormer]]
//! widget = "update"
//! ```
//!
//! Each starts one of eave-build's systemd units, `eave-update-check.service` or
//! `eave-update.service`, which run `eave-build check` or `eave-build update` (D-96). Not the
//! bar's children: an update restarts the bar, and its unit carries on. The plugin follows the
//! units through systemd's signals and, when one finishes, tells what its summary says in a
//! notification. A check that finds updates offers Update on it. A bar the update restarted reads
//! the summary when it starts. Nothing runs unless picked. Without the units (an `eave-build init`
//! from before them) or without systemd, it lists nothing.

mod notify;
mod summary;

use eave_api::ui::{self, Action, Item};
use eave_api::{Effect, NoConfig, Registry, Subscription, Task, Widget};
use eave_dbus::systemd::{self, Unit};

pub use summary::{Job, Summary};

pub fn register(registry: &mut Registry) {
    registry.widget::<Update>("update");
}

pub struct Update {
    /// The units as systemd last said, `None` until it has.
    check: Option<Unit>,
    update: Option<Unit>,
    /// What the last check found would move, until an update moves it.
    waiting: Vec<String>,
    /// The notification offering Update, while it shows.
    offered: Option<u32>,
}

#[derive(Debug, Clone)]
pub enum Message {
    Unit(Job, Unit),
    Start(Job),
    /// Starting it failed: why.
    Failed(Job, String),
    /// The summary the job left, if it left one.
    Taken(Job, Option<Summary>),
    /// The notification that offers Update is up, with this id.
    Offered(u32),
    Notification(notify::Event),
    /// Picked while busy.
    Nothing,
}

impl Widget for Update {
    type Config = NoConfig;
    type Message = Message;

    fn new(_: NoConfig) -> (Self, Task<Message>) {
        let update = Self {
            check: None,
            update: None,
            waiting: Vec::new(),
            offered: None,
        };
        (update, Task::none())
    }

    fn update(&mut self, message: Message) -> Effect<Message> {
        match message {
            Message::Unit(job, unit) => {
                // Finished, or done before this bar started: whatever it left is to tell.
                let done = unit.exists() && !unit.busy();
                *self.unit(job) = Some(unit);
                match done {
                    true => Effect::task(
                        Task::future(take(job)).map(move |summary| Message::Taken(job, summary)),
                    ),
                    false => Effect::none(),
                }
            }
            Message::Start(_) | Message::Nothing if self.busy() => Effect::none(),
            Message::Start(job) => {
                // Shown as running at once; systemd's word follows.
                if let Some(unit) = self.unit(job) {
                    unit.active = "activating".into();
                }
                let close = match (job, self.offered.take()) {
                    (Job::Update, Some(id)) => Task::future(notify::close(id)).discard(),
                    _ => Task::none(),
                };
                let start =
                    Task::future(systemd::start(job.unit())).then(move |result| match result {
                        Ok(()) => Task::none(),
                        Err(error) => Task::done(Message::Failed(job, error.to_string())),
                    });
                Effect::task(close.chain(start))
            }
            Message::Failed(job, error) => {
                eprintln!("eave: update: can't start {}: {error}", job.unit());
                if let Some(unit) = self.unit(job) {
                    unit.active = "failed".into();
                }
                Effect::none()
            }
            Message::Taken(_, None) | Message::Nothing => Effect::none(),
            Message::Taken(job, Some(summary)) => {
                match (job, summary.ok) {
                    (Job::Check, true) => {
                        self.waiting = summary.moved.iter().map(|m| m.label.clone()).collect();
                    }
                    (Job::Update, true) => self.waiting.clear(),
                    _ => {}
                }
                let note = summary::note(job, &summary);
                let offers = note.offer_update;
                Effect::task(
                    Task::future(notify::send(note)).then(move |sent| match sent {
                        Ok(id) if offers => Task::done(Message::Offered(id)),
                        Ok(_) => Task::none(),
                        Err(error) => {
                            eprintln!("eave: update: can't notify: {error}");
                            Task::none()
                        }
                    }),
                )
            }
            Message::Offered(id) => {
                self.offered = Some(id);
                Effect::none()
            }
            Message::Notification(notify::Event::Invoked(id, key))
                if Some(id) == self.offered && key == notify::UPDATE =>
            {
                self.offered = None;
                self.update(Message::Start(Job::Update))
            }
            Message::Notification(notify::Event::Closed(id)) if Some(id) == self.offered => {
                self.offered = None;
                Effect::none()
            }
            Message::Notification(_) => Effect::none(),
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        let units = [
            Subscription::run(check_unit).map(|unit| Message::Unit(Job::Check, unit)),
            Subscription::run(update_unit).map(|unit| Message::Unit(Job::Update, unit)),
        ];
        // Only while a notification offers Update.
        let picks = self
            .offered
            .map(|_| Subscription::run(notify::events).map(Message::Notification));
        Subscription::batch(units.into_iter().chain(picks))
    }

    fn item(&self) -> Item<Message> {
        Item::hidden()
    }

    fn actions(&self) -> Vec<Action<Message>> {
        let installed = [&self.check, &self.update]
            .iter()
            .all(|unit| unit.as_ref().is_some_and(Unit::exists));
        if !installed {
            return Vec::new();
        }
        let words = ["eave", "upgrade", "eave-build", "plugins", "new version"];
        let busy = |unit: &Option<Unit>| unit.as_ref().is_some_and(Unit::busy);
        if busy(&self.check) {
            return vec![
                ui::action("Checking for updates…", Message::Nothing)
                    .id("check")
                    .keywords(words),
            ];
        }
        if busy(&self.update) {
            return vec![
                ui::action("Updating eave…", Message::Nothing)
                    .id("update")
                    .keywords(words),
            ];
        }
        let update = ui::action("Update eave", Message::Start(Job::Update))
            .id("update")
            .keywords(words);
        let update = match self.waiting.as_slice() {
            [] => update.subtitle("Fetch, build and install the newest eave and plugins"),
            waiting => update.subtitle(format!("Newer: {}", waiting.join(", "))),
        };
        vec![
            ui::action("Check for updates", Message::Start(Job::Check))
                .id("check")
                .subtitle("Say what Update eave would move, moving nothing")
                .keywords(words),
            update,
        ]
    }
}

impl Update {
    fn unit(&mut self, job: Job) -> &mut Option<Unit> {
        match job {
            Job::Check => &mut self.check,
            Job::Update => &mut self.update,
        }
    }

    /// Whether either unit is running: they'd fight over the checkouts.
    fn busy(&self) -> bool {
        [&self.check, &self.update]
            .iter()
            .any(|unit| unit.as_ref().is_some_and(Unit::busy))
    }
}

fn check_unit() -> impl eave_dbus::Stream<Item = Unit> {
    systemd::watch(Job::Check.unit())
}

fn update_unit() -> impl eave_dbus::Stream<Item = Unit> {
    systemd::watch(Job::Update.unit())
}

/// The summary `job` left, off the UI thread.
async fn take(job: Job) -> Option<Summary> {
    let taken = tokio::task::spawn_blocking(move || summary::take(job)).await;
    match taken {
        Ok(Ok(summary)) => summary,
        Ok(Err(error)) => {
            eprintln!("eave: update: reading what {} left: {error}", job.unit());
            None
        }
        Err(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(active: &str) -> Unit {
        Unit {
            load: "loaded".into(),
            active: active.into(),
            invocation: String::new(),
        }
    }

    fn titles(update: &Update) -> Vec<String> {
        update
            .actions()
            .into_iter()
            .map(|action| action.into_parts().title)
            .collect()
    }

    fn installed() -> Update {
        let mut update = Update::new(NoConfig::default()).0;
        let _ = update.update(Message::Unit(Job::Check, unit("inactive")));
        let _ = update.update(Message::Unit(Job::Update, unit("inactive")));
        update
    }

    #[test]
    fn nothing_without_the_units() {
        let mut update = Update::new(NoConfig::default()).0;
        assert!(update.actions().is_empty());
        let _ = update.update(Message::Unit(Job::Check, unit("inactive")));
        let missing = Unit {
            load: "not-found".into(),
            ..unit("inactive")
        };
        let _ = update.update(Message::Unit(Job::Update, missing));
        assert!(update.actions().is_empty());
        assert!(update.item().into_parts().content.is_none());
    }

    #[test]
    fn a_running_job_is_all_there_is() {
        let mut update = installed();
        assert_eq!(titles(&update), ["Check for updates", "Update eave"]);
        let _ = update.update(Message::Start(Job::Check));
        assert_eq!(titles(&update), ["Checking for updates…"]);
        // Picking anything meanwhile starts nothing.
        let _ = update.update(Message::Start(Job::Update));
        assert_eq!(update.update.as_ref().unwrap().active, "inactive");
        let _ = update.update(Message::Unit(Job::Check, unit("inactive")));
        let _ = update.update(Message::Unit(Job::Update, unit("activating")));
        assert_eq!(titles(&update), ["Updating eave…"]);
    }

    #[test]
    fn a_check_says_whats_waiting_until_the_update() {
        let mut update = installed();
        let found: Summary = toml::from_str(
            "ok = true\n[[moved]]\nlabel = \"eave\"\ncommits = 3\n\
             [[moved]]\nlabel = \"weather\"\ncommits = 1\n",
        )
        .unwrap();
        let _ = update.update(Message::Taken(Job::Check, Some(found)));
        let subtitle = |update: &Update| {
            update
                .actions()
                .into_iter()
                .nth(1)
                .unwrap()
                .into_parts()
                .subtitle
        };
        assert_eq!(subtitle(&update).as_deref(), Some("Newer: eave, weather"));
        let done: Summary = toml::from_str("ok = true\nmoved = []\n").unwrap();
        let _ = update.update(Message::Taken(Job::Update, Some(done)));
        assert!(!subtitle(&update).unwrap().starts_with("Newer"));
    }

    #[test]
    fn update_on_the_notification_updates() {
        let mut update = installed();
        let _ = update.update(Message::Offered(7));
        // Another notification's action, or another action: nothing.
        let _ = update.update(Message::Notification(notify::Event::Invoked(
            8,
            "update".into(),
        )));
        let _ = update.update(Message::Notification(notify::Event::Invoked(
            7,
            "default".into(),
        )));
        assert_eq!(titles(&update), ["Check for updates", "Update eave"]);
        let _ = update.update(Message::Notification(notify::Event::Invoked(
            7,
            "update".into(),
        )));
        assert_eq!(titles(&update), ["Updating eave…"]);
        assert_eq!(update.offered, None);
    }

    #[test]
    fn a_closed_offer_is_forgotten() {
        let mut update = installed();
        let _ = update.update(Message::Offered(7));
        let _ = update.update(Message::Notification(notify::Event::Closed(7)));
        assert_eq!(update.offered, None);
    }
}

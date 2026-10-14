//! The bar's own actions, in the dormer: calibrate the screen, restart the bar, and check its
//! health (D-98). Nothing in the bar itself.
//!
//! ```toml
//! [[dormer]]
//! widget = "essential"
//! ```
//!
//! Each runs what a terminal would. Calibrate screen and Check health run `eave calibrate` and
//! `eave doctor`, with the binary the bar runs, through Hyprland's exec, so they aren't the bar's
//! children. Check health waits for the report doctor writes and shows it in a dialog. Restart
//! bar asks systemd to restart eave.service, and is there only when this bar is that service.

mod health;

use eave_api::ui::{self, Action, Icon, Item, Panel, Tone};
use eave_api::{Effect, NoConfig, Registry, Task, Widget};

pub use health::{Check, Level};

const SERVICE: &str = "eave.service";

pub fn register(registry: &mut Registry) {
    registry.widget::<Essential>("essential");
}

pub struct Essential {
    /// Whether this bar is eave.service's, so restarting that restarts it.
    service: bool,
    health: Health,
}

/// Where Check health is.
#[derive(Debug, Clone, PartialEq)]
pub enum Health {
    /// Never checked.
    Unknown,
    /// eave doctor is running.
    Checking,
    /// What it found, problems first.
    Checked(Vec<Check>),
}

#[derive(Debug, Clone)]
pub enum Message {
    /// Whether this bar is eave.service's.
    Service(bool),
    Calibrate,
    Restart,
    CheckHealth,
    /// What eave doctor found, or why it couldn't say.
    Checked(Result<Vec<Check>, String>),
}

impl Widget for Essential {
    type Config = NoConfig;
    type Message = Message;

    fn new(_: NoConfig) -> (Self, Task<Message>) {
        let essential = Self {
            service: false,
            health: Health::Unknown,
        };
        (essential, Task::future(service()).map(Message::Service))
    }

    fn update(&mut self, message: Message) -> Effect<Message> {
        match message {
            Message::Service(service) => {
                self.service = service;
                Effect::none()
            }
            Message::Calibrate => Effect::task(Task::future(calibrate()).discard()),
            Message::Restart => Effect::task(Task::future(restart()).discard()),
            Message::CheckHealth if self.health == Health::Checking => Effect::none(),
            Message::CheckHealth => {
                self.health = Health::Checking;
                Effect::task(Task::future(health::check()).map(Message::Checked))
            }
            Message::Checked(result) => {
                self.health = Health::Checked(health::ordered(result.unwrap_or_else(|error| {
                    vec![Check {
                        level: Level::Problem,
                        title: "Couldn't run eave doctor".into(),
                        detail: error,
                    }]
                })));
                Effect::none().open_dialog()
            }
        }
    }

    fn item(&self) -> Item<Message> {
        Item::hidden()
    }

    fn dialog(&self) -> Option<Panel<Message>> {
        let Health::Checked(checks) = &self.health else {
            return None;
        };
        let rows = checks.iter().map(|check| {
            let (icon, tone) = match check.level {
                Level::Good => (Icon::Check, Tone::Good),
                Level::Warning => (Icon::Info, Tone::Warning),
                Level::Problem => (Icon::Close, Tone::Critical),
            };
            let row = ui::list_item(&check.title).leading(ui::icon(icon).tone(tone));
            match check.detail.is_empty() {
                true => row,
                false => row.subtitle(check.detail.replace('\n', " ")),
            }
        });
        let content = ui::column([
            ui::header("Health").subtitle(health::summary(checks)),
            ui::list(rows),
        ]);
        Some(Panel::new(content))
    }

    fn actions(&self) -> Vec<Action<Message>> {
        let mut actions = vec![
            ui::action("Calibrate screen", Message::Calibrate)
                .id("calibrate")
                .subtitle("Fit the bar to the screen's corners and notch")
                .keywords(["eave", "notch", "corners", "measure", "screen", "display"]),
        ];
        if self.service {
            actions.push(
                ui::action("Restart bar", Message::Restart)
                    .id("restart")
                    .keywords(["eave", "reload", "renderer"]),
            );
        }
        let checking = self.health == Health::Checking;
        actions.push(
            ui::action(
                match checking {
                    true => "Checking health…",
                    false => "Check health",
                },
                Message::CheckHealth,
            )
            .id("health")
            .subtitle("config.toml, widgets, keys, screen and systemd")
            .keywords(["eave", "doctor", "problems", "config", "diagnose"]),
        );
        actions
    }
}

/// The binary this bar runs, to run its other commands with. Linux names a binary replaced since
/// (by eave-build) "… (deleted)": the new one, at the same path, is the one to run.
fn exe() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    let exe = exe.to_str()?;
    Some(exe.strip_suffix(" (deleted)").unwrap_or(exe).to_owned())
}

/// `text` as one word for a shell, which Hyprland's exec runs commands with.
fn quoted(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

/// Has Hyprland run this bar's binary with `args`.
async fn run(args: &str) -> Result<(), String> {
    let exe = exe().ok_or("can't find the bar's own binary")?;
    let command = format!("{} {args}", quoted(&exe));
    let escaped = command.replace('\\', r"\\").replace('"', "\\\"");
    let lua = format!("hl.dsp.exec_cmd(\"{escaped}\")");
    eave_hyprland::dispatch(&lua, &format!("exec {command}"))
        .await
        .map_err(|error| format!("can't run `{command}`: {error}"))
}

async fn calibrate() {
    if let Err(error) = run("calibrate").await {
        eprintln!("eave: essential: {error}");
    }
}

async fn restart() {
    if let Err(error) = eave_dbus::systemd::restart(SERVICE).await {
        eprintln!("eave: essential: can't restart {SERVICE}: {error}");
    }
}

/// Whether this bar is eave.service's current run: systemd gives the processes it starts the
/// run's id. A bar started by hand, or from Hyprland's config, isn't, even with the service up.
async fn service() -> bool {
    let Ok(ours) = std::env::var("INVOCATION_ID") else {
        return false;
    };
    match eave_dbus::systemd::unit(SERVICE).await {
        Ok(unit) => unit.invocation == ours,
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eave_api::erased::DialogRequest;

    fn essential() -> Essential {
        Essential::new(NoConfig::default()).0
    }

    fn titles(essential: &Essential) -> Vec<String> {
        essential
            .actions()
            .into_iter()
            .map(|action| action.into_parts().title)
            .collect()
    }

    #[test]
    fn restart_is_there_only_under_the_service() {
        let mut essential = essential();
        assert_eq!(titles(&essential), ["Calibrate screen", "Check health"]);
        let _ = essential.update(Message::Service(true));
        assert_eq!(
            titles(&essential),
            ["Calibrate screen", "Restart bar", "Check health"]
        );
        assert!(essential.item().into_parts().content.is_none());
    }

    #[test]
    fn checking_health_shows_until_the_report_comes() {
        let mut essential = essential();
        assert!(essential.dialog().is_none());
        let _ = essential.update(Message::CheckHealth);
        assert!(titles(&essential).contains(&"Checking health…".to_owned()));
        // Picked again while it runs: still the one check.
        let _ = essential.update(Message::CheckHealth);
        assert_eq!(essential.health, Health::Checking);

        let report = vec![
            Check {
                level: Level::Good,
                title: "config.toml reads".into(),
                detail: String::new(),
            },
            Check {
                level: Level::Problem,
                title: "`weather` isn't in this build".into(),
                detail: "Run eave-build.".into(),
            },
        ];
        let effect = essential.update(Message::Checked(Ok(report)));
        assert_eq!(effect.dialog_request(), Some(DialogRequest::Open));
        assert!(titles(&essential).contains(&"Check health".to_owned()));
        let Health::Checked(checks) = &essential.health else {
            panic!("checked");
        };
        assert_eq!(checks[0].level, Level::Problem);
        assert!(essential.dialog().is_some());
    }

    #[test]
    fn a_doctor_that_cant_run_is_a_problem_to_show() {
        let mut essential = essential();
        let _ = essential.update(Message::CheckHealth);
        let _ = essential.update(Message::Checked(Err("no Hyprland".into())));
        let Health::Checked(checks) = &essential.health else {
            panic!("checked");
        };
        assert_eq!(checks.len(), 1);
        assert_eq!(checks[0].level, Level::Problem);
        assert_eq!(checks[0].detail, "no Hyprland");
    }

    #[test]
    fn paths_are_one_word_for_the_shell() {
        assert_eq!(quoted("/home/a b/eave"), "'/home/a b/eave'");
        assert_eq!(quoted("/x/it's"), r"'/x/it'\''s'");
    }
}

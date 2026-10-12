use crate::Widget;
use crate::erased::{Constructor, construct};

/// The widgets every plugin makes available, by the name `config.toml` uses.
/// Cheap to clone: names and function pointers.
#[derive(Default, Clone)]
pub struct Registry {
    widgets: Vec<(&'static str, Constructor)>,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Makes `W` available as `widget = "<name>"` in the config.
    pub fn widget<W: Widget>(&mut self, name: &'static str) {
        self.widgets.push((name, construct::<W>));
    }

    #[doc(hidden)]
    pub fn get(&self, name: &str) -> Option<Constructor> {
        self.widgets
            .iter()
            .find(|(registered, _)| *registered == name)
            .map(|(_, constructor)| *constructor)
    }

    /// Names registered more than once. Two plugins claiming one name is a build error to report,
    /// not something to resolve silently.
    #[doc(hidden)]
    pub fn duplicates(&self) -> Vec<&'static str> {
        let mut names: Vec<_> = self.widgets.iter().map(|(name, _)| *name).collect();
        names.sort_unstable();
        let mut duplicates: Vec<_> = names
            .windows(2)
            .filter(|pair| pair[0] == pair[1])
            .map(|pair| pair[0])
            .collect();
        duplicates.dedup();
        duplicates
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::{Item, label};
    use crate::{Effect, Task};

    struct Dummy;

    impl Widget for Dummy {
        type Config = toml::Table;
        type Message = ();

        fn new(_: Self::Config) -> (Self, Task<()>) {
            (Self, Task::none())
        }

        fn update(&mut self, _: ()) -> Effect<()> {
            Effect::none()
        }

        fn item(&self) -> Item<()> {
            Item::new(label("dummy"))
        }
    }

    #[test]
    fn finds_and_reports_duplicates() {
        let mut registry = Registry::new();
        registry.widget::<Dummy>("a");
        registry.widget::<Dummy>("b");
        registry.widget::<Dummy>("a");
        assert!(registry.get("b").is_some());
        assert!(registry.get("c").is_none());
        assert_eq!(registry.duplicates(), ["a"]);
    }
}

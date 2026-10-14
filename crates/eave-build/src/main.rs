//! Builds an eave binary with the plugins `config.toml` uses, the way xcaddy builds Caddy.
//!
//! ```text
//! eave-build [--config <config.toml>]          build, then install and restart if it changed
//! eave-build update [--config <config.toml>]   fetch newer eave and plugins first
//! eave-build init                              starter config.toml and systemd units
//! ```
//!
//! Plugins are compiled in, never loaded at runtime ([D-12](../../docs/decisions.md)). Every
//! `widget = "…"` in config.toml names where it comes from ([D-30](../../docs/decisions.md)):
//! `clock` is built in, `someone/weather` is a GitHub repository, and a git URL or a local path
//! works too. For each source this finds its checkout (cloning it the first time), finds the
//! plugin it names (in its `eave-collection.toml`, or the `eave-plugin.toml` at its root), checks
//! it targets this eave's plugin API, then generates a small crate whose `main` calls `eave::run`
//! with every plugin's `register`, and builds it with cargo ([D-46](../../docs/decisions.md)).
//!
//! Nothing moves to a newer version unless you run `update` ([D-31](../../docs/decisions.md)).
//! The binary is copied to `~/.local/bin/eave` only when it differs from the one there, and then a
//! running bar is restarted on it: through systemd when the `eave.service` user unit runs it,
//! otherwise by stopping it and relaunching it through Hyprland's socket
//! ([D-22](../../docs/decisions.md)).

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::Duration;

use serde::Deserialize;

/// Where eave comes from when config.toml has no `eave = "…"`.
const DEFAULT_EAVE: &str = "eaveshell/eave";
/// Set when a distribution package builds eave-build ([D-54](../../docs/decisions.md)). The bar
/// then defaults to the eave release the package is, and the package manager updates eave-build.
const PACKAGED: bool = option_env!("EAVE_PACKAGE").is_some();
const SERVICE: &str = "eave.service";
/// In the environment of an eave-build that `update` just installed and handed over to.
const HANDED_OVER: &str = "EAVE_BUILD_HANDED_OVER";
/// One plugin's manifest, beside its Cargo.toml.
const PLUGIN_FILE: &str = "eave-plugin.toml";
/// At a repository's root, the directories of the plugins it holds.
const COLLECTION_FILE: &str = "eave-collection.toml";
const USAGE: &str = "\
usage: eave-build [update | init] [--config <config.toml>]

  eave-build           build the plugins config.toml uses; install and restart if it changed
  eave-build update    fetch newer eave and plugins first, then the same
  eave-build init      write a starter config.toml and the systemd user units";

// What `init` writes. Taken from the repository, so `cargo install --path` or `--git` is needed,
// or a package built from the repository's tarball: this crate can't be published on its own.
const STARTER_CONFIG: &str = include_str!("../../../dist/config.toml");
const UNITS: [(&str, &str); 3] = [
    (
        "eave.service",
        include_str!("../../../dist/systemd/eave.service"),
    ),
    (
        "eave-rebuild.path",
        include_str!("../../../dist/systemd/eave-rebuild.path"),
    ),
    (
        "eave-rebuild.service",
        include_str!("../../../dist/systemd/eave-rebuild.service"),
    ),
];

type Result<T> = std::result::Result<T, String>;

/// Where a plugin's source is.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Location {
    /// A plugin of the eave checkout's own collection.
    Builtin(String),
    Git(String),
    Path(PathBuf),
}

/// A parsed `widget = "<source>[@rev][#widget]"`.
#[derive(Debug, Clone, PartialEq)]
struct Reference {
    text: String,
    location: Location,
    /// A commit, tag or branch. Without one, the checkout stays wherever `update` last put it.
    rev: Option<String>,
    /// The plugin it uses, which is also the widget's name: `#widget`, or the repository's name.
    widget: String,
}

#[derive(Deserialize)]
struct ManifestFile {
    plugin: Manifest,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    /// The one widget the plugin registers.
    name: String,
    #[serde(default)]
    description: String,
    api: String,
}

#[derive(Deserialize)]
struct CollectionFile {
    collection: Collection,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Collection {
    #[serde(default)]
    #[allow(dead_code)]
    description: String,
    /// Plugin directories, relative to the collection file.
    plugins: Vec<PathBuf>,
}

struct Plugin {
    manifest: Manifest,
    crate_name: String,
    crate_dir: PathBuf,
    module: String,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("eave-build: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let mut config_path = config_dir()?.join("eave/config.toml");
    let mut command: Option<String> = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "update" | "init" if command.is_none() => command = Some(arg),
            "--config" => {
                config_path = args
                    .next()
                    .map(PathBuf::from)
                    .ok_or("--config needs a path")?
            }
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(());
            }
            other => return Err(format!("unknown argument `{other}` (try --help)")),
        }
    }
    match command.as_deref() {
        Some("init") => init(&config_path),
        update => build(&config_path, update.is_some()),
    }
}

fn build(config_path: &Path, update: bool) -> Result<()> {
    let text = std::fs::read_to_string(config_path).map_err(|e| {
        format!(
            "{}: {e}. `eave-build init` writes a starter one",
            config_path.display()
        )
    })?;
    let (eave, widgets) =
        read_config(&text).map_err(|e| format!("{}: {e}", config_path.display()))?;
    let base = config_path.parent().unwrap_or(Path::new("."));
    let mut sources = Sources {
        dir: data_dir()?.join("eave/src"),
        base: base.to_path_buf(),
        update,
        moved: false,
    };

    let eave = parse_reference(eave.as_deref().unwrap_or(default_eave(PACKAGED)))
        .map_err(|e| format!("`eave = …`: {e}"))?;
    if let Location::Builtin(_) = eave.location {
        return Err(format!(
            "`eave = \"{}\"` must be owner/repo, a git URL or a path",
            eave.text
        ));
    }
    let eave_dir = sources.dir_for(&eave, "eave")?;
    let eave_moved = sources.moved;
    // Set by the run that just reinstalled this binary and handed over to it.
    let handed_over = std::env::var_os(HANDED_OVER).is_some();
    if update && !handed_over {
        update_self(config_path, &eave_dir, eave_moved)?;
    }
    let api_version = package_version(&eave_dir.join("crates/eave-api/Cargo.toml"))?;
    eprintln!(
        "eave-build: eave {} (plugin API {api_version})",
        eave_dir.display()
    );

    let mut plugins = Vec::new();
    let mut crates = HashSet::new();
    for group in group_references(widgets)? {
        let first = &group[0];
        let root = match &first.location {
            Location::Builtin(_) => eave_dir.clone(),
            _ => sources.dir_for(first, &first.widget)?,
        };
        let members = members(&root)?;
        let mut used = HashSet::new();
        for reference in &group {
            if !used.insert(&reference.widget) {
                continue;
            }
            let Some((dir, manifest)) = pick(&members, reference) else {
                if let Location::Builtin(name) = &reference.location {
                    // The bar reports it too, and skips it.
                    eprintln!("eave-build: no built-in widget `{name}`, skipping it");
                    continue;
                }
                return Err(missing(&members, reference));
            };
            let plugin = load_plugin(dir, manifest.clone(), &api_version)?;
            if !crates.insert(plugin.crate_name.clone()) {
                return Err(format!(
                    "crate `{}` comes from two sources",
                    plugin.crate_name
                ));
            }
            let description = &plugin.manifest.description;
            let gap = if description.is_empty() { "" } else { " " };
            eprintln!(
                "eave-build: + {} ({}){gap}{description}",
                plugin.manifest.name, plugin.crate_name
            );
            plugins.push(plugin);
        }
    }

    let build_dir = cache_dir()?.join("eave/build");
    generate(&build_dir, &eave_dir, &plugins)?;

    let status = Command::new("cargo")
        .args(["build", "--release", "--manifest-path"])
        .arg(build_dir.join("Cargo.toml"))
        .status()
        .map_err(|e| format!("running cargo: {e}"))?;
    if !status.success() {
        return Err("cargo build failed".into());
    }

    let target = home()?.join(".local/bin/eave");
    if install(&build_dir.join("target/release/eave"), &target)? {
        eprintln!("eave-build: installed {}", target.display());
        restart_bar(&target)?;
    } else {
        eprintln!("eave-build: {} is up to date", target.display());
    }

    // A new eave-build may ship new units. This keeps config.toml and rewrites only the units.
    if eave_moved || handed_over {
        init(config_path)?;
        // Without systemd (units written but unused) there's nothing to reload.
        let _ = Command::new("systemctl")
            .args(["--user", "daemon-reload"])
            .status();
    }
    Ok(())
}

/// When eave moved, reinstalls eave-build from it and lets the new binary finish the update: a new
/// eave may need a new build recipe (a patch it vendors), which only the new eave-build knows.
/// Returns only when there's nothing to install. A marker survives a failed install, so the next
/// `update` tries again although eave no longer moves.
fn update_self(config_path: &Path, eave_dir: &Path, eave_moved: bool) -> Result<()> {
    let marker = data_dir()?.join("eave/eave-build-outdated");
    if !eave_moved && !marker.exists() {
        return Ok(());
    }
    // Before the install replaces this binary: Linux then reports its path as "… (deleted)".
    let exe = std::env::current_exe().map_err(|e| format!("finding eave-build itself: {e}"))?;
    if PACKAGED || !exe.starts_with(cargo_bin()?) {
        // A package manager owns it: a cargo copy would shadow it (D-54).
        eprintln!("eave-build: eave-build itself is updated by your package manager");
        return Ok(());
    }
    std::fs::write(&marker, "").map_err(|e| format!("{}: {e}", marker.display()))?;
    let status = Command::new("cargo")
        .args(["install", "--locked", "--quiet", "--path"])
        .arg(eave_dir.join("crates/eave-build"))
        .status()
        .map_err(|e| format!("running cargo: {e}"))?;
    if !status.success() {
        return Err(
            "updating eave-build itself failed. The bar is untouched; `eave-build update` tries again"
                .into(),
        );
    }
    let _ = std::fs::remove_file(&marker);
    eprintln!("eave-build: updated eave-build, continuing with it");
    use std::os::unix::process::CommandExt;
    let error = Command::new(exe)
        .arg("update")
        .arg("--config")
        .arg(config_path)
        .env(HANDED_OVER, "1")
        .exec();
    Err(format!("running the new eave-build: {error}"))
}

/// `eave = "…"` and every `widget = "…"`, in bar order, then the dormer's. Malformed entries are
/// left to the bar, which reports them.
fn read_config(text: &str) -> Result<(Option<String>, Vec<String>)> {
    let table: toml::Table = text.parse().map_err(|e| format!("{e}"))?;
    let eave = match table.get("eave") {
        None => None,
        Some(toml::Value::String(eave)) => Some(eave.clone()),
        Some(_) => return Err("`eave` must be a string".into()),
    };
    let widgets = ["left", "center", "right", "dormer"]
        .iter()
        .filter_map(|zone| table.get(*zone)?.as_array())
        .flatten()
        .filter_map(|entry| Some(entry.get("widget")?.as_str()?.to_owned()))
        .collect();
    Ok((eave, widgets))
}

/// References grouped by source, in order of first use. One source asked for at two revisions is
/// an error: a binary can hold only one.
fn group_references(widgets: Vec<String>) -> Result<Vec<Vec<Reference>>> {
    let mut groups: Vec<Vec<Reference>> = Vec::new();
    for text in widgets {
        let reference = parse_reference(&text)?;
        match groups
            .iter_mut()
            .find(|group| group[0].location == reference.location)
        {
            Some(group) if group[0].rev != reference.rev => {
                return Err(format!(
                    "`{}` and `{}` ask for the same plugin at different revisions",
                    group[0].text, reference.text
                ));
            }
            Some(group) => group.push(reference),
            None => groups.push(vec![reference]),
        }
    }
    Ok(groups)
}

fn parse_reference(text: &str) -> Result<Reference> {
    let (rest, explicit) = match text.rsplit_once('#') {
        Some((_, "")) => return Err(format!("`{text}`: nothing after `#`")),
        Some((rest, name)) => (rest, Some(name)),
        None => (text, None),
    };
    let (source, rev) = split_rev(rest);
    if source.is_empty() || rev == Some("") {
        return Err(format!("`{text}` isn't a widget"));
    }
    let location = if source.starts_with(['/', '~', '.']) {
        Location::Path(PathBuf::from(source))
    } else if !source.contains(['/', ':']) {
        Location::Builtin(source.to_owned())
    } else if source.contains(':') {
        // `https://…`, `ssh://…`, `git@host:x/y`.
        Location::Git(source.to_owned())
    } else if source.matches('/').count() == 1 {
        Location::Git(format!("https://github.com/{source}"))
    } else {
        Location::Git(format!("https://{source}"))
    };
    match (&location, rev) {
        (Location::Builtin(_), Some(_)) => {
            return Err(format!(
                "`{text}`: built-in widgets come with eave; pin eave instead (`eave = \"…@rev\"`)"
            ));
        }
        (Location::Path(_), Some(_)) => {
            return Err(format!("`{text}`: a local path has no revision"));
        }
        _ => {}
    }
    Ok(Reference {
        text: text.to_owned(),
        location,
        rev: rev.map(str::to_owned),
        widget: explicit.unwrap_or_else(|| default_name(source)).to_owned(),
    })
}

/// `source@rev` → (`source`, `rev`). A revision follows the last `/` and never holds a `:`, so
/// `git@host:x/y` keeps its `@`.
fn split_rev(text: &str) -> (&str, Option<&str>) {
    let tail = text.rfind('/').map_or(0, |slash| slash + 1);
    match text[tail..].rfind('@') {
        Some(at) if !text[tail + at + 1..].contains(':') => {
            (&text[..tail + at], Some(&text[tail + at + 1..]))
        }
        _ => (text, None),
    }
}

/// The repository's name without `.git` and a leading `eave-`: `someone/eave-weather` → `weather`.
/// The bar applies the same rule (`config::widget_name`).
fn default_name(source: &str) -> &str {
    let last = source
        .trim_end_matches('/')
        .rsplit(['/', ':'])
        .next()
        .unwrap_or(source);
    let last = last.strip_suffix(".git").unwrap_or(last);
    last.strip_prefix("eave-").unwrap_or(last)
}

/// Finds source directories, cloning git sources the first time. Checkouts live in
/// `$XDG_DATA_HOME/eave/src`, not the cache, so clearing the cache never upgrades anything.
struct Sources {
    dir: PathBuf,
    /// Where relative paths start: the directory holding config.toml.
    base: PathBuf,
    update: bool,
    /// Whether the last `dir_for` moved a checkout.
    moved: bool,
}

impl Sources {
    /// The directory holding `reference`'s source. Built-ins have none of their own: they are
    /// in the eave checkout.
    fn dir_for(&mut self, reference: &Reference, label: &str) -> Result<PathBuf> {
        self.moved = false;
        match &reference.location {
            Location::Builtin(name) => Err(format!("`{name}` is built in, not a source")),
            Location::Path(path) => {
                let path = expand_home(path)?;
                let path = if path.is_absolute() {
                    path
                } else {
                    self.base.join(path)
                };
                std::fs::canonicalize(&path).map_err(|e| format!("{}: {e}", path.display()))
            }
            Location::Git(url) => self.checkout(url, reference.rev.as_deref(), label),
        }
    }

    /// The checkout of `url`. It stays where it is unless `rev` names another commit, or this is
    /// an `update`, which fetches and moves to the newest commit of `rev` or the default branch.
    fn checkout(&mut self, url: &str, rev: Option<&str>, label: &str) -> Result<PathBuf> {
        let dir = self.dir.join(slug(url));
        if !dir.exists() {
            eprintln!("eave-build: cloning {url}");
            std::fs::create_dir_all(&self.dir)
                .map_err(|e| format!("{}: {e}", self.dir.display()))?;
            git(None, &["clone", "--quiet", url, &dir.to_string_lossy()])?;
        }
        let old = git_output(&dir, &["rev-parse", "HEAD"])?;
        let wanted = match rev {
            _ if self.update => {
                git(
                    Some(&dir),
                    &["fetch", "--quiet", "--tags", "--force", "origin"],
                )?;
                resolve(&dir, rev.unwrap_or("HEAD"))
            }
            // A pin just edited in config.toml may name something not fetched yet.
            Some(rev) => match resolve(&dir, rev) {
                Some(commit) => Some(commit),
                None => {
                    git(
                        Some(&dir),
                        &["fetch", "--quiet", "--tags", "--force", "origin"],
                    )?;
                    resolve(&dir, rev)
                }
            },
            None => Some(old.clone()),
        };
        let wanted =
            wanted.ok_or_else(|| format!("{url}: no revision `{}`", rev.unwrap_or("HEAD")))?;
        if wanted != old {
            git(Some(&dir), &["checkout", "--quiet", "--detach", &wanted])?;
            self.moved = true;
            let ahead = git_output(&dir, &["rev-list", "--count", &format!("{old}..{wanted}")])
                .unwrap_or_default();
            eprintln!(
                "eave-build: {label}: {} → {} ({ahead} new commits)",
                &old[..7],
                &wanted[..7]
            );
        } else if self.update {
            eprintln!("eave-build: {label}: up to date at {}", &old[..7]);
        }
        Ok(dir)
    }
}

/// The commit `rev` names in `dir`. A branch name must win over a stale local branch, so the
/// remote's is tried first; `HEAD` is the remote's default branch.
fn resolve(dir: &Path, rev: &str) -> Option<String> {
    [format!("origin/{rev}"), rev.to_owned()]
        .iter()
        .find_map(|rev| {
            git_output(
                dir,
                &[
                    "rev-parse",
                    "--verify",
                    "--quiet",
                    &format!("{rev}^{{commit}}"),
                ],
            )
            .ok()
        })
}

/// Copies `binary` over `target` unless they're already the same. Returns whether it did, so a
/// config edit that didn't change the plugins doesn't restart the bar.
fn install(binary: &Path, target: &Path) -> Result<bool> {
    let new = std::fs::read(binary).map_err(|e| format!("{}: {e}", binary.display()))?;
    if std::fs::read(target).is_ok_and(|old| old == new) {
        return Ok(false);
    }
    let bin_dir = target.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(bin_dir).map_err(|e| format!("{}: {e}", bin_dir.display()))?;
    // Copy next to the target, then rename: replacing a running binary in place would fail.
    let staging = bin_dir.join(".eave.new");
    std::fs::copy(binary, &staging).map_err(|e| format!("{}: {e}", staging.display()))?;
    std::fs::rename(&staging, target).map_err(|e| format!("{}: {e}", target.display()))?;
    Ok(true)
}

/// Writes a starter config.toml unless there is one, and the systemd user units. Enabling them
/// is up to the user (or install.sh): which one to enable depends on how Hyprland is started.
fn init(config_path: &Path) -> Result<()> {
    starter_config(config_path)?;

    let units = config_dir()?.join("systemd/user");
    std::fs::create_dir_all(&units).map_err(|e| format!("{}: {e}", units.display()))?;
    let watched = std::path::absolute(config_path).map_err(|e| format!("{e}"))?;
    let watched = (watched != home()?.join(".config/eave/config.toml")).then_some(watched);
    let exe = std::env::current_exe().map_err(|e| format!("finding eave-build itself: {e}"))?;
    for (name, unit) in UNITS {
        write_if_changed(
            &units.join(name),
            &customize_unit(unit, watched.as_deref(), &exe),
        )?;
    }
    eprintln!(
        "eave-build: wrote the systemd user units to {}",
        units.display()
    );
    Ok(())
}

/// Writes the starter config.toml if there's none. A symlink (a dotfiles manager's) is never
/// replaced or written through: one pointing nowhere is left for its owner to fix.
fn starter_config(config_path: &Path) -> Result<()> {
    if config_path.exists() {
        eprintln!("eave-build: keeping {}", config_path.display());
    } else if let Ok(target) = std::fs::read_link(config_path) {
        eprintln!(
            "eave-build: {} points to {}, which doesn't exist; not writing a starter config",
            config_path.display(),
            target.display()
        );
    } else {
        let dir = config_path.parent().unwrap_or(Path::new("."));
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        std::fs::write(config_path, STARTER_CONFIG)
            .map_err(|e| format!("{}: {e}", config_path.display()))?;
        eprintln!("eave-build: wrote {}", config_path.display());
    }
    Ok(())
}

/// Fills in a unit's paths. The path unit watches ~/.config/eave/config.toml unless `watched`
/// says otherwise. The rebuild runs `exe`, the eave-build that wrote the units: systemd only
/// finds a bare command name in /usr/bin and /usr/local/bin, so it needs the full path.
fn customize_unit(unit: &str, watched: Option<&Path>, exe: &Path) -> String {
    let mut unit = unit.to_owned();
    if let Some(watched) = watched {
        unit = unit.replace(
            "%h/.config/eave/config.toml",
            &watched.to_string_lossy().replace('%', "%%"),
        );
    }
    let exe = exe
        .to_string_lossy()
        .replace('%', "%%")
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    unit.replace(
        "ExecStart=%h/.cargo/bin/eave-build",
        &format!("ExecStart=\"{exe}\""),
    )
}

/// The plugins a source holds, with their directories: those its `eave-collection.toml` lists,
/// or the one whose `eave-plugin.toml` is at its root.
fn members(root: &Path) -> Result<Vec<(PathBuf, Manifest)>> {
    let collection_path = root.join(COLLECTION_FILE);
    let single = root.join(PLUGIN_FILE).exists();
    if !collection_path.exists() {
        if !single {
            return Err(format!(
                "{}: no {COLLECTION_FILE} or {PLUGIN_FILE}",
                root.display()
            ));
        }
        return Ok(vec![(root.to_path_buf(), read_manifest(root)?)]);
    }
    if single {
        return Err(format!(
            "{}: has both {COLLECTION_FILE} and {PLUGIN_FILE}; list the plugin in the collection instead",
            root.display()
        ));
    }
    let text = std::fs::read_to_string(&collection_path)
        .map_err(|e| format!("{}: {e}", collection_path.display()))?;
    let collection = toml::from_str::<CollectionFile>(&text)
        .map_err(|e| format!("{}: {e}", collection_path.display()))?
        .collection;
    let mut members: Vec<(PathBuf, Manifest)> = Vec::new();
    for dir in collection.plugins {
        let dir = root.join(dir);
        let manifest = read_manifest(&dir)?;
        if members.iter().any(|(_, m)| m.name == manifest.name) {
            return Err(format!(
                "{}: two plugins are named `{}`",
                collection_path.display(),
                manifest.name
            ));
        }
        members.push((dir, manifest));
    }
    Ok(members)
}

fn read_manifest(dir: &Path) -> Result<Manifest> {
    let path = dir.join(PLUGIN_FILE);
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let table: toml::Table = text
        .parse()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let plugin = table.get("plugin").and_then(|p| p.as_table());
    if plugin.is_some_and(|p| p.contains_key("crate") || p.contains_key("path")) {
        return Err(format!(
            "{}: `crate` and `path` are gone; the manifest sits beside its Cargo.toml now, so remove them",
            path.display()
        ));
    }
    toml::from_str::<ManifestFile>(&text)
        .map(|file| file.plugin)
        .map_err(|e| format!("{}: {e}", path.display()))
}

/// The member `reference` uses: the one with its name.
fn pick<'a>(
    members: &'a [(PathBuf, Manifest)],
    reference: &Reference,
) -> Option<&'a (PathBuf, Manifest)> {
    members.iter().find(|(_, m)| m.name == reference.widget)
}

/// Why `reference` names no member, and what it could say instead.
fn missing(members: &[(PathBuf, Manifest)], reference: &Reference) -> String {
    let source = reference
        .text
        .rsplit_once('#')
        .map_or(reference.text.as_str(), |(source, _)| source);
    let choices: Vec<String> = members
        .iter()
        .map(|(_, m)| format!("`{source}#{}`", m.name))
        .collect();
    format!(
        "`{}`: no plugin `{}` there. Write one of: {}",
        reference.text,
        reference.widget,
        choices.join(", ")
    )
}

/// The plugin in `dir`, once its manifest is checked against this eave's API.
fn load_plugin(dir: &Path, manifest: Manifest, api_version: &str) -> Result<Plugin> {
    if !api_compatible(&manifest.api, api_version) {
        return Err(format!(
            "plugin `{}` targets eave plugin API {}, but this eave provides {api_version}",
            manifest.name, manifest.api
        ));
    }

    let cargo_toml = dir.join("Cargo.toml");
    let package: toml::Table = read_toml(&cargo_toml)?;
    let name = package
        .get("package")
        .and_then(|p| p.get("name"))
        .and_then(|n| n.as_str())
        .ok_or_else(|| format!("{}: no package name", cargo_toml.display()))?;
    let module = package
        .get("lib")
        .and_then(|lib| lib.get("name"))
        .and_then(|n| n.as_str())
        .map_or_else(|| name.replace('-', "_"), str::to_owned);

    Ok(Plugin {
        manifest,
        crate_name: name.to_owned(),
        crate_dir: std::fs::canonicalize(dir).map_err(|e| format!("{}: {e}", dir.display()))?,
        module,
    })
}

/// Puts the freshly installed `binary` in place of a running bar. Only called after a successful
/// install, so a failed build never takes the old bar down.
fn restart_bar(binary: &Path) -> Result<()> {
    let under_systemd = Command::new("systemctl")
        .args(["--user", "is-active", "--quiet", SERVICE])
        .status()
        .is_ok_and(|status| status.success());
    if under_systemd {
        let status = Command::new("systemctl")
            .args(["--user", "restart", SERVICE])
            .status()
            .map_err(|e| format!("running systemctl: {e}"))?;
        if !status.success() {
            return Err(format!("systemctl --user restart {SERVICE} failed"));
        }
        eprintln!("eave-build: restarted {SERVICE}");
        return Ok(());
    }

    let pids = running_bars(binary);
    if pids.is_empty() {
        eprintln!("eave-build: eave isn't running; not starting it");
        return Ok(());
    }
    // Check before stopping anything: without Hyprland we couldn't bring the bar back.
    let socket = hyprland_socket().map_err(|error| {
        format!("eave is running, but it can't be relaunched through Hyprland ({error}); restart it yourself")
    })?;

    for &pid in &pids {
        let pid =
            rustix::process::Pid::from_raw(pid as i32).ok_or_else(|| format!("bad pid {pid}"))?;
        match rustix::process::kill_process(pid, rustix::process::Signal::TERM) {
            // Exited on its own in the meantime.
            Ok(()) | Err(rustix::io::Errno::SRCH) => {}
            Err(error) => return Err(format!("couldn't stop the running eave: {error}")),
        }
    }
    // A one-off wait in a CLI, not the bar: polling /proc here is fine.
    let gone = || {
        pids.iter()
            .all(|pid| !Path::new(&format!("/proc/{pid}")).exists())
    };
    for _ in 0..40 {
        if gone() {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    if !gone() {
        return Err("the running eave didn't exit within 2 s".into());
    }

    // Through Hyprland, so the bar gets the session's environment and outlives this terminal.
    // Lua configs (Hyprland 0.56+) take `eval`; older ones only know `dispatch exec`. A failed
    // `eval` launched nothing, so trying both can't start two bars.
    // Both run the command through a shell.
    let command = shell_quote(&binary.to_string_lossy());
    let launched = hyprland(
        &socket,
        &format!("eval hl.exec_cmd({})", lua_string(&command)),
    )
    .or_else(|_| hyprland(&socket, &format!("dispatch exec {command}")));
    if let Err(error) = launched {
        return Err(format!("relaunching eave through Hyprland: {error}"));
    }
    eprintln!("eave-build: restarted eave");
    Ok(())
}

/// Hyprland's request socket. `HYPRLAND_INSTANCE_SIGNATURE` names the instance. Without it, as
/// in a systemd unit when the session didn't export it, the one running instance is used.
fn hyprland_socket() -> Result<PathBuf> {
    // Hyprland moved its sockets from /tmp/hypr to $XDG_RUNTIME_DIR/hypr in 0.40.
    let dirs: Vec<PathBuf> = std::env::var_os("XDG_RUNTIME_DIR")
        .map(|dir| PathBuf::from(dir).join("hypr"))
        .into_iter()
        .chain([PathBuf::from("/tmp/hypr")])
        .collect();
    if let Some(signature) = std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE") {
        let dir = dirs
            .iter()
            .find(|dir| dir.join(&signature).exists())
            .unwrap_or(&dirs[dirs.len() - 1]);
        return Ok(dir.join(signature).join(".socket.sock"));
    }
    dirs.iter()
        .find_map(|dir| only_instance(dir))
        .ok_or_else(|| {
            "HYPRLAND_INSTANCE_SIGNATURE isn't set, and there isn't exactly one Hyprland running"
                .into()
        })
}

/// The request socket of the only Hyprland instance in `dir`. None when there are none, or
/// several and we can't tell which one is ours.
fn only_instance(dir: &Path) -> Option<PathBuf> {
    let mut sockets = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|entry| entry.path().join(".socket.sock"))
        .filter(|socket| socket.exists());
    let socket = sockets.next()?;
    sockets.next().is_none().then_some(socket)
}

/// One command on Hyprland's request socket, as `hyprctl` sends it, without needing `hyprctl`.
/// Hyprland answers `ok` on success and an error message otherwise.
fn hyprland(path: &Path, command: &str) -> Result<()> {
    use std::io::{Read, Write};

    let mut socket = std::os::unix::net::UnixStream::connect(path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let mut reply = String::new();
    socket
        .write_all(command.as_bytes())
        .and_then(|()| socket.read_to_string(&mut reply))
        .map_err(|e| format!("{}: {e}", path.display()))?;
    match reply.trim() {
        "ok" => Ok(()),
        error => Err(error.to_owned()),
    }
}

/// A single-quoted POSIX shell word.
fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

/// A double-quoted Lua string literal.
fn lua_string(text: &str) -> String {
    let mut quoted = String::from("\"");
    for c in text.chars() {
        match c {
            '\\' | '"' => quoted.extend(['\\', c]),
            '\n' => quoted.push_str("\\n"),
            c => quoted.push(c),
        }
    }
    quoted.push('"');
    quoted
}

/// Our running bars started from `binary`. A dev build elsewhere (`target/release/eave`) is left
/// alone. Other users' processes are skipped because their `exe` can't be read.
fn running_bars(binary: &Path) -> Vec<u32> {
    let binary = std::fs::canonicalize(binary).unwrap_or_else(|_| binary.to_path_buf());
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let pid: u32 = entry.file_name().to_str()?.parse().ok()?;
            let comm = std::fs::read_to_string(entry.path().join("comm")).ok()?;
            let exe = std::fs::read_link(entry.path().join("exe")).ok()?;
            is_bar(&comm, &exe, &binary).then_some(pid)
        })
        .collect()
}

/// `exe` gains a ` (deleted)` suffix once the install has renamed a new binary over the old one.
fn is_bar(comm: &str, exe: &Path, binary: &Path) -> bool {
    let exe = exe.as_os_str().as_encoded_bytes();
    let binary = binary.as_os_str().as_encoded_bytes();
    comm.trim_end() == "eave"
        && exe
            .strip_prefix(binary)
            .is_some_and(|rest| rest.is_empty() || rest == b" (deleted)")
}

/// Cargo's caret rule: `0.x` releases are compatible within a minor, later ones within a major.
fn api_compatible(wanted: &str, provided: &str) -> bool {
    let parse =
        |v: &str| -> Vec<u64> { v.split('.').map_while(|part| part.parse().ok()).collect() };
    let (wanted, provided) = (parse(wanted), parse(provided));
    let (Some(&w_major), Some(&p_major)) = (wanted.first(), provided.first()) else {
        return false;
    };
    if w_major != p_major {
        return false;
    }
    let w_minor = wanted.get(1).copied().unwrap_or(0);
    let p_minor = provided.get(1).copied().unwrap_or(0);
    if w_major == 0 {
        w_minor == p_minor
    } else {
        w_minor <= p_minor
    }
}

/// What a git command prints, trimmed.
fn git_output(dir: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .map_err(|e| format!("running git: {e}"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    } else {
        Err(format!("git {}: failed", args.join(" ")))
    }
}

fn git(dir: Option<&Path>, args: &[&str]) -> Result<()> {
    let mut command = Command::new("git");
    if let Some(dir) = dir {
        command.arg("-C").arg(dir);
    }
    let output = command
        .args(args)
        .output()
        .map_err(|e| format!("running git: {e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

fn generate(dir: &Path, eave_dir: &Path, plugins: &[Plugin]) -> Result<()> {
    let api_dir = eave_dir.join("crates/eave-api");
    let mut manifest = String::from(
        "# Generated by eave-build. Edit config.toml instead.\n\
         [package]\nname = \"eave-custom\"\nversion = \"0.0.0\"\nedition = \"2024\"\npublish = false\n\n\
         [[bin]]\nname = \"eave\"\npath = \"src/main.rs\"\n\n\
         [dependencies]\n",
    );
    // Without the built-in plugins it doesn't use, but with both renderers (D-67).
    manifest += &format!(
        "eave = {{ path = {}, default-features = false, features = [\"gpu\"] }}\n",
        quote(&eave_dir.join("crates/eave"))
    );
    for plugin in plugins {
        manifest += &format!(
            "{} = {{ path = {} }}\n",
            plugin.crate_name,
            quote(&plugin.crate_dir)
        );
    }
    // Plugins depend on `eave-api = "0.x"`. The patch points that at this eave's own copy, so the
    // host and every plugin share one eave-api, published on crates.io or not.
    // Same for the service crates plugins may use.
    manifest += &format!(
        "\n[patch.crates-io]\neave-api = {{ path = {} }}\n",
        quote(&api_dir)
    );
    for service in [
        "eave-dbus",
        "eave-hyprland",
        "eave-iwd",
        "eave-mpris",
        "eave-net",
        "eave-pulse",
    ] {
        let dir = eave_dir.join("crates").join(service);
        manifest += &format!("{service} = {{ path = {} }}\n", quote(&dir));
    }
    // eave's patched softbuffer, so transparent pixels stay transparent (D-25).
    manifest += &format!(
        "softbuffer = {{ path = {} }}\n",
        quote(&eave_dir.join("vendor/softbuffer"))
    );
    // And its patched iced_tiny_skia, which redraws less (D-66). Only if it has one: an older eave,
    // pinned to a tag, predates it.
    let tiny_skia = eave_dir.join("vendor/iced_tiny_skia");
    if tiny_skia.is_dir() {
        manifest += &format!("iced_tiny_skia = {{ path = {} }}\n", quote(&tiny_skia));
    }
    // And its patched layershellev, which hears touchpad holds (D-79). Only if it has one.
    let layershellev = eave_dir.join("vendor/layershellev");
    if layershellev.is_dir() {
        manifest += &format!("layershellev = {{ path = {} }}\n", quote(&layershellev));
    }
    // Same profile as eave's own release builds (D-10).
    manifest += "\n[profile.release]\nlto = \"thin\"\ncodegen-units = 1\nstrip = true\npanic = \"abort\"\n\n\
                 # Standalone: never part of an enclosing workspace.\n[workspace]\n";

    let registers: Vec<String> = plugins
        .iter()
        .map(|p| format!("{}::register", p.module))
        .collect();
    let main = format!(
        "// Generated by eave-build. Edit config.toml instead.\n\n\
         fn main() -> Result<(), eave::Error> {{\n    eave::run(&[{}])\n}}\n",
        registers.join(", ")
    );

    std::fs::create_dir_all(dir.join("src")).map_err(|e| format!("{}: {e}", dir.display()))?;
    write_if_changed(&dir.join("Cargo.toml"), &manifest)?;
    write_if_changed(&dir.join("src/main.rs"), &main)
}

/// Leaves unchanged files alone, so cargo can skip the rebuild.
fn write_if_changed(path: &Path, contents: &str) -> Result<()> {
    if std::fs::read_to_string(path).is_ok_and(|old| old == contents) {
        return Ok(());
    }
    std::fs::write(path, contents).map_err(|e| format!("{}: {e}", path.display()))
}

fn quote(path: &Path) -> String {
    toml::Value::String(path.to_string_lossy().into_owned()).to_string()
}

fn read_toml(path: &Path) -> Result<toml::Table> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    text.parse().map_err(|e| format!("{}: {e}", path.display()))
}

fn package_version(cargo_toml: &Path) -> Result<String> {
    read_toml(cargo_toml)?
        .get("package")
        .and_then(|p| p.get("version"))
        .and_then(|v| v.as_str())
        .map(str::to_owned)
        .ok_or_else(|| format!("{}: no package version", cargo_toml.display()))
}

/// A directory name for a git URL: `https://github.com/a/b` → `github.com-a-b`.
fn slug(url: &str) -> String {
    let trimmed = url
        .split("://")
        .last()
        .unwrap_or(url)
        .trim_end_matches(".git");
    let slug: String = trimmed
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    // `file:///x` would start with `-`, which commands read as an option.
    slug.trim_start_matches('-').to_owned()
}

/// The eave a config.toml without `eave = "…"` builds. A packaged eave-build builds its own
/// release, so upgrading the package moves the bar too.
fn default_eave(packaged: bool) -> &'static str {
    if packaged {
        concat!("eaveshell/eave@v", env!("CARGO_PKG_VERSION"))
    } else {
        DEFAULT_EAVE
    }
}

/// Where `cargo install` puts binaries, as install.sh finds it.
fn cargo_bin() -> Result<PathBuf> {
    let root = match std::env::var_os("CARGO_INSTALL_ROOT").filter(|v| !v.is_empty()) {
        Some(root) => PathBuf::from(root),
        None => xdg("CARGO_HOME", ".cargo")?,
    };
    Ok(root.join("bin"))
}

fn home() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| "HOME is not set".into())
}

fn expand_home(path: &Path) -> Result<PathBuf> {
    match path.strip_prefix("~") {
        Ok(rest) => Ok(home()?.join(rest)),
        Err(_) => Ok(path.to_path_buf()),
    }
}

fn xdg(var: &str, fallback: &str) -> Result<PathBuf> {
    match std::env::var_os(var).filter(|v| !v.is_empty()) {
        Some(dir) => Ok(PathBuf::from(dir)),
        None => Ok(home()?.join(fallback)),
    }
}

fn config_dir() -> Result<PathBuf> {
    xdg("XDG_CONFIG_HOME", ".config")
}

fn data_dir() -> Result<PathBuf> {
    xdg("XDG_DATA_HOME", ".local/share")
}

fn cache_dir() -> Result<PathBuf> {
    xdg("XDG_CACHE_HOME", ".cache")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn units_run_eave_build_by_its_full_path() {
        let rebuild = UNITS[2].1;
        let exe = Path::new("/home/a b/.cargo/bin/eave-build");
        let unit = customize_unit(rebuild, None, exe);
        assert!(unit.contains("\nExecStart=\"/home/a b/.cargo/bin/eave-build\"\n"));
        assert!(!unit.contains("ExecStart=%h"));

        let odd = customize_unit(rebuild, None, Path::new("/x/50%\"q/eave-build"));
        assert!(odd.contains("ExecStart=\"/x/50%%\\\"q/eave-build\""));

        let path = customize_unit(UNITS[1].1, Some(Path::new("/c/eave.toml")), exe);
        assert!(path.contains("PathChanged=/c/eave.toml"));
    }

    #[test]
    fn api_compatibility_follows_caret_rules() {
        assert!(api_compatible("0.1", "0.1.0"));
        assert!(api_compatible("0.1", "0.1.7"));
        assert!(!api_compatible("0.1", "0.2.0"));
        assert!(!api_compatible("0.2", "0.1.0"));
        assert!(api_compatible("1.2", "1.4.0"));
        assert!(!api_compatible("1.5", "1.4.0"));
        assert!(!api_compatible("2.0", "1.4.0"));
        assert!(!api_compatible("banana", "0.1.0"));
    }

    #[test]
    fn packages_build_their_own_release() {
        assert_eq!(default_eave(false), "eaveshell/eave");
        let packaged = parse_reference(default_eave(true)).unwrap();
        assert_eq!(
            packaged.rev.as_deref(),
            Some(concat!("v", env!("CARGO_PKG_VERSION")))
        );
        assert_eq!(
            packaged.location,
            parse_reference("eaveshell/eave").unwrap().location
        );
    }

    #[test]
    fn finds_bars_running_the_installed_binary() {
        let binary = Path::new("/home/me/.local/bin/eave");
        assert!(is_bar("eave\n", binary, binary));
        assert!(is_bar(
            "eave\n",
            Path::new("/home/me/.local/bin/eave (deleted)"),
            binary
        ));
        assert!(!is_bar(
            "eave\n",
            Path::new("/home/me/code/eave/target/release/eave"),
            binary
        ));
        assert!(!is_bar(
            "eave\n",
            Path::new("/home/me/.local/bin/eave-old"),
            binary
        ));
        assert!(!is_bar("eave-build\n", binary, binary));
    }

    #[test]
    fn finds_the_only_hyprland_instance() {
        let dir = std::env::temp_dir().join(format!("eave-build-hypr-{}", std::process::id()));
        let instance = |name: &str| {
            std::fs::create_dir_all(dir.join(name)).unwrap();
            std::fs::write(dir.join(name).join(".socket.sock"), "").unwrap();
        };
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(only_instance(&dir), None);
        instance("a");
        // A leftover directory without a socket doesn't count.
        std::fs::create_dir_all(dir.join("stale")).unwrap();
        assert_eq!(only_instance(&dir), Some(dir.join("a/.socket.sock")));
        instance("b");
        assert_eq!(only_instance(&dir), None);
        assert_eq!(only_instance(&dir.join("missing")), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn lua_strings_are_escaped() {
        assert_eq!(lua_string("/a b/eave"), r#""/a b/eave""#);
        assert_eq!(lua_string(r#"a"b\c"#), r#""a\"b\\c""#);
    }

    #[test]
    fn shell_words_are_quoted() {
        assert_eq!(shell_quote("/a b/eave"), "'/a b/eave'");
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
    }

    #[test]
    fn slugs_are_flat_directory_names() {
        assert_eq!(
            slug("https://github.com/someone/eave-weather.git"),
            "github.com-someone-eave-weather"
        );
        assert_eq!(slug("file:///srv/git/weather"), "srv-git-weather");
    }

    #[test]
    fn references_name_their_widget() {
        // Keep in sync with the bar's `config::tests::references_name_their_widget`.
        for (reference, name) in [
            ("clock", "clock"),
            ("someone/weather", "weather"),
            ("someone/eave-weather", "weather"),
            ("someone/eave-weather@v0.2", "weather"),
            ("someone/tools#cpu", "cpu"),
            ("someone/tools@main#cpu", "cpu"),
            ("https://git.sr.ht/~x/eave-mail", "mail"),
            ("https://codeberg.org/x/eave-mail.git@1a2b3c", "mail"),
            ("git@github.com:x/eave-mail.git", "mail"),
            ("git@host:mail.git", "mail"),
            ("codeberg.org/x/mail/", "mail"),
            ("~/code/eave-weather", "weather"),
            ("./weather", "weather"),
        ] {
            assert_eq!(
                parse_reference(reference).unwrap().widget,
                name,
                "{reference}"
            );
        }
    }

    #[test]
    fn references_find_their_source() {
        let source = |text: &str| {
            let reference = parse_reference(text).unwrap();
            (reference.location, reference.rev)
        };
        let git = |url: &str| Location::Git(url.into());
        assert_eq!(source("clock"), (Location::Builtin("clock".into()), None));
        assert_eq!(
            source("someone/weather@v0.2"),
            (
                git("https://github.com/someone/weather"),
                Some("v0.2".into())
            )
        );
        assert_eq!(
            source("codeberg.org/x/mail"),
            (git("https://codeberg.org/x/mail"), None)
        );
        assert_eq!(
            source("git@github.com:x/mail.git@main#inbox"),
            (git("git@github.com:x/mail.git"), Some("main".into()))
        );
        assert_eq!(
            source("~/code/weather"),
            (Location::Path("~/code/weather".into()), None)
        );
        assert!(parse_reference("clock@v1").is_err());
        assert!(parse_reference("~/code/weather@v1").is_err());
        assert!(parse_reference("someone/weather@").is_err());
        assert!(parse_reference("someone/tools#").is_err());
    }

    #[test]
    fn references_group_by_source() {
        let groups = group_references(
            ["clock", "someone/tools#cpu", "clock", "someone/tools#ram"]
                .map(String::from)
                .to_vec(),
        )
        .unwrap();
        let sizes: Vec<_> = groups.iter().map(Vec::len).collect();
        assert_eq!(sizes, [2, 2]);
        assert!(
            group_references(vec![
                "someone/tools@v1#cpu".into(),
                "someone/tools#ram".into()
            ])
            .is_err()
        );
    }

    #[test]
    fn config_lists_widgets_in_bar_order() {
        let (eave, widgets) = read_config(
            r#"
            eave = "~/code/eave"
            [[right]]
            widget = "battery"
            [[left]]
            widget = "someone/weather"
            units = "metric"
            [[dormer]]
            widget = "apps"
            [[left]]
            format = "no widget: the bar reports it"
            "#,
        )
        .unwrap();
        assert_eq!(eave.as_deref(), Some("~/code/eave"));
        assert_eq!(widgets, ["someone/weather", "battery", "apps"]);
    }

    /// A scratch directory holding `files` (path, contents), removed by the caller.
    fn tree(label: &str, files: &[(&str, &str)]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("eave-build-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for (path, contents) in files {
            let path = dir.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, contents).unwrap();
        }
        dir
    }

    fn manifest(name: &str) -> String {
        format!("[plugin]\nname = \"{name}\"\napi = \"0.1\"\n")
    }

    fn names(members: &[(PathBuf, Manifest)]) -> Vec<&str> {
        members.iter().map(|(_, m)| m.name.as_str()).collect()
    }

    #[test]
    fn collections_hold_their_listed_plugins() {
        let (cpu, mem) = (manifest("cpu"), manifest("mem"));
        let dir = tree(
            "collection",
            &[
                (
                    COLLECTION_FILE,
                    "[collection]\nplugins = [\"cpu\", \"crates/mem\"]\n",
                ),
                ("cpu/eave-plugin.toml", &cpu),
                ("cpu/Cargo.toml", "[package]\nname = \"eave-cpu\"\n"),
                ("crates/mem/eave-plugin.toml", &mem),
                ("unlisted/eave-plugin.toml", &manifest("unlisted")),
            ],
        );
        let members = members(&dir).unwrap();
        assert_eq!(names(&members), ["cpu", "mem"]);

        let reference = parse_reference("./tools#cpu").unwrap();
        let (cpu_dir, cpu) = pick(&members, &reference).unwrap();
        let plugin = load_plugin(cpu_dir, cpu.clone(), "0.1.3").unwrap();
        assert_eq!(
            (plugin.crate_name.as_str(), plugin.module.as_str()),
            ("eave-cpu", "eave_cpu")
        );

        // `./tools` alone names a `tools` plugin, which isn't there.
        let reference = parse_reference("./tools").unwrap();
        assert!(pick(&members, &reference).is_none());
        assert_eq!(
            missing(&members, &reference),
            "`./tools`: no plugin `tools` there. Write one of: `./tools#cpu`, `./tools#mem`"
        );
        let reference = parse_reference("someone/tools@main#nope").unwrap();
        assert!(
            missing(&members, &reference)
                .ends_with("`someone/tools@main#cpu`, `someone/tools@main#mem`")
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn single_plugins_answer_only_to_their_name() {
        let dir = tree("single", &[(PLUGIN_FILE, &manifest("weather"))]);
        let members = members(&dir).unwrap();
        assert_eq!(names(&members), ["weather"]);
        for (text, found) in [
            ("someone/eave-weather", true),
            ("someone/forecast#weather", true),
            ("someone/forecast", false),
            ("someone/eave-weather#rain", false),
        ] {
            let reference = parse_reference(text).unwrap();
            assert_eq!(pick(&members, &reference).is_some(), found, "{text}");
        }
        assert_eq!(
            missing(&members, &parse_reference("someone/forecast").unwrap()),
            "`someone/forecast`: no plugin `forecast` there. Write one of: `someone/forecast#weather`"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn sources_need_exactly_one_kind_of_manifest() {
        let both = tree(
            "both",
            &[
                (COLLECTION_FILE, "[collection]\nplugins = []\n"),
                (PLUGIN_FILE, &manifest("a")),
            ],
        );
        assert!(members(&both).unwrap_err().contains("both"));
        let neither = tree("neither", &[("README", "")]);
        assert!(members(&neither).is_err());
        let twice = tree(
            "twice",
            &[
                (COLLECTION_FILE, "[collection]\nplugins = [\"a\", \"b\"]\n"),
                ("a/eave-plugin.toml", &manifest("same")),
                ("b/eave-plugin.toml", &manifest("same")),
            ],
        );
        assert!(members(&twice).unwrap_err().contains("two plugins"));
        let old = tree(
            "old",
            &[(
                PLUGIN_FILE,
                "[plugin]\nname = \"a\"\ncrate = \"eave-a\"\npath = \".\"\napi = \"0.1\"\n",
            )],
        );
        assert!(members(&old).unwrap_err().contains("are gone"));
        for dir in [both, neither, twice, old] {
            std::fs::remove_dir_all(dir).unwrap();
        }
    }

    #[test]
    fn the_built_ins_are_eaves_collection() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let members = members(&root).unwrap();
        let mut listed: Vec<String> = members.iter().map(|(_, m)| m.name.clone()).collect();
        let mut on_disk: Vec<String> = std::fs::read_dir(root.join("plugins"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        listed.sort();
        on_disk.sort();
        assert_eq!(listed, on_disk);
        for (dir, manifest) in members {
            load_plugin(&dir, manifest, env!("CARGO_PKG_VERSION")).unwrap();
        }
    }

    #[test]
    fn the_starter_config_leaves_symlinks_alone() {
        use std::os::unix::fs::symlink;

        let dir = std::env::temp_dir().join(format!("eave-build-link-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("dotfiles")).unwrap();

        // A link to a config: kept, and so is what it points to.
        let real = dir.join("dotfiles/config.toml");
        std::fs::write(&real, "# mine\n").unwrap();
        let link = dir.join("config.toml");
        symlink(&real, &link).unwrap();
        starter_config(&link).unwrap();
        assert!(std::fs::symlink_metadata(&link).unwrap().is_symlink());
        assert_eq!(std::fs::read_to_string(&real).unwrap(), "# mine\n");

        // A link to nothing: still a link to nothing.
        let gone = dir.join("dotfiles/gone.toml");
        let dangling = dir.join("dangling.toml");
        symlink(&gone, &dangling).unwrap();
        starter_config(&dangling).unwrap();
        assert!(std::fs::symlink_metadata(&dangling).unwrap().is_symlink());
        assert!(!gone.exists());

        // No file at all: the starter.
        let fresh = dir.join("new/config.toml");
        starter_config(&fresh).unwrap();
        assert_eq!(std::fs::read_to_string(&fresh).unwrap(), STARTER_CONFIG);

        std::fs::remove_dir_all(dir).unwrap();
    }
}

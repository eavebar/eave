//! Builds an eave binary with the plugins listed in `plugins.toml`, the way xcaddy builds Caddy.
//!
//! ```text
//! eave-build [--config <plugins.toml>] [--install | --restart]
//! ```
//!
//! Plugins are compiled in, never loaded at runtime ([D-12](../../docs/decisions.md)). For each
//! plugin this fetches its repository (or uses a local path), reads the `eave-plugin.toml`
//! manifest, checks it targets this eave's plugin API, then generates a small crate whose `main`
//! calls `eave::run` with every plugin's `register`, and builds it with cargo.
//!
//! `--install` copies the binary to `~/.local/bin/eave`. `--restart` also restarts a running bar
//! on it: through systemd when the `eave.service` user unit runs it, otherwise by stopping it and
//! relaunching it through Hyprland's socket ([D-22](../../docs/decisions.md)).

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::Duration;

use serde::Deserialize;

const DEFAULT_EAVE_GIT: &str = "https://github.com/francescomengarelli/eave";
const SERVICE: &str = "eave.service";
const USAGE: &str = "usage: eave-build [--config <plugins.toml>] [--install | --restart]";

type Result<T> = std::result::Result<T, String>;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PluginsFile {
    /// Where to get eave itself. Defaults to the main repository.
    eave: Option<Source>,
    #[serde(default, rename = "plugin")]
    plugins: Vec<Source>,
}

#[derive(Deserialize, Clone)]
#[serde(deny_unknown_fields)]
struct Source {
    git: Option<String>,
    /// A commit, tag or branch. Defaults to the remote's default branch.
    rev: Option<String>,
    path: Option<PathBuf>,
    /// Subdirectory holding `eave-plugin.toml`, when it isn't at the repository root.
    #[serde(default)]
    dir: PathBuf,
}

#[derive(Deserialize)]
struct ManifestFile {
    plugin: Manifest,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    name: String,
    #[serde(default)]
    description: String,
    #[serde(rename = "crate")]
    crate_name: String,
    #[serde(default = "here")]
    path: PathBuf,
    api: String,
}

fn here() -> PathBuf {
    PathBuf::from(".")
}

struct Plugin {
    manifest: Manifest,
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
    let mut config_path = config_dir()?.join("eave/plugins.toml");
    let mut install = false;
    let mut restart = false;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--config" => {
                config_path = args
                    .next()
                    .map(PathBuf::from)
                    .ok_or("--config needs a path")?
            }
            "--install" => install = true,
            "--restart" => (install, restart) = (true, true),
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(());
            }
            other => return Err(format!("unknown argument `{other}` (try --help)")),
        }
    }

    let source = std::fs::read_to_string(&config_path)
        .map_err(|e| format!("{}: {e}", config_path.display()))?;
    let file: PluginsFile =
        toml::from_str(&source).map_err(|e| format!("{}: {e}", config_path.display()))?;
    let base = config_path.parent().unwrap_or(Path::new("."));
    let cache = cache_dir()?.join("eave");

    let eave_source = file.eave.unwrap_or(Source {
        git: Some(DEFAULT_EAVE_GIT.into()),
        rev: None,
        path: None,
        dir: PathBuf::new(),
    });
    let eave_dir = fetch(&eave_source, base, &cache)?;
    let api_version = package_version(&eave_dir.join("crates/eave-api/Cargo.toml"))?;
    eprintln!(
        "eave-build: eave {} (plugin API {api_version})",
        eave_dir.display()
    );

    let mut plugins = Vec::new();
    let mut seen = HashSet::new();
    for source in &file.plugins {
        let plugin = load_plugin(source, base, &cache, &api_version)?;
        if !seen.insert(plugin.manifest.crate_name.clone()) {
            return Err(format!(
                "crate `{}` is listed twice",
                plugin.manifest.crate_name
            ));
        }
        eprintln!(
            "eave-build: + {} ({}) {}",
            plugin.manifest.name, plugin.manifest.crate_name, plugin.manifest.description
        );
        plugins.push(plugin);
    }

    let build_dir = cache.join("build");
    generate(&build_dir, &eave_dir, &plugins)?;

    let status = Command::new("cargo")
        .args(["build", "--release", "--manifest-path"])
        .arg(build_dir.join("Cargo.toml"))
        .status()
        .map_err(|e| format!("running cargo: {e}"))?;
    if !status.success() {
        return Err("cargo build failed".into());
    }
    let binary = build_dir.join("target/release/eave");

    if install {
        let bin_dir = home()?.join(".local/bin");
        std::fs::create_dir_all(&bin_dir).map_err(|e| format!("{}: {e}", bin_dir.display()))?;
        let target = bin_dir.join("eave");
        // Copy next to the target, then rename: replacing a running binary in place would fail.
        let staging = bin_dir.join(".eave.new");
        std::fs::copy(&binary, &staging).map_err(|e| format!("{}: {e}", staging.display()))?;
        std::fs::rename(&staging, &target).map_err(|e| format!("{}: {e}", target.display()))?;
        eprintln!("eave-build: installed {}", target.display());
        if restart {
            restart_bar(&target)?;
        }
    } else {
        eprintln!("eave-build: built {}", binary.display());
    }
    Ok(())
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
    if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_none() {
        return Err(
            "eave is running, but HYPRLAND_INSTANCE_SIGNATURE isn't set, so it can't be \
             relaunched through Hyprland; restart it yourself"
                .into(),
        );
    }

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
    let launched = hyprland(&format!("eval hl.exec_cmd({})", lua_string(&command)))
        .or_else(|_| hyprland(&format!("dispatch exec {command}")));
    if let Err(error) = launched {
        return Err(format!("relaunching eave through Hyprland: {error}"));
    }
    eprintln!("eave-build: restarted eave");
    Ok(())
}

/// One command on Hyprland's request socket, as `hyprctl` sends it, without needing `hyprctl`.
/// Hyprland answers `ok` on success and an error message otherwise.
fn hyprland(command: &str) -> Result<()> {
    use std::io::{Read, Write};

    let signature = std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE")
        .ok_or("HYPRLAND_INSTANCE_SIGNATURE isn't set")?;
    // Hyprland moved its sockets from /tmp/hypr to $XDG_RUNTIME_DIR/hypr in 0.40.
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(|dir| PathBuf::from(dir).join("hypr"))
        .filter(|dir| dir.join(&signature).exists())
        .unwrap_or_else(|| PathBuf::from("/tmp/hypr"));
    let path = dir.join(signature).join(".socket.sock");

    let mut socket = std::os::unix::net::UnixStream::connect(&path)
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

fn load_plugin(source: &Source, base: &Path, cache: &Path, api_version: &str) -> Result<Plugin> {
    let root = fetch(source, base, cache)?.join(&source.dir);
    let manifest_path = root.join("eave-plugin.toml");
    let text = std::fs::read_to_string(&manifest_path)
        .map_err(|e| format!("{}: {e}", manifest_path.display()))?;
    let manifest = toml::from_str::<ManifestFile>(&text)
        .map_err(|e| format!("{}: {e}", manifest_path.display()))?
        .plugin;

    if !api_compatible(&manifest.api, api_version) {
        return Err(format!(
            "plugin `{}` targets eave plugin API {}, but this eave provides {api_version}",
            manifest.name, manifest.api
        ));
    }

    let crate_dir = root.join(&manifest.path);
    let cargo_toml = crate_dir.join("Cargo.toml");
    let package: toml::Table = read_toml(&cargo_toml)?;
    let name = package
        .get("package")
        .and_then(|p| p.get("name"))
        .and_then(|n| n.as_str())
        .ok_or_else(|| format!("{}: no package name", cargo_toml.display()))?;
    if name != manifest.crate_name {
        return Err(format!(
            "{}: manifest says crate `{}`, but {} is `{name}`",
            manifest_path.display(),
            manifest.crate_name,
            cargo_toml.display()
        ));
    }
    let module = package
        .get("lib")
        .and_then(|lib| lib.get("name"))
        .and_then(|n| n.as_str())
        .map_or_else(|| name.replace('-', "_"), str::to_owned);

    Ok(Plugin {
        manifest,
        crate_dir: std::fs::canonicalize(&crate_dir)
            .map_err(|e| format!("{}: {e}", crate_dir.display()))?,
        module,
    })
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

/// A local directory with the source: a path as given, or a git checkout in the cache.
fn fetch(source: &Source, base: &Path, cache: &Path) -> Result<PathBuf> {
    match (&source.git, &source.path) {
        (Some(_), Some(_)) => Err("a source has both `git` and `path`; pick one".into()),
        (None, None) => Err("a source needs `git` or `path`".into()),
        (None, Some(path)) => {
            let path = expand_home(path)?;
            let path = if path.is_absolute() {
                path
            } else {
                base.join(path)
            };
            std::fs::canonicalize(&path).map_err(|e| format!("{}: {e}", path.display()))
        }
        (Some(url), None) => {
            let dir = cache.join("src").join(slug(url));
            if !dir.exists() {
                eprintln!("eave-build: cloning {url}");
                git(None, &["clone", "--quiet", url, &dir.to_string_lossy()])?;
            } else {
                git(
                    Some(&dir),
                    &["fetch", "--quiet", "--tags", "--force", "origin"],
                )?;
            }
            let rev = match &source.rev {
                // A branch name must win over a stale local branch, so try the remote's first.
                Some(rev)
                    if git(
                        Some(&dir),
                        &["rev-parse", "--verify", "--quiet", &format!("origin/{rev}")],
                    )
                    .is_ok() =>
                {
                    format!("origin/{rev}")
                }
                Some(rev) => rev.clone(),
                None => "origin/HEAD".into(),
            };
            git(Some(&dir), &["checkout", "--quiet", "--detach", &rev])?;
            Ok(dir)
        }
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
        "# Generated by eave-build. Edit plugins.toml instead.\n\
         [package]\nname = \"eave-custom\"\nversion = \"0.0.0\"\nedition = \"2024\"\npublish = false\n\n\
         [[bin]]\nname = \"eave\"\npath = \"src/main.rs\"\n\n\
         [dependencies]\n",
    );
    manifest += &format!(
        "eave = {{ path = {}, default-features = false }}\n",
        quote(&eave_dir.join("crates/eave"))
    );
    for plugin in plugins {
        manifest += &format!(
            "{} = {{ path = {} }}\n",
            plugin.manifest.crate_name,
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
    for service in ["eave-dbus", "eave-hyprland", "eave-net", "eave-pulse"] {
        let dir = eave_dir.join("crates").join(service);
        manifest += &format!("{service} = {{ path = {} }}\n", quote(&dir));
    }
    // Same profile as eave's own release builds (D-10).
    manifest += "\n[profile.release]\nlto = \"thin\"\ncodegen-units = 1\nstrip = true\npanic = \"abort\"\n\n\
                 # Standalone: never part of an enclosing workspace.\n[workspace]\n";

    let registers: Vec<String> = plugins
        .iter()
        .map(|p| format!("{}::register", p.module))
        .collect();
    let main = format!(
        "// Generated by eave-build. Edit plugins.toml instead.\n\n\
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
    trimmed
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect()
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

fn cache_dir() -> Result<PathBuf> {
    xdg("XDG_CACHE_HOME", ".cache")
}

#[cfg(test)]
mod tests {
    use super::*;

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
    }
}

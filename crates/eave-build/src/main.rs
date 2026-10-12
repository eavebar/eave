//! Builds an eave binary with the plugins listed in `plugins.toml`, the way xcaddy builds Caddy.
//!
//! ```text
//! eave-build [--config <plugins.toml>] [--install]
//! ```
//!
//! Plugins are compiled in, never loaded at runtime ([D-12](../../docs/decisions.md)). For each
//! plugin this fetches its repository (or uses a local path), reads the `eave-plugin.toml`
//! manifest, checks it targets this eave's plugin API, then generates a small crate whose `main`
//! calls `eave::run` with every plugin's `register`, and builds it with cargo.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use serde::Deserialize;

const DEFAULT_EAVE_GIT: &str = "https://github.com/francescomengarelli/eave";

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
            "-h" | "--help" => {
                println!("usage: eave-build [--config <plugins.toml>] [--install]");
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
    } else {
        eprintln!("eave-build: built {}", binary.display());
    }
    Ok(())
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
        "\n[patch.crates-io]\neave-api = {{ path = {} }}\neave-hyprland = {{ path = {} }}\n",
        quote(&api_dir),
        quote(&eave_dir.join("crates/eave-hyprland"))
    );
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
    fn slugs_are_flat_directory_names() {
        assert_eq!(
            slug("https://github.com/someone/eave-weather.git"),
            "github.com-someone-eave-weather"
        );
    }
}

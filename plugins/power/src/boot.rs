//! Other systems to reboot into, and how to have the next boot start one (D-131).
//!
//! - **The boot loader's entries**, when it tells logind about them (systemd-boot, and any loader
//!   that follows the Boot Loader Interface): found on their own, and picked through logind with
//!   no setup.
//! - **macOS on Apple Silicon**: found when `eave-boot-macos.service` is installed, which has
//!   `asahi-bless` pick it in Apple's NVRAM for the next boot.
//! - **GRUB's entries, and the firmware's own (EFI) ones**: named in config.toml, and picked by
//!   `eave-boot-grub@.service` (`grub-reboot`) or `eave-boot-efi@.service` (`efibootmgr
//!   --bootnext`).
//!
//! The units are in `dist/power`, installed once as root with a polkit rule that lets the active
//! user start them and nothing else. The bar starts them over D-Bus and runs nothing itself.

use std::path::Path;

use serde::Deserialize;

/// The firmware's own variables, which anyone may read.
const EFIVARS: &str = "/sys/firmware/efi/efivars";
/// The UEFI spec's GUID for its global variables: `Boot####`, `BootCurrent`.
const GLOBAL: &str = "8be4df61-93ca-11d2-aa0d-00e098032b8c";
/// The Boot Loader Interface's GUID: `LoaderEntrySelected`.
const LOADER: &str = "4a67b082-0a4c-41cf-b6c7-440b29bb8c4f";

pub const MACOS_UNIT: &str = "eave-boot-macos.service";
/// What the unit runs, which only Apple Silicon machines have.
const ASAHI_BLESS: &str = "/usr/bin/asahi-bless";

/// A system to reboot into.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// What it's called: "macOS", "Windows".
    pub name: String,
    pub via: Via,
}

/// How the next boot is set to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Via {
    /// This boot loader entry, through logind.
    Entry(String),
    /// This system unit, which sets it as root.
    Unit(String),
}

/// One `boot` entry in config.toml: its name, and one of `loader`, `grub` or `efi`.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Configured {
    pub name: String,
    /// A boot loader entry's id, as logind lists them: `auto-windows`, `arch.conf`.
    #[serde(default)]
    pub loader: Option<String>,
    /// A GRUB menu entry's title or id, for `grub-reboot`.
    #[serde(default)]
    pub grub: Option<String>,
    /// A firmware boot entry's description, as `efibootmgr` shows it: `Windows Boot Manager`.
    #[serde(default)]
    pub efi: Option<String>,
}

/// What a configured target becomes, before anything is looked up.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Wanted {
    Entry(String),
    Grub(String),
    Efi(String),
}

impl Configured {
    fn wanted(&self) -> Result<Wanted, String> {
        match (&self.loader, &self.grub, &self.efi) {
            (Some(entry), None, None) => Ok(Wanted::Entry(entry.clone())),
            (None, Some(entry), None) => Ok(Wanted::Grub(entry.clone())),
            (None, None, Some(entry)) => Ok(Wanted::Efi(entry.clone())),
            _ => Err(format!(
                "boot target \"{}\" needs one of loader, grub or efi",
                self.name
            )),
        }
    }
}

/// Whether this is Apple Silicon with macOS to pick: `asahi-bless` is there.
pub fn asahi() -> bool {
    Path::new(ASAHI_BLESS).exists()
}

/// The boot loader entry this system was started from, when the loader says.
pub fn running_entry() -> Option<String> {
    read_string(&Path::new(EFIVARS).join(format!("LoaderEntrySelected-{LOADER}")))
}

/// Every target there is: the configured ones first, in their order, then the loader's entries
/// config.toml didn't name, but the `running` one, then macOS on `asahi`. `entries` is what
/// logind lists, and `unit_exists` says whether the system has a unit.
pub async fn targets<F, Fut>(
    configured: &[Configured],
    entries: &[String],
    running: Option<&str>,
    asahi: bool,
    unit_exists: F,
) -> Vec<Target>
where
    F: Fn(String) -> Fut,
    Fut: Future<Output = bool>,
{
    let mut targets = Vec::new();
    for configured in configured {
        let via = match configured.wanted() {
            Ok(Wanted::Entry(entry)) if entries.contains(&entry) => Via::Entry(entry),
            Ok(Wanted::Entry(entry)) => {
                eprintln!("eave-power: the boot loader has no entry {entry}");
                continue;
            }
            Ok(Wanted::Grub(entry)) => Via::Unit(instance("eave-boot-grub", &entry)),
            Ok(Wanted::Efi(description)) => match efi_entry(Path::new(EFIVARS), &description) {
                Some(number) => Via::Unit(instance("eave-boot-efi", &number)),
                None => {
                    eprintln!("eave-power: the firmware has no boot entry \"{description}\"");
                    continue;
                }
            },
            Err(error) => {
                eprintln!("eave-power: {error}");
                continue;
            }
        };
        if let Via::Unit(unit) = &via
            && !unit_exists(unit.clone()).await
        {
            eprintln!("eave-power: {unit} isn't installed; see dist/power");
            continue;
        }
        targets.push(Target {
            name: configured.name.clone(),
            via,
        });
    }
    for entry in entries {
        let named = targets
            .iter()
            .any(|target| target.via == Via::Entry(entry.clone()));
        if named || running == Some(entry.as_str()) {
            continue;
        }
        if let Some(name) = entry_name(entry) {
            targets.push(Target {
                name,
                via: Via::Entry(entry.clone()),
            });
        }
    }
    let has_macos = targets.iter().any(|target| target.name == "macOS");
    if asahi && !has_macos && unit_exists(MACOS_UNIT.into()).await {
        targets.push(Target {
            name: "macOS".into(),
            via: Via::Unit(MACOS_UNIT.into()),
        });
    }
    targets
}

/// What a boot loader entry is called in the menu, or `None` for one that isn't a system:
/// systemd-boot's own reboot, power off and firmware entries, and the EFI shell.
fn entry_name(entry: &str) -> Option<String> {
    let name = match entry {
        "auto-windows" => "Windows",
        "auto-osx" => "macOS",
        _ if entry.starts_with("auto-") => return None,
        _ => entry.strip_suffix(".conf").unwrap_or(entry),
    };
    Some(name.to_owned())
}

/// The unit `template@instance.service`, with `instance` escaped the way `systemd-escape` does,
/// so `%I` in the unit gives it back as it was.
fn instance(template: &str, instance: &str) -> String {
    let mut escaped = String::new();
    for (index, byte) in instance.bytes().enumerate() {
        match byte {
            b'/' => escaped.push('-'),
            b'.' if index == 0 => escaped.push_str("\\x2e"),
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b':' | b'_' | b'.' => {
                escaped.push(char::from(byte));
            }
            _ => escaped.push_str(&format!("\\x{byte:02x}")),
        }
    }
    format!("{template}@{escaped}.service")
}

/// A string variable, after the four bytes of its attributes: UTF-16, ending in a nul.
fn read_string(path: &Path) -> Option<String> {
    let data = std::fs::read(path).ok()?;
    utf16(data.get(4..)?)
}

/// UTF-16LE up to its first nul, or the end.
fn utf16(bytes: &[u8]) -> Option<String> {
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .take_while(|&unit| unit != 0)
        .collect();
    String::from_utf16(&units).ok()
}

/// The `####` of the firmware's boot entry described as `description`.
fn efi_entry(efivars: &Path, description: &str) -> Option<String> {
    let suffix = format!("-{GLOBAL}");
    let mut numbers: Vec<String> = std::fs::read_dir(efivars)
        .ok()?
        .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
        .filter_map(|name| {
            let number = name.strip_prefix("Boot")?.strip_suffix(&suffix)?;
            let hex = number.len() == 4 && number.bytes().all(|byte| byte.is_ascii_hexdigit());
            hex.then(|| number.to_owned())
        })
        .collect();
    numbers.sort();
    numbers.into_iter().find(|number| {
        let path = efivars.join(format!("Boot{number}{suffix}"));
        let data = std::fs::read(path).unwrap_or_default();
        load_option(&data).as_deref() == Some(description)
    })
}

/// The description in a `Boot####` variable: its attributes (4 bytes), then an EFI_LOAD_OPTION,
/// whose own attributes (4) and device path's length (2) come before it.
fn load_option(data: &[u8]) -> Option<String> {
    utf16(data.get(10..)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The start of this MacBook's `Boot0000`, as U-Boot wrote it.
    const BOOT0000: &[u8] = &[
        0x07, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x68, 0x00, b'n', 0x00, b'v', 0x00, b'm',
        0x00, b'e', 0x00, b' ', 0x00, b'0', 0x00, 0x00, 0x00, 0x01, 0x04, 0x1c, 0x00, 0xb9, 0x73,
    ];

    #[test]
    fn reads_a_boot_entrys_description() {
        assert_eq!(load_option(BOOT0000).as_deref(), Some("nvme 0"));
        assert_eq!(load_option(&BOOT0000[..6]), None);
    }

    #[test]
    fn finds_a_firmware_entry_by_its_description() {
        let dir = std::env::temp_dir().join(format!("eave-power-efi-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut windows = BOOT0000[..10].to_vec();
        for unit in "Windows Boot Manager".encode_utf16() {
            windows.extend(unit.to_le_bytes());
        }
        windows.extend([0, 0]);
        std::fs::write(dir.join(format!("Boot0000-{GLOBAL}")), BOOT0000).unwrap();
        std::fs::write(dir.join(format!("Boot001A-{GLOBAL}")), &windows).unwrap();
        std::fs::write(dir.join(format!("BootOrder-{GLOBAL}")), [7, 0, 0, 0, 0, 0]).unwrap();
        assert_eq!(
            efi_entry(&dir, "Windows Boot Manager").as_deref(),
            Some("001A")
        );
        assert_eq!(efi_entry(&dir, "Linux"), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn instances_are_escaped_like_systemd_escape() {
        assert_eq!(
            instance("eave-boot-efi", "0001"),
            "eave-boot-efi@0001.service"
        );
        assert_eq!(
            instance("eave-boot-grub", "Windows Boot Manager (on /dev/sda1)"),
            r"eave-boot-grub@Windows\x20Boot\x20Manager\x20\x28on\x20-dev-sda1\x29.service"
        );
        assert_eq!(instance("x", ".a-b"), r"x@\x2ea\x2db.service");
    }

    #[test]
    fn loader_entries_that_are_systems_get_names() {
        assert_eq!(entry_name("auto-windows").as_deref(), Some("Windows"));
        assert_eq!(entry_name("auto-osx").as_deref(), Some("macOS"));
        assert_eq!(entry_name("arch-lts.conf").as_deref(), Some("arch-lts"));
        assert_eq!(entry_name("auto-reboot-to-firmware-setup"), None);
        assert_eq!(entry_name("auto-efi-shell"), None);
    }

    #[test]
    fn a_target_needs_exactly_one_way() {
        let configured = |loader: Option<&str>, grub: Option<&str>| Configured {
            name: "Windows".into(),
            loader: loader.map(Into::into),
            grub: grub.map(Into::into),
            efi: None,
        };
        assert_eq!(
            configured(None, Some("Windows")).wanted(),
            Ok(Wanted::Grub("Windows".into()))
        );
        assert!(configured(None, None).wanted().is_err());
        assert!(
            configured(Some("auto-windows"), Some("x"))
                .wanted()
                .is_err()
        );
    }

    fn run<T>(future: impl Future<Output = T>) -> T {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(future)
    }

    #[test]
    fn configured_targets_come_first_then_the_loaders_then_macos() {
        let configured = [
            Configured {
                name: "Windows 11".into(),
                loader: Some("auto-windows".into()),
                grub: None,
                efi: None,
            },
            Configured {
                name: "Fedora".into(),
                loader: None,
                grub: Some("Fedora".into()),
                efi: None,
            },
        ];
        let entries = [
            "auto-windows".to_owned(),
            "arch.conf".to_owned(),
            "fedora.conf".to_owned(),
            "auto-reboot-to-firmware-setup".to_owned(),
        ];
        let found = run(targets(
            &configured,
            &entries,
            Some("arch.conf"),
            true,
            |_| async { true },
        ));
        let names: Vec<_> = found.iter().map(|target| target.name.as_str()).collect();
        assert_eq!(names, ["Windows 11", "Fedora", "fedora", "macOS"]);
        assert_eq!(
            found[1].via,
            Via::Unit("eave-boot-grub@Fedora.service".into())
        );

        // Without the units installed, only the loader's entries are left.
        let found = run(targets(&configured, &entries, None, true, |_| async {
            false
        }));
        let names: Vec<_> = found.iter().map(|target| target.name.as_str()).collect();
        assert_eq!(names, ["Windows 11", "arch", "fedora"]);

        // Not Apple Silicon: no macOS, even with the unit installed.
        let found = run(targets(&configured, &[], None, false, |_| async { true }));
        let names: Vec<_> = found.iter().map(|target| target.name.as_str()).collect();
        assert_eq!(names, ["Fedora"]);
    }
}

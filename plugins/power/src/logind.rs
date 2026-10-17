//! What logind does for the session: lock it, suspend, hibernate, reboot and power off, and pick
//! the boot loader's entry for the next boot. All on the system bus, as the active session's
//! user, which logind's polkit policy lets do these without asking.

use eave_dbus::zbus;

const SERVICE: &str = "org.freedesktop.login1";
const PATH: &str = "/org/freedesktop/login1";
const MANAGER: &str = "org.freedesktop.login1.Manager";
const SESSION: &str = "/org/freedesktop/login1/session/auto";

/// What logind says it would do, from its `Can*` answers.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Can {
    pub suspend: bool,
    pub hibernate: bool,
    pub reboot: bool,
    pub power_off: bool,
    /// The boot loader's entries the next boot can be set to, when it can.
    pub entries: Vec<String>,
}

pub async fn can() -> zbus::Result<Can> {
    let connection = eave_dbus::system().await?;
    let ask = |method: &'static str| {
        let connection = connection.clone();
        async move {
            let reply = connection
                .call_method(Some(SERVICE), PATH, Some(MANAGER), method, &())
                .await?;
            let answer: String = reply.body().deserialize()?;
            zbus::Result::Ok(allowed(&answer))
        }
    };
    let entries = match ask("CanRebootToBootLoaderEntry").await? {
        true => {
            let manager = eave_dbus::properties(&connection, SERVICE, PATH, MANAGER).await?;
            manager.get("BootLoaderEntries")
        }
        false => Vec::new(),
    };
    Ok(Can {
        suspend: ask("CanSuspend").await?,
        hibernate: ask("CanHibernate").await?,
        reboot: ask("CanReboot").await?,
        power_off: ask("CanPowerOff").await?,
        entries,
    })
}

/// `yes`, or `challenge` (polkit would ask): worth showing. `no` and `na` aren't.
fn allowed(answer: &str) -> bool {
    matches!(answer, "yes" | "challenge")
}

/// Asks whatever locks the session (hypridle's `lock_cmd`, for one) to lock it.
pub async fn lock() -> zbus::Result<()> {
    let connection = eave_dbus::system().await?;
    connection
        .call_method(
            Some(SERVICE),
            SESSION,
            Some("org.freedesktop.login1.Session"),
            "Lock",
            &(),
        )
        .await?;
    Ok(())
}

/// `Suspend`, `Hibernate`, `Reboot` or `PowerOff`, without polkit asking.
pub async fn manager(method: &str) -> zbus::Result<()> {
    let connection = eave_dbus::system().await?;
    connection
        .call_method(Some(SERVICE), PATH, Some(MANAGER), method, &(false,))
        .await?;
    Ok(())
}

/// Has the boot loader start `entry` next time, once.
pub async fn boot_into(entry: &str) -> zbus::Result<()> {
    let connection = eave_dbus::system().await?;
    connection
        .call_method(
            Some(SERVICE),
            PATH,
            Some(MANAGER),
            "SetRebootToBootLoaderEntry",
            &(entry,),
        )
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_yes_and_challenge_are_shown() {
        assert!(allowed("yes"));
        assert!(allowed("challenge"));
        assert!(!allowed("no"));
        assert!(!allowed("na"));
    }

    #[test]
    #[ignore = "needs logind"]
    fn asks_real_logind() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let can = runtime.block_on(can()).unwrap();
        assert!(can.reboot, "{can:?}");
    }
}

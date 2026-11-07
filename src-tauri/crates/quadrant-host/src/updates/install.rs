//! Putting a verified release in place: over the running app, or beside it
//! when switching to the other app. Each path leaves the installed app
//! running, or about to run, and expects the caller to quit.

use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use anyhow::{Context as _, Result, anyhow, ensure};

use super::{App, Bundle};
use crate::app_paths::Launch;

/// How the running copy was installed, or `None` when it shouldn't update
/// itself: a build run from `target/`, or a package manager that owns its
/// updates. `package` is its Linux package's name.
pub fn detect(package: &str) -> Option<Bundle> {
    let exe = env::current_exe().ok()?;
    if cfg!(windows) {
        // The NSIS setup puts its uninstaller beside the exe.
        exe.with_file_name("uninstall.exe")
            .exists()
            .then_some(Bundle::Nsis)
    } else if cfg!(target_os = "macos") {
        app_bundle(&exe).map(|_| Bundle::App)
    } else if env::var_os("APPIMAGE").is_some() {
        Some(Bundle::AppImage)
    } else if Path::new(&format!("/var/lib/dpkg/info/{package}.list")).exists() {
        Some(Bundle::Deb)
    } else {
        let rpm = Command::new("rpm")
            .args(["-q", "--quiet", package])
            .stderr(Stdio::null())
            .status();
        rpm.is_ok_and(|status| status.success())
            .then_some(Bundle::Rpm)
    }
}

/// Where each app's packages put its binary.
fn linux_binary(app: App) -> PathBuf {
    PathBuf::from(match app {
        App::Quadrant => "/usr/bin/quadrant_next",
        App::QuadrantNative => "/usr/bin/quadrant-native",
    })
}

/// Installs `bytes`, the verified artifact of `app` for `bundle`, and
/// starts it. The running app restarts with its arguments; the other app
/// starts without any. `release` runs just before the installed app can
/// start, to let go of the single instance socket it is going to claim.
pub(super) fn install(
    app: App,
    running: App,
    bundle: Bundle,
    bytes: &[u8],
    release: impl FnOnce(),
) -> Result<()> {
    let switching = app != running;
    let args: Vec<_> = if switching {
        Vec::new()
    } else {
        env::args_os().skip(1).collect()
    };
    match bundle {
        Bundle::Nsis => run_nsis(app, bytes, release),
        Bundle::AppImage => {
            let current = PathBuf::from(env::var_os("APPIMAGE").context("APPIMAGE isn't set")?);
            let target = if switching {
                beside(&current, &format!("{}.AppImage", app.name()))?
            } else {
                current
            };
            write_appimage(&target, bytes)?;
            release();
            launch(&target, &args)
        }
        Bundle::Deb | Bundle::Rpm => {
            let (extension, tool) = match bundle {
                Bundle::Deb => ("deb", ["dpkg", "-i"]),
                _ => ("rpm", ["rpm", "-U"]),
            };
            install_package(app, bytes, extension, &tool)?;
            let binary = if switching {
                linux_binary(app)
            } else {
                env::current_exe()?
            };
            release();
            launch(&binary, &args)
        }
        Bundle::App => {
            let current = app_bundle(&env::current_exe()?).context("not running from an .app")?;
            let target = if switching {
                beside(&current, &format!("{}.app", app.name()))?
            } else {
                current
            };
            replace_app(&target, bytes)?;
            release();
            // LaunchServices doesn't pass the environment on, so the new app
            // starts without the handoff marker; the release covers it.
            let status = Command::new("open").arg("-n").arg(&target).status()?;
            ensure!(status.success(), "couldn't start {} ({status})", app.name());
            Ok(())
        }
    }
}

/// `name` in the folder holding `path`.
fn beside(path: &Path, name: &str) -> Result<PathBuf> {
    Ok(path
        .parent()
        .with_context(|| format!("{} has no parent folder", path.display()))?
        .join(name))
}

/// Runs the setup passively (`/P`); `/R` starts the installed app when it
/// is done, in both apps' NSIS templates. A setup closes its own app if it
/// is still running.
fn run_nsis(app: App, bytes: &[u8], release: impl FnOnce()) -> Result<()> {
    let setup = env::temp_dir().join(format!(
        "{}-update-{}-setup.exe",
        app.slug(),
        std::process::id()
    ));
    fs::write(&setup, bytes).with_context(|| format!("couldn't write {}", setup.display()))?;
    // Start-Process goes through ShellExecute, so a setup that needs
    // elevation asks for it instead of failing.
    let quoted = setup.display().to_string().replace('\'', "''");
    release();
    let mut command = Command::new("powershell.exe");
    Launch::Handoff.mark(&mut command);
    command.args([
        "-NoProfile",
        "-NonInteractive",
        "-WindowStyle",
        "Hidden",
        "-Command",
        &format!("Start-Process -FilePath '{quoted}' -ArgumentList '/P','/R'"),
    ]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let status = command.status().context("couldn't start the setup")?;
    ensure!(status.success(), "couldn't start the setup ({status})");
    Ok(())
}

/// Writes an AppImage to `target` through a staged file, so a running one
/// is swapped rather than overwritten.
fn write_appimage(target: &Path, bytes: &[u8]) -> Result<()> {
    let staged = target.with_extension("update");
    fs::write(&staged, bytes).with_context(|| format!("couldn't write {}", staged.display()))?;
    make_executable(&staged)?;
    fs::rename(&staged, target).with_context(|| format!("couldn't write {}", target.display()))
}

/// Installs or upgrades a system package through polkit.
fn install_package(app: App, bytes: &[u8], extension: &str, tool: &[&str]) -> Result<()> {
    let dir = private_temp_dir(app)?;
    let package = dir.join(format!("{}.{extension}", app.slug()));
    fs::write(&package, bytes)?;
    let status = Command::new("pkexec").args(tool).arg(&package).status();
    let _ = fs::remove_dir_all(&dir);
    let status = status.context("couldn't run pkexec")?;
    ensure!(status.success(), "the package manager failed ({status})");
    Ok(())
}

/// Unpacks an `.app.tar.gz` beside `target` and puts its `.app` there,
/// keeping the previous one until the new one is in place.
fn replace_app(target: &Path, bytes: &[u8]) -> Result<()> {
    let parent = target.parent().context("the .app has no parent folder")?;
    let staging = parent.join(format!(".quadrant-update-{}", std::process::id()));
    let _ = fs::remove_dir_all(&staging);
    fs::create_dir_all(&staging)?;
    let result = (|| {
        let archive = staging.join("update.app.tar.gz");
        fs::write(&archive, bytes)?;
        let status = Command::new("tar")
            .arg("-xzf")
            .arg(&archive)
            .arg("-C")
            .arg(&staging)
            .status()?;
        ensure!(status.success(), "couldn't unpack the update ({status})");
        let unpacked = fs::read_dir(&staging)?
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .find(|path| path.extension().is_some_and(|ext| ext == "app"))
            .context("the update has no .app inside")?;
        let old = staging.join("previous.app");
        let had_one = target.exists();
        if had_one {
            fs::rename(target, &old)?;
        }
        if let Err(error) = fs::rename(&unpacked, target) {
            if had_one {
                fs::rename(&old, target)?;
            }
            return Err(error.into());
        }
        Ok(())
    })();
    let _ = fs::remove_dir_all(&staging);
    result
}

/// The `.app` folder holding `exe`, as in `Foo.app/Contents/MacOS/foo`.
fn app_bundle(exe: &Path) -> Option<PathBuf> {
    exe.ancestors()
        .find(|path| path.extension().is_some_and(|ext| ext == "app"))
        .map(Path::to_path_buf)
}

fn launch(program: &Path, args: &[std::ffi::OsString]) -> Result<()> {
    Launch::Handoff
        .mark(&mut Command::new(program))
        .args(args)
        .spawn()
        .map(drop)
        .map_err(|error| anyhow!("couldn't start {}: {error}", program.display()))
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
    Ok(())
}

#[cfg(not(unix))]
fn make_executable(_: &Path) -> Result<()> {
    Ok(())
}

/// A temp folder only this user can read, so nobody swaps the package
/// between writing it and installing it as root.
#[cfg(unix)]
fn private_temp_dir(app: App) -> Result<PathBuf> {
    use std::os::unix::fs::DirBuilderExt as _;
    let dir = env::temp_dir().join(format!("{}-update-{}", app.slug(), std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::DirBuilder::new().mode(0o700).create(&dir)?;
    Ok(dir)
}

#[cfg(not(unix))]
fn private_temp_dir(_: App) -> Result<PathBuf> {
    Ok(env::temp_dir())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_app_bundle_of_its_binary() {
        let exe = Path::new("/Applications/Quadrant-Native.app/Contents/MacOS/quadrant-native");
        assert_eq!(
            app_bundle(exe),
            Some(PathBuf::from("/Applications/Quadrant-Native.app"))
        );
        assert_eq!(app_bundle(Path::new("/usr/bin/quadrant-native")), None);
    }

    #[test]
    fn a_build_in_target_doesnt_update_itself() {
        assert_eq!(detect("quadrant-native"), None);
    }

    #[test]
    fn the_other_app_goes_beside_this_one_under_its_own_name() {
        let current = Path::new("/home/a/Apps/Quadrant.AppImage");
        assert_eq!(
            beside(current, "Quadrant-Native.AppImage").unwrap(),
            Path::new("/home/a/Apps/Quadrant-Native.AppImage")
        );
    }

    #[test]
    fn write_appimage_leaves_an_executable_file() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("Quadrant-Native.AppImage");
        fs::write(&target, b"old").unwrap();
        write_appimage(&target, b"new").unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"new");
        assert!(!target.with_extension("update").exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = fs::metadata(&target).unwrap().permissions().mode();
            assert_eq!(mode & 0o111, 0o111);
        }
    }

    #[test]
    fn packages_put_each_app_at_its_own_binary() {
        assert_eq!(
            linux_binary(App::Quadrant),
            Path::new("/usr/bin/quadrant_next")
        );
        assert_eq!(
            linux_binary(App::QuadrantNative),
            Path::new("/usr/bin/quadrant-native")
        );
    }
}

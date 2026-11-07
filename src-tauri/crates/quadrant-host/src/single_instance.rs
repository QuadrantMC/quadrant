//! One running app per data folder, across Quadrant and Quadrant-Native: a
//! second launch of either hands its links to the running one over a local
//! socket (a named pipe on Windows) and exits, and the running one brings its
//! window back and opens them.
//!
//! The socket's name comes from the user's home and the data folder, so a
//! sandbox's data folder runs beside the real app, and another sandbox,
//! rather than handing its links to them. Both apps must name it the same
//! way, so it is only made here.

use std::{
    io::{BufRead as _, BufReader, Write as _},
    path::Path,
    thread,
    time::Duration,
};

use anyhow::{Context as _, Result, bail};
use interprocess::local_socket::{
    GenericNamespaced, Listener, ListenerOptions, Name, Stream, ToNsName as _, prelude::*,
};

/// How a launch went.
pub enum Claim {
    /// No other instance runs for this data folder; this one listens for
    /// later launches.
    Primary(Primary),
    /// The running instance has the links; this one should exit.
    Forwarded,
}

/// The listening end, until [`Primary::serve`] hands it to its thread.
pub struct Primary(Listener);

/// A stable name for this user and the data folder at `data_dir`.
pub fn key(data_dir: &Path) -> String {
    key_for(dirs::home_dir().as_deref(), data_dir)
}

/// `DefaultHasher` may change between Rust releases, and an updated app has
/// to find the one it replaces, so this is FNV-1a.
fn key_for(home: Option<&Path>, data_dir: &Path) -> String {
    let mut text = format!(
        "{}\0{}",
        home.map(Path::display)
            .map(|home| home.to_string())
            .unwrap_or_default(),
        data_dir.display()
    );
    // Windows paths compare without case.
    if cfg!(windows) {
        text = text.to_lowercase();
    }
    let hash = text.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    });
    format!("{hash:016x}")
}

/// Hands `urls` to the instance running for the data folder at `data_dir`,
/// or becomes it.
pub fn claim(data_dir: &Path, urls: &[String]) -> Result<Claim> {
    claim_key(&key(data_dir), urls)
}

fn claim_key(key: &str, urls: &[String]) -> Result<Claim> {
    let name = format!("quadrant-{key}.sock");
    // Two launches at once both find no listener; the one that loses the
    // race to create it hands over to the winner.
    for _ in 0..20 {
        let name = name.as_str().to_ns_name::<GenericNamespaced>()?;
        match forward(name.clone(), urls) {
            Ok(()) => return Ok(Claim::Forwarded),
            Err(error) if no_listener(&error) => {}
            // Busy or mid-start: ask again.
            Err(_) => {
                thread::sleep(Duration::from_millis(50));
                continue;
            }
        }
        // A socket file left by a crashed instance answers nobody; only
        // Unix keeps one, and it's replaced here.
        match ListenerOptions::new()
            .name(name)
            .try_overwrite(true)
            .create_sync()
        {
            Ok(listener) => return Ok(Claim::Primary(Primary(listener))),
            Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => {}
            Err(error) => return Err(error).context("couldn't listen for later launches"),
        }
        thread::sleep(Duration::from_millis(50));
    }
    bail!("another instance holds {name} but doesn't answer")
}

fn no_listener(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused
    )
}

/// Sends one launch: its links as a JSON array on a line, answered by an
/// empty line once the running instance has them.
fn forward(name: Name<'_>, urls: &[String]) -> std::io::Result<()> {
    let mut stream = BufReader::new(Stream::connect(name)?);
    allow_foreground();
    let mut message = serde_json::to_string(urls)?;
    message.push('\n');
    stream.get_mut().write_all(message.as_bytes())?;
    let mut answer = String::new();
    stream.read_line(&mut answer)?;
    if answer != "\n" {
        return Err(std::io::Error::other("the running instance hung up"));
    }
    Ok(())
}

/// Lets the running instance take the foreground, which Windows otherwise
/// keeps for the process the user just started: this one.
fn allow_foreground() {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::UI::WindowsAndMessaging::{ASFW_ANY, AllowSetForegroundWindow};
        AllowSetForegroundWindow(ASFW_ANY);
    }
}

impl Primary {
    /// Hands each later launch's links to `deliver`, on a thread of its own.
    pub fn serve(self, deliver: impl Fn(Vec<String>) + Send + 'static) {
        let listener = self.0;
        let spawned = thread::Builder::new()
            .name("single-instance".into())
            .spawn(move || {
                for connection in listener.incoming() {
                    match connection.and_then(receive) {
                        Ok(urls) => deliver(urls),
                        Err(error) => log::warn!("A later launch couldn't hand over: {error}"),
                    }
                }
            });
        if let Err(error) = spawned {
            log::error!("Later launches won't reach this one: {error}");
        }
    }
}

fn receive(stream: Stream) -> std::io::Result<Vec<String>> {
    let mut stream = BufReader::new(stream);
    let mut line = String::new();
    stream.read_line(&mut line)?;
    let urls = serde_json::from_str(&line)?;
    stream.get_mut().write_all(b"\n")?;
    Ok(urls)
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use super::*;

    #[test]
    fn keys_tell_data_folders_and_users_apart() {
        let home = Some(Path::new("/home/a"));
        let real = key_for(home, Path::new("/home/a/.local/share/app"));
        assert_eq!(real, key_for(home, Path::new("/home/a/.local/share/app")));
        assert_ne!(real, key_for(home, Path::new("/tmp/sandbox/data")));
        assert_ne!(
            real,
            key_for(
                Some(Path::new("/home/b")),
                Path::new("/home/a/.local/share/app")
            )
        );
        assert_eq!(real.len(), 16);
    }

    #[test]
    fn a_second_launch_hands_its_links_to_the_first() {
        let key = format!("test-{}", std::process::id());
        let Claim::Primary(primary) = claim_key(&key, &[]).unwrap() else {
            panic!("nothing else listens under a fresh key");
        };
        let (sent, received) = mpsc::channel();
        primary.serve(move |urls| sent.send(urls).unwrap());
        let urls = vec!["modrinth://mod/sodium".to_string()];
        assert!(matches!(claim_key(&key, &urls).unwrap(), Claim::Forwarded));
        assert_eq!(received.recv_timeout(Duration::from_secs(5)).unwrap(), urls);
        assert!(matches!(claim_key(&key, &[]).unwrap(), Claim::Forwarded));
        assert_eq!(
            received.recv_timeout(Duration::from_secs(5)).unwrap(),
            Vec::<String>::new()
        );
        let Claim::Primary(_) = claim_key(&format!("{key}-other"), &[]).unwrap() else {
            panic!("another key is another instance");
        };
    }
}

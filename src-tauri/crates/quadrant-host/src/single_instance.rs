//! One running app per data folder, across Quadrant and Quadrant-Native: a
//! second launch of either hands its links to the running one over a local
//! socket (a named pipe on Windows) and exits, and the running one brings its
//! window back and opens them.
//!
//! The socket's name comes from the user's home and the data folder, so a
//! sandbox's data folder runs beside the real app, and another sandbox,
//! rather than handing its links to them. Both apps must name it the same
//! way, so it is only made here.
//!
//! An app that starts its successor (an update, a switch to the other app)
//! releases the socket first with [`Serving::release`], and starts it with
//! [`crate::app_paths::Launch::Handoff`], whose claim waits for the socket
//! instead of handing its links back to the app that is quitting.

use std::{
    io::{BufRead as _, BufReader, Write as _},
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

use anyhow::{Context as _, Result, bail};
use interprocess::local_socket::{
    GenericNamespaced, Listener, ListenerOptions, Name, Stream, ToNsName as _, prelude::*,
};

const RETRY: Duration = Duration::from_millis(50);
/// How long a handoff waits for its predecessor to let go.
const HANDOFF_WAIT: Duration = Duration::from_secs(10);

/// How a launch went.
pub enum Claim {
    /// No other instance runs for this data folder; this one listens for
    /// later launches.
    Primary(Primary),
    /// The running instance has the links; this one should exit.
    Forwarded,
}

/// The listening end, until [`Primary::serve`] hands it to its thread.
pub struct Primary {
    listener: Listener,
    name: String,
}

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
/// or becomes it. A `handoff` launch was started by the running instance,
/// which is quitting: it waits to take the socket over instead.
pub fn claim(data_dir: &Path, urls: &[String], handoff: bool) -> Result<Claim> {
    claim_key(&key(data_dir), urls, handoff)
}

fn claim_key(key: &str, urls: &[String], handoff: bool) -> Result<Claim> {
    let name = format!("quadrant-{key}.sock");
    let attempts = if handoff {
        (HANDOFF_WAIT.as_millis() / RETRY.as_millis()) as usize
    } else {
        20
    };
    // Two launches at once both find no listener; the one that loses the
    // race to create it hands over to the winner.
    for _ in 0..attempts {
        let ns_name = name.as_str().to_ns_name::<GenericNamespaced>()?;
        if !handoff {
            match forward(ns_name.clone(), urls) {
                Ok(()) => return Ok(Claim::Forwarded),
                Err(error) if no_listener(&error) => {}
                // Busy or mid-start: ask again.
                Err(_) => {
                    thread::sleep(RETRY);
                    continue;
                }
            }
        }
        // A socket file left by a crashed instance answers nobody; only
        // Unix keeps one, and it's replaced here.
        match ListenerOptions::new()
            .name(ns_name)
            .try_overwrite(true)
            .create_sync()
        {
            Ok(listener) => return Ok(Claim::Primary(Primary { listener, name })),
            Err(error) if in_use(&error) => {}
            Err(error) => return Err(error).context("couldn't listen for later launches"),
        }
        thread::sleep(RETRY);
    }
    bail!("another instance holds {name} but doesn't answer")
}

/// Another instance has the name. Windows refuses a second pipe server's
/// first instance with access denied.
fn in_use(error: &std::io::Error) -> bool {
    error.kind() == std::io::ErrorKind::AddrInUse
        || (cfg!(windows) && error.kind() == std::io::ErrorKind::PermissionDenied)
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

/// A primary serving later launches, until [`Serving::release`].
#[derive(Clone)]
pub struct Serving(Arc<ServingInner>);

struct ServingInner {
    name: String,
    released: AtomicBool,
    /// Signalled once the serving thread has dropped the listener.
    closed: Mutex<Option<mpsc::Receiver<()>>>,
}

impl Primary {
    /// Hands each later launch's links to `deliver`, on a thread of its own.
    pub fn serve(self, deliver: impl Fn(Vec<String>) + Send + 'static) -> Serving {
        let (closed_tx, closed_rx) = mpsc::channel();
        let serving = Serving(Arc::new(ServingInner {
            name: self.name,
            released: AtomicBool::new(false),
            closed: Mutex::new(Some(closed_rx)),
        }));
        let listener = self.listener;
        let state = serving.clone();
        let spawned = thread::Builder::new()
            .name("single-instance".into())
            .spawn(move || {
                for connection in listener.incoming() {
                    if state.0.released.load(Ordering::SeqCst) {
                        break;
                    }
                    match connection.and_then(receive) {
                        Ok(urls) => deliver(urls),
                        Err(error) => log::warn!("A later launch couldn't hand over: {error}"),
                    }
                }
                drop(listener);
                let _ = closed_tx.send(());
            });
        if let Err(error) = spawned {
            log::error!("Later launches won't reach this one: {error}");
        }
        serving
    }
}

impl Serving {
    /// Stops listening, so the app this one starts next can take the socket
    /// over. Later launches start instances of their own until it does.
    pub fn release(&self) {
        if self.0.released.swap(true, Ordering::SeqCst) {
            return;
        }
        let Some(closed) = self
            .0
            .closed
            .lock()
            .ok()
            .and_then(|mut closed| closed.take())
        else {
            return;
        };
        // Wakes the serving thread, which sees the flag and lets go. A wake
        // that arrives before the thread accepts is retried.
        for _ in 0..20 {
            if let Ok(name) = self.0.name.as_str().to_ns_name::<GenericNamespaced>() {
                let _ = Stream::connect(name);
            }
            match closed.recv_timeout(Duration::from_millis(100)) {
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                _ => return,
            }
        }
        log::warn!("The single-instance socket didn't close in time");
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
        let Claim::Primary(primary) = claim_key(&key, &[], false).unwrap() else {
            panic!("nothing else listens under a fresh key");
        };
        let (sent, received) = mpsc::channel();
        let _serving = primary.serve(move |urls| sent.send(urls).unwrap());
        let urls = vec!["modrinth://mod/sodium".to_string()];
        assert!(matches!(
            claim_key(&key, &urls, false).unwrap(),
            Claim::Forwarded
        ));
        assert_eq!(received.recv_timeout(Duration::from_secs(5)).unwrap(), urls);
        assert!(matches!(
            claim_key(&key, &[], false).unwrap(),
            Claim::Forwarded
        ));
        assert_eq!(
            received.recv_timeout(Duration::from_secs(5)).unwrap(),
            Vec::<String>::new()
        );
        let Claim::Primary(_) = claim_key(&format!("{key}-other"), &[], false).unwrap() else {
            panic!("another key is another instance");
        };
    }

    /// An app that starts its successor while it still holds the socket:
    /// the successor must not hand its launch back to the app that is
    /// quitting, or neither stays open.
    #[test]
    fn a_handoff_takes_over_once_the_predecessor_releases() {
        let key = format!("test-handoff-{}", std::process::id());
        let Claim::Primary(primary) = claim_key(&key, &[], false).unwrap() else {
            panic!("nothing else listens under a fresh key");
        };
        let (sent, old_received) = mpsc::channel();
        let old = primary.serve(move |urls| sent.send(urls).unwrap());

        let successor = {
            let key = key.clone();
            thread::spawn(move || claim_key(&key, &["x://y".into()], true))
        };
        thread::sleep(Duration::from_millis(300));
        old.release();
        let Claim::Primary(new) = successor.join().unwrap().unwrap() else {
            panic!("the successor handed its launch to the quitting app");
        };
        assert!(
            old_received.try_recv().is_err(),
            "the old instance got the successor's links"
        );

        let (sent, new_received) = mpsc::channel();
        let _new = new.serve(move |urls| sent.send(urls).unwrap());
        let urls = vec!["modrinth://mod/iris".to_string()];
        assert!(matches!(
            claim_key(&key, &urls, false).unwrap(),
            Claim::Forwarded
        ));
        assert_eq!(
            new_received.recv_timeout(Duration::from_secs(5)).unwrap(),
            urls
        );
    }

    #[test]
    fn a_released_primary_lets_a_plain_launch_take_over() {
        let key = format!("test-release-{}", std::process::id());
        let Claim::Primary(primary) = claim_key(&key, &[], false).unwrap() else {
            panic!("nothing else listens under a fresh key");
        };
        let serving = primary.serve(|_| {});
        serving.release();
        serving.release();
        assert!(matches!(
            claim_key(&key, &[], false).unwrap(),
            Claim::Primary(_)
        ));
    }
}

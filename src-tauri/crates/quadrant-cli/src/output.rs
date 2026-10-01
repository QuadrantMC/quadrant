use std::{
    fmt::Display,
    io::{IsTerminal, Write},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use anyhow::{Error, Result, bail};
use quadrant_host::{HostEventEnvelope, QuadrantHost};
use serde::Serialize;
use serde_json::Value;
use tokio::{sync::broadcast::error::RecvError, task::JoinHandle};

/// What a command produced: the value `--json` prints and the text a person
/// reads otherwise, plus the failure a command that partly succeeded still
/// exits with once that is printed.
pub struct Report {
    /// `None` for a command that printed its results as they came.
    json: Option<Value>,
    human: String,
    failure: Option<Error>,
}

impl Report {
    pub fn new<T: Serialize>(value: &T, human: impl FnOnce(&T) -> String) -> Result<Self> {
        Ok(Self {
            json: Some(serde_json::to_value(value)?),
            human: human(value),
            failure: None,
        })
    }

    /// A command with no result value, which prints `null` under `--json`.
    pub fn message(text: impl Into<String>) -> Self {
        Self {
            json: Some(Value::Null),
            human: text.into(),
            failure: None,
        }
    }

    /// A command that streamed its output, so nothing is left to print, not
    /// even the `null` that stands for "no value" under `--json`.
    pub fn streamed() -> Self {
        Self {
            json: None,
            human: String::new(),
            failure: None,
        }
    }

    /// Prints the result, then fails the command with `failure` if there is one.
    pub fn failing_with(self, failure: Option<Error>) -> Self {
        Self { failure, ..self }
    }

    /// The text stdout gets, if any.
    fn rendered(&self, json: bool) -> Result<Option<String>> {
        Ok(if json {
            self.json
                .as_ref()
                .map(serde_json::to_string_pretty)
                .transpose()?
        } else {
            Some(self.human.trim_end().to_string()).filter(|text| !text.trim().is_empty())
        })
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Output {
    pub json: bool,
    pub quiet: bool,
}

impl Output {
    pub fn print(&self, report: Report) -> Result<()> {
        if let Some(text) = report.rendered(self.json)? {
            writeln!(std::io::stdout().lock(), "{text}")?;
        }
        match report.failure {
            Some(failure) => Err(failure),
            None => Ok(()),
        }
    }

    /// A side remark on stderr, so it never mixes into `--json` output.
    pub fn note(&self, text: impl Display) {
        if !self.quiet {
            eprintln!("{text}");
        }
    }

    /// Something that failed while the command carried on. Unlike a note it
    /// shows under `--quiet`, since it is an error, not chatter.
    pub fn warn(&self, text: impl Display) {
        eprintln!("{text}");
    }

    /// Renders the host's progress events on stderr until the returned guard
    /// is dropped.
    pub fn track_progress(&self, host: &QuadrantHost) -> Progress {
        let mut receiver = host.subscribe_events();
        let drew = Arc::new(AtomicBool::new(false));
        let interactive = std::io::stderr().is_terminal();
        let task = (!self.quiet).then(|| {
            let drew = drew.clone();
            tokio::spawn(async move {
                // Exports and modpack installs report completion twice: for
                // the last file and once more when they finish.
                let mut last_done = None;
                loop {
                    match receiver.recv().await {
                        Ok(event) => {
                            let Some((line, done)) = progress_line(&event) else {
                                continue;
                            };
                            if interactive {
                                eprint!("\r{line}\x1b[K");
                                drew.store(true, Ordering::Relaxed);
                            } else if done && last_done.as_ref() != Some(&line) {
                                eprintln!("{line}");
                                last_done = Some(line);
                            }
                        }
                        Err(RecvError::Lagged(_)) => {}
                        Err(RecvError::Closed) => break,
                    }
                }
            })
        });
        Progress { task, drew }
    }
}

pub struct Progress {
    task: Option<JoinHandle<()>>,
    drew: Arc<AtomicBool>,
}

impl Drop for Progress {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
        if self.drew.load(Ordering::Relaxed) {
            eprintln!();
        }
    }
}

/// The line a progress event is shown as, and whether it reports completion.
fn progress_line(event: &HostEventEnvelope) -> Option<(String, bool)> {
    let (label, percent) = match event.event.as_str() {
        "modDownloadProgress" | "modInstallProgress" => {
            let verb = if event.event == "modDownloadProgress" {
                "Downloading"
            } else {
                "Installing"
            };
            let id = event.payload.get("modId")?.as_str()?;
            (
                format!("{verb} {id}"),
                event.payload.get("progress")?.as_f64()?,
            )
        }
        // Modpack progress arrives as a fraction of 1, mod progress in percent.
        "modpackDownloadProgress" => (
            "Downloading modpack".to_string(),
            event.payload.as_f64()? * 100.0,
        ),
        "quadrantExportProgress" => (
            "Exporting modpack".to_string(),
            event.payload.as_f64()? * 100.0,
        ),
        _ => return None,
    };
    Some((format!("{label}: {percent:.0}%"), percent >= 100.0))
}

/// Cloud records store sync dates in seconds and local ones in milliseconds;
/// the cutoff `syncDates.ts` uses tells them apart.
pub fn sync_date(timestamp: i64) -> String {
    let millis = if timestamp <= 170_406_720_000 {
        timestamp.saturating_mul(1000)
    } else {
        timestamp
    };
    chrono::DateTime::from_timestamp_millis(millis)
        .map(|date| date.format("%Y-%m-%d %H:%M UTC").to_string())
        .unwrap_or_default()
}

/// Asks before something that cannot be undone. `--yes` answers for the user,
/// and without a terminal to ask on the answer is no.
pub fn confirm(question: &str, yes: bool) -> Result<()> {
    if yes {
        return Ok(());
    }
    if !std::io::stdin().is_terminal() {
        bail!("{question} Pass --yes to confirm.");
    }
    eprint!("{question} [y/N] ");
    let mut answer = String::new();
    std::io::stdin().read_line(&mut answer)?;
    match answer.trim().to_lowercase().as_str() {
        "y" | "yes" => Ok(()),
        _ => bail!("Cancelled."),
    }
}

/// Pads rows into left-aligned columns separated by two spaces.
pub fn table<const N: usize>(rows: &[[String; N]]) -> String {
    let mut widths = [0usize; N];
    for row in rows {
        for (width, cell) in widths.iter_mut().zip(row) {
            *width = (*width).max(cell.chars().count());
        }
    }
    rows.iter()
        .map(|row| {
            let cells: Vec<String> = row
                .iter()
                .zip(widths)
                .enumerate()
                .map(|(index, (cell, width))| {
                    if index + 1 == N {
                        cell.clone()
                    } else {
                        format!("{cell:<width$}")
                    }
                })
                .collect();
            cells.join("  ").trim_end().to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn event(name: &str, payload: Value) -> HostEventEnvelope {
        HostEventEnvelope {
            event: name.to_string(),
            payload,
        }
    }

    #[test]
    fn progress_lines_name_the_mod_and_flag_completion() {
        assert_eq!(
            progress_line(&event(
                "modDownloadProgress",
                json!({"modId": "sodium", "progress": 40})
            )),
            Some(("Downloading sodium: 40%".to_string(), false))
        );
        assert_eq!(
            progress_line(&event("quadrantExportProgress", json!(0.5))),
            Some(("Exporting modpack: 50%".to_string(), false))
        );
        assert_eq!(
            progress_line(&event("quadrantExportProgress", json!(1.0))),
            Some(("Exporting modpack: 100%".to_string(), true))
        );
        assert_eq!(
            progress_line(&event("modpackDownloadProgress", json!(1))),
            Some(("Downloading modpack: 100%".to_string(), true))
        );
        assert_eq!(
            progress_line(&event("refreshNotifications", json!([]))),
            None
        );
    }

    #[test]
    fn json_prints_null_for_a_message_and_nothing_after_a_stream() {
        let message = Report::message("Done.");
        assert_eq!(message.rendered(true).unwrap().as_deref(), Some("null"));
        assert_eq!(message.rendered(false).unwrap().as_deref(), Some("Done."));
        assert_eq!(Report::streamed().rendered(true).unwrap(), None);
        assert_eq!(Report::streamed().rendered(false).unwrap(), None);
    }

    #[test]
    fn a_partly_failed_report_prints_then_fails() {
        let out = Output {
            json: false,
            quiet: true,
        };
        let report =
            Report::message("").failing_with(Some(anyhow::anyhow!("couldn't check 1 mod")));
        assert_eq!(
            out.print(report).unwrap_err().to_string(),
            "couldn't check 1 mod"
        );
        assert!(out.print(Report::message("")).is_ok());
    }

    #[test]
    fn sync_dates_accept_seconds_and_milliseconds() {
        assert_eq!(sync_date(1_700_000_000), "2023-11-14 22:13 UTC");
        assert_eq!(sync_date(1_700_000_000_000), "2023-11-14 22:13 UTC");
    }

    #[test]
    fn table_aligns_every_column_but_the_last() {
        let rows = [
            ["a".to_string(), "long".to_string(), "x".to_string()],
            ["bbb".to_string(), "s".to_string(), "".to_string()],
        ];
        assert_eq!(table(&rows), "a    long  x\nbbb  s");
    }
}

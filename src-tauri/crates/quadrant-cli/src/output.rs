use std::{
    fmt::Display,
    io::{IsTerminal, Write},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use anyhow::Result;
use quadrant_host::{HostEventEnvelope, QuadrantHost};
use serde::Serialize;
use serde_json::Value;
use tokio::{sync::broadcast::error::RecvError, task::JoinHandle};

/// What a command produced: the value `--json` prints and the text a person
/// reads otherwise.
pub struct Report {
    json: Value,
    human: String,
}

impl Report {
    pub fn new<T: Serialize>(value: &T, human: impl FnOnce(&T) -> String) -> Result<Self> {
        Ok(Self {
            json: serde_json::to_value(value)?,
            human: human(value),
        })
    }

    /// A command with no result value, which prints `null` under `--json`.
    pub fn message(text: impl Into<String>) -> Self {
        Self {
            json: Value::Null,
            human: text.into(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Output {
    pub json: bool,
    pub quiet: bool,
}

impl Output {
    pub fn print(&self, report: Report) -> Result<()> {
        let mut stdout = std::io::stdout().lock();
        if self.json {
            writeln!(stdout, "{}", serde_json::to_string_pretty(&report.json)?)?;
        } else if !report.human.trim().is_empty() {
            writeln!(stdout, "{}", report.human.trim_end())?;
        }
        Ok(())
    }

    /// A side remark on stderr, so it never mixes into `--json` output.
    pub fn note(&self, text: impl Display) {
        if !self.quiet {
            eprintln!("{text}");
        }
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
                loop {
                    match receiver.recv().await {
                        Ok(event) => {
                            let Some((line, done)) = progress_line(&event) else {
                                continue;
                            };
                            if interactive {
                                eprint!("\r{line}\x1b[K");
                                drew.store(true, Ordering::Relaxed);
                            } else if done {
                                eprintln!("{line}");
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
        "modpackDownloadProgress" => ("Downloading modpack".to_string(), event.payload.as_f64()?),
        "quadrantExportProgress" => ("Exporting modpack".to_string(), event.payload.as_f64()?),
        _ => return None,
    };
    Some((format!("{label}: {percent:.0}%"), percent >= 100.0))
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
            progress_line(&event("quadrantExportProgress", json!(100.0))),
            Some(("Exporting modpack: 100%".to_string(), true))
        );
        assert_eq!(
            progress_line(&event("refreshNotifications", json!([]))),
            None
        );
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

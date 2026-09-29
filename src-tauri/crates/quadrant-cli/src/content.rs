use anyhow::{Result, anyhow, bail};
use clap::Subcommand;
use quadrant_core::{
    content::{ContentLocation, ContentLocationKind},
    mc_mod::ModType,
};

use crate::{
    Ctx, args,
    output::{Report, confirm, table},
};

#[derive(Debug, Subcommand)]
pub enum ContentCommand {
    /// List installed resource packs and shader packs in every location.
    List {
        /// Only list the locations.
        #[arg(long)]
        no_files: bool,
    },
    /// Copy packs from one location to another, skipping ones already there.
    Copy {
        /// Location id, as `quadrantmc content list` shows it.
        #[arg(long)]
        from: String,
        #[arg(long)]
        to: String,
        /// resourcepack or shaderpack
        #[arg(long = "type", value_parser = args::pack_type)]
        mod_type: ModType,
        #[arg(required_unless_present = "all")]
        files: Vec<String>,
        /// Copy every pack of that type.
        #[arg(long, conflicts_with = "files")]
        all: bool,
    },
    /// Delete packs from a location.
    Delete {
        #[arg(long)]
        location: String,
        /// resourcepack or shaderpack
        #[arg(long = "type", value_parser = args::pack_type)]
        mod_type: ModType,
        #[arg(required = true)]
        files: Vec<String>,
        /// Don't ask for confirmation.
        #[arg(long, short)]
        yes: bool,
    },
    /// Print the folder a location keeps a pack type in.
    Folder {
        #[arg(long)]
        location: String,
        /// resourcepack or shaderpack
        #[arg(long = "type", value_parser = args::pack_type)]
        mod_type: ModType,
        /// Open it in the file manager.
        #[arg(long)]
        open: bool,
    },
}

pub async fn run(command: ContentCommand, ctx: &Ctx) -> Result<Report> {
    let host = &ctx.host;
    match command {
        ContentCommand::List { no_files } => {
            let locations = host.get_installed_content(!no_files)?;
            Report::new(&locations, |locations| {
                locations
                    .iter()
                    .map(|location| describe_location(location, !no_files))
                    .collect::<Vec<_>>()
                    .join("\n\n")
            })
        }
        ContentCommand::Copy {
            from,
            to,
            mod_type,
            files,
            all,
        } => {
            let locations = host.get_installed_content(true)?;
            let source_files = file_names(&locations, &from, mod_type)?;
            let already_there = file_names(&locations, &to, mod_type)?;
            let wanted = if all { source_files } else { files };
            let to_copy: Vec<String> = wanted
                .into_iter()
                .filter(|file| !already_there.contains(file))
                .collect();
            if to_copy.is_empty() {
                return Ok(Report::message("Nothing to copy."));
            }
            let copied = host.copy_content(&from, &to, mod_type, to_copy)?;
            Report::new(&copied, |copied| {
                format!("Copied {} to {to}.", packs(*copied))
            })
        }
        ContentCommand::Delete {
            location,
            mod_type,
            files,
            yes,
        } => {
            confirm(
                &format!("Permanently delete {} from {location}?", files.join(", ")),
                yes,
            )?;
            let deleted = host.delete_content(&location, mod_type, files)?;
            Report::new(&deleted, |deleted| format!("Deleted {}.", packs(*deleted)))
        }
        ContentCommand::Folder {
            location,
            mod_type,
            open,
        } => {
            let folder = host.content_folder(&location, mod_type)?;
            if open {
                std::fs::create_dir_all(&folder)?;
                open::that_detached(&folder)?;
            }
            Report::new(&folder, |folder| folder.display().to_string())
        }
    }
}

fn file_names(locations: &[ContentLocation], id: &str, mod_type: ModType) -> Result<Vec<String>> {
    let location = locations
        .iter()
        .find(|location| location.id == id)
        .ok_or_else(|| {
            anyhow!("no content location {id:?}; `quadrantmc content list` shows them")
        })?;
    let Some(section) = location
        .sections
        .iter()
        .find(|section| section.mod_type == mod_type)
    else {
        bail!("{id} has no {mod_type} folder");
    };
    Ok(section
        .files
        .iter()
        .map(|file| file.file_name.clone())
        .collect())
}

fn describe_location(location: &ContentLocation, with_files: bool) -> String {
    let title = match location.kind {
        ContentLocationKind::Minecraft => "Minecraft".to_string(),
        ContentLocationKind::Prism => format!("Prism: {}", location.name),
    };
    let mut text = format!("{title} [{}]\n{}", location.id, location.path);
    if !with_files {
        return text;
    }
    for section in &location.sections {
        let heading = match section.mod_type {
            ModType::ShaderPack => "Shader packs",
            _ => "Resource packs",
        };
        text.push_str(&format!("\n  {heading} ({})", section.files.len()));
        let rows: Vec<[String; 2]> = section
            .files
            .iter()
            .map(|file| {
                let size = if file.is_directory {
                    "folder".to_string()
                } else {
                    format_size(file.size)
                };
                [format!("    {}", file.file_name), size]
            })
            .collect();
        if !rows.is_empty() {
            text.push('\n');
            text.push_str(&table(&rows));
        }
    }
    text
}

fn packs(count: usize) -> String {
    if count == 1 {
        "1 pack".to_string()
    } else {
        format!("{count} packs")
    }
}

fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit + 1 < UNITS.len() {
        size /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{size:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_use_binary_units() {
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1536), "1.5 KB");
        assert_eq!(format_size(5 * 1024 * 1024), "5.0 MB");
    }
}

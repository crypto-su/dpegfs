use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use dpegfs::{Compression, Disk, dimensions_for_size};

#[derive(Parser)]
#[command(
    version,
    about = "A virtual filesystem inside the RGB pixels of a lossless PNG"
)]
#[command(
    after_help = "Start with: dpegfs create disk.png --size 16MiB\nThen:       dpegfs put disk.png report.txt --name docs/report.txt"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create a new image (default: 1024 x 1024 RGB pixels). Never overwrites.
    Create {
        image: PathBuf,
        /// Target raw pixel capacity, including metadata; e.g. 16MiB or 65536B.
        #[arg(long, value_parser = parse_size, conflicts_with_all = ["width", "height"])]
        size: Option<u64>,
        #[arg(long, requires = "height")]
        width: Option<u32>,
        #[arg(long, requires = "width")]
        height: Option<u32>,
    },
    /// Import one host file into the disk.
    Put {
        image: PathBuf,
        source: PathBuf,
        /// Relative virtual path (default: source basename).
        #[arg(long)]
        name: Option<String>,
        #[arg(long, value_enum, default_value_t = CompressionArg::Auto)]
        compression: CompressionArg,
        /// Explicitly allow replacing an existing virtual file.
        #[arg(long)]
        overwrite: bool,
    },
    /// Export a virtual file to an explicit host path.
    Get {
        image: PathBuf,
        name: String,
        output: PathBuf,
        /// Explicitly allow replacing an existing regular output file.
        #[arg(long)]
        overwrite: bool,
    },
    /// Write a virtual file's original bytes to stdout (including binary files).
    Cat { image: PathBuf, name: String },
    /// List virtual files in path order.
    #[command(visible_alias = "ls")]
    List {
        image: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Delete one virtual file and reclaim its space.
    #[command(visible_alias = "rm")]
    Remove { image: PathBuf, name: String },
    /// Rename a virtual file without replacing another file.
    #[command(visible_alias = "mv")]
    Rename {
        image: PathBuf,
        from: String,
        to: String,
    },
    /// Show logical capacity, metadata overhead and physical PNG file length.
    Info {
        image: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Verify the complete stored image and every decompressed file's SHA-256.
    Verify { image: PathBuf },
}

#[derive(Clone, Copy, ValueEnum)]
enum CompressionArg {
    Auto,
    None,
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            if error
                .downcast_ref::<io::Error>()
                .is_some_and(|e| e.kind() == io::ErrorKind::BrokenPipe)
            {
                return ExitCode::SUCCESS;
            }
            eprintln!("error: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<()> {
    let stdout = io::stdout();
    let mut out = stdout.lock();
    match cli.command {
        Command::Create {
            image,
            size,
            width,
            height,
        } => {
            let (width, height) = match size {
                Some(bytes) => dimensions_for_size(bytes)?,
                None => (width.unwrap_or(1024), height.unwrap_or(1024)),
            };
            let disk = Disk::create(&image, width, height)?;
            let info = disk.info()?;
            writeln!(
                out,
                "Created {}: {width} x {height} RGB8 pixels",
                image.display()
            )?;
            writeln!(
                out,
                "Pixel capacity: {} bytes; free: {} bytes; PNG: {} bytes",
                info.capacity_bytes, info.free_bytes, info.png_bytes
            )?;
        }
        Command::Put {
            image,
            source,
            name,
            compression,
            overwrite,
        } => {
            let name = match name {
                Some(name) => name,
                None => source
                    .file_name()
                    .and_then(|s| s.to_str())
                    .context("Source basename is not UTF-8; provide --name explicitly")?
                    .to_owned(),
            };
            let compression = match compression {
                CompressionArg::Auto => Compression::Auto,
                CompressionArg::None => Compression::None,
            };
            let mut disk = Disk::open(image)?;
            disk.put_file(source, &name, compression, overwrite)?;
            writeln!(out, "Stored {name}")?;
        }
        Command::Get {
            image,
            name,
            output,
            overwrite,
        } => {
            Disk::open_read_only(image)?.extract(&name, &output, overwrite)?;
            writeln!(out, "Extracted {name} to {}", output.display())?;
        }
        Command::Cat { image, name } => {
            out.write_all(&Disk::open_read_only(image)?.read(&name)?)?;
        }
        Command::List { image, json } => {
            let files = Disk::open_read_only(image)?.list();
            if json {
                serde_json::to_writer_pretty(&mut out, &files)?;
                writeln!(out)?;
            } else {
                writeln!(
                    out,
                    "{:>14} {:>14} {:>6}  PATH",
                    "ORIGINAL", "STORED", "CODEC"
                )?;
                for file in files {
                    writeln!(
                        out,
                        "{:>14} {:>14} {:>6}  {}",
                        file.original_bytes, file.stored_bytes, file.encoding, file.path
                    )?;
                }
            }
        }
        Command::Remove { image, name } => {
            Disk::open(image)?.remove(&name)?;
            writeln!(out, "Removed {name}")?;
        }
        Command::Rename { image, from, to } => {
            Disk::open(image)?.rename(&from, &to)?;
            writeln!(out, "Renamed {from} to {to}")?;
        }
        Command::Info { image, json } => {
            let info = Disk::open_read_only(image)?.info()?;
            if json {
                serde_json::to_writer_pretty(&mut out, &info)?;
                writeln!(out)?;
            } else {
                writeln!(out, "Format version:   {}", info.format_version)?;
                writeln!(
                    out,
                    "Dimensions:       {} x {} RGB8",
                    info.width, info.height
                )?;
                writeln!(out, "Files:            {}", info.files)?;
                writeln!(out, "Pixel capacity:   {} bytes", info.capacity_bytes)?;
                writeln!(out, "Used pixels:      {} bytes", info.used_bytes)?;
                writeln!(out, "Free pixels:      {} bytes", info.free_bytes)?;
                writeln!(out, "Metadata:         {} bytes", info.metadata_bytes)?;
                writeln!(out, "Original files:   {} bytes", info.original_bytes)?;
                writeln!(out, "Stored payload:   {} bytes", info.stored_bytes)?;
                writeln!(out, "Physical PNG:     {} bytes", info.png_bytes)?;
                if info.original_bytes > 0 {
                    let savings =
                        (1.0 - info.png_bytes as f64 / info.original_bytes as f64) * 100.0;
                    writeln!(
                        out,
                        "PNG vs originals: {savings:.2}% smaller (negative means larger)"
                    )?;
                }
            }
        }
        Command::Verify { image } => {
            let disk = Disk::open_read_only(image)?;
            disk.verify()?;
            writeln!(
                out,
                "OK: verified disk structure and all {} file(s)",
                disk.list().len()
            )?;
        }
    }
    out.flush()?;
    Ok(())
}

fn parse_size(input: &str) -> Result<u64, String> {
    let input = input.trim();
    let split = input
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(input.len());
    let number = input[..split]
        .parse::<u64>()
        .map_err(|_| "Size must start with a positive whole number".to_owned())?;
    let multiplier: u64 = match input[split..].trim().to_ascii_lowercase().as_str() {
        "" | "b" => 1,
        "kib" => 1024,
        "mib" => 1024 * 1024,
        "gib" => 1024 * 1024 * 1024,
        "kb" => 1000,
        "mb" => 1000 * 1000,
        "gb" => 1000 * 1000 * 1000,
        _ => return Err("Supported units: B, KiB, MiB, GiB, KB, MB, GB".to_owned()),
    };
    let bytes = number
        .checked_mul(multiplier)
        .ok_or_else(|| "Size overflow".to_owned())?;
    if bytes == 0 {
        return Err("Size must be greater than zero".to_owned());
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_binary_decimal_and_invalid_sizes() {
        assert_eq!(parse_size("2MiB").unwrap(), 2_097_152);
        assert_eq!(parse_size("2MB").unwrap(), 2_000_000);
        assert_eq!(parse_size(" 12 kib ").unwrap(), 12_288);
        assert_eq!(parse_size("99").unwrap(), 99);
        for invalid in ["", "0", "-1", "1.5MiB", "1XB", "18446744073709551615GiB"] {
            assert!(parse_size(invalid).is_err(), "accepted {invalid:?}");
        }
    }

    #[test]
    fn dimensions_are_paired_and_exclude_size() {
        assert!(Cli::try_parse_from(["dpegfs", "create", "disk.png", "--width", "10"]).is_err());
        assert!(
            Cli::try_parse_from([
                "dpegfs", "create", "disk.png", "--width", "10", "--height", "10", "--size", "1MiB"
            ])
            .is_err()
        );
    }
}

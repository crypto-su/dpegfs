//! Reproducible size comparison, including an ordinary zlib baseline.
use std::io::Write;

use anyhow::Result;
use dpegfs::{Compression, Disk};
use flate2::write::ZlibEncoder;

fn main() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let size = 1024 * 1024;
    let line = b"dpegfs: data lives in pixels, compression depends on the input.\n";
    let text: Vec<u8> = line.iter().copied().cycle().take(size).collect();
    // Deterministic synthetic data for measurement, NOT a cryptographic RNG.
    let mut state = 0x4d59_5df4_d0f3_3173u64;
    let random: Vec<u8> = (0..size)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state as u8
        })
        .collect();

    println!(
        "| Input | Original bytes | Plain zlib bytes | dpegfs PNG bytes | Pixel payload bytes |"
    );
    println!("| --- | ---: | ---: | ---: | ---: |");
    for (name, data) in [("repeated-text", text), ("pseudo-random", random)] {
        let mut encoder = ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(&data)?;
        let zlib = encoder.finish()?;
        let mut disk = Disk::create(directory.path().join(format!("{name}.png")), 1024, 1024)?;
        disk.put("sample.bin", &data, Compression::Auto, false)?;
        disk.verify()?;
        assert_eq!(disk.read("sample.bin")?, data);
        let info = disk.info()?;
        println!(
            "| {name} | {} | {} | {} | {} |",
            data.len(),
            zlib.len(),
            info.png_bytes,
            info.stored_bytes
        );
    }
    println!("\nEach PNG has 3,145,728 bytes of logical RGB capacity; empty pixels compress too.");
    println!(
        "This measures file lengths, not allocated filesystem blocks. Temporary files are removed."
    );
    Ok(())
}

# dpegfs

**A virtual filesystem that stores files in the RGB pixels of a lossless PNG, written in Rust.**

`d` = database · `peg` = the photography/image idea · `fs` = filesystem

[Türkçe](../README.md) · [Format specification](FORMAT.md) · [Roadmap](ROADMAP.md)

Choose the image dimensions, add files, then recover their original bytes. Both the file table and payload occupy actual RGB channel values. Data is not stored in PNG comments, private chunks, or an archive appended after the image. The generated image is a data image; it does not preserve an existing photograph or implement cover-image steganography.

Version **0.1.0 is experimental**. It provides a CLI and a Rust library. OS mounting, POSIX semantics, and database queries are not implemented.

## Capacity and compression

An RGB8 pixel holds three bytes. Raw capacity is `width × height × 3`; a 1024 × 1024 disk holds **3 MiB**, including a 128-byte header and its variable-length file table.

Turning bytes into pixels does not inherently save space. `auto` compresses each file with zlib only when doing so reduces its payload size. PNG then losslessly compresses the entire pixel stream, including unused zero pixels. Compressible original files may therefore total more than the raw pixel capacity; an almost-empty PNG can also be physically tiny. Neither effect creates unlimited capacity. Already compressed or random data may produce a PNG larger than the input.

[PNG preserves pixel samples losslessly](https://www.w3.org/TR/png/), which is essential for this format. Conventional lossy JPEG can change the bytes. The project name does not imply that its disks use JPEG.

## Install

Install [Rust and Cargo](https://www.rust-lang.org/tools/install) **1.89+**, then clone and build the project:

```sh
git clone https://github.com/crypto-su/dpegfs.git
cd dpegfs
cargo build --release --locked
cargo install --path . --locked
dpegfs --help
```

Alternatively, replace `dpegfs` with `cargo run --locked --` to run without installing. The release binary is `target/release/dpegfs` (`target\release\dpegfs.exe` on Windows). No crates.io publication or downloadable release binary is assumed.

## Quick start

Run in the repository root, using a new output directory:

```sh
mkdir demo-output
dpegfs create demo-output/disk.png --size 16MiB
dpegfs put demo-output/disk.png README.md --name docs/README.md
dpegfs list demo-output/disk.png
dpegfs info demo-output/disk.png
dpegfs get demo-output/disk.png docs/README.md demo-output/restored.md
dpegfs verify demo-output/disk.png
```

On macOS/Linux, `cmp README.md demo-output/restored.md` checks that extraction is byte-identical. In PowerShell, compare the results of `Get-FileHash README.md, demo-output/restored.md -Algorithm SHA256`.

## Commands

| Operation | Command |
| --- | --- |
| Create the default 1024 × 1024 disk | `dpegfs create disk.png` |
| Choose dimensions | `dpegfs create disk.png --width 1920 --height 1080` |
| Choose raw capacity | `dpegfs create disk.png --size 16MiB` |
| Import with the source basename | `dpegfs put disk.png report.pdf` |
| Choose a virtual path | `dpegfs put disk.png report.pdf --name docs/report.pdf` |
| Replace an existing virtual file | `dpegfs put disk.png report.pdf --name docs/report.pdf --overwrite` |
| Disable per-file compression | `dpegfs put disk.png data.bin --compression none` |
| List files | `dpegfs list disk.png` or `dpegfs ls disk.png` |
| Machine-readable list | `dpegfs list disk.png --json` |
| Export to a host path | `dpegfs get disk.png docs/report.pdf restored.pdf` |
| Replace an existing output | `dpegfs get disk.png docs/report.pdf restored.pdf --overwrite` |
| Write original bytes to stdout | `dpegfs cat disk.png notes.txt` |
| Rename/move | `dpegfs rename disk.png notes.txt archive/notes.txt` |
| Remove | `dpegfs remove disk.png archive/notes.txt` |
| Report capacity and physical size | `dpegfs info disk.png --json` |
| Verify all stored data | `dpegfs verify disk.png` |

`mv` and `rm` are aliases for `rename` and `remove`. Use `dpegfs <command> --help` for details.

`create` never overwrites. `put` and `get` require `--overwrite` to replace an existing file. `rename` never replaces an existing virtual file. Image dimensions stay fixed; deletion immediately reclaims space.

`--size` includes metadata, not just user data, and is unrelated to the PNG's physical length. It rounds to a near-square whole-pixel image. It cannot be combined with `--width/--height`, which must be supplied together. Units are `B`, `KiB`, `MiB`, `GiB`, `KB`, `MB`, and `GB`; integers only. The rounded image must still fit the implementation limit, so a request at the exact upper limit may need to be slightly reduced.

Virtual paths are case-sensitive UTF-8 strings of at most 1024 bytes. Directories are implicit. Absolute paths, empty components, `.`/`..`, backslashes, colons, control characters, and file/directory conflicts are rejected. Empty directories, permissions, timestamps, symlinks, directory imports and bulk extraction are not supported. `get` requires an explicit output path with an existing parent directory.

`--compression none` disables per-file zlib; PNG compression still applies. `cat` emits binary data when appropriate. Do not redirect stdout onto the image: a shell may truncate it before dpegfs starts. Use `get` for protected file output.

Exit codes: `0` success, `1` operation failure, `2` invalid CLI arguments. Errors go to stderr.

## Understanding `info`

All sizes are bytes. `capacity_bytes` is raw RGB capacity. `metadata_bytes` includes the header and file table. `stored_bytes` counts encoded file payloads. `used_bytes = metadata_bytes + stored_bytes`, and `free_bytes = capacity_bytes - used_bytes`. Adding a file needs room for its **metadata as well as its payload**.

`original_bytes` is the total original file length; `png_bytes` is the current physical PNG file length, not allocated filesystem blocks. The human-readable savings percentage compares the PNG with original file lengths; negative means the PNG is larger.

## Reproduce the size experiment

```sh
cargo run --locked --release --example measure
```

The example uses two separate 1024 × 1024 disks, verifies round trips, compares plain zlib at the same compression level, and removes its temporary files.

| Input | Original bytes | Plain zlib | dpegfs PNG | Encoded payload |
| --- | ---: | ---: | ---: | ---: |
| Repeated text | 1,048,576 | 3,130 | 3,555 | 3,130 |
| Deterministic pseudo-random data | 1,048,576 | 1,048,752 | 1,051,761 | 1,048,576 |

These are sample results from the committed lockfile, not a promised ratio. The text savings come from compression. The random PNG is about 0.30% larger than its input, and direct zlib is smaller than the PNG in both examples.

## Rust API

Add a local dependency to another project:

```toml
[dependencies]
dpegfs = { path = "../dpegfs" }
anyhow = "1"
```

```rust
use dpegfs::{Compression, Disk};

fn main() -> anyhow::Result<()> {
    let mut disk = Disk::create("disk.png", 1024, 1024)?;
    disk.put("notes/hello.txt", b"Hello, pixels!", Compression::Auto, false)?;
    assert_eq!(disk.read("notes/hello.txt")?, b"Hello, pixels!");
    disk.rename("notes/hello.txt", "archive/hello.txt")?;
    disk.verify()?;
    drop(disk);

    let snapshot = Disk::open_read_only("disk.png")?;
    snapshot.extract("archive/hello.txt", "restored.txt", false)?;
    Ok(())
}
```

`Disk::open` opens an existing writable image and holds its writer lock until dropped. `Disk::open_read_only` takes an immutable snapshot and creates no sidecar. Reopen a snapshot to see later changes. Successful `put`, `put_file`, `remove`, and `rename` calls are already persisted; no separate save call is needed. Generate API docs with `cargo doc --no-deps --open`.

## Integrity and operational limits

The header and stored body have separate SHA-256 digests. Each file also has a digest of its original bytes. Opening validates the header, body, bounds, paths, layout and zero padding; reading validates the selected file; `verify` decompresses and checks every file. Hashes detect corruption but do not provide repair, authentication or encryption.

Changes are prepared and capacity-checked in memory, staged to a temporary file in the destination directory, synced, and published with atomic replacement. Rejected writes preserve the previous image. Every mutation rewrites the complete PNG. Same-directory temporary files need additional host disk space.

Cooperating writers use an OS lock on `disk.png.dpegfs.lock`. A second writer fails immediately. Closing the process releases the lock; the empty sidecar can remain and contains no data. Do not delete it while dpegfs is running. Copying just the PNG is enough to transfer the disk. Readers see a complete snapshot. Noncooperating external writers, network filesystem locking/rename behavior, and full power-loss durability are outside v1's guarantees. The parent directory is not fsynced; there is no journal.

Images are loaded into memory. Limits: **256 MiB decoded RGB capacity**, **256 MiB original size per file**, **16,384 pixels per dimension**, **10,000 files**, **16 MiB file table**. Peak memory may be several times the image's raw capacity. This implementation targets small experimental volumes.

Final-component symlinks are rejected for images and outputs. Unix also rejects writable images with multiple hard links; use one ordinary image path on other systems too. Parent directory symlinks are canonicalized.

Resizing, cropping, color adjustments or JPEG conversion can destroy the stored data. Services that recompress uploaded images can do the same: transfer the original file. Deletion is not secure erasure. Keep independent copies of important data while using this experimental format.

## Development

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets
cargo test --locked --doc
```

Tests cover binary/Unicode/empty files, direct RGB storage, compression, capacity and overwrite failures, atomic staging failure, concurrent writers, malformed formats, truncated PNG/zlib data, path constraints, and complete CLI workflows. GitHub Actions is configured for Linux, macOS and Windows tests, plus minimum Rust checking; local verification alone does not establish results on all platforms.

License: [MIT](../LICENSE).

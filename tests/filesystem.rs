use std::fs;
use std::io::BufReader;

use dpegfs::{Compression, Disk, Encoding, MAX_CAPACITY, dimensions_for_size, pixel_capacity};
use tempfile::tempdir;

#[test]
fn binary_unicode_and_empty_files_survive_reopen() -> anyhow::Result<()> {
    let dir = tempdir()?;
    let path = dir.path().join("disk.png");
    let binary: Vec<u8> = (0..=255).cycle().take(4096).collect();
    {
        let mut disk = Disk::create(&path, 128, 128)?;
        disk.put("data/all-bytes.bin", &binary, Compression::None, false)?;
        disk.put(
            "notlar/İstanbul 🦀.txt",
            "Merhaba, pikseller!\n".as_bytes(),
            Compression::Auto,
            false,
        )?;
        disk.put("empty", &[], Compression::Auto, false)?;
        disk.verify()?;
    }
    let disk = Disk::open_read_only(&path)?;
    assert_eq!(disk.read("data/all-bytes.bin")?, binary);
    assert_eq!(
        disk.read("notlar/İstanbul 🦀.txt")?,
        "Merhaba, pikseller!\n".as_bytes()
    );
    assert!(disk.read("empty")?.is_empty());
    assert_eq!(disk.list().len(), 3);
    disk.verify()?;
    Ok(())
}

#[test]
fn payload_really_lives_in_rgb_pixels() -> anyhow::Result<()> {
    let dir = tempdir()?;
    let path = dir.path().join("disk.png");
    let mut disk = Disk::create(&path, 32, 32)?;
    let payload: Vec<u8> = (0..=255).collect();
    disk.put("raw.bin", &payload, Compression::None, false)?;
    let mut png = png::Decoder::new(BufReader::new(fs::File::open(path)?)).read_info()?;
    assert_eq!(png.info().color_type, png::ColorType::Rgb);
    assert_eq!(png.info().bit_depth, png::BitDepth::Eight);
    let mut pixels = vec![0; png.output_buffer_size().unwrap()];
    png.next_frame(&mut pixels)?;
    assert_eq!(&pixels[..8], b"DPEGFS\0\0");
    let manifest_length = u64::from_le_bytes(pixels[24..32].try_into()?) as usize;
    assert_eq!(
        &pixels[128 + manifest_length..128 + manifest_length + payload.len()],
        payload
    );
    Ok(())
}

#[test]
fn compression_fits_more_logical_data_and_none_does_not() -> anyhow::Result<()> {
    let dir = tempdir()?;
    let mut disk = Disk::create(dir.path().join("disk.png"), 32, 32)?;
    let data = vec![b'A'; 20_000];
    disk.put("large.txt", &data, Compression::Auto, false)?;
    assert_eq!(disk.list()[0].encoding, Encoding::Zlib);
    assert_eq!(disk.read("large.txt")?, data);
    assert_eq!(disk.info()?.original_bytes, 20_000);
    assert!(disk.info()?.stored_bytes < 200);
    assert!(
        disk.put("raw.txt", &data, Compression::None, false)
            .is_err()
    );
    assert_eq!(disk.list().len(), 1);
    assert_eq!(disk.list()[0].path, "large.txt");
    Ok(())
}

#[test]
fn auto_keeps_small_incompressible_data_raw() -> anyhow::Result<()> {
    let dir = tempdir()?;
    let mut disk = Disk::create(dir.path().join("disk.png"), 32, 32)?;
    disk.put("one", &[42], Compression::Auto, false)?;
    assert_eq!(disk.list()[0].encoding, Encoding::Raw);
    assert_eq!(disk.list()[0].stored_bytes, 1);
    Ok(())
}

#[test]
fn failed_writes_leave_memory_and_png_unchanged() -> anyhow::Result<()> {
    let dir = tempdir()?;
    let path = dir.path().join("disk.png");
    let mut disk = Disk::create(&path, 32, 32)?;
    disk.put("keep", b"original", Compression::None, false)?;
    let before = fs::read(&path)?;
    let free = disk.info()?.free_bytes;
    assert!(
        disk.put("too-big", &[1; 4096], Compression::None, false)
            .is_err()
    );
    assert!(
        disk.put("keep", &[1; 4096], Compression::None, true)
            .is_err()
    );
    assert_eq!(disk.read("keep")?, b"original");
    assert_eq!(disk.info()?.free_bytes, free);
    assert_eq!(fs::read(path)?, before);
    Ok(())
}

#[test]
fn explicit_overwrite_is_required_and_persists() -> anyhow::Result<()> {
    let dir = tempdir()?;
    let path = dir.path().join("disk.png");
    let mut disk = Disk::create(&path, 32, 32)?;
    disk.put("one", b"first", Compression::None, false)?;
    assert!(
        disk.put("one", b"second", Compression::Auto, false)
            .is_err()
    );
    disk.put("one", b"second", Compression::Auto, true)?;
    drop(disk);
    assert_eq!(Disk::open_read_only(path)?.read("one")?, b"second");
    Ok(())
}

#[test]
fn create_never_clobbers_an_existing_host_file() -> anyhow::Result<()> {
    let dir = tempdir()?;
    let path = dir.path().join("existing.png");
    fs::write(&path, b"important original")?;
    assert!(Disk::create(&path, 32, 32).is_err());
    assert_eq!(fs::read(path)?, b"important original");
    Ok(())
}

#[test]
fn deletion_reclaims_space_and_rename_preserves_content() -> anyhow::Result<()> {
    let dir = tempdir()?;
    let path = dir.path().join("disk.png");
    let mut disk = Disk::create(&path, 32, 32)?;
    let initial_free = disk.info()?.free_bytes;
    disk.put("folder/old", b"content", Compression::Auto, false)?;
    disk.rename("folder/old", "new/place.txt")?;
    assert!(disk.read("folder/old").is_err());
    assert_eq!(disk.read("new/place.txt")?, b"content");
    disk.rename("new/place.txt", "new/place.txt")?;
    disk.remove("new/place.txt")?;
    assert_eq!(disk.info()?.free_bytes, initial_free);
    assert!(disk.remove("missing").is_err());
    drop(disk);
    let disk = Disk::open_read_only(path)?;
    assert!(disk.list().is_empty());
    disk.verify()?;
    Ok(())
}

#[test]
fn paths_cannot_escape_or_conflict_with_directories() -> anyhow::Result<()> {
    let dir = tempdir()?;
    let mut disk = Disk::create(dir.path().join("disk.png"), 32, 32)?;
    for name in [
        "",
        "/etc/file",
        "..",
        "../file",
        "a/../b",
        "a/./b",
        "a//b",
        "a/",
        "a\\b",
        "C:foo",
        "bad\nname",
        "bad\0name",
    ] {
        assert!(
            disk.put(name, b"x", Compression::None, false).is_err(),
            "accepted {name:?}"
        );
    }
    disk.put("docs/readme", b"x", Compression::None, false)?;
    assert!(disk.put("docs", b"x", Compression::None, false).is_err());
    assert!(
        disk.put("docs/readme/child", b"x", Compression::None, false)
            .is_err()
    );
    disk.put("target", b"y", Compression::None, false)?;
    assert!(disk.rename("target", "docs/readme").is_err());
    assert!(disk.rename("target", "docs").is_err());
    assert_eq!(disk.read("target")?, b"y");
    Ok(())
}

#[test]
fn rename_cannot_overflow_metadata_capacity() -> anyhow::Result<()> {
    let dir = tempdir()?;
    let path = dir.path().join("disk.png");
    let mut disk = Disk::create(&path, 16, 16)?;
    disk.put("x", b"x", Compression::None, false)?;
    let before = fs::read(&path)?;
    assert!(disk.rename("x", &"a".repeat(1000)).is_err());
    assert_eq!(disk.read("x")?, b"x");
    assert_eq!(fs::read(path)?, before);
    Ok(())
}

#[test]
fn writers_are_exclusive_and_readers_observe_snapshots() -> anyhow::Result<()> {
    let dir = tempdir()?;
    let path = dir.path().join("disk.png");
    let mut writer = Disk::create(&path, 32, 32)?;
    assert!(Disk::open(&path).is_err());
    let snapshot = Disk::open_read_only(&path)?;
    writer.put("new", b"new", Compression::None, false)?;
    assert!(snapshot.read("new").is_err());
    assert_eq!(Disk::open_read_only(&path)?.read("new")?, b"new");
    drop(writer);
    Disk::open(&path)?;
    Ok(())
}

#[test]
fn read_only_open_needs_no_sidecar_and_refuses_mutations() -> anyhow::Result<()> {
    let dir = tempdir()?;
    let path = dir.path().join("disk.png");
    drop(Disk::create(&path, 32, 32)?);
    let lock = dir.path().join("disk.png.dpegfs.lock");
    fs::remove_file(&lock)?;
    let mut disk = Disk::open_read_only(&path)?;
    assert!(!lock.exists());
    assert!(disk.put("x", b"x", Compression::None, false).is_err());
    assert!(disk.remove("x").is_err());
    assert!(disk.rename("x", "y").is_err());
    assert!(!lock.exists());
    Ok(())
}

#[test]
fn extraction_protects_existing_files_disk_and_lock() -> anyhow::Result<()> {
    let dir = tempdir()?;
    let path = dir.path().join("disk.png");
    let mut disk = Disk::create(&path, 32, 32)?;
    disk.put("file", b"inside", Compression::Auto, false)?;
    let output = dir.path().join("output.bin");
    fs::write(&output, b"outside")?;
    assert!(disk.extract("file", &output, false).is_err());
    assert_eq!(fs::read(&output)?, b"outside");
    disk.extract("file", &output, true)?;
    assert_eq!(fs::read(output)?, b"inside");
    assert!(disk.extract("file", &path, true).is_err());
    assert!(
        disk.extract("file", dir.path().join("disk.png.dpegfs.lock"), true)
            .is_err()
    );
    assert!(disk.extract("file", dir.path(), true).is_err());
    disk.verify()?;
    Ok(())
}

#[test]
fn rejects_bad_dimensions_and_too_small_images_without_output() -> anyhow::Result<()> {
    let dir = tempdir()?;
    let path = dir.path().join("disk.png");
    for (w, h) in [(0, 1), (1, 0), (u32::MAX, u32::MAX), (16384, 16384), (1, 1)] {
        assert!(Disk::create(&path, w, h).is_err());
        assert!(!path.exists());
    }
    assert!(dimensions_for_size(0).is_err());
    assert!(dimensions_for_size(MAX_CAPACITY + 1).is_err());
    let (w, h) = dimensions_for_size(1024 * 1024)?;
    assert!(pixel_capacity(w, h)? >= 1024 * 1024);
    assert_eq!(pixel_capacity(1024, 1024)?, 3 * 1024 * 1024);
    Ok(())
}

#[test]
fn rejects_truncated_png_and_ordinary_image() -> anyhow::Result<()> {
    let dir = tempdir()?;
    let path = dir.path().join("disk.png");
    drop(Disk::create(&path, 32, 32)?);
    let good = fs::read(&path)?;
    for length in [0, 8, good.len() / 2, good.len() - 12, good.len() - 1] {
        fs::write(&path, &good[..length])?;
        assert!(
            Disk::open_read_only(&path).is_err(),
            "accepted truncated PNG of {length} bytes"
        );
    }
    let mut encoder = png::Encoder::new(fs::File::create(&path)?, 32, 32);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    let mut png = encoder.write_header()?;
    png.write_image_data(&vec![0; 32 * 32 * 3])?;
    png.finish()?;
    assert!(Disk::open_read_only(path).is_err());
    Ok(())
}

#[cfg(unix)]
#[test]
fn refuses_symbolic_link_outputs_and_writable_hard_links() -> anyhow::Result<()> {
    use std::os::unix::fs::symlink;
    let dir = tempdir()?;
    let path = dir.path().join("disk.png");
    let mut disk = Disk::create(&path, 32, 32)?;
    disk.put("x", b"x", Compression::None, false)?;
    let link = dir.path().join("link.png");
    symlink(&path, &link)?;
    assert!(disk.extract("x", &link, true).is_err());
    assert!(Disk::open(&link).is_err());
    drop(disk);
    fs::hard_link(&path, dir.path().join("hard.png"))?;
    assert!(Disk::open(&path).is_err());
    Disk::open_read_only(&path)?.verify()?;
    Ok(())
}

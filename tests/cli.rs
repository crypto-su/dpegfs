use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use anyhow::Result;
use dpegfs::Disk;

fn run(directory: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_dpegfs"))
        .current_dir(directory)
        .args(args)
        .output()
        .unwrap()
}

fn success(directory: &Path, args: &[&str]) -> Output {
    let output = run(directory, args);
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

#[test]
fn complete_cli_workflow_roundtrips_binary_and_reports_json() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let root = directory.path();
    let bytes: Vec<u8> = (0..=255).cycle().take(8192).collect();
    fs::write(root.join("input.bin"), &bytes)?;
    success(root, &["create", "disk.png", "--size", "64KiB"]);
    success(
        root,
        &["put", "disk.png", "input.bin", "--name", "docs/input.bin"],
    );
    let listed = success(root, &["ls", "disk.png", "--json"]);
    let files: serde_json::Value = serde_json::from_slice(&listed.stdout)?;
    assert_eq!(files[0]["path"], "docs/input.bin");
    assert_eq!(files[0]["original_bytes"], 8192);
    let stats = success(root, &["info", "disk.png", "--json"]);
    let info: serde_json::Value = serde_json::from_slice(&stats.stdout)?;
    assert!(info["capacity_bytes"].as_u64().unwrap() >= 65536);
    assert_eq!(
        info["png_bytes"],
        fs::metadata(root.join("disk.png"))?.len()
    );
    assert_eq!(
        success(root, &["cat", "disk.png", "docs/input.bin"]).stdout,
        bytes
    );
    success(root, &["get", "disk.png", "docs/input.bin", "output.bin"]);
    assert_eq!(fs::read(root.join("output.bin"))?, bytes);
    assert!(
        !run(root, &["get", "disk.png", "docs/input.bin", "output.bin"])
            .status
            .success()
    );
    success(
        root,
        &[
            "get",
            "disk.png",
            "docs/input.bin",
            "output.bin",
            "--overwrite",
        ],
    );
    success(root, &["verify", "disk.png"]);
    success(root, &["mv", "disk.png", "docs/input.bin", "archive.bin"]);
    success(root, &["rm", "disk.png", "archive.bin"]);
    let empty = success(root, &["list", "disk.png", "--json"]);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&empty.stdout)?,
        serde_json::json!([])
    );
    Ok(())
}

#[test]
fn errors_have_failure_exit_codes_and_do_not_destroy_images() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let root = directory.path();
    success(
        root,
        &["create", "disk.png", "--width", "32", "--height", "32"],
    );
    let before = fs::read(root.join("disk.png"))?;
    for args in [
        vec!["create", "disk.png"],
        vec!["get", "disk.png", "missing", "output.bin"],
        vec!["put", "disk.png", "missing.bin"],
        vec!["rename", "disk.png", "missing", "name"],
        vec!["remove", "disk.png", "missing"],
    ] {
        let output = run(root, &args);
        assert!(!output.status.success());
        assert!(!output.stderr.is_empty());
        assert_eq!(fs::read(root.join("disk.png"))?, before);
    }
    assert!(!root.join("output.bin").exists());
    Ok(())
}

#[test]
fn writer_lock_blocks_another_process_but_allows_readers() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let root = directory.path();
    let disk = Disk::create(root.join("disk.png"), 32, 32)?;
    fs::write(root.join("file.txt"), b"hello")?;
    let blocked = run(root, &["put", "disk.png", "file.txt"]);
    assert!(!blocked.status.success());
    assert!(String::from_utf8_lossy(&blocked.stderr).contains("writer lock"));
    success(root, &["verify", "disk.png"]);
    drop(disk);
    success(root, &["put", "disk.png", "file.txt"]);
    Ok(())
}

use std::io::Write;

use super::*;

fn fixture() -> Vec<u8> {
    let mut files = BTreeMap::new();
    for name in ["a", "b"] {
        files.insert(
            name.to_owned(),
            StoredFile {
                original_bytes: 3,
                encoding: Encoding::Raw,
                sha256: hash_hex(b"abc"),
                data: b"abc".to_vec().into(),
            },
        );
    }
    encode(&Image {
        width: 32,
        height: 32,
        files,
    })
    .unwrap()
}

fn rehash_header(pixels: &mut [u8]) {
    let hash = Sha256::digest(&pixels[..80]);
    pixels[80..112].copy_from_slice(&hash);
}

fn rehash_all(pixels: &mut [u8]) {
    let end = HEADER_BYTES + le_u64(pixels, 24) as usize + le_u64(pixels, 32) as usize;
    let hash = Sha256::digest(&pixels[HEADER_BYTES..end]);
    pixels[48..80].copy_from_slice(&hash);
    rehash_header(pixels);
}

fn alter_manifest(edit: impl FnOnce(&mut Manifest)) -> Vec<u8> {
    let source = fixture();
    let start = HEADER_BYTES + le_u64(&source, 24) as usize;
    let payload_len = le_u64(&source, 32) as usize;
    let mut manifest: Manifest = serde_json::from_slice(&source[HEADER_BYTES..start]).unwrap();
    edit(&mut manifest);
    let json = serde_json::to_vec(&manifest).unwrap();
    let mut pixels = vec![0; source.len()];
    pixels[..HEADER_BYTES].copy_from_slice(&source[..HEADER_BYTES]);
    pixels[24..32].copy_from_slice(&(json.len() as u64).to_le_bytes());
    pixels[40..48].copy_from_slice(&(manifest.files.len() as u64).to_le_bytes());
    pixels[HEADER_BYTES..HEADER_BYTES + json.len()].copy_from_slice(&json);
    pixels[HEADER_BYTES + json.len()..HEADER_BYTES + json.len() + payload_len]
        .copy_from_slice(&source[start..start + payload_len]);
    rehash_all(&mut pixels);
    pixels
}

#[test]
fn detects_changed_header_body_and_unused_pixels() {
    let good = fixture();
    assert!(decode(&good, 32, 32).is_ok());
    for offset in [0, 16, 48, 80, 112, 128, good.len() - 1] {
        let mut bad = good.clone();
        bad[offset] ^= 1;
        assert!(
            decode(&bad, 32, 32).is_err(),
            "missed corruption at {offset}"
        );
    }
}

#[test]
fn rejects_unsupported_versions_even_with_valid_header_hash() {
    let mut pixels = fixture();
    pixels[8..12].copy_from_slice(&2u32.to_le_bytes());
    rehash_header(&mut pixels);
    assert!(
        decode(&pixels, 32, 32)
            .err()
            .unwrap()
            .to_string()
            .contains("Unsupported")
    );
}

#[test]
fn rejects_forged_extents_and_counts_without_panicking() {
    for (offset, value) in [(24, u64::MAX), (32, u64::MAX), (40, 10001), (40, 0)] {
        let mut pixels = fixture();
        pixels[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
        rehash_header(&mut pixels);
        assert!(decode(&pixels, 32, 32).is_err());
    }
    assert!(decode(&fixture(), 31, 32).is_err());
    for length in 0..HEADER_BYTES {
        assert!(decode(&vec![0; length], 1, 1).is_err());
    }
}

#[test]
fn rejects_invalid_paths_order_duplicates_and_directory_conflicts() {
    for name in ["../escape", "/absolute", "a//b", "a\\b", "b", "z"] {
        let pixels = alter_manifest(|m| m.files[0].path = name.to_owned());
        assert!(decode(&pixels, 32, 32).is_err(), "accepted {name}");
    }
    // Parent paths are not necessarily adjacent in a lexically sorted table.
    let pixels = alter_manifest(|m| {
        m.files[1].path = "a/child".to_owned();
    });
    assert!(decode(&pixels, 32, 32).is_err());
}

#[test]
fn rejects_invalid_file_offsets_sizes_checksums_and_unreferenced_payload() {
    let bad_offset = alter_manifest(|m| m.files[1].offset = 0);
    let bad_length = alter_manifest(|m| m.files[1].stored_bytes = u64::MAX);
    let bad_original = alter_manifest(|m| m.files[0].original_bytes = MAX_FILE_SIZE + 1);
    let bad_hash = alter_manifest(|m| m.files[0].sha256 = "g".repeat(64));
    let extra_payload = alter_manifest(|m| {
        m.files.pop();
    });
    for pixels in [
        bad_offset,
        bad_length,
        bad_original,
        bad_hash,
        extra_payload,
    ] {
        assert!(decode(&pixels, 32, 32).is_err());
    }
}

#[test]
fn file_checksum_is_checked_even_if_outer_hashes_are_recomputed() {
    let mut pixels = fixture();
    let payload_start = HEADER_BYTES + le_u64(&pixels, 24) as usize;
    pixels[payload_start] ^= 1;
    rehash_all(&mut pixels);
    let image = decode(&pixels, 32, 32).unwrap();
    assert!(decode_file(&image.files["a"]).is_err());
}

fn compressed_file() -> StoredFile {
    let original = vec![b'X'; 4096];
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&original).unwrap();
    StoredFile {
        original_bytes: original.len() as u64,
        encoding: Encoding::Zlib,
        sha256: hash_hex(&original),
        data: encoder.finish().unwrap().into(),
    }
}

#[test]
fn zlib_requires_stream_end_exact_length_and_no_trailing_bytes() {
    let good = compressed_file();
    assert_eq!(decode_file(&good).unwrap(), vec![b'X'; 4096]);
    for length in [0, 1, good.data.len() - 4, good.data.len() - 1] {
        let mut bad = good.clone();
        bad.data = good.data[..length].into();
        assert!(
            decode_file(&bad).is_err(),
            "accepted truncated stream of {length} bytes"
        );
    }
    let mut extra = good.data.to_vec();
    extra.push(0);
    let mut bad = good.clone();
    bad.data = extra.into();
    assert!(decode_file(&bad).is_err());
    let mut oversized = good.clone();
    oversized.original_bytes = 1;
    assert!(decode_file(&oversized).is_err());
    let mut undersized = good;
    undersized.original_bytes += 1;
    assert!(decode_file(&undersized).is_err());
}

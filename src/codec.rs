use std::collections::BTreeMap;
use std::fs::File;
use std::io::BufReader;

use anyhow::{Context, Result, ensure};
use flate2::{Decompress, FlushDecompress, Status};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{Encoding, Image, MAX_FILE_SIZE, MAX_FILES, StoredFile, pixel_capacity, validate_name};

pub(crate) const VERSION: u32 = 1;
pub(crate) const HEADER_BYTES: usize = 128;
pub(crate) const MAX_MANIFEST_BYTES: usize = 16 * 1024 * 1024;
const MAGIC: &[u8; 8] = b"DPEGFS\0\0";

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    files: Vec<Entry>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    path: String,
    offset: u64,
    stored_bytes: u64,
    original_bytes: u64,
    encoding: Encoding,
    sha256: String,
}

pub(crate) fn hash_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub(crate) fn manifest(image: &Image) -> Result<Vec<u8>> {
    let mut offset = 0;
    let files = image
        .files
        .iter()
        .map(|(path, file)| {
            let entry = Entry {
                path: path.clone(),
                offset,
                stored_bytes: file.data.len() as u64,
                original_bytes: file.original_bytes,
                encoding: file.encoding,
                sha256: file.sha256.clone(),
            };
            offset += entry.stored_bytes;
            entry
        })
        .collect();
    let bytes = serde_json::to_vec(&Manifest { files })?;
    ensure!(
        bytes.len() <= MAX_MANIFEST_BYTES,
        "File table exceeds the 16 MiB limit"
    );
    Ok(bytes)
}

pub(crate) fn encode(image: &Image) -> Result<Vec<u8>> {
    let capacity = usize::try_from(pixel_capacity(image.width, image.height)?)?;
    let manifest = manifest(image)?;
    let payload_len: usize = image.files.values().map(|file| file.data.len()).sum();
    let used = HEADER_BYTES + manifest.len() + payload_len;
    ensure!(
        used <= capacity,
        "Not enough space: need {used} pixel bytes including metadata, but capacity is {capacity}"
    );
    let mut pixels = vec![0; capacity];
    pixels[..8].copy_from_slice(MAGIC);
    pixels[8..12].copy_from_slice(&VERSION.to_le_bytes());
    pixels[12..16].copy_from_slice(&(HEADER_BYTES as u32).to_le_bytes());
    pixels[16..20].copy_from_slice(&image.width.to_le_bytes());
    pixels[20..24].copy_from_slice(&image.height.to_le_bytes());
    pixels[24..32].copy_from_slice(&(manifest.len() as u64).to_le_bytes());
    pixels[32..40].copy_from_slice(&(payload_len as u64).to_le_bytes());
    pixels[40..48].copy_from_slice(&(image.files.len() as u64).to_le_bytes());
    let mut cursor = HEADER_BYTES;
    pixels[cursor..cursor + manifest.len()].copy_from_slice(&manifest);
    cursor += manifest.len();
    for file in image.files.values() {
        pixels[cursor..cursor + file.data.len()].copy_from_slice(&file.data);
        cursor += file.data.len();
    }
    let body_hash = Sha256::digest(&pixels[HEADER_BYTES..used]);
    pixels[48..80].copy_from_slice(&body_hash);
    let header_hash = Sha256::digest(&pixels[..80]);
    pixels[80..112].copy_from_slice(&header_hash);
    Ok(pixels)
}

fn le_u32(bytes: &[u8], start: usize) -> u32 {
    let mut value = [0; 4];
    value.copy_from_slice(&bytes[start..start + 4]);
    u32::from_le_bytes(value)
}

fn le_u64(bytes: &[u8], start: usize) -> u64 {
    let mut value = [0; 8];
    value.copy_from_slice(&bytes[start..start + 8]);
    u64::from_le_bytes(value)
}

fn decode(pixels: &[u8], width: u32, height: u32) -> Result<Image> {
    ensure!(
        pixels.len() >= HEADER_BYTES,
        "Image is too small for a dpegfs header"
    );
    ensure!(
        &pixels[..8] == MAGIC,
        "Not a dpegfs image (invalid magic bytes)"
    );
    ensure!(
        pixels[80..112] == Sha256::digest(&pixels[..80])[..],
        "Corrupt disk header: SHA-256 mismatch"
    );
    ensure!(
        le_u32(pixels, 8) == VERSION,
        "Unsupported dpegfs format version: {}",
        le_u32(pixels, 8)
    );
    ensure!(
        le_u32(pixels, 12) == HEADER_BYTES as u32,
        "Invalid header length"
    );
    ensure!(
        le_u32(pixels, 16) == width && le_u32(pixels, 20) == height,
        "PNG dimensions do not match the disk header"
    );
    ensure!(
        pixels[112..128].iter().all(|&b| b == 0),
        "Nonzero reserved header bytes"
    );
    let manifest_len = usize::try_from(le_u64(pixels, 24)).context("Invalid file table length")?;
    let payload_len = usize::try_from(le_u64(pixels, 32)).context("Invalid payload length")?;
    let file_count = le_u64(pixels, 40);
    ensure!(
        file_count <= MAX_FILES as u64,
        "Too many files in disk header"
    );
    ensure!(
        manifest_len <= MAX_MANIFEST_BYTES,
        "File table exceeds the 16 MiB limit"
    );
    let payload_start = HEADER_BYTES
        .checked_add(manifest_len)
        .context("File table length overflow")?;
    let used = payload_start
        .checked_add(payload_len)
        .context("Payload length overflow")?;
    ensure!(
        used <= pixels.len(),
        "File table or payload extends beyond the image"
    );
    ensure!(
        pixels[48..80] == Sha256::digest(&pixels[HEADER_BYTES..used])[..],
        "Corrupt disk body: SHA-256 mismatch"
    );
    ensure!(
        pixels[used..].iter().all(|&b| b == 0),
        "Corrupt free space: expected zero padding"
    );
    let manifest: Manifest = serde_json::from_slice(&pixels[HEADER_BYTES..payload_start])
        .context("Invalid file table JSON")?;
    ensure!(
        manifest.files.len() as u64 == file_count,
        "File count does not match header"
    );
    ensure!(
        serde_json::to_vec(&manifest)? == pixels[HEADER_BYTES..payload_start],
        "File table is not in the canonical v1 JSON representation"
    );
    let mut files = BTreeMap::new();
    let mut expected_offset = 0u64;
    let mut previous_path = None::<String>;
    for entry in manifest.files {
        validate_name(&entry.path).context("Invalid path in file table")?;
        if let Some(previous) = &previous_path {
            ensure!(
                *previous < entry.path,
                "File table paths must be unique and sorted"
            );
        }
        for (index, _) in entry.path.match_indices('/') {
            ensure!(
                !files.contains_key(&entry.path[..index]),
                "File/directory conflict in file table"
            );
        }
        ensure!(
            entry.offset == expected_offset,
            "Invalid file offset: payloads must be contiguous"
        );
        ensure!(
            entry.original_bytes <= MAX_FILE_SIZE,
            "Stored file exceeds the 256 MiB decoded size limit"
        );
        ensure!(
            entry.sha256.len() == 64
                && entry
                    .sha256
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "Invalid file SHA-256 encoding"
        );
        if entry.encoding == Encoding::Raw {
            ensure!(
                entry.stored_bytes == entry.original_bytes,
                "Raw file length mismatch"
            );
        } else {
            ensure!(entry.stored_bytes > 0, "Empty zlib stream");
        }
        let end = entry
            .offset
            .checked_add(entry.stored_bytes)
            .context("File length overflow")?;
        ensure!(end <= payload_len as u64, "File extends beyond payload");
        let start_index = payload_start + usize::try_from(entry.offset)?;
        let end_index = payload_start + usize::try_from(end)?;
        previous_path = Some(entry.path.clone());
        files.insert(
            entry.path,
            StoredFile {
                original_bytes: entry.original_bytes,
                encoding: entry.encoding,
                sha256: entry.sha256,
                data: pixels[start_index..end_index].into(),
            },
        );
        expected_offset = end;
    }
    ensure!(
        expected_offset == payload_len as u64,
        "Unreferenced bytes in payload"
    );
    Ok(Image {
        width,
        height,
        files,
    })
}

pub(crate) fn decode_file(file: &StoredFile) -> Result<Vec<u8>> {
    let decoded = match file.encoding {
        Encoding::Raw => file.data.to_vec(),
        Encoding::Zlib => {
            // One spare byte detects streams expanding beyond their declared
            // size. Requiring StreamEnd also rejects truncated zlib checksums.
            let expected = usize::try_from(file.original_bytes)?;
            let mut bytes = vec![0; expected + 1];
            let mut decoder = Decompress::new(true);
            let status = decoder
                .decompress(&file.data, &mut bytes, FlushDecompress::Finish)
                .context("Invalid zlib payload")?;
            ensure!(
                status == Status::StreamEnd,
                "Incomplete or oversized zlib payload"
            );
            ensure!(
                decoder.total_in() == file.data.len() as u64,
                "Trailing bytes in zlib payload"
            );
            ensure!(
                decoder.total_out() == file.original_bytes,
                "Decompressed file size mismatch"
            );
            bytes.truncate(expected);
            bytes
        }
    };
    ensure!(
        decoded.len() as u64 == file.original_bytes,
        "Original file size mismatch"
    );
    ensure!(hash_hex(&decoded) == file.sha256, "File SHA-256 mismatch");
    Ok(decoded)
}

pub(crate) fn write_png(out: &mut File, width: u32, height: u32, pixels: &[u8]) -> Result<()> {
    let mut encoder = png::Encoder::new(out, width, height);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::Balanced);
    encoder.set_filter(png::Filter::NoFilter);
    let mut writer = encoder.write_header()?;
    writer.write_image_data(pixels)?;
    writer.finish()?;
    Ok(())
}

pub(crate) fn read_png(file: File) -> Result<Image> {
    let decoder = png::Decoder::new_with_limits(
        BufReader::new(file),
        png::Limits {
            bytes: 32 * 1024 * 1024,
        },
    );
    let mut reader = decoder.read_info().context("Cannot decode PNG header")?;
    let info = reader.info();
    ensure!(
        info.color_type == png::ColorType::Rgb && info.bit_depth == png::BitDepth::Eight,
        "dpegfs requires an 8-bit RGB PNG"
    );
    ensure!(
        info.animation_control.is_none(),
        "Animated PNG disks are not supported"
    );
    let (width, height) = (info.width, info.height);
    let capacity = usize::try_from(pixel_capacity(width, height)?)?;
    ensure!(
        reader.output_buffer_size() == Some(capacity),
        "Unexpected decoded PNG size"
    );
    let mut pixels = vec![0; capacity];
    let frame = reader
        .next_frame(&mut pixels)
        .context("Cannot decode PNG pixels")?;
    ensure!(frame.buffer_size() == capacity, "Incomplete PNG pixel data");
    reader.finish().context("Invalid or truncated PNG ending")?;
    decode(&pixels, width, height)
}

#[cfg(test)]
mod tests;

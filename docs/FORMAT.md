# dpegfs pixel format, version 1

This specifies the format implemented by dpegfs 0.1.0. Versioning applies to the decoded pixel stream, independently of the PNG encoder's compression decisions. Unknown versions are rejected. There is no automatic migration yet.

## Outer image and byte mapping

The container is a static, 8-bit, truecolor RGB PNG (PNG color type 2). The writer uses no alpha channel, palette, animation, text chunks, private chunks or data appended after the PNG. Interlaced RGB8 PNGs can be read if their decoded pixels satisfy this specification; the writer emits non-interlaced images.

Decode PNG to unmodified RGB samples. Read pixels left to right, top to bottom, with channels in R, G, B order. PNG filtering and DEFLATE are undone by the PNG decoder, not by dpegfs itself.

For pixel `(x, y)` and zero-based channel `c` (R = 0, G = 1, B = 2):

```text
stream_offset = 3 × (y × width + x) + c
capacity = width × height × 3
```

The decoded stream is:

```text
[128-byte header][UTF-8 JSON manifest][contiguous file payloads][zero padding]
```

All multibyte integers in the **dpegfs header** are unsigned little-endian. PNG's own chunk fields retain their normal PNG byte order. There is no alignment padding between the dpegfs sections or files.

## Header

Ranges use an exclusive end offset.

| Byte range | Length | Contents |
| --- | ---: | --- |
| `0..8` | 8 | Magic: `44 50 45 47 46 53 00 00` (`DPEGFS\0\0`) |
| `8..12` | 4 | Format version: `1` |
| `12..16` | 4 | Header length: `128` |
| `16..20` | 4 | Width; must equal PNG width |
| `20..24` | 4 | Height; must equal PNG height |
| `24..32` | 8 | Manifest byte length |
| `32..40` | 8 | Total encoded file payload byte length |
| `40..48` | 8 | File count |
| `48..80` | 32 | Binary SHA-256 of manifest followed by all encoded payloads |
| `80..112` | 32 | Binary SHA-256 of header bytes `0..80` |
| `112..128` | 16 | Reserved; all zero |

Define `payload_start = 128 + manifest_length` and `used = payload_start + payload_length`. All additions must be checked for overflow. `used` must not exceed RGB capacity. Padding from `used` to capacity must be zero. Reserved bytes and zero padding are validated separately from the hashes.

For an empty disk, the manifest is exactly `{"files":[]}` (12 bytes), so used capacity is 140 bytes. Since capacity is a multiple of three, a realizable empty image needs at least 141 bytes.

## Manifest

The manifest uses the compact representation produced by the typed serde JSON serializer in this repository. It has no insignificant whitespace or extra fields. Top-level and entry field order is fixed as shown below. UTF-8 characters are written directly; quotes and required JSON escapes follow serde_json's string representation. The decoder reserializes the manifest and requires an exact byte match. This is a format-specific canonical encoding, not a claim of RFC 8785 compliance.

This display is pretty-printed for readability; stored JSON has no extra whitespace:

```json
{
  "files": [
    {
      "path": "notes/hello.txt",
      "offset": 0,
      "stored_bytes": 3,
      "original_bytes": 3,
      "encoding": "raw",
      "sha256": "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    }
  ]
}
```

The example's payload is `abc`. The SHA-256 string always contains exactly 64 lowercase hexadecimal characters and describes the **original, uncompressed** file.

- Entries are sorted by UTF-8 path byte order, strictly increasing; paths are unique.
- Paths have 1–1024 UTF-8 bytes and use `/` as an implicit directory separator. Empty components, `.`/`..`, control characters, `\`, and `:` are forbidden. Leading or trailing `/` is invalid.
- A path cannot also be a directory prefix of another file. Empty directories are not represented. Unicode normalization is not applied, and names are case-sensitive.
- Offsets are relative to the payload section, not the full pixel stream.
- The first offset is zero; each subsequent offset equals the previous offset plus its stored length. Zero-length files can share an offset. Files never overlap or leave holes, and the final end equals the header payload length.
- `raw` stores exactly `original_bytes` unchanged bytes, so the two sizes must be equal.
- `zlib` stores one complete zlib stream, including its wrapper/checksum. Decoding must reach stream end, consume the full encoded payload without trailing bytes and produce exactly `original_bytes`. A missing trailer or larger expansion is an error. The writer chooses this encoding only if it reduces file size.
- Empty files are written as `raw`, length zero, with SHA-256 of the empty byte string.
- Unknown fields, unknown encodings, missing fields and duplicate JSON fields are rejected.

The body checksum detects damage before metadata is trusted. File digests additionally check decompression and extraction. Neither is a keyed MAC or digital signature: someone who can rewrite the image can recompute these hashes.

## v1 implementation limits

| Resource | Limit |
| --- | ---: |
| Width / height | 1–16,384 pixels each |
| Decoded RGB stream | 256 MiB |
| Encoded PNG file length | 272 MiB |
| Manifest | 16 MiB |
| Files | 10,000 |
| Original size of one file | 256 MiB |
| PNG decoder internal allocation budget | 32 MiB, in addition to application buffers |

The implementation validates image dimensions before allocating the decoded RGB buffer and bounds each file's decompression. It still uses memory proportional to image size and allocates separate candidate/payload buffers during writes. The limits are practical resource bounds, not a constant-memory guarantee or a total runtime limit for hostile inputs.

## Save and locking protocol

1. Normalize the image's parent directory and acquire the OS exclusive lock on `<image>.dpegfs.lock` for a writable handle. Read-only handles need no lock.
2. Decode and validate the existing image, or construct an empty state for creation.
3. Apply a mutation to a candidate state. Serialize its manifest and verify that header + manifest + encoded files fit the fixed image dimensions.
4. Write a complete PNG to a temporary file in the same host directory. Flush and `sync_all` the staged file.
5. Publish with atomic replacement; creation and ordinary extraction instead use a no-clobber publication unless overwrite was explicitly requested.
6. Adopt the candidate state in memory only after publication succeeds.

The sidecar is an advisory coordination mechanism for dpegfs writers and contains no filesystem data. Never unlink it while writers may be running; processes must lock the same inode. The OS releases the lock on handle close or process exit. No fallible durability step follows successful publication, so an error is not misreported as a rollback after a completed rename.

The v1 protocol does not fsync the parent directory and does not provide a journal, recovery log, multi-operation transactions, error correction or full power-loss guarantees. Each operation rewrites the entire PNG. Success makes that operation visible; successive `put` calls are separate commits. Atomicity relies on the host filesystem's rename/publication semantics. Concurrent changes by software that ignores the sidecar are unsupported.

## References

- [PNG specification](https://www.w3.org/TR/png/) — outer lossless image representation.
- [RFC 1950: ZLIB Compressed Data Format](https://www.rfc-editor.org/rfc/rfc1950) — per-file `zlib` encoding.
- [Implementation](../src/codec.rs) and [format validation tests](../src/codec/tests.rs).

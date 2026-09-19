# Sidexis Import Hash: Behavior Specification

## Summary

When Sidexis imports a DICOM image, it computes a **SHA-1** hash of the image's pixel data and reports it as a 40-character uppercase hexadecimal string.

The hash is computed over a **reconstructed pixel buffer**, not the raw Pixel Data element bytes. Two details make the hash easy to get wrong:

1. For 8-bit RGB images, the **red and blue channels are swapped** before hashing (BGR order).
2. For 8-bit RGB images, **each row is padded to a 4-byte (DWORD) boundary** with `0x00` bytes, matching a Windows DIB (Device Independent Bitmap) memory layout.

---

## What is hashed

**Input file:** the DICOM file as it arrives at Sidexis, before import.

**Hashed data:** the decoded pixel array from the Pixel Data element `(7FE0,0010)`, reconstructed into a layout-specific byte sequence (see below). Not hashed: the element tag, the VR and length fields, the rest of the dataset, the file meta information, any DICOM trailing pad byte, and any other element.

**Algorithm:** SHA-1 over the byte sequence defined below. The result is 20 bytes, written as 40 uppercase hexadecimal characters.

---

## Byte sequence by pixel layout

### 8-bit, 3 samples per pixel, RGB, interleaved (PlanarConfiguration = 0)

1. Decode the pixel array from the Pixel Data value: `Rows` rows of `Columns` pixels, each pixel being 3 bytes (R, G, B).
2. Reconstruct into a **Windows DIB memory layout**:
   a. In every pixel, swap the first and third byte, so `R, G, B` becomes `B, G, R`. The middle byte is unchanged.
   b. After each row of `Columns × 3` BGR bytes, append `pad_per_row` zero bytes to reach a 4-byte boundary: `pad_per_row = (4 - (Columns × 3) % 4) % 4`.
   c. Rows are stored **top-to-bottom** (no vertical flip).
3. The resulting byte sequence has length `Rows × (Columns × 3 + pad_per_row)`.
4. Hash this sequence with SHA-1.
5. The DICOM trailing pad byte (if present in the Pixel Data element) is **not** included in the hash.

### 16-bit, 1 sample per pixel, MONOCHROME2

Hash the Pixel Data value exactly as stored. There is no channel swap, no byte-order change, no row padding and no conversion. A 16-bit image always has an even byte count, so there is no pad byte.

### Other layouts

See "Coverage" below. They have not been verified.

---

## The pad byte (correction)

Earlier analysis concluded that the DICOM trailing pad byte was part of the hash and required a 256-value sweep for carved files. **This was incorrect.**

The hash is computed from the decoded pixel array reconstructed into Windows DIB layout, not from the raw Pixel Data element value. The DICOM trailing pad byte (appended to make the element length even) is not included in the hashed buffer. Row alignment padding (`0x00` bytes to reach a DWORD boundary) is used instead.

This means:
- The hash is **deterministic** regardless of the DICOM pad byte value.
- No pad byte sweep is needed.
- The same image stored in different DICOM files (with different pad bytes) produces the same hash, as long as the decoded pixel content is identical.

---

## What does not affect the hash

- **Header elements.** Patient name, patient ID, sex, UIDs, dates, and so on are not hashed. Sidexis replaces patient demographics on import, and this has no effect on the hash.
- **File meta information and transfer syntax bytes** are not hashed.
- **DICOM trailing pad byte.** The pad byte appended by the DICOM writer for even-length alignment is not part of the hash.
- **Pixel validity.** The hash is computed over whatever bytes are present, including corrupted or garbage data. It is a fingerprint of what was imported, not an integrity check against an original image. A hash match does not show that an image is intact.
- **Pixel content on import.** In every case observed, Sidexis did not repair, correct or modify pixel data during import.

---

## Why the row padding is easy to miss

When `Columns × 3` is already a multiple of 4, `pad_per_row` is 0 and no padding is added. In that case, the DIB layout and the flat BGR buffer are identical, and an implementation that omits row padding will still produce correct hashes. The error only manifests when `Columns × 3 % 4 ≠ 0`.

For example, with `Columns = 795`: `795 × 3 = 2385`, `2385 % 4 = 1`, so `pad_per_row = 3`. Each row in the hashed buffer is 2388 bytes instead of 2385.

---

## Why the channel swap is easy to miss

If every pixel has R = G = B (a grayscale image stored as RGB), swapping the first and third bytes changes nothing, so the RGB-order and BGR-order byte sequences are identical. An implementation that hashes in RGB order will therefore match on pure gray images and only fail on images containing colour or damaged data. Do not treat matches on gray images as confirmation that the channel order is correct.

---

## Implementation checklist

1. Read the raw Pixel Data element value from the DICOM file.
2. Determine the layout from `SamplesPerPixel`, `BitsAllocated`, `PlanarConfiguration`, `Rows` and `Columns`.
3. For 8-bit RGB with `PlanarConfiguration = 0`:
   a. Decode the pixel array: `Rows × Columns` pixels, each 3 bytes.
   b. Swap bytes 1 and 3 of every 3-byte pixel (`R, G, B` → `B, G, R`).
   c. Compute `pad_per_row = (4 - (Columns × 3) % 4) % 4`.
   d. Build the output buffer: for each row, append the `Columns × 3` BGR bytes followed by `pad_per_row` zero bytes.
   e. The DICOM trailing pad byte is excluded.
4. For 16-bit single-sample images, use the Pixel Data value as stored.
5. Compute SHA-1 and format it as 40 uppercase hex characters.

---

## Coverage

| Layout | Status |
|---|---|
| 8-bit RGB, interleaved (PlanarConfiguration 0), uncompressed, Explicit VR Little Endian | Confirmed (Windows DIB row-stride alignment) |
| 16-bit MONOCHROME2, 1 sample per pixel, uncompressed, Explicit VR Little Endian | Confirmed |
| RGB with PlanarConfiguration 1 (planar) | Not verified |
| YBR photometric interpretations | Not verified |
| 8-bit monochrome | Not verified |
| 16-bit RGB | Not verified |
| Multi-frame images | Not verified |
| Compressed or encapsulated transfer syntaxes | Not verified |
| Implicit VR Little Endian | Not verified (likely identical) |

For any unverified layout, do not assume the rules above apply. Confirm them with a test import before relying on them.

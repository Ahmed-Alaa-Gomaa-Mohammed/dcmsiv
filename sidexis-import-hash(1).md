# Sidexis Import Hash: Behavior Specification

## Summary

When Sidexis imports a DICOM image, it computes a **SHA-1** hash of the image's pixel data and reports it as a 40-character uppercase hexadecimal string.

Only the value of the Pixel Data element `(7FE0,0010)` is hashed. Nothing else in the file is included. Two details make the hash easy to get wrong:

1. For 8-bit RGB images, the **red and blue channels are swapped** before hashing (BGR order).
2. The **trailing pad byte** is included in the hash, exactly as it appears in the file being imported.

---

## What is hashed

**Input file:** the DICOM file as it arrives at Sidexis, before import. Do not use the copy that Sidexis stores afterwards (see "The pad byte" below).

**Hashed data:** the value field of the Pixel Data element `(7FE0,0010)`, including any trailing pad byte. Not hashed: the element tag, the VR and length fields, the rest of the dataset, the file meta information, and any other element.

**Algorithm:** SHA-1 over the byte sequence defined below. The result is 20 bytes, written as 40 uppercase hexadecimal characters.

---

## Byte sequence by pixel layout

### 8-bit, 3 samples per pixel, RGB, interleaved (PlanarConfiguration = 0)

1. Take the pixel bytes: the first `Rows × Columns × 3` bytes of the Pixel Data value.
2. Treat them as consecutive 3-byte pixels. In every pixel, swap the first and third byte, so `R, G, B` becomes `B, G, R`. The middle byte is unchanged.
3. Keep pixel order and row order exactly as stored. There is no vertical flip, no row padding and no stride alignment.
4. Append whatever bytes follow the pixel bytes in the Pixel Data value. This is the pad byte, and it is appended as stored, unswapped and unmodified.
5. Hash the resulting sequence with SHA-1.

### 16-bit, 1 sample per pixel, MONOCHROME2

Hash the Pixel Data value exactly as stored. There is no channel swap, no byte-order change and no conversion. A 16-bit image always has an even byte count, so there is no pad byte.

### Other layouts

See "Coverage" below. They have not been verified.

---

## The pad byte

DICOM requires every element value to have an even length. When the real pixel data has an odd length, the writer appends one extra byte. For 8-bit RGB this happens whenever `Rows × Columns` is odd, because `Rows × Columns × 3` is then odd.

- **The pad byte is part of the hash.** Do not trim the value down to `Rows × Columns × SamplesPerPixel × BytesPerSample` before hashing.
- **The pad byte used is the one in the incoming file.** Well-behaved writers use `0x00`, but files that were carved or recovered from disk can have an arbitrary non-zero value there.
- **Sidexis rewrites the pad byte to `0x00` in its stored copy.** The hash is computed from the incoming file, before that rewrite. So hashing the stored copy reproduces the reported hash only if the incoming pad byte was already `0x00`. If it wasn't, the stored copy hashes differently even though the pixel content is otherwise identical.
- **If only the stored copy is available**, the original pad byte can be recovered. Hash the pixel bytes with each of the 256 possible pad values and compare each result against the hash Sidexis reported.

---

## What does not affect the hash

- **Header elements.** Patient name, patient ID, sex, UIDs, dates, and so on are not hashed. Sidexis replaces patient demographics on import, and this has no effect on the hash.
- **File meta information and transfer syntax bytes** are not hashed.
- **Pixel validity.** The hash is computed over whatever bytes are present, including corrupted or garbage data. It is a fingerprint of what was imported, not an integrity check against an original image. A hash match does not show that an image is intact.
- **Pixel content on import.** In every case observed, Sidexis did not repair, correct or modify pixel data during import. The pad byte in the stored copy was the only pixel-data difference.

---

## Why the channel swap is easy to miss

If every pixel has R = G = B (a grayscale image stored as RGB), swapping the first and third bytes changes nothing, so the RGB-order and BGR-order byte sequences are identical. An implementation that hashes in RGB order will therefore match on pure gray images and only fail on images containing colour or damaged data. Do not treat matches on gray images as confirmation that the channel order is correct.

---

## Implementation checklist

1. Read the raw Pixel Data element value from the incoming (pre-import) file, keeping the trailing pad byte.
2. Determine the layout from `SamplesPerPixel`, `BitsAllocated`, `PlanarConfiguration`, `Rows` and `Columns`.
3. For 8-bit RGB with `PlanarConfiguration = 0`, swap bytes 1 and 3 of every 3-byte pixel across the first `Rows × Columns × 3` bytes. Leave any trailing bytes untouched. If swapping in chunks, chunk on 3-byte boundaries.
4. For 16-bit single-sample images, use the value as stored.
5. Compute SHA-1 and format it as 40 uppercase hex characters.
6. Compare it with the Sidexis-reported hash. If the incoming pad byte is unavailable, sweep the 256 pad values as described above.

---

## Coverage

| Layout | Status |
|---|---|
| 8-bit RGB, interleaved (PlanarConfiguration 0), uncompressed, Explicit VR Little Endian | Confirmed |
| 16-bit MONOCHROME2, 1 sample per pixel, uncompressed, Explicit VR Little Endian | Confirmed |
| RGB with PlanarConfiguration 1 (planar) | Not verified |
| YBR photometric interpretations | Not verified |
| 8-bit monochrome | Not verified |
| 16-bit RGB | Not verified |
| Multi-frame images | Not verified |
| Compressed or encapsulated transfer syntaxes | Not verified |
| Implicit VR Little Endian | Not verified (likely identical) |

For any unverified layout, do not assume the rules above apply. Confirm them with a test import before relying on them.

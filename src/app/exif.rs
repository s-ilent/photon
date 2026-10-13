//! Exif tag table for the Image Information window (Roadmap §9).
//!
//! The window hands [`parse`] the raw TIFF-structured Exif block from
//! `app/image_info.rs` (PNG `eXIf` / JPEG APP1 / WebP EXIF, no `Exif\0\0`
//! prefix); the block is read as a TIFF container and each field becomes a
//! display row. Malformed input yields no rows instead of an error.

/// One tag row of the Exif table: the IFD holding the tag, its name, and its
/// human-readable value.
pub(crate) struct ExifRow {
    pub ifd: &'static str,
    pub tag: String,
    pub value: String,
}

/// Row cap, so a hostile file cannot balloon the window body.
const MAX_ROWS: usize = 200;

/// Parse a raw TIFF-structured Exif block into display rows. Malformed or
/// truncated input returns an empty `Vec`; never panics.
pub(crate) fn parse(block: &[u8]) -> Vec<ExifRow> {
    // The block is a bare TIFF structure, which `read_from_container`
    // detects by its byte-order mark.
    let mut cursor = std::io::Cursor::new(block);
    let Ok(exif) = exif::Reader::new().read_from_container(&mut cursor) else {
        return Vec::new();
    };
    exif.fields()
        .take(MAX_ROWS)
        .map(|field| {
            let value = field.display_value().with_unit(&exif).to_string();
            ExifRow {
                ifd: ifd_name(field),
                tag: field.tag.to_string(),
                value: if value.is_empty() {
                    format!("{:?}", field.value)
                } else {
                    value
                },
            }
        })
        .collect()
}

/// IFD label for a field: sub-IFDs are identified by the tag context, the
/// primary/thumbnail pair by index (`kamadak` numbers sub-IFD fields after
/// their parent, so the context is the only discriminator).
fn ifd_name(field: &exif::Field) -> &'static str {
    use exif::{Context, In};
    match field.tag.context() {
        Context::Gps => "GPS",
        Context::Exif => "Exif",
        Context::Interop => "Interoperability",
        // `Context` is non-exhaustive; TIFF-context and future variants fall
        // back to the IFD index.
        _ => match field.ifd_num {
            In::PRIMARY => "IFD0",
            In::THUMBNAIL => "IFD1",
            // A chained IFD beyond the primary/thumbnail pair.
            _ => "IFD",
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal little-endian TIFF block with two IFD0 entries: Make (ASCII,
    /// stored at an offset) and Orientation (SHORT 6, inline).
    fn tiff_block() -> Vec<u8> {
        let mut block = Vec::new();
        block.extend_from_slice(b"II\x2a\x00"); // little-endian TIFF magic
        block.extend_from_slice(&8u32.to_le_bytes()); // IFD offset
        block.extend_from_slice(&2u16.to_le_bytes()); // entry count

        // Make (0x010f), ASCII, 8 bytes at offset 38.
        block.extend_from_slice(&0x010fu16.to_le_bytes());
        block.extend_from_slice(&2u16.to_le_bytes());
        block.extend_from_slice(&8u32.to_le_bytes());
        block.extend_from_slice(&38u32.to_le_bytes());
        // Orientation (0x0112), SHORT, 1 value, inline 6.
        block.extend_from_slice(&0x0112u16.to_le_bytes());
        block.extend_from_slice(&3u16.to_le_bytes());
        block.extend_from_slice(&1u32.to_le_bytes());
        block.extend_from_slice(&6u16.to_le_bytes());
        block.extend_from_slice(&0u16.to_le_bytes()); // inline value padding
        block.extend_from_slice(&0u32.to_le_bytes()); // next IFD = none
        block.extend_from_slice(b"TestCam\x00");
        block
    }

    #[test]
    fn parses_make_and_orientation() {
        let rows = parse(&tiff_block());
        let make = rows.iter().find(|r| r.tag == "Make").expect("Make row");
        assert_eq!(make.ifd, "IFD0");
        // kamadak's default display quotes ASCII values.
        assert_eq!(make.value, "\"TestCam\"");

        let orientation = rows
            .iter()
            .find(|r| r.tag == "Orientation")
            .expect("Orientation row");
        assert_eq!(orientation.ifd, "IFD0");
        // kamadak renders SHORT 6 as a phrase rather than the bare number.
        assert_eq!(orientation.value, "row 0 at right and column 0 at top");
    }

    #[test]
    fn truncated_block_yields_no_rows() {
        let block = tiff_block();
        assert!(parse(&block[..6]).is_empty());
    }

    #[test]
    fn garbage_block_yields_no_rows() {
        assert!(parse(b"not a tiff").is_empty());
    }
}

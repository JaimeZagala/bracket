//! Text encodings: detection, decoding and exact re-encoding.
//!
//! The rule is "no data loss": a file is only editable if encoding its decoded text again
//! reproduces exactly the original bytes. Otherwise it opens read-only. Saving never replaces
//! characters the target encoding cannot represent; it fails instead.
//!
//! Detection order: byte order mark, UTF-16 without BOM (by the pattern of zero bytes), valid
//! UTF-8, then a guess among legacy encodings with `chardetng`.

use std::borrow::Cow;
use std::fmt;

use encoding_rs::{Encoding, UTF_8, UTF_16BE, UTF_16LE};

/// How much of a file is used to guess a legacy encoding or detect UTF-16 without BOM.
const SAMPLE_LEN: usize = 1 << 20;

/// An encoding plus whether the file starts with a byte order mark.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextEncoding {
    encoding: &'static Encoding,
    bom: bool,
}

impl TextEncoding {
    pub const UTF8: Self = Self {
        encoding: UTF_8,
        bom: false,
    };
    pub const UTF8_BOM: Self = Self {
        encoding: UTF_8,
        bom: true,
    };
    pub const UTF16LE: Self = Self {
        encoding: UTF_16LE,
        bom: true,
    };
    pub const UTF16BE: Self = Self {
        encoding: UTF_16BE,
        bom: true,
    };

    /// `bom` is only kept for UTF-8 and UTF-16; other encodings have none.
    pub fn new(encoding: &'static Encoding, bom: bool) -> Self {
        let unicode = encoding == UTF_8 || encoding == UTF_16LE || encoding == UTF_16BE;
        Self {
            encoding,
            bom: bom && unicode,
        }
    }

    pub fn encoding(self) -> &'static Encoding {
        self.encoding
    }

    pub fn has_bom(self) -> bool {
        self.bom
    }

    pub fn is_utf8(self) -> bool {
        self.encoding == UTF_8
    }

    fn is_utf16(self) -> bool {
        self.encoding == UTF_16LE || self.encoding == UTF_16BE
    }

    /// Short technical name for the status bar and menus, with no words to translate:
    /// `UTF-8`, `UTF-8 BOM`, `UTF-16 LE BOM`, `UTF-16 LE` (no BOM), `windows-1252`...
    pub fn label(self) -> String {
        let base = match self.encoding.name() {
            "UTF-16LE" => "UTF-16 LE",
            "UTF-16BE" => "UTF-16 BE",
            other => other,
        };
        if self.bom {
            format!("{base} BOM")
        } else {
            base.to_owned()
        }
    }

    /// Configuration code: `utf-8`, `utf-8-bom`, `utf-16le`, `utf-16be`, or any WHATWG label.
    pub fn from_code(code: &str) -> Option<Self> {
        match code.trim().to_ascii_lowercase().as_str() {
            "utf-8" | "utf8" => Some(Self::UTF8),
            "utf-8-bom" | "utf8-bom" => Some(Self::UTF8_BOM),
            "utf-16le" => Some(Self::UTF16LE),
            "utf-16be" => Some(Self::UTF16BE),
            other => Encoding::for_label(other.as_bytes()).map(|e| Self::new(e, false)),
        }
    }

    pub fn code(self) -> String {
        match (self.encoding.name(), self.bom) {
            ("UTF-8", false) => "utf-8".to_owned(),
            ("UTF-8", true) => "utf-8-bom".to_owned(),
            (name, _) => name.to_ascii_lowercase(),
        }
    }

    fn bom_bytes(self) -> &'static [u8] {
        match (self.bom, self.encoding.name()) {
            (true, "UTF-8") => &[0xEF, 0xBB, 0xBF],
            (true, "UTF-16LE") => &[0xFF, 0xFE],
            (true, "UTF-16BE") => &[0xFE, 0xFF],
            _ => &[],
        }
    }

    /// Encodes `text`, BOM included. Fails if a character cannot be represented.
    pub fn encode(self, text: &str) -> Result<Vec<u8>, EncodeError> {
        let mut out = self.bom_bytes().to_vec();
        if self.encoding == UTF_16LE {
            out.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
        } else if self.encoding == UTF_16BE {
            out.extend(text.encode_utf16().flat_map(u16::to_be_bytes));
        } else if self.encoding == UTF_8 {
            out.extend_from_slice(text.as_bytes());
        } else {
            let (bytes, _, unmappable) = self.encoding.encode(text);
            if unmappable {
                return Err(EncodeError {
                    encoding: self,
                    character: first_unmappable(self.encoding, text),
                });
            }
            out.extend_from_slice(&bytes);
        }
        Ok(out)
    }
}

/// A character the target encoding cannot represent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EncodeError {
    pub encoding: TextEncoding,
    pub character: Option<char>,
}

impl fmt::Display for EncodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.character {
            Some(c) => write!(
                f,
                "'{c}' (U+{:04X}) cannot be encoded in {}",
                c as u32,
                self.encoding.label()
            ),
            None => write!(f, "the text cannot be encoded in {}", self.encoding.label()),
        }
    }
}

impl std::error::Error for EncodeError {}

fn first_unmappable(encoding: &'static Encoding, text: &str) -> Option<char> {
    let mut buffer = [0u8; 4];
    text.chars()
        .find(|c| encoding.encode(c.encode_utf8(&mut buffer)).2)
}

/// Encodings offered in the "reopen with" and "save with" menus.
pub fn selectable() -> Vec<TextEncoding> {
    let mut list = vec![
        TextEncoding::UTF8,
        TextEncoding::UTF8_BOM,
        TextEncoding::UTF16LE,
        TextEncoding::UTF16BE,
    ];
    for label in [
        "windows-1252",
        "iso-8859-15",
        "windows-1250",
        "windows-1251",
        "windows-1253",
        "windows-1254",
        "koi8-r",
        "shift_jis",
        "euc-jp",
        "gbk",
        "gb18030",
        "big5",
        "euc-kr",
    ] {
        if let Some(encoding) = Encoding::for_label(label.as_bytes()) {
            list.push(TextEncoding::new(encoding, false));
        }
    }
    list
}

/// The result of decoding a file.
#[derive(Debug)]
pub struct Decoded {
    pub text: String,
    pub encoding: TextEncoding,
    /// False when encoding `text` again would not reproduce the original bytes: the file must
    /// then be read-only. `text` may contain replacement characters in that case.
    pub exact: bool,
}

/// Detects the encoding of `bytes` and decodes them. Valid UTF-8, by far the most common case,
/// is turned into the string without copying.
pub fn decode(mut bytes: Vec<u8>) -> Decoded {
    if let Some((encoding, bom_len)) = Encoding::for_bom(&bytes) {
        if encoding == UTF_8 {
            bytes.drain(..bom_len);
            return decode_utf8(bytes, TextEncoding::UTF8_BOM);
        }
        return decode_body(&bytes[bom_len..], TextEncoding::new(encoding, true));
    }
    // Before UTF-8: ASCII text in UTF-16 is also valid UTF-8 (full of NUL characters).
    if let Some(encoding) = utf16_without_bom(&bytes) {
        return decode_body(&bytes, TextEncoding::new(encoding, false));
    }
    match String::from_utf8(bytes) {
        Ok(text) => Decoded {
            text,
            encoding: TextEncoding::UTF8,
            exact: true,
        },
        Err(err) => {
            let bytes = err.into_bytes();
            decode_body(&bytes, TextEncoding::new(guess_legacy(&bytes), false))
        }
    }
}

fn decode_utf8(bytes: Vec<u8>, encoding: TextEncoding) -> Decoded {
    match String::from_utf8(bytes) {
        Ok(text) => Decoded {
            text,
            encoding,
            exact: true,
        },
        Err(err) => Decoded {
            text: String::from_utf8_lossy(err.as_bytes()).into_owned(),
            encoding,
            exact: false,
        },
    }
}

/// Decodes `bytes` as `encoding` (for "reopen with encoding"). A BOM matching the encoding is
/// recognized; otherwise the file is taken as having none.
pub fn decode_as(bytes: &[u8], encoding: TextEncoding) -> Decoded {
    let with_bom = TextEncoding::new(encoding.encoding, true);
    let bom = with_bom.bom_bytes();
    if !bom.is_empty() && bytes.starts_with(bom) {
        decode_body(&bytes[bom.len()..], with_bom)
    } else {
        decode_body(bytes, TextEncoding::new(encoding.encoding, false))
    }
}

/// Decodes the bytes after the BOM and checks that re-encoding reproduces them.
fn decode_body(body: &[u8], encoding: TextEncoding) -> Decoded {
    match encoding
        .encoding
        .decode_without_bom_handling_and_without_replacement(body)
    {
        Some(text) => {
            let exact = encoding.is_utf8()
                || encoding.is_utf16()
                || reencodes_exactly(&text, encoding, body);
            Decoded {
                text: text.into_owned(),
                encoding,
                exact,
            }
        }
        None => {
            let (text, _) = encoding.encoding.decode_without_bom_handling(body);
            Decoded {
                text: text.into_owned(),
                encoding,
                exact: false,
            }
        }
    }
}

fn reencodes_exactly(text: &str, encoding: TextEncoding, original: &[u8]) -> bool {
    let (bytes, _, unmappable): (Cow<'_, [u8]>, _, bool) = encoding.encoding.encode(text);
    !unmappable && bytes.as_ref() == original
}

/// UTF-16 without BOM: text in Latin scripts has a zero byte in almost every other position.
fn utf16_without_bom(bytes: &[u8]) -> Option<&'static Encoding> {
    let sample = &bytes[..bytes.len().min(SAMPLE_LEN)];
    if sample.len() < 4 || !bytes.len().is_multiple_of(2) {
        return None;
    }
    let pairs = sample.len() / 2;
    let zeros_at = |offset: usize| {
        sample
            .iter()
            .skip(offset)
            .step_by(2)
            .filter(|b| **b == 0)
            .count()
    };
    let (even, odd) = (zeros_at(0), zeros_at(1));
    let mostly = |n: usize| n * 10 >= pairs * 4;
    let rarely = |n: usize| n * 20 <= pairs;
    if mostly(odd) && rarely(even) {
        Some(UTF_16LE)
    } else if mostly(even) && rarely(odd) {
        Some(UTF_16BE)
    } else {
        None
    }
}

fn guess_legacy(bytes: &[u8]) -> &'static Encoding {
    let sample = &bytes[..bytes.len().min(SAMPLE_LEN)];
    let mut detector = chardetng::EncodingDetector::new(chardetng::Iso2022JpDetection::Deny);
    detector.feed(sample, sample.len() == bytes.len());
    detector.guess(None, chardetng::Utf8Detection::Deny)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf8_with_and_without_bom() {
        let plain = decode("año".as_bytes().to_vec());
        assert_eq!((plain.encoding, plain.exact), (TextEncoding::UTF8, true));
        let bom = decode(b"\xEF\xBB\xBFa\xC3\xB1".to_vec());
        assert_eq!(bom.encoding, TextEncoding::UTF8_BOM);
        assert_eq!(bom.text, "añ");
    }

    #[test]
    fn utf16_with_bom_round_trips() {
        for encoding in [TextEncoding::UTF16LE, TextEncoding::UTF16BE] {
            let bytes = encoding.encode("Hola, ñ 日本 😀\r\n").unwrap();
            let decoded = decode(bytes.clone());
            assert_eq!(decoded.encoding, encoding);
            assert!(decoded.exact);
            assert_eq!(decoded.encoding.encode(&decoded.text).unwrap(), bytes);
        }
    }

    #[test]
    fn utf16_without_bom_is_detected() {
        let text = "public class Program { }\r\n";
        let le: Vec<u8> = text.encode_utf16().flat_map(u16::to_le_bytes).collect();
        let be: Vec<u8> = text.encode_utf16().flat_map(u16::to_be_bytes).collect();
        let d = decode(le.clone());
        assert_eq!(
            (d.encoding.encoding(), d.encoding.has_bom()),
            (UTF_16LE, false)
        );
        assert_eq!(d.text, text);
        assert_eq!(
            d.encoding.encode(&d.text).unwrap(),
            le,
            "saved without a BOM, as found"
        );
        assert_eq!(decode(be).encoding.encoding(), UTF_16BE);
    }

    #[test]
    fn windows_1252_is_guessed_and_round_trips() {
        let bytes = b"Configuraci\xF3n del \xE1rbol: se\xF1al y m\xE1s cosas en espa\xF1ol";
        let decoded = decode(bytes.to_vec());
        assert_eq!(decoded.encoding.encoding().name(), "windows-1252");
        assert!(decoded.exact);
        assert!(decoded.text.contains("Configuración"));
        assert_eq!(decoded.encoding.encode(&decoded.text).unwrap(), bytes);
    }

    #[test]
    fn malformed_bytes_are_not_exact() {
        // A UTF-8 BOM followed by invalid UTF-8.
        let decoded = decode(b"\xEF\xBB\xBFab\xFFcd".to_vec());
        assert!(!decoded.exact);
        assert!(decoded.text.contains('\u{FFFD}'));
    }

    #[test]
    fn non_round_tripping_legacy_text_is_not_exact() {
        // Shift_JIS has duplicate codes: NEC 0xED40 decodes to U+7E8A, which encodes as 0xFA5C.
        let shift_jis = TextEncoding::new(Encoding::for_label(b"shift_jis").unwrap(), false);
        let decoded = decode_as(b"\xED\x40", shift_jis);
        assert_eq!(decoded.text, "\u{7E8A}");
        assert!(!decoded.exact);
        assert!(
            decode_as(b"\x82\xA0", shift_jis).exact,
            "a regular kana round-trips"
        );
    }

    #[test]
    fn decode_as_reinterprets_the_bytes() {
        let bytes = b"caf\xE9";
        let as_1252 = decode_as(bytes, TextEncoding::from_code("windows-1252").unwrap());
        assert_eq!(as_1252.text, "café");
        assert!(!decode_as(bytes, TextEncoding::UTF8).exact);
    }

    #[test]
    fn unmappable_characters_fail_instead_of_being_replaced() {
        let latin1 = TextEncoding::from_code("windows-1252").unwrap();
        let err = latin1.encode("price: 5 € or 日").unwrap_err();
        assert_eq!(err.character, Some('日'));
        assert_eq!(latin1.encode("price: 5 €").unwrap(), b"price: 5 \x80");
    }

    #[test]
    fn codes_and_labels() {
        assert_eq!(
            TextEncoding::from_code("UTF-8-BOM"),
            Some(TextEncoding::UTF8_BOM)
        );
        assert_eq!(
            TextEncoding::from_code("latin1").unwrap().code(),
            "windows-1252"
        );
        assert_eq!(TextEncoding::UTF8_BOM.code(), "utf-8-bom");
        assert_eq!(TextEncoding::UTF16BE.label(), "UTF-16 BE BOM");
        assert_eq!(TextEncoding::new(UTF_16LE, false).label(), "UTF-16 LE");
        assert_eq!(TextEncoding::from_code("nonsense"), None);
        assert!(selectable().len() >= 15);
        assert!(!TextEncoding::new(Encoding::for_label(b"gbk").unwrap(), true).has_bom());
    }
}

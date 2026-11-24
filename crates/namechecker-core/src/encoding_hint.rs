//! Legacy CJK raw-byte detection and lossless decoding hints (`CP949`, `Shift_JIS`, `GB18030`, `Big5`).

use chardetng::{EncodingDetector, Iso2022JpDetection, Utf8Detection};
use encoding_rs::{BIG5, EUC_KR, Encoding, GB18030, GBK, SHIFT_JIS};

use crate::model::{EncodingHint, RawOsName};

const LEGACY_CJK_CANDIDATES: &[(&str, &Encoding)] = &[
    ("CP949", EUC_KR),
    ("Shift_JIS", SHIFT_JIS),
    ("GB18030", GB18030),
    ("Big5", BIG5),
];

/// Checks if a filename exceeds cross-platform component limits (255 UTF-8 bytes or 255 UTF-16 units).
#[must_use]
pub fn exceeds_cross_platform_name_limit(name: &str) -> (bool, usize, usize) {
    let utf8_bytes = name.len();
    let utf16_units = name.encode_utf16().count();
    (
        utf8_bytes > 255 || utf16_units > 255,
        utf8_bytes,
        utf16_units,
    )
}

/// Detects non-UTF-8 raw OS names or unpaired UTF-16 surrogates and returns ranked decoding hints.
#[must_use]
pub fn detect_legacy_encoding_hints(raw: &RawOsName) -> Vec<EncodingHint> {
    match raw {
        RawOsName::UnixBytes(bytes) => {
            if std::str::from_utf8(bytes).is_ok() {
                return Vec::new();
            }
            let mut detector = EncodingDetector::new(Iso2022JpDetection::Deny);
            detector.feed(bytes, true);
            let top_guess = detector.guess(None, Utf8Detection::Deny);

            let matches_top = |enc: &'static Encoding| -> bool {
                enc == top_guess || (enc == GB18030 && top_guess == GBK)
            };

            let mut ordered: Vec<(&str, &Encoding)> =
                Vec::with_capacity(LEGACY_CJK_CANDIDATES.len());
            for &(label, enc) in LEGACY_CJK_CANDIDATES {
                if matches_top(enc) {
                    ordered.push((label, enc));
                }
            }
            for &(label, enc) in LEGACY_CJK_CANDIDATES {
                if !matches_top(enc) {
                    ordered.push((label, enc));
                }
            }

            let mut hints = Vec::new();
            for (label, enc) in ordered {
                let (decoded, had_errors) = enc.decode_without_bom_handling(bytes);
                if !had_errors {
                    hints.push(EncodingHint {
                        encoding: label.to_string(),
                        decoded_name: decoded.into_owned(),
                    });
                }
            }

            if hints.is_empty() {
                hints.push(EncodingHint {
                    encoding: "RawBytes-Lossy".to_string(),
                    decoded_name: String::from_utf8_lossy(bytes).into_owned(),
                });
            }
            hints
        }
        RawOsName::WindowsWide(units) => {
            if String::from_utf16(units).is_ok() {
                return Vec::new();
            }
            vec![EncodingHint {
                encoding: "UTF-16-Unpaired".to_string(),
                decoded_name: String::from_utf16_lossy(units),
            }]
        }
    }
}

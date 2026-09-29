use std::collections::BTreeSet;

/// G0 has one semantic text model: Unicode Text.
/// Encoding exists only at I/O/storage/interchange boundaries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CoreTextCodec {
    /// Default and preferred interchange codec.
    Utf8,
    /// Explicit interop codecs for ecosystems that require UTF-16.
    Utf16Le,
    Utf16Be,
    /// Explicit interop codecs for systems that require fixed-width Unicode.
    Utf32Le,
    Utf32Be,
}

/// Non-Core codecs are adapters. They never change Text semantics and must
/// convert to/from Unicode Text explicitly at a boundary.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum TextCodec {
    Core(CoreTextCodec),
    Adapter(String),
}

impl TextCodec {
    pub fn utf8() -> Self {
        Self::Core(CoreTextCodec::Utf8)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextNormalization {
    /// Every G0 Text value is canonicalized before it becomes Text.
    Nfc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextSegmentation {
    /// User-facing character boundaries follow extended grapheme clusters.
    ExtendedGraphemeCluster,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnicodeProfile {
    /// Unicode data/rules version selected by the platform profile.
    /// It is deliberately not part of G0 Core semantics.
    pub unicode_version: String,
    pub normalization: TextNormalization,
    pub segmentation: TextSegmentation,
}

impl UnicodeProfile {
    pub fn new(unicode_version: impl Into<String>) -> Self {
        Self {
            unicode_version: unicode_version.into(),
            normalization: TextNormalization::Nfc,
            segmentation: TextSegmentation::ExtendedGraphemeCluster,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GraphemeIndex(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GraphemeRange {
    pub start: GraphemeIndex,
    pub end: GraphemeIndex,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextDecodeError {
    InvalidUtf8 { valid_up_to: usize },
    InvalidUtf16,
    InvalidUtf32(u32),
}

pub fn validate_utf8(bytes: &[u8]) -> Result<&str, TextDecodeError> {
    std::str::from_utf8(bytes).map_err(|error| TextDecodeError::InvalidUtf8 {
        valid_up_to: error.valid_up_to(),
    })
}

pub fn decode_utf16(
    units: impl IntoIterator<Item = u16>,
) -> Result<String, TextDecodeError> {
    char::decode_utf16(units)
        .map(|value| value.map_err(|_| TextDecodeError::InvalidUtf16))
        .collect()
}

pub fn decode_utf32(
    units: impl IntoIterator<Item = u32>,
) -> Result<String, TextDecodeError> {
    units
        .into_iter()
        .map(|value| {
            char::from_u32(value).ok_or(TextDecodeError::InvalidUtf32(value))
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextBoundaryOperation {
    /// Decode a declared boundary codec and normalize into semantic Unicode Text.
    DecodeAndNormalize(TextCodec),
    /// Encode semantic Text into a declared boundary codec.
    Encode(TextCodec),
    /// Slice by user-perceived character boundaries, never raw code units.
    SliceGraphemes,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextContract {
    pub profile: UnicodeProfile,
    /// Default boundary codec. UTF-8 by default.
    pub default_codec: TextCodec,
    /// Closed Core set enabled by the selected platform.
    pub enabled_core_codecs: BTreeSet<CoreTextCodec>,
    /// Optional explicit adapters. These do not become additional Text models.
    pub enabled_adapters: BTreeSet<String>,
    pub allow_silent_replacement: bool,
    pub allow_raw_byte_indexing: bool,
}

impl TextContract {
    pub fn strict(unicode_version: impl Into<String>) -> Self {
        Self {
            profile: UnicodeProfile::new(unicode_version),
            default_codec: TextCodec::utf8(),
            enabled_core_codecs: [CoreTextCodec::Utf8].into_iter().collect(),
            enabled_adapters: BTreeSet::new(),
            allow_silent_replacement: false,
            allow_raw_byte_indexing: false,
        }
    }

    pub fn enable_core_codec(&mut self, codec: CoreTextCodec) {
        self.enabled_core_codecs.insert(codec);
    }

    pub fn enable_adapter(&mut self, id: impl Into<String>) {
        self.enabled_adapters.insert(id.into());
    }

    pub fn codec_enabled(&self, codec: &TextCodec) -> bool {
        match codec {
            TextCodec::Core(codec) => self.enabled_core_codecs.contains(codec),
            TextCodec::Adapter(id) => self.enabled_adapters.contains(id),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextContractIssue {
    SilentReplacementEnabled,
    RawByteIndexingEnabled,
    DefaultCodecDisabled,
    NonCanonicalNormalization,
    NonGraphemeSegmentation,
}

pub fn validate_text_contract(
    contract: &TextContract,
) -> Result<(), Vec<TextContractIssue>> {
    let mut issues = Vec::new();

    if contract.allow_silent_replacement {
        issues.push(TextContractIssue::SilentReplacementEnabled);
    }
    if contract.allow_raw_byte_indexing {
        issues.push(TextContractIssue::RawByteIndexingEnabled);
    }
    if !contract.codec_enabled(&contract.default_codec) {
        issues.push(TextContractIssue::DefaultCodecDisabled);
    }
    if contract.profile.normalization != TextNormalization::Nfc {
        issues.push(TextContractIssue::NonCanonicalNormalization);
    }
    if contract.profile.segmentation
        != TextSegmentation::ExtendedGraphemeCluster
    {
        issues.push(TextContractIssue::NonGraphemeSegmentation);
    }

    if issues.is_empty() {
        Ok(())
    } else {
        Err(issues)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_has_one_unicode_text_model_with_utf8_default() {
        let contract = TextContract::strict("platform-selected");
        assert_eq!(contract.default_codec, TextCodec::utf8());
        assert_eq!(
            contract.profile.normalization,
            TextNormalization::Nfc
        );
        assert_eq!(
            contract.profile.segmentation,
            TextSegmentation::ExtendedGraphemeCluster
        );
        assert!(validate_text_contract(&contract).is_ok());
    }

    #[test]
    fn utf16_can_be_enabled_without_creating_another_text_type() {
        let mut contract = TextContract::strict("platform-selected");
        contract.enable_core_codec(CoreTextCodec::Utf16Le);

        assert!(contract.codec_enabled(&TextCodec::Core(
            CoreTextCodec::Utf16Le
        )));
        assert!(validate_text_contract(&contract).is_ok());
    }

    #[test]
    fn explicit_external_codec_adapter_is_possible() {
        let mut contract = TextContract::strict("platform-selected");
        contract.enable_adapter("legacy.vendor.codec");

        assert!(contract.codec_enabled(&TextCodec::Adapter(
            "legacy.vendor.codec".into()
        )));
    }

    #[test]
    fn invalid_utf8_is_an_error_not_silent_replacement() {
        let invalid = [0xf0, 0x28, 0x8c, 0x28];
        assert!(matches!(
            validate_utf8(&invalid),
            Err(TextDecodeError::InvalidUtf8 { .. })
        ));
    }

    #[test]
    fn full_unicode_utf8_is_accepted() {
        let value = "A🙂العربية日本語";
        assert_eq!(validate_utf8(value.as_bytes()).unwrap(), value);
    }

    #[test]
    fn utf16_decodes_into_same_unicode_text_semantics() {
        let utf16: Vec<u16> = "A🙂日本語".encode_utf16().collect();
        assert_eq!(decode_utf16(utf16).unwrap(), "A🙂日本語");
    }

    #[test]
    fn ascii_requires_no_separate_mode() {
        let ascii = b"plain ASCII";
        assert_eq!(validate_utf8(ascii).unwrap(), "plain ASCII");
    }

    #[test]
    fn byte_indexing_cannot_be_enabled_in_core_contract() {
        let mut contract = TextContract::strict("platform-selected");
        contract.allow_raw_byte_indexing = true;

        assert_eq!(
            validate_text_contract(&contract),
            Err(vec![TextContractIssue::RawByteIndexingEnabled])
        );
    }
}

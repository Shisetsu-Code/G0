#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextEncoding {
    /// The only Core text encoding.
    Utf8,
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
    pub encoding: TextEncoding,
    pub normalization: TextNormalization,
    pub segmentation: TextSegmentation,
}

impl UnicodeProfile {
    pub fn new(unicode_version: impl Into<String>) -> Self {
        Self {
            unicode_version: unicode_version.into(),
            encoding: TextEncoding::Utf8,
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
}

pub fn validate_utf8(bytes: &[u8]) -> Result<&str, TextDecodeError> {
    std::str::from_utf8(bytes).map_err(|error| TextDecodeError::InvalidUtf8 {
        valid_up_to: error.valid_up_to(),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextBoundaryOperation {
    /// Construct semantic Text from UTF-8 and canonicalize it to the Core normalization form.
    DecodeAndNormalize,
    /// Encode semantic Text for interchange/storage. Always UTF-8.
    EncodeUtf8,
    /// Slice by user-perceived character boundaries, never raw code units.
    SliceGraphemes,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextContract {
    pub profile: UnicodeProfile,
    pub allow_silent_replacement: bool,
    pub allow_raw_byte_indexing: bool,
}

impl TextContract {
    pub fn strict(unicode_version: impl Into<String>) -> Self {
        Self {
            profile: UnicodeProfile::new(unicode_version),
            allow_silent_replacement: false,
            allow_raw_byte_indexing: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextContractIssue {
    SilentReplacementEnabled,
    RawByteIndexingEnabled,
    NonUtf8Encoding,
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
    if contract.profile.encoding != TextEncoding::Utf8 {
        issues.push(TextContractIssue::NonUtf8Encoding);
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
    fn core_has_one_text_model() {
        let contract = TextContract::strict("platform-selected");
        assert_eq!(contract.profile.encoding, TextEncoding::Utf8);
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
        assert_eq!(
            validate_utf8(value.as_bytes()).unwrap(),
            value
        );
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

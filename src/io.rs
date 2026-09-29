use crate::text::TextCodec;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IoSourceKind {
    File,
    Network,
    Store,
    Memory,
    Device,
    Process,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IoValueKind {
    Bytes,
    Text,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IoTransform {
    /// All raw I/O enters G0 as Bytes.
    ReadBytes,
    /// Explicit boundary conversion from Bytes to semantic Unicode Text.
    DecodeText(TextCodec),
    /// Explicit boundary conversion from semantic Unicode Text to Bytes.
    EncodeText(TextCodec),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IoPipeline {
    pub source: IoSourceKind,
    pub transforms: Vec<IoTransform>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IoPipelineIssue {
    TextWithoutDecode,
    DuplicateDecode,
    EncodeBeforeTextExists,
}

pub fn validate_io_pipeline(
    pipeline: &IoPipeline,
    expected: IoValueKind,
) -> Result<(), Vec<IoPipelineIssue>> {
    let mut issues = Vec::new();
    let mut kind = IoValueKind::Bytes;
    let mut decoded = false;

    for transform in &pipeline.transforms {
        match transform {
            IoTransform::ReadBytes => {
                kind = IoValueKind::Bytes;
                decoded = false;
            }
            IoTransform::DecodeText(_) => {
                if decoded {
                    issues.push(IoPipelineIssue::DuplicateDecode);
                }
                kind = IoValueKind::Text;
                decoded = true;
            }
            IoTransform::EncodeText(_) => {
                if kind != IoValueKind::Text {
                    issues.push(IoPipelineIssue::EncodeBeforeTextExists);
                } else {
                    kind = IoValueKind::Bytes;
                    decoded = false;
                }
            }
        }
    }

    if expected == IoValueKind::Text && kind != IoValueKind::Text {
        issues.push(IoPipelineIssue::TextWithoutDecode);
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
    use crate::text::{CoreTextCodec, TextCodec};

    #[test]
    fn raw_io_is_bytes_until_explicitly_decoded() {
        let pipeline = IoPipeline {
            source: IoSourceKind::File,
            transforms: vec![IoTransform::ReadBytes],
        };

        assert_eq!(
            validate_io_pipeline(&pipeline, IoValueKind::Text),
            Err(vec![IoPipelineIssue::TextWithoutDecode])
        );
    }

    #[test]
    fn every_source_uses_the_same_text_decode_operation() {
        for source in [
            IoSourceKind::File,
            IoSourceKind::Network,
            IoSourceKind::Store,
            IoSourceKind::Memory,
            IoSourceKind::Device,
            IoSourceKind::Process,
        ] {
            let pipeline = IoPipeline {
                source,
                transforms: vec![
                    IoTransform::ReadBytes,
                    IoTransform::DecodeText(TextCodec::Core(
                        CoreTextCodec::Utf8,
                    )),
                ],
            };

            assert!(
                validate_io_pipeline(&pipeline, IoValueKind::Text).is_ok()
            );
        }
    }

    #[test]
    fn utf16_is_just_another_boundary_codec_not_another_string_type() {
        let pipeline = IoPipeline {
            source: IoSourceKind::File,
            transforms: vec![
                IoTransform::ReadBytes,
                IoTransform::DecodeText(TextCodec::Core(
                    CoreTextCodec::Utf16Le,
                )),
            ],
        };

        assert!(validate_io_pipeline(&pipeline, IoValueKind::Text).is_ok());
    }
}

use super::outputs::{Action, AnnexResult};
use super::*;
use bytes::Bytes;
use relative_path::RelativePathBuf;
use serde::Deserialize;
use url::Url;

pub(crate) struct AddURLInput {
    pub(crate) url: Url,
    pub(crate) path: RelativePathBuf,
}

impl AnnexInput for AddURLInput {
    type Error = std::io::Error;

    fn for_input(&self) -> Result<Bytes, Self::Error> {
        Ok(Bytes::from(format!("{} {}", self.url, self.path)))
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(untagged)]
pub(crate) enum AddURLOutput {
    #[serde(rename_all = "kebab-case")]
    Progress {
        byte_progress: usize,
        total_size: Option<usize>,
        percent_progress: Option<String>,
        action: Action,
    },
    Completion {
        #[serde(default)]
        key: Option<String>,
        #[serde(flatten)]
        action: Action,
        #[serde(flatten)]
        result: AnnexResult,
        #[serde(default)]
        note: Option<String>,
    },
}

impl AddURLOutput {
    pub(crate) fn file(&self) -> &Option<RelativePathBuf> {
        &match self {
            AddURLOutput::Progress { action, .. } => action,
            AddURLOutput::Completion { action, .. } => action,
        }
        .file
    }

    pub(crate) fn check(self) -> Result<Self, AnnexError> {
        match self {
            AddURLOutput::Progress { .. } => Ok(self),
            AddURLOutput::Completion { ref result, .. } => {
                if result.success {
                    Ok(self)
                } else {
                    Err(AnnexError(result.error_messages.clone()))
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_addurl_output_success() {
        let s = r#"{"key":"MD5E-s3405224--dd15380fc1b27858f647a30cc2399a52.pdf","command":"addurl","file":"programming/gameboy.pdf","input":["https://archive.org/download/GameBoyProgManVer1.1/GameBoyProgManVer1.1.pdf programming/gameboy.pdf"],"success":true,"error-messages":[],"note":"to programming/gameboy.pdf"}"#;
        let parsed = serde_json::from_str::<AddURLOutput>(s).unwrap();
        assert_eq!(parsed,
            AddURLOutput::Completion {
                key: Some(String::from("MD5E-s3405224--dd15380fc1b27858f647a30cc2399a52.pdf")),
                action: Action {
                    command: String::from("addurl"),
                    file: Some(RelativePathBuf::from_path("programming/gameboy.pdf").unwrap()),
                    input: vec![String::from("https://archive.org/download/GameBoyProgManVer1.1/GameBoyProgManVer1.1.pdf programming/gameboy.pdf")],
                },
                result: AnnexResult {
                    success: true,
                    error_messages: Vec::new(),
                },
                note: Some(String::from("to programming/gameboy.pdf")),
            }
        )
    }

    #[test]
    fn test_load_addurl_output_success_no_key() {
        let s = r#"{"command":"addurl","file":"text/shakespeare/hamlet.txt","input":["https://gutenberg.org/files/1524/1524-0.txt text/shakespeare/hamlet.txt"],"success":true,"error-messages":[],"note":"to text/shakespeare/hamlet.txt\nnon-large file; adding content to git repository"}"#;
        let parsed = serde_json::from_str::<AddURLOutput>(s).unwrap();
        assert_eq!(parsed,
            AddURLOutput::Completion {
                key: None,
                action: Action {
                    command: String::from("addurl"),
                    file: Some(RelativePathBuf::from_path("text/shakespeare/hamlet.txt").unwrap()),
                    input: vec![String::from("https://gutenberg.org/files/1524/1524-0.txt text/shakespeare/hamlet.txt")],
                },
                result: AnnexResult {
                    success: true,
                    error_messages: Vec::new(),
                },
                note: Some(String::from("to text/shakespeare/hamlet.txt\nnon-large file; adding content to git repository")),
            }
        )
    }

    #[test]
    fn test_load_addurl_output_failure() {
        let s = r#"{"command":"addurl","file":"nexists.pdf","input":["https://www.varonathe.org/nonexistent.pdf nexists.pdf"],"success":false,"error-messages":["  download failed: Not Found"]}"#;
        let parsed = serde_json::from_str::<AddURLOutput>(s).unwrap();
        assert_eq!(
            parsed,
            AddURLOutput::Completion {
                key: None,
                action: Action {
                    command: String::from("addurl"),
                    file: Some(RelativePathBuf::from_path("nexists.pdf").unwrap()),
                    input: vec![String::from(
                        "https://www.varonathe.org/nonexistent.pdf nexists.pdf"
                    )],
                },
                result: AnnexResult {
                    success: false,
                    error_messages: vec![String::from("  download failed: Not Found")],
                },
                note: None,
            }
        )
    }

    #[test]
    fn test_load_addurl_output_progress() {
        let s = r#"{"byte-progress":605788,"total-size":3405224,"percent-progress":"17.79%","action":{"command":"addurl","file":"programming/gameboy.pdf","input":["https://archive.org/download/GameBoyProgManVer1.1/GameBoyProgManVer1.1.pdf programming/gameboy.pdf"]}}"#;
        let parsed = serde_json::from_str::<AddURLOutput>(s).unwrap();
        assert_eq!(parsed,
            AddURLOutput::Progress {
                byte_progress: 605788,
                total_size: Some(3405224),
                percent_progress: Some(String::from("17.79%")),
                action: Action {
                    command: String::from("addurl"),
                    file: Some(RelativePathBuf::from_path("programming/gameboy.pdf").unwrap()),
                    input: vec![String::from("https://archive.org/download/GameBoyProgManVer1.1/GameBoyProgManVer1.1.pdf programming/gameboy.pdf")],
                },
            }
        )
    }

    #[test]
    fn test_load_addurl_output_progress_no_total_null_file() {
        let s = r#"{"byte-progress":8192,"action":{"command":"addurl","file":null,"input":["https://www.httpwatch.com/httpgallery/chunked/chunkedimage.aspx"]}}"#;
        let parsed = serde_json::from_str::<AddURLOutput>(s).unwrap();
        assert_eq!(
            parsed,
            AddURLOutput::Progress {
                byte_progress: 8192,
                total_size: None,
                percent_progress: None,
                action: Action {
                    command: String::from("addurl"),
                    file: None,
                    input: vec![String::from(
                        "https://www.httpwatch.com/httpgallery/chunked/chunkedimage.aspx"
                    )],
                },
            }
        )
    }
}

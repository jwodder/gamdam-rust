use serde::Deserialize;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(untagged)]
pub enum AddurlOutput {
    #[serde(rename_all = "kebab-case")]
    Progress {
        byte_progress: usize,
        total_size: Option<usize>,
        percent_progress: Option<String>,
        action: Action,
    },

    #[serde(rename_all = "kebab-case")]
    Completion {
        #[serde(default)]
        key: Option<String>,

        #[serde(flatten)]
        action: Action,

        success: bool,

        error_messages: Vec<String>,

        #[serde(default)]
        note: Option<String>,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct Action {
    pub command: String,
    // `file` can be `None` for an in-progress download requested without an
    // explicit download path
    pub file: Option<String>,
    pub input: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_addurl_success() {
        let s = r#"{"key":"MD5E-s3405224--dd15380fc1b27858f647a30cc2399a52.pdf","command":"addurl","file":"programming/gameboy.pdf","input":["https://archive.org/download/GameBoyProgManVer1.1/GameBoyProgManVer1.1.pdf programming/gameboy.pdf"],"success":true,"error-messages":[],"note":"to programming/gameboy.pdf"}"#;
        let parsed = serde_json::from_str::<AddurlOutput>(s).unwrap();
        assert_eq!(parsed,
            AddurlOutput::Completion {
                key: Some(String::from("MD5E-s3405224--dd15380fc1b27858f647a30cc2399a52.pdf")),
                action: Action {
                    command: String::from("addurl"),
                    file: Some(String::from("programming/gameboy.pdf")),
                    input: vec![String::from("https://archive.org/download/GameBoyProgManVer1.1/GameBoyProgManVer1.1.pdf programming/gameboy.pdf")],
                },
                success: true,
                error_messages: Vec::new(),
                note: Some(String::from("to programming/gameboy.pdf")),
            }
        )
    }

    #[test]
    fn test_addurl_success_no_key() {
        let s = r#"{"command":"addurl","file":"text/shakespeare/hamlet.txt","input":["https://gutenberg.org/files/1524/1524-0.txt text/shakespeare/hamlet.txt"],"success":true,"error-messages":[],"note":"to text/shakespeare/hamlet.txt\nnon-large file; adding content to git repository"}"#;
        let parsed = serde_json::from_str::<AddurlOutput>(s).unwrap();
        assert_eq!(parsed,
            AddurlOutput::Completion {
                key: None,
                action: Action {
                    command: String::from("addurl"),
                    file: Some(String::from("text/shakespeare/hamlet.txt")),
                    input: vec![String::from("https://gutenberg.org/files/1524/1524-0.txt text/shakespeare/hamlet.txt")],
                },
                success: true,
                error_messages: Vec::new(),
                note: Some(String::from("to text/shakespeare/hamlet.txt\nnon-large file; adding content to git repository")),
            }
        )
    }

    #[test]
    fn test_addurl_failure() {
        let s = r#"{"command":"addurl","file":"nexists.pdf","input":["https://www.varonathe.org/nonexistent.pdf nexists.pdf"],"success":false,"error-messages":["  download failed: Not Found"]}"#;
        let parsed = serde_json::from_str::<AddurlOutput>(s).unwrap();
        assert_eq!(
            parsed,
            AddurlOutput::Completion {
                key: None,
                action: Action {
                    command: String::from("addurl"),
                    file: Some(String::from("nexists.pdf")),
                    input: vec![String::from(
                        "https://www.varonathe.org/nonexistent.pdf nexists.pdf"
                    )],
                },
                success: false,
                error_messages: vec![String::from("  download failed: Not Found")],
                note: None,
            }
        )
    }

    #[test]
    fn test_addurl_progress() {
        let s = r#"{"byte-progress":605788,"total-size":3405224,"percent-progress":"17.79%","action":{"command":"addurl","file":"programming/gameboy.pdf","input":["https://archive.org/download/GameBoyProgManVer1.1/GameBoyProgManVer1.1.pdf programming/gameboy.pdf"]}}"#;
        let parsed = serde_json::from_str::<AddurlOutput>(s).unwrap();
        assert_eq!(parsed,
            AddurlOutput::Progress {
                byte_progress: 605788,
                total_size: Some(3405224),
                percent_progress: Some(String::from("17.79%")),
                action: Action {
                    command: String::from("addurl"),
                    file: Some(String::from("programming/gameboy.pdf")),
                    input: vec![String::from("https://archive.org/download/GameBoyProgManVer1.1/GameBoyProgManVer1.1.pdf programming/gameboy.pdf")],
                },
            }
        )
    }

    #[test]
    fn test_addurl_progress_no_total_null_file() {
        let s = r#"{"byte-progress":8192,"action":{"command":"addurl","file":null,"input":["https://www.httpwatch.com/httpgallery/chunked/chunkedimage.aspx"]}}"#;
        let parsed = serde_json::from_str::<AddurlOutput>(s).unwrap();
        assert_eq!(
            parsed,
            AddurlOutput::Progress {
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

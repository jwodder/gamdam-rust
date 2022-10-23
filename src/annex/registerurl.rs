#![allow(dead_code)]
use super::outputs::{Action, AnnexResult};
use super::*;
use bytes::Bytes;
use serde::Deserialize;
use url::Url;

pub struct RegisterURLInput {
    pub key: String,
    pub url: Url,
}

impl AnnexInput for RegisterURLInput {
    type Error = std::io::Error;

    fn for_input(&self) -> Result<Bytes, Self::Error> {
        Ok(Bytes::from(format!("{} {}", self.key, self.url)))
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct RegisterURLOutput {
    #[serde(flatten)]
    pub action: Action,
    #[serde(flatten)]
    pub result: AnnexResult,
}

impl RegisterURLOutput {
    pub(crate) fn check(self) -> Result<Self, AnnexError> {
        if self.result.success {
            Ok(self)
        } else {
            Err(AnnexError(self.result.error_messages))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_registerurl_output_success() {
        // NOTE TO SELF: This is output from an invocation that passed the
        // arguments on the command line.  An invocation in batch mode will
        // have `input` set to a list of a single string, the entire line.
        let s = r#"{"command":"registerurl","error-messages":[],"file":null,"input":["SHA256E-s19--6fef386efa7208eaf1c596b6ab2f8a5a3583696ef8649be0552ab3effad1e191.txt","https://www.varonathe.org/tmp/file.txt"],"success":true}"#;
        let parsed = serde_json::from_str::<RegisterURLOutput>(s).unwrap();
        assert_eq!(parsed,
            RegisterURLOutput {
                action: Action {
                    command: String::from("registerurl"),
                    file: None,
                    input: vec![String::from("SHA256E-s19--6fef386efa7208eaf1c596b6ab2f8a5a3583696ef8649be0552ab3effad1e191.txt"), String::from("https://www.varonathe.org/tmp/file.txt")],
                },
                result: AnnexResult {
                    success: true,
                    error_messages: Vec::new(),
                },
            }
        )
    }
}

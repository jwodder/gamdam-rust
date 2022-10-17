#![allow(unused)]
use super::outputs::{Action, AnnexResult};
use super::*;
use bytes::Bytes;
use serde::Deserialize;
use std::path::Path;

pub struct RegisterURL {
    process: RawAnnexProcess,
}

impl RegisterURL {
    pub async fn new<P: AsRef<Path>>(repo: P) -> Result<Self, anyhow::Error> {
        Ok(RegisterURL {
            process: RawAnnexProcess::new(
                "registerurl",
                ["--batch", "--json", "--json-error-messages"],
                repo,
            )
            .await?,
        })
    }
}

impl AnnexProcess for RegisterURL {
    type Input = RegisterURLInput;
    type Output = RegisterURLOutput;

    fn process(&self) -> &RawAnnexProcess {
        &self.process
    }
}

pub struct RegisterURLInput {
    pub key: String,
    pub url: String,
}

impl AnnexInput for RegisterURLInput {
    fn serialize(self) -> Bytes {
        Bytes::from(format!("{} {}", self.key, self.url))
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct RegisterURLOutput {
    #[serde(flatten)]
    pub action: Action,
    #[serde(flatten)]
    pub result: AnnexResult,
}

impl AnnexOutput for RegisterURLOutput {
    fn deserialize(data: Bytes) -> Result<Self, anyhow::Error> {
        Ok(serde_json::from_slice(&data)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_registerurl_output_success() {
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

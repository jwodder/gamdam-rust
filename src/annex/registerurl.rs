#![allow(unused)]
use super::*;
use bytes::Bytes;
use std::path::Path;

pub struct RegisterURL {
    process: RawAnnexProcess,
}

impl RegisterURL {
    pub async fn new<P: AsRef<Path>>(repo: P) -> Result<Self, TODOError> {
        Ok(RegisterURL {
            process: RawAnnexProcess::new(
                ["registerurl", "--batch", "--json", "--json-error-messages"],
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

pub struct RegisterURLOutput {
    pub success: bool,
    pub error_messages: Vec<String>,
    // TODO: Other fields?
}

impl AnnexOutput for RegisterURLOutput {
    fn deserialize(data: Bytes) -> Result<Self, TODOError> {
        unimplemented!()
    }
}

#![allow(unused)]
use super::*;
use bytes::Bytes;
use std::collections::HashMap;
use std::path::Path;

pub struct Metadata {
    process: RawAnnexProcess,
}

impl Metadata {
    pub async fn new<P: AsRef<Path>>(repo: P) -> Result<Self, TODOError> {
        Ok(Metadata {
            process: RawAnnexProcess::new(
                ["metadata", "--batch", "--json", "--json-error-messages"],
                repo,
            )
            .await?,
        })
    }
}

impl AnnexProcess for Metadata {
    type Input = MetadataInput;
    type Output = MetadataOutput;

    fn process(&self) -> &RawAnnexProcess {
        &self.process
    }
}

pub struct MetadataInput {
    pub file: String,
    pub fields: HashMap<String, Vec<String>>,
}

impl AnnexInput for MetadataInput {
    fn serialize(self) -> Bytes {
        unimplemented!()
    }
}

pub struct MetadataOutput {
    pub success: bool,
    pub error_messages: Vec<String>,
    // TODO: Other fields?
}

impl AnnexOutput for MetadataOutput {
    fn deserialize(data: Bytes) -> Result<Self, TODOError> {
        unimplemented!()
    }
}

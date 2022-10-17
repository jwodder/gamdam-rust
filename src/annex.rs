#![allow(unused)]
use async_trait::async_trait;
use bytes::Bytes;
use std::ffi::OsStr;
use std::path::Path;

pub struct TODOError;

pub struct RawAnnexProcess {
    // TODO
}

impl RawAnnexProcess {
    pub async fn new<I, S, P>(args: I, repo: P) -> Result<Self, TODOError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
        P: AsRef<Path>,
    {
        unimplemented!()
    }

    pub async fn writeline(&self, line: &[u8]) -> Result<(), TODOError> {
        unimplemented!()
    }

    pub async fn readline(&self) -> Result<Bytes, TODOError> {
        unimplemented!()
    }

    // TODO: Method(s) for closing/terminating/killing
}

#[async_trait]
pub trait AnnexProcess {
    type Input;
    type Output;

    fn process(&self) -> &RawAnnexProcess;

    // TODO: Method for passing to a Func and closing/terminating/killing on
    // return

    async fn send(&self, value: Self::Input) -> Result<(), TODOError>
    where
        Self::Input: AnnexInput + Send,
    {
        // Serialize input and write to process
        unimplemented!()
    }

    async fn recv(&self) -> Result<Self::Output, TODOError>
    where
        Self::Output: AnnexOutput,
    {
        // Read line from process and deserialize
        unimplemented!()
    }

    async fn chat(&self, value: Self::Input) -> Result<Self::Output, TODOError>
    where
        Self::Input: AnnexInput + Send,
        Self::Output: AnnexOutput,
    {
        self.send(value).await?;
        self.recv().await
    }
}

pub trait AnnexInput {
    fn serialize(self) -> Bytes;
}

pub trait AnnexOutput {
    fn deserialize(data: &[u8]) -> Result<Self, TODOError>
    where
        Self: Sized;
}

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
    fn deserialize(data: &[u8]) -> Result<Self, TODOError> {
        unimplemented!()
    }
}

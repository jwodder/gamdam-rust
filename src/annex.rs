#![allow(unused)]
mod addurl;
mod metadata;
mod registerurl;
pub use addurl::*;
use async_trait::async_trait;
use bytes::Bytes;
pub use metadata::*;
pub use registerurl::*;
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
        // This function is the one that adds the '\n'
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
        self.process().writeline(&value.serialize()).await
    }

    async fn recv(&self) -> Result<Self::Output, TODOError>
    where
        Self::Output: AnnexOutput,
    {
        Self::Output::deserialize(self.process().readline().await?)
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
    fn deserialize(data: Bytes) -> Result<Self, TODOError>
    where
        Self: Sized;
}

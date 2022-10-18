#![allow(unused)]
use super::outputs::{Action, AnnexResult};
use super::*;
use anyhow::Context;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

pub struct Metadata {
    process: RawAnnexProcess,
}

impl Metadata {
    pub async fn new<P: AsRef<Path>>(repo: P) -> Result<Self, anyhow::Error> {
        Ok(Metadata {
            process: RawAnnexProcess::new(
                "metadata",
                ["--batch", "--json", "--json-error-messages"],
                repo,
            )
            .await?,
        })
    }
}

impl AnnexProcess for Metadata {
    type Input = MetadataInput;
    type Output = MetadataOutput;

    fn process(&mut self) -> &mut RawAnnexProcess {
        &mut self.process
    }
}

pub struct MetadataInput {
    pub file: String,
    pub fields: HashMap<String, Vec<String>>,
}

impl AnnexInput for MetadataInput {
    fn serialize(self) -> String {
        unimplemented!()
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct MetadataOutput {
    pub fields: HashMap<String, Vec<String>>,
    #[serde(flatten)]
    pub action: Action,
    #[serde(flatten)]
    pub result: AnnexResult,
    #[serde(default)]
    pub note: Option<String>,
}

impl AnnexOutput for MetadataOutput {
    fn deserialize(data: &str) -> Result<Self, anyhow::Error> {
        serde_json::from_str(data)
            .with_context(|| format!("Unable to decode `git-annex metadata` output: {data:?}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_metadata_output_success() {
        let s = r#"{"command":"metadata","error-messages":[],"fields":{"color":["blue"],"color-lastchanged":["2022-10-17@19-53-03"],"flavors":["charmed","strange"],"flavors-lastchanged":["2022-10-17@19-53-03"],"lastchanged":["2022-10-17@19-53-03"]},"file":"file.txt","input":["{\"file\": \"file.txt\", \"fields\": {\"color\": [\"blue\"], \"flavors\": [\"strange\", \"charmed\"], \"mouthfeel\": []}}"],"key":"SHA256E-s19--6fef386efa7208eaf1c596b6ab2f8a5a3583696ef8649be0552ab3effad1e191.txt","note":"color=blue\ncolor-lastchanged=2022-10-17@19-53-03\nflavors=charmed\nflavors=strange\nflavors-lastchanged=2022-10-17@19-53-03\nlastchanged=2022-10-17@19-53-03\n","success":true}"#;
        let parsed = serde_json::from_str::<MetadataOutput>(s).unwrap();
        assert_eq!(parsed,
            MetadataOutput {
                fields: HashMap::from([
                    (String::from("color"), vec![String::from("blue")]),
                    (String::from("color-lastchanged"), vec![String::from("2022-10-17@19-53-03")]),
                    (String::from("flavors"), vec![String::from("charmed"), String::from("strange")]),
                    (String::from("flavors-lastchanged"), vec![String::from("2022-10-17@19-53-03")]),
                    (String::from("lastchanged"), vec![String::from("2022-10-17@19-53-03")]),
                ]),
                action: Action {
                    file: Some(String::from("file.txt")),
                    command: String::from("metadata"),
                    input: vec![String::from(r#"{"file": "file.txt", "fields": {"color": ["blue"], "flavors": ["strange", "charmed"], "mouthfeel": []}}"#)],
                },
                result: AnnexResult {
                    success: true,
                    error_messages: Vec::new(),
                },
                note: Some(String::from("color=blue\ncolor-lastchanged=2022-10-17@19-53-03\nflavors=charmed\nflavors=strange\nflavors-lastchanged=2022-10-17@19-53-03\nlastchanged=2022-10-17@19-53-03\n")),
            }
        )
    }
}

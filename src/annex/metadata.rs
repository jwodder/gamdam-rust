#![allow(dead_code)]
use super::outputs::{Action, AnnexResult};
use super::*;
use bytes::Bytes;
use relative_path::{RelativePath, RelativePathBuf};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Debug, Serialize, Eq, PartialEq)]
pub struct MetadataInput {
    pub file: RelativePathBuf,
    pub fields: HashMap<String, Vec<String>>,
}

impl AnnexInput for MetadataInput {
    type Error = serde_json::Error;

    fn for_input(&self) -> Result<Bytes, Self::Error> {
        Ok(Bytes::from(serde_json::to_string(&self)?))
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

impl MetadataOutput {
    pub(crate) fn file(&self) -> &RelativePath {
        self.action
            .file
            .as_deref()
            .unwrap_or_else(|| RelativePath::from_path("<unknown file>").unwrap())
    }

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
                    file: Some(RelativePathBuf::from_path("file.txt").unwrap()),
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

    #[test]
    fn test_dump_metadata_input() {
        let mi = MetadataInput {
            file: RelativePathBuf::from_path("file.txt").unwrap(),
            fields: HashMap::from([(String::from("color"), vec![String::from("blue")])]),
        };
        let s = r#"{"file":"file.txt","fields":{"color":["blue"]}}"#.as_bytes();
        assert_eq!(mi.for_input().unwrap(), s);
    }
}

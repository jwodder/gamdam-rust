use super::outputs::{Action, AnnexResult};
use super::*;
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Debug, Serialize, Eq, PartialEq)]
pub(crate) struct MetadataInput {
    // We operate on keys rather than files so as to avoid issues with unlocked
    // files (e.g., on crippled Windows filesystems) when addurl has not yet
    // exited.
    pub(crate) key: String,
    pub(crate) fields: HashMap<String, Vec<String>>,
}

impl AnnexInput for MetadataInput {
    type Error = serde_json::Error;

    fn for_input(&self) -> Result<Bytes, Self::Error> {
        Ok(Bytes::from(serde_json::to_string(&self)?))
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub(crate) struct MetadataOutput {
    pub(crate) fields: HashMap<String, Vec<String>>,
    #[serde(flatten)]
    pub(crate) action: Action,
    #[serde(flatten)]
    pub(crate) result: AnnexResult,
    #[serde(default)]
    pub(crate) note: Option<String>,
}

impl MetadataOutput {
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
    use crate::filepath::FilePath;

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
                    file: Some(FilePath::try_from("file.txt").unwrap()),
                    command: String::from("metadata"),
                    input: vec![String::from(r#"{"file": "file.txt", "fields": {"color": ["blue"], "flavors": ["strange", "charmed"], "mouthfeel": []}}"#)],
                },
                result: AnnexResult {
                    success: true,
                    error_messages: Vec::new(),
                },
                note: Some(String::from("color=blue\ncolor-lastchanged=2022-10-17@19-53-03\nflavors=charmed\nflavors=strange\nflavors-lastchanged=2022-10-17@19-53-03\nlastchanged=2022-10-17@19-53-03\n")),
            }
        );
    }

    #[test]
    fn test_dump_metadata_input() {
        let mi = MetadataInput {
            key: "SHA256E-s14239--c3784aaf20ae0867e2f491504a57a15f19eafafb59ed9faea1cfc5cfbbea2b1b.txt".into(),
            fields: HashMap::from([(String::from("color"), vec![String::from("blue")])]),
        };
        let s = r#"{"key":"SHA256E-s14239--c3784aaf20ae0867e2f491504a57a15f19eafafb59ed9faea1cfc5cfbbea2b1b.txt","fields":{"color":["blue"]}}"#.as_bytes();
        assert_eq!(mi.for_input().unwrap(), s);
    }
}

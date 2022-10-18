mod annex;
use relative_path::RelativePathBuf;
use serde::Deserialize;
use std::collections::HashMap;
use url::Url;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct Downloadable {
    pub path: RelativePathBuf,
    pub url: Url,
    #[serde(default)]
    pub metadata: HashMap<String, Vec<String>>,
    #[serde(default)]
    pub extra_urls: Vec<Url>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_downloadable_defaults() {
        let s = r#"{"path": "foo/bar/baz.txt", "url": "https://example.com/baz.txt"}"#;
        let parsed = serde_json::from_str::<Downloadable>(s).unwrap();
        assert_eq!(
            parsed,
            Downloadable {
                path: RelativePathBuf::from_path("foo/bar/baz.txt").unwrap(),
                url: Url::parse("https://example.com/baz.txt").unwrap(),
                metadata: HashMap::new(),
                extra_urls: Vec::new(),
            }
        );
    }

    // <https://github.com/udoprog/relative-path/issues/41>
    /*
    #[test]
    fn test_load_downloadable_absolute_path() {
        let s = r#"{"path": "/foo/bar/baz.txt", "url": "https://example.com/baz.txt"}"#;
        match serde_json::from_str::<Downloadable>(s) {
            Err(_) => (),
            Ok(v) => panic!("Deserialization did not fail: {v:?}"),
        }
    }
    */
}

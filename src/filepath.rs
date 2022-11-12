use serde::de::{Deserializer, Unexpected, Visitor};
use serde::ser::Serializer;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::{Component, Path};
use thiserror::Error;

/// A normalized, nonempty, forward-slash-separated, UTF-8 encoded, relative
/// file path
#[derive(Clone, Eq, Hash, PartialEq)]
pub struct FilePath(String);

impl FilePath {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for FilePath {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(&self.0, f)
    }
}

impl fmt::Display for FilePath {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl Serialize for FilePath {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

struct FilePathVisitor;

impl<'de> Visitor<'de> for FilePathVisitor {
    type Value = FilePath;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a normalized, nonempty, relative path")
    }

    fn visit_str<E>(self, input: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        FilePath::try_from(input).map_err(|_| E::invalid_value(Unexpected::Str(input), &self))
    }
}

impl<'de> Deserialize<'de> for FilePath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_str(FilePathVisitor)
    }
}

/// Error returned when trying to construct a [`FilePath`] from an invalid,
/// unnormalized, or undecodable relative path
#[derive(Copy, Clone, Debug, Hash, Eq, Error, PartialEq)]
pub enum FilePathError {
    #[error("Path contains no pathnames")]
    Empty,
    #[error("Path is not normalized")]
    NotNormalized,
    #[error("Path is not relative")]
    NotRelative,
    #[error("Path is not Unicode")]
    Undecodable,
}

impl TryFrom<&Path> for FilePath {
    type Error = FilePathError;

    fn try_from(path: &Path) -> Result<FilePath, FilePathError> {
        // TODO: Prohibit paths that end with a file path separator
        let mut output = String::new();
        for c in path.components() {
            match c {
                Component::Normal(part) => match part.to_str() {
                    Some(s) => {
                        if !output.is_empty() {
                            output.push('/');
                        }
                        output.push_str(s);
                    }
                    None => return Err(FilePathError::Undecodable),
                },
                Component::CurDir => (),
                Component::ParentDir => return Err(FilePathError::NotNormalized),
                _ => return Err(FilePathError::NotRelative),
            }
        }
        if output.is_empty() {
            return Err(FilePathError::Empty);
        }
        Ok(FilePath(output))
    }
}

impl TryFrom<&str> for FilePath {
    type Error = FilePathError;

    fn try_from(path: &str) -> Result<FilePath, FilePathError> {
        Path::new(path).try_into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case("foo", r#""foo""#)]
    #[case("foo/bar", r#""foo/bar""#)]
    #[case("foo\n/\tbar", r#""foo\n/\tbar""#)]
    #[case("foo\x1B—🐐bar", r#""foo\u{1b}—🐐bar""#)]
    fn test_debug(#[case] path: &str, #[case] repr: &str) {
        let path = FilePath::try_from(path).unwrap();
        assert_eq!(format!("{path:?}"), repr);
    }

    #[rstest]
    #[case("foo", "foo")]
    #[case("foo/bar", "foo/bar")]
    #[case("foo/.", "foo")]
    #[case("./foo", "foo")]
    #[case("foo/./bar", "foo/bar")]
    #[case("foo/", "foo")]
    #[case("foo//bar", "foo/bar")]
    #[cfg_attr(windows, case(r#"foo\bar"#, "foo/bar"))]
    fn test_filepath_try_from(#[case] path: &str, #[case] displayed: &str) {
        assert_eq!(FilePath::try_from(path).unwrap().to_string(), displayed);
    }

    #[rstest]
    #[case("", FilePathError::Empty)]
    #[case(".", FilePathError::Empty)]
    #[case("..", FilePathError::NotNormalized)]
    #[case("/", FilePathError::NotRelative)]
    #[case("/foo", FilePathError::NotRelative)]
    #[case("foo/..", FilePathError::NotNormalized)]
    #[case("../foo", FilePathError::NotNormalized)]
    #[case("foo/../bar", FilePathError::NotNormalized)]
    #[case("foo/bar/..", FilePathError::NotNormalized)]
    #[cfg_attr(windows, case(r#"\foo\bar"#, FilePathError::NotRelative))]
    #[cfg_attr(windows, case(r#"C:\foo\bar"#, FilePathError::NotRelative))]
    fn test_filepath_try_from_err(#[case] path: &str, #[case] err: FilePathError) {
        assert_eq!(FilePath::try_from(path), Err(err));
    }

    #[derive(Debug, Deserialize, Eq, PartialEq, Serialize)]
    struct Structure {
        path: FilePath,
    }

    #[test]
    fn test_serialize() {
        let st = Structure {
            path: "foo/bar".try_into().unwrap(),
        };
        assert_eq!(serde_json::to_string(&st).unwrap(), r#"{"path":"foo/bar"}"#);
    }

    #[test]
    fn test_deserialize() {
        let s = r#"{"path":"foo/bar"}"#;
        let parsed = serde_json::from_str::<Structure>(s).unwrap();
        assert_eq!(
            parsed,
            Structure {
                path: "foo/bar".try_into().unwrap()
            }
        );
    }

    #[test]
    #[cfg(windows)]
    fn test_deserialize_windows() {
        let s = r#"{"path":"foo\\bar"}"#;
        let parsed = serde_json::from_str::<Structure>(s).unwrap();
        assert_eq!(
            parsed,
            Structure {
                path: "foo/bar".try_into().unwrap()
            }
        );
    }

    #[rstest]
    #[case(r#"{"path":42}"#)]
    #[case(r#"{"path":""}"#)]
    #[case(r#"{"path":"."}"#)]
    #[case(r#"{"path":".."}"#)]
    #[case(r#"{"path":"../foo/bar"}"#)]
    #[case(r#"{"path":"foo/../bar"}"#)]
    #[case(r#"{"path":"/foo/bar"}"#)]
    fn test_deserialize_err(#[case] s: &str) {
        assert!(serde_json::from_str::<Structure>(s).is_err());
    }
}

use std::fmt;
use std::path::{Component, Path};
use thiserror::Error;

/// A normalized, nonempty, forward-slash-separated UTF-8 encoded relative file
/// path
#[derive(Clone, Eq, Hash, PartialEq)]
pub struct FilePath(Vec<String>);

impl fmt::Debug for FilePath {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("\"")?;
        for (i, part) in self.0.iter().enumerate() {
            if i > 0 {
                f.write_str("/")?;
            }
            write!(f, "{}", part.escape_debug())?;
        }
        f.write_str("\"")?;
        Ok(())
    }
}

impl fmt::Display for FilePath {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        for (i, part) in self.0.iter().enumerate() {
            if i > 0 {
                f.write_str("/")?;
            }
            f.write_str(part)?;
        }
        Ok(())
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
        // TODO: Prohibit paths that ends with a file path separator
        let mut output = Vec::new();
        for c in path.components() {
            match c {
                Component::Normal(part) => match part.to_str() {
                    Some(s) => output.push(String::from(s)),
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
}

use crate::filepath::FilePath;
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct AnnexResult {
    pub(crate) success: bool,
    pub(crate) error_messages: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub(crate) struct Action {
    pub(crate) command: String,
    // `file` can be `None` for an in-progress download requested without an
    // explicit download path
    pub(crate) file: Option<FilePath>,
    pub(crate) input: Vec<String>,
}

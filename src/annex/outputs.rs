use serde::Deserialize;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub struct AnnexResult {
    pub success: bool,
    pub error_messages: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct Action {
    pub command: String,
    // `file` can be `None` for an in-progress download requested without an
    // explicit download path
    pub file: Option<String>,
    pub input: Vec<String>,
}

use crate::domain::table::ai::ExamplePurpose;
use rbatis::PageRequest;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AiExamplePageDTO {
    pub purpose: ExamplePurpose,
    pub page_no: Option<u64>,
    pub page_size: Option<u64>,
}

impl From<&AiExamplePageDTO> for PageRequest {
    fn from(arg: &AiExamplePageDTO) -> Self {
        PageRequest::new(arg.page_no.unwrap_or(1), arg.page_size.unwrap_or(10))
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AiImageDTO {
    pub mime_type: String,
    /// Raw base64, without a `data:` prefix.
    pub data_base64: String,
}

/// Add (no id) or update (with id) an example. Update replaces all of its images.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AiExampleSaveDTO {
    pub id: Option<String>,
    pub purpose: ExamplePurpose,
    #[serde(default)]
    pub question_text: String,
    pub expected_choice: String,
    pub expected_explanation: Option<String>,
    #[serde(default)]
    pub images: Vec<AiImageDTO>,
}

/// Which training examples a new version learns from.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
pub enum ExampleSelection {
    All,
    Selected,
    /// A baseline: plain Gemma, the score to beat.
    None,
}

/// "Train": create a model version.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AiVersionAddDTO {
    pub name: Option<String>,
    #[serde(default)]
    pub instructions: String,
    pub selection: ExampleSelection,
    /// Used when selection is SELECTED.
    #[serde(default)]
    pub example_ids: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AiEvalStartDTO {
    pub version_id: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AiEvalCompareDTO {
    pub run_a: String,
    pub run_b: String,
}

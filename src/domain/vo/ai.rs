use crate::context::CONTEXT;
use crate::domain::table::ai::{
    AiEvalResultRow, AiEvalRunRow, AiExample, AiExampleImage, AiExampleListRow, AiVersionListRow,
    ExamplePurpose, Outcome, RunStatus,
};
use rbatis::rbdc::DateTime;
use serde::{Deserialize, Serialize};

fn format_date(date: Option<DateTime>) -> Option<String> {
    date.map(|v| v.format(&CONTEXT.config.datetime_format))
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AiExampleVO {
    pub id: Option<String>,
    pub purpose: Option<ExamplePurpose>,
    pub question_text: Option<String>,
    pub expected_choice: Option<String>,
    pub expected_explanation: Option<String>,
    pub image_count: u64,
    pub create_date: Option<String>,
    pub update_date: Option<String>,
}

impl From<AiExampleListRow> for AiExampleVO {
    fn from(arg: AiExampleListRow) -> Self {
        let mut vo = AiExampleVO::from(arg.example);
        vo.image_count = arg.image_count.unwrap_or_default();
        vo
    }
}

impl From<AiExample> for AiExampleVO {
    fn from(arg: AiExample) -> Self {
        Self {
            id: arg.id,
            purpose: arg.purpose,
            question_text: arg.question_text,
            expected_choice: arg.expected_choice,
            expected_explanation: arg.expected_explanation,
            image_count: 0,
            create_date: format_date(arg.create_date),
            update_date: format_date(arg.update_date),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AiImageVO {
    pub mime_type: Option<String>,
    pub data_base64: Option<String>,
}

impl From<AiExampleImage> for AiImageVO {
    fn from(arg: AiExampleImage) -> Self {
        Self {
            mime_type: arg.mime_type,
            data_base64: arg.data_base64,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AiExampleDetailVO {
    #[serde(flatten)]
    pub example: AiExampleVO,
    pub images: Vec<AiImageVO>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AiVersionVO {
    pub id: Option<String>,
    pub name: Option<String>,
    pub base_model: Option<String>,
    pub instructions: Option<String>,
    pub example_count: u64,
    pub create_date: Option<String>,
}

impl From<AiVersionListRow> for AiVersionVO {
    fn from(arg: AiVersionListRow) -> Self {
        let v = arg.version;
        Self {
            id: v.id,
            name: v.name,
            base_model: v.base_model,
            instructions: v.instructions,
            example_count: arg.example_count.unwrap_or_default(),
            create_date: format_date(v.create_date),
        }
    }
}

/// A test run. The page computes the score as correct / total.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AiEvalRunVO {
    pub id: Option<String>,
    pub version_id: Option<String>,
    pub version_name: Option<String>,
    pub status: Option<RunStatus>,
    pub total: i32,
    pub done: u64,
    pub correct: u64,
    pub errors: u64,
    pub error_message: Option<String>,
    pub create_date: Option<String>,
    pub finish_date: Option<String>,
}

impl From<AiEvalRunRow> for AiEvalRunVO {
    fn from(arg: AiEvalRunRow) -> Self {
        let r = arg.run;
        Self {
            id: r.id,
            version_id: r.version_id,
            version_name: arg.version_name,
            status: r.status,
            total: r.total.unwrap_or_default(),
            done: arg.done.unwrap_or_default(),
            correct: arg.correct.unwrap_or_default(),
            errors: arg.errors.unwrap_or_default(),
            error_message: r.error_message,
            create_date: format_date(r.create_date),
            finish_date: format_date(r.finish_date),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AiEvalResultVO {
    pub example_id: Option<String>,
    /// NULL if the example was deleted after the run.
    pub question_text: Option<String>,
    pub expected_choice: Option<String>,
    pub outcome: Option<Outcome>,
    pub model_choice: Option<String>,
    pub model_explanation: Option<String>,
    pub error_message: Option<String>,
    pub prompt_tokens: Option<i32>,
    pub duration_ms: Option<i64>,
}

impl From<AiEvalResultRow> for AiEvalResultVO {
    fn from(arg: AiEvalResultRow) -> Self {
        let r = arg.result;
        Self {
            example_id: r.example_id,
            question_text: arg.question_text,
            expected_choice: r.expected_choice_snapshot,
            outcome: r.outcome,
            model_choice: r.model_choice,
            model_explanation: r.model_explanation,
            error_message: r.error_message,
            prompt_tokens: r.prompt_tokens,
            duration_ms: r.duration_ms,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AiEvalDetailVO {
    pub run: AiEvalRunVO,
    pub results: Vec<AiEvalResultVO>,
    /// The context window, so the page can warn when prompts get close to it.
    pub num_ctx: u32,
}

/// Two runs compared on the test examples both of them answered.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AiCompareVO {
    pub run_a: AiEvalRunVO,
    pub run_b: AiEvalRunVO,
    pub common_count: u64,
    pub a_correct: u64,
    pub b_correct: u64,
    pub rows: Vec<AiCompareRowVO>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AiCompareRowVO {
    pub example_id: Option<String>,
    pub question_text: Option<String>,
    pub expected_choice: Option<String>,
    pub a_outcome: Option<Outcome>,
    pub a_choice: Option<String>,
    pub b_outcome: Option<Outcome>,
    pub b_choice: Option<String>,
}

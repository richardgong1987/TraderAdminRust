//! Tables for AI training and testing. Design: doc/ai-training.md.
use crate::domain::dto::AiExamplePageDTO;
use rbatis::executor::Executor;
use rbatis::rbdc::DateTime;
use rbatis::{RBatis, crud, html_sql};
use rbs::value;
use serde::{Deserialize, Serialize};

/// Training examples teach a model version; test examples only measure it and are never shown to it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum ExamplePurpose {
    Train,
    Test,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum RunStatus {
    Running,
    Done,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Outcome {
    Correct,
    Wrong,
    Error,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AiExample {
    pub id: Option<String>,
    pub purpose: Option<ExamplePurpose>,
    pub question_text: Option<String>,
    pub expected_choice: Option<String>,
    /// NULL: no explanation given.
    pub expected_explanation: Option<String>,
    pub create_by: Option<String>,
    pub create_date: Option<DateTime>,
    pub update_date: Option<DateTime>,
}
crud!(AiExample {});

/// One row of the example list: the example without its images, plus how many it has.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AiExampleListRow {
    #[serde(flatten)]
    pub example: AiExample,
    pub image_count: Option<u64>,
}

#[html_sql("src/domain/table/ai.html")]
impl AiExample {
    pub async fn select_page(
        conn: &dyn Executor,
        page_request: &rbatis::PageRequest,
        dto: &AiExamplePageDTO,
    ) -> rbatis::Result<Page<AiExampleListRow>> {
        impled!()
    }
    pub async fn select_choices(conn: &dyn Executor) -> rbatis::Result<Vec<ChoiceRow>> {
        impled!()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChoiceRow {
    pub expected_choice: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AiExampleImage {
    pub id: Option<String>,
    pub example_id: Option<String>,
    pub sort_no: Option<i32>,
    pub mime_type: Option<String>,
    /// Raw base64, without a `data:` prefix: the form Ollama takes.
    pub data_base64: Option<String>,
    pub create_date: Option<DateTime>,
}
crud!(AiExampleImage {});

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AiModelVersion {
    pub id: Option<String>,
    pub name: Option<String>,
    pub base_model: Option<String>,
    pub instructions: Option<String>,
    pub create_by: Option<String>,
    pub create_date: Option<DateTime>,
}
crud!(AiModelVersion {});

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AiVersionListRow {
    #[serde(flatten)]
    pub version: AiModelVersion,
    pub example_count: Option<u64>,
}

#[html_sql("src/domain/table/ai.html")]
impl AiModelVersion {
    pub async fn select_list(conn: &dyn Executor) -> rbatis::Result<Vec<AiVersionListRow>> {
        impled!()
    }
}

/// Which training examples a model version learns from.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AiModelVersionExample {
    pub id: Option<String>,
    pub version_id: Option<String>,
    pub example_id: Option<String>,
}
crud!(AiModelVersionExample {});

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AiEvalRun {
    pub id: Option<String>,
    pub version_id: Option<String>,
    pub status: Option<RunStatus>,
    /// Snapshot: size of the test set when the run started.
    pub total: Option<i32>,
    pub error_message: Option<String>,
    pub create_by: Option<String>,
    pub create_date: Option<DateTime>,
    /// NULL while the run is still going.
    pub finish_date: Option<DateTime>,
}
crud!(AiEvalRun {});

/// A run with its version name and the counts computed from its results.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AiEvalRunRow {
    #[serde(flatten)]
    pub run: AiEvalRun,
    pub version_name: Option<String>,
    pub done: Option<u64>,
    pub correct: Option<u64>,
    pub errors: Option<u64>,
}

#[html_sql("src/domain/table/ai.html")]
impl AiEvalRun {
    pub async fn select_recent(
        conn: &dyn Executor,
        limit: u64,
    ) -> rbatis::Result<Vec<AiEvalRunRow>> {
        impled!()
    }
    pub async fn select_by_ids(
        conn: &dyn Executor,
        ids: &[String],
    ) -> rbatis::Result<Vec<AiEvalRunRow>> {
        impled!()
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AiEvalResult {
    pub id: Option<String>,
    pub run_id: Option<String>,
    /// No foreign key: deleting a test example keeps the results it produced.
    pub example_id: Option<String>,
    /// Snapshot: the expected choice when the run happened, so old scores stay valid after edits.
    pub expected_choice_snapshot: Option<String>,
    pub outcome: Option<Outcome>,
    /// NULL exactly when outcome is ERROR.
    pub model_choice: Option<String>,
    pub model_explanation: Option<String>,
    /// Set exactly when outcome is ERROR.
    pub error_message: Option<String>,
    pub prompt_tokens: Option<i32>,
    pub duration_ms: Option<i64>,
    pub create_date: Option<DateTime>,
}
crud!(AiEvalResult {});

/// A result with the question it answered (NULL if the example was deleted since).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AiEvalResultRow {
    #[serde(flatten)]
    pub result: AiEvalResult,
    pub question_text: Option<String>,
}

#[html_sql("src/domain/table/ai.html")]
impl AiEvalResult {
    pub async fn select_by_run(
        conn: &dyn Executor,
        run_id: &str,
    ) -> rbatis::Result<Vec<AiEvalResultRow>> {
        impled!()
    }
}

/// Column types are given explicitly: the sync's defaults would make every string TEXT and
/// skip the primary key.
pub async fn sync_tables(conn: &dyn Executor, rb: &RBatis) {
    let tables = [
        (
            "ai_example",
            value! {
                "id": "VARCHAR(50) PRIMARY KEY",
                "purpose": "VARCHAR(10) NOT NULL",
                "question_text": "TEXT NOT NULL",
                "expected_choice": "VARCHAR(50) NOT NULL",
                "expected_explanation": "TEXT",
                "create_by": "VARCHAR(50) NOT NULL",
                "create_date": "DATETIME NOT NULL",
                "update_date": "DATETIME NOT NULL",
            },
        ),
        (
            "ai_example_image",
            value! {
                "id": "VARCHAR(50) PRIMARY KEY",
                "example_id": "VARCHAR(50) NOT NULL",
                "sort_no": "INT NOT NULL",
                "mime_type": "VARCHAR(50) NOT NULL",
                "data_base64": "LONGTEXT NOT NULL",
                "create_date": "DATETIME NOT NULL",
            },
        ),
        (
            "ai_model_version",
            value! {
                "id": "VARCHAR(50) PRIMARY KEY",
                "name": "VARCHAR(100) NOT NULL",
                "base_model": "VARCHAR(100) NOT NULL",
                "instructions": "TEXT NOT NULL",
                "create_by": "VARCHAR(50) NOT NULL",
                "create_date": "DATETIME NOT NULL",
            },
        ),
        (
            "ai_model_version_example",
            value! {
                "id": "VARCHAR(50) PRIMARY KEY",
                "version_id": "VARCHAR(50) NOT NULL",
                "example_id": "VARCHAR(50) NOT NULL",
            },
        ),
        (
            "ai_eval_run",
            value! {
                "id": "VARCHAR(50) PRIMARY KEY",
                "version_id": "VARCHAR(50) NOT NULL",
                "status": "VARCHAR(10) NOT NULL",
                "total": "INT NOT NULL",
                "error_message": "TEXT",
                "create_by": "VARCHAR(50) NOT NULL",
                "create_date": "DATETIME NOT NULL",
                "finish_date": "DATETIME",
            },
        ),
        (
            "ai_eval_result",
            value! {
                "id": "VARCHAR(50) PRIMARY KEY",
                "run_id": "VARCHAR(50) NOT NULL",
                "example_id": "VARCHAR(50) NOT NULL",
                "expected_choice_snapshot": "VARCHAR(50) NOT NULL",
                "outcome": "VARCHAR(10) NOT NULL",
                "model_choice": "VARCHAR(50)",
                "model_explanation": "TEXT",
                "error_message": "TEXT",
                "prompt_tokens": "INT",
                "duration_ms": "BIGINT",
                "create_date": "DATETIME NOT NULL",
            },
        ),
    ];
    for (name, columns) in tables {
        let _ = RBatis::sync(conn, rb, &columns, name).await;
    }

    // MySQL has no "IF NOT EXISTS" for indexes or constraints, so each statement fails harmlessly
    // with "Duplicate ..." after the first start.
    let constraints = [
        // Example list: by purpose, newest first.
        "CREATE INDEX idx_ai_example_purpose_date ON ai_example (purpose, create_date)",
        // One example's images, in order.
        "CREATE INDEX idx_ai_example_image_example ON ai_example_image (example_id, sort_no)",
        "ALTER TABLE ai_example_image ADD CONSTRAINT fk_ai_example_image_example
            FOREIGN KEY (example_id) REFERENCES ai_example (id) ON DELETE CASCADE",
        "CREATE UNIQUE INDEX uk_ai_version_example ON ai_model_version_example (version_id, example_id)",
        "ALTER TABLE ai_model_version_example ADD CONSTRAINT fk_ai_version_example_version
            FOREIGN KEY (version_id) REFERENCES ai_model_version (id) ON DELETE CASCADE",
        "ALTER TABLE ai_model_version_example ADD CONSTRAINT fk_ai_version_example_example
            FOREIGN KEY (example_id) REFERENCES ai_example (id) ON DELETE CASCADE",
        // Runs of one version, newest first.
        "CREATE INDEX idx_ai_eval_run_version_date ON ai_eval_run (version_id, create_date)",
        "ALTER TABLE ai_eval_run ADD CONSTRAINT fk_ai_eval_run_version
            FOREIGN KEY (version_id) REFERENCES ai_model_version (id) ON DELETE CASCADE",
        // One answer per test example per run; also serves "results of a run".
        "CREATE UNIQUE INDEX uk_ai_eval_result_run_example ON ai_eval_result (run_id, example_id)",
        "ALTER TABLE ai_eval_result ADD CONSTRAINT fk_ai_eval_result_run
            FOREIGN KEY (run_id) REFERENCES ai_eval_run (id) ON DELETE CASCADE",
    ];
    for sql in constraints {
        if let Err(e) = conn.exec(sql, vec![]).await
            && !e.to_string().contains("Duplicate")
        {
            log::warn!("[abs_admin] ai table constraint failed: {} ({})", e, sql);
        }
    }
}

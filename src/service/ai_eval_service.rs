use crate::context::CONTEXT;
use crate::domain::table::ai::{
    AiEvalResult, AiEvalResultRow, AiEvalRun, ExamplePurpose, Outcome, RunStatus,
};
use crate::domain::vo::{AiCompareRowVO, AiCompareVO, AiEvalDetailVO, AiEvalResultVO, AiEvalRunVO};
use crate::error::{Error, Result};
use crate::service::ai_prompt::{
    PromptExample, answer_schema, build_messages, parse_answer, score, system_prompt,
};
use crate::service::ollama_client::{ChatOptions, ChatRequest};
use crate::{error_info, pool};
use rbatis::object_id::ObjectId;
use rbatis::rbdc::DateTime;
use rbs::value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Mutex;

const RECENT_RUNS: u64 = 100;

/// Everything a background test run needs, loaded before it starts.
struct TestPlan {
    run_id: String,
    model: String,
    system_prompt: String,
    answer_schema: serde_json::Value,
    worked_examples: Vec<PromptExample>,
    questions: Vec<PromptExample>,
}

#[derive(Default)]
pub struct AiEvalService {
    /// Held by the running test. Gemma already uses every CPU core, so runs go one at a time.
    running: Arc<Mutex<()>>,
}

impl AiEvalService {
    /// Starts testing a version on the whole test set in the background. Returns the run id.
    pub async fn start(&self, version_id: &str, user_id: &str) -> Result<String> {
        let running = self
            .running
            .clone()
            .try_lock_owned()
            .map_err(|_| Error::from(error_info!("ai.test_running")))?;
        let plan = self.plan(version_id).await?;
        let run = AiEvalRun {
            id: Some(plan.run_id.clone()),
            version_id: Some(version_id.to_string()),
            status: Some(RunStatus::Running),
            total: Some(plan.questions.len() as i32),
            error_message: None,
            create_by: Some(user_id.to_string()),
            create_date: Some(DateTime::now()),
            finish_date: None,
        };
        AiEvalRun::insert(pool!(), &run).await?;
        let run_id = plan.run_id.clone();
        tokio::spawn(async move {
            let _running = running;
            let outcome = answer_all(&plan).await;
            finish_run(&plan.run_id, outcome).await;
        });
        Ok(run_id)
    }

    async fn plan(&self, version_id: &str) -> Result<TestPlan> {
        let version = CONTEXT.ai_version_service.get(version_id).await?;
        let examples = &CONTEXT.ai_example_service;
        let test_ids = examples.ids_by_purpose(ExamplePurpose::Test).await?;
        if test_ids.is_empty() {
            return Err(Error::from(error_info!("ai.no_test_examples")));
        }
        let choices = examples.choices().await?;
        let worked_ids = CONTEXT.ai_version_service.example_ids(version_id).await?;
        Ok(TestPlan {
            run_id: ObjectId::new().to_string(),
            model: version.base_model.unwrap_or_default(),
            system_prompt: system_prompt(&choices, version.instructions.as_deref().unwrap_or("")),
            answer_schema: answer_schema(&choices),
            worked_examples: examples.load_for_prompt(&worked_ids).await?,
            questions: examples.load_for_prompt(&test_ids).await?,
        })
    }

    /// A run still RUNNING at startup was cut off by a restart; nothing will ever finish it.
    pub async fn fail_interrupted_runs(&self) -> Result<u64> {
        let interrupted = AiEvalRun {
            status: Some(RunStatus::Failed),
            error_message: Some("interrupted: the app restarted during the test".to_string()),
            finish_date: Some(DateTime::now()),
            ..Default::default()
        };
        let updated =
            AiEvalRun::update_by_map(pool!(), &interrupted, value! {"status": RunStatus::Running})
                .await?;
        Ok(updated.rows_affected)
    }

    pub async fn list(&self) -> Result<Vec<AiEvalRunVO>> {
        let runs = AiEvalRun::select_recent(pool!(), RECENT_RUNS).await?;
        Ok(runs.into_iter().map(AiEvalRunVO::from).collect())
    }

    pub async fn detail(&self, run_id: &str) -> Result<AiEvalDetailVO> {
        let run = self.run(run_id).await?;
        let results = AiEvalResult::select_by_run(pool!(), run_id).await?;
        Ok(AiEvalDetailVO {
            run,
            results: results.into_iter().map(AiEvalResultVO::from).collect(),
            num_ctx: CONTEXT.config.ollama_num_ctx,
        })
    }

    pub async fn compare(&self, run_a: &str, run_b: &str) -> Result<AiCompareVO> {
        let a = AiEvalResult::select_by_run(pool!(), run_a).await?;
        let b = AiEvalResult::select_by_run(pool!(), run_b).await?;
        let rows = compare_results(a, b);
        let correct = |outcome: &Option<Outcome>| *outcome == Some(Outcome::Correct);
        Ok(AiCompareVO {
            run_a: self.run(run_a).await?,
            run_b: self.run(run_b).await?,
            common_count: rows.len() as u64,
            a_correct: rows.iter().filter(|r| correct(&r.a_outcome)).count() as u64,
            b_correct: rows.iter().filter(|r| correct(&r.b_outcome)).count() as u64,
            rows,
        })
    }

    async fn run(&self, run_id: &str) -> Result<AiEvalRunVO> {
        AiEvalRun::select_by_ids(pool!(), &[run_id.to_string()])
            .await?
            .into_iter()
            .next()
            .map(AiEvalRunVO::from)
            .ok_or_else(|| Error::from(error_info!("ai.run_not_found")))
    }
}

/// Saves each answer as soon as it arrives, so the page can show progress.
async fn answer_all(plan: &TestPlan) -> Result<()> {
    for question in &plan.questions {
        let result = answer_one(plan, question).await;
        AiEvalResult::insert(pool!(), &result).await?;
    }
    Ok(())
}

/// Never fails: a failed answer is an ERROR result, and the run goes on with the next question.
async fn answer_one(plan: &TestPlan, question: &PromptExample) -> AiEvalResult {
    let started = Instant::now();
    let request = ChatRequest {
        model: &plan.model,
        messages: build_messages(&plan.system_prompt, &plan.worked_examples, question),
        format: plan.answer_schema.clone(),
        stream: false,
        // Temperature 0: the same question gets the same answer, so score changes come from training.
        options: ChatOptions {
            temperature: 0.0,
            num_ctx: CONTEXT.config.ollama_num_ctx,
        },
    };
    let reply = match CONTEXT.ollama_client.chat(&request).await {
        Ok(response) => parse_answer(&response.message.content)
            .map(|answer| (answer, response.prompt_eval_count)),
        Err(e) => Err(e),
    };
    let (outcome, answer, error_message, prompt_tokens) = match reply {
        Ok((answer, tokens)) => (
            score(&question.expected_choice, &answer),
            Some(answer),
            None,
            tokens,
        ),
        Err(e) => (Outcome::Error, None, Some(e.to_string()), None),
    };
    AiEvalResult {
        id: Some(ObjectId::new().to_string()),
        run_id: Some(plan.run_id.clone()),
        example_id: Some(question.id.clone()),
        expected_choice_snapshot: Some(question.expected_choice.clone()),
        outcome: Some(outcome),
        model_choice: answer.as_ref().map(|a| a.choice.clone()),
        model_explanation: answer.map(|a| a.explanation),
        error_message,
        prompt_tokens,
        duration_ms: Some(started.elapsed().as_millis() as i64),
        create_date: Some(DateTime::now()),
    }
}

async fn finish_run(run_id: &str, outcome: Result<()>) {
    let (status, error_message) = match outcome {
        Ok(()) => (RunStatus::Done, None),
        Err(e) => (RunStatus::Failed, Some(e.to_string())),
    };
    let finished = AiEvalRun {
        status: Some(status),
        error_message,
        finish_date: Some(DateTime::now()),
        ..Default::default()
    };
    if let Err(e) = AiEvalRun::update_by_map(pool!(), &finished, value! {"id": run_id}).await {
        log::error!("[abs_admin] could not finish test run {}: {}", run_id, e);
    }
}

/// Pairs up the answers both runs gave to the same test example, in run A's order.
fn compare_results(a: Vec<AiEvalResultRow>, b: Vec<AiEvalResultRow>) -> Vec<AiCompareRowVO> {
    let mut b_by_example: HashMap<String, AiEvalResultRow> = b
        .into_iter()
        .filter_map(|row| row.result.example_id.clone().map(|id| (id, row)))
        .collect();
    a.into_iter()
        .filter_map(|row_a| {
            let example_id = row_a.result.example_id.clone()?;
            let row_b = b_by_example.remove(&example_id)?;
            Some(AiCompareRowVO {
                example_id: Some(example_id),
                question_text: row_a.question_text,
                expected_choice: row_a.result.expected_choice_snapshot,
                a_outcome: row_a.result.outcome,
                a_choice: row_a.result.model_choice,
                b_outcome: row_b.result.outcome,
                b_choice: row_b.result.model_choice,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::compare_results;
    use crate::domain::table::ai::{AiEvalResult, AiEvalResultRow, Outcome};

    fn row(example_id: &str, outcome: Outcome) -> AiEvalResultRow {
        AiEvalResultRow {
            result: AiEvalResult {
                example_id: Some(example_id.to_string()),
                expected_choice_snapshot: Some("BUY".to_string()),
                outcome: Some(outcome),
                ..Default::default()
            },
            question_text: None,
        }
    }

    #[test]
    fn compares_only_the_examples_both_runs_answered() {
        let a = vec![
            row("1", Outcome::Wrong),
            row("2", Outcome::Correct),
            row("3", Outcome::Wrong),
        ];
        let b = vec![
            row("2", Outcome::Correct),
            row("3", Outcome::Correct),
            row("4", Outcome::Correct),
        ];

        let rows = compare_results(a, b);

        let ids: Vec<_> = rows.iter().map(|r| r.example_id.as_deref()).collect();
        assert_eq!(ids, [Some("2"), Some("3")]);
        assert_eq!(rows[1].a_outcome, Some(Outcome::Wrong));
        assert_eq!(rows[1].b_outcome, Some(Outcome::Correct));
    }
}

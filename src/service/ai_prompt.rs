//! How a model version's examples become a prompt, and how an answer is scored.
//! Pure functions, so the rules can be tested without Ollama or a database.
use crate::domain::table::ai::Outcome;
use crate::error::{Error, Result};
use crate::service::ollama_client::ChatMessage;
use serde::{Deserialize, Serialize};

/// An example as the prompt needs it: question, images, and (for worked examples) the answer.
#[derive(Clone, Debug)]
pub struct PromptExample {
    pub id: String,
    pub question_text: String,
    pub images_base64: Vec<String>,
    pub expected_choice: String,
    pub expected_explanation: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct ModelAnswer {
    pub choice: String,
    pub explanation: String,
}

/// Choices are compared trimmed and upper-case, so "buy " and "BUY" are the same answer.
pub fn normalize_choice(choice: &str) -> String {
    choice.trim().to_uppercase()
}

pub fn system_prompt(choices: &[String], instructions: &str) -> String {
    let mut prompt = format!(
        "Answer each question the way the worked examples before it are answered. \
         A question has text, images, or both.\n\
         Reply with JSON only: {{\"choice\": <one of {}>, \"explanation\": <one or two sentences>}}.\n\
         Write the explanation in the language of the question.",
        choices.join(", ")
    );
    if !instructions.trim().is_empty() {
        prompt.push_str("\n\n");
        prompt.push_str(instructions.trim());
    }
    prompt
}

/// The worked examples come before the question, so every question in a test run shares the same
/// prompt prefix, which Ollama can reuse instead of reading the examples again.
pub fn build_messages<'a>(
    system_prompt: &str,
    worked_examples: &'a [PromptExample],
    question: &'a PromptExample,
) -> Vec<ChatMessage<'a>> {
    let mut messages = vec![ChatMessage {
        role: "system",
        content: system_prompt.to_string(),
        images: vec![],
    }];
    for example in worked_examples {
        messages.push(user_message(example));
        let answer = ModelAnswer {
            choice: example.expected_choice.clone(),
            explanation: example.expected_explanation.clone().unwrap_or_default(),
        };
        messages.push(ChatMessage {
            role: "assistant",
            content: serde_json::to_string(&answer).unwrap_or_default(),
            images: vec![],
        });
    }
    messages.push(user_message(question));
    messages
}

fn user_message(example: &PromptExample) -> ChatMessage<'_> {
    ChatMessage {
        role: "user",
        content: example.question_text.clone(),
        images: example.images_base64.iter().map(String::as_str).collect(),
    }
}

/// JSON schema for Ollama's `format`: the model can only answer with one of the choices.
pub fn answer_schema(choices: &[String]) -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "choice": { "type": "string", "enum": choices },
            "explanation": { "type": "string" }
        },
        "required": ["choice", "explanation"]
    })
}

pub fn parse_answer(content: &str) -> Result<ModelAnswer> {
    serde_json::from_str(content.trim()).map_err(|e| {
        Error::from(format!(
            "answer is not the expected JSON ({}): {}",
            e, content
        ))
    })
}

pub fn score(expected_choice: &str, answer: &ModelAnswer) -> Outcome {
    if normalize_choice(expected_choice) == normalize_choice(&answer.choice) {
        Outcome::Correct
    } else {
        Outcome::Wrong
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn example(id: &str, choice: &str, images: &[&str]) -> PromptExample {
        PromptExample {
            id: id.to_string(),
            question_text: format!("question {}", id),
            images_base64: images.iter().map(|s| s.to_string()).collect(),
            expected_choice: choice.to_string(),
            expected_explanation: Some(format!("because {}", id)),
        }
    }

    #[test]
    fn worked_examples_come_before_the_question_as_user_assistant_pairs() {
        let worked = vec![example("1", "BUY", &["img1"]), example("2", "SELL", &[])];
        let question = example("q", "HOLD", &["imgq"]);

        let messages = build_messages("rules", &worked, &question);

        let roles: Vec<&str> = messages.iter().map(|m| m.role).collect();
        assert_eq!(
            roles,
            ["system", "user", "assistant", "user", "assistant", "user"]
        );
        assert_eq!(messages[1].images, ["img1"]);
        assert_eq!(
            messages[2].content,
            r#"{"choice":"BUY","explanation":"because 1"}"#
        );
        assert_eq!(messages[5].content, "question q");
        assert_eq!(messages[5].images, ["imgq"]);
    }

    #[test]
    fn the_question_never_reveals_its_expected_answer() {
        let question = example("q", "HOLD", &[]);
        let messages = build_messages("rules", &[], &question);
        assert!(messages.iter().all(|m| !m.content.contains("HOLD")));
    }

    #[test]
    fn instructions_are_added_after_the_answer_rules() {
        let prompt = system_prompt(
            &["BUY".into(), "SELL".into()],
            "  Only trade with the trend. ",
        );
        assert!(prompt.contains("one of BUY, SELL"));
        assert!(prompt.ends_with("Only trade with the trend."));
    }

    #[test]
    fn scoring_ignores_case_and_surrounding_spaces() {
        let answer = ModelAnswer {
            choice: " buy".into(),
            explanation: String::new(),
        };
        assert_eq!(score("BUY", &answer), Outcome::Correct);
        assert_eq!(score("SELL", &answer), Outcome::Wrong);
    }

    #[test]
    fn rejects_an_answer_that_is_not_json() {
        assert!(parse_answer("I think BUY").is_err());
        assert_eq!(
            parse_answer(r#" {"choice":"BUY","explanation":"up"} "#).ok(),
            Some(ModelAnswer {
                choice: "BUY".into(),
                explanation: "up".into()
            })
        );
    }
}

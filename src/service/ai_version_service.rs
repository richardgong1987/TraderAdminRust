use crate::context::CONTEXT;
use crate::domain::dto::{AiVersionAddDTO, ExampleSelection};
use crate::domain::table::ai::{AiExample, AiModelVersion, AiModelVersionExample, ExamplePurpose};
use crate::domain::vo::AiVersionVO;
use crate::error::{Error, Result};
use crate::util::tx::commit_or_rollback;
use crate::{error_info, pool};
use rbatis::object_id::ObjectId;
use rbatis::rbdc::DateTime;
use rbs::value;

/// A model version is "training" in prompt form: instructions plus the training examples it learns from.
pub struct AiVersionService {}

impl AiVersionService {
    pub async fn list(&self) -> Result<Vec<AiVersionVO>> {
        let rows = AiModelVersion::select_list(pool!()).await?;
        Ok(rows.into_iter().map(AiVersionVO::from).collect())
    }

    /// "Train": returns the new version's id.
    pub async fn add(&self, dto: AiVersionAddDTO, user_id: &str) -> Result<String> {
        let example_ids = self.resolve_examples(&dto).await?;
        let id = ObjectId::new().to_string();
        let name = match dto.name.as_deref().map(str::trim) {
            Some(name) if !name.is_empty() => name.to_string(),
            _ => self.next_default_name().await?,
        };
        let version = AiModelVersion {
            id: Some(id.clone()),
            name: Some(name),
            base_model: Some(CONTEXT.config.ollama_model.clone()),
            instructions: Some(dto.instructions.trim().to_string()),
            create_by: Some(user_id.to_string()),
            create_date: Some(DateTime::now()),
        };
        let links: Vec<AiModelVersionExample> = example_ids
            .into_iter()
            .map(|example_id| AiModelVersionExample {
                id: Some(ObjectId::new().to_string()),
                version_id: Some(id.clone()),
                example_id: Some(example_id),
            })
            .collect();

        let tx = pool!().acquire_begin().await?;
        let saved: Result<()> = async {
            AiModelVersion::insert(&tx, &version).await?;
            if !links.is_empty() {
                AiModelVersionExample::insert_batch(&tx, &links, links.len() as u64).await?;
            }
            Ok(())
        }
        .await;
        commit_or_rollback(tx, saved).await?;
        Ok(id)
    }

    /// Its links to examples and its test runs go with it (foreign key cascade).
    pub async fn remove(&self, id: &str) -> Result<u64> {
        Ok(AiModelVersion::delete_by_map(pool!(), value! {"id": id})
            .await?
            .rows_affected)
    }

    pub async fn get(&self, id: &str) -> Result<AiModelVersion> {
        AiModelVersion::select_by_map(pool!(), value! {"id": id})
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| Error::from(error_info!("ai.version_not_found")))
    }

    pub async fn example_ids(&self, version_id: &str) -> Result<Vec<String>> {
        let links =
            AiModelVersionExample::select_by_map(pool!(), value! {"version_id": version_id})
                .await?;
        Ok(links.into_iter().filter_map(|l| l.example_id).collect())
    }

    /// Only training examples may be learned from: a test example in the prompt would make the
    /// test score meaningless.
    async fn resolve_examples(&self, dto: &AiVersionAddDTO) -> Result<Vec<String>> {
        match dto.selection {
            ExampleSelection::None => Ok(vec![]),
            ExampleSelection::All => {
                CONTEXT
                    .ai_example_service
                    .ids_by_purpose(ExamplePurpose::Train)
                    .await
            }
            ExampleSelection::Selected => {
                let selected = AiExample::select_by_map(
                    pool!(),
                    value! {"id": &dto.example_ids, "column": ["id", "purpose"]},
                )
                .await?;
                let all_training = selected.len() == dto.example_ids.len()
                    && selected
                        .iter()
                        .all(|e| e.purpose == Some(ExamplePurpose::Train));
                if !all_training {
                    return Err(Error::from(error_info!("ai.not_training_example")));
                }
                Ok(dto.example_ids.clone())
            }
        }
    }

    async fn next_default_name(&self) -> Result<String> {
        let count = AiModelVersion::select_by_map(pool!(), value! {"column": ["id"]})
            .await?
            .len();
        Ok(format!("v{}", count + 1))
    }
}

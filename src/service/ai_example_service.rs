use crate::domain::dto::{AiExamplePageDTO, AiExampleSaveDTO, AiImageDTO};
use crate::domain::table::ai::{AiExample, AiExampleImage, ExamplePurpose};
use crate::domain::vo::{AiExampleDetailVO, AiExampleVO, AiImageVO};
use crate::error::{Error, Result};
use crate::service::ai_prompt::{PromptExample, normalize_choice};
use crate::util::tx::commit_or_rollback;
use crate::{error_info, pool};
use rbatis::executor::Executor;
use rbatis::object_id::ObjectId;
use rbatis::rbdc::DateTime;
use rbatis::{Page, PageRequest};
use rbs::value;
use std::collections::HashMap;

/// Images are the costliest part of a prompt; a handful per example is plenty.
const MAX_IMAGES: usize = 5;

pub struct AiExampleService {}

impl AiExampleService {
    pub async fn page(&self, dto: &AiExamplePageDTO) -> Result<Page<AiExampleVO>> {
        let page = AiExample::select_page(pool!(), &PageRequest::from(dto), dto).await?;
        Ok(Page::<AiExampleVO>::from(page))
    }

    pub async fn detail(&self, id: &str) -> Result<AiExampleDetailVO> {
        let example = AiExample::select_by_map(pool!(), value! {"id": id})
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| Error::from(error_info!("ai.example_not_found")))?;
        let images = self.images_of(&[id.to_string()]).await?;
        let mut vo = AiExampleVO::from(example);
        vo.image_count = images.len() as u64;
        Ok(AiExampleDetailVO {
            example: vo,
            images: images.into_iter().map(AiImageVO::from).collect(),
        })
    }

    /// Returns the new example's id.
    pub async fn add(&self, dto: AiExampleSaveDTO, user_id: &str) -> Result<String> {
        validate(&dto)?;
        let id = ObjectId::new().to_string();
        let mut example = editable_fields(&dto);
        example.id = Some(id.clone());
        example.create_by = Some(user_id.to_string());
        example.create_date = example.update_date.clone();
        let tx = pool!().acquire_begin().await?;
        let saved = async {
            AiExample::insert(&tx, &example).await?;
            insert_images(&tx, &id, dto.images).await
        }
        .await;
        commit_or_rollback(tx, saved).await?;
        Ok(id)
    }

    pub async fn update(&self, dto: AiExampleSaveDTO) -> Result<()> {
        validate(&dto)?;
        let id = dto.id.clone().unwrap_or_default();
        let changes = editable_fields(&dto);
        let tx = pool!().acquire_begin().await?;
        let saved = async {
            // Listing the columns makes rbatis write NULLs too, so a removed explanation is cleared.
            let updated = AiExample::update_by_map(
                &tx,
                &changes,
                value! {"id": &id, "column": ["purpose", "question_text", "expected_choice", "expected_explanation", "update_date"]},
            )
            .await?;
            if updated.rows_affected == 0 {
                return Err(Error::from(error_info!("ai.example_not_found")));
            }
            AiExampleImage::delete_by_map(&tx, value! {"example_id": &id}).await?;
            insert_images(&tx, &id, dto.images).await
        }
        .await;
        commit_or_rollback(tx, saved).await
    }

    /// Its images go with it (foreign key cascade).
    pub async fn remove(&self, id: &str) -> Result<u64> {
        Ok(AiExample::delete_by_map(pool!(), value! {"id": id})
            .await?
            .rows_affected)
    }

    /// Every choice used by any example: the answers a model may give.
    pub async fn choices(&self) -> Result<Vec<String>> {
        Ok(AiExample::select_choices(pool!())
            .await?
            .into_iter()
            .map(|row| row.expected_choice)
            .collect())
    }

    pub async fn ids_by_purpose(&self, purpose: ExamplePurpose) -> Result<Vec<String>> {
        let examples =
            AiExample::select_by_map(pool!(), value! {"purpose": purpose, "column": ["id"]})
                .await?;
        Ok(examples.into_iter().filter_map(|e| e.id).collect())
    }

    /// Examples with their images, ready for a prompt, in the order of `ids`.
    pub async fn load_for_prompt(&self, ids: &[String]) -> Result<Vec<PromptExample>> {
        if ids.is_empty() {
            return Ok(vec![]);
        }
        let mut examples: HashMap<String, AiExample> =
            AiExample::select_by_map(pool!(), value! {"id": ids})
                .await?
                .into_iter()
                .filter_map(|e| e.id.clone().map(|id| (id, e)))
                .collect();
        let mut images: HashMap<String, Vec<String>> = HashMap::new();
        for image in self.images_of(ids).await? {
            images
                .entry(image.example_id.unwrap_or_default())
                .or_default()
                .push(image.data_base64.unwrap_or_default());
        }
        Ok(ids
            .iter()
            .filter_map(|id| {
                let example = examples.remove(id)?;
                Some(PromptExample {
                    id: id.clone(),
                    question_text: example.question_text.unwrap_or_default(),
                    images_base64: images.remove(id).unwrap_or_default(),
                    expected_choice: example.expected_choice.unwrap_or_default(),
                    expected_explanation: example.expected_explanation,
                })
            })
            .collect())
    }

    /// Images of the given examples, each example's in paste order.
    async fn images_of(&self, example_ids: &[String]) -> Result<Vec<AiExampleImage>> {
        let mut images =
            AiExampleImage::select_by_map(pool!(), value! {"example_id": example_ids}).await?;
        images.sort_by_key(|i| i.sort_no);
        Ok(images)
    }
}

fn validate(dto: &AiExampleSaveDTO) -> Result<()> {
    if dto.question_text.trim().is_empty() && dto.images.is_empty() {
        return Err(Error::from(error_info!("ai.example_empty")));
    }
    if normalize_choice(&dto.expected_choice).is_empty() {
        return Err(Error::from(error_info!("ai.choice_empty")));
    }
    if dto.images.len() > MAX_IMAGES {
        return Err(Error::from(error_info!("ai.too_many_images")));
    }
    if dto
        .images
        .iter()
        .any(|i| !i.mime_type.starts_with("image/"))
    {
        return Err(Error::from(error_info!("ai.image_invalid")));
    }
    Ok(())
}

/// The fields a user edits, cleaned up: trimmed text, normalized choice, blank explanation = none.
fn editable_fields(dto: &AiExampleSaveDTO) -> AiExample {
    AiExample {
        purpose: Some(dto.purpose),
        question_text: Some(dto.question_text.trim().to_string()),
        expected_choice: Some(normalize_choice(&dto.expected_choice)),
        expected_explanation: dto
            .expected_explanation
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(str::to_string),
        update_date: Some(DateTime::now()),
        ..Default::default()
    }
}

async fn insert_images(
    executor: &dyn Executor,
    example_id: &str,
    images: Vec<AiImageDTO>,
) -> Result<()> {
    let rows: Vec<AiExampleImage> = images
        .into_iter()
        .enumerate()
        .map(|(index, image)| AiExampleImage {
            id: Some(ObjectId::new().to_string()),
            example_id: Some(example_id.to_string()),
            sort_no: Some(index as i32),
            mime_type: Some(image.mime_type),
            data_base64: Some(image.data_base64),
            create_date: Some(DateTime::now()),
        })
        .collect();
    if !rows.is_empty() {
        AiExampleImage::insert_batch(executor, &rows, rows.len() as u64).await?;
    }
    Ok(())
}

//! AI training and testing endpoints. Design: doc/ai-training.md.
use crate::context::CONTEXT;
use crate::domain::dto::{
    AiEvalCompareDTO, AiEvalStartDTO, AiExamplePageDTO, AiExampleSaveDTO, AiVersionAddDTO, IdDTO,
};
use crate::domain::vo::RespVO;
use crate::error_info;
use crate::middleware::auth_axum::JwtAuth;
use axum::Json;
use axum::response::IntoResponse;

pub async fn example_page(arg: Json<AiExamplePageDTO>) -> impl IntoResponse {
    RespVO::from_result(CONTEXT.ai_example_service.page(&arg.0).await)
}

pub async fn example_detail(arg: Json<IdDTO>) -> impl IntoResponse {
    let id = arg.0.id.unwrap_or_default();
    RespVO::from_result(CONTEXT.ai_example_service.detail(&id).await)
}

pub async fn example_add(jwt: JwtAuth, arg: Json<AiExampleSaveDTO>) -> impl IntoResponse {
    RespVO::from_result(CONTEXT.ai_example_service.add(arg.0, &jwt.id).await)
}

pub async fn example_update(arg: Json<AiExampleSaveDTO>) -> impl IntoResponse {
    if arg.id.is_none() {
        return RespVO::<()>::from_error(error_info!("id_empty"));
    }
    RespVO::from_result(CONTEXT.ai_example_service.update(arg.0).await)
}

pub async fn example_remove(arg: Json<IdDTO>) -> impl IntoResponse {
    let id = arg.0.id.unwrap_or_default();
    RespVO::from_result(CONTEXT.ai_example_service.remove(&id).await)
}

pub async fn choices() -> impl IntoResponse {
    RespVO::from_result(CONTEXT.ai_example_service.choices().await)
}

pub async fn version_list() -> impl IntoResponse {
    RespVO::from_result(CONTEXT.ai_version_service.list().await)
}

/// "Train": create a model version.
pub async fn version_add(jwt: JwtAuth, arg: Json<AiVersionAddDTO>) -> impl IntoResponse {
    RespVO::from_result(CONTEXT.ai_version_service.add(arg.0, &jwt.id).await)
}

pub async fn version_remove(arg: Json<IdDTO>) -> impl IntoResponse {
    let id = arg.0.id.unwrap_or_default();
    RespVO::from_result(CONTEXT.ai_version_service.remove(&id).await)
}

/// Returns at once with the run id; the test runs in the background.
pub async fn eval_start(jwt: JwtAuth, arg: Json<AiEvalStartDTO>) -> impl IntoResponse {
    RespVO::from_result(
        CONTEXT
            .ai_eval_service
            .start(&arg.0.version_id, &jwt.id)
            .await,
    )
}

pub async fn eval_list() -> impl IntoResponse {
    RespVO::from_result(CONTEXT.ai_eval_service.list().await)
}

pub async fn eval_detail(arg: Json<IdDTO>) -> impl IntoResponse {
    let id = arg.0.id.unwrap_or_default();
    RespVO::from_result(CONTEXT.ai_eval_service.detail(&id).await)
}

pub async fn eval_compare(arg: Json<AiEvalCompareDTO>) -> impl IntoResponse {
    RespVO::from_result(
        CONTEXT
            .ai_eval_service
            .compare(&arg.0.run_a, &arg.0.run_b)
            .await,
    )
}

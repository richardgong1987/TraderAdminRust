use crate::context::CONTEXT;
use crate::domain::vo::RespVO;
use crate::middleware::auth_axum::JwtAuth;
use crate::service::{UPLOAD_ROOT, check_inside, user_upload_dir};
use axum::body::Body;
use axum::extract::Multipart;
use axum::extract::Query;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use std::path::Path;

///上传，返回外网访问地址
pub async fn upload(jwt_auth: JwtAuth, mut multipart: Multipart) -> impl IntoResponse {
    let file_opt = match multipart.next_field().await {
        Ok(f) => f,
        Err(e) => return RespVO::from_error(format!("read multipart error: {}", e)),
    };

    let Some(file) = file_opt else {
        return RespVO::from_error("no file".to_string());
    };

    let Some(filename) = file.file_name().map(|s| s.to_string()) else {
        return RespVO::from_error("no filename".to_string());
    };

    let data = match file.bytes().await {
        Ok(d) => d,
        Err(e) => return RespVO::from_error(format!("read bytes error: {}", e)),
    };

    let user_dir = user_upload_dir(&jwt_auth.id);
    let name = user_dir.join(&filename).to_string_lossy().to_string();
    if let Err(e) = check_inside(&user_dir, &name) {
        return RespVO::from_error(e.to_string());
    }

    let path = CONTEXT
        .storage_service
        .upload(name, data.to_vec())
        .await
        .map(|v| format!("{}{}", CONTEXT.config.storage_host, v));
    RespVO::from_result(path)
}

#[derive(Deserialize)]
pub struct QueryParams {
    pub name: String,
}

/// Delete one of the caller's own uploads.
pub async fn delete(jwt_auth: JwtAuth, Query(param): Query<QueryParams>) -> impl IntoResponse {
    if let Err(e) = check_inside(&user_upload_dir(&jwt_auth.id), &param.name) {
        return RespVO::from_error(e.to_string());
    }
    let result = CONTEXT.storage_service.remove(param.name).await;
    RespVO::from_result(result)
}

/// Download an uploaded file. No login: the admin UI shows uploads with `<img src>`, which cannot
/// send the Authorization header. Only files under the upload folder are served.
pub async fn download(Query(param): Query<QueryParams>) -> impl IntoResponse {
    let data = match check_inside(Path::new(UPLOAD_ROOT), &param.name) {
        Ok(()) => CONTEXT.storage_service.download(param.name).await,
        Err(e) => Err(e),
    };
    match data {
        Ok(data) => Response::new(Body::from(data)).into_response(),
        Err(e) => {
            let resp = match Response::builder()
                .header("Access-Control-Allow-Origin", "*")
                .header("Cache-Control", "no-cache")
                .header("Content-Type", "json")
                .body(Body::from(
                    serde_json::json!(&RespVO::<()>::from_error(e.to_string())).to_string(),
                )) {
                Ok(r) => r,
                Err(_) => {
                    return Response::builder()
                        .header("Access-Control-Allow-Origin", "*")
                        .header("Cache-Control", "no-cache")
                        .header("Content-Type", "json")
                        .body(Body::from(r#"{"code":500,"msg":"internal error"}"#))
                        .unwrap_or_else(|_| Response::new(Body::from("internal error")));
                }
            };
            resp
        }
    }
}

use crate::error::{Error, Result};
use crate::error_info;
use crate::service::FileLocalService;
use async_trait::async_trait;
use std::fmt::Debug;
use std::ops::Deref;
use std::path::{Component, Path, PathBuf};

/// Uploaded files live here, one folder per user.
pub const UPLOAD_ROOT: &str = "target/upload";

pub fn user_upload_dir(user_id: &str) -> PathBuf {
    Path::new(UPLOAD_ROOT).join(user_id)
}

/// File names in requests come from clients, so each one must stay inside the folder the request
/// may touch. Rejects names like `/proc/self/environ` or `target/upload/1/../../../application.json5`.
pub fn check_inside(dir: &Path, name: &str) -> Result<()> {
    let path = Path::new(name);
    let stays_inside =
        path.starts_with(dir) && path.components().all(|c| matches!(c, Component::Normal(_)));
    if stays_inside {
        Ok(())
    } else {
        Err(Error::from(error_info!("access_denied")))
    }
}

#[async_trait]
pub trait IStorageService: Sync + Send + Debug {
    async fn upload(&self, name: String, data: Vec<u8>) -> Result<String>;
    async fn download(&self, name: String) -> Result<Vec<u8>>;
    async fn list(&self, name: String) -> Result<Vec<String>>;
    async fn remove(&self, name: String) -> Result<()>;
}

pub struct StorageService {
    pub inner: Box<dyn IStorageService>,
}
impl Deref for StorageService {
    type Target = dyn IStorageService;

    fn deref(&self) -> &Self::Target {
        self.inner.as_ref()
    }
}

impl StorageService {
    pub fn new(storage: &str) -> Result<StorageService> {
        if storage == "local" {
            return Ok(Self {
                inner: Box::new(FileLocalService::new()),
            });
        } else if storage.starts_with("s3://") {
            #[cfg(feature = "storage_s3")]
            {
                return Ok(Self {
                    inner: Box::new(crate::service::FileS3Service::new(
                        crate::service::S3Config::load(storage)?,
                    )),
                });
            }
        }
        Err(Error::from(format!(
            "Unsupported storage service: {}",
            storage
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::{UPLOAD_ROOT, check_inside, user_upload_dir};
    use std::path::Path;

    #[test]
    fn accepts_a_file_inside_the_folder() {
        assert!(check_inside(&user_upload_dir("1"), "target/upload/1/chart.png").is_ok());
    }

    #[test]
    fn rejects_a_path_that_climbs_out_with_dot_dot() {
        let name = "target/upload/1/../../../application.json5";
        assert!(check_inside(&user_upload_dir("1"), name).is_err());
    }

    #[test]
    fn rejects_an_absolute_path() {
        assert!(check_inside(Path::new(UPLOAD_ROOT), "/proc/self/environ").is_err());
    }

    #[test]
    fn rejects_a_file_outside_the_upload_root() {
        assert!(check_inside(Path::new(UPLOAD_ROOT), "application.json5").is_err());
        assert!(check_inside(Path::new(UPLOAD_ROOT), "target/upload_other/a.png").is_err());
    }

    #[test]
    fn rejects_another_users_file() {
        assert!(check_inside(&user_upload_dir("1"), "target/upload/2/chart.png").is_err());
    }
}

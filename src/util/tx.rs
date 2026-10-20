use crate::error::Result;
use rbatis::executor::RBatisTxExecutor;

/// Commits when every step inside the transaction succeeded; otherwise rolls back and returns the
/// first error. Usage: `let steps = async { ... }.await; commit_or_rollback(tx, steps).await`.
pub async fn commit_or_rollback<T>(tx: RBatisTxExecutor, steps: Result<T>) -> Result<T> {
    match steps {
        Ok(value) => {
            tx.commit().await?;
            Ok(value)
        }
        Err(e) => {
            let _ = tx.rollback().await;
            Err(e)
        }
    }
}

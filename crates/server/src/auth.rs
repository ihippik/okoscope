use sha2::{Digest, Sha256};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SessionScope {
    pub organization_id: Uuid,
    pub cluster_id: Uuid,
}

#[derive(Clone, Debug)]
pub struct CredentialAuthenticator {
    pool: PgPool,
}

impl CredentialAuthenticator {
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn authenticate(
        &self,
        credential: &str,
    ) -> Result<Option<SessionScope>, sqlx::Error> {
        if credential.is_empty() {
            return Ok(None);
        }
        let digest = Sha256::digest(credential.as_bytes()).to_vec();
        let scope = sqlx::query_as::<_, (Uuid, Uuid)>(
            "SELECT organization_id, cluster_id FROM cluster_credentials WHERE credential_hash = $1 AND revoked_at IS NULL",
        )
        .bind(digest)
        .fetch_optional(&self.pool)
        .await?;
        Ok(scope.map(|(organization_id, cluster_id)| SessionScope {
            organization_id,
            cluster_id,
        }))
    }
}

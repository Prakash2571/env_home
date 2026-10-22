use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use time::OffsetDateTime;
use tokio::sync::Mutex;
use tokio_postgres::{Client, NoTls, Row};

use super::{AttemptRecord, MIGRATIONS, RequestRecord, StoredToken};

#[derive(Clone)]
pub struct PgStore {
    params: Arc<String>,
    client: Arc<Mutex<Option<Arc<Client>>>>,
}

fn moment(unix: i64) -> Result<OffsetDateTime> {
    OffsetDateTime::from_unix_timestamp(unix)
        .map_err(|_| anyhow!("timestamp {unix} is out of range"))
}

fn unix(row: &Row, column: &str) -> i64 {
    row.get::<_, OffsetDateTime>(column).unix_timestamp()
}

impl PgStore {
    pub fn new(params: &str) -> Self {
        Self {
            params: Arc::new(params.to_owned()),
            client: Arc::new(Mutex::new(None)),
        }
    }

    async fn client(&self) -> Result<Arc<Client>> {
        let mut slot = self.client.lock().await;
        if let Some(client) = slot.as_ref().filter(|client| !client.is_closed()) {
            return Ok(Arc::clone(client));
        }
        let (mut client, connection) = tokio_postgres::connect(&self.params, NoTls)
            .await
            .context("could not connect to Postgres")?;
        tokio::spawn(async move {
            if let Err(error) = connection.await {
                tracing::warn!(%error, "the Postgres connection ended");
            }
        });
        migrate(&mut client).await?;
        let client = Arc::new(client);
        *slot = Some(Arc::clone(&client));
        Ok(client)
    }

    pub async fn is_reachable(&self) -> bool {
        match self.client().await {
            Ok(client) => client.simple_query("SELECT 1").await.is_ok(),
            Err(_) => false,
        }
    }

    pub async fn save_token(&self, token: &StoredToken) -> Result<()> {
        self.client()
            .await?
            .execute(
                "INSERT INTO broker_tokens (alias, client_id, sealed_token, obtained_at, expires_at, drive) \
                 VALUES ($1, $2, $3, $4, $5, $6)",
                &[
                    &token.alias,
                    &token.client_id,
                    &token.sealed,
                    &moment(token.obtained_at)?,
                    &moment(token.expires_at)?,
                    &token.drive,
                ],
            )
            .await
            .context("could not store the token")?;
        Ok(())
    }

    pub async fn latest_token(&self, alias: &str) -> Result<Option<StoredToken>> {
        let row = self
            .client()
            .await?
            .query_opt(
                "SELECT alias, client_id, sealed_token, obtained_at, expires_at, drive FROM broker_tokens \
                 WHERE alias = $1 ORDER BY obtained_at DESC, id DESC LIMIT 1",
                &[&alias],
            )
            .await
            .context("could not read the latest token")?;
        Ok(row.map(|row| StoredToken {
            alias: row.get("alias"),
            client_id: row.get("client_id"),
            sealed: row.get("sealed_token"),
            obtained_at: unix(&row, "obtained_at"),
            expires_at: unix(&row, "expires_at"),
            drive: row.get("drive"),
        }))
    }

    pub async fn record_attempt(&self, attempt: &AttemptRecord) -> Result<()> {
        self.client()
            .await?
            .execute(
                "INSERT INTO login_attempts (alias, drive, started_at, finished_at, outcome, stage, detail, consent_spent) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
                &[
                    &attempt.alias,
                    &attempt.drive,
                    &moment(attempt.started_at)?,
                    &moment(attempt.finished_at)?,
                    &attempt.outcome,
                    &attempt.stage,
                    &attempt.detail,
                    &attempt.consent_spent,
                ],
            )
            .await
            .context("could not record the login attempt")?;
        Ok(())
    }

    pub async fn recent_attempts(&self, limit: i64) -> Result<Vec<AttemptRecord>> {
        let rows = self
            .client()
            .await?
            .query(
                "SELECT alias, drive, started_at, finished_at, outcome, stage, detail, consent_spent \
                 FROM login_attempts ORDER BY started_at DESC, id DESC LIMIT $1",
                &[&limit],
            )
            .await
            .context("could not read login attempts")?;
        Ok(rows
            .iter()
            .map(|row| AttemptRecord {
                alias: row.get("alias"),
                drive: row.get("drive"),
                started_at: unix(row, "started_at"),
                finished_at: unix(row, "finished_at"),
                outcome: row.get("outcome"),
                stage: row.get("stage"),
                detail: row.get("detail"),
                consent_spent: row.get("consent_spent"),
            })
            .collect())
    }

    pub async fn consents_since(&self, alias: &str, since: i64) -> Result<u32> {
        let row = self
            .client()
            .await?
            .query_one(
                "SELECT count(*) FROM login_attempts WHERE alias = $1 AND consent_spent AND started_at >= $2",
                &[&alias, &moment(since)?],
            )
            .await
            .context("could not count today's consents")?;
        Ok(u32::try_from(row.get::<_, i64>(0)).unwrap_or(u32::MAX))
    }

    pub async fn prune(&self, history_before: i64, tokens_before: i64) -> Result<u64> {
        let client = self.client().await?;
        let history = moment(history_before)?;
        let mut removed = 0;
        removed += client
            .execute("DELETE FROM token_requests WHERE at < $1", &[&history])
            .await
            .context("could not prune token requests")?;
        removed += client
            .execute(
                "DELETE FROM login_attempts WHERE started_at < $1",
                &[&history],
            )
            .await
            .context("could not prune login attempts")?;
        removed += client
            .execute(
                "DELETE FROM broker_tokens WHERE obtained_at < $1",
                &[&moment(tokens_before)?],
            )
            .await
            .context("could not prune old tokens")?;
        Ok(removed)
    }

    pub async fn record_request(&self, request: &RequestRecord) -> Result<()> {
        self.client()
            .await?
            .execute(
                "INSERT INTO token_requests (at, client_id, alias, ip, outcome, reason) VALUES ($1, $2, $3, $4, $5, $6)",
                &[
                    &moment(request.at)?,
                    &request.client_id,
                    &request.alias,
                    &request.ip,
                    &request.outcome,
                    &request.reason,
                ],
            )
            .await
            .context("could not record the token request")?;
        Ok(())
    }

    pub async fn recent_requests(&self, limit: i64) -> Result<Vec<RequestRecord>> {
        let rows = self
            .client()
            .await?
            .query(
                "SELECT at, client_id, alias, ip, outcome, reason FROM token_requests ORDER BY at DESC, id DESC LIMIT $1",
                &[&limit],
            )
            .await
            .context("could not read token requests")?;
        Ok(rows
            .iter()
            .map(|row| RequestRecord {
                at: unix(row, "at"),
                client_id: row.get("client_id"),
                alias: row.get("alias"),
                ip: row.get("ip"),
                outcome: row.get("outcome"),
                reason: row.get("reason"),
            })
            .collect())
    }
}

/// Any fixed key works; it only has to be the same for every instance of this service.
const MIGRATION_LOCK: i64 = 0x0000_6272_6f6b_6572;

/// Runs every pending migration in one transaction behind an advisory lock, so
/// instances starting together take turns instead of racing on the catalog.
async fn migrate(client: &mut Client) -> Result<()> {
    let transaction = client
        .transaction()
        .await
        .context("could not start the migration transaction")?;
    transaction
        .execute("SELECT pg_advisory_xact_lock($1)", &[&MIGRATION_LOCK])
        .await
        .context("could not take the migration lock")?;
    transaction
        .batch_execute(
            "CREATE TABLE IF NOT EXISTS schema_migrations (version INTEGER PRIMARY KEY, applied_at TIMESTAMPTZ NOT NULL DEFAULT now())",
        )
        .await
        .context("could not create schema_migrations")?;
    for (version, sql) in MIGRATIONS {
        let applied = transaction
            .query_opt(
                "SELECT 1 FROM schema_migrations WHERE version = $1",
                &[&version],
            )
            .await
            .context("could not read schema_migrations")?
            .is_some();
        if applied {
            continue;
        }
        transaction
            .batch_execute(sql)
            .await
            .with_context(|| format!("migration {version} failed"))?;
        transaction
            .execute(
                "INSERT INTO schema_migrations (version) VALUES ($1)",
                &[&version],
            )
            .await
            .with_context(|| format!("could not record migration {version}"))?;
        tracing::info!(version, "applied a database migration");
    }
    transaction
        .commit()
        .await
        .context("could not commit the migrations")
}

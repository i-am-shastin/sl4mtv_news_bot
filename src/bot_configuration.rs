use sqlx::SqlitePool;
use anyhow::Result;

use crate::admin_storage::BotDataRow;

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct BotConfiguration {
    pub comment_each_n_post: u32,
}

impl Default for BotConfiguration {
    fn default() -> Self {
        Self {
            comment_each_n_post: 7,
        }
    }
}

impl BotConfiguration {
    pub async fn from_db(pool: &SqlitePool) -> Result<Self> {
        let row = sqlx::query_as::<_, BotDataRow>("SELECT data FROM bot_configuration ORDER BY id DESC LIMIT 1")
            .fetch_optional(pool)
            .await?;

        Ok(row.map_or(Self::default(), |r| {
            serde_json::from_str(&r.data).unwrap_or_default()
        }))
    }

    pub async fn to_db(&self, pool: &SqlitePool) -> Result<bool> {
        let configuration_as_json = serde_json::to_string(&self)?;

        Ok(
            sqlx::query("INSERT INTO bot_configuration (data) VALUES (?)")
                .bind(configuration_as_json)
                .execute(pool)
                .await?
                .last_insert_rowid() > 0
        )
    }

    pub fn should_comment(&self, channel_posts_counter: u32) -> bool {
        self.comment_each_n_post > 0 && channel_posts_counter % self.comment_each_n_post == 0
    }
}

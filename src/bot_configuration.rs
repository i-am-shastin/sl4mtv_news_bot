use log::{info, warn};

use sqlx::SqlitePool;
use anyhow::Result;
use teloxide::{prelude::Requester, types::ChatId, Bot};

use crate::{admin_storage::BotDataRow, types::MyStorage, user_stats::UserStats};

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct BotConfiguration {
    pub comment_each_n_post: u32,
    pub apply_fix: bool,
}

impl Default for BotConfiguration {
    fn default() -> Self {
        Self {
            comment_each_n_post: 7,
            apply_fix: true,
        }
    }
}

impl BotConfiguration {
    pub fn should_apply_fix(&self) -> bool {
        self.apply_fix
    }

    pub async fn apply_fix(&mut self, bot: &Bot, pool: &SqlitePool, storage: &MyStorage) -> Result<bool> {
        let pending_users = sqlx::query_as::<_, UserStats>(
                "SELECT * FROM user_stats WHERE total_requests = 0"
            )
            .fetch_all(pool)
            .await?;

        info!("Found {} pending users", pending_users.len());

        for user in pending_users {
            if let Ok(Some(user_dialogue)) = storage.clone().get_dialogue(ChatId(user.user_id)).await {
                info!("Found dialogue for user {}", user.user_id);
                if user_dialogue.posts.values().any(|post| post.sent_to.is_empty()) {
                    info!("Sending fix message to user {}", user.user_id);
                    bot.send_message(
                        ChatId(user.user_id),
                        "Привет! Бот был запущен в тестовом режиме, и кое-что пошло не по плану :) Если твой пост ещё актуален - пожалуйста, прожми кнопку отправки ещё раз. Это автоматическое сообщение, отвечать на него не нужно."
                    ).await?;
                }
            } else {
                warn!("No dialogue found for user {}", user.user_id);
            }
        }

        self.apply_fix = false;
        Ok(true)
    }

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

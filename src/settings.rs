use std::env;
use anyhow::Result;
use teloxide::types::{ChatId, UserId};

#[derive(Clone)]
pub struct AppSettings {
    pub admins: Vec<ChatId>,
    pub db_filename: String,
    pub admin_db_filename: String,
    pub max_draft_posts: usize,
    pub max_sent_posts: usize,
}

impl AppSettings {
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            admins: env::var("BOT_ADMINS")?.split(',').map(|id| id.parse::<i64>().map(ChatId)).collect::<Result<Vec<ChatId>, _>>()?,
            db_filename: env::var("DB_FILENAME").unwrap_or("db.sqlite".to_string()),
            admin_db_filename: env::var("ADMIN_DB_FILENAME").unwrap_or("admin_db.sqlite".to_string()),
            max_draft_posts: env::var("MAX_DRAFT_POSTS")
                .ok()
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(15),
            max_sent_posts: env::var("MAX_SENT_POSTS")
                .ok()
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(3),
        })
    }

    pub fn is_admin(&self, user_id: UserId) -> bool {
        self.admins.contains(&user_id.into())
    }
}

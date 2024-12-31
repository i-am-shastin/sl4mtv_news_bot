use crate::user_stats::UserStats;

use std::sync::Arc;

use sqlx::sqlite::{SqlitePool, SqliteQueryResult};
use anyhow::Result;
use teloxide::types::{ChatId, MessageId, User, UserId};
use chrono::Utc;

pub struct AdminStorage {
    pool: SqlitePool,
}

#[derive(sqlx::FromRow)]
struct BotAutoResponseRow {
    data: String,
}

#[derive(sqlx::FromRow)]
struct AdminDbRow {
    reply_msg_id: i32,
    reply: String,
}

impl AdminStorage {
    pub async fn open(path: &str) -> Result<Arc<Self>> {
        let pool = SqlitePool::connect(format!("sqlite:{path}?mode=rwc").as_str()).await?;
        sqlx::query(
            "
CREATE TABLE IF NOT EXISTS user_stats (
    user_id BIGINT NOT NULL,
    user_name TEXT NOT NULL,
    total_requests BIGINT NOT NULL,
    requests_accepted BIGINT NOT NULL,
    requests_declined BIGINT NOT NULL,
    last_responded_request_date DATETIME,
    PRIMARY KEY(user_id)
);
            "
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "
CREATE TABLE IF NOT EXISTS bot_auto_response (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    data BLOB NOT NULL
);
            "
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "
CREATE TABLE IF NOT EXISTS admin_replies (
    request_msg_id BIGINT NOT NULL,
    admin_id BIGINT NOT NULL,
    reply_msg_id BIGINT NOT NULL,
    reply TEXT NOT NULL,
    PRIMARY KEY(admin_id, request_msg_id)
);
            "
        )
        .execute(&pool)
        .await?;

        Ok(Arc::new(Self { pool }))
    }

    pub async fn save_admin_message(&self, admin_id: UserId, request_msg_id: MessageId, reply_msg_id: MessageId, reply: &str) -> Result<SqliteQueryResult, sqlx::Error> {
        sqlx::query(
            "
        INSERT INTO admin_replies (admin_id, request_msg_id, reply_msg_id, reply) VALUES (?, ?, ?, ?)
        ON CONFLICT(admin_id, request_msg_id) DO UPDATE SET reply=excluded.reply, reply_msg_id=excluded.reply_msg_id
                "
            )
            .bind(admin_id.0 as i64)
            .bind(request_msg_id.0)
            .bind(reply_msg_id.0)
            .bind(reply)
            .execute(&self.pool)
            .await
    }

    pub async fn get_admin_message(&self, admin_id: UserId, request_msg_id: MessageId) -> Result<Option<(MessageId, String)>> {
        let admin_db_row = sqlx::query_as::<_, AdminDbRow>(
            "SELECT reply_msg_id, reply FROM admin_replies WHERE admin_id = ? AND request_msg_id = ?"
            )
            .bind(admin_id.0 as i64)
            .bind(request_msg_id.0)
            .fetch_optional(&self.pool)
            .await?;

        if let Some(row) = admin_db_row {
            self.delete_admin_message(admin_id, request_msg_id).await?;
            Ok(Some((MessageId(row.reply_msg_id), row.reply)))
        } else {
            Ok(None)
        }
    }

    async fn delete_admin_message(&self, admin_id: UserId, request_msg_id: MessageId) -> Result<SqliteQueryResult, sqlx::Error> {
        sqlx::query("DELETE FROM admin_replies WHERE admin_id = ? AND request_msg_id = ?")
            .bind(admin_id.0 as i64)
            .bind(request_msg_id.0)
            .execute(&self.pool)
            .await
    }

    pub async fn create_user_stats(&self, user: &User) -> Result<SqliteQueryResult, sqlx::Error> {
        sqlx::query(
            r#"
            INSERT INTO user_stats (
                user_id,
                user_name,
                total_requests,
                requests_accepted,
                requests_declined
            )
            VALUES ($1, $2, 0, 0, 0)
            ON CONFLICT(user_id) DO UPDATE
            SET user_name=excluded.user_name
            "#
        )
        .bind(user.id.0 as i64)
        .bind(user.username.as_ref().unwrap_or(&user.first_name))
        .execute(&self.pool)
        .await
    }

    pub async fn save_user_stats(&self, chat_id: &ChatId, is_accepted: bool) -> Result<SqliteQueryResult, sqlx::Error> {
        sqlx::query(
            r#"
            INSERT INTO user_stats (
                user_id,
                user_name,
                total_requests,
                requests_accepted,
                requests_declined,
                last_responded_request_date
            ) 
            VALUES ($1, "Unknown", 1, $2, $3, $4)
            ON CONFLICT(user_id) DO UPDATE
            SET total_requests=total_requests + 1,
                requests_accepted=requests_accepted + $2,
                requests_declined=requests_declined + $3,
                last_responded_request_date=$4
            "#
        )
        .bind(chat_id.0)
        .bind(if is_accepted { 1 } else { 0 })
        .bind(if is_accepted { 0 } else { 1 })
        .bind(Utc::now())
        .execute(&self.pool)
        .await
    }

    pub async fn get_user_stats(&self, user_id: UserId) -> Result<Option<UserStats>, sqlx::Error> {
        sqlx::query_as::<_, UserStats>("SELECT * FROM user_stats WHERE user_id = ?")
            .bind(user_id.0 as i64)
            .fetch_optional(&self.pool)
            .await
    }

    pub async fn get_stats(&self, top_n: u8) -> Result<String> {
        let top_users_by_rating = sqlx::query_as::<_, UserStats>(
                "SELECT * FROM user_stats WHERE total_requests > 0 ORDER BY (requests_accepted - requests_declined) DESC LIMIT ?"
            )
            .bind(top_n)
            .fetch_all(&self.pool)
            .await?;

        let top_users_by_accepted = sqlx::query_as::<_, UserStats>(
                "SELECT * FROM user_stats WHERE total_requests > 0 ORDER BY requests_accepted DESC LIMIT ?"
            )
            .bind(top_n)
            .fetch_all(&self.pool)
            .await?;

        let top_users_by_declined = sqlx::query_as::<_, UserStats>(
                "SELECT * FROM user_stats WHERE total_requests > 0 ORDER BY requests_declined DESC LIMIT ?"
            )
            .bind(top_n)
            .fetch_all(&self.pool)
            .await?;

            Ok(format!(
                "*Top {} by rating:*\n{}\n\n\
                *Top {} by accepted count:*\n{}\n\n\
                *Top {} by declined count:*\n{}",
                top_users_by_rating.len(),
                top_users_by_rating.iter().map(|user| user.format()).collect::<Vec<_>>().join("\n"),
                top_users_by_accepted.len(),
                top_users_by_accepted.iter().map(|user| user.format()).collect::<Vec<_>>().join("\n"),
                top_users_by_declined.len(),
                top_users_by_declined.iter().map(|user| user.format()).collect::<Vec<_>>().join("\n")
            ))
    }

    pub async fn update_bot_auto_response(&self, data: &str) -> Result<bool> {
        Ok(
            sqlx::query("INSERT INTO bot_auto_response (data) VALUES (?)")
                .bind(data)
                .execute(&self.pool)
                .await?
                .last_insert_rowid() > 0
        )
    }

    pub async fn get_bot_auto_response(&self) -> Result<Option<String>> {
        let row = sqlx::query_as::<_, BotAutoResponseRow>("SELECT data FROM bot_auto_response ORDER BY id DESC LIMIT 1")
            .fetch_optional(&self.pool)
            .await?;

        Ok(row.map(|r| r.data))
    }
}

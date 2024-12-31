use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

#[derive(Clone, Default, Serialize, Deserialize, sqlx::FromRow)]
pub struct UserStats {
    pub user_id: i64,
    pub user_name: String,
    pub total_requests: u16,
    pub requests_accepted: u16,
    pub requests_declined: u16,
    pub last_responded_request_date: Option<DateTime<Utc>>,
}

impl UserStats {
    pub fn format_rating(&self) -> String {
        if self.total_requests > 0 {
            teloxide::utils::markdown::escape(
                &format!("Твой рейтинг: {} ({} принято, {} отклонено)", self.requests_accepted - self.requests_declined, self.requests_accepted, self.requests_declined)
            )
        } else {
            "0".to_string()
        }
    }

    pub fn format(&self) -> String {
        teloxide::utils::markdown::escape(
            &format!("@{}: {} (↑{}/↓{})",
                self.user_name,
                self.requests_accepted - self.requests_declined,
                self.requests_accepted, 
                self.requests_declined
            )
        )
    }
}


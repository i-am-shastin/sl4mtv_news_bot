use crate::{message::PostMessage, post::Post};

use std::collections::{hash_map::Entry, HashMap};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use teloxide::types::{Message, MessageId};

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct DialogueState {
    pub has_ban_until: Option<DateTime<Utc>>,
    pub posts: HashMap<String, Post>,
    pub last_report_request_date: Option<DateTime<Utc>>,
    pub bot_is_banned: bool,
}

impl DialogueState {
    pub fn is_report_request_allowed(&self) -> bool {
        self.last_report_request_date.map_or(true, |last_date| Utc::now() - last_date > Duration::hours(24))
    }

    pub fn is_ban_active(&self) -> bool {
        self.has_ban_until.is_some_and(|ban_until| ban_until > Utc::now())
    }

    pub fn get_sent_posts_count(&self) -> usize {
        self.posts.values().filter(|post| !post.sent_to.is_empty()).count()
    }

    pub fn find_post_key_by_message_id(&self, message_id: MessageId) -> Option<String> {
        self.posts.iter().find_map(|(key, post)| {
            if post.entities.iter().any(|e| e.message_id == message_id) {
                Some(key.clone())
            } else {
                None
            }
        })
    }

    pub async fn process_user_message(&self, msg: &Message) -> (DialogueState, Option<MessageId>) {
        let mut new_state = self.clone();

        let pending_post_id = match new_state.posts.entry(msg.get_post_key()) {
            Entry::Occupied(mut entry) => {
                let post = entry.get_mut();
                post.entities.push(msg.into());
                None
            },
            Entry::Vacant(entry) => {
                entry.insert(Post {
                    entities: vec![msg.into()],
                    sent_to: vec![],
                });
                Some(msg.id)
            },
        };

        (new_state, pending_post_id)
    }
}
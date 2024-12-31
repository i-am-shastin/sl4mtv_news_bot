use crate::{message::{MessageReaction, PostMessage, Reaction}, variants};

use log::warn;
use serde::{Deserialize, Serialize};
use teloxide::{prelude::Requester, types::{ChatId, InlineKeyboardButton, InlineKeyboardMarkup, Message, MessageId}, Bot};
use anyhow::Result;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Post {
    pub sent_to: Vec<AdminRequest>,
    pub entities: Vec<PostEntity>,
}

impl Post {
    pub fn get_message_ids(&self) -> Vec<MessageId> {
        let mut ids = self.entities.iter().map(|entity| entity.message_id).collect::<Vec<_>>();
        ids.sort_by(|a, b| a.0.cmp(&b.0));
        ids
    }

    pub fn get_text(&self) -> String {
        self.entities.iter()
            .enumerate()
            .filter_map(|(index, entity)| {
                entity.text.as_ref().map(|text| format!("{}: {}", index + 1, text))
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub async fn get_links(&self, bot: &Bot) -> Result<String> {
        let mut entities_data = Vec::new();
        let token = bot.token();
        let api_url = bot.api_url();

        for entity in &self.entities {
            let Some(file_id) = entity.file_id.as_ref() else {
                continue;
            };

            let file = bot.get_file(file_id).await?;
            let file_url = api_url
                .join(&format!("file/bot{token}/{}", file.path))?
                .to_string();
    
            entities_data.push(format!(
                "{}: {}",
                entity.text.clone().unwrap_or_default(),
                file_url
            ));
        }

        Ok(format!("Ссылки будут доступны в течение 1 часа.\n\n{}", entities_data.join("\n")))
    }

    pub async fn try_remove_post(&self, bot: &Bot) {
        for admin_request in &self.sent_to {
            let result = bot.delete_messages(admin_request.admin_chat_id, admin_request.get_message_ids()).await;
            if result.is_err() {
                if bot.edit_message_reply_markup(admin_request.admin_chat_id, admin_request.message_id).await.is_err() {
                    warn!("Unable to update inline keyboard for message {} from admin {}", admin_request.message_id, admin_request.admin_chat_id);
                }
                if bot.react_to_message_with(admin_request.admin_chat_id, admin_request.message_id, Reaction::Salute).await.is_err() {
                    warn!("Unable to set reaction for message {} from admin {}", admin_request.message_id, admin_request.admin_chat_id);
                }
            }
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PostEntity {
    pub message_id: MessageId,
    pub file_id: Option<String>,
    pub text: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AdminRequest {
    pub forwarded_message_ids: Vec<MessageId>,
    pub message_id: MessageId,
    pub admin_chat_id: ChatId,
}

impl AdminRequest {
    pub fn get_message_ids(&self) -> Vec<MessageId> {
        self.forwarded_message_ids.clone().into_iter().chain(std::iter::once(self.message_id)).collect()
    }
}

impl From<&Message> for PostEntity {
    fn from(value: &Message) -> Self {
        PostEntity {
            message_id: value.id,
            file_id: value.get_message_file_id(),
            text: value.caption().or(value.text()).map(String::from),
        }
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub enum PostAction {
    Approve((ChatId, i32)),
    Decline((ChatId, i32)),
    GetFileLinks((ChatId, i32)),
    Ban((ChatId, i64)),
    SetAutoResponse,
}

impl PostAction {
    pub fn get_keyboard_for_message(chat_id: ChatId, post_id: i32) -> Result<InlineKeyboardMarkup> {
        Ok(
            InlineKeyboardMarkup::new(vec![
                vec![
                    InlineKeyboardButton::callback(variants::POST_GET_FILE_LINKS_TEXT, serde_json::to_string(&PostAction::GetFileLinks((chat_id, post_id)))?),
                ],
                vec![
                    InlineKeyboardButton::callback(variants::POST_ACCEPT_BUTTON_TEXT, serde_json::to_string(&PostAction::Approve((chat_id, post_id)))?),
                    InlineKeyboardButton::callback(variants::POST_DECLINE_BUTTON_TEXT, serde_json::to_string(&PostAction::Decline((chat_id, post_id)))?)
                ],
                vec![
                    InlineKeyboardButton::callback(variants::POST_BAN_BUTTON_7D_TEXT, serde_json::to_string(&PostAction::Ban((chat_id, 7)))?),
                    InlineKeyboardButton::callback(variants::POST_BAN_BUTTON_30D_TEXT, serde_json::to_string(&PostAction::Ban((chat_id, 30)))?),
                    InlineKeyboardButton::callback(variants::POST_BAN_BUTTON_INFINITE_TEXT, serde_json::to_string(&PostAction::Ban((chat_id, 9999)))?),
                ]
            ])
        )
    }
}

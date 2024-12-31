use crate::variants;
use crate::PostAction;

use strum::Display;
use teloxide::{payloads::SetMessageReaction, prelude::Requester, requests::JsonRequest, types::{ChatId, InlineKeyboardButton, InlineKeyboardMarkup, Message, MessageId, ReactionType}, Bot};
use anyhow::Result;

#[derive(Display, PartialEq)]
pub enum Reaction {
    Pending,
    Accepted,
    Declined,
    Okay,
    Salute,
    None
}

pub trait MessageReaction {
    fn react_to_message_with(&self, chat_id: ChatId, message_id: MessageId, reaction: Reaction) -> JsonRequest<SetMessageReaction>;
}

impl MessageReaction for Bot {
    fn react_to_message_with(
        &self,
        chat_id: ChatId,
        message_id: MessageId,
        reaction: Reaction
    ) -> JsonRequest<SetMessageReaction> {
        let mut action = self.set_message_reaction(chat_id, message_id);

        if reaction == Reaction::None {
            action.reaction = None;
        } else {
            let emoji = match reaction {
                Reaction::Pending => variants::emoji::REACTION_PENDING,
                Reaction::Accepted => variants::emoji::REACTION_ACCEPTED,
                Reaction::Declined => variants::emoji::REACTION_DECLINED,
                Reaction::Okay => variants::emoji::REACTION_OKAY,
                Reaction::Salute => variants::emoji::REACTION_SALUTE,
                _ => unreachable!(),
            }.to_string();

            action.reaction = Some(vec![ReactionType::Emoji { emoji }]);
        }

        action
    }
}

pub trait PostMessage {
    fn is_supported_type(&self) -> bool;
    fn is_channel_post(&self) -> bool;
    fn get_post_key(&self) -> String;
    fn get_message_file_id(&self) -> Option<String>;
    fn get_user_keyboard(&self) -> InlineKeyboardMarkup;
    fn get_admin_keyboard(&self) -> InlineKeyboardMarkup;
    fn get_from_username(&self) -> Result<String>;
}

impl PostMessage for Message {
    fn is_supported_type(&self) -> bool {
        self.text().is_some()
            || self.photo().is_some_and(|p| !p.is_empty())
            || self.video().is_some()
            || self.audio().is_some()
            || self.voice().is_some()
            || self.document().is_some()
    }
    
    fn is_channel_post(&self) -> bool {
        self.is_automatic_forward() && self.from.as_ref().is_some_and(|from| from.id.is_telegram())
    }

    fn get_post_key(&self) -> String {
        self.media_group_id().map_or_else(|| self.id.to_string(), |id| id.to_string())
    }

    fn get_message_file_id(&self) -> Option<String> {
        self.photo().and_then(|photos| photos.last().map(|photo| photo.file.id.to_owned()))
            .or_else(|| self.video().map(|video| video.file.id.to_owned()))
            .or_else(|| self.audio().map(|audio| audio.file.id.to_owned()))
            .or_else(|| self.voice().map(|voice| voice.file.id.to_owned()))
            .or_else(|| self.document().map(|doc| doc.file.id.to_owned()))
    }

    fn get_user_keyboard(&self) -> InlineKeyboardMarkup {
        InlineKeyboardMarkup::new(vec![
            vec![
                InlineKeyboardButton::callback(variants::POST_SEND_BUTTON_TEXT, serde_json::to_string(&(self.id, false)).unwrap())
            ],
            vec![
                InlineKeyboardButton::callback(variants::POST_SEND_ANONYMOUS_BUTTON_TEXT, serde_json::to_string(&(self.id, true)).unwrap())
            ]
        ])
    }

    fn get_admin_keyboard(&self) -> InlineKeyboardMarkup {
        InlineKeyboardMarkup::new(vec![
            vec![
                InlineKeyboardButton::callback(variants::POST_SEND_BUTTON_TEXT, serde_json::to_string(&PostAction::SetAutoResponse).unwrap())
            ]
        ])
    }

    fn get_from_username(&self) -> Result<String> {
        let Some(ref from) = self.from else {
            return Err(anyhow::anyhow!("Message from is None"));
        };

        Ok(from.username.as_ref().map_or_else(|| from.first_name.clone(), |username| format!("@{}", username)))
    }
}

use log::{error, info};

use crate::{admin_storage::AdminStorage, config::Config, dialogue_state::DialogueState, message::PostMessage, types::{HandlerResult, MyDialogue}, variants};

use std::sync::Arc;
use chrono::Utc;
use teloxide::{prelude::*, types::{Message, ParseMode, ReplyParameters}, Bot};

pub async fn start(
    bot: Bot,
    dialogue: MyDialogue,
    state: DialogueState,
    msg: Message,
    config: Config,
) -> HandlerResult {
    let Some(user) = &msg.from else {
        error!("Start command received, but user is missing for some reason. Message: {:?}", msg);
        return Ok(());
    };

    if state.is_ban_active() {
        return Ok(())
    }

    if state.bot_is_banned {
        let mut new_state = state.clone();
        new_state.bot_is_banned = false;
        dialogue.update(new_state).await?;
        info!("User {} [{}] unbanned bot", msg.get_from_username()?, user.id);
    }

    let text = if config.is_admin(user.id) {
        variants::ADMIN_START_INTRO_MESSAGE
    } else if state.bot_is_banned {
        variants::START_INTRO_MESSAGE_AFTER_BAN()
    } else {
        variants::START_INTRO_MESSAGE()
    };

    bot.send_message(msg.chat.id, text).await?;
    info!("{:?} has started the bot", user);
    Ok(())
}

pub async fn cancel(
    bot: Bot,
    dialogue: MyDialogue,
    state: DialogueState,
    msg: Message,
) -> HandlerResult {
    if state.is_ban_active() {
        return Ok(())
    }

    if let Some(reply) = msg.reply_to_message() {
        if let Some(post_key) = state.find_post_key_by_message_id(reply.id) {
            let mut new_state = state.clone();
            if let Some(post) = new_state.posts.remove(&post_key) {
                post.try_remove_post(&bot).await;
            }
            dialogue.update(new_state).await?;
        }
    } else {
        bot.send_message(msg.chat.id, variants::CANCEL_WITHOUT_REPLY()).await?;
    }
    Ok(())
}

pub async fn stats(
    bot: Bot,
    dialogue: MyDialogue,
    state: DialogueState,
    msg: Message,
    config: Config,
    admin_storage: Arc<AdminStorage>,
) -> HandlerResult {
    if state.is_ban_active() {
        return Ok(())
    }

    let Some(user) = &msg.from else {
        error!("Stats command received, but user is missing for some reason. Message: {:?}", msg);
        return Ok(());
    };

    if config.is_admin(user.id) {
        let top_n = msg.text()
            .and_then(|text| text.split_whitespace().nth(1))
            .and_then(|n| n.parse::<u8>().ok())
            .unwrap_or(5);

        info!("Stats command for top {top_n} users received from admin: {}", user.id);
        bot.send_message(msg.chat.id, admin_storage.get_stats(top_n).await?)
            .reply_parameters(ReplyParameters::new(msg.id))
            .parse_mode(ParseMode::MarkdownV2)
            .await?;

        return Ok(())
    }

    info!("Stats command received from user: {} [{}]", msg.get_from_username()?, user.id);
    let message_text = if !state.is_report_request_allowed() {
        variants::USER_STATS_TRY_LATER_MESSAGE().to_string()
    } else {
        let mut new_state = state.clone();
        new_state.last_report_request_date = Some(Utc::now());
        dialogue.update(new_state).await?;

        if let Some(user_stats) = admin_storage.get_user_stats(user.id).await? {
            user_stats.format_rating()
        } else {
            variants::USER_STATS_NOT_FOUND_MESSAGE().to_string()
        }
    };

    bot.send_message(msg.chat.id, message_text)
        .reply_parameters(ReplyParameters::new(msg.id))
        .parse_mode(ParseMode::MarkdownV2)
        .await?;

    Ok(())
}
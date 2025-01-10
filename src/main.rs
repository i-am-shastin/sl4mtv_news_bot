pub mod types;
pub mod variants;
pub mod settings;
pub mod dialogue_state;
pub mod post;
pub mod user_stats;
pub mod admin_storage;
pub mod message;
pub mod command_handle;
pub mod update_handle;
pub mod bot_configuration;

use dptree::case;
use log::{error, info, warn};

use std::sync::{atomic::{AtomicU32, Ordering}, Arc};
use teloxide::{
    dispatching::dialogue::{
        self, serializer::Json, ErasedStorage, SqliteStorage, Storage
    }, payloads::SendMessageSetters, prelude::*, types::{MediaKind, MediaText, MessageId, MessageKind, ReplyParameters}, utils::command::BotCommands
};
use anyhow::{anyhow, Result};
use chrono::{Duration, Utc};
use admin_storage::AdminStorage;
use settings::AppSettings;
use dialogue_state::DialogueState;
use post::{AdminRequest, PostAction};
use types::{HandlerResult, MyDialogue, MyStorage};
use message::{MessageReaction, PostMessage, Reaction};
use bot_configuration::BotConfiguration;

#[derive(Clone, BotCommands)]
#[command(rename_rule = "lowercase")]
pub enum Command {
    Start,
    #[command(description = "Team! Fall Back")]
    Cancel,
    #[command(description = "Report In")]
    Stats,
    #[command(description = "Send auto comment every N posts")]
    Every(u32)
}

const LOG4RS_CONFIG: &str = if cfg!(debug_assertions) {
    "log4rs.debug.yaml"
} else {
    "log4rs.yaml"
};

#[tokio::main]
async fn main() -> Result<()> {
    log4rs::init_file(LOG4RS_CONFIG, Default::default())?;
    info!("{}", "-".repeat(50));
    info!("Booting up");

    dotenvy::dotenv_override()?;

    let bot = Bot::from_env();
    let bot_info = bot.get_me().await?;
    info!("{} is running", bot_info.full_name());

    let settings = AppSettings::from_env()?;
    if settings.admins.is_empty() {
        return Err(anyhow!("No admins found. Do you have ADMIN_CHAT_ID in environment variables?"));
    }

    let storage: MyStorage = SqliteStorage::open(&settings.db_filename, Json).await.unwrap().erase();
    info!("Bot admins are: {:?}, database loaded from: {}", settings.admins, settings.db_filename);

    let admin_storage = AdminStorage::open(&settings.admin_db_filename).await?;
    info!("Admin database loaded from: {}", settings.admin_db_filename);

    let bot_configuration = BotConfiguration::from_db(&admin_storage.pool).await?;
    let channel_posts_counter = Arc::new(AtomicU32::new(0));

    let command_handler = teloxide::filter_command::<Command, _>()
        .branch(case![Command::Start].endpoint(command_handle::start))
        .branch(case![Command::Cancel].endpoint(command_handle::cancel))
        .branch(case![Command::Stats].endpoint(command_handle::stats))
        .endpoint(command_handle::action);

    let handler = dialogue::enter::<Update, ErasedStorage<DialogueState>, DialogueState, _>()
        .branch(
            Update::filter_message()
                .branch(Message::filter_forward_origin().endpoint(on_forwarded_message))
                .branch(command_handler)
                .branch(dptree::endpoint(on_message))
        )
        .branch(Update::filter_callback_query().endpoint(callback_handler))
        .branch(Update::filter_edited_message().endpoint(update_handle::on_edit_message))
        .branch(Update::filter_my_chat_member().endpoint(update_handle::on_my_chat_member))
        .branch(Update::filter_message_reaction_updated().endpoint(update_handle::on_reaction_updated))
        .endpoint(update_handle::on_update);

    info!("Initializing update dispatcher");
    Dispatcher::builder(bot, handler)
        .dependencies(dptree::deps![storage, admin_storage, settings, bot_configuration, channel_posts_counter])
        .enable_ctrlc_handler()
        .build()
        .dispatch()
        .await;

    info!("Shutting down");
    Ok(())
}

async fn on_message(
    bot: Bot,
    dialogue: MyDialogue,
    state: DialogueState,
    msg: Message,
    admin_storage: Arc<AdminStorage>,
    settings: AppSettings,
) -> HandlerResult {
    if !msg.chat.is_private() {
        return Ok(())
    }

    let Some(user) = &msg.from else {
        error!("Private message received, but user is missing for some reason. Message: {:?}", msg);
        return Ok(());
    };

    if state.is_ban_active() {
        return Ok(())
    }

    if settings.is_admin(user.id) {
        return on_admin_message(bot, &msg, user.id, admin_storage).await;
    }

    if !msg.is_supported_type() {
        bot.send_message(msg.chat.id, variants::INVALID_INPUT_TYPE_MESSAGE()).await?;
        return Ok(())
    }

    // TODO: Fix when media group is sent
    if state.posts.len() >= settings.max_draft_posts {
        bot.send_message(msg.chat.id, variants::MAX_DRAFT_POSTS_LIMIT_MESSAGE()).await?;
        return Ok(())
    }

    let (new_state, reply_to) = state.process_user_message(&msg).await;
    dialogue.update(new_state).await?;

    if let Some(message_id) = reply_to {
        bot.send_message(msg.chat.id, variants::POST_SEND_PENDING_MESSAGE)
            .reply_parameters(ReplyParameters::new(message_id))
            .reply_markup(msg.get_user_keyboard())
            .await?;
    }

    Ok(())
}

async fn on_admin_message(
    bot: Bot,
    msg: &Message,
    admin_id: UserId,
    admin_storage: Arc<AdminStorage>,
) -> HandlerResult {
    match msg.reply_to_message() {
        Some(reply_to) if reply_to.reply_markup().is_some() && msg.text().is_some() => {
            admin_storage.save_admin_message(admin_id, reply_to.id, msg.id, msg.text().unwrap()).await?;
            bot.react_to_message_with(msg.chat.id, msg.id, Reaction::Okay).await?;
        }
        _ => {
            bot.send_message(msg.chat.id, variants::ADMIN_UPDATE_AUTOPOST_MESSAGE)
                .reply_parameters(ReplyParameters::new(msg.id))
                .reply_markup(msg.get_admin_keyboard())
                .await?;
        }
    }
    Ok(())
}

async fn callback_handler(
    bot: Bot,
    dialogue: MyDialogue,
    state: DialogueState,
    settings: AppSettings,
    storage: MyStorage,
    admin_storage: Arc<AdminStorage>,
    q: CallbackQuery,
) -> HandlerResult {
    let Some(ref query_data) = q.data else {
        return Ok(())
    };

    bot.answer_callback_query(&q.id).await?;

    if state.is_ban_active() {
        return Ok(())
    }
    let Some(msg) = q.regular_message() else {
        return Ok(())
    };
    let Some(reply_to) = msg.reply_to_message() else {
        return Ok(())
    };

    if settings.is_admin(q.from.id) {
        return admin_callback_handler(&bot, &settings, storage, admin_storage, q.from.id, query_data, msg, reply_to).await;
    }

    if state.get_sent_posts_count() >= settings.max_sent_posts {
        bot.send_message(msg.chat.id, variants::MAX_SENT_POSTS_LIMIT_MESSAGE())
            .reply_parameters(ReplyParameters::new(msg.id))
            .await?;
        return Ok(())
    }

    let Ok((post_id, is_anonymous)) = serde_json::from_str::<(i32, bool)>(query_data) else {
        return Ok(());
    };

    admin_storage.create_user_stats(&q.from).await?;

    let message_id = MessageId(post_id);
    let mut new_state = state.clone();
    let Some(post) = new_state.posts.get_mut(&reply_to.get_post_key()) else {
        return Ok(())
    };

    let author_info = is_anonymous.then_some(variants::ADMIN_ANONYMOUS_POST_MESSAGE.to_string()).unwrap_or_default();
    let post_text = format!("{}\n\n{author_info}{}", post.get_text(), variants::POST_PENDING_ACTION_MESSAGE);
    let message_ids = post.get_message_ids();

    for admin_chat_id in settings.admins {
        let Ok(forwarded_message_ids) = bot.forward_messages(admin_chat_id, msg.chat.id, message_ids.clone()).await else {
            continue;
        };
        let Some(forwarded_message_id) = forwarded_message_ids.last() else {
            continue;
        };

        let Ok(message) = bot.send_message(admin_chat_id, post_text.clone())
            .reply_parameters(ReplyParameters::new(forwarded_message_id.clone()))
            .reply_markup(PostAction::get_keyboard_for_message(msg.chat.id, post_id)?)
            .await else {
                continue;
            };

        post.sent_to.push(AdminRequest {
            forwarded_message_ids,
            message_id: message.id,
            admin_chat_id,
        });
    }
    dialogue.update(new_state).await?;

    bot.react_to_message_with(msg.chat.id, message_id, Reaction::Pending).await?;
    if bot.delete_message(msg.chat.id, msg.id).await.is_err() {
        if bot.edit_message_reply_markup(msg.chat.id, msg.id).await.is_err() {
            warn!("Unable to delete or update inline keyboard for message {} from user {}", msg.id, msg.chat.id);
        }
    }
    Ok(())
}

async fn admin_callback_handler(
    bot: &Bot,
    settings: &AppSettings,
    storage: MyStorage,
    admin_storage: Arc<AdminStorage>,
    admin_id: UserId,
    query_data: &str,
    msg: &Message,
    reply_to: &Message,
) -> HandlerResult {
    let Ok(post_action) = serde_json::from_str::<PostAction>(query_data) else {
        error!("Unknown post action: {query_data}");
        return Ok(());
    };

    let (chat_id, mut response_text) = match post_action {
        PostAction::SetAutoResponse => {
            try_set_auto_response(bot, reply_to, admin_storage, settings, &admin_id).await?;
            return Ok(());
        },
        PostAction::GetFileLinks((chat_id, post_id)) => {
            let Some(current_user_state) = storage.clone().get_dialogue(chat_id).await? else {
                warn!("User {chat_id} has no active dialogue");
                return Ok(())
            };

            if let Some(post_key) = current_user_state.find_post_key_by_message_id(MessageId(post_id)) {
                if let Some(post) = current_user_state.posts.get(&post_key) {
                    bot.send_message(admin_id, post.get_links(bot).await?).await?;
                }
            }
            return Ok(());
        },
        PostAction::Approve((chat_id, post_id)) => {
            bot.react_to_message_with(chat_id, MessageId(post_id), Reaction::Accepted).await?;
            admin_storage.save_user_stats(&chat_id, true).await?;
            remove_post_from_user_state(bot, storage, chat_id, post_id).await?;

            info!("Admin {} approved post from user {}", admin_id, chat_id);
            (chat_id, variants::POST_ACCEPTED_MESSAGE().to_string())
        },
        PostAction::Decline((chat_id, post_id)) => {
            bot.react_to_message_with(chat_id, MessageId(post_id), Reaction::Declined).await?;
            admin_storage.save_user_stats(&chat_id, false).await?;
            remove_post_from_user_state(bot, storage, chat_id, post_id).await?;

            info!("Admin {} declined post from user {}", admin_id, chat_id);
            (chat_id, variants::POST_DECLINED_MESSAGE().to_string())
        },
        PostAction::Ban((chat_id, duration)) => {
            let Some(mut current_user_state) = storage.clone().get_dialogue(chat_id).await? else {
                warn!("User {chat_id} has no active dialogue");
                return Ok(())
            };
            current_user_state.has_ban_until = Some(Utc::now() + Duration::days(duration));
            for (_, post) in current_user_state.posts.drain() {
                post.try_remove_post(bot).await;
            }
            storage.update_dialogue(chat_id, current_user_state).await?;

            info!("Admin {} banned user {} for {} days", admin_id, chat_id, duration);
            (chat_id, format!("{}\n\nБан будет снят через {} дней", variants::POST_BAN_MESSAGE(), duration))
        }
    };

    if let Some((reply_msg_id, reply)) = admin_storage.get_admin_message(admin_id, msg.id).await? {
        response_text = format!("{}\n\nКомментарий администратора: {}", response_text, reply);
        if bot.delete_message(msg.chat.id, reply_msg_id).await.is_err() {
            if bot.react_to_message_with(msg.chat.id, reply_msg_id, Reaction::Salute).await.is_err() {
                error!("Unable to delete or set reaction for message {} from admin {}", reply_msg_id, msg.chat.id);
            } else {
                warn!("Unable to delete reply message {} from admin {}, reaction set", reply_msg_id, msg.chat.id);
            }
        }
    }

    bot.send_message(chat_id, response_text).await?;
    info!("User {chat_id} received response message");

    return Ok(())
}

async fn remove_post_from_user_state(
    bot: &Bot,
    storage: MyStorage,
    chat_id: ChatId,
    post_id: i32,
) -> HandlerResult {
    let Some(mut current_user_state) = storage.clone().get_dialogue(chat_id).await? else {
        warn!("User {chat_id} has no active dialogue");
        return Ok(())
    };

    if let Some(post_key) = current_user_state.find_post_key_by_message_id(MessageId(post_id)) {
        if let Some(post) = current_user_state.posts.remove(&post_key) {
            post.try_remove_post(bot).await;
        }
    }
    storage.update_dialogue(chat_id, current_user_state).await?;

    Ok(())
}

async fn try_set_auto_response(
    bot: &Bot,
    reply_to: &Message,
    admin_storage: Arc<AdminStorage>,
    settings: &AppSettings,
    admin_id: &UserId,
) -> Result<()> {
    let MessageKind::Common(common) = &reply_to.kind else {
        return Err(anyhow!("Message is not a common message"));
    };
    let MediaKind::Text(media_text) = &common.media_kind else {
        return Err(anyhow!("Message is not a text message"));
    };

    let message_as_json = serde_json::to_string(&media_text)?;
    let username = reply_to.get_from_username()?;

    if !admin_storage.update_bot_auto_response(&message_as_json).await? {
        return Err(anyhow!("Admin {} [{}] unabled to update bot auto response.", username, admin_id));
    }

    let auto_update_description = format!("Автоматический ответ обновлён администратором {username}:\n\n");
    let message_text = format!("{auto_update_description}{}", media_text.text);
    let description_length = auto_update_description.chars().count();

    let updated_entities = media_text.entities.iter()
        .map(|entity| {
            let mut new_entity = entity.clone();
            new_entity.offset += description_length;
            new_entity
        })
        .collect::<Vec<_>>();

    for admin_chat_id in &settings.admins {
        let mut auto_message = bot.send_message(admin_chat_id.clone(), &message_text).entities(updated_entities.clone());
        auto_message.link_preview_options = media_text.link_preview_options.clone();

        if auto_message.await.is_err() {
            error!("Failed to send auto response update notification for admin {}", admin_chat_id);
        }
    }

    info!("Admin {} [{}] updated bot auto response", username, admin_id);
    Ok(())
}

async fn on_forwarded_message(
    bot: Bot,
    msg: Message,
    admin_storage: Arc<AdminStorage>,
    channel_posts_counter: Arc<AtomicU32>,
    bot_configuration: BotConfiguration,
) -> HandlerResult {
    if msg.chat.is_private() {
        bot.send_message(msg.chat.id, variants::FORWARDING_NOT_ALLOWED_MESSAGE())
            .reply_parameters(ReplyParameters::new(msg.id))
            .await?;
        return Ok(());
    }

    if !msg.is_channel_post() {
        return Ok(());
    }

    info!("Channel post received");
    if let Some(media_group_id) = msg.media_group_id() {
        if admin_storage.has_media_group_response(&media_group_id) {
            info!("Already responded to media group {}", media_group_id);
            return Ok(());
        }
    }

    let Some(response_data) = admin_storage.get_bot_auto_response().await? else {
        warn!("No auto response found");
        return Ok(());
    };

    let response = serde_json::from_str::<MediaText>(&response_data)?;

    if bot_configuration.should_comment(channel_posts_counter.fetch_add(1, Ordering::Relaxed)) {
        let mut auto_message = bot.send_message(msg.chat.id, response.text);
        auto_message.link_preview_options = response.link_preview_options;
        auto_message
            .entities(response.entities)
            .reply_parameters(ReplyParameters::new(msg.id))
            .await?;
        info!("Auto response message sent");
    }

    Ok(())
}

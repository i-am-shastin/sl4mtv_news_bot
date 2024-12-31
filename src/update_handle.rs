use crate::{dialogue_state::DialogueState, types::{HandlerResult, MyDialogue}, variants};

use teloxide::{prelude::*, types::{Message, MessageReactionUpdated, ChatMemberUpdated}};

pub async fn on_edit_message(
    bot: Bot,
    state: DialogueState,
    msg: Message,
) -> HandlerResult {
    if state.is_ban_active() {
        return Ok(())
    }

    if !msg.chat.is_private() {
        return Ok(());
    }

    for post in state.posts.values() {
        if post.sent_to.len() > 0 && post.entities.iter().any(|e| e.message_id == msg.id) {
            bot.send_message(msg.chat.id, variants::POST_EDITED_WARNING_MESSAGE()).await?;
            return Ok(());
        }
    }

    Ok(())
}

pub async fn on_reaction_updated(
    bot: Bot,
    state: DialogueState,
    reaction_update: MessageReactionUpdated,
) -> HandlerResult {
    if state.is_ban_active() {
        return Ok(())
    }

    if reaction_update.chat.is_private() {
        bot.set_message_reaction(reaction_update.chat.id, reaction_update.message_id)
            .reaction(reaction_update.new_reaction)
            .await?;
    }
    Ok(())
}

pub async fn on_my_chat_member(
    dialogue: MyDialogue,
    state: DialogueState,
    chat_update: ChatMemberUpdated,
) -> HandlerResult {
    if chat_update.chat.is_private() && chat_update.new_chat_member.is_banned() {
        let mut new_state = state.clone();
        new_state.bot_is_banned = true;
        dialogue.update(new_state).await?;
    }
    Ok(())
}

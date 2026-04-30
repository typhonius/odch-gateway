//! Built-in bot commands.
//!
//! Each command is a function that returns a boxed async handler.

use super::{CommandContext, CommandEngine, CommandHandler, CommandResponse};
use crate::db::queries;

/// Register all built-in commands.
pub fn register_all(engine: &mut CommandEngine) {
    engine.register("help", &["h", "commands"], cmd(help));
    engine.register("tell", &["msg"], cmd(tell));
    engine.register("history", &["hist"], cmd(history));
    engine.register("search", &[], cmd(search));
    engine.register("seen", &[], cmd(seen));
    engine.register("first", &[], cmd(first));
    engine.register("last", &[], cmd(last));
    engine.register("quote", &["q"], cmd(quote));
    engine.register("stats", &[], cmd(stats));
    engine.register("watch", &["w"], cmd(watch));
    engine.register("unwatch", &["uw"], cmd(unwatch));
    engine.register("info", &[], cmd(info));
    engine.register("ban", &[], cmd(ban));
    engine.register("unban", &[], cmd(unban));
    engine.register("kick", &[], cmd(kick));
    engine.register("gag", &["mute"], cmd(gag));
    engine.register("ungag", &["unmute"], cmd(ungag));
    engine.register("topic", &[], cmd(topic));
}

/// Wrap an async fn into a CommandHandler.
fn cmd<F, Fut>(f: F) -> CommandHandler
where
    F: Fn(CommandContext) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = CommandResponse> + Send + 'static,
{
    Box::new(move |ctx| Box::pin(f(ctx)))
}

// ---------------------------------------------------------------------------
// Command implementations
// ---------------------------------------------------------------------------

async fn help(ctx: CommandContext) -> CommandResponse {
    let mut msg = String::from(
        "Commands: !help !tell !history !search !seen !first !last !quote \
         !stats !watch !unwatch !info !ban !unban !kick !gag !ungag !topic",
    );

    // Add external bot commands
    let bots = ctx.bot_registry.bots.read().await;
    for bot in bots.values() {
        if !bot.commands.is_empty() {
            let mut cmds: Vec<String> = bot.commands.iter().map(|c| format!("!{}", c)).collect();
            cmds.sort();
            msg.push_str(&format!(" | {}: {}", bot.nick, cmds.join(" ")));
        }
    }

    CommandResponse::ChatSingle(msg)
}

async fn tell(ctx: CommandContext) -> CommandResponse {
    let parts: Vec<&str> = ctx.args.splitn(2, ' ').collect();
    if parts.len() < 2 {
        return CommandResponse::BotPm("Usage: !tell <nick> <message>".to_string());
    }
    let to_nick = parts[0];
    let message = parts[1];

    match queries::create_tell(&ctx.db, &ctx.nick, to_nick, message).await {
        Ok(_) => CommandResponse::BotPm(format!("Tell saved for {}", to_nick)),
        Err(e) => CommandResponse::BotPm(format!("Failed to save tell: {}", e)),
    }
}

async fn history(ctx: CommandContext) -> CommandResponse {
    let limit: i64 = ctx.args.parse().unwrap_or(10).min(50);

    match queries::get_chat_history(&ctx.db, limit, 0).await {
        Ok(messages) => {
            if messages.is_empty() {
                return CommandResponse::ChatSingle("No chat history found.".to_string());
            }
            let mut lines = Vec::new();
            for msg in messages.iter().rev() {
                let ts = msg
                    .created_at
                    .map(|t| t.format("%H:%M").to_string())
                    .unwrap_or_default();
                lines.push(format!("[{}] <{}> {}", ts, msg.nick, msg.message));
            }
            CommandResponse::ChatSingle(lines.join("\n"))
        }
        Err(e) => CommandResponse::ChatSingle(format!("Failed to get history: {}", e)),
    }
}

async fn search(ctx: CommandContext) -> CommandResponse {
    if ctx.args.len() < 3 {
        return CommandResponse::ChatSingle("Usage: !search <query> (min 3 chars)".to_string());
    }

    match queries::search_chat(&ctx.db, &ctx.args, None, 10).await {
        Ok(messages) => {
            if messages.is_empty() {
                return CommandResponse::ChatSingle(format!("No results for '{}'", ctx.args));
            }
            let mut lines = Vec::new();
            for msg in &messages {
                let ts = msg
                    .created_at
                    .map(|t| t.format("%Y-%m-%d %H:%M").to_string())
                    .unwrap_or_default();
                lines.push(format!("[{}] <{}> {}", ts, msg.nick, msg.message));
            }
            CommandResponse::ChatSingle(lines.join("\n"))
        }
        Err(e) => CommandResponse::ChatSingle(format!("Search failed: {}", e)),
    }
}

async fn seen(ctx: CommandContext) -> CommandResponse {
    let nick = ctx.args.trim();
    if nick.is_empty() {
        return CommandResponse::ChatAll("Usage: !seen <nick>".to_string());
    }

    match queries::get_user(&ctx.db, nick).await {
        Ok(Some(user)) => {
            let last = user
                .last_seen
                .map(|t| t.format("%Y-%m-%d %H:%M UTC").to_string())
                .unwrap_or_else(|| "never".to_string());
            CommandResponse::ChatAll(format!("{} was last seen: {}", nick, last))
        }
        Ok(None) => CommandResponse::ChatAll(format!("Never seen '{}'", nick)),
        Err(e) => CommandResponse::ChatAll(format!("Error: {}", e)),
    }
}

async fn first(ctx: CommandContext) -> CommandResponse {
    let nick = ctx.args.trim();
    if nick.is_empty() {
        return CommandResponse::ChatAll("Usage: !first <nick>".to_string());
    }

    match queries::first_message(&ctx.db, nick).await {
        Ok(Some(msg)) => {
            let ts = msg
                .created_at
                .map(|t| t.format("%Y-%m-%d %H:%M").to_string())
                .unwrap_or_default();
            CommandResponse::ChatAll(format!("[{}] <{}> {}", ts, msg.nick, msg.message))
        }
        Ok(None) => CommandResponse::ChatAll(format!("No messages found for '{}'", nick)),
        Err(e) => CommandResponse::ChatAll(format!("Error: {}", e)),
    }
}

async fn last(ctx: CommandContext) -> CommandResponse {
    let nick = ctx.args.trim();
    if nick.is_empty() {
        return CommandResponse::ChatSingle("Usage: !last <nick>".to_string());
    }

    match queries::last_message(&ctx.db, nick).await {
        Ok(Some(msg)) => {
            let ts = msg
                .created_at
                .map(|t| t.format("%Y-%m-%d %H:%M").to_string())
                .unwrap_or_default();
            CommandResponse::ChatSingle(format!("[{}] <{}> {}", ts, msg.nick, msg.message))
        }
        Ok(None) => CommandResponse::ChatSingle(format!("No messages found for '{}'", nick)),
        Err(e) => CommandResponse::ChatSingle(format!("Error: {}", e)),
    }
}

async fn quote(ctx: CommandContext) -> CommandResponse {
    let nick_filter = if ctx.args.trim().is_empty() {
        None
    } else {
        Some(ctx.args.trim())
    };

    match queries::random_quote(&ctx.db, nick_filter).await {
        Ok(Some(q)) => {
            let ts = q
                .created_at
                .map(|t| t.format("%Y-%m-%d").to_string())
                .unwrap_or_default();
            CommandResponse::ChatAll(format!("[{}] <{}> {}", ts, q.nick, q.quote_text))
        }
        Ok(None) => CommandResponse::ChatAll("No quotes found.".to_string()),
        Err(e) => CommandResponse::ChatAll(format!("Error: {}", e)),
    }
}

async fn stats(ctx: CommandContext) -> CommandResponse {
    match queries::get_stats_history(&ctx.db, 1).await {
        Ok(snapshots) => {
            if let Some(s) = snapshots.first() {
                let share_gb = s.total_share as f64 / 1_073_741_824.0;
                let ts = s
                    .created_at
                    .map(|t| t.format("%Y-%m-%d %H:%M").to_string())
                    .unwrap_or_default();
                CommandResponse::ChatSingle(format!(
                    "Hub stats ({}): {} users, {:.1} GB shared",
                    ts, s.user_count, share_gb
                ))
            } else {
                CommandResponse::ChatSingle("No stats available yet.".to_string())
            }
        }
        Err(e) => CommandResponse::ChatSingle(format!("Error: {}", e)),
    }
}

async fn watch(ctx: CommandContext) -> CommandResponse {
    let watched = ctx.args.trim();
    if watched.is_empty() {
        return CommandResponse::ChatSingle("Usage: !watch <nick>".to_string());
    }

    match queries::create_watch(&ctx.db, &ctx.nick, watched).await {
        Ok(()) => CommandResponse::ChatSingle(format!("Now watching {}", watched)),
        Err(e) => CommandResponse::ChatSingle(format!("Failed: {}", e)),
    }
}

async fn unwatch(ctx: CommandContext) -> CommandResponse {
    let watched = ctx.args.trim();
    if watched.is_empty() {
        return CommandResponse::ChatSingle("Usage: !unwatch <nick>".to_string());
    }

    match queries::delete_watch(&ctx.db, &ctx.nick, watched).await {
        Ok(true) => CommandResponse::ChatSingle(format!("Stopped watching {}", watched)),
        Ok(false) => CommandResponse::ChatSingle(format!("You weren't watching {}", watched)),
        Err(e) => CommandResponse::ChatSingle(format!("Failed: {}", e)),
    }
}

async fn info(ctx: CommandContext) -> CommandResponse {
    let nick = if ctx.args.trim().is_empty() {
        &ctx.nick
    } else {
        ctx.args.trim()
    };

    match queries::get_user(&ctx.db, nick).await {
        Ok(Some(user)) => {
            let share_gb = user.share_size as f64 / 1_073_741_824.0;
            let first = user
                .first_seen
                .map(|t| t.format("%Y-%m-%d").to_string())
                .unwrap_or_else(|| "unknown".to_string());
            let last = user
                .last_seen
                .map(|t| t.format("%Y-%m-%d %H:%M").to_string())
                .unwrap_or_else(|| "unknown".to_string());
            CommandResponse::ChatSingle(format!(
                "User: {} | Share: {:.1} GB | First seen: {} | Last seen: {} | Email: {}",
                user.nick, share_gb, first, last, user.email
            ))
        }
        Ok(None) => CommandResponse::ChatSingle(format!("User '{}' not found", nick)),
        Err(e) => CommandResponse::ChatSingle(format!("Error: {}", e)),
    }
}

async fn ban(ctx: CommandContext) -> CommandResponse {
    let parts: Vec<&str> = ctx.args.splitn(2, ' ').collect();
    let target = match parts.first() {
        Some(t) if !t.is_empty() => *t,
        _ => return CommandResponse::ChatSingle("Usage: !ban <nick> [reason]".to_string()),
    };
    let reason = parts.get(1).unwrap_or(&"").to_string();

    match queries::create_ban(&ctx.db, Some(target), None, &reason, &ctx.nick, None).await {
        Ok(_) => {
            let cmd = serde_json::json!({"type": "ban", "entry": target});
            let _ = ctx.hub_tx.send(cmd.to_string()).await;
            let kick_cmd = serde_json::json!({"type": "kick", "nick": target});
            let _ = ctx.hub_tx.send(kick_cmd.to_string()).await;
            CommandResponse::ChatAll(format!(
                "{} banned {}{}",
                ctx.nick,
                target,
                if reason.is_empty() {
                    String::new()
                } else {
                    format!(": {}", reason)
                }
            ))
        }
        Err(e) => CommandResponse::ChatSingle(format!("Ban failed: {}", e)),
    }
}

async fn unban(ctx: CommandContext) -> CommandResponse {
    let target = ctx.args.trim();
    if target.is_empty() {
        return CommandResponse::ChatSingle("Usage: !unban <nick>".to_string());
    }

    match queries::check_ban(&ctx.db, target).await {
        Ok(Some(active_ban)) => {
            queries::delete_ban(&ctx.db, active_ban.id).await.ok();
            let cmd = serde_json::json!({"type": "unban", "entry": target});
            let _ = ctx.hub_tx.send(cmd.to_string()).await;
            CommandResponse::ChatAll(format!("{} unbanned {}", ctx.nick, target))
        }
        Ok(None) => CommandResponse::ChatSingle(format!("{} is not banned", target)),
        Err(e) => CommandResponse::ChatSingle(format!("Unban error: {}", e)),
    }
}

async fn kick(ctx: CommandContext) -> CommandResponse {
    let parts: Vec<&str> = ctx.args.splitn(2, ' ').collect();
    let target = match parts.first() {
        Some(t) if !t.is_empty() => *t,
        _ => return CommandResponse::ChatSingle("Usage: !kick <nick> [reason]".to_string()),
    };
    let reason = parts.get(1).unwrap_or(&"").to_string();

    // Send kick reason as Hub-Security PM to victim
    if !reason.is_empty() {
        let reason_pm = serde_json::json!({
            "type": "send_to",
            "nick": target,
            "message": format!("You have been kicked: {}", reason),
        });
        let _ = ctx.hub_tx.send(reason_pm.to_string()).await;
    }

    // Send kick command
    let cmd = serde_json::json!({"type": "kick", "nick": target});
    let _ = ctx.hub_tx.send(cmd.to_string()).await;

    // Public announcement
    CommandResponse::ChatAll(format!(
        "{} kicked {}{}",
        ctx.nick,
        target,
        if reason.is_empty() {
            String::new()
        } else {
            format!(": {}", reason)
        }
    ))
}

async fn gag(ctx: CommandContext) -> CommandResponse {
    let parts: Vec<&str> = ctx.args.splitn(2, ' ').collect();
    let target = match parts.first() {
        Some(t) if !t.is_empty() => *t,
        _ => return CommandResponse::ChatSingle("Usage: !gag <nick> [reason]".to_string()),
    };
    let reason = parts.get(1).unwrap_or(&"").to_string();

    match queries::create_gag(&ctx.db, target, &reason, &ctx.nick, None).await {
        Ok(_) => {
            // Send gag command to hub
            let cmd = serde_json::json!({"type": "gag", "nick": target});
            let _ = ctx.hub_tx.send(cmd.to_string()).await;

            // PM the victim directly (not through CommandResponse which targets the invoker)
            let victim_pm = serde_json::json!({
                "type": "send_pm_as",
                "from": "Hub-Security",
                "to": target,
                "message": format!("You have been gagged by {}: {}", ctx.nick, reason),
            });
            let _ = ctx.hub_tx.send(victim_pm.to_string()).await;

            // Public announcement
            CommandResponse::ChatAll(format!(
                "{} gagged {}{}",
                ctx.nick,
                target,
                if reason.is_empty() {
                    String::new()
                } else {
                    format!(": {}", reason)
                }
            ))
        }
        Err(e) => CommandResponse::ChatSingle(format!("Gag failed: {}", e)),
    }
}

async fn ungag(ctx: CommandContext) -> CommandResponse {
    let target = ctx.args.trim();
    if target.is_empty() {
        return CommandResponse::ChatSingle("Usage: !ungag <nick>".to_string());
    }

    match queries::check_gag(&ctx.db, target).await {
        Ok(Some(active_gag)) => {
            queries::delete_gag(&ctx.db, active_gag.id).await.ok();
            let cmd = serde_json::json!({"type": "ungag", "nick": target});
            let _ = ctx.hub_tx.send(cmd.to_string()).await;
            CommandResponse::ChatAll(format!("{} ungagged {}", ctx.nick, target))
        }
        Ok(None) => CommandResponse::ChatSingle(format!("{} is not gagged", target)),
        Err(e) => CommandResponse::ChatSingle(format!("Ungag error: {}", e)),
    }
}

async fn topic(ctx: CommandContext) -> CommandResponse {
    let new_topic = ctx.args.trim();
    if new_topic.is_empty() {
        return CommandResponse::ChatSingle("Usage: !topic <new topic>".to_string());
    }

    // Send raw $HubName to actually set the topic in DC clients
    let hub_name_cmd = serde_json::json!({
        "type": "send_all",
        "message": format!("$HubName {}", new_topic),
    });
    let _ = ctx.hub_tx.send(hub_name_cmd.to_string()).await;

    CommandResponse::ChatAll(format!("Topic set to: {}", new_topic))
}

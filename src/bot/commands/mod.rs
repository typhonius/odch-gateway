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

async fn help(_ctx: CommandContext) -> CommandResponse {
    CommandResponse::Reply(
        "Commands: !help !tell !history !search !seen !first !last !quote \
         !stats !watch !unwatch !info !ban !unban !kick !gag !ungag !topic"
            .to_string(),
    )
}

async fn tell(ctx: CommandContext) -> CommandResponse {
    let parts: Vec<&str> = ctx.args.splitn(2, ' ').collect();
    if parts.len() < 2 {
        return CommandResponse::Reply("Usage: !tell <nick> <message>".to_string());
    }
    let to_nick = parts[0];
    let message = parts[1];

    match queries::create_tell(&ctx.db, &ctx.nick, to_nick, message).await {
        Ok(_) => CommandResponse::Reply(format!("Tell saved for {}", to_nick)),
        Err(e) => CommandResponse::Reply(format!("Failed to save tell: {}", e)),
    }
}

async fn history(ctx: CommandContext) -> CommandResponse {
    let limit: i64 = ctx.args.parse().unwrap_or(10).min(50);

    match queries::get_chat_history(&ctx.db, limit, 0).await {
        Ok(messages) => {
            if messages.is_empty() {
                return CommandResponse::Reply("No chat history found.".to_string());
            }
            let mut lines = Vec::new();
            for msg in messages.iter().rev() {
                let ts = msg
                    .created_at
                    .map(|t| t.format("%H:%M").to_string())
                    .unwrap_or_default();
                lines.push(format!("[{}] <{}> {}", ts, msg.nick, msg.message));
            }
            CommandResponse::Reply(lines.join("\n"))
        }
        Err(e) => CommandResponse::Reply(format!("Failed to get history: {}", e)),
    }
}

async fn search(ctx: CommandContext) -> CommandResponse {
    if ctx.args.len() < 3 {
        return CommandResponse::Reply("Usage: !search <query> (min 3 chars)".to_string());
    }

    match queries::search_chat(&ctx.db, &ctx.args, None, 10).await {
        Ok(messages) => {
            if messages.is_empty() {
                return CommandResponse::Reply(format!("No results for '{}'", ctx.args));
            }
            let mut lines = Vec::new();
            for msg in &messages {
                let ts = msg
                    .created_at
                    .map(|t| t.format("%Y-%m-%d %H:%M").to_string())
                    .unwrap_or_default();
                lines.push(format!("[{}] <{}> {}", ts, msg.nick, msg.message));
            }
            CommandResponse::Reply(lines.join("\n"))
        }
        Err(e) => CommandResponse::Reply(format!("Search failed: {}", e)),
    }
}

async fn seen(ctx: CommandContext) -> CommandResponse {
    let nick = ctx.args.trim();
    if nick.is_empty() {
        return CommandResponse::Reply("Usage: !seen <nick>".to_string());
    }

    match queries::get_user(&ctx.db, nick).await {
        Ok(Some(user)) => {
            let last = user
                .last_seen
                .map(|t| t.format("%Y-%m-%d %H:%M UTC").to_string())
                .unwrap_or_else(|| "never".to_string());
            CommandResponse::Reply(format!("{} was last seen: {}", nick, last))
        }
        Ok(None) => CommandResponse::Reply(format!("Never seen '{}'", nick)),
        Err(e) => CommandResponse::Reply(format!("Error: {}", e)),
    }
}

async fn first(ctx: CommandContext) -> CommandResponse {
    let nick = ctx.args.trim();
    if nick.is_empty() {
        return CommandResponse::Reply("Usage: !first <nick>".to_string());
    }

    match queries::first_message(&ctx.db, nick).await {
        Ok(Some(msg)) => {
            let ts = msg
                .created_at
                .map(|t| t.format("%Y-%m-%d %H:%M").to_string())
                .unwrap_or_default();
            CommandResponse::Reply(format!("[{}] <{}> {}", ts, msg.nick, msg.message))
        }
        Ok(None) => CommandResponse::Reply(format!("No messages found for '{}'", nick)),
        Err(e) => CommandResponse::Reply(format!("Error: {}", e)),
    }
}

async fn last(ctx: CommandContext) -> CommandResponse {
    let nick = ctx.args.trim();
    if nick.is_empty() {
        return CommandResponse::Reply("Usage: !last <nick>".to_string());
    }

    match queries::last_message(&ctx.db, nick).await {
        Ok(Some(msg)) => {
            let ts = msg
                .created_at
                .map(|t| t.format("%Y-%m-%d %H:%M").to_string())
                .unwrap_or_default();
            CommandResponse::Reply(format!("[{}] <{}> {}", ts, msg.nick, msg.message))
        }
        Ok(None) => CommandResponse::Reply(format!("No messages found for '{}'", nick)),
        Err(e) => CommandResponse::Reply(format!("Error: {}", e)),
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
            CommandResponse::Reply(format!("[{}] <{}> {}", ts, q.nick, q.quote_text))
        }
        Ok(None) => CommandResponse::Reply("No quotes found.".to_string()),
        Err(e) => CommandResponse::Reply(format!("Error: {}", e)),
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
                CommandResponse::Reply(format!(
                    "Hub stats ({}): {} users, {:.1} GB shared",
                    ts, s.user_count, share_gb
                ))
            } else {
                CommandResponse::Reply("No stats available yet.".to_string())
            }
        }
        Err(e) => CommandResponse::Reply(format!("Error: {}", e)),
    }
}

async fn watch(ctx: CommandContext) -> CommandResponse {
    let watched = ctx.args.trim();
    if watched.is_empty() {
        return CommandResponse::Reply("Usage: !watch <nick>".to_string());
    }

    match queries::create_watch(&ctx.db, &ctx.nick, watched).await {
        Ok(()) => CommandResponse::Reply(format!("Now watching {}", watched)),
        Err(e) => CommandResponse::Reply(format!("Failed: {}", e)),
    }
}

async fn unwatch(ctx: CommandContext) -> CommandResponse {
    let watched = ctx.args.trim();
    if watched.is_empty() {
        return CommandResponse::Reply("Usage: !unwatch <nick>".to_string());
    }

    match queries::delete_watch(&ctx.db, &ctx.nick, watched).await {
        Ok(true) => CommandResponse::Reply(format!("Stopped watching {}", watched)),
        Ok(false) => CommandResponse::Reply(format!("You weren't watching {}", watched)),
        Err(e) => CommandResponse::Reply(format!("Failed: {}", e)),
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
            CommandResponse::Reply(format!(
                "User: {} | Share: {:.1} GB | First seen: {} | Last seen: {} | Email: {}",
                user.nick, share_gb, first, last, user.email
            ))
        }
        Ok(None) => CommandResponse::Reply(format!("User '{}' not found", nick)),
        Err(e) => CommandResponse::Reply(format!("Error: {}", e)),
    }
}

async fn ban(ctx: CommandContext) -> CommandResponse {
    let parts: Vec<&str> = ctx.args.splitn(2, ' ').collect();
    let target = match parts.first() {
        Some(t) if !t.is_empty() => *t,
        _ => return CommandResponse::Reply("Usage: !ban <nick> [reason]".to_string()),
    };
    let reason = parts.get(1).unwrap_or(&"").to_string();

    match queries::create_ban(&ctx.db, Some(target), None, &reason, &ctx.nick, None).await {
        Ok(_) => {
            let cmd = serde_json::json!({"type": "ban", "entry": target});
            let _ = ctx.hub_tx.send(cmd.to_string()).await;
            let kick_cmd = serde_json::json!({"type": "kick", "nick": target});
            let _ = ctx.hub_tx.send(kick_cmd.to_string()).await;
            CommandResponse::Reply(format!(
                "Banned {}{}",
                target,
                if reason.is_empty() {
                    String::new()
                } else {
                    format!(" ({})", reason)
                }
            ))
        }
        Err(e) => CommandResponse::Reply(format!("Ban failed: {}", e)),
    }
}

async fn unban(ctx: CommandContext) -> CommandResponse {
    let target = ctx.args.trim();
    if target.is_empty() {
        return CommandResponse::Reply("Usage: !unban <nick>".to_string());
    }

    match queries::check_ban(&ctx.db, target).await {
        Ok(Some(active_ban)) => {
            queries::delete_ban(&ctx.db, active_ban.id).await.ok();
            let cmd = serde_json::json!({"type": "unban", "entry": target});
            let _ = ctx.hub_tx.send(cmd.to_string()).await;
            CommandResponse::Reply(format!("Unbanned {}", target))
        }
        Ok(None) => CommandResponse::Reply(format!("{} is not banned", target)),
        Err(e) => CommandResponse::Reply(format!("Unban error: {}", e)),
    }
}

async fn kick(ctx: CommandContext) -> CommandResponse {
    let target = ctx.args.trim();
    if target.is_empty() {
        return CommandResponse::Reply("Usage: !kick <nick>".to_string());
    }

    let cmd = serde_json::json!({"type": "kick", "nick": target});
    match ctx.hub_tx.send(cmd.to_string()).await {
        Ok(()) => CommandResponse::Reply(format!("Kicked {}", target)),
        Err(e) => CommandResponse::Reply(format!("Kick failed: {}", e)),
    }
}

async fn gag(ctx: CommandContext) -> CommandResponse {
    let parts: Vec<&str> = ctx.args.splitn(2, ' ').collect();
    let target = match parts.first() {
        Some(t) if !t.is_empty() => *t,
        _ => return CommandResponse::Reply("Usage: !gag <nick> [reason]".to_string()),
    };
    let reason = parts.get(1).unwrap_or(&"").to_string();

    match queries::create_gag(&ctx.db, target, &reason, &ctx.nick, None).await {
        Ok(_) => {
            let cmd = serde_json::json!({"type": "gag", "nick": target});
            let _ = ctx.hub_tx.send(cmd.to_string()).await;
            CommandResponse::Reply(format!("Gagged {}", target))
        }
        Err(e) => CommandResponse::Reply(format!("Gag failed: {}", e)),
    }
}

async fn ungag(ctx: CommandContext) -> CommandResponse {
    let target = ctx.args.trim();
    if target.is_empty() {
        return CommandResponse::Reply("Usage: !ungag <nick>".to_string());
    }

    match queries::check_gag(&ctx.db, target).await {
        Ok(Some(active_gag)) => {
            queries::delete_gag(&ctx.db, active_gag.id).await.ok();
            let cmd = serde_json::json!({"type": "ungag", "nick": target});
            let _ = ctx.hub_tx.send(cmd.to_string()).await;
            CommandResponse::Reply(format!("Ungagged {}", target))
        }
        Ok(None) => CommandResponse::Reply(format!("{} is not gagged", target)),
        Err(e) => CommandResponse::Reply(format!("Ungag error: {}", e)),
    }
}

async fn topic(ctx: CommandContext) -> CommandResponse {
    let new_topic = ctx.args.trim();
    if new_topic.is_empty() {
        return CommandResponse::Reply("Usage: !topic <new topic>".to_string());
    }

    let cmd = serde_json::json!({"type": "send_all", "message": format!("$HubName {} - {}|", "Hub", new_topic)});
    match ctx.hub_tx.send(cmd.to_string()).await {
        Ok(()) => CommandResponse::Reply(format!("Topic set to: {}", new_topic)),
        Err(e) => CommandResponse::Reply(format!("Failed: {}", e)),
    }
}

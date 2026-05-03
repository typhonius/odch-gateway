//! Built-in bot commands.
//!
//! Each command is a function that returns a boxed async handler.

use super::{CommandContext, CommandEngine, CommandHandler, CommandResponse};
use crate::db::queries;
use crate::event::HubEvent;

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

/// Command descriptions for help display.
fn command_help(name: &str) -> Option<(&'static str, &'static str)> {
    // Returns (usage, description)
    match name {
        "help" | "h" | "commands" => Some(("!help [command]", "Show commands or help for a specific command")),
        "tell" | "msg" => Some(("!tell <nick> <message>", "Leave a message for an offline user")),
        "history" | "hist" => Some(("!history [count]", "Show recent chat history")),
        "search" => Some(("!search <query>", "Search chat history (min 3 chars)")),
        "seen" => Some(("!seen <nick>", "Check when a user was last online")),
        "first" => Some(("!first [nick]", "Show a user's first ever chat message")),
        "last" => Some(("!last [nick]", "Show a user's most recent chat message")),
        "quote" | "q" => Some(("!quote [nick]", "Random quote from chat history")),
        "stats" => Some(("!stats", "Show hub statistics")),
        "watch" | "w" => Some(("!watch <nick>", "Get notified when a user logs in/out")),
        "unwatch" | "uw" => Some(("!unwatch <nick>", "Stop watching a user")),
        "info" => Some(("!info [nick]", "Show user info (share, first seen, etc.)")),
        "ban" => Some(("!ban <nick> [reason]", "Ban and kick a user")),
        "unban" => Some(("!unban <nick>", "Remove a ban")),
        "kick" => Some(("!kick <nick> [reason]", "Kick a user from the hub")),
        "gag" | "mute" => Some(("!gag <nick> [reason]", "Silence a user")),
        "ungag" | "unmute" => Some(("!ungag <nick>", "Unsilence a user")),
        "topic" => Some(("!topic <text>", "Set the hub topic")),
        _ => None,
    }
}

async fn help(ctx: CommandContext) -> CommandResponse {
    let query = ctx.args.trim().to_lowercase();

    // Specific command help: !help tell
    if !query.is_empty() {
        let cmd_name = query.trim_start_matches('!');
        if let Some((usage, desc)) = command_help(cmd_name) {
            return CommandResponse::ChatSingle(format!(
                "Help: {}\n  {}", usage, desc
            ));
        }
        // Check external bot commands
        let bots = ctx.bot_registry.bots.read().await;
        for bot in bots.values() {
            if bot.commands.contains(cmd_name) {
                return CommandResponse::ChatSingle(format!(
                    "!{} — provided by {} (external bot)", cmd_name, bot.nick
                ));
            }
        }
        return CommandResponse::ChatSingle(format!(
            "Unknown command '{}'. Type !help for a list.", cmd_name
        ));
    }

    // General help: formatted list
    let mut msg = String::from("\n=== Hub Commands ===\n");
    msg.push_str("  !help [cmd]       Show help\n");
    msg.push_str("  !tell <nick> msg  Leave a message\n");
    msg.push_str("  !seen <nick>      Last seen\n");
    msg.push_str("  !first [nick]     First message\n");
    msg.push_str("  !last [nick]      Last message\n");
    msg.push_str("  !quote [nick]     Random quote\n");
    msg.push_str("  !history [n]      Chat history\n");
    msg.push_str("  !search <query>   Search chat\n");
    msg.push_str("  !info [nick]      User info\n");
    msg.push_str("  !stats            Hub stats\n");
    msg.push_str("  !watch <nick>     Watch login/logout\n");
    msg.push_str("  !unwatch <nick>   Stop watching\n");
    msg.push_str("  !topic <text>     Set hub topic\n");
    msg.push_str("  !kick <nick>      Kick user\n");
    msg.push_str("  !ban <nick>       Ban user\n");
    msg.push_str("  !unban <nick>     Unban user\n");
    msg.push_str("  !gag <nick>       Silence user\n");
    msg.push_str("  !ungag <nick>     Unsilence user\n");

    // External bot commands
    let bots = ctx.bot_registry.bots.read().await;
    for bot in bots.values() {
        if !bot.commands.is_empty() {
            let mut cmds: Vec<&String> = bot.commands.iter().collect();
            cmds.sort();
            msg.push_str(&format!("\n=== {} ===\n", bot.nick));
            for cmd in cmds {
                msg.push_str(&format!("  !{}\n", cmd));
            }
        }
    }

    msg.push_str("\nType !help <command> for details.");

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
        Some(ctx.args.trim().to_string())
    };

    // Pull a random message from chat history
    let query = if let Some(ref nick) = nick_filter {
        sqlx::query_as::<_, crate::db::models::ChatMessage>(
            "SELECT id, nick, message, created_at \
             FROM chat_messages WHERE nick = $1 ORDER BY RANDOM() LIMIT 1",
        )
        .bind(nick)
        .fetch_optional(&ctx.db)
        .await
    } else {
        sqlx::query_as::<_, crate::db::models::ChatMessage>(
            "SELECT id, nick, message, created_at \
             FROM chat_messages ORDER BY RANDOM() LIMIT 1",
        )
        .fetch_optional(&ctx.db)
        .await
    };

    match query {
        Ok(Some(msg)) => {
            let ts = msg
                .created_at
                .map(|t| t.format("%Y-%m-%d %H:%M").to_string())
                .unwrap_or_default();
            CommandResponse::ChatAll(format!("[{}] <{}> {}", ts, msg.nick, msg.message))
        }
        Ok(None) => CommandResponse::ChatAll("No chat history found.".to_string()),
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
            let level = match user.permission {
                3 => "Admin",
                2 => "OP",
                1 => "Registered",
                _ => "Regular",
            };
            CommandResponse::ChatSingle(format!(
                "User: {} | Level: {} | Share: {:.1} GB | First seen: {} | Last seen: {}",
                user.nick, level, share_gb, first, last
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
            // Kick the user (NMDC protocol operation)
            let kick_cmd = serde_json::json!({"type": "kick", "nick": target});
            let _ = ctx.hub_tx.send(kick_cmd.to_string()).await;

            ctx.event_bus.publish(HubEvent::Ban {
                nick: target.to_string(),
                by: ctx.nick.clone(),
                reason: reason.clone(),
                timestamp: chrono::Utc::now(),
            });

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

            ctx.event_bus.publish(HubEvent::Unban {
                nick: target.to_string(),
                by: ctx.nick.clone(),
                timestamp: chrono::Utc::now(),
            });

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
            // Notify the victim via raw protocol message
            let victim_msg = format!(
                "<Hub-Security> You have been gagged by {}: {}|",
                ctx.nick, reason
            );
            let cmd = serde_json::json!({
                "type": "send_raw_to",
                "nick": target,
                "data": victim_msg,
            });
            let _ = ctx.hub_tx.send(cmd.to_string()).await;

            ctx.event_bus.publish(HubEvent::Gag {
                nick: target.to_string(),
                by: ctx.nick.clone(),
                reason: reason.clone(),
                timestamp: chrono::Utc::now(),
            });

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

            ctx.event_bus.publish(HubEvent::Ungag {
                nick: target.to_string(),
                by: ctx.nick.clone(),
                timestamp: chrono::Utc::now(),
            });

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

    // Update gateway state
    *ctx.hub_state.topic.write().await = new_topic.to_string();

    // Persist to database
    queries::set_setting(&ctx.db, "hub_topic", new_topic).await.ok();

    // Broadcast topic change to all connected users via raw NMDC
    let hub_name = ctx.hub_state.hub_name.read().await.clone();
    let raw = if new_topic.is_empty() {
        format!("$HubName {}|", hub_name)
    } else {
        format!("$HubName {} - {}|", hub_name, new_topic)
    };
    let cmd = serde_json::json!({
        "type": "send_raw",
        "data": raw,
    });
    let _ = ctx.hub_tx.send(cmd.to_string()).await;

    CommandResponse::ChatAll(format!("Topic set to: {}", new_topic))
}

use std::error::Error;
use std::sync::Arc;
use std::time::{Duration, Instant};
use teloxide::{prelude::*, utils::command::BotCommands};
use teloxide::types::{InlineKeyboardButton, InlineKeyboardMarkup, CallbackQuery, InlineQuery, InlineQueryResult, InlineQueryResultArticle, InputMessageContent, InputMessageContentText, MenuButton};
use tokio::sync::Mutex;
use log::{error, info, warn};
use local_ip_address::local_ip;

mod synology;
use synology::SynologyClient;

mod bao;
use bao::{decrypt_ciphertext, generate_ephemeral_key, random_session_id, BaoClient};

// OpenBao unseal configuration (optional feature; enabled when both env vars are set)
struct BaoConfig {
    client: BaoClient,
    web_url: String,
}

impl BaoConfig {
    fn from_env() -> Option<Self> {
        // Note: compose always defines these vars (possibly empty) — treat empty as unset.
        let addr = std::env::var("STB_BAO_ADDR").ok().filter(|v| !v.is_empty())?;
        let web_url = std::env::var("STB_UNSEAL_WEB_URL").ok().filter(|v| !v.is_empty())?;
        info!("OpenBao unseal support enabled (addr: {}, web: {})", addr, web_url);
        Some(BaoConfig {
            client: BaoClient::new(&addr),
            web_url: web_url.trim_end_matches('/').to_string(),
        })
    }
}

// Pending unseal challenge: ephemeral keypair + session reference (lives only in RAM)
struct UnsealSession {
    chat_id: ChatId,
    session_id: String,
    identity: age::x25519::Identity,
    since: Instant,
}

const UNSEAL_SESSION_TIMEOUT: Duration = Duration::from_secs(300);

// Structure to hold the Synology client configuration
struct SynologyConfig {
    client: Option<SynologyClient>,
    nas_base_url: String,
    username: String,
    password: String,
    force_ipv4: bool,
}

// Callback data for menu buttons
const CALLBACK_SSH_MENU: &str = "ssh_menu";
const CALLBACK_SSH_ON: &str = "ssh_on";
const CALLBACK_SSH_OFF: &str = "ssh_off";
const CALLBACK_SETTINGS: &str = "settings";
const CALLBACK_BACK: &str = "back";

impl SynologyConfig {
    fn new() -> Self {
        let nas_base_url = std::env::var("STB_SYNOLOGY_NAS_BASE_URL").unwrap();
        let username = std::env::var("STB_SYNOLOGY_USERNAME").unwrap_or_else(|_| {
            warn!("STB_SYNOLOGY_USERNAME environment variable not set");
            String::new()
        });
        let password = std::env::var("STB_SYNOLOGY_PASSWORD").unwrap_or_else(|_| {
            warn!("STB_SYNOLOGY_PASSWORD environment variable not set");
            String::new()
        });

        // Check if IPv4 should be forced
        let force_ipv4 = std::env::var("STB_FORCE_IPV4")
            .map(|v| v.to_lowercase() == "true" || v == "1")
            .unwrap_or(false);

        if force_ipv4 {
            info!("IPv4 will be forced for Synology API requests");
        }

        info!("Initializing Synology configuration with base URL: {}", nas_base_url);

        SynologyConfig {
            client: None,
            nas_base_url,
            username,
            password,
            force_ipv4,
        }
    }

    fn create_client(&mut self) {
        self.client = Some(SynologyClient::new(
            &self.nas_base_url, 
            &self.username, 
            &self.password,
            self.force_ipv4
        ));
    }

    // Automatically login if needed
    async fn ensure_logged_in(&mut self) -> Result<bool, reqwest::Error> {
        // Create client if it doesn't exist
        if self.client.is_none() {
            // Check if username and password are set
            if self.username.is_empty() || self.password.is_empty() {
                warn!("Cannot login: Synology username or password not set in environment variables");
                return Ok(false);
            }

            self.create_client();
        }

        // The client will automatically attempt login when needed
        Ok(true)
    }
}

// Function to check if a chat ID is authorized
fn is_authorized_chat(chat_id: i64) -> bool {
    if let Ok(allowed_chat_id_str) = std::env::var("STB_ALLOWED_CHAT_ID") {
        if let Ok(allowed_chat_id) = allowed_chat_id_str.parse::<i64>() {
            return chat_id == allowed_chat_id;
        }
    }
    false
}

// Function to create the main menu keyboard
fn create_main_menu() -> InlineKeyboardMarkup {
    let mut keyboard: Vec<Vec<InlineKeyboardButton>> = Vec::new();

    // SSH Control button
    let ssh_button = InlineKeyboardButton::callback("🖥️ SSH Control", CALLBACK_SSH_MENU);

    // Add buttons to keyboard
    keyboard.push(vec![ssh_button]);

    InlineKeyboardMarkup::new(keyboard)
}

// Function to create the SSH menu keyboard based on current status
fn create_ssh_menu(ssh_enabled: bool) -> InlineKeyboardMarkup {
    let mut keyboard: Vec<Vec<InlineKeyboardButton>> = Vec::new();

    // Add the appropriate button based on current SSH status
    if ssh_enabled {
        // SSH is enabled, show disable option
        let ssh_off_button = InlineKeyboardButton::callback("❌ Disable SSH", CALLBACK_SSH_OFF);
        keyboard.push(vec![ssh_off_button]);
    } else {
        // SSH is disabled, show enable option
        let ssh_on_button = InlineKeyboardButton::callback("✅ Enable SSH", CALLBACK_SSH_ON);
        keyboard.push(vec![ssh_on_button]);
    }

    // Back button
    let back_button = InlineKeyboardButton::callback("🔙 Back to Main Menu", CALLBACK_BACK);
    keyboard.push(vec![back_button]);

    InlineKeyboardMarkup::new(keyboard)
}

#[derive(BotCommands, Clone, Debug)]
#[command(rename_rule = "snake_case", description = "Available commands:")]
enum Command {
    #[command(description = "Start the bot.")]
    Start,
    #[command(description = "Get SSH status or enable/disable SSH. Usage: /ssh [on|off]")]
    Ssh(String),
    #[command(description = "Enable SSH service (same as /ssh on)")]
    SshOn,
    #[command(description = "Disable SSH service (same as /ssh off)")]
    SshOff,
    #[command(description = "Show OpenBao seal status")]
    SealStatus,
    #[command(description = "Unseal OpenBao (asks for TOTP code)")]
    Unseal,
}

// Handle commands from BotCommands enum
async fn answer_command(
    bot: Bot,
    msg: Message,
    cmd: Command,
    synology_config: Arc<Mutex<SynologyConfig>>,
    bao_config: Arc<Option<BaoConfig>>,
    pending_unseal: Arc<Mutex<Option<UnsealSession>>>
) -> Result<(), Box<dyn Error + Send + Sync>> {
    // Check if the chat is authorized
    if !is_authorized_chat(msg.chat.id.0) {
        let first_name = msg.from()
            .map(|user| user.first_name.clone())
            .unwrap_or_else(|| String::from("Unknown"));

        warn!("Unauthorized access attempt from user {} with chat ID {}", first_name, msg.chat.id.0);

        bot.send_message(
            msg.chat.id,
            format!("Hello {}({}), unfortunately are not authorized to use this bot.", first_name, msg.chat.id.0)
        ).await?;

        return Ok(());
    }
    info!("Command {:?} received from chat {}", cmd, msg.chat.id.0);
    match cmd {
        Command::Start => {
            // Create the main menu keyboard
            let keyboard = create_main_menu();

            let chat_json = serde_json::to_string_pretty(&msg.chat).unwrap();
            info!("Chat info: {}", chat_json);

            // Send welcome message with the keyboard
            bot.send_message(
                msg.chat.id,
                format!("Welcome {} to your personal Telegram bot! Please select an option from the menu below:", msg.from().unwrap().first_name),
            )
            .reply_markup(keyboard)
            .await?;
        }
        Command::Ssh(arg) => {
            // Get the synology config
            let mut config = synology_config.lock().await;

            // Ensure logged in
            match config.ensure_logged_in().await {
                Ok(true) => {
                    // Now we're logged in, proceed with SSH operations
                    if let Some(client) = &mut config.client {
                        if arg.is_empty() {
                            // Just /ssh - get status
                            match client.get_ssh_status().await {
                                Ok(status) => {
                                    let status_text = if status { "enabled" } else { "disabled" };
                                    bot.send_message(
                                        msg.chat.id,
                                        format!("SSH service is currently {}", status_text)
                                    ).await?;
                                },
                                Err(e) => {
                                    bot.send_message(
                                        msg.chat.id,
                                        format!("Failed to get SSH status: {}", e)
                                    ).await?;
                                }
                            }
                        } else {
                            // /ssh on or /ssh off - set status
                            let command = arg.to_lowercase();

                            if command == "on" || command == "enable" {
                                match client.toggle_ssh(true).await {
                                    Ok(_) => {
                                        info!("SSH service enabled by chat {}", msg.chat.id.0);
                                        bot.send_message(
                                            msg.chat.id,
                                            "SSH service has been enabled"
                                        ).await?;
                                    },
                                    Err(e) => {
                                        error!("Failed to enable SSH service: {}", e);
                                        bot.send_message(
                                            msg.chat.id,
                                            format!("Failed to enable SSH service: {}", e)
                                        ).await?;
                                    }
                                }
                            } else if command == "off" || command == "disable" {
                                match client.toggle_ssh(false).await {
                                    Ok(_) => {
                                        info!("SSH service disabled by chat {}", msg.chat.id.0);
                                        bot.send_message(
                                            msg.chat.id,
                                            "SSH service has been disabled"
                                        ).await?;
                                    },
                                    Err(e) => {
                                        error!("Failed to disable SSH service: {}", e);
                                        bot.send_message(
                                            msg.chat.id,
                                            format!("Failed to disable SSH service: {}", e)
                                        ).await?;
                                    }
                                }
                            } else {
                                bot.send_message(
                                    msg.chat.id,
                                    "Usage: /ssh [on|off] - Get SSH status or enable/disable SSH"
                                ).await?;
                            }
                        }
                    }
                },
                Ok(false) => {
                    bot.send_message(
                        msg.chat.id, 
                        "Could not login to Synology NAS. Please check your SYNOLOGY_USERNAME and SYNOLOGY_PASSWORD environment variables."
                    ).await?;
                },
                Err(e) => {
                    bot.send_message(
                        msg.chat.id, 
                        format!("Failed to login to Synology NAS: {}", e)
                    ).await?;
                }
            }
        }
        Command::SshOn => {
            let mut config = synology_config.lock().await;
            match config.ensure_logged_in().await {
                Ok(true) => {
                    if let Some(client) = &mut config.client {
                        match client.toggle_ssh(true).await {
                            Ok(_) => {
                                info!("SSH service enabled by chat {}", msg.chat.id.0);
                                bot.send_message(msg.chat.id, "SSH service has been enabled").await?;
                            },
                            Err(e) => {
                                error!("Failed to enable SSH service: {}", e);
                                bot.send_message(msg.chat.id, format!("Failed to enable SSH service: {}", e)).await?;
                            }
                        }
                    }
                },
                Ok(false) => {
                    bot.send_message(msg.chat.id, "Could not login to Synology NAS. Please check your SYNOLOGY_USERNAME and SYNOLOGY_PASSWORD environment variables.").await?;
                },
                Err(e) => {
                    bot.send_message(msg.chat.id, format!("Failed to login to Synology NAS: {}", e)).await?;
                }
            }
        }
        Command::SshOff => {
            let mut config = synology_config.lock().await;
            match config.ensure_logged_in().await {
                Ok(true) => {
                    if let Some(client) = &mut config.client {
                        match client.toggle_ssh(false).await {
                            Ok(_) => {
                                info!("SSH service disabled by chat {}", msg.chat.id.0);
                                bot.send_message(msg.chat.id, "SSH service has been disabled").await?;
                            },
                            Err(e) => {
                                error!("Failed to disable SSH service: {}", e);
                                bot.send_message(msg.chat.id, format!("Failed to disable SSH service: {}", e)).await?;
                            }
                        }
                    }
                },
                Ok(false) => {
                    bot.send_message(msg.chat.id, "Could not login to Synology NAS. Please check your SYNOLOGY_USERNAME and SYNOLOGY_PASSWORD environment variables.").await?;
                },
                Err(e) => {
                    bot.send_message(msg.chat.id, format!("Failed to login to Synology NAS: {}", e)).await?;
                }
            }
        }
        Command::SealStatus => {
            match bao_config.as_ref() {
                None => {
                    bot.send_message(msg.chat.id, "OpenBao support is not configured (STB_BAO_* env vars missing).").await?;
                },
                Some(bao) => {
                    match bao.client.seal_status().await {
                        Ok(status) => {
                            let text = if !status.initialized {
                                "OpenBao is NOT INITIALIZED.".to_string()
                            } else if status.sealed {
                                format!("🔒 OpenBao is SEALED (progress {}/{})", status.progress, status.t)
                            } else {
                                "🔓 OpenBao is unsealed.".to_string()
                            };
                            bot.send_message(msg.chat.id, text).await?;
                        },
                        Err(e) => {
                            bot.send_message(msg.chat.id, format!("Failed to get seal status: {}", e)).await?;
                        }
                    }
                }
            }
        }
        Command::Unseal => {
            match bao_config.as_ref() {
                None => {
                    bot.send_message(msg.chat.id, "OpenBao support is not configured (STB_BAO_* env vars missing).").await?;
                },
                Some(bao) => {
                    match bao.client.seal_status().await {
                        Ok(status) => {
                            if !status.initialized {
                                bot.send_message(msg.chat.id, "OpenBao is not initialized yet — unseal not possible.").await?;
                            } else if !status.sealed {
                                bot.send_message(msg.chat.id, "OpenBao is already unsealed.").await?;
                            } else {
                                let key = generate_ephemeral_key();
                                let session_id = random_session_id();
                                let link = format!("{}/#s={}&k={}", bao.web_url, session_id, key.recipient);
                                info!("Unseal session {} created (chat {})", session_id, msg.chat.id.0);
                                *pending_unseal.lock().await = Some(UnsealSession {
                                    chat_id: msg.chat.id,
                                    session_id: session_id.clone(),
                                    identity: key.identity,
                                    since: Instant::now(),
                                });
                                bot.send_message(
                                    msg.chat.id,
                                    format!("🔑 Open this link (valid for {} minutes), paste your unseal token from Bitwarden, encrypt it, and send the ciphertext back here:\n\n{}", UNSEAL_SESSION_TIMEOUT.as_secs() / 60, link)
                                ).await?;
                            }
                        },
                        Err(e) => {
                            bot.send_message(msg.chat.id, format!("Cannot reach OpenBao: {}", e)).await?;
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

// Handle inline queries for command suggestions in the input line
async fn inline_query_handler(
    bot: Bot,
    q: InlineQuery,
    _synology_config: Arc<Mutex<SynologyConfig>>
) -> Result<(), Box<dyn Error + Send + Sync>> {
    // Create a simple text result
    let result = InlineQueryResultArticle::new(
        "1",
        "Command Menu",
        InputMessageContent::Text(
            InputMessageContentText::new("Use /start to see the menu")
                .entities(vec![])
        )
    )
    .description("Show available commands");

    // Convert to InlineQueryResult
    let results = vec![InlineQueryResult::Article(result)];

    // Answer the inline query
    bot.answer_inline_query(q.id, results)
        .cache_time(0) // Don't cache results
        .await?;

    Ok(())
}

// Handle callback queries from inline keyboards
async fn callback_handler(
    bot: Bot,
    q: CallbackQuery,
    synology_config: Arc<Mutex<SynologyConfig>>
) -> Result<(), Box<dyn Error + Send + Sync>> {
    // If the callback query has a message, check if the chat is authorized
    if let Some(message) = &q.message {
        if !is_authorized_chat(message.chat.id.0) {
            let first_name = q.from.first_name.clone();
            let chat_id = message.chat.id.0;

            warn!("Unauthorized callback query from user {} with chat ID {}", first_name, chat_id);

            // Answer the callback query with an error message
            bot.answer_callback_query(q.id)
                .text(format!("You ({}) are not authorized to use this bot. Your chat ID {} is not allowed.", first_name, chat_id))
                .show_alert(true)
                .await?;

            return Ok(());
        }
    }
    // If the callback query has no data, return
    if let Some(data) = &q.data {
        // Get the message and chat ID
        if let Some(message) = q.message {
            let chat_id = message.chat.id;
            info!("Callback '{}' received from chat {}", data, chat_id.0);

            match data.as_str() {
                // Main menu options
                CALLBACK_SSH_MENU => {
                    // Get current SSH status before showing the menu
                    let mut config = synology_config.lock().await;

                    // Ensure logged in
                    match config.ensure_logged_in().await {
                        Ok(true) => {
                            // Now we're logged in, proceed with getting SSH status
                            if let Some(client) = &mut config.client {
                                match client.get_ssh_status().await {
                                    Ok(status) => {
                                        // Create SSH menu based on current status
                                        let keyboard = create_ssh_menu(status);
                                        let status_text = if status { "enabled" } else { "disabled" };

                                        bot.edit_message_text(
                                            chat_id,
                                            message.id,
                                            format!("SSH Control Menu (currently {})", status_text)
                                        )
                                        .reply_markup(keyboard)
                                        .await?;
                                    },
                                    Err(e) => {
                                        error!("Failed to get SSH status: {}", e);
                                        bot.answer_callback_query(q.id)
                                            .text("Failed to get SSH status")
                                            .show_alert(true)
                                            .await?;
                                    }
                                }
                            }
                        },
                        Ok(false) => {
                            bot.answer_callback_query(q.id)
                                .text("Could not login to Synology NAS. Please check your SYNOLOGY_USERNAME and SYNOLOGY_PASSWORD environment variables.")
                                .show_alert(true)
                                .await?;
                        },
                        Err(e) => {
                            error!("Failed to login: {}", e);
                            bot.answer_callback_query(q.id)
                                .text("Failed to login to Synology NAS")
                                .show_alert(true)
                                .await?;
                        }
                    }
                }
                CALLBACK_SSH_ON => {
                    // Enable SSH
                    let mut config = synology_config.lock().await;

                    // Ensure logged in
                    match config.ensure_logged_in().await {
                        Ok(true) => {
                            // Now we're logged in, proceed with enabling SSH
                            if let Some(client) = &mut config.client {
                                match client.toggle_ssh(true).await {
                                    Ok(_) => {
                                        bot.answer_callback_query(q.id)
                                            .text("SSH service has been enabled")
                                            .await?;

                                        // Return to main menu
                                        let keyboard = create_main_menu();
                                        bot.edit_message_text(
                                            chat_id,
                                            message.id,
                                            "SSH service has been enabled. Please select an option from the menu below:"
                                        )
                                        .reply_markup(keyboard)
                                        .await?;
                                    },
                                    Err(e) => {
                                        error!("Failed to toggle ssh service: {}", e);
                                        bot.answer_callback_query(q.id)
                                            .text("Failed to enable SSH service")
                                            .show_alert(true)
                                            .await?;
                                    }
                                }
                            }
                        },
                        Ok(false) => {
                            bot.answer_callback_query(q.id)
                                .text("Could not login to Synology NAS. Please check your SYNOLOGY_USERNAME and SYNOLOGY_PASSWORD environment variables.")
                                .show_alert(true)
                                .await?;
                        },
                        Err(e) => {
                            error!("Failed to toggle ssh service: {}", e);
                            bot.answer_callback_query(q.id)
                                .text("Failed to login to Synology NAS")
                                .show_alert(true)
                                .await?;
                        }
                    }
                }
                CALLBACK_SSH_OFF => {
                    // Disable SSH
                    let mut config = synology_config.lock().await;

                    // Ensure logged in
                    match config.ensure_logged_in().await {
                        Ok(true) => {
                            // Now we're logged in, proceed with disabling SSH
                            if let Some(client) = &mut config.client {
                                match client.toggle_ssh(false).await {
                                    Ok(_) => {
                                        bot.answer_callback_query(q.id)
                                            .text("SSH service has been disabled")
                                            .await?;

                                        // Return to main menu
                                        let keyboard = create_main_menu();
                                        bot.edit_message_text(
                                            chat_id,
                                            message.id,
                                            "SSH service has been disabled. Please select an option from the menu below:"
                                        )
                                        .reply_markup(keyboard)
                                        .await?;
                                    },
                                    Err(e) => {
                                        bot.answer_callback_query(q.id)
                                            .text(format!("Failed to disable SSH service: {}", e))
                                            .show_alert(true)
                                            .await?;
                                    }
                                }
                            }
                        },
                        Ok(false) => {
                            bot.answer_callback_query(q.id)
                                .text("Could not login to Synology NAS. Please check your SYNOLOGY_USERNAME and SYNOLOGY_PASSWORD environment variables.")
                                .show_alert(true)
                                .await?;
                        },
                        Err(e) => {
                            bot.answer_callback_query(q.id)
                                .text(format!("Failed to login to Synology NAS: {}", e))
                                .show_alert(true)
                                .await?;
                        }
                    }
                }
                CALLBACK_SETTINGS => {
                    // Inform user that settings can only be configured via environment variables
                    bot.send_message(
                        chat_id,
                        "Synology settings must be configured via environment variable SYNOLOGY_NAS_BASE_URL. It cannot be changed via Telegram."
                    ).await?;
                }
                CALLBACK_BACK => {
                    // Return to main menu
                    let keyboard = create_main_menu();
                    bot.edit_message_text(
                        chat_id,
                        message.id,
                        "Please select an option from the menu below:"
                    )
                    .reply_markup(keyboard)
                    .await?;
                }
                _ => {
                    bot.answer_callback_query(q.id)
                        .text("Unknown command")
                        .await?;
                }
            }
        }
    }

    Ok(())
}

// Handle all messages
async fn message_handler(
    bot: Bot,
    msg: Message,
    synology_config: Arc<Mutex<SynologyConfig>>,
    bao_config: Arc<Option<BaoConfig>>,
    pending_unseal: Arc<Mutex<Option<UnsealSession>>>
) -> Result<(), Box<dyn Error + Send + Sync>> {
    // Check if the chat is authorized
    if !is_authorized_chat(msg.chat.id.0) {
        let first_name = msg.from()
            .map(|user| user.first_name.clone())
            .unwrap_or_else(|| String::from("Unknown"));

        warn!("Unauthorized access attempt from user {} with chat ID {}", first_name, msg.chat.id.0);

        bot.send_message(
            msg.chat.id,
            format!("You ({}) are not authorized to use this bot. Your chat ID {} is not allowed.", first_name, msg.chat.id.0)
        ).await?;

        return Ok(());
    }

    // Pending unseal session: the next text message in this chat is the age ciphertext
    {
        let mut pending = pending_unseal.lock().await;
        if let Some(p) = pending.as_ref() {
            if p.chat_id == msg.chat.id {
                if p.since.elapsed() > UNSEAL_SESSION_TIMEOUT {
                    let sid = p.session_id.clone();
                    *pending = None;
                    info!("Unseal session {} expired (chat {})", sid, msg.chat.id.0);
                    bot.send_message(msg.chat.id, "Unseal session expired. Call /unseal again for a new link.").await?;
                    return Ok(());
                }
                if let Some(text) = msg.text() {
                    // Take the session out (single attempt; /unseal restarts with a fresh link)
                    let session = pending.take().expect("session checked above");
                    let ciphertext = text.trim().to_string();
                    // Delete the message carrying the ciphertext (hygiene)
                    if let Err(e) = bot.delete_message(msg.chat.id, msg.id).await {
                        warn!("Could not delete ciphertext message: {}", e);
                    }
                    if !ciphertext.starts_with("-----BEGIN AGE ENCRYPTED FILE-----") {
                        warn!("Unseal session {}: message is not an age ciphertext (chat {})", session.session_id, msg.chat.id.0);
                        bot.send_message(msg.chat.id, "That was not an age ciphertext. Please encrypt the token in the web app and send the encrypted text. Call /unseal for a new link.").await?;
                        return Ok(());
                    }
                    match bao_config.as_ref() {
                        None => {
                            bot.send_message(msg.chat.id, "OpenBao support is not configured.").await?;
                        },
                        Some(bao) => {
                            match decrypt_ciphertext(&ciphertext, &session.identity) {
                                Ok(key) => {
                                    info!("Unseal session {}: ciphertext decrypted (chat {})", session.session_id, msg.chat.id.0);
                                    match bao.client.unseal(key.trim()).await {
                                        Ok(status) => {
                                            if status.sealed {
                                                error!("Unseal session {}: key accepted, still sealed (progress {}/{})", session.session_id, status.progress, status.t);
                                                bot.send_message(msg.chat.id, format!("⚠️ Key accepted, still sealed (progress {}/{})", status.progress, status.t)).await?;
                                            } else {
                                                info!("Unseal session {}: OpenBao unsealed via Telegram (chat {})", session.session_id, msg.chat.id.0);
                                                bot.send_message(msg.chat.id, "🔓 OpenBao is now unsealed.").await?;
                                            }
                                        },
                                        Err(e) => {
                                            error!("Unseal session {}: unseal API call failed: {}", session.session_id, e);
                                            bot.send_message(msg.chat.id, format!("Unseal failed: {}", e)).await?;
                                        }
                                    }
                                },
                                Err(e) => {
                                    warn!("Unseal session {}: decryption failed (chat {}): {}", session.session_id, msg.chat.id.0, e);
                                    bot.send_message(msg.chat.id, format!("❌ Decryption failed: {}. Call /unseal for a new link.", e)).await?;
                                }
                            }
                        }
                    }
                    return Ok(());
                }
            }
        }
    }

    if let Some(text) = msg.text() {
        // Try to parse as a command
        if let Ok(command) = Command::parse(text, "synology_bot") {
            return answer_command(bot.clone(), msg.clone(), command, synology_config.clone(), bao_config.clone(), pending_unseal.clone()).await;
        }

        // Handle custom commands


        if text.starts_with("/setnas") {
            // Inform user that settings can only be configured via environment variables
            bot.send_message(
                msg.chat.id, 
                "Synology settings can only be configured via environment variable SYNOLOGY_NAS_BASE_URL. It cannot be changed via Telegram."
            ).await?;
            return Ok(());
        }

        if text.starts_with("/ssh") {
            let parts: Vec<&str> = text.split_whitespace().collect();

            let mut config = synology_config.lock().await;

            // Ensure logged in
            match config.ensure_logged_in().await {
                Ok(true) => {
                    // Now we're logged in, proceed with SSH operations
                    if let Some(client) = &mut config.client {
                        if parts.len() == 1 {
                            // Just /ssh - get status
                            match client.get_ssh_status().await {
                                Ok(status) => {
                                    let status_text = if status { "enabled" } else { "disabled" };
                                    bot.send_message(
                                        msg.chat.id,
                                        format!("SSH service is currently {}", status_text)
                                    ).await?;
                                },
                                Err(e) => {
                                    bot.send_message(
                                        msg.chat.id,
                                        format!("Failed to get SSH status: {}", e)
                                    ).await?;
                                }
                            }
                        } else if parts.len() >= 2 {
                            // /ssh on or /ssh off - set status
                            let command = parts[1].to_lowercase();

                            if command == "on" || command == "enable" {
                                match client.toggle_ssh(true).await {
                                    Ok(_) => {
                                        info!("SSH service enabled by chat {}", msg.chat.id.0);
                                        bot.send_message(
                                            msg.chat.id,
                                            "SSH service has been enabled"
                                        ).await?;
                                    },
                                    Err(e) => {
                                        error!("Failed to enable SSH service: {}", e);
                                        bot.send_message(
                                            msg.chat.id,
                                            format!("Failed to enable SSH service: {}", e)
                                        ).await?;
                                    }
                                }
                            } else if command == "off" || command == "disable" {
                                match client.toggle_ssh(false).await {
                                    Ok(_) => {
                                        info!("SSH service disabled by chat {}", msg.chat.id.0);
                                        bot.send_message(
                                            msg.chat.id,
                                            "SSH service has been disabled"
                                        ).await?;
                                    },
                                    Err(e) => {
                                        error!("Failed to disable SSH service: {}", e);
                                        bot.send_message(
                                            msg.chat.id,
                                            format!("Failed to disable SSH service: {}", e)
                                        ).await?;
                                    }
                                }
                            } else {
                                bot.send_message(
                                    msg.chat.id,
                                    "Usage: /ssh [on|off] - Get SSH status or enable/disable SSH"
                                ).await?;
                            }
                        }
                    }
                },
                Ok(false) => {
                    bot.send_message(
                        msg.chat.id, 
                        "Could not login to Synology NAS. Please check your SYNOLOGY_USERNAME and SYNOLOGY_PASSWORD environment variables."
                    ).await?;
                },
                Err(e) => {
                    bot.send_message(
                        msg.chat.id, 
                        format!("Failed to login to Synology NAS: {}", e)
                    ).await?;
                }
            }
            return Ok(());
        }
    }

    Ok(())
}

#[tokio::main]
async fn main() {
    // Load .env file if present (optional) without overriding existing environment variables
    // This must happen before logger initialization so that STB_RUST_LOG from .env is respected.
    let dotenv_result = dotenvy::dotenv();

    // Initialize the logger
    env_logger::Builder::from_env(env_logger::Env::new().filter_or("STB_RUST_LOG", "debug")).init();

    // Log whether .env was found and from which path, or that it was not found
    match &dotenv_result {
        Ok(path) => info!("Loaded .env from: {}", path.display()),
        Err(err) => {
            // Not found is expected/okay; any other error should be reported
            if matches!(err, dotenvy::Error::Io(e) if e.kind() == std::io::ErrorKind::NotFound) {
                info!(".env file not found; continuing without it");
            } else {
                warn!("Failed to load .env: {}", err);
            }
        }
    }

    info!("Starting Synology Telegram Bot...");

    // Log the current IP address
    match local_ip() {
        Ok(ip) => info!("Current IP address: {}", ip),
        Err(e) => warn!("Could not determine local IP address: {}", e),
    };

    // Get the bot token from environment variable
    let bot_token = std::env::var("STB_TELEGRAM_BOT_TOKEN")
        .expect("STB_TELEGRAM_BOT_TOKEN environment variable is not set");

    // Initialize Synology configuration
    let synology_config = Arc::new(Mutex::new(SynologyConfig::new()));

    // OpenBao unseal support (optional)
    let bao_config: Arc<Option<BaoConfig>> = Arc::new(BaoConfig::from_env());
    if bao_config.is_none() {
        info!("OpenBao unseal support disabled (STB_BAO_ADDR / STB_UNSEAL_WEB_URL not fully set)");
    }
    let pending_unseal: Arc<Mutex<Option<UnsealSession>>> = Arc::new(Mutex::new(None));

    info!("Initializing bot ()...");
    let bot = Bot::new(bot_token);

    // Set the chat menu button to show commands
    info!("Setting chat menu button...");
    let menu_button = MenuButton::Commands;
    bot.set_chat_menu_button()
        .menu_button(menu_button)
        .await
        .expect("Failed to set chat menu button");

    // Register commands with Telegram to make them appear in the menu
    info!("Registering commands with Telegram...");
    bot.set_my_commands(Command::bot_commands())
        .await
        .expect("Failed to register commands");

    // Create a message handler
    let default_handler = Update::filter_message().branch(
        dptree::entry()
            .filter_command::<Command>()
            .endpoint(answer_command)
    );

    // Create a handler for all messages
    let message_handler = Update::filter_message().endpoint(message_handler);

    // Create a handler for callback queries
    let callback_handler = Update::filter_callback_query().endpoint(callback_handler);

    // Create a handler for inline queries
    let inline_query_handler = Update::filter_inline_query().endpoint(inline_query_handler);

    // Combine handlers
    let handler = dptree::entry()
        .branch(default_handler)
        .branch(message_handler)
        .branch(callback_handler)
        .branch(inline_query_handler);

    // Start the bot
    info!("Starting bot...");
    let me = bot.get_me().await.expect("Failed to get bot info");
    info!("Bot username: @{}", me.username());

    Dispatcher::builder(bot, handler)
        .dependencies(dptree::deps![synology_config, bao_config, pending_unseal])
        .enable_ctrlc_handler()
        .build()
        .dispatch()
        .await;
}

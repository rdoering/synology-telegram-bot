use std::error::Error;
use std::sync::Arc;
use teloxide::{prelude::*, utils::command::BotCommands};
use teloxide::types::{InlineKeyboardButton, InlineKeyboardMarkup, CallbackQuery, InlineQuery, InlineQueryResult, InlineQueryResultArticle, InputMessageContent, InputMessageContentText, MenuButton};
use tokio::sync::Mutex;
use log::{error, info, warn};

mod synology;
use synology::SynologyClient;

// Structure to hold the Synology client configuration
struct SynologyConfig {
    client: Option<SynologyClient>,
    nas_base_url: String,
    username: String,
    password: String,
}

// Callback data for menu buttons
const CALLBACK_LIST_FILES: &str = "list_files";
const CALLBACK_SSH_MENU: &str = "ssh_menu";
const CALLBACK_SSH_ON: &str = "ssh_on";
const CALLBACK_SSH_OFF: &str = "ssh_off";
const CALLBACK_SETTINGS: &str = "settings";
const CALLBACK_BACK: &str = "back";

impl SynologyConfig {
    fn new() -> Self {
        let nas_base_url = std::env::var("SYNOLOGY_NAS_BASE_URL").unwrap();
        let username = std::env::var("SYNOLOGY_USERNAME").unwrap_or_else(|_| {
            warn!("SYNOLOGY_USERNAME environment variable not set");
            String::new()
        });
        let password = std::env::var("SYNOLOGY_PASSWORD").unwrap_or_else(|_| {
            warn!("SYNOLOGY_PASSWORD environment variable not set");
            String::new()
        });

        info!("Initializing Synology configuration with base URL: {}", nas_base_url);

        SynologyConfig {
            client: None,
            nas_base_url,
            username,
            password,
        }
    }

    fn create_client(&mut self) {
        self.client = Some(SynologyClient::new(&self.nas_base_url, &self.username, &self.password));
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
    if let Ok(allowed_chat_id_str) = std::env::var("ALLOWED_CHAT_ID") {
        if let Ok(allowed_chat_id) = allowed_chat_id_str.parse::<i64>() {
            return chat_id == allowed_chat_id;
        }
    }
    false
}

// Function to create the main menu keyboard
fn create_main_menu() -> InlineKeyboardMarkup {
    let mut keyboard: Vec<Vec<InlineKeyboardButton>> = Vec::new();

    // List Files button
    let list_files_button = InlineKeyboardButton::callback("📁 List Files", CALLBACK_LIST_FILES);

    // SSH Control button
    let ssh_button = InlineKeyboardButton::callback("🖥️ SSH Control", CALLBACK_SSH_MENU);

    // Add buttons to keyboard
    keyboard.push(vec![list_files_button]);
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

#[derive(BotCommands, Clone)]
#[command(rename_rule = "lowercase", description = "Available commands:")]
enum Command {
    #[command(description = "Display this help message.")]
    Help,
    #[command(description = "Start the bot.")]
    Start,
    #[command(description = "Check if the bot is running.")]
    Ping,
}

// Handle commands from BotCommands enum
async fn answer_command(
    bot: Bot,
    msg: Message,
    cmd: Command
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
    match cmd {
        Command::Help => {
            let mut help_text = Command::descriptions().to_string();
            help_text.push_str("\n\nInteractive Menu:\n");
            help_text.push_str("Use /start to display the interactive menu for easier navigation.\n");
            help_text.push_str("\nAdditional commands:\n");
            help_text.push_str("/ls path - List files in a directory\n");
            help_text.push_str("/ssh [on|off] - Get SSH status or enable/disable SSH\n");
            help_text.push_str("\nConfiguration:\n");
            help_text.push_str("Synology settings must be configured via environment variables:\n");
            help_text.push_str("- SYNOLOGY_NAS_BASE_URL: Base URL of your Synology NAS (required, e.g. http://your-nas-ip:port)\n");
            help_text.push_str("- SYNOLOGY_USERNAME: Your Synology NAS username (required)\n");
            help_text.push_str("- SYNOLOGY_PASSWORD: Your Synology NAS password (required)\n");

            bot.send_message(msg.chat.id, help_text).await?;

            // Also show the main menu
            let keyboard = create_main_menu();
            bot.send_message(
                msg.chat.id,
                "You can also use the menu below:"
            )
            .reply_markup(keyboard)
            .await?;
        }
        Command::Start => {
            // Create the main menu keyboard
            let keyboard = create_main_menu();

            let chat_json = serde_json::to_string_pretty(&msg.chat).unwrap();
            info!("Chat info: {}", chat_json);

            // Send welcome message with the keyboard
            bot.send_message(
                msg.chat.id,
                format!("Welcome {} to your personal Telegram bot! Please select an option from the menu below:", msg.chat.id),
            )
            .reply_markup(keyboard)
            .await?;
        }
        Command::Ping => {
            bot.send_message(msg.chat.id, "Pong! Bot is running.".to_string()).await?;
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
            InputMessageContentText::new("Use /help to see available commands")
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

            match data.as_str() {
                // Main menu options
                CALLBACK_LIST_FILES => {
                    // Ask for path
                    bot.send_message(
                        chat_id,
                        "Please enter the path to list files using the format:\n/ls path"
                    ).await?;
                }
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
    synology_config: Arc<Mutex<SynologyConfig>>
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
    if let Some(text) = msg.text() {
        // Try to parse as a command
        if let Ok(command) = Command::parse(text, "synology_bot") {
            return answer_command(bot.clone(), msg.clone(), command).await;
        }

        // Handle custom commands

        if text.starts_with("/ls") {
            info!("received an `ls` command");
            let parts: Vec<&str> = text.split_whitespace().collect();
            if parts.len() >= 2 {
                let path = parts[1].to_string();

                let mut config = synology_config.lock().await;

                // Ensure logged in
                match config.ensure_logged_in().await {
                    Ok(true) => {
                        // Now we're logged in, proceed with listing files
                        if let Some(client) = &mut config.client {
                            match client.list_files(&path).await {
                                Ok(files) => {
                                    if files.is_empty() {
                                        bot.send_message(msg.chat.id, "No files found.").await?;
                                    } else {
                                        let mut file_list = String::from("Files:\n");
                                        for file in files {
                                            let file_type = if file.isdir { "📁" } else { "📄" };
                                            file_list.push_str(&format!("{} {}\n", file_type, file.name));
                                        }
                                        bot.send_message(msg.chat.id, file_list).await?;
                                    }
                                },
                                Err(e) => {
                                    bot.send_message(
                                        msg.chat.id, 
                                        format!("Failed to list files: {}", e)
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
            } else {
                bot.send_message(
                    msg.chat.id, 
                    "Usage: /ls path"
                ).await?;
            }
            return Ok(());
        }

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
                                        bot.send_message(
                                            msg.chat.id,
                                            "SSH service has been enabled"
                                        ).await?;
                                    },
                                    Err(e) => {
                                        bot.send_message(
                                            msg.chat.id,
                                            format!("Failed to enable SSH service: {}", e)
                                        ).await?;
                                    }
                                }
                            } else if command == "off" || command == "disable" {
                                match client.toggle_ssh(false).await {
                                    Ok(_) => {
                                        bot.send_message(
                                            msg.chat.id,
                                            "SSH service has been disabled"
                                        ).await?;
                                    },
                                    Err(e) => {
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
    // Initialize the logger
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("debug")).init();

    info!("Starting Synology Telegram Bot...");

    // Get the bot token from environment variable
    let bot_token = std::env::var("TELEGRAM_BOT_TOKEN")
        .expect("TELEGRAM_BOT_TOKEN environment variable is not set");

    // Initialize Synology configuration
    let synology_config = Arc::new(Mutex::new(SynologyConfig::new()));

    info!("Initializing bot ()...");
    let bot = Bot::new(bot_token);

    // Set the chat menu button to show commands
    info!("Setting chat menu button...");
    let menu_button = MenuButton::Commands;
    bot.set_chat_menu_button()
        .menu_button(menu_button)
        .await
        .expect("Failed to set chat menu button");

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
        .dependencies(dptree::deps![synology_config])
        .enable_ctrlc_handler()
        .build()
        .dispatch()
        .await;
}

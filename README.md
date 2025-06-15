# Synology Telegram Bot

A Telegram bot for interacting with your Synology NAS, built with Rust.

## Features

- Interactive menu interface for easy navigation
- Inline query menu in the Telegram input line
- Command menu button in the chat interface
- Basic bot commands: /start, /help, /ping
- Synology NAS integration:
  - Automatic login to your Synology NAS
  - List files in a directory
  - SSH service control (enable/disable)
  - Logout from your Synology NAS
  - Configure Synology NAS connection via environment variables

## Prerequisites

- Rust and Cargo installed
- A Telegram bot token (obtained from [@BotFather](https://t.me/BotFather))

## Setup

1. Clone this repository:
   ```
   git clone https://github.com/yourusername/synology-telegram-bot.git
   cd synology-telegram-bot
   ```

2. Set the required environment variables:
   ```
   export TELEGRAM_BOT_TOKEN=your_bot_token_here
   export SYNOLOGY_NAS_BASE_URL=http://your_synology_ip:port
   export SYNOLOGY_USERNAME=your_synology_username
   export SYNOLOGY_PASSWORD=your_synology_password
   export ALLOWED_CHAT_ID=your_telegram_chat_id
   ```
   For example:
   ```
   export SYNOLOGY_NAS_BASE_URL=http://192.168.1.100:5000
   export SYNOLOGY_USERNAME=admin
   export SYNOLOGY_PASSWORD=your_password
   export ALLOWED_CHAT_ID=123456789
   ```

3. Build and run the bot:
   ```
   cargo run
   ```

## Usage

Once the bot is running, you can interact with it using either the interactive menu or text commands.

### Interactive Menu

1. Start the bot by sending the `/start` command
2. The bot will display a menu with the following options:
   - 📁 **List Files** - List files in a directory
   - 🖥️ **SSH Control** - Enable or disable SSH service
   - 🚪 **Logout** - Logout from your Synology NAS

3. Click on any menu option to proceed with that action
   - Some options will ask you to enter additional information using text commands
   - The SSH Control option will display a submenu with options to enable or disable SSH

### Command Menu Button

The bot sets a command menu button in the chat interface, which provides easy access to all available commands:

1. Click on the menu button in the chat interface (usually located at the bottom of the chat)
2. A list of all available commands will appear
3. Select a command from the list to send it to the chat

### Inline Menu

The bot also supports inline queries, which allow you to access commands directly from the Telegram input line:

1. Type `@your_bot_username` in any chat
2. A menu will appear in the input line with available commands
3. Select a command from the menu to insert it into the chat
4. Edit the command parameters if needed and send the message

### Text Commands

You can also interact with the bot using the following text commands:

#### Basic Commands
- `/start` - Start the bot and display the interactive menu
- `/help` - Display available commands
- `/ping` - Check if the bot is running

#### Synology Commands
- `/ls path` - List files in a directory (e.g., `/ls /homes/admin/Documents`)
- `/ssh [on|off]` - Get SSH status or enable/disable SSH service
- `/logout` - Logout from your Synology NAS

## Development

### Environment Variables

- `TELEGRAM_BOT_TOKEN` - Your Telegram bot token (required)
- `SYNOLOGY_NAS_BASE_URL` - Base URL of your Synology NAS (required, e.g. http://your-nas-ip:port)
- `SYNOLOGY_USERNAME` - Your Synology NAS username (required)
- `SYNOLOGY_PASSWORD` - Your Synology NAS password (required)
- `ALLOWED_CHAT_ID` - Your Telegram chat ID that is allowed to use the bot (required)
- `FORCE_IPV4` - Set to "true" or "1" to force IPv4 connections to the Synology NAS (optional, default: false)

### Known Issues

#### IPv6 Bug in Synology DSM

There is a known issue with IPv6 connections to Synology DSM:

IPv6 sessions do not have permission to access the SYNO.Core.Terminal API, resulting in error code 105 ("The logged in session does not have permission").

If you experience these issues, set the `FORCE_IPV4` environment variable to force the bot to use IPv4 connections:

```
export FORCE_IPV4=true
```

### Adding New Features

There are four ways to extend the bot:

1. For simple commands without parameters:
   - Add a new variant to the `Command` enum in `src/main.rs`
   - Add a corresponding match arm in the `answer_command` function

2. For commands with parameters:
   - Add a new condition in the `message_handler` function that checks for your command prefix
   - Parse the parameters manually using `text.split_whitespace()`
   - Implement the command logic

3. For adding new menu options:
   - Add a new constant for the callback data in the constants section
   - Add a new button to the appropriate menu in the `create_main_menu` or `create_ssh_menu` function
   - Add a new case to the match statement in the `callback_handler` function to handle the button press
   - Implement the logic for the new menu option

4. For adding new inline query options:
   - Modify the `inline_query_handler` function in `src/main.rs`
   - Add a new `InlineQueryResultArticle` to the results vector
   - Make sure to wrap the `InputMessageContentText` in `InputMessageContent::Text`
   - Ensure the result is properly converted to `InlineQueryResult::Article`

## License

MIT

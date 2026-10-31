package bot

import (
	"context"

	tg "github.com/go-telegram/bot"
	"github.com/go-telegram/bot/models"
)

// Commands registered in the Telegram command menu.
var Commands = []models.BotCommand{
	{Command: "start", Description: "Show the main menu"},
	{Command: "ssh", Description: "Get or set SSH service: /ssh [on|off]"},
	{Command: "ssh_on", Description: "Enable SSH service"},
	{Command: "ssh_off", Description: "Disable SSH service"},
	{Command: "seal_status", Description: "Show OpenBao seal status"},
	{Command: "unseal", Description: "Unseal OpenBao via encrypted link"},
}

// Telegram implements Messenger on top of go-telegram/bot.
type Telegram struct{ B *tg.Bot }

func markup(kb Keyboard) models.ReplyMarkup {
	if kb == nil {
		return nil
	}
	rows := make([][]models.InlineKeyboardButton, len(kb))
	for i, row := range kb {
		for _, b := range row {
			rows[i] = append(rows[i], models.InlineKeyboardButton{Text: b.Text, CallbackData: b.Data})
		}
	}
	return &models.InlineKeyboardMarkup{InlineKeyboard: rows}
}

func (t Telegram) Send(ctx context.Context, chat int64, text string, kb Keyboard) error {
	_, err := t.B.SendMessage(ctx, &tg.SendMessageParams{ChatID: chat, Text: text, ReplyMarkup: markup(kb)})
	return err
}

func (t Telegram) Edit(ctx context.Context, chat int64, msgID int, text string, kb Keyboard) error {
	p := &tg.EditMessageTextParams{ChatID: chat, MessageID: msgID, Text: text}
	if m, ok := markup(kb).(*models.InlineKeyboardMarkup); ok {
		p.ReplyMarkup = m
	}
	_, err := t.B.EditMessageText(ctx, p)
	return err
}

func (t Telegram) AnswerCallback(ctx context.Context, id, text string, alert bool) error {
	_, err := t.B.AnswerCallbackQuery(ctx, &tg.AnswerCallbackQueryParams{CallbackQueryID: id, Text: text, ShowAlert: alert})
	return err
}

func (t Telegram) AnswerInline(ctx context.Context, id string) error {
	_, err := t.B.AnswerInlineQuery(ctx, &tg.AnswerInlineQueryParams{
		InlineQueryID: id,
		Results: []models.InlineQueryResult{&models.InlineQueryResultArticle{
			ID: "menu", Title: "Command Menu", Description: "Use /start to see the menu",
			InputMessageContent: &models.InputTextMessageContent{MessageText: "Use /start to see the menu"},
		}},
	})
	return err
}

func (t Telegram) Delete(ctx context.Context, chat int64, msgID int) error {
	_, err := t.B.DeleteMessage(ctx, &tg.DeleteMessageParams{ChatID: chat, MessageID: msgID})
	return err
}

// Route converts a Telegram update into a Handler call.
func (h *Handler) Route(ctx context.Context, u *models.Update) {
	switch {
	case u.CallbackQuery != nil && u.CallbackQuery.Message.Message != nil:
		m := u.CallbackQuery.Message.Message
		h.HandleCallback(ctx, Incoming{ChatID: m.Chat.ID, MessageID: m.ID, ID: u.CallbackQuery.ID, Data: u.CallbackQuery.Data})
	case u.InlineQuery != nil:
		h.HandleInline(ctx, Incoming{ID: u.InlineQuery.ID})
	case u.Message != nil:
		m := u.Message
		in := Incoming{ChatID: m.Chat.ID, MessageID: m.ID, Text: m.Text}
		if m.From != nil {
			in.FirstName = m.From.FirstName
		}
		h.HandleMessage(ctx, in)
	}
}

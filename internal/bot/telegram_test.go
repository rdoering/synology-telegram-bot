package bot

import (
	"context"
	"testing"

	"github.com/go-telegram/bot/models"
)

func TestMarkup(t *testing.T) {
	if markup(nil) != nil {
		t.Fatal("nil keyboard must stay nil")
	}
	m := markup(Keyboard{{{"A", "a"}, {"B", "b"}}, {{"C", "c"}}}).(*models.InlineKeyboardMarkup)
	if len(m.InlineKeyboard) != 2 || len(m.InlineKeyboard[0]) != 2 || m.InlineKeyboard[1][0].CallbackData != "c" {
		t.Fatalf("%+v", m)
	}
}

func TestRoute(t *testing.T) {
	h, m, s, _ := setup()
	ctx := context.Background()
	chatRef := models.Chat{ID: chat}

	h.Route(ctx, &models.Update{Message: &models.Message{ID: 1, Chat: chatRef, Text: "/ssh_on", From: &models.User{FirstName: "Rob"}}})
	if !s.on {
		t.Fatal("message not routed")
	}
	h.Route(ctx, &models.Update{Message: &models.Message{ID: 2, Chat: chatRef, Text: "/start"}}) // no sender
	if len(m.sent) != 2 {
		t.Fatal(m.sent)
	}
	h.Route(ctx, &models.Update{CallbackQuery: &models.CallbackQuery{ID: "c", Data: "back",
		Message: models.MaybeInaccessibleMessage{Message: &models.Message{ID: 3, Chat: chatRef}}}})
	if len(m.edited) != 1 {
		t.Fatal("callback not routed")
	}
	h.Route(ctx, &models.Update{InlineQuery: &models.InlineQuery{ID: "q"}})
	if m.inline != 1 {
		t.Fatal("inline not routed")
	}
	h.Route(ctx, &models.Update{}) // ignored
	h.Route(ctx, &models.Update{CallbackQuery: &models.CallbackQuery{ID: "x"}})
}

// Package bot contains the Telegram command logic, independent of the
// Telegram client library (see telegram.go for the adapter).
package bot

import (
	"context"
	"fmt"
	"log/slog"
	"strings"

	"filippo.io/age"

	"synology-telegram-bot/internal/bao"
	"synology-telegram-bot/internal/metrics"
	"synology-telegram-bot/internal/unseal"
)

// Button is one inline keyboard button; Keyboard is a list of rows.
type Button struct{ Text, Data string }
type Keyboard [][]Button

// Messenger is what the handlers need from Telegram.
type Messenger interface {
	Send(ctx context.Context, chat int64, text string, kb Keyboard) error
	Edit(ctx context.Context, chat int64, msgID int, text string, kb Keyboard) error
	AnswerCallback(ctx context.Context, id, text string, alert bool) error
	AnswerInline(ctx context.Context, id string) error
	Delete(ctx context.Context, chat int64, msgID int) error
}

// SSH controls the NAS SSH service.
type SSH interface {
	SSHEnabled(ctx context.Context) (bool, error)
	SetSSH(ctx context.Context, enable bool) error
}

// Bao is the OpenBao client; it may be nil when not configured.
type Bao interface {
	SealStatus(ctx context.Context) (bao.SealStatus, error)
	Unseal(ctx context.Context, key string) (bao.SealStatus, error)
}

// Incoming is a normalized Telegram update.
type Incoming struct {
	ChatID    int64
	MessageID int
	FirstName string
	Text      string
	// Callback and inline queries
	ID   string
	Data string
}

// Handler implements all bot behaviour.
type Handler struct {
	AllowedChat int64
	M           Messenger
	SSH         SSH
	Bao         Bao
	WebURL      string
	Sessions    *unseal.Store
}

const mainMenuText = "Please select an option from the menu below:"

var mainMenu = Keyboard{{{"🖥️ SSH Control", "ssh_menu"}}}

func (h *Handler) authorized(chat int64) bool { return chat == h.AllowedChat }

func (h *Handler) deny(ctx context.Context, in Incoming) {
	metrics.Unauthorized.Inc()
	slog.Warn("unauthorized access attempt", "chat", in.ChatID, "name", in.FirstName)
	_ = h.M.Send(ctx, in.ChatID, fmt.Sprintf("Hello %s(%d), unfortunately are not authorized to use this bot.", in.FirstName, in.ChatID), nil)
}

// HandleMessage processes text messages (commands and pending ciphertexts).
func (h *Handler) HandleMessage(ctx context.Context, in Incoming) {
	metrics.UpdatesReceived.Inc("message", authLabel(h.authorized(in.ChatID)))
	if !h.authorized(in.ChatID) {
		h.deny(ctx, in)
		return
	}
	if cmd, arg, ok := parseCommand(in.Text); ok {
		h.command(ctx, in, cmd, arg)
		return
	}
	if h.Sessions.Pending(in.ChatID) {
		h.ciphertext(ctx, in)
	}
}

func authLabel(ok bool) string {
	if ok {
		return "true"
	}
	return "false"
}

// commandLabel keeps the cmd label within the known command set.
func commandLabel(cmd string) string {
	for _, c := range metrics.CommandNames {
		if c == cmd {
			return cmd
		}
	}
	return "other"
}

func parseCommand(text string) (cmd, arg string, ok bool) {
	if !strings.HasPrefix(text, "/") {
		return "", "", false
	}
	head, arg, _ := strings.Cut(strings.TrimSpace(text), " ")
	cmd, _, _ = strings.Cut(strings.TrimPrefix(head, "/"), "@")
	return cmd, strings.ToLower(strings.TrimSpace(arg)), true
}

func (h *Handler) reply(ctx context.Context, in Incoming, text string) {
	_ = h.M.Send(ctx, in.ChatID, text, nil)
}

func (h *Handler) command(ctx context.Context, in Incoming, cmd, arg string) {
	slog.Info("command", "cmd", cmd, "chat", in.ChatID)
	metrics.Commands.Inc(commandLabel(cmd))
	switch cmd {
	case "start":
		_ = h.M.Send(ctx, in.ChatID, fmt.Sprintf("Welcome %s to your personal Telegram bot! %s", in.FirstName, mainMenuText), mainMenu)
	case "ssh":
		h.ssh(ctx, in, arg)
	case "ssh_on":
		h.ssh(ctx, in, "on")
	case "ssh_off":
		h.ssh(ctx, in, "off")
	case "seal_status":
		h.sealStatus(ctx, in)
	case "unseal":
		h.startUnseal(ctx, in)
	}
}

func sshTarget(arg string) (enable, ok bool) {
	switch arg {
	case "on", "enable":
		return true, true
	case "off", "disable":
		return false, true
	}
	return false, false
}

func onOff(b bool) string {
	if b {
		return "enabled"
	}
	return "disabled"
}

func (h *Handler) ssh(ctx context.Context, in Incoming, arg string) {
	if arg == "" {
		on, err := h.SSH.SSHEnabled(ctx)
		h.reply(ctx, in, ifErr(err, "Failed to get SSH status", fmt.Sprintf("SSH service is currently %s", onOff(on))))
		return
	}
	enable, ok := sshTarget(arg)
	if !ok {
		h.reply(ctx, in, "Usage: /ssh [on|off] - Get SSH status or enable/disable SSH")
		return
	}
	err := h.SSH.SetSSH(ctx, enable)
	h.reply(ctx, in, ifErr(err, "Failed to change SSH service", fmt.Sprintf("SSH service has been %s", onOff(enable))))
}

// ifErr returns "<prefix>: <err>" if err is set, otherwise ok.
func ifErr(err error, prefix, ok string) string {
	if err != nil {
		return fmt.Sprintf("%s: %v", prefix, err)
	}
	return ok
}

// SealText renders a seal status for humans.
func SealText(st bao.SealStatus) string {
	switch {
	case !st.Initialized:
		return "OpenBao is NOT INITIALIZED."
	case st.Sealed:
		return fmt.Sprintf("🔒 OpenBao is SEALED (progress %d/%d)", st.Progress, st.T)
	}
	return "🔓 OpenBao is unsealed."
}

func (h *Handler) baoConfigured(ctx context.Context, in Incoming) bool {
	if h.Bao == nil {
		h.reply(ctx, in, "OpenBao support is not configured (STB_BAO_* env vars missing).")
	}
	return h.Bao != nil
}

func (h *Handler) sealStatus(ctx context.Context, in Incoming) {
	if !h.baoConfigured(ctx, in) {
		return
	}
	st, err := h.Bao.SealStatus(ctx)
	h.reply(ctx, in, ifErr(err, "Failed to get seal status", SealText(st)))
}

func (h *Handler) startUnseal(ctx context.Context, in Incoming) {
	if !h.baoConfigured(ctx, in) {
		return
	}
	st, err := h.Bao.SealStatus(ctx)
	if msg := unsealBlocker(st, err); msg != "" {
		h.reply(ctx, in, msg)
		return
	}
	id, err := age.GenerateX25519Identity()
	if err != nil {
		h.reply(ctx, in, "Failed to create session key: "+err.Error())
		return
	}
	sid := bao.NewSessionID()
	h.Sessions.Start(in.ChatID, sid, id)
	link := fmt.Sprintf("%s/#s=%s&k=%s", h.WebURL, sid, id.Recipient())
	h.reply(ctx, in, "🔑 Open this link (valid for 5 minutes), paste your unseal token from Bitwarden, encrypt it, and send the ciphertext back here:\n\n"+link)
}

func unsealBlocker(st bao.SealStatus, err error) string {
	switch {
	case err != nil:
		return fmt.Sprintf("Cannot reach OpenBao: %v", err)
	case !st.Initialized:
		return "OpenBao is not initialized yet — unseal not possible."
	case !st.Sealed:
		return "OpenBao is already unsealed."
	}
	return ""
}

func (h *Handler) ciphertext(ctx context.Context, in Incoming) {
	sess, res := h.Sessions.Take(in.ChatID)
	if res == unseal.Expired {
		metrics.Unseals.Inc("expired")
		h.reply(ctx, in, "Unseal session expired. Call /unseal again for a new link.")
		return
	}
	if res == unseal.None {
		return
	}
	if err := h.M.Delete(ctx, in.ChatID, in.MessageID); err != nil {
		slog.Warn("could not delete ciphertext message", "err", err)
	}
	h.reply(ctx, in, h.finishUnseal(ctx, sess, in.Text))
}

const ageHeader = "-----BEGIN AGE ENCRYPTED FILE-----"

func (h *Handler) finishUnseal(ctx context.Context, sess unseal.Session, text string) string {
	if !strings.HasPrefix(strings.TrimSpace(text), ageHeader) {
		metrics.Unseals.Inc("invalid")
		return "That was not an age ciphertext. Please encrypt the token in the web app and send the encrypted text. Call /unseal for a new link."
	}
	key, err := bao.DecryptCiphertext(text, sess.Identity)
	if err != nil {
		metrics.Unseals.Inc("decrypt_failed")
		return fmt.Sprintf("❌ Decryption failed: %v. Call /unseal for a new link.", err)
	}
	st, err := h.Bao.Unseal(ctx, strings.TrimSpace(key))
	slog.Info("unseal attempt", "session", sess.ID, "ok", err == nil)
	switch {
	case err != nil:
		metrics.Unseals.Inc("rejected")
		return fmt.Sprintf("Unseal failed: %v", err)
	case st.Sealed:
		metrics.Unseals.Inc("partial")
		return fmt.Sprintf("⚠️ Key accepted, still sealed (progress %d/%d)", st.Progress, st.T)
	}
	metrics.Unseals.Inc("ok")
	return "🔓 OpenBao is now unsealed."
}

func sshMenu(enabled bool) (string, Keyboard) {
	toggle := Button{"✅ Enable SSH", "ssh_on"}
	if enabled {
		toggle = Button{"❌ Disable SSH", "ssh_off"}
	}
	return fmt.Sprintf("SSH Control Menu (currently %s)", onOff(enabled)), Keyboard{{toggle}, {{"🔙 Back to Main Menu", "back"}}}
}

// HandleCallback processes inline keyboard button presses.
func (h *Handler) HandleCallback(ctx context.Context, in Incoming) {
	metrics.UpdatesReceived.Inc("callback", authLabel(h.authorized(in.ChatID)))
	if !h.authorized(in.ChatID) {
		metrics.Unauthorized.Inc()
		_ = h.M.AnswerCallback(ctx, in.ID, "You are not authorized to use this bot.", true)
		return
	}
	switch in.Data {
	case "ssh_menu":
		h.menuCallback(ctx, in)
	case "ssh_on":
		h.toggleCallback(ctx, in, true)
	case "ssh_off":
		h.toggleCallback(ctx, in, false)
	case "back":
		_ = h.M.AnswerCallback(ctx, in.ID, "", false)
		_ = h.M.Edit(ctx, in.ChatID, in.MessageID, mainMenuText, mainMenu)
	default:
		_ = h.M.AnswerCallback(ctx, in.ID, "Unknown command", false)
	}
}

func (h *Handler) menuCallback(ctx context.Context, in Incoming) {
	on, err := h.SSH.SSHEnabled(ctx)
	if err != nil {
		_ = h.M.AnswerCallback(ctx, in.ID, "Failed to get SSH status: "+err.Error(), true)
		return
	}
	_ = h.M.AnswerCallback(ctx, in.ID, "", false)
	text, kb := sshMenu(on)
	_ = h.M.Edit(ctx, in.ChatID, in.MessageID, text, kb)
}

func (h *Handler) toggleCallback(ctx context.Context, in Incoming, enable bool) {
	if err := h.SSH.SetSSH(ctx, enable); err != nil {
		_ = h.M.AnswerCallback(ctx, in.ID, "Failed to change SSH service: "+err.Error(), true)
		return
	}
	msg := fmt.Sprintf("SSH service has been %s", onOff(enable))
	_ = h.M.AnswerCallback(ctx, in.ID, msg, false)
	_ = h.M.Edit(ctx, in.ChatID, in.MessageID, msg+". "+mainMenuText, mainMenu)
}

// HandleInline answers inline queries with a pointer to /start.
func (h *Handler) HandleInline(ctx context.Context, in Incoming) {
	metrics.UpdatesReceived.Inc("inline", "na")
	_ = h.M.AnswerInline(ctx, in.ID)
}

// Notify sends text to the allowed chat (used by the seal status monitor).
func (h *Handler) Notify(ctx context.Context, text string) {
	if err := h.M.Send(ctx, h.AllowedChat, text, nil); err != nil {
		slog.Warn("notify failed", "err", err)
	}
}

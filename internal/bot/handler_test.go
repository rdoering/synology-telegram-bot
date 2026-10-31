package bot

import (
	"bytes"
	"context"
	"errors"
	"strings"
	"testing"
	"time"

	"filippo.io/age"
	"filippo.io/age/armor"

	"synology-telegram-bot/internal/bao"
	"synology-telegram-bot/internal/metrics"
	"synology-telegram-bot/internal/unseal"
)

type fakeM struct {
	sent      []string
	kbs       []Keyboard
	edited    []string
	answers   []string
	alerts    []bool
	inline    int
	deleted   int
	deleteErr error
}

func (f *fakeM) Send(_ context.Context, _ int64, t string, kb Keyboard) error {
	f.sent, f.kbs = append(f.sent, t), append(f.kbs, kb)
	return nil
}
func (f *fakeM) Edit(_ context.Context, _ int64, _ int, t string, _ Keyboard) error {
	f.edited = append(f.edited, t)
	return nil
}
func (f *fakeM) AnswerCallback(_ context.Context, _, t string, a bool) error {
	f.answers, f.alerts = append(f.answers, t), append(f.alerts, a)
	return nil
}
func (f *fakeM) AnswerInline(context.Context, string) error { f.inline++; return nil }
func (f *fakeM) Delete(context.Context, int64, int) error   { f.deleted++; return f.deleteErr }

type fakeSSH struct {
	on  bool
	err error
}

func (f *fakeSSH) SSHEnabled(context.Context) (bool, error) { return f.on, f.err }
func (f *fakeSSH) SetSSH(_ context.Context, e bool) error   { f.on = e; return f.err }

type fakeBao struct {
	st        bao.SealStatus
	err       error
	unsealErr error
	key       string
}

func (f *fakeBao) SealStatus(context.Context) (bao.SealStatus, error) { return f.st, f.err }
func (f *fakeBao) Unseal(_ context.Context, k string) (bao.SealStatus, error) {
	f.key = k
	return f.st, f.unsealErr
}

const chat = 7

func setup() (*Handler, *fakeM, *fakeSSH, *fakeBao) {
	m, s, b := &fakeM{}, &fakeSSH{}, &fakeBao{st: bao.SealStatus{Initialized: true, Sealed: true, T: 3}}
	return &Handler{AllowedChat: chat, M: m, SSH: s, Bao: b, WebURL: "https://web", Sessions: unseal.NewStore(nil)}, m, s, b
}

func msg(text string) Incoming {
	return Incoming{ChatID: chat, MessageID: 5, FirstName: "Rob", Text: text}
}

func lastSent(m *fakeM) string { return m.sent[len(m.sent)-1] }

func TestUnauthorized(t *testing.T) {
	h, m, s, _ := setup()
	h.HandleMessage(context.Background(), Incoming{ChatID: 99, FirstName: "Eve", Text: "/ssh_on"})
	if s.on || !strings.Contains(lastSent(m), "not authorized") {
		t.Fatal(m.sent)
	}
	h.HandleCallback(context.Background(), Incoming{ChatID: 99, ID: "c", Data: "ssh_on"})
	if s.on || !m.alerts[0] {
		t.Fatal("callback must be rejected with alert")
	}
}

func TestStartAndSSHCommands(t *testing.T) {
	h, m, s, _ := setup()
	ctx := context.Background()
	h.HandleMessage(ctx, msg("/start@some_bot"))
	if !strings.HasPrefix(lastSent(m), "Welcome Rob") || m.kbs[0] == nil {
		t.Fatal(m.sent)
	}
	cases := []struct{ text, want string }{
		{"/ssh", "SSH service is currently disabled"},
		{"/ssh on", "SSH service has been enabled"},
		{"/ssh", "SSH service is currently enabled"},
		{"/ssh DISABLE", "SSH service has been disabled"},
		{"/ssh_on", "SSH service has been enabled"},
		{"/ssh_off", "SSH service has been disabled"},
		{"/ssh maybe", "Usage: /ssh"},
	}
	for _, c := range cases {
		h.HandleMessage(ctx, msg(c.text))
		if got := lastSent(m); !strings.Contains(got, c.want) {
			t.Fatalf("%s: %q", c.text, got)
		}
	}
	s.err = errors.New("boom")
	for _, text := range []string{"/ssh", "/ssh on"} {
		h.HandleMessage(ctx, msg(text))
		if !strings.Contains(lastSent(m), "boom") {
			t.Fatalf("%s: %q", text, lastSent(m))
		}
	}
}

func TestSealStatusCommand(t *testing.T) {
	h, m, _, b := setup()
	ctx := context.Background()
	h.HandleMessage(ctx, msg("/seal_status"))
	if lastSent(m) != "🔒 OpenBao is SEALED (progress 0/3)" {
		t.Fatal(lastSent(m))
	}
	b.err = errors.New("down")
	h.HandleMessage(ctx, msg("/seal_status"))
	if !strings.Contains(lastSent(m), "Failed to get seal status: down") {
		t.Fatal(lastSent(m))
	}
	h.Bao = nil
	h.HandleMessage(ctx, msg("/seal_status"))
	h.HandleMessage(ctx, msg("/unseal"))
	if !strings.Contains(lastSent(m), "not configured") {
		t.Fatal(lastSent(m))
	}
}

func TestSealText(t *testing.T) {
	if SealText(bao.SealStatus{}) != "OpenBao is NOT INITIALIZED." {
		t.Fatal("uninit")
	}
	if SealText(bao.SealStatus{Initialized: true}) != "🔓 OpenBao is unsealed." {
		t.Fatal("unsealed")
	}
}

func TestUnsealBlockers(t *testing.T) {
	h, m, _, b := setup()
	ctx := context.Background()
	b.err = errors.New("down")
	h.HandleMessage(ctx, msg("/unseal"))
	b.err, b.st = nil, bao.SealStatus{}
	h.HandleMessage(ctx, msg("/unseal"))
	b.st = bao.SealStatus{Initialized: true}
	h.HandleMessage(ctx, msg("/unseal"))
	want := []string{"Cannot reach OpenBao", "not initialized", "already unsealed"}
	for i, w := range want {
		if !strings.Contains(m.sent[i], w) {
			t.Fatalf("%d: %q", i, m.sent[i])
		}
	}
	if h.Sessions.Pending(chat) {
		t.Fatal("no session expected")
	}
}

// startUnseal runs /unseal and returns the session recipient from the link.
func startUnseal(t *testing.T, h *Handler, m *fakeM) string {
	h.HandleMessage(context.Background(), msg("/unseal"))
	link := lastSent(m)
	i := strings.Index(link, "&k=")
	if !strings.Contains(link, "https://web/#s=") || i < 0 {
		t.Fatalf("bad link %q", link)
	}
	return link[i+3:]
}

func encrypt(t *testing.T, recipient, plain string) string {
	r, err := age.ParseX25519Recipient(recipient)
	if err != nil {
		t.Fatal(err)
	}
	var buf bytes.Buffer
	aw := armor.NewWriter(&buf)
	w, _ := age.Encrypt(aw, r)
	_, _ = w.Write([]byte(plain))
	_ = w.Close()
	_ = aw.Close()
	return buf.String()
}

func TestUnsealFlowSuccess(t *testing.T) {
	h, m, _, b := setup()
	rcpt := startUnseal(t, h, m)
	b.st = bao.SealStatus{Initialized: true}
	h.HandleMessage(context.Background(), msg(encrypt(t, rcpt, " the-key\n")))
	if b.key != "the-key" || lastSent(m) != "🔓 OpenBao is now unsealed." || m.deleted != 1 {
		t.Fatalf("key=%q sent=%q deleted=%d", b.key, lastSent(m), m.deleted)
	}
	if h.Sessions.Pending(chat) {
		t.Fatal("session must be single use")
	}
}

func TestUnsealFlowOutcomes(t *testing.T) {
	cases := []struct {
		name    string
		mutate  func(b *fakeBao, m *fakeM)
		payload func(rcpt string, t *testing.T) string
		want    string
	}{
		{"still sealed", func(b *fakeBao, _ *fakeM) { b.st.Progress = 1 },
			func(r string, t *testing.T) string { return encrypt(t, r, "k") }, "still sealed (progress 1/3)"},
		{"unseal error", func(b *fakeBao, _ *fakeM) { b.unsealErr = errors.New("bad key") },
			func(r string, t *testing.T) string { return encrypt(t, r, "k") }, "Unseal failed: bad key"},
		{"not ciphertext", func(*fakeBao, *fakeM) {},
			func(string, *testing.T) string { return "hello" }, "not an age ciphertext"},
		{"wrong key", func(*fakeBao, *fakeM) {},
			func(string, *testing.T) string {
				other, _ := age.GenerateX25519Identity()
				return encrypt(t, other.Recipient().String(), "k")
			}, "Decryption failed"},
		{"delete fails but continues", func(b *fakeBao, m *fakeM) { m.deleteErr = errors.New("x"); b.st.Sealed = false },
			func(r string, t *testing.T) string { return encrypt(t, r, "k") }, "now unsealed"},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			h, m, _, b := setup()
			rcpt := startUnseal(t, h, m)
			c.mutate(b, m)
			h.HandleMessage(context.Background(), msg(c.payload(rcpt, t)))
			if !strings.Contains(lastSent(m), c.want) {
				t.Fatalf("got %q", lastSent(m))
			}
		})
	}
}

func TestUnsealExpiredAndIgnoredText(t *testing.T) {
	h, m, _, _ := setup()
	now := time.Unix(0, 0)
	h.Sessions = unseal.NewStore(func() time.Time { return now })
	h.HandleMessage(context.Background(), msg("random text")) // no session: ignored
	if len(m.sent) != 0 {
		t.Fatal("text without session must be ignored")
	}
	startUnseal(t, h, m)
	now = now.Add(unseal.TTL + time.Second)
	h.HandleMessage(context.Background(), msg("anything"))
	if !strings.Contains(lastSent(m), "expired") {
		t.Fatal(lastSent(m))
	}
	// ciphertext() with a vanished session does nothing
	n := len(m.sent)
	h.ciphertext(context.Background(), msg("x"))
	if len(m.sent) != n {
		t.Fatal("no reply expected")
	}
}

func TestCallbacks(t *testing.T) {
	h, m, s, _ := setup()
	ctx := context.Background()
	cb := func(data string) { h.HandleCallback(ctx, Incoming{ChatID: chat, MessageID: 1, ID: "c", Data: data}) }

	cb("ssh_menu")
	if m.edited[0] != "SSH Control Menu (currently disabled)" {
		t.Fatal(m.edited)
	}
	cb("ssh_on")
	if !s.on || !strings.HasPrefix(m.edited[1], "SSH service has been enabled. ") {
		t.Fatal(m.edited)
	}
	cb("back")
	cb("nope")
	if m.edited[2] != mainMenuText || m.answers[len(m.answers)-1] != "Unknown command" {
		t.Fatal(m.edited, m.answers)
	}
	s.on = true
	if text, kb := sshMenu(true); !strings.Contains(text, "enabled") || kb[0][0].Data != "ssh_off" {
		t.Fatal("menu for enabled ssh")
	}
	s.err = errors.New("boom")
	cb("ssh_menu")
	cb("ssh_off")
	n := len(m.alerts)
	if !m.alerts[n-1] || !m.alerts[n-2] {
		t.Fatal("errors must be shown as alerts")
	}
}

func TestInlineAndNotify(t *testing.T) {
	h, m, _, _ := setup()
	h.HandleInline(context.Background(), Incoming{ID: "q"})
	h.Notify(context.Background(), "hello")
	if m.inline != 1 || lastSent(m) != "hello" {
		t.Fatal("inline/notify")
	}
}

func TestParseCommand(t *testing.T) {
	if _, _, ok := parseCommand("hi"); ok {
		t.Fatal("not a command")
	}
	cmd, arg, ok := parseCommand("/ssh@bot  ON ")
	if !ok || cmd != "ssh" || arg != "on" {
		t.Fatal(cmd, arg)
	}
}

func TestMetricsCounted(t *testing.T) {
	h, _, _, _ := setup()
	ctx := context.Background()
	msgOK := metrics.UpdatesReceived.Value("message", "true")
	msgBad := metrics.UpdatesReceived.Value("message", "false")
	unauth := metrics.Unauthorized.Value()
	ssh := metrics.Commands.Value("ssh")
	other := metrics.Commands.Value("other")

	h.HandleMessage(ctx, msg("/ssh"))
	h.HandleMessage(ctx, msg("/nonsense"))
	h.HandleMessage(ctx, Incoming{ChatID: 99, Text: "/ssh"})
	h.HandleCallback(ctx, Incoming{ChatID: 99, ID: "c", Data: "ssh_on"})

	if metrics.UpdatesReceived.Value("message", "true") != msgOK+2 ||
		metrics.UpdatesReceived.Value("message", "false") != msgBad+1 ||
		metrics.Unauthorized.Value() != unauth+2 ||
		metrics.Commands.Value("ssh") != ssh+1 ||
		metrics.Commands.Value("other") != other+1 {
		t.Fatal("counters not updated")
	}
}

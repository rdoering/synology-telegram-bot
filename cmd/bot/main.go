// Command bot is the Synology/OpenBao Telegram bot.
package main

import (
	"context"
	"log/slog"
	"os"
	"os/signal"
	"strings"
	"syscall"
	"time"

	tg "github.com/go-telegram/bot"
	"github.com/go-telegram/bot/models"
	"github.com/joho/godotenv"

	"synology-telegram-bot/internal/bao"
	"synology-telegram-bot/internal/bot"
	"synology-telegram-bot/internal/config"
	"synology-telegram-bot/internal/metrics"
	"synology-telegram-bot/internal/synology"
	"synology-telegram-bot/internal/unseal"
	"synology-telegram-bot/internal/watch"
)

// version is set at build time via -ldflags "-X main.version=...".
var version = "dev"

func main() {
	if err := run(); err != nil {
		slog.Error("fatal", "err", err)
		os.Exit(1)
	}
}

func run() error {
	envErr := godotenv.Load() // never overrides real environment variables
	cfg, err := config.LoadEnv()
	if err != nil {
		return err
	}
	setupLogging(cfg.LogLevel)
	slog.Info("starting", "dotenv_loaded", envErr == nil, "bao", cfg.BaoEnabled())

	h := &bot.Handler{
		AllowedChat: cfg.AllowedChatID,
		SSH:         synology.New(cfg.NASBaseURL, cfg.NASUser, cfg.NASPassword, cfg.ForceIPv4),
		WebURL:      cfg.UnsealWebURL,
		Sessions:    unseal.NewStore(nil),
	}
	var bc *bao.Client
	if cfg.BaoEnabled() {
		bc = bao.NewClient(cfg.BaoAddr)
		h.Bao = bc
	}
	b, err := tg.New(cfg.Token, tg.WithDefaultHandler(func(ctx context.Context, _ *tg.Bot, u *models.Update) { h.Route(ctx, u) }))
	if err != nil {
		return err
	}
	h.M = bot.Telegram{B: b}

	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer stop()
	metrics.BuildInfo.Set(1, version)
	metrics.StartTime.Set(float64(time.Now().Unix()))
	metrics.Serve(ctx, cfg.MetricsAddr)
	registerMenu(ctx, b)
	if bc != nil {
		go watch.New(bc, h).Run(ctx, cfg.BaoPollInterval)
	}
	b.Start(ctx)
	return nil
}

func registerMenu(ctx context.Context, b *tg.Bot) {
	if _, err := b.SetMyCommands(ctx, &tg.SetMyCommandsParams{Commands: bot.Commands}); err != nil {
		slog.Warn("set commands failed", "err", err)
	}
	if _, err := b.SetChatMenuButton(ctx, &tg.SetChatMenuButtonParams{MenuButton: &models.MenuButtonCommands{Type: models.MenuButtonTypeCommands}}); err != nil {
		slog.Warn("set menu button failed", "err", err)
	}
}

func setupLogging(level string) {
	var l slog.Level
	if err := l.UnmarshalText([]byte(strings.ToLower(level))); err != nil {
		l = slog.LevelInfo
	}
	slog.SetDefault(slog.New(slog.NewTextHandler(os.Stderr, &slog.HandlerOptions{Level: l})))
}

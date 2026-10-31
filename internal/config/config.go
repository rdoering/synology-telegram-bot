// Package config reads the bot configuration from environment variables.
package config

import (
	"errors"
	"os"
	"strconv"
	"strings"
	"time"
)

// Config holds all settings of the bot.
type Config struct {
	Token           string
	NASBaseURL      string
	NASUser         string
	NASPassword     string
	AllowedChatID   int64
	ForceIPv4       bool
	LogLevel        string
	BaoAddr         string
	UnsealWebURL    string
	BaoPollInterval time.Duration
}

// BaoEnabled reports whether OpenBao support is configured.
func (c Config) BaoEnabled() bool { return c.BaoAddr != "" && c.UnsealWebURL != "" }

// Load reads the configuration using getenv (usually os.Getenv).
func Load(getenv func(string) string) (Config, error) {
	c := Config{
		Token:        getenv("STB_TELEGRAM_BOT_TOKEN"),
		NASBaseURL:   getenv("STB_SYNOLOGY_NAS_BASE_URL"),
		NASUser:      getenv("STB_SYNOLOGY_USERNAME"),
		NASPassword:  getenv("STB_SYNOLOGY_PASSWORD"),
		ForceIPv4:    parseBool(getenv("STB_FORCE_IPV4")),
		LogLevel:     firstNonEmpty(getenv("STB_LOG_LEVEL"), getenv("STB_RUST_LOG"), "info"),
		BaoAddr:      strings.TrimRight(getenv("STB_BAO_ADDR"), "/"),
		UnsealWebURL: strings.TrimRight(getenv("STB_UNSEAL_WEB_URL"), "/"),
	}
	c.AllowedChatID, _ = strconv.ParseInt(getenv("STB_ALLOWED_CHAT_ID"), 10, 64)
	c.BaoPollInterval = parseInterval(getenv("STB_BAO_POLL_INTERVAL"))
	return c, c.validate()
}

// LoadEnv is Load on the process environment.
func LoadEnv() (Config, error) { return Load(os.Getenv) }

func (c Config) validate() error {
	if c.Token == "" {
		return errors.New("STB_TELEGRAM_BOT_TOKEN is required")
	}
	if c.NASBaseURL == "" {
		return errors.New("STB_SYNOLOGY_NAS_BASE_URL is required")
	}
	return nil
}

func parseBool(v string) bool {
	return strings.EqualFold(v, "true") || v == "1"
}

func parseInterval(v string) time.Duration {
	d, err := time.ParseDuration(v)
	if err != nil || d <= 0 {
		return 10 * time.Second
	}
	return d
}

func firstNonEmpty(vals ...string) string {
	for _, v := range vals {
		if v != "" {
			return v
		}
	}
	return ""
}

package config

import (
	"testing"
	"time"
)

func env(m map[string]string) func(string) string {
	return func(k string) string { return m[k] }
}

func TestLoadDefaults(t *testing.T) {
	c, err := Load(env(map[string]string{
		"STB_TELEGRAM_BOT_TOKEN":    "tok",
		"STB_SYNOLOGY_NAS_BASE_URL": "http://nas",
		"STB_ALLOWED_CHAT_ID":       "42",
		"STB_FORCE_IPV4":            "TRUE",
		"STB_BAO_ADDR":              "http://bao/",
		"STB_UNSEAL_WEB_URL":        "https://web/",
	}))
	if err != nil {
		t.Fatal(err)
	}
	if c.AllowedChatID != 42 || !c.ForceIPv4 || c.LogLevel != "info" {
		t.Fatalf("unexpected: %+v", c)
	}
	if c.BaoAddr != "http://bao" || c.UnsealWebURL != "https://web" || !c.BaoEnabled() {
		t.Fatalf("bao not trimmed/enabled: %+v", c)
	}
	if c.BaoPollInterval != 10*time.Second {
		t.Fatalf("interval = %v", c.BaoPollInterval)
	}
}

func TestLoadRequired(t *testing.T) {
	if _, err := Load(env(nil)); err == nil {
		t.Fatal("expected token error")
	}
	if _, err := Load(env(map[string]string{"STB_TELEGRAM_BOT_TOKEN": "t"})); err == nil {
		t.Fatal("expected url error")
	}
}

func TestEmptyBaoIsDisabledAndLegacyLogVar(t *testing.T) {
	c, err := Load(env(map[string]string{
		"STB_TELEGRAM_BOT_TOKEN":    "t",
		"STB_SYNOLOGY_NAS_BASE_URL": "u",
		"STB_BAO_ADDR":              "",
		"STB_RUST_LOG":              "debug",
		"STB_BAO_POLL_INTERVAL":     "2s",
		"STB_FORCE_IPV4":            "1",
	}))
	if err != nil || c.BaoEnabled() || c.LogLevel != "debug" || c.BaoPollInterval != 2*time.Second || !c.ForceIPv4 {
		t.Fatalf("%+v %v", c, err)
	}
}

func TestParseIntervalInvalid(t *testing.T) {
	for _, v := range []string{"", "x", "-1s"} {
		if parseInterval(v) != 10*time.Second {
			t.Fatalf("%q", v)
		}
	}
}

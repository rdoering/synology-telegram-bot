// Package watch reports OpenBao seal status changes to the chat.
package watch

import (
	"context"
	"log/slog"
	"time"

	"synology-telegram-bot/internal/bao"
)

// StatusSource fetches the current seal status.
type StatusSource interface {
	SealStatus(ctx context.Context) (bao.SealStatus, error)
}

// Notifier delivers a message to the chat.
type Notifier interface {
	Notify(ctx context.Context, text string)
}

type state int

const (
	unknown state = iota
	unreachable
	notInitialized
	sealed
	unsealed
)

// Monitor remembers the last seen state and announces every change,
// including the first observation after startup.
type Monitor struct {
	src  StatusSource
	out  Notifier
	last state
}

// New creates a monitor whose state is initially unknown.
func New(src StatusSource, out Notifier) *Monitor {
	return &Monitor{src: src, out: out}
}

func classify(st bao.SealStatus) state {
	switch {
	case !st.Initialized:
		return notInitialized
	case st.Sealed:
		return sealed
	}
	return unsealed
}

var messages = map[state]string{
	unreachable:    "⚠️ OpenBao is not reachable.",
	notInitialized: "OpenBao is NOT INITIALIZED.",
	sealed:         "🔒 OpenBao is SEALED.",
	unsealed:       "🔓 OpenBao is unsealed.",
}

// Tick polls once and notifies if the state differs from the last one.
func (m *Monitor) Tick(ctx context.Context) {
	cur := unreachable
	st, err := m.src.SealStatus(ctx)
	if err == nil {
		cur = classify(st)
	} else {
		slog.Warn("seal status poll failed", "err", err)
	}
	if cur == m.last {
		return
	}
	m.last = cur
	m.out.Notify(ctx, messages[cur])
}

// Run polls every interval until ctx is cancelled; the first poll is immediate.
func (m *Monitor) Run(ctx context.Context, interval time.Duration) {
	m.Tick(ctx)
	t := time.NewTicker(interval)
	defer t.Stop()
	for {
		select {
		case <-ctx.Done():
			return
		case <-t.C:
			m.Tick(ctx)
		}
	}
}

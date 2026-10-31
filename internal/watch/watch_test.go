package watch

import (
	"context"
	"errors"
	"testing"
	"time"

	"synology-telegram-bot/internal/bao"
)

type fakeSrc struct {
	st  bao.SealStatus
	err error
}

func (f *fakeSrc) SealStatus(context.Context) (bao.SealStatus, error) { return f.st, f.err }

type fakeOut struct{ msgs []string }

func (f *fakeOut) Notify(_ context.Context, t string) { f.msgs = append(f.msgs, t) }

func TestAnnouncesFirstStatusAndChangesOnly(t *testing.T) {
	src := &fakeSrc{st: bao.SealStatus{Initialized: true, Sealed: true}}
	out := &fakeOut{}
	m := New(src, out)
	ctx := context.Background()

	m.Tick(ctx) // unknown -> sealed
	m.Tick(ctx) // unchanged
	src.st.Sealed = false
	m.Tick(ctx) // -> unsealed
	m.Tick(ctx)
	src.st.Sealed = true
	m.Tick(ctx) // unexpected re-seal

	want := []string{messages[sealed], messages[unsealed], messages[sealed]}
	if len(out.msgs) != len(want) {
		t.Fatalf("got %v", out.msgs)
	}
	for i := range want {
		if out.msgs[i] != want[i] {
			t.Fatalf("%d: %q", i, out.msgs[i])
		}
	}
}

func TestUnreachableReportedOnceAndRecovery(t *testing.T) {
	src := &fakeSrc{err: errors.New("down")}
	out := &fakeOut{}
	m := New(src, out)
	ctx := context.Background()
	m.Tick(ctx)
	m.Tick(ctx)
	src.err = nil
	src.st = bao.SealStatus{Initialized: false}
	m.Tick(ctx)
	if len(out.msgs) != 2 || out.msgs[0] != messages[unreachable] || out.msgs[1] != messages[notInitialized] {
		t.Fatalf("got %v", out.msgs)
	}
}

func TestRunPollsUntilCancelled(t *testing.T) {
	src := &fakeSrc{st: bao.SealStatus{Initialized: true}}
	out := &fakeOut{}
	ctx, cancel := context.WithCancel(context.Background())
	done := make(chan struct{})
	go func() { New(src, out).Run(ctx, time.Millisecond); close(done) }()
	time.Sleep(20 * time.Millisecond)
	cancel()
	<-done
	if len(out.msgs) != 1 {
		t.Fatalf("got %v", out.msgs)
	}
}

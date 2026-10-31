package unseal

import (
	"testing"
	"time"

	"filippo.io/age"
)

func TestStoreLifecycle(t *testing.T) {
	now := time.Unix(0, 0)
	s := NewStore(func() time.Time { return now })
	id, _ := age.GenerateX25519Identity()

	if _, r := s.Take(1); r != None || s.Pending(1) {
		t.Fatal("empty store")
	}
	s.Start(1, "abc", id)
	if !s.Pending(1) || s.Pending(2) {
		t.Fatal("pending per chat")
	}
	if _, r := s.Take(2); r != None {
		t.Fatal("other chat must not consume")
	}
	got, r := s.Take(1)
	if r != Found || got.ID != "abc" || s.Pending(1) {
		t.Fatalf("%+v %v", got, r)
	}
}

func TestStoreExpiry(t *testing.T) {
	now := time.Unix(0, 0)
	s := NewStore(func() time.Time { return now })
	id, _ := age.GenerateX25519Identity()
	s.Start(1, "abc", id)
	now = now.Add(TTL + time.Second)
	if _, r := s.Take(1); r != Expired {
		t.Fatal("expected expiry")
	}
	if s.Pending(1) {
		t.Fatal("expired session must be dropped")
	}
}

func TestDefaultClock(t *testing.T) {
	if NewStore(nil).now == nil {
		t.Fatal("clock")
	}
}

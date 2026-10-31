// Package unseal keeps the single pending link-based unseal session in memory.
package unseal

import (
	"sync"
	"time"

	"filippo.io/age"
)

// TTL is how long a session stays valid.
const TTL = 5 * time.Minute

// Session is one pending unseal request.
type Session struct {
	ChatID    int64
	ID        string
	Identity  *age.X25519Identity
	CreatedAt time.Time
}

// Result tells what Take found.
type Result int

const (
	None Result = iota
	Expired
	Found
)

// Store holds at most one session; a new one replaces the old one.
type Store struct {
	mu  sync.Mutex
	now func() time.Time
	cur *Session
}

// NewStore creates a store; now may be nil for time.Now.
func NewStore(now func() time.Time) *Store {
	if now == nil {
		now = time.Now
	}
	return &Store{now: now}
}

// Start replaces the pending session.
func (s *Store) Start(chatID int64, id string, identity *age.X25519Identity) {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.cur = &Session{ChatID: chatID, ID: id, Identity: identity, CreatedAt: s.now()}
}

// Pending reports whether chatID has a session, without consuming it.
func (s *Store) Pending(chatID int64) bool {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.cur != nil && s.cur.ChatID == chatID
}

// Take consumes the session of chatID (single use). Expired sessions are dropped.
func (s *Store) Take(chatID int64) (Session, Result) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.cur == nil || s.cur.ChatID != chatID {
		return Session{}, None
	}
	cur := *s.cur
	s.cur = nil
	if s.now().Sub(cur.CreatedAt) > TTL {
		return Session{}, Expired
	}
	return cur, Found
}

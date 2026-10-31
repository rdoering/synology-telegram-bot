// Package bao talks to OpenBao's unauthenticated sys endpoints and handles the
// age encryption used by the link-based unseal flow.
package bao

import (
	"bytes"
	"context"
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"strings"
	"time"

	"filippo.io/age"
	"filippo.io/age/armor"
)

// SealStatus is the relevant part of /v1/sys/seal-status.
type SealStatus struct {
	Initialized bool `json:"initialized"`
	Sealed      bool `json:"sealed"`
	Progress    int  `json:"progress"`
	T           int  `json:"t"`
}

// Client is an OpenBao HTTP client.
type Client struct {
	addr string
	http *http.Client
}

// NewClient creates a client for the given base address.
func NewClient(addr string) *Client {
	return &Client{addr: strings.TrimRight(addr, "/"), http: &http.Client{Timeout: 10 * time.Second}}
}

// SealStatus queries GET /v1/sys/seal-status.
func (c *Client) SealStatus(ctx context.Context) (SealStatus, error) {
	return c.do(ctx, http.MethodGet, "/v1/sys/seal-status", nil, "seal-status")
}

// Unseal submits one unseal key via PUT /v1/sys/unseal.
func (c *Client) Unseal(ctx context.Context, key string) (SealStatus, error) {
	body, _ := json.Marshal(map[string]string{"key": key})
	return c.do(ctx, http.MethodPut, "/v1/sys/unseal", body, "unseal")
}

func (c *Client) do(ctx context.Context, method, path string, body []byte, op string) (SealStatus, error) {
	var st SealStatus
	req, err := http.NewRequestWithContext(ctx, method, c.addr+path, bytes.NewReader(body))
	if err != nil {
		return st, err
	}
	resp, err := c.http.Do(req)
	if err != nil {
		return st, err
	}
	defer resp.Body.Close()
	raw, _ := io.ReadAll(resp.Body)
	if resp.StatusCode/100 != 2 {
		return st, fmt.Errorf("%s returned %d: %s", op, resp.StatusCode, apiMessage(raw))
	}
	return st, json.Unmarshal(raw, &st)
}

func apiMessage(raw []byte) string {
	var e struct {
		Errors []string `json:"errors"`
	}
	if json.Unmarshal(raw, &e) == nil && len(e.Errors) > 0 {
		return strings.Join(e.Errors, "; ")
	}
	return string(raw)
}

// NewSessionID returns 16 random bytes, hex encoded.
func NewSessionID() string {
	b := make([]byte, 16)
	_, _ = rand.Read(b)
	return hex.EncodeToString(b)
}

// DecryptCiphertext decrypts an armored age message with identity.
func DecryptCiphertext(armored string, id age.Identity) (string, error) {
	r, err := age.Decrypt(armor.NewReader(strings.NewReader(armored)), id)
	if err != nil {
		return "", err
	}
	out, err := io.ReadAll(r)
	return string(out), err
}

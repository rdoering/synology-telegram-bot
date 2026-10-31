// Package synology is a minimal DSM Web API client for the SSH terminal service.
package synology

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"net"
	"net/http"
	"net/url"
	"sync"
	"time"
)

var busy = "The network connection is unstable or the system is busy."

var errorTable = map[int]string{
	100: "Unknown error.",
	101: "No parameter of API, method or version.",
	102: "The requested API does not exist.",
	103: "The requested method does not exist.",
	104: "The requested version does not support the functionality.",
	105: "The logged in session does not have permission.",
	106: "Session timeout.",
	107: "Session interrupted by duplicated login.",
	108: "Failed to upload the file.",
	109: busy, 110: busy, 111: busy, 117: busy, 118: busy,
	114: "Lost parameters for this API.",
	115: "Not allowed to upload a file.",
	116: "Not allowed to perform for a demo site.",
	119: "Invalid session.",
	150: "Request source IP does not match the login IP.",
}

// ErrorDescription maps a DSM error code to a human readable text.
func ErrorDescription(code int) string {
	if d, ok := errorTable[code]; ok {
		return d
	}
	if code >= 112 && code <= 149 {
		return "Preserve for other purpose."
	}
	return "Unknown error code."
}

// Error is an error reported by the DSM API.
type Error struct{ Code int }

func (e *Error) Error() string {
	return fmt.Sprintf("DSM error %d: %s", e.Code, ErrorDescription(e.Code))
}

type response struct {
	Success bool            `json:"success"`
	Data    json.RawMessage `json:"data"`
	Error   *struct {
		Code int `json:"code"`
	} `json:"error"`
}

// Client talks to a single NAS. Every operation logs in and out on its own.
type Client struct {
	base, user, pass string
	http             *http.Client
	mu               sync.Mutex
}

// New builds a client; forceIPv4 restricts connections to IPv4.
func New(baseURL, user, pass string, forceIPv4 bool) *Client {
	tr := http.DefaultTransport.(*http.Transport).Clone()
	if forceIPv4 {
		d := &net.Dialer{Timeout: 10 * time.Second}
		tr.DialContext = func(ctx context.Context, _, addr string) (net.Conn, error) {
			return d.DialContext(ctx, "tcp4", addr)
		}
	}
	return &Client{base: baseURL, user: user, pass: pass, http: &http.Client{Transport: tr, Timeout: 20 * time.Second}}
}

func (c *Client) call(ctx context.Context, q url.Values) (response, error) {
	var r response
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, c.base+"/webapi/entry.cgi?"+q.Encode(), nil)
	if err != nil {
		return r, err
	}
	resp, err := c.http.Do(req)
	if err != nil {
		return r, err
	}
	defer resp.Body.Close()
	if err := json.NewDecoder(resp.Body).Decode(&r); err != nil {
		return r, err
	}
	if !r.Success {
		return r, apiError(r)
	}
	return r, nil
}

func apiError(r response) error {
	if r.Error == nil {
		return errors.New("DSM request failed without error code")
	}
	return &Error{Code: r.Error.Code}
}

func (c *Client) login(ctx context.Context) (string, error) {
	if c.user == "" || c.pass == "" {
		return "", errors.New("missing STB_SYNOLOGY_USERNAME or STB_SYNOLOGY_PASSWORD")
	}
	r, err := c.call(ctx, url.Values{"api": {"SYNO.API.Auth"}, "version": {"3"}, "method": {"login"}, "account": {c.user}, "passwd": {c.pass}})
	if err != nil {
		return "", err
	}
	var d struct {
		SID string `json:"sid"`
	}
	if err := json.Unmarshal(r.Data, &d); err != nil || d.SID == "" {
		return "", errors.New("login response contained no session id")
	}
	return d.SID, nil
}

func (c *Client) logout(ctx context.Context, sid string) {
	_, _ = c.call(ctx, url.Values{"api": {"SYNO.API.Auth"}, "version": {"3"}, "method": {"logout"}, "_sid": {sid}})
}

// session runs fn with a fresh session id and always logs out afterwards.
func (c *Client) session(ctx context.Context, fn func(sid string) error) error {
	c.mu.Lock()
	defer c.mu.Unlock()
	sid, err := c.login(ctx)
	if err != nil {
		return err
	}
	defer c.logout(ctx, sid)
	return fn(sid)
}

// SSHEnabled returns whether the SSH service is enabled.
func (c *Client) SSHEnabled(ctx context.Context) (bool, error) {
	var enabled bool
	err := c.session(ctx, func(sid string) error {
		r, err := c.call(ctx, url.Values{"api": {"SYNO.Core.Terminal"}, "version": {"1"}, "method": {"get"}, "_sid": {sid}})
		if err != nil {
			return err
		}
		enabled = parseSSH(r.Data)
		return nil
	})
	return enabled, err
}

// parseSSH accepts the several field names DSM versions are known to use.
func parseSSH(data json.RawMessage) bool {
	var d struct {
		ServiceStatus bool  `json:"service_status"`
		EnableSSH     *bool `json:"enable_ssh"`
		Enable        *bool `json:"enable"`
		Status        *bool `json:"status"`
		SSHStatus     *bool `json:"ssh_status"`
	}
	_ = json.Unmarshal(data, &d)
	for _, b := range []*bool{d.EnableSSH, d.Enable, d.Status, d.SSHStatus} {
		if b != nil && *b {
			return true
		}
	}
	return d.ServiceStatus
}

// SetSSH enables or disables the SSH service.
func (c *Client) SetSSH(ctx context.Context, enable bool) error {
	return c.session(ctx, func(sid string) error {
		_, err := c.call(ctx, url.Values{"api": {"SYNO.Core.Terminal"}, "version": {"1"}, "method": {"set"}, "enable_ssh": {fmt.Sprint(enable)}, "_sid": {sid}})
		return err
	})
}

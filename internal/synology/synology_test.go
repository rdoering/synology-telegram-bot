package synology

import (
	"context"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"synology-telegram-bot/internal/metrics"
)

// fakeDSM answers login/logout/get/set; terminal is the JSON for method=get.
func fakeDSM(t *testing.T, terminal string, setOK bool) (*Client, *[]string) {
	var calls []string
	s := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		q := r.URL.Query()
		calls = append(calls, q.Get("api")+"/"+q.Get("method"))
		switch {
		case q.Get("method") == "login" && q.Get("passwd") == "pw":
			_, _ = w.Write([]byte(`{"success":true,"data":{"sid":"SID"}}`))
		case q.Get("method") == "login":
			_, _ = w.Write([]byte(`{"success":false,"error":{"code":400}}`))
		case q.Get("_sid") != "SID":
			_, _ = w.Write([]byte(`{"success":false,"error":{"code":119}}`))
		case q.Get("method") == "get":
			_, _ = w.Write([]byte(terminal))
		case q.Get("method") == "set" && setOK:
			_, _ = w.Write([]byte(`{"success":true}`))
		case q.Get("method") == "set":
			_, _ = w.Write([]byte(`{"success":false,"error":{"code":105}}`))
		default:
			_, _ = w.Write([]byte(`{"success":true}`))
		}
	}))
	t.Cleanup(s.Close)
	return New(s.URL, "user", "pw", true), &calls
}

func TestSSHEnabledVariants(t *testing.T) {
	for body, want := range map[string]bool{
		`{"success":true,"data":{"service_status":true}}`: true,
		`{"success":true,"data":{"enable_ssh":true}}`:     true,
		`{"success":true,"data":{"ssh_status":true}}`:     true,
		`{"success":true,"data":{"enable":false}}`:        false,
		`{"success":true,"data":{}}`:                      false,
	} {
		c, calls := fakeDSM(t, body, true)
		got, err := c.SSHEnabled(context.Background())
		if err != nil || got != want {
			t.Fatalf("%s: got %v %v", body, got, err)
		}
		if len(*calls) != 3 || (*calls)[2] != "SYNO.API.Auth/logout" {
			t.Fatalf("calls: %v", *calls)
		}
	}
}

func TestSetSSH(t *testing.T) {
	c, _ := fakeDSM(t, "", true)
	if err := c.SetSSH(context.Background(), true); err != nil {
		t.Fatal(err)
	}
	c, calls := fakeDSM(t, "", false)
	err := c.SetSSH(context.Background(), false)
	if err == nil || !strings.Contains(err.Error(), "105") {
		t.Fatalf("%v", err)
	}
	if (*calls)[len(*calls)-1] != "SYNO.API.Auth/logout" {
		t.Fatal("must log out after failure")
	}
}

func TestLoginFailures(t *testing.T) {
	c, _ := fakeDSM(t, "", true)
	c.pass = "wrong"
	if _, err := c.SSHEnabled(context.Background()); err == nil {
		t.Fatal("expected login error")
	}
	c.user = ""
	if err := c.SetSSH(context.Background(), true); err == nil {
		t.Fatal("expected missing credentials")
	}
}

func TestTransportErrors(t *testing.T) {
	c := New("http://127.0.0.1:1", "u", "p", false)
	if _, err := c.SSHEnabled(context.Background()); err == nil {
		t.Fatal("expected conn error")
	}
	c = New("http://x\n", "u", "p", false)
	if _, err := c.SSHEnabled(context.Background()); err == nil {
		t.Fatal("expected request error")
	}
	bad := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) { _, _ = w.Write([]byte("<html>")) }))
	defer bad.Close()
	if _, err := New(bad.URL, "u", "p", false).SSHEnabled(context.Background()); err == nil {
		t.Fatal("expected decode error")
	}
}

func TestLoginWithoutSID(t *testing.T) {
	s := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		_, _ = w.Write([]byte(`{"success":true,"data":{}}`))
	}))
	defer s.Close()
	if _, err := New(s.URL, "u", "p", false).SSHEnabled(context.Background()); err == nil {
		t.Fatal("expected sid error")
	}
}

func TestErrorDescriptions(t *testing.T) {
	cases := map[int]string{
		100: "Unknown error.", 109: busy, 130: "Preserve for other purpose.",
		150: "Request source IP does not match the login IP.", 999: "Unknown error code.",
	}
	for code, want := range cases {
		if got := ErrorDescription(code); got != want {
			t.Fatalf("%d: %q", code, got)
		}
	}
	if !strings.Contains((&Error{Code: 106}).Error(), "Session timeout") {
		t.Fatal("error text")
	}
	if apiError(response{}).Error() == "" {
		t.Fatal("error without code")
	}
}

func TestMetricsRecorded(t *testing.T) {
	okBefore := metrics.SynologyRequests.Value("ssh_status", "ok")
	errBefore := metrics.SynologyRequests.Value("ssh_set", "error")
	code105 := metrics.SynologyErrors.Value("105")

	c, _ := fakeDSM(t, `{"success":true,"data":{}}`, false)
	if _, err := c.SSHEnabled(context.Background()); err != nil {
		t.Fatal(err)
	}
	if err := c.SetSSH(context.Background(), true); err == nil {
		t.Fatal("expected DSM error 105")
	}
	if metrics.SynologyRequests.Value("ssh_status", "ok") != okBefore+1 ||
		metrics.SynologyRequests.Value("ssh_set", "error") != errBefore+1 ||
		metrics.SynologyErrors.Value("105") != code105+1 {
		t.Fatal("counters not updated")
	}
	if metrics.SynologyLastSuccess.Value() == 0 {
		t.Fatal("last success not set")
	}

	other := metrics.SynologyErrors.Value("other")
	bad := New("http://127.0.0.1:1", "u", "pw", true)
	if _, err := bad.SSHEnabled(context.Background()); err == nil {
		t.Fatal("expected network error")
	}
	if metrics.SynologyErrors.Value("other") != other+1 {
		t.Fatal("non-DSM error not counted as other")
	}
}

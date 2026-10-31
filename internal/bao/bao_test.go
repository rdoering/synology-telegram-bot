package bao

import (
	"bytes"
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"os"
	"regexp"
	"testing"

	"filippo.io/age"
	"filippo.io/age/armor"
)

func TestAgeRoundtrip(t *testing.T) {
	id, _ := age.GenerateX25519Identity()
	var buf bytes.Buffer
	aw := armor.NewWriter(&buf)
	w, _ := age.Encrypt(aw, id.Recipient())
	_, _ = w.Write([]byte("secret"))
	_ = w.Close()
	_ = aw.Close()
	got, err := DecryptCiphertext(buf.String(), id)
	if err != nil || got != "secret" {
		t.Fatalf("%q %v", got, err)
	}
}

func TestDecryptsJSProducedCiphertext(t *testing.T) {
	raw, err := os.ReadFile("testdata/js-ciphertext.json")
	if err != nil {
		t.Fatal(err)
	}
	var fx struct{ Identity, Recipient, Ciphertext string }
	if err := json.Unmarshal(raw, &fx); err != nil {
		t.Fatal(err)
	}
	id, err := age.ParseX25519Identity(fx.Identity)
	if err != nil {
		t.Fatal(err)
	}
	got, err := DecryptCiphertext(fx.Ciphertext, id)
	if err != nil || got != "test-secret-123" {
		t.Fatalf("%q %v", got, err)
	}
}

func TestDecryptGarbage(t *testing.T) {
	id, _ := age.GenerateX25519Identity()
	if _, err := DecryptCiphertext("nope", id); err == nil {
		t.Fatal("expected error")
	}
}

func TestSessionID(t *testing.T) {
	if !regexp.MustCompile(`^[0-9a-f]{32}$`).MatchString(NewSessionID()) {
		t.Fatal("bad id")
	}
}

func server(t *testing.T, status int, body string) *Client {
	s := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(status)
		_, _ = w.Write([]byte(body))
	}))
	t.Cleanup(s.Close)
	return NewClient(s.URL + "/")
}

func TestSealStatus(t *testing.T) {
	c := server(t, 200, `{"initialized":true,"sealed":true,"progress":1,"t":3}`)
	st, err := c.SealStatus(context.Background())
	if err != nil || !st.Sealed || st.Progress != 1 || st.T != 3 {
		t.Fatalf("%+v %v", st, err)
	}
}

func TestErrors(t *testing.T) {
	if _, err := server(t, 503, "down").SealStatus(context.Background()); err == nil {
		t.Fatal("expected status error")
	}
	_, err := server(t, 400, `{"errors":["bad key","x"]}`).Unseal(context.Background(), "k")
	if err == nil || err.Error() != "unseal returned 400: bad key; x" {
		t.Fatalf("%v", err)
	}
	if _, err := server(t, 200, "{").SealStatus(context.Background()); err == nil {
		t.Fatal("expected json error")
	}
	if _, err := NewClient("http://127.0.0.1:1").SealStatus(context.Background()); err == nil {
		t.Fatal("expected conn error")
	}
	if _, err := NewClient("http://x\n").SealStatus(context.Background()); err == nil {
		t.Fatal("expected request error")
	}
}

func TestUnsealOK(t *testing.T) {
	c := server(t, 200, `{"initialized":true,"sealed":false}`)
	st, err := c.Unseal(context.Background(), "k")
	if err != nil || st.Sealed {
		t.Fatalf("%+v %v", st, err)
	}
}

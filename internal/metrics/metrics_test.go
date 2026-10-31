package metrics

import (
	"net/http/httptest"
	"strings"
	"testing"
)

func render(r *Registry) string {
	var b strings.Builder
	r.Write(&b)
	return b.String()
}

func TestCounterGaugeSummary(t *testing.T) {
	r := &Registry{}
	c := r.NewCounter("c_total", "a counter", "k")
	g := r.NewGauge("g", "a gauge")
	s := r.NewSummary("s_seconds", "a summary")
	c.Inc("a")
	c.Add(2, "a")
	c.Init("b")
	g.Set(7)
	s.Observe(1.5)
	s.Observe(0.5)

	out := render(r)
	for _, want := range []string{
		"# TYPE c_total counter",
		`c_total{k="a"} 3`,
		`c_total{k="b"} 0`,
		"g 7",
		"s_seconds_sum 2",
		"s_seconds_count 2",
	} {
		if !strings.Contains(out, want) {
			t.Errorf("output missing %q:\n%s", want, out)
		}
	}
}

func TestLabelEscapingAndArity(t *testing.T) {
	r := &Registry{}
	c := r.NewCounter("c", "h", "k")
	c.Inc(`a"b\c`)
	if out := render(r); !strings.Contains(out, `c{k="a\"b\\c"} 1`) {
		t.Errorf("label not escaped:\n%s", out)
	}
	defer func() {
		if recover() == nil {
			t.Error("expected panic on wrong label count")
		}
	}()
	c.Inc("x", "y")
}

func TestHandler(t *testing.T) {
	r := &Registry{}
	r.NewGauge("up_test", "h").Set(1)
	for path, want := range map[string]string{"/metrics": "up_test 1", "/healthz": "ok"} {
		rec := httptest.NewRecorder()
		r.Handler().ServeHTTP(rec, httptest.NewRequest("GET", path, nil))
		if rec.Code != 200 || !strings.Contains(rec.Body.String(), want) {
			t.Errorf("%s: code %d body %q", path, rec.Code, rec.Body.String())
		}
	}
}

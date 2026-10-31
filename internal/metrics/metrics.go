// Package metrics is a tiny Prometheus text-format exporter (stdlib only).
//
// Metrics are declared once at package level (NewCounter, NewGauge,
// NewSummary) and registered on Default; Serve exposes them on /metrics.
package metrics

import (
	"context"
	"errors"
	"fmt"
	"io"
	"log/slog"
	"net/http"
	"sort"
	"strings"
	"sync"
	"time"
)

// Registry holds metric families and renders them in exposition format.
type Registry struct {
	mu       sync.Mutex
	families []*family
}

// Default is the registry used by the package-level constructors.
var Default = &Registry{}

type kind string

const (
	counterKind kind = "counter"
	gaugeKind   kind = "gauge"
	summaryKind kind = "summary"
)

type family struct {
	name, help string
	kind       kind
	labels     []string
	mu         sync.Mutex
	vals       map[string]float64 // key: joined label values
	sums       map[string]float64 // summary only
}

// Counter is a monotonically increasing metric.
type Counter struct{ f *family }

// Gauge is a metric that can go up and down.
type Gauge struct{ f *family }

// Summary tracks <name>_sum and <name>_count.
type Summary struct{ f *family }

func (r *Registry) add(k kind, name, help string, labels []string) *family {
	f := &family{name: name, help: help, kind: k, labels: labels, vals: map[string]float64{}, sums: map[string]float64{}}
	r.mu.Lock()
	r.families = append(r.families, f)
	r.mu.Unlock()
	return f
}

// NewCounter registers a counter on r.
func (r *Registry) NewCounter(name, help string, labels ...string) *Counter {
	return &Counter{r.add(counterKind, name, help, labels)}
}

// NewGauge registers a gauge on r.
func (r *Registry) NewGauge(name, help string, labels ...string) *Gauge {
	return &Gauge{r.add(gaugeKind, name, help, labels)}
}

// NewSummary registers a summary (sum + count only) on r.
func (r *Registry) NewSummary(name, help string, labels ...string) *Summary {
	return &Summary{r.add(summaryKind, name, help, labels)}
}

// NewCounter registers a counter on Default.
func NewCounter(name, help string, labels ...string) *Counter {
	return Default.NewCounter(name, help, labels...)
}

// NewGauge registers a gauge on Default.
func NewGauge(name, help string, labels ...string) *Gauge {
	return Default.NewGauge(name, help, labels...)
}

// NewSummary registers a summary on Default.
func NewSummary(name, help string, labels ...string) *Summary {
	return Default.NewSummary(name, help, labels...)
}

func (f *family) key(vals []string) string {
	if len(vals) != len(f.labels) {
		panic(fmt.Sprintf("metrics: %s expects %d label values, got %d", f.name, len(f.labels), len(vals)))
	}
	return strings.Join(vals, "\xff")
}

// Init makes the series visible with value 0 before the first event.
func (c *Counter) Init(vals ...string) { c.Add(0, vals...) }

// Inc adds 1.
func (c *Counter) Inc(vals ...string) { c.Add(1, vals...) }

// Add adds v to the series identified by the label values.
func (c *Counter) Add(v float64, vals ...string) {
	c.f.mu.Lock()
	c.f.vals[c.f.key(vals)] += v
	c.f.mu.Unlock()
}

// Set sets the series to v.
func (g *Gauge) Set(v float64, vals ...string) {
	g.f.mu.Lock()
	g.f.vals[g.f.key(vals)] = v
	g.f.mu.Unlock()
}

// Observe records one observation of v.
func (s *Summary) Observe(v float64, vals ...string) {
	k := s.f.key(vals)
	s.f.mu.Lock()
	s.f.sums[k] += v
	s.f.vals[k]++
	s.f.mu.Unlock()
}

func escape(v string) string {
	return strings.NewReplacer(`\`, `\\`, `"`, `\"`, "\n", `\n`).Replace(v)
}

func (f *family) series(suffix, key string, v float64, w io.Writer) {
	var parts []string
	if len(f.labels) > 0 {
		for i, val := range strings.Split(key, "\xff") {
			parts = append(parts, fmt.Sprintf(`%s="%s"`, f.labels[i], escape(val)))
		}
	}
	lbl := ""
	if len(parts) > 0 {
		lbl = "{" + strings.Join(parts, ",") + "}"
	}
	fmt.Fprintf(w, "%s%s%s %g\n", f.name, suffix, lbl, v)
}

func (f *family) write(w io.Writer) {
	f.mu.Lock()
	defer f.mu.Unlock()
	fmt.Fprintf(w, "# HELP %s %s\n# TYPE %s %s\n", f.name, f.help, f.name, f.kind)
	keys := make([]string, 0, len(f.vals))
	for k := range f.vals {
		keys = append(keys, k)
	}
	sort.Strings(keys)
	for _, k := range keys {
		if f.kind == summaryKind {
			f.series("_sum", k, f.sums[k], w)
			f.series("_count", k, f.vals[k], w)
			continue
		}
		f.series("", k, f.vals[k], w)
	}
}

// Write renders all families in Prometheus text format.
func (r *Registry) Write(w io.Writer) {
	r.mu.Lock()
	fams := append([]*family(nil), r.families...)
	r.mu.Unlock()
	for _, f := range fams {
		f.write(w)
	}
}

// Handler serves /metrics and /healthz for r.
func (r *Registry) Handler() http.Handler {
	mux := http.NewServeMux()
	mux.HandleFunc("/metrics", func(w http.ResponseWriter, _ *http.Request) {
		w.Header().Set("Content-Type", "text/plain; version=0.0.4")
		r.Write(w)
	})
	mux.HandleFunc("/healthz", func(w http.ResponseWriter, _ *http.Request) { _, _ = io.WriteString(w, "ok\n") })
	return mux
}

// Serve exposes Default on addr until ctx is cancelled. An empty addr disables it.
func Serve(ctx context.Context, addr string) {
	if addr == "" {
		return
	}
	srv := &http.Server{Addr: addr, Handler: Default.Handler(), ReadHeaderTimeout: 5 * time.Second}
	go func() {
		<-ctx.Done()
		shutdown, cancel := context.WithTimeout(context.Background(), 2*time.Second)
		defer cancel()
		_ = srv.Shutdown(shutdown)
	}()
	go func() {
		if err := srv.ListenAndServe(); err != nil && !errors.Is(err, http.ErrServerClosed) {
			slog.Error("metrics server failed", "addr", addr, "err", err)
		}
	}()
	slog.Info("metrics listening", "addr", addr)
}

func (f *family) value(vals []string) float64 {
	f.mu.Lock()
	defer f.mu.Unlock()
	return f.vals[f.key(vals)]
}

// Value returns the current value of a series (0 if unset); mainly for tests.
func (c *Counter) Value(vals ...string) float64 { return c.f.value(vals) }

// Value returns the current value of a series (0 if unset); mainly for tests.
func (g *Gauge) Value(vals ...string) float64 { return g.f.value(vals) }

// Command crap computes the CRAP score (comp^2 * (1-cov)^3 + comp) per
// function from a Go cover profile and fails if any function reaches the limit.
//
//	go run ./tools/crap -profile cover.out -max 30
package main

import (
	"bufio"
	"flag"
	"fmt"
	"go/ast"
	"go/parser"
	"go/token"
	"math"
	"os"
	"path/filepath"
	"sort"
	"strings"
)

type block struct{ startLine, startCol, endLine, endCol, stmts, count int }

type fn struct {
	name       string
	comp       int
	start, end int
	total, hit int
}

func (f fn) crap() float64 {
	cov := 1.0
	if f.total > 0 {
		cov = float64(f.hit) / float64(f.total)
	}
	c := float64(f.comp)
	return c*c*math.Pow(1-cov, 3) + c
}

func main() {
	profile := flag.String("profile", "cover.out", "cover profile")
	limit := flag.Float64("max", 30, "fail when a function's CRAP score is >= max")
	module := flag.String("module", "synology-telegram-bot", "module path prefix")
	skip := flag.String("skip", "cmd/", "comma separated path prefixes to ignore")
	flag.Parse()

	blocks, err := readProfile(*profile, *module)
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(2)
	}
	var all []fn
	for file, bs := range blocks {
		if skipped(file, *skip) || strings.HasSuffix(file, "_test.go") {
			continue
		}
		fns, err := analyze(file, bs)
		if err != nil {
			fmt.Fprintln(os.Stderr, err)
			os.Exit(2)
		}
		all = append(all, fns...)
	}
	sort.Slice(all, func(i, j int) bool { return all[i].crap() > all[j].crap() })
	bad := 0
	for i, f := range all {
		if f.crap() >= *limit {
			bad++
		}
		if i < 10 || f.crap() >= *limit {
			fmt.Printf("%7.1f  comp=%-2d  %s\n", f.crap(), f.comp, f.name)
		}
	}
	if bad > 0 {
		fmt.Printf("FAIL: %d function(s) with CRAP >= %.0f\n", bad, *limit)
		os.Exit(1)
	}
	fmt.Printf("OK: %d functions, all CRAP < %.0f\n", len(all), *limit)
}

func skipped(file, prefixes string) bool {
	for _, p := range strings.Split(prefixes, ",") {
		if p != "" && strings.HasPrefix(file, p) {
			return true
		}
	}
	return false
}

func readProfile(path, module string) (map[string][]block, error) {
	f, err := os.Open(path)
	if err != nil {
		return nil, err
	}
	defer f.Close()
	out := map[string][]block{}
	sc := bufio.NewScanner(f)
	for sc.Scan() {
		line := sc.Text()
		if strings.HasPrefix(line, "mode:") {
			continue
		}
		var file string
		var b block
		i := strings.LastIndex(line, ":")
		file = strings.TrimPrefix(line[:i], module+"/")
		if _, err := fmt.Sscanf(line[i+1:], "%d.%d,%d.%d %d %d", &b.startLine, &b.startCol, &b.endLine, &b.endCol, &b.stmts, &b.count); err != nil {
			return nil, fmt.Errorf("bad profile line %q: %w", line, err)
		}
		out[filepath.ToSlash(file)] = append(out[filepath.ToSlash(file)], b)
	}
	return out, sc.Err()
}

func analyze(file string, bs []block) ([]fn, error) {
	fset := token.NewFileSet()
	af, err := parser.ParseFile(fset, file, nil, 0)
	if err != nil {
		return nil, err
	}
	var fns []fn
	for _, d := range af.Decls {
		fd, ok := d.(*ast.FuncDecl)
		if !ok || fd.Body == nil {
			continue
		}
		f := fn{name: file + ":" + fd.Name.Name, comp: complexity(fd),
			start: fset.Position(fd.Pos()).Line, end: fset.Position(fd.End()).Line}
		for _, b := range bs {
			if b.startLine >= f.start && b.endLine <= f.end {
				f.total += b.stmts
				if b.count > 0 {
					f.hit += b.stmts
				}
			}
		}
		fns = append(fns, f)
	}
	return fns, nil
}

// complexity is the cyclomatic complexity: 1 + branching constructs.
func complexity(fd *ast.FuncDecl) int {
	c := 1
	ast.Inspect(fd, func(n ast.Node) bool {
		switch x := n.(type) {
		case *ast.IfStmt, *ast.ForStmt, *ast.RangeStmt:
			c++
		case *ast.CaseClause:
			if x.List != nil {
				c++
			}
		case *ast.CommClause:
			if x.Comm != nil {
				c++
			}
		case *ast.BinaryExpr:
			if x.Op == token.LAND || x.Op == token.LOR {
				c++
			}
		}
		return true
	})
	return c
}

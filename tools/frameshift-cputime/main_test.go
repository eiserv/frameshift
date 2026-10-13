package main

import (
	"bytes"
	"encoding/json"
	"reflect"
	"strings"
	"testing"
	"time"
)

func TestParseArgsAcceptsOptionsBeforeTheProgram(t *testing.T) {
	opts, err := parseArgs([]string{"--interval", "15", "--log", "run.jsonl", "--", "fuzz", "--", "x"})
	if err != nil {
		t.Fatal(err)
	}
	if opts.interval != 15*time.Second || opts.log != "run.jsonl" {
		t.Fatalf("options %+v", opts)
	}
	if !reflect.DeepEqual(opts.command, []string{"fuzz", "--", "x"}) {
		t.Fatalf("command %q", opts.command)
	}
	opts, err = parseArgs([]string{"--", "fuzz"})
	if err != nil || opts.interval != 5*time.Second || opts.log != "" {
		t.Fatalf("defaults %+v %v", opts, err)
	}
}

func TestParseArgsRejectsInvalidInput(t *testing.T) {
	for _, args := range [][]string{
		{},
		{"fuzz"},
		{"--"},
		{"--interval"},
		{"--interval", "0", "--", "fuzz"},
		{"--interval", "-1", "--", "fuzz"},
		{"--interval", "1.5", "--", "fuzz"},
		{"--log", "", "--", "fuzz"},
		{"--seconds", "1", "--", "fuzz"},
	} {
		if _, err := parseArgs(args); err == nil {
			t.Errorf("accepted %q", args)
		}
	}
	var stderr bytes.Buffer
	if code := run([]string{"--unknown"}, &stderr); code != exitHelper {
		t.Fatalf("exit code %d", code)
	}
	if !strings.Contains(stderr.String(), "usage:") {
		t.Fatalf("stderr %q", stderr.String())
	}
}

func TestRecordsHaveAFixedVersionedLayout(t *testing.T) {
	var out bytes.Buffer
	log := &logWriter{encoder: json.NewEncoder(&out)}
	log.write(headerRecord{
		Record:     "header",
		Format:     logFormat,
		Version:    logVersion,
		Platform:   "windows/amd64",
		IntervalMS: 5000,
		Command:    []string{"fuzz", "run"},
	})
	for _, record := range processRecords(1500*time.Millisecond, []*process{{
		pid:    7,
		parent: 3,
		name:   "fuzz.exe",
		user:   1234567 * time.Microsecond,
		kernel: 89 * time.Millisecond,
		exact:  true,
	}}) {
		log.write(record)
	}
	want := `{"record":"header","format":"frameshift-cputime","version":1,"platform":"windows/amd64","interval_ms":5000,"command":["fuzz","run"]}
{"record":"sample","elapsed_ms":1500,"pid":7,"parent":3,"name":"fuzz.exe","user_ms":1234,"kernel_ms":89,"exact":true}
`
	if out.String() != want || log.err != nil {
		t.Fatalf("records %q, error %v", out.String(), log.err)
	}
	var summary bytes.Buffer
	summarize(&summary, "fuzz", endRecord{ElapsedMS: 61002, ExitCode: 1, Processes: processRecords(0, []*process{{
		pid: 7, parent: 3, name: "fuzz.exe", user: 3 * time.Second, kernel: 5 * time.Millisecond,
	}})})
	wantSummary := "frameshift-cputime: fuzz exited with code 1 after 61.002 s\n" +
		"frameshift-cputime: pid 7 (parent 3) fuzz.exe: 3.005 s processor time (user 3.000 s, kernel 0.005 s), last observed\n"
	if summary.String() != wantSummary {
		t.Fatalf("summary %q", summary.String())
	}
}

func TestParseProcStatReadsFieldsAfterTheLastParenthesis(t *testing.T) {
	line := "4242 (fuzz (worker) x) R 4241 4242 4200 0 -1 4194304 100 0 0 0 31337 271 0 0 20 0 1 0 987654 1000 100 18446744073709551615\n"
	stat, err := parseProcStat(line)
	if err != nil {
		t.Fatal(err)
	}
	want := procStat{pid: 4242, name: "fuzz (worker) x", state: 'R', parent: 4241, user: 31337, kernel: 271, start: 987654}
	if stat != want {
		t.Fatalf("stat %+v", stat)
	}
	for _, line := range []string{
		"",
		"4242 fuzz R 1",
		"(fuzz) R 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20",
		"4242 (fuzz) R 1 2 3",
		"4242 (fuzz) R x 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20",
		"4242 (fuzz) RR 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20",
		"x42 (fuzz) R 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20",
	} {
		if _, err := parseProcStat(line); err == nil {
			t.Errorf("accepted %q", line)
		}
	}
}

func TestAdoptFollowsDescendantsAndSkipsReusedIDs(t *testing.T) {
	starts := map[uint32]uint64{10: 100, 11: 150, 12: 200, 13: 50, 20: 300, 30: 400, 31: 450}
	startOf := func(row entry) (uint64, bool) {
		start, ok := starts[row.pid]
		return start, ok
	}
	tracked := map[uint32]uint64{10: 100}
	table := []entry{
		{pid: 12, parent: 11, name: "grandchild"},
		{pid: 11, parent: 10, name: "child"},
		{pid: 13, parent: 10, name: "older process with a reused parent ID"},
		{pid: 14, parent: 10, name: "ended before its start time was read"},
		{pid: 20, parent: 1, name: "unrelated"},
		{pid: 31, parent: 30, name: "unrelated descendant"},
		{pid: 10, parent: 1, name: "root"},
	}
	var names []string
	for _, found := range adopt(tracked, table, startOf) {
		names = append(names, found.name)
	}
	if !reflect.DeepEqual(names, []string{"child", "grandchild"}) {
		t.Fatalf("adopted %q", names)
	}
	if len(tracked) != 3 || tracked[11] != 150 || tracked[12] != 200 {
		t.Fatalf("tracked %v", tracked)
	}
	if again := adopt(tracked, table, startOf); len(again) != 0 {
		t.Fatalf("adopted again %v", again)
	}
}

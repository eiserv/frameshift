package main

import (
	"bufio"
	"encoding/json"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
	"testing"
	"time"
)

// helperRole selects what the test binary does when it runs as the measured
// program. The variable exists only between this test and its children.
const helperRole = "CPUTIME_TEST_ROLE"

// burn keeps one core busy for the given wall-clock time.
func burn(duration time.Duration) {
	deadline := time.Now().Add(duration)
	value := uint64(1)
	for time.Now().Before(deadline) {
		for range 10_000 {
			value = value*6364136223846793005 + 1442695040888963407
		}
	}
	if value == 0 {
		os.Exit(99)
	}
}

func TestMain(m *testing.M) {
	switch os.Getenv(helperRole) {
	case "":
		os.Exit(m.Run())
	case "parent":
		// Starts a busy child, keeps busy itself, and exits with code 3.
		child := exec.Command(os.Args[0])
		child.Env = append(os.Environ(), helperRole+"=child")
		if err := child.Start(); err != nil {
			os.Exit(98)
		}
		burn(700 * time.Millisecond)
		if err := child.Wait(); err != nil {
			os.Exit(97)
		}
		os.Exit(3)
	case "child":
		burn(700 * time.Millisecond)
		os.Exit(0)
	default:
		os.Exit(96)
	}
}

func TestMeasuresTheProgramAndItsDescendants(t *testing.T) {
	if runtime.GOOS != "windows" && !(runtime.GOOS == "linux" && (runtime.GOARCH == "amd64" || runtime.GOARCH == "arm64")) {
		t.Skip("processor time sampling is not supported here")
	}
	t.Setenv(helperRole, "parent")
	log := filepath.Join(t.TempDir(), "run.jsonl")
	var stderr strings.Builder
	code := measure(options{
		interval: 50 * time.Millisecond,
		log:      log,
		command:  []string{os.Args[0]},
	}, strings.NewReader(""), io.Discard, &stderr)
	if code != 3 {
		t.Fatalf("exit code %d, stderr %s", code, stderr.String())
	}
	file, err := os.Open(log)
	if err != nil {
		t.Fatal(err)
	}
	defer file.Close()
	var header headerRecord
	var end endRecord
	samples := 0
	scanner := bufio.NewScanner(file)
	for scanner.Scan() {
		var kind struct{ Record string }
		if err := json.Unmarshal(scanner.Bytes(), &kind); err != nil {
			t.Fatal(err)
		}
		switch kind.Record {
		case "header":
			err = json.Unmarshal(scanner.Bytes(), &header)
		case "sample":
			samples++
		case "end":
			err = json.Unmarshal(scanner.Bytes(), &end)
		default:
			t.Fatalf("record %q", kind.Record)
		}
		if err != nil {
			t.Fatal(err)
		}
	}
	if header.Format != logFormat || header.Version != logVersion || samples == 0 {
		t.Fatalf("header %+v, %d samples", header, samples)
	}
	if end.ExitCode != 3 || !end.Complete || len(end.Processes) != 2 {
		t.Fatalf("end %+v", end)
	}
	parent, child := end.Processes[0], end.Processes[1]
	if child.Parent != parent.PID {
		t.Fatalf("processes %+v", end.Processes)
	}
	for _, p := range end.Processes {
		// Each process was busy for 700 ms of wall-clock time; a loaded
		// machine may give it less processor time, but not almost none.
		if p.UserMS+p.KernelMS < 200 {
			t.Errorf("process %+v measured too little processor time", p)
		}
	}
	if runtime.GOOS == "windows" && (!parent.Exact || !child.Exact) {
		t.Errorf("processes %+v are not final", end.Processes)
	}
	if !strings.Contains(stderr.String(), "exited with code 3") {
		t.Fatalf("stderr %s", stderr.String())
	}
}

func TestRefusesToOverwriteALog(t *testing.T) {
	log := filepath.Join(t.TempDir(), "run.jsonl")
	if err := os.WriteFile(log, []byte("kept"), 0o644); err != nil {
		t.Fatal(err)
	}
	var stderr strings.Builder
	code := measure(options{interval: time.Second, log: log, command: []string{os.Args[0]}},
		strings.NewReader(""), io.Discard, &stderr)
	if code != exitHelper {
		t.Fatalf("exit code %d", code)
	}
	if data, _ := os.ReadFile(log); string(data) != "kept" {
		t.Fatalf("log changed to %q", data)
	}
}

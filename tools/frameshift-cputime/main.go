// Command frameshift-cputime runs a program and measures the processor time
// of the program and of every process it starts, while they run.
//
// It is a development-only helper of Frameshift (D-046). The Rust standard
// library has no clock for processor time; the Go standard library reads it
// from the operating system without unsafe code or external modules.
//
// Usage:
//
//	frameshift-cputime [--interval <seconds>] [--log <file>] -- <program> [<argument>...]
//
// The program inherits standard input, output, and error. Every interval,
// and once more after the program ends, the helper reads the user and kernel
// time of the program and its descendants. With --log it writes one JSON
// object per line to a new file. At the end it prints a summary to standard
// error and exits with the program's exit code, or with 125 when it cannot
// run the program or measure its processor time.
package main

import (
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"os/exec"
	"os/signal"
	"runtime"
	"strconv"
	"syscall"
	"time"
)

// exitHelper is the exit code for failures of the helper itself, as used by
// env and timeout.
const exitHelper = 125

const usage = "usage: frameshift-cputime [--interval <seconds>] [--log <file>] -- <program> [<argument>...]\n"

// logFormat and logVersion identify the log protocol.
const (
	logFormat  = "frameshift-cputime"
	logVersion = 1
)

type options struct {
	interval time.Duration
	log      string
	command  []string
}

// process is one measured process. Times are the last values read; exact
// means they were read after the process ended and are final.
type process struct {
	pid       uint32
	parent    uint32
	name      string
	firstSeen time.Duration
	user      time.Duration
	kernel    time.Duration
	exact     bool
}

// meter follows the process tree of one program.
type meter interface {
	// sample adopts new descendants and refreshes the times of every
	// tracked process.
	sample(elapsed time.Duration) error
	// processes returns the tracked processes in the order they were found.
	processes() []*process
	close()
}

func main() {
	os.Exit(run(os.Args[1:], os.Stderr))
}

func run(args []string, stderr io.Writer) int {
	opts, err := parseArgs(args)
	if err != nil {
		fmt.Fprintf(stderr, "frameshift-cputime: %v\n%s", err, usage)
		return exitHelper
	}
	return measure(opts, os.Stdin, os.Stdout, stderr)
}

func parseArgs(args []string) (options, error) {
	opts := options{interval: 5 * time.Second}
	for index := 0; index < len(args); index++ {
		switch arg := args[index]; arg {
		case "--":
			opts.command = args[index+1:]
			if len(opts.command) == 0 {
				return options{}, errors.New("missing program after --")
			}
			return opts, nil
		case "--interval", "--log":
			if index+1 >= len(args) {
				return options{}, fmt.Errorf("missing value for %s", arg)
			}
			index++
			value := args[index]
			if arg == "--log" {
				if value == "" {
					return options{}, errors.New("empty log path")
				}
				opts.log = value
				continue
			}
			seconds, err := strconv.ParseUint(value, 10, 32)
			if err != nil || seconds == 0 {
				return options{}, fmt.Errorf("interval must be a positive whole number of seconds: %q", value)
			}
			opts.interval = time.Duration(seconds) * time.Second
		default:
			return options{}, fmt.Errorf("unknown argument %q", arg)
		}
	}
	return options{}, errors.New("missing -- before the program")
}

// Log records, one JSON object per line. Field order is fixed by the
// structs, and times are whole milliseconds.
type headerRecord struct {
	Record     string   `json:"record"`
	Format     string   `json:"format"`
	Version    int      `json:"version"`
	Platform   string   `json:"platform"`
	IntervalMS int64    `json:"interval_ms"`
	Command    []string `json:"command"`
}

type processRecord struct {
	Record    string `json:"record"`
	ElapsedMS int64  `json:"elapsed_ms"`
	PID       uint32 `json:"pid"`
	Parent    uint32 `json:"parent"`
	Name      string `json:"name"`
	UserMS    int64  `json:"user_ms"`
	KernelMS  int64  `json:"kernel_ms"`
	Exact     bool   `json:"exact"`
}

type endRecord struct {
	Record    string `json:"record"`
	ElapsedMS int64  `json:"elapsed_ms"`
	ExitCode  int    `json:"exit_code"`
	// Complete is false when a sample failed, so processor time may be
	// missing.
	Complete bool `json:"complete"`
	// The program's own times from its exit status. On Linux they include
	// the descendants that the program waited for; on Windows they do not.
	WaitUserMS   int64           `json:"wait_user_ms"`
	WaitKernelMS int64           `json:"wait_kernel_ms"`
	Processes    []processRecord `json:"processes"`
}

func processRecords(elapsed time.Duration, processes []*process) []processRecord {
	records := make([]processRecord, 0, len(processes))
	for _, p := range processes {
		records = append(records, processRecord{
			Record:    "sample",
			ElapsedMS: elapsed.Milliseconds(),
			PID:       p.pid,
			Parent:    p.parent,
			Name:      p.name,
			UserMS:    p.user.Milliseconds(),
			KernelMS:  p.kernel.Milliseconds(),
			Exact:     p.exact,
		})
	}
	return records
}

// logWriter writes records and remembers the first error.
type logWriter struct {
	encoder *json.Encoder
	err     error
}

func (w *logWriter) write(record any) {
	if w == nil || w.err != nil {
		return
	}
	w.err = w.encoder.Encode(record)
}

func measure(opts options, stdin io.Reader, stdout, stderr io.Writer) int {
	var log *logWriter
	if opts.log != "" {
		file, err := os.OpenFile(opts.log, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0o644)
		if err != nil {
			fmt.Fprintf(stderr, "frameshift-cputime: %v\n", err)
			return exitHelper
		}
		defer file.Close()
		log = &logWriter{encoder: json.NewEncoder(file)}
	}
	log.write(headerRecord{
		Record:     "header",
		Format:     logFormat,
		Version:    logVersion,
		Platform:   runtime.GOOS + "/" + runtime.GOARCH,
		IntervalMS: opts.interval.Milliseconds(),
		Command:    opts.command,
	})

	// An interrupt reaches the whole console or process group. The program
	// decides how to end; the helper keeps measuring until it has.
	interrupts := make(chan os.Signal, 1)
	signal.Notify(interrupts, os.Interrupt)
	defer signal.Stop(interrupts)

	cmd := exec.Command(opts.command[0], opts.command[1:]...)
	cmd.Stdin, cmd.Stdout, cmd.Stderr = stdin, stdout, stderr
	start := time.Now()
	if err := cmd.Start(); err != nil {
		fmt.Fprintf(stderr, "frameshift-cputime: %v\n", err)
		return exitHelper
	}
	// The meter starts before anything waits for the program, so the
	// program's ID cannot be reused in between.
	m, err := newMeter(cmd.Process.Pid)
	if err != nil {
		// Without a meter the run would report nothing, so stop the program.
		fmt.Fprintf(stderr, "frameshift-cputime: %v\n", err)
		_ = cmd.Process.Kill()
		_ = cmd.Wait()
		return exitHelper
	}
	defer m.close()
	done := make(chan error, 1)
	go func() { done <- cmd.Wait() }()

	complete := true
	sample := func() {
		elapsed := time.Since(start)
		if err := m.sample(elapsed); err != nil {
			complete = false
			fmt.Fprintf(stderr, "frameshift-cputime: sample at %d ms: %v\n", elapsed.Milliseconds(), err)
		}
		for _, record := range processRecords(elapsed, m.processes()) {
			log.write(record)
		}
	}
	ticker := time.NewTicker(opts.interval)
	defer ticker.Stop()
	var waitErr error
wait:
	for {
		select {
		case <-ticker.C:
			sample()
		case waitErr = <-done:
			break wait
		}
	}
	sample()
	elapsed := time.Since(start)

	state := cmd.ProcessState
	exitCode := exitHelper
	if state != nil {
		exitCode = state.ExitCode()
		if status, ok := state.Sys().(syscall.WaitStatus); ok && status.Signaled() {
			exitCode = 128 + int(status.Signal())
		}
	} else {
		complete = false
		fmt.Fprintf(stderr, "frameshift-cputime: %v\n", waitErr)
	}
	end := endRecord{
		Record:    "end",
		ElapsedMS: elapsed.Milliseconds(),
		ExitCode:  exitCode,
		Complete:  complete,
		Processes: processRecords(elapsed, m.processes()),
	}
	if state != nil {
		end.WaitUserMS = state.UserTime().Milliseconds()
		end.WaitKernelMS = state.SystemTime().Milliseconds()
	}
	log.write(end)
	summarize(stderr, opts.command[0], end)
	if log != nil && log.err != nil {
		fmt.Fprintf(stderr, "frameshift-cputime: writing %s: %v\n", opts.log, log.err)
		return exitHelper
	}
	if !complete {
		return exitHelper
	}
	return exitCode
}

func summarize(w io.Writer, program string, end endRecord) {
	fmt.Fprintf(w, "frameshift-cputime: %s exited with code %d after %s s\n",
		program, end.ExitCode, seconds(end.ElapsedMS))
	for _, p := range end.Processes {
		state := "last observed"
		if p.Exact {
			state = "final"
		}
		fmt.Fprintf(w, "frameshift-cputime: pid %d (parent %d) %s: %s s processor time (user %s s, kernel %s s), %s\n",
			p.PID, p.Parent, p.Name, seconds(p.UserMS+p.KernelMS), seconds(p.UserMS), seconds(p.KernelMS), state)
	}
}

func seconds(ms int64) string {
	return fmt.Sprintf("%d.%03d", ms/1000, ms%1000)
}

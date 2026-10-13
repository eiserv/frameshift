package main

import (
	"errors"
	"strconv"
	"strings"
)

// procStat holds the fields of /proc/<pid>/stat that the Linux meter reads
// (proc_pid_stat(5)). Times are in clock ticks.
type procStat struct {
	pid    uint32
	name   string
	state  byte
	parent uint32
	user   uint64
	kernel uint64
	start  uint64
}

// parseProcStat parses one /proc/<pid>/stat line. The name is enclosed in
// parentheses and may itself contain spaces and parentheses, so the fields
// after it start at the last closing parenthesis.
func parseProcStat(line string) (procStat, error) {
	open := strings.IndexByte(line, '(')
	closing := strings.LastIndexByte(line, ')')
	if open < 1 || closing < open {
		return procStat{}, errors.New("malformed stat line")
	}
	pid, err := strconv.ParseUint(strings.TrimSpace(line[:open]), 10, 32)
	if err != nil {
		return procStat{}, errors.New("malformed process ID")
	}
	// Fields 3 and later; field n is at index n - 3.
	fields := strings.Fields(line[closing+1:])
	if len(fields) < 20 || len(fields[0]) != 1 {
		return procStat{}, errors.New("too few stat fields")
	}
	number := func(index int, bits int) (uint64, error) {
		return strconv.ParseUint(fields[index], 10, bits)
	}
	parent, err := number(1, 32)
	if err != nil {
		return procStat{}, errors.New("malformed parent process ID")
	}
	user, err := number(11, 64)
	if err != nil {
		return procStat{}, errors.New("malformed user time")
	}
	kernel, err := number(12, 64)
	if err != nil {
		return procStat{}, errors.New("malformed kernel time")
	}
	start, err := number(19, 64)
	if err != nil {
		return procStat{}, errors.New("malformed start time")
	}
	return procStat{
		pid:    uint32(pid),
		name:   line[open+1 : closing],
		state:  fields[0][0],
		parent: uint32(parent),
		user:   user,
		kernel: kernel,
		start:  start,
	}, nil
}

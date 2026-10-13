package main

import (
	"errors"
	"fmt"
	"os"
	"reflect"
	"syscall"
	"time"
)

// processQueryLimitedInformation is PROCESS_QUERY_LIMITED_INFORMATION, which
// the syscall package does not define.
const processQueryLimitedInformation = 0x1000

// windowsMeter keeps a handle to every tracked process. A handle keeps the
// process object alive, so its times stay readable after it ends and its ID
// is not reused while the meter runs.
type windowsMeter struct {
	order   []*process
	handles map[uint32]syscall.Handle
	starts  map[uint32]uint64
}

func newMeter(root int) (meter, error) {
	pid := uint32(root)
	handle, err := openProcess(pid)
	if err != nil {
		return nil, fmt.Errorf("OpenProcess %d: %w", pid, err)
	}
	m := &windowsMeter{
		handles: map[uint32]syscall.Handle{pid: handle},
		starts:  map[uint32]uint64{},
	}
	creation, err := m.read(handle, &process{})
	if err != nil {
		m.close()
		return nil, err
	}
	m.starts[pid] = creation
	m.order = []*process{{pid: pid, parent: uint32(os.Getpid())}}
	return m, nil
}

func openProcess(pid uint32) (syscall.Handle, error) {
	return syscall.OpenProcess(processQueryLimitedInformation|syscall.SYNCHRONIZE, false, pid)
}

// filetime converts a FILETIME interval of 100-nanosecond units.
func filetime(value syscall.Filetime) uint64 {
	return uint64(value.HighDateTime)<<32 | uint64(value.LowDateTime)
}

// read refreshes the times of p and returns its creation time.
func (m *windowsMeter) read(handle syscall.Handle, p *process) (uint64, error) {
	var creation, exit, kernel, user syscall.Filetime
	if err := syscall.GetProcessTimes(handle, &creation, &exit, &kernel, &user); err != nil {
		return 0, fmt.Errorf("GetProcessTimes: %w", err)
	}
	event, err := syscall.WaitForSingleObject(handle, 0)
	if event == syscall.WAIT_FAILED {
		return 0, fmt.Errorf("WaitForSingleObject: %w", err)
	}
	p.user = time.Duration(filetime(user) * 100)
	p.kernel = time.Duration(filetime(kernel) * 100)
	p.exact = event == syscall.WAIT_OBJECT_0
	return filetime(creation), nil
}

func processTable() ([]entry, error) {
	snapshot, err := syscall.CreateToolhelp32Snapshot(syscall.TH32CS_SNAPPROCESS, 0)
	if err != nil {
		return nil, fmt.Errorf("CreateToolhelp32Snapshot: %w", err)
	}
	defer syscall.CloseHandle(snapshot)
	var row syscall.ProcessEntry32
	row.Size = uint32(reflect.TypeFor[syscall.ProcessEntry32]().Size())
	var table []entry
	for err = syscall.Process32First(snapshot, &row); err == nil; err = syscall.Process32Next(snapshot, &row) {
		table = append(table, entry{
			pid:    row.ProcessID,
			parent: row.ParentProcessID,
			name:   syscall.UTF16ToString(row.ExeFile[:]),
		})
	}
	if !errors.Is(err, syscall.ERROR_NO_MORE_FILES) {
		return nil, fmt.Errorf("Process32Next: %w", err)
	}
	return table, nil
}

func (m *windowsMeter) sample(elapsed time.Duration) error {
	table, err := processTable()
	if err != nil {
		return err
	}
	for _, p := range m.order {
		for _, row := range table {
			if row.pid == p.pid && p.name == "" {
				p.name = row.name
			}
		}
	}
	// Handles opened to read start times; adopted ones move to m.handles.
	opened := map[uint32]syscall.Handle{}
	startOf := func(row entry) (uint64, bool) {
		handle, err := openProcess(row.pid)
		if err != nil {
			return 0, false
		}
		opened[row.pid] = handle
		creation, err := m.read(handle, &process{})
		return creation, err == nil
	}
	for _, found := range adopt(m.starts, table, startOf) {
		m.handles[found.pid] = opened[found.pid]
		delete(opened, found.pid)
		m.order = append(m.order, &process{
			pid:       found.pid,
			parent:    found.parent,
			name:      found.name,
			firstSeen: elapsed,
		})
	}
	for _, handle := range opened {
		syscall.CloseHandle(handle)
	}
	var failed error
	for _, p := range m.order {
		if _, err := m.read(m.handles[p.pid], p); err != nil && failed == nil {
			failed = fmt.Errorf("process %d: %w", p.pid, err)
		}
	}
	return failed
}

func (m *windowsMeter) processes() []*process {
	return m.order
}

func (m *windowsMeter) close() {
	for pid, handle := range m.handles {
		syscall.CloseHandle(handle)
		delete(m.handles, pid)
	}
}

//go:build linux && (amd64 || arm64)

package main

import (
	"fmt"
	"os"
	"strconv"
	"time"
)

// userHz is the unit of the times in /proc, USER_HZ, which Linux fixes at
// 100 on amd64 and arm64.
const userHz = 100

// linuxMeter reads /proc. A process's entry disappears once its parent has
// waited for it, so the times of an ended process are its last sample unless
// a sample saw it as a zombie.
type linuxMeter struct {
	order  []*process
	byPID  map[uint32]*process
	starts map[uint32]uint64
}

func newMeter(root int) (meter, error) {
	pid := uint32(root)
	stat, err := readStat(pid)
	if err != nil {
		return nil, fmt.Errorf("reading /proc/%d/stat: %w", pid, err)
	}
	p := &process{pid: pid, parent: stat.parent, name: stat.name}
	update(p, stat)
	return &linuxMeter{
		order:  []*process{p},
		byPID:  map[uint32]*process{pid: p},
		starts: map[uint32]uint64{pid: stat.start},
	}, nil
}

func readStat(pid uint32) (procStat, error) {
	data, err := os.ReadFile("/proc/" + strconv.FormatUint(uint64(pid), 10) + "/stat")
	if err != nil {
		return procStat{}, err
	}
	return parseProcStat(string(data))
}

func update(p *process, stat procStat) {
	p.user = time.Duration(stat.user) * time.Second / userHz
	p.kernel = time.Duration(stat.kernel) * time.Second / userHz
	p.exact = stat.state == 'Z'
}

func (m *linuxMeter) sample(elapsed time.Duration) error {
	entries, err := os.ReadDir("/proc")
	if err != nil {
		return err
	}
	stats := map[uint32]procStat{}
	var table []entry
	for _, dirEntry := range entries {
		pid, err := strconv.ParseUint(dirEntry.Name(), 10, 32)
		if err != nil {
			continue
		}
		// A process can end between listing and reading; it is skipped.
		stat, err := readStat(uint32(pid))
		if err != nil {
			continue
		}
		stats[stat.pid] = stat
		table = append(table, entry{pid: stat.pid, parent: stat.parent, name: stat.name})
	}
	startOf := func(row entry) (uint64, bool) {
		stat, ok := stats[row.pid]
		return stat.start, ok
	}
	for _, found := range adopt(m.starts, table, startOf) {
		p := &process{pid: found.pid, parent: found.parent, name: found.name, firstSeen: elapsed}
		m.order = append(m.order, p)
		m.byPID[found.pid] = p
	}
	for pid, p := range m.byPID {
		// A different start time means the ID now belongs to another process.
		if stat, ok := stats[pid]; ok && stat.start == m.starts[pid] {
			update(p, stat)
		}
	}
	return nil
}

func (m *linuxMeter) processes() []*process {
	return m.order
}

func (m *linuxMeter) close() {}

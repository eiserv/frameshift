package main

// entry is one row of the operating system's process table.
type entry struct {
	pid    uint32
	parent uint32
	name   string
}

// adoption is a row that descends from a tracked process, with its start
// time.
type adoption struct {
	entry
	start uint64
}

// adopt returns the rows of table that descend from the tracked processes,
// given as start times by process ID, so that every parent precedes its
// children. startOf reads the start time of a row. A row that started before
// its parent belongs to an older process whose ID the system reused, and a
// row whose start time cannot be read has ended; both are skipped. adopt
// adds the rows it returns to tracked.
func adopt(tracked map[uint32]uint64, table []entry, startOf func(entry) (uint64, bool)) []adoption {
	var adopted []adoption
	skipped := make(map[uint32]bool)
	for changed := true; changed; {
		changed = false
		for _, row := range table {
			if _, known := tracked[row.pid]; known || skipped[row.pid] {
				continue
			}
			parentStart, ok := tracked[row.parent]
			if !ok {
				continue
			}
			start, ok := startOf(row)
			if !ok || start < parentStart {
				skipped[row.pid] = true
				continue
			}
			tracked[row.pid] = start
			adopted = append(adopted, adoption{entry: row, start: start})
			changed = true
		}
	}
	return adopted
}

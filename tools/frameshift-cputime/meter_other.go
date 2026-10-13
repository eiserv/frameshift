//go:build !windows && !(linux && (amd64 || arm64))

package main

import (
	"fmt"
	"runtime"
)

// Other platforms have no process table that the Go standard library can
// read without unsafe code, so the helper refuses to run there.
func newMeter(int) (meter, error) {
	return nil, fmt.Errorf("processor time sampling is not supported on %s/%s", runtime.GOOS, runtime.GOARCH)
}

// Frameshift's development-only deterministic archive and checksum tool.
package main

import (
	"archive/tar"
	"archive/zip"
	"bytes"
	"compress/gzip"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"flag"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"regexp"
	"sort"
	"strconv"
	"strings"
	"time"
)

var versionPattern = regexp.MustCompile(`^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$`)

type entry struct {
	name string
	data []byte
	mode int64
}

func main() {
	if err := run(os.Args[1:]); err != nil {
		fmt.Fprintln(os.Stderr, "error:", err)
		os.Exit(1)
	}
}

func run(args []string) error {
	if len(args) == 0 {
		return errors.New("expected pack or checksums")
	}
	switch args[0] {
	case "pack":
		flags := flag.NewFlagSet("pack", flag.ContinueOnError)
		binary := flags.String("binary", "", "built Frameshift binary")
		docs := flags.String("docs", ".", "repository containing release documentation")
		out := flags.String("out", "", "existing artifact directory")
		version := flags.String("version", "", "three-part release version")
		platform := flags.String("os", "", "linux, macos, or windows")
		arch := flags.String("arch", "", "x86_64 or arm64")
		epoch := flags.String("epoch", "", "commit Unix time")
		if err := flags.Parse(args[1:]); err != nil {
			return err
		}
		if flags.NArg() != 0 || *binary == "" || *out == "" {
			return errors.New("pack requires a binary, output directory, and no positional arguments")
		}
		seconds, err := strconv.ParseInt(*epoch, 10, 64)
		if err != nil || seconds < 315532800 || seconds > 4354819199 {
			return errors.New("commit time must fit ZIP's 1980 through 2107 timestamp range")
		}
		return pack(*binary, *docs, *out, *version, *platform, *arch, time.Unix(seconds, 0).UTC())
	case "checksums":
		if len(args) != 3 || !versionPattern.MatchString(args[2]) {
			return errors.New("checksums requires an artifact directory and three-part version")
		}
		return checksums(args[1], args[2])
	default:
		return errors.New("unknown release-tool command")
	}
}

func pack(binary, docs, out, version, platform, arch string, stamp time.Time) error {
	if !versionPattern.MatchString(version) || (platform != "linux" && platform != "macos" && platform != "windows") || (arch != "x86_64" && arch != "arm64") {
		return errors.New("invalid version, platform, or architecture")
	}
	base := "frameshift-" + version + "-" + platform + "-" + arch
	name := "frameshift"
	if platform == "windows" {
		name += ".exe"
	}
	inputs := []struct {
		name, path string
		mode       int64
	}{
		{name, binary, 0755},
		{"README.md", filepath.Join(docs, "README.md"), 0644},
		{"LICENSE", filepath.Join(docs, "LICENSE"), 0644},
		{"THIRD_PARTY_NOTICES.md", filepath.Join(docs, "THIRD_PARTY_NOTICES.md"), 0644},
	}
	entries := make([]entry, 0, len(inputs))
	for _, input := range inputs {
		info, err := os.Lstat(input.path)
		if err != nil {
			return err
		}
		if !info.Mode().IsRegular() || info.Size() > 256*1024*1024 {
			return errors.New("archive inputs must be regular files of at most 256 MiB")
		}
		data, err := os.ReadFile(input.path)
		if err != nil {
			return err
		}
		entries = append(entries, entry{base + "/" + input.name, data, input.mode})
	}
	sort.Slice(entries, func(i, j int) bool { return entries[i].name < entries[j].name })
	var archive bytes.Buffer
	extension := ".tar.gz"
	if platform == "windows" {
		extension = ".zip"
		if err := writeZip(&archive, entries, stamp); err != nil {
			return err
		}
	} else if err := writeTar(&archive, entries, stamp); err != nil {
		return err
	}
	if err := verifyArchive(archive.Bytes(), entries, platform); err != nil {
		return err
	}
	return writeNew(filepath.Join(out, base+extension), archive.Bytes())
}

func writeZip(out io.Writer, entries []entry, stamp time.Time) error {
	w := zip.NewWriter(out)
	for _, item := range entries {
		h := &zip.FileHeader{Name: item.name, Method: zip.Deflate}
		h.SetMode(os.FileMode(item.mode))
		h.SetModTime(stamp)
		f, err := w.CreateHeader(h)
		if err != nil {
			return err
		}
		if _, err := f.Write(item.data); err != nil {
			return err
		}
	}
	return w.Close()
}

func writeTar(out io.Writer, entries []entry, stamp time.Time) error {
	gz := gzip.NewWriter(out)
	w := tar.NewWriter(gz)
	for _, item := range entries {
		if err := w.WriteHeader(&tar.Header{Name: item.name, Mode: item.mode, Size: int64(len(item.data)), ModTime: stamp, Typeflag: tar.TypeReg, Format: tar.FormatPAX}); err != nil {
			return err
		}
		if _, err := w.Write(item.data); err != nil {
			return err
		}
	}
	if err := w.Close(); err != nil {
		return err
	}
	return gz.Close()
}

func verifyArchive(data []byte, entries []entry, platform string) error {
	check := func(index int, name string, body io.Reader, mode int64) error {
		if index >= len(entries) || entries[index].name != name || entries[index].mode != mode {
			return errors.New("archive entry or mode differs from release inputs")
		}
		actual, err := io.ReadAll(io.LimitReader(body, int64(len(entries[index].data))+1))
		if err != nil || !bytes.Equal(actual, entries[index].data) {
			return errors.New("archive contents differ from release inputs")
		}
		return nil
	}
	if platform == "windows" {
		reader, err := zip.NewReader(bytes.NewReader(data), int64(len(data)))
		if err != nil {
			return err
		}
		if len(reader.File) != len(entries) {
			return errors.New("archive entry count differs from release inputs")
		}
		for i, file := range reader.File {
			body, err := file.Open()
			if err != nil {
				return err
			}
			err = check(i, file.Name, body, int64(file.Mode().Perm()))
			closeErr := body.Close()
			if err != nil {
				return err
			}
			if closeErr != nil {
				return closeErr
			}
		}
		return nil
	}
	gz, err := gzip.NewReader(bytes.NewReader(data))
	if err != nil {
		return err
	}
	defer gz.Close()
	reader := tar.NewReader(gz)
	for i := 0; ; i++ {
		h, err := reader.Next()
		if err == io.EOF && i == len(entries) {
			// Drain the gzip trailer to validate its checksum as well.
			_, err = io.Copy(io.Discard, gz)
			return err
		}
		if err != nil {
			return errors.New("archive entry count differs from release inputs")
		}
		if h.Typeflag != tar.TypeReg {
			return errors.New("archive contains a non-regular entry")
		}
		if err := check(i, h.Name, reader, h.Mode); err != nil {
			return err
		}
	}
}

func checksums(dir, version string) error {
	files, err := os.ReadDir(dir)
	if err != nil {
		return err
	}
	pattern := regexp.MustCompile(`^frameshift-` + regexp.QuoteMeta(version) + `-((linux|macos)-(x86_64|arm64)\.tar\.gz|windows-(x86_64|arm64)\.zip)$`)
	var result strings.Builder
	count := 0
	for _, file := range files {
		if !pattern.MatchString(file.Name()) {
			return errors.New("checksum directory must contain only release archives")
		}
		info, err := file.Info()
		if err != nil || !info.Mode().IsRegular() {
			return errors.New("checksum inputs must be regular files")
		}
		f, err := os.Open(filepath.Join(dir, file.Name()))
		if err != nil {
			return err
		}
		hash := sha256.New()
		_, copyErr := io.Copy(hash, f)
		closeErr := f.Close()
		if copyErr != nil {
			return copyErr
		}
		if closeErr != nil {
			return closeErr
		}
		fmt.Fprintf(&result, "%s  %s\n", hex.EncodeToString(hash.Sum(nil)), file.Name())
		count++
	}
	if count == 0 {
		return errors.New("no archives to checksum")
	}
	return writeNew(filepath.Join(dir, "frameshift-"+version+"-checksums.txt"), []byte(result.String()))
}

func writeNew(path string, data []byte) error {
	f, err := os.OpenFile(path, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0644)
	if err != nil {
		return err
	}
	_, writeErr := f.Write(data)
	closeErr := f.Close()
	if writeErr != nil {
		return writeErr
	}
	return closeErr
}

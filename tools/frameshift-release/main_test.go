package main

import (
	"archive/zip"
	"bytes"
	"crypto/sha256"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

func TestArchivesRepeatAndPreserveInputs(t *testing.T) {
	for _, platform := range []string{"linux", "macos", "windows"} {
		t.Run(platform, func(t *testing.T) {
			root := t.TempDir()
			for name, data := range map[string]string{"binary": "fictional executable\x00", "README.md": "Frameshift\n", "THIRD_PARTY_NOTICES.md": "Notices\n"} {
				if err := os.WriteFile(filepath.Join(root, name), []byte(data), 0600); err != nil {
					t.Fatal(err)
				}
			}
			stamp := time.Unix(1791928801, 0).UTC()
			var outputs [][]byte
			for _, dir := range []string{"one", "two"} {
				out := filepath.Join(root, dir)
				if err := os.Mkdir(out, 0700); err != nil {
					t.Fatal(err)
				}
				if err := pack(filepath.Join(root, "binary"), root, out, "0.0.0", platform, "arm64", stamp); err != nil {
					t.Fatal(err)
				}
				files, _ := os.ReadDir(out)
				if len(files) != 1 {
					t.Fatal("expected one archive")
				}
				data, err := os.ReadFile(filepath.Join(out, files[0].Name()))
				if err != nil {
					t.Fatal(err)
				}
				outputs = append(outputs, data)
				if err := checksums(out, "0.0.0"); err != nil {
					t.Fatal(err)
				}
				sums, _ := os.ReadFile(filepath.Join(out, "frameshift-0.0.0-checksums.txt"))
				if string(sums) != fmt.Sprintf("%x  %s\n", sha256.Sum256(data), files[0].Name()) {
					t.Fatal("incorrect checksum")
				}
				if err := pack(filepath.Join(root, "binary"), root, out, "0.0.0", platform, "arm64", stamp); err == nil {
					t.Fatal("replaced an existing archive")
				}
			}
			if !bytes.Equal(outputs[0], outputs[1]) {
				t.Fatal("archive depends on output directory")
			}
		})
	}
}

func TestRejectsMalformedOptionsAndArchiveTampering(t *testing.T) {
	for _, args := range [][]string{nil, {"unknown"}, {"pack"}, {"pack", "--binary", "x", "--out", "y", "--epoch", "-1"}, {"checksums", ".", "../escape"}} {
		if run(args) == nil {
			t.Fatalf("accepted %q", args)
		}
	}
	for _, version := range []string{"../x", "01.0.0", "v1.0.0", "1.0.0\n", "1.0"} {
		if pack("", "", "", version, "linux", "arm64", time.Time{}) == nil {
			t.Fatal("accepted invalid version")
		}
	}
	entries := []entry{{"frameshift-0.0.0-windows-arm64/frameshift.exe", []byte("binary"), 0755}}
	var archive bytes.Buffer
	if err := writeZip(&archive, entries, time.Unix(1791928801, 0).UTC()); err != nil {
		t.Fatal(err)
	}
	reader, err := zip.NewReader(bytes.NewReader(archive.Bytes()), int64(archive.Len()))
	if err != nil || reader.File[0].Mode().Perm() != 0755 {
		t.Fatal("ZIP mode not preserved")
	}
	if verifyArchive(archive.Bytes(), []entry{{entries[0].name, []byte("tampered"), 0755}}, "windows") == nil {
		t.Fatal("accepted different contents")
	}
	if verifyArchive(archive.Bytes(), nil, "windows") == nil {
		t.Fatal("accepted extra entry")
	}
}

func TestChecksumOrderAndForeignFiles(t *testing.T) {
	dir := t.TempDir()
	for _, name := range []string{"frameshift-0.0.0-windows-arm64.zip", "frameshift-0.0.0-linux-x86_64.tar.gz"} {
		if err := os.WriteFile(filepath.Join(dir, name), []byte(name), 0600); err != nil {
			t.Fatal(err)
		}
	}
	if err := checksums(dir, "0.0.0"); err != nil {
		t.Fatal(err)
	}
	data, _ := os.ReadFile(filepath.Join(dir, "frameshift-0.0.0-checksums.txt"))
	if strings.Index(string(data), "linux") > strings.Index(string(data), "windows") {
		t.Fatal("checksums are not sorted")
	}
	if checksums(dir, "0.0.0") == nil {
		t.Fatal("accepted existing checksum file")
	}
	if checksums(t.TempDir(), "0.0.0") == nil {
		t.Fatal("accepted empty directory")
	}
}

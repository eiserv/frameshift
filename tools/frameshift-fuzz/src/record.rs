//! The record of the input that the worker is executing (`D-044`).
//!
//! Before every execution, the worker overwrites one file with the input,
//! prefixed by its length as eight little-endian bytes, with one positional
//! write. A crash or hang can stop the worker at any instruction, but the
//! operating system keeps the written bytes, so the supervisor recovers the
//! input that caused it. The file is not flushed to the device, because only
//! the death of the process, not of the machine, must be survived.

use std::{
    fs::{self, File},
    io,
    path::Path,
};

/// The open record file and a reusable buffer.
pub struct Record {
    file: File,
    buffer: Vec<u8>,
}

impl Record {
    /// Creates or truncates the record file.
    pub fn create(path: &Path) -> io::Result<Self> {
        Ok(Self {
            file: File::create(path)?,
            buffer: Vec::new(),
        })
    }

    /// Replaces the recorded input.
    pub fn write(&mut self, input: &[u8]) -> io::Result<()> {
        self.buffer.clear();
        self.buffer
            .extend_from_slice(&(input.len() as u64).to_le_bytes());
        self.buffer.extend_from_slice(input);
        write_at(&self.file, &self.buffer)
    }
}

/// Reads the input of a record file, or `None` if the file holds no complete
/// record.
pub fn read(path: &Path) -> io::Result<Option<Vec<u8>>> {
    let bytes = fs::read(path)?;
    let Some((length, rest)) = bytes.split_first_chunk::<8>() else {
        return Ok(None);
    };
    let length = u64::from_le_bytes(*length);
    Ok(usize::try_from(length)
        .ok()
        .and_then(|length| rest.get(..length))
        .map(<[u8]>::to_vec))
}

#[cfg(windows)]
fn write_at(file: &File, mut bytes: &[u8]) -> io::Result<()> {
    use std::os::windows::fs::FileExt;
    let mut offset = 0;
    while !bytes.is_empty() {
        let written = file.seek_write(bytes, offset)?;
        if written == 0 {
            return Err(io::ErrorKind::WriteZero.into());
        }
        bytes = &bytes[written..];
        offset += written as u64;
    }
    Ok(())
}

#[cfg(unix)]
fn write_at(file: &File, bytes: &[u8]) -> io::Result<()> {
    use std::os::unix::fs::FileExt;
    file.write_all_at(bytes, 0)
}

#[cfg(not(any(windows, unix)))]
fn write_at(mut file: &File, bytes: &[u8]) -> io::Result<()> {
    use std::io::{Seek, SeekFrom, Write};
    file.seek(SeekFrom::Start(0))?;
    file.write_all(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_keep_only_the_latest_input() {
        let directory =
            std::env::temp_dir().join(format!("frameshift-fuzz-record-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("current");
        let mut record = Record::create(&path).unwrap();
        record.write(b"a much longer first input").unwrap();
        record.write(b"short").unwrap();
        assert_eq!(read(&path).unwrap().as_deref(), Some(&b"short"[..]));
        record.write(b"").unwrap();
        assert_eq!(read(&path).unwrap().as_deref(), Some(&b""[..]));
        drop(record);
        fs::write(&path, [3, 0, 0, 0, 0, 0, 0, 0, b'a']).unwrap();
        assert_eq!(read(&path).unwrap(), None);
        fs::write(&path, [1, 2]).unwrap();
        assert_eq!(read(&path).unwrap(), None);
        fs::remove_dir_all(&directory).unwrap();
    }
}

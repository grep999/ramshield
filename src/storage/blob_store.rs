use crate::error::{Result, RsError};
use lz4_flex::{compress_prepend_size, decompress_size_prepended};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::sync::{Arc, Mutex};

pub struct BlobHandle {
    pub offset:         u64,
    pub compressed_len: u32,
    pub original_len:   u32,
}

pub struct BlobStore {
    inner: Arc<Mutex<Inner>>,
}

struct Inner {
    file:   File,
    cursor: u64,
}

impl BlobStore {
    pub fn open(path: &str) -> Result<Self> {
        let file   = OpenOptions::new().create(true).read(true).write(true).open(path)?;
        let cursor = file.metadata()?.len();
        Ok(Self { inner: Arc::new(Mutex::new(Inner { file, cursor })) })
    }

    pub fn write(&self, data: &[u8]) -> Result<BlobHandle> {
        let compressed = compress_prepend_size(data);
        let comp_len   = compressed.len() as u32;
        let orig_len   = data.len() as u32;
        let mut g      = self.inner.lock().unwrap();
        g.file.seek(SeekFrom::End(0))?;
        g.file.write_all(&compressed)?;
        let offset  = g.cursor;
        g.cursor   += comp_len as u64;
        Ok(BlobHandle { offset, compressed_len: comp_len, original_len: orig_len })
    }

    pub fn read(&self, h: &BlobHandle) -> Result<Vec<u8>> {
        let mut g = self.inner.lock().unwrap();
        g.file.seek(SeekFrom::Start(h.offset))?;
        let mut buf = vec![0u8; h.compressed_len as usize];
        g.file.read_exact(&mut buf)?;
        decompress_size_prepended(&buf).map_err(|e| RsError::Serde(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blob_roundtrip() {
        let path = std::env::temp_dir().join("rs_blob.bin").to_string_lossy().to_string();
        let _ = std::fs::remove_file(&path);
        let store = BlobStore::open(&path).unwrap();
        let data  = b"ramshield blob store roundtrip test";
        let h     = store.write(data).unwrap();
        assert_eq!(store.read(&h).unwrap(), data);
        let _ = std::fs::remove_file(&path);
    }
}

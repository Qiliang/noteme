use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use fs2::FileExt;

use super::error::{Error, Result};
use super::format::*;
use super::name::validate_name;

#[derive(Debug, Clone, serde::Serialize)]
pub struct Meta {
    pub name: String,
    pub size: u64,
    pub ctime_ms: u64,
    pub mtime_ms: u64,
    pub atime_ms: u64,
    pub file_type: u16,
    pub content_hash: [u8; 32],
}

struct Inner {
    dir: PathBuf,
    header: File,
    data: File,
    _lock: File,
    sb: Superblock,
    entries: Vec<Entry>,
    name_index: HashMap<Vec<u8>, u32>,
    free_extents: Vec<FreeExtent>,
    dirty_slots: HashSet<u32>,
}

pub struct Store {
    inner: RwLock<Inner>,
}

impl Store {
    /// Open existing store, or create with `DEFAULT_ENTRY_COUNT` slots.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::open_with_entry_count(path, DEFAULT_ENTRY_COUNT)
    }

    pub fn open_with_entry_count(path: impl AsRef<Path>, entry_count: u32) -> Result<Self> {
        if entry_count == 0 {
            return Err(Error::Corrupt("entry_count must be > 0".into()));
        }
        let dir = path.as_ref().to_path_buf();
        fs::create_dir_all(&dir)?;

        let lock_path = dir.join("yfs.lock");
        let lock = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(&lock_path)?;
        lock.try_lock_exclusive()
            .map_err(|_| Error::Locked(dir.clone()))?;

        let header_path = dir.join("header.bin");
        let data_path = dir.join("data.bin");
        

        let (header, data, created) = if header_path.exists() {
            let header = OpenOptions::new()
                .read(true)
                .write(true)
                .open(&header_path)?;
            let data = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .open(&data_path)?;
            (header, data, false)
        } else {
            let header = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .open(&header_path)?;
            let data = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .open(&data_path)?;
            (header, data, true)
        };

        let mut inner = Inner {
            dir,
            header,
            data,
            _lock: lock,
            sb: Superblock::new(entry_count),
            entries: Vec::new(),
            name_index: HashMap::new(),
            free_extents: Vec::new(),
            dirty_slots: HashSet::new(),
        };

        if created {
            inner.init_new(entry_count)?;
        } else {
            inner.load_existing()?;
        }

        Ok(Self {
            inner: RwLock::new(inner),
        })
    }

    pub fn stat(&self, name: &str) -> Result<Meta> {
        let name = validate_name(name)?;
        let mut g = self.inner.write().map_err(|_| Error::Corrupt("lock poisoned".into()))?;
        let slot = *g.name_index.get(&name).ok_or(Error::NotFound)?;
        let entry = &mut g.entries[slot as usize];
        entry.atime_ms = now_ms();
        Ok(meta_from_entry(entry)?)
    }

    pub fn list(&self, prefix: &str) -> Result<Vec<Meta>> {
        let prefix_bytes = prefix.as_bytes();
        let g = self.inner.read().map_err(|_| Error::Corrupt("lock poisoned".into()))?;
        let mut out = Vec::new();
        for entry in &g.entries {
            if !entry.is_live() {
                continue;
            }
            if entry.name_bytes().starts_with(prefix_bytes) {
                out.push(meta_from_entry(entry)?);
            }
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    pub fn read(&self, name: &str) -> Result<Vec<u8>> {
        self.read_at(name, 0, u64::MAX)
    }

    pub fn read_at(&self, name: &str, off: u64, len: u64) -> Result<Vec<u8>> {
        let name = validate_name(name)?;
        let mut g = self.inner.write().map_err(|_| Error::Corrupt("lock poisoned".into()))?;
        let slot = *g.name_index.get(&name).ok_or(Error::NotFound)?;
        let (start_chunk, size) = {
            let entry = &mut g.entries[slot as usize];
            entry.atime_ms = now_ms();
            (entry.start_chunk, entry.size)
        };
        if off >= size || len == 0 {
            return Ok(Vec::new());
        }
        let end = off.saturating_add(len).min(size);
        let read_len = (end - off) as usize;
        let mut buf = vec![0u8; read_len];
        let data_off = u64::from(start_chunk) * u64::from(CHUNK_SIZE) + off;
        g.data.seek(SeekFrom::Start(data_off))?;
        g.data.read_exact(&mut buf)?;
        Ok(buf)
    }

    pub fn write(&self, name: &str, bytes: &[u8]) -> Result<()> {
        self.write_with_type(name, bytes, FILE_TYPE_BLOB)
    }

    pub fn write_with_type(&self, name: &str, bytes: &[u8], file_type: u16) -> Result<()> {
        let name = validate_name(name)?;
        if bytes.len() as u64 > MAX_FILE_SIZE {
            return Err(Error::FileTooLarge);
        }
        let mut g = self.inner.write().map_err(|_| Error::Corrupt("lock poisoned".into()))?;
        let need = chunks_for_size(bytes.len() as u64);
        let new_extent = if need == 0 {
            None
        } else {
            Some(g.alloc_extent(need)?)
        };

        if let Some(ext) = new_extent {
            let data_off = u64::from(ext.start) * u64::from(CHUNK_SIZE);
            g.data.seek(SeekFrom::Start(data_off))?;
            g.data.write_all(bytes)?;
            // zero-pad remainder of last chunk
            let padded = u64::from(need) * u64::from(CHUNK_SIZE);
            let pad = padded - bytes.len() as u64;
            if pad > 0 {
                let zeros = vec![0u8; pad as usize];
                g.data.write_all(&zeros)?;
            }
            g.data.sync_all()?;
        }

        let hash = content_hash(bytes);
        let now = now_ms();
        let old_extent;

        if let Some(&slot) = g.name_index.get(&name) {
            let entry = &mut g.entries[slot as usize];
            old_extent = if entry.n_chunks > 0 {
                Some(FreeExtent {
                    start: entry.start_chunk,
                    n_chunks: entry.n_chunks,
                })
            } else {
                None
            };
            entry.size = bytes.len() as u64;
            entry.mtime_ms = now;
            entry.atime_ms = now;
            entry.file_type = file_type;
            entry.content_hash = hash;
            if let Some(ext) = new_extent {
                entry.start_chunk = ext.start;
                entry.n_chunks = ext.n_chunks;
            } else {
                entry.start_chunk = 0;
                entry.n_chunks = 0;
            }
            g.dirty_slots.insert(slot);
        } else {
            old_extent = None;
            let slot = g.alloc_entry_slot()?;
            let entry = &mut g.entries[slot as usize];
            *entry = Entry {
                magic: MAGIC_ENT1,
                flags: 0,
                file_type,
                size: bytes.len() as u64,
                ctime_ms: now,
                mtime_ms: now,
                atime_ms: now,
                content_hash: hash,
                start_chunk: new_extent.map(|e| e.start).unwrap_or(0),
                n_chunks: new_extent.map(|e| e.n_chunks).unwrap_or(0),
                next_free: NIL,
                name_len: 0,
                name: [0; 256],
            };
            entry.set_name(&name)?;
            g.name_index.insert(name, slot);
            g.dirty_slots.insert(slot);
        }

        if let Some(old) = old_extent {
            g.free_extent(old);
        }
        g.commit()?;
        Ok(())
    }

    pub fn delete(&self, name: &str) -> Result<()> {
        let name = validate_name(name)?;
        let mut g = self.inner.write().map_err(|_| Error::Corrupt("lock poisoned".into()))?;
        let slot = g.name_index.remove(&name).ok_or(Error::NotFound)?;
        let entry = &g.entries[slot as usize];
        if entry.n_chunks > 0 {
            let ext = FreeExtent {
                start: entry.start_chunk,
                n_chunks: entry.n_chunks,
            };
            g.free_extent(ext);
        }
        let next = g.sb.free_entry_head;
        g.entries[slot as usize] = Entry::free(next);
        g.sb.free_entry_head = slot;
        g.dirty_slots.insert(slot);
        g.commit()?;
        Ok(())
    }

    pub fn rename(&self, old: &str, new: &str) -> Result<()> {
        let old = validate_name(old)?;
        let new = validate_name(new)?;
        if old == new {
            return Ok(());
        }
        let mut g = self.inner.write().map_err(|_| Error::Corrupt("lock poisoned".into()))?;
        if g.name_index.contains_key(&new) {
            return Err(Error::AlreadyExists);
        }
        let slot = g.name_index.remove(&old).ok_or(Error::NotFound)?;
        let entry = &mut g.entries[slot as usize];
        entry.set_name(&new)?;
        entry.mtime_ms = now_ms();
        g.name_index.insert(new, slot);
        g.dirty_slots.insert(slot);
        g.commit()?;
        Ok(())
    }

    pub fn path(&self) -> Result<PathBuf> {
        let g = self
            .inner
            .read()
            .map_err(|_| Error::Corrupt("lock poisoned".into()))?;
        Ok(g.dir.clone())
    }

    pub fn compact(&self) -> Result<()> {
        let mut g = self.inner.write().map_err(|_| Error::Corrupt("lock poisoned".into()))?;
        let mut cursor = 0u32;
        let mut live: Vec<(u32, u32, u32)> = Vec::new(); // slot, old_start, n_chunks
        for (slot, entry) in g.entries.iter().enumerate() {
            if entry.is_live() && entry.n_chunks > 0 {
                live.push((slot as u32, entry.start_chunk, entry.n_chunks));
            }
        }
        live.sort_by_key(|(_, start, _)| *start);

        // Copy into a temp buffer file then replace, to keep crash safety simple.
        let tmp_path = g.dir.join("data.bin.compact");
        let mut tmp = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&tmp_path)?;

        for (slot, old_start, n_chunks) in live {
            let bytes = n_chunks as usize * CHUNK_SIZE as usize;
            let mut buf = vec![0u8; bytes];
            let src = u64::from(old_start) * u64::from(CHUNK_SIZE);
            g.data.seek(SeekFrom::Start(src))?;
            g.data.read_exact(&mut buf)?;
            tmp.write_all(&buf)?;
            let entry = &mut g.entries[slot as usize];
            entry.start_chunk = cursor;
            g.dirty_slots.insert(slot);
            cursor += n_chunks;
        }
        tmp.sync_all()?;
        drop(tmp);

        let data_path = g.dir.join("data.bin");
        fs::rename(&tmp_path, &data_path)?;
        g.data = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&data_path)?;
        g.sb.chunk_count = cursor;
        g.free_extents.clear();
        g.sb.free_chunk_head = NIL;
        if cursor > 0 {
            g.data.set_len(u64::from(cursor) * u64::from(CHUNK_SIZE))?;
        } else {
            g.data.set_len(0)?;
        }
        g.data.sync_all()?;
        g.commit()?;
        Ok(())
    }
}

impl Inner {
    fn init_new(&mut self, entry_count: u32) -> Result<()> {
        self.sb = Superblock::new(entry_count);
        self.entries = (0..entry_count)
            .map(|i| {
                let next = if i + 1 < entry_count { i + 1 } else { NIL };
                Entry::free(next)
            })
            .collect();
        self.sb.free_entry_head = 0;
        self.sb.free_chunk_head = NIL;
        self.sb.chunk_count = 0;
        self.name_index.clear();
        self.free_extents.clear();

        self.header.set_len(header_file_size(entry_count))?;
        self.data.set_len(0)?;

        // Write all entries as dirty.
        for i in 0..entry_count {
            self.dirty_slots.insert(i);
        }
        self.commit()?;
        Ok(())
    }

    fn load_existing(&mut self) -> Result<()> {
        let mut sb_buf = [0u8; SUPERBLOCK_SIZE as usize];
        self.header.seek(SeekFrom::Start(0))?;
        self.header.read_exact(&mut sb_buf)?;
        let sb = Superblock::decode(&sb_buf)?;
        self.sb = sb;

        let commit = self.select_commit()?;
        self.sb.store_epoch = commit.store_epoch;
        self.sb.active_side = commit.active_side;
        self.sb.free_entry_head = commit.free_entry_head;
        self.sb.free_chunk_head = commit.free_chunk_head;
        self.sb.chunk_count = commit.chunk_count;
        self.sb.flags = commit.flags;

        self.entries = Vec::with_capacity(self.sb.entry_count as usize);
        let mut table_crc = 0u32;
        for slot in 0..self.sb.entry_count {
            let mut buf = [0u8; ENTRY_SIZE as usize];
            self.header.seek(SeekFrom::Start(entry_offset(slot)))?;
            self.header.read_exact(&mut buf)?;
            table_crc = crc32c::crc32c_append(table_crc, &buf);
            match Entry::decode(&buf) {
                Ok(entry) => self.entries.push(entry),
                Err(_) => {
                    // Damaged slot: treat as free for repair.
                    self.entries.push(Entry::free(NIL));
                    self.sb.flags |= FLAG_NEEDS_REPAIR;
                }
            }
        }
        if table_crc != commit.entry_table_crc {
            self.sb.flags |= FLAG_NEEDS_REPAIR;
        }

        self.rebuild_name_index()?;
        self.load_free_extents()?;

        if self.sb.flags & FLAG_NEEDS_REPAIR != 0 {
            self.repair()?;
        }
        Ok(())
    }

    fn select_commit(&mut self) -> Result<CommitRecord> {
        let mut best: Option<CommitRecord> = None;
        for side in 0..2u32 {
            let mut buf = [0u8; COMMIT_SIZE as usize];
            self.header
                .seek(SeekFrom::Start(commit_offset(self.sb.entry_count, side)))?;
            if self.header.read_exact(&mut buf).is_err() {
                continue;
            }
            if let Ok(rec) = CommitRecord::decode(&buf) {
                if best
                    .as_ref()
                    .map(|b| rec.store_epoch > b.store_epoch)
                    .unwrap_or(true)
                {
                    best = Some(rec);
                }
            }
        }
        best.ok_or_else(|| Error::Corrupt("no valid commit record".into()))
    }

    fn rebuild_name_index(&mut self) -> Result<()> {
        self.name_index.clear();
        for (slot, entry) in self.entries.iter().enumerate() {
            if entry.is_live() {
                let key = entry.name_bytes().to_vec();
                if self.name_index.insert(key, slot as u32).is_some() {
                    return Err(Error::Corrupt(format!(
                        "duplicate name in slot {slot}"
                    )));
                }
            }
        }
        Ok(())
    }

    fn load_free_extents(&mut self) -> Result<()> {
        self.free_extents.clear();
        let mut cur = self.sb.free_chunk_head;
        let mut guard = 0u32;
        while cur != NIL {
            if guard > self.sb.chunk_count {
                return Err(Error::Corrupt("free extent list loop".into()));
            }
            guard += 1;
            let mut buf = [0u8; 16];
            let off = u64::from(cur) * u64::from(CHUNK_SIZE);
            self.data.seek(SeekFrom::Start(off))?;
            self.data.read_exact(&mut buf)?;
            let (ext, next) = FreeExtent::decode(&buf, cur)?;
            if ext.start + ext.n_chunks > self.sb.chunk_count {
                return Err(Error::Corrupt("free extent out of range".into()));
            }
            self.free_extents.push(ext);
            cur = next;
        }
        Ok(())
    }

    fn persist_free_extents(&mut self) -> Result<()> {
        self.normalize_free_extents();
        if self.free_extents.is_empty() {
            self.sb.free_chunk_head = NIL;
            return Ok(());
        }
        self.sb.free_chunk_head = self.free_extents[0].start;
        for i in 0..self.free_extents.len() {
            let next = if i + 1 < self.free_extents.len() {
                self.free_extents[i + 1].start
            } else {
                NIL
            };
            let ext = self.free_extents[i];
            let buf = ext.encode(next);
            let off = u64::from(ext.start) * u64::from(CHUNK_SIZE);
            self.data.seek(SeekFrom::Start(off))?;
            self.data.write_all(&buf)?;
        }
        self.data.sync_all()?;
        Ok(())
    }

    fn normalize_free_extents(&mut self) {
        if self.free_extents.is_empty() {
            return;
        }
        self.free_extents.sort_by_key(|e| e.start);
        let mut merged = Vec::new();
        let mut cur = self.free_extents[0];
        for ext in self.free_extents.iter().skip(1) {
            if cur.start + cur.n_chunks == ext.start {
                cur.n_chunks += ext.n_chunks;
            } else {
                merged.push(cur);
                cur = *ext;
            }
        }
        merged.push(cur);
        self.free_extents = merged;
    }

    fn free_extent(&mut self, ext: FreeExtent) {
        if ext.n_chunks == 0 {
            return;
        }
        self.free_extents.push(ext);
        self.normalize_free_extents();
    }

    fn alloc_extent(&mut self, need: u32) -> Result<FreeExtent> {
        if need == 0 {
            return Err(Error::Corrupt("alloc zero chunks".into()));
        }
        self.normalize_free_extents();
        for i in 0..self.free_extents.len() {
            if self.free_extents[i].n_chunks >= need {
                let start = self.free_extents[i].start;
                if self.free_extents[i].n_chunks == need {
                    self.free_extents.remove(i);
                } else {
                    self.free_extents[i].start += need;
                    self.free_extents[i].n_chunks -= need;
                }
                return Ok(FreeExtent {
                    start,
                    n_chunks: need,
                });
            }
        }
        // Grow data.bin
        let start = self.sb.chunk_count;
        let new_count = self
            .sb
            .chunk_count
            .checked_add(need)
            .ok_or(Error::StoreFull)?;
        if new_count > MAX_CHUNK_COUNT {
            return Err(Error::StoreFull);
        }
        self.sb.chunk_count = new_count;
        self.data
            .set_len(u64::from(new_count) * u64::from(CHUNK_SIZE))?;
        Ok(FreeExtent {
            start,
            n_chunks: need,
        })
    }

    fn alloc_entry_slot(&mut self) -> Result<u32> {
        let slot = self.sb.free_entry_head;
        if slot == NIL {
            return Err(Error::StoreFull);
        }
        let next = self.entries[slot as usize].next_free;
        self.sb.free_entry_head = next;
        Ok(slot)
    }

    fn entry_table_crc(&self) -> u32 {
        let mut crc = 0u32;
        for entry in &self.entries {
            crc = crc32c::crc32c_append(crc, &entry.encode());
        }
        crc
    }

    fn commit(&mut self) -> Result<()> {
        self.persist_free_extents()?;

        // Write superblock + dirty entries.
        let sb_buf = self.sb.encode();
        self.header.seek(SeekFrom::Start(0))?;
        self.header.write_all(&sb_buf)?;

        let dirty: Vec<u32> = self.dirty_slots.drain().collect();
        for slot in dirty {
            let buf = self.entries[slot as usize].encode();
            self.header.seek(SeekFrom::Start(entry_offset(slot)))?;
            self.header.write_all(&buf)?;
        }
        self.header.sync_all()?;

        self.sb.store_epoch = self.sb.store_epoch.saturating_add(1);
        let next_side = 1 - self.sb.active_side;
        let rec = CommitRecord {
            store_epoch: self.sb.store_epoch,
            active_side: next_side,
            entry_table_crc: self.entry_table_crc(),
            free_entry_head: self.sb.free_entry_head,
            free_chunk_head: self.sb.free_chunk_head,
            chunk_count: self.sb.chunk_count,
            flags: self.sb.flags & !FLAG_NEEDS_REPAIR,
        };
        let buf = rec.encode();
        self.header
            .seek(SeekFrom::Start(commit_offset(self.sb.entry_count, next_side)))?;
        self.header.write_all(&buf)?;
        self.header.sync_all()?;

        self.sb.active_side = next_side;
        self.sb.flags = rec.flags;

        // Keep primary superblock in sync with committed heads.
        let sb_buf = self.sb.encode();
        self.header.seek(SeekFrom::Start(0))?;
        self.header.write_all(&sb_buf)?;
        self.header.sync_all()?;
        Ok(())
    }

    fn repair(&mut self) -> Result<()> {
        // Rebuild free entry list and reclaim unreferenced chunks.
        let mut used = vec![false; self.sb.chunk_count as usize];
        self.name_index.clear();
        let mut free_head = NIL;
        for slot in (0..self.sb.entry_count).rev() {
            let entry = &mut self.entries[slot as usize];
            if entry.is_live() {
                let key = entry.name_bytes().to_vec();
                if self.name_index.insert(key, slot).is_some() {
                    // Duplicate: free later slot (we're iterating rev so keep first seen = higher slot freed)
                    *entry = Entry::free(free_head);
                    free_head = slot;
                    self.dirty_slots.insert(slot);
                    continue;
                }
                if entry.n_chunks > 0 {
                    let start = entry.start_chunk as usize;
                    let end = start + entry.n_chunks as usize;
                    if end > used.len() {
                        entry.n_chunks = 0;
                        entry.start_chunk = 0;
                        entry.size = 0;
                        self.dirty_slots.insert(slot);
                    } else {
                        for b in &mut used[start..end] {
                            *b = true;
                        }
                    }
                }
            } else {
                *entry = Entry::free(free_head);
                free_head = slot;
                self.dirty_slots.insert(slot);
            }
        }
        self.sb.free_entry_head = free_head;

        // Build free extents from unused runs.
        self.free_extents.clear();
        let mut i = 0u32;
        while i < self.sb.chunk_count {
            if used[i as usize] {
                i += 1;
                continue;
            }
            let start = i;
            while i < self.sb.chunk_count && !used[i as usize] {
                i += 1;
            }
            self.free_extents.push(FreeExtent {
                start,
                n_chunks: i - start,
            });
        }
        self.sb.flags &= !FLAG_NEEDS_REPAIR;
        self.commit()?;
        Ok(())
    }
}

fn meta_from_entry(entry: &Entry) -> Result<Meta> {
    Ok(Meta {
        name: entry.name_string()?,
        size: entry.size,
        ctime_ms: entry.ctime_ms,
        mtime_ms: entry.mtime_ms,
        atime_ms: entry.atime_ms,
        file_type: entry.file_type,
        content_hash: entry.content_hash,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn tmp_dir() -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("yfs-test-{nanos}-{seq}"));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn write_read_delete_rename() {
        let dir = tmp_dir();
        let store = Store::open_with_entry_count(&dir, 32).unwrap();

        store.write("notes/a.md", b"hello").unwrap();
        assert_eq!(store.read("notes/a.md").unwrap(), b"hello");

        let listed = store.list("notes/").unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name, "notes/a.md");

        store.rename("notes/a.md", "notes/b.md").unwrap();
        assert!(matches!(store.read("notes/a.md"), Err(Error::NotFound)));
        assert_eq!(store.read("notes/b.md").unwrap(), b"hello");

        store.write("notes/b.md", b"world!!").unwrap();
        assert_eq!(store.read("notes/b.md").unwrap(), b"world!!");

        store.delete("notes/b.md").unwrap();
        assert!(matches!(store.read("notes/b.md"), Err(Error::NotFound)));

        drop(store);
        let store2 = Store::open_with_entry_count(&dir, 32).unwrap();
        assert!(store2.list("").unwrap().is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn cow_overwrite_and_empty_file() {
        let dir = tmp_dir();
        let store = Store::open_with_entry_count(&dir, 16).unwrap();
        store.write("x", b"12345").unwrap();
        store.write("x", b"").unwrap();
        assert_eq!(store.read("x").unwrap(), b"");
        let meta = store.stat("x").unwrap();
        assert_eq!(meta.size, 0);
        assert_eq!(meta.content_hash, [0u8; 32]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn compact_repacks() {
        let dir = tmp_dir();
        let store = Store::open_with_entry_count(&dir, 16).unwrap();
        store.write("a", b"aaa").unwrap();
        store.write("b", b"bbbb").unwrap();
        store.delete("a").unwrap();
        store.compact().unwrap();
        assert_eq!(store.read("b").unwrap(), b"bbbb");
        drop(store);
        let store2 = Store::open_with_entry_count(&dir, 16).unwrap();
        assert_eq!(store2.read("b").unwrap(), b"bbbb");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_invalid_names() {
        let dir = tmp_dir();
        let store = Store::open_with_entry_count(&dir, 8).unwrap();
        assert!(matches!(
            store.write("a/../b", b"x"),
            Err(Error::InvalidName(_))
        ));
        let _ = fs::remove_dir_all(&dir);
    }
}

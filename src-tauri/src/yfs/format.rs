use super::error::{Error, Result};

pub const FORMAT_VERSION: u16 = 1;
pub const SUPERBLOCK_SIZE: u64 = 4096;
pub const ENTRY_SIZE: u32 = 512;
pub const CHUNK_SIZE: u32 = 4096;
pub const COMMIT_SIZE: u64 = 4096;
pub const DEFAULT_ENTRY_COUNT: u32 = 65_536;
pub const MAX_NAME_LEN: usize = 255;
pub const MAX_FILE_SIZE: u64 = 256 * 1024 * 1024;
pub const MAX_CHUNK_COUNT: u32 = 8_388_608;

pub const MAGIC_YFS1: u32 = u32::from_le_bytes(*b"YFS1");
pub const MAGIC_ENT1: u32 = u32::from_le_bytes(*b"ENT1");
pub const MAGIC_FREE: u32 = u32::from_le_bytes(*b"FREE");
pub const MAGIC_FEXT: u32 = u32::from_le_bytes(*b"FEXT");
pub const MAGIC_CMT1: u32 = u32::from_le_bytes(*b"CMT1");

pub const FLAG_NEEDS_REPAIR: u16 = 1;
pub const ENTRY_FLAG_TOMBSTONE: u16 = 1;
#[allow(dead_code)]
pub const ENTRY_FLAG_DIR_HINT: u16 = 2;

pub const FILE_TYPE_BLOB: u16 = 0;
pub const FILE_TYPE_TEXT: u16 = 1;
pub const FILE_TYPE_MARKDOWN: u16 = 2;

pub const NIL: u32 = u32::MAX;

#[derive(Debug, Clone)]
pub struct Superblock {
    pub magic: u32,
    pub format_version: u16,
    pub flags: u16,
    pub entry_size: u32,
    pub entry_count: u32,
    pub chunk_size: u32,
    pub chunk_count: u32,
    pub store_epoch: u64,
    pub free_entry_head: u32,
    pub free_chunk_head: u32,
    pub active_side: u32,
}

impl Superblock {
    pub fn new(entry_count: u32) -> Self {
        Self {
            magic: MAGIC_YFS1,
            format_version: FORMAT_VERSION,
            flags: 0,
            entry_size: ENTRY_SIZE,
            entry_count,
            chunk_size: CHUNK_SIZE,
            chunk_count: 0,
            store_epoch: 0,
            free_entry_head: 0,
            free_chunk_head: NIL,
            active_side: 0,
        }
    }

    pub fn encode(&self) -> [u8; SUPERBLOCK_SIZE as usize] {
        let mut buf = [0u8; SUPERBLOCK_SIZE as usize];
        buf[0..4].copy_from_slice(&self.magic.to_le_bytes());
        buf[4..6].copy_from_slice(&self.format_version.to_le_bytes());
        buf[6..8].copy_from_slice(&self.flags.to_le_bytes());
        buf[8..12].copy_from_slice(&self.entry_size.to_le_bytes());
        buf[12..16].copy_from_slice(&self.entry_count.to_le_bytes());
        buf[16..20].copy_from_slice(&self.chunk_size.to_le_bytes());
        buf[20..24].copy_from_slice(&self.chunk_count.to_le_bytes());
        buf[24..32].copy_from_slice(&self.store_epoch.to_le_bytes());
        buf[32..36].copy_from_slice(&self.free_entry_head.to_le_bytes());
        buf[36..40].copy_from_slice(&self.free_chunk_head.to_le_bytes());
        buf[40..44].copy_from_slice(&self.active_side.to_le_bytes());
        let checksum = crc32c::crc32c(&buf[0..44]);
        buf[44..48].copy_from_slice(&checksum.to_le_bytes());
        buf
    }

    pub fn decode(buf: &[u8]) -> Result<Self> {
        if buf.len() < 48 {
            return Err(Error::Corrupt("superblock too short".into()));
        }
        let checksum = u32::from_le_bytes(buf[44..48].try_into().unwrap());
        let expect = crc32c::crc32c(&buf[0..44]);
        if checksum != expect {
            return Err(Error::Corrupt("superblock checksum mismatch".into()));
        }
        let magic = u32::from_le_bytes(buf[0..4].try_into().unwrap());
        if magic != MAGIC_YFS1 {
            return Err(Error::Corrupt(format!("bad magic {magic:#x}")));
        }
        let format_version = u16::from_le_bytes(buf[4..6].try_into().unwrap());
        if format_version != FORMAT_VERSION {
            return Err(Error::Corrupt(format!(
                "unsupported format_version {format_version}"
            )));
        }
        let entry_size = u32::from_le_bytes(buf[8..12].try_into().unwrap());
        if entry_size != ENTRY_SIZE {
            return Err(Error::Corrupt(format!("bad entry_size {entry_size}")));
        }
        let chunk_size = u32::from_le_bytes(buf[16..20].try_into().unwrap());
        if chunk_size != CHUNK_SIZE {
            return Err(Error::Corrupt(format!("bad chunk_size {chunk_size}")));
        }
        Ok(Self {
            magic,
            format_version,
            flags: u16::from_le_bytes(buf[6..8].try_into().unwrap()),
            entry_size,
            entry_count: u32::from_le_bytes(buf[12..16].try_into().unwrap()),
            chunk_size,
            chunk_count: u32::from_le_bytes(buf[20..24].try_into().unwrap()),
            store_epoch: u64::from_le_bytes(buf[24..32].try_into().unwrap()),
            free_entry_head: u32::from_le_bytes(buf[32..36].try_into().unwrap()),
            free_chunk_head: u32::from_le_bytes(buf[36..40].try_into().unwrap()),
            active_side: u32::from_le_bytes(buf[40..44].try_into().unwrap()),
        })
    }
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub magic: u32,
    pub flags: u16,
    pub file_type: u16,
    pub size: u64,
    pub ctime_ms: u64,
    pub mtime_ms: u64,
    pub atime_ms: u64,
    pub content_hash: [u8; 32],
    pub start_chunk: u32,
    pub n_chunks: u32,
    pub next_free: u32,
    pub name_len: u32,
    pub name: [u8; 256],
}

impl Entry {
    pub fn free(next_free: u32) -> Self {
        Self {
            magic: MAGIC_FREE,
            flags: 0,
            file_type: 0,
            size: 0,
            ctime_ms: 0,
            mtime_ms: 0,
            atime_ms: 0,
            content_hash: [0; 32],
            start_chunk: 0,
            n_chunks: 0,
            next_free,
            name_len: 0,
            name: [0; 256],
        }
    }

    pub fn is_live(&self) -> bool {
        self.magic == MAGIC_ENT1 && self.flags & ENTRY_FLAG_TOMBSTONE == 0
    }

    #[allow(dead_code)]
    pub fn is_free(&self) -> bool {
        self.magic == MAGIC_FREE
    }

    pub fn name_bytes(&self) -> &[u8] {
        let len = self.name_len.min(256) as usize;
        &self.name[..len]
    }

    pub fn name_string(&self) -> Result<String> {
        String::from_utf8(self.name_bytes().to_vec())
            .map_err(|_| Error::Corrupt("entry name is not utf-8".into()))
    }

    pub fn set_name(&mut self, name: &[u8]) -> Result<()> {
        if name.is_empty() || name.len() > MAX_NAME_LEN {
            return Err(Error::InvalidName("bad length".into()));
        }
        self.name = [0; 256];
        self.name[..name.len()].copy_from_slice(name);
        self.name_len = name.len() as u32;
        Ok(())
    }

    pub fn encode(&self) -> [u8; ENTRY_SIZE as usize] {
        let mut buf = [0u8; ENTRY_SIZE as usize];
        buf[0..4].copy_from_slice(&self.magic.to_le_bytes());
        buf[4..6].copy_from_slice(&self.flags.to_le_bytes());
        buf[6..8].copy_from_slice(&self.file_type.to_le_bytes());
        buf[8..16].copy_from_slice(&self.size.to_le_bytes());
        buf[16..24].copy_from_slice(&self.ctime_ms.to_le_bytes());
        buf[24..32].copy_from_slice(&self.mtime_ms.to_le_bytes());
        buf[32..40].copy_from_slice(&self.atime_ms.to_le_bytes());
        buf[40..72].copy_from_slice(&self.content_hash);
        buf[72..76].copy_from_slice(&self.start_chunk.to_le_bytes());
        buf[76..80].copy_from_slice(&self.n_chunks.to_le_bytes());
        buf[80..84].copy_from_slice(&self.next_free.to_le_bytes());
        buf[84..88].copy_from_slice(&self.name_len.to_le_bytes());
        buf[88..344].copy_from_slice(&self.name);
        let crc = crc32c::crc32c(&buf[0..344]);
        buf[344..348].copy_from_slice(&crc.to_le_bytes());
        buf
    }

    pub fn decode(buf: &[u8]) -> Result<Self> {
        if buf.len() < ENTRY_SIZE as usize {
            return Err(Error::Corrupt("entry too short".into()));
        }
        let crc = u32::from_le_bytes(buf[344..348].try_into().unwrap());
        let expect = crc32c::crc32c(&buf[0..344]);
        if crc != expect {
            return Err(Error::Corrupt("entry checksum mismatch".into()));
        }
        let mut name = [0u8; 256];
        name.copy_from_slice(&buf[88..344]);
        let mut content_hash = [0u8; 32];
        content_hash.copy_from_slice(&buf[40..72]);
        Ok(Self {
            magic: u32::from_le_bytes(buf[0..4].try_into().unwrap()),
            flags: u16::from_le_bytes(buf[4..6].try_into().unwrap()),
            file_type: u16::from_le_bytes(buf[6..8].try_into().unwrap()),
            size: u64::from_le_bytes(buf[8..16].try_into().unwrap()),
            ctime_ms: u64::from_le_bytes(buf[16..24].try_into().unwrap()),
            mtime_ms: u64::from_le_bytes(buf[24..32].try_into().unwrap()),
            atime_ms: u64::from_le_bytes(buf[32..40].try_into().unwrap()),
            content_hash,
            start_chunk: u32::from_le_bytes(buf[72..76].try_into().unwrap()),
            n_chunks: u32::from_le_bytes(buf[76..80].try_into().unwrap()),
            next_free: u32::from_le_bytes(buf[80..84].try_into().unwrap()),
            name_len: u32::from_le_bytes(buf[84..88].try_into().unwrap()),
            name,
        })
    }
}

#[derive(Debug, Clone, Copy)]
pub struct FreeExtent {
    pub start: u32,
    pub n_chunks: u32,
}

impl FreeExtent {
    pub fn encode(&self, next: u32) -> [u8; 16] {
        let mut buf = [0u8; 16];
        buf[0..4].copy_from_slice(&MAGIC_FEXT.to_le_bytes());
        buf[4..8].copy_from_slice(&self.n_chunks.to_le_bytes());
        buf[8..12].copy_from_slice(&next.to_le_bytes());
        let crc = crc32c::crc32c(&buf[0..12]);
        buf[12..16].copy_from_slice(&crc.to_le_bytes());
        buf
    }

    pub fn decode(buf: &[u8], start: u32) -> Result<(Self, u32)> {
        if buf.len() < 16 {
            return Err(Error::Corrupt("free extent too short".into()));
        }
        let magic = u32::from_le_bytes(buf[0..4].try_into().unwrap());
        if magic != MAGIC_FEXT {
            return Err(Error::Corrupt(format!(
                "bad free extent magic at chunk {start}"
            )));
        }
        let crc = u32::from_le_bytes(buf[12..16].try_into().unwrap());
        let expect = crc32c::crc32c(&buf[0..12]);
        if crc != expect {
            return Err(Error::Corrupt(format!(
                "free extent crc mismatch at chunk {start}"
            )));
        }
        Ok((
            Self {
                start,
                n_chunks: u32::from_le_bytes(buf[4..8].try_into().unwrap()),
            },
            u32::from_le_bytes(buf[8..12].try_into().unwrap()),
        ))
    }
}

#[derive(Debug, Clone)]
pub struct CommitRecord {
    pub store_epoch: u64,
    pub active_side: u32,
    pub entry_table_crc: u32,
    pub free_entry_head: u32,
    pub free_chunk_head: u32,
    pub chunk_count: u32,
    pub flags: u16,
}

impl CommitRecord {
    pub fn encode(&self) -> [u8; COMMIT_SIZE as usize] {
        let mut buf = [0u8; COMMIT_SIZE as usize];
        buf[0..4].copy_from_slice(&MAGIC_CMT1.to_le_bytes());
        buf[4..12].copy_from_slice(&self.store_epoch.to_le_bytes());
        buf[12..16].copy_from_slice(&self.active_side.to_le_bytes());
        buf[16..20].copy_from_slice(&self.entry_table_crc.to_le_bytes());
        buf[20..24].copy_from_slice(&self.free_entry_head.to_le_bytes());
        buf[24..28].copy_from_slice(&self.free_chunk_head.to_le_bytes());
        buf[28..32].copy_from_slice(&self.chunk_count.to_le_bytes());
        buf[32..34].copy_from_slice(&self.flags.to_le_bytes());
        let checksum = crc32c::crc32c(&buf[0..34]);
        buf[34..38].copy_from_slice(&checksum.to_le_bytes());
        buf
    }

    pub fn decode(buf: &[u8]) -> Result<Self> {
        if buf.len() < 38 {
            return Err(Error::Corrupt("commit too short".into()));
        }
        let magic = u32::from_le_bytes(buf[0..4].try_into().unwrap());
        if magic != MAGIC_CMT1 {
            return Err(Error::Corrupt("bad commit magic".into()));
        }
        let checksum = u32::from_le_bytes(buf[34..38].try_into().unwrap());
        let expect = crc32c::crc32c(&buf[0..34]);
        if checksum != expect {
            return Err(Error::Corrupt("commit checksum mismatch".into()));
        }
        Ok(Self {
            store_epoch: u64::from_le_bytes(buf[4..12].try_into().unwrap()),
            active_side: u32::from_le_bytes(buf[12..16].try_into().unwrap()),
            entry_table_crc: u32::from_le_bytes(buf[16..20].try_into().unwrap()),
            free_entry_head: u32::from_le_bytes(buf[20..24].try_into().unwrap()),
            free_chunk_head: u32::from_le_bytes(buf[24..28].try_into().unwrap()),
            chunk_count: u32::from_le_bytes(buf[28..32].try_into().unwrap()),
            flags: u16::from_le_bytes(buf[32..34].try_into().unwrap()),
        })
    }
}

pub fn header_body_size(entry_count: u32) -> u64 {
    SUPERBLOCK_SIZE + u64::from(entry_count) * u64::from(ENTRY_SIZE)
}

pub fn header_file_size(entry_count: u32) -> u64 {
    header_body_size(entry_count) + COMMIT_SIZE * 2
}

pub fn entry_offset(slot: u32) -> u64 {
    SUPERBLOCK_SIZE + u64::from(slot) * u64::from(ENTRY_SIZE)
}

pub fn commit_offset(entry_count: u32, side: u32) -> u64 {
    header_body_size(entry_count) + u64::from(side) * COMMIT_SIZE
}

pub fn chunks_for_size(size: u64) -> u32 {
    if size == 0 {
        0
    } else {
        ((size + u64::from(CHUNK_SIZE) - 1) / u64::from(CHUNK_SIZE)) as u32
    }
}

pub fn content_hash(data: &[u8]) -> [u8; 32] {
    if data.is_empty() {
        [0u8; 32]
    } else {
        *blake3::hash(data).as_bytes()
    }
}

pub fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

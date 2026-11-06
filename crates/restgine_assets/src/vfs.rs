use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

const PAK_MAGIC: &[u8; 8] = b"RESTPAK1";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PakEntry {
    pub offset: u64,
    pub length: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PakHeader {
    pub entries: HashMap<String, PakEntry>,
}

/// Virtual File System reader for packed release assets on Steam.
pub struct VfsReader {
    file: File,
    header: PakHeader,
}

impl VfsReader {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, std::io::Error> {
        let mut file = File::open(path)?;
        let mut magic = [0u8; 8];
        file.read_exact(&mut magic)?;
        if &magic != PAK_MAGIC {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Invalid Restgine pak file magic",
            ));
        }

        let mut header_len_bytes = [0u8; 8];
        file.read_exact(&mut header_len_bytes)?;
        let header_len = u64::from_le_bytes(header_len_bytes);

        let mut header_buf = vec![0u8; header_len as usize];
        file.read_exact(&mut header_buf)?;

        let header: PakHeader = serde_json::from_slice(&header_buf)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;

        Ok(Self { file, header })
    }

    pub fn read_file(&mut self, path: &str) -> Result<Vec<u8>, std::io::Error> {
        let entry = self.header.entries.get(path).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("File not in pak: {}", path),
            )
        })?;

        self.file.seek(SeekFrom::Start(entry.offset))?;
        let mut buf = vec![0u8; entry.length as usize];
        self.file.read_exact(&mut buf)?;
        Ok(buf)
    }

    pub fn has_file(&self, path: &str) -> bool {
        self.header.entries.contains_key(path)
    }
}

/// Compiler utility to package a directory of assets into a `.pak` file for Steam distribution.
pub struct VfsPacker {
    entries: HashMap<String, (PathBuf, u64)>,
}

impl Default for VfsPacker {
    fn default() -> Self {
        Self::new()
    }
}

impl VfsPacker {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    pub fn add_file(&mut self, vfs_path: String, disk_path: PathBuf) {
        if let Ok(meta) = std::fs::metadata(&disk_path) {
            self.entries.insert(vfs_path, (disk_path, meta.len()));
        }
    }

    pub fn write_to_file<P: AsRef<Path>>(&self, output_path: P) -> Result<(), std::io::Error> {
        let mut out = File::create(output_path)?;
        out.write_all(PAK_MAGIC)?;

        // Build temporary header table with offsets calculated after header
        let mut entries_map = HashMap::new();
        // Placeholder header length
        out.write_all(&0u64.to_le_bytes())?;

        // First pass: collect files and estimate header size
        for (vfs_path, (_, len)) in &self.entries {
            entries_map.insert(
                vfs_path.clone(),
                PakEntry {
                    offset: 0,
                    length: *len,
                },
            );
        }

        let header_json = serde_json::to_vec(&PakHeader {
            entries: entries_map.clone(),
        })
        .unwrap();

        // Data starts right after magic (8) + header_len (8) + header json
        let mut current_offset = 8 + 8 + header_json.len() as u64;

        // Recalculate true file offsets
        for (vfs_path, (_, len)) in &self.entries {
            entries_map.insert(
                vfs_path.clone(),
                PakEntry {
                    offset: current_offset,
                    length: *len,
                },
            );
            current_offset += *len;
        }

        let finalized_header_json = serde_json::to_vec(&PakHeader {
            entries: entries_map,
        })
        .unwrap();

        // Write header length and json
        out.seek(SeekFrom::Start(8))?;
        out.write_all(&(finalized_header_json.len() as u64).to_le_bytes())?;
        out.write_all(&finalized_header_json)?;

        // Write file contents sequentially
        for (disk_path, _) in self.entries.values() {
            let mut file_data = File::open(disk_path)?;
            std::io::copy(&mut file_data, &mut out)?;
        }

        Ok(())
    }
}

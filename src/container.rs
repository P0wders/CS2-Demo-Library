//! The outer `.dem` container: a 16-byte header followed by length-prefixed,
//! optionally Snappy-compressed protobuf frames.
//!
//! ```text
//! "PBDEMS2\0" | u32 LE offset of DEM_FileInfo | u32 LE offset of DEM_SpawnGroups
//! frame*: varint cmd | varint tick | varint size | payload[size]
//!         cmd & 64  => payload is Snappy-compressed
//!         cmd & !64 => EDemoCommands
//!         tick == u32::MAX before the game starts (signon)
//! ```

use crate::error::{Error, Result, decode_proto};
use crate::proto::{CDemoFileHeader, CDemoFileInfo, EDemoCommands};

pub const MAGIC: &[u8; 8] = b"PBDEMS2\0";
const COMPRESSED_FLAG: u32 = 64;

/// One top-level demo frame. `payload` is still compressed when `compressed` is set.
#[derive(Debug, Clone, Copy)]
pub struct Frame<'a> {
    /// Raw command id (without the compression flag). See [`EDemoCommands`].
    pub cmd: u32,
    /// Game tick, or -1 for pre-game (signon) frames.
    pub tick: i32,
    pub compressed: bool,
    pub payload: &'a [u8],
    /// Byte offset of this frame in the file.
    pub offset: usize,
}

impl Frame<'_> {
    pub fn command(&self) -> Option<EDemoCommands> {
        EDemoCommands::try_from(self.cmd as i32).ok()
    }

    /// Decompresses the payload into `buf` if needed and returns the bytes.
    pub fn data<'b>(&'b self, buf: &'b mut Vec<u8>) -> Result<&'b [u8]> {
        if !self.compressed {
            return Ok(self.payload);
        }
        let len = snap::raw::decompress_len(self.payload)?;
        buf.resize(len, 0);
        snap::raw::Decoder::new().decompress(self.payload, buf)?;
        Ok(&buf[..])
    }
}

#[inline]
fn read_varint(data: &[u8], pos: &mut usize) -> Option<u32> {
    let mut result = 0u32;
    let mut shift = 0;
    loop {
        let b = *data.get(*pos)?;
        *pos += 1;
        result |= ((b & 0x7f) as u32) << shift;
        if b & 0x80 == 0 {
            return Some(result);
        }
        shift += 7;
        if shift >= 35 {
            return None;
        }
    }
}

/// Reads the frame starting at `pos`. Returns `Ok(None)` at a clean end of file.
pub fn read_frame<'a>(data: &'a [u8], pos: &mut usize) -> Result<Option<Frame<'a>>> {
    if *pos >= data.len() {
        return Ok(None);
    }
    let offset = *pos;
    let eof = || Error::UnexpectedEof("frame header");
    let raw_cmd = read_varint(data, pos).ok_or_else(eof)?;
    let tick = read_varint(data, pos).ok_or_else(eof)? as i32;
    let size = read_varint(data, pos).ok_or_else(eof)? as usize;
    let end = pos
        .checked_add(size)
        .filter(|&e| e <= data.len())
        .ok_or(Error::UnexpectedEof("frame payload"))?;
    let payload = &data[*pos..end];
    *pos = end;
    Ok(Some(Frame {
        cmd: raw_cmd & !COMPRESSED_FLAG,
        tick,
        compressed: raw_cmd & COMPRESSED_FLAG != 0,
        payload,
        offset,
    }))
}

/// Iterator over all frames after the 16-byte file header.
pub struct FrameIter<'a> {
    data: &'a [u8],
    pos: usize,
    done: bool,
}

impl<'a> FrameIter<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self::at(data, 16)
    }

    /// Iterates from the frame starting at byte `pos` (see [`Frame::offset`]).
    pub fn at(data: &'a [u8], pos: usize) -> Self {
        FrameIter {
            data,
            pos,
            done: false,
        }
    }
}

impl<'a> Iterator for FrameIter<'a> {
    type Item = Result<Frame<'a>>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        match read_frame(self.data, &mut self.pos) {
            Ok(Some(f)) => Some(Ok(f)),
            Ok(None) => {
                self.done = true;
                None
            }
            Err(e) => {
                self.done = true;
                Some(Err(e))
            }
        }
    }
}

/// Information available without parsing the whole demo.
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DemoHeader {
    pub map_name: String,
    pub server_name: String,
    pub client_name: String,
    pub game_directory: String,
    pub demo_version_name: String,
    pub demo_version_guid: String,
    pub patch_version: i32,
    pub build_num: i32,
    pub server_start_tick: i32,
    /// From DEM_FileInfo (written at the end of the recording). `None` if the
    /// recording was cut short and the footer is missing.
    pub playback_time: Option<f32>,
    pub playback_ticks: Option<i32>,
    pub playback_frames: Option<i32>,
}

impl DemoHeader {
    /// Ticks per second, derived from the footer (CS2 demos are 64 tick).
    pub fn tick_rate(&self) -> f32 {
        match (self.playback_ticks, self.playback_time) {
            (Some(t), Some(s)) if s > 0.0 => (t as f32 / s).round(),
            _ => 64.0,
        }
    }
}

/// Validates the magic and reads DEM_FileHeader (first frame) plus DEM_FileInfo
/// (located through the offset in the file header).
pub fn read_header(data: &[u8]) -> Result<DemoHeader> {
    if data.len() < 16 {
        return Err(Error::UnexpectedEof("file header"));
    }
    let magic: [u8; 8] = data[..8].try_into().unwrap();
    if &magic != MAGIC {
        return Err(Error::BadMagic(magic));
    }
    let file_info_offset = u32::from_le_bytes(data[8..12].try_into().unwrap()) as usize;

    let mut buf = Vec::new();
    let mut pos = 16;
    let first = read_frame(data, &mut pos)?.ok_or(Error::UnexpectedEof("DEM_FileHeader"))?;
    if first.cmd != EDemoCommands::DemFileHeader as u32 {
        return Err(Error::malformed(format!(
            "first frame is command {} (expected DEM_FileHeader)",
            first.cmd
        )));
    }
    let fh: CDemoFileHeader = decode_proto("CDemoFileHeader", first.data(&mut buf)?)?;

    let mut header = DemoHeader {
        map_name: fh.map_name.unwrap_or_default(),
        server_name: fh.server_name.unwrap_or_default(),
        client_name: fh.client_name.unwrap_or_default(),
        game_directory: fh.game_directory.unwrap_or_default(),
        demo_version_name: fh.demo_version_name.unwrap_or_default(),
        demo_version_guid: fh.demo_version_guid.unwrap_or_default(),
        patch_version: fh.patch_version.unwrap_or_default(),
        build_num: fh.build_num.unwrap_or_default(),
        server_start_tick: fh.server_start_tick.unwrap_or_default(),
        ..Default::default()
    };

    // The footer may be absent (offset 0 or out of range) for truncated demos.
    if file_info_offset >= 16 && file_info_offset < data.len() {
        let mut pos = file_info_offset;
        if let Ok(Some(frame)) = read_frame(data, &mut pos)
            && frame.cmd == EDemoCommands::DemFileInfo as u32
            && let Ok(info) = decode_proto::<CDemoFileInfo>("CDemoFileInfo", frame.data(&mut buf)?)
        {
            header.playback_time = info.playback_time;
            header.playback_ticks = info.playback_ticks;
            header.playback_frames = info.playback_frames;
        }
    }
    Ok(header)
}

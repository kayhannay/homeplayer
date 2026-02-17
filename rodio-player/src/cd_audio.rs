//! CD audio reading module using Linux CDROM ioctls.
//!
//! This module provides functionality to:
//! - Read the Table of Contents (TOC) from an audio CD
//! - Stream raw PCM audio data from CD tracks via a [`rodio::Source`] implementation
//! - Eject the CD tray
//!
//! It uses the Linux kernel's CDROM ioctl interface directly (via `libc`),
//! so no external libraries like `libcdio` are required at runtime.
//!
//! **Important:** The CD device is always opened with `O_NONBLOCK` so that the
//! `open()` call does not block waiting for the drive to become ready (e.g.
//! when the tray is open or no disc is inserted).  This matches the behaviour
//! of the standard `eject` command.

use std::fs::{File, OpenOptions};
use std::io;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::io::AsRawFd;
use std::time::Duration;

use anyhow::{Context, Error, anyhow};
use tracing::{debug, error};

// ---------------------------------------------------------------------------
// Linux CDROM ioctl constants (from <linux/cdrom.h>)
// ---------------------------------------------------------------------------

/// Read TOC header: yields first and last track numbers.
const CDROMREADTOCHDR: libc::c_ulong = 0x5305;
/// Read a single TOC entry (track start position, type, etc.).
const CDROMREADTOCENTRY: libc::c_ulong = 0x5306;
/// Read raw audio sectors from the disc.
const CDROMREADAUDIO: libc::c_ulong = 0x530e;
/// Eject the CD tray.
const CDROMEJECT: libc::c_ulong = 0x5309;
/// Lock or unlock the drive door (0 = unlock, 1 = lock).
const CDROM_LOCKDOOR: libc::c_ulong = 0x5329;
/// Check drive status.
const CDROM_DRIVE_STATUS: libc::c_ulong = 0x5326;

/// Address format: Logical Block Address.
const CDROM_LBA: u8 = 0x01;

/// Pseudo-track number representing the lead-out area (end of disc).
const CDROM_LEADOUT: u8 = 0xAA;

/// Number of CD sectors (frames) per second (Red Book standard).
const CD_FRAMES_PER_SECOND: i32 = 75;

/// Size of one raw audio sector in bytes (2352 = 588 stereo 16-bit frames).
const SECTOR_SIZE: usize = 2352;

/// Number of 16-bit samples in one sector (2352 / 2).
const SAMPLES_PER_SECTOR: usize = SECTOR_SIZE / 2;

/// How many sectors to read per ioctl call.
/// 25 sectors ≈ 1/3 second of audio – a good trade-off between latency and
/// system-call overhead.
const SECTORS_PER_READ: i32 = 25;

/// CD audio sample rate (Red Book).
const CD_SAMPLE_RATE: u32 = 44_100;

/// CD audio channel count (stereo).
const CD_CHANNELS: u16 = 2;

/// Drive status: disc is present and the tray is closed.
const CDS_DISC_OK: libc::c_int = 4;

// ---------------------------------------------------------------------------
// C-compatible structs for the CDROM ioctls (repr(C))
// ---------------------------------------------------------------------------

/// TOC header – contains first and last track numbers.
#[repr(C)]
#[derive(Debug, Default)]
struct CdromTocHdr {
    cdth_trk0: u8,
    cdth_trk1: u8,
}

/// A single TOC entry for one track.
///
/// The kernel header defines `cdte_adr` and `cdte_ctrl` as 4-bit bitfields
/// packed into a single byte.  We store them as one `u8` and extract manually.
#[repr(C)]
#[derive(Debug, Default)]
struct CdromTocEntry {
    /// Track number to query (input).
    cdte_track: u8,
    /// Packed `adr` (low nibble) and `ctrl` (high nibble).
    cdte_adr_ctrl: u8,
    /// Address format: [`CDROM_LBA`] or MSF (input).
    cdte_format: u8,
    /// Track start address in LBA (output). Largest union member is `i32`.
    cdte_addr_lba: i32,
    /// Data mode (unused for audio tracks).
    cdte_datamode: u8,
}

/// Parameters for the `CDROMREADAUDIO` ioctl.
///
/// Uses `#[repr(C)]` so the compiler inserts the correct alignment padding
/// on both 32-bit and 64-bit targets automatically.
#[repr(C)]
struct CdromReadAudio {
    /// Starting LBA address.
    addr_lba: i32,
    /// Address format ([`CDROM_LBA`]).
    addr_format: u8,
    /// Number of 2352-byte sectors to read.
    nframes: i32,
    /// Pointer to the output buffer.
    buf: *mut u8,
}

// ---------------------------------------------------------------------------
// Public data types
// ---------------------------------------------------------------------------

/// Information about a single CD track, obtained from the TOC.
#[derive(Clone, Debug)]
pub struct CdTrackInfo {
    /// Track number (1-based).
    pub number: u8,
    /// First sector of the track (LBA).
    pub start_lba: i32,
    /// Past-the-end sector of the track (LBA, exclusive).
    pub end_lba: i32,
    /// Duration of the track.
    pub duration: Duration,
    /// `true` if this is an audio track, `false` for data tracks.
    pub is_audio: bool,
}

impl CdTrackInfo {
    /// Number of sectors in this track.
    pub fn sector_count(&self) -> i32 {
        self.end_lba - self.start_lba
    }

    /// Format the duration as `MM:SS`.
    pub fn duration_display(&self) -> String {
        let total_secs = self.duration.as_secs();
        let mins = total_secs / 60;
        let secs = total_secs % 60;
        format!("{mins}:{secs:02}")
    }
}

/// Overall information about an audio CD.
#[derive(Clone, Debug)]
pub struct CdInfo {
    /// First track number on the disc (usually 1).
    pub first_track: u8,
    /// Last track number on the disc.
    pub last_track: u8,
    /// Information for each track.
    pub tracks: Vec<CdTrackInfo>,
}

impl CdInfo {
    /// Return only the audio tracks.
    pub fn audio_tracks(&self) -> Vec<&CdTrackInfo> {
        self.tracks.iter().filter(|t| t.is_audio).collect()
    }

    /// Total duration of all audio tracks.
    pub fn total_duration(&self) -> Duration {
        self.tracks
            .iter()
            .filter(|t| t.is_audio)
            .map(|t| t.duration)
            .sum()
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Open the CD device with `O_RDONLY | O_NONBLOCK`.
///
/// `O_NONBLOCK` is essential on Linux: without it the kernel tries to read the
/// disc during `open()`, which blocks (or fails) when the tray is open, the
/// drive is still spinning up, or no medium is inserted.  Every standard tool
/// (e.g. `eject`, `cdparanoia`, `wodim`) opens the device this way.
fn open_cd_device(device: &str) -> Result<File, Error> {
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(device)
        .with_context(|| format!("Failed to open CD device '{device}'"))
}

/// Read a single TOC entry for the given track number.
fn read_toc_entry(fd: libc::c_int, track_num: u8) -> Result<CdromTocEntry, Error> {
    let mut entry = CdromTocEntry {
        cdte_track: track_num,
        cdte_adr_ctrl: 0,
        cdte_format: CDROM_LBA,
        cdte_addr_lba: 0,
        cdte_datamode: 0,
    };

    let ret = unsafe { libc::ioctl(fd, CDROMREADTOCENTRY, &mut entry as *mut CdromTocEntry) };
    if ret < 0 {
        return Err(anyhow!(
            "CDROMREADTOCENTRY ioctl failed for track {track_num}: {}",
            io::Error::last_os_error()
        ));
    }
    Ok(entry)
}

// ---------------------------------------------------------------------------
// Public functions
// ---------------------------------------------------------------------------

/// Check whether a disc is present in the given CD drive.
pub fn is_disc_present(device: &str) -> bool {
    let file = match open_cd_device(device) {
        Ok(f) => f,
        Err(_) => return false,
    };
    let fd = file.as_raw_fd();
    let status = unsafe { libc::ioctl(fd, CDROM_DRIVE_STATUS, 0 as libc::c_int) };
    status == CDS_DISC_OK
}

/// Read the Table of Contents from the CD in the given device.
///
/// Returns a [`CdInfo`] with information about every track on the disc.
pub fn read_cd_toc(device: &str) -> Result<CdInfo, Error> {
    let file = open_cd_device(device)?;
    let fd = file.as_raw_fd();

    // 1. Read the TOC header to get first/last track numbers.
    let mut toc_hdr = CdromTocHdr::default();
    let ret = unsafe { libc::ioctl(fd, CDROMREADTOCHDR, &mut toc_hdr as *mut CdromTocHdr) };
    if ret < 0 {
        return Err(anyhow!(
            "CDROMREADTOCHDR ioctl failed: {}",
            io::Error::last_os_error()
        ));
    }

    let first_track = toc_hdr.cdth_trk0;
    let last_track = toc_hdr.cdth_trk1;
    debug!("CD TOC: tracks {first_track}–{last_track}");

    // 2. Read each TOC entry (tracks + lead-out) to collect LBA positions.
    struct RawEntry {
        number: u8,
        start_lba: i32,
        is_audio: bool,
    }

    let mut entries: Vec<RawEntry> = Vec::new();

    for track_num in first_track..=last_track {
        let entry = read_toc_entry(fd, track_num)?;
        let ctrl = (entry.cdte_adr_ctrl >> 4) & 0x0F;
        let is_audio = ctrl & 0x04 == 0;
        debug!(
            "  Track {track_num}: LBA={}, audio={}",
            entry.cdte_addr_lba, is_audio
        );
        entries.push(RawEntry {
            number: track_num,
            start_lba: entry.cdte_addr_lba,
            is_audio,
        });
    }

    // Read lead-out to know where the last track ends.
    let leadout = read_toc_entry(fd, CDROM_LEADOUT)?;
    let leadout_lba = leadout.cdte_addr_lba;
    debug!("  Lead-out: LBA={leadout_lba}");

    // 3. Build CdTrackInfo for each track.
    let mut tracks: Vec<CdTrackInfo> = Vec::new();
    for (i, raw) in entries.iter().enumerate() {
        let end_lba = if i + 1 < entries.len() {
            entries[i + 1].start_lba
        } else {
            leadout_lba
        };
        let sector_count = end_lba - raw.start_lba;
        let duration_secs = sector_count as f64 / CD_FRAMES_PER_SECOND as f64;

        tracks.push(CdTrackInfo {
            number: raw.number,
            start_lba: raw.start_lba,
            end_lba,
            duration: Duration::from_secs_f64(duration_secs),
            is_audio: raw.is_audio,
        });
    }

    Ok(CdInfo {
        first_track,
        last_track,
        tracks,
    })
}

/// Eject the CD tray.
///
/// The drive door is typically locked by the OS when a disc is detected.
/// We must unlock it first with `CDROM_LOCKDOOR` before the `CDROMEJECT`
/// ioctl will physically open the tray.
pub fn eject_cd(device: &str) -> Result<(), Error> {
    let file = open_cd_device(device)?;
    let fd = file.as_raw_fd();

    // Unlock the drive door (0 = unlock).
    let ret = unsafe { libc::ioctl(fd, CDROM_LOCKDOOR, 0 as libc::c_int) };
    if ret < 0 {
        debug!(
            "CDROM_LOCKDOOR unlock failed (non-fatal): {}",
            io::Error::last_os_error()
        );
    }

    let ret = unsafe { libc::ioctl(fd, CDROMEJECT) };
    if ret < 0 {
        return Err(anyhow!(
            "CDROMEJECT ioctl failed: {}",
            io::Error::last_os_error()
        ));
    }
    debug!("CD tray ejected");
    Ok(())
}

/// Create a [`CdTrackSource`] that streams PCM audio for the given track.
pub fn open_track(device: &str, track: &CdTrackInfo) -> Result<CdTrackSource, Error> {
    if !track.is_audio {
        return Err(anyhow!("Track {} is not an audio track", track.number));
    }
    CdTrackSource::new(device, track.start_lba, track.end_lba)
}

// ---------------------------------------------------------------------------
// CdTrackSource – implements rodio::Source
// ---------------------------------------------------------------------------

/// A [`rodio::Source`] that reads raw 16-bit PCM audio directly from a CD track.
///
/// Audio CDs store data as 44 100 Hz, 16-bit signed, stereo PCM – which is
/// exactly what rodio expects, so no transcoding is necessary.
pub struct CdTrackSource {
    /// Open file handle to the CD device.
    file: File,
    /// Next sector to read from disc.
    current_lba: i32,
    /// Past-the-end sector for this track.
    end_lba: i32,
    /// Reusable byte buffer for raw sector reads (avoids per-call allocation).
    raw_buf: Vec<u8>,
    /// Buffer of decoded i16 samples.
    buffer: Vec<i16>,
    /// Current read position within `buffer`.
    buffer_pos: usize,
    /// Total number of samples in the entire track.
    total_samples: usize,
    /// Number of samples yielded so far via the iterator.
    samples_yielded: usize,
}

impl CdTrackSource {
    fn new(device: &str, start_lba: i32, end_lba: i32) -> Result<Self, Error> {
        let file = open_cd_device(device)?;

        let sector_count = (end_lba - start_lba) as usize;
        let total_samples = sector_count * SAMPLES_PER_SECTOR;

        debug!(
            "CdTrackSource: LBA {start_lba}–{end_lba} ({sector_count} sectors, {total_samples} samples)"
        );

        // Pre-allocate the raw byte buffer for the maximum read size so
        // `fill_buffer` never has to allocate on the heap.
        let max_bytes = SECTORS_PER_READ as usize * SECTOR_SIZE;

        Ok(Self {
            file,
            current_lba: start_lba,
            end_lba,
            raw_buf: vec![0u8; max_bytes],
            buffer: Vec::new(),
            buffer_pos: 0,
            total_samples,
            samples_yielded: 0,
        })
    }

    /// Fill the internal buffer by reading the next batch of sectors from disc.
    /// Returns `true` if samples were read, `false` if the track is finished.
    fn fill_buffer(&mut self) -> bool {
        if self.current_lba >= self.end_lba {
            return false;
        }

        let remaining_sectors = self.end_lba - self.current_lba;
        let sectors_to_read = remaining_sectors.min(SECTORS_PER_READ);

        let byte_count = sectors_to_read as usize * SECTOR_SIZE;
        self.raw_buf.resize(byte_count, 0u8);

        match read_audio_sectors(
            &self.file,
            self.current_lba,
            sectors_to_read,
            &mut self.raw_buf[..byte_count],
        ) {
            Ok(()) => {
                self.current_lba += sectors_to_read;

                // Convert raw bytes to i16 samples (little-endian on Linux).
                let sample_count = byte_count / 2;
                self.buffer.clear();
                self.buffer.reserve(sample_count);
                for chunk in self.raw_buf[..byte_count].chunks_exact(2) {
                    let sample = i16::from_le_bytes([chunk[0], chunk[1]]);
                    self.buffer.push(sample);
                }
                self.buffer_pos = 0;
                true
            }
            Err(e) => {
                // On read error, log and skip ahead by the failed sectors to
                // avoid getting stuck in a loop on a scratched region.
                error!(
                    "CD read error at LBA {}: {e} – skipping {sectors_to_read} sectors",
                    self.current_lba
                );
                self.current_lba += sectors_to_read;

                // Insert silence for the skipped sectors so playback continues.
                let sample_count = sectors_to_read as usize * SAMPLES_PER_SECTOR;
                self.buffer.clear();
                self.buffer.resize(sample_count, 0i16);
                self.buffer_pos = 0;
                true
            }
        }
    }
}

/// Perform the `CDROMREADAUDIO` ioctl on the given CD device file.
fn read_audio_sectors(
    file: &File,
    start_lba: i32,
    nframes: i32,
    buf: &mut [u8],
) -> Result<(), Error> {
    let mut read_cmd = CdromReadAudio {
        addr_lba: start_lba,
        addr_format: CDROM_LBA,
        nframes,
        buf: buf.as_mut_ptr(),
    };

    let fd = file.as_raw_fd();
    let ret = unsafe { libc::ioctl(fd, CDROMREADAUDIO, &mut read_cmd as *mut CdromReadAudio) };
    if ret < 0 {
        return Err(anyhow!(
            "CDROMREADAUDIO ioctl failed: {}",
            io::Error::last_os_error()
        ));
    }
    Ok(())
}

impl Iterator for CdTrackSource {
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        // Check if we've yielded all samples for the track.
        if self.samples_yielded >= self.total_samples {
            return None;
        }

        // Refill the buffer when exhausted.
        if self.buffer_pos >= self.buffer.len() {
            if !self.fill_buffer() {
                return None;
            }
        }

        let sample = self.buffer[self.buffer_pos];
        self.buffer_pos += 1;
        self.samples_yielded += 1;
        // Convert i16 sample to f32 in the range [-1.0, 1.0]
        Some(sample as f32 / i16::MAX as f32)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.total_samples.saturating_sub(self.samples_yielded);
        (remaining, Some(remaining))
    }
}

impl rodio::Source for CdTrackSource {
    fn current_span_len(&self) -> Option<usize> {
        // Return the number of frames remaining in the current buffer, or None
        // if we don't know (which tells rodio to just keep polling).
        let samples_in_buffer = self.buffer.len().saturating_sub(self.buffer_pos);
        if samples_in_buffer > 0 {
            // A frame is `channels` samples.
            Some(samples_in_buffer / CD_CHANNELS as usize)
        } else {
            None
        }
    }

    fn channels(&self) -> u16 {
        CD_CHANNELS
    }

    fn sample_rate(&self) -> u32 {
        CD_SAMPLE_RATE
    }

    fn total_duration(&self) -> Option<Duration> {
        let total_frames = self.total_samples / CD_CHANNELS as usize;
        let secs = total_frames as f64 / CD_SAMPLE_RATE as f64;
        Some(Duration::from_secs_f64(secs))
    }
}

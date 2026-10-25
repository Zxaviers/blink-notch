//! The clipboard, read-only and only when asked: Ctrl+V while a page is being typed into, and the shelf's paste button (and, when it
//! is switched on, the images that are copied).

use windows::core::w;
use windows::Win32::Foundation::{HGLOBAL, HWND};
use windows::Win32::System::DataExchange::{CloseClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard, RegisterClipboardFormatW};
use windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
use windows::Win32::UI::Shell::{DragQueryFileW, HDROP};

/// CF_UNICODETEXT.
const UNICODE_TEXT: u32 = 13;
const CF_DIB: u32 = 8;
const CF_HDROP: u32 = 15;
const CF_DIBV5: u32 = 17;
/// More than this is not a task title: cut it.
const MAX_CHARS: usize = 2000;
/// A picture larger than this is not put on a shelf.
const MAX_IMAGE_BYTES: usize = 64 * 1024 * 1024;

/// The clipboard's text, if it holds text.
pub fn read_text() -> Option<String> {
    unsafe {
        OpenClipboard(None).ok()?;
        let text = (|| {
            let handle = GetClipboardData(UNICODE_TEXT).ok()?;
            let memory = HGLOBAL(handle.0);
            let ptr = GlobalLock(memory) as *const u16;
            if ptr.is_null() {
                return None;
            }
            let mut len = 0;
            while len < MAX_CHARS && *ptr.add(len) != 0 {
                len += 1;
            }
            let text = String::from_utf16_lossy(std::slice::from_raw_parts(ptr, len));
            let _ = GlobalUnlock(memory);
            Some(text)
        })();
        let _ = CloseClipboard();
        text
    }
}

/// What the clipboard holds that the shelf can keep.
#[derive(Debug, PartialEq)]
pub enum Clip {
    /// Files or folders that were copied (in Explorer, say): their paths.
    Files(Vec<String>),
    /// A picture (a screenshot, an image copied in a browser), as a PNG file's bytes.
    Image(Vec<u8>),
    Text(String),
    Nothing,
}

/// Open the clipboard, trying again a few times: whoever just put something on it may still be holding it.
unsafe fn open_clipboard(owner: Option<HWND>) -> bool {
    for _ in 0..6 {
        if OpenClipboard(owner).is_ok() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(30));
    }
    false
}

/// The bytes of a global memory block the clipboard gave out (copied, so the clipboard can be closed).
unsafe fn bytes_of(handle: HGLOBAL) -> Option<Vec<u8>> {
    let size = GlobalSize(handle);
    if size == 0 || size > MAX_IMAGE_BYTES {
        return None;
    }
    let ptr = GlobalLock(handle) as *const u8;
    if ptr.is_null() {
        return None;
    }
    let bytes = std::slice::from_raw_parts(ptr, size).to_vec();
    let _ = GlobalUnlock(handle);
    Some(bytes)
}

/// What the clipboard held, before any conversion (the clipboard is closed again as soon as this is taken, so whoever put something
/// on it is not kept waiting).
enum Raw {
    Files(Vec<String>),
    Png(Vec<u8>),
    Dib(Vec<u8>),
    Text(String),
    Nothing,
}

/// What is on the clipboard now, for the shelf. `images_only` ignores files and text (the listener that takes copied pictures).
pub fn read_for_shelf(images_only: bool) -> Clip {
    let raw = unsafe {
        if !open_clipboard(None) {
            return Clip::Nothing;
        }
        let png_format = RegisterClipboardFormatW(w!("PNG"));
        let raw = (|| {
            if !images_only && IsClipboardFormatAvailable(CF_HDROP).is_ok() {
                if let Ok(handle) = GetClipboardData(CF_HDROP) {
                    let drop = HDROP(handle.0);
                    let count = DragQueryFileW(drop, u32::MAX, None);
                    let mut paths = Vec::new();
                    for i in 0..count.min(60) {
                        let len = DragQueryFileW(drop, i, None) as usize;
                        let mut buf = vec![0u16; len + 1];
                        let n = DragQueryFileW(drop, i, Some(&mut buf)) as usize;
                        paths.push(String::from_utf16_lossy(&buf[..n]));
                    }
                    if !paths.is_empty() {
                        return Raw::Files(paths);
                    }
                }
            }
            // A picture: a PNG as it is, else the bitmap Windows keeps for every copied picture.
            if png_format != 0 && IsClipboardFormatAvailable(png_format).is_ok() {
                if let Some(bytes) = GetClipboardData(png_format).ok().and_then(|h| bytes_of(HGLOBAL(h.0))) {
                    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
                        return Raw::Png(bytes);
                    }
                }
            }
            for format in [CF_DIBV5, CF_DIB] {
                if IsClipboardFormatAvailable(format).is_ok() {
                    if let Some(dib) = GetClipboardData(format).ok().and_then(|h| bytes_of(HGLOBAL(h.0))) {
                        return Raw::Dib(dib);
                    }
                }
            }
            if !images_only && IsClipboardFormatAvailable(UNICODE_TEXT).is_ok() {
                if let Ok(handle) = GetClipboardData(UNICODE_TEXT) {
                    let memory = HGLOBAL(handle.0);
                    let ptr = GlobalLock(memory) as *const u16;
                    if !ptr.is_null() {
                        let mut len = 0;
                        while len < 200_000 && *ptr.add(len) != 0 {
                            len += 1;
                        }
                        let text = String::from_utf16_lossy(std::slice::from_raw_parts(ptr, len));
                        let _ = GlobalUnlock(memory);
                        if !text.trim().is_empty() {
                            return Raw::Text(text);
                        }
                    }
                }
            }
            Raw::Nothing
        })();
        let _ = CloseClipboard();
        raw
    };
    match raw {
        Raw::Files(paths) => Clip::Files(paths),
        Raw::Png(bytes) => Clip::Image(bytes),
        Raw::Dib(dib) => dib_to_png(dib).map_or(Clip::Nothing, Clip::Image),
        Raw::Text(text) => Clip::Text(text),
        Raw::Nothing => Clip::Nothing,
    }
}

/// A device-independent bitmap (what the clipboard keeps of a picture) as the bytes of a BMP file.
fn dib_to_bmp(dib: &[u8]) -> Option<Vec<u8>> {
    let u32_at = |i: usize| dib.get(i..i + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
    let header = u32_at(0)? as usize;
    if header < 40 || dib.len() < header {
        return None;
    }
    let bits = u16::from_le_bytes([dib[14], dib[15]]);
    let compression = u32_at(16)?;
    let used = u32_at(32)? as usize;
    let masks = if compression == 3 && header == 40 { 12 } else { 0 };
    let palette = if bits <= 8 { (if used == 0 { 1usize << bits } else { used }) * 4 } else { used * 4 };
    let offset = 14 + header + masks + palette;
    let mut out = Vec::with_capacity(14 + dib.len());
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&((14 + dib.len()) as u32).to_le_bytes());
    out.extend_from_slice(&[0, 0, 0, 0]);
    out.extend_from_slice(&(offset as u32).to_le_bytes());
    out.extend_from_slice(dib);
    Some(out)
}

/// A bitmap off the clipboard as the bytes of a PNG file, made by Windows' own imaging (on a thread of its own, since it waits).
fn dib_to_png(dib: Vec<u8>) -> Option<Vec<u8>> {
    let bmp = dib_to_bmp(&dib)?;
    std::thread::Builder::new().name("blink-clip-png".into()).spawn(move || convert(&bmp)).ok()?.join().ok()?
}

fn convert(bmp: &[u8]) -> Option<Vec<u8>> {
    use windows::Graphics::Imaging::{BitmapAlphaMode, BitmapDecoder, BitmapEncoder, BitmapPixelFormat};
    use windows::Storage::Streams::{DataReader, DataWriter, InMemoryRandomAccessStream};
    use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};
    unsafe {
        let _ = RoInitialize(RO_INIT_MULTITHREADED);
    }
    let input = InMemoryRandomAccessStream::new().ok()?;
    let writer = DataWriter::CreateDataWriter(&input).ok()?;
    writer.WriteBytes(bmp).ok()?;
    writer.StoreAsync().ok()?.join().ok()?;
    let _ = writer.DetachStream();
    input.Seek(0).ok()?;
    let decoder = BitmapDecoder::CreateAsync(&input).ok()?.join().ok()?;
    // The alpha of a copied picture is often zero throughout: it is a picture of the screen, so it is opaque.
    let bitmap = decoder.GetSoftwareBitmapConvertedAsync(BitmapPixelFormat::Bgra8, BitmapAlphaMode::Ignore).ok()?.join().ok()?;
    let output = InMemoryRandomAccessStream::new().ok()?;
    let encoder = BitmapEncoder::CreateAsync(BitmapEncoder::PngEncoderId().ok()?, &output).ok()?.join().ok()?;
    encoder.SetSoftwareBitmap(&bitmap).ok()?;
    encoder.FlushAsync().ok()?.join().ok()?;
    let size = output.Size().ok()? as u32;
    let reader = DataReader::CreateDataReader(&output.GetInputStreamAt(0).ok()?).ok()?;
    reader.LoadAsync(size).ok()?.join().ok()?;
    let mut bytes = vec![0u8; size as usize];
    reader.ReadBytes(&mut bytes).ok()?;
    (bytes.len() > 8).then_some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bitmap_gets_the_header_of_a_bmp_file_with_the_pixels_where_they_start() {
        // A 1x1, 24-bit bitmap: a 40-byte header and one padded pixel row.
        let mut dib = vec![0u8; 40 + 4];
        dib[0..4].copy_from_slice(&40u32.to_le_bytes());
        dib[4..8].copy_from_slice(&1u32.to_le_bytes());
        dib[8..12].copy_from_slice(&1u32.to_le_bytes());
        dib[12..14].copy_from_slice(&1u16.to_le_bytes());
        dib[14..16].copy_from_slice(&24u16.to_le_bytes());
        let bmp = dib_to_bmp(&dib).unwrap();
        assert_eq!(&bmp[0..2], b"BM");
        assert_eq!(u32::from_le_bytes([bmp[2], bmp[3], bmp[4], bmp[5]]) as usize, bmp.len());
        assert_eq!(u32::from_le_bytes([bmp[10], bmp[11], bmp[12], bmp[13]]), 54, "pixels start after both headers");
        // 8-bit: a palette of 256 entries sits before the pixels; bit fields add three masks.
        dib[14..16].copy_from_slice(&8u16.to_le_bytes());
        assert_eq!(u32::from_le_bytes([dib_to_bmp(&dib).unwrap()[10], 0, 0, 0]) as usize & 0xff, (54 + 1024) & 0xff);
        let mut fields = vec![0u8; 40 + 12 + 4];
        fields[0..4].copy_from_slice(&40u32.to_le_bytes());
        fields[14..16].copy_from_slice(&32u16.to_le_bytes());
        fields[16..20].copy_from_slice(&3u32.to_le_bytes());
        assert_eq!(u32::from_le_bytes(dib_to_bmp(&fields).unwrap()[10..14].try_into().unwrap()), 14 + 40 + 12);
        assert!(dib_to_bmp(&[0u8; 10]).is_none());
    }
}

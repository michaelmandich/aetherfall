//! Messages on a TCP stream: a little-endian `u32` length, then that many
//! bytes of postcard. Blocking reads and writes; each side runs them on
//! threads of their own.

use std::io::{self, Read, Write};

/// Largest frame accepted (a resume batch of a long match stays well below).
pub const MAX_FRAME: usize = 16 << 20;

pub fn write_frame(w: &mut impl Write, payload: &[u8]) -> io::Result<()> {
    if payload.len() > MAX_FRAME {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "frame too large"));
    }
    w.write_all(&(payload.len() as u32).to_le_bytes())?;
    w.write_all(payload)?;
    w.flush()
}

pub fn read_frame(r: &mut impl Read) -> io::Result<Vec<u8>> {
    let mut len = [0u8; 4];
    r.read_exact(&mut len)?;
    let len = u32::from_le_bytes(len) as usize;
    if len > MAX_FRAME {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "frame too large"));
    }
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf)?;
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_round_trip_back_to_back() {
        let mut buf = Vec::new();
        write_frame(&mut buf, b"hello").unwrap();
        write_frame(&mut buf, b"").unwrap();
        write_frame(&mut buf, &[7; 1000]).unwrap();
        let mut r = io::Cursor::new(buf);
        assert_eq!(read_frame(&mut r).unwrap(), b"hello");
        assert_eq!(read_frame(&mut r).unwrap(), b"");
        assert_eq!(read_frame(&mut r).unwrap(), vec![7; 1000]);
        assert_eq!(read_frame(&mut r).unwrap_err().kind(), io::ErrorKind::UnexpectedEof);
    }

    #[test]
    fn oversized_frames_are_refused() {
        let mut r = io::Cursor::new(((MAX_FRAME + 1) as u32).to_le_bytes().to_vec());
        assert_eq!(read_frame(&mut r).unwrap_err().kind(), io::ErrorKind::InvalidData);
        assert!(write_frame(&mut Vec::new(), &vec![0; MAX_FRAME + 1]).is_err());
    }
}

use crate::NifError;

pub struct Cursor<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    pub fn new(buf: &'a [u8], pos: usize) -> Self {
        Self { buf, pos }
    }
    pub fn pos(&self) -> usize {
        self.pos
    }
    /// Bytes left before the end of the buffer (0 when the position is past it).
    pub fn remaining(&self) -> usize {
        self.buf.len().saturating_sub(self.pos)
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8], NifError> {
        let end = self.pos.checked_add(n).filter(|&e| e <= self.buf.len());
        match end {
            Some(e) => {
                let s = &self.buf[self.pos..e];
                self.pos = e;
                Ok(s)
            }
            None => Err(NifError::Truncated {
                at: self.pos,
                need: n,
            }),
        }
    }
    pub fn skip(&mut self, n: usize) -> Result<(), NifError> {
        self.take(n).map(|_| ())
    }
    pub fn u8(&mut self) -> Result<u8, NifError> {
        Ok(self.take(1)?[0])
    }
    pub fn u16(&mut self) -> Result<u16, NifError> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    pub fn u32(&mut self) -> Result<u32, NifError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    pub fn i32(&mut self) -> Result<i32, NifError> {
        Ok(i32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    pub fn u64(&mut self) -> Result<u64, NifError> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    pub fn f32(&mut self) -> Result<f32, NifError> {
        Ok(f32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    pub fn f32s<const N: usize>(&mut self) -> Result<[f32; N], NifError> {
        let mut out = [0.0; N];
        for v in &mut out {
            *v = self.f32()?;
        }
        Ok(out)
    }
    /// u32 length + bytes (nif.xml SizedString).
    pub fn sized_bytes(&mut self) -> Result<&'a [u8], NifError> {
        let n = self.u32()? as usize;
        self.take(n)
    }
    /// u8 length including the NUL + bytes (nif.xml ExportString); the NUL is stripped.
    pub fn export_bytes(&mut self) -> Result<&'a [u8], NifError> {
        let n = self.u8()? as usize;
        let s = self.take(n)?;
        Ok(s.strip_suffix(&[0]).unwrap_or(s))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reads_le_values_and_strings() {
        let mut b = vec![];
        b.extend(7u32.to_le_bytes());
        b.extend((-1i32).to_le_bytes());
        b.extend(1.5f32.to_le_bytes());
        b.extend(3u32.to_le_bytes());
        b.extend(b"abc");
        b.push(4);
        b.extend(b"xyz\0");
        let mut c = Cursor::new(&b, 0);
        assert_eq!(c.u32().unwrap(), 7);
        assert_eq!(c.i32().unwrap(), -1);
        assert_eq!(c.f32().unwrap(), 1.5);
        assert_eq!(c.sized_bytes().unwrap(), b"abc");
        assert_eq!(c.export_bytes().unwrap(), b"xyz");
        assert_eq!(c.pos(), b.len());
    }
    #[test]
    fn truncation_is_an_error_not_a_panic() {
        let b = [1u8, 2];
        let mut c = Cursor::new(&b, 0);
        assert!(matches!(
            c.u32(),
            Err(NifError::Truncated { at: 0, need: 4 })
        ));
        let mut c = Cursor::new(&[255, 255, 255, 255], 0);
        assert!(matches!(c.sized_bytes(), Err(NifError::Truncated { .. })));
    }
}

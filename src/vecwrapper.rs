pub struct VecNewtype {
    pub inner: Vec<u8>,
    pub fake_size: usize,
}
impl VecNewtype {
    pub fn new() -> VecNewtype {
        VecNewtype::with_fake_capacity(512)
    }
    pub fn with_fake_capacity(sz: usize) -> VecNewtype {
        VecNewtype {
            inner: Vec::with_capacity(sz),
            fake_size: sz,
        }
    }
}
impl From<Vec<u8>> for VecNewtype {
    fn from(inner: Vec<u8>) -> VecNewtype {
        let fake_size = 2 * inner.len().max(256);
        VecNewtype { inner, fake_size }
    }
}
impl scsi::Buffer for VecNewtype {
    fn size(&self) -> usize {
        self.inner.len()
    }
    fn capacity(&self) -> usize {
        self.fake_size
    }
    fn push_byte(&mut self, byte: u8) -> Result<usize, scsi::ScsiError> {
        if self.inner.len() >= self.fake_size {
            return Err(scsi::ScsiError::from_cause(scsi::ErrorCause::BufferTooSmallError {
                expected : self.fake_size + 1,
                actual : self.fake_size,
            }));
        }
        self.inner.push(byte);
        Ok(1)
    }
    fn pull_byte(&mut self) -> Result<u8, scsi::ScsiError> {
        if !self.inner.is_empty() {
            Ok( self.inner.remove(0) )
        }
        else {
            Err(scsi::ScsiError::from_cause(scsi::ErrorCause::BufferTooSmallError {
                expected : 1,
                actual : 0,
            }))
        }
    }
}

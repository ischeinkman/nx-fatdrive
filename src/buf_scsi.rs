use crate::usb_comm::{ReadEndpoint, UsbClient, WriteEndpoint};
use crate::vecwrapper::VecNewtype;
use scsi::Buffer;

use std::io;
use std::io::{BufRead, Read, Seek, SeekFrom, Write};

pub struct OffsetScsiDevice {
    device: scsi::scsi::ScsiBlockDevice<UsbClient, VecNewtype, VecNewtype, VecNewtype>,
    block_buffer: VecNewtype,
    partition_start: usize, //bytes
    partition_idx: usize,   //bytes from partition_start
    loaded_block_number: usize,
    needs_flush: bool,
}

impl Drop for OffsetScsiDevice {
    fn drop(&mut self) {
        self.flush();
    }
}

impl OffsetScsiDevice {
    pub fn new(
        device: scsi::scsi::ScsiBlockDevice<UsbClient, VecNewtype, VecNewtype, VecNewtype>,
        partition_start: usize,
    ) -> Self {
        let block_size = device.block_size() as usize;

        OffsetScsiDevice {
            device,
            block_buffer: VecNewtype::with_fake_capacity(block_size),
            partition_start,
            partition_idx: 0,
            loaded_block_number: 0,
            needs_flush: false,
        }
    }

    #[inline]
    fn raw_idx(&self) -> usize {
        self.partition_start + self.partition_idx
    }

    #[inline]
    fn buffered_block_raw_idx(&self) -> usize {
        self.device.block_size() as usize * self.loaded_block_number
    }

    #[inline]
    fn cur_block_raw_idx(&self) -> usize {
        let rel_offset = self.raw_idx() % self.device.block_size() as usize;
        let block_start = self.raw_idx() - rel_offset;
        block_start as usize
    }

    #[inline]
    fn cur_block_number(&self) -> usize {
        self.cur_block_raw_idx() / self.device.block_size() as usize
    }

    #[inline]
    fn offset_from_cur_block(&self) -> usize {
        self.raw_idx() - self.cur_block_raw_idx()
    }
}

impl BufRead for OffsetScsiDevice {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        if self.cur_block_number() != self.loaded_block_number {
            println!("std::BufRead: Got block number mismatch: have {} but want {} ({} + {}). Flushing & resetting.", self.loaded_block_number, self.cur_block_number(), self.partition_start, self.partition_idx);
            eprintln!("std::BufRead: Got block number mismatch: have {} but want {} ({} + {}). Flushing & resetting.", self.loaded_block_number, self.cur_block_number(), self.partition_start, self.partition_idx);
            self.flush()?;
            self.block_buffer.clear().map_err(|_e| {
                (io::Error::from(io::ErrorKind::Other))
            })?;
        }
        let block_idx = self.cur_block_raw_idx() as u32;
        if self.block_buffer.is_empty() {
            println!(
                "std::BufRead: Buffer is empty. Loading block {} (raw: {}).",
                self.cur_block_number(),
                block_idx
            );
            eprintln!(
                "std::BufRead: Buffer is empty. Loading block {} (raw: {}).",
                self.cur_block_number(),
                block_idx
            );
            let red = self
                .device
                .read(block_idx, &mut self.block_buffer)
                .map_err(|e| match e.cause {
                    scsi::ErrorCause::BufferTooSmallError { expected, actual } => io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        format!(
                            "Buffer too small: wanted {} but only have {}.",
                            expected, actual
                        ),
                    ),
                    e => io::Error::new(io::ErrorKind::Other, format!("Unmatched error : {:?}", e)),
                })?;
            self.loaded_block_number = self.cur_block_number();
            println!("std::BufRead: Loaded block {}.", self.loaded_block_number);
            eprintln!("std::BufRead: Loaded block {}.", self.loaded_block_number);
        }
        Ok(&self.block_buffer.inner.as_slice()[self.offset_from_cur_block()..])
    }

    fn consume(&mut self, amt: usize) {
        self.partition_idx += amt;
    }
}

impl Read for OffsetScsiDevice {
    fn read(&mut self, output_buf: &mut [u8]) -> io::Result<usize> {
        let needed_bytes = output_buf.len();
        println!("std::Read: Requested {} bytes.", needed_bytes);
        eprintln!("std::Read: Requested {} bytes.", needed_bytes);

        let mut output_idx = 0;
        while output_idx < needed_bytes {
            let byte = {
                let buff = self.fill_buf()?;
                if buff.is_empty() {
                    break;
                }
                buff[0]
            };
            output_buf[output_idx] = byte;
            output_idx += 1;
            self.consume(1);
        }
        println!("std::Read: Finished reading {} bytes.", output_idx);
        println!("std::Read: {:?}", output_buf);
        println!(
            "std::Read: self.partition_idx = {}, self.loaded_block_number = {}",
            self.partition_idx, self.loaded_block_number
        );
        eprintln!("std::Read: Finished reading {} bytes.", output_idx);
        eprintln!("std::Read: {:?}", output_buf);
        eprintln!(
            "std::Read: self.partition_idx = {}, self.loaded_block_number = {}",
            self.partition_idx, self.loaded_block_number
        );
        return Ok(output_idx);
    }
}

impl Write for OffsetScsiDevice {
    fn write(&mut self, to_write: &[u8]) -> io::Result<usize> {
        println!(
            "std::Write: Writing {} bytes starting at {}.",
            to_write.len(),
            self.raw_idx()
        );
        eprintln!(
            "std::Write: Writing {} bytes starting at {}.",
            to_write.len(),
            self.raw_idx()
        );
        let mut written_idx = 0;
        while written_idx < to_write.len() {
            self.fill_buf()?;
            if self.block_buffer.is_empty() {
                break;
            }

            let block_offset = self.offset_from_cur_block();
            if self.block_buffer.inner[block_offset] != to_write[written_idx] {
                self.block_buffer.inner[block_offset] = to_write[written_idx];
                self.needs_flush = true;
            }
            written_idx += 1;
            self.consume(1);
        }
        return Ok(written_idx);
    }

    fn flush(&mut self) -> io::Result<()> {
        println!("std::Flush: Entered flush.");
        eprintln!("std::Flush: Entered flush.");
        if !self.needs_flush {
            println!("std::Flush: Not doing flush.");
            eprintln!("std::Flush: Not doing flush.");
            return Ok(());
        }
        let raw_idx = self.buffered_block_raw_idx();
        println!("std::Flush: Doing flush.");
        eprintln!("std::Flush: Doing flush.");
        println!(
            "std::Flush: Raw writing block: {}, offset: {}.",
            self.loaded_block_number, raw_idx
        );
        eprintln!(
            "std::Flush: Raw writing block: {}, offset: {}.",
            self.loaded_block_number, raw_idx
        );
        let _ = self
            .device
            .write(raw_idx as u32, &mut self.block_buffer)
            .map_err(|_e| {
                (io::Error::from(io::ErrorKind::Other))
            })?;
        self.needs_flush = false;
        Ok(())
    }
}
impl Seek for OffsetScsiDevice {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        match pos {
            SeekFrom::Start(absr) => {
                self.partition_idx = absr as usize;
                println!(
                    "std::Seek: Seek via abs to raw {} ({} + {}) in block {}.",
                    self.raw_idx(),
                    self.partition_start,
                    self.partition_idx,
                    self.cur_block_number()
                );
                eprintln!(
                    "std::Seek: Seek via abs to raw {} ({} + {}) in block {}.",
                    self.raw_idx(),
                    self.partition_start,
                    self.partition_idx,
                    self.cur_block_number()
                );
                Ok(absr)
            }
            SeekFrom::Current(off) => {
                let absr = if off < 0 {
                    self.partition_idx - off.abs() as usize
                } else {
                    self.partition_idx + off.abs() as usize
                };

                self.partition_idx = absr;
                println!(
                    "std::Seek: Seek via abs to raw {} ({} + {}) in block {}.",
                    self.raw_idx(),
                    self.partition_start,
                    self.partition_idx,
                    self.cur_block_number()
                );
                eprintln!(
                    "std::Seek: Seek via abs to raw {} ({} + {}) in block {}.",
                    self.raw_idx(),
                    self.partition_start,
                    self.partition_idx,
                    self.cur_block_number()
                );
                Ok(absr as u64)
            }
            _ => unimplemented!(),
        }
    }
}

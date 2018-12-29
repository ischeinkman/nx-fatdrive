use std::alloc::Layout;
use libnx_rs::LibnxError;

pub struct AlignedBuffer {
    raw_ptr : *mut u8,
    layout : Layout
}

impl AlignedBuffer {
    pub fn from_layout(layout : Layout) -> Result<AlignedBuffer, LibnxError> {
        let actual_size = AlignedBuffer::aligned_size_raw(layout.size(), layout.align());
        let actual_layout =  Layout::from_size_align(layout.align(), actual_size);
        let raw_ptr = unsafe {std::alloc::alloc_zeroed(layout)};
        if raw_ptr.is_null() {
            return Err(LibnxError::from_msg("Allocation returned null!".to_owned()));
        }
        Ok(AlignedBuffer {
            raw_ptr,
            layout
        })
    }

    pub fn as_slice(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.raw_ptr, self.size()) }
    }

    pub fn as_aligned_slice(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.raw_ptr, self.aligned_size()) }
    }

    pub fn as_slice_mut(&mut self) -> &mut [u8] {
        unsafe { std::slice::from_raw_parts_mut(self.raw_ptr, self.size()) }
    }
    
    pub fn as_aligned_slice_mut(&mut self) -> &mut [u8] {
        unsafe { std::slice::from_raw_parts_mut(self.raw_ptr, self.aligned_size()) }
    }

    pub fn aligned_size(&self) -> usize {
        AlignedBuffer::aligned_size_raw(self.size(), self.alignment())
    }

    pub fn layout(&self) -> Layout {
        self.layout
    }

    pub fn size(&self) -> usize {
        self.layout.size()
    }

    pub fn alignment(&self) -> usize {
        self.layout.align()
    }

    fn aligned_size_raw(layout_size : usize, layout_align : usize) -> usize {
        let off = layout_size % layout_align;
        if off == 0 {
            layout_size
        }
        else {
            layout_size + layout_align - off
        }
    }
}

impl Drop for AlignedBuffer {
    fn drop(&mut self) {
        unsafe { std::alloc::dealloc(self.raw_ptr, self.layout) };
    }
}
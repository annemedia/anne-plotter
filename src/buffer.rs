// use aligned_alloc::{aligned_alloc, aligned_free};
use std::alloc::{alloc, dealloc, Layout};
use std::sync::{Arc, Mutex};

pub struct PageAlignedByteBuffer {
    data: Option<Arc<Mutex<Vec<u8>>>>,
    // pointer: *mut (),
    pointer: *mut u8,
    layout: Layout,
}

impl PageAlignedByteBuffer {
    pub fn new(buffer_size: usize) -> Self {
        let align = page_size::get();
        let layout = unsafe {
            // Page size is always a power-of-2 and valid align >= size_of::<usize>()
            Layout::from_size_align_unchecked(buffer_size, align)
        };
        let pointer = unsafe { alloc(layout) };
        assert!(!pointer.is_null(), "Allocation failed"); 
       // let pointer = aligned_alloc(buffer_size, page_size::get());
        // let data: Vec<u8>;
        // unsafe {
        //     data = Vec::from_raw_parts(pointer as *mut u8, buffer_size, buffer_size);
        // }
        let data = unsafe {
            Vec::from_raw_parts(pointer, buffer_size, buffer_size)
        };
        PageAlignedByteBuffer {
            data: Some(Arc::new(Mutex::new(data))),
            pointer,
            layout,
        }
    }

    pub fn get_buffer(&self) -> Arc<Mutex<Vec<u8>>> {
        self.data.as_ref().unwrap().clone()
    }
}

impl Drop for PageAlignedByteBuffer {
    fn drop(&mut self) {
        std::mem::forget(self.data.take().unwrap());
        // unsafe {
        //     aligned_free(self.pointer);
        // }
        unsafe {
            dealloc(self.pointer, self.layout);
        }
    }
}

unsafe impl Send for PageAlignedByteBuffer {}

#[cfg(test)]
mod buffer_tests {
    use super::PageAlignedByteBuffer;

    #[test]
    fn buffer_creation_destruction_test() {
        {
            let _test = PageAlignedByteBuffer::new(1024 * 1024);
        }
        assert!(true);
    }
}

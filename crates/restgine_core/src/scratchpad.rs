use std::cell::UnsafeCell;

const SCRATCHPAD_SIZE: usize = 16 * 1024 * 1024; // 16 MB per thread

/// High-performance thread-local 16MB linear arena scratchpad.
/// Eliminates runtime OS `malloc` and `free` allocations during active gameplay frames.
/// All memory is 64-byte cache-line aligned.
pub struct ThreadScratchpad {
    buffer: Box<[u8]>,
    offset: UnsafeCell<usize>,
}

thread_local! {
    static TL_SCRATCHPAD: ThreadScratchpad = ThreadScratchpad::new();
}

impl Default for ThreadScratchpad {
    fn default() -> Self {
        Self::new()
    }
}

impl ThreadScratchpad {
    pub fn new() -> Self {
        // Allocate 16MB zeroed buffer
        let vec = vec![0u8; SCRATCHPAD_SIZE];
        Self {
            buffer: vec.into_boxed_slice(),
            offset: UnsafeCell::new(0),
        }
    }

    /// Reset the scratchpad offset at frame boundary in O(1).
    pub fn reset(&self) {
        unsafe {
            *self.offset.get() = 0;
        }
    }

    /// Allocate a slice of type T with 64-byte alignment from the scratchpad.
    #[allow(clippy::mut_from_ref)]
    pub fn alloc_slice<T: Copy>(&self, count: usize) -> Option<&mut [T]> {
        let size = count * std::mem::size_of::<T>();
        if size == 0 {
            return Some(&mut []);
        }

        unsafe {
            let current = *self.offset.get();
            // 64-byte cache-line alignment
            let align_mask = 63;
            let aligned = (current + align_mask) & !align_mask;
            let next = aligned + size;

            if next > self.buffer.len() {
                return None; // Scratchpad overflow
            }

            *self.offset.get() = next;
            let ptr = self.buffer.as_ptr().add(aligned) as *mut T;
            Some(std::slice::from_raw_parts_mut(ptr, count))
        }
    }

    /// Number of bytes allocated in current frame.
    pub fn allocated_bytes(&self) -> usize {
        unsafe { *self.offset.get() }
    }
}

/// Helper functions operating on thread-local scratchpad.
pub struct Scratchpad;

impl Scratchpad {
    pub fn alloc_slice<T: Copy>(count: usize) -> Option<&'static mut [T]> {
        TL_SCRATCHPAD.with(|sp| {
            // Safety: Transmute lifetime to 'static for frame lifetime.
            // Reset must be called strictly between frames.
            sp.alloc_slice::<T>(count)
                .map(|s| unsafe { std::mem::transmute(s) })
        })
    }

    pub fn reset() {
        TL_SCRATCHPAD.with(|sp| sp.reset());
    }

    pub fn allocated_bytes() -> usize {
        TL_SCRATCHPAD.with(|sp| sp.allocated_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scratchpad_zero_alloc_reset() {
        Scratchpad::reset();
        assert_eq!(Scratchpad::allocated_bytes(), 0);

        let slice = Scratchpad::alloc_slice::<f32>(1024).expect("Allocation failed");
        assert_eq!(slice.len(), 1024);
        slice[0] = 42.0;
        assert_eq!(slice[0], 42.0);
        assert!(Scratchpad::allocated_bytes() >= 4096);

        Scratchpad::reset();
        assert_eq!(Scratchpad::allocated_bytes(), 0);
    }
}

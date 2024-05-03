use std::alloc::Layout;
use crate::sync_ptr::SyncPtr;

#[derive(Debug)]
pub(crate) struct HBufDestructor {
    data_ptr: SyncPtr,
    capacity: usize,
    destructor_info: usize,
    destructor: fn(*mut u8, usize, usize)
}

impl HBufDestructor {
    pub(crate) fn new(data_ptr: *mut u8, capacity: usize, destructor_info: usize, destructor: fn(*mut u8, usize, usize)) -> HBufDestructor {
        return HBufDestructor {
            data_ptr: data_ptr.into(),
            capacity,
            destructor_info,
            destructor,
        }
    }
}

impl Drop for HBufDestructor {
    fn drop(&mut self) {
        (self.destructor)(self.data_ptr.ptr(), self.capacity, self.destructor_info);
    }
}

pub(crate) fn call_destructor(data: *mut u8, size: usize, destructor: usize) {
    let destr: fn(*mut u8, usize) = unsafe {std::mem::transmute(destructor)};
    destr(data, size);
}

pub(crate) fn call_dealloc(data: *mut u8, size: usize, alignment: usize) {
    unsafe { std::alloc::dealloc(data, Layout::from_size_align_unchecked(size, alignment)) }
}

#[cfg(test)]
#[test]
fn test_sync() {
    static_assertions::assert_impl_all!(HBufDestructor: Sync);
}
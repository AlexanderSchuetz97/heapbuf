use std::alloc::Layout;
use crate::sync_ptr::SyncPtr;

#[derive(Debug)]
pub(crate) struct HBufDestructor {
    data_ptr: SyncPtr,
    capacity: usize,
    destructor_info: HBufDestructorInfo
}

#[derive(Debug)]
pub(crate) enum HBufDestructorInfo {
    Layout(Layout),
    Destructor(fn(*mut u8, usize))
}

impl HBufDestructor {
    pub(crate) fn new(data_ptr: *mut u8, capacity: usize, destructor_info: HBufDestructorInfo) -> HBufDestructor {
        return HBufDestructor {
            data_ptr: data_ptr.into(),
            capacity,
            destructor_info
        }
    }
}

impl Drop for HBufDestructor {
    fn drop(&mut self) {
        match self.destructor_info {
            HBufDestructorInfo::Layout(lay) => unsafe { std::alloc::dealloc(self.data_ptr.ptr(), lay) }
            HBufDestructorInfo::Destructor(destructor_fn) => destructor_fn(self.data_ptr.ptr(), self.capacity)
        }
    }
}

#[cfg(test)]
#[test]
fn test_sync() {
    static_assertions::assert_impl_all!(HBufDestructor: Sync);
}
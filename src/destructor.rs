use std::alloc::Layout;
use sync_ptr::SyncMutPtr;

#[derive(Debug)]
pub(crate) struct HBufDestructor {
    data_ptr: SyncMutPtr<u8>,
    capacity: usize,
    destructor_info: HBufDestructorInfo
}

#[derive(Debug)]
pub(crate) enum HBufDestructorInfo {
    Layout(Layout),
    Destructor(fn(*mut u8, usize))
}

impl HBufDestructor {
    pub(crate) fn new(data_ptr: SyncMutPtr<u8>, capacity: usize, destructor_info: HBufDestructorInfo) -> HBufDestructor {
        HBufDestructor {
            data_ptr,
            capacity,
            destructor_info
        }
    }
}

impl Drop for HBufDestructor {
    fn drop(&mut self) {
        match self.destructor_info {
            HBufDestructorInfo::Layout(lay) => unsafe { std::alloc::dealloc(self.data_ptr.inner(), lay) }
            HBufDestructorInfo::Destructor(destructor_fn) => destructor_fn(self.data_ptr.inner(), self.capacity)
        }
    }
}

#[cfg(test)]
#[test]
fn test_sync() {
    static_assertions::assert_impl_all!(HBufDestructor: Sync);
}
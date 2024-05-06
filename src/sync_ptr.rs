use std::fmt;
use std::fmt::{Display, Formatter, Pointer};
use std::ops::{Deref, DerefMut};

#[derive(Debug, Eq, PartialEq, Copy, Clone, Hash)]
#[repr(transparent)]
pub(crate) struct SyncPtr(*mut u8);
unsafe impl Sync for SyncPtr {}
unsafe impl Send for SyncPtr {}

impl Pointer for SyncPtr {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        fmt::Pointer::fmt(&self.0, f)
    }
}

impl Display for SyncPtr {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        fmt::Pointer::fmt(&self.0, f)
    }
}
impl SyncPtr {

    #[inline(always)]
    pub(crate) fn ptr(&self) -> *mut u8 {
        self.0
    }
}

impl Deref for SyncPtr {
    type Target = *mut u8;
    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for SyncPtr {

    #[inline(always)]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}


impl Into<*mut u8> for &SyncPtr {
    #[inline(always)]
    fn into(self) -> *mut u8 {
        self.0
    }
}

impl Into<*mut u8> for &mut SyncPtr {
    #[inline(always)]
    fn into(self) -> *mut u8 {
        self.0
    }
}

impl From<*mut u8> for SyncPtr {

    #[inline(always)]
    fn from(value: *mut u8) -> Self {
        SyncPtr(value)
    }
}
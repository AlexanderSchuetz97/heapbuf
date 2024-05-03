use std::alloc::{Layout, LayoutError};
use std::fmt::{Debug, Display, Formatter};
use std::io;
use std::io::{Error, ErrorKind, Read, Seek, SeekFrom, Write};
use std::mem::{align_of, size_of};
use std::ops::{Deref, DerefMut, Index, IndexMut};
use std::sync::Arc;
use crate::destructor;
use crate::destructor::HBufDestructor;
use crate::sync_ptr::SyncPtr;

pub enum HBufError {
    ZeroSize,
    OutOfMemory,
    LayoutError
}

impl From<LayoutError> for HBufError {
    fn from(_: LayoutError) -> Self {
        HBufError::LayoutError
    }
}

impl From<HBufError> for std::io::Error {
    fn from(value: HBufError) -> Self {
        match value {
            HBufError::ZeroSize => Error::new(ErrorKind::Other, "Cannot allocate zero sized buffer"),
            HBufError::OutOfMemory =>  Error::new(ErrorKind::OutOfMemory, "OutOfMemory"),
            HBufError::LayoutError => Error::new(ErrorKind::Other, "Invalid Memory Layout"),
        }
    }
}

impl Display for HBufError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        Debug::fmt(self, f)
    }
}

impl Debug for HBufError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            HBufError::ZeroSize => write!(f, "HBufError::ZeroSize"),
            HBufError::OutOfMemory => write!(f, "HBufError::OutOfMemory"),
            HBufError::LayoutError => write!(f, "HBufError::LayoutError")
        }
    }
}

#[derive(Debug)]
pub struct HBuf {
    data_ptr: SyncPtr,
    capacity: usize,
    limit: usize,
    position: usize,
    destructor: Option<Arc<HBufDestructor>>
}

macro_rules! known_type {
    ($type:ty, $name:ident, $mut_name:ident, $get_name:ident, $set_name:ident) => {

        ///
        /// Returns a slice if the HBuf is properly aligned.
        ///
        pub fn $name(&self) -> Option<&[$type]> {
            if self.data_ptr.align_offset(align_of::<$type>()) != 0 {
                return None;
            }
            return unsafe { Some(std::slice::from_raw_parts(self.data_ptr.ptr().cast::<$type>(), self.limit / size_of::<$type>()))};
        }

        ///
        /// Returns a mutable slice if the HBuf is properly aligned.
        ///
        pub fn $mut_name(&mut self) -> Option<&mut [$type]> {
            if self.data_ptr.align_offset(align_of::<$type>()) != 0 {
                return None;
            }
            return unsafe { Some(std::slice::from_raw_parts_mut(self.data_ptr.ptr().cast::<$type>(), self.limit / size_of::<$type>()))};
        }

        ///
        /// Reads a the value at the given offset.
        /// The value is read using read_unaligned.
        /// panics on out of bounds.
        ///
        pub fn $get_name(&self, index: usize) -> $type {
            let sz = size_of::<$type>()-1;
            if index+sz >= self.limit {
                panic!("Index {} is out of bounds for HBuf with limit {}", index+sz, self.limit);
            }
            unsafe { return self.data_ptr.wrapping_add(index).cast::<$type>().read_unaligned(); }
        }

        ///
        /// Reads a the value at the given offset.
        /// The value is read using read_unaligned.
        /// panics on out of bounds.
        ///
        pub fn $set_name<T: Sized>(&self, index: usize, value: $type) {
            let sz = size_of::<$type>()-1;
            if index+sz >= self.limit {
                panic!("Index {} is out of bounds for HBuf with limit {}", index+sz, self.limit);
            }
            unsafe { self.data_ptr.wrapping_add(index).cast::<$type>().write_unaligned(value); }
        }
    };
}

impl HBuf {

    ///
    /// Creates a HBuf from a pointer.
    /// Dropping the resulting HBuf is a noop.
    /// Caller must ensure that the pointer lives longer than HBuf and is valid.
    ///
    pub unsafe fn from_raw_parts(data: *mut u8, size: usize) -> HBuf {
        return HBuf {
            data_ptr: data.into(),
            capacity: size,
            limit: size,
            position: 0,
            destructor: None
        }
    }

    ///
    /// Creates a HBuf from a pointer.
    /// Dropping the resulting HBuf will call the provided destructor function.
    /// If the HBuf is shared with other threads then the destructor may be called in any thread.
    ///
    pub unsafe fn from_raw_parts_with_destructor(data: *mut u8, size: usize, destructor: fn(*mut u8, usize)) -> HBuf {

        let addr: usize = unsafe {std::mem::transmute(destructor)};

        return HBuf {
            data_ptr: data.into(),
            capacity: size,
            limit: size,
            position: 0,
            destructor: Some(Arc::new(HBufDestructor::new(data, size, addr, destructor::call_destructor)))
        }
    }

    ///
    /// Allocates memory using the standard rust allocator.
    /// The memory does not have any particular alignment.
    ///
    pub fn allocate(size: usize) -> Result<HBuf, HBufError> {
        return HBuf::allocate_aligned(size, 1);
    }

    ///
    /// Allocates memory using the standard rust allocator.
    /// The memory will be aligned to the given alignment.
    ///
    /// This function will fail if the allocator cannot allocate memory or allocates memory that does not have the desired alignment.
    ///
    ///
    pub fn allocate_aligned(size: usize, alignment: usize) -> Result<HBuf, HBufError> {
        if size == 0 || alignment == 0 {
            return Err(HBufError::LayoutError);
        }

        let layout = Layout::from_size_align(size, alignment)?;
        let data = unsafe {std::alloc::alloc(layout)};
        if data.is_null() {
            return Err(HBufError::OutOfMemory);
        }

        if data.align_offset(alignment) != 0 {
            unsafe { std::alloc::dealloc(data, layout) }
            return Err(HBufError::LayoutError);
        }

        return Ok(HBuf {
            data_ptr: data.into(),
            capacity: size,
            limit: size,
            position: 0,
            destructor: Some(Arc::new(HBufDestructor::new(data, size, 1, destructor::call_dealloc)))
        });
    }

    ///
    /// Returns the maximum (capacity) of this heap buffer.
    ///
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    ///
    /// Returns the currently usable size of this heap buffer. This is <= capacity.
    ///
    pub fn limit(&self) -> usize {
        self.limit
    }

    ///
    /// Returns the position in the heap HBuf.
    /// The position is only relevant when used in combination with the Seek trait
    ///
    pub fn position(&self) -> usize {
        self.position
    }

    ///
    /// Returns the amount of bytes remaining in the HBuf.
    /// The position/remaining bytes are only relevant when used in combination with the Seek trait
    ///
    pub fn remaining(&self) -> usize {
        self.limit - self.position
    }

    ///
    /// Returns the pointer to the start of the HBuf
    ///
    pub fn as_ptr(&self) -> *mut u8 {
        self.data_ptr.ptr()
    }

    ///
    /// Returns a slice that is backed by the HBuf.
    /// The size of the slice is the current limit.
    ///
    pub fn as_slice(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.data_ptr.ptr(), self.limit) }
    }

    ///
    /// Returns a mutable slice that is backed by the HBuf.
    /// The size of the slice is the current limit.
    ///
    pub fn as_mut_slice(&self) -> &mut [u8] {
        unsafe { std::slice::from_raw_parts_mut(self.data_ptr.ptr(), self.limit) }
    }

    ///
    /// Turns this HBuf into a slice of arbitrary data.
    /// This function will return None if the alignment of T does not match the alignment of the HBuf
    ///
    pub unsafe fn as_slice_generic<T: Sized>(&self) -> Option<&[T]> {
        if self.data_ptr.align_offset(align_of::<T>()) != 0 {
            return None;
        }
        return Some(std::slice::from_raw_parts(self.data_ptr.ptr().cast::<T>(), self.limit / size_of::<T>()));
    }

    ///
    /// Turns this HBuf into a mutable slice of arbitrary data.
    /// This function will return None if the alignment of T does not match the alignment of the HBuf
    ///
    pub unsafe fn as_mut_slice_generic<T: Sized>(&self) -> Option<&mut [T]> {
        if self.data_ptr.align_offset(align_of::<T>()) != 0 {
            return None;
        }
        return Some(std::slice::from_raw_parts_mut(self.data_ptr.ptr().cast::<T>(), self.limit / size_of::<T>()));
    }

    ///
    /// Copies the value T at the specified location out of the memory.
    /// This method uses read_unaligned so alignment is irrelevant for this method.
    ///
    pub unsafe fn get<T: Sized+Copy>(&self, index: usize) -> T {
        let sz = size_of::<T>();
        if index+sz-1 >= self.limit {
            panic!("Index {} is out of bounds for HBuffer with limit {}", index+sz-1, self.limit);
        }
        unsafe { return self.data_ptr.wrapping_add(index).cast::<T>().read_unaligned(); }
    }

    ///
    /// Returns a reference to a datatype stored at the given location in memory.
    /// This method is unsafe because it will always return a reference regardless of borrow checking/multithreading
    /// constraints that the type T may require.
    ///
    /// This function enforces alignment of T and will panic if the memory is not properly aligned.
    ///
    pub unsafe fn get_ref<T>(&self, index: usize) -> &T {
        let sz = size_of::<T>();
        if index+sz-1 >= self.limit {
            panic!("Index {} is out of bounds for HeapBuffer with limit {}", index+sz-1, self.limit);
        }

        let ptr = self.data_ptr.wrapping_add(index);
        if ptr.align_offset(align_of::<T>()) != 0 {
            panic!("Index {} is not properly aligned for {}", index+sz-1, align_of::<T>());
        }

        return ptr.cast::<T>().as_ref().unwrap();
    }

    ///
    /// Returns a reference to a datatype stored at the given location in memory.
    /// This method is unsafe because it will always return a reference regardless of borrow checking/multithreading
    /// constraints that the type T may require.
    ///
    /// This function enforces alignment of T and will panic if the memory is not properly aligned.
    ///
    pub unsafe fn get_ref_mut<T>(&self, index: usize) -> &mut T {
        let sz = size_of::<T>();
        if index+sz-1 >= self.limit {
            panic!("Index {} is out of bounds for HBuf with limit {}", index+sz-1, self.limit);
        }

        let ptr = self.data_ptr.wrapping_add(index);
        if ptr.align_offset(align_of::<T>()) != 0 {
            panic!("Index {} is not properly aligned for {}", index+sz-1, align_of::<T>());
        }

        return ptr.cast::<T>().as_mut().unwrap();
    }


    ///
    /// Sets the value at the given location to the value.
    /// The alignment of T and the memory location does not matter as this method uses "write_unaligned"
    /// to write memory.
    ///
    pub unsafe fn set<T: Sized>(&self, index: usize, value: T) {
        let sz = size_of::<T>();
        if index+sz-1 >= self.limit {
            panic!("Index {} is out of bounds for HBuf with limit {}", index+sz-1, self.limit);
        }
        unsafe { self.data_ptr.wrapping_add(index).cast::<T>().write_unaligned(value); }
    }

    known_type!(i8, as_slice_i8, as_mut_slice_i8, get_i8, set_i8);
    known_type!(i16, as_slice_i16, as_mut_slice_i16, get_i16, set_i16);
    known_type!(i32, as_slice_i32, as_mut_slice_i32, get_i32, set_i32);
    known_type!(i64, as_slice_i64, as_mut_slice_i64, get_i64, set_i64);
    known_type!(i128, as_slice_i128, as_mut_slice_i128, get_i128, set_i128);

    known_type!(u8, as_slice_u8, as_mut_slice_u8, get_u8, set_u8);
    known_type!(u16, as_slice_u16, as_mut_slice_u16, get_u16, set_u16);
    known_type!(u32, as_slice_u32, as_mut_slice_u32, get_u32, set_u32);
    known_type!(u64, as_slice_u64, as_mut_slice_u64, get_u64, set_u64);
    known_type!(u128, as_slice_u128, as_mut_slice_u128, get_u128, set_u128);

    known_type!(f32, as_slice_f32, as_mut_slice_f32, get_f32, set_f32);
    known_type!(f64, as_slice_f64, as_mut_slice_f64, get_f64, set_f64);

    #[cfg(feature = "uintx_support")]
    known_type!(uintx::u24, as_slice_u24, as_mut_slice_u24, get_u24, set_u24);

    #[cfg(feature = "uintx_support")]
    known_type!(uintx::u40, as_slice_u40, as_mut_slice_u40, get_u40, set_u40);

    #[cfg(feature = "uintx_support")]
    known_type!(uintx::u48, as_slice_u48, as_mut_slice_u48, get_u48, set_u48);

    #[cfg(feature = "uintx_support")]
    known_type!(uintx::u56, as_slice_u56, as_mut_slice_u56, get_u56, set_u56);

    #[cfg(feature = "uintx_support")]
    known_type!(uintx::u72, as_slice_u72, as_mut_slice_u72, get_u72, set_u72);

    #[cfg(feature = "uintx_support")]
    known_type!(uintx::u80, as_slice_u80, as_mut_slice_u80, get_u80, set_u80);

    #[cfg(feature = "uintx_support")]
    known_type!(uintx::u88, as_slice_u88, as_mut_slice_u88, get_u88, set_u88);

    #[cfg(feature = "uintx_support")]
    known_type!(uintx::u96, as_slice_u96, as_mut_slice_u96, get_u96, set_u96);

    #[cfg(feature = "uintx_support")]
    known_type!(uintx::u104, as_slice_u104, as_mut_slice_u104, get_u104, set_u104);

    #[cfg(feature = "uintx_support")]
    known_type!(uintx::u112, as_slice_u112, as_mut_slice_u112, get_u112, set_u112);

    #[cfg(feature = "uintx_support")]
    known_type!(uintx::u120, as_slice_u120, as_mut_slice_u120, get_u120, set_u120);

    #[cfg(feature = "f16_support")]
    known_type!(half::f16, as_slice_f16, as_mut_slice_f16, get_f16, set_f16);

    #[cfg(feature = "f128_support")]
    known_type!(f128::f128, as_slice_f128, as_mut_slice_f128, get_f128, set_f128);

    ///
    /// Changes the limit of accessible bytes in the buffer.
    /// This has no effect on slices creates prior to calling this method.
    ///
    /// panics if limit > capacity.
    ///
    pub fn set_limit(&mut self, new_limit: usize) {
        if new_limit > self.capacity {
            panic!("Limit {} is out of bounds for HBuf with capacity {}", new_limit, self.capacity);
        }

        self.limit = new_limit;

        if self.position > self.limit {
            self.position = self.limit;
        }
    }

    ///
    /// Changes the limit of accessible bytes in the buffer.
    /// This has no effect on slices creates prior to calling this method.
    ///
    /// returns false if limit > capacity
    ///
    pub fn try_set_limit(&mut self, new_limit: usize) -> bool {
        if new_limit > self.capacity {
            return false;
        }

        self.limit = new_limit;

        if self.position > self.limit {
            self.position = self.limit;
        }

        return true;
    }

    ///
    /// Changes the position. (Relevant for Seek trait)
    ///
    /// panics if position > limit
    ///
    pub fn set_position(&mut self, new_position: usize) {
        if new_position > self.limit {
            panic!("Position {} is out of bounds for HBuf with limit {}", new_position, self.limit);
        }
        self.position = new_position;
    }

    ///
    /// Changes the position. (Relevant for Seek trait)
    ///
    /// return false if position > limit
    ///
    pub fn try_set_position(&mut self, new_position: usize) -> bool {
        if new_position > self.limit {
            return false;
        }
        self.position = new_position;
        return true;
    }

    ///
    /// Flips the HeapBuf.
    /// It sets the limit ot the previous position and sets the position to 0.
    ///
    /// This is useful when transitioning a buffer from reading to writing and vice versa.
    ///
    pub fn flip(&mut self) {
        self.limit = self.position;
        self.position = 0;
    }

    ///
    /// Splits off a "sub" buffer that is backed by the same memory as this HeapBuf.
    /// The sub buffer may be smaller than the current capacity or start at a given offset.
    /// This function leaves this HeapBuf unmodified.
    ///
    /// The limit of the sub buffer is set to its capacity and the position is always initialized with 0.
    /// panics if off+length > capacity.
    ///
    pub fn split(&self, off: usize, length: usize) -> HBuf {
        if off+length > self.capacity {
            panic!("Cannot split of a HBuf with {} bytes at offset {} because the capacity of the source buffer is only {}", length, off, self.capacity);
        }

        return HBuf {
            data_ptr: self.data_ptr.wrapping_add(off).into(),
            capacity: length,
            limit: length,
            position: 0,
            destructor: self.destructor.clone(),
        }
    }

    ///
    /// Splits off a "sub" buffer that is backed by the same memory as this HeapBuf.
    /// The sub buffer may be smaller than the current capacity or start at a given offset.
    /// This function leaves this HeapBuf unmodified.
    ///
    /// The limit of the sub buffer is set to its capacity and the position is always initialized with 0.
    /// panics if off+length > capacity.
    ///
    pub fn try_split(&self, off: usize, length: usize) -> Option<HBuf> {
        if off+length > self.capacity {
            return None;
        }

        return Some(HBuf {
            data_ptr: self.data_ptr.wrapping_add(off).into(),
            capacity: length,
            limit: length,
            position: 0,
            destructor: self.destructor.clone(),
        });
    }

    fn seek_start(&mut self, from: u64) -> bool {
        if from > self.limit as u64 {
            return false;
        }

        self.position = from as usize;
        return true;
    }

    fn seek_end(&mut self, from: i64) -> bool {
        if from > 0 {
            return false;
        }

        let from = from.abs() as u64;
        if from > self.limit as u64 {
            return false;
        }

        self.position = self.limit - from as usize;
        return true;
    }

    fn seek_cur(&mut self, from: i64) -> bool {
        let pos = self.position as i64 + from;
        if pos < 0 {
            return false;
        }

        return self.seek_start(pos as u64);
    }



}

impl Seek for HBuf {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        let success = match pos {
            SeekFrom::Start(p) => self.seek_start(p),
            SeekFrom::End(p) => self.seek_end(p),
            SeekFrom::Current(p) => self.seek_cur(p)
        };

        if success {
            return Ok(self.position as u64);
        }

        return Err(Error::new(ErrorKind::UnexpectedEof, "out of bounds"));
    }
}

impl Write for HBuf {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let to_copy = buf.len().min(self.position-self.limit);
        if to_copy == 0 {
            return Ok(0);
        }

        self.position = self.position + to_copy;
        return Ok(to_copy);
    }

    fn flush(&mut self) -> io::Result<()> {
        //NOOP
        return Ok(());
    }

    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        if buf.len() == 0 {
            return Ok(());
        }

        if self.limit-self.position < buf.len() {
            return Err(Error::new(ErrorKind::UnexpectedEof, "failed write entire buffer"));
        }

        unsafe { std::ptr::copy(buf.as_ptr(), self.data_ptr.wrapping_add(self.position), buf.len()) }
        self.position = self.position + buf.len();
        return Ok(());
    }
}

impl Read for HBuf {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let to_copy = buf.len().min(self.position-self.limit);
        if to_copy == 0 {
            return Ok(0);
        }
        unsafe { std::ptr::copy(self.data_ptr.wrapping_add(self.position), buf.as_mut_ptr(), to_copy) }
        self.position = self.position + to_copy;
        return Ok(to_copy);
    }

    fn read_to_end(&mut self, buf: &mut Vec<u8>) -> io::Result<usize> {
        let to_copy = self.position-self.limit;
        if to_copy == 0 {
            return Ok(0);
        }
        let sl = unsafe { std::slice::from_raw_parts(self.data_ptr.wrapping_add(self.position), to_copy) };
        buf.write_all(sl)?;
        self.position = self.limit;
        return Ok(to_copy);
    }

    fn read_exact(&mut self, buf: &mut [u8]) -> io::Result<()> {
        if buf.len() == 0 {
            return Ok(());
        }

        if self.limit-self.position < buf.len() {
            return Err(Error::new(ErrorKind::UnexpectedEof, "failed to fill entire buffer"));
        }
        unsafe { std::ptr::copy(self.data_ptr.wrapping_add(self.position), buf.as_mut_ptr(), buf.len()) }
        self.position = self.position + buf.len();
        return Ok(());
    }
}

impl Clone for HBuf {
    fn clone(&self) -> Self {
        return HBuf {
            data_ptr: self.data_ptr.clone(),
            capacity: self.capacity,
            limit: self.limit,
            position: self.position,
            destructor: self.destructor.clone(),
        }
    }
}


impl Index<usize> for HBuf {
    type Output = u8;

    fn index(&self, index: usize) -> &Self::Output {
        if index >= self.limit {
            panic!("Index {} is out of bounds for HBuf with limit {}", index, self.limit);
        }
        unsafe { return &*self.data_ptr.wrapping_add(index); }
    }
}

impl IndexMut<usize> for HBuf {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        if index >= self.limit {
            panic!("Index {} is out of bounds for HBuf with limit {}", index, self.limit);
        }
        unsafe { return &mut *self.data_ptr.wrapping_add(index); }
    }
}

impl Deref for HBuf {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

impl DerefMut for HBuf {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.as_mut_slice()
    }
}

#[cfg(test)]
#[test]
fn test_sync() {
    static_assertions::assert_impl_all!(HBuf: Sync);
}
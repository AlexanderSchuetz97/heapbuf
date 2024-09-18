use std::alloc::{Layout, LayoutError};
use std::fmt::{Binary, Debug, Display, Formatter, LowerHex, UpperHex};
use std::hash::{Hash, Hasher};
use std::io;
use std::io::{Error, ErrorKind, Read, Seek, SeekFrom, Write};
use std::mem::{align_of, size_of};
use std::ops::{Deref, DerefMut, Index, IndexMut};
use std::sync::Arc;
use std::sync::atomic::{AtomicPtr, Ordering};
use sync_ptr::{FromMutPtr, SyncMutPtr};
use crate::destructor::{HBufDestructor, HBufDestructorInfo};

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
    data_ptr: SyncMutPtr<u8>,
    capacity: usize,
    limit: usize,
    position: usize,
    destructor: Arc<Option<HBufDestructor>>
}
impl Hash for HBuf {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write(self.as_slice());
    }
}

///
/// This implementation does not strip leading 0s.
/// Length of the format result will always be capacity*8
///
impl Binary for HBuf {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        unsafe {
            for x in 0..self.capacity {
                write!(f, "{:08o}", *self.data_ptr.add(x))?;
            }
        }

        return Ok(());
    }
}

///
/// This implementation does not strip leading 0s.
/// Length of the format result will always be capacity*2
///
impl LowerHex for HBuf {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        unsafe {
            for x in 0..self.capacity {
                write!(f, "{:02x}", *self.data_ptr.add(x))?;
            }
        }

        return Ok(());
    }
}

///
/// This implementation does not strip leading 0s.
/// Length of the format result will always be capacity*2
///
impl UpperHex for HBuf {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        unsafe {
            for x in 0..self.capacity {
                write!(f, "{:02X}", *self.data_ptr.add(x))?;
            }
        }

        return Ok(());
    }
}



///
/// This formats the "metadata" such as capacity/limit/position/ref-count of the HBuf plus all the data
/// in a Human-Readable form.
///
/// The data is formatted as a hex dump similar to what the application xxd would output if the buffer contents were
/// written out to a file and "xxd <filename>" were to be called on the file.
///
impl Display for HBuf {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        unsafe {
            write!(f, "\
            =============================================================================\n\
            Address: {:p}-{:p}\n\
            Capacity: {}\n\
            Limit: {}\n\
            Position: {}\n\
            Has destructor: {}\n\
            Reference count: {}\n\
            =============================================================================",
                   self.data_ptr,
                   self.data_ptr.add(self.capacity),
                   self.capacity,
                   self.limit,
                   self.position,
                   self.destructor.is_some(),
                   Arc::strong_count(&self.destructor))?;


            for idx_base in (0..self.capacity).step_by(16) {
                write!(f, "\n0x{:0width$x}:", self.data_ptr.add(idx_base) as usize, width = (usize::BITS / 4) as usize)?;

                for idx in 0..16usize {
                    if idx & 1 == 0 {
                        write!(f, " ")?;
                    }
                    if idx_base+idx >= self.capacity {
                        write!(f, "  ")?;
                        continue;
                    }
                    write!(f, "{:02x}", *self.data_ptr.add(idx+idx_base))?;
                }
                write!(f, "  ")?;

                for idx in 0..16usize {
                    if idx_base+idx >= self.capacity {
                        write!(f, " ")?;
                        continue;
                    }
                    let data = (*self.data_ptr.add(idx+idx_base)) as char;
                    if char::is_ascii_graphic(&data) {
                        write!(f, "{}", data)?;
                    } else {
                        write!(f, ".")?;
                    }
                }
            }

            write!(f, "\n=============================================================================")?;
            return Ok(());
        }
    }
}

macro_rules! atomic_type {
    ($type:ty, $atomic:ty, $as_slice_name:ident, $as_atomic:ident, $load_name:ident, $store_name:ident,  $swap_name:ident, $cas_name:ident, $cas_weak_name:ident) => {

        ///
        /// Returns a slice of Atomic "references" to the buffer.
        /// The "references" remain valid even if the buffer limit changes.
        ///
        /// This function requires the alignment of the buffer to match the alignment of the type.
        /// If the buffer is not properly aligned then this function returns None.
        ///
        ///
        #[inline]
        pub fn $as_slice_name(&self) -> Option<&[$atomic]> {
            if self.data_ptr.align_offset(align_of::<$atomic>()) != 0 {
                return None;
            }
            unsafe {
                return Some(std::slice::from_raw_parts(self.data_ptr.inner().cast::<$atomic>(), self.limit / size_of::<$atomic>()));
            }

        }

        ///
        /// Returns a Atomic "reference" of a given type to a index.
        /// The "reference" remains usable even if the limit changes.
        ///
        /// This function requires the alignment of the index to match the alignment of the type.
        /// If the index is not properly aligned or the index is out of bounds then this function returns None.
        ///
        ///
        #[inline]
        pub fn $as_atomic(&self, index: usize) -> Option<&$atomic> {
            let sz = size_of::<$atomic>();
            if index+sz-1 >= self.limit {
                return None;
            }
            let ptr = self.data_ptr.wrapping_add(index);
            if ptr.align_offset(align_of::<$atomic>()) != 0 {
                return None;
            }
            unsafe {
                return Some(<$atomic>::from_ptr(ptr.cast::<$type>()));
            }
        }

        ///
        /// Atomic "get" with memory ordering semantics.
        ///
        #[inline]
        pub fn $load_name(&self, index: usize, ordering: Ordering) -> $type {
            let sz = size_of::<$atomic>();
            if index+sz-1 >= self.limit {
                panic!("Index {} is out of bounds for HBuf with limit {}", index+sz-1, self.limit);
            }
            let ptr = self.data_ptr.wrapping_add(index);
            debug_assert_eq!(ptr.align_offset(align_of::<$atomic>()), 0);
            unsafe {
                return <$atomic>::from_ptr(ptr.cast::<$type>()).load(ordering);
            }
        }

        ///
        /// Atomic "set" with memory ordering semantics.
        ///
        #[inline]
        pub fn $store_name(&self, index: usize, value: $type, ordering: Ordering) {
            let sz = size_of::<$atomic>();
            if index+sz-1 >= self.limit {
                panic!("Index {} is out of bounds for HBuf with limit {}", index+sz-1, self.limit);
            }
            let ptr = self.data_ptr.wrapping_add(index);
            debug_assert_eq!(ptr.align_offset(align_of::<$atomic>()), 0);
            unsafe {
                return <$atomic>::from_ptr(ptr.cast::<$type>()).store(value, ordering);
            }
        }

        ///
        /// Atomic "swap" with memory ordering semantics.
        ///
        #[inline]
        pub fn $swap_name(&self, index: usize, value: $type, ordering: Ordering) -> $type {
            let sz = size_of::<$atomic>();
            if index+sz-1 >= self.limit {
                panic!("Index {} is out of bounds for HBuf with limit {}", index+sz-1, self.limit);
            }
            let ptr = self.data_ptr.wrapping_add(index);
            debug_assert_eq!(ptr.align_offset(align_of::<$atomic>()), 0);
            unsafe {
                return <$atomic>::from_ptr(ptr.cast::<$type>()).swap(value, ordering);
            }
        }

        ///
        /// Atomic "compare_exchange" with memory ordering semantics.
        ///
        #[inline]
        pub fn $cas_name(&self, index: usize, current: $type, update: $type, success_ordering: Ordering, failure_ordering: Ordering) -> Result<$type, $type> {
            let sz = size_of::<$atomic>();
            if index+sz-1 >= self.limit {
                panic!("Index {} is out of bounds for HBuf with limit {}", index+sz-1, self.limit);
            }
            let ptr = self.data_ptr.wrapping_add(index);
            debug_assert_eq!(ptr.align_offset(align_of::<$atomic>()), 0);
            unsafe {
                return <$atomic>::from_ptr(ptr.cast::<$type>()).compare_exchange(current, update, success_ordering, failure_ordering);
            }
        }

        ///
        /// Atomic "compare_exchange_weak" with memory ordering semantics.
        ///
        #[inline]
        pub fn $cas_weak_name(&self, index: usize, current: $type, update: $type, success_ordering: Ordering, failure_ordering: Ordering) -> Result<$type, $type> {
            let sz = size_of::<$atomic>();
            if index+sz-1 >= self.limit {
                panic!("Index {} is out of bounds for HBuf with limit {}", index+sz-1, self.limit);
            }
            let ptr = self.data_ptr.wrapping_add(index);
            debug_assert_eq!(ptr.align_offset(align_of::<$atomic>()), 0);
            unsafe {
                return <$atomic>::from_ptr(ptr.cast::<$type>()).compare_exchange_weak(current, update, success_ordering, failure_ordering);
            }
        }
    }
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
            return unsafe { Some(std::slice::from_raw_parts(self.data_ptr.inner().cast::<$type>(), self.limit / size_of::<$type>()))};
        }

        ///
        /// Returns a mutable slice if the HBuf is properly aligned.
        ///
        pub fn $mut_name(&mut self) -> Option<&mut [$type]> {
            if self.data_ptr.align_offset(align_of::<$type>()) != 0 {
                return None;
            }
            return unsafe { Some(std::slice::from_raw_parts_mut(self.data_ptr.inner().cast::<$type>(), self.limit / size_of::<$type>()))};
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
        pub fn $set_name<T: Sized>(&mut self, index: usize, value: $type) {
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
        HBuf {
            data_ptr: data.as_sync_mut(),
            capacity: size,
            limit: size,
            position: 0,
            destructor: Arc::new(None)
        }
    }

    ///
    /// Creates a HBuf from a pointer.
    /// Dropping the resulting HBuf will call the provided destructor function.
    /// If the HBuf is shared with other threads then the destructor may be called in any thread.
    ///
    pub unsafe fn from_raw_parts_with_destructor(data: *mut u8, size: usize, destructor: fn(*mut u8, usize)) -> HBuf {
        let data = data.as_sync_mut();
        HBuf {
            data_ptr: data,
            capacity: size,
            limit: size,
            position: 0,
            destructor: Arc::new(Some(HBufDestructor::new(data, size, HBufDestructorInfo::Destructor(destructor))))
        }
    }

    ///
    /// Allocates the given amount of memory with no particular alignment.
    /// This function panics/aborts if the amount of memory could not be allocated.
    /// (It calls std::alloc::handle_alloc_error on out of memory)
    ///
    pub fn allocate(size: usize) -> HBuf {
        HBuf::allocate_aligned(size, 1)
    }

    ///
    /// Allocates the given amount of memory with no particular alignment.
    /// This function panics/aborts if the amount of memory could not be allocated.
    /// (It calls std::alloc::handle_alloc_error on out of memory)
    ///
    pub fn allocate_zeroed(size: usize) -> HBuf {
        HBuf::allocate_aligned_zeroed(size, 1)
    }

    ///
    /// Allocates the given mount of memory with the given alignment.
    /// This function panics if the alignment is invalid.
    /// This function panics/aborts if the amount of memory could not be allocated.
    /// (It calls std::alloc::handle_alloc_error on out of memory)
    ///
    pub fn allocate_aligned_zeroed(size: usize, alignment: usize) -> HBuf {
        let mut buf =  HBuf::allocate_aligned(size, alignment);
        buf.fill(0);
        buf
    }

    ///
    /// Allocates the given mount of memory with the given alignment.
    /// This function panics if the alignment is invalid.
    /// This function panics/aborts if the amount of memory could not be allocated.
    /// (It calls std::alloc::handle_alloc_error on out of memory)
    ///
    #[allow(unreachable_code)]
    pub fn allocate_aligned(size: usize, alignment: usize) -> HBuf {
        if size == 0 {
            panic!("size is 0");
        }

        if alignment == 0 {
            panic!("alignment is 0");
        }

        let layout = Layout::from_size_align(size, alignment);
        if layout.is_err() {
            panic!("LayoutError when creating layout for size {} alignment {}", size, alignment);
        }
        let layout = layout.unwrap();
        let data = unsafe {std::alloc::alloc(layout)};
        if data.is_null() {
            std::alloc::handle_alloc_error(layout);
            panic!("handle_alloc_error failed to panic or abort after OutOfMemory!");
        }

        let data = unsafe {data.as_sync_mut()};

        HBuf {
            data_ptr: data,
            capacity: size,
            limit: size,
            position: 0,
            destructor: Arc::new(Some(HBufDestructor::new(data, size, HBufDestructorInfo::Layout(layout))))
        }
    }

    ///
    /// Allocates memory using the standard rust allocator.
    /// The memory does not have any particular alignment.
    ///
    pub fn try_allocate(size: usize) -> Result<HBuf, HBufError> {
        HBuf::try_allocate_aligned(size, 1)
    }

    ///
    /// Allocates memory using the standard rust allocator.
    /// The memory does not have any particular alignment.
    ///
    /// If the allocation is successful then it is zeroed out.
    ///
    pub fn try_allocate_zeroed(size: usize) -> Result<HBuf, HBufError> {
        let mut buf = HBuf::try_allocate_aligned(size, 1)?;
        buf.fill(0);
        Ok(buf)
    }

    ///
    /// Allocates memory using the standard rust allocator.
    /// The memory will be aligned to the given alignment.
    ///
    /// This function will fail if the allocator cannot allocate memory or allocates memory that does not have the desired alignment.
    ///
    /// If the allocation is successful then it is zeroed out.
    ///
    pub fn try_allocate_aligned_zeroed(size: usize, alignment: usize) -> Result<HBuf, HBufError> {
        let mut buf = HBuf::try_allocate_aligned(size, alignment)?;
        buf.fill(0);
        Ok(buf)
    }

    ///
    /// Allocates memory using the standard rust allocator.
    /// The memory will be aligned to the given alignment.
    ///
    /// This function will fail if the allocator cannot allocate memory or allocates memory that does not have the desired alignment.
    ///
    ///
    pub fn try_allocate_aligned(size: usize, alignment: usize) -> Result<HBuf, HBufError> {
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

        let data = unsafe {data.as_sync_mut()};

        Ok(HBuf {
            data_ptr: data,
            capacity: size,
            limit: size,
            position: 0,
            destructor: Arc::new(Some(HBufDestructor::new(data, size, HBufDestructorInfo::Layout(layout))))
        })
    }



    ///
    /// Returns the reference count of the HBuf.
    ///
    pub fn ref_count(&self) -> usize {
        Arc::strong_count(&self.destructor)
    }

    ///
    /// Returns true if this HBuf has a destructor that will run when all references to the HBuf are dropped.
    ///
    pub fn has_destructor(&self) -> bool {
        self.destructor.is_none()
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
        self.data_ptr.inner()
    }

    ///
    /// Returns a slice that is backed by the HBuf.
    /// The size of the slice is the current limit.
    ///
    pub fn as_slice(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.data_ptr.inner(), self.limit) }
    }

    ///
    /// Returns a mutable slice that is backed by the HBuf.
    /// The size of the slice is the current limit.
    ///
    pub fn as_mut_slice(&self) -> &mut [u8] {
        unsafe { std::slice::from_raw_parts_mut(self.data_ptr.inner(), self.limit) }
    }

    ///
    /// Turns this HBuf into a slice of arbitrary data.
    /// This function will return None if the alignment of T does not match the alignment of the HBuf
    ///
    pub unsafe fn as_slice_generic<T: Sized>(&self) -> Option<&[T]> {
        if self.data_ptr.align_offset(align_of::<T>()) != 0 {
            return None;
        }
        Some(std::slice::from_raw_parts(self.data_ptr.inner().cast::<T>(), self.limit / size_of::<T>()))
    }

    ///
    /// Turns this HBuf into a mutable slice of arbitrary data.
    /// This function will return None if the alignment of T does not match the alignment of the HBuf
    ///
    pub unsafe fn as_mut_slice_generic<T: Sized>(&self) -> Option<&mut [T]> {
        if self.data_ptr.align_offset(align_of::<T>()) != 0 {
            return None;
        }
        Some(std::slice::from_raw_parts_mut(self.data_ptr.inner().cast::<T>(), self.limit / size_of::<T>()))
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
        unsafe { self.data_ptr.wrapping_add(index).cast::<T>().read_unaligned() }
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

        ptr.cast::<T>().as_ref().unwrap()
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

        ptr.cast::<T>().as_mut().unwrap()
    }


    ///
    /// Sets the value at the given location to the value.
    /// The alignment of T and the memory location does not matter as this method uses "write_unaligned"
    /// to write memory.
    ///
    pub unsafe fn set<T: Sized>(&mut self, index: usize, value: T) {
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

    known_type!(usize, as_slice_usize, as_mut_slice_usize, get_usize, set_usize);
    known_type!(isize, as_slice_isize, as_mut_slice_isize, get_isize, set_isize);

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

    #[cfg(target_has_atomic = "8")]
    atomic_type!(u8, std::sync::atomic::AtomicU8, as_slice_atomic_u8, as_atomic_u8, load_u8, store_u8, swap_u8, compare_and_exchange_u8, compare_and_exchange_weak_u8);

    #[cfg(target_has_atomic = "8")]
    atomic_type!(i8, std::sync::atomic::AtomicI8, as_slice_atomic_i8, as_atomic_i8, load_i8, store_i8, swap_i8, compare_and_exchange_i8, compare_and_exchange_weak_i8);

    #[cfg(target_has_atomic = "16")]
    atomic_type!(u16, std::sync::atomic::AtomicU16, as_slice_atomic_u16, as_atomic_u16, atomic_load_u16, store_u16, swap_u16, compare_and_exchange_u16, compare_and_exchange_weak_u16);

    #[cfg(target_has_atomic = "16")]
    atomic_type!(i16, std::sync::atomic::AtomicI16, as_slice_atomic_i16, as_atomic_i16, atomic_load_i16, store_i16, swap_i16, compare_and_exchange_i16, compare_and_exchange_weak_i16);

    #[cfg(target_has_atomic = "32")]
    atomic_type!(u32, std::sync::atomic::AtomicU32, as_slice_atomic_u32, as_atomic_u32, atomic_load_u32, atomic_store_u32, atomic_swap_u32, atomic_compare_and_exchange_u32, atomic_compare_and_exchange_weak_u32);

    #[cfg(target_has_atomic = "32")]
    atomic_type!(i32, std::sync::atomic::AtomicI32, as_slice_atomic_i32, as_atomic_i32, atomic_load_i32, atomic_store_i32, atomic_swap_i32, atomic_compare_and_exchange_i32, atomic_compare_and_exchange_weak_i32);

    #[cfg(target_has_atomic = "64")]
    atomic_type!(u64, std::sync::atomic::AtomicU64, as_slice_atomic_u64, as_atomic_u64, atomic_load_u64, atomic_store_u64, atomic_swap_u64, atomic_compare_and_exchange_u64, atomic_compare_and_exchange_weak_u64);

    #[cfg(target_has_atomic = "64")]
    atomic_type!(i64, std::sync::atomic::AtomicI64, as_slice_atomic_i64, as_atomic_i64, atomic_load_i64, atomic_store_i64, atomic_swap_i64, atomic_compare_and_exchange_i64, atomic_compare_and_exchange_weak_i64);

    #[cfg(target_has_atomic = "ptr")]
    atomic_type!(usize, std::sync::atomic::AtomicUsize, as_slice_atomic_usize, as_atomic_usize, atomic_load_usize, atomic_store_usize, atomic_swap_usize, atomic_compare_and_exchange_usize, atomic_compare_and_exchange_weak_usize);

    #[cfg(target_has_atomic = "ptr")]
    atomic_type!(isize, std::sync::atomic::AtomicIsize, as_slice_atomic_isize, as_atomic_isize, atomic_load_isize, atomic_store_isize, atomic_swap_isize, atomic_compare_and_exchange_isize, atomic_compare_and_exchange_weak_isize);

     ///
    /// Returns a slice of Atomic "references" to the buffer.
    /// The "references" remain valid even if the buffer limit changes.
    ///
    /// This function requires the alignment of the buffer to match the pointer alignment.
    /// If the buffer is not properly aligned then this function returns None.
    ///
    ///
    #[cfg(target_has_atomic = "ptr")]
    #[inline]
    pub fn as_slice_atomic_ptr<T>(&self) -> Option<&[AtomicPtr<T>]> {
        if self.data_ptr.align_offset(align_of::<AtomicPtr<T>>()) != 0 {
            return None;
        }
        unsafe {
            Some(std::slice::from_raw_parts(self.data_ptr.inner().cast::<AtomicPtr<T>>(), self.limit / size_of::<AtomicPtr<T>>()))
        }
    }

    ///
    /// Returns a Atomic "reference" of a given type to a index.
    /// The "reference" remains usable even if the limit changes.
    ///
    /// This function requires the alignment of the index to match the alignment of the type.
    /// If the index is not properly aligned or the index is out of bounds then this function returns None.
    ///
    ///
    #[cfg(target_has_atomic = "ptr")]
    #[inline]
    pub fn as_atomic_ptr<T>(&self, index: usize) -> Option<&AtomicPtr<T>> {
        let sz = size_of::<AtomicPtr<T>>();
        if index+sz-1 >= self.limit {
            return None;
        }
        let ptr = self.data_ptr.wrapping_add(index);
        if ptr.align_offset(align_of::<AtomicPtr<T>>()) != 0 {
            return None;
        }
        unsafe {
            Some(<AtomicPtr<T>>::from_ptr(ptr.cast::<*mut T>()))
        }
    }

    ///
    /// Atomic "get" with memory ordering semantics.
    ///
    #[cfg(target_has_atomic = "ptr")]
    #[inline]
    pub fn atomic_load_ptr<T>(&self, index: usize, ordering: Ordering) -> *mut T {
        let sz = size_of::<AtomicPtr<T>>();
        if index+sz-1 >= self.limit {
            panic!("Index {} is out of bounds for HBuf with limit {}", index+sz-1, self.limit);
        }
        let ptr = self.data_ptr.wrapping_add(index);
        debug_assert_eq!(ptr.align_offset(align_of::<AtomicPtr<T>>()), 0);
        unsafe {
            <AtomicPtr<T>>::from_ptr(ptr.cast::<*mut T>()).load(ordering)
        }
    }

    ///
    /// Atomic "set" with memory ordering semantics.
    ///
    #[cfg(target_has_atomic = "ptr")]
    #[inline]
    pub fn atomic_store_ptr<T>(&self, index: usize, value: *mut T, ordering: Ordering) {
        let sz = size_of::<AtomicPtr<T>>();
        if index+sz-1 >= self.limit {
            panic!("Index {} is out of bounds for HBuf with limit {}", index+sz-1, self.limit);
        }
        let ptr = self.data_ptr.wrapping_add(index);
        debug_assert_eq!(ptr.align_offset(align_of::<AtomicPtr<T>>()), 0);
        unsafe {
            <AtomicPtr<T>>::from_ptr(ptr.cast::<*mut T>()).store(value, ordering)
        }
    }

    ///
    /// Atomic "swap" with memory ordering semantics.
    ///
    #[cfg(target_has_atomic = "ptr")]
    #[inline]
    pub fn atomic_swap_ptr<T>(&self, index: usize, value: *mut T, ordering: Ordering) -> *mut T {
        let sz = size_of::<AtomicPtr<T>>();
        if index+sz-1 >= self.limit {
            panic!("Index {} is out of bounds for HBuf with limit {}", index+sz-1, self.limit);
        }
        let ptr = self.data_ptr.wrapping_add(index);
        debug_assert_eq!(ptr.align_offset(align_of::<AtomicPtr<T>>()), 0);
        unsafe {
            <AtomicPtr<T>>::from_ptr(ptr.cast::<*mut T>()).swap(value, ordering)
        }
    }

    ///
    /// Atomic "compare_exchange" with memory ordering semantics.
    ///
    #[cfg(target_has_atomic = "ptr")]
    #[inline]
    pub fn atomic_compare_exchange_ptr<T>(&self, index: usize, current: *mut T, update: *mut T, success_ordering: Ordering, failure_ordering: Ordering) -> Result<*mut T, *mut T> {
        let sz = size_of::<AtomicPtr<T>>();
        if index+sz-1 >= self.limit {
            panic!("Index {} is out of bounds for HBuf with limit {}", index+sz-1, self.limit);
        }
        let ptr = self.data_ptr.wrapping_add(index);
        debug_assert_eq!(ptr.align_offset(align_of::<AtomicPtr<T>>()), 0);
        unsafe {
            <AtomicPtr<T>>::from_ptr(ptr.cast::<*mut T>()).compare_exchange(current, update, success_ordering, failure_ordering)
        }
    }

    ///
    /// Atomic "compare_exchange_weak" with memory ordering semantics.
    ///
    #[cfg(target_has_atomic = "ptr")]
    #[inline]
    pub fn atomic_compare_exchange_weak_ptr<T>(&self, index: usize, current: *mut T, update: *mut T, success_ordering: Ordering, failure_ordering: Ordering) -> Result<*mut T, *mut T> {
        let sz = size_of::<AtomicPtr<T>>();
        if index+sz-1 >= self.limit {
            panic!("Index {} is out of bounds for HBuf with limit {}", index+sz-1, self.limit);
        }
        let ptr = self.data_ptr.wrapping_add(index);
        debug_assert_eq!(ptr.align_offset(align_of::<AtomicPtr<T>>()), 0);
        unsafe {
            <AtomicPtr<T>>::from_ptr(ptr.cast::<*mut T>()).compare_exchange(current, update, success_ordering, failure_ordering)
        }
    }


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

        true
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
        true
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
    /// Resets position and limit.
    ///
    pub fn reset(&mut self) {
        self.limit = self.capacity;
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

        HBuf {
            data_ptr: unsafe {self.data_ptr.wrapping_add(off).as_sync_mut()},
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

        Some(HBuf {
            data_ptr: unsafe {self.data_ptr.wrapping_add(off).as_sync_mut()},
            capacity: length,
            limit: length,
            position: 0,
            destructor: self.destructor.clone(),
        })
    }

    fn seek_start(&mut self, from: u64) -> bool {
        if from > self.limit as u64 {
            return false;
        }

        self.position = from as usize;
        true
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
        true
    }

    fn seek_cur(&mut self, from: i64) -> bool {
        let pos = self.position as i64 + from;
        if pos < 0 {
            return false;
        }

        self.seek_start(pos as u64)
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

        Err(Error::new(ErrorKind::UnexpectedEof, "out of bounds"))
    }
}

impl Write for HBuf {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let to_copy = buf.len().min(self.position-self.limit);
        if to_copy == 0 {
            return Ok(0);
        }

        self.position = self.position + to_copy;
        Ok(to_copy)
    }

    fn flush(&mut self) -> io::Result<()> {
        //NOOP
        Ok(())
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
        Ok(())
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
        Ok(to_copy)
    }

    fn read_to_end(&mut self, buf: &mut Vec<u8>) -> io::Result<usize> {
        let to_copy = self.position-self.limit;
        if to_copy == 0 {
            return Ok(0);
        }
        let sl = unsafe { std::slice::from_raw_parts(self.data_ptr.wrapping_add(self.position), to_copy) };
        buf.write_all(sl)?;
        self.position = self.limit;
        Ok(to_copy)
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
        Ok(())
    }
}

impl Clone for HBuf {
    fn clone(&self) -> Self {
        HBuf {
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
        unsafe { &*self.data_ptr.wrapping_add(index) }
    }
}

impl IndexMut<usize> for HBuf {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        if index >= self.limit {
            panic!("Index {} is out of bounds for HBuf with limit {}", index, self.limit);
        }
        unsafe { &mut *self.data_ptr.wrapping_add(index) }
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
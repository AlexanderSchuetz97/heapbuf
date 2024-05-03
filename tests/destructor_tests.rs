

use std::ptr::null_mut;
use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

static PTR: AtomicPtr<u8> = AtomicPtr::new(null_mut());

static SZ: AtomicUsize = AtomicUsize::new(0);

fn test_it(ptr: *mut u8, sz: usize) {
    PTR.store(ptr, Ordering::SeqCst);
    assert_eq!(ptr, PTR.load(Ordering::SeqCst));
    SZ.store(sz, Ordering::SeqCst);
    assert_eq!(sz, SZ.load(Ordering::SeqCst));
}
#[test]
fn test_destructor_called() {
    let mut x = vec![0u8; 16];
    let ptr = x.as_mut_ptr();
    test_it(null_mut(), 2);


    let hb = unsafe { heap_buffer::HBuf::from_raw_parts_with_destructor(ptr, 16, test_it) };
    drop(hb);
    assert_eq!(ptr, PTR.load(Ordering::SeqCst));
    assert_eq!(16, SZ.load(Ordering::SeqCst));
}

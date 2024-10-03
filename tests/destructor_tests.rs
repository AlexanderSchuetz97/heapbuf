use heapbuf::DynDestructor;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};
use std::sync::Arc;

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
    PTR.store(null_mut(), Ordering::SeqCst);
    SZ.store(0, Ordering::SeqCst);

    let mut x = vec![0u8; 16];
    let ptr = x.as_mut_ptr();
    test_it(null_mut(), 2);


    let hb = unsafe { heapbuf::HBuf::from_raw_parts_with_destructor(ptr, 16, test_it) };
    let hb = std::hint::black_box(hb);
    drop(hb);
    assert_eq!(ptr, PTR.load(Ordering::SeqCst));
    assert_eq!(16, SZ.load(Ordering::SeqCst));
}


#[derive(Debug, Default, Clone)]
struct DynDes1(Arc<AtomicUsize>);

#[derive(Debug, Default, Clone)]
struct DynDes2(Arc<AtomicUsize>);

impl DynDestructor for DynDes1 {
    fn destroy(&mut self, ptr: *mut u8, size: usize) {
        self.0.fetch_add(1, Ordering::SeqCst);
        PTR.store(ptr, Ordering::SeqCst);
        SZ.store(size, Ordering::SeqCst);
    }
}

impl DynDestructor for DynDes2 {
    fn destroy(&mut self, ptr: *mut u8, size: usize) {
        self.0.fetch_add(2, Ordering::SeqCst);
        PTR.store(ptr, Ordering::SeqCst);
        SZ.store(size, Ordering::SeqCst);
    }
}
#[test]
fn test_dyn_destructor_called() {
    let mut x = vec![0u8; 16];
    let ptr = x.as_mut_ptr();
    let des1 = DynDes1::default();

    PTR.store(null_mut(), Ordering::SeqCst);
    SZ.store(0, Ordering::SeqCst);


    let hb = unsafe { heapbuf::HBuf::from_raw_parts_with_dyn_destructor(ptr, 16, Box::new(des1.clone())) };
    let hb = std::hint::black_box(hb);
    drop(hb);

    assert_eq!(ptr, PTR.load(Ordering::SeqCst));
    assert_eq!(16, SZ.load(Ordering::SeqCst));

    PTR.store(null_mut(), Ordering::SeqCst);
    SZ.store(0, Ordering::SeqCst);

    let des2 = DynDes2::default();
    let hb = unsafe { heapbuf::HBuf::from_raw_parts_with_dyn_destructor(ptr, 16, Box::new(des2.clone())) };
    let hb = std::hint::black_box(hb);
    drop(hb);

    assert_eq!(ptr, PTR.load(Ordering::SeqCst));
    assert_eq!(16, SZ.load(Ordering::SeqCst));

    PTR.store(null_mut(), Ordering::SeqCst);
    SZ.store(0, Ordering::SeqCst);

    assert_eq!(des1.0.load(Ordering::SeqCst), 1);
    assert_eq!(des2.0.load(Ordering::SeqCst), 2);
}

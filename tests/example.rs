use std::mem::{align_of, size_of};
use std::sync::atomic::{AtomicU16, AtomicU32, Ordering};
use std::thread;
use std::time::Duration;
use heapbuf::*;



#[test]
pub fn test() {
    let mut x : HBuf = HBuf::allocate(512);
    let _ : &[u8] = x.as_slice();
    let _ : &mut [u8] = x.as_mut_slice();
    x[0] = 1u8;
    assert_eq!(1, x[0]);

    //This option is None if the alignment is not 4. We did not specify alignment when allocating
    //So we may or may not be 4 byte aligned. Depends on the OS
    let _ : Option<&[u32]> = x.as_slice_u32();
    let x : HBuf = HBuf::allocate_aligned(512, 4);
    let _ : &[u32] = x.as_slice_u32().unwrap(); //This is safe because our alignment is 4!
    let _ : &[i16] = x.as_slice_i16().unwrap();
    let _ : &[f32] = x.as_slice_f32().unwrap();
    //Other supported types for slices are u16-u128, i8-i128, f32, f64
    //From other crates depending on selected crate features: f16, f128, u24...,


    //Example construct from pointer
    let some_vec: Vec<u8> = vec![123u8; 4096];
    let mut some_vec: std::mem::ManuallyDrop<Vec<u8>>  = std::mem::ManuallyDrop::new(some_vec);
    let some_pointer: *mut u8 = some_vec.as_mut_ptr();
    fn dealloc_vec(ptr: *mut u8 , size: usize) {
        //This just deallocates a Vec... This could also call a C function
        unsafe {
            drop(Vec::from_raw_parts(ptr, size, size));
        }
    }

    let buf : HBuf = unsafe {
        HBuf::from_raw_parts_with_destructor(some_pointer, some_vec.capacity(), dealloc_vec)
    };

    drop(buf); //Will run dealloc_vec and destroy the allocated vec.


    //Example reference counting
    let mut x : HBuf = HBuf::allocate_aligned(31, 4);
    let x2 = x.clone(); //Does not clone the heap buffer, only creates another reference. just lice Rc.clone()
    assert_eq!(x.ref_count(), 2);
    assert_eq!(x2.ref_count(), 2);
    x[0] = 1;
    drop(x); //Would not run any destructors since there is still 1 reference.
    assert_eq!(x2.ref_count(), 1);
    assert_eq!(x2[0], 1);
    drop(x2); //Will deallocate and run destructors since there are no more references.


    //Example Threading
    let x : HBuf = HBuf::allocate_aligned_zeroed(32, 4);
    let x2 = x.clone(); //This is Send/Sync
    let handle = thread::spawn(move || {
        loop {
            //You have to use the atomic operations if you use more than one thread
            //If you use the normal operations with different threads involved then there is no guarantee
            //When/That the threads will see each other's changes!
            //This is a busy waiting loop that you should never do btw!
            if x2.atomic_load_u32(4, Ordering::SeqCst) == 420 {
                return;
            }
        }
    });
    thread::sleep(Duration::from_millis(500));
    x.atomic_store_u32(4, 420, Ordering::SeqCst);
    handle.join().unwrap();
    //Option is None if alignment mismatches!
    let _ : &[AtomicU32] = x.as_slice_atomic_u32().unwrap();
    let _ : &[AtomicU16] = x.as_slice_atomic_u16().unwrap();
    let _ : &AtomicU32 = x.as_atomic_u32(4).unwrap();
    x.as_atomic_u32(8).unwrap().store(24, Ordering::SeqCst);
    assert_eq!(24, x.as_slice_u32().unwrap()[2]);

    //Example reading/writing structs
    #[derive(Clone, Copy)] //Only structs with the Copy trait can be read!
    struct Test {
        member1: u32,
        member2: u64,
    }
    let mut x: HBuf = HBuf::allocate_aligned_zeroed(size_of::<Test>() * 2, align_of::<Test>());
    unsafe {
        //This is not the best idea on structs that have Drop logic.
        //Use with pure data structs only!
        x.set(0, Test { member1: 12, member2: 24 });
        let my_struct : Test =  x.get::<Test>(0);
        assert_eq!(my_struct.member1, 12);
        assert_eq!(my_struct.member2, 24);

        //Reads the second struct
        let my_struct : Test =  x.get::<Test>(size_of::<Test>());
        //The exact numbers depend on the system (alignment/endian etc.)
        assert_eq!(my_struct.member1, 0);
        assert_eq!(my_struct.member2, 0);
        let slice: &[Test] = x.as_slice_generic().unwrap(); //Only succeeds if buffer is properly aligned
        assert_eq!(slice.len(), 2);
    }
}
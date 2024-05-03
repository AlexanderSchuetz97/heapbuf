use std::io::{Read};
use std::thread;
use half::f16;
use lazy_static::lazy_static;
use uintx::u24;
use heap_buffer::*;

#[test]
fn test_alloc() -> std::io::Result<()> {
    let mut buf = HBuf::allocate_aligned(512,4096)?;
    assert_eq!(0, buf.as_ptr().align_offset(4096));
    println!("{}", buf[0]);
    buf[0] = 0x44;
    println!("{}", buf[0]);
    let u = unsafe { buf.as_mut_slice_generic::<u32>().unwrap() };
    assert_eq!(u.len(), 512/4);
    assert_eq!(u[0], 0x44);
    buf[1] = 0x32;
    let u = unsafe { buf.as_mut_slice_generic::<u32>().unwrap() };

    assert_eq!(u[0], 0x3244);
    assert_eq!(8usize/3usize, 2usize);

    let f = unsafe { buf.as_mut_slice_generic::<f16>().unwrap() };

    assert_eq!("0.19580078", format!("{}", f[0]));
    drop(buf);
    return Ok(());
}

lazy_static! {
    static ref THE_BUF: HBuf = HBuf::allocate(512).unwrap();
}

#[test]
fn test_mt() -> std::io::Result<()> {
    let t = thread::spawn(|| {
        let mut x = vec![0u8; 16];
        THE_BUF.clone().read_exact(x.as_mut_slice()).expect("Failed");

    });

    t.join().expect("Failed");
    return Ok(());
}

#[test]
fn test_unaligned() -> std::io::Result<()> {
    let mut buf = HBuf::allocate(512)?;
    buf[0] = 0x24;
    buf[1] = 0x23;
    buf[2] = 0x22;
    buf[3] = 0x44;
    buf[4] = 0x25;

    let x = buf.split(1, buf.capacity()-1);
    let z = x.get_u32(0);
    assert_eq!(z, 0x25442223u32.to_le());
    assert!(x.as_slice_u32().is_none());
    assert!(x.as_slice_u24().is_some());
    let n = x.as_slice_u24().unwrap()[0];
    assert_eq!(n, u24::from_ne_bytes([0x23,0x22, 0x44]));

    return Ok(());
}
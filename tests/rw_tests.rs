use std::io::{ErrorKind, Seek, SeekFrom};

use rw_utils::num_read::NumRead;
use rw_utils::num_write::NumWrite;

use heapbuf::HBuf;

#[test]
fn test_read_write() -> std::io::Result<()> {
    let mut buf = HBuf::allocate_zeroed(512);
    buf[120] = 12;
    buf.seek(SeekFrom::Start(120))?;

    assert_eq!(buf.position(), 120);
    assert_eq!(12, buf.read_u8()?);
    assert_eq!(buf.position(), 121);
    assert_eq!(0, buf.read_u8()?);
    assert_eq!(buf.position(), 122);

    return Ok(());
}

#[test]
fn test_seek() -> std::io::Result<()> {
    let mut buf = HBuf::allocate_zeroed(12);
    buf.seek(SeekFrom::Start(12))?;
    assert_eq!(buf.position(), 12);
    buf.seek(SeekFrom::Start(10))?;
    assert_eq!(buf.position(), 10);
    let err = buf.seek(SeekFrom::Current(3));
    assert_eq!(buf.position(), 10);
    match err.unwrap_err().kind() {
        ErrorKind::UnexpectedEof => {}
        _ => panic!("Unexpected error")
    }
    let err = buf.seek(SeekFrom::Current(-11));
    match err.unwrap_err().kind() {
        ErrorKind::UnexpectedEof => {}
        _ => panic!("Unexpected error")
    }
    assert_eq!(buf.position(), 10);
    buf.seek(SeekFrom::Current(-8))?;
    assert_eq!(buf.position(), 2);
    let err = buf.seek(SeekFrom::End(12));
    match err.unwrap_err().kind() {
        ErrorKind::UnexpectedEof => {}
        _ => panic!("Unexpected error")
    }
    assert_eq!(buf.position(), 2);
    let err = buf.seek(SeekFrom::Current(11));
    match err.unwrap_err().kind() {
        ErrorKind::UnexpectedEof => {}
        _ => panic!("Unexpected error")
    }
    assert_eq!(buf.position(), 2);

    return Ok(());
}


#[test]
fn test_limit() -> std::io::Result<()> {
    let mut buf = HBuf::allocate_zeroed(113);
    assert_eq!(buf.position(), 0);
    assert_eq!(buf.limit(), 113);
    buf.write_u128_le(123456)?;
    assert_eq!(buf.position(), 16);
    buf.flip();
    assert_eq!(buf.position(), 0);
    assert_eq!(buf.limit(), 16);
    assert_eq!(buf.read_u128_le()?, 123456);
    let err = buf.read_u128_le();
    assert_eq!(err.is_err(), true);
    match err.unwrap_err().kind() {
        ErrorKind::UnexpectedEof => {}
        _ => panic!("Unexpected error")
    }
    assert_eq!(buf.position(), 16);
    assert_eq!(buf.limit(), 16);
    let err = buf.write_u8(1);
    assert_eq!(err.is_err(), true);
    match err.unwrap_err().kind() {
        ErrorKind::UnexpectedEof => {}
        _ => panic!("Unexpected error")
    }
    assert_eq!(buf.position(), 16);
    assert_eq!(buf.limit(), 16);

    buf.seek(SeekFrom::End(-3))?;
    assert_eq!(buf.position(), 13);
    let err = buf.read_u32_le();
    match err.unwrap_err().kind() {
        ErrorKind::UnexpectedEof => {}
        _ => panic!("Unexpected error")
    }
    assert_eq!(buf.position(), 13);
    buf.reset();
    assert_eq!(buf.position(), 0);
    assert_eq!(buf.limit(), 113);

    return Ok(());
}
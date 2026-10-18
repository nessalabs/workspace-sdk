//! Actual socket framing boundaries. Timeout classification does not establish
//! whether a partially sent request was dispatched, and these fixtures do not
//! establish the cause of historical PTY failures.
use super::{read_frame, write_frame, FRAME_LIMIT};
use crate::Error;
use std::{
    io::{ErrorKind, Read, Write},
    os::{fd::AsRawFd, unix::net::UnixStream},
    thread,
    time::{Duration, Instant},
};

#[test]
fn held_open_empty_frame_read_reports_timeout() {
    let (mut reader, _held_writer) = UnixStream::pair().unwrap();
    let started = Instant::now();
    let result = read_frame(&mut reader, Duration::from_millis(40));
    assert!(matches!(result, Err(Error::Timeout(_))), "{result:?}");
    assert!(started.elapsed() < Duration::from_secs(3));
}

#[test]
fn partial_frame_with_held_writer_reports_timeout_without_acknowledging_completion() {
    let (mut reader, mut held_writer) = UnixStream::pair().unwrap();
    held_writer.write_all(b"{\"partial\":true}").unwrap();
    let started = Instant::now();
    let result = read_frame(&mut reader, Duration::from_millis(40));
    assert!(matches!(result, Err(Error::Timeout(_))), "{result:?}");
    assert!(started.elapsed() < Duration::from_secs(3));
}

#[test]
fn blocked_frame_write_reports_timeout_after_partial_delivery() {
    let (mut writer, mut held_reader) = UnixStream::pair().unwrap();
    let requested: libc::c_int = 1024;
    let mut capacity: libc::c_int = 0;
    let mut length = std::mem::size_of_val(&capacity) as libc::socklen_t;
    assert_eq!(
        unsafe {
            libc::setsockopt(
                writer.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_SNDBUF,
                (&requested as *const libc::c_int).cast(),
                std::mem::size_of_val(&requested) as libc::socklen_t,
            )
        },
        0
    );
    assert_eq!(
        unsafe {
            libc::getsockopt(
                writer.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_SNDBUF,
                (&mut capacity as *mut libc::c_int).cast(),
                &mut length,
            )
        },
        0
    );
    assert_eq!(length as usize, std::mem::size_of_val(&capacity));
    let mut frame = vec![b'x'; FRAME_LIMIT];
    *frame.last_mut().unwrap() = b'\n';
    assert!(capacity > 0 && (capacity as usize) < frame.len());
    // Fill this owned socket with an actual request prefix first. Starting the
    // helper against a confirmed full buffer exercises its write-error path,
    // rather than only its next-iteration absolute-deadline check after a short
    // write. The peer deliberately drains nothing until the helper returns.
    writer.set_nonblocking(true).unwrap();
    let mut prefix = 0;
    loop {
        match writer.write(&frame[prefix..]) {
            Ok(0) => panic!("fixture socket closed while preparing backpressure"),
            Ok(size) => {
                prefix += size;
                assert!(
                    prefix < frame.len(),
                    "fixture send buffer did not constrain the frame"
                );
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock => break,
            Err(error) => panic!("unexpected fixture write failure: {error}"),
        }
    }
    assert!(prefix > 0);
    writer.set_nonblocking(false).unwrap();
    let started = Instant::now();
    let result = write_frame(&mut writer, &frame[prefix..]);
    assert!(matches!(result, Err(Error::Timeout(_))), "{result:?}");
    assert!(started.elapsed() < Duration::from_secs(3));
    // A timeout must not imply that nothing was delivered or permit replay.
    held_reader.set_nonblocking(true).unwrap();
    let mut delivered = Vec::new();
    let mut chunk = [0; 4096];
    loop {
        match held_reader.read(&mut chunk) {
            Ok(0) => break,
            Ok(size) => {
                delivered.extend_from_slice(&chunk[..size]);
                assert!(delivered.len() <= frame.len());
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock => break,
            Err(error) => panic!("unexpected fixture read failure: {error}"),
        }
    }
    assert!(delivered.len() >= prefix);
    assert!(delivered.len() < frame.len());
    assert_eq!(delivered, frame[..delivered.len()]);
}

#[test]
fn complete_newline_frame_round_trips_without_timeout() {
    let (mut writer, mut reader) = UnixStream::pair().unwrap();
    let bytes = b"{\"complete\":true}\n";
    write_frame(&mut writer, bytes).unwrap();
    assert_eq!(
        read_frame(&mut reader, Duration::from_secs(2)).unwrap(),
        bytes
    );
}

fn boundary_frame(size: usize) -> (super::Result<Vec<u8>>, std::io::Result<()>) {
    let (mut reader, mut writer) = UnixStream::pair().unwrap();
    writer
        .set_write_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut frame = vec![b'x'; size];
    *frame.last_mut().unwrap() = b'\n';
    thread::scope(|scope| {
        let producer = scope.spawn(move || writer.write_all(&frame));
        let result = read_frame(&mut reader, Duration::from_secs(2));
        // Join before assertions. The producer has a socket timeout even when
        // framing refuses bytes; it cannot outlive its disposable owned peer.
        let sent = producer.join().unwrap();
        (result, sent)
    })
}

#[test]
fn frame_size_boundary_remains_exact() {
    let (exact, sent) = boundary_frame(FRAME_LIMIT);
    sent.unwrap();
    let exact = exact.unwrap();
    assert_eq!(exact.len(), FRAME_LIMIT);
    assert_eq!(exact.last(), Some(&b'\n'));
    assert!(exact[..FRAME_LIMIT - 1].iter().all(|byte| *byte == b'x'));
    let (over, sent) = boundary_frame(FRAME_LIMIT + 1);
    sent.unwrap();
    assert!(matches!(over, Err(Error::InvalidInput(_))), "{over:?}");
}

#[test]
fn closed_peer_and_partial_eof_remain_incomplete_state_errors() {
    for partial in [false, true] {
        let (mut reader, mut writer) = UnixStream::pair().unwrap();
        if partial {
            writer.write_all(b"partial without newline").unwrap();
        }
        // Retain the peer while closing its write half: macOS rejects setting
        // SO_RCVTIMEO after the peer is fully closed, before read can observe EOF.
        writer.shutdown(std::net::Shutdown::Write).unwrap();
        let result = read_frame(&mut reader, Duration::from_secs(2));
        assert!(matches!(result, Err(Error::State(_))), "{result:?}");
    }
}

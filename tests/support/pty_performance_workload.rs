//! Standalone fixed-record producer with independently observable input/resize.
use std::{
    fs,
    io::{self, Read, Write},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};
fn main() {
    assert!(Command::new("/bin/stty")
        .args(["-echo", "-icanon", "min", "1", "time", "0", "-opost"])
        .stdin(Stdio::inherit())
        .status()
        .unwrap()
        .success());
    fs::write("ready", b"ready").unwrap();
    thread::spawn(|| {
        let mut input = io::stdin().lock();
        loop {
            let mut byte = [0];
            input.read_exact(&mut byte).unwrap();
            match byte[0] {
                b'i' => fs::write("input-ack", b"accepted").unwrap(),
                b'r' => {
                    let size = Command::new("/bin/stty")
                        .arg("size")
                        .stdin(Stdio::inherit())
                        .output()
                        .unwrap();
                    assert!(size.status.success());
                    fs::write("resize-ack", size.stdout).unwrap();
                }
                _ => panic!("unexpected input"),
            }
        }
    });
    let started = Instant::now();
    let mut output = io::stdout().lock();
    for sequence in 0.. {
        let mut record = [b'x'; 1024];
        record[..8].copy_from_slice(format!("{sequence:08}").as_bytes());
        record[1023] = b'\n';
        output.write_all(&record).unwrap();
        output.flush().unwrap();
        assert!(started.elapsed() < Duration::from_secs(25));
        thread::sleep(Duration::from_millis(1));
    }
}

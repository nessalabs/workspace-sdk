//! Native workload commands shared by SDK, provider, and runtime integration tests.
use std::env;
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::Path;
use std::process::{self, Command};
use std::thread;
use std::time::Duration;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--version") => println!("runtime-fixture 1.0.0"),
        Some("echo") => println!("{}", args[1..].join(" ")),
        Some("sleep") => {
            sleep(&args[1]);
            if let Some(message) = args.get(2) {
                println!("{message}");
            }
        }
        Some("sleep-print") => {
            println!("{}", args[2]);
            io::stdout().flush().unwrap();
            sleep(&args[1]);
            println!("{}", args[3]);
        }
        Some("append") => {
            if let Some(before) = args.get(3) {
                sleep(before);
            }
            let mut file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&args[1])
                .unwrap();
            file.write_all(args[2].as_bytes()).unwrap();
            file.sync_all().unwrap();
            if let Some(after) = args.get(4) {
                sleep(after);
            }
            if let Some(message) = args.get(5) {
                println!("{message}");
            }
        }
        Some("increment") => {
            let value = read_counter(Path::new(&args[1]));
            let increment: i64 = args[2].parse().unwrap();
            fs::write(&args[1], (value + increment).to_string()).unwrap();
        }
        Some("counter") => {
            let path = Path::new(&args[1]);
            let limit: i64 = args[2].parse().unwrap();
            let mut value = read_counter(path);
            while value < limit {
                value += 1;
                fs::write(path, value.to_string()).unwrap();
                sleep(&args[3]);
            }
        }
        Some("spawn-descendant") => {
            let child = Command::new(env::current_exe().unwrap())
                .args(["delayed-write", &args[1], &args[2], "bad"])
                .spawn()
                .unwrap();
            println!("ready");
            io::stdout().flush().unwrap();
            sleep(args.get(3).map(String::as_str).unwrap_or("30"));
            // Dropping this handle deliberately does not cancel the descendant.
            // The provider test verifies the supervisor cancels the whole group.
            drop(child);
        }
        Some("delayed-write") => {
            sleep(&args[2]);
            fs::write(&args[1], &args[3]).unwrap();
        }
        Some("envprint") => println!("{}", env::var(&args[1]).unwrap_or_default()),
        Some("env-length") => {
            let expected: usize = args[2].parse().unwrap();
            assert_eq!(env::var(&args[1]).unwrap().len(), expected);
        }
        Some("writefile") => fs::write(&args[1], &args[2]).unwrap(),
        Some("largeoutput") => {
            let count: usize = args[1].parse().unwrap();
            let mut output = io::stdout().lock();
            let block = [b'x'; 8192];
            for _ in 0..count / block.len() {
                output.write_all(&block).unwrap();
            }
            output.write_all(&block[..count % block.len()]).unwrap();
            if let Some(suffix) = args.get(2) {
                output.write_all(suffix.as_bytes()).unwrap();
            }
            output.flush().unwrap();
            if let Some(error) = args.get(3) {
                io::stderr().write_all(error.as_bytes()).unwrap();
            }
        }
        Some("stdin-echo") => {
            let input = read_stdin();
            io::stdout().write_all(&input).unwrap();
        }
        Some("stdin-count") => println!("{}", read_stdin().len()),
        Some("train") => {
            assert_eq!(env::var("FIXTURE_TOKEN").unwrap().len(), 15);
            println!("training completed");
        }
        Some("native-stdin") => {
            assert_eq!(read_stdin().len(), 13);
            println!("native input accepted");
        }
        Some("login") => {
            println!("use-device-code-fixture");
            io::stdout().flush().unwrap();
            thread::sleep(Duration::from_millis(200));
        }
        Some("cwd") => println!("{}", env::current_dir().unwrap().display()),
        _ => process::exit(2),
    }
}

fn sleep(seconds: &str) {
    thread::sleep(Duration::from_secs_f64(seconds.parse().unwrap()));
}

fn read_counter(path: &Path) -> i64 {
    match fs::read_to_string(path) {
        Ok(value) => value.trim().parse().unwrap(),
        Err(error) if error.kind() == io::ErrorKind::NotFound => 0,
        Err(error) => panic!("counter read failed: {error}"),
    }
}

fn read_stdin() -> Vec<u8> {
    let mut input = Vec::new();
    io::stdin().read_to_end(&mut input).unwrap();
    input
}

use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use windows_sys::Win32::Foundation::SYSTEMTIME;
use windows_sys::Win32::System::SystemInformation::GetLocalTime;

struct Logger {
    file: File,
    path: PathBuf,
}

static LOGGER: OnceLock<Mutex<Logger>> = OnceLock::new();
static VERBOSE: OnceLock<bool> = OnceLock::new();

fn default_path() -> PathBuf {
    std::env::temp_dir().join("awcc-lite.log")
}

fn open_file(path: &PathBuf) -> Option<File> {
    OpenOptions::new().create(true).append(true).open(path).ok()
}

fn rotate(path: &PathBuf) {
    let Ok(metadata) = std::fs::metadata(path) else {
        return;
    };
    if metadata.len() <= 2 * 1024 * 1024 {
        return;
    }
    let previous = path.with_file_name("awcc-lite.old.log");
    let _ = std::fs::remove_file(&previous);
    let _ = std::fs::rename(path, previous);
}

pub fn init() {
    let _ = VERBOSE.set(
        std::env::var_os("AWCC_LOG_VERBOSE").is_some_and(|value| !value.is_empty() && value != "0"),
    );

    let mut candidates = vec![default_path()];
    if let Ok(directory) = std::env::current_dir() {
        candidates.push(directory.join("awcc-lite.log"));
    }
    let mut opened = None;
    for path in candidates {
        rotate(&path);
        if let Some(file) = open_file(&path) {
            opened = Some(Logger { file, path });
            break;
        }
    }

    let Some(logger) = opened else {
        return;
    };
    let path = logger.path.display().to_string();
    let _ = LOGGER.set(Mutex::new(logger));
    info(format_args!("=== awcc-lite started ==="));
    info(format_args!("log file: {path}"));
}

pub fn path() -> String {
    LOGGER
        .get()
        .and_then(|logger| {
            logger
                .lock()
                .ok()
                .map(|logger| logger.path.display().to_string())
        })
        .unwrap_or_else(|| default_path().display().to_string())
}

pub fn info(args: fmt::Arguments) {
    write("INFO", args);
}

pub fn warn(args: fmt::Arguments) {
    write("WARN", args);
}

pub fn error(args: fmt::Arguments) {
    write("ERROR", args);
}

pub fn debug(args: fmt::Arguments) {
    if *VERBOSE.get().unwrap_or(&false) {
        write("DEBUG", args);
    }
}

fn write(level: &str, args: fmt::Arguments) {
    let Some(logger) = LOGGER.get() else {
        return;
    };
    let Ok(mut logger) = logger.lock() else {
        return;
    };

    let mut now: SYSTEMTIME = unsafe { std::mem::zeroed() };
    unsafe {
        GetLocalTime(&mut now);
    }
    let thread = std::thread::current().id();
    let _ = writeln!(
        logger.file,
        "{:02}:{:02}:{:02}.{:03} [{thread:?}] {level} {args}",
        now.wHour, now.wMinute, now.wSecond, now.wMilliseconds
    );
    let _ = logger.file.flush();
}

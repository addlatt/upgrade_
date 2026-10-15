//! The console the scripts drew on (/dev/tty6): opened raw once for the
//! whole wait, keys read from a non-blocking descriptor against a
//! monotonic deadline, the terminal put back after. What the scripts'
//! Python did (verify.sh 0.5.0 and 0.4.1: a key pressed during bash's
//! `read -t` left it blocked on the rig, 2026-09-26).

use std::time::{Duration, Instant};

pub struct Console {
    path: String,
    fd: std::cell::Cell<i32>,
    pub reopens: std::cell::Cell<u32>,
    /// the bytes that ended the last wait as a key, for the log
    pub key_bytes: std::cell::RefCell<Vec<u8>>,
    /// cursor-position reports read and not taken as a key, for the log
    pub reports: std::cell::RefCell<Vec<Vec<u8>>>,
    old: libc::termios,
    pub cols: u16,
    pub rows: u16,
}

pub fn is_char_device(path: &str) -> bool {
    use std::os::unix::fs::FileTypeExt;
    std::fs::metadata(path).map(|m| m.file_type().is_char_device()).unwrap_or(false)
}

impl Console {
    pub fn open(path: &str) -> Result<Console, String> {
        let c = std::ffi::CString::new(path).map_err(|e| e.to_string())?;
        let fd = unsafe { libc::open(c.as_ptr(), libc::O_RDWR | libc::O_NOCTTY | libc::O_NONBLOCK) };
        if fd < 0 {
            return Err(format!("{path}: {}", std::io::Error::last_os_error()));
        }
        let mut old: libc::termios = unsafe { std::mem::zeroed() };
        if unsafe { libc::tcgetattr(fd, &mut old) } != 0 {
            let e = std::io::Error::last_os_error();
            unsafe { libc::close(fd) };
            return Err(format!("{path}: tcgetattr: {e}"));
        }
        let mut raw = old;
        raw.c_lflag &= !(libc::ICANON | libc::ECHO | libc::ISIG);
        raw.c_cc[libc::VMIN] = 0;
        raw.c_cc[libc::VTIME] = 0;
        unsafe {
            libc::tcsetattr(fd, libc::TCSANOW, &raw);
            libc::tcflush(fd, libc::TCIFLUSH);
        }
        let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
        unsafe { libc::ioctl(fd, libc::TIOCGWINSZ, &mut ws) };
        Ok(Console { path: path.to_string(), fd: std::cell::Cell::new(fd), reopens: std::cell::Cell::new(0), key_bytes: std::cell::RefCell::new(Vec::new()), reports: std::cell::RefCell::new(Vec::new()), old, cols: ws.ws_col, rows: ws.ws_row })
    }

    /// The same console opened again, raw as before, its input flushed; the
    /// old descriptor closed. The size is not set again: a resize made
    /// before stays on the console itself.
    fn reopen(&self) -> Result<(), String> {
        let c = std::ffi::CString::new(self.path.as_str()).map_err(|e| e.to_string())?;
        let fd = unsafe { libc::open(c.as_ptr(), libc::O_RDWR | libc::O_NOCTTY | libc::O_NONBLOCK) };
        if fd < 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let mut raw = self.old;
        raw.c_lflag &= !(libc::ICANON | libc::ECHO | libc::ISIG);
        raw.c_cc[libc::VMIN] = 0;
        raw.c_cc[libc::VTIME] = 0;
        unsafe {
            libc::tcsetattr(fd, libc::TCSANOW, &raw);
            libc::tcflush(fd, libc::TCIFLUSH);
            libc::close(self.fd.get());
        }
        self.fd.set(fd);
        Ok(())
    }

    /// A console the kernel reports narrower than 40 columns is set to 80x25.
    pub fn resize(&mut self, rows: u16, cols: u16) -> Result<(), String> {
        let ws = libc::winsize { ws_row: rows, ws_col: cols, ws_xpixel: 0, ws_ypixel: 0 };
        if unsafe { libc::ioctl(self.fd.get(), libc::TIOCSWINSZ, &ws) } != 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let mut got: libc::winsize = unsafe { std::mem::zeroed() };
        unsafe { libc::ioctl(self.fd.get(), libc::TIOCGWINSZ, &mut got) };
        self.rows = got.ws_row;
        self.cols = got.ws_col;
        Ok(())
    }

    /// Write all of `text`. Like the scripts' Python: "try again" (EAGAIN) is
    /// retried every 10 ms, an interrupted call at once; any other error ends
    /// the write. A console hung up after it was opened answers every write
    /// with EIO (the rig, 2026-10-14: a shell started on tty6 after the
    /// program opened it); retrying that for ever kept the machine from
    /// restarting.
    pub fn put(&self, text: &str) -> Result<(), String> {
        let mut data = text.as_bytes();
        while !data.is_empty() {
            let n = unsafe { libc::write(self.fd.get(), data.as_ptr() as *const _, data.len()) };
            if n > 0 {
                data = &data[n as usize..];
                continue;
            }
            let e = std::io::Error::last_os_error();
            match e.raw_os_error() {
                Some(libc::EAGAIN) => std::thread::sleep(Duration::from_millis(10)),
                Some(libc::EINTR) => {}
                _ if n == 0 => return Err("the console took no bytes".into()),
                _ => return Err(e.to_string()),
            }
        }
        Ok(())
    }

    /// A key within `timeout`: true when one came.
    pub fn key_within(&self, timeout: Duration) -> bool {
        let mut pfd = libc::pollfd { fd: self.fd.get(), events: libc::POLLIN, revents: 0 };
        let ms = timeout.as_millis().min(i32::MAX as u128) as i32;
        let r = unsafe { libc::poll(&mut pfd, 1, ms) };
        if r > 0 {
            let mut buf = [0u8; 64];
            let n = unsafe { libc::read(self.fd.get(), buf.as_mut_ptr() as *mut _, buf.len()) };
            if n > 0 {
                let got = &buf[..n as usize];
                if only_cursor_reports(got) {
                    // the console's answer to another program's "where is the cursor?"
                    // (the installer's shell on tty6 asks as it starts, 2026-10-15), not a key
                    self.reports.borrow_mut().push(got.to_vec());
                    return false;
                }
                *self.key_bytes.borrow_mut() = got.to_vec();
            }
            return n > 0;
        }
        false
    }

    /// Draw `screen(left)` every second until the deadline or a key.
    /// Ok((true, _)) when a key cut it short; Err when the console could not
    /// be written (the scripts' Python died there, exit 1).
    pub fn wait(&self, seconds: f64, screen: &dyn Fn(u64) -> String, fine: bool) -> Result<(bool, f64), String> {
        let end = Instant::now() + Duration::from_secs_f64(seconds);
        let mut keyed = false;
        loop {
            let left = end.saturating_duration_since(Instant::now()).as_secs_f64();
            if left <= 0.0 {
                break;
            }
            let whole = left.ceil() as u64;
            if let Err(e) = self.put(&screen(whole)) {
                // Fedora's installer starts a shell on a console the moment it is
                // switched to, and that shell hangs the console up as it starts: an
                // open made before it is dead (the rig, 2026-10-14). Open it again,
                // set it up the same way, and draw again against the same deadline.
                // Three times at most; then the caller decides (a refusal restarts,
                // the countdown cancels).
                if self.reopens.get() >= 3 {
                    return Err(e);
                }
                self.reopens.set(self.reopens.get() + 1);
                self.reopen().map_err(|r| format!("{e}; opened again: {r}"))?;
                continue;
            }
            let timeout = if fine { (left - (whole as f64 - 1.0)).max(0.05) } else { left.min(1.0) };
            if self.key_within(Duration::from_secs_f64(timeout)) {
                keyed = true;
                break;
            }
        }
        let elapsed = seconds - end.saturating_duration_since(Instant::now()).as_secs_f64().max(0.0);
        Ok((keyed, elapsed))
    }
}

/// True when `bytes` is one or more cursor-position reports and nothing
/// else: `ESC [ <digits> ; <digits> R`, each. A Linux console's keyboard
/// never sends that shape (its keys are ESC [ [ A, ESC [ 1 ~ and the like);
/// the console writes it into its own input when a program asks where the
/// cursor is. Anything else in the read, or a report cut short, is a key.
pub fn only_cursor_reports(bytes: &[u8]) -> bool {
    let mut i = 0;
    let mut seen = 0;
    while i < bytes.len() {
        if bytes.get(i) != Some(&0x1b) || bytes.get(i + 1) != Some(&b'[') {
            return false;
        }
        i += 2;
        let row = bytes[i..].iter().take_while(|b| b.is_ascii_digit()).count();
        if row == 0 || bytes.get(i + row) != Some(&b';') {
            return false;
        }
        i += row + 1;
        let col = bytes[i..].iter().take_while(|b| b.is_ascii_digit()).count();
        if col == 0 || bytes.get(i + col) != Some(&b'R') {
            return false;
        }
        i += col + 1;
        seen += 1;
    }
    seen > 0
}

/// `printf '...' > /dev/tty6`: a fresh open each time, as bash's redirect
/// was; an error is ignored, as bash went on after a failed redirect.
pub fn say_on(path: &str, text: &str) {
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().write(true).open(path) {
        let _ = f.write_all(text.as_bytes());
    }
}

impl Drop for Console {
    fn drop(&mut self) {
        unsafe {
            libc::tcsetattr(self.fd.get(), libc::TCSANOW, &self.old);
            libc::close(self.fd.get());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A pseudo-terminal pair and the slave's path (ttyname_r: the tests run
    /// on several threads, and ttyname's one buffer is shared between them).
    fn pty() -> (i32, i32, String) {
        let (mut master, mut slave) = (0, 0);
        assert_eq!(unsafe { libc::openpty(&mut master, &mut slave, std::ptr::null_mut(), std::ptr::null(), std::ptr::null()) }, 0);
        let mut buf = [0 as libc::c_char; 128];
        assert_eq!(unsafe { libc::ttyname_r(slave, buf.as_mut_ptr(), buf.len()) }, 0);
        let name = unsafe { std::ffi::CStr::from_ptr(buf.as_ptr()) }.to_string_lossy().into_owned();
        (master, slave, name)
    }

    /// A pseudo-terminal whose other end is gone answers writes with EIO, as
    /// a hung-up virtual console does: the write must end, not loop.
    #[test]
    fn a_console_that_stops_taking_writes_ends_the_wait() {
        let (master, slave, name) = pty();
        let con = Console::open(&name).expect("the pty opens");
        unsafe {
            libc::close(master);
            libc::close(slave);
        }
        let started = Instant::now();
        let r = con.wait(5.0, &|left| format!("{left}\n").repeat(400), false);
        assert!(r.is_err(), "the wait went on after the console stopped taking writes: {r:?}");
        assert!(started.elapsed() < Duration::from_secs(2), "the wait did not end at once");
    }

    /// A descriptor that stops taking writes on a console that is still there
    /// (the installer's shell hung up the old open): the console is opened
    /// again and the wait goes on to its deadline.
    #[test]
    fn a_hung_up_open_is_replaced_and_the_wait_goes_on() {
        let (master, slave, name) = pty();
        let con = Console::open(&name).expect("the pty opens");
        // the old open goes dead: its number now holds a read-only open, so writes fail (EBADF)
        let c = std::ffi::CString::new(name.as_str()).unwrap();
        let ro = unsafe { libc::open(c.as_ptr(), libc::O_RDONLY | libc::O_NOCTTY | libc::O_NONBLOCK) };
        assert!(ro >= 0);
        assert_eq!(unsafe { libc::dup2(ro, con.fd.get()) }, con.fd.get());
        unsafe { libc::close(ro) };
        let r = con.wait(1.2, &|left| format!("{left}\n"), true);
        assert!(matches!(r, Ok((false, e)) if e >= 1.1), "{r:?}");
        assert_eq!(con.reopens.get(), 1);
        drop(con);
        unsafe {
            libc::close(master);
            libc::close(slave);
        }
    }

    #[test]
    fn the_consoles_own_answers_are_not_keys() {
        // the bytes read on the rig, 2026-10-15 (v9 Bdiag): two reports, one read
        assert!(only_cursor_reports(b"\x1b[9;1R\x1b[48;128R"));
        assert!(only_cursor_reports(b"\x1b[1;1R"));
        // keys stay keys
        for k in [&b" "[..], b"\r", b"a", b"\x1b", b"\x1b[A", b"\x1b[[A", b"\x1b[15~", b"\x03"] {
            assert!(!only_cursor_reports(k), "{k:?}");
        }
        // a report with a key beside it, or cut short, is a key
        assert!(!only_cursor_reports(b"\x1b[9;1R "));
        assert!(!only_cursor_reports(b" \x1b[9;1R"));
        assert!(!only_cursor_reports(b"\x1b[9;1"));
        assert!(!only_cursor_reports(b"\x1b[;1R"));
        assert!(!only_cursor_reports(b""));
    }

    #[test]
    fn a_cursor_report_does_not_end_the_wait_and_a_key_still_does() {
        let (master, slave, name) = pty();
        let con = Console::open(&name).expect("the pty opens");
        let m = master;
        let typist = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            let r = b"\x1b[9;1R\x1b[48;128R";
            unsafe { libc::write(m, r.as_ptr() as *const _, r.len()) };
            std::thread::sleep(Duration::from_millis(600));
            unsafe { libc::write(m, b" ".as_ptr() as *const _, 1) };
        });
        let r = con.wait(5.0, &|left| format!("{left}\n"), true);
        typist.join().unwrap();
        assert!(matches!(r, Ok((true, e)) if e > 0.6 && e < 2.0), "{r:?}");
        assert_eq!(con.reports.borrow().len(), 1);
        assert_eq!(&con.key_bytes.borrow()[..], b" ");
        drop(con);
        unsafe {
            libc::close(master);
            libc::close(slave);
        }
    }

    #[test]
    fn a_working_console_counts_down() {
        let (master, slave, name) = pty();
        let con = Console::open(&name).expect("the pty opens");
        let r = con.wait(1.2, &|left| format!("{left}\n"), true);
        assert!(matches!(r, Ok((false, _))), "{r:?}");
        unsafe {
            libc::close(master);
            libc::close(slave);
        }
    }
}

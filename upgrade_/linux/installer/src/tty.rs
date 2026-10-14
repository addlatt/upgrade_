//! The console the scripts drew on (/dev/tty6): opened raw once for the
//! whole wait, keys read from a non-blocking descriptor against a
//! monotonic deadline, the terminal put back after. What the scripts'
//! Python did (verify.sh 0.5.0 and 0.4.1: a key pressed during bash's
//! `read -t` left it blocked on the rig, 2026-09-26).

use std::time::{Duration, Instant};

pub struct Console {
    fd: i32,
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
        Ok(Console { fd, old, cols: ws.ws_col, rows: ws.ws_row })
    }

    /// A console the kernel reports narrower than 40 columns is set to 80x25.
    pub fn resize(&mut self, rows: u16, cols: u16) -> Result<(), String> {
        let ws = libc::winsize { ws_row: rows, ws_col: cols, ws_xpixel: 0, ws_ypixel: 0 };
        if unsafe { libc::ioctl(self.fd, libc::TIOCSWINSZ, &ws) } != 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let mut got: libc::winsize = unsafe { std::mem::zeroed() };
        unsafe { libc::ioctl(self.fd, libc::TIOCGWINSZ, &mut got) };
        self.rows = got.ws_row;
        self.cols = got.ws_col;
        Ok(())
    }

    pub fn put(&self, text: &str) {
        let mut data = text.as_bytes();
        while !data.is_empty() {
            let n = unsafe { libc::write(self.fd, data.as_ptr() as *const _, data.len()) };
            if n > 0 {
                data = &data[n as usize..];
            } else {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }

    /// A key within `timeout`: true when one came.
    pub fn key_within(&self, timeout: Duration) -> bool {
        let mut pfd = libc::pollfd { fd: self.fd, events: libc::POLLIN, revents: 0 };
        let ms = timeout.as_millis().min(i32::MAX as u128) as i32;
        let r = unsafe { libc::poll(&mut pfd, 1, ms) };
        if r > 0 {
            let mut buf = [0u8; 64];
            let n = unsafe { libc::read(self.fd, buf.as_mut_ptr() as *mut _, buf.len()) };
            return n > 0;
        }
        false
    }

    /// Draw `screen(left)` every second until the deadline or a key.
    /// Returns true when a key cut it short.
    pub fn wait(&self, seconds: f64, screen: &dyn Fn(u64) -> String, fine: bool) -> (bool, f64) {
        let end = Instant::now() + Duration::from_secs_f64(seconds);
        let mut keyed = false;
        loop {
            let left = end.saturating_duration_since(Instant::now()).as_secs_f64();
            if left <= 0.0 {
                break;
            }
            let whole = left.ceil() as u64;
            self.put(&screen(whole));
            let timeout = if fine { (left - (whole as f64 - 1.0)).max(0.05) } else { left.min(1.0) };
            if self.key_within(Duration::from_secs_f64(timeout)) {
                keyed = true;
                break;
            }
        }
        let elapsed = seconds - end.saturating_duration_since(Instant::now()).as_secs_f64().max(0.0);
        (keyed, elapsed)
    }
}

impl Drop for Console {
    fn drop(&mut self) {
        unsafe {
            libc::tcsetattr(self.fd, libc::TCSANOW, &self.old);
            libc::close(self.fd);
        }
    }
}

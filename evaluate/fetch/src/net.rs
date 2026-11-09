//! The network on a real machine: WinHTTP, Windows' own HTTP client
//! (chosen 2026-11-08 for item 24). It uses Windows' own TLS and
//! certificate store and the computer's proxy settings, and it brings no
//! crate and no C code into the build, so the bytes stay reproducible
//! (RISKS R14). Trust does not rest on it: every byte is checked against
//! the table's sha256 after it arrives (get.rs).

use crate::get::{range_start, Net, Response};
use std::ffi::c_void;
use std::io::Read;
use windows_sys::Win32::Networking::WinHttp::*;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn last_error() -> String {
    let e = unsafe { windows_sys::Win32::Foundation::GetLastError() };
    match e {
        12002 => "the connection timed out".into(),
        12007 => "the name could not be found (is the computer online?)".into(),
        12029 => "could not connect".into(),
        12030 => "the connection was dropped".into(),
        12175 => "the secure connection failed (certificate)".into(),
        _ => format!("WinHTTP error {e}"),
    }
}

struct Handle(*mut c_void);
impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { WinHttpCloseHandle(self.0) };
        }
    }
}

pub struct WinNet {
    session: Handle,
}

impl WinNet {
    pub fn new(agent: &str) -> Result<WinNet, String> {
        let a = wide(agent);
        let s = unsafe { WinHttpOpen(a.as_ptr(), WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, std::ptr::null(), std::ptr::null(), 0) };
        if s.is_null() {
            return Err(format!("could not start WinHTTP: {}", last_error()));
        }
        // resolve, connect, send: 30 s; receive: 60 s without a byte is a dropped connection
        unsafe { WinHttpSetTimeouts(s, 30_000, 30_000, 30_000, 60_000) };
        Ok(WinNet { session: Handle(s) })
    }
}

/// The body of one response; holds its handles until it is dropped.
struct Body {
    request: Handle,
    _connect: Handle,
}
impl Read for Body {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let mut n = 0u32;
        let want = buf.len().min(u32::MAX as usize) as u32;
        let ok = unsafe { WinHttpReadData(self.request.0, buf.as_mut_ptr() as *mut c_void, want, &mut n) };
        if ok == 0 {
            return Err(std::io::Error::new(std::io::ErrorKind::ConnectionAborted, last_error()));
        }
        Ok(n as usize)
    }
}

fn query_u64(req: *mut c_void, what: u32) -> Option<u64> {
    let mut v = 0u64;
    let mut len = std::mem::size_of::<u64>() as u32;
    let ok = unsafe { WinHttpQueryHeaders(req, what | WINHTTP_QUERY_FLAG_NUMBER64, std::ptr::null(), &mut v as *mut u64 as *mut c_void, &mut len, std::ptr::null_mut()) };
    if ok != 0 { Some(v) } else { None }
}

fn query_text(req: *mut c_void, what: u32) -> Option<String> {
    let mut buf = vec![0u16; 256];
    let mut len = (buf.len() * 2) as u32;
    let ok = unsafe { WinHttpQueryHeaders(req, what, std::ptr::null(), buf.as_mut_ptr() as *mut c_void, &mut len, std::ptr::null_mut()) };
    if ok == 0 {
        return None;
    }
    Some(String::from_utf16_lossy(&buf[..(len / 2) as usize]))
}

impl Net for WinNet {
    fn get(&mut self, url: &str, from: u64) -> Result<Response, String> {
        self.request("GET", url, from)
    }
    fn size(&mut self, url: &str) -> Result<(u16, Option<u64>), String> {
        self.request("HEAD", url, 0).map(|r| (r.status, r.length))
    }
}

impl WinNet {
    fn request(&mut self, verb_s: &str, url: &str, from: u64) -> Result<Response, String> {
        let u = wide(url);
        let mut host = vec![0u16; 256];
        let mut path = vec![0u16; 2048];
        let mut c: URL_COMPONENTS = unsafe { std::mem::zeroed() };
        c.dwStructSize = std::mem::size_of::<URL_COMPONENTS>() as u32;
        c.lpszHostName = host.as_mut_ptr();
        c.dwHostNameLength = host.len() as u32;
        c.lpszUrlPath = path.as_mut_ptr();
        c.dwUrlPathLength = path.len() as u32;
        if unsafe { WinHttpCrackUrl(u.as_ptr(), 0, 0, &mut c) } == 0 {
            return Err(format!("not an address: {url}"));
        }
        let host_s = String::from_utf16_lossy(&host[..c.dwHostNameLength as usize]);
        let path_s = String::from_utf16_lossy(&path[..c.dwUrlPathLength as usize]);
        let secure = c.nScheme == WINHTTP_INTERNET_SCHEME_HTTPS;
        let h = wide(&host_s);
        let connect = Handle(unsafe { WinHttpConnect(self.session.0, h.as_ptr(), c.nPort, 0) });
        if connect.0.is_null() {
            return Err(last_error());
        }
        let (verb, p) = (wide(verb_s), wide(&path_s));
        let request = Handle(unsafe { WinHttpOpenRequest(connect.0, verb.as_ptr(), p.as_ptr(), std::ptr::null(), std::ptr::null(), std::ptr::null(), if secure { WINHTTP_FLAG_SECURE } else { 0 }) });
        if request.0.is_null() {
            return Err(last_error());
        }
        let headers = if from > 0 { wide(&format!("Range: bytes={from}-\r\n")) } else { wide("") };
        let hl = if from > 0 { (headers.len() - 1) as u32 } else { 0 };
        let sent = unsafe { WinHttpSendRequest(request.0, if from > 0 { headers.as_ptr() } else { std::ptr::null() }, hl, std::ptr::null(), 0, 0, 0) };
        if sent == 0 || unsafe { WinHttpReceiveResponse(request.0, std::ptr::null_mut()) } == 0 {
            return Err(last_error());
        }
        let mut status = 0u32;
        let mut len = 4u32;
        unsafe { WinHttpQueryHeaders(request.0, WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER, std::ptr::null(), &mut status as *mut u32 as *mut c_void, &mut len, std::ptr::null_mut()) };
        let length = query_u64(request.0, WINHTTP_QUERY_CONTENT_LENGTH);
        let start = query_text(request.0, WINHTTP_QUERY_CONTENT_RANGE).and_then(|r| range_start(&r));
        Ok(Response { status: status as u16, length, range_start: start, body: Box::new(Body { request, _connect: connect }) })
    }
}

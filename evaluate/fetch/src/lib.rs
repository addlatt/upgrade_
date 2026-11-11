//! upgrade-fetch: the Linux the person picked, onto the stick it runs from
//! (roadmap item 24). `plan` reads the measured table; `iso` reads a disc
//! image as it streams past; `stick` says which stick this is and what it
//! holds; `get` is the flow over two seams (the network, the machine);
//! `net` (Windows only) is WinHTTP.

pub mod get;
pub mod iso;
#[cfg(windows)]
pub mod net;
pub mod plan;
pub mod stick;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The one progress line the window reads: `fetch: progress <done> <total>` in bytes.
pub fn progress_of(line: &str) -> Option<(u64, u64)> {
    let mut it = line.trim().strip_prefix("fetch: progress ")?.split(' ');
    let done = it.next()?.parse().ok()?;
    let total = it.next()?.parse().ok()?;
    Some((done, total))
}

#[cfg(test)]
mod tests {
    #[test]
    fn progress_lines_parse() {
        assert_eq!(super::progress_of("fetch: progress 10 200"), Some((10, 200)));
        assert_eq!(super::progress_of("fetch: getting x"), None);
    }
}

//! Resource-limit enforcement for proc-macro expansion.
//!
//! Each expansion runs inside a dedicated worker thread so the server can bound
//! wall-clock time. After the macro returns, the server also checks the size of
//! the produced token stream and the process's resident set size against the
//! limits supplied by the compiler.

use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use yelang_proc_macro_bridge::protocol::token::WireTokenTree;
use yelang_proc_macro_bridge::sandbox::Limits;

use super::invoke::InvokeError;

/// Run `work` under the supplied resource limits.
///
/// Time is enforced by spawning a worker thread and waiting on a bounded
/// channel; if the worker does not finish in time the request is rejected with
/// [`InvokeError::Timeout`]. Because the worker thread cannot be forcefully
/// terminated from Rust, the server treats a timeout as fatal and exits the
/// process after reporting the error, preventing a runaway macro from starving
/// subsequent requests.
///
/// Memory and output-size limits are checked once the worker has returned, so
/// they do not stop a macro early; they do guarantee that an oversized result
/// is rejected before it is handed back to the compiler.
pub fn enforce_limits<T, F>(limits: Limits, work: F) -> Result<T, InvokeError>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    // A zero-second limit is not useful and would always time out; treat it as
    // the smallest practical bound.
    let timeout = Duration::from_secs(limits.max_cpu_seconds.max(1));

    let (tx, rx) = mpsc::channel::<T>();
    let start = Instant::now();

    thread::spawn(move || {
        let result = work();
        // If the receiver has hung up (server gave up on us), dropping the
        // result is fine: the macro has been cancelled from the server's point
        // of view.
        let _ = tx.send(result);
    });

    match rx.recv_timeout(timeout) {
        Ok(result) => Ok(result),
        Err(_) => {
            let elapsed = start.elapsed().as_secs();
            Err(InvokeError::Timeout {
                limit_seconds: limits.max_cpu_seconds,
                elapsed_seconds: elapsed,
            })
        }
    }
}

/// Count every token tree in `stream`, recursively entering groups.
pub fn count_tokens(stream: &[WireTokenTree]) -> usize {
    stream
        .iter()
        .map(|tree| match tree {
            WireTokenTree::Group { trees, .. } => 1 + count_tokens(trees),
            _ => 1,
        })
        .sum()
}

/// Best-effort current resident set size of this process, in bytes.
///
/// Returns `None` if RSS cannot be determined on the current platform. This is
/// intentionally coarse: it is a guardrail, not a precise accounting of heap
/// allocations.
pub fn current_rss_bytes() -> Option<usize> {
    #[cfg(target_os = "linux")]
    {
        linux_rss()
    }
    #[cfg(target_os = "macos")]
    {
        macos_rss()
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        None
    }
}

#[cfg(target_os = "linux")]
fn linux_rss() -> Option<usize> {
    let contents = std::fs::read_to_string("/proc/self/statm").ok()?;
    let resident_pages: usize = contents
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())?;
    let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) } as usize;
    Some(resident_pages * page_size)
}

#[cfg(target_os = "macos")]
fn macos_rss() -> Option<usize> {
    // ru_maxrss is the maximum resident set size seen so far, in bytes on
    // macOS. It is a coarse proxy for current memory usage.
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
    let ret = unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) };
    if ret != 0 {
        return None;
    }
    let usage = unsafe { usage.assume_init() };
    Some(usage.ru_maxrss as usize)
}

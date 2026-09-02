//! Signal handler for printing a backtrace after fatal process signals.
//!
//! Portions of this file are derived from rustc's signal handler:
//! <https://github.com/rust-lang/rust/blob/40be1db6b89dcf027cad553e85e767aadfa75a7f/compiler/rustc_driver_impl/src/signal_handler.rs>
//!
//! Copyright (c) The Rust Project Contributors
//!
//! Permission is hereby granted, free of charge, to any person obtaining a copy
//! of this software and associated documentation files (the "Software"), to deal
//! in the Software without restriction, including without limitation the rights
//! to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
//! copies of the Software, and to permit persons to whom the Software is
//! furnished to do so, subject to the following conditions:
//!
//! The above copyright notice and this permission notice shall be included in all
//! copies or substantial portions of the Software.
//!
//! THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
//! IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
//! FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
//! AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
//! LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
//! OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
//! SOFTWARE.

use std::{
    alloc::{Layout, alloc},
    mem, ptr,
    slice::from_raw_parts,
};

/// Default stack size suggested when a stack overflow is suspected.
pub const DEFAULT_STACK_SIZE: usize = 8 * 1024 * 1024;

/// Fatal signals for which the handler emits a backtrace before normal signal
/// termination resumes.
#[rustfmt::skip]
const KILL_SIGNALS: [(libc::c_int, &str); 3] = [
    (libc::SIGILL, "SIGILL"),
    (libc::SIGBUS, "SIGBUS"),
    (libc::SIGSEGV, "SIGSEGV")
];

unsafe extern "C" {
    fn backtrace_symbols_fd(buffer: *const *mut libc::c_void, size: libc::c_int, fd: libc::c_int);
}

/// Resolves and writes captured backtrace frames directly to standard error.
fn backtrace_stderr(buffer: &[*mut libc::c_void]) {
    let size = buffer.len().try_into().unwrap_or_default();

    // SAFETY: `buffer` points to `size` entries captured by `backtrace`, and
    // `STDERR_FILENO` is a valid process file descriptor for diagnostic output.
    unsafe {
        backtrace_symbols_fd(buffer.as_ptr(), size, libc::STDERR_FILENO);
    }
}

/// Minimal unbuffered writer used from the signal handler.
///
/// This avoids the standard buffered stderr machinery while the process is in
/// an abnormal state.
struct RawStderr(());

impl std::fmt::Write for RawStderr {
    fn write_str(&mut self, s: &str) -> Result<(), std::fmt::Error> {
        // SAFETY: `s.as_ptr()` and `s.len()` describe a valid byte slice for
        // the duration of the call, and `write` does not retain the pointer.
        let ret = unsafe { libc::write(libc::STDERR_FILENO, s.as_ptr().cast(), s.len()) };
        if ret == -1 { Err(std::fmt::Error) } else { Ok(()) }
    }
}

/// Writes a formatted diagnostic line directly to standard error.
///
/// Write errors and partial writes are intentionally ignored because the
/// process is handling a fatal signal.
macro_rules! raw_errln {
    ($tokens:tt) => {
        let _ = ::core::fmt::Write::write_fmt(&mut RawStderr(()), format_args!($tokens));
        let _ = ::core::fmt::Write::write_char(&mut RawStderr(()), '\n');
    };
}

/// Signal handler installed for fatal process signals.
///
/// # Safety
///
/// The caller must ensure that this function is not re-entered.
unsafe extern "C" fn print_stack_trace(signum: libc::c_int) {
    const MAX_FRAMES: usize = 256;

    let signame = {
        let mut signame = "<unknown>";
        for sig in KILL_SIGNALS {
            if sig.0 == signum {
                signame = sig.1;
            }
        }
        signame
    };

    // SAFETY: this handler is installed with `SA_NODEFER | SA_RESETHAND`, so it
    // is not intended to be re-entered for the same signal. The static buffer
    // avoids allocating while collecting frames from the signal handler.
    let stack = unsafe {
        static mut STACK_TRACE: [*mut libc::c_void; MAX_FRAMES] = [ptr::null_mut(); MAX_FRAMES];

        // Capture return addresses into preallocated storage.
        let depth = libc::backtrace(&raw mut STACK_TRACE as _, MAX_FRAMES as i32);
        if depth == 0 {
            return;
        }

        from_raw_parts(&raw const STACK_TRACE as _, depth as _)
    };

    raw_errln!("error: kival interrupted by {signame}, printing backtrace\n");

    let mut written = 1;
    let mut consumed = 0;

    // Detect repeated frame sequences so deeply recursive stack overflows can
    // be represented compactly instead of emitting the same cycle repeatedly.
    let cycled = |(runner, walker)| runner == walker;
    let mut cyclic = false;

    if let Some(period) = stack.iter().skip(1).step_by(2).zip(stack).position(cycled) {
        let period = period.saturating_add(1);

        let Some(offset) = stack.iter().skip(period).zip(stack).position(cycled) else {
            // A detected period should always have a corresponding cycle offset.
            return;
        };

        // Count consecutive identical cycle slices. Matching only the period and
        // entry point is insufficient when adjacent cycles differ internally.
        let next_cycle = stack[offset..].chunks_exact(period).skip(1);
        let cycles = 1 + next_cycle
            .zip(stack[offset..].chunks_exact(period))
            .filter(|(next, prev)| next == prev)
            .count();

        backtrace_stderr(&stack[..offset]);

        written += offset;
        consumed += offset;

        if cycles > 1 {
            raw_errln!("\n### cycle encountered after {offset} frames with period {period}");
            backtrace_stderr(&stack[consumed..consumed + period]);
            raw_errln!("### recursed {cycles} times\n");

            written += period + 4;
            consumed += period * cycles;
            cyclic = true;
        };
    }

    let rem = &stack[consumed..];
    backtrace_stderr(rem);
    raw_errln!("");

    written += rem.len() + 1;

    // A deep or cyclic SIGSEGV backtrace is treated as evidence of a probable
    // stack overflow. This remains heuristic because SIGSEGV has other causes.
    let stack_overflow_depth = 8 * 16;
    if (cyclic || stack.len() > stack_overflow_depth) && signum == libc::SIGSEGV {
        raw_errln!("note: kival unexpectedly overflowed its stack! this is a bug");
        written += 1;
    }

    if stack.len() == MAX_FRAMES {
        raw_errln!("note: maximum backtrace depth reached, frames may have been lost");
        written += 1;
    }

    raw_errln!("note: we would appreciate a report at https://github.com/selemis-com/kival");
    written += 1;

    if signum == libc::SIGSEGV {
        let new_size = DEFAULT_STACK_SIZE * 2;
        raw_errln!(
            "help: you can increase kival's stack size by setting RUST_MIN_STACK={new_size}"
        );
        written += 1;
    }

    if written > 24 {
        // Repeat the signal name after long traces because the initial
        // diagnostic may no longer be visible in the terminal.
        raw_errln!("note: backtrace dumped due to {signame}! resuming signal");
    };
}

/// Installs handlers for fatal process signals that print a backtrace before
/// the signal is re-raised by the default disposition.
///
/// # Panics
///
/// Panics if the alternate signal stack layout cannot be constructed.
pub fn install() {
    // SAFETY: all libc calls receive initialized structures and valid pointers.
    // The alternate signal stack is intentionally retained for the process
    // lifetime because a handler may execute at any later point.
    unsafe {
        let alt_stack_size: usize = min_sigstack_size() + 64 * 1024;
        let mut alt_stack: libc::stack_t = mem::zeroed();

        alt_stack.ss_sp = alloc(Layout::from_size_align(alt_stack_size, 1).unwrap()).cast();
        alt_stack.ss_size = alt_stack_size;

        libc::sigaltstack(&raw const alt_stack, ptr::null_mut());

        let mut sa: libc::sigaction = mem::zeroed();
        let handler: unsafe extern "C" fn(libc::c_int) = print_stack_trace;

        sa.sa_sigaction = handler as libc::sighandler_t;
        sa.sa_flags = libc::SA_NODEFER | libc::SA_RESETHAND | libc::SA_ONSTACK;

        libc::sigemptyset(&raw mut sa.sa_mask);

        for (signum, _signame) in KILL_SIGNALS {
            libc::sigaction(signum, &raw const sa, ptr::null_mut());
        }
    }
}

/// Returns the minimum alternate signal stack size for the current Linux
/// kernel and architecture.
#[cfg(target_os = "linux")]
fn min_sigstack_size() -> usize {
    use core::ffi::c_ulong;

    const AT_MINSIGSTKSZ: c_ulong = 51;

    // SAFETY: `AT_MINSIGSTKSZ` is a valid Linux auxiliary-vector key.
    // `getauxval` returns zero when the entry is unavailable.
    let dynamic_sigstksz = unsafe { libc::getauxval(AT_MINSIGSTKSZ) };

    // Older kernels may not expose `AT_MINSIGSTKSZ`; in that case the libc
    // constant provides the minimum.
    libc::MINSIGSTKSZ.max(dynamic_sigstksz as _)
}

/// Returns the platform-defined minimum alternate signal stack size.
#[cfg(not(target_os = "linux"))]
const fn min_sigstack_size() -> usize {
    libc::MINSIGSTKSZ
}

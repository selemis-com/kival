//! Signal handler to extract a backtrace from stack overflow.
//!
//! Implementation modified from [`rustc`](https://github.com/rust-lang/rust/blob/40be1db6b89dcf027cad553e85e767aadfa75a7f/compiler/rustc_driver_impl/src/signal_handler.rs).

use std::{
    alloc::{Layout, alloc},
    mem, ptr,
    slice::from_raw_parts,
};

/// Default stack size for the signal handler.
pub const DEFAULT_STACK_SIZE: usize = 8 * 1024 * 1024;

/// Signals that represent that we have a bug, and our prompt termination has
/// been ordered.
#[rustfmt::skip]
const KILL_SIGNALS: [(libc::c_int, &str); 3] = [
    (libc::SIGILL, "SIGILL"),
    (libc::SIGBUS, "SIGBUS"),
    (libc::SIGSEGV, "SIGSEGV")
];

unsafe extern "C" {
    fn backtrace_symbols_fd(buffer: *const *mut libc::c_void, size: libc::c_int, fd: libc::c_int);
}

/// Writes resolved backtrace symbols to standard error.
fn backtrace_stderr(buffer: &[*mut libc::c_void]) {
    let size = buffer.len().try_into().unwrap_or_default();
    // SAFETY: `buffer` points to `size` entries captured by `backtrace`, and
    // `STDERR_FILENO` is a valid process file descriptor for diagnostic output.
    unsafe {
        backtrace_symbols_fd(buffer.as_ptr(), size, libc::STDERR_FILENO);
    }
}

/// Unbuffered, unsynchronized writer to stderr.
///
/// Only acceptable because everything will end soon anyways.
struct RawStderr(());

impl std::fmt::Write for RawStderr {
    fn write_str(&mut self, s: &str) -> Result<(), std::fmt::Error> {
        // SAFETY: `s.as_ptr()` and `s.len()` describe a valid byte slice for
        // the duration of the call, and writing to stderr does not retain it.
        let ret = unsafe { libc::write(libc::STDERR_FILENO, s.as_ptr().cast(), s.len()) };
        if ret == -1 { Err(std::fmt::Error) } else { Ok(()) }
    }
}

/// We don't really care how many bytes we actually get out. SIGSEGV comes for our head.
/// Splash stderr with letters of our own blood to warn our friends about the monster.
macro_rules! raw_errln {
    ($tokens:tt) => {
        let _ = ::core::fmt::Write::write_fmt(&mut RawStderr(()), format_args!($tokens));
        let _ = ::core::fmt::Write::write_char(&mut RawStderr(()), '\n');
    };
}

/// Signal handler installed for SIGSEGV
///
/// # Safety
///
/// Caller must ensure that this function is not re-entered.
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
    // is not intended to be re-entered for the same signal. The static buffer is
    // used only inside the handler to avoid allocation while collecting frames.
    let stack = unsafe {
        // Reserve data segment so we don't have to malloc in a signal handler, which might fail
        // in incredibly undesirable and unexpected ways due to e.g. the allocator deadlocking
        static mut STACK_TRACE: [*mut libc::c_void; MAX_FRAMES] = [ptr::null_mut(); MAX_FRAMES];
        // Collect return addresses
        let depth = libc::backtrace(&raw mut STACK_TRACE as _, MAX_FRAMES as i32);
        if depth == 0 {
            return;
        }
        from_raw_parts(&raw const STACK_TRACE as _, depth as _)
    };

    // Just a stack trace is cryptic. Explain what we're doing.
    raw_errln!("error: kival interrupted by {signame}, printing backtrace\n");

    let mut written = 1;
    let mut consumed = 0;
    // Begin elaborating return addrs into symbols and writing them directly to stderr
    // Most backtraces are stack overflow, most stack overflows are from recursion
    // Check for cycles before writing 250 lines of the same ~5 symbols
    let cycled = |(runner, walker)| runner == walker;
    let mut cyclic = false;
    if let Some(period) = stack.iter().skip(1).step_by(2).zip(stack).position(cycled) {
        let period = period.saturating_add(1); // avoid "what if wrapped?" branches
        let Some(offset) = stack.iter().skip(period).zip(stack).position(cycled) else {
            // impossible.
            return;
        };

        // Count matching trace slices, else we could miscount "biphasic cycles"
        // with the same period + loop entry but a different inner loop
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

    let random_depth = || 8 * 16; // chosen by random diceroll (2d20)
    if (cyclic || stack.len() > random_depth()) && signum == libc::SIGSEGV {
        // technically speculation, but assert it with confidence anyway.
        // kival only arrived in this signal handler because bad things happened
        // and this message is for explaining it's not the programmer's fault
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
        // We probably just scrolled the earlier "interrupted by {signame}" message off the terminal
        raw_errln!("note: backtrace dumped due to {signame}! resuming signal");
    };
}

/// When one of the KILL signals is delivered to the process, print a stack trace and then exit.
///
/// # Panics
///
/// Panics if the alternate signal stack layout cannot be constructed.
pub fn install() {
    // SAFETY: all libc calls are given initialized structures and valid
    // pointers. The alternate signal stack is intentionally leaked for the
    // process lifetime because signal handlers may run at any later point.
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

/// Modern kernels on modern hardware can have dynamic signal stack sizes.
#[cfg(target_os = "linux")]
fn min_sigstack_size() -> usize {
    use core::ffi::c_ulong;

    const AT_MINSIGSTKSZ: c_ulong = 51;
    // SAFETY: `AT_MINSIGSTKSZ` is a valid auxv key on Linux. Missing entries
    // are reported as `0`, which is handled below.
    let dynamic_sigstksz = unsafe { libc::getauxval(AT_MINSIGSTKSZ) };
    // If getauxval couldn't find the entry, it returns 0,
    // so take the higher of the "constant" and auxval.
    // This transparently supports older kernels which don't provide AT_MINSIGSTKSZ
    libc::MINSIGSTKSZ.max(dynamic_sigstksz as _)
}

/// Not all OS support hardware where this is needed.
#[cfg(not(target_os = "linux"))]
const fn min_sigstack_size() -> usize {
    libc::MINSIGSTKSZ
}

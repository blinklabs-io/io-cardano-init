mod cli;
mod contract;
mod doctor;
mod registry;
mod scaffold;

fn main() {
    reset_sigpipe();
    std::process::exit(cli::run());
}

/// Restore the default SIGPIPE disposition (terminate the process) instead of
/// Rust's default of ignoring it. Without this, writing to a closed pipe — e.g.
/// piping output into `head`/`less` and quitting early — surfaces as an EPIPE
/// panic ("failed printing to stdout: Broken pipe") rather than a clean exit.
#[cfg(unix)]
fn reset_sigpipe() {
    // SAFETY: resetting a signal to SIG_DFL is async-signal-safe and this runs
    // once at startup, before any threads are spawned.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
}

#[cfg(not(unix))]
fn reset_sigpipe() {}

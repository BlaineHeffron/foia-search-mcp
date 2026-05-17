use std::io;
use std::process::{Child, ExitStatus};
use std::time::{Duration, Instant};

#[cfg(unix)]
use std::os::raw::c_int;

#[cfg(unix)]
const SIGKILL: c_int = 9;
const WAIT_POLL_INTERVAL: Duration = Duration::from_millis(10);

#[cfg(unix)]
unsafe extern "C" {
    fn kill(pid: c_int, sig: c_int) -> c_int;
}

#[derive(Debug)]
pub enum ChildWaitOutcome {
    Completed(ExitStatus),
    TimedOut,
    Cancelled,
}

pub fn wait_for_child_with_controls(
    child: &mut Child,
    timeout: Duration,
    is_cancelled: &dyn Fn() -> bool,
) -> io::Result<ChildWaitOutcome> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(ChildWaitOutcome::Completed(status));
        }
        if is_cancelled() {
            kill_child_process_tree(child);
            let _ = child.wait();
            return Ok(ChildWaitOutcome::Cancelled);
        }
        if Instant::now() >= deadline {
            kill_child_process_tree(child);
            let _ = child.wait();
            return Ok(ChildWaitOutcome::TimedOut);
        }
        std::thread::sleep(WAIT_POLL_INTERVAL);
    }
}

fn kill_child_process_tree(child: &mut Child) {
    #[cfg(unix)]
    {
        if let Ok(pid) = c_int::try_from(child.id()) {
            // SAFETY: the child is started in its own process group using
            // process_group(0), so -pid is scoped to that command tree.
            let killed_group = unsafe { kill(-pid, SIGKILL) } == 0;
            if killed_group {
                return;
            }
        }
    }

    let _ = child.kill();
}

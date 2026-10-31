//! Converter helpers are killed if their owning viewer exits.
use std::os::unix::process::CommandExt;
use std::process::{Child, Command};

pub fn configure(command: &mut Command) {
    let parent = std::process::id() as libc::pid_t;
    command.process_group(0);
    unsafe {
        command.pre_exec(move || {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            if libc::getppid() != parent {
                libc::_exit(125);
            }
            // A corrupt converter cannot fill the disk with a multi-GB result.
            let limit = libc::rlimit {
                rlim_cur: 256 * 1024 * 1024,
                rlim_max: 256 * 1024 * 1024,
            };
            libc::setrlimit(libc::RLIMIT_FSIZE, &limit);
            Ok(())
        });
    }
}
pub fn kill(child: &mut Child) {
    unsafe {
        libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL);
    }
    let _ = child.kill();
    let _ = child.wait();
}

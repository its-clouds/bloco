//! The working directory of a process, asked from the OS. Used when the shell
//! does not report its directory with OSC 7.

use std::path::PathBuf;

/// The current directory of process `pid`. `None` when the process is gone or
/// the platform has no implementation (only macOS for now).
#[cfg(target_os = "macos")]
pub fn pid_cwd(pid: u32) -> Option<PathBuf> {
    use std::ffi::{c_int, c_void};
    use std::os::unix::ffi::OsStrExt;

    /// `PROC_PIDVNODEPATHINFO` from `sys/proc_info.h`: the vnode paths of the
    /// current and root directories.
    const PROC_PIDVNODEPATHINFO: c_int = 9;
    /// `sizeof(struct vnode_info)`: a `vinfo_stat` (136 bytes), a type, a pad and an fsid.
    const VNODE_INFO_SIZE: usize = 152;
    const MAXPATHLEN: usize = 1024;
    const VNODE_INFO_PATH_SIZE: usize = VNODE_INFO_SIZE + MAXPATHLEN;

    /// Mirrors `struct proc_vnodepathinfo`. Only `cdir` is read. The bytes
    /// are treated as opaque, so the layout only has to match in size.
    #[repr(C)]
    struct ProcVnodePathInfo {
        cdir: [u8; VNODE_INFO_PATH_SIZE],
        rdir: [u8; VNODE_INFO_PATH_SIZE],
    }

    unsafe extern "C" {
        fn proc_pidinfo(
            pid: c_int,
            flavor: c_int,
            arg: u64,
            buffer: *mut c_void,
            buffersize: c_int,
        ) -> c_int;
    }

    let mut info = ProcVnodePathInfo {
        cdir: [0; VNODE_INFO_PATH_SIZE],
        rdir: [0; VNODE_INFO_PATH_SIZE],
    };
    let size = std::mem::size_of::<ProcVnodePathInfo>();
    // SAFETY: `info` is a writable buffer of `size` bytes, and the call writes at most that many.
    let written = unsafe {
        proc_pidinfo(
            pid as c_int,
            PROC_PIDVNODEPATHINFO,
            0,
            (&mut info as *mut ProcVnodePathInfo).cast(),
            size as c_int,
        )
    };
    if written as usize != size {
        return None;
    }
    let path = &info.cdir[VNODE_INFO_SIZE..];
    let len = path.iter().position(|&b| b == 0)?;
    (len > 0).then(|| PathBuf::from(std::ffi::OsStr::from_bytes(&path[..len])))
}

#[cfg(not(target_os = "macos"))]
pub fn pid_cwd(_pid: u32) -> Option<PathBuf> {
    None
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn reads_the_directory_of_this_process() {
        let expected = std::env::current_dir().unwrap().canonicalize().unwrap();
        let actual = pid_cwd(std::process::id()).unwrap().canonicalize().unwrap();
        assert_eq!(actual, expected);
    }
}

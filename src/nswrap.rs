use quota_update::*;

use std::os::fd::{BorrowedFd, AsRawFd};

pub fn nswrap_update_quota(root_ns_arg: *mut marfs_ns, root_fd: BorrowedFd) -> Result<(), String> {

    let root_ns;
    unsafe {
        root_ns = *root_ns_arg;

        // return a buffer to be parsed into a vector of nswrap_entry structs
        if nswrap_update_quota_c(root_ns_arg, root_fd.as_raw_fd()) == -1 {
            return Err(String::from("nswrap_update_quota_c returned -1"));
        }

    }

    Ok(())
}
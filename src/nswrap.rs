use quota_update::*;

use std::os::fd::{BorrowedFd, AsRawFd};

struct nswrap_entry {
    ino: u64,
    path: String,
}

pub fn nswrap_gen_list(root_ns_arg: *mut marfs_ns, root_fd: BorrowedFd) -> Result<(), String> {

    let root_ns;
    unsafe {
        root_ns = *root_ns_arg;

        println!("{:?}", root_ns);

        // return a buffer to be parsed into a vector of nswrap_entry structs
        if nswrap_gen_list_c(root_ns_arg, root_fd.as_raw_fd()).is_null() {
            return Err(String::from("nswrap_gen_list_c returned NULL"));
        }

    }

    Ok(())
}
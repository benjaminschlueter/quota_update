use quota_update::*;

use std::os::fd::{BorrowedFd, AsRawFd};
use std::collections::HashMap;
use std::ffi::CStr;



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

pub fn nswrap_build_map(root_ns_arg: *mut marfs_ns) -> Result<HashMap<String, u64>, String> {
    
    let mut map: HashMap<String, u64> = HashMap::new();

    let root_ns;
    unsafe {
        root_ns = *root_ns_arg;

        let map_buf_raw = nswrap_build_map_c(root_ns_arg);

        if map_buf_raw.is_null() {
            return Err(String::from("nswrap_update_quota_c returned -1"));
        }

        let map_buf = Vec::from_raw_parts(map_buf_raw as *mut nswrap_entry, BUF_MAX_NS_COUNT as usize, BUF_MAX_NS_COUNT as usize);
        
        for entry in &map_buf {
            if entry.ino == 0 {
                break;
            }

            let ns_key_str = CStr::from_ptr(entry.ns_key as *const i8).to_str().expect("bad ns_key string").to_owned(); 

            map.insert(ns_key_str.replace('/', "#"), entry.ino);
        }
    }

    Ok(map)
}
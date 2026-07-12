/* nswrap helps quota_update work with MarFS namespaces
 */

use quota_update::bindings::*;

use std::alloc::{alloc, Layout};
use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::os::fd::{AsRawFd, BorrowedFd};
use std::{env, ptr};

/* Call C function to update quota files for all namespaces.
 * See nswrap.c for more info.
 */
pub fn nswrap_update_quota(root_ns_arg: *mut marfs_ns, root_fd: BorrowedFd) -> Result<(), String> {
    unsafe {
        // return a buffer to be parsed into a vector of nswrap_entry structs
        if nswrap_update_quota_c(root_ns_arg, root_fd.as_raw_fd()) == -1 {
            return Err(String::from("nswrap_update_quota_c returned -1"));
        }
    }

    Ok(())
}

/* Creates a HashMap with namespace string keys and inodes of namespace root dir.
 * The C function walks the namespace tree and stats each namespace root, returning an array buffer of structs to be parsed.
 * See nswrap.c for more info.
 */
pub fn nswrap_build_map(root_ns_arg: *mut marfs_ns) -> Result<HashMap<String, u64>, String> {
    let mut map: HashMap<String, u64> = HashMap::new();

    let map_buf_raw;
    unsafe {
        map_buf_raw = nswrap_build_map_c(root_ns_arg);
    }

    if map_buf_raw.is_null() {
        return Err(String::from("nswrap_update_quota_c returned -1"));
    }

    let map_buf;
    unsafe {
        map_buf = Vec::from_raw_parts(
            map_buf_raw as *mut nswrap_entry,
            BUF_MAX_NS_COUNT as usize,
            BUF_MAX_NS_COUNT as usize,
        );
    }

    for entry in &map_buf {
        if entry.ino == 0 {
            break;
        }

        let ns_key_str;
        unsafe {
            ns_key_str = CStr::from_ptr(entry.ns_key as *const i8)
                .to_str()
                .expect("bad ns_key string")
                .to_owned();
        }

        map.insert(ns_key_str.replace('/', "#"), entry.ino);
    }

    Ok(map)
}

/* Hide ugliness of calling config_init from Rust
 * @param config_path: path of config to use or empty for env var MARFS_CONFIG_PATH
 * @return marfs_config struct
 */
pub fn wrap_config_init(config_path: String) -> Result<marfs_config, String> {
    let layout = Layout::new::<pthread_mutex_t>();
    let erasure_lock;
    unsafe {
        erasure_lock = alloc(layout) as *mut pthread_mutex_t; // allocate memory for pthread_mutex_t
        pthread_mutex_init(erasure_lock, ptr::null());

        let config;
        if config_path.is_empty() {
            config = config_init(
                CString::new(env::var("MARFS_CONFIG_PATH").expect("MARFS_CONFIG_PATH not set"))
                    .expect("bad MARFS_CONFIG_PATH string")
                    .as_ptr(),
                erasure_lock,
            );
        } else {
            config = config_init(
                CString::new(config_path)
                    .expect("bad config_path string")
                    .as_ptr(),
                erasure_lock,
            );
        }

        if config.is_null() {
            return Err(std::io::Error::last_os_error().to_string());
        }

        Ok(*config)
    }
}

/* Hide ugly unsafe code of creating FTAG from marfs xattr
 */
pub fn get_ftag(marfs_xattr: &str) -> Result<FTAG, String> {
    unsafe {
        let ftag_buf = libc::calloc(1, std::mem::size_of::<FTAG>());

        if ftag_initstr(
            ftag_buf as *mut FTAG,
            CString::new(marfs_xattr)
                .expect("bad xattr string")
                .as_ptr() as *mut i8,
        ) == -1
        {
            return Err(String::from("ftag_initstr returned an error"));
        }

        let ftag = Vec::from_raw_parts(ftag_buf as *mut FTAG, 1, 1)[0];

        return Ok(ftag);
    }
}


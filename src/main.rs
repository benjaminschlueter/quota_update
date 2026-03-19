use quota_update::*;

mod scoutwrap;
use scoutwrap::*; 

mod nswrap;
use nswrap::*;

use std::fs::OpenOptions;
use std::os::fd::{AsFd, AsRawFd, BorrowedFd};
use std::path::Path;
use std::ffi::{c_char, c_void, CString, CStr};
use std::io::{Error, ErrorKind};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::time::{Instant, Duration};
use std::alloc::{alloc, Layout};
use std::ptr;
use std::env;

use nix::fcntl::{OFlag, AtFlags};
use nix::sys::stat::{Mode, SFlag};
use nix::sys::stat::fstat;
use nix::sys::stat::fstatat;

const STARTUP_VERBOSE: bool = true;
const LOOP_VERBOSE: bool = true;
const QUOTA_MAGIC_NUM: u32 = 123;
const BATCH_SIZE: usize = 3;
const FS_ROOT_PATH: &str = "/marfs/mdal-root2";
const STATE_FILE: &str = ".state";
const NEW_STATE_FILE: &str = ".state.new";
const CHECKPOINT_MS: u64 = 60000; // WARNING: will fail to update state file if this is too small
const CONFIG_PATH: &str = "/opt/campaign/install/etc/marfs-config.xml";

fn main() {

    // abort if not root
    if users::get_current_uid() != 0 {
        panic!("Must run as root!");
    }

    let mut starting_major: i32 = 0;
    let mut starting_ino: i32 = 0;
    let mut starting_minor: i32 = 0;

    // read state from state file 
    let state_file_res = OpenOptions::new()
                        .read(true)
                        .open(STATE_FILE);

    // if state file does not exist, create it and start from 0. On all other errors, panic. 
    match state_file_res {
        Ok(f) => {
            let mut reader = BufReader::new(&f);
            let mut starting_state_str = String::new();

            reader.read_to_string(&mut starting_state_str);
            let input_vec: Vec<String> = starting_state_str.split("\n").map(|s| s.to_string()).collect();

            starting_major = input_vec[0].trim().parse().expect("state file does not contain valid integer");
            starting_ino = input_vec[1].trim().parse().expect("state file does not contain valid integer");
            starting_minor = input_vec[2].trim().parse().expect("state file does not contain valid integer");

            drop(f); // needs to close before rename at end of execution
        }
        Err(e) => {
            if e.kind() == ErrorKind::NotFound {
                if STARTUP_VERBOSE {
                    println!("No state file found: starting at initial state 0");
                }
            }
            else {
                panic!("open: {}\nFailed to open state file", e.to_string());
            }
        }
    }
    
    // check for existing NEW_STATE_FILE 
    let state_file_new_res = OpenOptions::new()
                         .read(true)
                         .open(NEW_STATE_FILE);

    match state_file_new_res {
        Ok(f) => {
            if STARTUP_VERBOSE {
                println!("Reading starting_state from found state swap file")
            }

            let mut reader = BufReader::new(&f);
            let mut starting_state_str = String::new();

            reader.read_to_string(&mut starting_state_str);
            
            let input_vec: Vec<String> = starting_state_str.split("\n").map(|s| s.to_string()).collect();

            starting_major = input_vec[0].trim().parse().expect("state file does not contain valid integer");
            starting_ino = input_vec[1].trim().parse().expect("state file does not contain valid integer");
            starting_minor = input_vec[2].trim().parse().expect("state file does not contain valid integer");

            drop(f); // needs to close before rename at end of execution
        }
        Err(e) => {
            if e.kind() != ErrorKind::NotFound {
                panic!("open: {}\nFailed to open state swap file", e.to_string());
            }
        }
    }

    // open fd for filesystem root
    
    let fs_root = OpenOptions::new().read(true).open(FS_ROOT_PATH);
    if let Err(e) = fs_root {
        panic!("open: {}\nFailed to open filesystem root at {}", e, FS_ROOT_PATH);
    }

    let fs_root = fs_root.unwrap();

    // setup walk_inodes struct
    
    let first = ScoutwrapWalkInodesEntry {
        major: starting_major as u64,
        ino: starting_ino as u64,
        minor: starting_minor as u32,  
    }; 

    let last = ScoutwrapWalkInodesEntry {
        major: std::u64::MAX,
        ino: std::u64::MAX,
        minor: std::u32::MAX,  
    }; 
    
    let mut user = ScoutwrapWalkInodes {
        first: first,
        last: last,
        entries_vec: Vec::new(),
        nr_entries: BATCH_SIZE,
        index: 0,
    };

    let mut ns_inode_cache = HashMap::new();

    let mut final_major = 0;
    let mut final_ino = 0;
    let mut final_minor = 0;

    // MarFS config processing 

    let config;
    match wrap_config_init(CONFIG_PATH.to_string()) {
        Ok(c) => config = c,
        Err(e) => panic!("config_init: {}", e),
    }

    println!("Running quota_update with starting state (major: {}, ino: {}, minor: {})", starting_major, starting_ino, starting_minor);

    let start_time = Instant::now();
    let mut last_checkpoint = Duration::from_millis(0);

    // process batches until entries vector is empty
    loop {

        let user_res = scoutwrap_walk_inodes(&fs_root, user.clone()); 

        match user_res {
            Ok(u) => user = u,
            Err(e) => {
                panic!("scoutwrap_walk_inodes: {}", e);
            }
        }

        // batch vector will never be empty: last element always part of next for full batches and non-full batches will be the last batch
        
        let mut last_batch = false;
        if user.entries_vec.len() < BATCH_SIZE {
            last_batch = true;
        }

        // process all but last element: last will be starting point of next run
        for entry in &user.entries_vec {
            
            // don't process the last entry of batches that are not the last. The last entry of the final batch will be processed.
            if !last_batch && entry.ino == user.entries_vec.last().unwrap().ino {
                user.first.major = user.entries_vec.last().unwrap().major; 
                user.first.ino = user.entries_vec.last().unwrap().ino;
                user.first.minor = user.entries_vec.last().unwrap().minor;
                break;
            }

            let major = entry.major;
            let ino = entry.ino;
            let minor = entry.minor;

            // skip entry if it matches the starting values: it was processed in the last run
            if major == starting_major as u64 && ino == starting_ino as u64 && minor == starting_minor as u32 {
                continue;
            }
            
            // skip inode 1 (root directory)
            if ino == 1 {
               continue;
            }
            
            let path_struct = ScoutwrapInoPath {
                ino: ino,
                dir_ino: 0,
                dir_pos: 0,
                result_ptr: 0,
                result_bytes: STR_BUF_SIZE,
            };

            // path ioctl
            let path_res = scoutwrap_ino_path(&fs_root, path_struct.clone()); 
            let mut path = String::new();
            match path_res {
                Ok(p) => path = p.path,
                Err(e) => {
                    if std::io::Error::last_os_error().kind() == ErrorKind::NotFound {
                        // handle a case where a deleted files inode will still show up in the changelog
                        println!("WARNING: INO_PATH returned ENOENT. Skipping this entry.")
                    }
                    else {
                        panic!("scoutwrap_ino_path: {} on inode {}", e, ino);
                    }
                }
            }    

            // open fd for path 
            let fd;
            match nix::fcntl::openat(fs_root.as_fd(), Path::new(&path), OFlag::empty(), Mode::from_bits_truncate(S_IRWXU)) {
                Ok(f) => fd = f,
                Err(e) => {
                    panic!("openat: {} at {}", e, path);
                }
            }

            let stat_struct;
            match fstat(&fd) {
                Ok(s) => stat_struct = s,
                Err(e) => {
                    panic!("fstat: {} at {}", e, path);
                }
            }
            
            // skip directories
            if !SFlag::from_bits_truncate(stat_struct.st_mode).contains(SFlag::S_IFDIR) {
                
                if LOOP_VERBOSE {
                    println!("\npath: {path}");
                    println!("{:?}", entry);
                }

                let mut marfs_xattr = String::new();
                match wrap_libc_fgetxattr(fd.as_fd()) {
                    Ok(x) => marfs_xattr = x,
                    Err(e) => {
                        panic!("fgetxattr: {e} at {path}");
                    }
                }

                if LOOP_VERBOSE {
                    println!("{marfs_xattr}");
                }

                // get info from ftag
                let ftag;
                match get_ftag(&marfs_xattr) {
                    Ok(f) => ftag = f,
                    Err(e) => {
                        panic!("get_ftag: {e} at {path}");
                    }
                }

                let existing_xattrs = ScoutwrapListxattrHidden {
                    id_pos: 0,
                    xattr_list: Vec::new(),
                    buf_bytes: 4096,
                    hash_pos: 0,
                };

                // find if xattr exists for this file: just need to check first byte of return buffer
                let mut xattr_exists_bool = false;
                match scoutwrap_check_xattr_exists(fd.as_fd(), existing_xattrs) {
                    Ok(b) => xattr_exists_bool = b,
                    Err(e) => {
                        panic!("scoutwrap_listxattr_hidden: {e} at {path}");
                    }
                }
                
                if LOOP_VERBOSE {
                    println!("detected existing xattr: {:?}", xattr_exists_bool);
                }
                
                // get namespace root dir inode for xattr name
                let mut ns_path = String::new();
                match ns_path_from_streamid(&ftag) {
                    Ok(s) => ns_path = s,
                    Err(e) => {
                        panic!("ns_path_from_streamid: {e} at {path}");
                    }
                }
                
                if LOOP_VERBOSE {
                    println!("namespace path: {}", &ns_path);
                }

                // get namespace inode
                let ns_stat_struct;
                match fstatat(fs_root.as_fd(), Path::new(&ns_path), AtFlags::empty()) {
                    Ok(s) => ns_stat_struct = s,
                    Err(e) => {
                        panic!("fstatat: {e} at {}", &ns_path);
                    }
                }

                let int1 = QUOTA_MAGIC_NUM;
                let int2 = 0; // repo num
                let int3 = ns_stat_struct.st_ino;

                let xattr_name = format!("scoutfs.hide.totl.acct.{}.{}.{}", int1, int2, int3);

                if LOOP_VERBOSE {
                    println!("xattr name: {xattr_name}");
                }

                let file_mode;
                match get_marfs_file_mode(&marfs_xattr) {
                    Ok(m) => file_mode = m,
                    Err(e) => {
                        panic!("get_marfs_file_mode: {e}");
                    }
                }

                // if files are complete, have link count 2 and no xattr: add to quota
                if (!xattr_exists_bool && file_mode == "COMP" && stat_struct.st_nlink == 2) {
                    
                    if let Err(e) = wrap_libc_fsetxattr(fd.as_fd(), xattr_name, ftag.bytes.to_string(), ftag.bytes.to_string().len()) {
                        panic!("wrap_libc_fsetxattr: {e} at {path}");
                    }

                    ns_inode_cache.insert(ns_stat_struct.st_ino, ns_path.clone());


                    println!("Namespace {} Quota + {}", &ns_path, ftag.bytes);

                }
                // files that have an xattr but are user deleted: subtract from quota
                else if (xattr_exists_bool && stat_struct.st_nlink < 2) {
                    if let Err(e) = wrap_libc_fremovexattr(fd.as_fd(), xattr_name) {
                        panic!("wrap_libc_fremovexattr: {e} at {path}");
                    }

                    ns_inode_cache.insert(ns_stat_struct.st_ino, ns_path.clone());

                    println!("Namespace {} Quota - {}", &ns_path, ftag.bytes);
                    
                }
                else {
                    if LOOP_VERBOSE {
                        println!("no quota change");
                    }
                }
            }

            // set final state to the last file processed. This means the last file will be processed again in the next run, but this tool is idempotent.
            final_major = major;
            final_ino = ino;
            final_minor = minor;
        }

        let cur_time = start_time.elapsed();

        // save state on last batch or every CHECKPOINT_MS
        if last_batch || cur_time - last_checkpoint > Duration::from_millis(CHECKPOINT_MS) {
            if LOOP_VERBOSE {
                println!("checkpoint at {:?}", cur_time);
            }

            // update state file with final state
            if final_major != starting_major as u64 && final_major != 0 {
                let mut new_state_file = OpenOptions::new()
                                        .write(true)
                                        .create(true)
                                        .open(NEW_STATE_FILE)
                                        .expect("failed to open temporary state file");
                
                let write_str = format!("{}\n{}\n{}", final_major.to_string(), final_ino.to_string(), final_minor.to_string());
                new_state_file.write_all(write_str.as_bytes());

                std::fs::rename(NEW_STATE_FILE, STATE_FILE);
            }

            last_checkpoint = cur_time;
        }
        
        if last_batch {
            break;
        }
       
    }

    // update MarFS quotas

    match nswrap_gen_list(config.rootns, fs_root.as_fd()) {
        Ok(l) => {}
        Err(e) => panic!("{}", e),
    }

    // if there is nothing to do, the finals won't be updated from 0. print startings instead to not confuse user
    if final_major == 0 && final_ino == 0 && final_minor == 0 {
        println!("Finished quota_update at final state (major: {}, ino: {}, minor: {})", starting_major, starting_ino, starting_minor);
    }
    else {
        println!("Finished quota_update at final state (major: {}, ino: {}, minor: {})", final_major, final_ino, final_minor);
    }
}

// using libc fgetxattr to operations similar to
fn wrap_libc_fgetxattr(fd: BorrowedFd) -> Result<String, String> {

    unsafe {
        let value_str;
        let mut value_str_buf = libc::calloc(1, STR_BUF_SIZE); 
    
        if fgetxattr(fd.as_raw_fd(), CString::new("user.MDAL_MARFS-FILE").expect("bad path").as_ptr() as *const c_char, value_str_buf, STR_BUF_SIZE) == -1 {
            return Err(std::io::Error::last_os_error().to_string());
        } 

        match CString::into_string(CString::from_raw(value_str_buf as *mut i8)) {
            Ok(s) => value_str = s,
            Err(e) => return Err(e.to_string()),
        }
        Ok(value_str)
    }
}

// using libc fgetxattr to operations similar to
fn wrap_libc_fsetxattr(fd: BorrowedFd, name: String, value: String, length: usize) -> Result<(), String> {

    unsafe {
        if fsetxattr(fd.as_raw_fd(), CString::new(name).expect("bad name string").as_ptr() as *const c_char, CString::new(value).expect("bad value string").as_ptr() as *const c_void, length, 0) == -1 {
            return Err(std::io::Error::last_os_error().to_string());
        } 
    }

    Ok(())
}

fn wrap_libc_fremovexattr(fd: BorrowedFd, name: String) -> Result<(), String> {

    unsafe {
        if fremovexattr(fd.as_raw_fd(), CString::new(name).expect("bad name string").as_ptr() as *const c_char) == -1 {
            return Err(std::io::Error::last_os_error().to_string());
        } 
    }

    Ok(())
}

fn get_ftag(marfs_xattr: &str) -> Result<FTAG, String>{
    
    unsafe { 
        let ftag_buf = libc::calloc(1, std::mem::size_of::<FTAG>());

        if ftag_initstr(ftag_buf as *mut FTAG, CString::new(marfs_xattr).expect("bad xattr string").as_ptr() as *mut i8) == -1 {
            return Err(String::from("ftag_initstr returned an error"));
        }

        let ftag = Vec::from_raw_parts(ftag_buf as *mut FTAG, 1, 1)[0];
        
        return Ok(ftag) 

    }
}

fn ns_path_from_streamid(ftag: &FTAG) -> Result<String, String> {

    // convert streamid to Rust string
    let streamid_rust_str;

    unsafe {
        streamid_rust_str = CStr::from_ptr(ftag.streamid).to_string_lossy().into_owned();
    }

    let vec1: Vec<String> = streamid_rust_str.split("##").map(|s| s.to_string()).collect();
    
    if vec1.len() != 2 {
        
        return Err(String::from("incorrect vec1 length during streamid parsing"))
    }

    let mut vec2: Vec<&str> = vec1[1].split('#').collect();
    vec2.pop();

    if vec2.len() == 0 {
        return Err(String::from("incorrect vec2 length during streamid parsing"))
    }

    let mut ns_path = String::new();
    for entry in &vec2 {
        ns_path = ns_path + "MDAL_subspaces/" + entry + "/";
    }

    return Ok(ns_path)
}

fn get_marfs_file_mode(marfs_xattr: &str) -> Result<String, String> {

    if marfs_xattr.contains("INIT") {
        return Ok(String::from("INIT"));
    }
    else if marfs_xattr.contains("COMP") {
        return Ok(String::from("COMP"));
    }
    else {
        return Ok(String::new());
    }
}

/*
 * @param config_path: path of config to use or empty for env var MARFS_CONFIG_PATH
 * @return marfs_config struct
 */
fn wrap_config_init(config_path: String) -> Result<marfs_config, String> {
    
    let layout = Layout::new::<pthread_mutex_t>();
    let erasure_lock;
    unsafe {
        erasure_lock = alloc(layout) as *mut pthread_mutex_t; // allocate memory for pthread_mutex_t
        pthread_mutex_init(erasure_lock, ptr::null());

        let config;
        if config_path.is_empty() {
            config = config_init(CString::new(env::var("MARFS_CONFIG_PATH").expect("MARFS_CONFIG_PATH not set")).expect("bad MARFS_CONFIG_PATH string").as_ptr(), erasure_lock);
        }
        else {
            config = config_init(CString::new(config_path).expect("bad config_path string").as_ptr(), erasure_lock);
        }
        
        if config.is_null() {
            return Err(std::io::Error::last_os_error().to_string());
        }

        Ok(*config)
    }
}

fn get_root_ns_string(root_ns: *mut marfs_ns) -> String{
    unsafe { 
        CStr::from_ptr((*root_ns).idstr as *const i8).to_str().expect("bad namespace id string").to_owned() 
    }
}

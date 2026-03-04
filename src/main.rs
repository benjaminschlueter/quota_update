use quota_update::*;

mod scoutwrap;
use scoutwrap::*; 

use std::fs::OpenOptions;
use std::os::fd::{AsFd, AsRawFd, BorrowedFd};
use std::path::Path;
use std::ffi::{c_char, c_void, CString, CStr};
use std::io::Error;

use nix::fcntl::{OFlag, AtFlags};
use nix::sys::stat::{Mode, SFlag};
use nix::sys::stat::fstat;
use nix::sys::stat::fstatat;

const VERBOSE: bool = true;
const QUOTA_MAGIC_NUM: u32 = 123;
const BATCH_SIZE: usize = 3;
const FS_ROOT_PATH: &str = "/marfs/mdal-root2";

fn main() {

    // abort if not root
    if users::get_current_uid() != 0 {
        panic!("Must run as root!");
    }

    // open root_fd
    
    let fs_root = OpenOptions::new().read(true).open(FS_ROOT_PATH);
    if let Err(e) = fs_root {
        println!("open: {}", e);
        panic!("failed to open filesystem root");
    }
    let fs_root = fs_root.unwrap();

    // setup walk_inodes struct
    
    let first = ScoutwrapWalkInodesEntry {
        major: 0,
        ino: 0,
        minor: 0 ,  
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
        //index: SCOUTFS_IOC_WALK_INODES_META_SEQ,
        index: 0,
    };

    // process batches until entries vector is empty
    loop {

        let user_res = scoutwrap_walk_inodes(&fs_root, user.clone()); 

        match user_res {
            Ok(u) => user = u,
            Err(e) => {
                println!("scoutwrap_walk_inodes: {}", e);
                println!("skipping this batch");
                continue;
            }
        }

        // batch vector will never be empty: last element always part of next for full batches and non-full batches will be the last batch
        
        let mut last_batch = false;
        if user.entries_vec.len() < BATCH_SIZE {
            last_batch = true;
        }

        let mut major = 0; // will never not be updated to a non-zero major
        let mut ino = 0;
        let mut minor = 0;
        
        // process all but last element: last will be starting point of next run
        for entry in &user.entries_vec {
            
            // don't process the last entry of batches that are not the last. The last entry of the final batch will be processed.
            if !last_batch && entry.ino == user.entries_vec.last().unwrap().ino {
                user.first.major = user.entries_vec.last().unwrap().major; 
                user.first.ino = user.entries_vec.last().unwrap().ino;
                user.first.minor = user.entries_vec.last().unwrap().minor;
                break;
            }

            major = entry.major;
            ino = entry.ino;
            minor = entry.minor;

            
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
            let path;
            match path_res {
                Ok(p) => path = p.path,
                Err(e) => {
                    println!("scoutwrap_path_ino: {}", e);
                    println!("skipping this file...");
                    continue;
                }
            }    

            // open fd for path 
            let fd;
            match nix::fcntl::openat(fs_root.as_fd(), Path::new(&path), OFlag::empty(), Mode::from_bits_truncate(S_IRWXU)) {
                Ok(f) => fd = f,
                Err(e) => {
                    println!("openat: {} at {}", e, path);
                    println!("skipping this file...");
                    continue;
                }
            }

            let stat_struct;
            match fstat(&fd) {
                Ok(s) => stat_struct = s,
                Err(e) => {
                    println!("fstat: {} at {}", e, path);
                    println!("skipping this file...");
                    continue;
                }
            }
            
            // skip directories
            if !SFlag::from_bits_truncate(stat_struct.st_mode).contains(SFlag::S_IFDIR) {
                
                if VERBOSE {
                    println!("\npath: {path}");
                    println!("{:?}", entry);
                }

                let mut marfs_xattr = String::new();
                match wrap_libc_fgetxattr(fd.as_fd()) {
                    Ok(x) => marfs_xattr = x,
                    Err(e) => {
                        println!("fgetxattr: {e}"); // skip files with no MarFS xattr set
                        continue;
                    }
                }

                if VERBOSE {
                    println!("{marfs_xattr}");
                }

                // get info from ftag
                let ftag;
                match get_ftag(&marfs_xattr) {
                    Ok(f) => ftag = f,
                    Err(e) => {
                        println!("get_ftag: {e}");
                        continue;
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
                        println!("scoutwrap_listxattr_hidden: {e} at {path}");
                        continue;
                    }
                }
                
                if VERBOSE {
                    println!("detected existing xattr: {:?}", xattr_exists_bool);
                }
                
                // get namespace root dir inode for xattr name
                let mut ns_path = String::new();
                match ns_path_from_streamid(&ftag) {
                    Ok(s) => ns_path = s,
                    Err(e) => {
                        println!("ns_path_from_streamid: {e}");
                        continue;
                    }
                }
                
                if VERBOSE {
                    println!("namespace path: {ns_path}");
                }

                // get namespace inode
                let ns_stat_struct;
                match fstatat(fs_root.as_fd(), Path::new(&ns_path), AtFlags::empty()) {
                    Ok(s) => ns_stat_struct = s,
                    Err(e) => {
                        println!("fstatat: {e} at {ns_path}");
                        continue;
                    }
                }
                
                let int1 = QUOTA_MAGIC_NUM;
                let int2 = 0; // repo num
                let int3 = ns_stat_struct.st_ino;

                let xattr_name = format!("scoutfs.hide.totl.acct.{}.{}.{}", int1, int2, int3);

                if VERBOSE {
                    println!("xattr name: {xattr_name}");
                }

                let file_mode;
                match get_marfs_file_mode(&marfs_xattr) {
                    Ok(m) => file_mode = m,
                    Err(e) => {
                        println!("get_marfs_file_mode: {e}");
                        continue;
                    }
                }

                if VERBOSE {
                    println!("{}", file_mode)
                }

                // if files are complete, have link count 2 and no xattr: add to quota
                if (!xattr_exists_bool && file_mode == "COMP" && stat_struct.st_nlink == 2) {
                    
                    if let Err(e) = wrap_libc_fsetxattr(fd.as_fd(), xattr_name, ftag.bytes.to_string(), ftag.bytes.to_string().len()) {
                        println!("{e}");
                        continue;
                    }
                    
                    if VERBOSE {
                        println!("Namespace {ns_path} Quota + {}", ftag.bytes);
                    }
                }
                // files that have an xattr but are user deleted: subtract from quota
                else if (xattr_exists_bool && stat_struct.st_nlink < 2) {
                    if let Err(e) = wrap_libc_fremovexattr(fd.as_fd(), xattr_name) {
                        println!("{e}");
                        continue;
                    }

                    if VERBOSE {
                        println!("Namespace {ns_path} Quota - {}", ftag.bytes);
                    }
                }
                else {
                    if VERBOSE {
                        println!("no action taken");
                    }
                }
            }
        }

        if last_batch {
            break;
        }
       
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

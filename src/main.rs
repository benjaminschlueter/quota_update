use quota_update::*;

mod scoutwrap;
use scoutwrap::*; 

use std::fs::OpenOptions;
use std::os::fd::{AsFd, AsRawFd, BorrowedFd};
use std::path::Path;
use std::ffi::{c_char, c_void, CString};
use std::io::Error;

use nix::fcntl::OFlag;
use nix::sys::stat::{Mode, SFlag};
use nix::sys::stat::fstat;

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
                println!("{path}");

                let mut marfs_xattr = String::new();
                match wrap_libc_fgetxattr(fd.as_fd()) {
                    Ok(x) => marfs_xattr = x,
                    Err(e) => {
                        println!("{e}");
                        continue;
                    }
                }

                println!("{marfs_xattr}");
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

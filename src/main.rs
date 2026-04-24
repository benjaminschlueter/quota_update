use quota_update::*;

mod scoutwrap;
use scoutwrap::*; 

mod nswrap;
use nswrap::*;

use std::fs::OpenOptions;
use std::os::fd::{AsFd, AsRawFd, BorrowedFd};
use std::path::Path;
use std::ffi::{c_char, c_void, CString, CStr};
use std::io::ErrorKind;
use std::io::{BufReader, Read, Write};
use std::time::{Instant, Duration};
use std::alloc::{alloc, Layout};
use std::ptr;
use std::env;

use nix::fcntl::OFlag;
use nix::sys::stat::{Mode, SFlag};
use nix::sys::stat::fstat;

use rustix::fs::XattrFlags;

use clap::{Parser, ArgAction};

const QUOTA_MAGIC_NUM: u32 = 123;
const QUOTA_FILE_NAME: &str = "MDAL_datasize";

fn main() {

    // abort if not root
    if users::get_current_uid() != 0 {
        panic!("Must run as root!");
    }

    let args = Args::parse();

    // set vars 
    let CHECKPOINT_MS: u64; // WARNING: will fail to update state file if this is too small (< 100)
    match args.checkpoint_ms {
        Some(m) => CHECKPOINT_MS = m,
        None => CHECKPOINT_MS = 60000,
    }

    let BATCH_SIZE: usize;
    match args.batch_size {
        Some(b) => BATCH_SIZE = b,
        None => BATCH_SIZE = 65536,
    }

    let STATE_VERBOSE = args.state_verbose.unwrap();
    let LOOP_VERBOSE = args.loop_verbose.unwrap();
    let QUOTA_UPDATE_VERBOSE = args.quota_update_verbose.unwrap();
    
    /*
    match args.state_verbose {
        Some(s) => STATE_VERBOSE = s,
        None => STATE_VERBOSE = true,
    }

    let LOOP_VERBOSE: bool;
    match args.loop_verbose {
        Some(l) => LOOP_VERBOSE = l,
        None => LOOP_VERBOSE = false,
    }
    
    let QUOTA_UPDATE_VERBOSE: bool;
    match args.quota_update_verbose {
        Some(q) => QUOTA_UPDATE_VERBOSE = q,
        None => QUOTA_UPDATE_VERBOSE = true,
    }
    */

    let CONFIG_PATH: String;
    match args.config_path {
        Some(p) => CONFIG_PATH = p,
        None => CONFIG_PATH = env::var("MARFS_CONFIG_PATH").expect("no config path provided and MARFS_CONFIG_PATH not set"),
    }

    let STATE_FILE: String;
    let STATE_SWAP_FILE: String;
    match args.state_file_path {
        Some(p) => {
            STATE_FILE = p;
            STATE_SWAP_FILE = format!("{STATE_FILE}.swp");
        }
        None => {
            STATE_FILE = String::from(".state");
            STATE_SWAP_FILE = String::from(".state.swp");
        }
    }
   
    let FS_ROOT_PATH = args.root_scoutfs;
    
    let mut starting_major: i64 = 0;
    let mut starting_ino: i64 = 0;
    let mut starting_minor: i64 = 0;

    // read state from state file 
    let state_file_res = OpenOptions::new()
                        .read(true)
                        .open(&STATE_FILE);

    // if state file does not exist, create it and start from 0. On all other errors, panic. 

    match state_file_res {
        Ok(f) => {
            let mut reader = BufReader::new(&f);
            let mut starting_state_str = String::new();

            if let Err(e) = reader.read_to_string(&mut starting_state_str) {
                panic!("read_to_string: {}", e.to_string());
            }

            let input_vec: Vec<String> = starting_state_str.split("\n").map(|s| s.to_string()).collect();
            
            starting_major = input_vec[0].trim().parse().expect("state file does not contain valid integer");
            starting_ino = input_vec[1].trim().parse().expect("state file does not contain valid integer");
            starting_minor = input_vec[2].trim().parse().expect("state file does not contain valid integer");

            drop(f); // needs to close before rename at end of execution
        }
        Err(e) => {
            if e.kind() == ErrorKind::NotFound {
                if STATE_VERBOSE {
                    println!("No state file found: starting at initial state 0");
                }
            }
            else {
                panic!("open: {}\nFailed to open state file", e.to_string());
            }
        }
    }
    
    // check for existing STATE_SWAP_FILE 
    let state_file_new_res = OpenOptions::new()
                         .read(true)
                         .open(&STATE_SWAP_FILE);

    if let Ok(_f) = state_file_new_res {
        if STATE_VERBOSE {
            println!("Detected state tmp file... removing")
        }

        if let Err(e) = std::fs::remove_file(Path::new(&STATE_SWAP_FILE)) {
            panic!("failed to remove tmp state file: {e}");
        }
    }

    // open fd for filesystem root
    
    let fs_root = OpenOptions::new().read(true).open(&FS_ROOT_PATH);
    if let Err(e) = fs_root {
        panic!("open: {}\nFailed to open filesystem root at {}", e, &FS_ROOT_PATH);
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

    let mut final_major = 0;
    let mut final_ino = 0;
    let mut final_minor = 0;

    // MarFS config processing 

    let config;
    match wrap_config_init(CONFIG_PATH) {
        Ok(c) => config = c,
        Err(e) => panic!("config_init: {}", e),
    }

    // fill with inode mappings to all namespaces
    let ns_inode_map;
    match nswrap_build_map(config.rootns) {
        Ok(m) => ns_inode_map = m,
        Err(e) => panic!("nswrap_build_map: {e}"),
    }
    
    if STATE_VERBOSE {
        println!("Running quota_update with starting state (major: {}, ino: {}, minor: {})", starting_major, starting_ino, starting_minor);
    }

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
                final_major = major;
                final_ino = ino;
                final_minor = minor;
     
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

            // skip the quota file itself
            if path.contains(QUOTA_FILE_NAME) {
                final_major = major;
                final_ino = ino;
                final_minor = minor;
     
                continue;
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

            if ino != stat_struct.st_ino {
                panic!("ioctl result inode different from stat struct inode");
            }
            
            // skip directories
            if !SFlag::from_bits_truncate(stat_struct.st_mode).contains(SFlag::S_IFDIR) {
                
                if LOOP_VERBOSE {
                    println!("\npath: {path}");
                    println!("{:?}", entry);
                }
                
                let marfs_xattr: String;
                let mut buf = vec![0u8; STR_BUF_SIZE];
                match rustix::fs::fgetxattr(fd.as_fd(), String::from("user.MDAL_MARFS-FILE").as_bytes(), &mut buf) {
                    Ok(b) => {
                        buf.truncate(b);
                        marfs_xattr = String::from_utf8(buf).expect("bad xattr string");
                    }
                    Err(e) =>  {
                        if e == rustix::io::Errno::NODATA {
                            if LOOP_VERBOSE {
                                println!("MarFS xattr not found, skipping this file")
                            }
                            
                            final_major = major;
                            final_ino = ino;
                            final_minor = minor;
                             
                            continue;
                        }
                        else {
                            panic!("fgetxattr: {} at {}", e.to_string(), path);
                        }
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

                let streamid_key;
                match get_streamid_key(ftag) {
                    Ok(k) => streamid_key = k,
                    Err(e) => panic!("get_streamid_key: {e}"),
                }

                if LOOP_VERBOSE {
                    println!("streamid_key: {}", streamid_key);
                }

                let existing_xattrs = ScoutwrapListxattrHidden {
                    id_pos: 0,
                    xattr_list: Vec::new(),
                    buf_bytes: 4096,
                    hash_pos: 0,
                };

                let int1 = QUOTA_MAGIC_NUM;
                let int2 = 0; // repo num
                let int3;

                match ns_inode_map.get(&streamid_key) {
                    Some(i) => int3 = i,
                    None => panic!("no inode found for streamid {streamid_key}"),
                }

                let mut xattr_name = format!("scoutfs.hide.totl.acct.{}.{}.{}", int1, int2, int3);

                if LOOP_VERBOSE {
                    println!("xattr name: {xattr_name}");
                }

                // find if xattr exists for this file: just need to check first byte of return buffer
                let xattr_str_vec;
                match scoutwrap_listxattr_hidden(fd.as_fd(), existing_xattrs) {
                    Ok(v) => xattr_str_vec = v,
                    Err(e) => {
                        panic!("scoutwrap_listxattr_hidden: {e} at {path}");
                    }
                }

                // search xattr list for this programs xattr name
                let mut xattr_exists_bool = false;
                for xattr in &xattr_str_vec {
                    if *xattr == xattr_name {
                        xattr_exists_bool = true;
                        break;
                    }
                }

                // if there are any error strings in the vector and a valid xattr was not found, panic.
                // no need to panic if another xattr string errored and we have confirmation of a valid quota_update xattr
                if !xattr_exists_bool {
                    for xattr in &xattr_str_vec {
                        if (*xattr).contains("error") {
                            panic!("found an error string in xattr_list without prior confirmation of a valid quota_update xattr");
                        }
                }
                }
                
                if LOOP_VERBOSE {
                    println!("detected existing xattr: {:?}", xattr_exists_bool);
                }

                let file_mode;
                match get_marfs_file_mode(&marfs_xattr) {
                    Ok(m) => file_mode = m,
                    Err(e) => {
                        panic!("get_marfs_file_mode: {e}");
                    }
                }

                // if files are complete, have link count 2 and no xattr: add to quota
                if !xattr_exists_bool && file_mode == "COMP" && stat_struct.st_nlink == 2 {

                    if let Err(e) = rustix::fs::fsetxattr(fd.as_fd(), xattr_name.as_bytes(), ftag.bytes.to_string().as_bytes(), XattrFlags::empty()) {
                        panic!("fsetxattr: {e} at {path}");
                    }

                    if LOOP_VERBOSE {
                        println!("Namespace {} Quota + {}", &streamid_key, ftag.bytes);
                    }

                }
                // files that have an xattr but are user deleted: subtract from quota
                else if xattr_exists_bool && stat_struct.st_nlink < 2 {

                    if let Err(e) = rustix::fs::fremovexattr(fd.as_fd(), xattr_name.as_bytes()) {
                        panic!("fremovexattr: {e} at {path}");
                    }

                    if LOOP_VERBOSE {
                        println!("Namespace {} Quota - {}", &streamid_key, ftag.bytes);
                    }
                    
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
                                        .open(&STATE_SWAP_FILE)
                                        .expect("failed to open temporary state file");
                
                let write_str = format!("{}\n{}\n{}", final_major.to_string(), final_ino.to_string(), final_minor.to_string());
                
                if let Err(e) = new_state_file.write_all(write_str.as_bytes()) {
                    panic!("failed to write new state: {}", e.to_string());
                }

                if let Err(e) = std::fs::rename(&STATE_SWAP_FILE, &STATE_FILE) {
                    panic!("failed to rename tmp state file: {}", e.to_string())
                }
                
                // print an update for each checkpoint
                if QUOTA_UPDATE_VERBOSE && !last_batch {
                    println!("MAJOR {} CHECKPOINT:", final_major);
                    if let Err(e) = nswrap_update_quota(config.rootns, fs_root.as_fd()) {
                        panic!("nswrap_update_quota: {}", e);
                    }
                    println!("");
                }

            }

            last_checkpoint = cur_time;
        }
        
        if last_batch {
            break;
        }
       
    }
   
    if QUOTA_UPDATE_VERBOSE { 
        if let Err(e) = nswrap_update_quota(config.rootns, fs_root.as_fd()) {
                panic!("nswrap_update_quota: {}", e);
        }
    }

    if STATE_VERBOSE {
        if final_major == 0 && final_ino == 0 && final_minor == 0 {
            println!("Finished quota_update at final state (major: {}, ino: {}, minor: {})", starting_major, starting_ino, starting_minor);
            return;
        }
        else {
            println!("Finished quota_update at final state (major: {}, ino: {}, minor: {})", final_major, final_ino, final_minor);
        }
    }

}

/* Hide ugly unsafe code of creating FTAG from marfs xattr
 */
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

/* Return owned string if xattr contains, INIT, COMP or neither
 */
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

/* Hide ugliness of calling config_init from Rust
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

/* Turns FTAG.streamid into a namespace unique key for the hash table
 * @param ftag: FTAG struct for this file
 * @return: owned string with format <REPO>##<NS1>#<NS2>#<NS...>
 */
fn get_streamid_key(ftag: FTAG) -> Result<String, String> {
    unsafe {
        let full = CStr::from_ptr(ftag.streamid as *const i8).to_str().expect("bad streamid string").to_owned(); 
        
        let mut vec1: Vec<String> = full.split("#").map(|s| s.to_string()).collect();

        if vec1.len() < 3 {
            return Err(String::from("incorrect vec1 length during streamid parsing"))
        }

        vec1.pop();

        Ok(vec1.join("#"))
    }
}

/// MarFS quota management tool based on ScoutFS xattr accounting
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
        /// Frequency of checkpoints to the state file and mid run quota updates [default: 60000]
        #[arg(short = 'm', long)]
        checkpoint_ms: Option<u64>,
        
        /// Number of inodes to process in a single batch [default: 65536]
        #[arg(short, long)]
        batch_size: Option<usize>,

        /// Path to MarFS config if not using $MARFS_CONFIG_PATH
        #[arg(short = 'c', long)]
        config_path: Option<String>,

        /// Print info on start/final state and state file existence [default: true]
        #[arg(short, long, action = ArgAction::SetTrue)]
        state_verbose: Option<bool>,

        /// Print details for each processing step for each file. For debugging purposes (lots of output) [default: false]
        #[arg(short, long, action = ArgAction::SetTrue)]
        loop_verbose: Option<bool>,

        /// Print the updated quotas at each checkpoint and the end of the run [default: true]
        #[arg(short, long, action = ArgAction::SetTrue)]
        quota_update_verbose: Option<bool>,

        /// Location of state file (and state swap file) [default: ".state"]
        #[arg(short = 'p', long)]
        state_file_path: Option<String>,
       
        /// Root of ScoutFS filesystem 
        #[arg(short, long)]
        root_scoutfs: String,
}     



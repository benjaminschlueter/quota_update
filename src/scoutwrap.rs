use quota_update::*;

use std::fs::File;
use std::os::fd::AsRawFd;
use std::mem;
use std::alloc;
use std::alloc::Layout;
use std::ptr;
use std::io;
use libc::calloc;
use libc::c_void;


/* Represents a point in the ScoutFS changelog.
 * @param major: major timestamp
 * @param ino: inode number
 * @param minor: minor timestamp
 */
#[derive(Debug)]
pub struct ScoutwrapWalkInodesEntry {
    pub major: u64,
    pub ino: u64,
    pub minor: u32,
}

/* To be used with scoutwrap_walk_inodes()
 * @param first: starting point in the change log
 * @param last: stop point in the change log
 * @param entries_vec: to be populated with ScoutwrapWalkInodesEntry structs. Will be overwritten at the end of the function. 
 * @param nr_entries: tells ScoutFS the limit of entry structs to fill the buffer with
 * @param index: see ScoutFS ioctl.h for which macro to set this with
 */
#[derive(Debug)]
pub struct ScoutwrapWalkInodes {
    pub first: ScoutwrapWalkInodesEntry,
    pub last: ScoutwrapWalkInodesEntry,
    pub entries_vec: Vec<ScoutwrapWalkInodesEntry>, // MUST BE EMPTY
    pub nr_entries: usize,
    pub index: u8,
}


/* WALK_INODES
 * Populates the entries_vec in user_arg to contain nr_entries entry structs from inodes within the minor:major range. This function allocates the buffer. The caller does not have to worry about setting up a buffer.  
 * It returns the user's struct back to them instead. 
 */
pub fn scoutwrap_walk_inodes(root_fs: File, mut user_arg: ScoutwrapWalkInodes) -> Result<ScoutwrapWalkInodes, io::Error> {

    // create scoutfs_ioctl_walk_inodes and entries structs

    let first_c = scoutfs_ioctl_walk_inodes_entry {
        major: user_arg.first.major as __u64,
        ino: user_arg.first.ino as __u64,
        minor: user_arg.first.minor as __u32,
        _pad: [0u8; 4usize],
    };
    
    let last_c = scoutfs_ioctl_walk_inodes_entry {
        major: user_arg.last.major as __u64,
        ino: user_arg.last.ino as __u64,
        minor: user_arg.last.minor as __u32,
        _pad: [0u8; 4usize],
    };
    
    let entries_ptr;
    unsafe {
        entries_ptr = libc::calloc(user_arg.nr_entries as usize, std::mem::size_of::<scoutfs_ioctl_walk_inodes_entry>());
        if entries_ptr.is_null() {

        }
    }


    let mut user = scoutfs_ioctl_walk_inodes {
        first: first_c,
        last: last_c,
        entries_ptr: entries_ptr as u64,
        nr_entries: user_arg.nr_entries as u32, 
        index: user_arg.index,
        _pad: [0u8; 11usize],
    };
   
    let entries_c; 
    unsafe {
        if wrap_walk_inodes(root_fs.as_raw_fd(), &mut user) == -1 {
            println!("wrap_walk_inodes failed");
        }

        // takes ownership of the calloc buffer and will drop it
        entries_c = Vec::from_raw_parts(user.entries_ptr as *mut scoutfs_ioctl_walk_inodes_entry, user.nr_entries as usize, user.nr_entries as usize);
    }        
     
    let mut entries = Vec::<ScoutwrapWalkInodesEntry>::new();
    
    // add entries to the rust buffer until the end is found
    for entry in &entries_c {

        if entry.major == 0 && entry.ino == 0 && entry.minor == 0 {
            break;
        }

        let tmp = ScoutwrapWalkInodesEntry { 
            major: entry.major as u64, 
            ino: entry.ino as u64,
            minor: entry.minor as u32,
        };

        entries.push(tmp);
    }   

    user_arg.entries_vec = entries;

    Ok(user_arg)

    

}




















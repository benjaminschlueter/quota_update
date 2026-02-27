use quota_update::*;

mod scoutwrap;
use scoutwrap::*; 

use std::fs::OpenOptions;

const BATCH_SIZE: usize = 128;
const FS_ROOT_PATH: &str = "/marfs/mdal-root2";

fn main() {

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

    user = scoutwrap_walk_inodes(fs_root, user).expect("falied to allocate buffer"); 

    println!("{:?}", user.entries_vec);

}

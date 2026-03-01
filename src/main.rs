use quota_update::*;

mod scoutwrap;
use scoutwrap::*; 

use std::fs::OpenOptions;

const BATCH_SIZE: usize = 3;
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
        
        if user.entries_vec.is_empty() {
            break;
        }

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
            println!("{:?}", entry);
        }

        if last_batch {
            break;
        }
       
    }
}

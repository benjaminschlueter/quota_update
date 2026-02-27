
/* Represents a point in the ScoutFS changelog.
 * @param major: major timestamp
 * @param ino: inode number
 * @param minor: minor timestamp
 */
pub struct scoutwrap_walk_inodes_entry {
    pub major: u64,
    pub ino: u64,
    pub minor: u32,
}

/* To be used with scoutwrap_walk_inodes()
 * @param first: starting point in the change log
 * @param last: stop point in the change log
 * @param entries_vec: to be populated with scoutwrap_walk_inodes_entry structs. Will be overwritten at the end of the function. 
 * @param nr_entries: tells ScoutFS the limit of entry structs to fill the buffer with
 * @param index: see ScoutFS ioctl.h for which macro to set this with
 */
pub struct scoutwrap_walk_inodes {
    pub first: scoutwrap_walk_inodes_entry,
    pub last: scoutwrap_walk_inodes_entry,
    pub entries_vec: Vec<scoutwrap_walk_inodes_entry>, // MUST BE EMPTY
    pub nr_entries: u32,
    pub index: u8,
}


/* WALK_INODES
 * 
 */
/*
fn scoutwrap_walk_inodes() {

}
*/

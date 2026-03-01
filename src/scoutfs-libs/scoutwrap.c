/* The ScoutFS ioctl is kernel code and does not provide a library that Rust can link to.
 * This file serves as a wrapper for the ioctl and any ScoutFS code that needs to be called
 * from C.
 */

#include <stdio.h>

#include "scoutwrap.h"

// WALK_INODES
int wrap_walk_inodes(int root_fd, struct scoutfs_ioctl_walk_inodes *user) {
	return ioctl(root_fd, SCOUTFS_IOC_WALK_INODES, user); // access errno from Rust
}

// INO_PATH
int wrap_ino_path(int root_fd, struct scoutfs_ioctl_ino_path *path) {
	return ioctl(root_fd, SCOUTFS_IOC_INO_PATH, path); // access errno from Rust
}

// return first path string in INO_PATH buffer
char* wrap_ino_path_get_str(int root_fd, struct scoutfs_ioctl_ino_path *path) {
	if (ioctl(root_fd, SCOUTFS_IOC_INO_PATH, path) == -1) {
		perror("ioctl(SCOUTFS_IOC_INO_PATH)");
		return NULL;
	}	

	/*
	char* first_str = calloc(1, 4096); // generous buf size, this won't use buf size value defined above in Rust
	sprintf(first_str, "%s", path.path); // could break if scoutfs returns bad data
	*/

	return ((struct scoutfs_ioctl_ino_path_result *) path->result_ptr)->path;
	
}
// LISTXATTR_HIDDEN
int wrap_listxattr_hidden(int root_fd, struct scoutfs_ioctl_listxattr_hidden *existing_attrs) {
	return ioctl(root_fd, SCOUTFS_IOC_LISTXATTR_HIDDEN, existing_attrs); // access errno from Rust
}
// READ_XATTR_TOTALS
int wrap_read_xattr_totals(int root_fd, struct scoutfs_ioctl_read_xattr_totals *totals) {
	return ioctl(root_fd, SCOUTFS_IOC_READ_XATTR_TOTALS, totals); // access errno from Rust
}

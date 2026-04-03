#include <stdio.h>
#include <stdlib.h>
#include <assert.h>

#include "nswrap.h"

int glob_buf_offset = 0; // MAKE ATOMIC

/* Calls the ScoutFS ioctl function READ_XATTR_TOTALS to get a list of xattrs and quota usages per namespace. 
 * Calls recursive function to walk the namespace tree and update the quota file to the ioctl returned value.
 * This is implemented in C to prevent complicated back and forth type conversion in Rust.
 */
int nswrap_update_quota_c(marfs_ns* root_ns, int root_fd) {
    if (root_ns == NULL) {
        return -1;
    }

    if (root_fd == 0) {
        return -1;
    }

    // scoutfs ioctl: get (inode, usage) buffer
    struct scoutfs_ioctl_read_xattr_totals *totals = calloc(1, sizeof(struct scoutfs_ioctl_read_xattr_totals));
    totals->totals_bytes = 4096;
    totals->totals_ptr = calloc(1, totals->totals_bytes);

    if (ioctl(root_fd, SCOUTFS_IOC_READ_XATTR_TOTALS, totals) < 0) {
            perror("ioctl(SCOUTFS_IOC_READ_XATTR_TOTALS)");
    }
    
    struct scoutfs_ioctl_xattr_total *xattr_totals_buf = totals->totals_ptr;

    if (xattr_totals_buf == NULL) {
        return -1;
    }

    rec_ns_subspace_walk_quota(root_ns, xattr_totals_buf);

    return 0;
}

/* Recursive tree walk function for updating namespace quotas. Stats the MarFS namespace to get inode, searches xattr buffer for quota and updates quota file.
*/
int rec_ns_subspace_walk_quota(marfs_ns* ns, struct scoutfs_ioctl_xattr_total* xattr_totals_buf) {
        
    // stat namespace root to get inode
    MDAL_CTXT dup_ctxt = ns->prepo->metascheme.mdal->dupctxt(ns->prepo->metascheme.mdal->ctxt);
    if (dup == NULL) {
        perror("mdal->dupctxt");
        return -1;
    }

    // get NS path from idstr
    char* repo_str = calloc(1, STR_BUF_SIZE_C);
    char* path_str = calloc(1, STR_BUF_SIZE_C);
    if (config_nsinfo(ns->idstr, &repo_str, &path_str) == -1) {
        perror("ns_info");
        return -1;
    }
    
    if (ns->prepo->metascheme.mdal->setnamespace(dup_ctxt, path_str) == -1) {
        perror("mdal->setnamespace");
        return -1;
    }

    struct stat* ns_stat = calloc(1, sizeof(struct stat));

    if (ns->prepo->metascheme.mdal->stat(dup_ctxt, ".", ns_stat, 0) == -1) {
        perror("mdal->stat");
        return -1;
    }

    int ns_ino = ns_stat->st_ino;
  
    // search xattr_totals_buf for namespace size
    int i = 0;
    while (xattr_totals_buf[i].name[2] != 0) {
        if (xattr_totals_buf[i].name[2] == (u64) ns_ino) {
            	
	    if (VERBOSE_C) {
	    	printf("%s%s QUOTA: %lld\n", repo_str, path_str, xattr_totals_buf[i].total);
            }

            // update quota file in namespace through MDAL
            if (ns->prepo->metascheme.mdal->setdatausage(dup_ctxt, xattr_totals_buf[i].total) == -1) {
                perror("mdal->setdatausage");
                return -1;
            }
        }

        i++;
    }

    // operate on subspaces if any
    for (int i = 0; i < ns->subnodecount; i++) {
        assert(rec_ns_subspace_walk_quota((marfs_ns *) (ns->subnodes + i)->content, xattr_totals_buf) != -1);
    }

    return 0;
}

/* Creates a buffer to be converted to a Rust Hashmap of NS key strings and inodes.
 * Calls recursive tree walk function to stat each namespace root.
 */
nswrap_entry* nswrap_build_map_c(marfs_ns* root_ns) {
    
    if (root_ns == NULL) {
        return NULL;
    }

    // allocate buffer
    nswrap_entry* map_buf = calloc(1, BUF_MAX_NS_COUNT);

    if (map_buf == NULL) {
        return NULL;
    }

    rec_ns_subspace_walk_map(root_ns, map_buf);

    return map_buf;

}

/* Recursive tree walk function for building ns, inode hash table.  
 */
int rec_ns_subspace_walk_map(marfs_ns* ns, nswrap_entry* map_buf) {
    
    // stat namespace root to get inode
    MDAL_CTXT dup_ctxt = ns->prepo->metascheme.mdal->dupctxt(ns->prepo->metascheme.mdal->ctxt);
    if (dup == NULL) {
        perror("mdal->dupctxt");
        return -1;
    }

    // get NS path from idstr
    char* repo_str = calloc(1, STR_BUF_SIZE_C);
    char* path_str = calloc(1, STR_BUF_SIZE_C);
    if (config_nsinfo(ns->idstr, &repo_str, &path_str) == -1) {
        perror("ns_info");
        return -1;
    }

    if (ns->prepo->metascheme.mdal->setnamespace(dup_ctxt, path_str) == -1) {
        perror("mdal->setnamespace");
        return -1;
    }

    struct stat* ns_stat = calloc(1, sizeof(struct stat));

    if (ns->prepo->metascheme.mdal->stat(dup_ctxt, ".", ns_stat, 0) == -1) {
        perror("mdal->stat");
        return -1;
    }

    map_buf[glob_buf_offset].ino = ns_stat->st_ino;
    map_buf[glob_buf_offset].ns_key = calloc(1, STR_BUF_SIZE_C);

    // copy formatted string to ns_key
    snprintf(map_buf[glob_buf_offset].ns_key, STR_BUF_SIZE_C, "%s##%s", repo_str, path_str + 1);

    glob_buf_offset++;
    
    for (int i = 0; i < ns->subnodecount; i++) {
        assert(rec_ns_subspace_walk_map((marfs_ns *) (ns->subnodes + i)->content, map_buf) != -1);
    }

    return 0;
}



#include <stdio.h>
#include <stdlib.h>
#include <assert.h>

#include "nswrap.h"

int glob_buf_offset = 0;

/* Take the root namespace and return a buffer of structs containing inode, ns path mappings.
 * @return pointer to buffer on success, NULL on all error cases
 */
nswrap_entry_c* nswrap_gen_list_c(marfs_ns* root_ns, int root_fd) {
    if (root_ns == NULL) {
        return NULL;
    }

    if (root_fd == 0) {
        return NULL;
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
        return NULL;
    }

    nswrap_entry_c* nswrap_buf = calloc(4096, sizeof(nswrap_entry_c));

    if (nswrap_buf == NULL) {
        return NULL;
    }

    rec_fill_ns_buf(root_ns, nswrap_buf, xattr_totals_buf);

    return NULL;
}


int rec_fill_ns_buf(marfs_ns* ns, nswrap_entry_c* nswrap_buf, struct scoutfs_ioctl_xattr_total* xattr_totals_buf) {
    
    printf("%s\n", (char *) ns->idstr);
    
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
    printf("ns inode: %d\n", ns_ino);

    // search xattr_totals_buf for namespace size
    int i = 0;
    while (xattr_totals_buf[i].name[2] != 0) {
        if (xattr_totals_buf[i].name[2] == (u64) ns_ino) {
            // mdal->setdatausage to update trunc file
            printf("Updating %s quota to %d\n", path_str, xattr_totals_buf[i].total);
            if (ns->prepo->metascheme.mdal->setdatausage(dup_ctxt, xattr_totals_buf[i].total) == -1) {
                perror("mdal->setdatausage");
                return -1;
            }
        }

        i++;
    }

    for (int i = 0; i < ns->subnodecount; i++) {
        assert(rec_fill_ns_buf((marfs_ns *) (ns->subnodes + i)->content, nswrap_buf, xattr_totals_buf) != -1);
    }

    return 0;
}
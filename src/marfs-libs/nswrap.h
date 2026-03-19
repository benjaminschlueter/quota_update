#include "config/config.h"

#include <sys/ioctl.h>
#include "utils/src/util.h"
#include "kmod/src/ioctl.h"

#define BUF_MAX_NS_COUNT 4096
#define STR_BUF_SIZE_C 512

typedef struct nswrap_entry_c_ {
    int inode;
    char* path;
} nswrap_entry_c;

nswrap_entry_c* nswrap_gen_list_c(marfs_ns* root_ns, int root_fd);

int rec_fill_ns_buf(marfs_ns* ns, nswrap_entry_c* nswrap_buf, struct scoutfs_ioctl_xattr_total* xattr_totals_buf);


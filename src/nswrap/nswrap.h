#include "config/config.h"

#include <sys/ioctl.h>
#include "utils/src/util.h"
#include "kmod/src/ioctl.h"

#define BUF_MAX_NS_COUNT 4096
#define STR_BUF_SIZE_C 512
#define VERBOSE_C 1

typedef struct nswrap_entry_ {
    char* ns_key;
    u64 ino;
} nswrap_entry;

int nswrap_update_quota_c(marfs_ns* root_ns, int root_fd);
int rec_ns_subspace_walk_quota(marfs_ns* ns, struct scoutfs_ioctl_xattr_total* xattr_totals_buf);

nswrap_entry* nswrap_build_map_c(marfs_ns* root_ns);
int rec_ns_subspace_walk_map(marfs_ns* ns, nswrap_entry* map_buf);


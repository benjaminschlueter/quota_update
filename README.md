# quota_update

A tool for updating MarFS namespace quotas using ScoutFS xattr capabilities.

## Building

Cargo must be set up to run the build script

The build script assumes MarFS and ScoutFS are subdirectories of src: src/marfs and src/scoutfs

The user is responsible for manually compiling MarFS and ScoutFS

The wrappers nswrap and scoutwrap are automatically compiled by the build script

## Running

The ./run executable exists as a shortcut for executing the program without cargo

Must run as root

Environment variable MARFS_CONFIG_PATH should be set, unless the CONFIG_PATH defined string in main.rs is preferred

## Behavior

Interfaces with the ScoutFS ioctl to obtain batches of recently changed files. Iterates through the batches and sets or removes ScoutFS xattrs when a file is newly added or deleted. Each namespace has a unique xattr consisting of three integers, the last being the inode of the namespace root directory. All files within a namespace get this xattr, whose value is the size of the file. The ScoutFS command READ_XATTR_TOTALS reports the sum of the values of all files with a specific name, which are the namespace total usages. The MarFS MDAL is used to update the usage file on each run.

This tool maintains state in a file, containing the ScoutFS major/minor timestamp values and the inode. This state file is updated at the end of batches after a certain duration, configurable by editing the CHECKPOINT_MS constant. Quotas are only updated at the end of execution at the end of the changelog. On failure, the tool will restart from the last checkpoint. This tool is idempotent. Reprocessing inodes from previous majors has no effect. The state file can be removed completely to make the tool start processing from major 0 to cover every file in the change log.  

The ScoutFS changelog sometimes lags behind, so it can sometimes take seconds to minutes for changes to appear and quotas to be updated. 

We panic before proceding with bad state. If the program panics on a file, LOOP_VERBOSE can be enabled to get more info on the file that caused the error for an admin to investigate. 

## Configuration

Batch size, checkpoint duration, verbosity, and all static paths can be changed by defining and recompiling.




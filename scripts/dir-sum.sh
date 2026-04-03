find $1 -type f -print0 | du --files0-from=- -bc | tail -n 1

use std::{path::PathBuf, process::Command};

const SCOUTWRAP_PATH: &'static str = "src/scoutwrap";
const SCOUTFS_PATH: &'static str = "src/scoutfs";
const NSWRAP_PATH: &'static str = "src/nswrap";
const MARFS_PATH: &'static str = "src/marfs";

fn main() {
    println!("cargo:rerun-if-changed={}/scoutwrap.c", SCOUTWRAP_PATH);
    println!("cargo:rerun-if-changed={}/scoutwrap.h", SCOUTWRAP_PATH);
    println!("cargo:rerun-if-changed={}/Makefile", SCOUTWRAP_PATH);

    println!("cargo:rerun-if-changed={}/nswrap.c", NSWRAP_PATH);
    println!("cargo:rerun-if-changed={}/nswrap.h", NSWRAP_PATH);
    println!("cargo:rerun-if-changed={}/Makefile", NSWRAP_PATH);

    // compile C libraries nswrap, scoutwrap; user is responsible for compiling MarFS, ScoutFS on their own
    Command::new("make")
        .arg("-C")
        .arg(NSWRAP_PATH)
        .arg("clean")
        .status()
        .expect("failed to clean nswrap");

    Command::new("make")
        .arg("-C")
        .arg(NSWRAP_PATH)
        .status()
        .expect("failed to make nswrap");

    Command::new("make")
        .arg("-C")
        .arg(SCOUTWRAP_PATH)
        .arg("clean")
        .status()
        .expect("failed to clean scoutwrap");

    Command::new("make")
        .arg("-C")
        .arg(SCOUTWRAP_PATH)
        .status()
        .expect("failed to make scoutwrap");

    let bindings_path = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("bindings.rs");

    // ScoutFS is kernel code and does not provide libs; Need a user library wrapper for the ioctl

    let bindings = bindgen::Builder::default()
        .header(format!("{}/src/marfs_auto_config.h", MARFS_PATH))
        .header(format!("{}/src/config/config.h", MARFS_PATH))
        .header(format!("{}/src/tagging/tagging.h", MARFS_PATH))
        .header(format!("{}/scoutwrap.h", SCOUTWRAP_PATH))
        .header(format!("{}/nswrap.h", NSWRAP_PATH))
        .clang_arg(format!("-I{}/src", MARFS_PATH))
        .clang_arg(format!("-I{}", SCOUTFS_PATH))
        .clang_arg("-I/usr/include/libxml2")
        .allowlist_item("BUF_MAX_NS_COUNT")
        .allowlist_item("S_IRWXU")
        .allowlist_type("marfs_ns")
        .allowlist_function("nswrap_update_quota_c")
        .allowlist_function("nswrap_build_map_c")
        .allowlist_type("nswrap_entry")
        .allowlist_type("FTAG")
        .allowlist_function("ftag_initstr")
        .allowlist_type("pthread_mutex_t")
        .allowlist_function("pthread_mutex_init")
        .allowlist_function("config_init")
        .allowlist_type("scoutfs_ioctl_walk_inodes")
        .allowlist_type("scoutfs_ioctl_walk_inodes_entry")
        .allowlist_function("wrap_walk_inodes")
        .allowlist_type("scoutfs_ioctl_ino_path")
        .allowlist_type("scoutfs_ioctl_ino_path_result")
        .allowlist_function("wrap_ino_path")
        .allowlist_type("scoutfs_ioctl_listxattr_hidden")
        .allowlist_function("wrap_listxattr_hidden")
        .generate()
        .expect("Failed to generate bindings");

    bindings
        .write_to_file(bindings_path)
        .expect("Failed to write to {bindings_path}");

    println!(
        "cargo:rustc-link-arg=-Wl,-rpath,{}",
        format!("{}/src/api/.libs", MARFS_PATH)
    );
    println!(
        "cargo:rustc-link-search={}",
        format!("{}/src/api/.libs", MARFS_PATH)
    );
    println!("cargo:rustc-link-search={}", SCOUTWRAP_PATH);
    println!("cargo:rustc-link-search={}", NSWRAP_PATH);
    println!(
        "cargo:rustc-env=LD_LIBRARY_PATH={}:{}:{}",
        format!("{}/src/api/.libs", MARFS_PATH),
        SCOUTWRAP_PATH,
        NSWRAP_PATH
    );
    println!("cargo:rustc-link-lib=marfs");
    println!("cargo:rustc-link-lib=scoutwrap");
    println!("cargo:rustc-link-lib=nswrap");
}

use std::{env, path::PathBuf, process::Command};

fn main() {
    println!("cargo::rerun-if-changed=src/clib/builtin.c");
    println!("cargo::rerun-if-changed=src/clib/bdwgc/*");
    let out_dir = env::var("OUT_DIR").unwrap();

    Command::new("mkdir")
        .args(["src/clib/bdwgc/build"])
        .status()
        .unwrap();
    Command::new("cmake")
        .current_dir("src/clib/bdwgc/build")
        .args([".."])
        .status()
        .unwrap();
    Command::new("cmake")
        .current_dir("src/clib/bdwgc/build")
        .args(["--build", "."])
        .status()
        .unwrap();

    let out_path = PathBuf::from(out_dir).join("builtin.bc");

    let status = Command::new("clang")
        .args([
            "-emit-llvm",
            "-c",
            "src/clib/builtin.c",
            "-o",
            out_path.to_str().unwrap(),
        ])
        .status()
        .unwrap();

    if !status.success() {
        panic!("Failed to compile builtin.c");
    }
}

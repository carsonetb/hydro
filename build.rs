use cc::Build;

fn main() {
    println!("cargo::rerun-if-changed=src/clib/hashmap.c/hashmap.c");
    println!("cargo::rerun-if-changed=src/clib/builtin.c");
    Build::new()
        .file("src/clib/hashmap.c/hashmap.c")
        .compile("hashmap");
}

use std::env;
use std::path::PathBuf;

fn main() {

    let descriptor_path = PathBuf::from(env::var("OUT_DIR").unwrap()).join("api.bin");

    tonic_build::configure()
//        .build_client(false)
       // .file_descriptor_set_path("api.bin")
       .file_descriptor_set_path(descriptor_path)
        .compile(&["protos/api.proto"], &[".","googleapis"])
        .unwrap();
}
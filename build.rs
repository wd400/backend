use std::env;
use std::path::PathBuf;
use std::fs;

fn main() -> std::io::Result<()> {

  let filename="api_pb.bin";
    let descriptor_path = PathBuf::from(env::var("OUT_DIR").unwrap()).join(filename);

    tonic_build::configure()
//        .build_client(false)
       // .file_descriptor_set_path("api.bin")
       .file_descriptor_set_path(filename)
     //  .file_descriptor_set_path(descriptor_path)
        .compile(&["protos/api.proto"], &[".","googleapis"])
        .unwrap();

    fs::copy(filename, descriptor_path)?;
    fs::rename(filename, PathBuf::from("docker-compose").join(filename))?;
    
    Ok(())
}
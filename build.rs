use std::env;
use std::path::PathBuf;
use std::fs;

fn main() -> std::io::Result<()> {

  let serde_headers="#[derive(serde::Serialize, serde::Deserialize)]";
  let filename="api_pb.bin";
    let descriptor_path = PathBuf::from(env::var("OUT_DIR").unwrap()).join(filename);

    tonic_build::configure()
     //   .build_client(false)
       // .file_descriptor_set_path("api.bin")
       .file_descriptor_set_path(filename)
       .type_attribute(".", serde_headers)
   //    .field_attribute(path, attribute)
   // .server_attribute("Api", "#[derive(Debug)]")
     //  .file_descriptor_set_path(descriptor_path)
        .compile(&["protos/api.proto"], &[".","googleapis"])
        .unwrap();

    fs::copy(filename, descriptor_path)?;
    fs::rename(filename, PathBuf::from("docker-compose").join(filename))?;
    
    Ok(())
}
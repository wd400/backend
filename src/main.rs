pub mod api {
    tonic_include_protos::include_protos!("v1");

    pub(crate) const FILE_DESCRIPTOR_SET: &[u8] =
    tonic::include_file_descriptor_set!("api");
}

mod service;
use crate::service::{MyApi};


use api::v1::{api_server::{ApiServer}};


 #[tokio::main]
 async fn main() -> anyhow::Result<()> {
     let addr = ([0, 0, 0, 0], 3000).into();

     let reflection = tonic_reflection::server::Builder::configure()
     .register_encoded_file_descriptor_set(api::FILE_DESCRIPTOR_SET)
     .build()
     .unwrap();

     tonic::transport::Server::builder()
         .add_service(ApiServer::new(MyApi::default() ))
        .add_service(reflection)
         .serve(addr)
         .await?;
     Ok(())
 }
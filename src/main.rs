use crate::service::{MyApi};
use api::v1::{api_server::{ApiServer}};
use std::env;
use serde::{Serialize, Deserialize};
use jsonwebtoken::{encode, decode, Header, Algorithm, Validation, EncodingKey, DecodingKey};
use tonic::{Request, Response, Status};


pub mod api {
    tonic_include_protos::include_protos!("v1");
    pub(crate) const FILE_DESCRIPTOR_SET: &[u8] =
    tonic::include_file_descriptor_set!("api_pb");
}


mod service;

 #[tokio::main]
 async fn main() -> anyhow::Result<()> {
     let addr = ([0, 0, 0, 0], 3000).into();

     let reflection = tonic_reflection::server::Builder::configure()
     .register_encoded_file_descriptor_set(api::FILE_DESCRIPTOR_SET)
     .build().unwrap();

   //  const JWT_TOKEN:EncodingKey=EncodingKey::from_secret("test".as_ref());

    
   let api_server = ApiServer::new(MyApi{
       jwt_key:EncodingKey::from_secret(env::var("JWT_SECRET").expect("JWT_SECRET").as_ref()),
       google_client_id:env::var("GOOGLE_CLIENT_ID").expect("GOOGLE_CLIENT_ID"),
       google_client_secret:env::var("GOOGLE_CLIENT_SECRET").expect("GOOGLE_CLIENT_SECRET"),
       facebook_client_id:env::var("FACEBOOK_CLIENT_ID").expect("FACEBOOK_CLIENT_ID"),
       facebook_client_secret:env::var("FACEBOOK_CLIENT_ID").expect("FACEBOOK_CLIENT_ID"),    
    });


   //  let api_server = tonic_web::enable(api_server);

   tonic::transport::Server::builder()
         .add_service(api_server )
        .add_service(reflection)
         .serve(addr)
         .await?;
     Ok(())
 }
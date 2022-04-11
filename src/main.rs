use crate::service::{MyApi};
use api::v1::{api_server::{ApiServer}};
use std::env;
use serde::{Serialize, Deserialize};
use jsonwebtoken::{encode, decode, Header, Algorithm, Validation, EncodingKey, DecodingKey};
use tonic::{Request, Response, Status};
use aws_config::meta::region::RegionProviderChain;
use aws_sdk_dynamodb::{Client, Error};
use base85;
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

   let region_provider = RegionProviderChain::default_provider().or_else("us-east-1");
   let config = aws_config::from_env().region(region_provider).load().await;
   let client = Client::new(&config);

    
   let api_server = ApiServer::new(MyApi{
       jwt_key:EncodingKey::from_secret(env::var("JWT_SECRET").expect("JWT_SECRET").as_ref()),
       google_client_id:env::var("GOOGLE_CLIENT_ID").expect("GOOGLE_CLIENT_ID"),
       google_client_secret:env::var("GOOGLE_CLIENT_SECRET").expect("GOOGLE_CLIENT_SECRET"),
       facebook_client_id:env::var("FACEBOOK_CLIENT_ID").expect("FACEBOOK_CLIENT_ID"),
       facebook_client_secret:env::var("FACEBOOK_CLIENT_SECRET").expect("FACEBOOK_CLIENT_SECRET"),    
       dynamodb_client: client,
       hash_salt:env::var("HASH_SALT").expect("HASH_SALT"),
    });


   //  let api_server = tonic_web::enable(api_server);

   tonic::transport::Server::builder()
         .add_service(api_server )
        .add_service(reflection)
         .serve(addr)
         .await?;
     Ok(())
 }
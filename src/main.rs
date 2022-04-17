use crate::service::{MyApi};
use api::v1::{api_server::{ApiServer}};
use std::env;
use serde::{Serialize, Deserialize};
use jsonwebtoken::{encode, decode, Header, Algorithm, Validation, EncodingKey, DecodingKey};
use tonic::{Request, Response, Status};
use aws_config::meta::region::RegionProviderChain;
use aws_sdk_dynamodb::{Client as DynamoClient, Error, Config as DynamoConfig,};
use base85;
use moka::future::Cache;
pub mod api {
    tonic_include_protos::include_protos!("v1");
    pub(crate) const FILE_DESCRIPTOR_SET: &[u8] =
    tonic::include_file_descriptor_set!("api_pb");
}

use aws_sdk_s3::{Client as S3Client, Config as S3Config, Region, PKG_VERSION};


use bb8_redis::{
    bb8,
    redis::{cmd, AsyncCommands},
    RedisConnectionManager
};

use mongodb::{Client as MongoClient, options::{ClientOptions, DriverInfo, Credential, ServerAddress}};


mod service;
mod cache_init;

 #[tokio::main]
 async fn main() -> anyhow::Result<()> {

    let mongo_client=MongoClient::with_options(
        ClientOptions::builder()
        .app_name(String::from("Server"))
        .credential(Credential::builder()
        .username(env::var("MONGODB_ROOT_USER").expect("MONGODB_ROOT_USER"))
        .password(env::var("MONGODB_ROOT_PASSWORD").expect("MONGODB_ROOT_PASSWORD"))
        .build())
        .hosts(vec![ServerAddress::Tcp 
            { host: String::from("db"),
                 port: Some(27017)
                }])
            
        .default_database(String::from("DB"))
        
        
        .build()).unwrap();

    

     let addr = ([0, 0, 0, 0], 3000).into();

     let reflection = tonic_reflection::server::Builder::configure()
     .register_encoded_file_descriptor_set(api::FILE_DESCRIPTOR_SET)
     .build().unwrap();

     let manager = RedisConnectionManager::new("redis://cache:6379").unwrap();
     let keydb_pool = bb8::Pool::builder().build(manager).await.unwrap();
 

     cache_init::cache_init(keydb_pool.clone(), &mongo_client.clone()).await;

   //  const JWT_TOKEN:EncodingKey=EncodingKey::from_secret("test".as_ref());

   let region_provider = RegionProviderChain::default_provider().or_else("us-east-1");
   let config = aws_config::from_env().region(region_provider).load().await;

   let s3_client: S3Client= S3Client::new(&config);
   let dynamo_client:DynamoClient = DynamoClient::new(&config);

   let jwt_secret=env::var("JWT_SECRET").expect("JWT_SECRET");
let algo=Validation::new(Algorithm::HS256);
   let api_server = ApiServer::new(MyApi{
       jwt_encoding_key:EncodingKey::from_secret(jwt_secret.as_ref()),
       jwt_decoding_key:DecodingKey::from_secret(jwt_secret.as_ref()),
       jwt_algo:algo,
    //   google_client_id:env::var("GOOGLE_CLIENT_ID").expect("GOOGLE_CLIENT_ID"),
       google_client_secret:env::var("GOOGLE_CLIENT_SECRET").expect("GOOGLE_CLIENT_SECRET"),
       facebook_client_id:env::var("FACEBOOK_CLIENT_ID").expect("FACEBOOK_CLIENT_ID"),
       facebook_client_secret:env::var("FACEBOOK_CLIENT_SECRET").expect("FACEBOOK_CLIENT_SECRET"), 
       path_salt:   env::var("PATH_SALT").expect("PATH_SALT"), 
       dynamodb_client: dynamo_client,
       s3_client:s3_client,
     //  userid_salt:env::var("USERID_SALT").expect("USERID_SALT"),
  //     cache:Cache::new(10_000),
       keydb_pool:keydb_pool,
       mongo_client:mongo_client,
    });


   //  let api_server = tonic_web::enable(api_server);

   tonic::transport::Server::builder()
         .add_service(api_server )
        .add_service(reflection)
         .serve(addr)
         .await?;
     Ok(())
 }
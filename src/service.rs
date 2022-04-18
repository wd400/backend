#![allow(unused)]
use base64ct::Base64UrlUnpadded;
use bson::oid::ObjectId;
use futures::StreamExt;
use redis::{RedisConnectionInfo, RedisError};
use serde::{Serialize, Deserialize};
use jsonwebtoken::{encode, decode, Header, Algorithm, Validation, EncodingKey, DecodingKey, decode_header, TokenData, crypto::verify};
use tonic::{Request, Response, Status};
use crate::{api::{*, feed::FeedType, common_types::{Genre, Votes, FileUploadResponse}, conversation::{NewConvRequestResponse,ConvHeader,ConvHeaderList, ConversationComponent, conversation_component, ConvData}}, cache_init::{ConversationRank, get_epoch, TIMEFEEDTYPES, feedType2seconds}};
use reqwest;
use moka::future::Cache;
use std::{collections::{HashMap, hash_map::RandomState, BTreeMap}, borrow::{BorrowMut, Borrow}, time::{SystemTime, Duration}, hash::Hash, str::FromStr};
use sha2::{Sha256, Sha512, Digest};
use aws_sdk_s3::{presigning::config::PresigningConfig, types::{ByteStream, DateTime}};
use bb8_redis::{
    bb8::{self, Pool, PooledConnection},
    redis::{cmd, AsyncCommands},
    RedisConnectionManager
};
use celes::Country;
use crate::service::login::LoginRequest;
use std::collections::HashSet;
use iso639_enum::{Language, IsoCompat};
use rand::{distributions::Alphanumeric, Rng}; // 0.8
use base64ct::{Base64, Encoding};
const BUF_SIZE: usize = 128;
use uuid::Uuid;
use std::io::Cursor;
use image::{io::Reader as ImageReader, ImageFormat};

use mongodb::{Client as MongoClient, options::{ClientOptions, DriverInfo, Credential, ServerAddress, FindOptions, FindOneOptions}, bson::{doc, Document}};

//extern crate rusoto_core;
//extern crate rusoto_dynamodb;

use aws_sdk_dynamodb::{Client as DynamoClient, Error, model::{AttributeValue, ReturnValue}, types::{SdkError, self}, error::{ConditionalCheckFailedException, PutItemError, conditional_check_failed_exception, PutItemErrorKind}};
use aws_sdk_s3::{Client as S3Client, Region, PKG_VERSION};

use std::time::{UNIX_EPOCH};




#[derive(Debug, Serialize, Deserialize)]
struct JWTPayload {
    exp: u64,
    pseudo: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct JWTSetPseudo {
    exp: i64,
    userid: String,
}

//#[derive(Default)]
pub struct  MyApi {
    pub jwt_encoding_key:EncodingKey,
    pub jwt_decoding_key:DecodingKey,
    pub jwt_algo:Validation,
 //   pub google_client_id: String,
    pub google_client_secret: String,
    pub facebook_client_id: String,
    pub facebook_client_secret: String,
    pub dynamodb_client: DynamoClient,
    pub s3_client: S3Client,
//    pub userid_salt:String,
    pub keydb_pool:Pool<RedisConnectionManager>,
    pub mongo_client:MongoClient,
    pub path_salt:String
 //   pub cache:Cache<String, String>,

}

// todo parse, to_string, verify
struct SecurePath {
    pub nonce: String,
    pub secret: String
}

#[derive(Deserialize)]
struct FacebookToken {
    access_token: String,
    token_type: String,
    expires_in: i64
}

const GOOGLE_WEB_CLIENT_ID:&str="470515755626-27pkbok135g1d8v9533ukd98smqneilg.apps.googleusercontent.com";

const GRANT_TYPE : &str="authorization_code";

const GOOGLE_WEB_REDIRECT_URL:&str="https://example.com";

const SCREENSHOTS_BUCKET:&str="screenshots-s3-bucket";

#[derive(Deserialize,Debug,Default)]
struct GoogleTokensJSON {
 //   id_token: String,
    expires_in: u32,
    access_token:String,
  //  id_token: String,
    scope: String,
    token_type: String,
    //this field is only present in this response if you set the access_type parameter to offline in the initial request to Google's authorization server. 
    refresh_token: String

}


fn Map2ConvHeader(map:&BTreeMap<String, String>)->ConvHeader {
// /!\ pb si anonyme
    ConvHeader{
        convid:  map.get("convid").unwrap().to_string(),
        title: map.get("title").unwrap().to_string(),
    //     userid: map.get("userid").unwrap().to_string(),
        pseudo: map.get("pseudo").unwrap().to_string() ,
        upvote: map.get("upvote").unwrap().parse::<i32>().unwrap(),
            downvote: map.get("downvote").unwrap().parse::<i32>().unwrap(),
       
        description: map.get("description").unwrap().to_string(),
        categories: map.get("categories").unwrap().to_string(),
        created_at:  map.get("created_at").unwrap().parse::<u32>().unwrap(),
        language:map.get("language").unwrap().to_string(),
    }
}



#[derive(Debug, Serialize, Deserialize)]
struct MongoConv {
    #[serde(rename="_id", skip_serializing_if="Option::is_none")]
    id: Option<ObjectId>,
  //  _id: ObjectId,
    title: String,
    description:String,
    categories:String,
    pseudo: String,
    score: i32, //upvote-downvote
    votes: Votes,
 //   upvote: i32,
 //   downvote: i32,
    created_at:u64,
    language:String,
    private: bool,
    anonym:bool,
    conv:Vec<ConversationComponent>
}

#[derive(Debug, Serialize, Deserialize)]
struct MongoUser{
    pseudo: String
}


pub fn genre2str(genre: common_types::Genre)->Option< &'static str>{
    match genre {
        common_types::Genre::F=>Some("F"),
        common_types::Genre::M=>Some("M"),
        common_types::Genre::Other=>Some("O"),
        _ =>None
    }
}



async fn get_conv_header(convid:&str,keydb_pool:Pool<RedisConnectionManager>,mongo_client:&MongoClient)->ConvHeader{

    let mut keydb_conn = keydb_pool.get().await.expect("keydb_pool failed");

    println!("CONVID {:#?}",convid);
    let cached:BTreeMap<String, String>=   cmd("hgetall")
                    .arg(convid).query_async(&mut *keydb_conn).await.expect("hgetall failed");
        println!("cache: {:#?}",cached);
     
                //cache miss
                if cached.is_empty() {


                    let header_filter: mongodb::bson::Document = doc! {
                        "_id": i32::from(1),
                        "title": i32::from(1),
                        "description":i32::from(1),
                        "categories":i32::from(1),
                        "pseudo":i32::from(1),
                //        "userid":i32::from(1),
                 //       "userid":i32::from(1),
                        "upvote": i32::from(1),
                        "downvote": i32::from(1),
                        "created_at":  i32::from(1),
                        "language": i32::from(1),
                  //      "votes":i32::from(1),

                    };

                    let options = FindOneOptions::builder().projection(header_filter).build();
                

                    let conversations = mongo_client.database("DB")
                    .collection::<ConvHeader>("convs");
                    //::<ConvHeader>
let pipeline = vec![
     doc! { "$match": { "_id" :  bson::oid::ObjectId::parse_str(convid).unwrap() }} ,
    doc!{ "$limit": i32::from(1) },

   doc! { "$project": {
       "convid": { "$toString": "$_id"},
        "title":i32::from(1),
        "pseudo":{"$cond": ["$anonym", "", "$pseudo"]},
        "upvote":i32::from(1),
        "downvote":i32::from(1),
        "categories":i32::from(1),
        "created_at":i32::from(1),
        "description":i32::from(1),
        "language":i32::from(1),
}
}
                ];


let mut results = conversations.aggregate(pipeline, None).await.unwrap();

// Loop through the results and print a summary and the comments:

if let Some(result) = results.next().await {

   let conv_header: ConvHeader = bson::from_document(result.unwrap()).unwrap();

   println!("{:#?}", conv_header);






   let _:()=   cmd("hmset")
   .arg(&vec![
           convid,
           "convid",convid,
           "title",&conv_header.title,
       //    "userid",&conv_header.userid,
           "pseudo",&conv_header.pseudo,
           "upvote",&conv_header.upvote.to_string(),
           "downvote",&conv_header.upvote.to_string(),
           "categories",&conv_header.categories,
           "created_at",&conv_header.created_at.to_string(),
           "description",&conv_header.description,
           "language",&conv_header.language,
           ] ).query_async(&mut *keydb_conn).await.unwrap();


   let _:()=   cmd("expire")
   .arg(&[convid,"60"]).query_async(&mut *keydb_conn).await.unwrap();


   return   conv_header ;



} else {
    //not found
    return ConvHeader::default();
}
 }
                //cache hit
                else {
                
                let _:()=   cmd("expire")
                .arg(&[convid,"2"]).query_async(&mut *keydb_conn).await.unwrap();

                return Map2ConvHeader(&cached);
                }
}

pub fn feedType2cacheTable(feed_type: feed::FeedType)->Option<&'static str>{
    match feed_type {
        feed::FeedType::AllTime =>Some("AllTime"),
        feed::FeedType::Emergency=>Some("Emergency"),
        feed::FeedType::LastActivity=>Some("LastActivity"),
        feed::FeedType::LastDay=>Some("LastDay"),
        feed::FeedType::LastMonth=>Some("LastMonth"),
        feed::FeedType::LastWeek=>Some("LastWeek"),
        feed::FeedType::LastYear=>Some("LastYear"),
        _ =>None
    }
}

pub fn generate_path_secret(userid:&str,path_salt:&str,nonce:&str)->String{

let mut hasher = Sha256::new();
hasher.update(userid);
hasher.update(path_salt);
hasher.update(nonce);

let hash = hasher.finalize();

assert!(Base64::encoded_len(&hash) <= BUF_SIZE);

let mut enc_buf = [0u8; BUF_SIZE];

let encoded = Base64UrlUnpadded::encode(&hash, &mut enc_buf).unwrap();



return String::from(encoded)

}

#[tonic::async_trait]
impl v1::api_server::Api for MyApi {

    async fn login(&self, request: Request<login::LoginRequest>) -> Result<Response<login::LoginResponse>, Status> {
     //   println!("Got a request: {:#?}", &request);
        let request:&LoginRequest=request.get_ref();
        println!("{:#?}",request);


        let third_party = request.third_party();
        let client_type=request.client_type();
        
        let userid:String =
         if third_party==login::ThirdParty::Facebook {
        //    return Err(Status::new(tonic::Code::InvalidArgument, "not yet implemented"));

                    // https://developers.google.com/identity/protocols/oauth2/openid-connect#exchangecode
                    let client = reqwest::Client::new();

                    let facebook_request = client.get("https://graph.facebook.com/oauth/access_token")
                    .query(
                        &[
                            //safe? optimal?
                            ("redirect_uri","https://example.com/".into()),
                            ("code", "foobar".into()),
                            ("client_id",self.facebook_client_id.clone()),
                            ("client_secret",self.facebook_client_secret.clone()),

                        ]).send().await;

                    let facebook_request = match facebook_request {
                        Ok(facebook_request) => facebook_request,
                        Err(_) => return Err(Status::new(tonic::Code::InvalidArgument, "oauth request error"))
                    };
        
                    let facebook_response = match facebook_request.json::<FacebookToken>().await {
                        Ok(facebook_response) => facebook_response,
                        Err(_) => return Err(Status::new(tonic::Code::InvalidArgument, "oauth json error"))
                    };

                    let facebook_request = client.get("https://graph.facebook.com/me")
                    .query(
                        &[
                            //safe? optimal?
                            ("fields","id".into()),
                            ("access_token", facebook_response.access_token)

                        ]).send().await;


                        let facebook_request = match facebook_request {
                            Ok(facebook_request) => facebook_request,
                            Err(_) => return Err(Status::new(tonic::Code::InvalidArgument, "oauth request error"))
                        };

                        let facebook_response = match facebook_request.json::<HashMap<String, String>>().await {
                            Ok(facebook_response) => facebook_response,
                            Err(_) => return Err(Status::new(tonic::Code::InvalidArgument, "oauth json error"))
                        };

                        match facebook_response.get("id").cloned() {
                            Some(sub) => sub,
                            None => return Err(Status::new(tonic::Code::InvalidArgument, "id json error"))
                        }
    



            }
        else if third_party==login::ThirdParty::Google {

            let client = reqwest::Client::new();
            // https://developers.google.com/identity/protocols/oauth2/openid-connect#exchangecode
            
            let access_token = if client_type==login::ClientType::Android || client_type == login::ClientType::Ios {
                match request.auth.as_ref().unwrap() {
    login::login_request::Auth::AccessToken(token) => token.to_string(),
    _ => {
        return Err(Status::new(tonic::Code::InvalidArgument, "invalid device auth"))
    },
}
            } else {      
            let pkce_payload= match  request.auth.as_ref().unwrap() {

                login::login_request::Auth::PkcePayload(payload)=>{
                    payload
                },
                _=>{
                    return Err(Status::new(tonic::Code::InvalidArgument, "invalid device auth"))   
                }

            };
            let google_tokens = client.post("https://oauth2.googleapis.com/token")
            .form(
                &[
                    //safe? optimal?
                    ("code", &pkce_payload.code  ),
                    ("client_id", &GOOGLE_WEB_CLIENT_ID.to_string()),
                    ("client_secret",&self.google_client_secret),
                  
                    ("redirect_uri",&GOOGLE_WEB_REDIRECT_URL.to_string() ),
                
                    ("grant_type",&GRANT_TYPE.to_string()),
                    //TODO iif APP?
                    ("code_verifier",&pkce_payload.code_verifier)
                ]
            );
            match google_tokens.send().await {
                Ok(google_tokens) => {
                    
                    println!("FIRST {:#?}",google_tokens);
                    let json=google_tokens.json::<GoogleTokensJSON>().await;
                    println!("SECOND {:#?}",json);

        match json {
            Ok(google_tokens) => {

               // let google_tokens:GoogleTokensJSON=google_tokens;
               String::from("G:")+&google_tokens.access_token

             },
            Err(_) => return Err(Status::new(tonic::Code::InvalidArgument, "oauth json error"))
        } },
    Err(_) => return Err(Status::new(tonic::Code::InvalidArgument, "oauth request error"))
                    }

            /*
        let google_tokens:GoogleTokensJSON=GoogleTokensJSON{ 
            access_token: "a".to_string(),
             expires_in:1, 
             scope: "a".to_string(),
              token_type: "a".to_string(),
            refresh_token: "a".to_string() };
            */
                
            };

            let token_infos=client.get("https://oauth2.googleapis.com/tokeninfo?")
            .query(&[("access_token",access_token)])
            .send().await;
            match token_infos {
                Ok(token_infos) => {
                    match token_infos.json::<HashMap<String, String>>().await {
                        Ok(token_infos) => {
                            match token_infos.get("sub").cloned() {
                                Some(sub) => sub,
                                None => return Err(Status::new(tonic::Code::InvalidArgument, "oauth json error"))
                            }  },
                        Err(_) => return Err(Status::new(tonic::Code::InvalidArgument, "oauth json error"))
                    } },
                Err(_) => return Err(Status::new(tonic::Code::InvalidArgument, "tokeninfo request error"))
            }
        } else {
                return Err(Status::new(tonic::Code::InvalidArgument, "third party is invalid"))
        };
        
     //   println!("openid: {}",open_id);
        /*
        let mut hasher = Sha256::new();
        hasher.update(self.userid_salt.to_owned()+&open_id);
        let hash = hasher.finalize();
          let userid=base85::encode(&hash);
          */
        



      //check if user in mongo
      let header_filter: mongodb::bson::Document = doc! { "pseudo":i32::from(1) };
    let options = FindOneOptions::builder().projection(header_filter).build();

    let users = self.mongo_client.database("DB")
    .collection::<MongoUser>("users");
    

    let mut cursor = users.find_one(doc!{
        "userid":&userid
    },
        options
  //  FindOneOptions::default() 
        ).await;
    match cursor {
    Ok(value) => {

        match value {
            None=>{
                //new user

                let payload: JWTSetPseudo = JWTSetPseudo {
                    exp:(get_epoch()+60*60) as i64,
                     userid:userid };

                let token = encode(&Header::default(), &payload, &self.jwt_encoding_key).expect("INVALID TOKEN");
                    Ok(Response::new(login::LoginResponse{
                        access_token:token,
                        is_new: false
                    }))

            },
            
            Some(value)=>{
                //known user

                let payload: JWTPayload = JWTPayload {
                    exp:get_epoch()+60*60*24*60,
                    pseudo:value.pseudo };

                let token = encode(&Header::default(), &payload, &self.jwt_encoding_key).expect("INVALID TOKEN");
                    Ok(Response::new(login::LoginResponse{
                        access_token:token,
                        is_new: false
                    }))



            }
        }
    },
    Err(_) => { return Err(Status::new(tonic::Code::InvalidArgument, "db error"))},
}
/*
        let token = encode(&Header::default(), &payload, &self.jwt_encoding_key)
        .expect("INVALID TOKEN");

        //userid redondant
        let response = login::LoginResponse{
            access_token:token,
            is_new: is_new
        };

        Ok(Response::new(response))
        */
    }

    async fn refresh_token(&self,request:Request<common_types::AuthenticatedRequest>) ->  Result<Response<common_types::RefreshToken>, Status> {
        
        let request=request.get_ref();
        let data=decode::<JWTPayload>(&request.access_token,&self.jwt_decoding_key,&self.jwt_algo);
        let data=match data {
            Ok(data)=>data,
            _=>{ return Err(Status::new(tonic::Code::InvalidArgument, "invalid token"))}
        };

        
        let payload: JWTPayload = JWTPayload {
            exp:(get_epoch()+60*60*24*60) as u64,
            pseudo:data.claims.pseudo,
       //     open_id:data.claims.open_id
            
        };

        let token = encode(&Header::default(), &payload, &self.jwt_encoding_key)
        .expect("INVALID TOKEN");

        //userid redondant
        let response = common_types::RefreshToken{
            access_token:token
        };

        Ok(Response::new(response))

    }

    async fn set_account(&self,request:Request<user::SetAccountRequest> ) ->  Result<Response<common_types::RefreshToken> ,Status > {
          
        let request=request.get_ref();
        let data=decode::<JWTSetPseudo>(&request.access_token,&self.jwt_decoding_key,&self.jwt_algo);
        let data=match data {
            Ok(data)=>data,
            _=>{ return Err(Status::new(tonic::Code::InvalidArgument, "invalid token"))}
        };
        

        let conversations = self.mongo_client.database("DB")
        .collection("users");


        if  request.pseudo.len() < 4 || request.pseudo.len() > 13 {
            return Err(Status::new(tonic::Code::InvalidArgument, "invalid pseudo size"))
        }

        if ! request.pseudo.chars().all(char::is_alphanumeric) {
            return Err(Status::new(tonic::Code::InvalidArgument, "invalid pseudo char"))
        }

      //  check valid  genre
       if Country::from_alpha2(&request.country).is_err(){
        return Err(Status::new(tonic::Code::InvalidArgument, "invalid country"))
       }

       if request.date_of_birth<=0 {
        return Err(Status::new(tonic::Code::InvalidArgument, "invalid date_of_birth"))
       }

       
       
       
        //check valid userid,
       if let Err(_) = conversations.insert_one(
           doc!{
                "pseudo": &request.pseudo,
                "userid": &data.claims.userid,
                "genre": 
                match genre2str(request.genre()) {
                    Some(value)=>value,
                    None=> {
                        return Err(Status::new(tonic::Code::InvalidArgument, "invalid genre"))
                    }
                }                
                
                ,
                "date_of_birth":&request.date_of_birth,
                "country":&request.country.to_lowercase() }
            ,None).await {
               return Err(Status::new(tonic::Code::InvalidArgument,"db error"))
}

        //ConditionalCheckFailedException
               let res = self.dynamodb_client.put_item()
               .table_name("users")
               .item("pseudo",AttributeValue::S(request.pseudo.to_string()))
               .item("amount",AttributeValue::N(String::from("0")))
          //     .item("openid",AttributeValue::S(open_id.to_string()))
               .condition_expression("attribute_not_exists(amount)")
               .return_values(ReturnValue::AllOld).send().await;
       
             let is_new=match res {
               Err(SdkError::ServiceError {
                   err:
                       PutItemError {
                           kind: PutItemErrorKind::ConditionalCheckFailedException(_),
                           ..
                       },
                   raw: _,
               }) => {
                   false
               },
               Ok(_)=>{
                   true
               },
               _ => {return Err(Status::new(tonic::Code::InvalidArgument, "db error"))}
             };
    
    
             let payload: JWTPayload = JWTPayload {
                exp:get_epoch()+60*60*24*60,
                pseudo:request.pseudo.clone() };

            let token = encode(&Header::default(), &payload, &self.jwt_encoding_key).expect("INVALID TOKEN");
                Ok(Response::new(common_types::RefreshToken{
                    access_token:token,
                }))

            }

    async fn feed(&self,request:Request<feed::FeedRequest> ) -> Result<Response<conversation::ConvHeaderList>,Status> {
        let request=request.get_ref();



        let cache_table_name= match feedType2cacheTable(request.feed_type()){
    Some(cache_table_name)=>cache_table_name,
    _=>return Err(Status::new(tonic::Code::InvalidArgument, "invalid feed"))
        };
        
        let offset=request.offset;
        if offset<0 {
          return   Err(Status::new(tonic::Code::InvalidArgument, "invalid offset"))
        }
        


        let pool = self.keydb_pool.clone();
        let mut conn = pool.get().await;
        let mut conn = match conn {
            Ok(conn)=>conn,
            Err(_)=>return   Err(Status::new(tonic::Code::InvalidArgument, "cache connection error"))
        
        };
        //<Vec<(String, isize)> , "withscores"
        let reply:Result<Vec<String>, RedisError>= cmd("zrevrange")
        .arg(&[cache_table_name,
            &offset.to_string(),
            &(offset+20).to_string()]).query_async(&mut *conn).await;

       
        let reply = match reply {
            Ok(reply)=>reply,
            Err(_)=>return   Err(Status::new(tonic::Code::InvalidArgument, "cache error"))
        
        };
        println!("{:#?}",reply);
        let mut replylist:Vec<ConvHeader>=vec![];
        for convid in reply {
            replylist.push(get_conv_header(&convid,self.keydb_pool.clone(),&self.mongo_client).await);
        }

        //https://crates.io/crates/moka

        
        return  Ok(Response::new(ConvHeaderList{ convheaders: replylist }))
    }

    async fn new_conv(&self,request:Request<conversation::NewConvRequest> ) ->  Result<Response<conversation::NewConvRequestResponse> ,Status > {

        let request=request.get_ref();
        
        let data=decode::<JWTPayload>(&request.access_token,&self.jwt_decoding_key,&self.jwt_algo);
        let data=match data {
            Ok(data)=>data,
            _=>{ return Err(Status::new(tonic::Code::InvalidArgument, "invalid token"))}
        };
        
        let conv_data = match &request.conv_data {
            Some(value)=>value,
            None=>{
            return Err(Status::new(tonic::Code::InvalidArgument, "invalid token"))
            }
        };

      if  conv_data.title.len() > 100 {
        return Err(Status::new(tonic::Code::InvalidArgument, "title too large"))
      }
      if  conv_data.description.len() > 400 {
        return Err(Status::new(tonic::Code::InvalidArgument, "description too large"))
      }

      if Language::from_iso639_3(&conv_data.language).is_err() {
         
        return Err(Status::new(tonic::Code::InvalidArgument, "invalid language"))
      }

      if conv_data.components.len() > 100 {
        return Err(Status::new(tonic::Code::InvalidArgument, "conv too long"))
      }


     let mut new_screens_uri :Vec<&String>=vec![];
     let mut box_ids:HashSet<i32> = HashSet::new();


     for component in &conv_data.components { 
        match &component.component {

            Some(value)=>{

                match value {
    conversation_component::Component::Screen(screen) => {
        //check if owned
     let path_secret = generate_path_secret(&data.claims.pseudo,&self.path_salt,&screen.uri[..7]);
     if screen.uri[7..] != path_secret {
        return Err(Status::new(tonic::Code::InvalidArgument, "screen not owned"))
     }
     new_screens_uri.push(&screen.uri);
    },
    conversation_component::Component::Info(info) => {
        //check length
        if info.text.len() > 300 {
            return Err(Status::new(tonic::Code::InvalidArgument, "info too long"))
          }
    },
    conversation_component::Component::ReplyBox(replybox) => {

        // /!\box can exists without being linked to the conv
        if ! box_ids.insert(replybox.boxid) {
            return Err(Status::new(tonic::Code::InvalidArgument, "replybox duplicated"))
        }
    },
}
            },
    None => { return Err(Status::new(tonic::Code::InvalidArgument,"invalid component")) }, 
}
        println!("{:#?}",component.component);

      }
      //move screens from tmp/ to pictues/
      for screen_uri in new_screens_uri {

        let mut source_bucket_and_object: String = "".to_owned();
        source_bucket_and_object.push_str(SCREENSHOTS_BUCKET);
        source_bucket_and_object.push_str("/tmp/");
        source_bucket_and_object.push_str(screen_uri);

   self.s3_client.copy_object()
      .bucket(SCREENSHOTS_BUCKET)
      .copy_source(source_bucket_and_object)
        .key("pictures/".to_string()+screen_uri+".jpg")
          .send()
          .await ;
        
    self.s3_client.delete_object()
    .bucket(SCREENSHOTS_BUCKET)
    .key("tmp/".to_string()+screen_uri).send().await;

      }

       

        //mongo: add new conv
        let conversations = self.mongo_client.database("DB")
        .collection::<MongoConv>("convs");
       
        let conv_data=request.conv_data.as_ref().unwrap();

       let convid= conversations.insert_one(
            
            MongoConv{
                id:None,
                pseudo: data.claims.pseudo,
                 title: conv_data.title.clone(),
                 description: conv_data.description.clone(), 
                 categories: conv_data.categories.clone(), 

            //     author: todo!(), 
                 score: 0, 
                 votes: Votes::default(),
       //          upvote: 0, 
       //          downvote: 0,
                  created_at: get_epoch(), 
                  language: conv_data.language.clone(), 
                  private: conv_data.private, 
                  anonym: conv_data.anonym, 
                  
                  
                  conv: conv_data.components.clone()  }

            ,None).await.unwrap().inserted_id.to_string();
            
        
        //if not private: keydb: add to all feeds(including new activities)
if !conv_data.private {
        const NEW_FEEDTYPES_UPLOAD: [feed::FeedType; 2]  = [
    feed::FeedType::LastActivity,

  feed::FeedType::New,
];


let current_timestamp = get_epoch();

let mut keydb_conn = self.keydb_pool.get().await.expect("keydb_pool failed");


for feed_type in TIMEFEEDTYPES {
    let expiration = current_timestamp+ feedType2seconds(feed_type);
    let cache_table=feedType2cacheTable(feed_type).unwrap();
    let _:()=   cmd("zadd")
        .arg(&[cache_table,
            "0",
 &convid  ]).query_async(&mut *keydb_conn).await.expect("zadd error");


   let _:()= cmd("expirememberat")
    .arg(&[cache_table,
        &convid,
&expiration.to_string()]).query_async(&mut *keydb_conn).await.expect("expirememberat error");
//      println!("{:#?}",res);
  }

//ALLTIME
  let _:()=   cmd("zadd")
  .arg(&[feedType2cacheTable(feed::FeedType::AllTime).unwrap(),
    "0",
&convid  ]).query_async(&mut *keydb_conn).await.expect("zadd error");

let timestamp_str=current_timestamp.to_string();

//NEW
  let _:()=   cmd("zadd")
  .arg(&[
      feedType2cacheTable(feed::FeedType::New).unwrap(),
  &timestamp_str,
&convid  ]).query_async(&mut *keydb_conn).await.expect("zadd error");

//LASTACTIVITY
let _:()=   cmd("zadd")
.arg(&[
    feedType2cacheTable(feed::FeedType::LastActivity).unwrap(),
&timestamp_str,
&convid  ]).query_async(&mut *keydb_conn).await.expect("zadd error");
}

        return  Ok(Response::new(NewConvRequestResponse { convid: "a".to_string() }))
      
    }

   async fn upload_file(&self,request:Request<common_types::FileUploadRequest> ,) ->  Result<tonic::Response<common_types::FileUploadResponse> ,tonic::Status, >  {

    
 //   const file: Vec<u8>=request.get_mut().file;
 let request=request.get_ref();

 let mut file:&Vec<u8>= request.file.borrow();

    if file.len()>10*1_024*1_024 {
        return Err(Status::new(tonic::Code::InvalidArgument, "file too large"))
    }
    
    match ImageReader::new(Cursor::new(file)).with_guessed_format(){
        Ok(value)=>{
           match value.format() {
            Some(value)=>{
                match value {
                    ImageFormat::Jpeg => {},
                    _=> {return Err(Status::new(tonic::Code::InvalidArgument, "invalid image format")) }
                }
               },
               _=> {return Err(Status::new(tonic::Code::InvalidArgument, "invalid image")) }
           }
        },
        Err(_)=>{return Err(Status::new(tonic::Code::InvalidArgument, "invalid file")) },
    }
    

    let data=decode::<JWTPayload>(&request.access_token,&self.jwt_decoding_key,&self.jwt_algo);
    let data=match data {
        Ok(data)=>data,
        _=>{ return Err(Status::new(tonic::Code::InvalidArgument, "invalid token"))}
        
    };
    


//nonce:base64(hash(pseudo,secret,nonce))

let nonce:String=rand::thread_rng()
.sample_iter(&Alphanumeric)
.take(7)
.map(char::from).collect();

let encoded=generate_path_secret(&data.claims.pseudo,&self.path_salt,&nonce);

let filename=nonce+&encoded;
let path=String::from("tmp/")+&filename;
    match self.s3_client
        .put_object()
        .bucket(SCREENSHOTS_BUCKET)
        .body(ByteStream::from(file.to_owned()))
        .key(path)
        
       // .expires(DateTime::from(SystemTime::now() + Duration::from_secs(60*5)))
        .send().await {
    Ok(value) => {
        println!("{:#?}",value);
        return  Ok(Response::new(FileUploadResponse { file_id: filename }))
    },
    Err(_) => return Err(Status::new(tonic::Code::InvalidArgument, "upload error")),
}

}

    async fn search(&self,request:Request<search::SearchRequest> ) ->  Result<Response<search::SearchResponse> ,Status > {


        let request=request.get_ref();

        if &request.access_token != ""{

            let data=decode::<JWTPayload>(&request.access_token,&self.jwt_decoding_key,&self.jwt_algo);
            
            let data=match data {
                Ok(data)=>data,
                _=>{ return Err(Status::new(tonic::Code::InvalidArgument, "invalid token"))}
            };
        }


        todo!()
    }

    fn modify_conv< 'life0, 'async_trait>(& 'life0 self,request:tonic::Request<conversation::Conversation> ,) ->  core::pin::Pin<Box<dyn core::future::Future<Output = Result<tonic::Response<common_types::Empty> ,tonic::Status> > + core::marker::Send+ 'async_trait> >where 'life0: 'async_trait,Self: 'async_trait {
       //lock entre les deux
        //retrieve old conv screens&answerbox-> diff -> delete or import screens/ delete convs boxes
/*
        copy new
        replace by new returning old
        diff new/olds
        */

        //mongodb findone id+userid pour verif si owner
        todo!()
    }

    fn get_conv< 'life0, 'async_trait>(& 'life0 self,request:tonic::Request<common_types::AuthenticatedObjectRequest, > ,) ->  core::pin::Pin<Box<dyn core::future::Future<Output = Result<tonic::Response<visibility::Visibility> ,tonic::Status, > > + core::marker::Send+ 'async_trait> >where 'life0: 'async_trait,Self: 'async_trait {
        todo!()
    }

    async fn delete_conv(&self,request:Request<common_types::AuthenticatedObjectRequest> ) ->  Result<Response<common_types::Empty> ,Status > {
        todo!()
    }

    fn submit_reply< 'life0, 'async_trait>(& 'life0 self,request:tonic::Request<replies::ReplyRequest, > ,) ->  core::pin::Pin<Box<dyn core::future::Future<Output = Result<tonic::Response<common_types::Empty> ,tonic::Status> > + core::marker::Send+ 'async_trait> >where 'life0: 'async_trait,Self: 'async_trait {
        //transaction:check if box exists and add reply

                /*
        reply:
        {"replyid":unique_id,
        "convid":123,
        "boxid":4,
        "replyfrom":replyid_or_root,
        "text":xxx,
        "writer":xxx,
        "anonym":boolean,
        "upvote":5,
        "downvote":2,
        "score":upvote-downvote,
        "created_at", xxx}
        */
         todo!()

     }
 
     async fn delete_reply(&self,request:Request<common_types::AuthenticatedObjectRequest> ) ->  Result<Response<common_types::Empty> ,Status > {
        todo!()
    }


    fn downvote_reply< 'life0, 'async_trait>(& 'life0 self,request:tonic::Request<common_types::AuthenticatedObjectRequest, > ,) ->  core::pin::Pin<Box<dyn core::future::Future<Output = Result<tonic::Response<common_types::Empty> ,tonic::Status> > + core::marker::Send+ 'async_trait> >where 'life0: 'async_trait,Self: 'async_trait {
        todo!()
    }


    fn upvote_reply< 'life0, 'async_trait>(& 'life0 self,request:tonic::Request<common_types::AuthenticatedObjectRequest, > ,) ->  core::pin::Pin<Box<dyn core::future::Future<Output = Result<tonic::Response<common_types::Empty> ,tonic::Status> > + core::marker::Send+ 'async_trait> >where 'life0: 'async_trait,Self: 'async_trait {
        todo!()
    }

    fn get_replies< 'life0, 'async_trait>(& 'life0 self,request:tonic::Request<replies::GetRepliesRequest> ,) ->  core::pin::Pin<Box<dyn core::future::Future<Output = Result<tonic::Response<replies::ReplyList> ,tonic::Status> > + core::marker::Send+ 'async_trait> >where 'life0: 'async_trait,Self: 'async_trait {
        todo!()
    }

    fn upvote_conv< 'life0, 'async_trait>(& 'life0 self,request:tonic::Request<common_types::AuthenticatedObjectRequest, > ,) ->  core::pin::Pin<Box<dyn core::future::Future<Output = Result<tonic::Response<common_types::Empty> ,tonic::Status> > + core::marker::Send+ 'async_trait> >where 'life0: 'async_trait,Self: 'async_trait {
        todo!()
    }

    fn downvote_conv< 'life0, 'async_trait>(& 'life0 self,request:tonic::Request<common_types::AuthenticatedObjectRequest, > ,) ->  core::pin::Pin<Box<dyn core::future::Future<Output = Result<tonic::Response<common_types::Empty> ,tonic::Status> > + core::marker::Send+ 'async_trait> >where 'life0: 'async_trait,Self: 'async_trait {
        todo!()
    }


    fn get_qa_space< 'life0, 'async_trait>(& 'life0 self,request:tonic::Request<common_types::AuthenticatedObjectRequest, > ,) ->  core::pin::Pin<Box<dyn core::future::Future<Output = Result<tonic::Response<qa::QaSpace> ,tonic::Status> > + core::marker::Send+ 'async_trait> >where 'life0: 'async_trait,Self: 'async_trait {
        todo!()
    }


    fn preview_qa_space< 'life0, 'async_trait>(& 'life0 self,request:tonic::Request<common_types::AuthenticatedObjectRequest, > ,) ->  core::pin::Pin<Box<dyn core::future::Future<Output = Result<tonic::Response<qa::QaSpace> ,tonic::Status> > + core::marker::Send+ 'async_trait> >where 'life0: 'async_trait,Self: 'async_trait {
        todo!()
    }


    fn edit_qa_space< 'life0, 'async_trait>(& 'life0 self,request:tonic::Request<qa::EditQaSpaceRequest> ,) ->  core::pin::Pin<Box<dyn core::future::Future<Output = Result<tonic::Response<common_types::Empty> ,tonic::Status> > + core::marker::Send+ 'async_trait> >where 'life0: 'async_trait,Self: 'async_trait {
        todo!()
    }


    async fn delete_account(& self,request:Request<common_types::AuthenticatedRequest> ,) -> Result<Response<common_types::Empty> ,tonic::Status >  {
     //   todo!()

     //todo:clean cache?

     let request=request.get_ref();
     let data=decode::<JWTPayload>(&request.access_token,&self.jwt_decoding_key,&self.jwt_algo);
     let data=match data {
         Ok(data)=>data,
         _=>{ return Err(Status::new(tonic::Code::InvalidArgument, "invalid token"))}
     };

     match self.dynamodb_client.delete_item()
     .table_name("users")
     .key("pseudo",AttributeValue::S(data.claims.pseudo.to_string())).send().await
     {
        Ok(_) => println!("Deleted item from table"),
        Err(e) => {
            return Err(Status::new(tonic::Code::InvalidArgument, "invalid token"))
        }
    };


     Ok(Response::new(common_types::Empty{}))

    }



    fn feedback< 'life0, 'async_trait>(& 'life0 self,request:tonic::Request<user::FeedbackRequest> ,) ->  core::pin::Pin<Box<dyn core::future::Future<Output = Result<tonic::Response<common_types::Empty> ,tonic::Status, > > + core::marker::Send+ 'async_trait> >where 'life0: 'async_trait,Self: 'async_trait {
        todo!()
    }


   async fn report(& self, request:Request<common_types::AuthenticatedRequest>,) ->  Result<Response<common_types::Empty>, tonic::Status>
    { todo!() }


   async fn get_balance(& self, request:Request<common_types::AuthenticatedRequest>,) ->  Result<Response<user::BalanceResponse>, tonic::Status>
    { todo!() }

    async fn buy_emergency(& self, request:Request<common_types::AuthenticatedRequest>,) ->  Result<Response<common_types::Empty>, tonic::Status>
    { todo!() }





    fn decline_invitation< 'life0, 'async_trait>(& 'life0 self,request:tonic::Request<user::ResourceRequest> ,) ->  core::pin::Pin<Box<dyn core::future::Future<Output = Result<tonic::Response<common_types::Empty> ,tonic::Status> > + core::marker::Send+ 'async_trait> >where 'life0: 'async_trait,Self: 'async_trait {
        todo!()
    }


    fn block_user< 'life0, 'async_trait>(& 'life0 self,request:tonic::Request<user::BlockRequest> ,) ->  core::pin::Pin<Box<dyn core::future::Future<Output = Result<tonic::Response<common_types::Empty> ,tonic::Status> > + core::marker::Send+ 'async_trait> >where 'life0: 'async_trait,Self: 'async_trait {
        todo!()
    }


    fn unblock_user< 'life0, 'async_trait>(& 'life0 self,request:tonic::Request<user::BlockRequest> ,) ->  core::pin::Pin<Box<dyn core::future::Future<Output = Result<tonic::Response<common_types::Empty> ,tonic::Status> > + core::marker::Send+ 'async_trait> >where 'life0: 'async_trait,Self: 'async_trait {
        todo!()
    }


    fn list_invitations< 'life0, 'async_trait>(& 'life0 self,request:tonic::Request<user::PersonalAssetsRequest> ,) ->  core::pin::Pin<Box<dyn core::future::Future<Output = Result<tonic::Response<conversation::ConvHeaderList> ,tonic::Status, > > + core::marker::Send+ 'async_trait> >where 'life0: 'async_trait,Self: 'async_trait {
        todo!()
    }

    fn get_visibility< 'life0, 'async_trait>(& 'life0 self,request:tonic::Request<common_types::AuthenticatedObjectRequest, > ,) ->  core::pin::Pin<Box<dyn core::future::Future<Output = Result<tonic::Response<conversation::Conversation> ,tonic::Status, > > + core::marker::Send+ 'async_trait> >where 'life0: 'async_trait,Self: 'async_trait {
        todo!()
    }

    fn update_wallet< 'life0, 'async_trait>(& 'life0 self,request:tonic::Request<common_types::AuthenticatedObjectRequest> ,) ->  core::pin::Pin<Box<dyn core::future::Future<Output = Result<tonic::Response<common_types::Empty> ,tonic::Status, > > + core::marker::Send+ 'async_trait> >where 'life0: 'async_trait,Self: 'async_trait {
        todo!()
    }

    fn get_notifications< 'life0, 'async_trait>(& 'life0 self,request:tonic::Request<notifications::GetNotificationsRequest> ,) ->  core::pin::Pin<Box<dyn core::future::Future<Output = Result<tonic::Response<notifications::NotificationsResponse> ,tonic::Status, > > + core::marker::Send+ 'async_trait> >where 'life0: 'async_trait,Self: 'async_trait {
        todo!()
    }

    fn get_informations< 'life0, 'async_trait>(& 'life0 self,request:tonic::Request<common_types::AuthenticatedRequest> ,) ->  core::pin::Pin<Box<dyn core::future::Future<Output = Result<tonic::Response<settings::UserInformationsResponse> ,tonic::Status, > > + core::marker::Send+ 'async_trait> >where 'life0: 'async_trait,Self: 'async_trait {
        todo!()
    }


    fn change_informations< 'life0, 'async_trait>(& 'life0 self,request:tonic::Request<settings::UserInformations> ,) ->  core::pin::Pin<Box<dyn core::future::Future<Output = Result<tonic::Response<common_types::Empty> ,tonic::Status> > + core::marker::Send+ 'async_trait> >where 'life0: 'async_trait,Self: 'async_trait {
        todo!()
    }


    fn list_user_convs< 'life0, 'async_trait>(& 'life0 self,request:tonic::Request<user::UserAssetsRequest> ,) ->  core::pin::Pin<Box<dyn core::future::Future<Output = Result<tonic::Response<conversation::ConvHeaderList> ,tonic::Status, > > + core::marker::Send+ 'async_trait> >where 'life0: 'async_trait,Self: 'async_trait {
        todo!()
    }


    fn list_user_replies< 'life0, 'async_trait>(& 'life0 self,request:tonic::Request<user::UserAssetsRequest> ,) ->  core::pin::Pin<Box<dyn core::future::Future<Output = Result<tonic::Response<common_types::ReplyHeaderList> ,tonic::Status, > > + core::marker::Send+ 'async_trait> >where 'life0: 'async_trait,Self: 'async_trait {
        todo!()
    }


    fn list_user_upvotes< 'life0, 'async_trait>(& 'life0 self,request:tonic::Request<user::UserAssetsRequest> ,) ->  core::pin::Pin<Box<dyn core::future::Future<Output = Result<tonic::Response<conversation::ConvHeaderList> ,tonic::Status, > > + core::marker::Send+ 'async_trait> >where 'life0: 'async_trait,Self: 'async_trait {
        todo!()
    }


    fn list_user_downvotes< 'life0, 'async_trait>(& 'life0 self,request:tonic::Request<user::UserAssetsRequest> ,) ->  core::pin::Pin<Box<dyn core::future::Future<Output = Result<tonic::Response<conversation::ConvHeaderList> ,tonic::Status, > > + core::marker::Send+ 'async_trait> >where 'life0: 'async_trait,Self: 'async_trait {
        todo!()
    }


    fn modify_visibility< 'life0, 'async_trait>(& 'life0 self,request:tonic::Request<visibility::ModifyVisibilityRequest> ,) ->  core::pin::Pin<Box<dyn core::future::Future<Output = Result<tonic::Response<common_types::Empty> ,tonic::Status> > + core::marker::Send+ 'async_trait> >where 'life0: 'async_trait,Self: 'async_trait {
        todo!()
    }


}

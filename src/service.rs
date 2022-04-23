#![allow(unused)]
use base64ct::Base64UrlUnpadded;
use bson::{oid::ObjectId, Bson};
use futures::StreamExt;
use redis::{RedisConnectionInfo, RedisError};
use serde::{Serialize, Deserialize};
use jsonwebtoken::{encode, decode, Header, Algorithm, Validation, EncodingKey, DecodingKey, decode_header, TokenData, crypto::verify};
use tonic::{Request, Response, Status};
use crate::{api::{*, feed::FeedType, common_types::{Genre, Votes, FileUploadResponse}, conversation::{NewConvRequestResponse,ConvHeader,ConvHeaderList, ConversationComponent, conversation_component, ConvData, ConvDetails, ConvMetadata}, search::{ SearchConvResponse, SearchUserResponse}, visibility::Visibility, replies::{ReplyRequestResponse, ReplyList, Reply}}, cache_init::{ConversationRank, get_epoch, TIMEFEEDTYPES, feedType2seconds}};
use reqwest;
use moka::future::Cache;
use std::{collections::{HashMap, hash_map::RandomState, BTreeMap}, borrow::{BorrowMut, Borrow}, time::{SystemTime, Duration}, hash::Hash, str::FromStr, num::{NonZeroI64, NonZeroI32}};
use sha2::{Sha256, Sha512, Digest};
use aws_sdk_s3::{presigning::config::PresigningConfig, types::{ByteStream, DateTime}};
use bb8_redis::{
    bb8::{self, Pool, PooledConnection},
    redis::{cmd, AsyncCommands},
    RedisConnectionManager
};

use regex::Regex;
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
use array_tool::vec::Uniq;
use mongodb::{Client as MongoClient, 
    options::{Acknowledgment, ReadConcern, TransactionOptions, WriteConcern,ClientOptions, DriverInfo, Credential,
         ServerAddress, FindOptions, FindOneOptions,
          UpdateModifications, FindOneAndUpdateOptions, 
          FindOneAndDeleteOptions, UpdateOptions, InsertOneOptions,
           CountOptions}, bson::{doc, Document}, Collection,
           ClientSession,
           error::{Result as MongoResult, Error as MongoError,TRANSIENT_TRANSACTION_ERROR, UNKNOWN_TRANSACTION_COMMIT_RESULT}};





//extern crate rusoto_core;
//extern crate rusoto_dynamodb;

use aws_sdk_dynamodb::{Client as DynamoClient, Error, model::{AttributeValue, ReturnValue}, types::{SdkError, self}, error::{ConditionalCheckFailedException, PutItemError, conditional_check_failed_exception, PutItemErrorKind}};
use aws_sdk_s3::{Client as S3Client, Region, PKG_VERSION};

use std::time::{UNIX_EPOCH};

#[derive(Debug, Serialize, Deserialize)]
struct ConvReplyScope {
    visibility: Visibility,
    box_ids: Vec<i32>,
}


//convs replies
async fn execute_transaction(
    pseudo:&str,
    reply:&str,
    boxid:i32,
    anonym:bool,
    convid:bson::oid::ObjectId,
    replyto:&str,
    convs: &Collection<ConvReplyScope>, replies: &Collection<Document>, session: &mut ClientSession) -> Result<Result<String,MongoError>,tonic::Status> {
    
    //get conv visibility & boxids
    //if visibility & boxid correct
        //if replyfromid, count one replyfromid, convid 
        //append
    let filter: mongodb::bson::Document = doc! { "visibility":i32::from(1),"box_ids":i32::from(1) };
    let options = FindOneOptions::builder().projection(filter).build();
        
 let reply_id= match convs.find_one_with_session(doc!{
        "_id":convid
    }, options, session).await.unwrap() {
        Some(conv) => {
            if legitimate(pseudo,&conv.visibility) && conv.box_ids.contains(&boxid) {
                if replyto!="" {
                    let options = CountOptions::builder().limit(1).build();


                   match replies.count_documents_with_session(doc!{
"convid":convid,
"replyto":replyto
                    }, options, session).await {
                        Ok(count) => {

                            if count <= 0 {
                                
                                return Err(Status::new(tonic::Code::InvalidArgument, "reply not found auth"))
                            }
                        },
                        Err(_) =>  {
                            return Err(Status::new(tonic::Code::InvalidArgument, "count error"))
                        },
                    }
                }

match replies.insert_one_with_session(doc!{
    "pseudo":pseudo,
    "anonym":anonym,
    "reply":reply,
    "boxid":boxid,
    "convid":convid.to_string(),
    "replyto":replyto,
    "created_at":get_epoch() as f64,
    "upvotes":i32::from(0),
    "downvotes":i32::from(0),
    "score":i32::from(0)
}, None, session).await {
    Ok(value) => {
        value.inserted_id.to_string()
    },
    Err(_) =>  return Err(Status::new(tonic::Code::InvalidArgument, "insert error")),
}
        } else {
                return Err(Status::new(tonic::Code::InvalidArgument, "invalid insertion"))
            }


        },
        None => {
            return Err(Status::new(tonic::Code::InvalidArgument, "find conv error"))
        },
    };

    // An "UnknownTransactionCommitResult" label indicates that it is unknown whether the
    // commit has satisfied the write concern associated with the transaction. If an error
    // with this label is returned, it is safe to retry the commit until the write concern is
    // satisfied or an error without the label is returned.
    loop {
        let result = session.commit_transaction().await;
        if let Err(ref error) = result {
            if error.contains_label(UNKNOWN_TRANSACTION_COMMIT_RESULT) {
                continue;
            }
        }
        match result {
            Ok(_) => return Ok(Ok(reply_id)),
            Err(err) =>  return Ok(Err(err))
            ,
        }
    }
}

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
    pub path_salt:String,
    pub regex:Regex
 //   pub cache:Cache<String, String>,

}

// todo parse, to_string, verify
struct SecurePath {
    pub nonce: String,
    pub secret: String
}

#[derive(Debug, Serialize, Deserialize)]
struct ScreensToDelete {
    screens_uri:Vec<String>,
}


#[derive(Debug, Serialize, Deserialize)]
struct Pseudo {
    pseudo:String,
}


#[derive(Debug, Serialize, Deserialize)]
struct ConvHeaderCache {
 //   convid:String ,
    details:ConvDetails,
    metadata:ConvMetadata,
 //   visibility:Visibility
}

#[derive(Debug, Serialize, Deserialize)]
struct FullConv {
    details:ConvDetails,
    metadata:ConvMetadata,
    visibility:Visibility,
    flow:Vec<ConversationComponent>
}



#[derive(Deserialize)]
struct FacebookToken {
    access_token: String,
    token_type: String,
    expires_in: i64
}

const GOOGLE_WEB_CLIENT_ID:&str="470515755626-27pkbok135g1d8v9533ukd98smqneilg.apps.googleusercontent.com";

const GRANT_TYPE : &str="authorization_code";

const GOOGLE_WEB_REDIRECT_URL:&str="https://conv911.com/logged";

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

//for cache
fn Map2ConvHeader(convid:&str,map:&BTreeMap<String, String>)->ConvHeader {
    ConvHeader{
        convid:  convid.to_string(),
        details:Some(ConvDetails{ 
            title: map.get("title").unwrap().to_string(), 
            description: map.get("description").unwrap().to_string(), 
            categories: map.get("categories").unwrap().split(':').map(
                |category| String::from(category)).collect::<Vec<String>>(), 
            language: map.get("language").unwrap().to_string() }),
            metadata:Some(ConvMetadata{
                pseudo: map.get("pseudo").unwrap().to_string(), 
                upvote: map.get("upvote").unwrap().parse::<i32>().unwrap(), 
                downvote: map.get("downvote").unwrap().parse::<i32>().unwrap(), 
                created_at: map.get("created_at").unwrap().parse::<i64>().unwrap()
            }),

    }
}

fn Map2Visibility(map:&BTreeMap<String, String>)->Visibility {
    return Visibility{
        anonym: map.get("anonym").unwrap().parse::<bool>().unwrap(),
        anon_hide:  map.get("anon_hide").unwrap().parse::<bool>().unwrap(),
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct MongoConv {
    #[serde(rename="_id", skip_serializing_if="Option::is_none")]
    id: Option<ObjectId>,
  //  _id: ObjectId,
    details:ConvDetails,
    flow:Vec<ConversationComponent>,
    visibility:Visibility,
    metadata:ConvMetadata,
    score:i32
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

async fn get_conv_visibility(convid:&str,keydb_pool:Pool<RedisConnectionManager>,mongo_client:&MongoClient)->Option<Visibility> {

    let mut keydb_conn = keydb_pool.get().await.expect("keydb_pool failed");

    println!("CONVID {:#?}",convid);
    let cached:BTreeMap<String, String>=   cmd("hgetall")
                    .arg("V:".to_owned()+convid).query_async(&mut *keydb_conn).await.expect("hgetall failed");
        println!("cache: {:#?}",cached);
     
                //cache miss
                if cached.is_empty() {
                
                    let conversations = mongo_client.database("DB")
                    .collection::<Visibility>("convs");


let filter: mongodb::bson::Document = doc! {
    "anonym":"$visibility.anonym",
    "anon_hide":"$visibility.anon_hide",
 };
let options = FindOneOptions::builder().projection(filter).build();



let mut result = conversations.find_one( 
    doc! { "_id" :  bson::oid::ObjectId::parse_str(convid).unwrap() },
     options).await.unwrap();

     match result {
        Some(value) => {
            let _:()=   cmd("hmset")
            .arg(&vec![
                 &("V:".to_owned()+   convid) as &str,
                    "anonym",&value.anonym.to_string(),
                    "anon_hide",&value.anon_hide.to_string()
                    ] ).query_async(&mut *keydb_conn).await.unwrap();
            let _:()=   cmd("expire")
            .arg(&[convid,"60"]).query_async(&mut *keydb_conn).await.unwrap();

       return Some(value)
       
        },
        None =>{
            return None
        },
    }


}
  //cache hit
  else {
    
    let _:()=   cmd("expire")
    .arg(&[convid,"2"]).query_async(&mut *keydb_conn).await.unwrap();

 return Some(Map2Visibility(&cached));

              
                }

}

async fn get_conv_header(convid:&str,keydb_pool:Pool<RedisConnectionManager>,mongo_client:&MongoClient)->Option<ConvHeader>{

    let mut keydb_conn = keydb_pool.get().await.expect("keydb_pool failed");

    println!("CONVID {:#?}",convid);
    let cached:BTreeMap<String, String>=   cmd("hgetall")
                    .arg(convid).query_async(&mut *keydb_conn).await.expect("hgetall failed");
        println!("cache: {:#?}",cached);
     
                //cache miss
                if cached.is_empty() {
                
                    let conversations = mongo_client.database("DB")
                    .collection::<ConvHeaderCache>("convs");
let pipeline = vec![
     doc! { "$match": { "_id" :  bson::oid::ObjectId::parse_str(convid).unwrap() }} ,
     doc!{ "$limit": i32::from(1) },

   doc! { "$project": {
   //    "convid": { "$toString": "$_id"},
        "details":i32::from(1),
        "metadata.upvote":i32::from(1),
        "metadata.downvote":i32::from(1),
        "metadata.created_at":i32::from(1),
        "metadata.pseudo":{"$cond": ["$visibility.anonym", "", "$pseudo"]},  
   //     "visibility"  :   i32::from(1)
},

}  ];


let mut results = conversations.aggregate(pipeline, None).await.unwrap();


if let Some(result) = results.next().await {

   let conv_header: ConvHeaderCache = bson::from_document(result.unwrap()).unwrap();


   let _:()=   cmd("hmset")
   .arg(&vec![
           convid,
           "title",&conv_header.details.title,
       //    "userid",&conv_header.userid,
           "pseudo",&conv_header.metadata.pseudo,
           "upvote",&conv_header.metadata.upvote.to_string(),
           "downvote",&conv_header.metadata.downvote.to_string(),
           "categories",&conv_header.details.categories.join(":"),
           "created_at",&conv_header.metadata.created_at.to_string(),
           "description",&conv_header.details.description,
           "language",&conv_header.details.language,
       //    "anon_hide",&conv_header.visibility.anon_hide.to_string(),
       //    "anonym",&conv_header.visibility.anonym.to_string(),
           ] ).query_async(&mut *keydb_conn).await.unwrap();
   let _:()=   cmd("expire")
   .arg(&[convid,"60"]).query_async(&mut *keydb_conn).await.unwrap();


   return   Some(ConvHeader{ convid: convid.to_string(),
                     details: Some(conv_header.details),
                     metadata: Some(conv_header.metadata) } )
  

} else {
    //not found
    return None
}
 } //cache hit
  else {
                
                let _:()=   cmd("expire")
                .arg(&[convid,"2"]).query_async(&mut *keydb_conn).await.unwrap();

                return Some(Map2ConvHeader(convid,&cached));

              
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
        feed::FeedType::New=>Some("New"),
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

fn check_conv_details(details:&ConvDetails) ->Result<(), tonic::Status> {

    if details.categories.len()>3 {
        return Err(Status::new(tonic::Code::InvalidArgument, "too many categories"))
    }

    let re= Regex::new(r"^[a-zA-Z0-9]{4,13}$").unwrap();
for cat in &details.categories {
    if ! re.is_match(cat) {
        return Err(Status::new(tonic::Code::InvalidArgument, "invalid category"))
    }
}

  if  details.title.len() > 100 {
    return Err(Status::new(tonic::Code::InvalidArgument, "title too large"))
  }
  if  details.description.len() > 400 {
    return Err(Status::new(tonic::Code::InvalidArgument, "description too large"))
  }

  if Language::from_iso639_3(&details.language).is_err() {
     
    return Err(Status::new(tonic::Code::InvalidArgument, "invalid language"))
  }

  Ok(())

}

async fn check_conv_flow(flow:&Vec<ConversationComponent>,pseudo:&str,path_salt:&str)->Result<ConvCheck, Status> {


  if flow.len() > 100 {
    return Err(Status::new(tonic::Code::InvalidArgument, "conv too long"))
  }


 let mut new_screens_uri :HashSet<String>=HashSet::new();
 let mut box_ids:HashSet<i32> = HashSet::new();


 for component in flow { 
    match &component.component {

        Some(value)=>{

            match value {
conversation_component::Component::Screen(screen) => {
    //check if owned
 let path_secret = generate_path_secret(pseudo,path_salt,&screen.uri[..7]);
 if screen.uri[7..] != path_secret {
    return Err(Status::new(tonic::Code::InvalidArgument, "screen not owned"))
 }


 if ! new_screens_uri.insert(screen.uri.clone()) {
    return Err(Status::new(tonic::Code::InvalidArgument, "screen duplicated"))
}

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

  return Ok(ConvCheck { screens_uri: new_screens_uri, box_ids: box_ids})
}

struct ConvCheck {
    screens_uri:HashSet<String>,
    box_ids: HashSet<i32>
}

#[derive(Debug, Serialize, Deserialize)]
struct ConvResources {
    screens_uri:Vec<String>,
    box_ids: Vec<i32>
}

async fn tmp2pictures(screen_uri:&str,s3_client:&S3Client) {
    let mut source_bucket_and_object: String = "".to_owned();
    source_bucket_and_object.push_str(SCREENSHOTS_BUCKET);
    source_bucket_and_object.push_str("/tmp/");
    source_bucket_and_object.push_str(&screen_uri);

s3_client.copy_object()
  .bucket(SCREENSHOTS_BUCKET)
  .copy_source(source_bucket_and_object)
    .key("static/pictures/".to_string()+&screen_uri+".jpg")
      .send()
      .await ;
    
s3_client.delete_object()
.bucket(SCREENSHOTS_BUCKET)
.key("tmp/".to_string()+&screen_uri).send().await;
}

fn legitimate(pseudo:&str,visibility:&Visibility)->bool{
    if visibility.anon_hide && pseudo=="" {
        return false
    }
    return true
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

                    let access_token = if client_type==login::ClientType::Android || client_type == login::ClientType::Ios {
                        match request.auth.as_ref().unwrap() {
            login::login_request::Auth::AccessToken(token) => token.to_string(),
            _ => {
                return Err(Status::new(tonic::Code::InvalidArgument, "invalid device auth"))
            },
        }
                    } else {    
                
                        let code= match  request.auth.as_ref().unwrap() {

    login::login_request::Auth::Code(code) =>code,
    _=>{
        return Err(Status::new(tonic::Code::InvalidArgument, "invalid device auth"))  
    }
};

                    let facebook_request = client.get("https://graph.facebook.com/oauth/access_token")
                    .query(
                        &[
                            //safe? optimal?
                            ("redirect_uri","https://example.com/"),
                            ("code", code),
                            ("client_id",&self.facebook_client_id),
                            ("client_secret",&self.facebook_client_secret),

                        ]).send().await;

                    let facebook_request = match facebook_request {
                        Ok(facebook_request) => facebook_request,
                        Err(_) => return Err(Status::new(tonic::Code::InvalidArgument, "oauth request error"))
                    };
        
                    let facebook_response = match facebook_request.json::<FacebookToken>().await {
                        Ok(facebook_response) => facebook_response,
                        Err(_) => return Err(Status::new(tonic::Code::InvalidArgument, "oauth json error"))
                    };
                    facebook_response.access_token
                    };

                    println!("access_token {:#?}",access_token);
                    let facebook_request = client.get("https://graph.facebook.com/me")
                    .query(
                        &[
                            //safe? optimal?
                            ("fields","id"),
                            ("access_token", &access_token)

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
      let filter: mongodb::bson::Document = doc! { "pseudo":i32::from(1) };
    let options = FindOneOptions::builder().projection(filter).build();

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
                        is_new: true
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

    async fn set_account(&self,request:Request<user::SetAccountRequest> ) ->  Result<Response<common_types::RefreshToken> ,Status > {
          
        let request=request.get_ref();
        let data=decode::<JWTSetPseudo>(&request.access_token,&self.jwt_decoding_key,&self.jwt_algo);
        let data=match data {
            Ok(data)=>data,
            _=>{ return Err(Status::new(tonic::Code::InvalidArgument, "invalid token"))}
        };
        

        let conversations = self.mongo_client.database("DB")
        .collection("users");

        /*

        if  request.pseudo.len() < 4 || request.pseudo.len() > 13 {
            return Err(Status::new(tonic::Code::InvalidArgument, "invalid pseudo size"))
        }
        */

       let re= Regex::new(r"^[a-zA-Z0-9_-]{4,20}$").unwrap();



        if ! re.is_match(&request.pseudo) {
            return Err(Status::new(tonic::Code::InvalidArgument, "invalid pseudo char"))
        }


      if self.regex.is_match(&request.pseudo.to_lowercase()) {
        return Err(Status::new(tonic::Code::InvalidArgument, "invalid substring"))
        }

      //  check valid  genre
       if Country::from_alpha2(&request.country).is_err(){
        return Err(Status::new(tonic::Code::InvalidArgument, "invalid country"))
       }

       if request.date_of_birth<=0 {
        return Err(Status::new(tonic::Code::InvalidArgument, "invalid date_of_birth"))
       }


        let options = CountOptions::builder().limit(1).build();



        let count=conversations.count_documents(
            doc!{
"pseudo": &request.pseudo.to_lowercase()
            } , options).await.unwrap();

        if count>0 {
            return Err(Status::new(tonic::Code::InvalidArgument, "banned pseudo"))   
        }

       
        //check valid userid,
       if let Err(_) = conversations.insert_one(
           doc!{
                "pseudo": &request.pseudo.to_lowercase(),
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

    async fn refresh_token(&self,request:Request<common_types::AuthenticatedRequest>) ->  Result<Response<common_types::RefreshToken>, Status> {
        
        let request=request.get_ref();
        let data=decode::<JWTPayload>(&request.access_token,&self.jwt_decoding_key,&self.jwt_algo);
        let data=match data {
            Ok(data)=>data,
            _=>{ return Err(Status::new(tonic::Code::InvalidArgument, "invalid token"))}
        };

        
        let payload: JWTPayload = JWTPayload {
            exp:(get_epoch()+60*60*24*30) as u64,
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

    async fn feed(&self,request:Request<feed::FeedRequest> ) -> Result<Response<conversation::ConvHeaderList>,Status> {
        let request=request.get_ref();

        let pseudo = if !&request.access_token.is_empty(){

            match decode::<JWTPayload>(&request.access_token,&self.jwt_decoding_key,&self.jwt_algo) {
                 Ok(data)=>data.claims.pseudo.to_owned(),
                 _=>{ return Err(Status::new(tonic::Code::InvalidArgument, "invalid token"))}
        
             } } else {
     String::from("")
         };

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
let header= get_conv_header(&convid,self.keydb_pool.clone(),&self.mongo_client).await;
let visibility=get_conv_visibility(&convid,self.keydb_pool.clone(),&self.mongo_client).await;
match header
{
    Some(value) => match visibility  {
        Some(visib) => {

            if legitimate(&pseudo,&visib) {
                replylist.push(value)
            }
        },
        None=>(),

    } ,
    None => (),
}
          
        }

        //https://crates.io/crates/moka

        
        return  Ok(Response::new(ConvHeaderList{ convheaders: replylist }))
    }

    async fn search_user(&self,request:Request<search::SearchUserRequest> ) ->  Result<Response<search::SearchUserResponse> ,Status > {
        let request=request.get_ref();

//search 

let mut pseudo_list:Vec<String> = vec![];

let conversations = self.mongo_client.database("DB")
.collection::<Pseudo>("convs");
//::<ConvHeader>

let offset=request.offset;
if offset<0 {
  return   Err(Status::new(tonic::Code::InvalidArgument, "invalid offset"))
}

//rename _id to convid
let pipeline = vec![
doc! {   "$match": { "$text": { "$search": &request.query } }  } ,
doc!{ "$skip" : offset },
doc!{ "$limit": i32::from(10) },

doc! { "$project": {
"pseudo":i32::from(1),

}}];
let mut results = conversations.aggregate(pipeline, None).await.unwrap();

while let Some(result) = results.next().await {
    let bson_pseudo: Pseudo = bson::from_document(result.unwrap()).unwrap();
    pseudo_list.push(bson_pseudo.pseudo);
    }


    
Ok(Response::new(SearchUserResponse{ pseudos: pseudo_list }))

    }

    async fn search_conv(&self,request:Request<search::SearchConvRequest> ) ->  Result<Response<search::SearchConvResponse> ,Status > {

        let request=request.get_ref();

        let pseudo = if !&request.access_token.is_empty(){

            match decode::<JWTPayload>(&request.access_token,&self.jwt_decoding_key,&self.jwt_algo) {
                 Ok(data)=>data.claims.pseudo.to_owned(),
                 _=>{ return Err(Status::new(tonic::Code::InvalidArgument, "invalid token"))}
        
             }
            } else {
                String::from("")
                    };
//search 

let mut header_list:Vec<ConvHeader> = vec![];

let conversations = self.mongo_client.database("DB")
.collection::<ConvHeader>("convs");
//::<ConvHeader>

let offset=request.offset;
if offset<0 {
  return   Err(Status::new(tonic::Code::InvalidArgument, "invalid offset"))
}

//rename _id to convid
let pipeline = vec![

doc! {   "$match": { "$text": { "$search": &request.query } }  } ,

doc!{ "$skip" : offset },

doc!{ "$limit": i32::from(10) },

doc! { "$project": {
"convid": { "$toString": "$_id"},
"details":i32::from(1),
//mongo intection
"metadata":i32::from(1),
"visibility":i32::from(1),
}
},
doc!{ "$set": {
    "metadata.pseudo":{"$cond": ["$anonym", "", "$pseudo"]},
}}


];
let mut results = conversations.aggregate(pipeline, None).await.unwrap();

while let Some(result) = results.next().await {
    let conv_header: ConvHeader = bson::from_document(result.unwrap()).unwrap();


match get_conv_visibility(&conv_header.convid,self.keydb_pool.clone(),&self.mongo_client).await {
        Some(visib) => {

            if legitimate(&pseudo,&visib) {
                header_list.push(conv_header);
            }
        },
        None=>(),
    }
    
    }


    
Ok(Response::new(SearchConvResponse{ convheaders: header_list }))
   
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
       
    async fn get_conv(& self,request:Request<common_types::AuthenticatedObjectRequest, > ,) ->  Result<tonic::Response<conversation::ConvData> ,tonic::Status, > {

        //annon_hide
        //owner

        let request=request.get_ref();
        let pseudo = if !&request.access_token.is_empty(){

       match decode::<JWTPayload>(&request.access_token,&self.jwt_decoding_key,&self.jwt_algo) {
            Ok(data)=>data.claims.pseudo.to_owned(),
            _=>{ return Err(Status::new(tonic::Code::InvalidArgument, "invalid token"))}
   
        }
       
       
    } else {
String::from("")
    };

        //LOGIC: owner or not private
        //check if invited





//mongo intection
// 




let conversations = self.mongo_client.database("DB")
.collection::<FullConv>("convs");


let mut results = conversations.aggregate([doc!{
  "$match":{  "_id" :  bson::oid::ObjectId::parse_str(&request.id).unwrap()
}},
doc!{"$limit":i32::from(1)},

doc!{"$project":{
    "details":i32::from(1),
    "metadata":i32::from(1),
    "visibility":i32::from(1),
    "flow":i32::from(1),

}},
doc!{"$set": {
    "metadata.pseudo": {"$cond": ["$anonym", {"$cond": [{"$eq": [ "$pseudo",&pseudo ]}, "$pseudo", ""]}, "$pseudo"]},
}
}
]

,None).await.unwrap();




if let Some(result) = results.next().await {
    let result:FullConv = bson::from_document(result.unwrap()).unwrap();
//println!("{:#?}", conv_header);
if legitimate(&pseudo,&result.visibility) {

    Ok(Response::new(ConvData{
        details: Some(result.details),
         metadata: Some(result.metadata), 
         flow: result.flow
       }))
} else {
    return Err(Status::new(tonic::Code::NotFound, "conv not found"))
}



} else {

    return Err(Status::new(tonic::Code::NotFound, "conv not found"))
    //todo: invited or not exists

}


        
    }

    async fn get_visibility(& self,request:tonic::Request<common_types::AuthenticatedObjectRequest, > ,) ->   Result<tonic::Response<visibility::Visibility> ,tonic::Status, > {
      //check si conv à nous
      //renvoi

      let request=request.get_ref();
      let data=decode::<JWTPayload>(&request.access_token,&self.jwt_decoding_key,&self.jwt_algo);
      let data=match data {
          Ok(data)=>data,
          _=>{ return Err(Status::new(tonic::Code::InvalidArgument, "invalid token"))}
      };

      

      //check if user in mongo
      let filter: mongodb::bson::Document = doc! { "visibility":i32::from(1) };
    let options = FindOneOptions::builder().projection(filter).build();

    let users = self.mongo_client.database("DB")
    .collection::<Visibility>("users");
    

    let mut cursor = users.find_one(doc!{
        "_id":&bson::oid::ObjectId::parse_str(&request.id).unwrap(),
        "pseudo": &data.claims.pseudo
    },
        options
  //  FindOneOptions::default() 
        ).await;
    match cursor {
    Ok(value) => {
        match value {
            None=>{
                return Err(Status::new(tonic::Code::InvalidArgument, "conv not found"))
            },
            Some(value)=>{
                //known user
              return   Ok(Response::new(value))
            }
        }
    },
    Err(_) => { return Err(Status::new(tonic::Code::InvalidArgument, "db error"))},
}
      
    }

    async fn modify_visibility(& self,request:tonic::Request<visibility::ModifyVisibilityRequest> ,) ->  Result<tonic::Response<common_types::Empty> ,tonic::Status>  {
        let request=request.get_ref();
        
        let data=decode::<JWTPayload>(&request.access_token,&self.jwt_decoding_key,&self.jwt_algo);
        let data=match data {
            Ok(data)=>data,
            _=>{ return Err(Status::new(tonic::Code::InvalidArgument, "invalid token"))}
        };
    

    
    let conversations = self.mongo_client.database("DB")
    .collection::<ConvResources>("convs");
    
    
    
    
    let convid=&bson::oid::ObjectId::parse_str(&request.convid).unwrap();
    
    let previous_ressources= match conversations.find_one_and_update(
    
        doc! { "_id": convid,
               "pseudo": &data.claims.pseudo
             },
        doc! { "$set": bson::to_bson(&request.visibility.as_ref().unwrap()).unwrap() },
    
        None
    ).await {
    Ok(value) => {
        match value {
    Some(value) => {
        value
    },
    None => {
        return Err(Status::new(tonic::Code::NotFound, "conv not found for user"))  
    },
    }
        
    },
    Err(_) => {
        return Err(Status::new(tonic::Code::InvalidArgument, "db err"))
    },
    };
    
    //invalidate cache
let mut keydb_conn = self.keydb_pool.get().await.expect("keydb_pool failed");
let _:()=cmd("unlink").arg("V:".to_owned()+&request.convid).query_async(&mut *keydb_conn).await.expect("unlink failed");

    
    
    Ok(Response::new(common_types::Empty{}))
    }

    async fn new_conv(&self,request:Request<conversation::NewConvRequest> ) ->  Result<Response<conversation::NewConvRequestResponse> ,Status > {

        let request=request.get_ref();

        let data=decode::<JWTPayload>(&request.access_token,&self.jwt_decoding_key,&self.jwt_algo);
        let data=match data {
            Ok(data)=>data,
            _=>{ return Err(Status::new(tonic::Code::InvalidArgument, "invalid token"))}
        };
        


    let check=check_conv_details(request.details.as_ref().unwrap());
    if check.is_err(){
return Err(check.err().unwrap())
    }



        
   

        let conv_check=check_conv_flow(&request.flow,&data.claims.pseudo,&self.path_salt).await;

        let conv_check = match conv_check {
            Err(value)=>return Err(value),
            Ok(value)=>value  };

  //move screens from tmp/ to pictues/
  for screen_uri in &conv_check.screens_uri.to_owned() {
tmp2pictures(screen_uri,&self.s3_client).await;
  }


        //mongo: add new conv
        let conversations = self.mongo_client.database("DB")
        .collection::<MongoConv>("convs");
       
        let current_timestamp = get_epoch();
        // let conv_data=request.conv_data.as_ref().unwrap();
       // let a=request.details.as_ref().unwrap();
        let convid= conversations.insert_one(
            
            MongoConv{id:None,
                details: request.details.as_ref().unwrap().to_owned(),
                flow: request.flow.to_owned(),
                visibility: request.visibility.as_ref().unwrap().to_owned(),
                metadata: ConvMetadata{
                    upvote:0,
                    downvote:0,
                    created_at:current_timestamp as i64,
                    pseudo:data.claims.pseudo

                },
            score:0 }


            ,None).await.unwrap().inserted_id.to_string();
            
        
        //if not private: keydb: add to all feeds(including new activities)
//if !request.private {



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
//}

        return  Ok(Response::new(NewConvRequestResponse { convid: "a".to_string() }))
      
    }

    async fn delete_conv(&self,request:Request<common_types::AuthenticatedObjectRequest> ) ->  Result<Response<common_types::Empty> ,Status > {
        // /!\ if create and delete juste before create ends, => screens may not
          let request=request.get_ref();
          
          let data=decode::<JWTPayload>(&request.access_token,&self.jwt_decoding_key,&self.jwt_algo);
          let data=match data {
              Ok(data)=>data,
              _=>{ return Err(Status::new(tonic::Code::InvalidArgument, "invalid token"))}
          };
  
          //TODO: LOGIC: owner or not private
          //check if invited
  
          let convid= bson::oid::ObjectId::parse_str(&request.id).unwrap();
  
  
          let conversations = self.mongo_client.database("DB")
          .collection::<ScreensToDelete>("convs");
          //::<ConvHeader>
  
          let header_filter: mongodb::bson::Document = doc! {
              "screens":i32::from(1) ,};
          let options = FindOneAndDeleteOptions::builder().projection(header_filter).build();
      
  
      match  conversations.find_one_and_delete(
              doc! { "$match": 
              { "_id" : convid ,
                  "pseudo":data.claims.pseudo}}
              , options ).await {
      Ok(value) => {
          match value {
      Some(value) => {
         for screen in &value.screens_uri {
  
          self.s3_client.delete_object().bucket(SCREENSHOTS_BUCKET)
          .key("pictues/".to_string()+screen).send().await;
  
         }
    
         let replies = self.mongo_client.database("DB").collection::<crate::service::common_types::Empty>("replies");
         replies.delete_many( doc! {  "conv" : convid}  , None ).await;
    
         let votes = self.mongo_client.database("DB").collection::<crate::service::common_types::Empty>("votes");
         votes.delete_many( doc! {  "conv" : convid}  , None ).await;
  
         Ok(Response::new(common_types::Empty{}))
      },
      None => {
          //not found
          return Err(Status::new(tonic::Code::InvalidArgument, "conv not found"))
      },
  }
      },
      Err(_) => {
          return Err(Status::new(tonic::Code::InvalidArgument, "db error"))
      },
  }
      }
  
    async fn modify_conv_details(&self,request:Request<conversation::ModifyConvDetailsRequest> ,) -> Result<tonic::Response<common_types::Empty> ,tonic::Status> {

    let request=request.get_ref();
        
    let data=decode::<JWTPayload>(&request.access_token,&self.jwt_decoding_key,&self.jwt_algo);
    let data=match data {
        Ok(data)=>data,
        _=>{ return Err(Status::new(tonic::Code::InvalidArgument, "invalid token"))}
    };

    match check_conv_details(request.details.as_ref().unwrap()){
    Ok(_) => (),
    Err(err) => return Err(err),
}

let conversations = self.mongo_client.database("DB")
.collection::<ConvResources>("convs");




let convid=&bson::oid::ObjectId::parse_str(&request.convid).unwrap();

let previous_ressources= match conversations.find_one_and_update(

    doc! { "_id": convid,
           "pseudo": &data.claims.pseudo
         },
    doc! { "$set": bson::to_bson(&request.details.as_ref().unwrap()).unwrap() },

    None
).await {
Ok(value) => {
    match value {
Some(value) => {
    value
},
None => {
    return Err(Status::new(tonic::Code::NotFound, "conv not found for user"))  
},
}
    
},
Err(_) => {
    return Err(Status::new(tonic::Code::InvalidArgument, "db err"))
},
};




Ok(Response::new(common_types::Empty{}))
}

    async fn modify_conv_flow(&self,request:Request<conversation::ModifyConvFlowRequest> ,) -> Result<tonic::Response<common_types::Empty> ,tonic::Status> {

    let request=request.get_ref();
        
    let data=decode::<JWTPayload>(&request.access_token,&self.jwt_decoding_key,&self.jwt_algo);
    let data=match data {
        Ok(data)=>data,
        _=>{ return Err(Status::new(tonic::Code::InvalidArgument, "invalid token"))}
    };

    let conv_check=check_conv_flow(&request.flow,&data.claims.pseudo,&self.path_salt).await;

    let mut conv_check = match conv_check {
        Err(value)=>return Err(value),
        Ok(value)=>value  };

    
    let conversations = self.mongo_client.database("DB")
    .collection::<ConvResources>("convs");


    let header_filter: mongodb::bson::Document = doc! { 
        "screens":i32::from(1) ,
        "box_ids":i32::from(1) ,
    };
    let options = FindOneAndUpdateOptions::builder().projection(header_filter).build();

    let convid=&bson::oid::ObjectId::parse_str(&request.convid).unwrap();

    let previous_ressources= match conversations.find_one_and_update(

        doc! { "_id": convid,
               "pseudo": &data.claims.pseudo
             },
        doc! { "$set": bson::to_bson(&request.flow).unwrap() },

        options
    ).await {
    Ok(value) => {
        match value {
    Some(value) => {
        value
    },
    None => {
        return Err(Status::new(tonic::Code::NotFound, "conv not found for user"))  
    },
}
        
    },
    Err(_) => {
        return Err(Status::new(tonic::Code::InvalidArgument, "db err"))
    },
};

//
let mut pics_to_delete:Vec<&String> = vec![];

for old_screen in &previous_ressources.screens_uri  {
    if ! conv_check.screens_uri.remove(old_screen) {
        pics_to_delete.push(old_screen);
    }
  //  if new_screen in 
}

for pic_to_delete in &pics_to_delete {
    self.s3_client.delete_object().bucket(SCREENSHOTS_BUCKET)
    .key("static/pictues/".to_string()+&pic_to_delete).send().await;
}

for pic_to_move in &conv_check.screens_uri {
    tmp2pictures(pic_to_move,&self.s3_client).await;
}

let reps_table:Collection<crate::service::common_types::Empty> = self.mongo_client.database("DB")
.collection("reps");


for previous_box in &previous_ressources.box_ids {

    if ! conv_check.box_ids.contains(previous_box)   {

        let delete_result = reps_table.delete_many(
    doc! {
       "convid": convid,
       "boxid":previous_box
    },
    None,
 ).await;

println!("del result {:#?}",delete_result);

    }
}

//invalidate cache
let mut keydb_conn = self.keydb_pool.get().await.expect("keydb_pool failed");
let _:()=cmd("unlink").arg(&request.convid).query_async(&mut *keydb_conn).await.expect("unlink failed");


let _:()=   cmd("zadd")
.arg(&[feedType2cacheTable(feed::FeedType::LastActivity).unwrap(),
&get_epoch().to_string(),
&request.convid ]).query_async(&mut *keydb_conn).await.expect("zadd error");

Ok(Response::new(common_types::Empty{}))
}

    async fn submit_reply(&self,request:Request<replies::ReplyRequest, > ,) ->  Result<tonic::Response<replies::ReplyRequestResponse> ,Status> {
    //transaction:check if box and origin exists and add reply

                    /*
    reply:
    {"replyid":unique_id,
    "convid":123,
  //  "boxid":4,
    "replyfrom":replyid_or_boxid,
    "text":xxx,
    "writer":xxx,
    "anonym":boolean,
    "upvote":5,
    "downvote":2,
    "score":upvote-downvote,
    "created_at", xxx}
    */

    //get visibility+boxids

    //insert
    //check
    //if err rollback


    let request=request.get_ref();
        
    let data=decode::<JWTPayload>(&request.access_token,&self.jwt_decoding_key,&self.jwt_algo);
    let data=match data {
        Ok(data)=>data,
        _=>{ return Err(Status::new(tonic::Code::InvalidArgument, "invalid token"))}
    };


    let convs = self.mongo_client.database("DB")
    .collection::<ConvReplyScope>("convs");

    let replies = self.mongo_client.database("DB")
    .collection::<Document>("replies");

let mut session = self.mongo_client.start_session(None).await.unwrap();
let options = TransactionOptions::builder()
    .read_concern(ReadConcern::majority())
    .write_concern(WriteConcern::builder().w(Acknowledgment::Majority).build())
    .build();


session.start_transaction(options).await.unwrap();
// A "TransientTransactionError" label indicates that the entire transaction can be retried
// with a reasonable expectation that it will succeed.
let mut tried=0;
loop{
     let result=execute_transaction(
    &data.claims.pseudo, &request.reply,request.boxid,request.anonym,
    bson::oid::ObjectId::parse_str(&request.convid).unwrap(),&request.replyto,&convs,&replies,&mut session).await;
    
        match result{
    Ok(value) => {
match value {
    Ok(id) => {

        let mut keydb_conn = self.keydb_pool.get().await.expect("keydb_pool failed");


        let _:()=   cmd("zadd")
.arg(&[feedType2cacheTable(feed::FeedType::LastActivity).unwrap(),
&get_epoch().to_string(),
&request.convid ]).query_async(&mut *keydb_conn).await.expect("zadd error");

        return Ok(Response::new(ReplyRequestResponse{ replyid: id }))
    
    },
    Err(error) =>{

        if !error.contains_label(TRANSIENT_TRANSACTION_ERROR) {
            return Err(Status::new(tonic::Code::InvalidArgument, "transaction err"))
        }
    },
}
    },
    Err(err) => return Err(err),
}
if tried>4 {
    break;
}
 tried+=1;    
}
return Err(Status::new(tonic::Code::InvalidArgument, "internal err"))


 }

    async fn get_replies(&self,request:tonic::Request<replies::GetRepliesRequest> ,) -> Result<tonic::Response<replies::ReplyList> ,tonic::Status>  {
    //check authorized
    //return

    let request=request.get_ref();
          
    let data=decode::<JWTPayload>(&request.access_token,&self.jwt_decoding_key,&self.jwt_algo);
    let data=match data {
        Ok(data)=>data,
        _=>{ return Err(Status::new(tonic::Code::InvalidArgument, "invalid token"))}
    };

    let visibility=get_conv_visibility(&request.convid, self.keydb_pool.clone(), &self.mongo_client).await;
    match visibility {
        Some(vis)=> {
            if !legitimate(&data.claims.pseudo,&vis){
                return Err(Status::new(tonic::Code::InvalidArgument, "invalid conv"))
            }
        },
        None=> return Err(Status::new(tonic::Code::InvalidArgument, "invalid conv"))
    }



    //TODO: LOGIC: owner or not private
    //check if invited

    let convid= bson::oid::ObjectId::parse_str(&request.convid).unwrap();


    
    let conversations = self.mongo_client.database("DB")
    .collection::<ConvHeaderCache>("replies");
let pipeline = vec![
doc! { "$match": { "convid" :  &request.convid,
                   "boxid":request.boxid }} ,


doc!{ "$sort" : { 
    match request.sort() {
        replies::Sort::Latest => "created_at",
        replies::Sort::Popular => "score",
    } : match request.order() {
        replies::Order::Asc => i32::from(1),
        replies::Order::Desc => i32::from(-1),
    }

} },


doc!{ "$limit": i32::from(20) },

doc! { "$project": {
    "replyid": { "$toString": "$_id"},
    "reply":i32::from(20) ,
    "upvotes":i32::from(20) ,
    "downvotes":i32::from(20) ,
"pseudo":{"$cond": ["$anonym", "", "$pseudo"]},  
//     "visibility"  :   i32::from(1)
},

}  ];


let mut results = conversations.aggregate(pipeline, None).await.unwrap();

let mut reply_list :Vec<Reply> = vec![];
if let Some(result) = results.next().await {

let reply_header: Reply = bson::from_document(result.unwrap()).unwrap();


reply_list.push(reply_header);


} 

return Ok(Response::new(ReplyList{ reply_list }))


}



async fn delete_reply(&self,request:Request<common_types::AuthenticatedObjectRequest> ) ->  Result<Response<common_types::Empty> ,Status > {
    todo!()
}

   fn get_informations< 'life0, 'async_trait>(& 'life0 self,request:tonic::Request<common_types::AuthenticatedRequest> ,) ->  core::pin::Pin<Box<dyn core::future::Future<Output = Result<tonic::Response<settings::UserInformationsResponse> ,tonic::Status, > > + core::marker::Send+ 'async_trait> >where 'life0: 'async_trait,Self: 'async_trait {
        todo!()
    }

    fn change_informations< 'life0, 'async_trait>(& 'life0 self,request:tonic::Request<settings::UserInformations> ,) ->  core::pin::Pin<Box<dyn core::future::Future<Output = Result<tonic::Response<common_types::Empty> ,tonic::Status> > + core::marker::Send+ 'async_trait> >where 'life0: 'async_trait,Self: 'async_trait {
        todo!()
    }

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
 
    fn upvote_conv< 'life0, 'async_trait>(& 'life0 self,request:tonic::Request<common_types::AuthenticatedObjectRequest, > ,) ->  core::pin::Pin<Box<dyn core::future::Future<Output = Result<tonic::Response<common_types::Empty> ,tonic::Status> > + core::marker::Send+ 'async_trait> >where 'life0: 'async_trait,Self: 'async_trait {
        todo!()
    }

    fn downvote_conv< 'life0, 'async_trait>(& 'life0 self,request:tonic::Request<common_types::AuthenticatedObjectRequest, > ,) ->  core::pin::Pin<Box<dyn core::future::Future<Output = Result<tonic::Response<common_types::Empty> ,tonic::Status> > + core::marker::Send+ 'async_trait> >where 'life0: 'async_trait,Self: 'async_trait {
        todo!()
    }


/*
    async fn delete_account(& self,request:Request<common_types::AuthenticatedRequest> ,) -> Result<Response<common_types::Empty> ,tonic::Status >  {
    // /!\ delete account request -> delay until token expires -> delete (need refresh token function change)
     //todo:clean cache   

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

    let header_filter: mongodb::bson::Document = doc! {
        "screens":i32::from(1) ,};

let conversations = self.mongo_client.database("DB").collection::<crate::service::common_types::Empty>("users");
conversations.delete_one(
        doc! { "$match": {   "pseudo":&data.claims.pseudo}}
        , None ).await;

//delete after token exiration

//rename user convs
let conversations = self.mongo_client.database("DB").collection::<crate::service::common_types::Empty>("convs");
conversations.update_many(
        doc! { "$match": 
        {   "pseudo":&data.claims.pseudo}},
        doc!{"$set":{
            "pseudo":"d"
        }}
        , None ).await;

//rename user replies
let replies = self.mongo_client.database("DB").collection::<crate::service::common_types::Empty>("replies");
replies.find(
    doc! { "$match": 
    {   "pseudo":&data.claims.pseudo}},
 None ).await;
    


replies.update_many(
        doc! { "$match": 
        {   "pseudo":&data.claims.pseudo}},
        doc!{"$set":{
            "pseudo":"d"
        }}
        , None ).await;
        
//delete conv votes
let conv_votes = self.mongo_client.database("DB").collection::<crate::service::common_types::Empty>("conv_votes");
conv_votes.delete_many(
    doc! {
       "pseudo": &data.claims.pseudo,
    },
    None,
 ).await;

//delete conv votes
let conv_votes = self.mongo_client.database("DB").collection::<crate::service::common_types::Empty>("reply_votes");
conv_votes.delete_many(
    doc! {
       "pseudo": &data.claims.pseudo,
    },
    None,
 ).await;


     Ok(Response::new(common_types::Empty{}))

    }
*/


   fn downvote_reply< 'life0, 'async_trait>(& 'life0 self,request:tonic::Request<common_types::AuthenticatedObjectRequest, > ,) ->  core::pin::Pin<Box<dyn core::future::Future<Output = Result<tonic::Response<common_types::Empty> ,tonic::Status> > + core::marker::Send+ 'async_trait> >where 'life0: 'async_trait,Self: 'async_trait {
       // insert -> return err if already exists
        todo!()
    }


   fn upvote_reply< 'life0, 'async_trait>(& 'life0 self,request:tonic::Request<common_types::AuthenticatedObjectRequest, > ,) ->  core::pin::Pin<Box<dyn core::future::Future<Output = Result<tonic::Response<common_types::Empty> ,tonic::Status> > + core::marker::Send+ 'async_trait> >where 'life0: 'async_trait,Self: 'async_trait {
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

    fn get_notifications< 'life0, 'async_trait>(& 'life0 self,request:tonic::Request<notifications::GetNotificationsRequest> ,) ->  core::pin::Pin<Box<dyn core::future::Future<Output = Result<tonic::Response<notifications::NotificationsResponse> ,tonic::Status, > > + core::marker::Send+ 'async_trait> >where 'life0: 'async_trait,Self: 'async_trait {
        todo!()
    }

    fn update_wallet< 'life0, 'async_trait>(& 'life0 self,request:tonic::Request<common_types::AuthenticatedObjectRequest> ,) ->  core::pin::Pin<Box<dyn core::future::Future<Output = Result<tonic::Response<common_types::Empty> ,tonic::Status, > > + core::marker::Send+ 'async_trait> >where 'life0: 'async_trait,Self: 'async_trait {
        todo!()
    }


    async fn report(& self, request:Request<common_types::AuthenticatedRequest>,) ->  Result<Response<common_types::Empty>, tonic::Status>
    { todo!() }


    async fn get_balance(& self, request:Request<common_types::AuthenticatedRequest>,) ->  Result<Response<user::BalanceResponse>, tonic::Status>
    { todo!() }





    async fn buy_emergency(& self, request:Request<common_types::AuthenticatedRequest>,) ->  Result<Response<common_types::Empty>, tonic::Status>
    { todo!() }









}

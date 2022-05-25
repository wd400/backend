use base64ct::Base64UrlUnpadded;
use bson::oid::ObjectId;
use futures::StreamExt;
use async_recursion::async_recursion;

use redis::RedisError;
use serde::{Serialize, Deserialize};
use jsonwebtoken::{encode, decode, Header,  Validation, EncodingKey, DecodingKey};
use tonic::{Request, Response, Status};
use crate::{api::{*,common_types:: FileUploadResponse, conversation::{NewConvRequestResponse,ConvHeader,ConvHeaderList, ConversationComponent, conversation_component, ConvData, ConvDetails, ConvMetadata,   EmergencyConvHeader, EmergencyConvHeaderList, RawHeader}, search::{ SearchConvResponse, SearchUserResponse}, visibility::Visibility, replies::{ReplyRequestResponse,  Reply,   reply_request, VoteReply, ReplyList}, vote::VoteValue, user::BalanceResponse}, cache_init::{ get_epoch, TIMEFEEDTYPES, feed_type2seconds}};
use reqwest;
use stripe::Client as StripeClient;
use std::{collections::{HashMap, BTreeMap}, borrow::Borrow };
use sha2::{Sha256,  Digest};
use bb8_redis::{
    bb8::Pool,
    redis::cmd,
    RedisConnectionManager
};


//TODO SI LISTE SES CONVS PAS BESOIN DE FAIRE CERTAINS CHECKS
use regex::Regex;
use celes::Country;
use crate::service::login::LoginRequest;
use std::collections::HashSet;
use iso639_enum::{Language, IsoCompat};
use rand::{distributions::Alphanumeric, Rng}; // 0.8
use base64ct::{Base64, Encoding};
const BUF_SIZE: usize = 128;
use std::io::Cursor;
use image::{io::Reader as ImageReader, ImageFormat};

use mongodb::{Client as MongoClient, 
    options::{Acknowledgment, ReadConcern, TransactionOptions, WriteConcern,
        FindOptions, FindOneOptions,
          FindOneAndUpdateOptions, 
          FindOneAndDeleteOptions, 
           CountOptions, ReturnDocument,}, bson::{doc, Document}, Collection,
           ClientSession,
           error::{ Error as MongoError,TRANSIENT_TRANSACTION_ERROR, UNKNOWN_TRANSACTION_COMMIT_RESULT}};





#[derive(Debug, Serialize, Deserialize)]
struct ReplyVote {
    value:i32,
id:String,
convid:String,
}

#[derive(Debug, Serialize, Deserialize)]
struct ConvVote {
    value:i32,
    id:String
}

#[derive(Debug, Serialize, Deserialize)]
struct ObjId {
    _id:ObjectId
}

#[derive(Debug, Serialize, Deserialize)]
struct ProjReply {
    txtreply:String,
    upvote:i32,
    downvote:i32,
    created_at:u64,
    anonym:bool,
    boxid:i32,
    subreplies:i32
}

#[derive(Debug, Serialize, Deserialize)]
pub struct EmergencyEntry {
   pub convid: String,
   pub add_time:u64,
   pub amount:i32,
}

#[derive(Debug, Serialize, Deserialize)]
struct ReplyExtra {
    pub convid: String,
    pub boxid:i32
}

use aws_sdk_dynamodb::{Client as DynamoClient, model::{AttributeValue, ReturnValue}, types::{SdkError}, error::{ PutItemError,PutItemErrorKind}};
use aws_sdk_s3::{Client as S3Client, types::ByteStream};


#[derive(Debug, Serialize, Deserialize)]
struct ConvReplyScope {
    visibility: Visibility,
    box_ids: Vec<i32>,
    pseudo:String
}

#[derive(Debug, Serialize, Deserialize)]
struct MiniConvReplyScope {
    visibility: Visibility,
    pseudo:String
}

#[derive(Debug, Serialize, Deserialize)]
struct Vote {
    value: i32,
}


#[derive(Debug, Serialize, Deserialize)]
struct DeleteReplyProj {
    boxid:i32,
    convid:String,
    replyto:String
}

//convs replies
async fn execute_transaction(
    pseudo:&str,
    reply:&str,
    anonym:bool,
  //  replyto:&crate::service::reply_request::Origin,
    convs: &Collection<Document>, replies: &Collection<Document>, session: &mut ClientSession,origin:crate::service::reply_request::Origin) -> Result<Result<(String,String),MongoError>,tonic::Status> {
    
    //get conv visibility & boxids
    //if visibility & boxid correct
        //if replyfromid, count one replyfromid, convid 
        //append

  let (convid,boxid,replytoid) = match origin{
        reply_request::Origin::Root(root) => {

            let filter: mongodb::bson::Document = doc! {"pseudo":"$metadata.pseudo", "visibility":i32::from(1),"box_ids":i32::from(1) };
            let object_convid=bson::oid::ObjectId::parse_str(&root.convid).unwrap();
            let options = FindOneOptions::builder().projection(filter).build();

            match convs.find_one_with_session(doc!{"_id":object_convid}, options, session).await.unwrap() {
                Some(conv) => {
                    let conv:ConvReplyScope=bson::from_document(conv).unwrap();
                   if conv.box_ids.contains(&root.boxid)
                    {
                       if   legitimate(pseudo,
                    &CacheVisibility {
                     anonym:conv.visibility.anonym,
                     anon_hide:conv.visibility.anon_hide,
                     pseudo:conv.pseudo.clone()   
                    }) { 
                        (root.convid,root.boxid,String::from(""))
                    } else   {
                        return Err(Status::new(tonic::Code::InvalidArgument, "unavailable"))
                    }
                        
                   } else {
                    return Err(Status::new(tonic::Code::InvalidArgument, "invalid boxid"))
                   }
                },
                None => {
                    return Err(Status::new(tonic::Code::InvalidArgument, "conv not found"))
                },
            }

        },
        reply_request::Origin::Replyto(replyid) => {

            let filter: mongodb::bson::Document = doc! {"convid":i32::from(1),"boxid":i32::from(1)};

            let options = FindOneAndUpdateOptions::builder().projection(filter).build();


            match replies.find_one_and_update_with_session(
                doc!{"_id":bson::oid::ObjectId::parse_str(&replyid).unwrap()},
                doc!{"$inc":{"subreplies":1}},
                 options, session).await.unwrap() {
                Some(res) => {
                    let extra:ReplyExtra=bson::from_document(res).unwrap();
                   
                    let filter: mongodb::bson::Document = doc! {"pseudo":"$metadata.pseudo", "visibility":i32::from(1)};
                    let object_convid=bson::oid::ObjectId::parse_str(&extra.convid).unwrap();
                    let options = FindOneOptions::builder().projection(filter).build();
                    
                    match convs.find_one_with_session(doc!{"_id":object_convid}, options, session).await.unwrap() {
                        Some(conv) => {
                           let conv: MiniConvReplyScope=bson::from_document(conv).unwrap();
                            if   legitimate(pseudo,
                                &CacheVisibility {
                                 anonym:conv.visibility.anonym,
                                 anon_hide:conv.visibility.anon_hide,
                                 pseudo:conv.pseudo.clone()   
                                }) { 
                                    (extra.convid,extra.boxid,replyid)
                                } else {
                                    return Err(Status::new(tonic::Code::InvalidArgument, "unavailable"))
                                }
                        },
                        None =>{
                            return Err(Status::new(tonic::Code::InvalidArgument, "invalid conv"))
                        },
                    }
                },
                None => {
                    return Err(Status::new(tonic::Code::InvalidArgument, "reply not found"))
                }
            //get convid+boxid
            }
            
            
            //return visibility

        },
    };

   

let reply_id=match replies.insert_one_with_session(doc!{
    "pseudo":pseudo,
    "anonym":anonym,
    "txtreply":reply,
    "boxid":boxid,
    //change bjson
    "convid":&convid  ,
    "replyto":replytoid,
    "created_at":i64::try_from(get_epoch()).unwrap(),
    "upvote":i32::from(0),
    "downvote":i32::from(0),
    "score":i32::from(0),
    "subreplies":i32::from(0)
  //  "owner":&conv.pseudo

}, None, session).await {
    Ok(value) => {


        value.inserted_id.as_object_id().unwrap().to_hex()
    },
    Err(_) =>  return Err(Status::new(tonic::Code::InvalidArgument, "insert error")),
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
            Ok(_) => return Ok(Ok((convid,reply_id))),
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
    exp: u64,
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
    pub regex:Regex,
    pub stripe_client:StripeClient
 //   pub cache:Cache<String, String>,

}

// todo parse, to_string, verify


#[derive(Debug, Serialize, Deserialize)]
struct ScreensToDelete {
    screens:Vec<String>,
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






const SCREENSHOTS_BUCKET:&str="screenshots-s3-bucket";



//for cache
fn map2_conv_header(convid:&str,map:&BTreeMap<String, String>,vote:VoteValue)->ConvHeader {
    ConvHeader{
        vote:vote as i32,
        rawheader: Some(RawHeader{
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
                created_at: map.get("created_at").unwrap().parse::<u64>().unwrap()
            }),
        }),

    }
}

fn map2_cache_visibility(map:&BTreeMap<String, String>)->CacheVisibility {
    return CacheVisibility{
        anonym: map.get("anonym").unwrap().parse::<bool>().unwrap(),
        anon_hide:  map.get("anon_hide").unwrap().parse::<bool>().unwrap(),
        pseudo:map.get("pseudo").unwrap().to_owned()
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
    score:i32,
    box_ids:Vec<i32>,
    screens:Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct MongoUser{
    pseudo: String
}



#[derive(Debug, Serialize, Deserialize)]
struct ConvId{
    convid: String,
 //   owner:String
}

#[derive(Debug, Serialize, Deserialize)]
struct Replyto {
    replyto:String
}




#[derive(Debug, Serialize, Deserialize)]
struct CacheVisibility {
    anonym:bool,
    anon_hide:bool,
    pseudo:String
}

async fn get_conv_visibility(convid:&str,keydb_pool:&Pool<RedisConnectionManager>,mongo_client:&MongoClient)->Option<CacheVisibility> {

    let mut keydb_conn = keydb_pool.get().await.expect("keydb_pool failed");

    println!("CONVID {:#?}",convid);
    let cached:BTreeMap<String, String>=   cmd("hgetall")
                    .arg("V:".to_owned()+convid).query_async(&mut *keydb_conn).await.expect("hgetall failed");
        println!("cache: {:#?}",cached);
     
                //cache miss
                if (&cached).is_empty() {
                
                    let conversations = mongo_client.database("DB")
                    .collection::<CacheVisibility>("convs");


let filter: mongodb::bson::Document = doc! {
    "anonym":"$visibility.anonym",
    "anon_hide":"$visibility.anon_hide",
    "pseudo":"$metadata.pseudo"
 };
let options = FindOneOptions::builder().projection(filter).build();



let  result = conversations.find_one( 
    doc! { "_id" :  bson::oid::ObjectId::parse_str(convid).unwrap() },
     options).await.unwrap();

     match result {
        Some(value) => {
            let _:()=   cmd("hmset")
            .arg(&vec![
                 &("V:".to_owned()+   convid) as &str,
                    "anonym",&value.anonym.to_string(),
                    "anon_hide",&value.anon_hide.to_string(),
                    "pseudo",&value.pseudo
                    ] ).query_async(&mut *keydb_conn).await.unwrap();
            let _:()=   cmd("expire")
            .arg(&[convid,"3600"]).query_async(&mut *keydb_conn).await.unwrap();

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
    .arg(&[convid,"3600"]).query_async(&mut *keydb_conn).await.unwrap();

 return Some(map2_cache_visibility(&cached));

              
                }

}

async fn get_conv_header(pseudo:&str,convid:&str,keydb_pool:&Pool<RedisConnectionManager>,mongo_client:&MongoClient)->Option<ConvHeader>{

    
    let mut keydb_conn = keydb_pool.get().await.expect("keydb_pool failed");

    println!("CONVID {:#?}",convid);
    let cached:BTreeMap<String, String>=   cmd("hgetall")
                    .arg(convid).query_async(&mut *keydb_conn).await.expect("hgetall failed");
        println!("cache: {:#?}",cached);
     
                //cache miss
                if (&cached).is_empty() {
                
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
        "metadata.pseudo":{"$cond": ["$visibility.anonym", "", "$metadata.pseudo"]},  
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
   .arg(&[convid,"3600"]).query_async(&mut *keydb_conn).await.unwrap();


   let vote =   get_conv_vote(convid,pseudo,keydb_pool,mongo_client).await;


   return   Some(ConvHeader{ 
       rawheader:Some(RawHeader{ convid: convid.to_string(),
                     details: Some(conv_header.details),
                     metadata: Some(conv_header.metadata) ,
       }),
                     vote:vote as i32

                    } )
  

} else {
    //not found
    return None
}
 } //cache hit
  else {
                
    println!("before get_conv_header EXPIRE");
                let _:()=   cmd("expire")
                .arg(&[convid,"3600"]).query_async(&mut *keydb_conn).await.unwrap();

    println!("after get_conv_header EXPIRE");          
                let vote =  get_conv_vote(convid,pseudo,keydb_pool,mongo_client).await ;
                println!("after get_conv_header GETVOTE");    
                return Some(map2_conv_header(convid,&cached,vote));

              
                }
}

pub fn feed_type2cache_table(feed_type: &feed::FeedType)->&'static str{
    match feed_type {
        feed::FeedType::AllTime =>"AllTime",
        feed::FeedType::Emergency=>"Emergency",
        feed::FeedType::LastActivity=>"LastActivity",
        feed::FeedType::LastDay=>"LastDay",
        feed::FeedType::LastMonth=>"LastMonth",
        feed::FeedType::LastWeek=>"LastWeek",
        feed::FeedType::LastYear=>"LastYear",
        feed::FeedType::New=>"New",
      
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

    let re= Regex::new(r"^[a-z0-9]{4,13}$").unwrap();
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

  println!("lang {}",&details.language);
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
    screens:Vec<String>,
    box_ids: Vec<i32>
}

#[derive(Debug, Serialize, Deserialize)]
struct Value {
    value:i32,
}
#[derive(Debug, Serialize, Deserialize)]
struct Id {
    id : String
}

async fn tmp2pictures(screen_uri:&str,s3_client:&S3Client) {
    let mut source_bucket_and_object: String = "".to_owned();
    source_bucket_and_object.push_str(SCREENSHOTS_BUCKET);
    source_bucket_and_object.push_str("/tmp/");
    source_bucket_and_object.push_str(&screen_uri);

 s3_client.copy_object()
  .bucket(SCREENSHOTS_BUCKET)
  .copy_source(source_bucket_and_object)
    .key("pictures/".to_string()+&screen_uri+".jpg")
      .send()
      .await.unwrap();
    
s3_client.delete_object()
.bucket(SCREENSHOTS_BUCKET)
.key("tmp/".to_string()+&screen_uri).send().await.unwrap();
}

fn legitimate(pseudo:&str,visibility:&CacheVisibility)->bool{
    if visibility.anon_hide && pseudo=="" {
        return false
    }
    if pseudo==visibility.pseudo{
        return true
    }
    
    return true
}


async fn update_cache(convid:&str,incr:i32,keydb_pool:&Pool<RedisConnectionManager>){
    let mut keydb_conn = keydb_pool.get().await.expect("keydb_pool failed");

    for feed_type in &TIMEFEEDTYPES {
        let cache_table=feed_type2cache_table(feed_type);
        
        println!("update: {:#?}",cache_table);

        //maybe use TTL instead
        // if TTL>10s=>update
        
         let result:i32=   cmd("eval")
            .arg("if redis.call('zscore',KEYS[1],KEYS[2]) == nil then return 1 else redis.call('zincrby', KEYS[1], KEYS[3], KEYS[2]) return 0 end")
            .arg::<i32>(3).arg(cache_table).arg(convid).arg(incr).query_async(&mut *keydb_conn).await.expect("zincrby error");
            println!("cache score update {}",result);
        if result ==1 {
            break;
        }

//      println!("{:#?}",res);
    
let _:()=   cmd("zincrby")
.arg(feed_type2cache_table(&feed::FeedType::AllTime)).arg(
    incr).arg(
    convid ).query_async(&mut *keydb_conn).await.expect("zincrby error");



}} 

async fn del_conv_from_cache(convid:&str,keydb_pool:&Pool<RedisConnectionManager>){
    let mut keydb_conn = keydb_pool.get().await.expect("keydb_pool failed");

    for feed_type in &TIMEFEEDTYPES {
        let cache_table=feed_type2cache_table(feed_type);
        


         let _:()=   cmd("zrem")
            .arg(cache_table).arg(convid).query_async(&mut *keydb_conn).await.expect("zrem error");



//      println!("{:#?}",res);


}


let _:()=   cmd("zrem")
.arg(feed_type2cache_table(&feed::FeedType::New)).arg(
    convid ).query_async(&mut *keydb_conn).await.expect("zrem error");


let _:()=   cmd("zrem")
.arg(feed_type2cache_table(&feed::FeedType::LastActivity)).arg(
    convid ).query_async(&mut *keydb_conn).await.expect("zrem error");


let _:()=   cmd("zrem")
.arg(feed_type2cache_table(&feed::FeedType::AllTime)).arg(
    convid ).query_async(&mut *keydb_conn).await.expect("zrem error");

    let _:()=   cmd("zrem")
    .arg(feed_type2cache_table(&feed::FeedType::Emergency)).arg(
        convid ).query_async(&mut *keydb_conn).await.expect("zrem error");
    

        let _:()=   cmd("rem").arg(
            convid ).query_async(&mut *keydb_conn).await.expect("zrem error");
        

} 


fn vote_value2_shift(vote:VoteValue)-> i32 {
    match vote{
        VoteValue::Upvote => 1,
        VoteValue::Downvote => -1,
        VoteValue::Neutral => 0,
    }
}

fn shift2_vote_value(shift:i32)-> VoteValue {
    if shift==1{
        return VoteValue::Upvote
    } else if shift ==-1 {
        return VoteValue::Downvote
    } else if shift == 0 {
        return VoteValue::Neutral
    }
    panic!("wrong shift")
}


async fn update_metadata(table:&str, id:&str , newvote:i32,oldvote:i32,mongo_client:&MongoClient){

    if newvote!=oldvote{
    
        let mongotable = mongo_client.database("DB")
        .collection::<Document>(table);



        let upincr=(newvote==1 && oldvote!=1) as i32 - (newvote!=1 && oldvote==1) as i32 ;
        let downincr=(newvote==-1 && oldvote!=-1) as i32-(newvote!=-1 && oldvote==-1) as i32 ;
        //convid replyid
       // println!( "upincr:{} downincr:{}",upincr,downincr);
        if table=="convs"{
          mongotable.update_one(
        doc! { "_id":bson::oid::ObjectId::parse_str(id).unwrap()}
        , doc!
            {"$inc":{"metadata.upvote":upincr,"metadata.downvote":downincr,"score":upincr-downincr}}

        ,
        None ).await.unwrap() ;

   } else if table=="replies"{

    mongotable.update_one(
        doc! { "_id":bson::oid::ObjectId::parse_str(id).unwrap()}
        , doc!
            {"$inc":{"upvote":upincr,"downvote":downincr,"score":upincr-downincr}}

        ,
        None ).await.unwrap() ;

   }
    }
}

#[async_recursion]
async fn delete_replies_recursive(convid:&str,boxid:i32,replyid:String, mongo_client:&MongoClient){

    println!("delete_replies_recursive {}",&replyid);

    let votes = mongo_client.database("DB").collection::<Document>("reply_votes");

    votes.delete_many(
        doc! {   "convid":convid,"boxid":boxid,"id":&replyid},
        None ).await.unwrap();

    //get reply
    let options=FindOptions::builder().projection(doc!{
        "_id":i32::from(1),
    }).build();
    let replies = mongo_client.database("DB").collection::<ObjId>("replies");
let mut list=replies.find(
    doc! {   "convid":convid,"boxid":boxid,"replyto":&replyid},
 options ).await.unwrap();


while let Some(result) = list.next().await {
    //delete
    match result {
        Ok(reply) => {



            replies.find_one_and_delete(
                doc! { "_id": reply._id}
                , None ).await.unwrap() ;

            let newid=reply._id.to_hex();



                delete_replies_recursive(convid,boxid,newid,mongo_client).await;
        },
        Err(err) => {

            println!("{}",err);
        },
    }

    //recursive
}

}


async fn get_conv_vote(convid:&str,pseudo:&str,keydb_pool:&Pool<RedisConnectionManager>, mongo_client:&MongoClient)->VoteValue{


if pseudo.is_empty(){
    return VoteValue::Neutral
}
    
    let mut keydb_conn = keydb_pool.get().await.expect("keydb_pool failed");

    let key=String::from(convid)+pseudo;
    
    let cached:Result<String,RedisError>=   cmd("get")
                    .arg(&key).query_async(&mut *keydb_conn).await;
        println!("cache: {:#?}",cached);
     
                //cache miss
        match cached {
            //cache miss
            Err(_)    => {
                let filter: mongodb::bson::Document = doc! { "value":i32::from(1) };
                let options = FindOneOptions::builder().projection(filter).build();
                
                let users = mongo_client.database("DB")
                .collection::<Value>("conv_votes");
                
                
                let  cursor = users.find_one(doc!{
                    "pseudo":pseudo,"id":convid
                },
                    options
                //  FindOneOptions::default() 
                    ).await.unwrap();
                
             let vote=   if let Some(result) = cursor {
                 result.value
                
                } else {
                    0
                 };

             
                
                   let _:()=   cmd("set")
                   .arg(key).arg(vote).arg("EX").arg(10).query_async(&mut *keydb_conn).await.unwrap();
                
                
                   return   shift2_vote_value(vote) 
                  
                
                
                 } //cache hit,
            //cache hit
            Ok(cached)    => {
                
                
                    let _:()=   cmd("expire")
                    .arg(key).arg(3600).query_async(&mut *keydb_conn).await.unwrap();
               
                    //my_string.parse::<i32>().unwrap();
                    return shift2_vote_value(cached.parse::<i32>().unwrap()) 
    
                  
                    
            },
        }
                 
 


}

#[tonic::async_trait]
impl v1::api_server::Api for MyApi {

    async fn login(&self, request: Request<login::LoginRequest>) -> Result<Response<login::LoginResponse>, Status> {
     //   println!("Got a request: {:#?}", &request);
        let request:&LoginRequest=request.get_ref();
        println!("{:#?}",request);


        let third_party = request.third_party();
     //   let client_type=request.client_type();
        
        let userid:String =
         if third_party==login::ThirdParty::Facebook {
        //    return Err(Status::new(tonic::Code::InvalidArgument, "not yet implemented"));

                    // https://developers.google.com/identity/protocols/oauth2/openid-connect#exchangecode
                    let client = reqwest::Client::new();


                   // println!("access_token {:#?}",access_token);
                    let facebook_request = client.get("https://graph.facebook.com/me")
                    .query(
                        &[
                            //safe? optimal?
                            ("fields","id"),
                            ("access_token", &request.access_token)

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
            


            let token_infos=client.get("https://oauth2.googleapis.com/tokeninfo?")
            .query(&[("access_token",&request.access_token)])
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
    

    let  cursor = users.find_one(doc!{
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
                    exp:get_epoch()+60*60,
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
        


        /*

        if  request.pseudo.len() < 4 || request.pseudo.len() > 13 {
            return Err(Status::new(tonic::Code::InvalidArgument, "invalid pseudo size"))
        }
        */

       let re= Regex::new(r"^[a-z0-9_-]{4,20}$").unwrap();



        if ! re.is_match(&request.pseudo) {
            return Err(Status::new(tonic::Code::InvalidArgument, "invalid pseudo char"))
        }


      if self.regex.is_match(&request.pseudo) {
        return Err(Status::new(tonic::Code::InvalidArgument, "invalid substring"))
        }

      //  check valid  genre
       if Country::from_alpha2(&request.country).is_err(){
        return Err(Status::new(tonic::Code::InvalidArgument, "invalid country"))
       }

       /*
       if request.date_of_birth<=0 {
        return Err(Status::new(tonic::Code::InvalidArgument, "invalid date_of_birth"))
       }
       */


        let options = CountOptions::builder().limit(1).build();

        let banned:Collection<crate::service::common_types::Empty> = self.mongo_client.database("DB")
        .collection("banned");


        match banned.count_documents(
            doc!{
"pseudo": &request.pseudo
            } , options).await {
    Ok(count) => {
        if count>0 {
            return Err(Status::new(tonic::Code::InvalidArgument, "banned pseudo"))   
        }

    },
    Err(_) => {
        return Err(Status::new(tonic::Code::InvalidArgument,"db error"))
    },
}



        let users = self.mongo_client.database("DB")
        .collection("users");
       


        //check valid userid,
       if let Err(_) = users.insert_one(
           doc!{
                "pseudo": &request.pseudo,
                "userid": &data.claims.userid,
                /*
                "genre": 
                match genre2str(request.genre()) {
                    Some(value)=>value,
                    None=> {
                        return Err(Status::new(tonic::Code::InvalidArgument, "invalid genre"))
                    }
                }                
                
                ,
                
                "date_of_birth":i64::try_from(request.date_of_birth).unwrap(),
                */
                "country":&request.country.to_lowercase(),
            "created_at":i64::try_from(get_epoch()).unwrap() }
            ,None).await {
               return Err(Status::new(tonic::Code::InvalidArgument,"db error"))
}




        //ConditionalCheckFailedException
               let res = self.dynamodb_client.put_item()
               .table_name("users")
               .item("pseudo",AttributeValue::S(request.pseudo.to_string()))
               .item("balance",AttributeValue::N(String::from("0")))
          //     .item("openid",AttributeValue::S(open_id.to_string()))
               .condition_expression("attribute_not_exists(balance)")
               .return_values(ReturnValue::AllOld).send().await;
       
            match res {
               Err(SdkError::ServiceError {
                   err:
                       PutItemError {
                           kind: PutItemErrorKind::ConditionalCheckFailedException(_),
                           ..
                       },
                   raw: _,
               }) => {
                return Err(Status::new(tonic::Code::InvalidArgument, "duplicate user"))
               },
               Ok(_)=>{
                   ()
               },
               _ => {return Err(Status::new(tonic::Code::InvalidArgument, "db error"))}
             }


            //TODO: stripe create account
            let customer_params = stripe::CreateCustomer::new();
       match   stripe::Customer::create(&self.stripe_client,customer_params ).await{
             
            Ok(val)=>{

               match  users.update_one(
                    doc!  {"pseudo":&request.pseudo}
                    , doc!
                        {"$set":{"customerid":val.id.as_str()}}
            
                    ,
                    None ).await {
    Ok(value) => {

       if value.matched_count==1 {
       
        let payload: JWTPayload = JWTPayload {
            exp:get_epoch()+60*60*24*60,
            pseudo:request.pseudo.clone() };

        let token = encode(&Header::default(), &payload, &self.jwt_encoding_key).expect("INVALID TOKEN");

          return   Ok(Response::new(common_types::RefreshToken{
                access_token:token,
            }))
        } else {
            return Err(Status::new(tonic::Code::InvalidArgument, "db error"))
        }
 
    },
    Err(err) => {
        println!("{:#?}",err);
        //todo rollback
        return Err(Status::new(tonic::Code::InvalidArgument, "internal error"))

    },
}},
             Err(_)=>{
                 //TODO rollback
                return Err(Status::new(tonic::Code::InvalidArgument, "stripe error"))

             }

        

         }
    
    


            }

    async fn refresh_token(&self,request:Request<common_types::AuthenticatedRequest>) ->  Result<Response<common_types::RefreshToken>, Status> {
        
        let request=request.get_ref();
        let data=decode::<JWTPayload>(&request.access_token,&self.jwt_decoding_key,&self.jwt_algo);
        let data=match data {
            Ok(data)=>data,
            _=>{ return Err(Status::new(tonic::Code::InvalidArgument, "invalid token"))}
        };

        
        let payload: JWTPayload = JWTPayload {
            exp:(get_epoch()+60*60*24*30) ,
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

        let pseudo = if !(&request.access_token).is_empty(){

            match decode::<JWTPayload>(&request.access_token,&self.jwt_decoding_key,&self.jwt_algo) {
                 Ok(data)=>data.claims.pseudo.to_owned(),
                 _=>{ return Err(Status::new(tonic::Code::InvalidArgument, "invalid token"))}
        
             } } else {
     String::from("")
         };

        let cache_table_name=  feed_type2cache_table(&request.feed_type());
        
        let offset=request.offset;
        if offset<0 {
          return   Err(Status::new(tonic::Code::InvalidArgument, "invalid offset"))
        }
        


        let  conn = self.keydb_pool.get().await;
        let mut conn = match conn {
            Ok(conn)=>conn,
            Err(_)=>return   Err(Status::new(tonic::Code::InvalidArgument, "cache connection error"))
        
        };
        //<Vec<(String, isize)> , "withscores"
        let reply:Result<Vec<String>, RedisError>= cmd("zrevrange")
        .arg(cache_table_name).arg(
            &offset).arg(
            offset+20).query_async(&mut *conn).await;

       
        let reply = match reply {
            Ok(reply)=>reply,
            Err(_)=>return   Err(Status::new(tonic::Code::InvalidArgument, "cache error"))
        
        };
        println!("reply::: {:#?}",reply);
        let mut replylist:Vec<ConvHeader>=vec![];
        for convid in reply {
let header= get_conv_header(&pseudo,&convid,&self.keydb_pool,&self.mongo_client).await;

let visibility=get_conv_visibility(&convid,&self.keydb_pool,&self.mongo_client).await;

match header
{
    Some(value) => match visibility  {
        Some(visib) => {

            if legitimate(&pseudo,&visib)  {
                
                println!("PUSH {:#?}",value);
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

    async fn emergency_feed(&self,request:Request<feed::EmergencyFeedRequest>)-> Result<Response<conversation::EmergencyConvHeaderList>,Status> {
      
        let request=request.get_ref();

        let pseudo = if !(&request.access_token).is_empty(){

            match decode::<JWTPayload>(&request.access_token,&self.jwt_decoding_key,&self.jwt_algo) {
                 Ok(data)=>data.claims.pseudo.to_owned(),
                 _=>{ return Err(Status::new(tonic::Code::InvalidArgument, "invalid token"))}
        
             } } else {
     String::from("")
         };


        
        let offset=request.offset;
        if offset<0 {
          return   Err(Status::new(tonic::Code::InvalidArgument, "invalid offset"))
        }
        


        let  conn = self.keydb_pool.get().await;
        let mut conn = match conn {
            Ok(conn)=>conn,
            Err(_)=>return   Err(Status::new(tonic::Code::InvalidArgument, "cache connection error"))
        
        };
        //<Vec<(String, isize)> , "withscores"
        let reply:Result<Vec<(String,i32)>, RedisError>= cmd("zrevrange")
        .arg(feed_type2cache_table(&feed::FeedType::Emergency)).arg(
            &offset).arg(
            offset+20).arg("WITHSCORES").query_async(&mut *conn).await;


       
        let reply = match reply {
            Ok(reply)=>reply,
            Err(_)=>return   Err(Status::new(tonic::Code::InvalidArgument, "cache error"))
        
        };
        println!("reply::: {:#?}",reply);
        let mut conv_list:Vec<EmergencyConvHeader>=vec![];
        for (convid,score) in reply {

         let ttl:i64=   match cmd("TTL")
         .arg(feed_type2cache_table(&feed::FeedType::Emergency)).arg(&convid).query_async(&mut *conn).await {
    Ok(ttl) => {
        ttl
    },
    Err(_) => {
        continue
    },
};
    if ttl<=0{
        continue
    }
     

          //  let convid=&convid_timestamp[..24];
         //   let timestamp=&convid_timestamp[24..];

let header= get_conv_header(&pseudo,&convid,&self.keydb_pool,&self.mongo_client).await;
let visibility=get_conv_visibility(&convid,&self.keydb_pool,&self.mongo_client).await;
match header
{
    Some(value) => match visibility  {
        Some(visib) => {

            if legitimate(&pseudo,&visib)  {
                conv_list.push(
                    EmergencyConvHeader{
conv_header:Some(value),
price:score,
add_time: ttl as u64,
                    }
                    )
            }
        },
        None=>(),

    } ,
    None => (),
}
        }
        return  Ok(Response::new(EmergencyConvHeaderList{ convheaders: conv_list }))
    }
 
    async fn search_user(&self,request:Request<search::SearchUserRequest> ) ->  Result<Response<search::SearchUserResponse> ,Status > {
        let request=request.get_ref();

//search 

let mut pseudo_list:Vec<String> = vec![];

let users = self.mongo_client.database("DB")
.collection::<Pseudo>("users");
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
let mut results = users.aggregate(pipeline, None).await.unwrap();

while let Some(result) = results.next().await {
    let bson_pseudo: Pseudo = bson::from_document(result.unwrap()).unwrap();
    pseudo_list.push(bson_pseudo.pseudo);
    }


    
Ok(Response::new(SearchUserResponse{ pseudos: pseudo_list }))

    }

    async fn search_conv(&self,request:Request<search::SearchConvRequest> ) ->  Result<Response<search::SearchConvResponse> ,Status > {

        let request=request.get_ref();

        let pseudo = if !(&request.access_token).is_empty(){

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
.collection::<RawHeader>("convs");
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
    "metadata.pseudo":{"$cond": ["$visibility.anonym", "", "$metadata.pseudo"]},
}}


];
let mut results = conversations.aggregate(pipeline, None).await.unwrap();

while let Some(result) = results.next().await {
    let conv_header: RawHeader = bson::from_document(result.unwrap()).unwrap();


match get_conv_visibility(&conv_header.convid,&self.keydb_pool,&self.mongo_client).await {
        Some(visib) => {

            if legitimate(&pseudo,&visib) {
                let vote=  get_conv_vote(&conv_header.convid,&pseudo,&self.keydb_pool,&self.mongo_client).await as i32;
                header_list.push(ConvHeader{rawheader:Some(conv_header),vote:vote});
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
       
        let  file:&Vec<u8>= request.file.borrow();
       
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
        let pseudo = if !(&request.access_token).is_empty(){

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
    "metadata.pseudo": {"$cond": ["$visibility.anonym", {"$cond": [{"$eq": [ "$metadata.pseudo",&pseudo ]}, "$metadata.pseudo", ""]}, "$metadata.pseudo"]},
}
}
]

,None).await.unwrap();




if let Some(result) = results.next().await {
    println!("{:#?}",result);
    let result:FullConv = bson::from_document(result.unwrap()).unwrap();
//println!("{:#?}", conv_header);
if legitimate(&pseudo,
    &CacheVisibility{
         anonym: result.visibility.anonym, 
         anon_hide: result.visibility.anon_hide,
          pseudo:result.metadata.pseudo.to_string() }
   

) {

    Ok(Response::new(ConvData{
        details: Some(result.details),
         metadata: Some(result.metadata), 
         flow: result.flow,
         vote: (get_conv_vote(&request.id,&pseudo,&self.keydb_pool,&self.mongo_client).await)  as i32
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
    .collection::<Visibility>("convs");
    

    let  cursor = users.find_one(doc!{
        "_id":&bson::oid::ObjectId::parse_str(&request.id).unwrap(),
        "metadata.pseudo": &data.claims.pseudo
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
    .collection::<Document>("convs");
    
    
    
    
    let convid=&bson::oid::ObjectId::parse_str(&request.convid).unwrap();
    
   match conversations.find_one_and_update(
    
        doc! { "_id": convid,
               "metadata.pseudo": &data.claims.pseudo
             },
        doc! { "$set": {"visibility":bson::to_bson(&request.visibility.as_ref().unwrap()).unwrap() }},
    
        None
    ).await {
    Ok(value) => {
        match value {
    Some(_) => {
        
    },
    None => {
        return Err(Status::new(tonic::Code::NotFound, "conv not found for user"))  
    },
    }
        
    },
    Err(err) => {
        println!("err {:#?}",err);
        return Err(Status::new(tonic::Code::InvalidArgument, "db err"))
    },
    }
    
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
                    created_at:current_timestamp ,
                    pseudo:data.claims.pseudo

                },
            score:0,
        box_ids:conv_check.box_ids.into_iter().collect(),
        screens:conv_check.screens_uri.into_iter().collect()
         }


            ,None).await.unwrap().inserted_id.as_object_id().unwrap().to_hex() ;
            
        
        //if not private: keydb: add to all feeds(including new activities)
//if !request.private {



let mut keydb_conn = self.keydb_pool.get().await.expect("keydb_pool failed");


for feed_type in &TIMEFEEDTYPES {
    let expiration = current_timestamp+ feed_type2seconds(feed_type);
    let cache_table=feed_type2cache_table(feed_type);
    let _:()=   cmd("zadd")
        .arg(cache_table).arg(
            0).arg(
 &convid  ).query_async(&mut *keydb_conn).await.expect("zadd error");

 println!("expiration {:#?}",expiration);

   let _:()= cmd("expirememberat")
    .arg(cache_table).arg(
        &convid).arg(
expiration).query_async(&mut *keydb_conn).await.expect("expirememberat error");
//      println!("{:#?}",res);
  }

//ALLTIME
  let _:()=   cmd("zadd")
  .arg(feed_type2cache_table(&feed::FeedType::AllTime)).arg(
    0).arg(
&convid  ).query_async(&mut *keydb_conn).await.expect("zadd error");

let timestamp_str=current_timestamp.to_string();

//NEW
  let _:()=   cmd("zadd")
  .arg(
    feed_type2cache_table(&feed::FeedType::New)).arg(
  &timestamp_str).arg(
&convid  ).query_async(&mut *keydb_conn).await.expect("zadd error");

//LASTACTIVITY
let _:()=   cmd("zadd")
.arg(
    feed_type2cache_table(&feed::FeedType::LastActivity)).arg(
&timestamp_str).arg(
&convid  ).query_async(&mut *keydb_conn).await.expect("zadd error");
//}

        return  Ok(Response::new(NewConvRequestResponse { convid: convid }))
      
    }

    async fn delete_conv(&self,request:Request<common_types::AuthenticatedObjectRequest> ) ->  Result<Response<common_types::Empty> ,Status > {
        // /!\ if create and delete juste before create ends, => screens may not
          let request=request.get_ref();
          
          let data=decode::<JWTPayload>(&request.access_token,&self.jwt_decoding_key,&self.jwt_algo);
          let data=match data {
              Ok(data)=>data,
              _=>{ return Err(Status::new(tonic::Code::InvalidArgument, "invalid token"))}
          };
  

          let convid= bson::oid::ObjectId::parse_str(&request.id).unwrap();
  
  
          let conversations = self.mongo_client.database("DB")
          .collection::<ScreensToDelete>("convs");
          //::<ConvHeader>
  
          let header_filter: mongodb::bson::Document = doc! {
              "screens":i32::from(1) ,};
          let options = FindOneAndDeleteOptions::builder().projection(header_filter).build();
      
  
      match  conversations.find_one_and_delete(
              doc! 
              { "_id" : &convid ,
                  "metadata.pseudo":&data.claims.pseudo}
              , options ).await {
      Ok(value) => {
          match value {
      Some(value) => {




//CLEAN CACHE
del_conv_from_cache(&request.id,&self.keydb_pool).await;


         for screen in &value.screens {
  
          self.s3_client.delete_object().bucket(SCREENSHOTS_BUCKET)
          .key("pictures/".to_string()+screen).send().await.unwrap();
  
         }
    
         let replies = self.mongo_client.database("DB").collection::<crate::service::common_types::Empty>("replies");
         replies.delete_many( doc! {  "conv" : convid}  , None ).await.unwrap();
    
         let votes = self.mongo_client.database("DB").collection::<crate::service::common_types::Empty>("votes");
         votes.delete_many( doc! {  "conv" : convid}  , None ).await.unwrap();

         let votes = self.mongo_client.database("DB").collection::<crate::service::common_types::Empty>("conv_votes");
         votes.delete_many( doc! {  "id" : convid}  , None ).await.unwrap();      

         let votes = self.mongo_client.database("DB").collection::<crate::service::common_types::Empty>("reply_votes");
         votes.delete_many( doc! {  "convid" : convid}  , None ).await.unwrap();  
  
             
    //invalidate cache
let mut keydb_conn = self.keydb_pool.get().await.expect("keydb_pool failed");
let _:()=cmd("unlink").arg("V:".to_owned()+&request.id).query_async(&mut *keydb_conn).await.expect("unlink failed");


         Ok(Response::new(common_types::Empty{}))
      },
      None => {
          //not found
          return Err(Status::new(tonic::Code::InvalidArgument, "conv not found"))
      },
  }
      },
      Err(err) => {
          println!("{:#?}",err);
          return Err(Status::new(tonic::Code::InvalidArgument, "db error 1"))
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
.collection::<Document>("convs");




let convid=&bson::oid::ObjectId::parse_str(&request.convid).unwrap();

match conversations.find_one_and_update(

    doc! { "_id": convid,
           "metadata.pseudo": &data.claims.pseudo
         },
    doc! { "$set": {"details":bson::to_bson(&request.details.as_ref().unwrap()).unwrap() }},

    None
).await {
Ok(value) => {
    match value {
Some(_) => {
},
None => {
    return Err(Status::new(tonic::Code::NotFound, "conv not found for user"))  
},
}
    
},
Err(_) => {
    return Err(Status::new(tonic::Code::InvalidArgument, "db err"))
},
}




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
               "metadata.pseudo": &data.claims.pseudo
             },
        doc! { "$set": {"flow":bson::to_bson(&request.flow).unwrap(),
    "box_ids":bson::to_bson(&conv_check.box_ids).unwrap(),
    "screens":bson::to_bson(&conv_check.screens_uri).unwrap() }},

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
    Err(err) => {
        println!("{:#?}",err);
        return Err(Status::new(tonic::Code::InvalidArgument, "db err"))
    },
};

//
let mut pics_to_delete:Vec<&String> = vec![];

for old_screen in &previous_ressources.screens  {
    if ! conv_check.screens_uri.remove(old_screen) {
        pics_to_delete.push(old_screen);
    }
  //  if new_screen in 
}

for pic_to_delete in &pics_to_delete {
    println!("{:#?}",  self.s3_client.delete_object().bucket(SCREENSHOTS_BUCKET)
    .key("static/pictues/".to_string()+&pic_to_delete).send().await);
}

for pic_to_move in &conv_check.screens_uri {
  
    tmp2pictures(pic_to_move,&self.s3_client).await;
}

let replies:Collection<crate::service::common_types::Empty> = self.mongo_client.database("DB")
.collection("replies");


let reply_votes:Collection<crate::service::common_types::Empty> = self.mongo_client.database("DB")
.collection("reply_votes");

for previous_box in &previous_ressources.box_ids {

    if ! conv_check.box_ids.contains(previous_box)   {

        let delete_result = replies.delete_many(
    doc! {
       "convid": convid,
       "boxid":previous_box
    },
    None,
 ).await;
println!("del result {:#?}",delete_result);

//delete votes
reply_votes.delete_many(
    doc! {
       "convid": convid,
       "boxid":previous_box
    },
    None,
 ).await.unwrap();

    }
}

//invalidate cache
let mut keydb_conn = self.keydb_pool.get().await.expect("keydb_pool failed");
let _:()=cmd("unlink").arg(&request.convid).query_async(&mut *keydb_conn).await.expect("unlink failed");


let _:()=   cmd("zadd")
.arg(feed_type2cache_table(&feed::FeedType::LastActivity)).arg(
get_epoch()).arg(
&request.convid ).query_async(&mut *keydb_conn).await.expect("zadd error");

Ok(Response::new(common_types::Empty{}))
}

    async fn submit_reply(&self,request:Request<replies::ReplyRequest, > ,) ->  Result<tonic::Response<replies::ReplyRequestResponse> ,Status> {

    let request=request.get_ref();
        
    let data=decode::<JWTPayload>(&request.access_token,&self.jwt_decoding_key,&self.jwt_algo);
    let data=match data {
        Ok(data)=>data,
        _=>{ return Err(Status::new(tonic::Code::InvalidArgument, "invalid token"))}
    };

    if request.reply.len()<3 {
        return Err(Status::new(tonic::Code::InvalidArgument, "invalid reply"))
    }

    let convs = self.mongo_client.database("DB")
    .collection::<Document>("convs");

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
    &data.claims.pseudo, &request.reply,request.anonym,

//   &request.origin.as_ref().unwrap(),
   
   &convs,&replies,&mut session,request.origin.as_ref().unwrap().to_owned()).await;
    
        match result{
    Ok(value) => {
match value {
    Ok((convid,replyid)) => {

        let mut keydb_conn = self.keydb_pool.get().await.expect("keydb_pool failed");


        let _:()=   cmd("zadd")
.arg(&[feed_type2cache_table(&feed::FeedType::LastActivity),
&get_epoch().to_string(),
&convid ]).query_async(&mut *keydb_conn).await.expect("zadd error");

        return Ok(Response::new(ReplyRequestResponse{ replyid: replyid }))
    
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
    let pseudo = if !(&request.access_token).is_empty(){
    let data=decode::<JWTPayload>(&request.access_token,&self.jwt_decoding_key,&self.jwt_algo);
    match data {
        Ok(data)=>data.claims.pseudo,
        _=>{ return Err(Status::new(tonic::Code::InvalidArgument, "invalid token"))}
        
    }
} else {
    String::from("")
};

    let visibility=get_conv_visibility(&request.convid,& self.keydb_pool, &self.mongo_client).await;
    match visibility {
        Some(vis)=> {
            if !legitimate(&pseudo,&vis){
                return Err(Status::new(tonic::Code::InvalidArgument, "invalid conv"))
            }
        },
        None=> return Err(Status::new(tonic::Code::InvalidArgument, "invalid conv"))
    }




    //    if replyto=="" { "_" }  else  { replyto}

    let offset=request.offset;
    if offset<0 {
      return   Err(Status::new(tonic::Code::InvalidArgument, "invalid offset"))
    }
    
    let replies = self.mongo_client.database("DB")
    .collection::<Document>("replies");
let pipeline = vec![
doc! { "$match": { "convid" :  &request.convid,
                   "boxid":request.boxid,
                   "replyto":&request.replyid


                 }} ,


doc!{ "$sort" : { 
    match request.sort() {
        replies::Sort::Latest => "created_at",
        replies::Sort::Popular => "score",
    } : match request.order() {
        replies::Order::Asc => i32::from(1),
        replies::Order::Desc => i32::from(-1),
    }

} },

doc!{ "$skip" : offset },
doc!{ "$limit": i32::from(20) },

doc! { "$project": {
    "replyid": { "$toString": "$_id"},
    "txtreply":i32::from(20) ,
    "upvote":i32::from(20) ,
    "downvote":i32::from(20) ,
    "created_at":i32::from(20) ,
"pseudo":{"$cond": ["$anonym", "", "$pseudo"]},  
//     "visibility"  :   i32::from(1)
//todo optim
"convid":i32::from(20),
"boxid":i32::from(20)
},

}  ];


let mut results = replies.aggregate(pipeline, None).await.unwrap();

let votes = self.mongo_client.database("DB")
.collection::<Vote>("reply_votes");

let mut reply_list :Vec<VoteReply> = vec![];
while let Some(result) = results.next().await {


let reply_header: Reply = bson::from_document(result.unwrap()).unwrap();

let vote=if (&pseudo).is_empty(){
    VoteValue::Neutral
} else {
let filter: mongodb::bson::Document = doc! { "value":i32::from(1) ,};
let options = FindOneOptions::builder().projection(filter).build();
match   votes.find_one(
    doc! { "id":&reply_header.replyid,  "pseudo":&pseudo}
    , options ).await  {
    Ok(vote) => {match vote {
    Some(value) => shift2_vote_value(value.value),
    None =>VoteValue::Neutral,}},
    Err(_) => continue,}};
println!("VOTE {:#?}",vote as i32);
reply_list.push(
    VoteReply{
        reply: Some(reply_header),
        vote: vote as i32 }
    
);


} 

return Ok(Response::new(ReplyList{ reply_list }))


}

    async fn delete_reply(&self,request:Request<common_types::AuthenticatedObjectRequest> ) ->  Result<Response<common_types::Empty> ,Status > {
    let request=request.get_ref();
          
    let data=decode::<JWTPayload>(&request.access_token,&self.jwt_decoding_key,&self.jwt_algo);
    let data=match data {
        Ok(data)=>data,
        _=>{ return Err(Status::new(tonic::Code::InvalidArgument, "invalid token"))}
    };

    //convid, boxid
    let replies = self.mongo_client.database("DB").collection::<DeleteReplyProj>("replies");


    let filter: mongodb::bson::Document = doc! {
        "convid":i32::from(1),
        "boxid":i32::from(1),
        "replyto":i32::from(1)
    
    };

        let options = FindOneAndDeleteOptions::builder().projection(filter).build();


        println!("{}",&data.claims.pseudo);
match replies.find_one_and_delete(
        doc!  {   "pseudo":&data.claims.pseudo,
         "_id":&bson::oid::ObjectId::parse_str(&request.id).unwrap(),
        
        }
        , options ).await {
    Ok(value) => {
        match value {
    Some(res) => {

       

      
            //recursive delete replies & votes
            println!("{:#?}",delete_replies_recursive(&res.convid,res.boxid,request.id.clone(), &self.mongo_client).await);
         

            println!("ok1");

            // -1 nb of replies
            replies.update_one(
                
                doc!  {"_id":&bson::oid::ObjectId::parse_str(&res.replyto).unwrap()}
                , doc!
                    {"$inc":{"subreplies":-1}}
        
                ,
                None
            ).await.unwrap();




            Ok(Response::new(crate::service::common_types::Empty{  }))
    

    },
    None =>{
        return Err(Status::new(tonic::Code::InvalidArgument, "conv not found"))
    },
}

    },
    Err(err) => {
        println!("{:#?}",err);
        return Err(Status::new(tonic::Code::InvalidArgument, "db error"))
    },
}

}

    async fn list_user_convs(& self,request:tonic::Request<user::UserAssetsRequest> ,) ->  Result<tonic::Response<conversation::ConvHeaderList> ,tonic::Status, > {
    //if pseudo!=user filtre anon=false
    //    todo!()

    let request=request.get_ref();
    let pseudo = if !(&request.access_token).is_empty(){
    let data=decode::<JWTPayload>(&request.access_token,&self.jwt_decoding_key,&self.jwt_algo);
    match data {
        Ok(data)=>data.claims.pseudo,
        _=>{ return Err(Status::new(tonic::Code::InvalidArgument, "invalid token"))}
        
    }
} else {
    String::from("")
};

let offset=request.offset;
if offset<0 {
  return   Err(Status::new(tonic::Code::InvalidArgument, "invalid offset"))
}


let conversations = self.mongo_client.database("DB")
.collection::<ConvHeader>("convs");
let pipeline = vec![
if  &request.pseudo==&pseudo {
    doc!{  "$match": { "metadata.pseudo" : &request.pseudo  }} 
} else {
    doc!{  "$match": { "metadata.pseudo" : &request.pseudo , "visibility.anonym":false }} 
},

doc!{ "$sort" : {  "created_at" :  i32::from(-1)}} ,
doc!{ "$skip" : offset },
doc!{ "$limit": i32::from(20) },

doc! { "$project": {
"convid": { "$toString": "$_id"},
"details":i32::from(1),
"metadata":i32::from(1),
  //   "visibility"  :   i32::from(1)
},

}  ];


let mut results = conversations.aggregate(pipeline, None).await.unwrap();

let mut convs_list :Vec<ConvHeader> = vec![];
while let Some(result) = results.next().await {

let conv_header: RawHeader = bson::from_document(result.unwrap()).unwrap();

let visibility=get_conv_visibility(&conv_header.convid, &self.keydb_pool, &self.mongo_client).await;
match visibility {
    Some(vis)=> {
        if  legitimate(&pseudo,&vis){

            let vote=  get_conv_vote(&conv_header.convid,&pseudo,&self.keydb_pool,&self.mongo_client).await as i32;
            convs_list.push(ConvHeader{rawheader:Some(conv_header),vote:vote});

        }
    },
    None=> ()
}



} 
return Ok(Response::new(ConvHeaderList{ convheaders: convs_list }))

}

    async fn list_user_replies(& self,request:tonic::Request<user::UserAssetsRequest> ,) ->  Result<tonic::Response<replies::ReplyList> ,tonic::Status, > {
    let request=request.get_ref();
    let pseudo = if !(&request.access_token).is_empty(){
    let data=decode::<JWTPayload>(&request.access_token,&self.jwt_decoding_key,&self.jwt_algo);
    match data {
        Ok(data)=>data.claims.pseudo,
        _=>{ return Err(Status::new(tonic::Code::InvalidArgument, "invalid token"))}
        
    }
} else {
    String::from("")
};

let offset=request.offset;
if offset<0 {
  return   Err(Status::new(tonic::Code::InvalidArgument, "invalid offset"))
}



let conversations = self.mongo_client.database("DB")
.collection::<Document>("replies");
let pipeline = vec![
if  &request.pseudo==&pseudo {
    doc!{  "$match": { "pseudo" : &request.pseudo  }} 
} else {
    doc!{  "$match": { "pseudo" : &request.pseudo , "anonym":false }} 
},

doc!{ "$sort" : {  "created_at" :  i32::from(-1)}} ,
doc!{ "$skip" : offset },
doc!{ "$limit": i32::from(20) },

doc! { "$project": {
"replyid": { "$toString": "$_id"},
"convid":i32::from(1),
"boxid":i32::from(1),
"txtreply":i32::from(1),
"upvote":i32::from(1),
"downvote":i32::from(1),
"pseudo":"",
"created_at":i32::from(1)
//     "visibility"  :   i32::from(1)
},

}  ];

let votes = self.mongo_client.database("DB")
.collection::<Vote>("reply_votes");

let mut results = conversations.aggregate(pipeline, None).await.unwrap();

let mut reply_list :Vec<VoteReply> = vec![];
while let Some(result) = results.next().await {

    //TODO:add votes
let reply_header: Reply = bson::from_document(result.unwrap()).unwrap();

let visibility=get_conv_visibility(&reply_header.convid,& self.keydb_pool, &self.mongo_client).await;
match visibility {
    Some(vis)=> {
        if legitimate(&pseudo,&vis){





            let filter: mongodb::bson::Document = doc! { "value":i32::from(1) ,};
let options = FindOneOptions::builder().projection(filter).build();
let vote=match   votes.find_one(
    doc! { "id":&reply_header.replyid,  "pseudo":&pseudo}
    , options ).await  {
    Ok(vote) => {match vote {
    Some(value) => shift2_vote_value(value.value),
    None =>VoteValue::Neutral,}},
    Err(_) => continue,};



            reply_list.push(
                VoteReply{ reply:Some(reply_header), 
                    vote: vote as i32
                 }
                );
        }
    },
    None=> ()
}



} 
return Ok(Response::new(ReplyList{ reply_list: reply_list }))
} 

    async fn vote_conv(& self,request:tonic::Request<vote::VoteConvRequest, > ,) ->  Result<tonic::Response<vote::VoteResponse> ,tonic::Status> {

        let request=request.get_ref();
          
        let data=decode::<JWTPayload>(&request.access_token,&self.jwt_decoding_key,&self.jwt_algo);
        let data=match data {
            Ok(data)=>data,
            _=>{ return Err(Status::new(tonic::Code::InvalidArgument, "invalid token"))}
        };
        


let visibility=get_conv_visibility(&request.convid,&self.keydb_pool,&self.mongo_client).await;
println!("{:#?}",visibility);
match visibility  {
        Some(visib) => {

            if legitimate(&data.claims.pseudo,&visib) {
                let votes = self.mongo_client.database("DB")
                .collection::<Vote>("conv_votes");

                let vote=request.vote();

                if vote==vote::VoteValue::Neutral {

                    let filter: mongodb::bson::Document = doc! {
                        "value":i32::from(1) ,};
                
                        let options = FindOneAndDeleteOptions::builder().projection(filter).build();
      
             match   votes.find_one_and_delete(
                        doc! { "id":&request.convid,  "pseudo":&data.claims.pseudo}
                        , options ).await {

                   Ok(res) => {
                       match res {
    Some(initvote) => {
        

        //update mongo conv metadata
     
        update_metadata("convs",&request.convid,0,initvote.value,&self.mongo_client).await;

        update_cache(&request.convid,- initvote.value,&self.keydb_pool).await;



        //invalidate vote cache
        let key=String::from(&request.convid)+&data.claims.pseudo;
        let mut keydb_conn = self.keydb_pool.get().await.expect("keydb_pool failed");
        let _:()=cmd("unlink").arg(key).query_async(&mut *keydb_conn).await.expect("unlink failed");

        //invalidate  conv header
        let _:()=cmd("unlink").arg(&request.convid).query_async(&mut *keydb_conn).await.expect("unlink failed");

        return Ok(Response::new(vote::VoteResponse{ previous: shift2_vote_value(initvote.value) as i32 }))
    },
    None => {
        return Ok(Response::new(vote::VoteResponse{ previous: VoteValue::Neutral as i32 }))
    },
}
                        
                   },
                    Err(err) => {
                        println!("{:#?}",err);
                        return Err(Status::new(tonic::Code::InvalidArgument, "db err 1"))
                    },

}
                }

                let header_filter: mongodb::bson::Document = doc! { 
                    "value": i32::from(1) ,
                };
                let options = FindOneAndUpdateOptions::builder().projection(header_filter).return_document(ReturnDocument::Before).upsert(true).build();
                
                let votevalue=vote_value2_shift(vote);

                match votes.find_one_and_update(
                
                    doc! { "id": &request.convid,
                           "pseudo": &data.claims.pseudo
                         },
                    doc! { "$set": {"value":votevalue
                    
 

                    ,
                    "created_at":i64::try_from(get_epoch()).unwrap()

                                       
                
                } },
                
                    options
                ).await {
                Ok(value) => {
                    let prev_vote=    match value {
                Some(prev_vote) => {prev_vote.value},
                None=>{0}
                    };

                    if prev_vote!=votevalue {

                    update_cache(&request.convid,votevalue- prev_vote,&self.keydb_pool).await;

                    //update cache & mongo

                    update_metadata("convs",&request.convid,votevalue,prev_vote,&self.mongo_client).await;
                    

                            //invalidate vote cache
                let key=String::from(&request.convid)+&data.claims.pseudo;
                let mut keydb_conn = self.keydb_pool.get().await.expect("keydb_pool failed");
                let _:()=cmd("unlink").arg(key).query_async(&mut *keydb_conn).await.expect("unlink failed");
        //invalidate  conv header
        let _:()=cmd("unlink").arg(&request.convid).query_async(&mut *keydb_conn).await.expect("unlink failed");


                    }
                    return Ok(Response::new(vote::VoteResponse{ previous:   shift2_vote_value(prev_vote) as i32}))
                }
                
                    
               
                Err(_) => {
                    return Err(Status::new(tonic::Code::InvalidArgument, "db err 2"))
                },
                }
            }
            else {
                return Err(Status::new(tonic::Code::NotFound, "conv not found for user 2")) 
            }
        } ,
        None=>{
            return Err(Status::new(tonic::Code::NotFound, "conv not found for user 3")) 
        },

    }

    //    vote(request.value(),"convs",&request.id,&request.id,&data.claims.pseudo,self.keydb_pool.clone(),&self.mongo_client).await
}

    async fn vote_reply(& self,request:tonic::Request<vote::VoteReplyRequest, > ,) -> Result<tonic::Response<vote::VoteResponse> ,tonic::Status> {
    let request=request.get_ref();
          
    let data=decode::<JWTPayload>(&request.access_token,&self.jwt_decoding_key,&self.jwt_algo);
    let data=match data {
        Ok(data)=>data,
        _=>{ return Err(Status::new(tonic::Code::InvalidArgument, "invalid token"))}
    };

    let filter: mongodb::bson::Document = doc! {
        "convid":i32::from(1),
        
     //   "convowner":i32::from(1),
     };
    let options = FindOneOptions::builder().projection(filter).build();
    
    let replies = self.mongo_client.database("DB")
    .collection::<crate::service::common_types::Empty>("replies");
    

    let  result = replies.find_one( 
        doc! { "_id" : bson::oid::ObjectId::parse_str(&request.replyid).unwrap(),
    "convid":&request.convid,
"boxid":&request.boxid},
         options).await.unwrap();
    println!("{:#?}",result);
       match result {
            Some(_) => { },
            None =>{
                return  Err(Status::new(tonic::Code::InvalidArgument, "invalid reply"))
            },
        }
        let visibility=get_conv_visibility(&request.convid,&self.keydb_pool,&self.mongo_client).await;

    
        match visibility  {
            Some(visib) => {
    
                if legitimate(&data.claims.pseudo,&visib){



                    let votes = self.mongo_client.database("DB")
                    .collection::<Vote>("reply_votes");
    
                    let vote=request.vote();
    
                    if vote==vote::VoteValue::Neutral {
    
                        let filter: mongodb::bson::Document = doc! {
                            "value":i32::from(1) ,};
                    
                            let options = FindOneAndDeleteOptions::builder().projection(filter).build();
          
                 match   votes.find_one_and_delete(
                            doc! { "id":&request.replyid, "convid":&request.convid, "boxid":&request.boxid, "pseudo":&data.claims.pseudo}
                            , options ).await {
    
                       Ok(res) => {
                           match res {
        Some(initvote) => {
    
            //update mongo conv metadata
         
            update_metadata("replies",&request.replyid,0,initvote.value,&self.mongo_client).await;
    

    
            return Ok(Response::new(vote::VoteResponse{ previous: shift2_vote_value(initvote.value) as i32 }))
        },
        None => {
            return Ok(Response::new(vote::VoteResponse{ previous: VoteValue::Neutral as i32 }))
        },
    }
                            
                       },
                        Err(err) => {
                            println!("{:#?}",err);
                            return Err(Status::new(tonic::Code::InvalidArgument, "db err 1"))
                        },
    
    }
                    }
    
                    let header_filter: mongodb::bson::Document = doc! { 
                        "value": i32::from(1) ,
                    };
                    let options = FindOneAndUpdateOptions::builder().projection(header_filter).return_document(ReturnDocument::Before).upsert(true).build();
                    
                    let votevalue=vote_value2_shift(vote);
    
                    match votes.find_one_and_update(
                    

                        doc! { "id": &request.replyid,
                               "pseudo": &data.claims.pseudo,
                               "convid":&request.convid,
                                "boxid":&request.boxid, 
                             },
                        doc! { "$set": {"value":votevalue
                        
     
    
                        ,
                        "created_at":i64::try_from(get_epoch()).unwrap()
    
                                           
                    
                    } },
                    
                        options
                    ).await {
                    Ok(value) => {
                        let prev_vote=    match value {
                    Some(prev_vote) => {prev_vote.value},
                    None=>{0}
                        };
    

                        //update cache & mongo
    
                        update_metadata("replies",&request.replyid,votevalue,prev_vote,&self.mongo_client).await;
                        
                        return Ok(Response::new(vote::VoteResponse{ previous:   shift2_vote_value(prev_vote) as i32}))
                    }
                    
                        
                   
                    Err(_) => {
                        return Err(Status::new(tonic::Code::InvalidArgument, "db err 2"))
                    },
                    }




                }
                else {
                    return Err(Status::new(tonic::Code::NotFound, "reply not found for user")) 
                }
            } ,
            None=>{
                return Err(Status::new(tonic::Code::NotFound, "reply not found for user")) 
            },
    
        }
    

 }

    async fn list_user_conv_votes(&self,request:tonic::Request<user::UserAssetsRequest> ,) ->  Result<tonic::Response<conversation::ConvHeaderList> ,tonic::Status, > {
    // /!\ verify 

        //if pseudo!=user filtre anon=false
    //    todo!()

    let request=request.get_ref();
    let pseudo = if !(&request.access_token).is_empty(){
    let data=decode::<JWTPayload>(&request.access_token,&self.jwt_decoding_key,&self.jwt_algo);
    match data {
        Ok(data)=>data.claims.pseudo,
        _=>{ return Err(Status::new(tonic::Code::InvalidArgument, "invalid token"))}
        
    }
} else {
    String::from("")
};

let offset=request.offset;
if offset<0 {
  return   Err(Status::new(tonic::Code::InvalidArgument, "invalid offset"))
}


let votes = self.mongo_client.database("DB")
.collection::<Document>("conv_votes");
let pipeline = vec![
    doc!{  "$match": { "pseudo" : &request.pseudo  }} ,


doc!{ "$sort" : {  "created_at" :  i32::from(-1)}} ,
doc!{ "$skip" : offset },
doc!{ "$limit": i32::from(20) },

doc! { "$project": {
"value":i32::from(1),
"id":i32::from(1)
  //   "visibility"  :   i32::from(1)
},

}  ];


let mut results = votes.aggregate(pipeline, None).await.unwrap();

let mut convs_list :Vec<ConvHeader> = vec![];
while let Some(result) = results.next().await {

let conv_vote: ConvVote = bson::from_document(result.unwrap()).unwrap();
let conv_header=get_conv_header(&pseudo,&conv_vote.id,&self.keydb_pool,&self.mongo_client).await;
match conv_header {
    Some(header)=> {
let visibility=get_conv_visibility(&header.rawheader.as_ref().unwrap().convid,& self.keydb_pool, &self.mongo_client).await;
match visibility {
    Some(vis)=> {
        if  legitimate(&pseudo,&vis){
            convs_list.push( header
                
                );
        }
    },
    None=> ()
}
    }
    None=>()


}
} 
return Ok(Response::new(ConvHeaderList{convheaders:convs_list }))


  //    todo!()
  }
  
    async fn list_user_rep_votes(& self,request:tonic::Request<user::UserAssetsRequest> ,) ->Result<tonic::Response<replies::ReplyList> ,tonic::Status, > {
    //CHECK IF reply in anonym

    let request=request.get_ref();
    let pseudo = if !(&request.access_token).is_empty(){
    let data=decode::<JWTPayload>(&request.access_token,&self.jwt_decoding_key,&self.jwt_algo);
    match data {
        Ok(data)=>data.claims.pseudo,
        _=>{ return Err(Status::new(tonic::Code::InvalidArgument, "invalid token"))}
        
    }
} else {
    String::from("")
};

let offset=request.offset;
if offset<0 {
  return   Err(Status::new(tonic::Code::InvalidArgument, "invalid offset"))
}


let votes = self.mongo_client.database("DB")
.collection::<Document>("reply_votes");
let pipeline = vec![
    doc!{  "$match": { "pseudo" : &request.pseudo  }} ,


doc!{ "$sort" : {  "created_at" :  i32::from(-1)}} ,
doc!{ "$skip" : offset },
doc!{ "$limit": i32::from(20) },

doc! { "$project": {
"value":i32::from(1),
"id":i32::from(1),
"convid":i32::from(1),
  //   "visibility"  :   i32::from(1)
},

}  ];


let mut results = votes.aggregate(pipeline, None).await.unwrap();

let mut reply_list :Vec<VoteReply> = vec![];
while let Some(result) = results.next().await {

let reply_vote: ReplyVote = bson::from_document(result.unwrap()).unwrap();

//get conv


let conv_header=get_conv_header(&pseudo,&reply_vote.convid,&self.keydb_pool,&self.mongo_client).await;
match conv_header {

    Some(header)=> {
let visibility=get_conv_visibility(&header.rawheader.unwrap().convid, &self.keydb_pool, &self.mongo_client).await;
match visibility {
    Some(vis)=> {
        // /!\ TODO reflechir 


        let replies = self.mongo_client.database("DB")
        .collection::<ConvHeaderCache>("replies");
let pipeline = vec![
doc! { "$match": {
    "_id" :  bson::oid::ObjectId::parse_str(&reply_vote.id).unwrap(),
     "anonym":true }} ,
doc!{ "$limit": i32::from(1) },

doc! { "$project": {
   // "replyid": { "$toString": "$_id"},
"txtreply":i32::from(1),
"upvote":i32::from(1),
"downvote":i32::from(1),
"created_at":i32::from(1),
"anonym":i32::from(1),
"boxid":i32::from(1),
"subreplies":i32::from(1),
//"convid":i32::from(1),
//"pseudo":"", 

//     "visibility"  :   i32::from(1)
},

}  ];


let mut reply_results = replies.aggregate(pipeline, None).await.unwrap();


if let Some(reply_result) = reply_results.next().await {

    let  reply: ProjReply = bson::from_document(reply_result.unwrap()).unwrap();
    

    if  legitimate(&pseudo,&vis){
        if !reply.anonym  || &pseudo ==&vis.pseudo{

            
        reply_list.push( 
          //  Reply
          // conv_vote_header
          // vote
          VoteReply{ reply: Some(Reply{
              replyid: reply_vote.id,
              convid: reply_vote.convid,
              boxid: reply.boxid,
              txtreply:reply.txtreply,
              pseudo:String::from(""),
              upvote: reply.upvote,
              downvote: reply.downvote,
              created_at: reply.created_at ,
          subreplies:reply.subreplies  })
            , vote: shift2_vote_value(reply_vote.value) as i32}
            
            );
        }
    }

    /*
    Some(
                Reply{ 
                replyid: reply_vote.id,
                txtreply: reply.reply,
                pseudo:vis.pseudo,
                upvote: reply.upvote,

                downvote: reply.downvote,
                created_at: reply.created_at }),
                 */


}




    //if reply in annon => pass




    },
    None=> ()
}
    }
    None=>()


}
} 
return Ok(Response::new(ReplyList{reply_list:reply_list }))


  }

    async fn get_balance(& self, request:Request<common_types::AuthenticatedRequest>,) ->  Result<Response<user::BalanceResponse>, tonic::Status>
  {
      let request=request.get_ref(); 
      let pseudo = {
          let data=decode::<JWTPayload>(&request.access_token,&self.jwt_decoding_key,&self.jwt_algo);
          match data {
              Ok(data)=>data.claims.pseudo,
              _=>{ return Err(Status::new(tonic::Code::InvalidArgument, "invalid token"))}
              
          }
      };
      
    let balance=  match self.dynamodb_client.get_item()
      .table_name("users")
      .key("pseudo",AttributeValue::S(pseudo.to_string()))
 //     .item("openid",AttributeValue::S(open_id.to_string()))
      .send().await{
          
      

      Err(_)=>{
          return Err(Status::new(tonic::Code::InvalidArgument, "db error"))
      } ,
      Ok(res)=>{
        match   res.item {
  Some(item) => {
     match item.get("balance") {
  Some(balance) => {
      match balance.as_n() {
  Ok(balance) => {
      match balance.parse::<i32>() {
  Ok(balance) => balance,
  Err(_) => {
      return Err(Status::new(tonic::Code::InvalidArgument, "balance parsing error")) 
  },
}
  },
  Err(_) => {
      return Err(Status::new(tonic::Code::InvalidArgument, "balance error")) 
  },
}
  },
  None =>{
      return Err(Status::new(tonic::Code::InvalidArgument, "balance not found")) 
  },
}
  },
  None => {
      return Err(Status::new(tonic::Code::InvalidArgument, "pseudo not found")) 
  },
}
      },

  };

  return Ok(Response::new(BalanceResponse{ balance: balance }))


}

/* 
    async fn buy_emergency(& self, request:Request<common_types::EmergencyRequest>,) ->  Result<Response<common_types::Empty>, tonic::Status>
{
    // /!\ petite racecondition sur conv_exists

    let request=request.get_ref(); 
    let pseudo = {
        let data=decode::<JWTPayload>(&request.access_token,&self.jwt_decoding_key,&self.jwt_algo);
        match data {
            Ok(data)=>data.claims.pseudo,
            _=>{ return Err(Status::new(tonic::Code::InvalidArgument, "invalid token"))}
            
        }
    };

if request.amount<=0 {
    return Err(Status::new(tonic::Code::InvalidArgument,"invalid amount"))
}
//check if conv exists
let options = CountOptions::builder().limit(1).build();

let banned:Collection<crate::service::common_types::Empty> = self.mongo_client.database("DB")
.collection("convs");


match banned.count_documents(
    doc!{
"pseudo": &pseudo, "_id":bson::oid::ObjectId::parse_str(&request.convid).unwrap()
    } , options).await {
Ok(count) => {
if count<=0 {
    return Err(Status::new(tonic::Code::InvalidArgument, "conv not found for user"))   
}

},
Err(_) => {
return Err(Status::new(tonic::Code::InvalidArgument,"db error"))
},
}

//update balance
let res = self.dynamodb_client.update_item()
.table_name("users")
.key("pseudo",AttributeValue::S(pseudo.to_string()))
.update_expression("add balance -:amount")
//     .item("openid",AttributeValue::S(open_id.to_string()))
.condition_expression("balance>=:amount")
.expression_attribute_values(
    ":amount",
    AttributeValue::N(request.amount.to_string()),
)
.return_values(ReturnValue::UpdatedNew).send().await;

let balance=match res {
Err(SdkError::ServiceError {
    err:
    UpdateItemError {
            kind: aws_sdk_dynamodb::error::UpdateItemErrorKind::ConditionalCheckFailedException(_),
            ..
        },
    raw: _,
}) => {
 return Err(Status::new(tonic::Code::Cancelled, "not enough coins"))
},
Ok(res)=>{
    match res.attributes {
    Some(res) => {
        res.get("balance").unwrap().as_n().unwrap().parse::<i32>().unwrap()
    },
    None => {
        return Err(Status::new(tonic::Code::InvalidArgument, "db error"))
    },
}
},
_ => {return Err(Status::new(tonic::Code::InvalidArgument, "db error"))}
};


    
    // update mongo
    // add to emergency table
    let add_time=get_epoch();

    let emergency = self.mongo_client.database("DB")
    .collection::<EmergencyEntry>("emergency");
    emergency.insert_one(
        EmergencyEntry{ 
            convid: request.convid.to_owned(),
             add_time: add_time,
             
             amount: request.amount }
         ,None).await.unwrap();

    let cacheid = request.convid.to_string()+":"+&add_time.to_string();

    
    // update cache
    //convid:timestamp
    let mut keydb_conn = self.keydb_pool.get().await.expect("keydb_pool failed");

    let expiration = add_time+ EMERGENCY_DURATION;
    let _:()=   cmd("zadd")
    .arg("emergency").arg(
        request.amount).arg(
            &cacheid  ).query_async(&mut *keydb_conn).await.expect("zadd error");

        let _:()= cmd("expirememberat")
        .arg("emergency").arg(
          &cacheid)
  .arg(expiration).query_async(&mut *keydb_conn).await.expect("expirememberat error");
    
    
  return Ok(Response::new(crate::service::common_types::Empty{  }))


}

 */
    async fn get_customer_id(& self, request:tonic::Request<common_types::AuthenticatedRequest>,) ->  Result<Response<user::CustomerId>, tonic::Status>
    {
        let request=request.get_ref();
        
        let data=decode::<JWTPayload>(&request.access_token,&self.jwt_decoding_key,&self.jwt_algo);
        let data=match data {
            Ok(data)=>data,
            _=>{ return Err(Status::new(tonic::Code::InvalidArgument, "invalid token"))}
        };

        let filter: mongodb::bson::Document = doc! { "customerid":i32::from(1) };
        let options = FindOneOptions::builder().projection(filter).build();
    
        let users = self.mongo_client.database("DB")
        .collection::<user::CustomerId>("users");
        
    
        let  cursor = users.find_one(doc!{
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

async fn report(& self, request:Request<report::ReportRequest>,) ->  Result<Response<common_types::Empty>, tonic::Status>
{
    
    let request=request.get_ref();
        
    let data=decode::<JWTPayload>(&request.access_token,&self.jwt_decoding_key,&self.jwt_algo);
    let data=match data {
        Ok(data)=>data,
        _=>{ return Err(Status::new(tonic::Code::InvalidArgument, "invalid token"))}
    };

    let report = self.mongo_client.database("DB")
    .collection::<Document>("report");
  
    if let Err(_) = report.insert_one(
        doc!{
            "pseudo":&data.claims.pseudo,
             "details": &request.details,
             "reason":&request.reason,
             "resource_id":&request.resource_id,
             "resource_type":&request.resource_type
               }
         ,None).await {
            return Err(Status::new(tonic::Code::InvalidArgument,"db error"))
}

return Ok(Response::new(common_types::Empty{  }))
}

fn get_qa_space< 'life0, 'async_trait>(& 'life0 self,_request:tonic::Request<common_types::AuthenticatedObjectRequest, > ,) ->  core::pin::Pin<Box<dyn core::future::Future<Output = Result<tonic::Response<qa::QaSpace> ,tonic::Status> > + core::marker::Send+ 'async_trait> >where 'life0: 'async_trait,Self: 'async_trait {
    todo!()
}

fn preview_qa_space< 'life0, 'async_trait>(& 'life0 self,_request:tonic::Request<common_types::AuthenticatedObjectRequest, > ,) ->  core::pin::Pin<Box<dyn core::future::Future<Output = Result<tonic::Response<qa::QaSpace> ,tonic::Status> > + core::marker::Send+ 'async_trait> >where 'life0: 'async_trait,Self: 'async_trait {
    todo!()
}

fn edit_qa_space< 'life0, 'async_trait>(& 'life0 self,_request:tonic::Request<qa::EditQaSpaceRequest> ,) ->  core::pin::Pin<Box<dyn core::future::Future<Output = Result<tonic::Response<common_types::Empty> ,tonic::Status> > + core::marker::Send+ 'async_trait> >where 'life0: 'async_trait,Self: 'async_trait {
    todo!()
}


    fn change_informations< 'life0, 'async_trait>(& 'life0 self,_request:tonic::Request<settings::UserInformations> ,) ->  core::pin::Pin<Box<dyn core::future::Future<Output = Result<tonic::Response<common_types::Empty> ,tonic::Status> > + core::marker::Send+ 'async_trait> >where 'life0: 'async_trait,Self: 'async_trait {
        todo!()
    }

   fn decline_invitation< 'life0, 'async_trait>(& 'life0 self,_request:tonic::Request<user::ResourceRequest> ,) ->  core::pin::Pin<Box<dyn core::future::Future<Output = Result<tonic::Response<common_types::Empty> ,tonic::Status> > + core::marker::Send+ 'async_trait> >where 'life0: 'async_trait,Self: 'async_trait {
        todo!()
    }

    fn block_user< 'life0, 'async_trait>(& 'life0 self,_request:tonic::Request<user::BlockRequest> ,) ->  core::pin::Pin<Box<dyn core::future::Future<Output = Result<tonic::Response<common_types::Empty> ,tonic::Status> > + core::marker::Send+ 'async_trait> >where 'life0: 'async_trait,Self: 'async_trait {
        todo!()
    }

    fn unblock_user< 'life0, 'async_trait>(& 'life0 self,_request:tonic::Request<user::BlockRequest> ,) ->  core::pin::Pin<Box<dyn core::future::Future<Output = Result<tonic::Response<common_types::Empty> ,tonic::Status> > + core::marker::Send+ 'async_trait> >where 'life0: 'async_trait,Self: 'async_trait {
        todo!()
    }

    fn list_invitations< 'life0, 'async_trait>(& 'life0 self,_request:tonic::Request<user::PersonalAssetsRequest> ,) ->  core::pin::Pin<Box<dyn core::future::Future<Output = Result<tonic::Response<conversation::ConvHeaderList> ,tonic::Status, > > + core::marker::Send+ 'async_trait> >where 'life0: 'async_trait,Self: 'async_trait {
        todo!()
    }




    fn get_notifications< 'life0, 'async_trait>(& 'life0 self,_request:tonic::Request<notifications::GetNotificationsRequest> ,) ->  core::pin::Pin<Box<dyn core::future::Future<Output = Result<tonic::Response<notifications::NotificationsResponse> ,tonic::Status, > > + core::marker::Send+ 'async_trait> >where 'life0: 'async_trait,Self: 'async_trait {
        todo!()
    }

    fn update_wallet< 'life0, 'async_trait>(& 'life0 self,_request:tonic::Request<common_types::AuthenticatedObjectRequest> ,) ->  core::pin::Pin<Box<dyn core::future::Future<Output = Result<tonic::Response<common_types::Empty> ,tonic::Status, > > + core::marker::Send+ 'async_trait> >where 'life0: 'async_trait,Self: 'async_trait {
        todo!()
    }






    async fn get_informations(& self,_request:tonic::Request<common_types::AuthenticatedRequest> ,) ->  Result<tonic::Response<settings::UserInformationsResponse> ,tonic::Status, >  {
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



}

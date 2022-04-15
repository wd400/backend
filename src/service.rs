#![allow(unused)]
use redis::{RedisConnectionInfo, RedisError};
use serde::{Serialize, Deserialize};
use jsonwebtoken::{encode, decode, Header, Algorithm, Validation, EncodingKey, DecodingKey, decode_header, TokenData, crypto::verify};
use tonic::{Request, Response, Status, codegen::http::request};
use crate::{api::{*, feed::FeedType, common_types::{ConvHeader, Genre, ConvHeaderList, User, Votes}}, cache_init::ConversationRank};
use reqwest;
use moka::future::Cache;
use std::{collections::{HashMap, hash_map::RandomState, BTreeMap}, borrow::{BorrowMut, Borrow}};
use sha2::{Sha256, Sha512, Digest};

use bb8_redis::{
    bb8::{self, Pool, PooledConnection},
    redis::{cmd, AsyncCommands},
    RedisConnectionManager
};

use mongodb::{Client as MongoClient, options::{ClientOptions, DriverInfo, Credential, ServerAddress, FindOptions, FindOneOptions}, bson::{doc, Document}};

//extern crate rusoto_core;
//extern crate rusoto_dynamodb;

use aws_sdk_dynamodb::{Client as AWSClient, Error, model::{AttributeValue, ReturnValue}, types::{SdkError, self}, error::{ConditionalCheckFailedException, PutItemError, conditional_check_failed_exception, PutItemErrorKind}};
 

#[derive(Debug, Serialize, Deserialize)]
struct JWTPayload {
    exp: i64,
    userid: String,
    open_id: String,
 //   country_code: String,
 //   timestamp_dob: i32,
 //   genre:  RustGenre
}


#[derive(Debug, Serialize, Deserialize)]
enum RustGenre {
    M,
    F,
    Other,
}

fn protoGenre2RustGenre(genre:Genre)->Option<RustGenre>{
    match genre {
        Genre::M=>Some(RustGenre::M),
        Genre::F=>Some(RustGenre::F),
        Genre::Other=>Some(RustGenre::Other),
        _ => None
    } 
    
}

//#[derive(Default)]
pub struct  MyApi {
    pub jwt_encoding_key:EncodingKey,
    pub jwt_decoding_key:DecodingKey,
    pub jwt_algo:Validation,
    pub google_client_id: String,
    pub google_client_secret: String,
    pub facebook_client_id: String,
    pub facebook_client_secret: String,
    pub dynamodb_client: AWSClient,
    pub hash_salt:String,
    pub keydb_pool:Pool<RedisConnectionManager>,
    pub mongo_client:MongoClient
 //   pub cache:Cache<String, String>,

}


#[derive(Deserialize)]
struct FacebookToken {
    access_token: String,
    token_type: String,
    expires_in: i64
}

const T :&str="lo";

const GRANT_TYPE :&str= "authorization_code";


#[derive(Deserialize)]
struct GoogleTokensJSON {
    id_token: String,
    expires_in: u32,
  //  id_token: String,
    scope: String,
    token_type: String,
    //this field is only present in this response if you set the access_type parameter to offline in the initial request to Google's authorization server. 
    //refresh_token: String

}




fn Map2ConvHeader(map:BTreeMap<String, String>)->ConvHeader {
// /!\ pb si anonyme
    ConvHeader{
        convid:  map.get("convid").unwrap().parse::<i32>().unwrap(),
        title: map.get("title").unwrap().to_string(),
        writer: Some(User{
             userid: map.get("userid").unwrap().to_string(),
             username: map.get("username").unwrap().to_string() }),
        votes: Some(Votes{
            upvote: map.get("upvote").unwrap().parse::<i32>().unwrap(),
            downvote: map.get("downvote").unwrap().parse::<i32>().unwrap(),
        }),
        description: map.get("description").unwrap().to_string(),
        categories: map.get("categories").unwrap().to_string(),
        created_at:  map.get("created_at").unwrap().parse::<u32>().unwrap(),
    }
}

fn ConvHeader2Map(header: ConvHeader)->BTreeMap<String, String> {
let writer=header.writer.unwrap();
let votes=header.votes.unwrap();
    BTreeMap::from([
        ("convid".to_string(), header.convid.to_string()),
        ("title".to_string(), header.title),
        ("userid".to_string(), writer.userid),
        ("username".to_string(),writer.username),
        ("upvote".to_string(), votes.upvote.to_string()),
        ("downvote".to_string(),  votes.downvote.to_string()),
        ("description".to_string(),header.description),
        ("categories".to_string(), header.categories),
        ("created_at".to_string(),header.created_at.to_string())
    ])
}

#[derive(Debug, Serialize, Deserialize,Clone)]
struct RustConvHeader {
    convid:i32,
    title:String,
    userid:String,
    username:String,
    upvote:i32,
    downvote:i32,
    description:String,
    categories:String,
    created_at:u32
    

}

fn RustConvHeader2ConvHeader(header: &RustConvHeader)->ConvHeader{

    ConvHeader{
        convid:  header.convid,
        title: header.title.to_string(),
        writer: Some(User{
             userid: header.userid.to_string(),
             username: header.username.to_string() }),
        votes: Some(Votes{
            upvote: header.upvote,
            downvote: header.downvote,
        }),
        description: header.description.to_string(),
        categories: header.categories.to_string(),
        created_at:  header.created_at,
    }
}



async fn get_conv_header(convid:&str,keydb_pool:Pool<RedisConnectionManager>,mongo_client:&MongoClient)->ConvHeader{

    let mut keydb_conn = keydb_pool.get().await.expect("keydb_pool failed");

    let cached:BTreeMap<String, String>=   cmd("hgetall")
                    .arg(convid).query_async(&mut *keydb_conn).await.expect("hgetall failed");
        println!("cache: {:#?}",cached);
     
           
            
   

                //cache miss
                if cached.is_empty() {


                    let header_filter: mongodb::bson::Document = doc! {
                        "convid": i32::from(1),
                        "title": i32::from(1),
                        "description":i32::from(1),
                        "categories":i32::from(1),
                        "username":i32::from(1),
                        "upvote": i32::from(1),
                        "downvote": i32::from(1),
                        "timestamp":  i32::from(1),
                    };
    
                    
    
                    let options = FindOneOptions::builder().projection(header_filter).build();
                
                    let conversations = mongo_client.database("DB")
                    .collection::<RustConvHeader>("convs");
                    
                    let convid_int=convid.parse::<i32>().unwrap();
                    println!("convid_int {:#?}",convid_int);
                    let mut cursor = conversations.find_one(doc!{
                        "convid":convid_int
                    },
                    //    options
                    FindOneOptions::default() 
                        ).await.unwrap();

                    println!("mongodb {:#?}",cursor);

                    match cursor {
                        Some(rust_conv_header)=> {
    
    let conv_header=RustConvHeader2ConvHeader(&rust_conv_header);
    
                            let _:()=   cmd("hmset")
                            .arg(&vec![
                                    convid,
                                    "convid",convid,
                                    "title",&rust_conv_header.title,
                                    "userid",&rust_conv_header.userid.to_string(),
                                    "username",&rust_conv_header.username,
                                    "upvote",&rust_conv_header.upvote.to_string(),
                                    "downvote",&rust_conv_header.downvote.to_string(),
                                    "categories",&rust_conv_header.categories,
                                    "created_at",&rust_conv_header.created_at.to_string(),
                                    "description",&rust_conv_header.description
                                    ] ).query_async(&mut *keydb_conn).await.unwrap();
    
    
                            let _:()=   cmd("expire")
                            .arg(&[convid,"60"]).query_async(&mut *keydb_conn).await.unwrap();
    
                        
                        return   conv_header ;
    
    
                        },
                        None=>{
                           return ConvHeader::default();
                            //conv not found
                        }
                    }
    
    

                }
                //cache hit
                else {
                
                let _:()=   cmd("expire")
                .arg(&[convid,"60"]).query_async(&mut *keydb_conn).await.unwrap();

                return Map2ConvHeader(cached);
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

#[tonic::async_trait]
impl v1::api_server::Api for MyApi {

    async fn login(&self, request: Request<login::LoginRequest>) -> Result<Response<login::LoginResponse>, Status> {
     //   println!("Got a request: {:#?}", &request);
        let request=request.get_ref();



        let third_party = request.third_party();
        
        let open_id:String =
         if third_party==login::ThirdParty::Facebook {
        //    return Err(Status::new(tonic::Code::InvalidArgument, "not yet implemented"));

                    // https://developers.google.com/identity/protocols/oauth2/openid-connect#exchangecode
                    let client = reqwest::Client::new();

                    let facebook_request = client.get("https://graph.facebook.com/oauth/access_token")
                    .query(
                        &[
                            //safe? optimal?
                            ("redirect_uri","https://example.com/".into()),
                            ("code", request.code.clone()),
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

            // https://developers.google.com/identity/protocols/oauth2/openid-connect#exchangecode
            let client = reqwest::Client::new();
         
            
            let google_tokens = client.post("https://oauth2.googleapis.com/token")
            .form(
                &[
                    //safe? optimal?
                    ("code", request.code.clone()),
                    ("client_id",self.google_client_id.clone()),
                    ("client_secret",self.google_client_secret.clone()),
                    ("redirect_uri","https://example.com".into()),
                    ("grant_type",GRANT_TYPE.into())
                ]
            );
            // .mime_str("text/plain")?;
            
            let google_tokens = google_tokens.send().await;

            let google_tokens = match google_tokens {
                Ok(google_tokens) => google_tokens,
                Err(_) => return Err(Status::new(tonic::Code::InvalidArgument, "oauth request error"))
            };

            let google_tokens:GoogleTokensJSON = match google_tokens.json().await {
                Ok(google_tokens) => google_tokens,
                Err(_) => return Err(Status::new(tonic::Code::InvalidArgument, "oauth json error"))
            };

        let token_infos=client.get("https://oauth2.googleapis.com/tokeninfo?")
        .query(&[("id_token",google_tokens.id_token)])
        .send().await;
        
        let token_infos = match token_infos {
            Ok(token_infos) => token_infos,
            Err(_) => return Err(Status::new(tonic::Code::InvalidArgument, "tokeninfo request error"))
        };

        let token_infos = match token_infos.json::<HashMap<String, String>>().await {
            Ok(token_infos) => token_infos,
            Err(_) => return Err(Status::new(tonic::Code::InvalidArgument, "oauth json error"))
        };

        let sub = match token_infos.get("sub").cloned() {
            Some(sub) => sub,
            None => return Err(Status::new(tonic::Code::InvalidArgument, "oauth json error"))
        };
    
         sub
                
            }
        else {
                return Err(Status::new(tonic::Code::InvalidArgument, "third party is invalid"))
        };
        
        println!("openid: {}",open_id);
        

        //ConditionalCheckFailedException
        let res = self.dynamodb_client.put_item()
        .table_name("users")
        .item("openid",AttributeValue::S(open_id.to_string()))
        .item("amount",AttributeValue::N(String::from("0")))
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


      let mut hasher = Sha256::new();
      hasher.update(self.hash_salt.to_owned()+&open_id);

      let hash = hasher.finalize();


        let userid=base85::encode(&hash);
        let payload: JWTPayload = JWTPayload {
            exp:chrono::offset::Local::now().timestamp()+60*60*24*60,
            userid:userid,
            open_id:open_id
            
        };

        let token = encode(&Header::default(), &payload, &self.jwt_encoding_key)
        .expect("INVALID TOKEN");

        //userid redondant
        let response = login::LoginResponse{
            access_token:token,
            is_new: is_new
        };

        Ok(Response::new(response))
    }

    async fn refresh_token(&self,request:Request<common_types::AuthenticatedRequest>) ->  Result<Response<common_types::RefreshToken>, Status> {
        
        let request=request.get_ref();
        let data=decode::<JWTPayload>(&request.access_token,&self.jwt_decoding_key,&self.jwt_algo);
        let data=match data {
            Ok(data)=>data,
            _=>{ return Err(Status::new(tonic::Code::InvalidArgument, "invalid token"))}
        };

        
        let payload: JWTPayload = JWTPayload {
            exp:chrono::offset::Local::now().timestamp()+60*60*24*60,
            userid:data.claims.userid,
            open_id:data.claims.open_id
            
        };

        let token = encode(&Header::default(), &payload, &self.jwt_encoding_key)
        .expect("INVALID TOKEN");

        //userid redondant
        let response = common_types::RefreshToken{
            access_token:token
        };

        Ok(Response::new(response))

    }

    async fn feed(&self,request:Request<feed::FeedRequest> ) -> Result<Response<common_types::ConvHeaderList>,Status> {
        let request=request.get_ref();

        if &request.access_token != ""{

        let data=decode::<JWTPayload>(&request.access_token,&self.jwt_decoding_key,&self.jwt_algo);
        
        let data=match data {
            Ok(data)=>data,
            _=>{ return Err(Status::new(tonic::Code::InvalidArgument, "invalid token"))}
        };
    }

        let cache_table_name= match feedType2cacheTable(request.feed_type()){
    Some(cache_table_name)=>cache_table_name,
    _=>return Err(Status::new(tonic::Code::InvalidArgument, "invalid feed"))
        };
        
        let offset=request.offset;
        if offset<0{
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

        todo!()
    }



    async fn delete_conv(&self,request:Request<common_types::AuthenticatedObjectRequest> ) ->  Result<Response<common_types::Empty> ,Status > {

        todo!()
    }


    async fn delete_reply(&self,request:Request<common_types::AuthenticatedObjectRequest> ) ->  Result<Response<common_types::Empty> ,Status > {

        todo!()
    }

    async fn search(&self,request:Request<search::SearchRequest> ) ->  Result<Response<search::SearchResponse> ,Status > {

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


    fn list_invitations< 'life0, 'async_trait>(& 'life0 self,request:tonic::Request<user::PersonalAssetsRequest> ,) ->  core::pin::Pin<Box<dyn core::future::Future<Output = Result<tonic::Response<common_types::ConvHeaderList> ,tonic::Status, > > + core::marker::Send+ 'async_trait> >where 'life0: 'async_trait,Self: 'async_trait {
        todo!()
    }


    fn list_user_convs< 'life0, 'async_trait>(& 'life0 self,request:tonic::Request<user::UserAssetsRequest> ,) ->  core::pin::Pin<Box<dyn core::future::Future<Output = Result<tonic::Response<common_types::ConvHeaderList> ,tonic::Status, > > + core::marker::Send+ 'async_trait> >where 'life0: 'async_trait,Self: 'async_trait {
        todo!()
    }


    fn list_user_replies< 'life0, 'async_trait>(& 'life0 self,request:tonic::Request<user::UserAssetsRequest> ,) ->  core::pin::Pin<Box<dyn core::future::Future<Output = Result<tonic::Response<common_types::ReplyHeaderList> ,tonic::Status, > > + core::marker::Send+ 'async_trait> >where 'life0: 'async_trait,Self: 'async_trait {
        todo!()
    }


    fn list_user_upvotes< 'life0, 'async_trait>(& 'life0 self,request:tonic::Request<user::UserAssetsRequest> ,) ->  core::pin::Pin<Box<dyn core::future::Future<Output = Result<tonic::Response<common_types::ConvHeaderList> ,tonic::Status, > > + core::marker::Send+ 'async_trait> >where 'life0: 'async_trait,Self: 'async_trait {
        todo!()
    }


    fn list_user_downvotes< 'life0, 'async_trait>(& 'life0 self,request:tonic::Request<user::UserAssetsRequest> ,) ->  core::pin::Pin<Box<dyn core::future::Future<Output = Result<tonic::Response<common_types::ConvHeaderList> ,tonic::Status, > > + core::marker::Send+ 'async_trait> >where 'life0: 'async_trait,Self: 'async_trait {
        todo!()
    }


    fn get_conv< 'life0, 'async_trait>(& 'life0 self,request:tonic::Request<common_types::AuthenticatedObjectRequest, > ,) ->  core::pin::Pin<Box<dyn core::future::Future<Output = Result<tonic::Response<visibility::Visibility> ,tonic::Status, > > + core::marker::Send+ 'async_trait> >where 'life0: 'async_trait,Self: 'async_trait {
        todo!()
    }


    fn modify_visibility< 'life0, 'async_trait>(& 'life0 self,request:tonic::Request<visibility::ModifyVisibilityRequest> ,) ->  core::pin::Pin<Box<dyn core::future::Future<Output = Result<tonic::Response<common_types::Empty> ,tonic::Status> > + core::marker::Send+ 'async_trait> >where 'life0: 'async_trait,Self: 'async_trait {
        todo!()
    }


    fn upvote_conv< 'life0, 'async_trait>(& 'life0 self,request:tonic::Request<common_types::AuthenticatedObjectRequest, > ,) ->  core::pin::Pin<Box<dyn core::future::Future<Output = Result<tonic::Response<common_types::Empty> ,tonic::Status> > + core::marker::Send+ 'async_trait> >where 'life0: 'async_trait,Self: 'async_trait {
        todo!()
    }


    fn downvote_conv< 'life0, 'async_trait>(& 'life0 self,request:tonic::Request<common_types::AuthenticatedObjectRequest, > ,) ->  core::pin::Pin<Box<dyn core::future::Future<Output = Result<tonic::Response<common_types::Empty> ,tonic::Status> > + core::marker::Send+ 'async_trait> >where 'life0: 'async_trait,Self: 'async_trait {
        todo!()
    }


    fn get_visibility< 'life0, 'async_trait>(& 'life0 self,request:tonic::Request<common_types::AuthenticatedObjectRequest, > ,) ->  core::pin::Pin<Box<dyn core::future::Future<Output = Result<tonic::Response<conversation::Conversation> ,tonic::Status, > > + core::marker::Send+ 'async_trait> >where 'life0: 'async_trait,Self: 'async_trait {
        todo!()
    }


    fn modify_conv< 'life0, 'async_trait>(& 'life0 self,request:tonic::Request<conversation::Conversation> ,) ->  core::pin::Pin<Box<dyn core::future::Future<Output = Result<tonic::Response<common_types::Empty> ,tonic::Status> > + core::marker::Send+ 'async_trait> >where 'life0: 'async_trait,Self: 'async_trait {
        todo!()
    }


    fn downvote_reply< 'life0, 'async_trait>(& 'life0 self,request:tonic::Request<common_types::AuthenticatedObjectRequest, > ,) ->  core::pin::Pin<Box<dyn core::future::Future<Output = Result<tonic::Response<common_types::Empty> ,tonic::Status> > + core::marker::Send+ 'async_trait> >where 'life0: 'async_trait,Self: 'async_trait {
        todo!()
    }


    fn upvote_reply< 'life0, 'async_trait>(& 'life0 self,request:tonic::Request<common_types::AuthenticatedObjectRequest, > ,) ->  core::pin::Pin<Box<dyn core::future::Future<Output = Result<tonic::Response<common_types::Empty> ,tonic::Status> > + core::marker::Send+ 'async_trait> >where 'life0: 'async_trait,Self: 'async_trait {
        todo!()
    }


    fn submit_reply< 'life0, 'async_trait>(& 'life0 self,request:tonic::Request<replies::ReplyRequest, > ,) ->  core::pin::Pin<Box<dyn core::future::Future<Output = Result<tonic::Response<common_types::Empty> ,tonic::Status> > + core::marker::Send+ 'async_trait> >where 'life0: 'async_trait,Self: 'async_trait {
        todo!()
    }


    fn get_replies< 'life0, 'async_trait>(& 'life0 self,request:tonic::Request<replies::GetRepliesRequest> ,) ->  core::pin::Pin<Box<dyn core::future::Future<Output = Result<tonic::Response<replies::ReplyList> ,tonic::Status> > + core::marker::Send+ 'async_trait> >where 'life0: 'async_trait,Self: 'async_trait {
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

    async fn delete_account(& self,request:Request<common_types::AuthenticatedRequest> ,) -> Result<Response<common_types::Empty> ,tonic::Status >  {
     //   todo!()

     let request=request.get_ref();
     let data=decode::<JWTPayload>(&request.access_token,&self.jwt_decoding_key,&self.jwt_algo);
     let data=match data {
         Ok(data)=>data,
         _=>{ return Err(Status::new(tonic::Code::InvalidArgument, "invalid token"))}
     };



     match self.dynamodb_client.delete_item()
     .table_name("users")
     .key("openid",AttributeValue::S(data.claims.open_id.to_string())).send().await
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

    fn upload_file< 'life0, 'async_trait>(& 'life0 self,request:tonic::Request<common_types::FileUploadRequest> ,) ->  core::pin::Pin<Box<dyn core::future::Future<Output = Result<tonic::Response<common_types::FileUploadResponse> ,tonic::Status, > > + core::marker::Send+ 'async_trait> >where 'life0: 'async_trait,Self: 'async_trait {
        todo!()
    }


   async fn report(& self, request:Request<common_types::AuthenticatedRequest>,) ->  Result<Response<common_types::Empty>, tonic::Status>
    { todo!() }


   async fn get_balance(& self, request:Request<common_types::AuthenticatedRequest>,) ->  Result<Response<user::BalanceResponse>, tonic::Status>
    { todo!() }

    async fn buy_emergency(& self, request:Request<common_types::AuthenticatedRequest>,) ->  Result<Response<common_types::Empty>, tonic::Status>
    { todo!() }

}

#[macro_use(bson, doc)]
use bb8_redis::{bb8::Pool, RedisConnectionManager};
use mongodb::{Client as MongoClient, options::{ClientOptions, DriverInfo, Credential, ServerAddress}, bson::Bson};
use redis::{cmd, RedisResult};

use crate::{service::feedType2cacheTable, api::{feed, common_types::Votes}};
use std::time::{Duration, SystemTime, UNIX_EPOCH}; 
use mongodb::{bson::doc, bson::oid::ObjectId, options::FindOptions, Collection};
use futures::{stream::StreamExt, TryStreamExt};
use  bson::serde_helpers::serialize_hex_string_as_object_id;
use serde::{Deserialize, Serialize};

pub fn get_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

//must be ordered
pub const TIMEFEEDTYPES: [feed::FeedType; 4]  = [
  //  feed::FeedType::Emergency,
  //  feed::FeedType::LastActivity,
  feed::FeedType::LastYear,
  feed::FeedType::LastMonth,
  feed::FeedType::LastWeek,
    feed::FeedType::LastDay,

  //  feed::FeedType::AllTime,
];

#[derive(Debug, Serialize, Deserialize)]
pub struct ConversationRank {
    // #[serde(serialize_with = "serialize_hex_string_as_object_id")]
    _id: ObjectId,
  //  upvote: i32,
  //  downvote: i32,
    created_at: u64,
    score: i32,
  //  votes: Votes,
}


pub fn feedType2seconds(feed_type: feed::FeedType)->u64{
    match feed_type {
    //    feed::FeedType::AllTime =>None,
    //    feed::FeedType::Emergency=>Some("Emergency"),
    //    feed::FeedType::LastActivity=>Some("LastActivity"),
        feed::FeedType::LastDay=>60*60*24,
        feed::FeedType::LastMonth=>60*60*24*30,
        feed::FeedType::LastWeek=>60*60*24*7,
        feed::FeedType::LastYear=>60*60*24*365,
        _ =>0
    }
}

//convid,score

pub async fn cache_init(keydb_pool: Pool<RedisConnectionManager>,mongo_client:&MongoClient){
    // let keydb_pool = keydb_pool.clone();
    let mut keydb_conn = keydb_pool.get().await.expect("keydb_pool failed");

  //  println!("ICIIIIIIIIIII");
// List the names of the databases in that deployment.


    let conversations = mongo_client.database("DB")
    .collection::<ConversationRank>("convs");


    let projection = doc! {
        "_id": i32::from(1),
   //     "upvote": i32::from(1),
   //     "downvote": i32::from(1),
  // "votes":i32::from(1),
  "score":i32::from(1),
        "created_at":  i32::from(1),
    };

    let options = FindOptions::builder().projection(projection).build();

    let mut cursor = conversations.find(doc!{"private":false},
        options
        
        ).await.unwrap();

let current_timestamp = get_epoch();
let all_time_table=feedType2cacheTable(feed::FeedType::AllTime).unwrap();
while let Some(result) = cursor.next().await {
    println!("RESULT {:#?}",result);
    match result {
        Ok(result) => {
            let convid= &result._id.to_string();
            for feed_type in TIMEFEEDTYPES {
                let expiration = result.created_at+ feedType2seconds(feed_type);
                
              //  println!("{}",current_timestamp);
                if current_timestamp< expiration {
                    let cache_table=feedType2cacheTable(feed_type).unwrap();
                    
                    //todo:chunk+transactions
                    println!("{} {} {}",cache_table,convid,expiration);
                 let _:()=   cmd("zadd")
                    .arg(&[cache_table,
                        &result.score.to_string(),
             convid  ]).query_async(&mut *keydb_conn).await.expect("zadd error");


               let _:()= cmd("expirememberat")
                .arg(&[cache_table,
                    convid,
          &expiration.to_string()]).query_async(&mut *keydb_conn).await.expect("expirememberat error");
    //      println!("{:#?}",res);
                    
                } else {
                    break;
                }

            }

            let _:()=   cmd("zadd")
            .arg(&[all_time_table,
                &(result.score).to_string(),
     convid  ]).query_async(&mut *keydb_conn).await.expect("zadd error");



        }
        Err(_) => {
            break;
        }
}


 }

//TODO: emergency table
}
 
 
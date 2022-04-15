#[macro_use(bson, doc)]
use bb8_redis::{bb8::Pool, RedisConnectionManager};
use mongodb::{Client as MongoClient, options::{ClientOptions, DriverInfo, Credential, ServerAddress}, bson::Bson};
use redis::{cmd, RedisResult};

use crate::{service::feedType2cacheTable, api::feed};
use std::time::Duration; 
use mongodb::{bson::doc, bson::oid::ObjectId, options::FindOptions, Collection};
use futures::{stream::StreamExt, TryStreamExt};

use serde::{Deserialize, Serialize};

//must be ordered
const TIMEFEEDTYPES: [feed::FeedType; 4]  = [
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
    convid: i32,
    upvote: i32,
    downvote: i32,
    created_at: i64,
}


pub fn feedType2seconds(feed_type: feed::FeedType)->i64{
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
        "convid": i32::from(1),
        "upvote": i32::from(1),
        "downvote": i32::from(1),
        "created_at":  i32::from(1),
    };

    let options = FindOptions::builder().projection(projection).build();

    let mut cursor = conversations.find(doc!{"private":false},
        options
        
        ).await.unwrap();
//optim
let current_timestamp = chrono::offset::Local::now().timestamp();
let all_time_table=feedType2cacheTable(feed::FeedType::AllTime).unwrap();
while let Some(result) = cursor.next().await {
    match result {
        Ok(result) => {
            let convid= &result.convid.to_string();
            for feed_type in TIMEFEEDTYPES {
                let expiration = result.created_at+ feedType2seconds(feed_type);
                
                println!("{}",current_timestamp);
                if current_timestamp< expiration {
                    let cache_table=feedType2cacheTable(feed_type).unwrap();
                    
                    //todo:chunk+transactions
                    println!("{} {} {}",cache_table,convid,expiration);
                 let _:()=   cmd("zadd")
                    .arg(&[cache_table,
                        &(result.upvote-result.downvote).to_string(),
             convid  ]).query_async(&mut *keydb_conn).await.expect("zadd error");


               let _:()= cmd("expirememberat")
                .arg(&[cache_table,
                    &result.convid.to_string(),
          &expiration.to_string()]).query_async(&mut *keydb_conn).await.expect("expirememberat error");
    //      println!("{:#?}",res);
                    
                } else {
                    break;
                }

            }

            let _:()=   cmd("zadd")
            .arg(&[all_time_table,
                &(result.upvote-result.downvote).to_string(),
     convid  ]).query_async(&mut *keydb_conn).await.expect("zadd error");



        }
        Err(_) => {
            break;
        }
}


 }

//TODO: emergency table
}
 
 
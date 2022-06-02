
use bb8_redis::{bb8::Pool, RedisConnectionManager};
use mongodb::Client as MongoClient;
use redis::cmd;

use crate::{service::{feed_type2cache_table, EmergencyEntry}, api::{feed}};
use std::time::{ SystemTime, UNIX_EPOCH}; 
use mongodb::bson::doc;
use futures::stream::StreamExt;
//use  bson::serde_helpers::serialize_hex_string_as_object_id;
use serde::{Deserialize, Serialize};

pub const  EMERGENCY_DURATION:u64=60*60*24;

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
pub struct MiniMeta {
  created_at: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ConversationRank {
    // #[serde(serialize_with = "serialize_hex_string_as_object_id")]
    convid: String,
  //  upvote: i32,
  //  downvote: i32,
 //   metadata : MiniMeta,
//    metadata_created_at: u64,
created_at: u64,
    score: i32,
  //  votes: Votes,
}


pub fn feed_type2seconds(feed_type: &feed::FeedType)->u64{
    match feed_type {
    //    feed::FeedType::AllTime =>None,
    //    feed::FeedType::Emergency=>60*60*24,
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



//"$match":{"private":false}
    let mut cursor = conversations.aggregate(
      vec![doc!{"$match":{}},
      doc! { "$project": {
        "created_at": "$metadata.created_at",
        "convid": { "$toString": "$_id"},
         "score":i32::from(1),}}
         ],
    None    
        
        ).await.unwrap();

let current_timestamp = get_epoch();
let all_time_table=feed_type2cache_table(&feed::FeedType::AllTime);
while let Some(result) = cursor.next().await {
//  println!("RESULT {:#?}",result);
    let result:ConversationRank = bson::from_document(result.unwrap()).unwrap();
    println!("RESULT {:#?}",result);

            for feed_type in &TIMEFEEDTYPES {
                let expiration = result.created_at+ feed_type2seconds(feed_type);
                
              //  println!("{}",current_timestamp);
                if current_timestamp< expiration {
                    let cache_table=feed_type2cache_table(feed_type);
                    
                    //todo:chunk+transactions
                    println!("{} {} {}",cache_table,&result.convid,expiration);
                 let _:()=   cmd("zadd")
                    .arg(cache_table).arg(
                        &result.score).arg(
                        &result.convid ).query_async(&mut *keydb_conn).await.expect("zadd error");


               let _:()= cmd("expirememberat")
                .arg(cache_table).arg(
                  &result.convid)
          .arg(expiration).query_async(&mut *keydb_conn).await.expect("expirememberat error");
    //      println!("{:#?}",res);
                    
                } else {
                    break;
                }

            }

            let _:()=   cmd("zadd")
            .arg(all_time_table).arg(
                &result.score).arg(
                &result.convid ).query_async(&mut *keydb_conn).await.expect("zadd error");





 }



//TODO: emergency table

let emergency = mongo_client.database("DB")
.collection::<EmergencyEntry>("emergency");



//"$match":{"private":false}

let min_created_at=current_timestamp-EMERGENCY_DURATION;
let mut cursor = emergency.aggregate(
  vec![doc!
  {"$match": {"add_time":{"$gt":i64::try_from(min_created_at).unwrap()}}}
  ,
  doc! { "$project": {
    "add_time": i32::from(1),
    "convid": i32::from(1),
     "amount":i32::from(1),}}
     ],
None    
    
    ).await.unwrap();

    while let Some(result) = cursor.next().await {

      let result:EmergencyEntry = bson::from_document(result.unwrap()).unwrap();
                       
      let expiration=result.add_time+EMERGENCY_DURATION;
    //  let cacheid=result.convid+&expiration.to_string();
      let cacheid=result.convid;
                          let _:()=   cmd("zadd")
                             .arg(feed_type2cache_table(&feed::FeedType::Emergency)).arg(
                                 &result.amount).arg(
                                 &cacheid ).query_async(&mut *keydb_conn).await.expect("zadd error");
         
         
                        let _:()= cmd("expirememberat")
                         .arg(feed_type2cache_table(&feed::FeedType::Emergency)).arg(
                           &cacheid)
                   .arg(expiration).query_async(&mut *keydb_conn).await.expect("expirememberat error");
             //      println!("{:#?}",res);

    }

}
 
 
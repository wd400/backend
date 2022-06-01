use crate::service::MyApi;
use api::v1::api_server::ApiServer;
use std::{env, time::Duration};
use jsonwebtoken::{ Algorithm, Validation, EncodingKey, DecodingKey};

use aws_config::meta::region::RegionProviderChain;
use aws_sdk_dynamodb::{Client as DynamoClient};

pub mod api {
    tonic_include_protos::include_protos!("v1");
    pub(crate) const FILE_DESCRIPTOR_SET: &[u8] =
    tonic::include_file_descriptor_set!("api_pb");
}
use regex::Regex;
use stripe::Client as StripeClient;

use aws_sdk_s3::{Client as S3Client,};


use bb8_redis::{
    bb8,
    RedisConnectionManager
};

use mongodb::{Client as MongoClient, options::{ClientOptions, Credential, ServerAddress}};

const REGEX:&str=r"xx|ass|cum|fag|fat|fuk|fux|gae|gai|gay|gey|gfy|god|hiv|jap|jiz|kkk|kum|len|lez|nad|nig|nob|omg|pcp|pee|bbw|sex|tit|pms|pot|rum|sob|uzi|vag|wad|wop|wtf|admin|anal|anus|arse|bdsm|boob|cawk|cipa|clit|cnut|cock|coon|crap|cunt|damn|dick|dink|dlck|dong|dyke|fack|faig|fart|fcuk|feck|foad|fook|fuck|fxck|ghay|ghey|gook|gtfo|hebe|heeb|hell|hemp|herp|hoar|hoer|homo|hoor|hore|hump|jerk|jism|kawk|kike|kill|klan|kock|kwif|kyke|lech|lmao|loin|lube|lust|mams|maxi|meth|milf|mofo|muff|nazi|nude|oral|orgy|ovum|paki|pawn|pedo|phuk|phuq|pimp|piss|2g1c|butt|dvda|guro|poof|poon|porn|pthc|quim|rape|scat|shit|slut|smut|spic|suck|twat|wank|yaoi|poop|prig|pron|pube|puss|puto|racy|scag|shag|shiz|skag|spac|spik|stfu|tard|teat|teez|terd|thug|toke|turd|tush|ugly|wang|weed|whiz|womb|yury|arian|arrse|aryan|bimbo|bitch|boner|busty|chink|dildo|doosh|dopey|drunk|duche|dummy|erect|fanny|fanyy|felch|fisty|freex|frigg|fubar|ganja|glans|guido|heshe|hobag|homey|honky|hooch|horny|hussy|hymen|injun|junky|kinky|kooch|kraut|labia|leper|lesbo|lmfao|moron|mutha|naked|nappy|negro|ninny|nooky|opium|organ|ovary|paddy|panty|pasty|penis|phuck|pinko|ecchi|fecal|grope|juggs|queaf|queef|semen|shota|skeet|spunk|twink|vulva|yiffy|prick|prude|pubic|pubis|punky|queer|reich|revue|ruski|screw|scrog|scrot|scrud|sissy|skank|slave|slope|snuff|sodom|souse|sperm|strip|teets|teste|toots|tramp|twunt|unwed|urine|vigra|vixen|vodka|vomit|wazoo|wench|willy|woody|woose|areole|booobs|buceta|bukake|crotch|doggin|doofus|douche|erotic|extacy|extasy|facial|feltch|femdom|fisted|flange|floozy|fondle|foobar|gigolo|goatse|gokkun|gringo|hardon|hentai|heroin|hitler|hookah|hooker|hootch|hooter|inbred|incest|junkie|kondum|kootch|l3itch|menses|molest|moolie|murder|muther|napalm|nimrod|nipple|nympho|opiate|orgasm|orgies|pantie|pastie|pecker|penial|penile|peyote|phalli|beaner|darkie|dommes|escort|eunuch|goatcx|honkey|lolita|nambla|nudity|punany|raping|rapist|rectum|rimjob|sadism|snatch|spooge|cancer|tosser|tranny|voyeur|polack|quicky|raunch|rectal|rectus|reefer|rimjaw|sadist|schizo|scroat|seaman|seamen|seduce|sleaze|sleazy|smegma|sniper|steamy|stiffy|stoned|stroke|stupid|tampon|tawdry|testis|thrust|tinkle|trashy|undies|urinal|uterus|valium|viagra|virgin|vulgar|wedgie|weenie|weewee|weiner|weirdo|whitey|wigger|bestial|blowjob|bondage|boooobs|breasts|bukkake|cowgirl|erotism|fellate|fisting|footjob|hamflap|handjob|jackoff|kinbaku|nutsack|orgasim|camgirl|dolcett|figging|jigaboo|nawashi|pegging|playboy|raghead|rimming|schlong|shemale|shibari|splooge|strapon|swinger|topless|tubgirl|upskirt|wetback|pollock|sandbar|shibary|whoring|willies|asanchez|ballsack|beastial|booooobs|essohbee|fellatio|foreskin|futanari|futanary|gangbang|horniest|jackhole|lesbians|masterb8|numbnuts|babeland|bangbros|bareback|birdlock|blumpkin|bollocks|bunghole|cornhole|creampie|frotting|genitals|goregasm|hardcore|jailbait|jiggaboo|kinkster|omorashi|ponyplay|slanteye|swastika|vibrator|scantily|testical|testicle|ejaculate|ejakulate|fingering|masochist|penetrate|anilingus|jiggerboo|shrimping|strappado|threesome|throating|towelhead|tribadism|urophilia|zoophilia|shamedame|booooooobs|cokmuncher|cunilingus|deepthroat|doggystyle|eathairpie|flogthelog|kunilingus|masterbate|masturbate|menstruate|perversion|domination|dominatrix|fingerbang|lovemaking|paedophile|scissoring|undressing|teabagging|cunillingus|cunnilingus|doggiestyle|ejaculating|ejaculation|enlargement|fudgepacker|howtomurdep|penetration|pillowbiter|coprolagnia|coprophilia|dingleberry|intercourse|nimphomania|snowballing|donkeyribber|goldenshower|masterbating|masterbation|masturbating|masturbation|menstruation|dendrophilia|vorarephilia|sausagequeen|sumofabiatch|carpetmuncher|dingleberries|acrotomophilia";

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
  //      .retry_writes(false)
  //      .retry_reads(false)
        
        .build()).unwrap();

    

     let addr = ([0, 0, 0, 0], 3000).into();

     let reflection = tonic_reflection::server::Builder::configure()
     .register_encoded_file_descriptor_set(api::FILE_DESCRIPTOR_SET)
     .build().unwrap();

     let manager = RedisConnectionManager::new("redis://cache:6379").unwrap();
     let keydb_pool = bb8::Pool::builder().max_size(100).max_lifetime(Some(Duration::from_secs(1))).build(manager).await.unwrap();
 

     cache_init::cache_init(keydb_pool.clone(), &mongo_client.clone()).await;

   //  const JWT_TOKEN:EncodingKey=EncodingKey::from_secret("test".as_ref());

   let region_provider = RegionProviderChain::default_provider().or_else("us-east-1");
   let config = aws_config::from_env().region(region_provider).load().await;

   let s3_client: S3Client= S3Client::new(&config);
   let dynamo_client:DynamoClient = DynamoClient::new(&config);

   let jwt_secret=env::var("JWT_SECRET").expect("JWT_SECRET");
let algo=Validation::new(Algorithm::HS256);

let stripe_key=env::var("STRIPE_SECRET_KEY").expect("STRIPE_SECRET_KEY");

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
       regex:Regex::new(REGEX).unwrap(),
       mongo_client:mongo_client,
       stripe_client:StripeClient::new(&stripe_key),
       stripe_key:stripe_key
    });


   //  let api_server = tonic_web::enable(api_server);

   tonic::transport::Server::builder()
         .add_service(api_server )
        .add_service(reflection)
         .serve(addr)
         .await?;
     Ok(())
 }
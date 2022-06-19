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
use stripe::Client as StripeClient;

use aws_sdk_s3::{Client as S3Client,};


use bb8_redis::{
    bb8,
    RedisConnectionManager
};

use mongodb::{Client as MongoClient, options::{ClientOptions, Credential, ServerAddress}};

//const REGEX:&str=r"xx|ass|cum|fag|fat|fuk|fux|gae|gai|gay|gey|gfy|god|hiv|jap|jiz|kkk|kum|len|lez|nad|nig|nob|omg|pcp|pee|bbw|sex|tit|pms|pot|rum|sob|uzi|vag|wad|wop|wtf|admin|anal|anus|arse|bdsm|boob|cawk|cipa|clit|cnut|cock|coon|crap|cunt|damn|dick|dink|dlck|dong|dyke|fack|faig|fart|fcuk|feck|foad|fook|fuck|fxck|ghay|ghey|gook|gtfo|hebe|heeb|hell|hemp|herp|hoar|hoer|homo|hoor|hore|hump|jerk|jism|kawk|kike|kill|klan|kock|kwif|kyke|lech|lmao|loin|lube|lust|mams|maxi|meth|milf|mofo|muff|nazi|nude|oral|orgy|ovum|paki|pawn|pedo|phuk|phuq|pimp|piss|2g1c|butt|dvda|guro|poof|poon|porn|pthc|quim|rape|scat|shit|slut|smut|spic|suck|twat|wank|yaoi|poop|prig|pron|pube|puss|puto|racy|scag|shag|shiz|skag|spac|spik|stfu|tard|teat|teez|terd|thug|toke|turd|tush|ugly|wang|weed|whiz|womb|yury|arian|arrse|aryan|bimbo|bitch|boner|busty|chink|dildo|doosh|dopey|drunk|duche|dummy|erect|fanny|fanyy|felch|fisty|freex|frigg|fubar|ganja|glans|guido|heshe|hobag|homey|honky|hooch|horny|hussy|hymen|injun|junky|kinky|kooch|kraut|labia|leper|lesbo|lmfao|moron|mutha|naked|nappy|negro|ninny|nooky|opium|organ|ovary|paddy|panty|pasty|penis|phuck|pinko|ecchi|fecal|grope|juggs|queaf|queef|semen|shota|skeet|spunk|twink|vulva|yiffy|prick|prude|pubic|pubis|punky|queer|reich|revue|ruski|screw|scrog|scrot|scrud|sissy|skank|slave|slope|snuff|sodom|souse|sperm|strip|teets|teste|toots|tramp|twunt|unwed|urine|vigra|vixen|vodka|vomit|wazoo|wench|willy|woody|woose|areole|booobs|buceta|bukake|crotch|doggin|doofus|douche|erotic|extacy|extasy|facial|feltch|femdom|fisted|flange|floozy|fondle|foobar|gigolo|goatse|gokkun|gringo|hardon|hentai|heroin|hitler|hookah|hooker|hootch|hooter|inbred|incest|junkie|kondum|kootch|l3itch|menses|molest|moolie|murder|muther|napalm|nimrod|nipple|nympho|opiate|orgasm|orgies|pantie|pastie|pecker|penial|penile|peyote|phalli|beaner|darkie|dommes|escort|eunuch|goatcx|honkey|lolita|nambla|nudity|punany|raping|rapist|rectum|rimjob|sadism|snatch|spooge|cancer|tosser|tranny|voyeur|polack|quicky|raunch|rectal|rectus|reefer|rimjaw|sadist|schizo|scroat|seaman|seamen|seduce|sleaze|sleazy|smegma|sniper|steamy|stiffy|stoned|stroke|stupid|tampon|tawdry|testis|thrust|tinkle|trashy|undies|urinal|uterus|valium|viagra|virgin|vulgar|wedgie|weenie|weewee|weiner|weirdo|whitey|wigger|bestial|blowjob|bondage|boooobs|breasts|bukkake|cowgirl|erotism|fellate|fisting|footjob|hamflap|handjob|jackoff|kinbaku|nutsack|orgasim|camgirl|dolcett|figging|jigaboo|nawashi|pegging|playboy|raghead|rimming|schlong|shemale|shibari|splooge|strapon|swinger|topless|tubgirl|upskirt|wetback|pollock|sandbar|shibary|whoring|willies|asanchez|ballsack|beastial|booooobs|essohbee|fellatio|foreskin|futanari|futanary|gangbang|horniest|jackhole|lesbians|masterb8|numbnuts|babeland|bangbros|bareback|birdlock|blumpkin|bollocks|bunghole|cornhole|creampie|frotting|genitals|goregasm|hardcore|jailbait|jiggaboo|kinkster|omorashi|ponyplay|slanteye|swastika|vibrator|scantily|testical|testicle|ejaculate|ejakulate|fingering|masochist|penetrate|anilingus|jiggerboo|shrimping|strappado|threesome|throating|towelhead|tribadism|urophilia|zoophilia|shamedame|booooooobs|cokmuncher|cunilingus|deepthroat|doggystyle|eathairpie|flogthelog|kunilingus|masterbate|masturbate|menstruate|perversion|domination|dominatrix|fingerbang|lovemaking|paedophile|scissoring|undressing|teabagging|cunillingus|cunnilingus|doggiestyle|ejaculating|ejaculation|enlargement|fudgepacker|howtomurdep|penetration|pillowbiter|coprolagnia|coprophilia|dingleberry|intercourse|nimphomania|snowballing|donkeyribber|goldenshower|masterbating|masterbation|masturbating|masturbation|menstruation|dendrophilia|vorarephilia|sausagequeen|sumofabiatch|carpetmuncher|dingleberries|acrotomophilia";

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

let  bad_words:[String;525]=[String::from("gfy"),String::from("god"),String::from("hiv"),String::from("jap"),String::from("jiz"),String::from("kkk"),String::from("kum"),String::from("len"),String::from("lez"),String::from("nad"),String::from("nig"),String::from("nob"),String::from("omg"),String::from("pcp"),String::from("pee"),String::from("bbw"),String::from("sex"),String::from("tit"),String::from("pms"),String::from("pot"),String::from("rum"),String::from("sob"),String::from("uzi"),String::from("vag"),String::from("wad"),String::from("wop"),String::from("wtf"),String::from("admin"),String::from("anal"),String::from("anus"),String::from("arse"),String::from("bdsm"),String::from("boob"),String::from("cawk"),String::from("cipa"),String::from("clit"),String::from("cnut"),String::from("cock"),String::from("coon"),String::from("crap"),String::from("cunt"),String::from("damn"),String::from("dick"),String::from("dink"),String::from("dlck"),String::from("dong"),String::from("dyke"),String::from("fack"),String::from("faig"),String::from("fart"),String::from("fcuk"),String::from("feck"),String::from("foad"),String::from("fook"),String::from("fuck"),String::from("fxck"),String::from("ghay"),String::from("ghey"),String::from("gook"),String::from("gtfo"),String::from("hebe"),String::from("heeb"),String::from("hell"),String::from("hemp"),String::from("herp"),String::from("hoar"),String::from("hoer"),String::from("homo"),String::from("hoor"),String::from("hore"),String::from("hump"),String::from("jerk"),String::from("jism"),String::from("kawk"),String::from("kike"),String::from("kill"),String::from("klan"),String::from("kock"),String::from("kwif"),String::from("kyke"),String::from("lech"),String::from("lmao"),String::from("loin"),String::from("lube"),String::from("lust"),String::from("mams"),String::from("maxi"),String::from("meth"),String::from("milf"),String::from("mofo"),String::from("muff"),String::from("nazi"),String::from("nude"),String::from("oral"),String::from("orgy"),String::from("ovum"),String::from("paki"),String::from("pawn"),String::from("pedo"),String::from("phuk"),String::from("phuq"),String::from("pimp"),String::from("piss"),String::from("2g1c"),String::from("butt"),String::from("dvda"),String::from("guro"),String::from("poof"),String::from("poon"),String::from("porn"),String::from("pthc"),String::from("quim"),String::from("rape"),String::from("scat"),String::from("shit"),String::from("slut"),String::from("smut"),String::from("spic"),String::from("suck"),String::from("twat"),String::from("wank"),String::from("yaoi"),String::from("poop"),String::from("prig"),String::from("pron"),String::from("pube"),String::from("puss"),String::from("puto"),String::from("racy"),String::from("scag"),String::from("shag"),String::from("shiz"),String::from("skag"),String::from("spac"),String::from("spik"),String::from("stfu"),String::from("tard"),String::from("teat"),String::from("teez"),String::from("terd"),String::from("thug"),String::from("toke"),String::from("turd"),String::from("tush"),String::from("ugly"),String::from("wang"),String::from("weed"),String::from("whiz"),String::from("womb"),String::from("yury"),String::from("arian"),String::from("arrse"),String::from("aryan"),String::from("bimbo"),String::from("bitch"),String::from("boner"),String::from("busty"),String::from("chink"),String::from("dildo"),String::from("doosh"),String::from("dopey"),String::from("drunk"),String::from("duche"),String::from("dummy"),String::from("erect"),String::from("fanny"),String::from("fanyy"),String::from("felch"),String::from("fisty"),String::from("freex"),String::from("frigg"),String::from("fubar"),String::from("ganja"),String::from("glans"),String::from("guido"),String::from("heshe"),String::from("hobag"),String::from("homey"),String::from("honky"),String::from("hooch"),String::from("horny"),String::from("hussy"),String::from("hymen"),String::from("injun"),String::from("junky"),String::from("kinky"),String::from("kooch"),String::from("kraut"),String::from("labia"),String::from("leper"),String::from("lesbo"),String::from("lmfao"),String::from("moron"),String::from("mutha"),String::from("naked"),String::from("nappy"),String::from("negro"),String::from("ninny"),String::from("nooky"),String::from("opium"),String::from("organ"),String::from("ovary"),String::from("paddy"),String::from("panty"),String::from("pasty"),String::from("penis"),String::from("phuck"),String::from("pinko"),String::from("ecchi"),String::from("fecal"),String::from("grope"),String::from("juggs"),String::from("queaf"),String::from("queef"),String::from("semen"),String::from("shota"),String::from("skeet"),String::from("spunk"),String::from("twink"),String::from("vulva"),String::from("yiffy"),String::from("prick"),String::from("prude"),String::from("pubic"),String::from("pubis"),String::from("punky"),String::from("queer"),String::from("reich"),String::from("revue"),String::from("ruski"),String::from("screw"),String::from("scrog"),String::from("scrot"),String::from("scrud"),String::from("sissy"),String::from("skank"),String::from("slave"),String::from("slope"),String::from("snuff"),String::from("sodom"),String::from("souse"),String::from("sperm"),String::from("strip"),String::from("teets"),String::from("teste"),String::from("toots"),String::from("tramp"),String::from("twunt"),String::from("unwed"),String::from("urine"),String::from("vigra"),String::from("vixen"),String::from("vodka"),String::from("vomit"),String::from("wazoo"),String::from("wench"),String::from("willy"),String::from("woody"),String::from("woose"),String::from("areole"),String::from("booobs"),String::from("buceta"),String::from("bukake"),String::from("crotch"),String::from("doggin"),String::from("doofus"),String::from("douche"),String::from("erotic"),String::from("extacy"),String::from("extasy"),String::from("facial"),String::from("feltch"),String::from("femdom"),String::from("fisted"),String::from("flange"),String::from("floozy"),String::from("fondle"),String::from("foobar"),String::from("gigolo"),String::from("goatse"),String::from("gokkun"),String::from("gringo"),String::from("hardon"),String::from("hentai"),String::from("heroin"),String::from("hitler"),String::from("hookah"),String::from("hooker"),String::from("hootch"),String::from("hooter"),String::from("inbred"),String::from("incest"),String::from("junkie"),String::from("kondum"),String::from("kootch"),String::from("l3itch"),String::from("menses"),String::from("molest"),String::from("moolie"),String::from("murder"),String::from("muther"),String::from("napalm"),String::from("nimrod"),String::from("nipple"),String::from("nympho"),String::from("opiate"),String::from("orgasm"),String::from("orgies"),String::from("pantie"),String::from("pastie"),String::from("pecker"),String::from("penial"),String::from("penile"),String::from("peyote"),String::from("phalli"),String::from("beaner"),String::from("darkie"),String::from("dommes"),String::from("escort"),String::from("eunuch"),String::from("goatcx"),String::from("honkey"),String::from("lolita"),String::from("nambla"),String::from("nudity"),String::from("punany"),String::from("raping"),String::from("rapist"),String::from("rectum"),String::from("rimjob"),String::from("sadism"),String::from("snatch"),String::from("spooge"),String::from("cancer"),String::from("tosser"),String::from("tranny"),String::from("voyeur"),String::from("polack"),String::from("quicky"),String::from("raunch"),String::from("rectal"),String::from("rectus"),String::from("reefer"),String::from("rimjaw"),String::from("sadist"),String::from("schizo"),String::from("scroat"),String::from("seaman"),String::from("seamen"),String::from("seduce"),String::from("sleaze"),String::from("sleazy"),String::from("smegma"),String::from("sniper"),String::from("steamy"),String::from("stiffy"),String::from("stoned"),String::from("stroke"),String::from("stupid"),String::from("tampon"),String::from("tawdry"),String::from("testis"),String::from("thrust"),String::from("tinkle"),String::from("trashy"),String::from("undies"),String::from("urinal"),String::from("uterus"),String::from("valium"),String::from("viagra"),String::from("virgin"),String::from("vulgar"),String::from("wedgie"),String::from("weenie"),String::from("weewee"),String::from("weiner"),String::from("weirdo"),String::from("whitey"),String::from("wigger"),String::from("bestial"),String::from("blowjob"),String::from("bondage"),String::from("boooobs"),String::from("breasts"),String::from("bukkake"),String::from("cowgirl"),String::from("erotism"),String::from("fellate"),String::from("fisting"),String::from("footjob"),String::from("hamflap"),String::from("handjob"),String::from("jackoff"),String::from("kinbaku"),String::from("nutsack"),String::from("orgasim"),String::from("camgirl"),String::from("dolcett"),String::from("figging"),String::from("jigaboo"),String::from("nawashi"),String::from("pegging"),String::from("playboy"),String::from("raghead"),String::from("rimming"),String::from("schlong"),String::from("shemale"),String::from("shibari"),String::from("splooge"),String::from("strapon"),String::from("swinger"),String::from("topless"),String::from("tubgirl"),String::from("upskirt"),String::from("wetback"),String::from("pollock"),String::from("sandbar"),String::from("shibary"),String::from("whoring"),String::from("willies"),String::from("asanchez"),String::from("ballsack"),String::from("beastial"),String::from("booooobs"),String::from("essohbee"),String::from("fellatio"),String::from("foreskin"),String::from("futanari"),String::from("futanary"),String::from("gangbang"),String::from("horniest"),String::from("jackhole"),String::from("lesbians"),String::from("masterb8"),String::from("numbnuts"),String::from("babeland"),String::from("bangbros"),String::from("bareback"),String::from("birdlock"),String::from("blumpkin"),String::from("bollocks"),String::from("bunghole"),String::from("cornhole"),String::from("creampie"),String::from("frotting"),String::from("genitals"),String::from("goregasm"),String::from("hardcore"),String::from("jailbait"),String::from("jiggaboo"),String::from("kinkster"),String::from("omorashi"),String::from("ponyplay"),String::from("slanteye"),String::from("swastika"),String::from("vibrator"),String::from("scantily"),String::from("testical"),String::from("testicle"),String::from("ejaculate"),String::from("ejakulate"),String::from("fingering"),String::from("masochist"),String::from("penetrate"),String::from("anilingus"),String::from("jiggerboo"),String::from("shrimping"),String::from("strappado"),String::from("threesome"),String::from("throating"),String::from("towelhead"),String::from("tribadism"),String::from("urophilia"),String::from("zoophilia"),String::from("shamedame"),String::from("booooooobs"),String::from("cokmuncher"),String::from("cunilingus"),String::from("deepthroat"),String::from("doggystyle"),String::from("eathairpie"),String::from("flogthelog"),String::from("kunilingus"),String::from("masterbate"),String::from("masturbate"),String::from("menstruate"),String::from("perversion"),String::from("domination"),String::from("dominatrix"),String::from("fingerbang"),String::from("lovemaking"),String::from("paedophile"),String::from("scissoring"),String::from("undressing"),String::from("teabagging"),String::from("cunillingus"),String::from("cunnilingus"),String::from("doggiestyle"),String::from("ejaculating"),String::from("ejaculation"),String::from("enlargement"),String::from("fudgepacker"),String::from("howtomurdep"),String::from("penetration"),String::from("pillowbiter"),String::from("coprolagnia"),String::from("coprophilia"),String::from("dingleberry"),String::from("intercourse"),String::from("nimphomania"),String::from("snowballing"),String::from("donkeyribber"),String::from("goldenshower"),String::from("masterbating"),String::from("masterbation"),String::from("masturbating"),String::from("masturbation"),String::from("menstruation"),String::from("dendrophilia"),String::from("vorarephilia"),String::from("sausagequeen"),String::from("sumofabiatch"),String::from("carpetmuncher"),String::from("dingleberries"),String::from("acrotomophilia")];



   let api_server = ApiServer::new(MyApi{
       jwt_encoding_key:EncodingKey::from_secret(jwt_secret.as_ref()),
       jwt_decoding_key:DecodingKey::from_secret(jwt_secret.as_ref()),
       jwt_algo:algo,
    //   google_client_id:env::var("GOOGLE_CLIENT_ID").expect("GOOGLE_CLIENT_ID"),
       path_salt:   env::var("PATH_SALT").expect("PATH_SALT"), 
       dynamodb_client: dynamo_client,
       s3_client:s3_client,
     //  userid_salt:env::var("USERID_SALT").expect("USERID_SALT"),
  //     cache:Cache::new(10_000),

       keydb_pool:keydb_pool,
       bad_words:bad_words,
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
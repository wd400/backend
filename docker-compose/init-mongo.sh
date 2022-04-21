#https://www.elastic.co/guide/en/beats/metricbeat/current/metricbeat-module-mongodb.htm
set -e
#use $MONGO_INITDB_DATABASE

#
mongo admin -u "$MONGODB_ROOT_USER" -p "$MONGODB_ROOT_PASSWORD" <<EOF
db.createUser(
    {
        user: "beats",
        pwd: '$MONGO_BEATS_PWD',
        roles: ["clusterMonitor"]
    }
);

db = new Mongo().getDB("DB");
db.createCollection("convs", { capped: false });
db.createCollection("users", { capped: false });
db.createCollection("replies", { capped: false });
db.createCollection("conv_votes", { capped: false });
db.createCollection("reply_votes", { capped: false });

//conv text search
db.convs.createIndex({title:"text",description:"text"});
//last user convs
db.convs.createIndex({"pseudo":1,"created_at":1});

//pseudo search
db.users.createIndex({pseudo:"text"});
//unique
db.users.createIndex({"pseudo":1,"userid":1},{unique: true});

//last pseudo replies
db.replies.createIndex({"pseudo":1,"created_at":1});
//reply access
db.replies.createIndex({"convid":1,"boxid":1,"replyfrom":1,"score":1});
db.replies.createIndex({"convid":1,"boxid":1,"replyfrom":1,"score":-1});
db.replies.createIndex({"convid":1,"boxid":1,"replyfrom":1,"created_at":1});
db.replies.createIndex({"convid":1,"boxid":1,"replyfrom":1,"created_at":-1});

//list pseudo conv_votes
db.conv_votes.createIndex({"pseudo":1,"created_at":1});
//check if already voted
db.conv_votes.createIndex({"pseudo":1,"convid":1},{unique: true});

//list pseudo reply_votes
db.reply_votes.createIndex({"pseudo":1,"created_at":1});
//check if already voted
db.reply_votes.createIndex({"pseudo":1,"replyid":1},{unique: true});


use DB;
EOF

printf FIIIIIIIIIIIIIIIIIIIN


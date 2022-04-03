#https://www.elastic.co/guide/en/beats/metricbeat/current/metricbeat-module-mongodb.htm
set -e

mongo <<EOF


use $MONGO_INITDB_DATABASE

db.createUser(
    {
        user: "beats",
        pwd: '$MONGO_BEATS_PWD',
        roles: ["clusterMonitor"]
    }
)

db = new Mongo().getDB("testDB");

db.createCollection('users', { capped: false });




EOF
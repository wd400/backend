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
use DB;
EOF

printf FIIIIIIIIIIIIIIIIIIIN
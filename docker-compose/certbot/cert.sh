#!/bin/bash

certbot certonly --force-renew --standalone --agree-tos --non-interactive --email zacharie.bugaud@laposte.net --preferred-challenges http -d api.conv911.com

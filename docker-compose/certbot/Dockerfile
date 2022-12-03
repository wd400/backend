FROM certbot/certbot:latest 
WORKDIR /home

COPY cert.sh /home/cert.sh

RUN mkdir /var/www
RUN mkdir /var/www/certbot

ENTRYPOINT [ "/bin/sh", "-c", "trap exit TERM; sh /home/cert.sh; while :; do certbot renew  --force-renewal; sleep 48h; done;" ]

FROM certbot/certbot:latest 
WORKDIR /home

COPY cert.sh /home/cert.sh

RUN mkdir /var/www
RUN mkdir /var/www/certbot

ENTRYPOINT [ "/bin/sh", "-c", "trap exit TERM; while :; do certbot renew; sleep 12h; done;" ]
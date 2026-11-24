# Service `web` du projet Compose de test.
FROM docker.io/library/busybox:1.37
RUN mkdir -p /srv && echo ok > /srv/index.html
CMD ["httpd", "-f", "-p", "8080", "-h", "/srv"]

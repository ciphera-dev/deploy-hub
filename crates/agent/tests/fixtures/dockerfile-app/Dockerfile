# Application de test du build par Dockerfile (frontend dockerfile.v0).
FROM docker.io/library/busybox:1.37
COPY hello.txt /srv/hello.txt
RUN test -s /srv/hello.txt
CMD ["httpd", "-f", "-p", "8080", "-h", "/srv"]

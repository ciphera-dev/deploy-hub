# Cloisonnement des caches (build_it.rs) : monte les memes caches que
# cache-writer et note si son marqueur y est (vu) ou non (absent).
FROM docker.io/library/busybox:1.37
RUN --mount=type=cache,id=x,target=/cache-x \
    --mount=type=cache,target=/root/.npm \
    mkdir -p /srv \
    && if [ -e /cache-x/marker ]; then echo vu; else echo absent; fi > /srv/id-x.txt \
    && if [ -e /root/.npm/marker ]; then echo vu; else echo absent; fi > /srv/npm.txt
CMD ["cat", "/srv/id-x.txt", "/srv/npm.txt"]

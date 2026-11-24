# Cloisonnement des caches (build_it.rs) : ce projet ecrit un marqueur
# dans un cache de montage nomme x, puis dans un cache sans nom (son
# identifiant par defaut est le chemin cible, celui du cache de npm).
FROM docker.io/library/busybox:1.37
RUN --mount=type=cache,id=x,target=/cache-x echo marqueur > /cache-x/marker
RUN --mount=type=cache,target=/root/.npm echo marqueur > /root/.npm/marker

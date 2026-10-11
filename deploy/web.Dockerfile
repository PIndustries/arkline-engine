# Build with a reviewed staging directory containing web/, downloads/ and nginx.conf.
FROM nginxinc/nginx-unprivileged:1.28-alpine
COPY nginx.conf /etc/nginx/nginx.conf
COPY release.conf /etc/nginx/arkline-release.conf
COPY web/ /srv/web/
COPY downloads/ /srv/downloads/
EXPOSE 8080
USER 101:101
ENTRYPOINT ["nginx", "-g", "daemon off;"]

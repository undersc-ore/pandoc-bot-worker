FROM pandoc/latex:latest-ubuntu

RUN apt-get update && \
    apt-get install -y curl unzip && \
    curl -fsSL https://deno.land/x/install/install.sh | sh

ENV DENO_INSTALL="/root/.deno"
ENV PATH="/root/.deno/bin:${PATH}"

COPY ./main.ts /app/
WORKDIR /app/
ENTRYPOINT [ "deno", "run", "--allow-net", "--allow-run", "--allow-write", "--allow-read", "--allow-env", "/app/main.ts" ]

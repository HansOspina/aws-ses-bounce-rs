FROM rust:1.69
WORKDIR /usr/src/app
RUN apt-get update && apt-get install -y git
COPY . .
RUN cargo build --config net.git-fetch-with-cli=true --target x86_64-unknown-linux-musl --release && \
    cp target/x86_64-unknown-linux-musl/release/aws-ses-bounce /usr/local/bin/aws-ses-bounce
CMD ["aws-ses-bounce"]
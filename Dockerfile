FROM --platform=$BUILDPLATFORM rust:1.70.0-bookworm AS builder

RUN for a in armhf arm64 amd64; do dpkg --add-architecture $a; done \
  && apt update && apt install -y \
    build-essential \
    clang \
    cmake \
    crossbuild-essential-amd64 \
    crossbuild-essential-arm64 \
    crossbuild-essential-armhf \
    lld \
  && true  
RUN rustup target add aarch64-unknown-linux-musl x86_64-unknown-linux-musl armv7-unknown-linux-musleabihf
ENV CARGO_TARGET_ARMV7_UNKNOWN_LINUX_MUSLEABI_LINKER=/usr/bin/clang
ENV CARGO_TARGET_ARMV7_UNKNOWN_LINUX_MUSLEABI_RUSTFLAGS="-C link-arg=--ld-path=/usr/bin/ld.lld -C link-arg=--target=armv7-unknown-linux-musleabihf"
ENV CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=/usr/bin/clang
ENV CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_RUSTFLAGS="-C link-arg=--ld-path=/usr/bin/ld.lld -C link-arg=--target=aarch64-unknown-linux-musl"
ENV CARGO_TARGET_X86_64_UNKOWN_LINUX_MUSL_LINKER=/usr/bin/clang
ENV CARGO_TARGET_X86_64_UNKOWN_LINUX_MUSL_RUSTFLAGS="-C link-arg=--ld-path=/usr/bin/ld.lld -C link-arg=--target=x86_64-unknown-linux-musl"

WORKDIR /usr/src/pup
COPY Cargo.toml Cargo.lock ./
RUN cargo fetch --locked
COPY . .
ARG TARGETPLATFORM
RUN \
  set -eu; \
  if test $TARGETPLATFORM = linux/amd64; then \
	export CARGO_BUILD_TARGET=x86_64-unknown-linux-musl; \
  elif test $TARGETPLATFORM = linux/arm64; then \
	export CARGO_BUILD_TARGET=aarch64-unknown-linux-musl; \
  elif test $TARGETPLATFORM = linux/arm/v7; then \
	export CARGO_BUILD_TARGET=armv7-unknown-linux-musleabihf; \
  else \
	echo $TARGETPLATFORM not supported; \
    exit -1; \
  fi; \
  cargo build --release --bin pup; \
  cp target/$CARGO_BUILD_TARGET/release/pup target/pup

FROM scratch as final
COPY --from=builder /usr/src/pup/target/pup /pup
ENTRYPOINT ["/pup"]

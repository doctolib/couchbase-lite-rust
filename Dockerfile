FROM --platform=amd64 rust AS build_community
RUN apt-get update
RUN apt-get -y install clang
RUN mkdir /build
WORKDIR /build
ENV LIBCLANG_PATH=/usr/lib/llvm-11/lib/
ADD Cargo.toml Cargo.toml
ADD build.rs build.rs
ADD libcblite_community libcblite_community
ADD src src
RUN cargo c
RUN cargo test --features=community -- --test-threads=1

FROM --platform=amd64 rust AS build_enterprise
RUN apt-get update
RUN apt-get -y install clang
RUN mkdir /build
WORKDIR /build
ENV LIBCLANG_PATH=/usr/lib/llvm-11/lib/
ADD Cargo.toml Cargo.toml
ADD build.rs build.rs
ADD libcblite_enterprise libcblite_enterprise
ADD src src
RUN cargo c
RUN cargo test --features=enterprise -- --test-threads=1

FROM --platform=amd64 rust AS strip-stage_community
RUN apt-get update
RUN apt-get -y install binutils binutils-aarch64-linux-gnu
RUN mkdir /build
WORKDIR /build
ADD libcblite_community libcblite_community
RUN strip /build/libcblite_community/lib/x86_64-linux-android/libcblite.so -o /build/libcblite_community/lib/x86_64-linux-android/libcblite.stripped.so
RUN strip /build/libcblite_community/lib/i686-linux-android/libcblite.so -o /build/libcblite_community/lib/i686-linux-android/libcblite.stripped.so
RUN /usr/aarch64-linux-gnu/bin/strip /build/libcblite_community/lib/aarch64-linux-android/libcblite.so -o /build/libcblite_community/lib/aarch64-linux-android/libcblite.stripped.so
RUN /usr/aarch64-linux-gnu/bin/strip /build/libcblite_community/lib/arm-linux-androideabi/libcblite.so -o /build/libcblite_community/lib/arm-linux-androideabi/libcblite.stripped.so
RUN strip /build/libcblite_community/lib/x86_64-pc-windows-gnu/cblite.dll -o /build/libcblite_community/lib/x86_64-pc-windows-gnu/cblite.stripped.dll

FROM --platform=amd64 rust AS strip-stage_enterprise
RUN apt-get update
RUN apt-get -y install binutils binutils-aarch64-linux-gnu
RUN mkdir /build
WORKDIR /build
ADD libcblite_enterprise libcblite_enterprise
RUN strip /build/libcblite_enterprise/lib/x86_64-linux-android/libcblite.so -o /build/libcblite_enterprise/lib/x86_64-linux-android/libcblite.stripped.so
RUN strip /build/libcblite_enterprise/lib/i686-linux-android/libcblite.so -o /build/libcblite_enterprise/lib/i686-linux-android/libcblite.stripped.so
RUN /usr/aarch64-linux-gnu/bin/strip /build/libcblite_enterprise/lib/aarch64-linux-android/libcblite.so -o /build/libcblite_enterprise/lib/aarch64-linux-android/libcblite.stripped.so
RUN /usr/aarch64-linux-gnu/bin/strip /build/libcblite_enterprise/lib/arm-linux-androideabi/libcblite.so -o /build/libcblite_enterprise/lib/arm-linux-androideabi/libcblite.stripped.so
RUN strip /build/libcblite_enterprise/lib/x86_64-pc-windows-gnu/cblite.dll -o /build/libcblite_enterprise/lib/x86_64-pc-windows-gnu/cblite.stripped.dll

FROM scratch AS strip 
COPY --from=strip-stage_community /build/libcblite_community/ .
COPY --from=strip-stage_enterprise /build/libcblite_enterprise/ .

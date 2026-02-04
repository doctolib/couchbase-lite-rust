ARG PLATFORM=amd64
FROM --platform=${PLATFORM} rust@sha256:e35d0f677e0e0be6f4b4f93bf16e6f93ab4f427dc440a0ef12511026f8b7d7e3 AS strip-stage
ARG DIRNAME
RUN apt-get update
RUN apt-get -y install binutils binutils-aarch64-linux-gnu
RUN mkdir /build
WORKDIR /build
ADD ${DIRNAME} ${DIRNAME}
RUN strip /build/${DIRNAME}/lib/x86_64-linux-android/libcblite.so -o /build/${DIRNAME}/lib/x86_64-linux-android/libcblite.stripped.so
RUN strip /build/${DIRNAME}/lib/i686-linux-android/libcblite.so -o /build/${DIRNAME}/lib/i686-linux-android/libcblite.stripped.so
RUN /usr/aarch64-linux-gnu/bin/strip /build/${DIRNAME}/lib/aarch64-linux-android/libcblite.so -o /build/${DIRNAME}/lib/aarch64-linux-android/libcblite.stripped.so
RUN /usr/aarch64-linux-gnu/bin/strip /build/${DIRNAME}/lib/arm-linux-androideabi/libcblite.so -o /build/${DIRNAME}/lib/arm-linux-androideabi/libcblite.stripped.so
RUN strip /build/${DIRNAME}/lib/x86_64-pc-windows-gnu/cblite.dll -o /build/${DIRNAME}/lib/x86_64-pc-windows-gnu/cblite.stripped.dll

FROM scratch AS strip
ARG DIRNAME
COPY --from=strip-stage /build/${DIRNAME}/ ${DIRNAME}/

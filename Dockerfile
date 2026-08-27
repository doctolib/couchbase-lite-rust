ARG PLATFORM=amd64
FROM --platform=${PLATFORM} rust@sha256:271849e998ffce5776454bbf98c5dc21baafc854ff8e566197908d3aca9a81e8 AS strip-stage
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

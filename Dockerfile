FROM --platform=amd64 rust@sha256:5771a3cc2081935c59ac52b92d49c9e164d4fed92c9f6420aa8cc50364aead6e AS strip-stage
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
COPY --from=strip-stage /build/${DIRNAME}/ .

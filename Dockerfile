FROM ubuntu:24.04

# stop ubuntu-20 interactive options
ENV DEBIAN_FRONTEND noninteractive
ARG TARGETPLATFORM

# stop script if any individual command fails
RUN set -e

# LLVM version
ENV llvm_version=16.0.0

# SVF home directory
ENV HOME=/home/SVF-tools

# dependencies for SVF
ENV lib_deps="cmake g++ gcc git zlib1g-dev libncurses5-dev libtinfo6 build-essential libssl-dev libpcre2-dev zip libzstd-dev"
ENV build_deps="wget xz-utils git tcl software-properties-common"

# fetch 
RUN apt-get update --fix-missing
RUN apt-get install -y $build_deps $lib_deps

# eadsnakes PPA for multiple python versions 
RUN add-apt-repository ppa:deadsnakes/ppa
RUN apt-get update
RUN set -ex; \
    if [ "$TARGETPLATFORM" = "linux/amd64" ]; then \
        apt-get update && apt-get install -y python3.10-dev \
        && update-alternatives --install /usr/bin/python3 python3 /usr/bin/python3.10 1; \
    elif [ "$TARGETPLATFORM" = "linux/arm64" ]; then \
        apt-get update && apt-get install -y python3.8-dev \
        && update-alternatives --install /usr/bin/python3 python3 /usr/bin/python3.8 1; \
    else \
        echo "Unsupported platform: $TARGETPLATFORM" && exit 1; \
    fi

# build SVF
RUN echo "Downloading LLVM and building SVF to " ${HOME}
WORKDIR ${HOME}
RUN git clone "https://github.com/SVF-tools/SVF.git"
WORKDIR ${HOME}/SVF
RUN echo "Building SVF ..."
RUN bash ./build.sh

# export SVF, llvm, z3 paths
ENV PATH=${HOME}/SVF/Release-build/bin:$PATH
ENV PATH=${HOME}/SVF/llvm-$llvm_version.obj/bin:$PATH
ENV SVF_DIR=${HOME}/SVF
ENV LLVM_DIR=${HOME}/SVF/llvm-$llvm_version.obj
ENV Z3_DIR=${HOME}/SVF/z3.obj
RUN ln -s ${Z3_DIR}/bin/libz3.so ${Z3_DIR}/bin/libz3.so.4

# copy crema, svf driver, target repositories
ADD ./crema /home/SVF-tools/crema
ADD ./SVF-example /home/SVF-tools/SVF-example
ADD ./tests_and_target_repos /home/SVF-tools/tests_and_target_repos
WORKDIR /home/SVF-tools

# install curl and get rustup
RUN apt install curl -y
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
  | sh -s -- -y
  
# make cargo/rustup available on the PATH
ENV PATH="${HOME}/.cargo/bin:${PATH}"  
RUN rustup toolchain install nightly-2024-11-21
RUN rustup override set nightly-2024-11-21
RUN rustup component add rust-src rustc-dev llvm-tools-preview
RUN cd tests_and_target_repos \
 && chmod +x cargo_build_all_dirs.sh \
 && ./cargo_build_all_dirs.sh
RUN cd crema && cargo build

# clang 14 for replicating the results
RUN apt install clang-14 lld-14 lldb-14 clang-tools-14 -y
RUN update-alternatives --install /usr/bin/clang   clang   /usr/bin/clang-14   100 \
                    --slave  /usr/bin/clang++ clang++ /usr/bin/clang++-14
RUN update-alternatives --install /usr/bin/clang   clang   /home/SVF-tools/SVF/llvm-16.0.0.obj/bin/clang   90 \
                    --slave  /usr/bin/clang++ clang++ /home/SVF-tools/SVF/llvm-16.0.0.obj/bin/clang++                    
RUN update-alternatives --set clang /usr/bin/clang-14
ENV PATH="/usr/bin:/usr/local/bin:${PATH}"

CMD ["tail", "-f", "/dev/null"] 

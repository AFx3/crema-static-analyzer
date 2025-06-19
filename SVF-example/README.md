## 0. Install npm, zlib, unzip, cmake, gcc, nodejs wget (skip this step if you machine has these libs)

```
sudo apt-get install zlib1g-dev unzip cmake gcc g++ libtinfo5 nodejs wget libncurses5-dev
```

## Install JSON LIB
```
sudo apt-get install libjsoncpp-dev
```
## CHECK 
```
ls /usr/include/jsoncpp/json/json.h
```
## OUTPU SHOULD BE: 
```
/usr/include/jsoncpp/json/json.h
```
## ADD THAT TO THE CMakeLists.ts in src
```
# Add jsoncpp to the includes and link the library
include_directories(/usr/include/jsoncpp)
target_link_libraries(svf-example jsoncpp)
```

## 1. Install SVF and its dependence (LLVM pre-built binary) via npm
```
npm i --silent svf-lib --prefix ${HOME}
```

## 2. Clone repository
```
git clone https://github.com/SVF-tools/SVF-example.git
```

## 3. Setup SVF environment and build your project 
```
source ./env.sh
```
cmake the project (`cmake -DCMAKE_BUILD_TYPE=Debug .` for debug build)
```
cmake . && make
```
## If -lSvfLLVM not found :
```
cmake -DSVF_DIR=/home/af/Documenti/a-phd/SVF/Release-build .
cmake . && make
```

## Analyze 
```
clang -S -c -fno-discard-value-names -emit-llvm example.c -o example.ll
./src/svf-example example.ll
```

## . Analyze a bc file using svf-ex executable
## NB: THIS WILL PRODUCE dbg infos i.e. dbg node in pta, icfg etc GIVEN THE -g flag
```
clang -S -c -g -fno-discard-value-names -emit-llvm example.c -o example.ll
./src/svf-example example.ll
```


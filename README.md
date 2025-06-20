<img src="./crema/img/crema_logo6.png" alt="logo"/>

# RUN WITH DOCKER
### Step 0: install Docker
```bash
https://docs.docker.com/get-started/get-docker/
```
### Step 1: clone this repo and navigate to *crema-static-analyzer* folder
```bash
git clone https://github.com/AFx3/crema-static-analyzercd c

cd crema-static-analyzer
```
### Step 2: build the Docker image and run the container
```bash
docker build --tag 'crema' .

docker run -it crema bash
```
### Step 3: enter in crema folder and start to analyze cargo projects:
```bash
cd crema
```

## How analyze project / prototype notes :
Crema is in very early development phase, so note that currenty the cargo target projects requires a main.rs entry point

* cargo project as target

```bash
cargo run /<path_to_cargo_target_project>
``` 

* specify a custum entry point:
```bash
cargo run /<path_to_cargo_target_project> -f "<your::target_function::bb0>"
``` 


# REQUIREMENTS
crema, cargo_project_test, svf, svf-driver, query_github_rust_prj

# WORK ENVIRONMENT
Set up the work environment, it must be
```bash
/home/<user>/<working_dir>
```
```
/home/<user>/<current_working_dir>
.
├── rustc_mir_callback
├── SVF
├── SVF-example
├── cargo_project_test
```
## Contents
rustc_mir_callback: **Rust-C static analyzer**
SVF: **underlying tool for manging llvm ir**
SVF-example: **driver to interact with SVF and extracing the icfg in json**
cargo_project_test: **some tests cases**

NOW TAKE AS EXAMPLE MY WORKING DIR (ALSO FOR THE CONFIGURATION OF ENV VARIABLES)
```bash
/home/af/Documenti/a-phd
```
---------

## ENV
Note that you have to modify working env variables by updating the shell config file.
I use zsh (do it with yours)
```bash
find  ~/.zshrc
vim /path/to/your/shell_config_file
```
### This is what at the and of each repository configuration your work envirmonment should be
```bash
#rustc
export RUSTC_SYSROOT=/home/crema_tools/crema/rust

export PATH=/opt/cmake/bin:$PATH
export PATH=/opt/cmake/bin:$PATH

# Setting up environment for SVF
export SVF_DIR=/home/crema_tools/SVF
export LLVM_DIR=/home/crema_tools/SVF/llvm-16.0.0.obj
export Z3_DIR=/home/crema_tools/SVF/z3.obj

# Update PATH to include SVF binaries
export PATH=$SVF_DIR/Release-build/bin:$PATH


# Update LD_LIBRARY_PATH for dynamic linker to find SVF libraries
export LD_LIBRARY_PATH=$SVF_DIR/lib:$LLVM_DIR/lib:$Z3_DIR/lib:$LD_LIBRARY_PATH

```
----

# DOWNLOAD THE REPOs
## 1. Install SVF
link: https://github.com/svf-tools/SVF/wiki/Setup-Guide#getting-started

Cmake install:
```bash
sudo apt install cmake gcc g++ libtinfo5 libz-dev libzstd-dev zip wget libncurses5-dev ##(If running on Ubuntu 20.04)

git clone https://github.com/SVF-tools/SVF.git
cd SVF
source ./build.sh
```

SET UP ENV VAR (go to ENV step above)

## 2. Install SVF-driver
(This is a custom driver to interact with SVF)

Get the repository

```bash
git clone https://github.com/AFx3/SVF-driver.git
```

```bash
cd SVF-example
```

Follow the README.md file in the SVF-example folder for the configuration 

SET UP ENV VAR (go to ENV step above)

## 3. Install this repo

### Get this repo
In the working dir:
```bash
git clone https://github.com/AFx3/rustc_mir_callback.git
```

### Install the Rust nightly compiler 
After getting this repository, install the following version of the rust nightly compiler and set up the rust toolchain.

Enter in the repo project dir:
```bash
cd rust_mir_callback
```

We need the
```bash
rustc version: 1.84.0-nightly (b19329a37 2024-11-21)
```

So install it:
```bash
rustup toolchain install nightly-2024-11-21
```

Check if it correctly installed:
```bash
rustup show
```

And set it up:
```bash
rustup override set nightly-2024-11-21

```

Install rust nightly tools:
```bash
rustup component add rust-src rustc-dev llvm-tools-preview
```
(if you find errors, lunch again utill succesfully it downloads all the files)

Again, check now if it active the right version:
```bash
rustup show
```

The output will be something like:
```bash
installed toolchains
--------------------
nightly-x86_64-unknown-linux-gnu
nightly-2021-12-05-x86_64-unknown-linux-gnu (default)
nightly-2022-08-11-x86_64-unknown-linux-gnu
nightly-2022-11-08-x86_64-unknown-linux-gnu
nightly-2024-11-21-x86_64-unknown-linux-gnu (active)

active toolchain
----------------
name: nightly-2024-11-21-x86_64-unknown-linux-gnu
active because: directory override for '/home/andrea/Immagini/test/a'
installed targets:
  x86_64-unknown-linux-gnu

```

if you get in trobles: https://rust-lang.github.io/rustup/overrides.html

You can have different toolchains, but the one already specified must be active in the folder of rust_mir_callcback
and you can see it with:

```
rustup show

-->

Default host: x86_64-unknown-linux-gnu
rustup home:  /home/af/.rustup

installed toolchains
--------------------

stable-x86_64-unknown-linux-gnu
nightly-2019-10-25-x86_64-unknown-linux-gnu
nightly-2021-09-01-x86_64-unknown-linux-gnu
nightly-2021-10-21-x86_64-unknown-linux-gnu
nightly-2021-12-05-x86_64-unknown-linux-gnu
nightly-x86_64-unknown-linux-gnu (default)
1.63-x86_64-unknown-linux-gnu

active toolchain
----------------

nightly-x86_64-unknown-linux-gnu (default)
rustc 1.84.0-nightly (b19329a37 2024-11-21) -----------------
```

SET UP ENV VAR (go to ENV step above)


# Finally, Build and Run the tool

You must be in this projec dir

```bash
cd rust_mir_callback
cargo build
```

Run the tool
```bash
cargo run ./path_to_cargo_project_to_be_analyzed
```
Example:
```bash
cargo run ../cargo_project_test/if-else_cf_cargo 
```
Output:
```bash
⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦
🤖💬 Potential memory issues detected 🚀:

☢ Double Free Issues ☢:
Free detected at source line: CallICFGNode11 {fun: print_and}
   call void @free(ptr noundef %1) #3 CallICFGNode: 
☢ Never Free Issues ☢:
{"{32@rust::main::bb20, Local(_31)}": 1}
⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦
```
Of corse, to run this example you must download the repo containing some vulnerable cargo project (next step)


# Not mandatory: Download vulnerable cargo projects:

# Always in your workdir, clone the repo
```bash
git clone https://github.com/AFx3/cargo_project_test.git
```

-----------------------------------------------------------------------------------------------------
# Some commands and infos:

## List of ffi's in json (output file in ffi_extraction/target) and stdout FROM THE MAIN ROOT:
```
cargo run --package ffi_extraction
```
## The list of FFIs will be on the stdout and in:
```
./target
```

## Extract mir
```
cargo run
```

## build the project
```cargo build```
## extract mir
```./target/debug/rustc_mir_callback ./test.rs```

## Rust nightly version:
```
mr-phism➜  rustc_mir_callback : main ✘ :✖✭ ᐅ  rustup show

Default host: x86_64-unknown-linux-gnu
rustup home:  /home/af/.rustup

installed toolchains
--------------------

stable-x86_64-unknown-linux-gnu
nightly-2019-10-25-x86_64-unknown-linux-gnu
nightly-2021-09-01-x86_64-unknown-linux-gnu
nightly-2021-10-21-x86_64-unknown-linux-gnu
nightly-2021-12-05-x86_64-unknown-linux-gnu
nightly-x86_64-unknown-linux-gnu (default)
1.63-x86_64-unknown-linux-gnu

active toolchain
----------------

nightly-x86_64-unknown-linux-gnu (default)
rustc 1.84.0-nightly (b19329a37 2024-11-21) -----------------
```
# MIR SUPPORTED:
```
1.86.0-nightly
```
#### 

# RUST SANITIZER for memory leaks:
```bash
clang -fsanitize=memory -c "ffi.c" -o "ffi.o"  
ar rcs libffi.a ffi.o    
rustc "main.rs" -L "ffi" -l ffi -Z sanitizer=memory
```
## RUN WITH SPECIFYING A CARGO PROJECT:
```bash
cargo run -- "./target code/uafmulcargo"
```

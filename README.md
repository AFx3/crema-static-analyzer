<img src="./crema/img/crema_logo6.png" alt="logo"/>

* Crema is a static analysis tool for Rust-C FFI. It detects memory errors (memory leak, double-free and use-after-free) in pure Rust code and Rust interacting with C code.
* Crema constructs an inter-procedural Control Flow Graph (ICFG) capturing the interactions between Rust and C functions. 
* To build such a graph, crema exploits Rust MIR and LLVM IR (for C representation).
* The ICFG includes dummyCall and dummyRet nodes storing the association between the two different IRs.
* The abstract domain tracks taint information related to heap management, focusing specifically on memory objects passed through FFI and their ownership. 
* For testing Crema, see REPLICATE_RESULTS.md 
* Crema is in a very early development stage, so note that currently the cargo target projects require a main.rs entry point.


# Run with Docker
### Step 0: install Docker (see online Docker installation guide)
```bash
open https://docs.docker.com/get-started/get-docker/
```
### Step 1: clone this repository and navigate to *crema-static-analyzer* folder
```bash
git clone https://github.com/AFx3/crema-static-analyzer
```
```bash
cd crema-static-analyzer
```
### Step 2: build the Docker image and run the container
```bash
docker build --tag 'crema' .
```
```bash
docker run -it crema bash
```

--------------------------------------------------------------------------------------------------------------------------

# How to analyze project / prototype notes :
Crema is in a very early development phase, so note that currently the cargo target projects require a main.rs entry point

* From `crema` folder, run the tool specifying a cargo project as target
```bash
cargo run /<path_to_cargo_target_project>
``` 

* Or specify a custom entry point:
```bash
cargo run /<path_to_cargo_target_project> -f "<your::target_function::bb0>"
``` 
--------------------------------------------------------------------------------------------------------------------------
# Working directory

The working directory must be
```bash
/home/<user>/.../<path_to_crema_static_analyzer>/crema_static_analyzer
```
# Project structure/working directory content
```bash
. crema-static-analyzer         (whole project folder)
├── crema                       (crema static analyzer)
├── query_github_rust_prj       (script to get GitHub repositories)
├── README.md                   (this file)
├── REPLICATE_RESULTS.md        (instructions for replicating the results)
├── MEMORY_ERRORS_DISCOVERED    (memory errors discovered, including Valgrind outputs)
├── SVF-example                 (SVF driver to interact with SVF)      
├── Dockerfile                  (Dockerfile, run crema easily. Already includes SVF, SVF-driver, crema ...)
├── SVF                         (SVF, NEED TO BE INSTALLED)   
└── tests_and_target_repos      (test and target repositories)
```


## Env
Note that you have to modify working environment variables by updating the shell config file e.g.: zsh
```bash
find  ~/.zshrc
vim /path/to/your/shell_config_file
```
### At the end of each repository configuration, your work environment should be as follows
```bash
#rustc
export RUSTC_SYSROOT=/home/<user>/.../<path_to_crema_static_analyzer>/crema_static_analyzer

export PATH=/opt/cmake/bin:$PATH
export PATH=/opt/cmake/bin:$PATH

# Setting up the environment for SVF
export SVF_DIR=/home/<user>/.../<path_to_crema_static_analyzer>/crema_static_analyzer/SVF
export LLVM_DIR=/home/<user>/.../<path_to_crema_static_analyzer>/crema_static_analyzer/SVF/llvm-16.0.0.obj
export Z3_DIR=/home/<user>/.../<path_to_crema_static_analyzer>/crema_static_analyzer/SVF/z3.obj

# Update PATH to include SVF binaries
export PATH=$SVF_DIR/Release-build/bin:$PATH

# Update LD_LIBRARY_PATH for the dynamic linker to find SVF libraries
export LD_LIBRARY_PATH=$SVF_DIR/lib:$LLVM_DIR/lib:$Z3_DIR/lib:$LD_LIBRARY_PATH
```
----

# Setup

## 0. Clone this repository
```bash
git clone https://github.com/AFx3/crema-static-analyzer
```
```bash
cd crema-static-analyzer
```
## 1. Install SVF
Follow the instructions at: https://github.com/svf-tools/SVF/wiki/Setup-Guide#getting-started

Cmake install:
```bash
sudo apt install cmake gcc g++ libtinfo5 libz-dev libzstd-dev zip wget libncurses5-dev ##(If running on Ubuntu 20.04)

git clone https://github.com/SVF-tools/SVF.git
cd SVF
source ./build.sh
```

Check ENV VAR (go to ENV step above)


Follow the README.md file in the SVF-example folder for the configuration 

SET UP ENV VAR (go to ENV step above)


### Install the Rust nightly compiler 

* After getting this repository, install the following version of the Rust nightly compiler and set up the Rust toolchain.

* Version required
```bash
rustc version: 1.84.0-nightly (b19329a37 2024-11-21)
```
* From crema-static-analyzer directory
* Install the required Rust nightly version:
```bash
rustup toolchain install nightly-2024-11-21
```

* Check if it has been correctly installed:
```bash
rustup show
```

* Set it up:
```bash
rustup override set nightly-2024-11-21
```

* Install Rust nightly tools:
```bash
rustup component add rust-src rustc-dev llvm-tools-preview
```
(if you find errors, lunch again utill it successfully downloads all the files)

* Again, check now if it active the right version:
```bash
rustup show
```


* If you get in trouble, see: https://rust-lang.github.io/rustup/overrides.html

You can have different toolchains, but the one already specified must be active in the folder of `crema-static-analyzer`
and you can see it with:

```
rustup show:

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

CHECK ENV VAR has been correctly set (go to ENV step above)


# Finally, Build and Run the tool

You must be in this project dir

```bash
cd crema
```
```bash
cargo build
```

Run the tool
```bash
cargo run ./path_to_cargo_project_to_be_analyzed
```

You can run the tool specifying a custom entry point with -f:
```bash
cargo run ./path_to_cargo_project_to_be_analyzed -f -f "<rust::function_name::bb<0,1,..n>"
```
## User can see if the tool is correctly installed by looking at RELICATE_RESULTS.md

# MIR SUPPORTED:
```
1.86.0-nightly
```
------------------------------------------------------------------------------
# Future work
* Improve target cargo projects dependency handling
* Include more MIR statements in the analysis
* Improve interprocedural support
* Including pointer/alias analysis modules
* Consider other languages by FFIs 
* Consider also heap allocations from C and deallocations in Rust
------------------------------------------------------------------------------

# Contributions are welcome!

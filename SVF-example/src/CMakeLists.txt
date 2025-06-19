

add_executable(svf-example svf-ex.cpp)
target_link_libraries(svf-example ${llvm_libs} ${SVF_LIB})

# Add jsoncpp to the includes and link the library
include_directories(/usr/include/jsoncpp)
target_link_libraries(svf-example jsoncpp)

# Optionally, find jsoncpp if it's installed via a package manager
# find_package(jsoncpp REQUIRED)
# target_link_libraries(svf-example jsoncpp)



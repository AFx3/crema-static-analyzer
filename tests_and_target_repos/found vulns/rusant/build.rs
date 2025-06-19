fn main() {
    cc::Build::new()
        .file("src/libsantapanelo_bridge.c")
        .include("src")
        .compile("santapanelo_bridge");
}
fn main() {
    tonic_build::configure()
//        .build_client(false)
        .compile(&["protos/api.proto"], &[".","googleapis"])
        .unwrap();
}
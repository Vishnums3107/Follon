//! Protobuf compiler for the deployed trading API.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let protoc = protoc_bin_vendored::protoc_bin_path()?;
    std::env::set_var("PROTOC", protoc);
    let proto = "../../contracts/protobuf/follon/trading/v1/operating_system.proto";
    tonic_prost_build::configure()
        .build_server(true)
        .build_client(true)
        .compile_protos(&[proto], &["../../contracts/protobuf"])?;
    println!("cargo:rerun-if-changed={proto}");
    Ok(())
}

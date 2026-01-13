fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_build::configure()
        .build_server(true)
        .build_client(true)
        .compile_protos(
            &["proto/ln_liquid_swap/v1/swap.proto"],
            &["proto", "proto_deps"],
        )?;

    Ok(())
}

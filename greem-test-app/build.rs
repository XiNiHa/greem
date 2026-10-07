fn main() -> Result<(), Box<dyn std::error::Error>> {
    greem_build::configure()
        .reference_executor(true)
        .compile(&["schema.graphql"])?;
    Ok(())
}

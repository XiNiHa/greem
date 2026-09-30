fn main() -> Result<(), Box<dyn std::error::Error>> {
    greem_build::configure()
        .scalar("UUID", greem_build::Codec::Uuid)
        .compile(&["schema.graphql"])?;
    Ok(())
}

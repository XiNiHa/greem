fn main() -> Result<(), Box<dyn std::error::Error>> {
    greem_build::configure()
        .file_name("property.rs")
        .scalar("UUID", greem_build::Codec::Uuid)
        .scalar("JSON", greem_build::Codec::Json)
        .absent_aware(&["UserPatch"])
        .reference_executor(true)
        .compile(&["schemas/property.graphql"])?;
    Ok(())
}

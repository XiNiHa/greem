fn main() -> Result<(), anyhow::Error> {
    greem_build::codegen(
        greem_build::config::ConfigBuilder::default()
            .schema(vec!["./schema/*.graphql"])
            .build()?,
    )?;

    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    greem_build::configure()
        .file_name("property.rs")
        .scalar("UUID", greem_build::Codec::Uuid)
        .scalar("JSON", greem_build::Codec::Json)
        .absent_aware(&["UserPatch"])
        .compile(&["schemas/property.graphql"])?;
    for suite in [
        "stream",
        "defer",
        "nonnull",
        "error_propagation",
        "abstract",
        "union_interface",
        "lists",
        "variables",
        "oneof",
        "directives",
        "mutations",
        "executor",
        "subscribe",
        "schema",
    ] {
        greem_build::configure()
            .file_name(format!("graphql_js_{suite}.rs"))
            .compile(&[format!("schemas/graphql_js/{suite}.graphql")])?;
    }
    Ok(())
}

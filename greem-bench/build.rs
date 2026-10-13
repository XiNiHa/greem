fn main() -> Result<(), Box<dyn std::error::Error>> {
    greem_build::configure().compile(&["workloads/schema.graphql"])?;
    Ok(())
}

use derive_builder::Builder;

#[derive(Builder)]
pub struct Config {
    /// Glob paths to source schema files.
    pub schema: Vec<&'static str>,
    /// Output directory for generated code.
    #[builder(default = r#""./__generated__""#)]
    pub output_directory: &'static str,
}

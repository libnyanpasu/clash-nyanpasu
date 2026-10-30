fn main() {
    let config = slint_build::CompilerConfiguration::new().with_style("fluent".into());
    slint_build::compile_with_config("ui/reporter.slint", config)
        .expect("the crash reporter UI compiles");
}

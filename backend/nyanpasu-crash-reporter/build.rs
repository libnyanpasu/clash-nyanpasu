fn main() {
    // `nyanpasu-slint`'s build script exports its `.slint` library's project
    // file, imported as `@nyanpasu-slint`.
    let project = std::env::var_os("DEP_NYANPASU_SLINT_PROJECT")
        .expect("nyanpasu-slint exports its library's project file");
    let config = slint_build::CompilerConfiguration::new()
        .with_style("fluent".into())
        .with_edition((1, 19))
        .with_library_paths([("nyanpasu-slint".into(), project.into())].into());
    slint_build::compile_with_config("ui/reporter.slint", config)
        .expect("the crash reporter UI compiles");
}

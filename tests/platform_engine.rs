use nctool_tpl::{Renderer, Value};
#[test]
fn engine_has_no_implicit_domain_extensions_and_rejects_duplicates() {
    let mut renderer = Renderer::new();
    assert!(renderer
        .render("{{ 1 | nc_strip }}", "test", &Value::UNDEFINED)
        .is_err());
    assert!(renderer
        .render("{{ 4 | sqrt }}", "test", &Value::UNDEFINED)
        .is_err());
    renderer.add_filter("double", |x: f64| x * 2.).unwrap();
    assert!(renderer.add_filter("double", |x: f64| x).is_err());
    assert!(renderer.add_filter("upper", |x: f64| x).is_err());
    assert_eq!(
        renderer
            .render("{{ 2 | double }}", "test", &Value::UNDEFINED)
            .unwrap(),
        "4.0"
    );
}
#[test]
fn execution_and_streamed_output_are_bounded() {
    let renderer = Renderer::new().with_limits(100, 32).unwrap();
    assert!(renderer
        .render(
            "{% for x in range(1000000) %}{{ x }}{% endfor %}",
            "loop",
            &Value::UNDEFINED
        )
        .is_err());
    assert!(renderer
        .render(&"x".repeat(33), "large", &Value::UNDEFINED)
        .is_err());
    assert_eq!(
        renderer
            .render("{{ 1 + 1 }}", "normal", &Value::UNDEFINED)
            .unwrap(),
        "2"
    );
}
#[cfg(unix)]
#[test]
fn filesystem_loader_rejects_symlink_escape() {
    let root = std::env::temp_dir().join(format!(
        "nctool-loader-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&root).unwrap();
    std::os::unix::fs::symlink("/etc/passwd", root.join("escape")).unwrap();
    let mut renderer = Renderer::new();
    renderer.set_path_loader(&root);
    assert!(renderer
        .render_template("escape", &Value::UNDEFINED)
        .is_err());
    std::fs::remove_dir_all(root).unwrap();
}

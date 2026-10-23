use nctool_plugin_sdk::*;

#[test]
fn engine_errors_keep_categories_and_available_locations() {
    let parsed = nctool_tpl::parse("Hello\n{% if %}", "report.j2").unwrap_err();
    let error = PluginError::from(parsed);
    assert_eq!(error.code, "template_parse");
    let d = &error.diagnostics[0];
    assert_eq!(d.template.as_deref(), Some("report.j2"));
    assert_eq!(d.line, Some(2));
    assert!(d.column.unwrap() > 0);
    assert!(d.path.is_none());
    let mut renderer = Renderer::new();
    renderer.add_template("root", "{{ missing }}").unwrap();
    let error = PluginError::from(
        renderer
            .render_template("root", &nctool_tpl::Value::from(()))
            .unwrap_err(),
    );
    assert_eq!(error.code, "template_undefined_variable");
    assert_eq!(error.diagnostics[0].template.as_deref(), Some("root"));
    assert!(
        error.diagnostics[0].line.is_none(),
        "do not invent unavailable locations"
    );
    let error =
        validate_schema(&json!({"type":"integer"}), &json!("bad"), "invalid_input").unwrap_err();
    assert_eq!(error.diagnostics[0].path.as_deref(), Some(""));
    assert!(error.diagnostics[0].template.is_none());
}

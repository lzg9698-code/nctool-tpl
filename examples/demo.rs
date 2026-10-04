use nctool_tpl::{extract_undeclared, parse, Renderer};
fn main() {
    let source = "Hello {{ user.name }}!{% for item in items %}\n- {{ item }}{% endfor %}";
    let ast = parse(source, "hello").unwrap();
    println!("Variables: {:?}", extract_undeclared(&ast));
    let context = minijinja::context! {user=>minijinja::context!{name=>"Ada"},items=>vec!["templates","plugins"]};
    println!(
        "{}",
        Renderer::new().render(source, "hello", &context).unwrap()
    );
}

//! The configuration template in `README.md` must load.
//!
//! The README is what an operator copies from. A template with a key that
//! does not exist, or a section missing a field that has no default, is
//! not documentation - it is a startup failure waiting to happen. This
//! test parses the template out of the README and loads it through the
//! real `Config` path, which parses AND validates.

use ramshield_config::Config;

/// Extract the first ```toml block under the "## Configuration Template"
/// heading.
fn readme_template() -> String {
    let readme = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/README.md"))
        .expect("README.md is readable");

    let after_heading = readme
        .split_once("## Configuration Template")
        .expect("README has a Configuration Template section")
        .1;

    let start = after_heading
        .find("```toml")
        .expect("the template section holds a toml block");
    let body = &after_heading[start + "```toml".len()..];
    let end = body.find("```").expect("the toml block is closed");

    body[..end].to_string()
}

#[test]
fn the_readme_template_loads() {
    let template = readme_template();
    assert!(
        !template.trim().is_empty(),
        "the template block in README.md is empty"
    );

    // The README ships the documented dev key so operators can copy a working
    // example. Validation rejects it on purpose (finding 7); sanitize it the
    // same way the installers do, then check the template's structure loads.
    let template = template.replace(
        "k1:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        "k1:1111111111111111111111111111111111111111111111111111111111111111",
    );

    // `Config::from_toml_file` reads a file, so write the block to one.
    // stdlib only: no tempfile dependency for a single file.
    let path = std::env::temp_dir().join(format!(
        "ramshield-readme-template-{}.toml",
        std::process::id()
    ));
    std::fs::write(&path, &template).expect("the template is written");

    let loaded = Config::from_toml_file(path.to_str().expect("a utf-8 path"));
    let _ = std::fs::remove_file(&path);

    let cfg = loaded.unwrap_or_else(|e| panic!("the README template does not load: {e:#}"));

    // The template must be a configuration, not a sketch: the values an
    // operator relies on have to be the ones the code reads.
    assert_eq!(cfg.ipc.tcp_addr, "127.0.0.1:7890");
    assert_eq!(cfg.dashboard.http_addr, "127.0.0.1:9999");
    assert_eq!(cfg.mesh.node_id, 1);
    assert!(!cfg.mesh.enabled, "mesh stays off in the template");
    assert_eq!(cfg.synproxy.ports, vec![22, 53, 80, 443]);
}

use super::*;

#[test]
fn icon_must_be_string() {
    let mut mapping = noyalib::Mapping::new();
    mapping.insert("icon", noyalib::Value::Number(noyalib::Number::from(42)));
    let result = interpret_front_matter(Some(&mapping), "body".to_string());
    assert!(result.is_err());
}

use super::*;

#[test]
fn parses_no_parameters() {
    let params = parse_closure_parameters("{|| 42}");
    assert_eq!(params, vec![]);
}

#[test]
fn parses_one_parameter() {
    let params = parse_closure_parameters("{|x| $x * 2}");
    assert_eq!(
        params,
        vec![ClosureParameter {
            name: "x".to_string(),
            is_required: true
        }]
    );
}

#[test]
fn parses_two_parameters() {
    let params = parse_closure_parameters("{|x, y| $x + $y}");
    assert_eq!(
        params,
        vec![
            ClosureParameter {
                name: "x".to_string(),
                is_required: true
            },
            ClosureParameter {
                name: "y".to_string(),
                is_required: true
            },
        ]
    );
}

#[test]
fn parses_optional_parameter() {
    let params = parse_closure_parameters("{|x, y?| $x + $y}");
    assert_eq!(
        params,
        vec![
            ClosureParameter {
                name: "x".to_string(),
                is_required: true
            },
            ClosureParameter {
                name: "y".to_string(),
                is_required: false
            },
        ]
    );
}

#[test]
fn handles_whitespace() {
    let params = parse_closure_parameters("{| x , y | $x + $y}");
    assert_eq!(
        params,
        vec![
            ClosureParameter {
                name: "x".to_string(),
                is_required: true
            },
            ClosureParameter {
                name: "y".to_string(),
                is_required: true
            },
        ]
    );
}

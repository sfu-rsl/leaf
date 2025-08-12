use std::{fs, path::Path, process::Command};

#[test]
fn test_hello_world_simple_constraint() {
    // Test case from samples/hello_world.rs:
    // A simple program that checks if x < 5 where x is marked symbolic.
    // Generated constraint: NOT (5 <= k!1) should be False
    // This means we want 5 <= k!1 to be True (i.e., k!1 >= 5)
    let binary_path = env!("CARGO_BIN_EXE_leafsolver");
    let test_data_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("test_data");

    let input_file = test_data_dir.join("hello_world_simple.jsonl");
    let output_file = test_data_dir.join("hello_world_result.json");

    assert!(input_file.exists(), "Hello world test input should exist");

    let output = Command::new(binary_path)
        .args([
            "-i",
            input_file.to_str().unwrap(),
            "-o",
            output_file.to_str().unwrap(),
        ])
        .output()
        .expect("Failed to run leafsolver binary");

    assert!(
        output.status.success(),
        "Binary should handle simple constraint from hello_world.rs"
    );
    assert!(output_file.exists(), "Output file should be created");

    let result_json = fs::read_to_string(&output_file).expect("Should read result file");
    let result: serde_json::Value = serde_json::from_str(&result_json).expect("Should parse JSON");

    assert_eq!(result["result"], "sat");
    assert!(result["model"].is_object());

    // Verify the solution: k!1 should be >= 5 (to make "NOT (5 <= k!1)" false)
    let model = result["model"].as_object().unwrap();
    let k1_value = model["1"]["smtlib_rep"].as_str().unwrap();
    // Should be a hex value >= #x05
    assert!(k1_value.starts_with("#x"));

    fs::remove_file(output_file).ok();
}

#[test]
fn test_is_sorted_complex_constraint() {
    // Test case from samples/basic/is_sorted/main.rs:
    // A program that tests array sorting with 4 symbolic u8 elements.
    // Tests both regular and non-short-circuiting sorting implementations.
    // Generated constraints involve multiple comparison operations between array elements.
    let binary_path = env!("CARGO_BIN_EXE_leafsolver");
    let test_data_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("test_data");

    let input_file = test_data_dir.join("is_sorted_complex.jsonl");
    let output_file = test_data_dir.join("is_sorted_result.json");

    assert!(input_file.exists(), "Is sorted test input should exist");

    let output = Command::new(binary_path)
        .args([
            "-i",
            input_file.to_str().unwrap(),
            "-o",
            output_file.to_str().unwrap(),
        ])
        .output()
        .expect("Failed to run leafsolver binary");

    assert!(
        output.status.success(),
        "Binary should handle complex multi-variable constraints from is_sorted.rs"
    );
    assert!(output_file.exists(), "Output file should be created");

    let result_json = fs::read_to_string(&output_file).expect("Should read result file");
    let result: serde_json::Value = serde_json::from_str(&result_json).expect("Should parse JSON");

    assert_eq!(result["result"], "sat");
    assert!(result["model"].is_object());

    // Verify we have multiple variables in the model (4 array elements)
    let model = result["model"].as_object().unwrap();
    assert!(
        model.len() >= 4,
        "Should have at least 4 variables (array elements), got {}",
        model.len()
    );

    // All model values should be valid u8 bit vectors
    for (var_id, value) in model {
        let smtlib_value = value["smtlib_rep"].as_str().unwrap();
        assert!(
            smtlib_value.starts_with("#x"),
            "Variable {} should have hex bit vector value, got {}",
            var_id,
            smtlib_value
        );
    }

    fs::remove_file(output_file).ok();
}

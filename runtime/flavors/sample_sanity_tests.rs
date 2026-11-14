use common::testing::sanity_tests::run_sample_sanity;
use macros::gen_sample_tests_rs;

#[gen_sample_tests_rs(
    folder = "samples",
    ignore_files = [
        ".sanity_test.ignore",
        ".sanity_test.runtime.ignore",
        ".sanity_test.{backend}.ignore"
    ],
    backend = package_name
)]
fn test_sample_sanity_rs(fic: &str, backend: &str) {
    run_sample_sanity(fic, backend);
}

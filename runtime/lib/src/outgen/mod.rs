use std::fmt::{Display, Write};

use common::log_info;

pub fn log_json<'a, Id: 'a + Display, Val: 'a + Display>(
    answers: impl Iterator<Item = (&'a Id, &'a Val)>,
) {
    let mut answers_str = String::new();
    writeln!(answers_str, "{{").unwrap();
    for (i, v) in answers {
        writeln!(answers_str, "    \"{}\": {},", i, v).unwrap();
    }
    writeln!(answers_str, "}}").unwrap();
    log_info!("Found a solution:\n{answers_str}");
}

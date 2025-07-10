mod investigate;
mod value;

type ConstraintId = usize; // Step index

pub(super) use value::{Antecedents, Implied, Precondition, PreconditionConstraints};

pub(super) use investigate::default_implication_investigator;

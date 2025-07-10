/* NOTE: This module exists even if the `implicit_flow` feature is not enabled.
 * Because we don't want to make BasicValue conditional. It will be always `Implied` but
 * when the feature is not enabled, the precondition will be an instance of a ZST.
 */

#[cfg(feature = "implicit_flow")]
mod investigate;
mod value;

type ConstraintId = usize; // Step index

pub(super) use value::{
    Antecedents, Implied, ImpliedValue, PreconditionConstraints, PreconditionConstruct,
    PreconditionQuery,
};
pub(super) type Precondition = <Implied<super::ValueRef> as ImpliedValue>::Precondition;

#[cfg(feature = "implicit_flow")]
pub(super) use investigate::default_implication_investigator;

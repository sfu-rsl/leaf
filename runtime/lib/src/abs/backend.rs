use std::collections::HashMap;

use super::{BasicBlockLocation, Constraint, ConstraintKind, FuncDef, IntType, ValueType};

pub(crate) trait Shutdown {
    fn shutdown(&mut self);
}

/// Keeps track of all the compounding constraints in a single trace
pub(crate) trait TraceManager<S, V, C> {
    fn notify_step(&mut self, step: S, constraint: Constraint<V, C>);
}

pub(crate) type Model<I, A> = HashMap<I, A>;

/// A trait for the SMT solver.
/// It takes a set of constraints to check satisfiability of them together.
pub(crate) trait Solver {
    type Value;
    type Case;
    type Model;

    fn check(
        &mut self,
        constraints: impl Iterator<Item = Constraint<Self::Value, Self::Case>>,
    ) -> SolveResult<Self::Model>;
}

/// The result of the checking performed by [`Solver`].
/// [`Sat`]: The constraints are satisfiable and a model is found.
/// [`Unsat`]: The constraints are unsatisfiable.
/// [`Unknown`]: The solver could not determine the satisfiability.
pub(crate) enum SolveResult<M> {
    Sat(M),
    Unsat,
    Unknown,
}

pub(crate) use common::type_info::TypeDatabase;

macro_rules! fn_by_name {
    ($($name:ident),*$(,)?) => {
        $(
            #[allow(unused)]
            fn $name(&self) -> V;
        )*
    };
}

pub trait CoreTypeProvider<V> {
    common::type_info::pass_core_type_names_to!(fn_by_name);

    fn int_type(&self, ty: IntType) -> V {
        match (ty.is_signed, ty.bit_size as u32) {
            (true, i8::BITS) => self.i8(),
            (false, u8::BITS) => self.u8(),
            (true, i16::BITS) => self.i16(),
            (false, u16::BITS) => self.u16(),
            (true, i32::BITS) => self.i32(),
            (false, u32::BITS) => self.u32(),
            (true, i64::BITS) => self.i64(),
            (false, u64::BITS) => self.u64(),
            (true, i128::BITS) => self.i128(),
            (false, u128::BITS) => self.u128(),
            _ => unreachable!("Unexpected integer type: {:?}", ty),
        }
    }

    fn try_to_value_type<'a>(&self, ty: V) -> Option<ValueType>;
}

pub(crate) trait PhasedCallTraceRecorder {
    fn start_call(&mut self, call_site: BasicBlockLocation<FuncDef>);

    fn finish_call(&mut self, entered_func: FuncDef, broken: bool);

    fn start_return(&mut self, ret_point: BasicBlockLocation<FuncDef>);

    /// # Returns
    /// The caller which the execution has returned to.
    fn finish_return(&mut self, broken: bool) -> FuncDef;
}

pub(crate) trait DecisionTraceRecorder {
    type Case;

    /// # Returns
    /// The step index.
    fn notify_decision(
        &mut self,
        node_location: BasicBlockLocation,
        kind: &ConstraintKind<Self::Case>,
    ) -> usize;
}

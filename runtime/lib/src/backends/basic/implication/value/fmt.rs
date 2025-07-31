use core::fmt::{Display, Formatter, Result};

use super::*;

impl<V: Display> Display for Implied<V, enabled::Precondition> {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        use super::enabled::Precondition::*;
        match &self.by {
            NoneOrUnknown => self.value.fmt(f),
            Constraints(..) => write!(f, "(..) => {}", self.value),
        }
    }
}

impl<V: Display> Display for Implied<V, disabled::Precondition> {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        self.value.fmt(f)
    }
}

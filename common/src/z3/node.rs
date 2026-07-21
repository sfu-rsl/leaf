use std::prelude::rust_2024::*;

use derive_more as dm;
use serde::{Deserialize, Serialize};
use z3::ast::{self, Ast};

/* NOTE: Why not using `Dynamic`?
 * In this way we have a little more freedom to include our information such
 * as whether the bit vector is signed or not.
 */
#[derive(Debug, Clone, PartialEq, Eq, dm::Display)]
#[display("{_0}")]
pub enum AstNode {
    Bool(ast::Bool),
    BitVector(BVNode),
    Array(ArrayNode),
}

impl From<BVNode> for AstNode {
    fn from(node: BVNode) -> Self {
        Self::BitVector(node)
    }
}

impl From<ArrayNode> for AstNode {
    fn from(node: ArrayNode) -> Self {
        Self::Array(node)
    }
}

#[derive(Debug, Clone, dm::Display, PartialEq, Eq)]
#[display("{_0}")]
pub struct BVNode(pub ast::BV, pub BVSort);

impl BVNode {
    pub fn new(ast: ast::BV, is_signed: bool) -> Self {
        Self(ast, BVSort { is_signed })
    }

    #[inline]
    pub fn map<F>(&self, f: F) -> Self
    where
        F: FnOnce(&ast::BV) -> ast::BV,
    {
        Self(f(&self.0), self.1)
    }

    #[inline(always)]
    pub fn is_signed(&self) -> bool {
        self.1.is_signed
    }

    #[inline(always)]
    pub fn size(&self) -> u32 {
        self.0.get_size()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, dm::Display)]
#[display("{_0}")]
pub struct ArrayNode(pub ast::Array, pub ArraySort);

#[derive(Debug, Clone, PartialEq, Eq, dm::From, Serialize, Deserialize)]
pub enum AstNodeSort {
    Bool,
    BitVector(BVSort),
    Array(ArraySort),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BVSort {
    pub is_signed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, dm::From, Serialize, Deserialize)]
pub struct ArraySort {
    pub range: Box<AstNodeSort>,
}

impl From<ast::Bool> for AstNode {
    fn from(ast: ast::Bool) -> Self {
        Self::Bool(ast)
    }
}

impl AstNode {
    pub fn from_ubv(ast: ast::BV) -> Self {
        BVNode::new(ast, false).into()
    }

    pub fn from_ast(ast: ast::Dynamic, sort: &AstNodeSort) -> Self {
        match sort {
            AstNodeSort::Bool => ast.as_bool().map(Self::Bool),
            AstNodeSort::BitVector(sort) => {
                ast.as_bv().map(|ast| Self::BitVector(BVNode(ast, *sort)))
            }
            AstNodeSort::Array(sort) => ast
                .as_array()
                .map(|ast| Self::Array(ArrayNode(ast, sort.clone()))),
        }
        .unwrap_or_else(|| {
            core::panic!(
                "Sort of ${:?} is not compatible with the expected one.",
                ast
            )
        })
    }
}

impl AstNode {
    pub fn as_bool(&self) -> &ast::Bool {
        match self {
            Self::Bool(ast) => ast,
            _ => core::panic!("Expected the value to be a boolean expression."),
        }
    }

    pub fn as_bit_vector(&self) -> &ast::BV {
        match self {
            Self::BitVector(BVNode(ast, _)) => ast,
            _ => core::panic!("Expected the value to be a bit vector: {:?}", self),
        }
    }

    pub fn unwrap_as_bit_vector(self) -> ast::BV {
        match self {
            Self::BitVector(BVNode(ast, _)) => ast,
            _ => core::panic!("Expected the value to be a bit vector: {:?}", self),
        }
    }
}

impl AstNode {
    pub fn ast(&self) -> &dyn ast::Ast {
        match self {
            Self::Bool(ast) => ast,
            Self::BitVector(BVNode(ast, _)) => ast,
            Self::Array(ArrayNode(ast, _)) => ast,
        }
    }

    pub fn dyn_ast(&self) -> ast::Dynamic {
        ast::Dynamic::from_ast(self.ast())
    }

    pub fn sort(&self) -> AstNodeSort {
        match self {
            Self::Bool(_) => AstNodeSort::Bool,
            Self::BitVector(BVNode(_, sort)) => AstNodeSort::BitVector(*sort),
            Self::Array(ArrayNode(_, sort)) => AstNodeSort::Array(sort.clone()),
        }
    }

    pub fn z3_sort(&self) -> z3::Sort {
        match self {
            Self::Bool(ast) => ast.get_sort(),
            Self::BitVector(BVNode(ast, _)) => ast.get_sort(),
            Self::Array(ArrayNode(ast, _)) => ast.get_sort(),
        }
    }

    pub fn to_smtlib2(&self) -> String {
        macro_rules! to_smt_string {
            ($ast:expr) => {
                $ast.simplify().to_string()
            };
        }
        match self {
            Self::Bool(ast) => to_smt_string!(ast),
            Self::BitVector(BVNode(ast, _)) => to_smt_string!(ast),
            Self::Array(ArrayNode(ast, _)) => to_smt_string!(ast),
        }
    }
}

#[derive(Debug, Clone, dm::Deref, dm::Display)]
#[display("{value}")]
pub struct AstAndVars<I> {
    #[deref]
    pub value: AstNode,
    pub variables: Vec<(I, AstNode)>,
}

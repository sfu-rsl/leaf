use std::prelude::rust_2021::*;

use derive_more as dm;
use serde::{Deserialize, Serialize};
use z3::ast::{self, Ast};

/* NOTE: Why not using `Dynamic`?
 * In this way we have a little more freedom to include our information such
 * as whether the bit vector is signed or not.
 */
#[derive(Debug, Clone, PartialEq, Eq, dm::Display)]
#[display("{_0}")]
pub enum AstNode<'ctx> {
    Bool(ast::Bool<'ctx>),
    BitVector(BVNode<'ctx>),
    Array(ArrayNode<'ctx>),
}

impl<'ctx> From<BVNode<'ctx>> for AstNode<'ctx> {
    fn from(node: BVNode<'ctx>) -> Self {
        Self::BitVector(node)
    }
}

impl<'ctx> From<ArrayNode<'ctx>> for AstNode<'ctx> {
    fn from(node: ArrayNode<'ctx>) -> Self {
        Self::Array(node)
    }
}

#[derive(Debug, Clone, dm::Display, PartialEq, Eq)]
#[display("{_0}")]
pub struct BVNode<'ctx>(pub ast::BV<'ctx>, pub BVSort);

impl<'ctx> BVNode<'ctx> {
    pub fn new(ast: ast::BV<'ctx>, is_signed: bool) -> Self {
        Self(ast, BVSort { is_signed })
    }

    #[inline]
    pub fn map<F>(&self, f: F) -> Self
    where
        F: FnOnce(&ast::BV<'ctx>) -> ast::BV<'ctx>,
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
pub struct ArrayNode<'ctx>(pub ast::Array<'ctx>, pub ArraySort);

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

impl<'ctx> From<ast::Bool<'ctx>> for AstNode<'ctx> {
    fn from(ast: ast::Bool<'ctx>) -> Self {
        Self::Bool(ast)
    }
}

impl<'ctx> AstNode<'ctx> {
    pub fn from_ubv(ast: ast::BV<'ctx>) -> Self {
        BVNode::new(ast, false).into()
    }

    pub fn from_ast(ast: ast::Dynamic<'ctx>, sort: &AstNodeSort) -> Self {
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
            panic!(
                "Sort of ${:?} is not compatible with the expected one.",
                ast
            )
        })
    }
}

impl<'ctx> AstNode<'ctx> {
    pub fn as_bool(&self) -> &ast::Bool<'ctx> {
        match self {
            Self::Bool(ast) => ast,
            _ => panic!("Expected the value to be a boolean expression."),
        }
    }

    pub fn as_bit_vector(&self) -> &ast::BV<'ctx> {
        match self {
            Self::BitVector(BVNode(ast, _)) => ast,
            _ => panic!("Expected the value to be a bit vector: {:?}", self),
        }
    }
}

impl<'ctx> AstNode<'ctx> {
    pub fn ast(&self) -> &dyn ast::Ast<'ctx> {
        match self {
            Self::Bool(ast) => ast,
            Self::BitVector(BVNode(ast, _)) => ast,
            Self::Array(ArrayNode(ast, _)) => ast,
        }
    }

    pub fn dyn_ast(&self) -> ast::Dynamic<'ctx> {
        ast::Dynamic::from_ast(self.ast())
    }

    pub fn sort(&self) -> AstNodeSort {
        match self {
            Self::Bool(_) => AstNodeSort::Bool,
            Self::BitVector(BVNode(_, sort)) => AstNodeSort::BitVector(*sort),
            Self::Array(ArrayNode(_, sort)) => AstNodeSort::Array(sort.clone()),
        }
    }

    pub fn z3_sort(&self) -> z3::Sort<'ctx> {
        match self {
            Self::Bool(ast) => ast.get_sort(),
            Self::BitVector(BVNode(ast, _)) => ast.get_sort(),
            Self::Array(ArrayNode(ast, _)) => ast.get_sort(),
        }
    }

    pub fn to_smtlib2(&self) -> String {
        match self {
            Self::Bool(ast) => ast.to_string(),
            Self::BitVector(BVNode(ast, _)) => ast.to_string(),
            Self::Array(ArrayNode(ast, _)) => ast.to_string(),
        }
    }
}

#[derive(Debug, Clone, dm::Deref, dm::Display)]
#[display("{value}")]
pub struct AstAndVars<'ctx, I> {
    #[deref]
    pub value: AstNode<'ctx>,
    pub variables: Vec<(I, AstNode<'ctx>)>,
}

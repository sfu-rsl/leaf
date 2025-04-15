use core::hash::Hash;
use core::{iter, str::FromStr};
use std::prelude::rust_2021::*;
use std::{collections::HashMap, ffi, format};

use derive_more as dm;
use serde::{Deserialize, Serialize};
use z3::{Context, ast, ast::Ast};
use z3_sys::{
    Z3_ast_vector_get, Z3_ast_vector_size, Z3_get_app_decl, Z3_get_decl_name,
    Z3_parse_smtlib2_string, Z3_to_app,
};

use super::node::{AstAndVars, AstNode, AstNodeSort};

#[derive(Debug, Serialize, Deserialize)]
struct VarDecl {
    name: String,
    sort: AstNodeSort,
    smtlib_rep: String,
}

#[derive(Debug, Serialize, Deserialize, dm::Display)]
#[display("{smtlib_rep}")]
struct Expr {
    sort: AstNodeSort,
    smtlib_rep: String,
}

#[derive(Debug, Serialize, Deserialize, dm::Display)]
#[display("{expr}")]
pub struct SmtLibExpr {
    #[serde(default)]
    decls: HashMap<String, VarDecl>,
    #[serde(flatten)]
    expr: Expr,
}

impl From<Expr> for SmtLibExpr {
    fn from(value: Expr) -> Self {
        Self {
            decls: Default::default(),
            expr: value,
        }
    }
}

impl<'ctx, I: ToString + FromStr> AstAndVars<'ctx, I> {
    pub fn serializable(&self) -> impl Serialize {
        SmtLibExpr {
            expr: Expr {
                sort: self.value.sort(),
                smtlib_rep: self.value.to_smtlib2(),
            },
            decls: self
                .variables
                .iter()
                .map(|(id, node)| {
                    (id.to_string(), {
                        let decl = node
                            .ast()
                            .safe_decl()
                            .expect("Variable is expected to have a declaration");
                        VarDecl {
                            name: decl.name(),
                            sort: node.sort(),
                            smtlib_rep: decl.to_string(),
                        }
                    })
                })
                .collect(),
        }
    }

    pub fn parse(context: &'ctx Context, smtlib: &SmtLibExpr) -> Self {
        let variables = smtlib
            .decls
            .iter()
            .map(|(id, decl)| {
                (
                    I::from_str(id).unwrap_or_else(|_| panic!("Invalid id: {id}")),
                    parse_var_decl(context, decl),
                )
            })
            .collect::<Vec<_>>();
        let value = parse_expr(
            context,
            &smtlib.expr,
            variables.iter().map(|(_, node)| node),
        );
        Self { variables, value }
    }
}

impl SmtLibExpr {
    pub fn parse<'ctx, I: FromStr + Eq + Hash + Clone>(
        &self,
        context: &'ctx Context,
        vars: &mut HashMap<I, AstNode<'ctx>>,
    ) -> AstAndVars<'ctx, I> {
        let variables = self
            .decls
            .iter()
            .map(|(id, decl)| {
                let id = I::from_str(id).unwrap_or_else(|_| panic!("Invalid id: {id}"));
                let decl = vars
                    .entry(id.clone())
                    .or_insert_with(|| parse_var_decl(context, decl))
                    .clone();
                (id, decl)
            })
            .collect::<Vec<_>>();
        let value = parse_expr(context, &self.expr, variables.iter().map(|(_, node)| node));
        AstAndVars { variables, value }
    }

    pub fn parse_as_const<'ctx>(&self, context: &'ctx Context) -> Option<AstNode<'ctx>> {
        self.decls
            .is_empty()
            .then(|| parse_expr(context, &self.expr, iter::empty()))
    }
}

impl<'ctx> AstNode<'ctx> {
    pub fn serializable(&self) -> impl Serialize {
        Expr {
            sort: self.sort(),
            smtlib_rep: self.to_smtlib2(),
        }
    }
}

fn parse_var_decl<'ctx>(context: &'ctx Context, decl: &VarDecl) -> AstNode<'ctx> {
    let smtlib = [
        decl.smtlib_rep.as_str(),
        dummy_assertion(&decl.name).as_str(),
    ]
    .concat();
    let dummy_ast = unsafe { parse_single_expr(context, smtlib, iter::empty()) };
    extract_expr_from_dummy(dummy_ast, &decl.sort)
}

fn parse_expr<'ctx: 'a, 'a>(
    context: &'ctx Context,
    expr: &Expr,
    decls: impl Iterator<Item = &'a AstNode<'ctx>>,
) -> AstNode<'ctx> {
    let smtlib = dummy_assertion(&expr.smtlib_rep);
    let dummy_ast = unsafe { parse_single_expr(context, smtlib, decls.map(|d| d.ast())) };
    extract_expr_from_dummy(dummy_ast, &expr.sort)
}

fn dummy_assertion(expr: &str) -> String {
    format!("(assert (= {expr} {expr}))")
}

fn extract_expr_from_dummy<'ctx>(ast: ast::Dynamic<'ctx>, sort: &AstNodeSort) -> AstNode<'ctx> {
    assert_eq!(ast.num_children(), 2);
    AstNode::from_ast(
        ast.nth_child(0).expect("Unexpected structure").simplify(),
        sort,
    )
}

unsafe fn parse_single_expr<'ctx: 'a, 'a, S: Into<Vec<u8>>>(
    context: &'ctx Context,
    smtlib: S,
    decls: impl Iterator<Item = &'a (dyn ast::Ast<'ctx> + 'a)>,
) -> ast::Dynamic<'ctx> {
    let c = context.get_z3_context();
    let decls = decls
        .map(|d| {
            let app = Z3_to_app(d.get_ctx().get_z3_context(), d.get_z3_ast());
            Z3_get_app_decl(context.get_z3_context(), app)
        })
        .collect::<Vec<_>>();
    let decl_names = decls
        .iter()
        .map(|d| Z3_get_decl_name(context.get_z3_context(), *d))
        .collect::<Vec<_>>();
    let vec = Z3_parse_smtlib2_string(
        c,
        ffi::CString::new(smtlib).unwrap().as_ptr(),
        0,
        core::ptr::null(),
        core::ptr::null(),
        decls.len() as u32,
        decl_names.as_ptr(),
        decls.as_ptr(),
    );
    assert_eq!(Z3_ast_vector_size(c, vec), 1);
    ast::Dynamic::wrap(context, Z3_ast_vector_get(c, vec, 0))
}

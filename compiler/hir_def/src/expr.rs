use crate::arena::{Idx, RawIdx};
use crate::name::Name;
use crate::pat::PatId;
use crate::path::Path;
use crate::type_ref::LocalTypeRefId;

pub type ExprId = Idx<Expr>;

pub(crate) fn dummy_expr_id() -> ExprId {
    ExprId::from_raw(RawIdx::from(0))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    Missing,
    Typed {
        expr: ExprId,
        ty: LocalTypeRefId,
    },
    Path {
        path: Path,
    },
    Lit {
        lit: Literal,
    },
    Infix {
        op: Path,
        lhs: ExprId,
        rhs: ExprId,
    },
    App {
        base: ExprId,
        arg: ExprId,
    },
    Field {
        base: ExprId,
        field: Name,
    },
    Index {
        base: ExprId,
        index: ExprId,
    },
    Tuple {
        exprs: Vec<ExprId>,
    },
    Record {
        fields: Vec<RecordField<ExprId>>,
    },
    Array {
        exprs: Vec<ExprId>,
    },
    Do {
        stmts: Vec<Stmt>,
    },
    Clos {
        pats: Vec<PatId>,
        stmts: Vec<Stmt>,
    },
    If {
        cond: ExprId,
        then: ExprId,
        else_: Option<ExprId>,
        inverse: bool,
    },
    Case {
        pred: ExprId,
        arms: Vec<CaseArm>,
    },
    While {
        cond: ExprId,
        body: ExprId,
        inverse: bool,
    },
    Loop {
        body: ExprId,
    },
    Next {
        expr: Option<ExprId>,
    },
    Break {
        expr: Option<ExprId>,
    },
    Yield {
        exprs: Vec<ExprId>,
    },
    Return {
        expr: Option<ExprId>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Literal {
    String(String),
    Char(char),
    Int(i128),
    Float(u64),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordField<ID> {
    pub name: Name,
    pub val: ID,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stmt {
    Let { pat: PatId, val: ExprId },
    Bind { pat: PatId, val: ExprId },
    Expr { expr: ExprId },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaseArm {
    pub pat: PatId,
    pub guard: Option<ExprId>,
    pub expr: ExprId,
}

impl Expr {
    pub fn walk(&self, mut f: impl FnMut(ExprId)) {
        match self {
            | Expr::Missing | Expr::Path { .. } | Expr::Lit { .. } => {},
            | Expr::Typed { expr, .. } => f(*expr),
            | Expr::Infix { lhs, rhs, .. } => {
                f(*lhs);
                f(*rhs);
            },
            | Expr::App { base, arg } => {
                f(*base);
                f(*arg);
            },
            | Expr::Field { base, .. } => f(*base),
            | Expr::Index { base, index } => {
                f(*base);
                f(*index);
            },
            | Expr::Tuple { exprs } | Expr::Array { exprs } => {
                exprs.iter().copied().for_each(f);
            },
            | Expr::Record { fields } => {
                fields.iter().for_each(|i| f(i.val));
            },
            | Expr::Do { stmts } => {
                stmts.iter().for_each(|stmt| match stmt {
                    | Stmt::Let { val, .. } => f(*val),
                    | Stmt::Bind { val, .. } => f(*val),
                    | Stmt::Expr { expr } => f(*expr),
                });
            },
            | Expr::Clos { pats: _, stmts } => {
                stmts.iter().for_each(|stmt| match stmt {
                    | Stmt::Let { val, .. } => f(*val),
                    | Stmt::Bind { val, .. } => f(*val),
                    | Stmt::Expr { expr } => f(*expr),
                });
            },
            | Expr::If { cond, then, else_, .. } => {
                f(*cond);
                f(*then);
                else_.map(f);
            },
            | Expr::Case { pred, arms } => {
                f(*pred);
                arms.iter().for_each(|arm| {
                    arm.guard.map(&mut f);
                    f(arm.expr);
                });
            },
            | Expr::While { cond, body, .. } => {
                f(*cond);
                f(*body);
            },
            | Expr::Loop { body } => f(*body),
            | Expr::Next { expr } | Expr::Break { expr } | Expr::Return { expr } => {
                expr.map(f);
            },
            | Expr::Yield { exprs } => {
                exprs.iter().copied().for_each(f);
            },
        }
    }
}

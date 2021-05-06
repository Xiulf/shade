use crate::arena::{Arena, Idx};
use crate::body::Body;
use crate::db::DefDatabase;
use crate::expr::{Expr, ExprId, Stmt};
use crate::id::{DefWithBodyId, Lookup, TypeVarId};
use crate::name::Name;
use crate::pat::{Pat, PatId};
use crate::type_ref::{TypeRef, TypeRefId};
use rustc_hash::FxHashMap;
use std::sync::Arc;

pub type ExprScopeId = Idx<ExprScopeData>;
pub type TypeScopeId = Idx<TypeScopeData>;

#[derive(Debug, PartialEq, Eq)]
pub struct ExprScopes {
    scopes: Arena<ExprScopeData>,
    scopes_by_expr: FxHashMap<ExprId, ExprScopeId>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ExprScopeData {
    parent: Option<ExprScopeId>,
    entries: Vec<ExprScopeEntry>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ExprScopeEntry {
    name: Name,
    pat: PatId,
}

#[derive(Debug, PartialEq, Eq)]
pub struct TypeScopes {
    scopes: Arena<TypeScopeData>,
    scopes_by_type: FxHashMap<TypeRefId, TypeScopeId>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct TypeScopeData {
    parent: Option<TypeScopeId>,
    entries: Vec<TypeScopeEntry>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct TypeScopeEntry {
    name: Name,
    type_var: TypeVarId,
}

impl ExprScopeEntry {
    pub fn name(&self) -> &Name {
        &self.name
    }

    pub fn pat(&self) -> PatId {
        self.pat
    }
}

impl TypeScopeEntry {
    pub fn name(&self) -> &Name {
        &self.name
    }

    pub fn type_var(&self) -> TypeVarId {
        self.type_var
    }
}

impl ExprScopes {
    pub fn expr_scopes_query(db: &dyn DefDatabase, def: DefWithBodyId) -> Arc<ExprScopes> {
        let body = db.body(def);

        Arc::new(ExprScopes::new(&body))
    }

    fn new(body: &Body) -> ExprScopes {
        let mut scopes = ExprScopes {
            scopes: Arena::default(),
            scopes_by_expr: FxHashMap::default(),
        };

        let root = scopes.root_scope();

        scopes.add_params_bindings(body, root, &body.params);
        compute_expr_scopes(body.body_expr, body, &mut scopes, root);
        scopes
    }

    pub fn entries(&self, id: ExprScopeId) -> &[ExprScopeEntry] {
        &self.scopes[id].entries
    }

    pub fn scope_chain(&self, scope: Option<ExprScopeId>) -> impl Iterator<Item = ExprScopeId> + '_ {
        std::iter::successors(scope, move |&scope| self.scopes[scope].parent)
    }

    pub fn scope_for(&self, id: ExprId) -> Option<ExprScopeId> {
        self.scopes_by_expr.get(&id).copied()
    }

    fn root_scope(&mut self) -> ExprScopeId {
        self.scopes.alloc(ExprScopeData {
            parent: None,
            entries: Vec::new(),
        })
    }

    fn new_scope(&mut self, parent: ExprScopeId) -> ExprScopeId {
        self.scopes.alloc(ExprScopeData {
            parent: Some(parent),
            entries: Vec::new(),
        })
    }

    fn add_bindings(&mut self, body: &Body, scope: ExprScopeId, pat: PatId) {
        let pattern = &body[pat];

        if let Pat::Bind { name, .. } = pattern {
            let entry = ExprScopeEntry {
                name: name.clone(),
                pat,
            };

            self.scopes[scope].entries.push(entry);
        }

        pattern.walk(|pat| self.add_bindings(body, scope, pat));
    }

    fn add_params_bindings(&mut self, body: &Body, scope: ExprScopeId, params: &[PatId]) {
        params.iter().for_each(|pat| self.add_bindings(body, scope, *pat));
    }

    fn set_scope(&mut self, expr: ExprId, scope: ExprScopeId) {
        self.scopes_by_expr.insert(expr, scope);
    }
}

impl TypeScopes {
    pub fn type_scopes_query(db: &dyn DefDatabase, id: TypeRefId) -> Arc<TypeScopes> {
        Arc::new(TypeScopes::new(db, id))
    }

    fn new(db: &dyn DefDatabase, id: TypeRefId) -> Self {
        let mut scopes = TypeScopes {
            scopes: Arena::default(),
            scopes_by_type: FxHashMap::default(),
        };

        let root = scopes.root_scope();

        compute_type_scopes(id, db, &mut scopes, root);
        scopes
    }

    pub fn from_type_vars(db: &dyn DefDatabase, vars: impl Iterator<Item = TypeVarId>) -> (Self, TypeScopeId) {
        let mut scopes = TypeScopes {
            scopes: Arena::default(),
            scopes_by_type: FxHashMap::default(),
        };

        let root = scopes.root_scope();

        for var in vars {
            let data = var.lookup(db);

            if let Some(kind) = data.kind {
                compute_type_scopes(kind, db, &mut scopes, root);
            }

            scopes.add_binding(&data.name, var, root);
        }

        (scopes, root)
    }

    pub fn entries(&self, id: TypeScopeId) -> &[TypeScopeEntry] {
        &self.scopes[id].entries
    }

    pub fn scope_chain(&self, scope: Option<TypeScopeId>) -> impl Iterator<Item = TypeScopeId> + '_ {
        std::iter::successors(scope, move |&scope| self.scopes[scope].parent)
    }

    pub fn scope_for(&self, id: TypeRefId) -> Option<TypeScopeId> {
        self.scopes_by_type.get(&id).copied()
    }

    fn root_scope(&mut self) -> TypeScopeId {
        self.scopes.alloc(TypeScopeData {
            parent: None,
            entries: Vec::new(),
        })
    }

    fn new_scope(&mut self, parent: TypeScopeId) -> TypeScopeId {
        self.scopes.alloc(TypeScopeData {
            parent: Some(parent),
            entries: Vec::new(),
        })
    }

    fn add_binding(&mut self, name: &Name, type_var: TypeVarId, scope: TypeScopeId) {
        let entry = TypeScopeEntry {
            name: name.clone(),
            type_var,
        };

        self.scopes[scope].entries.push(entry);
    }

    fn set_scope(&mut self, ty: TypeRefId, scope: TypeScopeId) {
        self.scopes_by_type.insert(ty, scope);
    }
}

fn compute_expr_scopes(expr: ExprId, body: &Body, scopes: &mut ExprScopes, scope: ExprScopeId) {
    scopes.set_scope(expr, scope);

    match &body[expr] {
        | Expr::Do { stmts } => {
            compute_block_scopes(stmts, body, scopes, scope);
        },
        | Expr::Case { pred, arms } => {
            compute_expr_scopes(*pred, body, scopes, scope);

            for arm in arms {
                let scope = scopes.new_scope(scope);

                scopes.add_bindings(body, scope, arm.pat);

                if let Some(guard) = arm.guard {
                    compute_expr_scopes(guard, body, scopes, scope);
                }

                compute_expr_scopes(arm.expr, body, scopes, scope);
            }
        },
        | e => e.walk(|e| compute_expr_scopes(e, body, scopes, scope)),
    }
}

fn compute_block_scopes(stmts: &[Stmt], body: &Body, scopes: &mut ExprScopes, mut scope: ExprScopeId) {
    for stmt in stmts {
        match stmt {
            | Stmt::Let { pat, val } | Stmt::Bind { pat, val } => {
                compute_expr_scopes(*val, body, scopes, scope);
                scope = scopes.new_scope(scope);
                scopes.add_bindings(body, scope, *pat);
            },
            | Stmt::Expr { expr } => {
                compute_expr_scopes(*expr, body, scopes, scope);
            },
        }
    }
}

fn compute_type_scopes(ty: TypeRefId, db: &dyn DefDatabase, scopes: &mut TypeScopes, mut scope: TypeScopeId) {
    scopes.set_scope(ty, scope);

    match ty.lookup(db) {
        | TypeRef::Forall(vars, inner) => {
            for &var in vars.iter() {
                let data = var.lookup(db);

                if let Some(kind) = data.kind {
                    compute_type_scopes(kind, db, scopes, scope);
                }

                scopes.add_binding(&data.name, var, scope);
            }

            scope = scopes.new_scope(scope);
            compute_type_scopes(inner, db, scopes, scope);
        },
        | TypeRef::Kinded(ty, kind) => {
            compute_type_scopes(ty, db, scopes, scope);
            compute_type_scopes(kind, db, scopes, scope);
        },
        | TypeRef::App(base, arg) => {
            compute_type_scopes(base, db, scopes, scope);
            compute_type_scopes(arg, db, scopes, scope);
        },
        | TypeRef::Tuple(tys) => {
            for &ty in tys.iter() {
                compute_type_scopes(ty, db, scopes, scope);
            }
        },
        | TypeRef::Ptr(ty, _) | TypeRef::Slice(ty) | TypeRef::Array(ty, _) => {
            compute_type_scopes(ty, db, scopes, scope)
        },
        | TypeRef::Func(arg, ret) => {
            compute_type_scopes(arg, db, scopes, scope);
            compute_type_scopes(ret, db, scopes, scope);
        },
        | TypeRef::Constraint(ctnt, ty) => {
            for &ty in ctnt.types.iter() {
                compute_type_scopes(ty, db, scopes, scope);
            }

            compute_type_scopes(ty, db, scopes, scope);
        },
        | _ => {},
    }
}
